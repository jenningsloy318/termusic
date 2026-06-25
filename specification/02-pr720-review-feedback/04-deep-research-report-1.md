---
name: deep-research-report-1
description: Deep research report resolving ISS-001 through ISS-006 from the initial research pass — TUI-to-server migration contract, refresh_on_startup representation, per-podcast interval exposure, episode ordering, download concurrency architecture, and last_checked timestamp management.
doc-type: research-report
gate-profile: null
---

# Deep Research Report: PR #720 Podcast Synchronization — Issue Resolution

## Metadata

| Field | Value |
|-------|-------|
| Title | Deep Research: Resolving Architecture Design Issues ISS-001 through ISS-006 |
| Date | 2026-06-25 |
| Author | super-dev:research-agent |
| Research Period | 2026-06-25 |
| Technologies | Rust, Tokio, tonic/gRPC, Protobuf, rusqlite, TOML/serde |
| Freshness | Fresh (< 6mo) — codebase actively developed, PR under review |

---

## Executive Summary

- ISS-001 (TUI-to-server migration contract): The existing gRPC service uses `PlayerCmd` internally but `player.proto` RPC externally. Two viable patterns exist: (A) new RPC methods in player.proto for podcast operations, or (B) a generic `ServerCommand` RPC that wraps podcast-specific payloads. Pattern A is recommended for type safety and reviewer preference (SRC-001, SRC-004).
- ISS-002 (refresh_on_startup representation): The cleanest design is a single `interval_at` start-time toggle using `Option<Duration>` with `None` = disabled, combined with an independent `refresh_on_startup: bool`. This matches the reviewer's suggestion while preserving user control (SRC-001, SRC-005).
- ISS-003 (per-podcast check_interval exposure): DB-only storage with future TUI/CLI exposure is the correct approach. The TOML config should NOT expose per-podcast overrides (SRC-001, SRC-014).
- ISS-004 (episode ordering between podcast groups): Subscription order (database insertion order = `podcasts.id ASC`) is the most deterministic and user-understandable ordering, matching Miniflux's batch approach (SRC-014, SRC-016).
- ISS-005 (download tasks blocking feed receiver): A two-phase architecture (all feeds first, then all downloads) using a single shared TaskPool is the correct design for this codebase's complexity level (SRC-001, SRC-010).
- ISS-006 (last_checked not updated): Investigation reveals `last_checked` IS updated on success via `update_podcast` (which receives `Utc::now()` from `parse_feed_data`), but NOT on feed-fetch failure. For per-podcast scheduling, an explicit `UPDATE podcasts SET last_checked = ? WHERE id = ?` after processing each podcast (success or failure) is needed (SRC-009, SRC-017).

**Recommendation**: Implement the per-issue resolutions as described in Options Comparison. All 6 issues have clear resolution paths. Confidence: High.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| tonic grpc service podcast refresh download rpc language:rust | GitHub Code Search | 0 | 0 |
| interval_at Instant::now select cancelled periodic task language:rust | GitHub Code Search | 133 | 4 |
| tokio spawn_blocking read_dir HashSet existing files language:rust | GitHub Code Search | 43 | 3 |
| TUI-server communication podcast operations | DeepWiki (tramhao/termusic) | 1 | 1 |
| Miniflux per-feed polling frequency scheduling | DeepWiki (miniflux/v2) | 1 | 1 |
| player.proto service definition (codebase) | Codebase analysis | 1 | 1 |
| podcast_sync.rs sync_once implementation (codebase) | Codebase analysis | 1 | 1 |
| podcast/db/podcast_db.rs update_podcast (codebase) | Codebase analysis | 1 | 1 |
| podcast/mod.rs parse_feed_data last_checked (codebase) | Codebase analysis | 1 | 1 |
| music_player_service.rs RPC implementation pattern (codebase) | Codebase analysis | 1 | 1 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-001 | PR #720 Review Comments (59 threads) — github.com/tramhao/termusic/pull/720 | GitHub PR Review | 2026-06-24 | Fresh | High |
| SRC-004 | DeepWiki: termusic TUI-server podcast communication analysis | AI Documentation | 2026-06-25 | Fresh | Medium |
| SRC-005 | tokio::time::interval_at — docs.rs/tokio/latest/tokio/time/fn.interval_at.html | Official Docs | 2026-06-25 | Fresh | High |
| SRC-009 | termusic lib/src/podcast/db/migration.rs — DB migration pattern | Codebase | 2026-06-24 | Fresh | High |
| SRC-010 | termusic lib/src/taskpool.rs — TaskPool with Semaphore + CancellationToken | Codebase | 2026-06-24 | Fresh | High |
| SRC-014 | Miniflux scheduling architecture (DeepWiki analysis) | AI Documentation | 2026-06-25 | Fresh | Medium |
| SRC-015 | termusic lib/src/config/v2/server/mod.rs — ServerSettings config structure | Codebase | 2026-06-24 | Fresh | High |
| SRC-016 | termusic lib/proto/player.proto — existing gRPC service definition | Codebase | 2026-06-24 | Fresh | High |
| SRC-017 | termusic lib/src/podcast/db/podcast_db.rs — update_podcast SQL | Codebase | 2026-06-24 | Fresh | High |
| SRC-018 | termusic server/src/music_player_service.rs — RPC impl pattern | Codebase | 2026-06-24 | Fresh | High |
| SRC-019 | termusic tui/src/ui/components/podcast.rs — TUI direct calls | Codebase | 2026-06-24 | Fresh | High |
| SRC-020 | estuary/flow task_manager.rs — periodic refresh with interval_at | GitHub | 2026-06-25 | Fresh | Medium |
| SRC-021 | openai/codex websocket.rs — interval_at + MissedTickBehavior | GitHub | 2026-06-25 | Fresh | Medium |
| SRC-022 | n0-computer/iroh socket.rs — periodic_stun interval_at pattern | GitHub | 2026-06-25 | Fresh | Medium |

---

## Per-Issue Analysis

### ISS-001: TUI-to-Server Migration — Communication Contract Design

**Prior Understanding**: The TUI currently calls `check_feed()` and `download_list()` directly from `termusiclib::podcast` (SRC-019). Migrating these to server-side requires new communication mechanisms. The existing `PlayerCmd` enum has no podcast-specific variants.

**Investigation Summary**: Analysis of the existing architecture reveals:
1. The gRPC `MusicPlayer` service in `player.proto` defines RPC methods that map to `PlayerCmd` variants (SRC-016).
2. `MusicPlayerService` translates gRPC calls into `PlayerCmd` sends via `cmd_tx` (SRC-018).
3. The TUI already has a `Playback` client struct in `tui/src/ui/music_player_client.rs` that calls gRPC methods (SRC-004).
4. Currently, podcast operations bypass this entirely — the TUI opens its own `Database` connection and calls `check_feed`/`download_list` directly with its own `TaskPool` (SRC-019).
5. There is NO existing `PlayerCmd` variant for podcast operations — only `PlaylistAddTrack` is used post-download (SRC-019).

**Resolution Status**: RESOLVED — clear design path identified.

---

## Options Comparison

| Criterion | Option A: New Proto RPC Methods | Option B: PlayerCmd Variants Only (No New RPC) | Option C: Generic ServerCommand RPC | Option D: Hybrid (RPC for trigger, internal PlayerCmd for state) |
|-----------|--------------------------------|-----------------------------------------------|-------------------------------------|--------------------------------------------------------------|
| Maturity | 5 | 4 | 3 | 5 |
| Community/Support | 5 | 4 | 3 | 4 |
| Performance | 4 | 5 | 4 | 4 |
| Bundle Size / Footprint | 4 | 5 | 4 | 4 |
| Learning Curve | 4 | 5 | 3 | 4 |
| Maintenance Burden | 4 | 3 | 3 | 5 |
| Project Fit | 5 | 3 | 2 | 5 |
| Innovation/Momentum | 4 | 3 | 4 | 4 |
| **TOTAL** | **35** | **32** | **26** | **35** |

### Option A: New Proto RPC Methods (Dedicated Podcast Service Extension)

**Summary**: Add new RPC methods to the existing `MusicPlayer` service in `player.proto` for podcast operations: `RefreshPodcastFeeds`, `DownloadEpisodes`, `GetPodcastList`, `AddPodcast`, `RemovePodcast`.

```protobuf
// Additions to service MusicPlayer in player.proto
rpc RefreshPodcastFeeds(PodcastRefreshRequest) returns (stream PodcastSyncUpdate);
rpc DownloadEpisodes(EpisodeDownloadRequest) returns (stream EpisodeDownloadUpdate);
rpc GetPodcasts(Empty) returns (PodcastList);
rpc AddPodcast(AddPodcastRequest) returns (PodcastAddResult);
rpc RemovePodcast(RemovePodcastRequest) returns (Empty);
```

- **Strengths**: Type-safe gRPC contract. Streaming responses allow progress updates to TUI. Follows existing pattern in `MusicPlayerService` (SRC-018). Server can process podcast operations independently of TUI lifecycle (SRC-004). Reviewer expects server ownership of podcast ops (SRC-001).
- **Weaknesses**: Adds proto definitions and codegen. More complex than internal-only approach. Streaming RPCs have slightly higher complexity than unary. Requires corresponding client changes in TUI.
- **Best For**: Long-term maintainability and clean architecture. Enables future headless/CLI clients.

### Option B: PlayerCmd Variants Only (No New RPC)

**Summary**: Add podcast-specific variants to `PlayerCmd` enum (`PodcastRefreshAll`, `PodcastRefreshOne(i64)`, `PodcastDownloadEpisodes(Vec<EpData>)`) and trigger them via the existing `AddToPlaylist` RPC or a single generic command channel.

```rust
// Additions to PlayerCmd in playback/src/lib.rs
PodcastRefreshAll,
PodcastRefreshOne(i64),
PodcastDownloadEpisodes { podcast_id: i64, episodes: Vec<EpData> },
```

- **Strengths**: Minimal API surface change. Uses existing `cmd_tx` channel. No proto changes needed initially. Lower complexity for Phase 1.
- **Weaknesses**: No type-safe gRPC contract for podcast ops. Progress updates to TUI must use a separate mechanism (e.g., `StreamUpdates`). Mixes player commands with podcast commands in one enum — violates SRP. No streaming feedback to TUI during operations (SRC-004). Does not enable future headless/CLI podcast management without TUI.
- **Best For**: Quick Phase 1 migration if proto changes are deferred.

### Option C: Generic ServerCommand RPC

**Summary**: Add a single `rpc ExecuteCommand(ServerCommandRequest) returns (stream ServerCommandResponse)` that wraps arbitrary command types via a `oneof` payload.

- **Strengths**: Single RPC handles all future extensibility. Flexible payload.
- **Weaknesses**: Loses type safety at the proto boundary. Hard to document. Unusual pattern for gRPC services — reviewers may reject as over-engineering. Does not match the existing per-operation RPC style of the codebase (SRC-016). Harder for clients to discover available operations.
- **Best For**: Projects with many rapidly-evolving commands where proto stability is critical. Not appropriate here.

### Option D: Hybrid (RPC for trigger, internal PlayerCmd for state changes) — RECOMMENDED

**Summary**: Add focused RPC methods for podcast trigger operations (`RefreshPodcastFeeds`, `DownloadEpisodes`) that the server processes internally. Results are communicated back via `SubscribeServerUpdates` stream (already exists in proto — SRC-016). Internal state changes (like playlist additions from downloaded episodes) continue to use existing `PlayerCmd::PlaylistAddTrack`.

```protobuf
// New RPCs (trigger operations, server processes autonomously)
rpc RefreshPodcastFeeds(PodcastRefreshRequest) returns (PodcastRefreshResponse);
rpc DownloadEpisodes(EpisodeDownloadRequest) returns (EpisodeDownloadResponse);
rpc GetPodcasts(Empty) returns (PodcastList);
rpc AddPodcast(AddPodcastRequest) returns (PodcastAddResult);
rpc RemovePodcast(RemovePodcastRequest) returns (Empty);

// Progress delivered via existing StreamUpdates (extended with new variant)
message UpdatePodcastSync {
  oneof type {
    PodcastSyncProgress progress = 1;
    PodcastSyncComplete complete = 2;
  }
}
```

- **Strengths**: Clean separation — podcast RPCs trigger server-side processing, results flow via existing streaming mechanism. Reuses `SubscribeServerUpdates` for progress (no new streaming RPCs needed). Matches existing architecture where server owns state and broadcasts updates (SRC-016, SRC-018). TUI becomes thin client that just triggers and displays.
- **Weaknesses**: Requires extending `StreamUpdates` oneof with podcast variants. Slightly more complex than Option B for initial implementation.
- **Best For**: This exact project — leverages existing streaming infrastructure while adding type-safe triggers.

---

### ISS-002: refresh_on_startup Representation

**Prior Understanding**: With condensed `interval` field (0/absent = disabled), the role of `refresh_on_startup` was ambiguous — should it be independent boolean, tri-state enum, or eliminated via `interval_at(Instant::now())`?

**Investigation Summary**: The reviewer's comment (SRC-001) suggests combining startup sync and periodic sync into a single code path via `interval_at` start-time adjustment. However, there are three distinct user intentions:
1. "Check feeds on startup, then periodically" (refresh_on_startup=true, interval>0)
2. "Check feeds periodically but not on startup" (refresh_on_startup=false, interval>0)
3. "Never check feeds automatically" (interval=0 or absent)

The current implementation (SRC-002 lines 347-383) already handles cases 1 and 2 correctly with separate paths. The reviewer's suggestion to use `interval_at(Instant::now())` would unify cases 1 and 2 into a single loop but requires the boolean to control start time.

**Resolution**: Keep `refresh_on_startup` as an independent `bool` (default `true`). Unify the implementation:

```rust
let start = if refresh_on_startup {
    Instant::now()           // immediate first tick
} else {
    Instant::now() + duration // first tick after one interval
};
let mut timer = interval_at(start, duration);
timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
```

This eliminates the separate `if refresh_on_startup { sync_once(...).await }` block and the separate periodic loop. One `select!` loop handles both cases (SRC-005, SRC-020, SRC-021).

**Resolution Status**: RESOLVED.

**Design Decision**:
- `interval: Option<Duration>` — `None` or `Some(Duration::ZERO)` = disabled (AC-05)
- `refresh_on_startup: bool` — independent of interval, controls start time (AC-06)
- Single code path via `interval_at` start-time adjustment (AC-19)

---

### ISS-003: Per-Podcast check_interval Override Exposure

**Prior Understanding**: Should per-podcast `check_interval` be in TOML config, CLI-only, or TUI-managed? DB schema should support it regardless.

**Investigation Summary**: Miniflux's approach (SRC-014) stores per-feed intervals in the database only, not in configuration files. The global `POLLING_FREQUENCY` is in config, but per-feed overrides are managed programmatically. This makes sense because:
1. Config files should not contain per-entity overrides when entities are dynamic (podcasts are added/removed at runtime)
2. Per-podcast intervals are best managed via the TUI/CLI where the user interacts with individual podcasts
3. The DB is the single source of truth for podcast state

**Resolution**: Store `check_interval` in the database only (nullable INTEGER column, seconds). Do NOT expose in TOML config. Future TUI/CLI can allow setting it per-podcast.

```sql
-- migrations/002.sql
ALTER TABLE podcasts ADD COLUMN check_interval INTEGER;
-- NULL means "use global interval from config"
-- Non-NULL is per-podcast override in seconds
```

```rust
// In sync scheduling logic:
fn effective_interval(podcast: &Podcast, global: Duration) -> Duration {
    podcast.check_interval
        .map(|secs| Duration::from_secs(secs as u64))
        .unwrap_or(global)
}
```

**Resolution Status**: RESOLVED.

---

### ISS-004: Episode Ordering Between Podcast Groups in Same Sync Pass

**Prior Understanding**: Requirements specify within-podcast chronological ordering (oldest first) but not ordering BETWEEN podcast groups when multiple podcasts have new episodes.

**Investigation Summary**: Miniflux orders feeds by `next_check_at ASC` — feeds due soonest are processed first (SRC-014). In termusic, the order of podcasts in `get_podcasts()` is determined by the SQLite query, which returns them in `id ASC` order (insertion order = subscription order).

Three options exist:
1. **Subscription order** (database ID order): Deterministic, user-understandable ("podcasts I subscribed to first get enqueued first")
2. **Alphabetical**: Predictable but arbitrary and locale-dependent
3. **Last-checked order** (least recently checked first): Adaptive but non-obvious to users

**Resolution**: Use subscription order (`podcasts.id ASC`, which is the current `get_podcasts()` order). This is deterministic, requires no code changes beyond documentation, and matches user expectations. The playlist will contain groups ordered as:

```
[Podcast A episodes (oldest→newest)] [Podcast B episodes (oldest→newest)] ...
```

Where A, B ordering = subscription order.

**Resolution Status**: RESOLVED.

---

### ISS-005: Download Tasks Blocking Feed Receiver

**Prior Understanding**: The current implementation creates a per-podcast `dl_taskpool` inside the feed-processing loop, which means downloads for podcast A block feed processing for podcast B (the `while let Some(dl_result) = dl_rx.recv().await` loop at line 240 in sync_once).

**Investigation Summary**: The reviewer stated: "a download should be a task in-of-itself instead of blocking the podcast feed update receiver" (SRC-001). Two architectures are possible:

**Architecture 1: Sequential Phases (all feeds, then all downloads)**
```
Phase 1: Fetch all feeds → collect all episodes needing download
Phase 2: Download all episodes using shared TaskPool → drain results
```

**Architecture 2: Concurrent (spawn download tasks immediately per-podcast)**
```
For each feed result:
  - spawn download tasks immediately
  - collect results via shared channel
Feed processing and downloads interleave
```

Architecture 1 is simpler, avoids complex synchronization, and matches the reviewer's concern (feed processing is never blocked by downloads). Architecture 2 offers slightly better latency (downloads start sooner) but introduces complexity around shared state and makes ordering guarantees harder.

**Resolution**: Architecture 1 (Sequential Phases) with a single shared TaskPool.

```rust
pub async fn sync_once(...) -> Result<SyncPassStats> {
    // Phase 1: Fetch all feeds, collect episodes to download
    let taskpool = TaskPool::new(concurrent_downloads_max);
    let episodes_to_download: Vec<(PodcastId, Vec<EpData>)> = Vec::new();
    
    // ... dispatch all feed fetches via taskpool ...
    // ... process all feed results, collecting episodes_to_download ...
    
    // Phase 2: Download all collected episodes using SAME taskpool
    let (dl_tx, mut dl_rx) = unbounded_channel();
    for (pod_id, episodes) in &episodes_to_download {
        download_list(episodes, &pod_dir, max_retries, &taskpool, ...);
    }
    
    // ... drain all download results ...
}
```

This ensures:
- Feed processing is NEVER blocked by downloads (SRC-001)
- A single TaskPool bounds total concurrency (SRC-001, SRC-010)
- Episode ordering is maintained (groups by podcast, per-podcast chronological)

**Resolution Status**: RESOLVED.

---

### ISS-006: sync_once Does NOT Update last_checked After Processing

**Prior Understanding**: The initial report stated `sync_once` does not update `last_checked`. This was identified as blocking per-podcast scheduling.

**Investigation Summary**: Detailed code analysis reveals this is PARTIALLY INCORRECT:
- `parse_feed_data()` in `lib/src/podcast/mod.rs:146` sets `last_checked = Utc::now()` (SRC-017)
- `check_feed()` returns a `PodcastNoId` with this timestamp in the `SyncData` variant
- `sync_once` calls `db.update_podcast(pod_id, &pod_data)` at line 115 (SRC-002)
- `update_podcast` SQL includes `SET ... last_checked = :last_checked` (SRC-017)

So on the SUCCESS path, `last_checked` IS updated (to the time when the feed was parsed).

However, on the FAILURE path (`PodcastSyncResult::Error`), `last_checked` is NOT updated. This means:
- A podcast with a persistently broken feed will be retried every sync pass (never skipped)
- For per-podcast scheduling to work correctly, we need to decide: should `last_checked` be updated on failure?

**Resolution**: Update `last_checked` on BOTH success and failure paths. The purpose of `last_checked` is "when did we last attempt to check this feed" — for scheduling purposes, a failed check still counts as a check. Without this, a broken feed will be re-checked every single pass, creating unnecessary load.

Add an explicit update after processing each podcast:

```rust
// After processing each podcast (success or failure):
if let Err(err) = db.update_last_checked(pod_id, Utc::now()) {
    warn!("Failed to update last_checked for podcast {pod_id}: {err}");
}
```

New DB method needed:
```rust
pub fn update_last_checked(&self, pod_id: i64, timestamp: DateTime<Utc>) -> Result<()> {
    self.conn.execute(
        "UPDATE podcasts SET last_checked = ? WHERE id = ?",
        params![timestamp.timestamp(), pod_id],
    )?;
    Ok(())
}
```

**Resolution Status**: RESOLVED.

---

## Best Practices

### BP-008: Hybrid RPC + StreamUpdates for Long-Running Server Operations

- **Pattern**: Use unary gRPC RPCs to trigger long-running operations (feed refresh, downloads). Deliver progress and completion notifications via the existing server-to-client streaming channel (`SubscribeServerUpdates`).
- **Rationale**: Avoids bidirectional streaming complexity. Leverages the existing broadcast mechanism. TUI reconnection automatically picks up missed events via `UpdateMissedEvents`. Follows the existing architecture where server owns state and clients subscribe (SRC-016, SRC-018).
- **Source**: SRC-016, SRC-018, SRC-004
- **Confidence**: High

### BP-009: Two-Phase Sync Architecture (Feeds-Then-Downloads)

- **Pattern**: Process all feed fetches first (collecting episodes to download), then process all downloads in a second phase. Use a single shared `TaskPool` for both phases.
- **Rationale**: Eliminates feed-processing blocking by downloads. Simplifies ordering guarantees (episodes can be sorted before download dispatch). Single TaskPool prevents over-subscription of network resources (SRC-001, SRC-010).
- **Source**: SRC-001, SRC-010, SRC-014
- **Confidence**: High
- **Example**:
```rust
// Phase 1: All feeds
let mut to_download: Vec<(i64, PathBuf, Vec<EpData>)> = Vec::new();
while let Some(feed_result) = feed_rx.recv().await {
    // ... process feed, accumulate episodes ...
    to_download.push((pod_id, pod_dir, episodes));
}

// Phase 2: All downloads (using same taskpool)
for (pod_id, dir, episodes) in to_download {
    download_list(episodes, &dir, retries, &taskpool, callback);
}
while let Some(dl_result) = dl_rx.recv().await { /* process */ }
```

### BP-010: Update last_checked on Both Success and Failure Paths

- **Pattern**: Always update the `last_checked` timestamp after attempting a feed check, regardless of whether the check succeeded or failed.
- **Rationale**: For per-podcast scheduling, `last_checked` represents "when was the last attempt" — not "when was the last success." Without this, broken feeds create unbounded retry storms. Miniflux uses the same approach: `next_check_at` is always advanced, with a separate error counter for disabling (SRC-014).
- **Source**: SRC-014, SRC-017
- **Confidence**: High

### BP-011: DB-Only Storage for Per-Entity Overrides

- **Pattern**: Store per-podcast `check_interval` in the database only. Global defaults go in TOML config. Per-entity overrides are managed via runtime interfaces (TUI/CLI), not config files.
- **Rationale**: Config files should be entity-agnostic. Podcasts are dynamic (added/removed at runtime). Mixing entity state into config creates synchronization problems and confusing UX. Miniflux stores all per-feed settings in the database with a web UI for management (SRC-014).
- **Source**: SRC-014, SRC-015
- **Confidence**: High

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| Downloads blocking feed loop iteration | One slow download delays processing of subsequent podcast feeds; O(podcasts * avg_download_time) latency | Two-phase: all feeds first, then all downloads | SRC-001 |
| Per-podcast TaskPool creation inside feed loop | N TaskPools for N podcasts defeats concurrency bounding; resource waste | Single shared TaskPool for entire sync pass | SRC-001, SRC-010 |
| Per-entity overrides in TOML config | Config becomes unwieldy with many entities; sync issues between config and runtime state | Store in database, manage via TUI/CLI | SRC-014, SRC-015 |
| Never updating last_checked on failure | Broken feeds checked every pass; unbounded retry storm; wasted resources | Always update last_checked; use separate error_count for eventual disable | SRC-014, SRC-017 |
| Separate startup-sync and periodic-sync code paths | Duplicated logic; divergent behavior if one path is updated but not the other | Single interval_at loop with conditional start time | SRC-001, SRC-005 |

---

## Implementation Considerations

### Performance

- The two-phase architecture means downloads only begin after ALL feeds are processed. For users with many podcasts (50+), this could add latency before downloads start. Mitigation: the shared TaskPool allows feed fetches and downloads to overlap if the pool is reused across phases without draining — but the simpler "drain feeds, then start downloads" approach is preferred for correctness (SRC-001, SRC-010).
- Pre-computing existing files as a `HashSet<String>` via `tokio::fs::read_dir` (or `spawn_blocking`) before the download phase avoids per-episode blocking I/O (SRC-001).
- `MissedTickBehavior::Delay` prevents burst catch-up if sync takes longer than the interval (SRC-005, SRC-021).

### Security

- New proto RPC methods for podcast management should not introduce authentication bypass — the existing UDS socket provides process-level access control (SRC-016).
- Test URLs in new integration tests must use `localhost`/`127.0.0.1` with wiremock (SRC-001).

### Compatibility

- Adding `check_interval INTEGER` column via `ALTER TABLE` is backward-compatible — nullable column with no default means existing rows get NULL which means "use global" (SRC-009).
- Proto additions are backward-compatible in protobuf (new fields/methods are ignored by old clients) (SRC-016).
- Config changes (moving `[synchronization]` under `[podcast]`) remain a breaking change for existing users and should be documented in release notes (SRC-015).

---

## Contradictions Found

| Topic | Position A (SRC-001) | Position B (SRC-002/SRC-017) | Assessment |
|-------|---------------------|------------------------------|------------|
| last_checked update timing | Reviewer implies sync_once should explicitly manage last_checked for per-podcast scheduling | Current code already updates last_checked implicitly via update_podcast on success path | Both are partially correct. The implicit update works for success, but an explicit update is needed on the failure path for scheduling correctness. Resolution: add explicit update_last_checked call for both paths. |
| TaskPool sharing scope | Reviewer says "one taskpool that shares concurrent_downloads_max" | Current code creates feed_taskpool at sync_once start (correct) but then creates per-podcast dl_taskpool (wrong) | Reviewer is correct. Use one taskpool for both feed fetches and downloads across the entire pass. |

---

## Issues and Ambiguities

- **ISS-007**: The `StreamUpdates` oneof in `player.proto` currently has 8 variants. Adding podcast sync progress variants will increase this. Should podcast updates be a sub-message (one variant with its own inner oneof) or multiple top-level variants? Sub-message is cleaner but adds nesting; top-level is simpler but pollutes the namespace.

- **ISS-008**: When per-podcast scheduling is implemented, should the scheduler wake up at the global interval and check which podcasts are due, or should it compute the next earliest due time and sleep exactly until then? The former is simpler (matches current interval_at pattern) but less efficient; the latter is more precise but requires recomputing sleep duration after each pass.

- **ISS-009**: The existing `update_podcast` SQL updates ALL fields (title, url, description, author, explicit, last_checked) from the fetched feed data. If only `last_checked` needs updating on failure, a new lightweight method `update_last_checked` is cleaner than calling `update_podcast` with stale data. This is a minor DB API addition.

---

## References

### Primary Sources (Official Documentation)

- SRC-005: tokio::time::interval_at — https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html

### Secondary Sources (Blogs, Papers, Guides)

- SRC-014: Miniflux scheduling architecture (per-feed polling, batch ordering) — DeepWiki analysis of miniflux/v2

### Community Sources (GitHub, Reddit, Discussions)

- SRC-001: PR #720 Review Comments by @hasezoey — https://github.com/tramhao/termusic/pull/720
- SRC-004: DeepWiki: termusic TUI-server communication analysis — https://deepwiki.com/tramhao/termusic
- SRC-020: estuary/flow task_manager.rs — periodic refresh with interval_at — https://github.com/estuary/flow
- SRC-021: openai/codex websocket.rs — interval_at + MissedTickBehavior — https://github.com/openai/codex
- SRC-022: n0-computer/iroh socket.rs — periodic_stun interval_at pattern — https://github.com/n0-computer/iroh

### Codebase Sources

- SRC-002: server/src/podcast_sync.rs — current sync implementation
- SRC-009: lib/src/podcast/db/migration.rs — DB migration infrastructure
- SRC-010: lib/src/taskpool.rs — TaskPool with Semaphore + CancellationToken
- SRC-015: lib/src/config/v2/server/mod.rs — ServerSettings structure
- SRC-016: lib/proto/player.proto — existing gRPC service definition
- SRC-017: lib/src/podcast/db/podcast_db.rs — update_podcast SQL with last_checked
- SRC-018: server/src/music_player_service.rs — RPC implementation pattern
- SRC-019: tui/src/ui/components/podcast.rs — TUI direct podcast calls
