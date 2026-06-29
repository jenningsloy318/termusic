# Task List: PR #720 Podcast Synchronization — Review Feedback Remediation

- **Date**: 2026-06-25
- **Author**: super-dev:spec-writer
- **Specification**: ./09-specification.md
- **Implementation Plan**: ./10-implementation-plan.md
- **Total Tasks**: 47

---

## Phase 1: Prerequisites and Migration

**Milestone**: Server owns all podcast network operations; TUI delegates via PlayerCmd.

- [ ] **T-01**: Add `PodcastFeedRefresh` variant to `PlayerCmd` enum
  - Files: playback/src/lib.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-01, SCENARIO-001

- [ ] **T-02**: Add `PodcastDownloadEpisodes(Vec<EpisodeDownloadRequest>)` variant to `PlayerCmd` enum
  - Files: playback/src/lib.rs
  - Type: modify
  - Effort: small
  - Depends on: T-01
  - AC: AC-01, SCENARIO-001

- [ ] **T-03**: Define `EpisodeDownloadRequest` struct with podcast_id, episode_url, episode_title fields
  - Files: playback/src/lib.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-01

- [ ] **T-04**: Add handler for `PlayerCmd::PodcastFeedRefresh` in server player loop that calls `check_feed()`
  - Files: server/src/server.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-01
  - AC: AC-01, SCENARIO-001, SCENARIO-005

- [ ] **T-05**: Add handler for `PlayerCmd::PodcastDownloadEpisodes` in server player loop that calls `download_list()`
  - Files: server/src/server.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-02, T-03
  - AC: AC-01, SCENARIO-001

- [ ] **T-06**: Replace direct `check_feed()` call in TUI podcast component with `PlayerCmd::PodcastFeedRefresh` send
  - Files: tui/src/ui/components/podcast.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-04
  - AC: AC-02, SCENARIO-002, SCENARIO-003

- [ ] **T-07**: Replace direct `download_list()` calls in TUI podcast component with `PlayerCmd::PodcastDownloadEpisodes` sends
  - Files: tui/src/ui/components/podcast.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-05
  - AC: AC-02, SCENARIO-002

- [ ] **T-08**: Verify OPML import/export routes through server correctly (or confirm no change needed)
  - Files: tui/src/ui/components/podcast.rs, server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-06, T-07
  - AC: AC-03, SCENARIO-004

---

## Phase 2: Architecture and Config Redesign

**Milestone**: Config nested under [podcast.synchronization], DB migrated with check_interval column, proto extended with UpdatePodcastSync.

- [ ] **T-09**: Change `SynchronizationSettings::default()` interval from 3600s to `Duration::ZERO` (disabled by default)
  - Files: lib/src/config/v2/server/synchronization.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-05, SCENARIO-007, SCENARIO-008

- [ ] **T-10**: Change `SynchronizationSettings::default()` refresh_on_startup from `true` to `false`
  - Files: lib/src/config/v2/server/synchronization.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-06, SCENARIO-009

- [ ] **T-11**: Add `AutoEnqueue` enum (Enabled, Disabled) with `#[serde(rename_all = "lowercase")]` and Default impl returning Enabled
  - Files: lib/src/config/v2/server/synchronization.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-11

- [ ] **T-12**: Add `auto_enqueue: AutoEnqueue` field to `SynchronizationSettings` struct
  - Files: lib/src/config/v2/server/synchronization.rs
  - Type: modify
  - Effort: small
  - Depends on: T-11
  - AC: AC-11, SCENARIO-015

- [ ] **T-13**: Add human-readable comments on all duration/numeric constants in Default impl
  - Files: lib/src/config/v2/server/synchronization.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09
  - AC: AC-07

- [ ] **T-14**: Move `synchronization: SynchronizationSettings` field from `ServerSettings` to `PodcastSettings`
  - Files: lib/src/config/v2/server/mod.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09, T-10, T-11, T-12
  - AC: AC-04, SCENARIO-006

- [ ] **T-15**: Update config access path in `server/src/podcast_sync.rs` from `config.synchronization` to `config.podcast.synchronization`
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: T-14
  - AC: AC-04

- [ ] **T-16**: Update config access path in `server/src/server.rs`
  - Files: server/src/server.rs
  - Type: modify
  - Effort: small
  - Depends on: T-14
  - AC: AC-04

- [ ] **T-17**: Create `lib/src/podcast/db/migrations/002.sql` with ALTER TABLE podcasts ADD COLUMN check_interval INTEGER
  - Files: lib/src/podcast/db/migrations/002.sql
  - Type: create
  - Effort: small
  - Depends on: None
  - AC: AC-09

- [ ] **T-18**: Update `lib/src/podcast/db/migration.rs` to apply 002.sql when user_version < 2 and set user_version to 2
  - Files: lib/src/podcast/db/migration.rs
  - Type: modify
  - Effort: small
  - Depends on: T-17
  - AC: AC-09

- [ ] **T-19**: Add standalone `update_last_checked(id, timestamp, conn)` function to `podcast_db.rs`
  - Files: lib/src/podcast/db/podcast_db.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-08, SCENARIO-010, SCENARIO-041

- [ ] **T-20**: Add `get_due_podcasts(global_interval_secs, conn)` function with COALESCE SQL query
  - Files: lib/src/podcast/db/podcast_db.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-18
  - AC: AC-08, AC-09, SCENARIO-011, SCENARIO-012, SCENARIO-013

- [ ] **T-21**: Re-export `update_last_checked` and `get_due_podcasts` from `lib/src/podcast/db/mod.rs`
  - Files: lib/src/podcast/db/mod.rs
  - Type: modify
  - Effort: small
  - Depends on: T-19, T-20
  - AC: AC-08, AC-09

- [ ] **T-22**: Add `UpdatePodcastSync` message with inner oneof (started/progress/complete/error) to `player.proto`
  - Files: lib/proto/player.proto
  - Type: modify
  - Effort: medium
  - Depends on: None
  - AC: SCENARIO-005

- [ ] **T-23**: Add `UpdatePodcastSyncEvents` enum and `PodcastSyncCompleteStats` struct to `lib/src/player.rs`
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-22
  - AC: SCENARIO-005

- [ ] **T-24**: Add `PodcastSync(UpdatePodcastSyncEvents)` variant to `UpdateEvents` enum and implement From conversions
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-23
  - AC: SCENARIO-005

- [ ] **T-25**: Update `synchronization_tests.rs` for new defaults (interval=ZERO means disabled, refresh_on_startup=false)
  - Files: lib/src/config/v2/server/synchronization_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-09, T-10, T-11, T-12, T-14
  - AC: AC-05, AC-06, SCENARIO-007, SCENARIO-008, SCENARIO-009

---

## Phase 3: Sync Logic Correctness

**Milestone**: sync_once fully rewritten with all correctness fixes — shared TaskPool, pre-scan, PodcastUrl, auto-enqueue, helper extraction.

- [ ] **T-26**: Replace per-podcast TaskPool with single shared TaskPool created before podcast processing loop
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: None
  - AC: AC-10, SCENARIO-014, SCENARIO-038

- [ ] **T-27**: Add `spawn_blocking` pre-scan that builds `ExistingFilesMap` (HashMap of PodcastDBId to HashSet of filenames) before async loop
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: None
  - AC: AC-15, SCENARIO-022, SCENARIO-023

- [ ] **T-28**: Replace `get_podcasts()` call with `get_due_podcasts(global_interval_secs)` in sync_once
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-08, AC-09, SCENARIO-011

- [ ] **T-29**: Replace all `PlaylistTrackSource::Path(...)` with `PlaylistTrackSource::PodcastUrl(episode.url.clone())` for enqueue operations
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-14, SCENARIO-021

- [ ] **T-30**: Implement `should_download_episode(episode, existing_filenames, expected_filename)` helper using pre-scanned HashSet and episode.played field
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-27
  - AC: AC-13, SCENARIO-018, SCENARIO-019, SCENARIO-020

- [ ] **T-31**: Implement filename derivation from episode title via `sanitize_filename` (matching create_podcast_dir sanitization options)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-13

- [ ] **T-32**: Replace reimplemented sanitize+create_dir logic with `create_podcast_dir(&config.read(), podcast.title.clone())`
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-17, SCENARIO-025, SCENARIO-042

- [ ] **T-33**: Add auto-enqueue gating: check `sync_config.auto_enqueue == AutoEnqueue::Enabled` before enqueue block
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-11, SCENARIO-015, SCENARIO-016

- [ ] **T-34**: Sort episodes oldest-first by pubdate before enqueueing, keeping per-podcast groups contiguous
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: T-33
  - AC: AC-12, SCENARIO-016, SCENARIO-017

- [ ] **T-35**: Call `update_last_checked(pod_id, Utc::now(), conn)` on both success path (after update_podcast) and failure path (after feed error)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-08, SCENARIO-010, SCENARIO-039, SCENARIO-041

- [ ] **T-36**: Extract `process_feed_result` helper function handling SyncData match arm logic
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-26, T-27, T-28, T-29, T-30, T-31, T-32, T-33, T-34, T-35
  - AC: AC-30

- [ ] **T-37**: Extract `find_episodes_to_download` helper function with max_new_episodes limit
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: T-30, T-31
  - AC: AC-30

- [ ] **T-38**: Extract `drain_download_results` helper function handling download channel draining
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: T-29, T-33, T-34
  - AC: AC-16, AC-30, SCENARIO-024

- [ ] **T-39**: Combine refresh_on_startup + periodic loop into single `interval_at` with conditional start time (Instant::now vs Instant::now + interval)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-19, SCENARIO-027

- [ ] **T-40**: Add `MINIMUM_SYNC_INTERVAL` constant (1 second) and apply `.max(MINIMUM_SYNC_INTERVAL)` clamp on interval
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-05

- [ ] **T-41**: Ensure download_list dispatches run as separate async tasks via shared TaskPool (non-blocking to feed_rx drain)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-26
  - AC: AC-16, SCENARIO-024

- [ ] **T-42**: Send `UpdatePodcastSync::Complete` via broadcast channel after sync_once returns
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: SCENARIO-005

- [ ] **T-43**: Verify `new_append_single`/`new_append_vec` in lib/src/player.rs correctly delegate to base constructor with AT_END (confirm existing implementation is correct)
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-18, SCENARIO-026

---

## Phase 4: Test Quality

**Milestone**: Clean test suite with meaningful assertions, shared TestHarness, localhost-only URLs.

- [ ] **T-44**: Remove redundant tests verifying struct derives and function signatures (5 tests identified: sync_pass_stats_struct_has_required_fields, sync_pass_stats_all_zeros, sync_pass_stats_implements_debug, sync_once_accepts_expected_parameters, sync_once_returns_anyhow_result_of_sync_pass_stats)
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-20, SCENARIO-028

- [ ] **T-45**: Create `TestHarness` struct with builder pattern (MockServer, Database, config, cmd channel) and refactor integration tests to use it; replace all external test URLs with localhost/127.0.0.1; fix error assertions to check specific variants; use indoc for multiline strings; replace abbreviations in test names
  - Files: server/src/podcast_sync.rs, lib/src/config/v2/server/synchronization_tests.rs
  - Type: modify
  - Effort: large
  - Depends on: T-44
  - AC: AC-21, AC-22, AC-23, AC-24, AC-25, AC-26, AC-27, SCENARIO-029, SCENARIO-030, SCENARIO-031, SCENARIO-032

---

## Phase 5: Style and Conventions

**Milestone**: All modules pass style review — doc comments, nesting limits, config struct references.

- [ ] **T-46**: Convert `server/src/podcast_sync.rs` module-level comments to `//!` doc comment format
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-29, SCENARIO-033

- [ ] **T-47**: Refactor any function signatures accepting multiple individual config values to accept `&SynchronizationSettings` or `&PodcastSettings` struct references; verify nesting does not exceed 3 levels after Phase 3 helper extraction
  - Files: server/src/podcast_sync.rs
  - Type: modify
  - Effort: small
  - Depends on: None
  - AC: AC-30, AC-31, SCENARIO-034, SCENARIO-035

---

## Summary

- Phase 1: Prerequisites and Migration — 8 tasks, large effort
- Phase 2: Architecture and Config Redesign — 17 tasks, medium effort
- Phase 3: Sync Logic Correctness — 18 tasks, large effort
- Phase 4: Test Quality — 2 tasks, medium effort
- Phase 5: Style and Conventions — 2 tasks, small effort
- **Total**: 47 tasks, large overall effort
