//! Two-phase parallel playlist loading helpers.
//!
//! This module extracts the classify-then-parallel-process logic from `Playlist::load()`
//! into testable functions. The architecture is:
//!
//! 1. **Collect**: Read lines from the playlist file, filtering empty/comment lines
//! 2. **Classify**: Partition lines into network addresses (http/https) and local file paths
//! 3. **Process**: Read local file metadata in parallel via rayon `par_iter`
//! 4. **Merge**: Combine results in original playlist order

use rayon::prelude::*;
use termusiclib::track::Track;

/// Result of classifying playlist lines into network addresses and local file paths.
#[derive(Debug)]
pub struct ClassifiedLines {
    /// Lines identified as network addresses (starting with "http://" or "https://").
    /// Each tuple is (`original_index`, `url_string`).
    pub network_entries: Vec<(usize, String)>,
    /// Lines identified as local file paths (anything not starting with "http://" or "https://").
    /// Each tuple is (`original_index`, `file_path_string`).
    pub local_entries: Vec<(usize, String)>,
}

/// Collect lines from an iterator, stopping at the first I/O error, and filtering
/// out empty lines and comment lines (starting with '#').
///
/// Uses `map_while(Result::ok)` to stop reading at the first error, matching
/// the original abort-on-first-I/O-error semantics of `line?` while enabling
/// batch collection.
///
/// The returned indices are sequential (0-based) among the *valid* lines only,
/// meaning they represent the position of each track in the final playlist.
pub fn collect_and_filter_lines(
    lines: impl Iterator<Item = Result<String, std::io::Error>>,
) -> Vec<(usize, String)> {
    lines
        .map_while(std::result::Result::ok)
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .enumerate()
        .collect()
}

/// Classify playlist lines into network addresses and local file paths.
///
/// Network addresses are lines starting with "http://" or "https://" (case-sensitive).
/// All other lines are treated as local file paths requiring metadata I/O.
///
/// Original indices are preserved for order-preserving merge after parallel processing.
#[must_use]
pub fn classify_playlist_lines(lines: Vec<(usize, String)>) -> ClassifiedLines {
    let (network_entries, local_entries): (Vec<_>, Vec<_>) = lines
        .into_iter()
        .partition(|(_, line)| line.starts_with("http://") || line.starts_with("https://"));

    ClassifiedLines {
        network_entries,
        local_entries,
    }
}

/// Read metadata for local file paths in parallel using rayon `par_iter`.
///
/// Each path is processed independently via `Track::read_track_from_path`.
/// Paths that do not exist on disk are skipped early to avoid creating tracks
/// with empty metadata. Failed reads (parse errors) are also silently excluded --
/// the debug-level logging inside `read_track_from_path` remains the sole
/// log point for failed reads.
///
/// # Panic Safety
///
/// Lofty 0.24.0 uses `ParsingMode::BestAttempt` by default and has extensive
/// fuzz testing (8+ fuzz targets). Panics are extremely unlikely. If a panic
/// does occur, rayon propagates it to the calling thread after other tasks
/// complete. No `catch_unwind` is added per-task as the cost outweighs the
/// near-zero probability of occurrence.
#[must_use]
pub fn parallel_read_local_tracks(local_entries: &[(usize, String)]) -> Vec<(usize, Track)> {
    local_entries
        .par_iter()
        .filter_map(|(original_index, file_path)| {
            // Skip non-existent files early to avoid creating tracks with empty metadata
            if !std::path::Path::new(file_path).exists() {
                return None;
            }
            Track::read_track_from_path(file_path)
                .ok()
                .map(|track| (*original_index, track))
        })
        .collect()
}

/// Merge local and network track results into a single Vec preserving original order.
///
/// Both input vectors contain `(original_index, Track)` tuples. The merge combines
/// them and sorts by `original_index` to restore the playlist file's line order,
/// regardless of the order in which parallel processing completed.
///
/// Gaps in indices (from failed tracks) are handled naturally -- only successfully
/// resolved tracks appear in the output.
#[must_use]
pub fn merge_indexed_tracks(
    local_tracks: Vec<(usize, Track)>,
    network_tracks: Vec<(usize, Track)>,
) -> Vec<Track> {
    let mut indexed_tracks: Vec<(usize, Track)> =
        Vec::with_capacity(local_tracks.len() + network_tracks.len());
    indexed_tracks.extend(local_tracks);
    indexed_tracks.extend(network_tracks);
    indexed_tracks.sort_unstable_by_key(|(index, _)| *index);

    indexed_tracks.into_iter().map(|(_, track)| track).collect()
}
