# Technical Specification: PR #720 Podcast Synchronization — Review Feedback Remediation

- **Date**: 2026-06-25
- **Author**: super-dev:spec-writer
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md
- **Architecture**: ./07-architecture.md

---

## 1. Overview

This specification defines the complete technical design for remediating PR #720's podcast synchronization feature based on 59 reviewer comments. The work restructures the periodic podcast sync feature across five implementation phases: migrating TUI-owned podcast network operations to the server crate, redesigning config placement and database schema for per-podcast scheduling, fixing sync logic correctness issues (shared TaskPool, PodcastUrl track source, non-blocking I/O, configurable enqueue), cleaning up the test suite, and applying style/convention fixes.

The technical approach preserves all existing functionality while addressing architectural deficiencies. The config moves from a top-level `[synchronization]` section to `[podcast.synchronization]`. Per-podcast scheduling replaces global-interval-for-all behavior using a database-backed wake-and-check pattern (Miniflux model). A single shared TaskPool bounds all podcast network concurrency. Blocking filesystem I/O moves outside async contexts via pre-scanning. Episodes are consistently enqueued with `PlaylistTrackSource::PodcastUrl` instead of `PlaylistTrackSource::Path`.

The design introduces zero new dependencies — all changes use existing workspace crates (tokio, rusqlite, tonic/prost, humantime-serde, sanitize-filename) with established patterns already present in the codebase.

## 2. Architecture

### 2.1. Crate Responsibilities After Migration

After Phase 1 (migration), the server crate owns ALL podcast network operations. The TUI sends commands via the existing gRPC/UDS communication layer and never calls `check_feed()` or `download_list()` directly.

- **server crate**: Spawns periodic sync task, dispatches feed fetches and downloads, manages TaskPool, sends playlist commands, reports progress via StreamUpdates
- **lib crate**: Provides shared types (config, DB, podcast parsing, TaskPool, player commands, utilities)
- **playback crate**: Defines `PlayerCmd` enum (at `playback/src/lib.rs:104`), `PlayerCmdSender`, and audio backend — unchanged by this work
- **tui crate**: Sends `PlayerCmd` variants to server, displays sync progress from StreamUpdates — no direct podcast network calls

### 2.2. Module Dependency Graph

```
server/src/podcast_sync.rs
    depends on:
        lib/src/config/v2/server/ (PodcastSettings, SynchronizationSettings)
        lib/src/podcast/ (check_feed, download_list, Episode types)
        lib/src/podcast/db/ (Database, update_last_checked, get_due_podcasts)
        lib/src/taskpool/ (TaskPool)
        lib/src/player.rs (PlaylistAddTrack, PlaylistTrackSource, UpdatePodcastSyncEvents)
        lib/src/utils.rs (create_podcast_dir)
        playback/src/lib.rs (PlayerCmd, PlayerCmdSender)
```

### 2.3. Communication Flow

The server-to-TUI communication for podcast sync events uses the existing `StreamUpdates` gRPC stream. One new variant `UpdatePodcastSync` (field 9) is added to the outer oneof, following the `UpdatePlaylist` sub-message pattern (inner oneof with started/progress/complete/error sub-types).

### 2.4. Scheduling Architecture (ADR-002)

The scheduler uses a global-interval wake-and-check pattern:
1. `tokio::time::interval_at` fires at the configured global interval
2. On each tick, `sync_once` queries the database for podcasts where `(now - last_checked) >= COALESCE(check_interval, global_interval)`
3. Only due podcasts are processed in each pass
4. `last_checked` is updated per-podcast on both success and failure paths

This matches Miniflux's proven architecture for RSS feed scheduling and requires no new dependencies.

## 3. Data Models

### 3.1. SynchronizationSettings

Configuration struct nested under `PodcastSettings` representing the `[podcast.synchronization]` TOML section.

```rust
// Location: lib/src/config/v2/server/synchronization.rs

/// Auto-enqueue behavior for newly downloaded podcast episodes.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutoEnqueue {
    /// Download and add to playlist (oldest first per podcast).
    Enabled,
    /// Download only, do not add to playlist.
    Disabled,
}

impl Default for AutoEnqueue {
    fn default() -> Self {
        Self::Enabled
    }
}

/// Settings for periodic podcast synchronization.
/// Nested under [podcast.synchronization] in the TOML config.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SynchronizationSettings {
    /// How often to check feeds. Duration::ZERO means sync is disabled.
    /// Absent field also means disabled (defaults to Duration::ZERO).
    /// Example: "1h", "30m", "2h30m"
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Whether to run a sync immediately on server startup when sync is enabled.
    /// Default: false
    pub refresh_on_startup: bool,

    /// Maximum new episodes to download per podcast per sync pass.
    /// 0 means unlimited. Default: 5
    pub max_new_episodes: u32,

    /// Whether to auto-enqueue downloaded episodes to the playlist.
    /// Default: Enabled
    pub auto_enqueue: AutoEnqueue,
}

impl Default for SynchronizationSettings {
    fn default() -> Self {
        Self {
            // Duration::ZERO means disabled — absent config means sync is off (AC-05, SCENARIO-008)
            interval: Duration::ZERO,
            refresh_on_startup: false,
            max_new_episodes: 5,
            auto_enqueue: AutoEnqueue::Enabled,
        }
    }
}
```

**Key design decision (addressing review revision)**: The `Default` impl sets `interval` to `Duration::ZERO` so that an absent `[podcast.synchronization]` section means sync is DISABLED. This satisfies AC-05 and SCENARIO-008's backward compatibility requirement. Users must explicitly set a non-zero interval (such as `interval = "1h"`) to enable periodic sync.

### 3.2. PodcastSettings (Updated)

The existing `PodcastSettings` struct gains a `synchronization` field.

```rust
// Location: lib/src/config/v2/server/mod.rs (within PodcastSettings)

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PodcastSettings {
    pub concurrent_downloads_max: NonZeroU8,
    pub max_download_retries: u8,
    pub download_dir: PathBuf,
    /// Periodic synchronization settings. Absent or interval=0 means disabled.
    pub synchronization: SynchronizationSettings,
}
```

### 3.3. Database Schema Migration (002.sql)

Adds per-podcast scheduling column to the podcasts table.

```sql
-- Location: lib/src/podcast/db/migrations/002.sql
-- Migration 002: Add per-podcast sync scheduling support
ALTER TABLE podcasts ADD COLUMN check_interval INTEGER;
```

The `last_checked` column already exists in the podcasts table (001.sql). The `check_interval` column is nullable INTEGER (seconds); NULL means use global interval.

### 3.4. SyncPassStats

Statistics collected during a single sync pass, used for both internal tracking and StreamUpdates reporting.

```rust
// Location: server/src/podcast_sync.rs

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncPassStats {
    pub podcasts_checked: usize,
    pub podcasts_failed: usize,
    pub episodes_downloaded: usize,
    pub episodes_enqueued: usize,
    pub episodes_failed: usize,
}
```

### 3.5. UpdatePodcastSyncEvents

Rust-side enum for podcast sync progress streaming, following the `UpdatePlaylistEvents` pattern.

```rust
// Location: lib/src/player.rs

#[derive(Debug, Clone, PartialEq)]
pub enum UpdatePodcastSyncEvents {
    Started { total_podcasts: u64 },
    Progress {
        podcast_title: String,
        episodes_found: u64,
        episodes_downloaded: u64,
    },
    Complete(PodcastSyncCompleteStats),
    Error {
        podcast_title: String,
        error_message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodcastSyncCompleteStats {
    pub podcasts_checked: u64,
    pub podcasts_failed: u64,
    pub episodes_downloaded: u64,
    pub episodes_enqueued: u64,
}
```

### 3.6. ExistingFilesMap

Pre-scanned filesystem state collected before async processing begins.

```rust
// Location: server/src/podcast_sync.rs (internal type)

/// Map of podcast DB ID to the set of filenames already present in that podcast's
/// download directory. Collected via spawn_blocking before async processing.
type ExistingFilesMap = HashMap<PodcastDBId, HashSet<String>>;
```

## 4. API Design

### 4.1. Protobuf StreamUpdates Extension

```protobuf
// Location: lib/proto/player.proto

message StreamUpdates {
  oneof type {
    // ... existing fields 1-8 ...
    UpdatePodcastSync podcast_sync = 9;
  }
}

message UpdatePodcastSync {
  oneof type {
    PodcastSyncStarted started = 1;
    PodcastSyncProgress progress = 2;
    PodcastSyncComplete complete = 3;
    PodcastSyncError error = 4;
  }
}

message PodcastSyncStarted {
  uint64 total_podcasts = 1;
}

message PodcastSyncProgress {
  string podcast_title = 1;
  uint64 episodes_found = 2;
  uint64 episodes_downloaded = 3;
}

message PodcastSyncComplete {
  uint64 podcasts_checked = 1;
  uint64 podcasts_failed = 2;
  uint64 episodes_downloaded = 3;
  uint64 episodes_enqueued = 4;
}

message PodcastSyncError {
  string podcast_title = 1;
  string error_message = 2;
}
```

### 4.2. Database API: update_last_checked

```rust
// Location: lib/src/podcast/db/podcast_db.rs

/// Update only the `last_checked` timestamp for a podcast.
/// Used on both success and failure paths during sync to record
/// when the feed was last attempted, enabling per-podcast scheduling.
pub fn update_last_checked(
    id: PodcastDBId,
    timestamp: DateTime<Utc>,
    con: &Connection,
) -> Result<usize, rusqlite::Error> {
    let mut stmt = con.prepare_cached(
        "UPDATE podcasts SET last_checked = ? WHERE id = ?;"
    )?;
    stmt.execute(params![timestamp.timestamp(), id])
}
```

**Error Cases:**
- `rusqlite::Error::QueryReturnedNoRows`: ID does not exist (returns Ok(0) rows affected)
- `rusqlite::Error::SqliteFailure`: Database locked or corrupted

### 4.3. Database API: get_due_podcasts

```rust
// Location: lib/src/podcast/db/podcast_db.rs

/// Retrieve podcasts that are due for a feed check.
/// A podcast is due when (now - last_checked) >= effective_interval,
/// where effective_interval = check_interval (per-podcast) or global_interval_secs (fallback).
/// Podcasts with NULL last_checked are always considered due.
pub fn get_due_podcasts(
    global_interval_secs: i64,
    con: &Connection,
) -> Result<Vec<PodcastDB>, rusqlite::Error> {
    let mut stmt = con.prepare_cached(
        "SELECT * FROM podcasts WHERE last_checked IS NULL
         OR (strftime('%s', 'now') - last_checked) >= COALESCE(check_interval, ?)"
    )?;
    stmt.query_map(params![global_interval_secs], |row| {
        PodcastDB::try_from_row_named(row)
    })?.collect()
}
```

**Error Cases:**
- `rusqlite::Error::SqliteFailure`: Database corrupted or schema mismatch

### 4.4. New PlayerCmd Variants

```rust
// Location: playback/src/lib.rs (within existing PlayerCmd enum at line 104)

pub enum PlayerCmd {
    // ... existing variants ...
    /// Request podcast feed refresh for all subscriptions (triggered from TUI).
    PodcastFeedRefresh,
    /// Request download of specific episodes (triggered from TUI).
    PodcastDownloadEpisodes(Vec<EpisodeDownloadRequest>),
}

/// Download request for a single episode.
#[derive(Debug, Clone)]
pub struct EpisodeDownloadRequest {
    pub podcast_id: PodcastDBId,
    pub episode_url: String,
    pub episode_title: String,
}
```

## 5. Implementation Details

### 5.1. PlayerCmd Variant Additions (Phase 1)

New `PlayerCmd` variants are added to `playback/src/lib.rs` (where the enum is defined at line 104) to support TUI-to-server podcast operation delegation. The TUI sends `PlayerCmd::PodcastFeedRefresh` and `PlayerCmd::PodcastDownloadEpisodes(requests)` instead of calling `check_feed()` or `download_list()` directly.

The server's player loop (`server/src/server.rs`) handles these new variants by invoking the existing `lib::podcast::check_feed()` and `lib::podcast::download_list()` functions.

### 5.2. Config Migration (Phase 2)

The `synchronization` field moves from `ServerSettings` to `PodcastSettings`:

1. Remove `pub synchronization: SynchronizationSettings` from `ServerSettings` struct in `lib/src/config/v2/server/mod.rs`
2. Add `pub synchronization: SynchronizationSettings` to `PodcastSettings` struct in the same file
3. Update all access paths from `config.synchronization.*` to `config.podcast.synchronization.*` in:
   - `server/src/podcast_sync.rs`
   - `server/src/server.rs`

The `SynchronizationSettings` Default impl sets `interval` to `Duration::ZERO` so that absent config means disabled (AC-05, SCENARIO-008).

### 5.3. Database Migration Application

The existing migration infrastructure uses `PRAGMA user_version` in `lib/src/podcast/db/migration.rs`. The current version is 1. Migration 002 increments to version 2.

```rust
// In migration.rs, add handling for version 1 -> 2:
if current_version < 2 {
    tx.execute_batch(include_str!("migrations/002.sql"))?;
}
tx.pragma_update(None, "user_version", 2)?;
```

### 5.4. Per-Podcast Scheduling Logic

The `sync_once` function filters podcasts using `get_due_podcasts(global_interval_secs, conn)`. This SQL-based approach:
- Returns podcasts where `last_checked IS NULL` (never checked) OR elapsed time exceeds the effective interval
- Uses `COALESCE(check_interval, ?)` to respect per-podcast overrides
- Costs microseconds for 50 rows — no performance concern (SCENARIO-011, SCENARIO-012, SCENARIO-013)

### 5.5. Pre-scan Filesystem (spawn_blocking)

Before the async feed-processing loop, all podcast download directories are scanned for existing files:

```rust
// Collect existing files outside async context (AC-15, SCENARIO-022, SCENARIO-023)
let existing_files: ExistingFilesMap = tokio::task::spawn_blocking(move || {
    let mut file_map: HashMap<PodcastDBId, HashSet<String>> = HashMap::new();
    for podcast in &due_podcasts_for_scan {
        let pod_dir = create_podcast_dir(&config_snapshot, podcast.title.clone());
        match pod_dir {
            Ok(dir_path) => {
                let filenames: HashSet<String> = std::fs::read_dir(&dir_path)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect();
                file_map.insert(podcast.id, filenames);
            }
            Err(err) => {
                warn!("Failed to scan directory for podcast '{}': {err}", podcast.title);
            }
        }
    }
    file_map
}).await?;
```

### 5.6. Shared TaskPool for All Network Operations

A single `TaskPool` is created once per sync pass (not per-podcast) and used for both feed fetches and episode downloads:

```rust
let shared_task_pool = TaskPool::new(concurrent_downloads_max);
// Used for check_feed() dispatches
// Used for download_list() dispatches
// Bounds total concurrent network operations across all podcasts (AC-10, SCENARIO-014)
```

### 5.7. Episode Filtering: Played+Deleted Exclusion (AC-13)

The filter for which episodes to download uses the pre-scanned `ExistingFilesMap` and the `played` field from `EpisodeDB`. Since `EpisodeDB` has no `path` field (paths are stored in the `files` table via `FileDB`), the filter derives expected filenames from episode metadata using `sanitize_filename`:

```rust
/// Determine if an episode should be downloaded.
/// Skip if: (1) file already exists on disk, or (2) played AND file was deleted.
/// Download if: file does not exist AND episode is not played.
fn should_download_episode(
    episode: &EpisodeDB,
    existing_filenames: &HashSet<String>,
    expected_filename: &str,
) -> bool {
    let file_exists = existing_filenames.contains(expected_filename);
    if file_exists {
        // File already present — skip regardless of played status (SCENARIO-020)
        return false;
    }
    if episode.played {
        // Played and file deleted — exclude from sync (SCENARIO-018)
        return false;
    }
    // File does not exist and episode is not played — download (SCENARIO-019)
    true
}
```

The `expected_filename` is derived from the episode title using `sanitize_filename` with the same options used by `create_podcast_dir`, ensuring consistency.

### 5.8. Enqueue with PlaylistTrackSource::PodcastUrl (AC-14)

All podcast episode enqueue operations use `PlaylistTrackSource::PodcastUrl(episode_url)`:

```rust
// Correct: always PodcastUrl for podcast episodes (SCENARIO-021)
let track_source = PlaylistTrackSource::PodcastUrl(episode.url.clone());
let add_track_command = PlaylistAddTrack::new_append_single(track_source);
```

The `PlaylistAddTrack::new_append_single` method (at `lib/src/player.rs:471`) delegates to the base constructor with `Self::AT_END` sentinel:

```rust
// Existing code in lib/src/player.rs:471-478
pub fn new_append_single(track: PlaylistTrackSource) -> Self {
    Self {
        at_index: Self::AT_END,
        tracks: vec![track],
    }
}
```

Note: The parameter order for `new_single` is `(at_index: u64, track: PlaylistTrackSource)` — at_index comes first (lib/src/player.rs:457).

### 5.9. Podcast Directory Creation (AC-17)

Directory creation reuses the existing utility function. The actual signature is:

```rust
// lib/src/utils.rs:111
pub fn create_podcast_dir(config: &ServerOverlay, pod_title: String) -> Result<PathBuf>
```

Usage in podcast_sync.rs:

```rust
let podcast_download_dir = create_podcast_dir(&config.read(), podcast.title.clone())?;
```

This function handles sanitization (via `sanitize_filename`) and `create_dir_all` internally. No duplicate sanitization logic is needed in `podcast_sync.rs` (SCENARIO-025).

### 5.10. Auto-Enqueue Gating (AC-11)

Episode enqueue is conditional on the `auto_enqueue` config field:

```rust
let sync_config = config.read().settings.podcast.synchronization.clone();
// ... after downloading episodes ...
if sync_config.auto_enqueue == AutoEnqueue::Enabled {
    // Sort episodes oldest-first by pubdate (AC-12, SCENARIO-016, SCENARIO-017)
    downloaded_episodes.sort_by_key(|ep| ep.pubdate);
    for episode in &downloaded_episodes {
        let track_source = PlaylistTrackSource::PodcastUrl(episode.url.clone());
        let command = PlayerCmd::PlaylistAddTrack(
            PlaylistAddTrack::new_append_single(track_source)
        );
        cmd_tx.send(command)?;
        stats.episodes_enqueued += 1;
    }
}
```

Episodes from the same podcast are grouped contiguously and ordered oldest-first (SCENARIO-017).

### 5.11. Combined refresh_on_startup + Periodic Loop (AC-19)

Instead of two separate code paths, the periodic loop uses `interval_at` with start time set to achieve immediate-first-tick when refresh_on_startup is desired:

```rust
let start_time = if sync_config.refresh_on_startup {
    Instant::now() // First tick fires immediately (SCENARIO-027)
} else {
    Instant::now() + interval_duration
};
let mut sync_interval = tokio::time::interval_at(start_time, interval_duration);
sync_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
```

### 5.12. Minimum Interval Clamp

When `interval > Duration::ZERO` (sync enabled), the actual interval is clamped to a minimum of 1 second to prevent `tokio::time::interval_at` from receiving `Duration::ZERO`:

```rust
const MINIMUM_SYNC_INTERVAL: Duration = Duration::from_secs(1);

let interval_duration = sync_config.interval.max(MINIMUM_SYNC_INTERVAL);
```

### 5.13. sync_once Helper Extraction (AC-30)

The monolithic 280-line `sync_once` function is decomposed into named helpers:

```rust
/// Process a successful feed fetch result: update DB, filter episodes, dispatch downloads.
async fn process_feed_result(
    podcast_id: PodcastDBId,
    podcast_data: PodcastData,
    db: &Database,
    existing_files: &ExistingFilesMap,
    sync_config: &SynchronizationSettings,
    task_pool: &TaskPool,
    cmd_tx: &PlayerCmdSender,
    stats: &mut SyncPassStats,
) -> Result<()>;

/// Determine which episodes from a feed result need downloading.
fn find_episodes_to_download(
    episodes: &[EpisodeDB],
    existing_filenames: &HashSet<String>,
    max_new_episodes: u32,
) -> Vec<&EpisodeDB>;

/// Drain download results from a channel and optionally enqueue to playlist.
async fn drain_download_results(
    download_rx: UnboundedReceiver<DownloadResult>,
    auto_enqueue: AutoEnqueue,
    cmd_tx: &PlayerCmdSender,
    stats: &mut SyncPassStats,
) -> Result<()>;
```

## 6. Testing Strategy

The testing approach emphasizes observable outcomes over implementation details. Redundant tests verifying Rust language semantics are removed. A `TestHarness` builder pattern provides shared setup infrastructure.

### 6.1. Unit Tests

- Verify `SynchronizationSettings::default()` produces `interval = Duration::ZERO` (disabled by default)
- Verify `AutoEnqueue` serde round-trip for "enabled" and "disabled" string values
- Verify `should_download_episode` returns correct boolean for all 4 combinations of played/file-exists states
- Verify `get_due_podcasts` SQL filtering with varied `last_checked` timestamps and `check_interval` overrides
- Verify `update_last_checked` writes correct timestamp to database
- Verify `SyncPassStats` accumulation across multiple podcasts
- Verify episode ordering logic (oldest-first sort by pubdate within same podcast)
- Verify config deserialization from TOML with `[podcast.synchronization]` section
- Verify absent `[podcast.synchronization]` defaults to disabled

### 6.2. Integration Tests

- Verify full sync pass with wiremock server: feed discovery, download, enqueue via spy channel
- Verify sync pass skips podcasts not yet due (per-podcast scheduling)
- Verify sync pass handles feed fetch failure with error isolation (one podcast fails, others succeed)
- Verify auto-enqueue disabled mode: episodes downloaded but not enqueued
- Verify pre-scan correctly identifies existing files and skips re-download
- Verify played+deleted episodes are excluded from download
- Verify PodcastUrl track source is used for all enqueued episodes (not Path)
- Verify shared TaskPool bounds total concurrent operations
- Verify empty podcast list completes immediately without error
- Verify all download failures still update last_checked

### 6.3. E2E Tests

- Verify TUI manual refresh command reaches server and triggers feed check
- Verify periodic sync task starts on server startup when interval is non-zero
- Verify periodic sync task does NOT start when interval is zero or absent
- Verify StreamUpdates broadcasts PodcastSyncComplete after sync pass

### 6.4. BDD Scenario References

- **SCENARIO-001** — integration — Covered (server owns feed refresh after Phase 1)
- **SCENARIO-002** — integration — Covered (TUI delegates via PlayerCmd)
- **SCENARIO-003** — e2e — Covered (manual refresh works identically)
- **SCENARIO-004** — integration — Covered (OPML import/export unchanged)
- **SCENARIO-005** — integration — Covered (server syncs without TUI)
- **SCENARIO-006** — unit — Covered (config nested under podcast.synchronization)
- **SCENARIO-007** — unit — Covered (interval=0 disables sync)
- **SCENARIO-008** — unit — Covered (absent interval defaults to Duration::ZERO = disabled)
- **SCENARIO-009** — unit — Covered (refresh_on_startup defaults to false)
- **SCENARIO-010** — integration — Covered (last_checked stored after feed check)
- **SCENARIO-011** — integration — Covered (per-podcast scheduling with due filtering)
- **SCENARIO-012** — integration — Covered (per-podcast interval override)
- **SCENARIO-013** — integration — Covered (missing override falls back to global)
- **SCENARIO-014** — integration — Covered (single shared TaskPool)
- **SCENARIO-015** — integration — Covered (auto-enqueue disabled)
- **SCENARIO-016** — integration — Covered (auto-enqueue enabled, oldest first)
- **SCENARIO-017** — integration — Covered (contiguous per-podcast groups)
- **SCENARIO-018** — unit — Covered (played+deleted excluded)
- **SCENARIO-019** — unit — Covered (unplayed+deleted re-downloaded)
- **SCENARIO-020** — unit — Covered (existing file skipped)
- **SCENARIO-021** — integration — Covered (PodcastUrl source)
- **SCENARIO-022** — integration — Covered (pre-scan before async loop)
- **SCENARIO-023** — integration — Covered (spawn_blocking for large dirs)
- **SCENARIO-024** — integration — Covered (downloads non-blocking)
- **SCENARIO-025** — unit — Covered (create_podcast_dir reused)
- **SCENARIO-026** — unit — Covered (new_append_single delegates to base)
- **SCENARIO-027** — unit — Covered (interval_at with Instant::now)
- **SCENARIO-028** — unit — Covered (redundant tests removed)
- **SCENARIO-029** — unit — Covered (localhost-only test URLs)
- **SCENARIO-030** — unit — Covered (specific error variant assertions)
- **SCENARIO-031** — unit — Covered (TestHarness eliminates boilerplate)
- **SCENARIO-032** — integration — Covered (spy channel verifies outcomes)
- **SCENARIO-033** — unit — Covered (module //! doc comments)
- **SCENARIO-034** — unit — Covered (helper extraction at 3+ nesting)
- **SCENARIO-035** — unit — Covered (config struct references)
- **SCENARIO-036** — integration — Covered (empty subscription list)
- **SCENARIO-037** — integration — Covered (zero new episodes)
- **SCENARIO-038** — integration — Covered (concurrent sync deduplication)
- **SCENARIO-039** — integration — Covered (timeout isolation)
- **SCENARIO-040** — unit — Covered (large interval accepted)
- **SCENARIO-041** — integration — Covered (last_checked updated on failure)
- **SCENARIO-042** — integration — Covered (empty directory handled)

## 7. Non-Functional Requirements

### 7.1. Performance

- The global-interval wake-and-check adds at most `interval_duration` latency before a due podcast is processed — acceptable for hourly podcast feeds
- SQL filtering for 50 podcasts costs microseconds (negligible versus network I/O)
- Pre-scan via `spawn_blocking` prevents blocking the tokio async thread pool for directories with thousands of files
- Single shared TaskPool with `concurrent_downloads_max` bounds total network concurrency
- `prepare_cached` used for `update_last_checked` avoids repeated SQL parsing on the hot path

### 7.2. Reliability

- Per-podcast error isolation: one failed feed does not abort the entire sync pass (SCENARIO-039)
- `last_checked` updated on both success and failure paths — scheduler always advances (SCENARIO-041)
- Partial failures (some episodes fail download) do not corrupt database state
- Database is single source of truth for scheduling — survives server restarts without in-memory timer reconstruction

### 7.3. Backward Compatibility

- Absent `[podcast.synchronization]` section means sync is DISABLED (interval defaults to `Duration::ZERO`)
- Existing config files without a synchronization section continue working unchanged — no sync is triggered
- Old TUI clients ignore the unknown `podcast_sync = 9` protobuf field (proto backward compatibility)
- All existing podcast functionality (manual refresh, download, OPML) works identically after migration

### 7.4. Security

- No new attack surface — all network operations use existing reqwest client with established timeouts
- UDS socket provides access control for gRPC communication
- Database operations use parameterized queries (no SQL injection)
- Test URLs restricted to localhost/127.0.0.1 (AC-22)

## 8. Risks and Mitigations

- **Risk**: TUI-to-server migration breaks existing podcast manual refresh workflow
  - Likelihood: medium
  - Impact: high
  - Mitigation: Integration tests verify identical behavior pre/post migration. TUI sends same commands, server returns same results. Phased rollout allows catching regressions early.

- **Risk**: Database migration (002.sql) fails on existing installations with podcast data
  - Likelihood: low
  - Impact: high
  - Mitigation: `ALTER TABLE ADD COLUMN` with nullable column is safe for existing data. `user_version` pragma ensures migration runs exactly once. Test migration on database with existing podcast rows.

- **Risk**: Pre-scan `spawn_blocking` call completes slowly for very large podcast directories
  - Likelihood: low
  - Impact: low
  - Mitigation: `spawn_blocking` does not block the async runtime. The sync pass waits for the scan to complete but this happens once per pass (not per-podcast inside the loop). Typical podcast directories have 50-500 files.

- **Risk**: Concurrent sync passes create duplicate downloads
  - Likelihood: low
  - Impact: medium
  - Mitigation: The `interval_at` timer with `MissedTickBehavior::Delay` ensures the next tick does not fire until the previous sync pass completes. At-most-one sync pass runs at a time (SCENARIO-038).

- **Risk**: `max_new_episodes = 0` (unlimited) combined with a podcast backlog of hundreds of episodes overwhelms bandwidth
  - Likelihood: medium
  - Impact: medium
  - Mitigation: Default `max_new_episodes = 5` prevents this for users who do not explicitly configure unlimited. The shared TaskPool bounds concurrent downloads regardless of episode count.

---

## Appendix: Phase Numbering Mapping

| Implementation Plan Phase | Requirements Phase | Description |
|--------------------------|-------------------|-------------|
| Phase 1 | Phase 0 | Prerequisites and Migration |
| Phase 2 | Phase 1 | Architecture and Config Redesign |
| Phase 3 | Phase 2 | Sync Logic Correctness |
| Phase 4 | Phase 3 | Test Quality |
| Phase 5 | Phase 4 | Style and Conventions |

This mapping table resolves the numbering inconsistency between requirements (Phase 0-4) and the implementation plan (Phase 1-5). The implementation plan uses 1-based numbering; requirements use 0-based numbering. Both refer to the same logical phases.

## Appendix: File Inventory

### Files to Create

| File | Purpose |
|------|---------|
| `lib/src/podcast/db/migrations/002.sql` | Database migration adding check_interval column |

### Files to Modify

| File | Changes |
|------|---------|
| `playback/src/lib.rs` | Add PodcastFeedRefresh, PodcastDownloadEpisodes variants to PlayerCmd enum |
| `lib/src/config/v2/server/mod.rs` | Move synchronization from ServerSettings to PodcastSettings |
| `lib/src/config/v2/server/synchronization.rs` | Change Default impl: interval=Duration::ZERO, refresh_on_startup=false; add AutoEnqueue enum |
| `lib/src/podcast/db/podcast_db.rs` | Add update_last_checked standalone function, add get_due_podcasts query |
| `lib/src/podcast/db/migration.rs` | Handle version 1->2 upgrade (apply 002.sql) |
| `lib/src/podcast/db/mod.rs` | Re-export update_last_checked and get_due_podcasts |
| `lib/proto/player.proto` | Add UpdatePodcastSync variant (field 9) and sub-messages |
| `lib/src/player.rs` | Add UpdatePodcastSyncEvents enum, PodcastSync variant to UpdateEvents, From impls |
| `server/src/podcast_sync.rs` | Rewrite sync_once: shared TaskPool, pre-scan, PodcastUrl source, auto-enqueue gating, helper extraction |
| `server/src/server.rs` | Update config access path, add PodcastFeedRefresh/PodcastDownloadEpisodes handlers |
| `tui/src/ui/components/podcast.rs` | Replace direct check_feed()/download_list() calls with PlayerCmd sends |
| `lib/src/config/v2/server/synchronization_tests.rs` | Update tests for new defaults and config structure |

### Files to Delete

None. All changes are modifications or additions.
