# Implementation Plan: Optimize Playlist Loading Performance

- **Date**: 2026-06-26
- **Author**: super-dev:spec-writer
- **Specification**: ./08-specification.md
- **Total Phases**: 4
- **Estimated Effort**: 1 day

---

## Phase Summary

- Phase 1: Dependency Setup — Domain: infrastructure, Effort: small, Depends on: None, Parallelizable with: None
- Phase 2: Core Parallelization — Domain: backend, Effort: medium, Depends on: Phase 1, Parallelizable with: Phase 3 (partially)
- Phase 3: Integration Testing — Domain: testing, Effort: medium, Depends on: Phase 2, Parallelizable with: None
- Phase 4: Performance Validation and Documentation — Domain: testing, Effort: small, Depends on: Phase 3, Parallelizable with: None

---

## Phase 1: Dependency Setup

- **Domain**: infrastructure
- **Effort**: small
- **Objective**: Add rayon as a direct workspace dependency and verify the project builds cleanly
- **Depends On**: None
- **Parallelizable With**: None

### Scope

Add rayon 1.12 to the workspace dependencies in root `Cargo.toml` and reference it from `playback/Cargo.toml`. Verify that `cargo build` succeeds and no new warnings are introduced. This phase produces no behavioral changes — only dependency declaration.

Out of scope: any code changes to `playlist.rs` or other source files.

### Tasks

1. Add rayon to workspace dependencies in root Cargo.toml
   - Files: Cargo.toml
   - Type: modify
2. Reference rayon workspace dependency in playback crate manifest
   - Files: playback/Cargo.toml
   - Type: modify
3. Add `use rayon::prelude::*` import to playlist.rs (preparation for Phase 2)
   - Files: playback/src/playlist.rs
   - Type: modify
4. Verify cargo build succeeds with no new warnings
   - Files: (none — verification step)
   - Type: verify

### Acceptance Criteria

- `rayon = "1.12"` appears in `[workspace.dependencies]` section of root Cargo.toml
- `rayon.workspace = true` appears in `[dependencies]` section of playback/Cargo.toml
- `cargo build --workspace` completes without errors
- `cargo clippy --workspace` produces no new warnings
- Binary size does not increase (rayon was already a transitive dependency)
- Addresses: AC-07, SCENARIO-015

### Risks

- Workspace dependency resolution conflict if rayon version differs from transitive version (mitigated: use same version 1.12 already in lock file)

---

## Phase 2: Core Parallelization

- **Domain**: backend
- **Effort**: medium
- **Objective**: Replace the sequential for-loop in Playlist::load() with the two-phase classify-then-parallel-process architecture
- **Depends On**: Phase 1
- **Parallelizable With**: None

### Scope

Modify `Playlist::load()` function body at `playback/src/playlist.rs:226-250` to implement:
1. Batch line collection with `map_while(Result::ok)`
2. Line classification (network address vs local path)
3. Parallel metadata reads via `par_iter` for local paths
4. Sequential resolution of podcast/radio entries
5. Order-preserving merge of results
6. Elapsed time logging

The function signature, return type, and observable behavior (same output for same input) remain identical. Error handling semantics are preserved: failed tracks are skipped, debug logs emitted.

Out of scope: changes to Track, changes to any other function, new public APIs.

### Tasks

1. Replace sequential line iteration with batch collection using map_while(Result::ok)
   - Files: playback/src/playlist.rs
   - Type: modify
2. Implement line classification into network addresses and local paths with original indices
   - Files: playback/src/playlist.rs
   - Type: modify
3. Implement parallel metadata read using par_iter over local path entries
   - Files: playback/src/playlist.rs
   - Type: modify
4. Implement sequential podcast/radio resolution for network address entries
   - Files: playback/src/playlist.rs
   - Type: modify
5. Implement order-preserving merge of parallel and sequential results
   - Files: playback/src/playlist.rs
   - Type: modify
6. Add elapsed time info! logging around the parallel processing section
   - Files: playback/src/playlist.rs
   - Type: modify
7. Add documentation comments explaining the two-phase architecture and error semantics
   - Files: playback/src/playlist.rs
   - Type: modify
8. Verify cargo clippy passes with no warnings on modified code
   - Files: (none — verification step)
   - Type: verify

### Acceptance Criteria

- `Playlist::load()` function signature is unchanged: `pub fn load() -> Result<(usize, Vec<Track>)>`
- Local file metadata reads execute in parallel via rayon par_iter
- Podcast URL and radio entries are resolved sequentially (not parallelized)
- Output track order matches input file line order for all entry types
- Failed metadata reads are excluded without crashing or logging above debug level
- Info-level log message emits track count and elapsed time after load completes
- `cargo test --workspace` passes all existing tests
- Addresses: AC-01, AC-02, AC-03, AC-04, AC-05, AC-06, AC-08
- Addresses: SCENARIO-001 through SCENARIO-014, SCENARIO-017, SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-021

### Risks

- Borrow checker rejection if `episode_by_url` HashMap lifetime does not satisfy par_iter closure bounds (mitigated: HashMap is Sync, shared reference is valid in rayon closures)
- Subtle ordering bug if indices are assigned incorrectly (mitigated: enumerate on sequential collection guarantees monotonic indices)
- Existing tests may rely on side effects of sequential ordering during load (mitigated: load is a pure data function with no observable side effects beyond the return value)

---

## Phase 3: Integration Testing

- **Domain**: testing
- **Effort**: medium
- **Objective**: Add targeted integration tests verifying order preservation, error handling, and mixed-entry scenarios
- **Depends On**: Phase 2
- **Parallelizable With**: None

### Scope

Create integration tests that exercise the parallel load path with controlled inputs. Tests cover:
- Order preservation with mixed entry types (local + podcast + radio)
- Graceful handling of invalid/missing file paths
- Empty playlist edge case
- Single track edge case
- All-failing tracks edge case

Out of scope: performance benchmarks (Phase 4), modification of existing tests.

### Tasks

1. Create test fixture playlist files with controlled content (mixed entries, invalid paths, empty, single track)
   - Files: playback/tests/fixtures/ (multiple fixture files)
   - Type: create
2. Add integration test: order preservation with interleaved local and network entries
   - Files: playback/tests/playlist_parallel_load_tests.rs
   - Type: create
3. Add integration test: graceful skip of invalid file paths during parallel load
   - Files: playback/tests/playlist_parallel_load_tests.rs
   - Type: modify
4. Add integration test: empty playlist loads without error
   - Files: playback/tests/playlist_parallel_load_tests.rs
   - Type: modify
5. Add integration test: single track playlist loads correctly
   - Files: playback/tests/playlist_parallel_load_tests.rs
   - Type: modify
6. Add integration test: all tracks failing produces empty playlist without crash
   - Files: playback/tests/playlist_parallel_load_tests.rs
   - Type: modify
7. Run full test suite (cargo test --workspace) and verify all 385+ tests pass
   - Files: (none — verification step)
   - Type: verify

### Acceptance Criteria

- All new integration tests pass on the parallelized implementation
- All 385 existing tests pass without modification (AC-04, SCENARIO-009)
- Test coverage includes SCENARIO-004, SCENARIO-006, SCENARIO-010, SCENARIO-011, SCENARIO-017, SCENARIO-018, SCENARIO-020, SCENARIO-021
- No flaky tests introduced (parallel execution is deterministic for ordering)

### Risks

- Test environment may not have valid audio files for metadata parsing (mitigated: use tiny valid MP3/FLAC fixtures or mock the file system path)
- Config path dependency in `Playlist::load()` may make isolated testing difficult (mitigated: use environment variable or test helper to set config directory)

---

## Phase 4: Performance Validation and Documentation

- **Domain**: testing
- **Effort**: small
- **Objective**: Verify performance improvement meets acceptance criteria and document the optimization
- **Depends On**: Phase 3
- **Parallelizable With**: None

### Scope

Run performance validation to confirm the parallelization achieves the expected speedup. Document the optimization in code comments and update any relevant project documentation.

Out of scope: implementing a metadata cache (future enhancement), benchmarks in CI.

### Tasks

1. Create a performance benchmark script or test with 200+ audio files measuring sequential vs parallel load time
   - Files: playback/benches/playlist_load_bench.rs
   - Type: create
2. Verify speedup exceeds 3x on a 4+ core machine (AC-01)
   - Files: (none — verification step)
   - Type: verify
3. Verify memory usage increase is bounded (no RSS spike beyond thread pool overhead)
   - Files: (none — verification step)
   - Type: verify
4. Run cargo clippy --workspace and cargo fmt --check to ensure code quality
   - Files: (none — verification step)
   - Type: verify

### Acceptance Criteria

- Measured speedup is at least 3x for 200+ tracks on a 4-core machine (AC-01, SCENARIO-001)
- Peak memory increase during load is bounded to approximately 8MB (thread pool stacks) (AC-08, SCENARIO-016)
- Small playlist (< 50 tracks) shows no measurable regression (SCENARIO-003)
- All clippy warnings resolved, code formatted correctly
- Addresses: SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-016, SCENARIO-019

### Risks

- Benchmark results are system-dependent; CI machines may have different core counts (mitigated: express criterion as "proportional to cores" rather than absolute time)
- Disk I/O variability may affect benchmark consistency (mitigated: use warm cache, multiple runs, report median)

---

## Cross-Cutting Concerns

### Thread Safety

The `Track` type is Send (verified: all fields are PathBuf/String/Option<Duration>/Option<String>, no Rc/Cell/raw pointers). The `episode_by_url` HashMap is Sync (shared immutable reference in par_iter closure). No new shared mutable state is introduced.

### Backward Compatibility

All public API signatures remain unchanged (AC-03). The optimization is invisible to callers — same inputs produce same outputs. The only observable difference is performance improvement and a new info-level log message.

### Error Semantics Change

The shift from `line?` (abort with Err) to `map_while(Result::ok)` (stop reading, return Ok with partial data) is a documented intentional change. The first line (track index) is still read with `?`, ensuring completely unreadable files fail fast. This change affects only the catastrophic mid-file I/O error case, which is near-impossible for a local regular file.

## Milestone Summary

- **M1: Build Ready**: Phase 1 — Deliverable: rayon dependency declared, project builds, Verification: `cargo build --workspace` succeeds
- **M2: Parallel Load Functional**: Phase 2 — Deliverable: Playlist::load() uses par_iter for local paths, Verification: `cargo test --workspace` all pass, info log shows parallel timing
- **M3: Tested and Verified**: Phase 3 — Deliverable: integration tests cover all critical scenarios, Verification: all new and existing tests pass
- **M4: Performance Validated**: Phase 4 — Deliverable: benchmark confirms 3x+ speedup, Verification: criterion benchmark output shows improvement
