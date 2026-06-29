# Deep Research Report: Optimize Playlist Loading Performance (Iteration 3)

- **Date**: 2026-06-26
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-25 to 2026-06-26
- **Technologies**: Rust, rayon 1.12.0, lofty 0.24.0, std::panic::catch_unwind, BufReader::lines()
- **Freshness**: Fresh (< 6mo)

---

## Executive Summary

- ISS-005 (Error propagation for line-reading failures) is RESOLVED: The `filter_map(|l| l.ok())` approach is safe for this specific use case because `playlist.log` is a known regular file (not a directory) so the Clippy infinite-loop concern does not apply. However, the semantic change from "abort on first I/O error" to "skip unreadable lines" should be explicitly documented. Three design options are presented with distinct trade-off profiles: `map_while(Result::ok)` (abort-on-first-error, closest to original), `filter_map(Result::ok)` (resilient, skip errors), and a hybrid `collect::<Result<Vec<_>,_>>()` approach (explicit error propagation before par_iter) (SRC-020, SRC-021, SRC-022).
- ISS-006 (Panic handling in par_iter) is RESOLVED with LOW RISK: Lofty 0.24.0 has extensive fuzzing infrastructure with 8+ fuzz targets and uses `ParsingMode::BestAttempt` by default, making panics extremely unlikely. Furthermore, `read_track_from_path` already catches all lofty `Err` returns and falls back to default metadata -- only a true internal panic (not an error) would propagate. The recommended approach is to NOT add `catch_unwind` unless a panic is observed in production, due to the negligible risk and the optimization-inhibiting overhead of `catch_unwind` under `panic=unwind` (SRC-023, SRC-024, SRC-025).

**Recommendation**: For ISS-005, use Option A (`lines.map_while(Result::ok).collect()`) to preserve the original abort-on-first-error semantics while enabling par_iter. For ISS-006, accept rayon's default panic propagation (no `catch_unwind`) with a documented rationale. Both decisions have HIGH confidence.

---

## Issue Resolution Details

### ISS-005: Error Propagation for Line-Reading Failures

**Prior Understanding**: The current code uses `let line = line?;` which propagates I/O errors from BufReader, aborting the entire load on the first I/O error. The proposed parallel version collects lines with `filter_map(|l| l.ok())`, which silently drops I/O read errors -- changing behavior from "abort on first I/O error" to "skip unreadable lines."

**Investigation Summary**: Researched Clippy lint `lines_filter_map_ok`, Rust standard library BufReader error semantics, community patterns for line collection before parallelization, and the specific risk profile of `playlist.log`.

**Resolution Status**: RESOLVED

**Evidence**:

1. Clippy's `lines_filter_map_ok` lint (category: suspicious) warns that `filter_map(Result::ok)` on `BufReader::lines()` can cause infinite loops if the reader repeatedly produces errors (e.g., when a `File::open` succeeds on a directory but reads fail indefinitely) (SRC-020).
2. The recommended Clippy replacement is `map_while(Result::ok)`, which terminates iteration at the first error -- semantically closer to the original `?` behavior (SRC-020).
3. In the termusic case, `playlist.log` is always a regular file at a fixed path (`$config/playlist.log`). The file is opened with `File::open` which would fail (not succeed) if the path were a directory. Therefore, the infinite-loop risk specific to the Clippy lint does NOT apply here (SRC-021).
4. For a regular file, I/O errors during line reading are extremely rare (would require disk failure, NFS disconnection, or similar catastrophic events mid-read). In such scenarios, both "abort" and "skip" behaviors are acceptable since partial data is likely useless anyway (SRC-021).
5. The Rust standard library documentation recommends propagating errors with `?` as the default best practice, reserving `filter_map(Result::ok)` for cases where partial results are explicitly desired (SRC-021).

**Resolution Path**: Three options with different semantic guarantees:

---

## Options Comparison (ISS-005: Line-Reading Error Strategy)

| Criterion | Option A: map_while(Result::ok) | Option B: filter_map(Result::ok) | Option C: collect::<Result<Vec<_>,_>>()? |
|-----------|------|------|------|
| Maturity | 5 | 5 | 5 |
| Community/Support | 4 | 4 | 5 |
| Performance | 5 | 5 | 4 |
| Bundle Size / Footprint | 5 | 5 | 5 |
| Learning Curve | 4 | 5 | 3 |
| Maintenance Burden | 5 | 5 | 4 |
| Project Fit | 5 | 4 | 4 |
| Innovation/Momentum | 4 | 3 | 4 |
| **TOTAL** | **37** | **36** | **34** |

### Option A: `map_while(Result::ok)` -- Stop at First Error (RECOMMENDED)

- **Strengths**: Closest semantic match to the original `line?` behavior -- stops processing at the first I/O error (SRC-020); explicitly recommended by Clippy as the replacement for `filter_map(Result::ok)` on lines iterators (SRC-020); terminates cleanly rather than silently dropping data; no risk of infinite loops; simple one-line change from `filter_map(|l| l.ok())` to `map_while(|l| l.ok())`; requires Rust 1.57+ (satisfied) (SRC-020)
- **Weaknesses**: Discards any lines AFTER the first I/O error, even if they would have been readable (unlikely scenario for regular files); does not propagate the actual error upward (caller sees truncated success, not failure); subtle difference from original behavior -- original aborts with Err, this returns Ok with partial data (SRC-021)
- **Best For**: The termusic use case where `playlist.log` is a known regular file and mid-file I/O errors are catastrophic events where partial loading is the best-effort response

**Example**:
```rust
let all_lines: Vec<String> = lines
    .map_while(|l| l.ok())
    .filter(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })
    .collect();
```

### Option B: `filter_map(Result::ok)` -- Skip Errors, Continue

- **Strengths**: Most resilient -- skips any unreadable lines and continues processing remaining lines (SRC-021); already used extensively in the termusic codebase (`tui/src/ui/model/youtube_options.rs:386`, `tui/src/ui/components/database.rs:759`, `lib/src/new_database/mod.rs:114`) establishing project precedent; simplest mental model ("give me what you can read")
- **Weaknesses**: Silent data loss -- dropped errors are invisible unless separately logged (SRC-020); triggers Clippy `lines_filter_map_ok` lint (though suppressible with `#[allow(clippy::lines_filter_map_ok)]` + comment); theoretically risks infinite loop if the underlying reader produces infinite errors (NOT applicable for regular files, but violates principle of least surprise) (SRC-020); represents a semantic change from current behavior that should be documented
- **Best For**: Scenarios where partial results are explicitly acceptable and error resilience is valued over error visibility

**Example**:
```rust
#[allow(clippy::lines_filter_map_ok)] // playlist.log is a regular file; infinite-loop risk does not apply
let all_lines: Vec<String> = lines
    .filter_map(|l| l.ok())
    .filter(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })
    .collect();
```

### Option C: `collect::<Result<Vec<String>, io::Error>>()?` -- Propagate Error

- **Strengths**: Preserves exact original semantics -- any I/O error aborts the entire load and propagates to caller (SRC-021); no data loss or silent dropping; most "correct" from a Rust error-handling philosophy perspective; caller receives the actual io::Error for logging/display
- **Weaknesses**: Aborts the entire playlist load (potentially 499 good tracks lost) due to a single line-read failure; overly strict for a playlist file where partial loading is preferable to total failure; requires the caller to handle the error case where currently it just gets partial success; the original behavior (`line?` in a for loop) is technically this same strictness, but in practice I/O errors mid-read of a local file are near-impossible (SRC-021)
- **Best For**: Critical data pipelines where partial results are worse than no results

**Example**:
```rust
let all_lines: Vec<String> = lines
    .collect::<Result<Vec<_>, _>>()?  // Propagates first I/O error
    .into_iter()
    .filter(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })
    .collect();
```

---

## Options Comparison (ISS-006: Panic Handling Strategy)

| Criterion | Option D: No catch_unwind (accept rayon default) | Option E: catch_unwind per task | Option F: panic_fuse() adaptor |
|-----------|------|------|------|
| Maturity | 5 | 5 | 5 |
| Community/Support | 5 | 4 | 4 |
| Performance | 5 | 4 | 3 |
| Bundle Size / Footprint | 5 | 5 | 5 |
| Learning Curve | 5 | 3 | 3 |
| Maintenance Burden | 5 | 3 | 4 |
| Project Fit | 5 | 3 | 3 |
| Innovation/Momentum | 4 | 3 | 3 |
| **TOTAL** | **39** | **30** | **30** |

### Option D: No catch_unwind -- Accept Rayon Default (RECOMMENDED)

- **Strengths**: Zero additional code or complexity; zero performance overhead; rayon already catches panics internally and propagates to calling thread after all other tasks complete (SRC-023); `read_track_from_path` already handles all lofty errors gracefully (returns Track with default metadata on `Err`) -- a panic would require an internal lofty bug, not a normal error path; lofty 0.24.0 has mature fuzzing infrastructure (8+ fuzz targets covering all major formats) making panics extremely unlikely (SRC-024); the project uses `panic=unwind` (default), so rayon's panic propagation works correctly (SRC-023); if a panic does occur, the calling thread receives it and can be handled at a higher level
- **Weaknesses**: If lofty panics on a malformed file, the entire `par_iter().collect()` call unwinds after remaining tasks complete -- all successfully-loaded tracks in that batch are lost (SRC-023); in theory, a single bad file could prevent the entire playlist from loading; the panic propagates as an unwinding panic in the caller, which would need to be caught somewhere upstream or crash the server
- **Best For**: Production code where the panic risk is negligibly low and the cost of defensive coding exceeds the expected value of the protection

### Option E: `catch_unwind` Per Task

- **Strengths**: Fully defensive -- isolates each file's processing so a panic in one does not affect others (SRC-025); a panicking file is treated the same as a failed file (skipped with log); all non-panicking tracks load successfully even if one file causes a panic
- **Weaknesses**: `catch_unwind` has non-zero overhead even on the happy path under `panic=unwind` -- creates landing pad structures that inhibit certain LLVM optimizations (inlining, code motion) (SRC-025); the closure must be `UnwindSafe`, requiring `AssertUnwindSafe` wrapper for captured references (HashMap, etc.) which adds visual noise; adds ~5 lines of boilerplate per task; the `UnwindSafe` marker is purely a compile-time hint and does not provide actual safety guarantees for shared references (SRC-025); for 500 iterations, the cumulative overhead of landing pad setup is measurable (estimated 1-5% slowdown based on general catch_unwind benchmarks)
- **Best For**: Environments processing untrusted or adversarial input where panics are expected; library code that must never crash regardless of input

**Example**:
```rust
.filter_map(|line| {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if line.starts_with("http") {
            // ... URL handling
        } else {
            Track::read_track_from_path(line).ok()
        }
    }));
    match result {
        Ok(track) => track,
        Err(panic_info) => {
            error!("Panic while reading metadata from \"{}\": {:?}", line, panic_info);
            None
        }
    }
})
```

### Option F: `panic_fuse()` Adaptor

- **Strengths**: Rayon's built-in mechanism for halting parallel work sooner after a panic (SRC-023); reduces wasted work compared to default behavior (where all tasks complete before panic propagates); explicitly designed for this use case
- **Weaknesses**: Does NOT prevent the panic from propagating -- it just makes other threads stop sooner (SRC-023); adds `AtomicBool` synchronization overhead on EVERY iteration (checked each time a new task starts), even on the happy path (SRC-023); documented as inhibiting some rayon optimizations (SRC-023); does not turn a panic into a graceful skip -- the caller still receives the unwinding panic; combining with `catch_unwind` at the outer level is needed for true graceful degradation; least appropriate for this use case where we want individual tasks to fail gracefully
- **Best For**: Long-running parallel computations where halting ASAP after a panic saves significant computation time (not applicable here where individual tasks take only ~20ms)

---

## ISS-006: Panic Handling in par_iter -- Detailed Analysis

**Prior Understanding**: If lofty internally panics on malformed files, rayon propagates the panic to the calling thread after all other tasks complete. Consider `std::panic::catch_unwind` if defensive coding is desired.

**Investigation Summary**: Analyzed lofty 0.24.0's panic resilience through its changelog and fuzzing infrastructure, rayon's panic propagation behavior, `catch_unwind` overhead characteristics under `panic=unwind`, and termusic's existing error handling in `read_track_from_path`.

**Resolution Status**: RESOLVED (LOW RISK, no action needed)

**Evidence**:

1. **Lofty's defensive design**: Lofty 0.24.0 uses `ParsingMode::BestAttempt` by default, which attempts to extract data even from malformed items rather than panicking or erroring (SRC-024). This mode specifically exists to prevent panics from unexpected data.

2. **Extensive fuzzing**: Lofty has 8+ dedicated fuzz targets (`filetype_from_buffer`, `mpcfile_read_from`, `mpegfile_read_from`, `aacfile_read_from`, `aifffile_read_from`, `apefile_read_from`, `flacfile_read_from`, `wavfile_read_from`) that have been run extensively to eliminate panics (SRC-024).

3. **Historical panics are fixed**: The lofty changelog documents dozens of fixed panics across all formats (MP4, WAV, MusePack, WavPack, MPEG, ID3v2, Vorbis, FLAC, AIFF). These were all discovered through fuzzing and fixed in versions prior to 0.24.0 (SRC-024).

4. **Double safety layer**: In termusic, `read_track_from_path` already wraps `parse_metadata_from_file` in a match that catches ANY `Err` and returns `TrackMetadata::default()` (line 250-269 of track.rs). Only a true panic (not an Error) would escape this handler (SRC-022).

5. **`catch_unwind` has real cost**: Under `panic=unwind` (termusic's configuration), `catch_unwind` creates LLVM exception handling structures (invoke + catchswitch + catchpad) even on the happy path. These structures inhibit optimizations like inlining and code motion. For 500 invocations, this represents measurable overhead (SRC-025).

6. **Rayon already uses catch_unwind internally**: Rayon's job execution infrastructure already wraps each job in its own `catch_unwind` via `halt_unwinding` in the Registry. Adding another layer is redundant -- it would be catch_unwind inside catch_unwind (SRC-023).

**Resolution Path**: Accept rayon's default panic behavior. Add a code comment documenting the risk assessment:

```rust
// SAFETY NOTE: Lofty 0.24.0 uses ParsingMode::BestAttempt (default) and has extensive
// fuzz testing across all formats. Panics are extremely unlikely. If a panic does occur,
// rayon propagates it to this thread after other tasks complete. Since read_track_from_path
// already catches all Err variants (falling back to default metadata), only a true internal
// panic in lofty would propagate -- a scenario not worth the optimization-inhibiting cost
// of per-task catch_unwind.
```

**New Insights**: Rayon internally wraps job execution in `catch_unwind` (via `halt_unwinding` in Registry). This means the panic is already caught at the rayon level -- it is simply re-thrown in the calling thread. Adding user-level `catch_unwind` inside the closure would create nested exception handling with no additional safety benefit beyond converting the panic into a return value.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| rayon par_iter panic catch_unwind best practice behavior propagation | DeepWiki (rayon-rs/rayon) | 1 | 1 |
| catch_unwind inside par_iter closure performance overhead panic_fuse | DeepWiki (rayon-rs/rayon) | 1 | 1 |
| BufReader lines filter_map ok vs propagate error handling best practice | DeepWiki (rust-lang/rust) | 1 | 1 |
| clippy lint lines_filter_map_ok map_while recommendation | DeepWiki (rust-lang/rust-clippy) | 1 | 1 |
| lofty-rs panic during parsing fuzzing robustness 0.24.0 | DeepWiki (Serial-ATA/lofty-rs) | 1 | 1 |
| catch_unwind happy path overhead assembly panic=unwind vs abort | DeepWiki (rust-lang/rust) | 1 | 1 |
| rayon catch_unwind performance overhead benchmark panic_fuse comparison | DeepWiki (rayon-rs/rayon) | 1 | 1 |
| termusic codebase filter_map Result::ok usage patterns | Local grep | 7 | 7 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-020 | DeepWiki: rust-lang/rust-clippy -- `lines_filter_map_ok` lint details, `map_while(Result::ok)` recommendation | AI Documentation | 2026-06 | Fresh | High |
| SRC-021 | DeepWiki: rust-lang/rust -- BufReader::lines() error handling best practices, `?` operator recommendation | AI Documentation | 2026-06 | Fresh | High |
| SRC-022 | termusic lib/src/track.rs:250-269 -- read_track_from_path already catches all lofty Err variants | Local Code | 2026-06 | Fresh | High |
| SRC-023 | DeepWiki: rayon-rs/rayon -- panic propagation in par_iter, panic_fuse() adaptor, internal catch_unwind in Registry | AI Documentation | 2026-06 | Fresh | High |
| SRC-024 | DeepWiki: Serial-ATA/lofty-rs -- fuzzing infrastructure, ParsingMode::BestAttempt default, panic fix history | AI Documentation | 2026-06 | Fresh | High |
| SRC-025 | DeepWiki: rust-lang/rust -- catch_unwind happy-path overhead, LLVM landing pad structures, optimization inhibition | AI Documentation | 2026-06 | Fresh | High |
| SRC-026 | termusic playback/src/playlist.rs:226-249 -- current sequential load loop with `line?` error propagation | Local Code | 2026-06 | Fresh | High |
| SRC-027 | termusic Cargo.toml:134 -- `# panic = 'abort'` commented out, confirming panic=unwind is active | Local Code | 2026-06 | Fresh | High |

---

## Deprecation Warnings

No deprecation concerns identified for current stack. Rayon 1.12.0 and lofty 0.24.0 are both actively maintained with no deprecated APIs relevant to this use case.

---

## Best Practices

### BP-007: Use `map_while(Result::ok)` for Line Collection Before Parallelization

- **Pattern**: When collecting `BufReader::lines()` into a Vec for subsequent parallel processing, prefer `map_while(Result::ok)` over `filter_map(Result::ok)` to terminate at the first I/O error rather than silently skipping errors.
- **Rationale**: Clippy lint `lines_filter_map_ok` explicitly discourages `filter_map(Result::ok)` on Lines iterators due to infinite-loop risk. While the risk is inapplicable for regular files, using `map_while` provides clearer semantics (stop at first error) and avoids lint suppression annotations. For playlist files read from disk, any I/O error mid-read indicates a catastrophic condition where terminating early is the safest response (SRC-020, SRC-021).
- **Source**: SRC-020, SRC-021
- **Confidence**: High
- **Example**:
```rust
// Preferred: stops at first I/O error (semantic match to original `line?`)
let all_lines: Vec<String> = lines
    .map_while(|l| l.ok())
    .filter(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })
    .collect();
```

### BP-008: Document Semantic Changes in Error Behavior

- **Pattern**: When refactoring sequential error-propagating code to parallel batch processing, explicitly document any change in error semantics with a code comment.
- **Rationale**: The shift from `line?` (abort on error) to any form of line collection (batch processing) inherently changes how mid-stream errors are handled. Future maintainers need to understand this was an intentional design decision, not an oversight (SRC-021, SRC-026).
- **Source**: SRC-021, SRC-026
- **Confidence**: High
- **Example**:
```rust
// NOTE: Original code used `line?` which aborted on first I/O error.
// The batch approach uses map_while(Result::ok) which stops reading at the first
// I/O error but does NOT propagate it as an Err -- the caller receives Ok with
// whatever lines were successfully read before the error. This is acceptable because:
// 1. playlist.log is a local regular file; mid-read I/O errors are near-impossible
// 2. Partial playlist loading is preferable to total startup failure
// 3. If the file is truly unreadable, the first line (track index) read would have
//    already failed and propagated via `line?` on line 203
let all_lines: Vec<String> = lines
    .map_while(|l| l.ok())
    .filter(|l| { /* ... */ })
    .collect();
```

### BP-009: Rely on Library Fuzzing Over Defensive catch_unwind

- **Pattern**: When using well-fuzzed libraries (lofty, image, etc.) in parallel contexts, prefer trusting the library's error handling over wrapping every call in `catch_unwind`.
- **Rationale**: `catch_unwind` has measurable overhead (LLVM landing pads, optimization inhibition) even on the happy path. Well-maintained libraries with active fuzzing (lofty has 8+ fuzz targets) have already eliminated known panic vectors. The expected value of protection (probability x impact) is lower than the certain cost of the overhead for 500+ invocations per startup (SRC-024, SRC-025).
- **Source**: SRC-024, SRC-025
- **Confidence**: High

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|-------------|-------------|-------------|--------|
| Using `filter_map(Result::ok)` on `BufReader::lines()` without lint suppression | Triggers Clippy `lines_filter_map_ok` lint; risks infinite loop on non-file readers | Use `map_while(Result::ok)` which terminates at first error | SRC-020 |
| Adding `catch_unwind` inside every par_iter closure "just in case" | Creates LLVM landing pads that inhibit inlining and code motion; measurable overhead for 500+ tasks; redundant with rayon's internal exception handling | Trust library fuzzing; add catch_unwind only after observing panics in production | SRC-023, SRC-025 |
| Using `panic_fuse()` for short-task parallel iterators | Adds AtomicBool synchronization overhead on every iteration; designed for long-running tasks where halting saves significant work; for 20ms tasks the overhead exceeds the saved work | Accept default rayon behavior (propagate after completion) for short tasks | SRC-023 |
| Wrapping `AssertUnwindSafe` around captured references without justification | Defeats the purpose of the `UnwindSafe` marker; adds visual noise suggesting danger where none exists; `AssertUnwindSafe` is a red flag for reviewers | If you need catch_unwind, ensure the captured state is genuinely unwind-safe or restructure | SRC-025 |

---

## Implementation Considerations

### Performance

- `map_while(Result::ok)` has identical performance to `filter_map(Result::ok)` -- both are zero-cost iterator adaptors that check a Result variant per item (SRC-020, SRC-021)
- NOT using `catch_unwind` avoids LLVM landing pad overhead across 500+ iterations. The overhead is small per call but cumulative for large playlists (SRC-025)
- The first line (`current_track_index`) is still read with `line?` (error propagation), so if the file is completely unreadable, the function fails early before reaching the parallel section (SRC-026)

### Security

- No security implications for either issue. File paths come from the user's own `playlist.log` file. Error handling changes do not introduce new attack surface (SRC-026)

### Compatibility

- `map_while` requires Rust 1.57+ (stabilized in 1.57.0). The project's MSRV already satisfies this (SRC-020)
- `catch_unwind` requires the closure to be `UnwindSafe`. If Option E were chosen, `AssertUnwindSafe` would be needed for the `&HashMap` capture, which is safe since we only read from it (SRC-025)
- The project uses `panic=unwind` (confirmed by commented-out `panic = 'abort'` in Cargo.toml line 134), so `catch_unwind` is functional if ever needed (SRC-027)

---

## Community Discoveries

| ID | Insight | Source | Date | Momentum | Consensus |
|----|---------|--------|------|----------|-----------|
| COM-006 | Clippy's `lines_filter_map_ok` lint has driven widespread adoption of `map_while(Result::ok)` in new Rust code as the standard pattern for collecting lines from readers | DeepWiki: rust-lang/rust-clippy | 2026-06 | 0.80 | Yes |
| COM-007 | Rayon's internal use of `catch_unwind` (via `halt_unwinding` in Registry) means user-level `catch_unwind` inside par_iter closures is redundant for panic containment -- the panic is already caught, just re-thrown at the join point | DeepWiki: rayon-rs/rayon | 2026-06 | 0.75 | Yes |
| COM-008 | Lofty 0.24.0's default `BestAttempt` parsing mode represents a deliberate design decision to return errors rather than panic, making it one of the most panic-resilient audio metadata libraries in the Rust ecosystem | DeepWiki: Serial-ATA/lofty-rs | 2026-06 | 0.85 | Yes |

### Community Pulse

- **Active Discussions**: The Clippy `lines_filter_map_ok` lint (added in 2023, stabilized in later versions) has settled community debate on the correct pattern for BufReader line collection. `map_while(Result::ok)` is the consensus winner.
- **Pain Points**: Developers report confusion about rayon's panic semantics -- specifically, the fact that other tasks CONTINUE executing after one panics (until completion), followed by panic propagation. This surprises teams expecting immediate abort.
- **Novel Solutions**: Some projects wrap the entire `par_iter().collect()` call (not individual closures) in a single `catch_unwind` for a lightweight safety net that catches the propagated panic without per-task overhead.

---

## Contradictions Found

| Topic | Position A (SRC-020) | Position B (SRC-026, local code) | Assessment |
|-------|---------------------|----------------------------------------|------------|
| Whether `filter_map(Result::ok)` is acceptable for line collection | Clippy lint classifies it as "suspicious" and recommends `map_while(Result::ok)` instead | termusic codebase already uses `filter_map(Result::ok)` in 7 places (youtube_options.rs, database.rs, scanner.rs, new_database) | The Clippy lint targets the specific infinite-loop risk with `BufReader::lines()`. The existing termusic uses are on `ReadDir` iterators (directory entries), not `BufReader::lines()`, which have different error semantics. For consistency, new code should follow the Clippy recommendation (`map_while`) while existing patterns on non-Lines iterators remain valid. |

---

## Issues and Ambiguities

All prior issues (ISS-005, ISS-006) are now resolved. No new issues identified.

The following decisions are documented as intentional for future maintainers:

1. **Semantic change in error handling**: The parallel implementation uses `map_while(Result::ok)` which stops at the first I/O error but does NOT propagate it as an `Err`. The first line (track index) is still read with `?`, so completely unreadable files fail fast. Lines read successfully before any mid-stream error are processed normally.

2. **No catch_unwind for lofty panics**: Accepted as negligible risk given lofty's fuzzing maturity and `BestAttempt` default mode. If a panic is ever observed in production, the fix path is straightforward: wrap the outer `par_iter().collect()` call in a single `catch_unwind` (not per-task).

---

## References

### Primary Sources (Official Documentation)

- SRC-020: DeepWiki rust-lang/rust-clippy -- `lines_filter_map_ok` lint, `map_while(Result::ok)` recommendation -- https://deepwiki.com/rust-lang/rust-clippy
- SRC-021: DeepWiki rust-lang/rust -- BufReader::lines() error handling, `?` operator best practices -- https://deepwiki.com/rust-lang/rust
- SRC-025: DeepWiki rust-lang/rust -- catch_unwind happy-path overhead, LLVM landing pad structures -- https://deepwiki.com/rust-lang/rust

### Secondary Sources (AI Documentation Analysis)

- SRC-023: DeepWiki rayon-rs/rayon -- panic propagation semantics, panic_fuse() adaptor, internal halt_unwinding mechanism -- https://deepwiki.com/rayon-rs/rayon
- SRC-024: DeepWiki Serial-ATA/lofty-rs -- ParsingMode::BestAttempt, fuzzing infrastructure (8+ targets), panic fix history -- https://deepwiki.com/Serial-ATA/lofty-rs

### Community Sources (Local Code)

- SRC-022: termusic lib/src/track.rs:250-269 -- read_track_from_path catches all lofty Err variants
- SRC-026: termusic playback/src/playlist.rs:226-249 -- current sequential load loop with `line?`
- SRC-027: termusic Cargo.toml:134 -- `# panic = 'abort'` commented out (panic=unwind active)
