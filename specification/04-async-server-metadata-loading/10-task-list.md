# Task List: Async Server Metadata Loading

- **Date**: 2026-06-26
- **Author**: super-dev:spec-writer
- **Specification**: ./08-specification.md
- **Implementation Plan**: ./09-implementation-plan.md
- **Total Tasks**: 24

---

## Phase 1: Foundation and Type Definitions

**Milestone**: PlayerCmd::PlaylistLoadComplete variant and PlaylistLoadingFlag type exist; workspace builds cleanly

- [ ] **T-01**: Add `PlayerCmd::PlaylistLoadComplete` enum variant to the PlayerCmd enum
  - Files: playback/src/lib.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC refs: AC-06
  - Details: Add `PlaylistLoadComplete,` variant after the existing `PodcastAddFeed(String)` variant. Add a doc-comment: `/// Background playlist metadata loading has completed. Triggers auto-play if startup_state is Playing.`

- [ ] **T-02**: Add no-op match arm for `PlaylistLoadComplete` in player_loop
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-01
  - AC refs: AC-06
  - Details: Add `PlayerCmd::PlaylistLoadComplete => { /* Phase 3: auto-play logic */ }` to the match statement in `player_loop`. This prevents a non-exhaustive match compile error.

- [ ] **T-03**: Add `PlaylistLoadingFlag` type alias and atomic imports
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC refs: AC-07, AC-02
  - Details: Add `use std::sync::atomic::{AtomicBool, Ordering};` to imports. Add type alias `type PlaylistLoadingFlag = Arc<AtomicBool>;` near the top of the file (after existing type definitions).

- [ ] **T-04**: Verify workspace builds and all tests pass
  - Files: (validation only)
  - Type: verify
  - Effort: small
  - Depends on: T-01, T-02, T-03
  - Details: Run `cargo build --workspace`, `cargo clippy --workspace`, `cargo test --workspace`. All must pass with no new warnings.

---

## Phase 2: Background Loading Task and Completion Handler

**Milestone**: start_background_playlist_load and complete_background_load functions implemented with ordering invariant

- [ ] **T-05**: Implement `complete_background_load()` function with ordering-invariant doc-comment
  - Files: server/src/server.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-04
  - AC refs: AC-03, AC-04, AC-06
  - Details: Implement the four-step completion sequence: (1) write-lock swap of tracks and current_track_index into SharedPlaylist, (2) AtomicBool store(false, Ordering::Release), (3) send PlaylistShuffled event via stream_tx with serialized playlist tracks, (4) send PlayerCmd::PlaylistLoadComplete via cmd_tx. Include the full ordering-invariant doc-comment from the specification section 4.2. Handle send errors with warn! logging.

- [ ] **T-06**: Implement `start_background_playlist_load()` function skeleton
  - Files: server/src/server.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-05
  - AC refs: AC-02, AC-09
  - Details: Follow the `start_podcast_sync_task` pattern. Accept Handle, CancellationToken, SharedPlaylist, PlaylistLoadingFlag, StreamTX, PlayerCmdSender, SharedServerSettings. Spawn async task with select! on CancellationToken. Inside the task: log "Starting background playlist metadata loading", call tokio::task::spawn_blocking to run Playlist::load() on PLAYLIST_POOL.

- [ ] **T-07**: Add success path in background loading task (calls complete_background_load)
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-06
  - AC refs: AC-03, AC-04
  - Details: When spawn_blocking returns Ok(Ok((loaded_index, loaded_tracks))), log track count and elapsed time at INFO level, then call complete_background_load() with the loaded data.

- [ ] **T-08**: Add error paths in background loading task
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-06
  - AC refs: AC-08, AC-09
  - Details: Handle three error cases: (1) spawn_blocking returns Ok(Err(load_error)) - log at ERROR level, clear is_loading flag with Release ordering; (2) spawn_blocking returns Err(join_error) - log panic at ERROR level, clear is_loading flag; (3) CancellationToken fires - log at INFO level "Background playlist load cancelled (shutdown)", break from select loop.

- [ ] **T-09**: Verify workspace builds and all tests pass
  - Files: (validation only)
  - Type: verify
  - Effort: small
  - Depends on: T-05, T-06, T-07, T-08
  - Details: Run `cargo build --workspace`, `cargo clippy --workspace`, `cargo test --workspace`. Functions exist but are not yet called from the startup path, so behavior is unchanged.

---

## Phase 3: Server Startup Integration and Save Protection

**Milestone**: Server starts with empty playlist, loads metadata in background, TUI connects within 1 second

- [ ] **T-10**: Replace `Playlist::new_shared()` with empty SharedPlaylist creation
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09
  - AC refs: AC-01
  - Details: Replace lines 148-149 (`let playlist = Playlist::new_shared(&config, stream_tx.clone()).context("Failed to load playlist")?;`) with `let playlist: SharedPlaylist = Arc::new(RwLock::new(Playlist::new(&config, stream_tx.clone())));`. Remove the `.context()` error handling since `Playlist::new()` is infallible.

- [ ] **T-11**: Create PlaylistLoadingFlag instance and set to true
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-10
  - AC refs: AC-07, AC-02
  - Details: Add `let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));` immediately after the playlist creation line.

- [ ] **T-12**: Call `start_background_playlist_load()` after `start_service()` returns
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-11
  - AC refs: AC-01, AC-02
  - Details: After the `start_service()` call and before `start_playlist_save_interval()`, add the call to `start_background_playlist_load(tokio_handle.clone(), service_cancel_token.clone(), playlist.clone(), playlist_is_loading.clone(), stream_tx.clone(), cmd_tx.clone(), config.clone())`.

- [ ] **T-13**: Modify `start_playlist_save_interval` to accept and check PlaylistLoadingFlag
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-11
  - AC refs: AC-07
  - Details: Add `playlist_is_loading: PlaylistLoadingFlag` parameter. Before calling `playlist.write().save_if_modified()`, check `if playlist_is_loading.load(Ordering::Acquire) { debug!("Skipping playlist save: background loading in progress"); continue; }`. Update the call site to pass `playlist_is_loading.clone()`.

- [ ] **T-14**: Update `PlayerCmd::PlaylistLoadComplete` handler with auto-play logic
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-10
  - AC refs: AC-06
  - Details: Replace the no-op match arm from T-02 with: `PlayerCmd::PlaylistLoadComplete => { info!("Background playlist load complete"); if player.config.read().settings.player.startup_state == StartupState::Playing { player.resume_from_stopped(); } }`

- [ ] **T-15**: Remove the immediate `startup_state == Playing` check at player_loop entry
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-14
  - AC refs: AC-06
  - Details: Remove or comment out lines 333-335: `if player.config.read().settings.player.startup_state == StartupState::Playing { player.resume_from_stopped(); }`. Auto-play now triggers exclusively via the PlaylistLoadComplete command from the background loading task.

- [ ] **T-16**: Manual validation with real playlist
  - Files: (validation only)
  - Type: verify
  - Effort: small
  - Depends on: T-10, T-11, T-12, T-13, T-14, T-15
  - Details: Build the server (`cargo build -p termusic-server`). Run with a playlist.log containing 200+ tracks. Verify: (1) server log shows "Server listening on..." within 1 second, (2) TUI connects without "Connecting is taking more time" message, (3) playlist appears in TUI after a few seconds, (4) playlist.log is not corrupted.

---

## Phase 4: Integration Testing and Validation

**Milestone**: Comprehensive integration test suite covering all 10 ACs and 27 BDD scenarios

- [ ] **T-17**: Create test file with module doc-comments and test helper infrastructure
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: create
  - Effort: small
  - Depends on: T-16
  - Details: Create the test file with `//!` module doc-comments listing Phase 4, targeted ACs (AC-01 through AC-10), and BDD scenario references. Add helper functions for: creating temp playlist.log fixtures, starting the server in-process with test config, measuring connection timing.

- [ ] **T-18**: Implement server startup timing tests (SCENARIO-001, SCENARIO-002, SCENARIO-003)
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17
  - AC refs: AC-01
  - Details: Test that gRPC connection is accepted within 1 second for playlist sizes of 0, 10, and 500+ tracks. Use `tokio::time::timeout(Duration::from_secs(1), connect_to_server())` pattern.

- [ ] **T-19**: Implement playlist correctness test (SCENARIO-006, SCENARIO-007)
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17
  - AC refs: AC-03
  - Details: Load a known playlist synchronously (using Playlist::load directly), then load the same playlist via the async server startup. Compare track order, metadata fields, and current_track_index. Assert identical results.

- [ ] **T-20**: Implement save protection test (SCENARIO-015, SCENARIO-016, SCENARIO-017)
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17
  - AC refs: AC-07
  - Details: Test that playlist.log file content is unchanged during the loading window. Verify by: (1) record file hash before test, (2) start server with large playlist, (3) immediately trigger save interval (or wait for first tick), (4) assert file hash unchanged, (5) wait for load complete, (6) trigger save, (7) assert file is written.

- [ ] **T-21**: Implement auto-play deferral test (SCENARIO-013, SCENARIO-014)
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17
  - AC refs: AC-06
  - Details: Configure startup_state=Playing. Start server. Verify no playback attempt during loading (playlist is empty, resume_from_stopped returns early). Verify playback begins after PlaylistLoadComplete is processed (requires observing player state or stream events).

- [ ] **T-22**: Implement graceful degradation tests (SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024)
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17
  - AC refs: AC-08
  - Details: Test with: (1) playlist.log containing mix of valid and invalid paths - assert partial load succeeds; (2) playlist.log with permission denied - assert server continues with empty playlist; (3) missing playlist.log - assert server starts normally with empty playlist.

- [ ] **T-23**: Implement shutdown during loading test (SCENARIO-021, SCENARIO-022, SCENARIO-026)
  - Files: server/tests/phase4_async_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17
  - AC refs: AC-09
  - Details: Start server with large playlist. Send Quit command before loading completes. Assert server exits within 1 second. Also test: shutdown after loading completes has no additional delay; immediate shutdown before loading begins any work.

- [ ] **T-24**: Final validation: run full test suite and verify no regressions
  - Files: (validation only)
  - Type: verify
  - Effort: small
  - Depends on: T-17, T-18, T-19, T-20, T-21, T-22, T-23
  - Details: Run `cargo test --workspace` and confirm all new tests pass alongside all existing tests. Run `cargo clippy --workspace` to confirm no new warnings. Verify test output references the correct SCENARIO-IDs in test names or comments.

---

## Summary

- Phase 1: Foundation and Type Definitions — 4 tasks, small effort
- Phase 2: Background Loading Task and Completion Handler — 5 tasks, medium effort
- Phase 3: Server Startup Integration and Save Protection — 7 tasks, medium effort
- Phase 4: Integration Testing and Validation — 8 tasks, medium effort
- **Total**: 24 tasks, medium overall effort (2-3 days)
