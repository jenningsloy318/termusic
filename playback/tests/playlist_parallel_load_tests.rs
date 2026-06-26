//! Phase 3: Integration Testing - RED Phase Tests
//!
//! These tests exercise the FULL parallel playlist loading pipeline with real file I/O.
//! Unlike Phase 2 unit tests (which test individual functions), these integration tests:
//!   - Read playlist fixture files from disk
//!   - Create temporary audio files that Track::read_track_from_path can load
//!   - Verify order preservation through the complete pipeline with real files
//!   - Test graceful error handling with actual missing/invalid files on disk
//!   - Verify the integration between all parallel_load module functions
//!
//! Coverage map (Phase 3 tasks T-12 through T-16):
//!   T-12: Fixture files — playlist_mixed.log, playlist_invalid_paths.log, playlist_empty.log,
//!         playlist_single.log, playlist_all_invalid.log
//!   T-13: SCENARIO-004, SCENARIO-005, SCENARIO-021 (order preservation with mixed entries)
//!   T-14: SCENARIO-010, SCENARIO-011 (invalid paths skipped gracefully)
//!   T-15: SCENARIO-017, SCENARIO-018, SCENARIO-020 (edge cases)
//!   T-16: Full test suite verification
//!
//! BDD Scenario coverage:
//!   SCENARIO-004: Track order matches playlist file order after parallel loading
//!   SCENARIO-005: Order is preserved regardless of individual track read duration
//!   SCENARIO-006: Order is preserved when some tracks fail metadata parsing
//!   SCENARIO-010: Failed metadata parsing skips the track with a debug log
//!   SCENARIO-011: Multiple consecutive failures do not halt parallel processing
//!   SCENARIO-013: Podcast feed address lookups remain unaffected by parallelization
//!   SCENARIO-014: Radio track creation remains unaffected by parallelization
//!   SCENARIO-017: Empty playlist file loads without error
//!   SCENARIO-018: Playlist with a single track loads correctly
//!   SCENARIO-019: Very large playlist does not exhaust system resources
//!   SCENARIO-020: All tracks fail metadata parsing results in empty playlist
//!   SCENARIO-021: Playlist file with mixed addresses and local paths preserves global order
//!
//! These tests will FAIL until the full parallel loading pipeline is working correctly
//! with real filesystem interactions. They test behavior that goes BEYOND what the Phase 2
//! unit tests cover by exercising the real I/O path.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;

use termusiclib::track::{MediaTypesSimple, Track};
use termusicplayback::playlist::parallel_load::{
    classify_playlist_lines, collect_and_filter_lines, load_playlist_from_path,
    merge_indexed_tracks, parallel_read_local_tracks,
};

/// Helper: Get the path to a test fixture file.
fn fixture_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Helper: Read and parse a playlist fixture file (skip first line which is the track index).
fn read_playlist_fixture(name: &str) -> Vec<(usize, String)> {
    let path = fixture_path(name);
    let file = fs::File::open(&path)
        .unwrap_or_else(|e| panic!("Failed to open fixture {}: {}", path.display(), e));
    let reader = BufReader::new(file);
    let mut lines_iter = reader.lines();

    // Skip the first line (track index line)
    let _track_index_line = lines_iter.next();

    collect_and_filter_lines(lines_iter)
}

/// Helper: Create a temporary directory with valid audio files that Track::read_track_from_path
/// can successfully load. We create minimal valid files (any file that exists on disk will
/// produce a Track with default metadata since read_track_from_path only fails on empty paths).
fn create_temp_audio_files(dir: &Path, count: usize) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(count);
    for i in 0..count {
        let file_path = dir.join(format!("track_{:04}.mp3", i));
        // Write minimal content — Track::read_track_from_path will succeed since the file
        // exists (it uses default metadata if parsing fails)
        fs::write(&file_path, b"fake audio content for testing").unwrap();
        paths.push(file_path);
    }
    paths
}

// =============================================================================
// T-13: Order preservation with interleaved local and network entries
// SCENARIO-004, SCENARIO-005, SCENARIO-021
// =============================================================================

/// Integration test: Load the mixed playlist fixture, verify classification
/// correctly separates local paths from network URLs while preserving indices.
///
/// SCENARIO-021: Playlist file with mixed addresses and local paths preserves global order.
#[test]
fn test_parallel_load_preserves_order_with_mixed_entries_from_fixture() {
    let all_lines = read_playlist_fixture("playlist_mixed.log");

    // The fixture has 7 entries: 4 local, 3 network (2 http + 1 https)
    assert_eq!(
        all_lines.len(),
        7,
        "Fixture should have 7 non-empty track entries after skipping index line"
    );

    let classified = classify_playlist_lines(all_lines);

    // Verify correct classification counts
    assert_eq!(
        classified.local_entries.len(),
        4,
        "Should have 4 local file paths"
    );
    assert_eq!(
        classified.network_entries.len(),
        3,
        "Should have 3 network URLs"
    );

    // Verify indices are preserved correctly for interleaved entries
    // Fixture order: local(0), http(1), local(2), https(3), local(4), http(5), local(6)
    assert_eq!(classified.local_entries[0].0, 0);
    assert_eq!(classified.local_entries[1].0, 2);
    assert_eq!(classified.local_entries[2].0, 4);
    assert_eq!(classified.local_entries[3].0, 6);

    assert_eq!(classified.network_entries[0].0, 1);
    assert_eq!(classified.network_entries[1].0, 3);
    assert_eq!(classified.network_entries[2].0, 5);
}

/// Integration test: Create real temporary audio files on disk, run the full pipeline,
/// and verify that the output tracks are in the exact order of the input playlist entries.
///
/// SCENARIO-004: Track order matches playlist file order after parallel loading.
/// SCENARIO-005: Order is preserved regardless of individual track read duration.
#[test]
fn test_parallel_load_preserves_order_with_real_files_on_disk() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create 10 audio files with sequential names
    let audio_files = create_temp_audio_files(dir, 10);

    // Build the classified entries simulating a playlist with:
    // - Local files at even indices (0, 2, 4, 6, 8)
    // - Network URLs at odd indices (1, 3, 5, 7, 9)
    let local_entries: Vec<(usize, String)> = audio_files
        .iter()
        .enumerate()
        .map(|(i, path)| (i * 2, path.to_string_lossy().to_string()))
        .collect();

    let network_entries: Vec<(usize, String)> = (0..5)
        .map(|i| (i * 2 + 1, format!("http://example.com/podcast/ep{}.mp3", i)))
        .collect();

    // Process local files in parallel (real file I/O)
    let local_tracks = parallel_read_local_tracks(&local_entries);

    // All files exist, so all should succeed
    assert_eq!(
        local_tracks.len(),
        10,
        "All 10 local files exist and should produce tracks"
    );

    // Process network entries (create radio tracks)
    let network_tracks: Vec<(usize, Track)> = network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();

    assert_eq!(network_tracks.len(), 5);

    // Merge and verify order
    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    assert_eq!(
        merged.len(),
        15,
        "Total should be 10 local + 5 network = 15"
    );

    // The merge sorts by original index. Original indices are:
    // Local files: 0, 2, 4, 6, 8, 10, 12, 14, 16, 18
    // Network URLs: 1, 3, 5, 7, 9
    // Sorted: [0:M, 1:R, 2:M, 3:R, 4:M, 5:R, 6:M, 7:R, 8:M, 9:R, 10:M, 12:M, 14:M, 16:M, 18:M]
    // First 10 positions alternate Music/Radio, last 5 are all Music
    for i in 0..10 {
        if i % 2 == 0 {
            assert_eq!(
                merged[i].media_type(),
                MediaTypesSimple::Music,
                "Track at merged position {} should be Music (local file)",
                i
            );
        } else {
            assert_eq!(
                merged[i].media_type(),
                MediaTypesSimple::LiveRadio,
                "Track at merged position {} should be LiveRadio (network URL)",
                i
            );
        }
    }
    // Remaining 5 positions (10-14) are all Music (local files at original indices 10,12,14,16,18)
    for i in 10..15 {
        assert_eq!(
            merged[i].media_type(),
            MediaTypesSimple::Music,
            "Track at merged position {} should be Music (remaining local file)",
            i
        );
    }
}

/// Integration test: Verify that parallel processing of files with varying sizes
/// still preserves the original playlist order.
///
/// SCENARIO-005: Order is preserved regardless of individual track read duration.
#[test]
fn test_parallel_load_order_independent_of_file_size() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create files of varying sizes to simulate different read durations
    let file_specs: Vec<(&str, usize)> = vec![
        ("small_track.mp3", 10),       // tiny file
        ("medium_track.flac", 10_000), // medium file
        ("large_track.ogg", 100_000),  // larger file
        ("tiny_track.wav", 5),         // very small
        ("big_track.mp3", 500_000),    // biggest
    ];

    let mut local_entries: Vec<(usize, String)> = Vec::new();
    for (i, (name, size)) in file_specs.iter().enumerate() {
        let path = dir.join(name);
        fs::write(&path, vec![0u8; *size]).unwrap();
        local_entries.push((i, path.to_string_lossy().to_string()));
    }

    // Process in parallel
    let results = parallel_read_local_tracks(&local_entries);

    // All files exist, so all should succeed
    assert_eq!(
        results.len(),
        5,
        "All 5 files exist and should produce tracks"
    );

    // Verify indices are monotonically increasing (order preserved)
    let indices: Vec<usize> = results.iter().map(|(idx, _)| *idx).collect();
    let mut sorted_indices = indices.clone();
    sorted_indices.sort();
    assert_eq!(
        indices, sorted_indices,
        "Result indices should be in ascending order"
    );

    // Verify the file paths match the expected order
    for (idx, track) in &results {
        let expected_name = file_specs[*idx].0;
        let track_path = track.path().expect("Local track should have a path");
        assert!(
            track_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&expected_name[..5]),
            "Track at index {} should correspond to file {}",
            idx,
            expected_name
        );
    }
}

/// Integration test: Verify order preservation with the full pipeline when mixing
/// real existing files with network URLs.
///
/// SCENARIO-021: Mixed addresses and local paths preserves global order.
#[test]
fn test_parallel_load_mixed_real_files_and_urls_preserves_global_order() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create 3 real audio files
    let file_a = dir.join("track_a.mp3");
    let file_b = dir.join("track_b.mp3");
    let file_c = dir.join("track_c.mp3");
    fs::write(&file_a, b"audio content a").unwrap();
    fs::write(&file_b, b"audio content b").unwrap();
    fs::write(&file_c, b"audio content c").unwrap();

    // Simulate interleaved playlist: [local-A, podcast-1, local-B, radio, local-C]
    let all_lines: Vec<(usize, String)> = vec![
        (0, file_a.to_string_lossy().to_string()),
        (1, "http://podcast.example.com/ep1.mp3".to_string()),
        (2, file_b.to_string_lossy().to_string()),
        (3, "https://radio.example.com/stream".to_string()),
        (4, file_c.to_string_lossy().to_string()),
    ];

    // Classify
    let classified = classify_playlist_lines(all_lines);
    assert_eq!(classified.local_entries.len(), 3);
    assert_eq!(classified.network_entries.len(), 2);

    // Process local paths in parallel (real I/O)
    let local_tracks = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(local_tracks.len(), 3, "All 3 local files should load");

    // Process network entries
    let network_tracks: Vec<(usize, Track)> = classified
        .network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();

    // Merge
    let merged = merge_indexed_tracks(local_tracks, network_tracks);
    assert_eq!(merged.len(), 5);

    // Verify order: Music, Radio, Music, Radio, Music
    assert_eq!(merged[0].media_type(), MediaTypesSimple::Music);
    assert_eq!(merged[1].media_type(), MediaTypesSimple::LiveRadio);
    assert_eq!(merged[2].media_type(), MediaTypesSimple::Music);
    assert_eq!(merged[3].media_type(), MediaTypesSimple::LiveRadio);
    assert_eq!(merged[4].media_type(), MediaTypesSimple::Music);

    // Verify specific paths for local tracks
    assert_eq!(merged[0].path().unwrap(), file_a.as_path());
    assert_eq!(merged[2].path().unwrap(), file_b.as_path());
    assert_eq!(merged[4].path().unwrap(), file_c.as_path());

    // Verify specific URLs for network tracks
    assert_eq!(
        merged[1].url().unwrap(),
        "http://podcast.example.com/ep1.mp3"
    );
    assert_eq!(merged[3].url().unwrap(), "https://radio.example.com/stream");
}

// =============================================================================
// T-14: Graceful skip of invalid file paths during parallel load
// SCENARIO-010, SCENARIO-011
// =============================================================================

/// Integration test: Load the invalid-paths fixture and verify that non-existent
/// file paths are skipped gracefully while valid paths succeed.
///
/// SCENARIO-010: Failed metadata parsing skips the track with a debug log.
#[test]
fn test_parallel_load_skips_invalid_paths_gracefully_from_fixture() {
    let all_lines = read_playlist_fixture("playlist_invalid_paths.log");

    // All entries in this fixture are local paths (no network URLs)
    let classified = classify_playlist_lines(all_lines);
    assert_eq!(classified.network_entries.len(), 0);
    assert!(classified.local_entries.len() > 0);

    // All paths are non-existent, so parallel processing should skip them all
    let results = parallel_read_local_tracks(&classified.local_entries);

    assert_eq!(
        results.len(),
        0,
        "All paths are non-existent, should produce 0 tracks"
    );
}

/// Integration test: Mix of valid and invalid paths — valid files should load,
/// invalid files should be skipped, and order is preserved for surviving tracks.
///
/// SCENARIO-006: Order is preserved when some tracks fail metadata parsing.
/// SCENARIO-010: Failed metadata parsing skips the track.
/// SCENARIO-011: Multiple consecutive failures do not halt parallel processing.
#[test]
fn test_parallel_load_mixed_valid_invalid_preserves_order_of_valid() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create only some files — gaps simulate failed tracks
    let file_0 = dir.join("track_0.mp3");
    let file_3 = dir.join("track_3.mp3");
    let file_4 = dir.join("track_4.mp3");
    fs::write(&file_0, b"valid content").unwrap();
    fs::write(&file_3, b"valid content").unwrap();
    fs::write(&file_4, b"valid content").unwrap();

    // Entries: 0=exists, 1=missing, 2=missing, 3=exists, 4=exists
    let local_entries: Vec<(usize, String)> = vec![
        (0, file_0.to_string_lossy().to_string()),
        (
            1,
            dir.join("nonexistent_1.mp3").to_string_lossy().to_string(),
        ),
        (
            2,
            dir.join("nonexistent_2.mp3").to_string_lossy().to_string(),
        ),
        (3, file_3.to_string_lossy().to_string()),
        (4, file_4.to_string_lossy().to_string()),
    ];

    let results = parallel_read_local_tracks(&local_entries);

    // Only 3 files exist
    assert_eq!(
        results.len(),
        3,
        "Only 3 existing files should produce tracks"
    );

    // Verify the indices match the files that exist (0, 3, 4)
    let result_indices: Vec<usize> = results.iter().map(|(idx, _)| *idx).collect();
    assert!(result_indices.contains(&0));
    assert!(result_indices.contains(&3));
    assert!(result_indices.contains(&4));
    assert!(!result_indices.contains(&1));
    assert!(!result_indices.contains(&2));

    // Verify order is preserved after merge
    let merged = merge_indexed_tracks(results, Vec::new());
    assert_eq!(merged.len(), 3);

    // Paths should be in original order: track_0, track_3, track_4
    assert_eq!(merged[0].path().unwrap(), file_0.as_path());
    assert_eq!(merged[1].path().unwrap(), file_3.as_path());
    assert_eq!(merged[2].path().unwrap(), file_4.as_path());
}

/// Integration test: Multiple consecutive failures followed by valid files.
///
/// SCENARIO-011: Multiple consecutive failures do not halt parallel processing.
#[test]
fn test_parallel_load_consecutive_failures_do_not_halt_processing() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Only the last file exists — 10 consecutive failures before it
    let valid_file = dir.join("valid_last.mp3");
    fs::write(&valid_file, b"valid content").unwrap();

    let mut local_entries: Vec<(usize, String)> = (0..10)
        .map(|i| {
            (
                i,
                dir.join(format!("missing_{}.mp3", i))
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();
    local_entries.push((10, valid_file.to_string_lossy().to_string()));

    let results = parallel_read_local_tracks(&local_entries);

    // Only the last file should succeed
    assert_eq!(
        results.len(),
        1,
        "Only one valid file should produce a track"
    );
    assert_eq!(results[0].0, 10);
    assert_eq!(results[0].1.path().unwrap(), valid_file.as_path());
}

/// Integration test: Verify that many scattered failures among valid files
/// does not corrupt the results of valid entries.
///
/// SCENARIO-011: Multiple failures do not halt processing.
#[test]
fn test_parallel_load_scattered_failures_among_valid_files() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create files at indices 0, 5, 10, 15, 20 — all others are missing
    let valid_indices: Vec<usize> = vec![0, 5, 10, 15, 20];
    let mut valid_files: Vec<PathBuf> = Vec::new();
    for &idx in &valid_indices {
        let path = dir.join(format!("track_{:02}.mp3", idx));
        fs::write(&path, b"audio data").unwrap();
        valid_files.push(path);
    }

    // Build entries with 25 total (only 5 valid)
    let local_entries: Vec<(usize, String)> = (0..25)
        .map(|i| {
            if valid_indices.contains(&i) {
                let path = dir.join(format!("track_{:02}.mp3", i));
                (i, path.to_string_lossy().to_string())
            } else {
                (
                    i,
                    dir.join(format!("missing_{:02}.mp3", i))
                        .to_string_lossy()
                        .to_string(),
                )
            }
        })
        .collect();

    let results = parallel_read_local_tracks(&local_entries);

    assert_eq!(results.len(), 5, "Only 5 valid files should produce tracks");

    // Verify correct indices
    let result_indices: Vec<usize> = results.iter().map(|(idx, _)| *idx).collect();
    for &expected_idx in &valid_indices {
        assert!(
            result_indices.contains(&expected_idx),
            "Index {} should be in results",
            expected_idx
        );
    }

    // Verify order after merge
    let merged = merge_indexed_tracks(results, Vec::new());
    assert_eq!(merged.len(), 5);
    for (i, &expected_idx) in valid_indices.iter().enumerate() {
        let expected_path = dir.join(format!("track_{:02}.mp3", expected_idx));
        assert_eq!(
            merged[i].path().unwrap(),
            expected_path.as_path(),
            "Track at merged position {} should be track_{:02}.mp3",
            i,
            expected_idx
        );
    }
}

// =============================================================================
// T-15: Edge cases — empty, single track, all-fail
// SCENARIO-017, SCENARIO-018, SCENARIO-020
// =============================================================================

/// Integration test: Empty playlist file loads without error.
///
/// SCENARIO-017: Empty playlist file loads without error.
#[test]
fn test_parallel_load_empty_playlist_from_fixture() {
    let all_lines = read_playlist_fixture("playlist_empty.log");

    // The fixture has only the track index line — no track entries
    assert_eq!(all_lines.len(), 0, "Empty fixture should produce 0 lines");

    // Pipeline should handle empty input gracefully
    let classified = classify_playlist_lines(all_lines);
    assert_eq!(classified.local_entries.len(), 0);
    assert_eq!(classified.network_entries.len(), 0);

    let local_tracks = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(local_tracks.len(), 0);

    let merged = merge_indexed_tracks(local_tracks, Vec::new());
    assert!(
        merged.is_empty(),
        "Empty playlist should produce empty result"
    );
}

/// Integration test: Single track playlist loads correctly.
///
/// SCENARIO-018: Playlist with a single track loads correctly.
#[test]
fn test_parallel_load_single_track_from_fixture() {
    let all_lines = read_playlist_fixture("playlist_single.log");

    assert_eq!(
        all_lines.len(),
        1,
        "Single-track fixture should produce 1 line"
    );

    let classified = classify_playlist_lines(all_lines);
    assert_eq!(classified.local_entries.len(), 1);
    assert_eq!(classified.network_entries.len(), 0);

    // The path is non-existent so parallel_read will skip it
    // But the classification should still work correctly
    assert_eq!(classified.local_entries[0].0, 0);
    assert_eq!(classified.local_entries[0].1, "/local/single_track.mp3");
}

/// Integration test: Single track with a real file on disk loads correctly.
///
/// SCENARIO-018: Playlist with a single track loads correctly.
#[test]
fn test_parallel_load_single_real_track() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let single_file = dir.join("only_track.mp3");
    fs::write(&single_file, b"single track content").unwrap();

    let local_entries: Vec<(usize, String)> = vec![(0, single_file.to_string_lossy().to_string())];

    let results = parallel_read_local_tracks(&local_entries);

    assert_eq!(
        results.len(),
        1,
        "Single existing file should produce 1 track"
    );
    assert_eq!(results[0].0, 0);
    assert_eq!(results[0].1.path().unwrap(), single_file.as_path());
    assert_eq!(results[0].1.media_type(), MediaTypesSimple::Music);
}

/// Integration test: All tracks fail metadata parsing results in empty playlist.
///
/// SCENARIO-020: All tracks fail metadata parsing results in empty playlist.
#[test]
fn test_parallel_load_all_tracks_fail_from_fixture() {
    let all_lines = read_playlist_fixture("playlist_all_invalid.log");

    assert!(
        all_lines.len() > 0,
        "All-invalid fixture should have entries"
    );

    let classified = classify_playlist_lines(all_lines);

    // All entries are local paths (no network URLs in this fixture)
    assert_eq!(classified.network_entries.len(), 0);
    assert!(classified.local_entries.len() >= 5);

    // All paths are non-existent
    let local_tracks = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(
        local_tracks.len(),
        0,
        "All invalid paths should produce 0 tracks"
    );

    let merged = merge_indexed_tracks(local_tracks, Vec::new());
    assert!(
        merged.is_empty(),
        "All-fail scenario should produce empty playlist"
    );
}

/// Integration test: All tracks fail but the operation completes without panic or hang.
///
/// SCENARIO-020: Load operation completes without error (no crash, no hang).
#[test]
fn test_parallel_load_all_fail_completes_without_hang() {
    let start = Instant::now();

    // 100 non-existent paths
    let local_entries: Vec<(usize, String)> = (0..100)
        .map(|i| {
            (
                i,
                format!("/absolutely/nonexistent/all_fail_hang_test/track_{}.mp3", i),
            )
        })
        .collect();

    let results = parallel_read_local_tracks(&local_entries);
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 0);
    assert!(
        elapsed.as_secs() < 10,
        "All-fail processing should complete quickly, took {:?}",
        elapsed
    );
}

// =============================================================================
// SCENARIO-013, SCENARIO-014: Podcast and radio isolation
// Integration tests verifying network entries are NOT parallelized
// =============================================================================

/// Integration test: Podcast feed URLs are classified as network entries and
/// NOT included in the parallel metadata read batch.
///
/// SCENARIO-013: Podcast feed address lookups remain unaffected by parallelization.
#[test]
fn test_parallel_load_podcast_urls_not_in_parallel_batch() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create some real local files
    let local_file = dir.join("local_track.mp3");
    fs::write(&local_file, b"local content").unwrap();

    let all_lines: Vec<(usize, String)> = vec![
        (0, local_file.to_string_lossy().to_string()),
        (1, "http://feeds.example.com/podcast/ep1.mp3".to_string()),
        (2, "https://feeds.example.com/podcast/ep2.mp3".to_string()),
    ];

    let classified = classify_playlist_lines(all_lines);

    // Only the local file should be in the parallel batch
    assert_eq!(classified.local_entries.len(), 1);
    assert_eq!(classified.network_entries.len(), 2);

    // Parallel read only processes local entries
    let local_tracks = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(local_tracks.len(), 1, "Local file should be loaded");
    assert_eq!(local_tracks[0].1.media_type(), MediaTypesSimple::Music);

    // Network entries would be processed separately (not by parallel_read_local_tracks)
    // Verify they are never passed to parallel_read
    let network_as_local = parallel_read_local_tracks(&classified.network_entries);
    assert_eq!(
        network_as_local.len(),
        0,
        "Network URLs should not produce tracks via parallel_read (they don't exist as files)"
    );
}

/// Integration test: Radio stream URLs are classified as network entries
/// and resolved via Track::new_radio (not via parallel metadata read).
///
/// SCENARIO-014: Radio track creation remains unaffected by parallelization.
#[test]
fn test_parallel_load_radio_urls_resolved_as_radio_tracks() {
    let all_lines: Vec<(usize, String)> = vec![
        (0, "http://radio.station.com/stream".to_string()),
        (1, "https://radio.another.org/live".to_string()),
        (2, "http://192.168.1.100:8000/stream.ogg".to_string()),
    ];

    let classified = classify_playlist_lines(all_lines);

    // All should be network entries
    assert_eq!(classified.local_entries.len(), 0);
    assert_eq!(classified.network_entries.len(), 3);

    // Create radio tracks from network entries (simulating the sequential path)
    let radio_tracks: Vec<(usize, Track)> = classified
        .network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();

    assert_eq!(radio_tracks.len(), 3);
    for (_, track) in &radio_tracks {
        assert_eq!(track.media_type(), MediaTypesSimple::LiveRadio);
    }

    // Verify URLs are preserved correctly
    assert_eq!(
        radio_tracks[0].1.url().unwrap(),
        "http://radio.station.com/stream"
    );
    assert_eq!(
        radio_tracks[1].1.url().unwrap(),
        "https://radio.another.org/live"
    );
    assert_eq!(
        radio_tracks[2].1.url().unwrap(),
        "http://192.168.1.100:8000/stream.ogg"
    );
}

// =============================================================================
// SCENARIO-019: Very large playlist resource bounds
// =============================================================================

/// Integration test: Large number of entries processed without exhausting resources.
///
/// SCENARIO-019: Very large playlist does not exhaust system resources.
#[test]
fn test_parallel_load_large_playlist_bounded_resources() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create 200 real files
    let audio_files = create_temp_audio_files(dir, 200);

    let local_entries: Vec<(usize, String)> = audio_files
        .iter()
        .enumerate()
        .map(|(i, path)| (i, path.to_string_lossy().to_string()))
        .collect();

    let start = Instant::now();
    let results = parallel_read_local_tracks(&local_entries);
    let elapsed = start.elapsed();

    // All 200 files should load successfully
    assert_eq!(results.len(), 200, "All 200 existing files should load");

    // Should complete in reasonable time (under 30 seconds even on slow systems)
    assert!(
        elapsed.as_secs() < 30,
        "Loading 200 files took too long: {:?}",
        elapsed
    );

    // Verify indices are correct
    let indices: Vec<usize> = results.iter().map(|(idx, _)| *idx).collect();
    let mut sorted = indices.clone();
    sorted.sort();
    assert_eq!(indices, sorted, "Indices should be in ascending order");
    assert_eq!(*indices.first().unwrap(), 0);
    assert_eq!(*indices.last().unwrap(), 199);
}

/// Integration test: Mix of 500 local + 500 network entries.
///
/// SCENARIO-019: Very large playlist does not exhaust system resources.
#[test]
fn test_parallel_load_1000_entries_mixed_completes_successfully() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create 500 real local files
    let audio_files = create_temp_audio_files(dir, 500);

    // Build interleaved entries: local at even indices, network at odd
    let local_entries: Vec<(usize, String)> = audio_files
        .iter()
        .enumerate()
        .map(|(i, path)| (i * 2, path.to_string_lossy().to_string()))
        .collect();

    let network_entries: Vec<(usize, String)> = (0..500)
        .map(|i| (i * 2 + 1, format!("http://example.com/podcast/ep{}.mp3", i)))
        .collect();

    // Process local files in parallel
    let start = Instant::now();
    let local_tracks = parallel_read_local_tracks(&local_entries);
    let local_elapsed = start.elapsed();

    assert_eq!(local_tracks.len(), 500, "All 500 local files should load");
    assert!(
        local_elapsed.as_secs() < 60,
        "Loading 500 local files took too long: {:?}",
        local_elapsed
    );

    // Create radio tracks for network entries
    let radio_tracks: Vec<(usize, Track)> = network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();

    // Merge
    let merged = merge_indexed_tracks(local_tracks, radio_tracks);
    assert_eq!(merged.len(), 1000, "Total should be 1000 tracks");

    // Spot-check order: first track should be Music (index 0), second Radio (index 1)
    assert_eq!(merged[0].media_type(), MediaTypesSimple::Music);
    assert_eq!(merged[1].media_type(), MediaTypesSimple::LiveRadio);
    assert_eq!(merged[998].media_type(), MediaTypesSimple::Music);
    assert_eq!(merged[999].media_type(), MediaTypesSimple::LiveRadio);
}

// =============================================================================
// Full pipeline integration: fixture file reading through to final track list
// =============================================================================

/// Integration test: Read the mixed fixture file, run the complete pipeline,
/// and verify the end-to-end behavior matches expectations.
///
/// This test exercises T-13 through T-15 combined: reading from a real file,
/// classifying, processing, and merging.
#[test]
fn test_full_pipeline_from_fixture_file_to_final_track_list() {
    let path = fixture_path("playlist_mixed.log");
    let file = fs::File::open(&path).expect("Failed to open mixed fixture");
    let reader = BufReader::new(file);
    let mut lines_iter = reader.lines();

    // Read track index (first line)
    let first_line = lines_iter.next().expect("Should have first line").unwrap();
    let track_index: usize = first_line.trim().parse().unwrap_or(0);
    assert_eq!(track_index, 0, "Fixture track index should be 0");

    // Collect and filter
    let all_lines = collect_and_filter_lines(lines_iter);
    assert_eq!(all_lines.len(), 7);

    // Classify
    let classified = classify_playlist_lines(all_lines);
    assert_eq!(classified.local_entries.len(), 4);
    assert_eq!(classified.network_entries.len(), 3);

    // Process local (all will fail since paths in fixture don't exist on disk)
    let local_tracks = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(local_tracks.len(), 0);

    // Process network as radio (no podcast DB available)
    let network_tracks: Vec<(usize, Track)> = classified
        .network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();
    assert_eq!(network_tracks.len(), 3);

    // Merge
    let merged = merge_indexed_tracks(local_tracks, network_tracks);
    assert_eq!(
        merged.len(),
        3,
        "Only network tracks survive (local paths don't exist)"
    );

    // All surviving tracks should be radio
    for track in &merged {
        assert_eq!(track.media_type(), MediaTypesSimple::LiveRadio);
    }

    // Verify order matches fixture order (network entries were at indices 1, 3, 5)
    assert_eq!(
        merged[0].url().unwrap(),
        "http://podcast.example.com/ep1.mp3"
    );
    assert_eq!(merged[1].url().unwrap(), "https://radio.example.com/stream");
    assert_eq!(
        merged[2].url().unwrap(),
        "http://podcast.example.com/ep2.mp3"
    );
}

/// Integration test: Verify that the collect_and_filter_lines function correctly
/// handles real BufReader output from a file.
///
/// This validates the I/O integration layer (not just in-memory iterators).
#[test]
fn test_collect_and_filter_from_real_file_io() {
    let path = fixture_path("playlist_mixed.log");
    let file = fs::File::open(&path).expect("Failed to open fixture");
    let reader = BufReader::new(file);
    let mut lines_iter = reader.lines();

    // Skip first line (track index)
    let _ = lines_iter.next();

    let collected = collect_and_filter_lines(lines_iter);

    // Verify we got the expected number of lines from the file
    assert_eq!(collected.len(), 7, "Mixed fixture has 7 track entries");

    // Verify sequential indexing (0-based after filtering)
    for (i, (idx, _)) in collected.iter().enumerate() {
        assert_eq!(*idx, i, "Index should be sequential");
    }
}

// =============================================================================
// Anti-hardcoding: Varied inputs to prevent implementation shortcuts
// =============================================================================

/// Integration test with different file extensions to ensure no extension-based shortcuts.
#[test]
fn test_parallel_load_various_file_extensions() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let extensions = ["mp3", "flac", "ogg", "wav", "m4a", "aac", "wma", "opus"];
    let mut local_entries: Vec<(usize, String)> = Vec::new();

    for (i, ext) in extensions.iter().enumerate() {
        let path = dir.join(format!("track.{}", ext));
        fs::write(&path, format!("fake {} content", ext).as_bytes()).unwrap();
        local_entries.push((i, path.to_string_lossy().to_string()));
    }

    let results = parallel_read_local_tracks(&local_entries);

    // All files exist, so all should produce tracks (metadata will be default)
    assert_eq!(
        results.len(),
        extensions.len(),
        "All {} files with various extensions should load",
        extensions.len()
    );
}

/// Integration test with paths containing special characters.
#[test]
fn test_parallel_load_paths_with_special_characters() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let special_names = [
        "track with spaces.mp3",
        "track-with-dashes.mp3",
        "track_with_underscores.mp3",
        "track.multiple.dots.mp3",
        "UPPERCASE.MP3",
    ];

    let mut local_entries: Vec<(usize, String)> = Vec::new();
    for (i, name) in special_names.iter().enumerate() {
        let path = dir.join(name);
        fs::write(&path, b"audio content").unwrap();
        local_entries.push((i, path.to_string_lossy().to_string()));
    }

    let results = parallel_read_local_tracks(&local_entries);

    assert_eq!(
        results.len(),
        special_names.len(),
        "All files with special character names should load"
    );
}

/// Integration test: Verify that the pipeline produces different results for different inputs.
/// Prevents hardcoded return values.
#[test]
fn test_parallel_load_different_inputs_produce_different_results() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Setup 1: 3 files
    let setup1_dir = dir.join("setup1");
    fs::create_dir(&setup1_dir).unwrap();
    for i in 0..3 {
        fs::write(setup1_dir.join(format!("t{}.mp3", i)), b"content").unwrap();
    }

    // Setup 2: 7 files
    let setup2_dir = dir.join("setup2");
    fs::create_dir(&setup2_dir).unwrap();
    for i in 0..7 {
        fs::write(setup2_dir.join(format!("s{}.mp3", i)), b"content").unwrap();
    }

    let entries1: Vec<(usize, String)> = (0..3)
        .map(|i| {
            (
                i,
                setup1_dir
                    .join(format!("t{}.mp3", i))
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();

    let entries2: Vec<(usize, String)> = (0..7)
        .map(|i| {
            (
                i,
                setup2_dir
                    .join(format!("s{}.mp3", i))
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();

    let results1 = parallel_read_local_tracks(&entries1);
    let results2 = parallel_read_local_tracks(&entries2);

    // Different inputs should produce different sized results
    assert_eq!(results1.len(), 3);
    assert_eq!(results2.len(), 7);
    assert_ne!(results1.len(), results2.len());
}

// =============================================================================
// END-TO-END INTEGRATION TESTS requiring `load_playlist_from_path`
//
// These tests verify the complete Playlist::load pipeline using a new testable
// entry point `load_playlist_from_path` that accepts an explicit playlist file
// path instead of relying on the user's config directory.
//
// This function DOES NOT EXIST YET and must be created as part of Phase 3
// to enable proper end-to-end integration testing without side effects.
//
// These tests will FAIL TO COMPILE until `load_playlist_from_path` is added
// to the parallel_load module.
//
// Coverage:
//   SCENARIO-004: Track order matches after full load
//   SCENARIO-006: Order preserved with failed tracks in full pipeline
//   SCENARIO-017: Empty playlist via full load path
//   SCENARIO-018: Single track via full load path
//   SCENARIO-020: All-fail via full load path
//   SCENARIO-021: Mixed entries via full load path
// =============================================================================

/// End-to-end integration test: Load a playlist file with real audio files and
/// verify the complete result including track index and ordered tracks.
///
/// SCENARIO-004: Track order matches playlist file order after parallel loading.
/// SCENARIO-021: Mixed addresses and local paths preserves global order.
#[test]
fn test_e2e_load_playlist_from_path_with_real_files() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Create real audio files
    let track_a = dir.join("track_a.mp3");
    let track_b = dir.join("track_b.mp3");
    let track_c = dir.join("track_c.mp3");
    fs::write(&track_a, b"audio content a").unwrap();
    fs::write(&track_b, b"audio content b").unwrap();
    fs::write(&track_c, b"audio content c").unwrap();

    // Write a playlist file with mixed content
    let playlist_path = dir.join("playlist.log");
    let playlist_content = format!(
        "2\n{}\nhttp://podcast.example.com/ep1.mp3\n{}\nhttps://radio.example.com/stream\n{}\n",
        track_a.display(),
        track_b.display(),
        track_c.display()
    );
    fs::write(&playlist_path, &playlist_content).unwrap();

    // Load via the testable path-based function
    let (track_index, tracks) =
        load_playlist_from_path(&playlist_path).expect("load_playlist_from_path should succeed");

    // Verify track index was read correctly from first line
    assert_eq!(track_index, 2, "Track index from file should be 2");

    // Verify total tracks: 3 local + 2 network = 5
    assert_eq!(tracks.len(), 5, "Should have 5 tracks total");

    // Verify order: Music, Radio, Music, Radio, Music
    assert_eq!(tracks[0].media_type(), MediaTypesSimple::Music);
    assert_eq!(tracks[1].media_type(), MediaTypesSimple::LiveRadio);
    assert_eq!(tracks[2].media_type(), MediaTypesSimple::Music);
    assert_eq!(tracks[3].media_type(), MediaTypesSimple::LiveRadio);
    assert_eq!(tracks[4].media_type(), MediaTypesSimple::Music);

    // Verify specific paths
    assert_eq!(tracks[0].path().unwrap(), track_a.as_path());
    assert_eq!(tracks[2].path().unwrap(), track_b.as_path());
    assert_eq!(tracks[4].path().unwrap(), track_c.as_path());

    // Verify specific URLs
    assert_eq!(
        tracks[1].url().unwrap(),
        "http://podcast.example.com/ep1.mp3"
    );
    assert_eq!(tracks[3].url().unwrap(), "https://radio.example.com/stream");
}

/// End-to-end integration test: Load an empty playlist file.
///
/// SCENARIO-017: Empty playlist file loads without error.
#[test]
fn test_e2e_load_playlist_from_path_empty_file() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Write an empty playlist (only track index line)
    let playlist_path = dir.join("playlist.log");
    fs::write(&playlist_path, "0\n").unwrap();

    let (track_index, tracks) =
        load_playlist_from_path(&playlist_path).expect("Empty playlist load should succeed");

    assert_eq!(track_index, 0);
    assert!(tracks.is_empty(), "Empty playlist should produce no tracks");
}

/// End-to-end integration test: Load a playlist where all tracks fail.
///
/// SCENARIO-020: All tracks fail metadata parsing results in empty playlist.
#[test]
fn test_e2e_load_playlist_from_path_all_fail() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    // Write a playlist with all non-existent paths
    let playlist_path = dir.join("playlist.log");
    let content = "0\n/nonexistent/a.mp3\n/nonexistent/b.mp3\n/nonexistent/c.mp3\n";
    fs::write(&playlist_path, content).unwrap();

    let (track_index, tracks) = load_playlist_from_path(&playlist_path)
        .expect("All-fail load should still succeed (not error)");

    assert_eq!(track_index, 0);
    assert!(
        tracks.is_empty(),
        "All-fail should produce empty track list"
    );
}

/// End-to-end integration test: Load a single-track playlist.
///
/// SCENARIO-018: Playlist with a single track loads correctly.
#[test]
fn test_e2e_load_playlist_from_path_single_track() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let track_file = dir.join("single.mp3");
    fs::write(&track_file, b"audio content").unwrap();

    let playlist_path = dir.join("playlist.log");
    let content = format!("0\n{}\n", track_file.display());
    fs::write(&playlist_path, &content).unwrap();

    let (track_index, tracks) =
        load_playlist_from_path(&playlist_path).expect("Single track load should succeed");

    assert_eq!(track_index, 0);
    assert_eq!(tracks.len(), 1, "Single track should produce 1 track");
    assert_eq!(tracks[0].media_type(), MediaTypesSimple::Music);
    assert_eq!(tracks[0].path().unwrap(), track_file.as_path());
}

/// End-to-end integration test: Verify track_index is clamped to playlist size.
///
/// The spec says: current_track_index = current_track_index.min(playlist_items.len().saturating_sub(1))
#[test]
fn test_e2e_load_playlist_from_path_clamps_track_index() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let track_file = dir.join("only_track.mp3");
    fs::write(&track_file, b"audio").unwrap();

    // Playlist file says track index is 100, but there's only 1 track
    let playlist_path = dir.join("playlist.log");
    let content = format!("100\n{}\n", track_file.display());
    fs::write(&playlist_path, &content).unwrap();

    let (track_index, tracks) =
        load_playlist_from_path(&playlist_path).expect("Load should succeed");

    assert_eq!(tracks.len(), 1);
    // Index should be clamped to len-1 = 0
    assert_eq!(
        track_index, 0,
        "Track index should be clamped to playlist size - 1"
    );
}

/// End-to-end integration test: Verify order preservation with mixed valid/invalid.
///
/// SCENARIO-006: Order is preserved when some tracks fail metadata parsing.
#[test]
fn test_e2e_load_playlist_from_path_order_with_failures() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let track_a = dir.join("a.mp3");
    let track_c = dir.join("c.mp3");
    fs::write(&track_a, b"audio a").unwrap();
    // track_b does NOT exist (simulates failure)
    fs::write(&track_c, b"audio c").unwrap();

    // Playlist: a(exists), b(missing), radio, c(exists)
    let playlist_path = dir.join("playlist.log");
    let content = format!(
        "0\n{}\n{}\nhttp://radio.example.com/stream\n{}\n",
        track_a.display(),
        dir.join("nonexistent_b.mp3").display(),
        track_c.display()
    );
    fs::write(&playlist_path, &content).unwrap();

    let (track_index, tracks) =
        load_playlist_from_path(&playlist_path).expect("Load with partial failures should succeed");

    assert_eq!(track_index, 0);
    // track_a + radio + track_c = 3 (track_b skipped)
    assert_eq!(tracks.len(), 3, "Should have 3 surviving tracks");

    // Order: a(Music), radio(LiveRadio), c(Music)
    assert_eq!(tracks[0].media_type(), MediaTypesSimple::Music);
    assert_eq!(tracks[0].path().unwrap(), track_a.as_path());
    assert_eq!(tracks[1].media_type(), MediaTypesSimple::LiveRadio);
    assert_eq!(tracks[1].url().unwrap(), "http://radio.example.com/stream");
    assert_eq!(tracks[2].media_type(), MediaTypesSimple::Music);
    assert_eq!(tracks[2].path().unwrap(), track_c.as_path());
}

/// End-to-end integration test: Non-existent playlist file should return error.
#[test]
fn test_e2e_load_playlist_from_path_nonexistent_file_errors() {
    let result =
        load_playlist_from_path(Path::new("/tmp/absolutely_nonexistent_playlist_12345.log"));

    assert!(
        result.is_err(),
        "Loading from non-existent path should return Err"
    );
}
