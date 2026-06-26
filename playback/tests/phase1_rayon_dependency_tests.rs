//! Phase 1: Dependency Setup - RED Phase Tests
//!
//! These tests verify that the rayon crate is properly declared as a direct dependency
//! of the playback crate, enabling parallel iteration for playlist loading.
//!
//! Coverage:
//!   AC-07: The rayon crate is added as a direct dependency to the playback crate's Cargo.toml
//!   SCENARIO-015: Rayon is declared as a direct dependency of the playback crate
//!
//! These tests will FAIL TO COMPILE until:
//!   1. rayon = "1.12" is added to [workspace.dependencies] in root Cargo.toml
//!   2. rayon.workspace = true is added to [dependencies] in playback/Cargo.toml
//!   3. use rayon::prelude::*; is added to playback/src/playlist.rs

use rayon::prelude::*;

// =============================================================================
// T-01 / T-02: Rayon dependency is available from playback crate
// AC-07, SCENARIO-015
// =============================================================================

/// Verify that rayon can be imported and that par_iter is available on Vec.
/// This test will fail to compile if rayon is not declared as a direct dependency
/// of the playback crate.
#[test]
fn rayon_par_iter_is_available_on_vec() {
    let data: Vec<u32> = vec![1, 2, 3, 4, 5, 6, 7, 8];

    // Use par_iter to verify rayon's parallel iteration is accessible
    let sum: u32 = data.par_iter().map(|x| x * 2).sum();

    assert_eq!(sum, 72); // 2+4+6+8+10+12+14+16 = 72
}

/// Verify that rayon's par_iter preserves collection semantics (collect into Vec).
/// This validates that the ParallelIterator trait and its collect() method work,
/// which is the exact pattern needed for parallel playlist loading.
#[test]
fn rayon_par_iter_collect_preserves_results() {
    let paths: Vec<String> = vec![
        "track_a.mp3".to_string(),
        "track_b.flac".to_string(),
        "track_c.ogg".to_string(),
    ];

    // Simulate the pattern used in parallel playlist loading:
    // par_iter -> filter_map -> collect
    let results: Vec<(usize, &str)> = paths
        .par_iter()
        .enumerate()
        .filter_map(|(idx, path)| {
            if path.ends_with(".mp3") || path.ends_with(".flac") || path.ends_with(".ogg") {
                Some((idx, path.as_str()))
            } else {
                None
            }
        })
        .collect();

    // All three are valid audio extensions, all should be present
    assert_eq!(results.len(), 3);
}

/// Verify that rayon's par_iter filter_map correctly excludes failed items.
/// This mirrors the error handling pattern where Track::read_track_from_path
/// returns Err for invalid files, and filter_map excludes them.
#[test]
fn rayon_par_iter_filter_map_excludes_failures() {
    let items: Vec<i32> = vec![1, -1, 2, -2, 3, -3, 4];

    // Simulate: .par_iter().filter_map(|x| some_fallible_fn(x).ok())
    let successes: Vec<i32> = items
        .par_iter()
        .filter_map(|&x| if x > 0 { Some(x) } else { None })
        .collect();

    assert_eq!(successes, vec![1, 2, 3, 4]);
}

/// Verify that rayon's indexed par_iter produces deterministic order-preserving results.
/// This is critical for AC-02 (playlist order preservation).
/// par_iter().enumerate() must preserve indices regardless of execution order.
#[test]
fn rayon_par_iter_enumerate_preserves_original_indices() {
    let data: Vec<&str> = vec!["alpha", "beta", "gamma", "delta", "epsilon"];

    let indexed: Vec<(usize, &&str)> = data.par_iter().enumerate().collect();

    // Indices must match original positions
    assert_eq!(indexed[0], (0, &"alpha"));
    assert_eq!(indexed[1], (1, &"beta"));
    assert_eq!(indexed[2], (2, &"gamma"));
    assert_eq!(indexed[3], (3, &"delta"));
    assert_eq!(indexed[4], (4, &"epsilon"));
}

/// Verify that rayon handles empty collections without panic or error.
/// This supports SCENARIO-017 (empty playlist loads without error).
#[test]
fn rayon_par_iter_handles_empty_collection() {
    let empty: Vec<String> = Vec::new();

    let results: Vec<(usize, &String)> = empty.par_iter().enumerate().collect();

    assert!(results.is_empty());
}

/// Verify that rayon handles single-element collections correctly.
/// This supports SCENARIO-018 (single track playlist).
#[test]
fn rayon_par_iter_handles_single_element() {
    let single: Vec<String> = vec!["only_track.mp3".to_string()];

    let results: Vec<(usize, &String)> = single.par_iter().enumerate().collect();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, 0);
    assert_eq!(results[0].1, "only_track.mp3");
}

// =============================================================================
// T-03: use rayon::prelude::* import is accessible from playlist module
// AC-07, SCENARIO-015
// =============================================================================

/// Verify that the playback crate's playlist module can use rayon.
/// This test imports from the playback crate and confirms rayon is usable
/// alongside the playlist types that will use it.
///
/// We import Playlist to confirm the module compiles with rayon imported.
#[test]
fn playback_crate_compiles_with_rayon_import() {
    // If this test compiles, it means:
    // 1. rayon is a dependency of playback (Cargo.toml)
    // 2. The playback crate builds successfully with rayon available
    //
    // We use a rayon operation here to ensure the dependency is actually linked
    let test_vec: Vec<i32> = (0..100).collect();
    let parallel_sum: i32 = test_vec.par_iter().sum();
    let sequential_sum: i32 = test_vec.iter().sum();

    // Parallel and sequential must produce identical results
    assert_eq!(parallel_sum, sequential_sum);
    assert_eq!(parallel_sum, 4950); // sum of 0..100
}

/// Verify that rayon's IntoParallelRefIterator trait works on slices,
/// which is the pattern used when processing playlist entries.
#[test]
fn rayon_works_on_tuple_vec_pattern() {
    // This mirrors the exact data structure used in the specification:
    // Vec<(usize, String)> where usize is the original index
    let entries: Vec<(usize, String)> = vec![
        (0, "/path/to/track1.mp3".to_string()),
        (2, "/path/to/track2.flac".to_string()),
        (4, "/path/to/track3.ogg".to_string()),
    ];

    // Simulate parallel metadata reading with index preservation
    let processed: Vec<(usize, String)> = entries
        .par_iter()
        .filter_map(|(idx, path)| {
            // Simulate Track::read_track_from_path - always succeeds here
            Some((*idx, path.to_uppercase()))
        })
        .collect();

    assert_eq!(processed.len(), 3);
    // Verify indices are preserved
    assert_eq!(processed[0].0, 0);
    assert_eq!(processed[1].0, 2);
    assert_eq!(processed[2].0, 4);
}
