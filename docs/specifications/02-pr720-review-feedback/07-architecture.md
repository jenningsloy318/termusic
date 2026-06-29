---
name: architecture
description: Architecture document for PR #720 podcast synchronization review feedback remediation — phased redesign of the periodic podcast sync feature.
doc-type: architecture
gate-profile: null
---

<document type="architecture">

<metadata>
  <field name="title">Architecture: PR #720 Podcast Synchronization — Review Feedback Remediation</field>
  <field name="date">2026-06-25</field>
  <field name="author">Claude</field>
  <field name="status">Draft</field>
</metadata>

<section title="Overview">
  <paragraph>This architecture redesigns the PR #720 podcast synchronization feature to address 59 reviewer comments. The work is decomposed into four phases: (Phase 0) migrate existing TUI podcast sync logic to the server crate, (Phase 1) redesign config placement and add per-podcast scheduling infrastructure, (Phase 2) fix sync logic correctness issues (shared TaskPool, PodcastUrl source, blocking I/O, enqueue behavior), and (Phase 3) clean up the test suite. Each phase is independently reviewable and produces a functioning, backward-compatible state.</paragraph>
</section>

<section title="Architectural Drivers">
  <list type="unordered">
    <item>Reviewer requirement: server must own all podcast network operations before periodic sync can land (AC-01, AC-02)</item>
    <item>Config must be nested under [podcast.synchronization] not top-level [synchronization] (AC-04)</item>
    <item>Per-podcast scheduling via last_checked/check_interval database columns with global-interval wake-and-check pattern (AC-08, AC-09)</item>
    <item>Single shared TaskPool for all podcast network operations — feed fetches AND downloads (AC-10)</item>
    <item>Zero blocking I/O in async contexts — pre-scan filesystem before async loop (AC-15)</item>
    <item>Configurable enqueue behavior with correct PodcastUrl track source (AC-11, AC-14)</item>
    <item>Minimal architecture change — proven patterns over novel approaches (Miniflux global-interval model)</item>
  </list>
</section>

<section title="Module Architecture">
  <diagram type="ascii">
+-----------------------------------------------------------------------+
|                         server crate                                   |
|                                                                       |
|  +---------------------+     +-----------------------------+          |
|  | podcast_sync        |     | server (main loop)          |          |
|  |---------------------|     |-----------------------------|          |
|  | start_sync_task()   |<----| spawns sync task on startup |          |
|  | sync_once()         |     | owns CancellationToken      |          |
|  | process_feed_result |     +-----------------------------+          |
|  | drain_downloads()   |                                              |
|  | find_episodes_to_dl |                                              |
|  +--------|------------+                                              |
|           |                                                           |
+-----------|----- gRPC/UDS commands ------------------------------------+
            |
            v
+-----------------------------------------------------------------------+
|                           lib crate                                    |
|                                                                       |
|  +-----------------------+  +--------------------+  +---------------+ |
|  | config/v2/server/     |  | podcast/           |  | taskpool      | |
|  |-----------------------|  |--------------------|  |---------------| |
|  | PodcastSettings       |  | check_feed()       |  | TaskPool::new | |
|  |   .synchronization    |  | download_list()    |  | TaskPool::run | |
|  | SyncSettings (nested) |  | OPML import/export |  +---------------+ |
|  +-----------------------+  +---------|----------+                    |
|                                       |                               |
|                             +---------|----------+  +---------------+ |
|                             | podcast/db/        |  | player.rs     | |
|                             |--------------------|  |---------------| |
|                             | Database           |  | PlaylistAdd-  | |
|                             | PodcastDBInsertable|  |   Track       | |
|                             | update_last_checked|  | PodcastUrl    | |
|                             | get_due_podcasts() |  | AT_END        | |
|                             | migration (002.sql)|  +---------------+ |
|                             +--------------------+                    |
|                                                                       |
|  +-----------------------+                                            |
|  | utils.rs              |                                            |
|  |-----------------------|                                            |
|  | create_podcast_dir()  |                                            |
|  +-----------------------+                                            |
+-----------------------------------------------------------------------+
            ^
            |  (after Phase 0 migration)
            |
+-----------------------------------------------------------------------+
|                         tui crate                                      |
|  +---------------------+                                              |
|  | podcast UI          |  Commands only — no direct feed/download     |
|  | (sends PlayerCmd)   |  calls after migration                       |
|  +---------------------+                                              |
+-----------------------------------------------------------------------+
  </diagram>
</section>

<section title="Module Specifications">

  <subsection title="Module 1: podcast_sync (server crate)">
    <field name="purpose">Orchestrate periodic podcast feed synchronization with per-podcast scheduling, download, and optional playlist enqueue.</field>
    <list type="unordered" label="Responsibilities">
      <item>Spawn periodic sync task with CancellationToken and interval_at timer</item>
      <item>Filter podcasts by due-for-check status using last_checked + effective_interval</item>
      <item>Pre-scan podcast directories for existing files (outside async context)</item>
      <item>Dispatch feed fetches and downloads through a single shared TaskPool</item>
      <item>Enqueue downloaded episodes using PlaylistTrackSource::PodcastUrl (if enqueue enabled)</item>
      <item>Update last_checked timestamp per-podcast on both success and failure paths</item>
      <item>Report sync progress via UpdatePodcastSync stream events</item>
    </list>
    <field name="dependencies">lib::config, lib::podcast, lib::podcast::db, lib::taskpool, lib::player::playlist_helpers, lib::utils</field>
    <field name="public-interface">
      <code lang="rust">
/// Statistics collected during a single sync pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPassStats {
    pub podcasts_checked: usize,
    pub podcasts_failed: usize,
    pub episodes_downloaded: usize,
    pub episodes_enqueued: usize,
    pub episodes_failed: usize,
}

/// Execute one full sync pass for podcasts that are due.
pub async fn sync_once(
    config: &amp;SharedServerSettings,
    cmd_tx: &amp;PlayerCmdSender,
    db_path: &amp;Path,
) -> Result&lt;SyncPassStats&gt;;

/// Spawn the periodic podcast sync task.
/// Only call when synchronization is enabled (interval > 0).
pub fn start_podcast_sync_task(
    handle: tokio::runtime::Handle,
    cancel_token: CancellationToken,
    config: SharedServerSettings,
    cmd_tx: PlayerCmdSender,
    db_path: PathBuf,
);
      </code>
    </field>
  </subsection>

  <subsection title="Module 2: config/v2/server (SynchronizationSettings nested under PodcastSettings)">
    <field name="purpose">Provide deserialization and defaults for podcast sync configuration nested under the [podcast.synchronization] TOML section.</field>
    <list type="unordered" label="Responsibilities">
      <item>Define SynchronizationSettings with interval (Duration), auto_enqueue (enum), max_new_episodes (u32), refresh_on_startup (bool)</item>
      <item>Condense enable + interval into single field: interval = 0 (or absent) means disabled</item>
      <item>Nest under PodcastSettings.synchronization field</item>
      <item>Provide Default impl with human-readable comments on constants</item>
    </list>
    <field name="dependencies">serde, humantime-serde</field>
    <field name="public-interface">
      <code lang="rust">
/// Auto-enqueue behavior for newly downloaded episodes.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutoEnqueue {
    /// Download and add to playlist (oldest first per podcast)
    Enabled,
    /// Download only, do not add to playlist
    Disabled,
}

/// Settings for periodic podcast synchronization.
/// Nested under [podcast.synchronization] in the TOML config.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SynchronizationSettings {
    /// How often to check feeds. Duration::ZERO or absence disables sync.
    /// Default: 3600s (1 hour)
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Whether to run a sync immediately on server startup.
    /// Default: true
    pub refresh_on_startup: bool,

    /// Maximum new episodes to download per podcast per pass.
    /// 0 = unlimited. Default: 5
    pub max_new_episodes: u32,

    /// Whether to auto-enqueue downloaded episodes.
    /// Default: Enabled
    pub auto_enqueue: AutoEnqueue,
}

/// Updated PodcastSettings with nested synchronization.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PodcastSettings {
    pub concurrent_downloads_max: NonZeroU8,
    pub max_download_retries: u8,
    pub download_dir: PathBuf,
    pub synchronization: SynchronizationSettings,
}
      </code>
    </field>
  </subsection>

  <subsection title="Module 3: podcast/db (Database schema and per-podcast scheduling)">
    <field name="purpose">Manage podcast SQLite persistence including per-podcast scheduling metadata (last_checked, check_interval columns).</field>
    <list type="unordered" label="Responsibilities">
      <item>Apply 002.sql migration: add check_interval INTEGER column to podcasts table</item>
      <item>Provide update_last_checked(id, timestamp, conn) standalone function</item>
      <item>Provide get_due_podcasts(global_interval, conn) query that filters by elapsed time</item>
      <item>Maintain existing insert/update/delete operations unchanged</item>
    </list>
    <field name="dependencies">rusqlite, chrono, anyhow</field>
    <field name="public-interface">
      <code lang="rust">
/// Update only the last_checked timestamp for a podcast.
/// Used on both success and failure paths during sync.
pub fn update_last_checked(
    id: PodcastDBId,
    timestamp: DateTime&lt;Utc&gt;,
    con: &amp;Connection,
) -> Result&lt;usize, rusqlite::Error&gt;;

/// Retrieve podcasts that are due for a feed check.
/// A podcast is due when (now - last_checked) >= effective_interval,
/// where effective_interval = check_interval ?? global_interval.
pub fn get_due_podcasts(
    global_interval_secs: i64,
    con: &amp;Connection,
) -> Result&lt;Vec&lt;PodcastDB&gt;, rusqlite::Error&gt;;
      </code>
    </field>
  </subsection>

  <subsection title="Module 4: podcast/db/migrations/002.sql">
    <field name="purpose">Add per-podcast scheduling column to the podcasts table.</field>
    <list type="unordered" label="Responsibilities">
      <item>ALTER TABLE podcasts ADD COLUMN check_interval INTEGER; (nullable, seconds)</item>
    </list>
    <field name="dependencies">001.sql (base schema)</field>
    <field name="public-interface">
      <code lang="sql">
-- Migration 002: Add per-podcast sync scheduling support
ALTER TABLE podcasts ADD COLUMN check_interval INTEGER;
      </code>
    </field>
  </subsection>

  <subsection title="Module 5: proto/player.proto (UpdatePodcastSync sub-message)">
    <field name="purpose">Define protobuf messages for podcast sync progress streaming to TUI clients via the existing StreamUpdates oneof.</field>
    <list type="unordered" label="Responsibilities">
      <item>Add single UpdatePodcastSync variant (field 9) to StreamUpdates oneof</item>
      <item>Define inner oneof with started/progress/complete/error sub-types</item>
      <item>Follow existing UpdatePlaylist sub-message pattern exactly</item>
    </list>
    <field name="dependencies">player.proto (existing StreamUpdates message)</field>
    <field name="public-interface">
      <code lang="protobuf">
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

message PodcastSyncStarted { uint64 total_podcasts = 1; }
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
      </code>
    </field>
  </subsection>

  <subsection title="Module 6: lib/player.rs (UpdatePodcastSyncEvents enum)">
    <field name="purpose">Provide the Rust-side enum representation of podcast sync update events, with From impl for the protobuf type.</field>
    <list type="unordered" label="Responsibilities">
      <item>Define UpdatePodcastSyncEvents enum matching proto sub-message</item>
      <item>Add PodcastSync(UpdatePodcastSyncEvents) variant to UpdateEvents enum</item>
      <item>Implement From conversions between Rust enum and proto types</item>
    </list>
    <field name="dependencies">lib/proto/player.proto (generated types)</field>
    <field name="public-interface">
      <code lang="rust">
#[derive(Debug, Clone, PartialEq)]
pub enum UpdatePodcastSyncEvents {
    Started { total_podcasts: u64 },
    Progress { podcast_title: String, episodes_found: u64, episodes_downloaded: u64 },
    Complete(SyncPassStats),
    Error { podcast_title: String, error_message: String },
}

// Added to existing UpdateEvents enum:
pub enum UpdateEvents {
    // ... existing variants ...
    PodcastSync(UpdatePodcastSyncEvents),
}
      </code>
    </field>
  </subsection>

</section>

<section title="Data Flow">
  <diagram type="ascii">
=== Periodic Sync Pass (Phase 2 final design) ===

Server Startup
    |
    v
[config.podcast.synchronization.interval > 0?]
    |yes                              |no
    v                                 v (skip)
start_podcast_sync_task()
    |
    |--[refresh_on_startup?]--yes--> sync_once() (immediate)
    |                                    |
    v                                    v
interval_at(now + interval, interval)
    |
    v (on each tick)
sync_once(config, cmd_tx, db_path)
    |
    +--1. Open DB connection
    |
    +--2. get_due_podcasts(global_interval) --> Vec<PodcastDB>
    |      (SQL: WHERE (now - last_checked) >= COALESCE(check_interval, global))
    |
    +--3. Pre-scan: for each due podcast, read_dir(pod_download_dir)
    |      --> HashMap<PodcastId, HashSet<PathBuf>>  [BLOCKING, outside async]
    |      Uses tokio::task::spawn_blocking or done before async loop
    |
    +--4. Create SINGLE shared TaskPool(concurrent_downloads_max)
    |
    +--5. Dispatch feed fetches via check_feed() + shared TaskPool
    |      --> feed_rx channel
    |
    +--6. Drain feed_rx:
    |      |
    |      +--SyncData(pod_id, pod_data):
    |      |    - db.update_podcast(pod_id, pod_data) [updates last_checked]
    |      |    - get_episodes(pod_id, undownloaded only)
    |      |    - filter: skip played+deleted (AC-13)
    |      |    - check pre-scanned existing files (no blocking I/O here)
    |      |    - limit to max_new_episodes
    |      |    - create_podcast_dir() via lib::utils (AC-17)
    |      |    - dispatch download_list() via SAME shared TaskPool
    |      |    - drain dl_rx (non-blocking: downloads run as separate tasks)
    |      |    - if auto_enqueue == Enabled:
    |      |        enqueue with PlaylistTrackSource::PodcastUrl(ep.url)
    |      |        episodes ordered oldest-first per podcast (AC-12)
    |      |
    |      +--Error(feed):
    |           - update_last_checked(pod_id, now) [advance scheduler]
    |           - log warning, increment stats.podcasts_failed
    |
    +--7. Return SyncPassStats
    |
    +--8. Send UpdatePodcastSync::Complete via broadcast channel
  </diagram>
</section>

<section title="Technology Stack">
  <table>
    <row header="true">
      <cell>Layer</cell>
      <cell>Technology</cell>
      <cell>Rationale</cell>
    </row>
    <row>
      <cell>Async runtime</cell>
      <cell>tokio 1.52 (existing)</cell>
      <cell>Already workspace dependency; interval_at, select!, spawn_blocking</cell>
    </row>
    <row>
      <cell>Task concurrency</cell>
      <cell>lib::taskpool::TaskPool (existing)</cell>
      <cell>Semaphore-based bounded concurrency already in codebase</cell>
    </row>
    <row>
      <cell>Database</cell>
      <cell>rusqlite 0.39 bundled (existing)</cell>
      <cell>Already manages podcast schema; user_version migration infrastructure in place</cell>
    </row>
    <row>
      <cell>IPC/Streaming</cell>
      <cell>tonic/prost 0.14 (existing)</cell>
      <cell>Existing gRPC service with StreamUpdates; add one proto variant</cell>
    </row>
    <row>
      <cell>Config parsing</cell>
      <cell>serde + toml + humantime-serde (existing)</cell>
      <cell>Already used for all server config; Duration formatting established</cell>
    </row>
    <row>
      <cell>Scheduling</cell>
      <cell>tokio::time::interval_at (existing pattern)</cell>
      <cell>Global-interval wake-and-check pattern; matches start_playlist_save_interval</cell>
    </row>
    <row>
      <cell>Cancellation</cell>
      <cell>tokio-util CancellationToken (existing)</cell>
      <cell>Already used for server shutdown coordination</cell>
    </row>
    <row>
      <cell>Filesystem</cell>
      <cell>std::fs (existing) + tokio::task::spawn_blocking</cell>
      <cell>Pre-scan directories outside async context; no new dependencies</cell>
    </row>
  </table>
</section>

<section title="ADRs">

### ADR-001: Config Placement — Nest Under [podcast.synchronization]

**Status**: Accepted

**Context and Problem Statement**: The current implementation places `SynchronizationSettings` as a top-level field on `ServerSettings` (alongside `com`, `player`, `podcast`, `backends`, `metadata`). The reviewer requires it under `[podcast]` since these settings are exclusively podcast-related.

**Decision Drivers**:
- Reviewer explicit requirement (AC-04)
- Semantic correctness — sync settings are podcast-specific
- Consistency with how podcast settings group related concerns
- Backward compatibility with existing config files

**Considered Options**:
1. Nest as `PodcastSettings.synchronization: SynchronizationSettings` (field on existing struct)
2. Merge all sync fields directly into `PodcastSettings` (flat)
3. Keep top-level `[synchronization]` section (current, rejected by reviewer)

**Decision Outcome**: Option 1 — Add `synchronization: SynchronizationSettings` field to `PodcastSettings`.

**Rationale**: Keeps synchronization settings grouped in their own struct (clear separation), renders as `[podcast.synchronization]` in TOML (reviewer's stated preference), allows the struct to grow independently (max_new_episodes, auto_enqueue, future fields) without bloating PodcastSettings, and maintains serde(default) behavior for backward compatibility.

**Pros and Cons**:
- Pro: Exact match for reviewer requirement
- Pro: Clean TOML rendering
- Pro: Struct-level defaults via `#[serde(default)]`
- Con: One extra nesting level in Rust access (`config.podcast.synchronization.interval`)
- Con: Requires updating all references (5 files per code assessment)

**Evaluation Matrix**:

| Criterion (Weight) | Option 1: Nested field | Option 2: Flat merge | Option 3: Top-level |
|---------------------|----------------------|--------------------|--------------------|
| Modularity (0.10) | 5 | 3 | 4 |
| Coupling (0.10) | 5 | 4 | 3 |
| Project Fit | 5 | 4 | 1 |
| Reviewer Approval | 5 | 4 | 0 |
| Migration Effort | 4 | 3 | 5 |

---

### ADR-002: Scheduler Strategy — Global Interval Wake-and-Check

**Status**: Accepted

**Context and Problem Statement**: The periodic sync needs to check podcasts at potentially different intervals (global default + per-podcast override). How should the scheduler determine when to process each podcast?

**Decision Drivers**:
- Simplicity — minimal architecture change from current implementation
- Proven pattern — Miniflux uses this for thousands of feeds (SRC-026)
- Per-podcast scheduling must be supported (AC-08, AC-09)
- Graceful restart behavior (no in-memory timer state to reconstruct)
- 5-50 podcasts typical for termusic users

**Considered Options**:
1. Global interval wake-and-check with SQL filter (Miniflux pattern)
2. tokio-util::time::DelayQueue with per-podcast timers
3. Per-podcast spawned tasks with individual sleep_until futures

**Decision Outcome**: Option 1 — Global interval wake-and-check.

**Rationale**: The SQL filter `WHERE (strftime('%s','now') - last_checked) >= COALESCE(check_interval, ?)` costs microseconds for 50 rows. DelayQueue adds feature dependencies and complex lifecycle management (Key tracking, reset on config change) with no measurable benefit at this scale. Per-podcast tasks create N tasks for N podcasts with complex synchronization.

**Pros and Cons**:
- Pro: Minimal change from existing interval_at architecture
- Pro: Database is single source of truth (survives restarts)
- Pro: No new dependencies
- Pro: Easy to debug (one wake, one query, one batch)
- Con: Up to global_interval latency before a newly-due podcast is processed
- Con: Not optimal for sub-minute precision (unnecessary for podcast feeds)

**Evaluation Matrix**:

| Criterion (Weight) | Option 1: Global wake | Option 2: DelayQueue | Option 3: Per-task |
|---------------------|---------------------|--------------------|--------------------|
| Modularity (0.10) | 4 | 4 | 3 |
| Coupling (0.10) | 5 | 4 | 3 |
| Scalability (0.10) | 3 | 5 | 4 |
| Performance (0.10) | 4 | 5 | 5 |
| Impl Complexity (0.08) | 5 | 3 | 2 |
| Risk (0.08) | 5 | 3 | 2 |
| Maintainability (0.04) | 5 | 3 | 2 |
| Testability (0.03) | 5 | 4 | 3 |
| **Weighted Total** | **4.4** | **3.9** | **3.1** |

---

### ADR-003: Enqueue Track Source — PodcastUrl over Path

**Status**: Accepted

**Context and Problem Statement**: When podcast episodes are enqueued to the playlist, should the track source reference the local file path or the podcast episode URL?

**Decision Drivers**:
- Reviewer flagged Path usage twice as WRONG (AC-14)
- PodcastUrl variant exists specifically for podcast episodes (lib/src/player.rs:403)
- Resume/re-download behavior depends on URL being preserved
- Existing PlaylistTrackSource enum already has the PodcastUrl variant

**Considered Options**:
1. Always use PlaylistTrackSource::PodcastUrl(episode_url) for podcast episodes
2. Use PlaylistTrackSource::Path when file exists locally (current, wrong)
3. Use both (custom compound source) — over-engineering

**Decision Outcome**: Option 1 — Always PodcastUrl.

**Rationale**: The `PodcastUrl` variant was designed for exactly this purpose. It enables: re-download if local file is deleted, position tracking keyed by episode URL, and proper podcast-specific UI behavior in the TUI. Using `Path` breaks these features.

---

### ADR-004: Blocking I/O Strategy — Pre-scan with spawn_blocking

**Status**: Accepted

**Context and Problem Statement**: The existing-file detection uses `std::fs::read_dir` inside the async recv loop. This blocks the tokio worker thread and violates AC-15.

**Decision Drivers**:
- Zero blocking I/O in async contexts (AC-15, SCENARIO-022/023)
- Large podcast directories (thousands of files) could block for milliseconds
- Need the file listing to determine which episodes to skip

**Considered Options**:
1. Pre-scan all podcast directories via `tokio::task::spawn_blocking` before the async processing loop, collecting results into a `HashMap<PodcastId, HashSet<PathBuf>>`
2. Use `tokio::fs::read_dir` (async filesystem API) inside the loop
3. Move the entire sync_once function into spawn_blocking

**Decision Outcome**: Option 1 — Pre-scan via spawn_blocking before async loop.

**Rationale**: Single blocking call outside the async context is simplest. The file set is immutable during the sync pass (no concurrent downloads have started yet). tokio::fs adds unnecessary complexity for a one-time scan. Moving everything to spawn_blocking would lose the benefit of async channel draining.

---

### ADR-005: StreamUpdates Expansion — Sub-message Pattern

**Status**: Accepted

**Context and Problem Statement**: Podcast sync needs to report progress/completion to connected TUI clients via the existing StreamUpdates gRPC stream. How should new event types be added?

**Decision Drivers**:
- Existing codebase precedent: UpdatePlaylist uses sub-message with inner oneof (lib/proto/player.proto:175-184)
- Namespace cleanliness: outer oneof currently has 8 variants
- Independent evolution: podcast sync events may grow without affecting other event types
- Backward compatibility: old clients ignore unknown proto fields

**Considered Options**:
1. Sub-message with inner oneof (one new variant in outer oneof)
2. Multiple top-level variants (4 new variants in outer oneof)
3. Separate streaming RPC endpoint

**Decision Outcome**: Option 1 — Sub-message, matching UpdatePlaylist pattern exactly.

**Rationale**: Direct precedent in codebase (SRC-023, SRC-024). One variant added to outer oneof (field 9). Inner oneof groups all podcast sync events. Follows the proto naming convention (Update prefix). Allows podcast events to evolve independently.

</section>

<section title="Parallelism Annotations">

### Phase Dependencies (Implementation Order)

[SERIAL: Phase 0 -> Phase 1 -> Phase 2]
[PARALLEL: Phase 3 (test cleanup) can proceed alongside Phase 1 and Phase 2]

### Within Phase 0 (Migration)
[SERIAL: Identify TUI sync calls -> Add server-side gRPC handlers -> Update TUI to send commands -> Remove TUI direct calls]

### Within Phase 1 (Architecture and Config)
[PARALLEL: Config restructuring, DB migration (002.sql), Proto expansion]
- Config restructuring (move synchronization into PodcastSettings) — independent of DB
- DB migration (add check_interval column + update_last_checked function) — independent of config
- Proto expansion (UpdatePodcastSync sub-message) — independent of config and DB

[SERIAL: Config restructuring -> Update server references to new config path]
[SERIAL: DB migration -> get_due_podcasts query function]

### Within Phase 2 (Sync Logic)
[PARALLEL: Pre-scan refactor, PodcastUrl fix, Auto-enqueue config]
- Pre-scan refactor (spawn_blocking + HashMap) — independent
- PodcastUrl fix (replace Path with PodcastUrl) — independent
- Auto-enqueue config (read config field, gate enqueue) — independent

[SERIAL: Shared TaskPool refactor -> integrate with pre-scan and download logic]
[SERIAL: All Phase 2 fixes -> combine refresh_on_startup + periodic into single interval_at path (AC-19)]

### Within Phase 3 (Tests)
[PARALLEL: Remove redundant tests, Create TestHarness, Fix test URLs, Fix error assertions]
- All test cleanup tasks can proceed in parallel as they touch independent test functions

</section>

<section title="Security Considerations">
  <list type="unordered">
    <item>No new attack surface — all network operations use existing reqwest client with established timeouts</item>
    <item>UDS socket provides access control for gRPC communication (existing pattern)</item>
    <item>Database operations use parameterized queries (no SQL injection risk in update_last_checked)</item>
    <item>Test URLs restricted to localhost/127.0.0.1 to prevent network leaks (AC-22)</item>
    <item>Proto oneof expansion is backward-compatible — old clients ignore unknown fields</item>
  </list>
</section>

<section title="Performance Considerations">
  <list type="unordered">
    <item>Global-interval wake adds at most interval_duration latency before a due podcast is processed — acceptable for hourly podcast feeds</item>
    <item>SQL filter for 50 podcasts costs microseconds — negligible vs network I/O for feed fetching</item>
    <item>Pre-scan with spawn_blocking prevents async thread blocking for large directories (thousands of files)</item>
    <item>Single shared TaskPool with concurrent_downloads_max bounds total network concurrency across all operations</item>
    <item>prepare_cached for update_last_checked avoids repeated SQL parsing on the hot path</item>
    <item>Per-podcast error isolation ensures one failed feed does not block others</item>
  </list>
</section>

<section title="Numeric Constants">

The following numeric constants are fixed in this design and should be validated:

| Constant | Value | Location | Rationale |
|----------|-------|----------|-----------|
| Default sync interval | 3600s (1 hour) | SynchronizationSettings::default() | Standard RSS polling frequency; matches Miniflux default |
| Minimum sync interval clamp | 1s | start_podcast_sync_task() | Prevents tokio panic on Duration::ZERO |
| Default max_new_episodes | 5 | SynchronizationSettings::default() | Limits bandwidth on first sync of podcast with large backlog |
| Default concurrent_downloads_max | 3 | PodcastSettings::default() | Existing value; balances throughput vs resource usage |
| Default max_download_retries | 3 | PodcastSettings::default() | Existing value |
| DB_VERSION after migration | 2 | migration.rs | Incremented from current value of 1 |

</section>

<section title="Error Handling Strategy">

| Boundary | Error Type | Handling | Recovery |
|----------|-----------|----------|----------|
| DB open failure | Fatal | Propagate via anyhow::Result | Sync pass aborted, logged at error level |
| Feed fetch timeout/error | Per-podcast | warn! log, update last_checked, continue | Next sync pass retries |
| Download failure | Per-episode | warn! log, increment stats.episodes_failed | Episode remains eligible for next pass |
| Channel send failure | Per-operation | warn! log, continue | Non-blocking; TUI may miss update |
| Directory creation failure | Per-podcast | warn! log, skip downloads for that podcast | Next pass retries |
| Config read failure | Fatal (should not happen) | Panic (RwLock poisoned) | Server restart required |

</section>

<section title="Future Considerations">
  <list type="unordered">
    <item>tokio-util::time::DelayQueue — if termusic ever supports 1000+ podcasts or sub-minute precision, replace SQL filter with DelayQueue without changing external API</item>
    <item>Per-podcast check_interval exposed in TUI — currently DB-only column, could add TUI command to set per-podcast override</item>
    <item>"Download only, never enqueue" mode — AutoEnqueue enum is extensible to add a DownloadOnly variant</item>
    <item>Feed truncation detection — when a feed returns fewer episodes than previously known, mark missing episodes as unavailable</item>
    <item>Podcast ordering in enqueue — currently per-podcast groups; could add subscription-order or pubdate-interleave modes</item>
  </list>
</section>

<section title="AC-to-Module Traceability">

| AC | Module(s) | Phase |
|----|-----------|-------|
| AC-01, AC-02, AC-03 | podcast_sync (server), TUI migration | Phase 0 |
| AC-04 | config/v2/server (PodcastSettings.synchronization) | Phase 1 |
| AC-05 | config/v2/server (interval = 0 disables) | Phase 1 |
| AC-06 | config/v2/server (refresh_on_startup field) | Phase 1 |
| AC-07 | config/v2/server (source comments) | Phase 1 |
| AC-08 | podcast/db (update_last_checked, migration 002) | Phase 1 |
| AC-09 | podcast/db (check_interval column, get_due_podcasts) | Phase 1 |
| AC-10 | podcast_sync (single shared TaskPool) | Phase 2 |
| AC-11 | config/v2/server (AutoEnqueue enum), podcast_sync | Phase 2 |
| AC-12 | podcast_sync (oldest-first ordering per podcast) | Phase 2 |
| AC-13 | podcast_sync (played+deleted filter) | Phase 2 |
| AC-14 | podcast_sync (PlaylistTrackSource::PodcastUrl) | Phase 2 |
| AC-15 | podcast_sync (spawn_blocking pre-scan) | Phase 2 |
| AC-16 | podcast_sync (separate download tasks, non-blocking) | Phase 2 |
| AC-17 | podcast_sync (use lib::utils::create_podcast_dir) | Phase 2 |
| AC-18 | lib/player.rs (new_append delegates to new_single + AT_END) | Phase 2 |
| AC-19 | podcast_sync (interval_at with Instant::now for immediate tick) | Phase 2 |
| AC-20-AC-27 | Test modules | Phase 3 |
| AC-28-AC-31 | Style/conventions (all modules) | Phase 4 (cross-cutting) |

</section>

</document>
