# Task List: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Updated**: 2026-06-23
- **Author**: super-dev:spec-writer
- **Specification**: ./09-specification.md
- **Implementation Plan**: ./10-implementation-plan.md
- **Total Tasks**: 26
- **Completed**: 26/26
- **Status**: COMPLETE

---

## Phase 1: Configuration Schema

**Milestone**: SynchronizationSettings struct with serde defaults, humantime-serde integrated, config roundtrip tests passing
**Status**: COMPLETE (19 unit tests passing)

- [x] **T-01**: Add `humantime-serde = "1.1"` to `[workspace.dependencies]` in root Cargo.toml
  - Files: Cargo.toml
  - Type: modify
  - Effort: small
  - Depends on: None
  - Note: Used v1.1 (latest stable) instead of spec's v0.2

- [x] **T-02**: Add `humantime-serde.workspace = true` to `[dependencies]` in lib/Cargo.toml
  - Files: lib/Cargo.toml
  - Type: modify
  - Effort: small
  - Depends on: T-01

- [x] **T-03**: Create `SynchronizationSettings` struct with custom `Deserialize` impl, `#[serde(with = "humantime_serde")]` on interval field, and `impl Default`
  - Files: lib/src/config/v2/server/synchronization.rs (created, 113 lines)
  - Type: create
  - Effort: small
  - Depends on: T-02

- [x] **T-04**: Add `pub mod synchronization;` declaration and `pub synchronization: SynchronizationSettings` field to `ServerSettings` struct
  - Files: lib/src/config/v2/server/mod.rs
  - Type: modify
  - Effort: small
  - Depends on: T-03

- [x] **T-05**: Write unit test: default config when `[synchronization]` section absent (SCENARIO-001)
  - Files: lib/src/config/v2/server/synchronization_tests.rs (created, 351 lines)
  - Type: create
  - Effort: small
  - Depends on: T-04
  - Note: Tests placed in separate file; 19 tests total covering T-05 through T-08 plus additional edge cases

- [x] **T-06**: Write unit test: explicit non-default values deserialize correctly (SCENARIO-002)
  - Files: lib/src/config/v2/server/synchronization_tests.rs
  - Type: create (part of T-05 file)
  - Effort: small
  - Depends on: T-04

- [x] **T-07**: Write unit test: serialization roundtrip preserves all fields (SCENARIO-003)
  - Files: lib/src/config/v2/server/synchronization_tests.rs
  - Type: create (part of T-05 file)
  - Effort: small
  - Depends on: T-04

- [x] **T-08**: Write unit test: invalid duration string produces deserialization error (SCENARIO-004)
  - Files: lib/src/config/v2/server/synchronization_tests.rs
  - Type: create (part of T-05 file)
  - Effort: small
  - Depends on: T-04

---

## Phase 2: PlaylistAddTrack API Extension

**Milestone**: AT_END constant and new_append_single/new_append_vec constructors available with passing unit tests
**Status**: COMPLETE (20 unit tests passing)

- [x] **T-09**: Add `pub const AT_END: u64 = u64::MAX` to `impl PlaylistAddTrack` block
  - Files: lib/src/player.rs (+22 lines)
  - Type: modify
  - Effort: small
  - Depends on: None

- [x] **T-10**: Add `pub fn new_append_single(track: PlaylistTrackSource) -> Self` method
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09

- [x] **T-11**: Add `pub fn new_append_vec(tracks: Vec<PlaylistTrackSource>) -> Self` method
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09

- [x] **T-12**: Write unit tests verifying AT_END value and constructor behavior
  - Files: lib/src/player_playlist_add_track_tests.rs (created, 269 lines, 20 tests)
  - Type: create
  - Effort: small
  - Depends on: T-10, T-11
  - Note: Tests in separate file registered via lib/src/lib.rs

---

## Phase 3: Sync Pass Logic

**Milestone**: sync_once function implemented with deduplication, error isolation, download signaling, and enqueue logic
**Status**: COMPLETE (20 unit tests passing)

- [x] **T-13**: Create `server/src/podcast_sync.rs` with module structure, imports, and `SyncPassStats` struct definition
  - Files: server/src/podcast_sync.rs (created, 931 lines initial)
  - Type: create
  - Effort: small
  - Depends on: T-04, T-12

- [x] **T-14**: Implement `sync_once` function signature and Database open + get_podcasts with early return on empty list (SCENARIO-021)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-13

- [x] **T-15**: Implement per-podcast feed fetch loop using check_feed with error isolation (warn + continue pattern) (SCENARIO-017, SCENARIO-018)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-14

- [x] **T-16**: Implement episode deduplication via database update_podcast and filtering for undownloaded episodes (path == None) (SCENARIO-010, SCENARIO-011, SCENARIO-012, SCENARIO-013)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-15

- [x] **T-17**: Implement download_list invocation with channel-drain pattern (move tx into closure, while-let-Some rx.recv) (SCENARIO-019)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-16

- [x] **T-18**: Implement enqueue logic: db.insert_file + PlaylistAddTrack::new_append_single via cmd_tx (SCENARIO-014, SCENARIO-015)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-17

- [x] **T-19**: Add basic unit tests for sync_once with in-memory database (no-podcasts case, dedup case)
  - Files: server/src/podcast_sync.rs (20 tests)
  - Type: modify
  - Effort: medium
  - Depends on: T-18

---

## Phase 4: Task Lifecycle and Wiring

**Milestone**: Full sync task lifecycle wired into server with startup sync, periodic execution, and graceful shutdown
**Status**: COMPLETE (29 total module tests passing)

- [x] **T-20**: Implement `start_podcast_sync_task` function with `interval_at` + `select!` on `cancel_token.cancelled()` (SCENARIO-008, SCENARIO-009, SCENARIO-023)
  - Files: server/src/podcast_sync.rs (+466/-4 lines)
  - Type: modify
  - Effort: medium
  - Depends on: T-19

- [x] **T-21**: Add `refresh_on_startup` handling: execute sync_once before entering periodic loop when flag is true (SCENARIO-006, SCENARIO-007)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: T-20

- [x] **T-22**: Register `mod podcast_sync;` in server crate module tree
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-21
  - Note: Completed in Phase 3 (required for compilation); no additional change needed in Phase 4

- [x] **T-23**: Wire `start_podcast_sync_task` call in `actual_main()` gated by `synchronization.enable`, with `get_app_config_path()` for db_path (SCENARIO-005, SCENARIO-020)
  - Files: server/src/server.rs (+14 lines)
  - Type: modify
  - Effort: small
  - Depends on: T-22

---

## Phase 5: Integration Tests and Verification

**Milestone**: All 23 BDD scenarios covered by tests, full test suite passes, clippy and fmt clean
**Status**: COMPLETE (40 total module tests passing, 11 integration tests)

- [x] **T-24**: Write integration test: full sync pass with mock feeds verifying episode download and enqueue (SCENARIO-010, SCENARIO-014, SCENARIO-015, SCENARIO-016)
  - Files: server/src/podcast_sync.rs (tests module, using wiremock 0.6)
  - Type: modify
  - Effort: large
  - Depends on: T-23
  - Tests: integration_full_flow_fetches_downloads_and_enqueues_new_episodes, integration_deduplication_across_multiple_sync_passes, integration_mixed_new_and_existing_episodes, integration_enqueue_format_enables_autostart_on_empty_queue

- [x] **T-25**: Write integration test: error isolation with failing/malformed feeds, verify other feeds succeed (SCENARIO-017, SCENARIO-018, SCENARIO-019)
  - Files: server/src/podcast_sync.rs (tests module)
  - Type: modify
  - Effort: medium
  - Depends on: T-23
  - Tests: integration_http_500_on_one_feed_does_not_abort_others, integration_malformed_feed_xml_does_not_crash, integration_one_episode_download_fails_others_succeed

- [x] **T-26**: Write integration test: task lifecycle (disabled sync, cancellation, active playback non-disruption) (SCENARIO-005, SCENARIO-009, SCENARIO-022)
  - Files: server/src/podcast_sync.rs (tests module)
  - Type: modify
  - Effort: medium
  - Depends on: T-23
  - Tests: integration_sync_during_playback_appends_at_end, integration_startup_sync_with_mock_server, integration_empty_feed_completes_without_downloads, integration_deduplication_by_enclosure_url_fallback

---

## Summary

- Phase 1: Configuration Schema -- 8 tasks, COMPLETE (19 tests)
- Phase 2: PlaylistAddTrack API Extension -- 4 tasks, COMPLETE (20 tests)
- Phase 3: Sync Pass Logic -- 7 tasks, COMPLETE (20 tests)
- Phase 4: Task Lifecycle and Wiring -- 4 tasks, COMPLETE (9 new tests, 29 total)
- Phase 5: Integration Tests and Verification -- 3 tasks, COMPLETE (11 integration tests, 40 total)
- **Total**: 26 tasks completed, 79 tests passing across all phases

## Files Created/Modified

| File | Action | Lines |
|------|--------|-------|
| `Cargo.toml` | modified | +3 |
| `Cargo.lock` | modified | +117 |
| `lib/Cargo.toml` | modified | +1 |
| `lib/src/config/v2/server/mod.rs` | modified | +8 |
| `lib/src/config/v2/server/synchronization.rs` | created | +113 |
| `lib/src/config/v2/server/synchronization_tests.rs` | created | +350 |
| `lib/src/lib.rs` | modified | +3 |
| `lib/src/player.rs` | modified | +22 |
| `lib/src/player_playlist_add_track_tests.rs` | created | +269 |
| `server/Cargo.toml` | modified | +5 |
| `server/src/podcast_sync.rs` | created | +2551 |
| `server/src/server.rs` | modified | +15 |
