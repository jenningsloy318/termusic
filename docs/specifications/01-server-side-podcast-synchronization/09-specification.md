# Technical Specification: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:spec-writer
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md
- **Architecture**: ./07-architecture.md

---

## 1. Overview

This specification defines the implementation of a server-internal periodic task that refreshes subscribed podcast RSS feeds, downloads new episodes (deduplicated by GUID with fallback to enclosure URL), and appends them to the play queue. The feature enables fully headless podcast synchronization independent of the TUI client, keeping subscriptions current even when no client is connected.

The technical approach mirrors the proven `start_playlist_save_interval` pattern already in the codebase: a tokio-spawned async task using `interval_at` for drift-free timing and `CancellationToken` for graceful shutdown. A new `SynchronizationSettings` config section (with `#[serde(default)]` for backward compatibility) controls the feature. The sync logic reuses existing infrastructure from `termusiclib::podcast` (feed fetching, downloading, database operations) and communicates with the player loop via the existing `PlayerCmd::PlaylistAddTrack` command channel.

The only new external dependency is `humantime-serde` (version 1.1, latest stable) for parsing human-readable duration strings like `"1h"` and `"30m"` from the TOML configuration file. Additionally, `wiremock = "0.6"` was added as a dev-dependency for integration testing with mock HTTP servers.

## 2. Architecture

### 2.1. System Integration Point

The podcast sync task runs as a sibling to the existing `start_playlist_save_interval` task within the server's tokio runtime. Both tasks share the same lifecycle pattern: spawned from `actual_main()`, receiving a `Handle`, `CancellationToken`, and task-specific dependencies. The sync task communicates with the player loop exclusively through the existing unbounded `PlayerCmdSender` channel, ensuring zero coupling with the player loop's internal state.

### 2.2. Module Decomposition

The implementation introduces two new files and modifies five existing files:

- **New**: `lib/src/config/v2/server/synchronization.rs` -- Configuration struct with serde defaults
- **New**: `server/src/podcast_sync.rs` -- Task lifecycle management and sync logic
- **Modified**: `Cargo.toml` (workspace root) -- Add `humantime-serde` dependency
- **Modified**: `lib/Cargo.toml` -- Add `humantime-serde` to lib crate dependencies
- **Modified**: `lib/src/config/v2/server/mod.rs` -- Register new config module and add field
- **Modified**: `lib/src/player.rs` -- Add `AT_END` constant and `new_append_single`/`new_append_vec` constructors
- **Modified**: `server/src/server.rs` -- Register module and wire task spawn in `actual_main()`

### 2.3. Concurrency Model

The sync task operates on the shared tokio runtime alongside the gRPC service and playlist save task. Network I/O (feed fetching, episode downloads) is fully async via reqwest. Download concurrency is bounded by the existing `podcast.concurrent_downloads_max` setting (default 3) through a `TaskPool` semaphore. The sync task opens its own `Database` connection per pass (SQLite supports concurrent readers), eliminating any shared mutable state with the player loop thread.

### 2.4. Communication Path

```
sync_once() --> cmd_tx.send(PlayerCmd::PlaylistAddTrack) --> player_loop thread
                                                               |
                                                               v
                                                         Playlist::add_tracks()
                                                               |
                                                               v
                                                         (auto-start if empty)
```

The `PlayerCmdSender` is an unbounded mpsc channel already used by the gRPC service. Adding the sync task as another sender requires no channel modification.

## 3. Data Models

### 3.1. SynchronizationSettings

Configuration struct controlling the podcast sync task behavior. Lives in the lib crate to be accessible from both server and test code. Follows the identical pattern of `MetadataSettings`, `BackendSettings`, and other config sections.

```rust
/// Settings for the periodic podcast synchronization task.
/// When absent from the config file, all fields use their defaults
/// due to #[serde(default)] on the struct.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SynchronizationSettings {
    /// Whether automatic podcast synchronization is enabled.
    /// Default: true
    pub enable: bool,

    /// How often to check all subscribed feeds for new episodes.
    /// Accepts human-readable duration strings: "1h", "30m", "2h30m".
    /// Default: "1h" (3600 seconds)
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Whether to run a full sync immediately on server startup
    /// before entering the periodic cycle.
    /// Default: true
    pub refresh_on_startup: bool,
}

impl Default for SynchronizationSettings {
    fn default() -> Self {
        Self {
            enable: true,
            interval: Duration::from_secs(3600),
            refresh_on_startup: true,
        }
    }
}
```

### 3.2. SyncPassStats

Statistics from a single sync pass, used for logging at `info` level after each pass completes.

```rust
/// Statistics collected during a single sync pass, for logging.
struct SyncPassStats {
    /// Number of subscribed podcasts whose feeds were checked.
    podcasts_checked: usize,
    /// Number of podcasts where feed fetch or parse failed.
    podcasts_failed: usize,
    /// Number of new episodes successfully downloaded.
    episodes_downloaded: usize,
    /// Number of downloaded episodes successfully enqueued.
    episodes_enqueued: usize,
    /// Number of episodes where download failed.
    episodes_failed: usize,
}
```

### 3.3. PlaylistAddTrack Extension (AT_END and constructors)

Extension to the existing `PlaylistAddTrack` struct in `lib/src/player.rs`:

```rust
impl PlaylistAddTrack {
    /// Sentinel value indicating tracks should be appended at the end of the playlist.
    /// Any `at_index >= playlist.len()` triggers end-append behavior in Playlist::add_tracks.
    pub const AT_END: u64 = u64::MAX;

    /// Create a request to append a single track at the end of the playlist.
    #[must_use]
    pub fn new_append_single(track: PlaylistTrackSource) -> Self {
        Self {
            at_index: Self::AT_END,
            tracks: vec![track],
        }
    }

    /// Create a request to append multiple tracks at the end of the playlist.
    #[must_use]
    pub fn new_append_vec(tracks: Vec<PlaylistTrackSource>) -> Self {
        Self {
            at_index: Self::AT_END,
            tracks,
        }
    }
}
```

### 3.4. ServerSettings Field Addition

The existing `ServerSettings` struct in `lib/src/config/v2/server/mod.rs` gains one new field:

```rust
// In the existing ServerSettings struct:
pub struct ServerSettings {
    // ... existing fields ...
    /// Podcast synchronization settings (automatic feed refresh and download).
    pub synchronization: SynchronizationSettings,
}
```

## 4. API Design

### 4.1. Internal Function: start_podcast_sync_task

This is an internal server function (not an external API endpoint). It spawns the periodic sync task.

```rust
/// Spawn the periodic podcast synchronization task.
///
/// The task executes `sync_once` either immediately (if `refresh_on_startup` is true)
/// or after the first interval tick. Subsequent ticks run at the configured interval.
///
/// The task exits cleanly when `cancel_token` is cancelled (server shutdown).
///
/// Only call this function when `config.read().settings.synchronization.enable` is true.
fn start_podcast_sync_task(
    handle: Handle,
    cancel_token: CancellationToken,
    config: SharedServerSettings,
    cmd_tx: PlayerCmdSender,
    db_path: PathBuf,
)
```

**Input Parameters:**
- `handle: Handle` -- tokio runtime handle for spawning the async task
- `cancel_token: CancellationToken` -- shared cancellation token from `service_cancel_token`
- `config: SharedServerSettings` -- `Arc<RwLock<ServerOverlay>>` for reading sync settings
- `cmd_tx: PlayerCmdSender` -- channel sender for dispatching PlaylistAddTrack commands
- `db_path: PathBuf` -- path to the application config directory (for opening Database)

**Output:** None (spawns a detached task on the handle)

**Error Cases:**
- The function itself does not return errors; it spawns a task that handles errors internally
- If `db_path` resolution fails, this is caught at the `actual_main()` call site before invoking this function

### 4.2. Internal Function: sync_once

Executes a single synchronization pass over all subscribed podcasts.

```rust
/// Execute one full sync pass: fetch all feeds, identify new episodes,
/// download them, and enqueue them on the playlist.
///
/// Per-podcast and per-episode errors are logged at warn level and do not
/// abort the pass. Only truly fatal errors (cannot open DB) propagate.
async fn sync_once(
    config: &SharedServerSettings,
    cmd_tx: &PlayerCmdSender,
    db_path: &Path,
) -> Result<SyncPassStats>
```

**Input Parameters:**
- `config: &SharedServerSettings` -- for reading `podcast.download_dir`, `podcast.concurrent_downloads_max`, `podcast.max_download_retries`
- `cmd_tx: &PlayerCmdSender` -- for sending PlaylistAddTrack after successful downloads
- `db_path: &Path` -- config directory path for opening a Database connection

**Output:** `Result<SyncPassStats>` -- statistics on success, `anyhow::Error` only on fatal failures

**Error Cases:**
- `DatabaseOpenError`: Cannot open SQLite connection at `db_path/data.db` -- fatal, propagated
- `GetPodcastsError`: Cannot read podcast list from database -- fatal, propagated
- `FeedFetchError`: Network or timeout error fetching a single feed -- logged, continues
- `FeedParseError`: RSS XML parse failure for a single feed -- logged, continues
- `DownloadError`: Single episode download failure -- logged, continues
- `EnqueueError`: Channel send failure (server shutting down) -- logged, continues

## 5. Implementation Details

### 5.1. Task Lifecycle (start_podcast_sync_task)

The task lifecycle follows this exact pattern from `start_playlist_save_interval`:

```rust
fn start_podcast_sync_task(
    handle: Handle,
    cancel_token: CancellationToken,
    config: SharedServerSettings,
    cmd_tx: PlayerCmdSender,
    db_path: PathBuf,
) {
    handle.spawn(async move {
        let interval_duration = config.read().settings.synchronization.interval;
        let refresh_on_startup = config.read().settings.synchronization.refresh_on_startup;

        // Immediate sync on startup if configured (AC-03, SCENARIO-006)
        if refresh_on_startup {
            match sync_once(&config, &cmd_tx, &db_path).await {
                Ok(stats) => info!("Startup sync complete: {stats:?}"),
                Err(err) => error!("Startup sync failed: {err:#}"),
            }
        }

        // Periodic sync loop (AC-04, SCENARIO-008)
        let mut timer = tokio::time::interval_at(
            Instant::now() + interval_duration,
            interval_duration,
        );
        loop {
            select! {
                _ = timer.tick() => {
                    match sync_once(&config, &cmd_tx, &db_path).await {
                        Ok(stats) => info!("Periodic sync complete: {stats:?}"),
                        Err(err) => error!("Periodic sync failed: {err:#}"),
                    }
                },
                _ = cancel_token.cancelled() => {
                    info!("Podcast sync task shutting down");
                    break;
                }
            }
        }
    });
}
```

Key behaviors:
- Uses `interval_at` (not `sleep`) to prevent timer drift (SCENARIO-023)
- The `select!` macro ensures clean cancellation mid-wait (SCENARIO-009)
- If a sync pass is in progress when the tick fires, `interval_at` naturally skips to the next tick without spawning duplicate operations (SCENARIO-023)

### 5.2. Sync Pass Logic (sync_once)

The sync pass executes these steps for each subscribed podcast with per-podcast error isolation:

1. Open a `Database::new(&db_path)` connection (fatal if fails)
2. Call `db.get_podcasts()` to retrieve all subscribed podcasts (fatal if fails)
3. For each podcast (error-isolated via `match` and `warn!`):
   a. Fetch the RSS feed via the existing `check_feed` pattern (using a TaskPool for bounded concurrency)
   b. Call `db.update_podcast(...)` which handles deduplication internally (GUID then URL matching per SCENARIO-010, SCENARIO-011, SCENARIO-012)
   c. Identify newly inserted episodes that have no download path (`path == None`)
   d. Call `download_list(episodes, &download_dir, max_retries, &taskpool, callback)` with a channel-drain callback
   e. For each successfully downloaded episode:
      - Call `db.insert_file(episode_id, &file_path)` to record the download
      - Send `PlayerCmd::PlaylistAddTrack(PlaylistAddTrack::new_append_single(PlaylistTrackSource::Path(file_path)))` via `cmd_tx` (SCENARIO-015)
4. Return `SyncPassStats` summarizing the pass

### 5.3. Download Completion Signaling

The sync task uses the channel-drain pattern (ADR-003) to await download completion:

```rust
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PodcastDLResult>();
download_list(
    episodes_to_download,
    &download_dir,
    max_retries,
    &taskpool,
    move |msg| { let _ = tx.send(msg); },
);
// tx moved into closure -- no local sender remains.
// Channel closes when all spawned download tasks complete.
while let Some(result) = rx.recv().await {
    match result {
        PodcastDLResult::DLComplete(episode_data) => {
            // Record file path in DB, then enqueue
            stats.episodes_downloaded += 1;
        }
        PodcastDLResult::DLStart(_) => { /* optional progress logging */ }
        _ => {
            // DLResponseError, DLFileCreateError, DLFileWriteError
            warn!("Episode download failed: {result:?}");
            stats.episodes_failed += 1;
        }
    }
}
```

### 5.4. Wiring in actual_main()

The task is wired in `server/src/server.rs` adjacent to the existing `start_playlist_save_interval` call, following the established spawn pattern (AC-11):

```rust
// In actual_main(), after start_playlist_save_interval call:
if config.read().settings.synchronization.enable {
    let config_dir = utils::get_app_config_path().context("sync task: config path")?;
    start_podcast_sync_task(
        handle.clone(),
        service_cancel_token.clone(),
        config.clone(),
        cmd_tx.clone(),
        config_dir,
    );
} else {
    info!("Podcast synchronization disabled");
}
```

When `synchronization.enable == false`, no task is spawned and the server operates identically to its current behavior (SCENARIO-005).

### 5.5. Episode Deduplication Strategy

Deduplication relies on the existing `Database::update_podcast` method which:
1. First matches by GUID (primary key in the episodes table) -- SCENARIO-010, SCENARIO-011
2. Falls back to matching by enclosure URL when GUID is absent -- SCENARIO-012
3. Episodes already in the database are not re-inserted regardless of matching method

Additionally, `sync_once` only schedules downloads for episodes where the `path` field is `None` (not yet downloaded). This prevents re-downloading episodes that are already on disk. The combination of database deduplication and path-existence checks ensures episodes already in the play queue are not re-added (SCENARIO-013).

### 5.6. Configuration Integration

The `SynchronizationSettings` struct is added to `ServerSettings` with `#[serde(default)]`. This means:
- Existing config files without a `[synchronization]` section parse without error (SCENARIO-001)
- The defaults (`enable: true`, `interval: "1h"`, `refresh_on_startup: true`) apply automatically
- Explicit values in the config file override defaults (SCENARIO-002)
- Invalid duration strings cause a parse error at config load time (SCENARIO-004)
- Serialization roundtrip preserves all field values (SCENARIO-003)

## 6. Testing Strategy

The testing approach validates all acceptance criteria and BDD scenarios through a combination of unit tests (fast, isolated), integration tests (database and async behavior), and manual verification of lifecycle behavior.

### 6.1. Unit Tests

- Config default deserialization: verify SynchronizationSettings defaults when section is absent (AC-01, SCENARIO-001)
- Config explicit values: verify non-default values deserialize correctly (AC-01, SCENARIO-002)
- Config roundtrip: serialize then deserialize, assert equality (AC-10, SCENARIO-003)
- Config invalid duration: verify deserialization error on malformed interval (SCENARIO-004)
- PlaylistAddTrack::AT_END value: assert equals u64::MAX
- PlaylistAddTrack::new_append_single: verify at_index is AT_END and tracks contains the single source
- PlaylistAddTrack::new_append_vec: verify at_index is AT_END and tracks contains all sources

### 6.2. Integration Tests

- sync_once with in-memory database and mock HTTP server: verify new episodes are detected, downloaded, and PlaylistAddTrack commands are sent (AC-05, AC-06, AC-07, SCENARIO-010, SCENARIO-014, SCENARIO-015)
- Deduplication: pre-populate database with episodes, run sync_once, verify no duplicates downloaded (SCENARIO-011, SCENARIO-012, SCENARIO-013)
- Error isolation: configure one feed URL to fail (return 500), verify other feeds process normally (AC-08, SCENARIO-017, SCENARIO-018, SCENARIO-019)
- Empty podcast list: run sync_once with no subscribed podcasts, verify clean completion (SCENARIO-021)
- Auto-start verification: send PlaylistAddTrack to an empty queue, verify the handler triggers playback (AC-07, SCENARIO-016)

### 6.3. E2E Tests

- Full lifecycle test: start server with synchronization enabled, verify sync_once executes on startup (SCENARIO-006)
- Disabled sync: start server with enable=false, verify no sync activity (SCENARIO-005, AC-02)
- Graceful shutdown: start sync task, cancel token, verify task exits without panic or resource leak (SCENARIO-009)
- Active playback non-disruption: during playback, trigger a sync pass, verify no audio interruption (SCENARIO-022)

### 6.4. BDD Scenario References

- **SCENARIO-001** -- unit -- Covered (config default deserialization test)
- **SCENARIO-002** -- unit -- Covered (config explicit values test)
- **SCENARIO-003** -- unit -- Covered (config roundtrip test)
- **SCENARIO-004** -- unit -- Covered (config invalid duration test)
- **SCENARIO-005** -- integration -- Covered (disabled sync test)
- **SCENARIO-006** -- integration -- Covered (startup sync test)
- **SCENARIO-007** -- integration -- Covered (no startup sync when disabled test)
- **SCENARIO-008** -- integration -- Covered (periodic interval test)
- **SCENARIO-009** -- integration -- Covered (graceful cancellation test)
- **SCENARIO-010** -- integration -- Covered (new episode detection test)
- **SCENARIO-011** -- integration -- Covered (GUID dedup test)
- **SCENARIO-012** -- integration -- Covered (URL fallback dedup test)
- **SCENARIO-013** -- integration -- Covered (queue dedup test)
- **SCENARIO-014** -- integration -- Covered (download to directory test)
- **SCENARIO-015** -- integration -- Covered (append to end test)
- **SCENARIO-016** -- integration -- Covered (auto-start on empty queue test)
- **SCENARIO-017** -- integration -- Covered (network error isolation test)
- **SCENARIO-018** -- integration -- Covered (malformed feed isolation test)
- **SCENARIO-019** -- integration -- Covered (per-episode download failure isolation test)
- **SCENARIO-020** -- integration -- Covered (task spawn pattern verification)
- **SCENARIO-021** -- integration -- Covered (empty podcast list test)
- **SCENARIO-022** -- e2e -- Covered (playback non-disruption test)
- **SCENARIO-023** -- integration -- Covered (concurrent tick handling via interval_at semantics)

## 7. Non-Functional Requirements

### 7.1. Performance

- All network I/O (feed fetching, episode downloads) runs on the tokio async runtime; the player loop thread is never blocked
- Download concurrency is bounded by `podcast.concurrent_downloads_max` (default 3) via `TaskPool` semaphore, preventing resource exhaustion
- Timer uses `tokio::time::interval_at` which compensates for execution time to prevent drift
- Database connection is opened per sync pass and dropped afterward, minimizing SQLite lock contention with the player loop
- Default 1-hour interval validated by prototype (fastest observed podcast publishes every 23.6 hours)

### 7.2. Reliability

- Per-podcast error isolation: a failing feed (network error, parse error) is logged at `warn` level and does not abort the sync pass (AC-08, SCENARIO-017, SCENARIO-018)
- Per-episode error isolation: a failing download does not prevent other episodes from being processed (SCENARIO-019)
- Fatal errors (cannot open DB) abort only the current sync pass, not the task -- the next interval tick will retry
- Cancellation safety: `select!` on `CancellationToken::cancelled()` ensures clean shutdown without resource leaks (AC-09)
- TaskPool Drop implementation cancels in-flight download tasks when the sync_once scope exits

### 7.3. Backward Compatibility

- `#[serde(default)]` on `SynchronizationSettings` ensures existing config files without a `[synchronization]` section parse without error (AC-01, AC-10)
- No existing `PlayerCmd` variants are modified or added -- the sync task uses the existing `PlaylistAddTrack` variant
- The `new_append_single`/`new_append_vec` constructors are additive; existing `new_single`/`new_vec` remain unchanged
- No changes to the gRPC service API, TUI client, or playback backends

### 7.4. Security

- No new network listeners or ports are opened
- Feed URLs are sourced exclusively from the user's own podcast database (user-subscribed feeds only)
- Downloads are restricted to the configured `podcast.download_dir` path
- The sync task reuses the existing reqwest HTTP client configuration (including connect_timeout)
- SQLite path validation follows the same `get_app_config_path` pattern as existing code

### 7.5. Minimal Dependencies

- Two new crates are introduced: `humantime-serde` version 1.1 and `wiremock` version 0.6 (dev-dependency only)
- `humantime-serde` is a thin wrapper around `humantime` (which is already a minimal, well-audited crate)
- `wiremock` is used exclusively in tests for HTTP integration testing with mock servers
- All other functionality reuses existing dependencies: tokio, tokio-util, reqwest, rusqlite, rss, serde, toml
- Additional dev-dependencies added: `tempfile` (for test database temp dirs) and `chrono` (for test fixture timestamps)

## 8. Risks and Mitigations

- **Risk**: Back-catalog flood on first sync for a podcast with hundreds of episodes
  - Likelihood: medium
  - Impact: medium (fills disk, floods queue)
  - Mitigation: The current spec syncs all new episodes. A future `max_episodes_per_sync` config field can cap this. For MVP, this is acceptable behavior as noted in requirements Open Questions. Users can set `refresh_on_startup: false` and a long interval to control initial load.

- **Risk**: SQLite write contention between sync task and player loop
  - Likelihood: low
  - Impact: low (SQLite handles concurrent access with WAL mode; worst case is a brief retry)
  - Mitigation: Sync task opens its own connection (ADR-002). Writes are infrequent (only when inserting new episodes). Player loop primarily reads.

- **Risk**: Channel-drain pattern hangs if download_list implementation changes
  - Likelihood: low
  - Impact: medium (sync task blocks indefinitely until next cancellation)
  - Mitigation: The channel-drain pattern is panic-safe and cancellation-safe (ADR-003). If `download_list` changes its closure semantics, the `select!` on `cancel_token.cancelled()` provides a safety net at the task level.

- **Risk**: humantime-serde crate becomes unmaintained
  - Likelihood: low (crate is stable with minimal scope)
  - Impact: low (can be replaced with manual parsing or `humantime` direct integration)
  - Mitigation: The crate has minimal surface area. If abandoned, replacing it requires changing only the `#[serde(with = ...)]` annotation on one field.

- **Risk**: Prototype borderline connect_timeout (9.34s vs 10s spec for large feeds)
  - Likelihood: low (the 9.34s was total response time, not connection time)
  - Impact: low (timeout would cause a single feed to fail, not the entire pass)
  - Mitigation: Error isolation ensures a timed-out feed logs a warning and the pass continues. The existing `connect_timeout(5s)` in `get_feed_data` applies to TCP connection only, not body download.

## 9. Implementation Deviations Log

The following deviations from the original specification were identified during implementation and code review:

### DEV-001: humantime-serde version 1.1 instead of 0.2

- **Original spec**: `humantime-serde = "0.2"` (Section 1, Section 7.5)
- **Actual implementation**: `humantime-serde = "1.1"`
- **Reason**: Version 0.2 is outdated; version 1.1 is the current stable release with the same API surface.
- **Impact**: None. API is identical; functionality is identical.

### DEV-002: Custom Deserialize implementation with dual-path parsing

- **Original spec**: Standard `#[serde(default)]` derive-based deserialization (Section 3.1)
- **Actual implementation**: Custom `Deserialize` impl with `SyncSettingsRaw` helper struct for dual-path parsing (nested vs flat TOML)
- **Reason**: Needed to handle both standalone TOML sections (for tests) and nested-field usage inside `ServerSettings`.
- **Impact**: Adds ~60 lines of complexity to `synchronization.rs` but is localized and correct. Identified in adversarial review A-01 as acceptable.

### DEV-003: Test file organization

- **Original spec**: Tests inline within implementation files (`#[cfg(test)] mod tests`)
- **Actual implementation**: Separate `*_tests.rs` files (`synchronization_tests.rs`, `player_playlist_add_track_tests.rs`)
- **Reason**: Improved code organization; test suites are significantly larger than implementation files.
- **Impact**: None on production behavior. Better maintainability.

### DEV-004: Module registration timing (T-22)

- **Original spec**: T-22 (register `mod podcast_sync;`) in Phase 4
- **Actual implementation**: Completed in Phase 3 alongside module creation
- **Reason**: Rust requires module declarations for compilation; without it the new file cannot compile or be tested.
- **Impact**: None. T-22 is effectively a no-op in Phase 4.

### DEV-005: sync_once parameter style

- **Original spec**: Individual parameters for config values in `sync_once` signature
- **Actual implementation**: `sync_once` takes `&SharedServerSettings` as first parameter
- **Reason**: More ergonomic and future-proof; reads all needed values under a single short-lived read lock.
- **Impact**: None on behavior. Slightly different internal API surface.

### DEV-006: Additional dev-dependencies

- **Original spec**: Only `humantime-serde` as new dependency
- **Actual implementation**: Also added `wiremock = "0.6"`, `tempfile = "3"` (dev-dependencies), and `chrono` (dev-dependency)
- **Reason**: Required for integration test infrastructure (mock HTTP, temp directories, timestamps).
- **Impact**: Dev-only; no production binary size increase.

### DEV-007: Test quantity exceeds specification

- **Original spec**: ~7 tests implied across all phases
- **Actual implementation**: 79 tests (19 config + 20 playlist + 20 sync logic + 9 lifecycle + 11 integration)
- **Reason**: Comprehensive coverage of custom implementations, edge cases, and all BDD scenarios.
- **Impact**: None negative. Provides strong regression safety net.
