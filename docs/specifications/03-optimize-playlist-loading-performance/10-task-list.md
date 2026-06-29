# Task List: Optimize Playlist Loading Performance

- **Date**: 2026-06-26
- **Author**: super-dev:spec-writer
- **Specification**: ./08-specification.md
- **Implementation Plan**: ./09-implementation-plan.md
- **Total Tasks**: 18

---

## Phase 1: Dependency Setup

**Milestone**: rayon dependency declared, project builds without errors

- [ ] **T-01**: Add rayon 1.12 to workspace dependencies in root Cargo.toml under `[workspace.dependencies]` section
  - Files: Cargo.toml
  - Type: modify
  - Effort: small
  - Depends on: None

- [ ] **T-02**: Add `rayon.workspace = true` to `[dependencies]` section of playback/Cargo.toml
  - Files: playback/Cargo.toml
  - Type: modify
  - Effort: small
  - Depends on: T-01

- [ ] **T-03**: Add `use rayon::prelude::*;` import to playback/src/playlist.rs alongside existing imports
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-02

- [ ] **T-04**: Verify `cargo build --workspace` succeeds and `cargo clippy --workspace` produces no new warnings
  - Files: (verification only)
  - Type: verify
  - Effort: small
  - Depends on: T-03

---

## Phase 2: Core Parallelization

**Milestone**: Playlist::load() processes local file metadata in parallel with order preservation

- [ ] **T-05**: Replace sequential `for line in lines` iteration (lines 226-250) with batch collection using `lines.map_while(|l| l.ok()).filter(non_empty_non_comment).enumerate().collect::<Vec<(usize, String)>>()`; add documentation comment explaining semantic change from `line?`
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-04

- [ ] **T-06**: Implement line classification using `partition()` to separate network addresses (lines starting with `http://` or `https://`) from local file paths, preserving original indices
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-05

- [ ] **T-07**: Implement parallel metadata read using `local_entries.par_iter().filter_map(|(index, path)| Track::read_track_from_path(path).ok().map(|track| (*index, track))).collect::<Vec<(usize, Track)>>()`; add safety note comment about lofty panic risk assessment
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-06

- [ ] **T-08**: Implement sequential network address resolution: iterate network entries, look up in `episode_by_url` HashMap for podcast episodes, fall back to `Track::new_radio()` for unmatched URLs, collect as `Vec<(usize, Track)>`
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-06

- [ ] **T-09**: Implement order-preserving merge: combine local_tracks and network_tracks into single Vec, sort_unstable_by_key on original_index, map to extract Track values into final `Vec<Track>`
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-07, T-08

- [ ] **T-10**: Add elapsed time logging: capture `Instant::now()` before parallel section, emit `info!("Loaded {} tracks ({} local, {} network) in {:?}", ...)` after merge completes
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09

- [ ] **T-11**: Run `cargo test --workspace` to verify all existing 385 tests pass without modification
  - Files: (verification only)
  - Type: verify
  - Effort: small
  - Depends on: T-10

---

## Phase 3: Integration Testing

**Milestone**: Integration tests cover order preservation, error handling, and edge cases

- [ ] **T-12**: Create test fixture files: a valid multi-track playlist with interleaved local paths and HTTP URLs, a playlist with invalid/nonexistent file paths, an empty playlist, a single-track playlist, and an all-invalid playlist
  - Files: playback/tests/fixtures/playlist_mixed.log, playback/tests/fixtures/playlist_invalid_paths.log, playback/tests/fixtures/playlist_empty.log, playback/tests/fixtures/playlist_single.log, playback/tests/fixtures/playlist_all_invalid.log
  - Type: create
  - Effort: small
  - Depends on: T-11

- [ ] **T-13**: Add integration test `test_parallel_load_preserves_order_with_mixed_entries`: load the mixed playlist fixture, assert track order matches file order for all entry types (covers SCENARIO-004, SCENARIO-005, SCENARIO-021)
  - Files: playback/tests/playlist_parallel_load_tests.rs
  - Type: create
  - Effort: medium
  - Depends on: T-12

- [ ] **T-14**: Add integration test `test_parallel_load_skips_invalid_paths_gracefully`: load the invalid-paths playlist fixture, assert valid tracks are present and invalid are excluded without panic (covers SCENARIO-010, SCENARIO-011)
  - Files: playback/tests/playlist_parallel_load_tests.rs
  - Type: modify
  - Effort: small
  - Depends on: T-13

- [ ] **T-15**: Add integration tests for edge cases: `test_parallel_load_empty_playlist` (SCENARIO-017), `test_parallel_load_single_track` (SCENARIO-018), `test_parallel_load_all_tracks_fail` (SCENARIO-020)
  - Files: playback/tests/playlist_parallel_load_tests.rs
  - Type: modify
  - Effort: small
  - Depends on: T-13

- [ ] **T-16**: Run full test suite `cargo test --workspace` and verify all tests (existing + new) pass
  - Files: (verification only)
  - Type: verify
  - Effort: small
  - Depends on: T-14, T-15

---

## Phase 4: Performance Validation and Documentation

**Milestone**: Performance improvement confirmed, benchmark available for future regression detection

- [ ] **T-17**: Create criterion benchmark `playlist_load_bench` that measures Playlist::load() wall-clock time with a fixture of 200+ valid audio files; compare against a sequential baseline to verify minimum 3x speedup on 4+ cores (covers SCENARIO-001, SCENARIO-002, SCENARIO-003)
  - Files: playback/benches/playlist_load_bench.rs
  - Type: create
  - Effort: medium
  - Depends on: T-16

- [ ] **T-18**: Run final verification: `cargo clippy --workspace` (no warnings), `cargo fmt --check` (formatted), `cargo test --workspace` (all pass), benchmark shows expected speedup
  - Files: (verification only)
  - Type: verify
  - Effort: small
  - Depends on: T-17

---

## Summary

- Phase 1: Dependency Setup — 4 tasks, small effort
- Phase 2: Core Parallelization — 7 tasks, medium effort
- Phase 3: Integration Testing — 5 tasks, medium effort
- Phase 4: Performance Validation and Documentation — 2 tasks, small effort
- **Total**: 18 tasks, estimated 1 day effort
