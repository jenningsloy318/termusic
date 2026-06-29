# Deep Research Report: Server-Side Podcast Synchronization (Iteration 2)

- **Date**: 2026-06-23
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-23
- **Technologies**: Rust, Tokio, rusqlite, humantime-serde, SQLite WAL, mpsc channels, TaskPool
- **Freshness**: Fresh (< 6mo)

---

## Executive Summary

- ISS-001 (race condition) is **resolved**: The unbounded `cmd_tx` mpsc channel serializes all `PlayerCmd` messages into a single-consumer `player_loop` thread, guaranteeing sequential processing. The `was_empty` check in `PlaylistAddTrack` handler is atomic relative to the command stream (SRC-001, SRC-002, SRC-016).
- ISS-002 (update_podcast vs custom insert) is **resolved**: `Database::update_podcast` should be reused because it already implements GUID-primary matching with 2-of-3 fallback deduplication, returning a `SyncResult` with the count of newly inserted episodes (SRC-003, SRC-004).
- ISS-003 (download_file private; use download_list) is **resolved**: The sync task should use the public `download_list` function with a new `TaskPool` instance. After each `PodcastDLResult::DLComplete`, the sync task calls `db.insert_file(ep_data.id, &file_path)` and then sends `PlayerCmd::PlaylistAddTrack` with `PlaylistTrackSource::PodcastUrl(ep.url)` (SRC-004, SRC-005, SRC-006).
- ISS-004 (episode cap) is **resolved**: Add `max_episodes_per_sync: Option<u32>` config field (default `Some(50)`). Sort new episodes by pubdate descending, take only the first N. Value of `None` or `0` means unlimited. This mirrors the `--limit` pattern used by podpull and `whats_new_episode_limit` in podcast-tui (SRC-007, SRC-008).

**Recommendation**: Proceed with implementation using the design outlined below. All four prior issues have clear resolution paths. Confidence: **High**.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| Rust tokio mpsc unbounded channel command serialization race condition safety 2025 2026 | Exa | 5 | 4 |
| Rust podcast app sync download episodes concurrency limit cap first sync strategy 2025 | Exa | 5 | 4 |
| Rust rusqlite SQLite WAL mode multiple connections concurrent read write busy_timeout 2025 2026 | Exa | 5 | 5 |
| humantime-serde Rust crate serde duration deserialize current status compatibility 2025 2026 | Exa | 5 | 3 |
| Rust download_list podcast TaskPool semaphore bounded concurrent downloads pattern 2025 | Exa | 5 | 3 |
| How does PlayerCmd::PlaylistAddTrack work with the cmd_tx channel and auto-play | DeepWiki | 1 | 1 |
| How does download_list work with EpData, TaskPool, and Database::insert_file | DeepWiki | 1 | 1 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-001 | Tokio mpsc module docs - https://docs.rs/tokio/latest/tokio/sync/mpsc/ | Official docs | 2026 | Fresh | High |
| SRC-002 | Tokio Channels Tutorial - https://tokio.rs/tokio/tutorial/channels | Official docs | 2026-05 | Fresh | High |
| SRC-003 | termusic Database::update_podcast + update_episodes - lib/src/podcast/db/mod.rs | Codebase | 2026-06 | Fresh | High |
| SRC-004 | termusic podcast module (download_list, EpData) - lib/src/podcast/mod.rs | Codebase | 2026-06 | Fresh | High |
| SRC-005 | termusic TaskPool implementation - lib/src/taskpool.rs | Codebase | 2026-06 | Fresh | High |
| SRC-006 | termusic TUI episode_download_complete - tui/src/ui/components/podcast.rs | Codebase | 2026-06 | Fresh | High |
| SRC-007 | podpull (Rust podcast sync CLI with --limit) - https://github.com/jakobwesthoff/podpull | GitHub | 2026-01 | Fresh | Medium |
| SRC-008 | podcast-tui (whats_new_episode_limit: 50) - https://github.com/lqdev/podcast-tui | GitHub | 2025-10 | Fresh | Medium |
| SRC-009 | SQLite concurrent writes blog - https://emschwartz.me/psa-your-sqlite-connection-pool-might-be-ruining-your-write-performance/ | Blog | 2026-02 | Fresh | High |
| SRC-010 | rusqlite Connection docs (busy_timeout) - https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html | Official docs | 2026 | Fresh | High |
| SRC-011 | sqlite-kit (read/write split pattern) - https://github.com/hotnsoursoup/sqlite-kit | GitHub | 2026-05 | Fresh | Medium |
| SRC-012 | SQLite concurrent writes best practices - https://tenthousandmeters.com/blog/sqlite-concurrent-writes-and-database-is-locked-errors/ | Blog | 2025-02 | Fresh | High |
| SRC-013 | humantime-serde crate - https://crates.io/crates/humantime-serde | Official docs | 2022-03 | Dated | High |
| SRC-014 | termusic server.rs (player_loop, start_playlist_save_interval) - server/src/server.rs | Codebase | 2026-06 | Fresh | High |
| SRC-015 | DeepWiki termusic analysis (PlayerCmd flow) - https://deepwiki.com | AI docs | 2026-06 | Fresh | Medium |
| SRC-016 | termusic PlayerCmd::PlaylistAddTrack handler - server/src/server.rs:502-514 | Codebase | 2026-06 | Fresh | High |
| SRC-017 | sqlite_rwc (reader-writer concurrency) - https://docs.rs/sqlite-rwc/latest/sqlite_rwc/ | Official docs | 2025 | Fresh | Medium |
| SRC-018 | AntennaPod concurrent download issue #7876 - https://github.com/AntennaPod/AntennaPod/issues/7876 | GitHub | 2025-07 | Fresh | Medium |

---

## Per-Issue Analysis

### ISS-001: Race Condition Between refresh_on_startup and Player Startup Auto-Play

**Prior Understanding**: Concern that if `startup_state == Playing` and `refresh_on_startup == true`, simultaneous access to the playlist could cause incorrect "queue was empty" detection or duplicate play triggers.

**Investigation Summary**: Examined the tokio mpsc channel semantics (SRC-001, SRC-002), the player loop implementation (SRC-014, SRC-016), and DeepWiki analysis of the architecture (SRC-015).

**Resolution Status**: **Resolved**

**Evidence**:

1. The `cmd_tx` channel is a `tokio::sync::mpsc::unbounded_channel` (SRC-001, SRC-014 line 134). An unbounded channel's `send()` always completes immediately and never blocks. Messages are received by a single consumer.

2. The `player_loop` runs on a dedicated thread and calls `cmd_rx.blocking_recv()` in a sequential loop (SRC-014 line 314). Each `PlayerCmd` is processed to completion before the next is dequeued. There is no concurrent processing of commands.

3. The startup sequence is:
   - `start_playlist_save_interval` is spawned (line 172)
   - The player thread is spawned (line 175-191)
   - Inside the player thread, if `startup_state == Playing`, `player.resume_from_stopped()` is called (line 311) -- this happens *before* the command loop starts.
   - The ticker thread is spawned (line 193)
   - The sync task (to be added) would be spawned adjacent to `start_playlist_save_interval`

4. The `PlaylistAddTrack` handler (SRC-016 line 502-514):
   ```rust
   PlayerCmd::PlaylistAddTrack(info) => {
       let mut playlist_write = player.playlist.write();
       let was_empty = playlist_write.is_empty();
       if let Err(err) = playlist_write.add_tracks(info, &player.db_podcast) {
           error!("Error adding tracks: {err}");
       }
       drop(playlist_write);
       if was_empty {
           player.resume_from_stopped();
       }
   }
   ```
   The `was_empty` check and `add_tracks` call happen atomically under a write lock on the playlist. No other command can interleave because the player loop is sequential.

5. The sync task sends `PlaylistAddTrack` via `cmd_tx.send()`. Since the player loop is already consuming commands sequentially, these messages will be processed one at a time in FIFO order. If the player already started (from `startup_state == Playing`), the playlist is not empty when the sync task's `PlaylistAddTrack` arrives, so `was_empty` will be false and no duplicate play will be triggered.

**Resolution Path**: No special handling needed. The existing architecture guarantees safety through sequential command processing. The sync task simply sends `PlayerCmd::PlaylistAddTrack` via the cloned `cmd_tx` sender, and the player loop handles it atomically.

---

### ISS-002: Use Database::update_podcast vs Custom Insert-Only Approach

**Prior Understanding**: Question of whether to use the existing `Database::update_podcast` (which handles deduplication internally) or implement a simpler insert-only flow for the sync task.

**Investigation Summary**: Analyzed `Database::update_podcast` and `update_episodes` implementation in detail (SRC-003), plus the episode deduplication logic.

**Resolution Status**: **Resolved**

**Evidence**:

1. `Database::update_podcast(pod_id, podcast)` (SRC-003 line 129-134) does two things:
   - Updates podcast metadata (title, URL, description, author, explicit, last_checked)
   - Calls `update_episodes(pod_id, &podcast.episodes)` which returns `SyncResult { added, updated }`

2. `update_episodes` (SRC-003 line 144-218) implements a sophisticated deduplication algorithm:
   - **Primary match**: GUID-based lookup in a HashMap of existing episodes
   - **Fallback match**: 2-of-3 matching on (title, url, pubdate) for episodes without GUIDs
   - **Update path**: If an existing episode has changed metadata, it updates non-sensitive fields (title, url, guid, description, pubdate, duration, image_url) while preserving `played`, `hidden`, and `last_position`
   - **Insert path**: Truly new episodes are inserted via `insert_episode`

3. The `SyncResult.added` field tells the sync task exactly how many new episodes were inserted. However, it does not directly return *which* episodes are new.

4. To identify which episodes need downloading, the sync task should:
   - Call `update_podcast(pod_id, &fetched_podcast_data)` -- this inserts new episodes into the DB
   - Then call `get_episodes(pod_id, true)` and filter for episodes where `ep.path.is_none()` (no entry in the `files` table)
   - This gives the list of episodes that need downloading (both newly synced and any previously failed downloads)

**Resolution Path**:
```rust
// 1. Refresh feed data
let podcast_data = get_feed_data(&podcast.url, max_retries).await?;

// 2. Sync to database (deduplication handled internally)
let sync_result = db.update_podcast(podcast.id, &podcast_data)?;
info!("Synced '{}': {} added, {} updated", podcast.title, sync_result.added, sync_result.updated);

// 3. Identify episodes needing download (no file path in DB)
let episodes = db.get_episodes(podcast.id, true)?;
let need_download: Vec<EpData> = episodes.iter()
    .filter(|ep| ep.path.is_none())
    .take(max_episodes_per_sync) // ISS-004 cap
    .map(|ep| EpData {
        id: ep.id,
        pod_id: ep.pod_id,
        title: ep.title.clone(),
        url: ep.url.clone(),
        pubdate: ep.pubdate,
        file_path: None,
    })
    .collect();
```

---

### ISS-003: download_file is Private; Use download_list with TaskPool

**Prior Understanding**: The `download_file` function is private to the podcast module. The sync task needs a way to download episodes using the existing infrastructure.

**Investigation Summary**: Analyzed `download_list` (SRC-004), `TaskPool` (SRC-005), and the TUI's download completion handler (SRC-006). Also reviewed DeepWiki analysis of the download pipeline (SRC-015).

**Resolution Status**: **Resolved**

**Evidence**:

1. `download_list` (SRC-004 line 467-484) is **public** (`pub fn download_list(...)`) and takes:
   - `episodes: Vec<EpData>` -- the episodes to download
   - `dest: &Path` -- the download directory
   - `max_retries: usize` -- retry count
   - `tp: &TaskPool` -- the concurrency limiter
   - `tx_to_main: impl Fn(PodcastDLResult) + Send + 'static + Clone` -- callback for results

2. The TUI's download completion handler (SRC-006 line 798-814) shows the post-download flow:
   ```rust
   pub fn episode_download_complete(&mut self, ep_data: EpData) -> Result<()> {
       let file_path = ep_data.file_path.unwrap();
       let res = self.podcast.db_podcast.insert_file(ep_data.id, &file_path);
       // ...
   }
   ```

3. `TaskPool` (SRC-005) creates its own `CancellationToken` internally. When the `TaskPool` is dropped, it cancels all spawned tasks and closes the semaphore. This means the sync task can create a `TaskPool` per sync pass, and if the sync task is cancelled (via the service cancel token), dropping the `TaskPool` will cancel all in-flight downloads.

4. The sync task needs to:
   - Create a `TaskPool` with `concurrent_downloads_max` capacity
   - Create an unbounded channel `(tx, rx)` for receiving `PodcastDLResult` messages
   - Call `download_list(episodes, &download_dir, max_retries, &tp, tx_callback)`
   - Await results on `rx`, handling each `PodcastDLResult::DLComplete` by calling `db.insert_file()` and sending `PlaylistAddTrack`
   - After all downloads complete, drop the `TaskPool`

5. The critical insight: the sync task must wait for all downloads to complete before moving to the next podcast (or at least track pending downloads). Using a counter of expected results (equal to `episodes.len()`) and decrementing on each result message works cleanly:

```rust
let taskpool = TaskPool::new(usize::from(config.podcast.concurrent_downloads_max.get()));
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

let episodes_count = episodes.len();
download_list(
    episodes,
    &config.podcast.download_dir,
    usize::from(config.podcast.max_download_retries),
    &taskpool,
    move |msg| { let _ = tx.send(msg); },
);

let mut completed = 0;
while let Some(result) = rx.recv().await {
    match result {
        PodcastDLResult::DLStart(_) => {},
        PodcastDLResult::DLComplete(ep_data) => {
            if let Some(ref file_path) = ep_data.file_path {
                if let Err(e) = db.insert_file(ep_data.id, file_path) {
                    warn!("Failed to register file in DB: {e:#}");
                }
                // Enqueue track
                let add_cmd = PlaylistAddTrack::new_single(
                    u64::MAX, // append to end
                    PlaylistTrackSource::PodcastUrl(ep_data.url.clone()),
                );
                let _ = cmd_tx.send(PlayerCmd::PlaylistAddTrack(add_cmd));
            }
            completed += 1;
        }
        PodcastDLResult::DLResponseError(ep) |
        PodcastDLResult::DLFileCreateError(ep) |
        PodcastDLResult::DLFileWriteError(ep) => {
            warn!("Download failed for '{}': {:?}", ep.title, result);
            completed += 1;
        }
    }
    if completed >= episodes_count {
        break;
    }
}
drop(taskpool); // Cancel any stragglers
```

**Resolution Path**: Use `download_list` as the public API. Create a per-podcast (or per-sync-pass) `TaskPool` and process results via an unbounded channel. Register files in the DB and enqueue tracks as each download completes.

---

### ISS-004: Episode Cap on First Sync

**Prior Understanding**: A podcast with 500+ episodes would cause unbounded downloads on first subscription. Need a configurable cap.

**Investigation Summary**: Reviewed how other Rust podcast applications handle this (SRC-007, SRC-008, SRC-018).

**Resolution Status**: **Resolved**

**Evidence**:

1. **podpull** (SRC-007) provides a `--limit` flag that "applies to episodes that haven't been downloaded yet. Already-downloaded episodes are excluded before the limit is applied." Episodes are sorted by publication date (most recent first).

2. **podcast-tui** (SRC-008) uses `whats_new_episode_limit: 50` in its config to limit the number of new episodes shown/processed.

3. **AntennaPod** issue #7876 (SRC-018) documents real-world problems when too many downloads happen simultaneously -- saturated connections, first episode availability delayed 20+ minutes, and apparent unkillable background downloads.

4. The termusic `update_episodes` function processes episodes in reverse order (`episodes.iter().rev()`) which corresponds to chronological order (oldest first in RSS, so reversed = newest first inserted). After `update_podcast` is called, `get_episodes(pod_id, true)` returns episodes ordered by `pubdate DESC` (SRC-003 line 318-331). So simply taking the first N from `get_episodes` gives the N most recent episodes.

5. Design for the config field:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SynchronizationSettings {
    pub enable: bool,
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    pub refresh_on_startup: bool,
    /// Maximum episodes to download per podcast per sync pass.
    /// None or 0 means unlimited.
    pub max_episodes_per_sync: Option<u32>,
}

impl Default for SynchronizationSettings {
    fn default() -> Self {
        Self {
            enable: true,
            interval: Duration::from_secs(3600), // 1h
            refresh_on_startup: true,
            max_episodes_per_sync: Some(50),
        }
    }
}
```

**Resolution Path**: Add `max_episodes_per_sync: Option<u32>` (default `Some(50)`) to the synchronization config. In the sync logic, after filtering for episodes needing download, apply `.take(max_episodes_per_sync.unwrap_or(usize::MAX))` to the sorted (by pubdate DESC) list. This naturally caps first-sync downloads while allowing all new episodes on subsequent syncs (where typically only 1-5 are new).

---

## Options Comparison: Sync Task Download Orchestration Design

| Criterion | Option A: Per-Podcast TaskPool + Channel | Option B: Shared TaskPool Across All Podcasts | Option C: Sequential Download (No TaskPool) |
|-----------|------------------------------------------|----------------------------------------------|---------------------------------------------|
| Maturity | 5 | 4 | 5 |
| Community/Support | 5 | 4 | 5 |
| Performance | 4 | 5 | 2 |
| Bundle Size / Footprint | 5 | 5 | 5 |
| Learning Curve | 4 | 3 | 5 |
| Maintenance Burden | 4 | 3 | 5 |
| Project Fit | 5 | 4 | 3 |
| Innovation/Momentum | 4 | 4 | 2 |
| **TOTAL** | **36** | **32** | **32** |

### Option A: Per-Podcast TaskPool + Channel (Recommended)

- **Strengths**: Creates a fresh `TaskPool` for each podcast being synced. Downloads for one podcast complete before moving to the next. Simple lifecycle management -- dropping the pool cancels in-flight downloads. Mirrors the TUI pattern exactly (SRC-006). Easy to implement the episode cap per podcast. Clear error boundaries per podcast (SRC-004, SRC-005).
- **Weaknesses**: Sequential podcast processing means total sync time = sum of all podcast sync times. If one podcast has many new episodes, it blocks processing of subsequent podcasts. However, for a background sync task, this is acceptable (SRC-014).
- **Best For**: This exact use case -- a background sync where reliability and simplicity matter more than parallelism across podcasts.

### Option B: Shared TaskPool Across All Podcasts

- **Strengths**: All downloads across all podcasts share a single `TaskPool`, maximizing download concurrency. Total sync time is minimized when multiple podcasts have new episodes. Downloads from different podcasts interleave efficiently (SRC-005).
- **Weaknesses**: More complex result handling -- need to track which podcast each `PodcastDLResult` belongs to. Cannot easily cap per-podcast downloads independently. Error in one podcast's download could interact with another's tracking. The `TaskPool` lifetime must span the entire sync pass, making cancellation granularity coarser. Harder to implement per-podcast error isolation (AC-08) cleanly (SRC-018).
- **Best For**: High-throughput sync scenarios with many podcasts needing simultaneous updates.

### Option C: Sequential Download (No TaskPool)

- **Strengths**: Simplest possible implementation. Download one episode at a time. No concurrency primitives needed. Zero risk of overwhelming network or disk (SRC-018). Easiest to reason about error handling and DB writes.
- **Weaknesses**: Very slow for podcasts with many new episodes (e.g., first sync of a podcast with 50 new episodes downloads them one at a time). Does not reuse existing `download_list` infrastructure -- would require making `download_file` public or reimplementing it. Misses the performance benefit of pipelining network requests (SRC-004).
- **Best For**: Minimal-complexity implementations or extremely constrained environments.

---

## Best Practices

### BP-001: Use PodcastUrl Track Source for Enqueuing Podcast Episodes

- **Pattern**: When adding podcast episodes to the play queue from the sync task, use `PlaylistTrackSource::PodcastUrl(episode_url)` rather than `PlaylistTrackSource::Path(file_path)`.
- **Rationale**: The `PodcastUrl` variant triggers `track_from_podcasturi` which looks up the episode in `db_podcast` by URL and creates a full `Track` object with podcast metadata (title, duration, last_position). Using `Path` would create a generic music track without podcast-specific behavior like position remembering (SRC-016, SRC-006).
- **Source**: SRC-016
- **Confidence**: High
- **Example**:
```rust
let add_cmd = PlaylistAddTrack::new_single(
    u64::MAX, // append to end (at_index >= playlist.len() triggers end-append)
    PlaylistTrackSource::PodcastUrl(ep_data.url.clone()),
);
cmd_tx.send(PlayerCmd::PlaylistAddTrack(add_cmd));
```

### BP-002: Separate Database Connection with busy_timeout for Sync Task

- **Pattern**: Open a dedicated `Database::new(&db_path)` instance for the sync task. Rely on the default 5000ms busy_timeout already set by rusqlite.
- **Rationale**: The player loop owns its own `db_podcast` instance. SQLite in WAL mode supports concurrent readers with a single writer (SRC-009, SRC-010, SRC-012). The sync task primarily reads (get_podcasts, get_episodes) and occasionally writes (insert_file after download). The default busy_timeout of 5000ms handles transient write contention gracefully (SRC-010).
- **Source**: SRC-009, SRC-010, SRC-012
- **Confidence**: High
- **Example**:
```rust
// In sync task initialization
let db_path = utils::get_app_config_path()?;
let db = Database::new(&db_path)?;
// db.conn automatically has busy_timeout(5000) from rusqlite defaults
```

### BP-003: Filter by ep.path.is_none() After update_podcast

- **Pattern**: After calling `update_podcast` (which inserts new episodes), query `get_episodes(pod_id, true)` and filter for episodes where `path.is_none()`. This identifies episodes needing download.
- **Rationale**: The `files` table stores the downloaded file path keyed by episode_id. If no entry exists in `files` for an episode, `ep.path` will be `None` in the joined query. This approach handles both newly inserted episodes and previously failed downloads in a single pass (SRC-003, SRC-006).
- **Source**: SRC-003, SRC-006
- **Confidence**: High

### BP-004: Drop TaskPool for Graceful Download Cancellation

- **Pattern**: When the sync task receives a cancellation signal (via `select!` on `cancel_token.cancelled()`), simply drop the `TaskPool` instance. This cancels all in-flight downloads via the pool's internal `CancellationToken`.
- **Rationale**: `TaskPool::drop` calls `cancel_token.cancel()` which terminates all spawned tasks via `tokio::select!` (SRC-005 lines 61-68). Combined with `select!` on the outer sync loop, this provides clean two-level cancellation: the sync pass stops and all downloads abort.
- **Source**: SRC-005
- **Confidence**: High

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| Making `download_file` public and calling it directly | Bypasses concurrency limiting and retry logic; tight coupling to internal implementation | Use `download_list` with `TaskPool` for bounded concurrency (SRC-004) | SRC-004 |
| Sharing `db_podcast` instance from player loop thread | `rusqlite::Connection` is not Sync; would require Mutex wrapping which blocks the player loop during sync DB calls | Open a separate `Database::new()` instance for the sync task (SRC-009, SRC-010) | SRC-009, SRC-010 |
| Downloading all episodes without limit on first subscription | Can saturate bandwidth for hours, fill disk, and delay availability of first episode (SRC-018) | Cap at configurable N most recent episodes per sync pass (SRC-007, SRC-008) | SRC-018 |
| Using `PlaylistTrackSource::Path` for podcast episodes | Loses podcast metadata (title, duration, last_position tracking) | Use `PlaylistTrackSource::PodcastUrl(url)` which resolves against db_podcast (SRC-016) | SRC-016 |

---

## Implementation Considerations

### Performance

- The sync task creates a new `TaskPool` per podcast. Creating a `TaskPool` involves allocating an `Arc<Semaphore>` and a `CancellationToken` -- negligible cost (SRC-005).
- SQLite WAL mode allows the sync task to read while the player loop writes (position updates) without blocking. The only contention point is when the sync task calls `insert_file` while the player loop calls `set_last_position` -- both are writes, but the 5s busy_timeout handles this gracefully (SRC-009, SRC-012).
- The unbounded `cmd_tx` channel means `send()` never blocks the sync task, even if the player loop is busy processing other commands (SRC-001).

### Security

- No new attack surface introduced. Feed URLs come from the user's existing database.
- Download file paths are sanitized via `sanitize_filename` with `truncate: true` and `windows: true` (SRC-004 line 533-538).
- Connect timeout of 10s on download requests prevents indefinite hangs (SRC-004 line 494).

### Compatibility

- `humantime-serde` 1.1.1 remains fully compatible with current `serde` 1.x and `humantime` 2.x. Despite last update in 2022, it has 58M+ downloads and 304 reverse dependencies with no reported compatibility issues (SRC-013).
- The sync task's `Database::new()` call uses the same SQLite file as the player loop. rusqlite 0.39 defaults to multi-threaded mode with busy_timeout(5000) (SRC-010).
- Existing configs without `[synchronization]` section will use `#[serde(default)]` defaults -- backward compatible (SRC-014).

---

## Contradictions Found

| Topic | Position A (SRC-009) | Position B (SRC-012) | Assessment |
|-------|---------------------|---------------------|------------|
| busy_timeout adequacy | "anything below 5 seconds led to occasional 'database is locked' errors given enough concurrent write transactions" (SRC-012) | rusqlite defaults to 5000ms busy_timeout (SRC-010) | For our use case (exactly 2 writers: player loop + sync task), the default 5000ms is more than adequate. The blog's concern is about many concurrent writers, which does not apply here. The default is sufficient. |
| Connection pooling necessity | sqlite-kit recommends read/write split with connection pool for "async services that need a single embedded SQLite database to behave well under concurrent load" (SRC-011) | For 2 consumers (player + sync), separate connections with busy_timeout is sufficient | A full connection pool is overkill. We have exactly one reader/writer (player loop) and one occasional writer (sync task). Two separate `Database::new()` instances with the default busy_timeout is the right level of abstraction. |

---

## Issues and Ambiguities

- **ISS-005**: The `at_index` parameter in `PlaylistAddTrack::new_single(u64::MAX, ...)` relies on the implementation detail that `at_index >= self.len()` triggers end-append (SRC-016 line 747). While this works correctly with the current code, a named constant or dedicated method would be clearer. **Resolution**: Use `u64::MAX` as it is guaranteed to be >= any reasonable playlist length. The existing code explicitly checks `if at_index >= self.len()` and appends.

- **ISS-006**: The sync task needs the `db_path` (config directory path) to open its own `Database` instance. This path is obtained via `utils::get_app_config_path()` which is already used in the `execute_action` function (SRC-014 line 676). The sync task should receive this path as a parameter. **Resolution**: Pass `config_dir_path` to the sync task function during spawn.

- **ISS-007**: After `download_list` spawns tasks into the `TaskPool`, the sync task must wait for all results before moving to the next podcast. If the `tx` sender is dropped (because `download_list` moves the closure into each task), the `rx.recv()` will return `None` when all senders are dropped. However, `download_list` clones the callback for each episode, and the original `tx` is moved into the closure that `download_list` takes. The sync task's `rx` will correctly receive `None` when all download tasks complete and their clones of `tx` are dropped. **Resolution**: The pattern works correctly -- drop the original `tx` after calling `download_list`, then loop on `rx.recv().await` until `None`.

---

## New Insights

1. **PlaylistTrackSource::PodcastUrl requirement**: The sync task MUST use `PodcastUrl(ep.url)` (not `Path(file_path)`) when enqueuing tracks. This is because the `track_from_podcasturi` function in the playlist performs a DB lookup to create a proper podcast `Track` with metadata. If we used `Path`, the episode would be treated as a generic music file and lose podcast-specific behaviors (position remembering, duration display).

2. **Two-phase download tracking**: The sync task's download flow requires two DB operations per episode: first `update_podcast` inserts the episode metadata, then after download, `insert_file` records the file path. The enqueue step (`PlaylistAddTrack`) must happen after `insert_file` because `track_from_podcasturi` calls `db_pod.get_episode_by_url` which needs the episode to exist with a file path to create a playable track. Actually -- re-examining the code, `track_from_podcasturi` (line 889-894) calls `db_pod.get_episode_by_url(uri)` and then `Track::from_podcast_episode(&ep)`. The `Episode` struct has `path: Option<PathBuf>`. If `path` is `None`, the Track may not be playable. Therefore, the enqueue MUST happen after `insert_file` registers the path.

3. **Cancellation during download awaiting**: If the service cancel token fires while the sync task is awaiting download results, the `select!` on the outer loop will trigger. The `TaskPool` will be dropped (cancelling downloads), and the `rx` channel will close. No cleanup of partial state is needed because:
   - Episodes already inserted via `update_podcast` remain in the DB (they will be retried on next sync)
   - Files already downloaded but not yet registered via `insert_file` remain on disk (harmless orphans)
   - The next sync pass will pick them up again (filter by `path.is_none()`)

---

## References

### Primary Sources (Official Documentation)

- SRC-001: Tokio mpsc module documentation -- https://docs.rs/tokio/latest/tokio/sync/mpsc/
- SRC-002: Tokio Channels Tutorial -- https://tokio.rs/tokio/tutorial/channels
- SRC-010: rusqlite Connection documentation (busy_timeout) -- https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html
- SRC-013: humantime-serde crate -- https://crates.io/crates/humantime-serde

### Secondary Sources (Blogs, Papers, Guides)

- SRC-009: PSA: Your SQLite Connection Pool Might Be Ruining Your Write Performance -- https://emschwartz.me/psa-your-sqlite-connection-pool-might-be-ruining-your-write-performance/
- SRC-012: SQLite concurrent writes and "database is locked" errors -- https://tenthousandmeters.com/blog/sqlite-concurrent-writes-and-database-is-locked-errors/
- SRC-015: DeepWiki termusic architecture analysis -- https://deepwiki.com

### Community Sources (GitHub, Reddit, X/Twitter)

- SRC-003: termusic Database::update_podcast -- lib/src/podcast/db/mod.rs (local codebase)
- SRC-004: termusic download_list function -- lib/src/podcast/mod.rs (local codebase)
- SRC-005: termusic TaskPool implementation -- lib/src/taskpool.rs (local codebase)
- SRC-006: termusic TUI episode_download_complete -- tui/src/ui/components/podcast.rs (local codebase)
- SRC-007: podpull (Rust podcast sync CLI) -- https://github.com/jakobwesthoff/podpull
- SRC-008: podcast-tui (Rust podcast TUI) -- https://github.com/lqdev/podcast-tui
- SRC-011: sqlite-kit (SQLite connection pool for Rust) -- https://github.com/hotnsoursoup/sqlite-kit
- SRC-014: termusic server.rs -- server/src/server.rs (local codebase)
- SRC-016: termusic PlaylistAddTrack handler -- server/src/server.rs:502-514 (local codebase)
- SRC-017: sqlite_rwc documentation -- https://docs.rs/sqlite-rwc/latest/sqlite_rwc/
- SRC-018: AntennaPod concurrent download issue -- https://github.com/AntennaPod/AntennaPod/issues/7876
