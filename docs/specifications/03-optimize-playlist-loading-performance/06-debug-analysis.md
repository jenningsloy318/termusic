# Debug Analysis: Sequential Metadata Reads Causing Slow Playlist Startup

## Metadata

| Field | Value |
|-------|-------|
| **Title** | Debug Analysis: Sequential Metadata Reads Causing Slow Playlist Startup |
| **Date** | 2026-06-26 |
| **Author** | super-dev:debug-analyzer |
| **Status** | Hypotheses Pending |
| **Severity** | High |

---

## Issue Summary

| Field | Value |
|-------|-------|
| Symptom | Startup takes 10+ seconds when the playlist contains 500+ local audio files |
| Expected Behavior | Server becomes interactive within 2-3 seconds regardless of playlist size |
| Actual Behavior | `Playlist::load()` blocks for ~10 seconds reading metadata sequentially for every track |
| First Observed | After PR #720 (podcast sync) significantly increased typical playlist sizes |
| Frequency | Always (100% reproducible with large playlists) |
| Environment | Linux 7.0.13-1-liquorix-amd64, Rust (stable), termusic master branch |

---

## Evidence Collected

### Error Messages

```text
No error messages — this is a performance degradation, not a functional failure.
The application eventually starts; it is simply slow.
```

### Logs

```text
No timing logs currently exist around Playlist::load(). The function completes silently.
Approximate timing: 500 tracks * ~20ms per metadata read = ~10 seconds wall-clock.
```

### Visual Evidence

Not applicable — terminal music player with no visual artifacts; the symptom is perceived latency.

### Context

- **Recent Changes**: PR #720 podcast synchronization feature dramatically increased the number of tracks in typical playlists (from ~50 to 500+)
- **Affected Scope**: All users with large playlists; most visible for podcast-heavy users
- **Related Issues**: No prior performance tickets found; this is a latent issue exposed by increased playlist sizes

---

## Reproduction Strategy

- **Technique**: CLI invocation
- **Deterministic**: Yes

### Steps to Reproduce

1. Create a `playlist.log` file in the config directory containing 500+ local audio file paths (one per line, first line is the track index)
2. Launch `termusic-server`
3. Measure time from process start to server responding to TUI connection

### Minimal Reproduction

A standalone benchmark that calls `Playlist::load()` with a prepared playlist file containing N valid local audio file paths and measures wall-clock time. Alternatively, instrument `Playlist::load()` with `std::time::Instant` around the `for line in lines` loop at `playback/src/playlist.rs:226`.

### Reproduction Confirmation

Not yet confirmed via instrumented run — this is the hypothesis-generation pass. The code path analysis below confirms the sequential nature of the loop.

---

## Code Execution Path

```
actual_main() [server/src/server.rs:149]
    → Playlist::new_shared() [playback/src/playlist.rs]
        → Playlist::load() [playback/src/playlist.rs:188]
            → BufReader::lines() [line 199]
            → for line in lines [line 226]
                → Track::read_track_from_path(&line) [line 247]
                    → parse_metadata_from_file(&path, ...) [lib/src/track.rs:250]
                        → lofty::Probe::open(path) [blocking disk I/O]
                        → TaggedFile::read(...) [ID3/MP4 parsing]
```

### Trace

| Step | Location | Action | Data State |
|------|----------|--------|------------|
| 1 | server/src/server.rs:149 | Call Playlist::new_shared() | Server startup blocked |
| 2 | playback/src/playlist.rs:188 | Open playlist.log, read lines | File opened, BufReader created |
| 3 | playback/src/playlist.rs:202 | Parse first line as track index | current_track_index set |
| 4 | playback/src/playlist.rs:214 | Load podcast DB, build URL HashMap | Episode lookup index ready |
| 5 | playback/src/playlist.rs:226 | Begin sequential for-loop over remaining lines | 500+ lines to process |
| 6 | playback/src/playlist.rs:247 | For each local path: Track::read_track_from_path | ~20ms blocking I/O per track |
| 7 | lib/src/track.rs:250 | parse_metadata_from_file opens file, reads tags | Blocking lofty crate call |
| 8 | playback/src/playlist.rs:254 | After ALL tracks loaded, clamp index | 10+ seconds have elapsed |

---

## Hypotheses

Ranked by likelihood. Each hypothesis has a falsifiable prediction.

### HYP-001: Sequential blocking I/O in the for-loop is the dominant bottleneck

- **Likelihood**: High (0.85)
- **Prediction**: If we replace the sequential `for line in lines` loop (line 226-250) with rayon `par_iter` over collected local-path lines, wall-clock time for the metadata-read phase will decrease by a factor of approximately N (where N = available CPU cores). On an 8-core machine, 10 seconds should drop to ~1.25 seconds.
- **Supporting Evidence**: Code at `playback/src/playlist.rs:226-250` shows a simple sequential loop with no concurrency; each iteration calls `Track::read_track_from_path` which performs blocking file I/O (lofty crate); 500 files * 20ms = 10 seconds; CPU is idle during each I/O wait, meaning parallelism can overlap I/O.
- **Contradicting Evidence**: None — the sequential nature is obvious from code inspection.
- **Verification Method**: Add `std::time::Instant::now()` timing around the for-loop, then replace with `par_iter` and measure again. The speedup factor should be proportional to core count (3-8x).
- **Result**: UNVERIFIED
- **Result Evidence**: Awaiting instrumented benchmark run.

### HYP-002: The podcast DB load and HashMap construction is a secondary bottleneck

- **Likelihood**: Low (0.10)
- **Prediction**: If we time the `db_podcast.get_podcasts()` call and HashMap construction (lines 214-224) separately from the track-reading loop, the DB phase will consume less than 500ms even with thousands of episodes. If this hypothesis were true, eliminating it alone would not meaningfully reduce total load time.
- **Supporting Evidence**: SQLite database reads and in-memory HashMap construction are generally fast (O(n) with n = episode count); requirements doc states these are "cheap HashMap lookups"; the DB is local SQLite.
- **Contradicting Evidence**: With a very large podcast database (10,000+ episodes), SQLite queries and HashMap allocation could become non-trivial. However, this would be secondary to 10 seconds of file I/O.
- **Verification Method**: Insert timing instrumentation between lines 213 and 226 to measure DB load time independently. If it exceeds 1 second, this becomes a contributing factor.
- **Result**: UNVERIFIED
- **Result Evidence**: Not yet measured.

### HYP-003: File system latency amplification (cold cache, HDD, NFS) makes per-file I/O worse than 20ms

- **Likelihood**: Medium (0.15)
- **Prediction**: If metadata reads are timed individually on a cold-cache or network-mounted filesystem, average per-file time will exceed 50ms (rather than 20ms), making the sequential bottleneck even worse than estimated. Conversely, on warm SSD cache, per-file time may be closer to 5-10ms, meaning the problem is less severe than reported.
- **Supporting Evidence**: The 20ms estimate comes from requirements analysis; actual timing depends on storage media, filesystem cache state, and file size. Podcast MP3 files with large embedded artwork can require reading more data for ID3 parsing. Users on HDDs or network storage would experience amplified latency.
- **Contradicting Evidence**: Most modern Linux systems have aggressive page cache; after first access, metadata reads may be sub-5ms.
- **Verification Method**: Instrument `Track::read_track_from_path` with per-call timing across 100 files on the target system. If median exceeds 20ms, this hypothesis is confirmed as an amplifying factor. If median is under 10ms, the 10-second estimate is overstated (but sequential bottleneck still exists).
- **Result**: UNVERIFIED
- **Result Evidence**: Not yet measured.

### HYP-004: Track type not being Send prevents straightforward parallelization

- **Likelihood**: Low (0.05)
- **Prediction**: If `Track` is not `Send`, then rayon `par_iter` with `Track::read_track_from_path` will fail to compile, requiring either a wrapper type or a different parallelization strategy. If this hypothesis is true, attempting to use `par_iter().map(|path| Track::read_track_from_path(path)).collect()` will produce a compile error about `Send` bounds.
- **Supporting Evidence**: The user report mentions "Track and related types are not Send" but this conflicts with the requirements document which states "all fields (MediaTypes containing PathBuf/String/Url, Option<Duration>, Option<String>) are Send-safe."
- **Contradicting Evidence**: Requirements analysis (01-requirements.md) explicitly verified: "Track is Send — verified: all fields are Send-safe. No Rc, no raw pointers, no Cell." The user's parenthetical about PathBuf being Send actually supports that Track IS Send. This hypothesis is likely false.
- **Verification Method**: Attempt to compile `fn assert_send<T: Send>() {} assert_send::<Track>();` — if it compiles, Track is Send and this hypothesis is refuted.
- **Result**: UNVERIFIED
- **Result Evidence**: Awaiting compile check.

### HYP-005: Interleaved podcast/radio entries prevent simple batch parallelization of local paths

- **Likelihood**: Medium (0.20)
- **Prediction**: If the playlist file interleaves HTTP URLs (podcast/radio) with local file paths, a naive "collect all lines then par_iter" approach will break ordering because the positions of URL-based tracks relative to local-path tracks must be preserved. If this hypothesis is true, simply parallelizing local paths and concatenating the results will produce incorrect track order when the playlist has mixed entry types.
- **Supporting Evidence**: Code at lines 237-249 shows HTTP lines are handled differently (podcast episode lookup or radio track creation) from local paths (metadata read). The for-loop interleaves both types in a single pass, and the output order depends on the input line order. BDD SCENARIO-021 explicitly tests this: "Playlist file with mixed addresses and local paths preserves global order."
- **Contradicting Evidence**: The parallelization can be structured as a two-phase approach: (1) collect and classify all lines with their original indices, (2) par_iter only the local paths for metadata reads, (3) merge results back into the correct positions. This is a solvable implementation challenge, not a fundamental blocker.
- **Verification Method**: Examine a real `playlist.log` file to determine the ratio and distribution of HTTP vs local-path entries. If the file is 95%+ local paths with minimal interleaving, a simpler approach (collect all local paths, par_iter, then merge) is feasible without complex index tracking. If heavily interleaved, the implementation must preserve positional indices.
- **Result**: UNVERIFIED
- **Result Evidence**: Not yet examined actual playlist file structure.

---

## Hypothesis Tree (Probability Distribution)

```
Root Cause: Why is playlist loading slow? (100%)
├── Compute/IO bound in track metadata reads (85%) [HYP-001]
│   ├── Sequential loop is sole bottleneck (70%) [HYP-001 direct]
│   └── Per-file I/O is amplified by storage conditions (15%) [HYP-003]
├── Secondary bottleneck in DB/podcast loading (10%) [HYP-002]
└── Implementation barrier to parallelization (5%)
    ├── Track not being Send (2%) [HYP-004]
    └── Interleaved ordering constraint (3%) [HYP-005]
```

Note: HYP-004 and HYP-005 are not root causes of the performance problem itself — they are potential barriers to the fix. The performance root cause is almost certainly HYP-001.

---

## Verified Root Cause

- **Confirmed Hypothesis**: (Pending verification — awaiting instrumented run)
- **Root Cause**: (Pending)
- **Location**: playback/src/playlist.rs:226-250

### Evidence Chain

1. (Pending instrumented timing confirmation)

### Why It Was Not Caught

The original implementation was designed for small playlists (< 50 tracks) where sequential loading completed in under 1 second. No performance budget was established during the podcast sync design phase (PR #720), which significantly increased typical playlist sizes without measuring load-time impact. No benchmark test or performance regression gate exists for playlist loading.

---

## Recommended Fix

### Fix Approach

Replace the sequential for-loop at `playback/src/playlist.rs:226-250` with a two-phase approach:
1. **Phase 1 (Classify)**: Collect all lines from the file into a Vec, classifying each as HTTP-URL or local-path while preserving original order indices.
2. **Phase 2 (Parallel Read)**: Use rayon `par_iter` over the local-path entries to read metadata in parallel, then merge results back into the correct positions alongside the URL-based tracks.

### Code Locations

| File | Line | Change |
|------|------|--------|
| playback/src/playlist.rs | 226-250 | Replace sequential for-loop with classify + par_iter + merge |
| playback/Cargo.toml | dependencies | Add `rayon = "1.12"` as direct dependency |

### Alternative Approaches

- **Lazy loading (Option 2)**: Avoids all upfront I/O but requires extensive Track refactoring and violates public API stability constraint (AC-03)
- **tokio::spawn_blocking (Option 3)**: Would require making Playlist::load() async, which is an API change
- **std::thread::scope (Option 4)**: More boilerplate than rayon, no work-stealing, manual work distribution

---

## Regression Test Strategy

### Test Seam

Integration level: test `Playlist::load()` with prepared playlist files of varying sizes and entry types. The seam is at the `Playlist::load()` function boundary — it takes no arguments (reads from config path) so tests need to set up the config environment.

### Test Cases

| Test Name | Input | Expected Output | Verifies |
|-----------|-------|-----------------|----------|
| test_load_preserves_order_with_mixed_entries | Playlist with interleaved URLs and local paths | Tracks in exact input order | HYP-005 / SCENARIO-021 |
| test_load_parallel_speedup | Playlist with 200+ local paths (on 4+ core machine) | Wall-clock < 3x sequential time | HYP-001 / SCENARIO-001 |
| test_load_handles_failed_metadata_gracefully | Playlist with some invalid file paths | Valid tracks loaded, invalid skipped | SCENARIO-010 |
| test_load_empty_playlist | Empty playlist file | Returns (0, empty vec) | SCENARIO-017 |
| test_load_single_track | Single local path entry | Returns (0, vec with 1 track) | SCENARIO-018 |

---

## Prevention Recommendations

- **Process**: Establish a performance budget for startup-critical paths; require benchmark tests for any feature that increases dataset sizes (like podcast sync adding hundreds of tracks)
- **Monitoring**: Add `tracing::info!` timing span around `Playlist::load()` to surface load-time regressions in development; consider a startup-time CI check that fails if load with a fixture playlist exceeds a threshold
- **Architecture**: For data-loading paths that scale with user content, default to parallel/batched I/O patterns rather than sequential loops; consider a metadata cache (mtime-based) to avoid re-reading unchanged files on subsequent launches
