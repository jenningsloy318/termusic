# Implementation Plan: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Updated**: 2026-06-23
- **Author**: super-dev:spec-writer
- **Specification**: ./09-specification.md
- **Total Phases**: 5
- **Status**: ALL PHASES COMPLETE
- **Actual Effort**: ~1 day (5 sequential commits)

---

## Phase Summary

| # | Name | Domain | Effort | Depends On | Status |
|---|------|--------|--------|------------|--------|
| 1 | Configuration Schema | backend | small | None | COMPLETE |
| 2 | PlaylistAddTrack API Extension | backend | small | None | COMPLETE |
| 3 | Sync Pass Logic | backend | large | Phase 1 and Phase 2 | COMPLETE |
| 4 | Task Lifecycle and Wiring | backend | medium | Phase 3 | COMPLETE |
| 5 | Integration Tests and Verification | testing | medium | Phase 4 | COMPLETE |

---

## Phase 1: Configuration Schema

- **Domain**: backend
- **Effort**: small
- **Status**: COMPLETE
- **Objective**: Add the `SynchronizationSettings` config struct with `#[serde(default)]` support, the `humantime-serde` dependency, and unit tests for config parsing and roundtrip serialization.
- **Depends On**: None
- **Commit**: `5656c4ce feat(configuration-schema)`
- **Tests**: 19 unit tests passing

### Scope

In scope:
- Add `humantime-serde = "0.2"` to workspace and lib crate dependencies
- Create `lib/src/config/v2/server/synchronization.rs` with the `SynchronizationSettings` struct
- Add `pub mod synchronization;` and the `synchronization` field to `ServerSettings` in `lib/src/config/v2/server/mod.rs`
- Unit tests for default deserialization, explicit value deserialization, roundtrip serialization, and invalid duration rejection

Out of scope:
- Sync task logic (Phase 3)
- Server wiring (Phase 4)

### Tasks

1. Add `humantime-serde` workspace dependency
   - Files: Cargo.toml (workspace root)
   - Type: modify
2. Add `humantime-serde` to lib crate dependencies
   - Files: lib/Cargo.toml
   - Type: modify
3. Create `SynchronizationSettings` struct with `#[serde(default)]` and `impl Default`
   - Files: lib/src/config/v2/server/synchronization.rs
   - Type: create
4. Register module and add field to `ServerSettings`
   - Files: lib/src/config/v2/server/mod.rs
   - Type: modify
5. Write unit tests for config parsing (default, explicit, roundtrip, invalid)
   - Files: lib/src/config/v2/server/synchronization.rs
   - Type: modify (add `#[cfg(test)] mod tests` section)

### Acceptance Criteria

- `cargo build --all` compiles without error after this phase
- `cargo test --all` passes, including new config tests
- A TOML config file without `[synchronization]` section parses successfully with default values (AC-01, AC-10, SCENARIO-001)
- A TOML config file with explicit `[synchronization]` values parses correctly (SCENARIO-002)
- Serialization followed by deserialization produces identical config (SCENARIO-003)
- An invalid `interval` string causes a deserialization error (SCENARIO-004)

### Risks

- `humantime-serde` API compatibility with `serde(with)` attribute pattern -- mitigated by well-documented usage in the crate's README and widespread adoption

---

## Phase 2: PlaylistAddTrack API Extension

- **Domain**: backend
- **Effort**: small
- **Status**: COMPLETE
- **Objective**: Add the `AT_END` constant and `new_append_single`/`new_append_vec` constructors to `PlaylistAddTrack`, providing a clean API for appending tracks without exposing the `u64::MAX` sentinel.
- **Depends On**: None
- **Commit**: `2012caab feat(playlistaddtrack-api-extension)`
- **Tests**: 20 unit tests passing

### Scope

In scope:
- Add `pub const AT_END: u64 = u64::MAX` to `PlaylistAddTrack`
- Add `pub fn new_append_single(track: PlaylistTrackSource) -> Self` method
- Add `pub fn new_append_vec(tracks: Vec<PlaylistTrackSource>) -> Self` method
- Unit tests for the new constructors

Out of scope:
- Refactoring existing call sites to use the new constructors (existing code continues to work)
- Modifying the protobuf layer

### Tasks

1. Add `AT_END` constant and `new_append_single`/`new_append_vec` constructors
   - Files: lib/src/player.rs
   - Type: modify
2. Write unit tests for the new constructors
   - Files: lib/src/player.rs
   - Type: modify (add tests in existing or new `#[cfg(test)]` section)

### Acceptance Criteria

- `cargo build --all` compiles without error
- `cargo test --all` passes, including new constructor tests
- `PlaylistAddTrack::new_append_single(source).at_index == u64::MAX`
- `PlaylistAddTrack::new_append_vec(sources).at_index == u64::MAX`
- Existing `new_single` and `new_vec` methods remain unchanged and functional

### Risks

- None significant. This is a purely additive change to an existing struct.

---

## Phase 3: Sync Pass Logic

- **Domain**: backend
- **Effort**: large
- **Status**: COMPLETE
- **Objective**: Implement the `sync_once` function that executes a single synchronization pass: open database, read podcasts, fetch feeds, deduplicate, download new episodes, and enqueue them via `PlaylistAddTrack`.
- **Depends On**: Phase 1 (for `SynchronizationSettings` to read config), Phase 2 (for `PlaylistAddTrack::new_append_single`)
- **Commit**: `ceab28eb feat(sync-pass-logic)`
- **Tests**: 20 unit tests passing
- **Note**: T-22 (module registration) was pulled forward into this phase for compilation.

### Scope

In scope:
- Create `server/src/podcast_sync.rs` with the `sync_once` async function
- Define `SyncPassStats` struct for logging
- Implement per-podcast error isolation (fetch, parse, update)
- Implement download completion signaling via channel-drain pattern
- Implement per-episode error isolation during download
- Implement enqueue logic using `PlaylistAddTrack::new_append_single`
- Handle the "no subscribed podcasts" edge case

Out of scope:
- Task lifecycle (start/stop/cancellation) -- Phase 4
- Server wiring -- Phase 4
- Full integration tests with mock HTTP servers -- Phase 5

### Tasks

1. Create `server/src/podcast_sync.rs` with module structure and imports
   - Files: server/src/podcast_sync.rs
   - Type: create
2. Define `SyncPassStats` struct
   - Files: server/src/podcast_sync.rs
   - Type: modify
3. Implement `sync_once` function skeleton with Database open and get_podcasts
   - Files: server/src/podcast_sync.rs
   - Type: modify
4. Implement per-podcast feed fetch loop with error isolation
   - Files: server/src/podcast_sync.rs
   - Type: modify
5. Implement episode deduplication via database update and path-existence filtering
   - Files: server/src/podcast_sync.rs
   - Type: modify
6. Implement download_list invocation with channel-drain pattern
   - Files: server/src/podcast_sync.rs
   - Type: modify
7. Implement enqueue logic (db.insert_file + PlaylistAddTrack send)
   - Files: server/src/podcast_sync.rs
   - Type: modify
8. Add basic unit tests for sync_once with in-memory database
   - Files: server/src/podcast_sync.rs
   - Type: modify

### Acceptance Criteria

- `cargo build --all` compiles without error
- `sync_once` returns `Ok(SyncPassStats)` when given a database with no podcasts (SCENARIO-021)
- `sync_once` correctly identifies new episodes by GUID absence (AC-05, SCENARIO-010)
- `sync_once` skips episodes with existing GUIDs (SCENARIO-011)
- `sync_once` uses enclosure URL fallback when GUID is absent (SCENARIO-012)
- Per-podcast errors are logged at warn level and do not abort the pass (AC-08, SCENARIO-017, SCENARIO-018)
- Per-episode download failures do not block other episodes (SCENARIO-019)
- Downloaded episodes send PlaylistAddTrack commands via cmd_tx (AC-07, SCENARIO-015)

### Risks

- Reusing `download_list` from lib crate may require careful handling of the closure move semantics -- mitigated by research report ISS-007 confirming correctness of the channel-drain pattern
- SQLite connection handling across async boundaries -- mitigated by opening/closing per sync pass (not held across await points)

---

## Phase 4: Task Lifecycle and Wiring

- **Domain**: backend
- **Effort**: medium
- **Status**: COMPLETE
- **Objective**: Implement `start_podcast_sync_task` with interval timing and cancellation, wire it into `actual_main()`, and verify the full lifecycle including startup sync, periodic execution, and graceful shutdown.
- **Depends On**: Phase 3
- **Commit**: `7cf128ab feat(task-lifecycle-and-wiring)`
- **Tests**: 29 total module tests (20 Phase 3 + 9 new lifecycle tests)

### Scope

In scope:
- Add `start_podcast_sync_task` function to `server/src/podcast_sync.rs`
- Add `mod podcast_sync;` to server crate root
- Modify `actual_main()` in `server/src/server.rs` to conditionally spawn the sync task
- Handle `refresh_on_startup` flag for immediate first sync
- Implement `select!` with `CancellationToken::cancelled()` for graceful shutdown
- Lifecycle tests (task not spawned when disabled, graceful cancellation)

Out of scope:
- Modifying existing periodic tasks
- Changes to the gRPC service

### Tasks

1. Implement `start_podcast_sync_task` function with interval_at and select!
   - Files: server/src/podcast_sync.rs
   - Type: modify
2. Handle `refresh_on_startup` flag for immediate first sync pass
   - Files: server/src/podcast_sync.rs
   - Type: modify
3. Register `mod podcast_sync;` in server crate
   - Files: server/src/main.rs or server/src/lib.rs (whichever declares modules)
   - Type: modify
4. Wire `start_podcast_sync_task` call in `actual_main()` gated by `synchronization.enable`
   - Files: server/src/server.rs
   - Type: modify
5. Add lifecycle tests (disabled task, cancellation, startup sync)
   - Files: server/src/podcast_sync.rs
   - Type: modify

### Acceptance Criteria

- `cargo build --all` compiles without error
- When `synchronization.enable == false`, no sync task is spawned (AC-02, SCENARIO-005)
- When `refresh_on_startup == true`, sync_once executes before periodic loop (AC-03, SCENARIO-006)
- When `refresh_on_startup == false`, no sync occurs until first interval tick (SCENARIO-007)
- The task spawns adjacent to `start_playlist_save_interval` in `actual_main()` (AC-11, SCENARIO-020)
- Cancelling the token causes the task to exit cleanly (AC-09, SCENARIO-009)
- Timer uses `interval_at` preventing drift (SCENARIO-023)

### Risks

- Race condition between startup sync and player auto-play from restored playlist -- mitigated by channel-based serialization (PlayerCmd channel ensures ordered processing)
- Long-running sync_once blocking the cancellation check -- mitigated by the architecture: cancellation is checked between full sync passes (not mid-download); TaskPool Drop cancels in-flight downloads

---

## Phase 5: Integration Tests and Verification

- **Domain**: testing
- **Effort**: medium
- **Status**: COMPLETE
- **Objective**: Write comprehensive integration tests that verify the full feature end-to-end, including mock HTTP servers for feed/download simulation, database state verification, and player command verification.
- **Depends On**: Phase 4
- **Commit**: `1a11b406 feat(integration-tests-and-verification)`
- **Tests**: 40 total module tests (29 unit + 11 integration with wiremock)

### Scope

In scope:
- Integration tests with mock HTTP server (using `wiremock` or inline test server) for feed responses
- Tests verifying deduplication across multiple sync passes
- Tests verifying error isolation with failing feeds
- Tests verifying PlaylistAddTrack commands reach the player channel
- Tests verifying auto-start behavior when queue is empty (SCENARIO-016)
- Tests verifying non-disruption during active playback (SCENARIO-022)
- Full `cargo test --all` pass with all existing and new tests

Out of scope:
- Performance benchmarking
- Load testing with hundreds of podcasts

### Tasks

1. Set up test infrastructure (mock HTTP server, test database helpers)
   - Files: server/src/podcast_sync.rs (tests module) or server/tests/podcast_sync.rs
   - Type: create or modify
2. Write integration test: new episode detection and download (SCENARIO-010, SCENARIO-014)
   - Files: server/src/podcast_sync.rs or server/tests/podcast_sync.rs
   - Type: modify
3. Write integration test: deduplication across sync passes (SCENARIO-011, SCENARIO-012, SCENARIO-013)
   - Files: server/src/podcast_sync.rs or server/tests/podcast_sync.rs
   - Type: modify
4. Write integration test: error isolation with failing feeds (SCENARIO-017, SCENARIO-018, SCENARIO-019)
   - Files: server/src/podcast_sync.rs or server/tests/podcast_sync.rs
   - Type: modify
5. Write integration test: queue append and auto-start (SCENARIO-015, SCENARIO-016)
   - Files: server/src/podcast_sync.rs or server/tests/podcast_sync.rs
   - Type: modify
6. Write integration test: active playback non-disruption (SCENARIO-022)
   - Files: server/src/podcast_sync.rs or server/tests/podcast_sync.rs
   - Type: modify
7. Run full test suite and verify all tests pass
   - Files: none (verification step)
   - Type: verify

### Acceptance Criteria

- All 23 BDD scenarios have at least one corresponding test that passes
- All 11 acceptance criteria are verified by at least one test
- `cargo test --all` passes with zero failures
- `cargo clippy --all -- -D warnings` reports no new warnings
- `cargo fmt --all --check` reports no formatting issues

### Risks

- Mock HTTP server setup complexity for download simulation -- mitigated by using test-scoped local HTTP server with pre-crafted RSS responses
- Test flakiness due to timing in lifecycle tests -- mitigated by using `tokio::time::pause()` for deterministic time control in tests

---

## Cross-Cutting Concerns

### Error Handling Consistency

All phases must follow the project's established error handling patterns:
- Fatal errors propagate via `anyhow::Result` with `.context()` enrichment
- Per-item errors log at `warn` level and continue processing
- Channel send failures use `let _ = tx.send(...)` pattern (log if channel closed)

### Backward Compatibility

Throughout all phases, existing config files must continue to parse without modification. The `#[serde(default)]` annotation on `SynchronizationSettings` guarantees this, but each phase's integration must be verified against the project's existing test TOML fixtures.

### Code Style Compliance

Every phase must pass `cargo clippy --all` (with pedantic lints enabled at workspace level) and `cargo fmt --all --check` before being considered complete.

### Documentation

All new public items must have `///` doc comments. Internal functions get `//` comments explaining non-obvious logic. The `SynchronizationSettings` doc comments serve as user-facing documentation for the TOML config.

---

## Milestone Summary

- **M1: Config Ready**: Phase 1 -- COMPLETE: `SynchronizationSettings` struct with 19 tests, `humantime-serde` v1.1 integrated
- **M2: API Ready**: Phase 2 -- COMPLETE: `PlaylistAddTrack::new_append_single/vec` constructors with 20 tests
- **M3: Core Logic**: Phase 3 -- COMPLETE: `sync_once` function with deduplication, error isolation, and 20 tests
- **M4: Feature Complete**: Phase 4 -- COMPLETE: Full sync task lifecycle wired into server with 9 lifecycle tests
- **M5: Quality Assured**: Phase 5 -- COMPLETE: 11 integration tests with wiremock, all 23 BDD scenarios covered, 40 total module tests passing

## Implementation Deviations

| Planned | Actual | Reason |
|---------|--------|--------|
| `humantime-serde = "0.2"` | `humantime-serde = "1.1"` | v0.2 outdated; v1.1 is current stable with same API |
| Tests inline in impl files | Tests in separate `*_tests.rs` files | Better code organization; 351-line test suite kept separate from 113-line impl |
| T-22 in Phase 4 | T-22 in Phase 3 | Rust requires module declarations for compilation |
| 4 config tests | 19 config tests | Comprehensive coverage of custom Deserialize impl |
| Single test task T-12 | 20 tests for PlaylistAddTrack | Fine-grained tests for all track source variants |
| `wiremock` or inline test server | `wiremock 0.6` | Standard Rust HTTP integration testing crate |
| `sync_once` takes individual params | `sync_once` takes `&SharedServerSettings` | More ergonomic and future-proof |
