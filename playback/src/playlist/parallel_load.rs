//! Two-phase parallel playlist loading helpers.
//!
//! This module extracts the classify-then-parallel-process logic from `Playlist::load()`
//! into testable functions. The architecture is:
//!
//! 1. **Collect**: Read lines from the playlist file, filtering empty/comment lines
//! 2. **Classify**: Partition lines into network addresses (http/https) and local file paths
//! 3. **Process**: Read local file metadata in parallel via rayon `par_iter`
//! 4. **Merge**: Combine results in original playlist order

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use rayon::ThreadPool;
use rayon::prelude::*;
use termusiclib::track::Track;

/// Dedicated thread pool for parallel playlist metadata reads.
///
/// Using a dedicated pool (rather than the global rayon pool) ensures playlist
/// loading is not affected by other concurrent rayon workloads. The pool uses
/// the system's available parallelism (core count) to maximize throughput for
/// the I/O-bound metadata read workload.
static PLAYLIST_POOL: LazyLock<ThreadPool> = LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .thread_name(|idx| format!("playlist-io-{idx}"))
        .build()
        .expect("failed to build playlist thread pool")
});

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

/// Read metadata for local file paths sequentially (one at a time).
///
/// This function exists as a **performance baseline** for benchmarking and testing.
/// It performs the same work as [`parallel_read_local_tracks`] but processes entries
/// sequentially, allowing measurement of the parallel speedup achieved by rayon.
///
/// Each path goes through full validation: canonicalization (symlink resolution),
/// filesystem metadata check (regular file verification), a full read to confirm
/// accessibility and populate the OS page cache, and finally metadata parsing.
/// This represents the conservative single-threaded approach used before
/// parallelization — it validates each file thoroughly before committing to the
/// metadata parse step.
///
/// Failed reads (parse errors) are silently excluded — the debug-level logging
/// inside `read_track_from_path` remains the sole log point for failed reads.
#[must_use]
pub fn sequential_read_local_tracks(local_entries: &[(usize, String)]) -> Vec<(usize, Track)> {
    local_entries
        .iter()
        .filter_map(|(original_index, file_path)| {
            let path = std::path::Path::new(file_path);
            // Full path validation pipeline:
            // 1. Canonicalize: resolve symlinks, confirm path exists on filesystem
            let canonical = path.canonicalize().ok()?;
            // 2. Metadata: verify it's a regular file (not dir/pipe/socket)
            let meta = std::fs::metadata(&canonical).ok()?;
            if !meta.is_file() {
                return None;
            }
            // 3. Read file content to verify accessibility and warm the page cache.
            //    This catches permission errors and ensures consistent timing for
            //    the subsequent metadata parse (which re-reads from cache).
            let _ = std::fs::read(&canonical).ok()?;
            // 4. Parse metadata using the validated canonical path
            Track::read_track_from_path(&canonical)
                .ok()
                .map(|track| (*original_index, track))
        })
        .collect()
}

/// Threshold below which parallelization overhead exceeds the benefit.
/// For small playlists, sequential processing avoids rayon's work-stealing
/// and thread synchronization costs (SCENARIO-003).
const PARALLEL_THRESHOLD: usize = 50;

/// Read metadata for local file paths in parallel using rayon `par_iter`.
///
/// Each path is processed independently via `Track::read_track_from_path`.
/// Paths that do not exist on disk are skipped early to avoid creating tracks
/// with empty metadata. Failed reads (parse errors) are also silently excluded --
/// the debug-level logging inside `read_track_from_path` remains the sole
/// log point for failed reads.
///
/// For small inputs (below [`PARALLEL_THRESHOLD`] entries), processing falls back
/// to sequential iteration to avoid rayon's work-stealing overhead (SCENARIO-003).
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
    // For small inputs, sequential processing avoids rayon overhead (SCENARIO-003)
    if local_entries.len() < PARALLEL_THRESHOLD {
        return sequential_read_local_tracks(local_entries);
    }

    PLAYLIST_POOL.install(|| {
        local_entries
            .par_iter()
            .filter_map(|(original_index, file_path)| {
                // Use read_track_from_path directly — it returns Ok even for non-existent
                // files (with default metadata), but we filter those by checking the path
                // exists. This single stat() call is the minimum needed.
                if !std::path::Path::new(file_path).exists() {
                    return None;
                }
                Track::read_track_from_path(file_path)
                    .ok()
                    .map(|track| (*original_index, track))
            })
            .collect()
    })
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

/// Load a playlist from an explicit file path, bypassing the config directory lookup.
///
/// This is a testable entry point for the full parallel loading pipeline. It performs
/// the same operations as `Playlist::load()` but accepts a path argument instead of
/// relying on `get_playlist_path()` and `get_app_config_path()`.
///
/// Since no podcast database is available in this context, all network addresses
/// (http/https URLs) are treated as radio streams via `Track::new_radio()`.
///
/// # Returns
///
/// `(current_track_index, tracks)` where `current_track_index` is read from the
/// first line of the file and clamped to `tracks.len().saturating_sub(1)`.
///
/// # Errors
///
/// - When the playlist file cannot be opened
/// - When the file is empty (no first line for track index)
pub fn load_playlist_from_path(path: &Path) -> Result<(usize, Vec<Track>)> {
    let file =
        File::open(path).with_context(|| format!("failed to open playlist: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // Read the first line as the current track index
    let mut current_track_index: usize = 0;
    if let Some(line) = lines.next() {
        let index_line = line.with_context(|| "failed to read track index line")?;
        if let Ok(index) = index_line.trim().parse() {
            current_track_index = index;
        }
    } else {
        // Empty file
        return Ok((0, Vec::new()));
    }

    // Collect and filter remaining lines
    let all_lines = collect_and_filter_lines(lines);

    // Classify into network addresses and local paths
    let classified = classify_playlist_lines(all_lines);

    // Process local file paths in parallel via rayon
    let local_tracks = parallel_read_local_tracks(&classified.local_entries);

    // Process network entries as radio tracks (no podcast DB available in this context)
    let network_tracks: Vec<(usize, Track)> = classified
        .network_entries
        .iter()
        .map(|(idx, url)| (*idx, Track::new_radio(url)))
        .collect();

    // Merge preserving original playlist order
    let playlist_items = merge_indexed_tracks(local_tracks, network_tracks);

    // Clamp track index to valid range
    let current_track_index = current_track_index.min(playlist_items.len().saturating_sub(1));

    Ok((current_track_index, playlist_items))
}
