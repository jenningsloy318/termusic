# Technical Specification: Optimize Playlist Loading Performance

- **Date**: 2026-06-26
- **Author**: super-dev:spec-writer
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md

---

## 1. Overview

This specification defines the technical approach for parallelizing metadata reads during playlist loading in termusic. The current `Playlist::load()` function at `playback/src/playlist.rs:188` processes local audio file metadata sequentially, causing 10+ second startup delays with 500+ track playlists. The optimization replaces the sequential for-loop with a two-phase approach: classify lines by type, then batch-process local file paths using rayon's `par_iter` for parallel metadata reads.

The solution targets the single hotspot (`playback/src/playlist.rs:226-250`) with approximately 30 lines of code change. It preserves all public API signatures (AC-03), maintains playlist ordering (AC-02), and achieves wall-clock speedup proportional to available CPU cores (AC-01). Rayon is added as a direct dependency to the `playback` crate, though it already exists in the dependency tree transitively via `image -> ravif -> rav1e`.

The design separates "cheap" operations (podcast URL HashMap lookups, radio track creation) from "expensive" operations (local file metadata I/O via lofty crate) and parallelizes only the expensive path. This approach minimizes risk, avoids async complexity, and fits cleanly into the existing synchronous calling context.

## 2. Architecture

### 2.1. Two-Phase Load Architecture

The current single-pass loop in `Playlist::load()` mixes three concerns: line classification, podcast/radio track creation (cheap), and local file metadata I/O (expensive). The new architecture separates these into two distinct phases:

**Phase A (Sequential Classification)**: Read all lines from the playlist file, classify each line as one of three types (podcast URL, radio stream, or local file path), and record its original index position.

**Phase B (Parallel Processing + Merge)**: Process local file paths in parallel using rayon `par_iter`, resolve podcast/radio entries via in-memory lookups (remains sequential/inline), then assemble the final `Vec<Track>` in original playlist order.

```
playlist.log
    |
    v
[Phase A: Sequential Line Collection]
    lines.map_while(Result::ok)
    .filter(non-empty, non-comment)
    .enumerate()
    .collect::<Vec<(usize, String)>>()
    |
    v
[Classify each (index, line)]
    +-- starts_with("http") --> PlaylistEntry::PodcastOrRadio { index, line }
    +-- else               --> PlaylistEntry::LocalPath { index, line }
    |
    v
[Phase B: Process by Type]
    Local paths: par_iter().filter_map(read_metadata).collect() --> Vec<(usize, Track)>
    Podcast/Radio: sequential HashMap lookup --> Vec<(usize, Track)>
    |
    v
[Merge: sort by original index, extract Track values]
    --> Vec<Track> (order-preserving)
```

### 2.2. Rayon Integration Point

Rayon's `par_iter` is applied exclusively to the local file path entries. The entry point is the call to `Track::read_track_from_path()` which internally calls `parse_metadata_from_file()` (lofty crate). Each invocation opens an independent file handle with no shared mutable state, making it safe for concurrent execution.

Rayon's global thread pool is used (no custom pool configuration). The pool is initialized on first use with a thread count equal to available CPU cores. This initialization cost is approximately 1ms and occurs only once per process lifetime.

### 2.3. Order Preservation Strategy

The classified entries retain their original line index from the playlist file. After parallel processing, results are merged by sorting on this index. This guarantees that regardless of parallel execution order, the final `Vec<Track>` matches the file's line order exactly.

Specifically:
- Each classified entry carries its `original_line_index: usize`
- Parallel results produce `Vec<(usize, Track)>` tuples
- Sequential results produce `Vec<(usize, Track)>` tuples
- Both vectors are combined and sorted by the `usize` index
- The final `Vec<Track>` is extracted by mapping over the sorted result

This addresses SCENARIO-004, SCENARIO-005, SCENARIO-006, and SCENARIO-021.

### 2.4. Error Handling Architecture (AC-05)

Line-reading errors use `map_while(Result::ok)` to stop at the first I/O error, preserving the original abort-on-first-error semantics while enabling batch collection. This follows Clippy lint `lines_filter_map_ok` recommendation and is documented as an intentional semantic choice.

Individual track metadata failures are handled identically to the current behavior (AC-05): `Track::read_track_from_path` returns `Err` for unparseable files, and the parallel filter_map silently excludes them. The debug-level logging inside `read_track_from_path` (track.rs:263) remains the sole log point for failed reads.

Panic handling accepts rayon's default propagation behavior. Lofty 0.24.0 has extensive fuzzing (8+ fuzz targets) and uses `ParsingMode::BestAttempt` by default, making panics extremely unlikely. No `catch_unwind` is added per task.

### 2.5. Podcast and Radio Isolation (AC-06)

Podcast URL lookups and radio track creation remain entirely unaffected by the parallelization (AC-06). These entries are classified during the sequential enumeration phase and resolved via in-memory HashMap lookups (O(1) per entry). They are never included in the rayon `par_iter` batch. This separation ensures that the cheap in-memory operations are not mixed with the expensive disk I/O path and that any future changes to podcast resolution logic remain decoupled from the parallelization strategy.

## 3. Data Models

### 3.1. PlaylistLineEntry Enum

An internal enum (not exported) used during the classification phase to tag each line with its type and original position. This enum exists only within the `Playlist::load()` function scope.

```rust
/// Internal classification of a playlist file line for parallel processing dispatch.
/// Not exported — exists only within Playlist::load() scope.
enum PlaylistLineEntry {
    /// A local file path requiring metadata I/O (expensive operation).
    LocalPath {
        original_index: usize,
        file_path: String,
    },
    /// An HTTP/HTTPS URL requiring podcast episode lookup or radio track creation (cheap operation).
    NetworkAddress {
        original_index: usize,
        url: String,
    },
}
```

### 3.2. Existing Types (Unchanged)

The following types are used but NOT modified:

```rust
// playback/src/playlist.rs — signature unchanged (AC-03)
impl Playlist {
    pub fn load() -> Result<(usize, Vec<Track>)>;
    pub fn new() -> Self;
    pub fn new_shared() -> SharedPlaylist;
    pub fn load_apply(&mut self) -> Result<()>;
}

// lib/src/track.rs — signature unchanged (AC-03)
impl Track {
    pub fn read_track_from_path(path: &str) -> Result<Self>;
}
```

## 4. API Design

### 4.1. No External API Changes

This optimization is entirely internal to `Playlist::load()`. No public function signatures, return types, or error types change. No new public types are introduced. The `PlaylistLineEntry` enum is a local implementation detail.

**Contract verification (AC-03)**:
- `Playlist::new()` — unchanged signature, unchanged behavior
- `Playlist::new_shared()` — unchanged signature, unchanged behavior
- `Playlist::load()` — unchanged signature `pub fn load() -> Result<(usize, Vec<Track>)>`; behavior change is strictly performance (same output for same input)
- `Playlist::load_apply(&mut self)` — unchanged signature, unchanged behavior
- `Track::read_track_from_path(&str)` — unchanged signature, unchanged behavior

### 4.2. Internal Function Contracts

The following internal helper may be extracted for clarity (optional, at implementer discretion):

```rust
/// Classify a playlist line as either a local file path or network address.
///
/// # Arguments
/// * `line` - A non-empty, non-comment line from playlist.log
///
/// # Returns
/// `true` if the line starts with "http://" or "https://", indicating a network address.
/// `false` for local file paths requiring metadata I/O.
fn is_network_address(line: &str) -> bool;
```

Input contract: `line` is a trimmed, non-empty string that does not start with `#`.
Output contract: Returns `true` for HTTP/HTTPS URLs, `false` for local paths.

## 5. Implementation Details

### 5.1. Line Collection with map_while

Replace the current `for line in lines` loop preamble with batch collection:

```rust
// NOTE: Original code used `line?` which aborted on first I/O error.
// The batch approach uses map_while(Result::ok) which stops reading at the first
// I/O error but does NOT propagate it as an Err — the caller receives Ok with
// whatever lines were successfully read before the error. This is acceptable because:
// 1. playlist.log is a local regular file; mid-read I/O errors are near-impossible
// 2. Partial playlist loading is preferable to total startup failure
// 3. If the file is truly unreadable, the first line (track index) read would have
//    already failed and propagated via the earlier `line?` on line 203
let all_lines: Vec<(usize, String)> = lines
    .map_while(|line_result| line_result.ok())
    .filter(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    })
    .enumerate()
    .collect();
```

This addresses SCENARIO-017 (empty playlist), SCENARIO-018 (single track), and maintains the abort-on-first-I/O-error semantics closest to the original `line?` behavior.

### 5.2. Line Classification

Separate lines into two groups based on their prefix:

```rust
let (network_entries, local_entries): (Vec<_>, Vec<_>) = all_lines
    .into_iter()
    .partition(|(_, line)| line.starts_with("http://") || line.starts_with("https://"));
```

Network entries (podcast URLs, radio streams) are processed sequentially via the existing `episode_by_url` HashMap lookup. Local entries are batched for parallel metadata reads.

### 5.3. Parallel Metadata Read with Rayon

```rust
use rayon::prelude::*;

// SAFETY NOTE: Lofty 0.24.0 uses ParsingMode::BestAttempt (default) and has extensive
// fuzz testing across all formats. Panics are extremely unlikely. If a panic does occur,
// rayon propagates it to this thread after other tasks complete. Since read_track_from_path
// already catches all Err variants (falling back to default metadata), only a true internal
// panic in lofty would propagate — a scenario not worth the optimization-inhibiting cost
// of per-task catch_unwind.
let local_tracks: Vec<(usize, Track)> = local_entries
    .par_iter()
    .filter_map(|(original_index, file_path)| {
        Track::read_track_from_path(file_path)
            .ok()
            .map(|track| (*original_index, track))
    })
    .collect();
```

This addresses SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-010, SCENARIO-011, and SCENARIO-012.

### 5.4. Sequential Network Address Resolution

```rust
let network_tracks: Vec<(usize, Track)> = network_entries
    .iter()
    .filter_map(|(original_index, url)| {
        if let Some(episode) = episode_by_url.get(url.as_str()) {
            Some((*original_index, Track::from_podcast_episode(episode)))
        } else {
            Some((*original_index, Track::new_radio(url)))
        }
    })
    .collect();
```

This addresses SCENARIO-013 and SCENARIO-014 — podcast and radio entries remain in their existing sequential resolution path.

### 5.5. Order-Preserving Merge

```rust
let mut indexed_tracks: Vec<(usize, Track)> = Vec::with_capacity(
    local_tracks.len() + network_tracks.len()
);
indexed_tracks.extend(local_tracks);
indexed_tracks.extend(network_tracks);
indexed_tracks.sort_unstable_by_key(|(index, _)| *index);

let playlist_items: Vec<Track> = indexed_tracks
    .into_iter()
    .map(|(_, track)| track)
    .collect();
```

This addresses SCENARIO-004, SCENARIO-005, SCENARIO-006, and SCENARIO-021.

### 5.6. Elapsed Time Logging

```rust
let load_start = std::time::Instant::now();
// ... parallel processing ...
info!(
    "Loaded {} tracks ({} local, {} network) in {:?}",
    playlist_items.len(),
    local_tracks_count,
    network_tracks_count,
    load_start.elapsed()
);
```

This enables future performance monitoring and regression detection.

### 5.7. Dependency Declaration

Add rayon to workspace dependencies in root `Cargo.toml`:

```toml
# In [workspace.dependencies]
rayon = "1.12"
```

Reference in `playback/Cargo.toml`:

```toml
# In [dependencies]
rayon.workspace = true
```

This addresses SCENARIO-015 and AC-07.

## 6. Testing Strategy

The testing approach covers correctness (order preservation, error handling), performance (speedup verification), and regression (existing tests pass unchanged).

### 6.1. Unit Tests

- Verify `PlaylistLineEntry` classification logic correctly identifies HTTP URLs versus local paths
- Verify that `map_while(Result::ok)` collection stops at first I/O error (mock reader test)
- Verify order-preserving merge produces correct output for interleaved inputs
- Verify empty input produces empty output without panic
- Verify single-item input produces single-item output

### 6.2. Integration Tests

- Load a prepared playlist file with 200+ valid local audio paths and verify all tracks are present in correct order
- Load a playlist with mixed podcast URLs and local paths, verify interleaved order is preserved
- Load a playlist where some entries reference non-existent files, verify failed tracks are skipped and valid tracks retain correct positions
- Load a playlist where ALL entries fail, verify empty result with no crash (SCENARIO-020)
- Run the full existing test suite (385 tests) without modification (SCENARIO-009)

### 6.3. E2E Tests

- Benchmark: measure wall-clock time for loading a 500-track playlist on a multi-core machine, verify speedup exceeds 3x compared to sequential baseline (SCENARIO-001, SCENARIO-002)
- Startup test: launch termusic-server with a large playlist, verify server becomes responsive within 3 seconds

### 6.4. BDD Scenario References

- **SCENARIO-001** — integration — Covered (parallel speedup test with 200+ tracks)
- **SCENARIO-002** — integration — Covered (scaling verification on multi-core)
- **SCENARIO-003** — integration — Covered (small playlist overhead check)
- **SCENARIO-004** — unit/integration — Covered (order preservation test)
- **SCENARIO-005** — unit — Covered (order independent of read duration)
- **SCENARIO-006** — integration — Covered (order preserved with failed tracks)
- **SCENARIO-007** — integration — Covered (compile-time signature check, existing tests)
- **SCENARIO-008** — integration — Covered (compile-time signature check, existing tests)
- **SCENARIO-009** — integration — Covered (run full test suite)
- **SCENARIO-010** — integration — Covered (failed metadata skip with debug log)
- **SCENARIO-011** — integration — Covered (multiple consecutive failures)
- **SCENARIO-012** — integration — Partial (rayon default panic propagation accepted; documented risk)
- **SCENARIO-013** — integration — Covered (podcast entries not parallelized)
- **SCENARIO-014** — integration — Covered (radio entries not parallelized)
- **SCENARIO-015** — integration — Covered (Cargo.toml inspection + cargo build)
- **SCENARIO-016** — e2e — Covered (memory profiling during large load)
- **SCENARIO-017** — unit — Covered (empty playlist test)
- **SCENARIO-018** — unit — Covered (single track test)
- **SCENARIO-019** — e2e — Covered (10K track resource bound check)
- **SCENARIO-020** — integration — Covered (all tracks fail test)
- **SCENARIO-021** — integration — Covered (mixed interleaved order test)

## 7. Non-Functional Requirements

### 7.1. Performance

- Playlist load time for 500 local tracks must drop from ~10s to under 3s on a 4-core machine (AC-01)
- Small playlists (< 50 tracks) must not experience measurable regression from parallelization overhead (SCENARIO-003)
- The optimization must not introduce latency for the podcast/radio lookup path (these remain O(1) HashMap lookups)
- Rayon's global thread pool initialization cost (~1ms) is acceptable at startup

### 7.2. Memory Efficiency

- Peak RSS increase bounded to rayon's per-thread stack overhead: approximately 8MB total for 8 worker threads (AC-08, SCENARIO-016)
- No per-track memory duplication beyond the normal Track allocation
- The intermediate `Vec<(usize, String)>` for all lines adds temporary memory proportional to playlist file size (bounded by file content already read into memory)

### 7.3. Reliability

- A panic in lofty during parallel metadata parsing does not crash the application in practice due to lofty's extensive fuzzing and `BestAttempt` mode (SCENARIO-012)
- Failed metadata reads are silently excluded from the playlist with debug-level logging (same as current behavior)
- The function returns `Ok` with a partial playlist if some tracks fail (graceful degradation)

### 7.4. Compatibility

- All 385 existing tests pass without modification (AC-04, SCENARIO-009)
- Public API signatures unchanged (AC-03, SCENARIO-007, SCENARIO-008)
- Minimum Rust version requirement satisfied (workspace uses edition 2024, rust-version 1.90)
- `map_while` stabilized in Rust 1.57 (satisfied)

## 8. Risks and Mitigations

- **Risk**: Rayon global thread pool contention if other parts of the application use rayon concurrently during startup
  - Likelihood: low
  - Impact: low
  - Mitigation: `Playlist::load()` is called once during startup before any other rayon-using code paths execute. The global pool is shared but contention is impossible at this point.

- **Risk**: Lofty panics on a malformed file causing rayon to propagate the panic and lose all loaded tracks from the parallel batch
  - Likelihood: low (lofty has 8+ fuzz targets, uses BestAttempt mode)
  - Impact: medium (entire playlist load fails on that startup)
  - Mitigation: Lofty's extensive fuzzing makes this extremely unlikely. If observed in production, a single `catch_unwind` can be wrapped around the outer `par_iter().collect()` call (not per-task) as a straightforward fix.

- **Risk**: File descriptor exhaustion on systems with very low ulimits when processing 10,000+ tracks
  - Likelihood: low
  - Impact: medium (some metadata reads fail)
  - Mitigation: Rayon's thread pool bounds concurrency to CPU core count. At most N files are open simultaneously (where N = core count). This is well within any reasonable ulimit.

- **Risk**: The `episode_by_url` HashMap borrows from `podcasts` on the stack, and the borrow checker rejects sharing it with the parallel closure
  - Likelihood: low (HashMap<&str, &Episode> is Sync; par_iter closures capture shared references)
  - Impact: low (compile error caught immediately)
  - Mitigation: The HashMap is only read (not mutated) during parallel processing. If borrow issues arise, clone the HashMap keys to owned Strings. However, shared `&HashMap` references are valid in rayon closures since HashMap is Sync.

- **Risk**: Ordering regression introduced by incorrect index assignment
  - Likelihood: low
  - Impact: high (silent playlist corruption)
  - Mitigation: Dedicated integration test (SCENARIO-004, SCENARIO-005, SCENARIO-021) verifies order preservation with deterministic inputs. The index is assigned during sequential enumeration, making it trivially correct.
