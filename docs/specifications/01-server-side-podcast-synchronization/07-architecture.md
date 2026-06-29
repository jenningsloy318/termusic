<document type="architecture">

<metadata>
  <field name="title">Architecture: Server-Side Podcast Synchronization</field>
  <field name="date">2026-06-23</field>
  <field name="updated">2026-06-23</field>
  <field name="author">Claude</field>
  <field name="status">Implemented</field>
</metadata>

<section title="Overview">
  <paragraph>This architecture adds a server-internal periodic task that refreshes subscribed podcast RSS feeds, downloads new episodes, and appends them to the play queue. The design follows the existing `start_playlist_save_interval` pattern -- a tokio-spawned task with `interval_at` and `CancellationToken` -- requiring minimal new abstraction. A new `SynchronizationSettings` config section controls enable/disable, interval, and startup behavior.</paragraph>
</section>

<section title="Architectural Drivers">
  <list type="unordered">
    <item>Headless operation: podcasts must sync without any TUI client connected (AC-02 through AC-07)</item>
    <item>Error isolation: one failing podcast must never crash the server or block other podcasts (AC-08)</item>
    <item>Pattern consistency: mirror the proven `start_playlist_save_interval` lifecycle pattern (AC-11)</item>
    <item>Backward compatibility: existing configs without `[synchronization]` must parse without error (AC-01, AC-10)</item>
    <item>Minimal new dependencies: only `humantime-serde` for human-readable duration parsing (NFR-MinDeps)</item>
    <item>Non-blocking: sync must never block the player loop or gRPC service (NFR-Performance)</item>
  </list>
</section>

<section title="Module Architecture">
  <diagram type="ascii">
+----------------------------------------------------------------------+
|                          server crate                                  |
|                                                                        |
|  actual_main()                                                         |
|    |                                                                   |
|    +-- start_playlist_save_interval(...)  [EXISTING]                   |
|    |                                                                   |
|    +-- start_podcast_sync_task(...)       [NEW]                        |
|         |                                                              |
|         +-- sync_once(...)               [NEW - async fn]              |
|              |                                                         |
|              +-- Database::new()         [opens own SQLite conn]        |
|              +-- db.get_podcasts()       [EXISTING - read feeds list]   |
|              +-- check_feed(...)         [EXISTING - fetch RSS]         |
|              +-- db.update_podcast(...)  [EXISTING - dedup + insert]    |
|              +-- download_list(...)      [EXISTING - download eps]      |
|              +-- cmd_tx.send(PlaylistAddTrack) [EXISTING - enqueue]     |
+----------------------------------------------------------------------+

+----------------------------------------------------------------------+
|                          lib crate                                     |
|                                                                        |
|  config/v2/server/synchronization.rs    [NEW - SynchronizationSettings]|
|  config/v2/server/mod.rs                [MODIFIED - add field]         |
|  podcast/mod.rs                          [EXISTING - check_feed,       |
|                                           download_list]               |
|  podcast/db/mod.rs                       [EXISTING - Database]         |
|  player.rs                               [MODIFIED - add new_append*]  |
|  taskpool.rs                             [EXISTING - TaskPool]         |
|  utils.rs                                [EXISTING - get_app_config_path]|
+----------------------------------------------------------------------+

Data Flow:
  Config(TOML) --> SynchronizationSettings --> start_podcast_sync_task
                                                       |
  RSS Feeds (internet) <-- check_feed <-- sync_once <--+
                                |                      |
                                v                      v
  Podcast DB (SQLite) <-- update_podcast       download_list --> local files
                                                       |
                                                       v
  Player Loop <-- cmd_tx <-- PlaylistAddTrack <-- sync_once
  </diagram>
</section>

<section title="Module Specifications">

  <subsection title="Module 1: SynchronizationSettings">
    <field name="purpose">Define the configuration schema for podcast synchronization with serde-based parsing and sensible defaults.</field>
    <list type="unordered" label="Responsibilities">
      <item>Declare `enable`, `interval`, and `refresh_on_startup` fields with their types</item>
      <item>Provide default values (enable=true, interval=1h, refresh_on_startup=true) via `impl Default`</item>
      <item>Support human-readable duration strings via `humantime-serde` integration</item>
      <item>Ensure backward compatibility via `#[serde(default)]` when the section is absent</item>
    </list>
    <field name="dependencies">serde, humantime-serde, std::time::Duration</field>
    <field name="public-interface">
      <code lang="rust">
/// Settings for the periodic podcast synchronization task.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SynchronizationSettings {
    /// Whether automatic podcast synchronization is enabled.
    pub enable: bool,
    /// How often to check for new episodes.
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    /// Whether to run a full sync immediately on server startup.
    pub refresh_on_startup: bool,
}

impl Default for SynchronizationSettings {
    fn default() -> Self {
        Self {
            enable: true,
            interval: Duration::from_secs(3600), // 1h
            refresh_on_startup: true,
        }
    }
}
      </code>
    </field>
  </subsection>

  <subsection title="Module 2: PodcastSyncTask">
    <field name="purpose">Manage the lifecycle of the periodic sync task including spawn, interval timing, startup sync, and graceful cancellation.</field>
    <list type="unordered" label="Responsibilities">
      <item>Spawn a tokio task on the provided Handle with CancellationToken integration</item>
      <item>Execute an immediate sync pass if `refresh_on_startup` is true</item>
      <item>Use `tokio::time::interval_at` for drift-free periodic execution</item>
      <item>Delegate actual sync work to `sync_once` on each tick</item>
      <item>Exit cleanly when the CancellationToken is cancelled (server shutdown)</item>
    </list>
    <field name="dependencies">SynchronizationSettings, sync_once, tokio, tokio_util::CancellationToken, Handle, SharedServerSettings, PlayerCmdSender, PathBuf</field>
    <field name="public-interface">
      <code lang="rust">
/// Spawn the periodic podcast sync task.
/// Only call when `config.read().settings.synchronization.enable` is true.
///
/// Mirrors the `start_playlist_save_interval` pattern exactly.
fn start_podcast_sync_task(
    handle: Handle,
    cancel_token: CancellationToken,
    config: SharedServerSettings,
    cmd_tx: PlayerCmdSender,
    db_path: PathBuf,
)
      </code>
    </field>
  </subsection>

  <subsection title="Module 3: SyncOnce">
    <field name="purpose">Execute a single sync pass: fetch all subscribed feeds, identify new episodes, download them, and enqueue them.</field>
    <list type="unordered" label="Responsibilities">
      <item>Open a dedicated Database connection for this sync pass</item>
      <item>Retrieve all subscribed podcasts from the database</item>
      <item>For each podcast, fetch the RSS feed via `check_feed` with error isolation</item>
      <item>Use `Database::update_podcast` for deduplication (GUID then URL-based matching)</item>
      <item>Identify episodes that are new (inserted by update_podcast) and not yet downloaded</item>
      <item>Download new episodes via `download_list` with bounded concurrency</item>
      <item>Record downloaded file paths in the database via `db.insert_file`</item>
      <item>Enqueue each downloaded episode via `cmd_tx.send(PlayerCmd::PlaylistAddTrack(...))`</item>
      <item>Log per-podcast and per-episode errors at warn level without aborting the pass</item>
    </list>
    <field name="dependencies">Database (podcast DB), check_feed, download_list, TaskPool, PlayerCmdSender, PlaylistAddTrack, PodcastSettings, PathBuf</field>
    <field name="public-interface">
      <code lang="rust">
/// Execute a single synchronization pass over all subscribed podcasts.
///
/// Errors on individual podcasts/episodes are logged and do not
/// propagate -- only truly fatal errors (e.g., cannot open DB) are returned.
async fn sync_once(
    config: &SharedServerSettings,
    cmd_tx: &PlayerCmdSender,
    db_path: &Path,
) -> Result<SyncPassStats>

/// Statistics from a single sync pass, for logging.
struct SyncPassStats {
    podcasts_checked: usize,
    podcasts_failed: usize,
    episodes_downloaded: usize,
    episodes_enqueued: usize,
    episodes_failed: usize,
}
      </code>
    </field>
  </subsection>

  <subsection title="Module 4: PlaylistAddTrack API Extension">
    <field name="purpose">Provide a clear, intent-based constructor for appending tracks at the end of the playlist without exposing the u64::MAX sentinel.</field>
    <list type="unordered" label="Responsibilities">
      <item>Add `new_append_single` constructor that uses u64::MAX internally</item>
      <item>Add `new_append_vec` constructor for batch appends</item>
      <item>Document the sentinel value semantics in one place</item>
    </list>
    <field name="dependencies">None (extends existing PlaylistAddTrack in lib/src/player.rs)</field>
    <field name="public-interface">
      <code lang="rust">
impl PlaylistAddTrack {
    /// Sentinel value: any at_index >= playlist.len() triggers end-append.
    pub const AT_END: u64 = u64::MAX;

    /// Create a request to append a single track at the end of the playlist.
    #[must_use]
    pub fn new_append_single(track: PlaylistTrackSource) -> Self {
        Self { at_index: Self::AT_END, tracks: vec![track] }
    }

    /// Create a request to append multiple tracks at the end of the playlist.
    #[must_use]
    pub fn new_append_vec(tracks: Vec<PlaylistTrackSource>) -> Self {
        Self { at_index: Self::AT_END, tracks }
    }
}
      </code>
    </field>
  </subsection>

</section>

<section title="Data Flow">
  <diagram type="ascii">
Periodic Tick / Startup Trigger
        |
        v
+-------------------+
| sync_once()       |
+-------------------+
        |
        | 1. Open Database connection
        v
+-------------------+       +--------------------+
| db.get_podcasts() | ----> | Vec&lt;Podcast&gt;       |
+-------------------+       +--------------------+
        |
        | 2. For each podcast (error-isolated):
        v
+-------------------+       +--------------------+
| check_feed(...)   | ----> | PodcastSyncResult  |
| (via TaskPool)    |       | ::SyncData(pod)    |
+-------------------+       +--------------------+
        |
        | 3. Update DB, get SyncResult with insert count
        v
+-------------------+       +--------------------+
| db.update_podcast | ----> | SyncResult{added}  |
+-------------------+       +--------------------+
        |
        | 4. Identify undownloaded new episodes (path == None)
        v
+-------------------+       +--------------------+
| download_list()   | ----> | PodcastDLResult    |
| (via TaskPool)    |       | ::DLComplete(ep)   |
+-------------------+       +--------------------+
        |
        | 5. Record file path, then enqueue
        v
+-------------------+       +--------------------+
| db.insert_file()  |       | PlayerCmd::        |
| cmd_tx.send(      | ----> | PlaylistAddTrack   |
|   PlaylistAddTrack|       | (append at end)    |
|   ::new_append_   |       +--------------------+
|    single(src))   |
+-------------------+
        |
        v
+-------------------+
| Player Loop       |
| (processes cmd)   |
+-------------------+
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
      <cell>Async Runtime</cell>
      <cell>tokio 1.52 (existing)</cell>
      <cell>Already the project runtime; provides interval_at, select!, spawn, mpsc</cell>
    </row>
    <row>
      <cell>Cancellation</cell>
      <cell>tokio_util::CancellationToken (existing)</cell>
      <cell>Proven pattern used by start_playlist_save_interval for graceful shutdown</cell>
    </row>
    <row>
      <cell>Task Bounding</cell>
      <cell>TaskPool (lib/src/taskpool.rs, existing)</cell>
      <cell>Semaphore-based concurrency limiter already used by download_list and check_feed</cell>
    </row>
    <row>
      <cell>RSS Parsing</cell>
      <cell>rss 2.0.13 (existing)</cell>
      <cell>Already used by get_feed_data for RSS feed parsing</cell>
    </row>
    <row>
      <cell>HTTP Client</cell>
      <cell>reqwest 0.13.4 (existing)</cell>
      <cell>Already used by download_file and get_feed_data</cell>
    </row>
    <row>
      <cell>Database</cell>
      <cell>rusqlite 0.39 (existing)</cell>
      <cell>Podcast database; sync task opens its own connection (thread-safe)</cell>
    </row>
    <row>
      <cell>Config Duration Parsing</cell>
      <cell>humantime-serde 0.2 (NEW)</cell>
      <cell>Enables "1h", "30m" in TOML config; tiny crate, well-maintained, no alternatives needed</cell>
    </row>
    <row>
      <cell>Serialization</cell>
      <cell>serde + toml (existing)</cell>
      <cell>Config parsing; #[serde(default)] pattern for backward compatibility</cell>
    </row>
  </table>
</section>

<section title="ADRs">

### ADR-001: Sync Task Architecture Pattern

- **Status**: Accepted
- **Context and Problem Statement**: The server needs a periodic background task to sync podcasts. Multiple patterns exist: tokio spawn with interval, std::thread with sleep, external cron/systemd timer.
- **Decision Drivers**:
  - Must integrate with existing CancellationToken shutdown mechanism
  - Must use async I/O (reqwest, database) without entering a new runtime
  - Must follow established codebase patterns for maintainability
  - Must not block the player loop or gRPC service
- **Considered Options**:
  1. Tokio periodic task mirroring `start_playlist_save_interval` (tokio::spawn + interval_at + select!)
  2. Separate background thread with std::thread::sleep loop
  3. External cron/systemd timer triggering a CLI subcommand
- **Decision Outcome**: Option 1 -- Tokio periodic task
- **Rationale**: This is the exact pattern already proven in the codebase at `server/src/server.rs:216-241`. It integrates naturally with CancellationToken, uses the existing tokio runtime for async network I/O, does not drift (interval_at vs repeated sleep), and requires zero new abstraction. Options 2 and 3 introduce unnecessary complexity or external dependencies.
- **Pros and Cons**:
  - Option 1: (+) Proven pattern, (+) clean cancellation, (+) no drift, (+) async-native. (-) None significant.
  - Option 2: (+) Simple. (-) Cannot cancel via CancellationToken, (-) cannot use async code without entering runtime, (-) breaks async pattern.
  - Option 3: (+) Zero in-process complexity. (-) Requires IPC or direct DB manipulation, (-) user must configure OS scheduling, (-) does not fulfill server-internal requirement.

### ADR-002: Database Connection Strategy

- **Status**: Accepted
- **Context and Problem Statement**: The sync task needs access to the podcast SQLite database. The `GeneralPlayer` in the player loop thread owns its own `Database` instance. Sharing it would require `Arc<Mutex<Database>>` across thread boundaries, which rusqlite's `Connection` does not support (not `Send`).
- **Decision Drivers**:
  - rusqlite::Connection is not Send -- cannot share across threads
  - SQLite supports concurrent readers from separate connections
  - The existing pattern resolves paths at call sites and passes them (PAT-LIB-003)
  - Testability requires passing paths as parameters
- **Considered Options**:
  1. Open a dedicated Database connection in the sync task, path passed from `actual_main()`
  2. Call `get_app_config_path()` inside the sync task
  3. Wrap the existing Database in Arc<Mutex> and share
- **Decision Outcome**: Option 1 -- Dedicated connection, path passed as parameter
- **Rationale**: Follows the established PAT-LIB-003 pattern from `execute_action` at `server/src/server.rs:674-676`. Makes the dependency explicit, enables testing with arbitrary paths, and handles errors at the call site where context is richer. SQLite handles concurrent readers safely. Option 3 is impossible due to `Connection` not being `Send`.
- **Pros and Cons**:
  - Option 1: (+) Explicit, (+) testable, (+) follows existing pattern, (+) error handling at call site. (-) One additional parameter.
  - Option 2: (+) Fewer parameters. (-) Hidden dependency, (-) less testable, (-) error inside spawned task cannot propagate to startup.
  - Option 3: Not viable -- rusqlite Connection is not Send.

### ADR-003: Download Completion Signaling

- **Status**: Accepted
- **Context and Problem Statement**: After `download_list` spawns tasks for each episode, the sync task must know when all downloads complete before proceeding to the next podcast or finishing the pass.
- **Decision Drivers**:
  - download_list moves the callback closure into spawned tasks
  - Tokio mpsc channel closes when all senders are dropped
  - Pattern must be panic-safe and cancellation-safe
  - Simplicity over premature optimization
- **Considered Options**:
  1. Counter-based drain: count terminal results against episodes.len()
  2. Channel-drain: `while let Some(msg) = rx.recv().await` until channel closes
  3. JoinHandle tracking via TaskTracker (requires TaskPool refactoring)
- **Decision Outcome**: Option 2 -- Channel-drain (while-let-Some)
- **Rationale**: This is the canonical Tokio pattern recommended in official tutorials. When `tx` is moved into the closure and `download_list` clones it per task, all sender clones are dropped when tasks complete, causing `recv()` to return `None`. This handles panics, cancellation, and future message variants automatically without counting logic. The research report (ISS-007, SRC-007, SRC-008, SRC-009) confirms correctness with high confidence.
- **Pros and Cons**:
  - Option 1: (+) Explicit termination count. (-) Must identify terminal variants, (-) hangs if task panics without sending.
  - Option 2: (+) Simplest, (+) panic-safe, (+) cancellation-safe, (+) idiomatic Tokio. (-) Processes DLStart messages too (trivial cost).
  - Option 3: (+) Explicit lifecycle. (-) Requires TaskPool refactoring, (-) overkill.

### ADR-004: Append Sentinel Value Strategy

- **Status**: Accepted
- **Context and Problem Statement**: The sync task needs to append tracks at the end of the playlist via `PlaylistAddTrack`. The `at_index` field is `u64`; using `u64::MAX` as a sentinel triggers end-append behavior, but this is not self-documenting.
- **Decision Drivers**:
  - Sync task cannot know current playlist length (runs asynchronously from player loop)
  - u64::MAX works correctly on 64-bit targets (the only supported targets)
  - API clarity for maintainers
  - Minimal change to existing public API
- **Considered Options**:
  1. Named constant `PlaylistAddTrack::AT_END` only
  2. Dedicated constructor `new_append_single` / `new_append_vec` (hides sentinel entirely)
  3. Status quo: raw u64::MAX with a comment at each call site
- **Decision Outcome**: Option 2 -- Dedicated constructors (with AT_END constant for internal use)
- **Rationale**: Research report ISS-005 scored Option B (constructors) at 39/40 vs Option A (constant) at 35/40. Constructors eliminate the need for callers to know about sentinel values entirely, providing a self-documenting API. The constant is still defined internally for documentation, but callers use `new_append_single(source)` which clearly communicates intent.
- **Pros and Cons**:
  - Option 1: (+) Minimal change. (-) Callers must still know to use the constant.
  - Option 2: (+) Self-documenting, (+) eliminates magic value at call sites, (+) coexists with existing API. (-) Two new methods.
  - Option 3: (+) Zero change. (-) Opaque to new contributors, (-) repeated comments needed.

</section>

<section title="Parallelism Annotations">

### Implementation Phases

[PARALLEL: Module 1 (SynchronizationSettings), Module 4 (PlaylistAddTrack API Extension)]
- These two modules have zero dependencies on each other and can be implemented simultaneously.

[SERIAL: Module 1 + Module 4 --> Module 3 (SyncOnce) --> Module 2 (PodcastSyncTask)]
- Module 3 (sync_once) depends on the config struct (Module 1) for reading settings and on the new_append constructors (Module 4) for enqueuing.
- Module 2 (PodcastSyncTask) depends on Module 3 to have `sync_once` available to call.

### Detailed Task DAG

```
Phase 1 [PARALLEL]:
  A: lib/src/config/v2/server/synchronization.rs (new file)
     + Modify lib/src/config/v2/server/mod.rs (add field)
     + Add humantime-serde to workspace Cargo.toml
     + Config roundtrip tests

  B: lib/src/player.rs (add AT_END const + new_append_single + new_append_vec)

Phase 2 [SERIAL: depends on A and B]:
  C: server/src/podcast_sync.rs (new file: sync_once function)
     + Unit tests with in-memory DB or mock feeds

Phase 3 [SERIAL: depends on C]:
  D: server/src/podcast_sync.rs (add start_podcast_sync_task function)
     + Modify server/src/server.rs actual_main() to wire it up
     + Integration test for task lifecycle
```

### Agent Parallelism Potential

- **Agent 1** can implement Phase 1A (config) without any context about the sync logic
- **Agent 2** can implement Phase 1B (PlaylistAddTrack extension) with only knowledge of lib/src/player.rs
- Both are leaf modules with stable, fully-defined interfaces above
- Phase 2 and 3 require sequential execution by a single agent with full context

</section>

<section title="Security Considerations">
  <list type="unordered">
    <item>No new attack surface: feed URLs come exclusively from the user's own database (user-subscribed podcasts)</item>
    <item>Downloads are restricted to the configured `podcast.download_dir` path only</item>
    <item>No new network listeners or ports are opened</item>
    <item>The sync task reuses existing reqwest HTTP client with connect_timeout (10s) preventing indefinite hangs</item>
    <item>SQLite connections use the same path validation as existing code (get_app_config_path creates dirs safely)</item>
  </list>
</section>

<section title="Performance Considerations">
  <list type="unordered">
    <item>Non-blocking: all network I/O is async via reqwest on the tokio runtime; the player loop thread is never blocked</item>
    <item>Bounded concurrency: download tasks are limited by `podcast.concurrent_downloads_max` (default 3) via TaskPool semaphore</item>
    <item>No timer drift: `tokio::time::interval_at` compensates for execution time, unlike repeated `sleep`</item>
    <item>Per-podcast isolation: a slow or hanging feed fetch does not delay other podcasts (each runs in its own TaskPool slot)</item>
    <item>Database connection opened per sync pass (not held open between passes): minimizes SQLite lock contention with the player loop</item>
    <item>Default interval of 3600 seconds (1 hour) balances freshness against resource usage</item>
    <item>Initial sync cap consideration: while not mandated by AC, the architecture supports adding a `max_episodes_per_sync` field in future to limit back-catalog downloads</item>
  </list>
</section>

<section title="Error Handling Strategy">

### Boundary Error Handling

| Boundary | Error Type | Handling | Source Pattern |
|----------|-----------|----------|---------------|
| Database open failure | Fatal for this pass | Log error, return early from sync_once | anyhow Result propagation |
| get_podcasts failure | Fatal for this pass | Log error, return early from sync_once | anyhow Result propagation |
| Individual feed fetch failure | Per-podcast | warn! + continue to next podcast | PAT-LIB-004 (import_from_opml) |
| Individual RSS parse failure | Per-podcast | warn! + continue to next podcast | PAT-LIB-004 |
| update_podcast failure | Per-podcast | warn! + continue to next podcast | PAT-LIB-004 |
| Individual episode download failure | Per-episode | warn! + continue to next episode | PodcastDLResult::DL*Error variants |
| insert_file failure | Per-episode | warn! + continue (episode downloaded but not tracked) | anyhow with context |
| PlaylistAddTrack send failure | Per-episode | warn! (channel closed means server shutting down) | `let _ = tx.send(...)` pattern |

### Cancellation Integration

The sync task uses `tokio::select!` with `cancel_token.cancelled()` at the interval loop level. During an active sync_once pass, cancellation is checked between podcasts (not mid-download). The TaskPool's Drop implementation cancels in-flight downloads when the sync task exits, preventing resource leaks.

</section>

<section title="Test Strategy">

### Unit Tests
- **Config roundtrip**: Serialize/deserialize SynchronizationSettings with default and non-default values (SCENARIO-001, 002, 003)
- **Invalid duration**: Verify deserialization failure on malformed interval string (SCENARIO-004)
- **PlaylistAddTrack constructors**: Verify new_append_single/new_append_vec produce correct at_index value

### Integration Tests
- **sync_once with mock feeds**: Use an in-memory database, verify episodes are downloaded and enqueued (SCENARIO-010, 014, 015)
- **Deduplication**: Insert episodes, re-run sync, verify no duplicates (SCENARIO-011, 012, 013)
- **Error isolation**: Inject network errors for one feed, verify others succeed (SCENARIO-017, 018, 019)

### Lifecycle Tests
- **Task not spawned when disabled**: Start with enable=false, verify no sync activity (SCENARIO-005)
- **Graceful cancellation**: Start task, cancel token, verify clean exit (SCENARIO-009)

</section>

<section title="Numeric Constants">

| Constant | Value | Context | Rationale |
|----------|-------|---------|-----------|
| Default sync interval | 3600 seconds (1h) | SynchronizationSettings::default().interval | Balances freshness vs. network/CPU load for typical podcast release cadence (daily/weekly) |
| PlaylistAddTrack::AT_END | u64::MAX (18446744073709551615) | Sentinel for end-append | Any value >= playlist.len() triggers append; u64::MAX is always >= any usize on 64-bit |
| connect_timeout for downloads | 10 seconds | reqwest ClientBuilder in download_file | Existing value; prevents indefinite connection hangs |
| Default concurrent_downloads_max | 3 | PodcastSettings::default() | Existing value; bounds TaskPool semaphore for feed fetches and downloads |
| Default max_download_retries | 3 | PodcastSettings::default() | Existing value; retries per episode before giving up |

</section>

<section title="Future Considerations">
  <list type="unordered">
    <item>max_episodes_per_sync config field: cap initial sync to N most recent episodes per podcast to avoid back-catalog floods (noted in requirements Open Questions)</item>
    <item>Per-podcast sync intervals: allow different refresh rates for different podcasts (e.g., daily news vs weekly shows)</item>
    <item>Sync status reporting via gRPC stream: notify connected TUI clients when new episodes are synced</item>
    <item>Retry queue for failed downloads: re-attempt failed episodes on the next sync pass with exponential backoff</item>
    <item>OPML import triggering immediate sync: after importing new subscriptions, run a targeted sync for just the new feeds</item>
  </list>
</section>

<section title="File Change Summary">

### New Files
| File | Module | Purpose |
|------|--------|---------|
| `lib/src/config/v2/server/synchronization.rs` | SynchronizationSettings | Config struct with serde support |
| `server/src/podcast_sync.rs` | PodcastSyncTask + SyncOnce | Task lifecycle and sync logic |

### Modified Files
| File | Change | Purpose |
|------|--------|---------|
| `Cargo.toml` (workspace root) | Add `humantime-serde = "0.2"` to `[workspace.dependencies]` | Duration parsing dependency |
| `lib/Cargo.toml` | Add `humantime-serde` to `[dependencies]` | Expose to lib crate |
| `lib/src/config/v2/server/mod.rs` | Add `pub mod synchronization;` and `synchronization: SynchronizationSettings` field to `ServerSettings` | Wire config |
| `lib/src/player.rs` | Add `AT_END` const and `new_append_single`/`new_append_vec` methods to `PlaylistAddTrack` | Clean append API |
| `server/src/server.rs` | Add `mod podcast_sync;` and call `start_podcast_sync_task(...)` in `actual_main()` | Wire task |

</section>

</document>
