# Implementation Plan: PR #720 Podcast Synchronization — Review Feedback Remediation

- **Date**: 2026-06-25
- **Author**: super-dev:spec-writer
- **Specification**: ./09-specification.md
- **Total Phases**: 5
- **Estimated Effort**: large (5 phases, ~40 tasks)

---

## Phase Summary

- Phase 1: Prerequisites and Migration — Domain: backend, Effort: large, Depends on: None, Parallelizable with: None
- Phase 2: Architecture and Config Redesign — Domain: backend, Effort: medium, Depends on: Phase 1, Parallelizable with: None
- Phase 3: Sync Logic Correctness — Domain: backend, Effort: large, Depends on: Phase 2, Parallelizable with: None
- Phase 4: Test Quality — Domain: testing, Effort: medium, Depends on: Phase 3, Parallelizable with: Phase 5
- Phase 5: Style and Conventions — Domain: backend, Effort: small, Depends on: Phase 3, Parallelizable with: Phase 4

---

## Phase Numbering Mapping

| Implementation Plan Phase | Requirements/BDD Phase | Description |
|--------------------------|------------------------|-------------|
| Phase 1 | Phase 0 | Prerequisites and Migration |
| Phase 2 | Phase 1 | Architecture and Config Redesign |
| Phase 3 | Phase 2 | Sync Logic Correctness |
| Phase 4 | Phase 3 | Test Quality |
| Phase 5 | Phase 4 | Style and Conventions |

---

## Phase 1: Prerequisites and Migration

- **Domain**: backend
- **Effort**: large
- **Objective**: Move all podcast network operations (feed refresh, download dispatch) from the TUI crate to the server crate, with the TUI delegating via PlayerCmd over the existing gRPC/UDS communication layer.
- **Depends On**: None
- **Parallelizable With**: None

### Scope

**In scope**: Adding PlayerCmd variants for podcast operations to `playback/src/lib.rs`, adding server-side handlers in `server/src/server.rs`, updating TUI to send commands instead of calling podcast functions directly, verifying OPML import/export still works.

**Out of scope**: Periodic sync changes, config restructuring, test cleanup. Those depend on this phase completing first.

### Tasks

1. Add `PodcastFeedRefresh` variant to `PlayerCmd` enum in `playback/src/lib.rs`
   - Files: playback/src/lib.rs
   - Type: modify
2. Add `PodcastDownloadEpisodes(Vec<EpisodeDownloadRequest>)` variant to `PlayerCmd` enum
   - Files: playback/src/lib.rs
   - Type: modify
3. Define `EpisodeDownloadRequest` struct in `playback/src/lib.rs`
   - Files: playback/src/lib.rs
   - Type: modify
4. Add handler for `PlayerCmd::PodcastFeedRefresh` in server player loop (`server/src/server.rs`)
   - Files: server/src/server.rs
   - Type: modify
5. Add handler for `PlayerCmd::PodcastDownloadEpisodes` in server player loop
   - Files: server/src/server.rs
   - Type: modify
6. Replace direct `check_feed()` call in TUI with `PlayerCmd::PodcastFeedRefresh` send
   - Files: tui/src/ui/components/podcast.rs
   - Type: modify
7. Replace direct `download_list()` calls in TUI with `PlayerCmd::PodcastDownloadEpisodes` sends
   - Files: tui/src/ui/components/podcast.rs
   - Type: modify
8. Verify OPML import/export routes through server (or confirm they already do)
   - Files: tui/src/ui/components/podcast.rs, server/src/server.rs
   - Type: modify

### Acceptance Criteria

- TUI contains zero direct invocations of `check_feed()` or `download_list()` (AC-02)
- Server handles all podcast network operations (AC-01)
- Manual podcast refresh produces identical results from user perspective (AC-03, SCENARIO-003)
- OPML import/export works identically (SCENARIO-004)
- All existing tests pass after migration

### Risks

- TUI podcast UI components depend on synchronous results from `check_feed()` — may need to adapt to async command-response pattern via StreamUpdates

---

## Phase 2: Architecture and Config Redesign

- **Domain**: backend
- **Effort**: medium
- **Objective**: Move sync config under `[podcast.synchronization]`, add per-podcast scheduling infrastructure (DB migration, update_last_checked, get_due_podcasts), and add UpdatePodcastSync protobuf messages.
- **Depends On**: Phase 1
- **Parallelizable With**: None

### Scope

**In scope**: Config struct restructuring, SynchronizationSettings Default changes, AutoEnqueue enum, database 002.sql migration, update_last_checked function, get_due_podcasts query, protobuf sub-message addition, UpdatePodcastSyncEvents Rust enum.

**Out of scope**: sync_once logic changes, TaskPool sharing, pre-scan refactor, test cleanup.

### Tasks

1. Change `SynchronizationSettings::default()` to set `interval = Duration::ZERO` and `refresh_on_startup = false`
   - Files: lib/src/config/v2/server/synchronization.rs
   - Type: modify
2. Add `AutoEnqueue` enum (Enabled, Disabled) with serde rename_all and Default impl
   - Files: lib/src/config/v2/server/synchronization.rs
   - Type: modify
3. Add `auto_enqueue: AutoEnqueue` field to `SynchronizationSettings`
   - Files: lib/src/config/v2/server/synchronization.rs
   - Type: modify
4. Move `synchronization` field from `ServerSettings` to `PodcastSettings`
   - Files: lib/src/config/v2/server/mod.rs
   - Type: modify
5. Update config access paths in `server/src/podcast_sync.rs` from `config.synchronization` to `config.podcast.synchronization`
   - Files: server/src/podcast_sync.rs
   - Type: modify
6. Update config access paths in `server/src/server.rs`
   - Files: server/src/server.rs
   - Type: modify
7. Create `lib/src/podcast/db/migrations/002.sql` with ALTER TABLE ADD COLUMN check_interval
   - Files: lib/src/podcast/db/migrations/002.sql
   - Type: create
8. Update `lib/src/podcast/db/migration.rs` to apply 002.sql when user_version < 2
   - Files: lib/src/podcast/db/migration.rs
   - Type: modify
9. Add standalone `update_last_checked(id, timestamp, conn)` function to `podcast_db.rs`
   - Files: lib/src/podcast/db/podcast_db.rs
   - Type: modify
10. Add `get_due_podcasts(global_interval_secs, conn)` function to `podcast_db.rs`
    - Files: lib/src/podcast/db/podcast_db.rs
    - Type: modify
11. Re-export `update_last_checked` and `get_due_podcasts` from `lib/src/podcast/db/mod.rs`
    - Files: lib/src/podcast/db/mod.rs
    - Type: modify
12. Add `UpdatePodcastSync` message and sub-messages to `lib/proto/player.proto`
    - Files: lib/proto/player.proto
    - Type: modify
13. Add `UpdatePodcastSyncEvents` enum and `PodcastSyncCompleteStats` struct to `lib/src/player.rs`
    - Files: lib/src/player.rs
    - Type: modify
14. Add `PodcastSync(UpdatePodcastSyncEvents)` variant to `UpdateEvents` enum
    - Files: lib/src/player.rs
    - Type: modify
15. Implement `From<UpdatePodcastSyncEvents>` for protobuf `UpdatePodcastSync` type
    - Files: lib/src/player.rs
    - Type: modify
16. Update `synchronization_tests.rs` for new defaults (interval=ZERO, refresh_on_startup=false)
    - Files: lib/src/config/v2/server/synchronization_tests.rs
    - Type: modify

### Acceptance Criteria

- Config section parses from `[podcast.synchronization]` in TOML (AC-04, SCENARIO-006)
- Absent config section means sync disabled: interval defaults to Duration::ZERO (AC-05, SCENARIO-007, SCENARIO-008)
- `refresh_on_startup` can be explicitly disabled (AC-06, SCENARIO-009)
- Duration default values have human-readable comments in source (AC-07)
- Database migration applies cleanly on existing databases (002.sql)
- `update_last_checked` writes correct timestamp (SCENARIO-010)
- `get_due_podcasts` returns only podcasts whose elapsed time exceeds their effective interval (SCENARIO-011, SCENARIO-012, SCENARIO-013)
- Per-podcast `check_interval` override is respected by `get_due_podcasts` (AC-09)
- Protobuf compiles successfully with new UpdatePodcastSync messages
- All existing tests pass

### Risks

- Protobuf field number 9 may conflict with other in-flight changes to player.proto (low likelihood given single-developer workflow)

---

## Phase 3: Sync Logic Correctness

- **Domain**: backend
- **Effort**: large
- **Objective**: Fix all sync_once logic issues: shared TaskPool, PodcastUrl track source, pre-scan filesystem outside async, configurable auto-enqueue, episode filtering, helper extraction, combined interval_at path.
- **Depends On**: Phase 2
- **Parallelizable With**: None

### Scope

**In scope**: Rewriting `sync_once` to use per-podcast scheduling, shared TaskPool, spawn_blocking pre-scan, PodcastUrl source, auto-enqueue gating, played+deleted filtering, create_podcast_dir reuse, helper extraction, combined refresh_on_startup logic.

**Out of scope**: Test suite cleanup (Phase 4), style fixes (Phase 5).

### Tasks

1. Create single shared `TaskPool` before the podcast processing loop (not per-podcast)
   - Files: server/src/podcast_sync.rs
   - Type: modify
2. Add `spawn_blocking` pre-scan that builds `ExistingFilesMap` before async loop
   - Files: server/src/podcast_sync.rs
   - Type: modify
3. Replace `get_podcasts()` with `get_due_podcasts(global_interval_secs)` in sync_once
   - Files: server/src/podcast_sync.rs
   - Type: modify
4. Replace `PlaylistTrackSource::Path` with `PlaylistTrackSource::PodcastUrl` for all enqueue operations
   - Files: server/src/podcast_sync.rs
   - Type: modify
5. Add `should_download_episode` helper function using pre-scanned HashSet and episode.played field
   - Files: server/src/podcast_sync.rs
   - Type: modify
6. Implement filename derivation from episode title via `sanitize_filename` (matching create_podcast_dir options)
   - Files: server/src/podcast_sync.rs
   - Type: modify
7. Replace reimplemented sanitize+create_dir logic with `create_podcast_dir(&config.read(), podcast.title.clone())`
   - Files: server/src/podcast_sync.rs
   - Type: modify
8. Add auto-enqueue gating: check `sync_config.auto_enqueue == AutoEnqueue::Enabled` before enqueue
   - Files: server/src/podcast_sync.rs
   - Type: modify
9. Sort episodes oldest-first by pubdate before enqueueing (per-podcast groups)
   - Files: server/src/podcast_sync.rs
   - Type: modify
10. Call `update_last_checked(pod_id, Utc::now(), conn)` on both success and failure paths
    - Files: server/src/podcast_sync.rs
    - Type: modify
11. Extract `process_feed_result` helper function from sync_once inner logic
    - Files: server/src/podcast_sync.rs
    - Type: modify
12. Extract `find_episodes_to_download` helper function
    - Files: server/src/podcast_sync.rs
    - Type: modify
13. Extract `drain_download_results` helper function
    - Files: server/src/podcast_sync.rs
    - Type: modify
14. Combine refresh_on_startup + periodic loop into single `interval_at` with conditional start time
    - Files: server/src/podcast_sync.rs
    - Type: modify
15. Add MINIMUM_SYNC_INTERVAL constant and clamp logic
    - Files: server/src/podcast_sync.rs
    - Type: modify
16. Ensure download operations are dispatched as separate tasks (non-blocking to feed processing)
    - Files: server/src/podcast_sync.rs
    - Type: modify
17. Send `UpdatePodcastSync::Complete` via broadcast channel after sync pass
    - Files: server/src/podcast_sync.rs
    - Type: modify
18. Ensure `new_append_single`/`new_append_vec` delegate to base constructors with AT_END sentinel (verify existing code or fix)
    - Files: lib/src/player.rs
    - Type: modify

### Acceptance Criteria

- Single shared TaskPool bounds all concurrent network operations (AC-10, SCENARIO-014)
- Auto-enqueue can be disabled (AC-11, SCENARIO-015)
- Episodes enqueued oldest-first per podcast, contiguous groups (AC-12, SCENARIO-016, SCENARIO-017)
- Played+deleted episodes excluded (AC-13, SCENARIO-018, SCENARIO-019, SCENARIO-020)
- PodcastUrl source used for all enqueued episodes (AC-14, SCENARIO-021)
- No blocking I/O in async context (AC-15, SCENARIO-022, SCENARIO-023)
- Downloads do not block feed processing (AC-16, SCENARIO-024)
- `create_podcast_dir` utility reused (AC-17, SCENARIO-025)
- Append helpers delegate to base constructors (AC-18, SCENARIO-026)
- Single code path for immediate + periodic sync (AC-19, SCENARIO-027)
- Empty podcast list handled gracefully (SCENARIO-036)
- Zero new episodes updates last_checked without error (SCENARIO-037)
- Timeout isolates to single podcast (SCENARIO-039)
- All download failures still advance last_checked (SCENARIO-041)
- Missing directory created via utility (SCENARIO-042)

### Risks

- Rewriting sync_once is a large change — risk of introducing regressions in edge cases
- Pre-scan timing: if a file is deleted between pre-scan and download check, a redundant download may occur (acceptable — idempotent operation)

---

## Phase 4: Test Quality

- **Domain**: testing
- **Effort**: medium
- **Objective**: Remove redundant tests, create TestHarness builder, fix test URLs to localhost, fix error assertions to check specific variants, use indoc for multiline strings, replace abbreviations in test names.
- **Depends On**: Phase 3
- **Parallelizable With**: Phase 5

### Scope

**In scope**: Test suite cleanup in `server/src/podcast_sync.rs` test modules and `lib/src/config/v2/server/synchronization_tests.rs`. Removing tests that verify Rust language semantics, consolidating duplicates, creating shared test helpers.

**Out of scope**: Adding new feature tests (those are part of Phase 3), production code changes.

### Tasks

1. Remove tests verifying struct field existence and derive traits (sync_pass_stats_struct_has_required_fields, sync_pass_stats_all_zeros, sync_pass_stats_implements_debug)
   - Files: server/src/podcast_sync.rs
   - Type: modify
2. Remove tests verifying function signature types (sync_once_accepts_expected_parameters, sync_once_returns_anyhow_result_of_sync_pass_stats)
   - Files: server/src/podcast_sync.rs
   - Type: modify
3. Consolidate duplicate tests (same assertion under different names)
   - Files: server/src/podcast_sync.rs
   - Type: modify
4. Create `TestHarness` struct with builder pattern for common test setup (MockServer, Database, config, channel)
   - Files: server/src/podcast_sync.rs
   - Type: modify
5. Refactor integration tests to use TestHarness instead of repeated inline setup
   - Files: server/src/podcast_sync.rs
   - Type: modify
6. Replace all external test URLs with localhost/127.0.0.1 addresses
   - Files: server/src/podcast_sync.rs
   - Type: modify
7. Fix error assertion tests to check specific error variant or message (not just is_err())
   - Files: server/src/podcast_sync.rs
   - Type: modify
8. Add `indoc` usage for multiline string literals in tests
   - Files: server/src/podcast_sync.rs, lib/src/config/v2/server/synchronization_tests.rs
   - Type: modify
9. Replace abbreviations in test names with full descriptive names
   - Files: server/src/podcast_sync.rs
   - Type: modify

### Acceptance Criteria

- Zero tests verify basic Rust language semantics (AC-20, SCENARIO-028)
- No duplicate tests remain (AC-21)
- All test URLs use localhost or 127.0.0.1 (AC-22, SCENARIO-029)
- Error tests assert specific variants or messages (AC-23, SCENARIO-030)
- Multiline strings use indoc (AC-24)
- No unexplained abbreviations in test names (AC-25)
- TestHarness eliminates boilerplate (AC-26, SCENARIO-031)
- Tests verify observable outcomes via spy channels (AC-27, SCENARIO-032)

### Risks

- Removing tests may accidentally remove one that caught a real edge case — verify each removed test adds no unique coverage before deletion

---

## Phase 5: Style and Conventions

- **Domain**: backend
- **Effort**: small
- **Objective**: Apply code style fixes: module doc comments, helper extraction for deep nesting, config struct references in function signatures.
- **Depends On**: Phase 3
- **Parallelizable With**: Phase 4

### Scope

**In scope**: Converting module comments to `//!` doc comments, extracting deeply nested logic (already partially done in Phase 3 helper extraction), refactoring function signatures to accept config struct references.

**Out of scope**: Functional logic changes (those are in Phase 3).

### Tasks

1. Convert `server/src/podcast_sync.rs` module-level comments to `//!` doc comment format
   - Files: server/src/podcast_sync.rs
   - Type: modify
2. Verify all extracted helpers reduce nesting to 3 levels maximum
   - Files: server/src/podcast_sync.rs
   - Type: modify
3. Refactor any function signatures that accept multiple individual config values to accept config struct references
   - Files: server/src/podcast_sync.rs
   - Type: modify
4. Ensure commit messages follow project scope format (feat/fix(podcast-sync): ...)
   - Files: (no file change — process requirement)
   - Type: modify

### Acceptance Criteria

- Module documentation uses `//!` doc comments (AC-29, SCENARIO-033)
- No nesting exceeds 3 levels (AC-30, SCENARIO-034)
- Functions accept config struct references not individual values (AC-31, SCENARIO-035)
- Commit messages follow `feat(podcast-sync):` / `fix(podcast-sync):` format (AC-28)

### Risks

- Minimal risk — these are cosmetic/style changes that do not affect functionality

---

## Cross-Cutting Concerns

### Error Handling Consistency

All phases must maintain the project's error handling pattern:
- Fatal errors (DB open failure): propagate via `anyhow::Result` with `.context()`
- Per-entity errors (feed fetch, download): `warn!` log, continue processing
- Channel send errors: `warn!` log, non-blocking

### Backward Compatibility

Every phase must leave the system in a backward-compatible state:
- Phase 1: TUI behavior unchanged from user perspective
- Phase 2: Absent config means disabled (no behavior change for existing users)
- Phase 3: New behavior only activates when user explicitly configures non-zero interval
- Phase 4/5: No behavioral changes (test/style only)

### Compilation Guarantee

Each phase must compile and pass all existing tests before the next phase begins. No phase leaves the codebase in a broken state.

## Milestone Summary

- **Phase 1 Complete**: Server owns all podcast operations — Deliverable: TUI sends commands, server handles them, Verification: zero direct check_feed/download_list calls in TUI crate
- **Phase 2 Complete**: Architecture redesigned — Deliverable: config nested, DB migrated, proto extended, Verification: `cargo build` succeeds, existing tests pass, new unit tests for config defaults
- **Phase 3 Complete**: Sync logic correct — Deliverable: full sync_once rewrite with all correctness fixes, Verification: integration tests pass with wiremock, all SCENARIO references validated
- **Phase 4 Complete**: Test suite clean — Deliverable: reduced test count, meaningful assertions only, Verification: `cargo test` passes, no redundant tests remain
- **Phase 5 Complete**: Style compliance — Deliverable: doc comments, reduced nesting, config struct refs, Verification: code review checklist passes
