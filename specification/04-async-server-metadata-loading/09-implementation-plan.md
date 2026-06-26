# Implementation Plan: Async Server Metadata Loading

- **Date**: 2026-06-26
- **Author**: super-dev:spec-writer
- **Specification**: ./08-specification.md
- **Total Phases**: 4
- **Estimated Effort**: medium (2-3 days)

---

## Phase Summary

- Phase 1: Foundation and Type Definitions — Domain: backend, Effort: small, Depends on: None, Parallelizable with: None
- Phase 2: Background Loading Task and Completion Handler — Domain: backend, Effort: medium, Depends on: Phase 1, Parallelizable with: None
- Phase 3: Server Startup Integration and Save Protection — Domain: backend, Effort: medium, Depends on: Phase 2, Parallelizable with: None
- Phase 4: Integration Testing and Validation — Domain: testing, Effort: medium, Depends on: Phase 3, Parallelizable with: None

---

## Phase 1: Foundation and Type Definitions

- **Domain**: backend
- **Effort**: small
- **Objective**: Add the `PlayerCmd::PlaylistLoadComplete` variant, the `PlaylistLoadingFlag` type alias, and required imports without changing any runtime behavior. Ensure the workspace builds and all existing tests pass.
- **Depends On**: None
- **Parallelizable With**: None

### Scope

**In scope:**
- Add `PlayerCmd::PlaylistLoadComplete` variant to the `PlayerCmd` enum in `playback/src/lib.rs`
- Add `PlaylistLoadingFlag` type alias (`Arc<AtomicBool>`) in `server/src/server.rs`
- Add no-op match arm for `PlaylistLoadComplete` in the `player_loop` match statement
- Add necessary imports (`std::sync::atomic::{AtomicBool, Ordering}`, `std::sync::Arc`)

**Out of scope:**
- Behavioral changes to server startup
- Background task implementation
- Save-interval modification

### Tasks

1. Add `PlayerCmd::PlaylistLoadComplete` enum variant to `playback/src/lib.rs`
   - Files: playback/src/lib.rs
   - Type: modify
2. Add no-op match arm for `PlaylistLoadComplete` in `player_loop` match statement
   - Files: server/src/server.rs
   - Type: modify
3. Add `PlaylistLoadingFlag` type alias and necessary atomic imports in server module
   - Files: server/src/server.rs
   - Type: modify
4. Verify workspace builds without warnings and all existing tests pass
   - Files: (none modified, validation only)
   - Type: verify

### Acceptance Criteria

- `cargo build --workspace` succeeds without new warnings
- `cargo clippy --workspace` produces no new warnings
- `cargo test --workspace` passes (all existing tests continue to pass)
- `PlayerCmd::PlaylistLoadComplete` variant exists and is handled (no-op) in player_loop
- `PlaylistLoadingFlag` type alias is defined and available for use in Phase 2

### Risks

- Adding a new `PlayerCmd` variant requires exhaustive match handling in all match statements on `PlayerCmd`. Verify there are no other match sites beyond `player_loop`.

---

## Phase 2: Background Loading Task and Completion Handler

- **Domain**: backend
- **Effort**: medium
- **Objective**: Implement `start_background_playlist_load()` and `complete_background_load()` functions with the four-step ordering invariant. These are standalone functions that can be tested in isolation before being wired into the server startup sequence.
- **Depends On**: Phase 1
- **Parallelizable With**: None

### Scope

**In scope:**
- Implement `start_background_playlist_load()` function following the `start_podcast_sync_task` pattern
- Implement `complete_background_load()` function with ordering-invariant doc-comment
- Handle error cases: total load failure (log and clear flag), JoinError (panic in spawn_blocking), cancellation
- Add logging: load start, load complete with timing, load failure

**Out of scope:**
- Modifying the server startup sequence (Phase 3)
- Modifying `start_playlist_save_interval` (Phase 3)
- Integration tests (Phase 4)

### Tasks

1. Implement `start_background_playlist_load()` function with Handle, CancellationToken, select! pattern
   - Files: server/src/server.rs
   - Type: modify
2. Implement `complete_background_load()` function with four-step ordering invariant and doc-comment
   - Files: server/src/server.rs
   - Type: modify
3. Handle error paths: total load failure logs error and clears loading flag; JoinError (panic) is caught and logged; CancellationToken fires exits cleanly
   - Files: server/src/server.rs
   - Type: modify
4. Add INFO-level logging for background load start and completion with timing
   - Files: server/src/server.rs
   - Type: modify
5. Verify workspace builds and all existing tests pass (no behavioral change yet)
   - Files: (none modified, validation only)
   - Type: verify

### Acceptance Criteria

- `start_background_playlist_load` function compiles and follows the `start_podcast_sync_task` pattern exactly (Handle + CancellationToken + select!)
- `complete_background_load` function has ordering-invariant doc-comment documenting all 4 steps
- Error paths log at appropriate levels (ERROR for total failure, WARN for send failures)
- `cargo build --workspace` succeeds without new warnings
- `cargo test --workspace` passes (functions exist but are not yet called from startup)

### Risks

- The `Playlist::load()` function requires access to the server config path. Ensure the necessary config data is accessible from within the `spawn_blocking` closure (it is: `SharedServerSettings` is `Arc<RwLock<..>>` and can be cloned into the closure).

---

## Phase 3: Server Startup Integration and Save Protection

- **Domain**: backend
- **Effort**: medium
- **Objective**: Wire the background loading into the server startup sequence: replace `Playlist::new_shared()` with empty playlist creation, call `start_background_playlist_load()` after `start_service()`, modify `start_playlist_save_interval` to check the loading flag, and update `player_loop` to handle `PlaylistLoadComplete` with auto-play logic.
- **Depends On**: Phase 2
- **Parallelizable With**: None

### Scope

**In scope:**
- Replace `Playlist::new_shared()` at server.rs:148-149 with `Arc::new(RwLock::new(Playlist::new(...)))`
- Create `PlaylistLoadingFlag` instance set to `true`
- Call `start_background_playlist_load()` after `start_service()` returns
- Modify `start_playlist_save_interval` to accept and check `PlaylistLoadingFlag`
- Update `PlayerCmd::PlaylistLoadComplete` match arm from no-op to auto-play trigger
- Remove the immediate `startup_state == Playing` check at player_loop entry
- Pass `playlist_is_loading` to `start_playlist_save_interval`
- Handle the empty-playlist edge case (playlist.log does not exist or is empty): no background loading spawned, flag set to false immediately

**Out of scope:**
- Integration/E2E tests (Phase 4)
- TUI-side changes (TUI already handles empty playlists and PlaylistShuffled events)

### Tasks

1. Replace `Playlist::new_shared()` call with empty `SharedPlaylist` creation and `PlaylistLoadingFlag` initialization
   - Files: server/src/server.rs
   - Type: modify
2. Call `start_background_playlist_load()` after `start_service()` returns successfully
   - Files: server/src/server.rs
   - Type: modify
3. Modify `start_playlist_save_interval` to accept `PlaylistLoadingFlag` and skip save when loading is true
   - Files: server/src/server.rs
   - Type: modify
4. Update `PlayerCmd::PlaylistLoadComplete` handler to call `player.resume_from_stopped()` when `startup_state == Playing`
   - Files: server/src/server.rs
   - Type: modify
5. Remove the immediate `startup_state == Playing` check at player_loop entry (lines 333-335)
   - Files: server/src/server.rs
   - Type: modify
6. Handle empty-playlist edge case: if load returns empty vec, still complete the full sequence (clear flag, send events)
   - Files: server/src/server.rs
   - Type: modify
7. Manual validation: build, run with a playlist of 200+ tracks, confirm TUI connects within 1 second, confirm playlist appears after loading completes
   - Files: (none modified, validation only)
   - Type: verify

### Acceptance Criteria

- Server starts gRPC listener within 1 second regardless of playlist size (AC-01, SCENARIO-001, SCENARIO-002, SCENARIO-003)
- `GetPlaylist` returns empty playlist during loading, full playlist after loading (AC-05, SCENARIO-010, SCENARIO-012)
- `playlist.log` is NOT overwritten during background loading (AC-07, SCENARIO-015)
- Auto-play triggers only after `PlaylistLoadComplete` is received (AC-06, SCENARIO-013, SCENARIO-014)
- Server continues operating with empty playlist on load failure (AC-08, SCENARIO-019)
- Loaded playlist matches synchronous implementation output (AC-03, SCENARIO-006, SCENARIO-007)
- TUI receives PlaylistShuffled notification when loading completes (AC-04, SCENARIO-008)
- Server shutdown during loading completes within 1 second (AC-09, SCENARIO-021)
- `cargo build --workspace` succeeds without new warnings
- `cargo test --workspace` passes (all existing tests continue to pass)

### Risks

- The removal of the immediate `startup_state` check means auto-play depends entirely on the `PlaylistLoadComplete` command being received. If the background task fails to send this command (channel closed, panic), auto-play never triggers. Mitigation: the error handler in `start_background_playlist_load` ensures the command is sent even on partial load success.
- The empty-playlist creation changes the initialization path. Any code between playlist creation and `start_service()` that reads from the playlist will see 0 tracks. Verify: `MusicPlayerService::new()` only stores a clone of the Arc, does not read tracks at construction time. `RunInfo::default()` does not depend on playlist state.

---

## Phase 4: Integration Testing and Validation

- **Domain**: testing
- **Effort**: medium
- **Objective**: Add comprehensive integration tests validating server startup timing, playlist correctness after async load, save protection during loading, auto-play deferral, graceful degradation on load failure, and clean shutdown. Reference all BDD scenarios.
- **Depends On**: Phase 3
- **Parallelizable With**: None

### Scope

**In scope:**
- Create test file `server/tests/phase4_async_loading_tests.rs`
- Test fixtures: playlist.log files with varying sizes (0, 10, 500, 1000 tracks), corrupt entries, missing file
- Integration tests for AC-01 through AC-10
- Timing-based assertion: server connection accepted within 1 second
- Correctness assertion: loaded playlist matches synchronous baseline
- Save protection assertion: playlist.log unchanged during loading
- Shutdown timing assertion: server exits within 1 second during loading

**Out of scope:**
- TUI-side testing (TUI changes not required for this feature)
- Performance benchmarks (covered by existing spec-03 benchmarks)
- Progressive loading (deferred to future enhancement)

### Tasks

1. Create test file with module doc-comments referencing ACs and BDD scenarios
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
2. Implement test fixtures: generate playlist.log files with known track paths
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
3. Test server_accepts_connection_within_1s_with_large_playlist (SCENARIO-001)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
4. Test server_accepts_connection_with_empty_playlist (SCENARIO-002, SCENARIO-024)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
5. Test playlist_correctness_matches_synchronous_baseline (SCENARIO-006, SCENARIO-007)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
6. Test get_playlist_returns_empty_during_loading (SCENARIO-010, SCENARIO-011)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
7. Test client_receives_notification_after_load_complete (SCENARIO-008, SCENARIO-009)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
8. Test save_interval_skipped_during_loading (SCENARIO-015, SCENARIO-016, SCENARIO-017)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
9. Test autoplay_deferred_until_load_complete (SCENARIO-013, SCENARIO-014)
   - Files: server/tests/phase4_async_loading_tests.rs
   - Type: create
10. Test graceful_degradation_on_load_failure (SCENARIO-018, SCENARIO-019, SCENARIO-020)
    - Files: server/tests/phase4_async_loading_tests.rs
    - Type: create
11. Test server_shutdown_during_loading_completes_within_1s (SCENARIO-021, SCENARIO-022, SCENARIO-026)
    - Files: server/tests/phase4_async_loading_tests.rs
    - Type: create
12. Verify all tests pass and no regressions in existing test suite
    - Files: (none modified, validation only)
    - Type: verify

### Acceptance Criteria

- All new integration tests pass
- All existing workspace tests continue to pass
- Test coverage addresses all 27 BDD scenarios (at least one test per scenario)
- Test file follows the established `phase*_*_tests.rs` naming convention
- Test file includes module doc-comments linking to ACs and scenarios
- Timing-based tests use reasonable tolerances (1 second for startup, 1 second for shutdown)
- No flaky tests (timing tests use generous margins; correctness tests are deterministic)

### Risks

- Timing-based tests can be flaky on slow CI machines. Mitigation: use 1-second thresholds (generous for an operation that should complete in <100ms on any machine). If flaky, increase to 2 seconds.
- Integration tests require spawning the actual server process. Mitigation: use the existing test infrastructure pattern from `server/tests/` (if present), or create a helper that starts the server in-process with a test config.

---

## Cross-Cutting Concerns

### Ordering Invariant Preservation

The four-step completion sequence in `complete_background_load()` has strict ordering requirements documented in the deep research report (ISS-005). All phases that modify or interact with this function must preserve the ordering. Code reviewers should check that no code is inserted between steps 1-4 in any future modification.

### Backward Compatibility

The server's external gRPC API is unchanged. Clients connecting to the server will observe:
- Empty playlist during the loading window (1-3 seconds after server start)
- Full playlist after loading completes (via PlaylistShuffled event or subsequent GetPlaylist call)

This is fully compatible with the existing TUI client which handles both empty and populated playlist responses.

### Logging Consistency

All new log statements follow the existing convention:
- `info!` for milestones (load start, load complete)
- `warn!` for non-fatal failures (send errors, partial load failures)
- `error!` for serious issues (total load failure)
- `debug!` for operational details (save skipped during loading)

## Milestone Summary

- **Phase 1 Complete**: Phase 1 — Deliverable: PlayerCmd::PlaylistLoadComplete variant and PlaylistLoadingFlag type exist; workspace builds cleanly, Verification: `cargo build --workspace && cargo test --workspace`
- **Phase 2 Complete**: Phase 2 — Deliverable: `start_background_playlist_load` and `complete_background_load` functions implemented with ordering invariant, Verification: `cargo build --workspace && cargo test --workspace` (functions compile but are not yet active)
- **Phase 3 Complete**: Phase 3 — Deliverable: Server starts with empty playlist, loads metadata in background, TUI connects within 1 second, auto-play deferred, saves protected, Verification: Manual test with 200+ track playlist confirms sub-1-second connection and eventual playlist population
- **Phase 4 Complete**: Phase 4 — Deliverable: Comprehensive integration test suite covering all 10 ACs and 27 BDD scenarios, Verification: `cargo test --workspace` passes all new and existing tests
