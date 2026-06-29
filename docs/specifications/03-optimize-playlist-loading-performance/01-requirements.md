# Requirements: Optimize Playlist Loading Performance

- **Date**: 2026-06-26
- **Author**: super-dev:requirements-clarifier
- **Type**: enhancement
- **Priority**: high
- **Status**: draft

---

## Executive Summary

Playlist loading in termusic performs sequential, blocking file I/O for every track entry during startup. With large playlists (500+ podcast MP3s), `Track::read_track_from_path` calls `parse_metadata_from_file` (lofty crate) one file at a time, causing 10+ second startup delays. This requirement specifies parallelizing metadata reads to reduce playlist load time proportionally to available CPU cores while maintaining API stability and correctness.

## The Real Need (Root Cause Analysis)

### Surface Request

Reduce startup time when loading playlists with hundreds of local audio files by parallelizing the metadata I/O in `Playlist::load()`.

### 5 Whys Analysis

1. **Why**: Startup takes 10+ seconds with 500+ tracks in the playlist.
2. **Why**: Each track's metadata is read sequentially via `parse_metadata_from_file`, which opens a file, parses ID3/MP4 tags, and extracts duration/title/artist — a blocking disk I/O operation taking ~20ms per file.
3. **Why**: The `Playlist::load()` loop at line 226 iterates through file paths one by one, calling `Track::read_track_from_path` with no concurrency.
4. **Why**: The original implementation was written for small playlists where sequential loading was imperceptible; the podcast sync feature (PR #720) significantly increased typical playlist sizes.
5. **Why**: No performance budget was established for playlist load time during the podcast sync design phase.

### Job to Be Done

When I launch termusic with a large playlist of podcast episodes and music tracks,
I want the application to become interactive within 2-3 seconds,
So I can start browsing and playing content without waiting for metadata parsing to complete.

- **Functional**: Load playlist metadata from disk in parallel, reducing wall-clock time proportionally to available cores
- **Emotional**: Eliminate the frustration of staring at a frozen/slow startup
- **Social**: N/A (single-user terminal application)

## Stakeholders

- **End user (power user with large podcast libraries)**: Experiences slow startup; primary beneficiary of this optimization
- **Termusic maintainers**: Must maintain the fix without increasing complexity; the solution should fit existing architectural patterns

## Workflow Context

### Before (Current State)

1. User launches `termusic-server`
2. `actual_main()` in `server/src/server.rs:149` calls `Playlist::new_shared()`
3. `Playlist::load()` opens `$config/playlist.log`, reads lines sequentially
4. For each local file path line, calls `Track::read_track_from_path(&line)` which:
   - Opens the audio file via `lofty::Probe::open(path)`
   - Parses ID3/MP4 tags (blocking I/O)
   - Extracts title, artist, album, duration, file_type
5. With 500 tracks at ~20ms each = ~10 seconds blocked
6. Only after ALL tracks are loaded does the server become responsive

### After (Desired State)

1. User launches `termusic-server`
2. `Playlist::load()` collects all local file paths from the playlist file
3. File paths are processed in parallel batches using rayon's `par_iter`
4. Metadata for multiple files is read concurrently (bounded by CPU core count)
5. Results are collected preserving original playlist order
6. With 8 cores: ~10s / 8 = ~1.25s wall-clock time for metadata reads
7. Server becomes responsive in under 2-3 seconds

## Solution Options

### Option 1: Parallel metadata reads with rayon `par_iter`

Collect all local file path lines into a Vec, then use `rayon::prelude::par_iter` to call `Track::read_track_from_path` in parallel. Results are collected in order. The podcast URL lookups and radio track creation remain sequential (they are cheap HashMap lookups, not I/O).

Implementation sketch:
- Separate the loop into two phases: (1) classify lines into podcast-URLs vs local-paths, (2) batch-process local paths with `par_iter().map(Track::read_track_from_path).collect()`
- Merge results back in playlist order

- **Pros**: Minimal code change; rayon already exists in the dependency tree (transitive via image/rav1e); work-stealing scales with cores; preserves order with `par_iter` indexed collection; `Track` is `Send` (all fields are Send-safe); no async complexity
- **Cons**: Adds a direct rayon dependency to `playback` crate; thread pool spawned on first use (minimal overhead); slightly more complex error handling
- **Effort**: low

### Option 2: Lazy metadata loading (deferred reads)

Store only the file path at playlist load time, defer metadata parsing until the track is displayed or played. The `Track` struct would need an internal state for "metadata not yet loaded."

- **Pros**: Near-instant startup regardless of playlist size; memory efficient for tracks never viewed
- **Cons**: Requires significant refactoring of `Track` (adding Option-wrapping or an enum for loaded/unloaded state); changes observable behavior — TUI would initially show "Unknown Title" for all tracks; every accessor (title, artist, duration, album) must handle the unloaded case; ripple effects across TUI rendering code (playlist.rs:584, 645); violates constraint of not changing public API
- **Effort**: high

### Option 3: Async parallel with tokio::spawn_blocking

Use `tokio::spawn_blocking` for each track read, then `join_all` the futures. This leverages the existing tokio runtime.

- **Pros**: No new dependency; uses existing tokio runtime; naturally async
- **Cons**: `Playlist::load()` is currently synchronous and called from `new_shared()` which is also sync; would require making `load()` async (API change); tokio's blocking thread pool has a default limit of 512 threads which could be hit with large playlists; adds async machinery to what is conceptually a pure data-loading function
- **Effort**: medium

### Option 4: Batch with std thread pool (scoped threads)

Use `std::thread::scope` to spawn N worker threads that process chunks of paths in parallel.

- **Pros**: No external dependency; explicit control over thread count; scoped threads guarantee lifetime safety
- **Cons**: More boilerplate than rayon; manual work distribution; no work-stealing (uneven file sizes cause stragglers); requires Rust 1.63+ (already satisfied)
- **Effort**: medium

## Acceptance Criteria

- **AC-01**: `Playlist::load()` processes local file metadata reads in parallel using rayon's `par_iter`, achieving wall-clock speedup proportional to available CPU cores (minimum 3x improvement on a 4-core machine with 200+ tracks).
- **AC-02**: Playlist track order after loading is identical to the order of entries in the playlist file, regardless of parallelization (order-preserving collection).
- **AC-03**: The public API signatures of `Playlist::new()`, `Playlist::new_shared()`, `Playlist::load()`, `Playlist::load_apply()`, and `Track::read_track_from_path()` remain unchanged.
- **AC-04**: All existing 385 tests pass without modification.
- **AC-05**: Tracks where metadata parsing fails are still gracefully handled (skipped with debug log, same as current behavior on line 247-249).
- **AC-06**: Podcast URL lookups and radio track creation remain unaffected by the change (these are cheap in-memory operations, not parallelized).
- **AC-07**: The `rayon` crate is added as a direct dependency to the `playback` crate's `Cargo.toml` (it already exists transitively in the dependency tree).
- **AC-08**: No regression in memory usage — peak RSS increase is bounded to the additional per-thread stack overhead of rayon's thread pool (typically 8MB total for 8 threads).

## Non-Functional Requirements

- **Performance** (high): Playlist load time for 500 local tracks must drop from ~10s to under 3s on a 4-core machine. The optimization must not introduce latency for small playlists (< 50 tracks) — rayon's overhead for small collections should be negligible.
- **Security** (low): No new attack surface introduced; file paths are already trusted input from the user's own playlist file.
- **Accessibility** (low): N/A — this is a backend performance optimization with no UI changes.
- **Reliability** (high): A panic in any parallel metadata read must not crash the application. Rayon's default panic behavior (propagate to calling thread) is acceptable since `parse_metadata_from_file` already handles errors gracefully (returns default metadata on failure).

## Open Questions

- What is the acceptable upper bound for playlist size? Should there be a hard cap on track count, or should the system degrade gracefully regardless of size?
- Should a metadata cache (e.g., SQLite or serde file) be considered as a follow-up optimization to avoid re-reading unchanged files on subsequent launches?
- Is there a measurable benefit to parallelizing the podcast DB lookup phase, or is the HashMap lookup already fast enough that it is negligible?

## Recommendations

1. **Option 1 (rayon par_iter) is the recommended approach**: It requires the least code change, has the best risk/reward ratio, fits the synchronous calling context perfectly, and rayon is already a transitive dependency. The implementation is approximately 15-20 lines of code change in `Playlist::load()`.
2. **Consider a metadata cache as a follow-up**: Even with parallelization, reading 500 files from disk on every startup is wasteful if the files have not changed. A follow-up ticket could introduce a simple file-path + mtime -> metadata cache to eliminate redundant reads entirely.
3. **Benchmark before and after**: Add a `tracing::info!` timing span around the parallel load section to enable future monitoring of playlist load performance.

## Assumptions

- `Track` is `Send` — verified: all fields (MediaTypes containing PathBuf/String/Url, Option<Duration>, Option<String>) are Send-safe. No Rc, no raw pointers, no Cell.
- `parse_metadata_from_file` is safe to call from multiple threads concurrently — it opens independent file handles and performs no shared mutable state access. The `lofty` crate is documented as thread-safe for concurrent reads of different files.
- The playlist file format is stable: first line is track index, subsequent lines are either URLs (http-prefixed) or local file paths. This structure allows clean separation of "cheap" lines (URLs) from "expensive" lines (local paths).
- rayon's global thread pool initialization is acceptable during startup (one-time cost of ~1ms).
- The current behavior where a failed `Track::read_track_from_path` silently skips the track (line 247-249) is intentional and should be preserved in the parallel version.
