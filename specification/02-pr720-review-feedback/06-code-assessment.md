---
name: code-assessment
description: Code assessment for PR #720 podcast synchronization feature — evaluating architecture, standards, dependencies, and patterns for the server/lib crates focused on the podcast sync implementation.
doc-type: code-assessment
gate-profile: null
---

# Code Assessment: PR #720 Podcast Synchronization

## Metadata

| Field | Value |
|-------|-------|
| Title | Code Assessment: PR #720 Podcast Synchronization |
| Date | 2026-06-25 |
| Author | super-dev:code-assessor |
| Scope | server/src/podcast_sync.rs, lib/src/config/v2/server/synchronization.rs, lib/src/podcast/, lib/src/taskpool.rs, lib/src/player.rs (playlist_helpers), server/src/server.rs |
| Focus | architecture, standards, dependencies, patterns |

## Executive Summary

The podcast synchronization implementation in PR #720 is functionally correct with good integration tests, but has several architectural issues flagged by the reviewer: (1) the synchronization config is placed at the top-level `ServerSettings` rather than nested under `podcast`, (2) `sync_once` uses blocking filesystem I/O (`std::fs::read_dir`) inside an async context, (3) a per-sync-pass `TaskPool` is created for downloads instead of sharing the global one, and (4) enqueued podcast episodes use `PlaylistTrackSource::Path` instead of `PlaylistTrackSource::PodcastUrl`. The test suite has meaningful integration tests but also contains redundant tests that verify Rust language semantics. The primary recommendation is to address the architectural issues before landing, as they represent the core reviewer feedback.

| Dimension | Score (1-5) | Issues |
|-----------|-------------|--------|
| Architecture | 3 | 5 |
| Code Standards | 4 | 2 |
| Dependencies | 4 | 1 |
| Framework Patterns | 3 | 4 |
| Maintainability | 3 | 3 |

Scoring: 5=Excellent, 4=Good, 3=Adequate, 2=Needs Improvement, 1=Critical

## Architecture Evaluation

### Organization

The workspace follows a clean 4-crate structure:
- `lib` (termusic-lib): Shared library with config, podcast DB, player types, utilities
- `server` (termusic-server): Server binary with gRPC service, player loop, podcast sync
- `playback` (termusic-playback): Audio backend abstraction
- `tui`: Terminal UI client

The podcast sync module (`server/src/podcast_sync.rs`) is correctly placed in the server crate. However, it duplicates podcast network logic that also exists in the TUI crate (`tui/src/ui/components/podcast.rs:452,673,774`), which is the core architectural concern from the reviewer.

### Module Boundaries

| Module | Responsibility | Coupling | Cohesion |
|--------|---------------|----------|----------|
| server/src/podcast_sync.rs | Periodic feed sync + download | Medium | Medium |
| lib/src/podcast/mod.rs | Feed fetch, download, OPML | Low | High |
| lib/src/podcast/db/mod.rs | SQLite podcast DB operations | Low | High |
| lib/src/config/v2/server/synchronization.rs | Sync config deserialization | Low | High |
| lib/src/taskpool.rs | Bounded async task execution | Low | High |
| lib/src/player.rs (playlist_helpers) | Playlist track types | Low | High |

### Data Flow

```
                   Server Startup
                        |
                        v
              [config.synchronization.enable?]
                   /          \
                 yes           no
                  |             |
                  v             v
     start_podcast_sync_task   (skip)
                  |
     [refresh_on_startup?]---yes---> sync_once()
                  |                       |
                  v                       v
        interval_at loop         Database::new(db_path)
              |                       |
              v                       v
         sync_once() <-------   get_podcasts()
              |                       |
              v                       v
    check_feed(TaskPool) ------> feed_rx channel
              |                       |
              v                       v
    download_list(TaskPool) ---> dl_rx channel
              |                       |
              v                       v
    cmd_tx.send(PlaylistAddTrack) --> player_loop
```

### Error Handling Consistency

Error handling follows a consistent pattern across the codebase:
- **Fatal errors** (cannot open DB): propagated via `anyhow::Result` with `.context()` annotations
- **Per-entity errors** (feed fetch failure, download failure): logged at `warn!` level, increment stats counters, continue processing
- **Channel errors**: logged at `warn!`, do not abort

This isolation pattern is correct and matches the project convention seen in `lib/src/podcast/mod.rs:327-358` (OPML import) and `server/src/server.rs:517-534` (playlist operations in player_loop).

### Findings

#### ARCH-001: Config placement violates reviewer requirement (AC-04)
**Severity**: High
**Location**: `lib/src/config/v2/server/mod.rs:36`

**Issue**: `SynchronizationSettings` is placed as a top-level field on `ServerSettings` rather than nested under `PodcastSettings`. The reviewer explicitly stated this must be under `[podcast.synchronization]` or merged into `PodcastSettings`.

**Impact**: Blocks PR approval. Creates a semantically incorrect config hierarchy where podcast-specific settings appear at the same level as player, com, and backend settings.

**Recommendation**: Move `synchronization: SynchronizationSettings` from `ServerSettings` into `PodcastSettings`, or add a `synchronization` field to `PodcastSettings`. Update all references in `server/src/podcast_sync.rs:68` and `server/src/server.rs:176`.

#### ARCH-002: Blocking filesystem I/O inside async context (AC-15)
**Severity**: High
**Location**: `server/src/podcast_sync.rs:172-186`

**Issue**: `std::fs::read_dir(&pod_download_dir)` is called inside the `while let Some(message) = feed_rx.recv().await` loop. This is synchronous filesystem I/O executing on the async runtime's thread pool, which can block other async tasks.

**Impact**: For large podcast directories (thousands of files), this blocks the tokio worker thread. Violates AC-15 and SCENARIO-022/023 from BDD scenarios.

**Recommendation**: Perform the directory scan once before the async loop using `tokio::task::spawn_blocking`, or collect all existing files into a `HashSet` before processing begins.

#### ARCH-003: Per-sync-pass TaskPool instead of shared global (AC-10)
**Severity**: Medium
**Location**: `server/src/podcast_sync.rs:226`

**Issue**: A new `TaskPool::new(concurrent_downloads_max)` is created inside the per-podcast download loop. The reviewer requires a single shared TaskPool for all podcast network operations (feed fetches AND downloads).

**Impact**: Multiple concurrent sync passes (if somehow triggered) could exceed the intended concurrency bound. Creates unnecessary allocation overhead. The feed TaskPool at line 79 is already correct for feeds, but downloads should share it.

**Recommendation**: Use the feed TaskPool (line 79) for both feed fetches and downloads, or create a single download TaskPool outside the per-podcast loop.

#### ARCH-004: Podcast episodes enqueued with Path instead of PodcastUrl (AC-14)
**Severity**: High
**Location**: `server/src/podcast_sync.rs:193-194`, `server/src/podcast_sync.rs:255-257`

**Issue**: Downloaded episodes are enqueued using `PlaylistTrackSource::Path(file_path)` instead of `PlaylistTrackSource::PodcastUrl(episode_url)`. The reviewer flagged this twice as WRONG. Podcast episodes must carry their feed URL for proper resume/re-download behavior.

**Impact**: Breaks podcast-specific features (resume from position, re-download on file deletion). The `PodcastUrl` variant exists specifically for this purpose (lib/src/player.rs:403).

**Recommendation**: Replace `PlaylistTrackSource::Path(...)` with `PlaylistTrackSource::PodcastUrl(ep.url.clone())` for all podcast episode enqueue operations.

#### ARCH-005: TUI still owns podcast sync logic (prerequisite not met, AC-01/AC-02)
**Severity**: Medium
**Location**: `tui/src/ui/components/podcast.rs:452,673,774`

**Issue**: The TUI crate directly calls `check_feed()` and `download_list()` for podcast operations. The reviewer stated that moving this to the server is a prerequisite before the periodic sync can land. Currently both TUI and server independently perform podcast network operations.

**Impact**: Duplication of podcast sync logic across two crates. Inconsistent behavior between manual (TUI) and automatic (server) sync. Blocks PR approval per reviewer's explicit prerequisite statement.

**Recommendation**: This is the Phase 0 work identified in the requirements. Migrate TUI podcast operations to send commands to the server via the existing gRPC/UDS communication layer.

## Code Standards

### Tooling Inventory

| Tool | Config File | Status |
|------|-------------|--------|
| Clippy (linter) | Cargo.toml [workspace.lints.clippy], clippy.toml | Active — pedantic + correctness + all |
| rustfmt (formatter) | (uses nightly default) | Active — `cargo +nightly fmt` per CONTRIBUTING.md |
| Rust edition | Cargo.toml `edition = "2024"` | Active — latest edition |
| unsafe_code | Cargo.toml [workspace.lints.rust] | Denied workspace-wide |

### Conventions Observed

- **Naming**: snake_case for functions/variables, PascalCase for types, SCREAMING_SNAKE for constants. Example: `sync_once`, `SyncPassStats`, `AT_END` (server/src/podcast_sync.rs:21-30, lib/src/player.rs:454)
- **File Organization**: One module per logical concern. Test modules inline via `#[cfg(test)] mod tests`. Separate test files for large test suites (e.g., `synchronization_tests.rs` referenced via `#[cfg(test)] mod synchronization_tests;`). Example: lib/src/config/v2/server/mod.rs:22
- **Import Ordering**: std libs first, then external crates, then internal crates (`termusiclib::`, `termusicplayback::`), then local `use super::*`. Example: server/src/podcast_sync.rs:1-17
- **Comment Style**: `///` for public API doc comments, `//` for inline implementation notes. Module-level `//!` doc comments used in test modules (lib/src/config/v2/server/synchronization_tests.rs:1-9). `#[must_use]` on pure functions returning values (lib/src/player.rs:456).
- **Error handling**: `anyhow::Result` with `.context("description")` for error propagation. `warn!`/`error!` macros for non-fatal logging. No `unwrap()` in production code paths.

### Findings

#### STD-001: Module documentation style inconsistency
**Severity**: Low
**Location**: `server/src/podcast_sync.rs:1-2`

**Issue**: The podcast_sync module uses `//` comments for module-level documentation instead of `//!` doc comments. The reviewer requires `//!` doc comment format (AC-29).

**Impact**: Minor — documentation does not show in `cargo doc` output for the module.

**Recommendation**: Convert lines 1-2 to `//!` format:
```rust
//! Podcast synchronization module.
//! Implements the sync pass logic and task lifecycle for periodic podcast feed refresh and download.
```

#### STD-002: Deep nesting exceeds 3 levels (AC-30)
**Severity**: Medium
**Location**: `server/src/podcast_sync.rs:106-315`

**Issue**: The `while let Some(message) = feed_rx.recv().await` block contains 6+ levels of nesting: match arm -> match arm -> for loop -> if-let -> match arm -> if-let. This makes the code difficult to follow and violates AC-30.

**Impact**: High cognitive load for reviewers and future maintainers. The `sync_once` function is 280 lines with deeply nested control flow.

**Recommendation**: Extract inner logic into named helper functions: `process_sync_data(pod_id, pod_data, ...)`, `process_download_results(dl_rx, ...)`, `check_existing_file(...)`.

## Dependencies

### Manifest Analysis

| Package | Current | Latest | Status | Health |
|---------|---------|--------|--------|--------|
| tokio | 1.52 | 1.52 | Current | Healthy |
| tokio-util | 0.7.18 | 0.7.18 | Current | Healthy |
| anyhow | 1.0.102 | 1.0.102 | Current | Healthy |
| rusqlite (bundled) | 0.39 | 0.39 | Current | Healthy |
| chrono | 0.4.45 | 0.4.45 | Current | Healthy |
| humantime-serde | 1.1 | 1.1 | Current | Healthy |
| wiremock (dev) | 0.6 | 0.6 | Current | Healthy |
| reqwest | 0.13.4 | 0.13.4 | Current | Healthy |
| sanitize-filename | 0.6 | 0.6 | Current | Healthy |
| parking_lot | 0.12.5 | 0.12.5 | Current | Healthy |
| tonic/prost | 0.14.x | 0.14.x | Current | Healthy |

### Security Advisories

None found in the dependency tree relevant to the podcast sync feature.

### Bundle/Binary Size Concerns

No concerns. The podcast sync feature uses only dependencies already present in the workspace. No new dependencies were introduced. `tokio-util` already has the `rt` feature in lib; the server crate uses it without additional features.

### Findings

#### DEP-001: sanitize-filename duplicated usage
**Severity**: Low
**Location**: `server/src/podcast_sync.rs:8`, `lib/src/podcast/mod.rs:24`

**Issue**: Both the server's `podcast_sync.rs` and lib's `podcast/mod.rs` import and use `sanitize_filename` with identical `Options` configuration (truncate: true, windows: true, replacement: ""). The existing `lib::utils::create_podcast_dir` function (lib/src/utils.rs:111) already handles this sanitization and directory creation.

**Impact**: Code duplication. If sanitization rules change, multiple locations need updating.

**Recommendation**: Use `lib::utils::create_podcast_dir` (AC-17) instead of reimplementing the sanitize+create_dir pattern in `sync_once`.

## Framework Patterns

### Patterns Inventory

| Pattern | Usage | Location | Assessment |
|---------|-------|----------|------------|
| Async task spawning | `Handle::spawn` + `CancellationToken` + `select!` | server/src/server.rs:231-256 | Appropriate — matches playlist_save_interval |
| Bounded concurrency | `TaskPool` (semaphore-based) | lib/src/taskpool.rs:11-68 | Appropriate |
| Channel-drain pattern | `unbounded_channel` + `recv().await` loop | server/src/podcast_sync.rs:81,105 | Appropriate |
| Shared config | `Arc<RwLock<ServerOverlay>>` (parking_lot) | lib/src/config/mod.rs:17 | Appropriate |
| Config deserialization | serde + toml + humantime-serde for Duration | lib/src/config/v2/server/synchronization.rs:22 | Appropriate |
| DB migration | user_version pragma | lib/src/podcast/db/migration.rs:30-47 | Appropriate |
| Proto sub-message | oneof with inner oneof (UpdatePlaylist pattern) | lib/proto/player.proto:175-184 | Appropriate — well-established |

### Test Structure

- **Framework**: Standard `#[test]` and `#[tokio::test]` with inline test modules
- **Assertions**: `pretty_assertions` for struct comparisons (lib tests), standard `assert_eq!`/`assert!` elsewhere
- **Test infrastructure**: `tempfile` for temp dirs, `wiremock` for HTTP mocking
- **Organization**: Tests grouped by logical section with `// ===` separator comments and `T-XX:` identifiers
- **Coverage**: 40 tests in podcast_sync.rs (19 unit + 21 integration), 19 tests in synchronization_tests.rs

### Findings

#### PAT-001: Redundant tests verifying Rust language semantics (AC-20)
**Severity**: Medium
**Location**: `server/src/podcast_sync.rs:444-489`

**Issue**: Tests `sync_pass_stats_struct_has_required_fields`, `sync_pass_stats_all_zeros`, and `sync_pass_stats_implements_debug` verify that a struct with `#[derive(Debug, Clone, PartialEq, Eq)]` has those traits and that setting field values works. These test basic Rust language semantics, not business logic.

**Impact**: Test suite bloat (3 tests contributing nothing). Reviewer explicitly called out this pattern.

**Recommendation**: Remove these tests. The struct definition with derive macros is the specification; if fields are missing, the compiler catches it.

#### PAT-002: Tests verify function signature existence (AC-20)
**Severity**: Medium
**Location**: `server/src/podcast_sync.rs:890-919`

**Issue**: Tests `sync_once_accepts_expected_parameters` and `sync_once_returns_anyhow_result_of_sync_pass_stats` exist solely to validate function signatures. The Rust compiler already ensures type safety; these tests add no behavioral verification.

**Impact**: Test suite bloat (2 tests), confusing intent for future maintainers.

**Recommendation**: Remove these tests. Function signatures are verified at compile time.

#### PAT-003: Per-sync TaskPool does not follow established pattern
**Severity**: Medium
**Location**: `server/src/podcast_sync.rs:226`

**Issue**: The `sync_once` function creates a new `TaskPool` for each podcast's downloads inside the per-podcast loop. The established pattern (seen in `import_from_opml` at lib/src/podcast/mod.rs:309) creates a single TaskPool and reuses it for all operations in a batch.

**Impact**: Inconsistency with existing patterns. Creates N TaskPools for N podcasts, each with `concurrent_downloads_max` permits — effectively allowing N * concurrent_downloads_max simultaneous downloads.

**Recommendation**: Create a single download TaskPool before the podcast processing loop (matching the feed TaskPool pattern at line 79).

#### PAT-004: Test helper duplication (AC-26)
**Severity**: Low
**Location**: `server/src/podcast_sync.rs:1592-1662` (and many similar blocks)

**Issue**: Multiple integration tests repeat identical setup: create MockServer, generate RSS feed, set up mocks, create temp dir, create Database, insert PodcastNoId, configure, run sync_once. While `make_test_config` and `make_cmd_channel` exist, the full mock server + DB setup is duplicated ~10 times.

**Impact**: Boilerplate makes tests harder to read. Changes to test setup require updating many locations.

**Recommendation**: Create a `TestHarness` struct that encapsulates MockServer + Database + config + channel setup, with builder methods for customization.

## Pattern Library

### Pattern 1: Async Task Spawning with Cancellation
**Canonical example**: `server/src/server.rs:231-256` (`start_playlist_save_interval`)
**Consistency score**: 100% — both `start_playlist_save_interval` and `start_podcast_sync_task` follow this exactly
**Violations**: None

The pattern: Accept `Handle`, `CancellationToken`, and owned resources. Use `handle.spawn()` with an async block containing `interval_at` + `loop { select! { tick => work, cancelled => break } }`.

### Pattern 2: Channel-Drain for Async Results
**Canonical example**: `lib/src/podcast/mod.rs:310-358` (`import_from_opml`)
**Consistency score**: 90% — `sync_once` follows this but creates per-podcast TaskPools (deviation)
**Violations**: `server/src/podcast_sync.rs:226` creates TaskPool inside loop

The pattern: Create `unbounded_channel`, dispatch work via TaskPool, drop the sender, drain results via `while let Some(msg) = rx.recv().await`, count messages to detect completion.

### Pattern 3: Config with serde(default) and Workspace Lints
**Canonical example**: `lib/src/config/v2/server/mod.rs:26-37` (`ServerSettings`)
**Consistency score**: 95% — all config structs use `#[serde(default)]`
**Violations**: None significant

The pattern: Structs derive `Deserialize, Serialize`, use `#[serde(default)]`, provide `Default` impl with sensible values, use workspace-level lint configuration.

### Pattern 4: Database Operations with Named Parameters
**Canonical example**: `lib/src/podcast/db/podcast_db.rs:70-111` (`PodcastDBInsertable`)
**Consistency score**: 100% — all DB operations use `prepare_cached` + `named_params!` or `params!`
**Violations**: None

The pattern: Use `prepare_cached` for all queries, `named_params!` for inserts/updates with many columns, `params!` for simple queries. Return `Result<usize, rusqlite::Error>` for write operations.

### Pattern 5: Error Handling with anyhow::Context
**Canonical example**: `server/src/server.rs:104-226` (`actual_main`)
**Consistency score**: 95% — consistently used across the codebase
**Violations**: Some early returns in `sync_once` use `warn!` + `continue` without context string for logging

The pattern: Use `anyhow::Result` as return type, chain `.context("descriptive message")` on fallible operations, use `bail!` for early error returns with messages. Non-fatal errors use `warn!`/`error!` macros and continue processing.

## Architecture Smell Detection

### God Function: sync_once (Medium)
**Location**: `server/src/podcast_sync.rs:39-319`
**Lines**: 280
**Responsibilities**: DB connection, podcast retrieval, config reading, feed dispatch, result processing, file existence checking, directory creation, download dispatch, download result processing, episode enqueue, stats accumulation

**Blast radius**: 1 file, but the function is the entire core logic of the feature.
**Recommendation**: Extract into 4-5 named helpers (already identified in STD-002).

### Inappropriate Intimacy: sync_once reimplements create_podcast_dir (Low)
**Location**: `server/src/podcast_sync.rs:123-134` vs `lib/src/utils.rs:111-117`
**Evidence**: Both construct download paths via sanitize_with_options + push + create_dir_all

**Blast radius**: 2 files
**Recommendation**: Call existing utility instead of reimplementing.

## Better Options Analysis

| Current Approach | Better Option | Benefit | Migration Effort |
|------------------|---------------|---------|-----------------|
| Top-level `[synchronization]` config | Nested under `[podcast.synchronization]` | Semantic correctness, reviewer approval | S |
| `PlaylistTrackSource::Path` for podcasts | `PlaylistTrackSource::PodcastUrl` | Correct resume/re-download, reviewer approval | S |
| Per-podcast `TaskPool` for downloads | Single shared `TaskPool` for all downloads | Bounded concurrency, matches pattern | S |
| Inline `std::fs::read_dir` in async | Pre-scan or `spawn_blocking` | No async thread blocking | S |
| 280-line `sync_once` function | Extract helpers for each phase | Readability, testability | M |
| Repeated test setup | `TestHarness` builder | DRY, easier to maintain | M |

## Technical Debt Inventory

| ID | Description | Location | Severity | Effort | Blast Radius | Priority |
|----|-------------|----------|----------|--------|--------------|----------|
| TD-001 | Config placement at top-level instead of under podcast | lib/src/config/v2/server/mod.rs:36 | High | S | 5 files | Now |
| TD-002 | PlaylistTrackSource::Path used instead of PodcastUrl | server/src/podcast_sync.rs:193,255 | High | S | 2 files | Now |
| TD-003 | Blocking read_dir inside async context | server/src/podcast_sync.rs:172-186 | High | S | 1 file | Now |
| TD-004 | Per-podcast TaskPool instead of shared | server/src/podcast_sync.rs:226 | Medium | S | 1 file | Soon |
| TD-005 | TUI still owns podcast sync logic (prerequisite) | tui/src/ui/components/podcast.rs:452,673,774 | High | L | 3+ files | Soon |
| TD-006 | Redundant tests (5 tests verifying language semantics) | server/src/podcast_sync.rs:444-919 | Low | S | 1 file | Eventually |
| TD-007 | sync_once is 280 lines with 6+ nesting levels | server/src/podcast_sync.rs:39-319 | Medium | M | 1 file | Eventually |
| TD-008 | Duplicate sanitize+create_dir logic (not using utils) | server/src/podcast_sync.rs:123-134 | Low | S | 1 file | Eventually |
| TD-009 | No per-podcast last_checked tracking (AC-08) | lib/src/podcast/db/ | Medium | M | 3 files | Soon |
| TD-010 | No configurable enqueue behavior (AC-11) | server/src/podcast_sync.rs:193-265 | Medium | S | 2 files | Soon |

## Prioritized Recommendations

| Priority | ID | Recommendation | Effort | Impact |
|----------|-----|---------------|--------|--------|
| 1 | REC-001 | Move `synchronization` field from `ServerSettings` into `PodcastSettings` (or as nested `[podcast.synchronization]`) | S | L |
| 2 | REC-002 | Replace `PlaylistTrackSource::Path` with `PlaylistTrackSource::PodcastUrl` for all podcast enqueue operations | S | L |
| 3 | REC-003 | Move `std::fs::read_dir` call outside the async loop — pre-scan all podcast directories into a HashMap before processing | S | L |
| 4 | REC-004 | Use a single shared TaskPool for downloads (create once before the podcast loop, not per-podcast) | S | M |
| 5 | REC-005 | Extract `sync_once` inner logic into named helpers: `process_feed_result()`, `find_episodes_to_download()`, `drain_downloads()` | M | M |
| 6 | REC-006 | Remove 5 redundant tests that verify struct derives and function signatures | S | S |
| 7 | REC-007 | Use `lib::utils::create_podcast_dir` instead of reimplementing sanitize+mkdir | S | S |
| 8 | REC-008 | Convert module comments to `//!` doc comments | S | S |
| 9 | REC-009 | Add per-podcast `last_checked` tracking with `update_last_checked` DB method (prerequisite for per-podcast scheduling) | M | L |
| 10 | REC-010 | Add configurable enqueue behavior (enable/disable auto-enqueue via config field) | S | M |

Priority ordering: High Impact + Low Effort first, then High Impact + High Effort, then Low Impact + Low Effort.

## File Coverage Report

| Category | Files Analyzed | Total Files | Coverage |
|----------|---------------|-------------|---------|
| Server crate (src/) | 4 | 7 | 57% |
| Lib config/v2/server/ | 3 | 5 | 60% |
| Lib podcast/ | 5 | 7 | 71% |
| Lib core (player, taskpool, utils) | 3 | 3 | 100% |
| Proto definitions | 1 | 1 | 100% |
| Root config (Cargo.toml, clippy.toml) | 4 | 4 | 100% |
| **Total relevant** | **20** | **27** | **74%** |

### Exclusions

- `playback/`: Not relevant to podcast sync feature — audio backend implementation
- `tui/src/` (most files): Only checked for podcast sync usage patterns, not full assessment
- `lib/src/songtag/`: Song metadata tagging, unrelated to podcast sync
- `lib/src/new_database/`: Music library database, separate from podcast DB
- `lib/src/invidious.rs`: YouTube/Invidious integration, unrelated
- `.github/`, `assets/`, `screenshots/`: Non-code resources
