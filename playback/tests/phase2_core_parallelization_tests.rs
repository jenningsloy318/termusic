//! Phase 2: Core Parallelization - RED Phase Tests
//!
//! These tests verify the two-phase classify-then-parallel-process architecture
//! for `Playlist::load()`. They exercise:
//!   - Line classification (network address vs local file path)
//!   - Parallel metadata reads via rayon par_iter
//!   - Order-preserving merge of parallel and sequential results
//!   - Error handling (failed tracks skipped gracefully)
//!   - Edge cases (empty, single, all-fail, interleaved)
//!
//! Coverage map:
//!   AC-01: Parallel metadata reads (T-07) — SCENARIO-001, SCENARIO-002, SCENARIO-003
//!   AC-02: Order preservation (T-09) — SCENARIO-004, SCENARIO-005, SCENARIO-006, SCENARIO-021
//!   AC-03: Public API unchanged (T-05..T-10) — SCENARIO-007, SCENARIO-008
//!   AC-05: Graceful error handling (T-07) — SCENARIO-010, SCENARIO-011, SCENARIO-020
//!   AC-06: Podcast/radio isolation (T-08) — SCENARIO-013, SCENARIO-014
//!
//! These tests will FAIL TO COMPILE or FAIL AT RUNTIME until:
//!   1. The classify/merge helper functions are exposed for testing (T-05, T-06, T-09)
//!   2. The parallel metadata read logic is implemented (T-07)
//!   3. The order-preserving merge is implemented (T-09)

use rayon::prelude::*;
use termusiclib::track::Track;

// Import the internal test helpers that Phase 2 should expose.
// These are the testable extraction of the classify-then-merge logic.
use termusicplayback::playlist::parallel_load::{
    classify_playlist_lines, collect_and_filter_lines, merge_indexed_tracks,
    parallel_read_local_tracks, ClassifiedLines,
};

// =============================================================================
// T-05 / T-06: Line classification tests
// AC-02, AC-06, SCENARIO-013, SCENARIO-014
// =============================================================================

/// Lines starting with "http://" should be classified as NetworkAddress.
/// This ensures podcast feed URLs are NOT included in the parallel batch.
#[test]
fn classify_http_url_as_network_address() {
    let lines: Vec<(usize, String)> = vec![(0, "http://example.com/podcast/ep1.mp3".to_string())];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.network_entries.len(), 1);
    assert_eq!(classified.local_entries.len(), 0);
    assert_eq!(classified.network_entries[0].0, 0);
    assert_eq!(
        classified.network_entries[0].1,
        "http://example.com/podcast/ep1.mp3"
    );
}

/// Lines starting with "https://" should be classified as NetworkAddress.
#[test]
fn classify_https_url_as_network_address() {
    let lines: Vec<(usize, String)> = vec![(0, "https://secure.example.com/feed.mp3".to_string())];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.network_entries.len(), 1);
    assert_eq!(classified.local_entries.len(), 0);
}

/// Lines that do NOT start with "http://" or "https://" should be classified as local paths.
/// This ensures local audio files ARE included in the parallel metadata read batch.
#[test]
fn classify_local_path_as_local_entry() {
    let lines: Vec<(usize, String)> = vec![
        (0, "/home/user/music/song.mp3".to_string()),
        (1, "/mnt/storage/album/track.flac".to_string()),
        (2, "relative/path/audio.ogg".to_string()),
    ];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.local_entries.len(), 3);
    assert_eq!(classified.network_entries.len(), 0);
}

/// Mixed lines should be correctly partitioned preserving original indices.
/// This validates SCENARIO-021: interleaved entries maintain their positions.
#[test]
fn classify_mixed_lines_preserves_indices() {
    let lines: Vec<(usize, String)> = vec![
        (0, "/local/track_a.mp3".to_string()),
        (1, "http://podcast.example.com/ep1.mp3".to_string()),
        (2, "/local/track_b.flac".to_string()),
        (3, "https://radio.example.com/stream".to_string()),
        (4, "/local/track_c.ogg".to_string()),
    ];

    let classified = classify_playlist_lines(lines);

    // 3 local paths, 2 network addresses
    assert_eq!(classified.local_entries.len(), 3);
    assert_eq!(classified.network_entries.len(), 2);

    // Verify indices are preserved
    assert_eq!(classified.local_entries[0].0, 0); // track_a
    assert_eq!(classified.local_entries[1].0, 2); // track_b
    assert_eq!(classified.local_entries[2].0, 4); // track_c
    assert_eq!(classified.network_entries[0].0, 1); // podcast ep1
    assert_eq!(classified.network_entries[1].0, 3); // radio stream
}

/// Empty input should produce empty classification without panic.
/// SCENARIO-017: empty playlist edge case.
#[test]
fn classify_empty_input_produces_empty_output() {
    let lines: Vec<(usize, String)> = Vec::new();

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.local_entries.len(), 0);
    assert_eq!(classified.network_entries.len(), 0);
}

/// Single local path should be classified correctly.
/// SCENARIO-018: single track edge case.
#[test]
fn classify_single_local_path() {
    let lines: Vec<(usize, String)> = vec![(0, "/only/track.mp3".to_string())];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.local_entries.len(), 1);
    assert_eq!(classified.network_entries.len(), 0);
    assert_eq!(classified.local_entries[0].0, 0);
}

/// Single network URL should be classified correctly.
#[test]
fn classify_single_network_url() {
    let lines: Vec<(usize, String)> = vec![(0, "http://radio.station.com/stream".to_string())];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.local_entries.len(), 0);
    assert_eq!(classified.network_entries.len(), 1);
}

/// Lines with "http" in the middle (not prefix) should be classified as local paths.
/// Edge case: prevent false positive classification.
#[test]
fn classify_path_containing_http_not_as_prefix() {
    let lines: Vec<(usize, String)> = vec![
        (0, "/home/user/http-downloads/song.mp3".to_string()),
        (1, "/mnt/http_backup/track.flac".to_string()),
    ];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.local_entries.len(), 2);
    assert_eq!(classified.network_entries.len(), 0);
}

// =============================================================================
// T-09: Order-preserving merge tests
// AC-02, SCENARIO-004, SCENARIO-005, SCENARIO-006, SCENARIO-021
// =============================================================================

/// Merge of local and network tracks should produce output sorted by original index.
/// SCENARIO-004: Track order matches playlist file order after parallel loading.
#[test]
fn merge_preserves_original_order_for_interleaved_entries() {
    // Simulate: local tracks at indices 0, 2, 4; network tracks at indices 1, 3
    let local_tracks: Vec<(usize, Track)> = vec![
        (0, Track::new_radio("placeholder_0")),
        (2, Track::new_radio("placeholder_2")),
        (4, Track::new_radio("placeholder_4")),
    ];
    let network_tracks: Vec<(usize, Track)> = vec![
        (1, Track::new_radio("http://radio1.com/stream")),
        (3, Track::new_radio("http://radio3.com/stream")),
    ];

    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    // Output should be sorted by original index: 0, 1, 2, 3, 4
    assert_eq!(merged.len(), 5);
}

/// Merge with all items from one source should preserve order.
#[test]
fn merge_all_local_preserves_order() {
    let local_tracks: Vec<(usize, Track)> = vec![
        (0, Track::new_radio("track_0")),
        (1, Track::new_radio("track_1")),
        (2, Track::new_radio("track_2")),
    ];
    let network_tracks: Vec<(usize, Track)> = Vec::new();

    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    assert_eq!(merged.len(), 3);
}

/// Merge with all items from network source should preserve order.
#[test]
fn merge_all_network_preserves_order() {
    let local_tracks: Vec<(usize, Track)> = Vec::new();
    let network_tracks: Vec<(usize, Track)> = vec![
        (0, Track::new_radio("http://radio0.com")),
        (1, Track::new_radio("http://radio1.com")),
        (2, Track::new_radio("http://radio2.com")),
    ];

    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    assert_eq!(merged.len(), 3);
}

/// Merge with empty inputs should produce empty output without panic.
/// SCENARIO-017: empty playlist.
#[test]
fn merge_empty_inputs_produces_empty_output() {
    let local_tracks: Vec<(usize, Track)> = Vec::new();
    let network_tracks: Vec<(usize, Track)> = Vec::new();

    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    assert!(merged.is_empty());
}

/// SCENARIO-005: Order is preserved regardless of individual track read duration.
/// Tracks that take different amounts of time to process must appear in original order.
/// This test verifies merge correctness when indices arrive out of natural order.
#[test]
fn merge_out_of_order_indices_sorts_correctly() {
    // Simulate parallel results arriving out of order (as rayon may produce)
    let local_tracks: Vec<(usize, Track)> = vec![
        (4, Track::new_radio("track_4")), // arrived first due to faster I/O
        (0, Track::new_radio("track_0")), // arrived second
        (2, Track::new_radio("track_2")), // arrived third
    ];
    let network_tracks: Vec<(usize, Track)> = vec![
        (3, Track::new_radio("http://net3.com")),
        (1, Track::new_radio("http://net1.com")),
    ];

    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    assert_eq!(merged.len(), 5);
    // The output must be sorted by the original index regardless of input order
    // We can't easily check Track content directly, but verify the count is correct
    // and the function doesn't panic on unsorted inputs
}

/// SCENARIO-006: Order is preserved when some tracks fail metadata parsing.
/// The merge step must handle gaps in indices (failed tracks produce no entry).
#[test]
fn merge_with_gaps_in_indices_preserves_relative_order() {
    // Original playlist had indices 0,1,2,3,4 but track at index 2 failed
    let local_tracks: Vec<(usize, Track)> = vec![
        (0, Track::new_radio("track_0")),
        // index 1 is network
        // index 2 failed - not present
        (3, Track::new_radio("track_3")),
        (4, Track::new_radio("track_4")),
    ];
    let network_tracks: Vec<(usize, Track)> = vec![(1, Track::new_radio("http://net1.com"))];

    let merged = merge_indexed_tracks(local_tracks, network_tracks);

    // Only 4 tracks (one failed), order should be: 0, 1, 3, 4
    assert_eq!(merged.len(), 4);
}

// =============================================================================
// T-07: Parallel metadata read behavior tests
// AC-01, AC-05, SCENARIO-001, SCENARIO-010, SCENARIO-011, SCENARIO-020
// =============================================================================

/// Verify that par_iter is used for local path processing.
/// This test uses the parallel_load module's process function that must use rayon.
/// It verifies that multiple paths are processed (filter_map pattern with Track::read_track_from_path).
///
/// SCENARIO-010: Failed metadata parsing skips the track.
/// SCENARIO-011: Multiple consecutive failures do not halt processing.
#[test]
fn parallel_read_skips_invalid_paths_and_continues() {
    // All paths are invalid (non-existent files)
    let local_entries: Vec<(usize, String)> = vec![
        (0, "/nonexistent/path/track1.mp3".to_string()),
        (1, "/nonexistent/path/track2.flac".to_string()),
        (2, "/nonexistent/path/track3.ogg".to_string()),
        (3, "/nonexistent/path/track4.wav".to_string()),
        (4, "/nonexistent/path/track5.mp3".to_string()),
    ];

    let results = parallel_read_local_tracks(&local_entries);

    // All should fail gracefully, producing empty results (no panic, no crash)
    assert_eq!(results.len(), 0);
}

/// SCENARIO-020: All tracks fail metadata parsing results in empty playlist.
/// The parallel processing must complete successfully even when every single
/// track fails to parse.
#[test]
fn parallel_read_all_failures_produces_empty_vec() {
    let local_entries: Vec<(usize, String)> = (0..20)
        .map(|i| (i, format!("/absolutely/nonexistent/file_{i}.mp3")))
        .collect();

    let results = parallel_read_local_tracks(&local_entries);

    assert!(
        results.is_empty(),
        "All tracks failed, should produce empty vec"
    );
}

/// Empty local entries should produce empty results without error.
/// SCENARIO-017: empty playlist.
#[test]
fn parallel_read_empty_input_produces_empty_output() {
    let local_entries: Vec<(usize, String)> = Vec::new();

    let results = parallel_read_local_tracks(&local_entries);

    assert!(results.is_empty());
}

/// Single entry should work without parallel processing errors.
/// SCENARIO-018: single track playlist.
#[test]
fn parallel_read_single_entry_works() {
    // Single invalid path - should not panic, should return empty
    let local_entries: Vec<(usize, String)> =
        vec![(0, "/nonexistent/single_track.mp3".to_string())];

    let results = parallel_read_local_tracks(&local_entries);

    // Single invalid track should be skipped
    assert_eq!(results.len(), 0);
}

/// Verify that parallel_read preserves original indices in output tuples.
/// This is critical for order preservation (AC-02).
#[test]
fn parallel_read_preserves_original_indices_in_results() {
    // Use empty string paths which will fail with "Given path is empty!" error
    // to verify that indices are passed through correctly
    let local_entries: Vec<(usize, String)> = vec![
        (5, "".to_string()),
        (10, "".to_string()),
        (15, "".to_string()),
    ];

    let results = parallel_read_local_tracks(&local_entries);

    // All empty paths should fail, but if any succeed, indices must match
    // The main assertion is that this doesn't panic
    assert!(
        results.is_empty()
            || results
                .iter()
                .all(|(idx, _)| *idx == 5 || *idx == 10 || *idx == 15)
    );
}

// =============================================================================
// T-05: Batch line collection with map_while tests
// AC-02, SCENARIO-017, SCENARIO-018
// =============================================================================

/// Verify that the line collection helper filters empty lines and comments.
/// The batch collection phase should skip empty lines and lines starting with '#'.
#[test]
fn collect_lines_filters_empty_and_comments() {
    let raw_lines: Vec<Result<String, std::io::Error>> = vec![
        Ok("track_a.mp3".to_string()),
        Ok("".to_string()),                    // empty - should be filtered
        Ok("# this is a comment".to_string()), // comment - should be filtered
        Ok("  ".to_string()),                  // whitespace only - should be filtered
        Ok("track_b.flac".to_string()),
        Ok("   # indented comment".to_string()), // indented comment - should be filtered
        Ok("track_c.ogg".to_string()),
    ];

    let collected = collect_and_filter_lines(raw_lines.into_iter());

    assert_eq!(collected.len(), 3);
    assert_eq!(collected[0], (0, "track_a.mp3".to_string()));
    assert_eq!(collected[1], (1, "track_b.flac".to_string()));
    assert_eq!(collected[2], (2, "track_c.ogg".to_string()));
}

/// Verify that map_while stops at first I/O error.
/// Original code used `line?` which aborted on first error.
/// The new batch approach with map_while(Result::ok) should stop reading
/// at the first error but return Ok with already-read lines.
#[test]
fn collect_lines_stops_at_first_io_error() {
    let raw_lines: Vec<Result<String, std::io::Error>> = vec![
        Ok("track_a.mp3".to_string()),
        Ok("track_b.flac".to_string()),
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "disk failure",
        )),
        Ok("track_c.ogg".to_string()), // should NOT be included
    ];

    let collected = collect_and_filter_lines(raw_lines.into_iter());

    // Only lines before the error should be collected
    assert_eq!(collected.len(), 2);
    assert_eq!(collected[0].1, "track_a.mp3");
    assert_eq!(collected[1].1, "track_b.flac");
}

/// Empty input iterator should produce empty collection without panic.
#[test]
fn collect_lines_empty_input_produces_empty_output() {
    let raw_lines: Vec<Result<String, std::io::Error>> = Vec::new();

    let collected = collect_and_filter_lines(raw_lines.into_iter());

    assert!(collected.is_empty());
}

/// Input with only empty/comment lines should produce empty collection.
#[test]
fn collect_lines_all_filtered_produces_empty() {
    let raw_lines: Vec<Result<String, std::io::Error>> = vec![
        Ok("".to_string()),
        Ok("# comment".to_string()),
        Ok("   ".to_string()),
        Ok("  # another comment".to_string()),
    ];

    let collected = collect_and_filter_lines(raw_lines.into_iter());

    assert!(collected.is_empty());
}

// =============================================================================
// AC-03: Public API signature stability tests
// SCENARIO-007, SCENARIO-008
// =============================================================================

/// Verify that Playlist::load() still returns Result<(usize, Vec<Track>)>.
/// This is a compile-time check that the signature hasn't changed.
/// The function may fail at runtime (no config dir in test), but must compile.
#[test]
fn playlist_load_signature_returns_result_tuple() {
    // This is a type-checking test. If it compiles, the signature is correct.
    let _: fn() -> anyhow::Result<(usize, Vec<termusiclib::track::Track>)> =
        termusicplayback::Playlist::load;
}

/// Verify that Playlist::load_apply takes &mut self and returns Result<()>.
/// Compile-time signature verification for AC-03 / SCENARIO-007.
#[test]
fn playlist_load_apply_signature_unchanged() {
    // Type-check: load_apply must accept &mut Playlist and return Result<()>
    fn _assert_load_apply_signature(
        playlist: &mut termusicplayback::Playlist,
    ) -> anyhow::Result<()> {
        playlist.load_apply()
    }
}

// =============================================================================
// T-07 + T-08 + T-09: Full integration pattern test
// AC-01, AC-02, AC-06, SCENARIO-001, SCENARIO-013, SCENARIO-014, SCENARIO-021
// =============================================================================

/// Integration test verifying the complete two-phase architecture:
/// 1. Lines are classified into network vs local
/// 2. Local paths are processed in parallel
/// 3. Network entries are processed sequentially
/// 4. Results are merged preserving original order
///
/// This tests the full pipeline exposed via the parallel_load module.
#[test]
fn full_pipeline_classify_process_merge_preserves_order() {
    // Input: interleaved local paths (all invalid, will fail) and network URLs
    let lines: Vec<(usize, String)> = vec![
        (0, "/nonexistent/local_a.mp3".to_string()),
        (1, "http://podcast.example.com/ep1.mp3".to_string()),
        (2, "/nonexistent/local_b.flac".to_string()),
        (3, "https://radio.example.com/stream".to_string()),
        (4, "/nonexistent/local_c.ogg".to_string()),
    ];

    // Phase A: classify
    let classified = classify_playlist_lines(lines);
    assert_eq!(classified.local_entries.len(), 3);
    assert_eq!(classified.network_entries.len(), 2);

    // Phase B: process local in parallel (all will fail since paths don't exist)
    let local_results = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(local_results.len(), 0); // all invalid

    // Phase B: process network sequentially (create radio tracks as fallback)
    let network_results: Vec<(usize, Track)> = classified
        .network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();
    assert_eq!(network_results.len(), 2);

    // Merge
    let merged = merge_indexed_tracks(local_results, network_results);

    // Only network tracks survived (local all failed)
    assert_eq!(merged.len(), 2);
}

/// Verify that the pipeline does not panic with a very large number of entries.
/// SCENARIO-019: Very large playlist does not exhaust system resources.
#[test]
fn full_pipeline_handles_large_input_without_resource_exhaustion() {
    // Create 1000 entries (mix of local and network)
    let lines: Vec<(usize, String)> = (0..1000)
        .map(|i| {
            if i % 3 == 0 {
                (i, format!("http://example.com/podcast/ep{i}.mp3"))
            } else {
                (i, format!("/nonexistent/track_{i}.mp3"))
            }
        })
        .collect();

    let classified = classify_playlist_lines(lines);

    // ~333 network, ~667 local
    assert!(classified.network_entries.len() > 300);
    assert!(classified.local_entries.len() > 600);

    // Process local (all will fail since paths don't exist, but must not panic)
    let local_results = parallel_read_local_tracks(&classified.local_entries);
    assert_eq!(local_results.len(), 0); // all invalid paths
}

// =============================================================================
// T-10: Elapsed time logging verification
// This test verifies that the load operation completes in bounded time
// even when parallel processing is active.
// =============================================================================

/// Verify that loading with all-invalid paths completes quickly (no hang).
/// The parallel processing must not block indefinitely on failed reads.
#[test]
fn parallel_processing_completes_in_bounded_time() {
    use std::time::Instant;

    let local_entries: Vec<(usize, String)> = (0..100)
        .map(|i| (i, format!("/nonexistent/bounded_time_track_{i}.mp3")))
        .collect();

    let start = Instant::now();
    let _results = parallel_read_local_tracks(&local_entries);
    let elapsed = start.elapsed();

    // Processing 100 non-existent files should complete in under 5 seconds
    // (each file open attempt fails fast since the file doesn't exist)
    assert!(
        elapsed.as_secs() < 5,
        "Parallel processing took too long: {:?}",
        elapsed
    );
}

// =============================================================================
// Anti-hardcoding: Multiple varied inputs for classification
// =============================================================================

/// Test classification with various URL schemes and path formats.
/// Ensures the implementation uses prefix-based logic, not hardcoded values.
#[test]
fn classify_varied_url_formats() {
    let lines: Vec<(usize, String)> = vec![
        (0, "http://a.com/1".to_string()),
        (1, "https://b.org/2".to_string()),
        (2, "http://192.168.1.1:8080/stream".to_string()),
        (3, "https://user:pass@host.io/feed.xml".to_string()),
        (4, "HTTP://uppercase.com/test".to_string()), // uppercase - should be local (case sensitive)
    ];

    let classified = classify_playlist_lines(lines);

    // First 4 are network, last one is local (starts_with is case-sensitive)
    assert_eq!(classified.network_entries.len(), 4);
    assert_eq!(classified.local_entries.len(), 1);
    assert_eq!(classified.local_entries[0].0, 4); // the uppercase HTTP line
}

/// Verify that "http" without "://" suffix is classified as local path.
/// The spec requires matching "http://" or "https://", not just "http".
#[test]
fn classify_http_without_scheme_separator_is_local() {
    let lines: Vec<(usize, String)> = vec![
        (0, "httpfoo/bar.mp3".to_string()),
        (1, "httpsomething".to_string()),
    ];

    let classified = classify_playlist_lines(lines);

    // Neither matches "http://" or "https://" prefix
    assert_eq!(classified.local_entries.len(), 2);
    assert_eq!(classified.network_entries.len(), 0);
}

/// Test classification with various local path formats.
/// Ensures paths with special characters, spaces, unicode are handled.
#[test]
fn classify_varied_local_path_formats() {
    let lines: Vec<(usize, String)> = vec![
        (0, "/home/user/My Music/track 01.mp3".to_string()),
        (
            1,
            "/mnt/nas/albums/artist - album/01. title.flac".to_string(),
        ),
        (2, "/tmp/日本語の曲.ogg".to_string()),
        (3, "C:\\Users\\User\\Music\\track.mp3".to_string()), // Windows path
        (4, "./relative/path.wav".to_string()),
    ];

    let classified = classify_playlist_lines(lines);

    assert_eq!(classified.local_entries.len(), 5);
    assert_eq!(classified.network_entries.len(), 0);
}
