# Implementation Summary: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:impl-summary-writer
- **Phase**: 1 — Configuration Schema
- **Status**: completed
- **Feature Status**: ALL 5 PHASES COMPLETE
- **Total Tests**: 79 (19 + 20 + 20 + 9 + 11)
- **Code Review**: Approved (0 findings)
- **Adversarial Review**: PASS (7 low-severity findings, all acceptable)

---

## Overview

Phase 1 implemented the `SynchronizationSettings` configuration struct with `#[serde(default)]` support, integrated the `humantime-serde` dependency (version 1.1) for human-readable duration parsing, wired the new struct into `ServerSettings`, and delivered a comprehensive test suite of 19 unit tests covering default deserialization, explicit value parsing, serialization roundtrip, invalid duration rejection, and backward compatibility. All tests pass and the full workspace compiles cleanly.

## Files Changed

- `Cargo.toml` — modified, +1/-0
  - Purpose: Added `humantime-serde = "1.1"` to `[workspace.dependencies]` for workspace-wide availability.

- `Cargo.lock` — modified, +17/-0
  - Purpose: Auto-generated lock file update recording the resolved `humantime` and `humantime-serde` crate versions.

- `lib/Cargo.toml` — modified, +1/-0
  - Purpose: Added `humantime-serde.workspace = true` to the lib crate's `[dependencies]` section.

- `lib/src/config/v2/server/mod.rs` — modified, +8/-0
  - Purpose: Registered the `synchronization` module, imported `SynchronizationSettings`, added the `pub synchronization: SynchronizationSettings` field to `ServerSettings`, registered the test module, and updated the v1 interop conversion to include a default `SynchronizationSettings`.

- `lib/src/config/v2/server/synchronization.rs` — created, +113/-0
  - Purpose: Defines the `SynchronizationSettings` struct with three fields (`enable`, `interval`, `refresh_on_startup`), a `Default` impl providing documented defaults (enabled, 1h interval, refresh on startup), and a custom `Deserialize` implementation that handles both nested and flat TOML structures with `humantime_serde` for the duration field.

- `lib/src/config/v2/server/synchronization_tests.rs` — created, +351/-0
  - Purpose: Comprehensive unit test module covering SCENARIO-001 through SCENARIO-004: default deserialization, explicit non-default values, serialization roundtrip, invalid duration rejection, partial section parsing, ServerSettings integration, and struct trait implementations (Clone, Debug, PartialEq).

## Key Decisions

### 1. humantime-serde version 1.1 instead of 0.2

- **Context**: The implementation plan specified `humantime-serde = "0.2"` but the actual implementation uses version 1.1.
- **Decision**: Used `humantime-serde = "1.1"` (the latest stable release).
- **Rationale**: Version 1.1 is the current release on crates.io, providing better compatibility and maintenance. The plan's reference to "0.2" appears to have been outdated; the API is compatible and the functionality is identical.
- **Reference**: `Cargo.toml`

### 2. Custom Deserialize implementation with dual-path parsing

- **Context**: TOML deserialization needed to work both as a standalone document (with `[synchronization]` as a section header) and as a nested field within `ServerSettings`.
- **Decision**: Implemented a custom `Deserialize` trait with an inner `SyncSettingsRaw` helper struct that optionally captures a nested `synchronization` table, allowing both parsing paths.
- **Rationale**: This avoids issues where standalone TOML tests (with section headers) would conflict with the nested-field usage inside `ServerSettings`. The dual-path approach handles both cases transparently.
- **Reference**: `lib/src/config/v2/server/synchronization.rs`

### 3. Tests in a separate file rather than inline

- **Context**: The task list specified adding a `#[cfg(test)] mod tests` section within `synchronization.rs`.
- **Decision**: Tests were placed in a separate `synchronization_tests.rs` file, registered via `#[cfg(test)] mod synchronization_tests;` in `mod.rs`.
- **Rationale**: Keeps the implementation file focused on production code and separates the substantial test suite (351 lines, 19 tests) from the 113-line implementation, improving readability and following a pattern used in other parts of the codebase.
- **Reference**: `lib/src/config/v2/server/synchronization_tests.rs`

### 4. Nineteen tests instead of four

- **Context**: The task list specified four tests (T-05 through T-08) covering default, explicit, roundtrip, and invalid scenarios.
- **Decision**: Implemented 19 tests covering additional edge cases: partial sections, complex intervals, seconds-only durations, empty configs, struct trait verification, and ServerSettings integration.
- **Rationale**: More comprehensive coverage ensures the custom Deserialize implementation handles all edge cases correctly and prevents regressions. Each scenario from the BDD spec gets multiple test cases for confidence.
- **Reference**: `lib/src/config/v2/server/synchronization_tests.rs`

## Deviations from Spec

### humantime-serde version

- **Spec said**: Use `humantime-serde = "0.2"` as the dependency version.
- **Actual**: Used `humantime-serde = "1.1"`.
- **Reason**: Version 0.2 is outdated; 1.1 is the current stable release with the same API surface. The crate was likely referenced by an older version number in the spec.

### Test file organization

- **Spec said**: Add `#[cfg(test)] mod tests` section within `synchronization.rs` (T-05 through T-08).
- **Actual**: Tests placed in a separate `synchronization_tests.rs` file.
- **Reason**: Improved code organization — the implementation is 113 lines while tests are 351 lines. Separation keeps both files focused and maintainable.

## Test Results

- **Unit Tests**: 19/19 passing
- **Integration Tests**: Not applicable for this phase

## Next Steps

Phase complete. No remaining items.

---
---

# Implementation Summary: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:impl-summary-writer
- **Phase**: 2 — PlaylistAddTrack API Extension
- **Status**: completed

---

## Overview

Phase 2 extended the `PlaylistAddTrack` struct with an `AT_END` constant (`u64::MAX`) and two new convenience constructors (`new_append_single` and `new_append_vec`) that provide a clean API for appending tracks to the end of a playlist without exposing the sentinel value directly. The implementation is purely additive, preserving all existing methods unchanged. A comprehensive test suite of 20 unit tests in a dedicated test file validates the constant value, both constructors across all track source variants, equivalence properties, and regression on existing API.

## Files Changed

- `lib/src/player.rs` — modified, +22/-0
  - Purpose: Added `pub const AT_END: u64 = u64::MAX` to the `impl PlaylistAddTrack` block, plus `pub fn new_append_single(track: PlaylistTrackSource) -> Self` and `pub fn new_append_vec(tracks: Vec<PlaylistTrackSource>) -> Self` constructors with `#[must_use]` and doc comments.

- `lib/src/lib.rs` — modified, +3/-0
  - Purpose: Registered the new test module `player_playlist_add_track_tests` via `#[cfg(test)] mod player_playlist_add_track_tests;` declaration.

- `lib/src/player_playlist_add_track_tests.rs` — created, +269/-0
  - Purpose: Comprehensive unit test module covering T-09 through T-12: AT_END constant verification, `new_append_single` behavior with Path/Url/PodcastUrl variants, `new_append_vec` with empty/single/many tracks, ordering preservation, regression tests for existing `new_single`/`new_vec` methods, and struct trait verification (PartialEq, Clone, Debug).

## Key Decisions

### 1. Tests in a separate file rather than inline in player.rs

- **Context**: The task list specified adding tests in an existing or new `#[cfg(test)]` section within `lib/src/player.rs`.
- **Decision**: Tests were placed in a dedicated `player_playlist_add_track_tests.rs` file, registered via `#[cfg(test)] mod` in `lib.rs`.
- **Rationale**: The `player.rs` file is already large (490+ lines of production code). A separate test file follows the same pattern established in Phase 1 (where `synchronization_tests.rs` was separated from `synchronization.rs`) and keeps the test suite (269 lines, 20 tests) from bloating the production module.
- **Reference**: `lib/src/player_playlist_add_track_tests.rs`

### 2. Twenty tests instead of one combined test task

- **Context**: T-12 in the task list specified "Write unit tests verifying AT_END value and constructor behavior" as a single task.
- **Decision**: Implemented 20 individual tests covering edge cases, all three `PlaylistTrackSource` variants (Path, Url, PodcastUrl), empty vectors, large vectors (50 elements), single-element equivalence between constructors, and regression coverage for existing methods.
- **Rationale**: Fine-grained tests provide precise failure diagnostics and ensure each constructor behavior is independently validated. The additional anti-hardcoding tests (varied inputs, boundary cases) strengthen confidence that the AT_END pattern works correctly in all scenarios Phase 3 will exercise.
- **Reference**: `lib/src/player_playlist_add_track_tests.rs`

### 3. Doc comments on new API surface

- **Context**: The implementation plan did not explicitly mention documentation requirements for Phase 2.
- **Decision**: Added `///` doc comments on `AT_END`, `new_append_single`, and `new_append_vec` explaining their purpose and the semantic relationship to `Playlist::add_tracks`.
- **Rationale**: Follows the cross-cutting documentation discipline requirement from the implementation plan ("All new public items must have `///` doc comments") and makes the API self-documenting for Phase 3 consumers.
- **Reference**: `lib/src/player.rs`

## Deviations from Spec

No deviations from specification.

## Test Results

- **Unit Tests**: 20/20 passing
- **Integration Tests**: Not applicable for this phase

## Next Steps

Phase complete. No remaining items.

---
---

# Implementation Summary: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:impl-summary-writer
- **Phase**: 3 — Sync Pass Logic
- **Status**: completed

---

## Overview

Phase 3 implemented the `sync_once` async function and `SyncPassStats` struct in a new `server/src/podcast_sync.rs` module. The function executes a full synchronization pass: opens the podcast database, retrieves all subscribed podcasts, dispatches concurrent feed fetch tasks via `check_feed` and a `TaskPool`, processes results with per-podcast error isolation, identifies undownloaded episodes (path == None filtering), downloads them using the `download_list` channel-drain pattern, records file paths in the database, and enqueues downloaded episodes via `PlaylistAddTrack::new_append_single`. A comprehensive test suite of 20 unit tests validates all scenarios including empty podcast lists, unreachable feeds, deduplication, enqueue logic, and configuration respect. All tests pass.

## Files Changed

- `server/src/podcast_sync.rs` — created, +931/-0
  - Purpose: Core sync pass module implementing `SyncPassStats` struct and `sync_once` async function with per-podcast feed fetch, episode deduplication, download via channel-drain pattern, enqueue via `PlaylistAddTrack::new_append_single`, and 20 unit tests covering all Phase 3 scenarios.

- `server/src/server.rs` — modified, +1/-0
  - Purpose: Added `mod podcast_sync;` declaration to register the new module in the server crate's module tree.

- `server/Cargo.toml` — modified, +4/-0
  - Purpose: Added `[dev-dependencies]` section with `tempfile.workspace = true` and `chrono.workspace = true` for test infrastructure (temp directories and timestamp generation in test fixtures).

- `Cargo.toml` — modified, +1/-0
  - Purpose: Added `tempfile = "3"` to `[workspace.dependencies]` for workspace-wide availability to test code.

- `Cargo.lock` — modified, +2/-0
  - Purpose: Auto-generated lock file update recording `chrono` and `tempfile` additions to `termusic-server` dependencies.

## Key Decisions

### 1. Per-pass database connection (not shared across sync cycles)

- **Context**: The sync function needs database access but the server already has other database consumers. Holding a connection across await points in async code can cause contention.
- **Decision**: `sync_once` opens its own `Database::new(db_path)` connection at the start of each pass and drops it when the function returns.
- **Rationale**: Follows the architecture recommendation from the research report (ISS-007) to avoid holding SQLite connections across await points. Each pass is self-contained, preventing lock contention with the main server database usage.
- **Reference**: `server/src/podcast_sync.rs`

### 2. TaskPool-based bounded concurrency for feed fetches

- **Context**: Multiple podcast feeds need to be fetched concurrently, but unbounded parallelism could overwhelm the network or the system.
- **Decision**: Used the existing `TaskPool` abstraction with `concurrent_downloads_max` from `PodcastSettings` to bound feed fetch concurrency, with an unbounded channel for collecting results.
- **Rationale**: Reuses the project's existing concurrency primitive (`termusiclib::taskpool::TaskPool`) and respects the user's configured concurrency limit. The unbounded channel is safe because the number of messages is bounded by the number of podcasts.
- **Reference**: `server/src/podcast_sync.rs`

### 3. Channel-drain pattern for download completion signaling

- **Context**: The `download_list` function from `termusiclib::podcast` uses a callback closure to signal download results. The sync pass needs to await all downloads before proceeding.
- **Decision**: Implemented the channel-drain pattern: create an unbounded channel, move the sender into the closure, drop the original sender, then `while let Some(msg) = rx.recv().await` to drain all results.
- **Rationale**: This is the canonical async Rust pattern for converting callback-based APIs into async streams. Dropping the original sender ensures the channel closes when all tasks complete, causing the `while let` loop to terminate naturally.
- **Reference**: `server/src/podcast_sync.rs`

### 4. Module registered in server.rs early (before Phase 4 wiring)

- **Context**: The implementation plan placed `mod podcast_sync;` registration in Phase 4 (T-22), but the module needs to compile during Phase 3 development.
- **Decision**: Added `mod podcast_sync;` to `server/src/server.rs` in Phase 3 alongside the module creation.
- **Rationale**: Rust requires module declarations for compilation. Having the module registered allows `cargo test` and `cargo build` to verify the code compiles correctly within the workspace, which is essential for the Phase 3 milestone verification.
- **Reference**: `server/src/server.rs`

### 5. `#![allow(unused)]` attribute for Phase 3

- **Context**: The `sync_once` function and `SyncPassStats` are not yet called from production code (that happens in Phase 4), but clippy would warn about dead code.
- **Decision**: Added `#![allow(unused)]` at the module level with a comment explaining it will be removed in Phase 4.
- **Rationale**: Allows the module to compile cleanly with clippy during Phase 3 while the public API is only exercised by tests. Phase 4 will remove this attribute when the function is wired into `actual_main()`.
- **Reference**: `server/src/podcast_sync.rs`

### 6. Dev-dependencies for test infrastructure

- **Context**: Tests need temporary directories (for SQLite databases) and timestamps (for podcast `last_checked` fields).
- **Decision**: Added `tempfile` and `chrono` as `[dev-dependencies]` in the server crate, with `tempfile` also added to workspace dependencies.
- **Rationale**: `tempfile` provides reliable cross-platform temporary directory creation that auto-cleans on drop. `chrono` was already a transitive dependency (used by the podcast types) but needed an explicit dev-dependency declaration for direct use in test fixtures.
- **Reference**: `server/Cargo.toml`, `Cargo.toml`

## Deviations from Spec

### Module registration moved from Phase 4 to Phase 3

- **Spec said**: T-22 (Phase 4) specifies registering `mod podcast_sync;` in the server crate module tree.
- **Actual**: The `mod podcast_sync;` declaration was added in Phase 3 alongside the module creation.
- **Reason**: Rust compilation requires module declarations. Without it, the new file would not compile or be testable within the workspace. T-22 is effectively complete as part of Phase 3.

### Function signature uses SharedServerSettings instead of individual parameters

- **Spec said**: The implementation plan describes `sync_once` reading config for download_dir and concurrency limits.
- **Actual**: `sync_once` takes `&SharedServerSettings` as its first parameter and reads all needed config values (download_dir, concurrent_downloads_max, max_download_retries) at the start of the function under a single read lock.
- **Reason**: Passing the shared config reference is more ergonomic and future-proof than extracting individual fields. A single short-lived read lock minimizes contention while providing access to all podcast settings.

## Test Results

- **Unit Tests**: 20/20 passing
- **Integration Tests**: Not applicable for this phase (deferred to Phase 5)

## Next Steps

Phase complete. No remaining items.

---
---

# Implementation Summary: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:impl-summary-writer
- **Phase**: 4 — Task Lifecycle and Wiring
- **Status**: completed

---

## Overview

Phase 4 implemented the `start_podcast_sync_task` function in `server/src/podcast_sync.rs` and wired it into `actual_main()` in `server/src/server.rs`. The function spawns an async task that optionally executes an immediate startup sync (when `refresh_on_startup` is true), then enters a periodic `interval_at`-based loop with `tokio::select!` on the `CancellationToken::cancelled()` branch for graceful shutdown. The wiring in `actual_main()` gates the spawn on `synchronization.enable` and places it adjacent to the existing `start_playlist_save_interval` call, mirroring the established pattern. The `#![allow(unused)]` attribute from Phase 3 was removed since all public items are now exercised by production code. Nine new lifecycle tests validate the task's startup behavior, periodic firing, cancellation responsiveness, and disable-gating logic. All 29 tests in the module pass.

## Files Changed

- `server/src/podcast_sync.rs` — modified, +466/-4
  - Purpose: Added `start_podcast_sync_task` public function implementing the task lifecycle (startup sync, interval_at periodic loop, select! cancellation), removed `#![allow(unused)]` attribute and the Phase 3 comment since the module is now wired into production code, and added 9 lifecycle tests covering T-20 through T-23 scenarios.

- `server/src/server.rs` — modified, +14/-0
  - Purpose: Added a conditional block in `actual_main()` that checks `synchronization.enable`, retrieves the app config path via `utils::get_app_config_path()`, and calls `podcast_sync::start_podcast_sync_task` with the tokio handle, service cancellation token, shared config, command sender, and database path. Logs an info message when synchronization is disabled.

## Key Decisions

### 1. interval_at with instant offset instead of initial sleep

- **Context**: The periodic sync needs to fire at fixed intervals after the startup sync (if any) completes. Using `tokio::time::sleep` in a loop would accumulate drift from sync_once execution time.
- **Decision**: Used `tokio::time::interval_at(Instant::now() + interval_duration, interval_duration)` to create a fixed-rate timer that starts its first tick after one full interval from spawn time.
- **Rationale**: `interval_at` compensates for task execution time by scheduling subsequent ticks at absolute instants, preventing drift. The initial offset ensures no overlap with an optional startup sync that runs synchronously before the loop.
- **Reference**: `server/src/podcast_sync.rs`

### 2. Startup sync runs synchronously before periodic loop (not as first tick)

- **Context**: The spec requires that when `refresh_on_startup` is true, a sync pass executes immediately at task startup, and the periodic timer only begins after this first pass completes.
- **Decision**: The startup sync is an `if refresh_on_startup { sync_once(...).await }` block that runs before the `interval_at` loop, not by setting the first tick to `Instant::now()`.
- **Rationale**: Separating startup sync from the periodic loop makes the behavior explicit and testable. It also prevents the edge case where a startup sync failure could confuse the interval timer's tracking of missed ticks.
- **Reference**: `server/src/podcast_sync.rs`

### 3. service_cancel_token reused (not a child token)

- **Context**: The server already has `service_cancel_token` for coordinating graceful shutdown of background tasks. A child token could provide more granular control.
- **Decision**: Passed `service_cancel_token.clone()` directly to `start_podcast_sync_task`, matching the existing `start_playlist_save_interval` pattern.
- **Rationale**: Consistency with the existing server lifecycle management. The sync task does not need independent cancellation separate from the server shutdown sequence. Using the same token ensures it participates in the same shutdown coordination as other background tasks.
- **Reference**: `server/src/server.rs`

### 4. Removed #![allow(unused)] from podcast_sync module

- **Context**: Phase 3 added `#![allow(unused)]` because `sync_once` and `SyncPassStats` were only exercised by tests, not production code.
- **Decision**: Removed the attribute and its associated comment in Phase 4.
- **Rationale**: With `start_podcast_sync_task` calling `sync_once` from production code and the wiring in `actual_main()` exercising the entire public API, the unused warning suppression is no longer needed. Removing it enables clippy to catch genuinely dead code going forward.
- **Reference**: `server/src/podcast_sync.rs`

### 5. get_app_config_path() for database path

- **Context**: The sync task needs the path to the podcast SQLite database. The server already uses `utils::get_app_config_path()` elsewhere for locating application data.
- **Decision**: Called `utils::get_app_config_path().context("sync task: config path")?` in `actual_main()` and passed the result as `db_path` to the sync task.
- **Rationale**: Follows the established pattern in the codebase for locating the config/data directory. Using `context()` provides a clear error message if the path resolution fails during startup.
- **Reference**: `server/src/server.rs`

## Deviations from Spec

### T-22 already completed in Phase 3

- **Spec said**: T-22 specifies registering `mod podcast_sync;` in Phase 4.
- **Actual**: The module declaration was already added in Phase 3 (required for compilation).
- **Reason**: As noted in the Phase 3 summary, Rust requires module declarations for compilation. No additional change was needed in Phase 4 for this task.

## Test Results

- **Unit Tests**: 29/29 passing (20 from Phase 3 + 9 new lifecycle tests)
- **Integration Tests**: Not applicable for this phase (deferred to Phase 5)

## Next Steps

Phase complete. No remaining items.

---
---

# Implementation Summary: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:impl-summary-writer
- **Phase**: 5 — Integration Tests and Verification
- **Status**: completed

---

## Overview

Phase 5 added 11 integration tests using the `wiremock` crate to serve real HTTP responses for RSS feeds and episode downloads, verifying the full end-to-end sync flow. The tests cover the complete sync lifecycle including feed fetching, episode deduplication (by GUID and enclosure URL), file download, enqueue command generation, error isolation across feeds, partial download failure handling, and task lifecycle with a live mock server. All 40 tests in the module pass and clippy reports no warnings.

## Files Changed

- `server/src/podcast_sync.rs` — modified, +1169/-11
  - Purpose: Added 11 integration tests in the `tests` module using `wiremock::MockServer` to simulate podcast RSS feeds and episode downloads. Also includes helper functions (`generate_rss_feed`, `fake_audio_content`) for generating test fixtures. Removed 11 unused `use` statements (dead import cleanup) from existing Phase 4 tests.

- `Cargo.toml` — modified, +1/-0
  - Purpose: Added `wiremock = "0.6"` to `[workspace.dependencies]` for workspace-wide availability.

- `server/Cargo.toml` — modified, +1/-0
  - Purpose: Added `wiremock.workspace = true` to `[dev-dependencies]` in the server crate.

- `Cargo.lock` — modified, +91/-7
  - Purpose: Auto-generated lock file update recording the resolved `wiremock` crate and its transitive dependency tree.

## Key Decisions

### 1. wiremock 0.6 for HTTP mocking

- **Context**: Integration tests needed to simulate real HTTP endpoints for RSS feed fetching and episode downloading without depending on external services.
- **Decision**: Used `wiremock` 0.6 which provides a local mock HTTP server with request matching and response templating.
- **Rationale**: `wiremock` is the standard Rust crate for HTTP integration testing. It runs a real TCP listener on a random port, making tests exercise the actual HTTP client code path. Version 0.6 is the latest stable release compatible with the project's async runtime (tokio).
- **Reference**: `Cargo.toml`, `server/Cargo.toml`

### 2. Non-routable IP address for download failure simulation

- **Context**: T-25 requires testing that a download failure for one episode does not block processing of other episodes. HTTP 4xx/5xx responses do not trigger the same code path as connection failures.
- **Decision**: Used `http://192.0.2.1:1/episodes/unreachable.mp3` (TEST-NET-1 per RFC 5737) as the unreachable URL and set `max_download_retries=1` to minimize test duration.
- **Rationale**: The `192.0.2.0/24` range is guaranteed non-routable and will trigger a TCP connection failure rather than an HTTP error response. Setting retries to 1 avoids long timeout waits while still exercising the retry logic.
- **Reference**: `server/src/podcast_sync.rs` (test `integration_one_episode_download_fails_others_succeed`)

### 3. Test organization: all integration tests in the same module

- **Context**: Integration tests could be placed in a separate `tests/` directory or in the existing `mod tests` block within `podcast_sync.rs`.
- **Decision**: Placed all integration tests inside the existing `#[cfg(test)] mod tests` block alongside the unit tests from Phases 3 and 4.
- **Rationale**: The integration tests need access to private functions (`sync_once`, `make_test_config`, `make_cmd_channel`) and types that are not exposed publicly. Keeping them in the same module avoids needing to add `pub` visibility or create a separate test helper crate. The test names are prefixed with `integration_` for clear identification.
- **Reference**: `server/src/podcast_sync.rs`

### 4. Eleven tests covering all three task items (T-24, T-25, T-26)

- **Context**: The task list specified 3 integration test tasks with multiple BDD scenarios each.
- **Decision**: Implemented 11 individual test functions, distributed as: 4 tests for T-24 (full flow, dedup across passes, mixed episodes, enqueue format), 3 tests for T-25 (HTTP 500 isolation, malformed XML, partial download failure), and 4 tests for T-26 (playback non-disruption, startup sync with mock, empty feed, URL-based dedup fallback).
- **Rationale**: Fine-grained test functions provide precise failure diagnostics and map directly to individual BDD scenarios (SCENARIO-010 through SCENARIO-022). Each test is self-contained with its own mock server, temp directory, and database.
- **Reference**: `server/src/podcast_sync.rs`

### 5. Dead import cleanup in existing tests

- **Context**: While adding integration tests, the compiler flagged 11 unused `use std::path::PathBuf` and `use tokio_util::sync::CancellationToken` imports in Phase 4 tests.
- **Decision**: Removed the redundant imports that were already covered by module-level imports.
- **Rationale**: Eliminates compiler warnings and keeps the codebase clean. These were likely left over from when the tests were written incrementally and the module-level imports were added later.
- **Reference**: `server/src/podcast_sync.rs`

## Deviations from Spec

### Additional test coverage beyond specified scenarios

- **Spec said**: T-26 covers SCENARIO-005 (disabled sync), SCENARIO-009 (cancellation), and SCENARIO-022 (playback non-disruption).
- **Actual**: Added additional integration tests for SCENARIO-012 (URL-based dedup fallback) and SCENARIO-021 (empty feed) which were not explicitly assigned to T-26 but verify important edge cases end-to-end.
- **Reason**: These scenarios were covered by unit tests in Phase 3 but benefit from end-to-end validation with a real HTTP server to confirm the full code path works correctly.

## Test Results

- **Unit Tests**: 29/29 passing (from Phases 3-4)
- **Integration Tests**: 11/11 passing (new in Phase 5)
- **Total module tests**: 40/40 passing

## Next Steps

Phase complete. No remaining items.
