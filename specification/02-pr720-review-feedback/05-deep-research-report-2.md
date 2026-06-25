---
name: deep-research-report-2
description: Deep research report resolving ISS-007 through ISS-009 from the first deep research pass — StreamUpdates oneof expansion strategy, per-podcast scheduler wake strategy, and DB API addition for lightweight last_checked update.
doc-type: research-report
gate-profile: null
---

# Deep Research Report 2: PR #720 Podcast Synchronization — Issue Resolution (ISS-007, ISS-008, ISS-009)

## Metadata

| Field | Value |
|-------|-------|
| Title | Deep Research: Resolving Design Issues ISS-007, ISS-008, ISS-009 |
| Date | 2026-06-25 |
| Author | super-dev:research-agent |
| Research Period | 2026-06-25 |
| Technologies | Rust, Tokio, tokio-util, Protobuf/tonic, rusqlite |
| Freshness | Fresh (< 6mo) — codebase actively developed, PR under review |

---

## Executive Summary

- ISS-007 (StreamUpdates oneof expansion): The codebase already uses the sub-message pattern (`UpdatePlaylist` with inner `oneof`) for grouping related events. Podcast sync updates should follow the same pattern: add one `UpdatePodcastSync` variant to the outer `StreamUpdates` oneof, with an inner oneof for progress/complete/error sub-types. This preserves namespace cleanliness and follows existing project conventions (SRC-023, SRC-024).
- ISS-008 (Per-podcast scheduler wake strategy): For termusic's use case (typically 5-50 podcasts), a global-interval wake-and-check approach is simpler and sufficient. `tokio-util::time::DelayQueue` exists in the dependency tree and provides optimal per-entity scheduling for future scaling, but the added complexity is not justified now. The recommended approach is the global-interval pattern matching Miniflux's architecture (SRC-025, SRC-026, SRC-027).
- ISS-009 (DB API for lightweight last_checked update): A dedicated `update_last_checked(id, timestamp)` method is straightforward to add alongside the existing `update_podcast` method. It should be a single `UPDATE podcasts SET last_checked = ? WHERE id = ?` query, consistent with the existing DB API style (SRC-028, SRC-029).

**Recommendation**: Implement all three resolutions as described. All issues have clear, low-risk resolution paths with strong precedent in the existing codebase. Confidence: High.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| protobuf oneof nested sub-message vs flat variants design pattern | DeepWiki (grpc/grpc) | 1 | 1 |
| tokio DelayQueue per-entity scheduling periodic tasks | DeepWiki (tokio-rs/tokio) | 1 | 1 |
| Miniflux per-feed polling scheduler wake strategy | DeepWiki (miniflux/v2) | 1 | 1 |
| tokio-util::time::DelayQueue documentation | WebFetch (docs.rs) | 1 | 1 |
| tokio::time::interval_at documentation | WebFetch (docs.rs) | 1 | 1 |
| MissedTickBehavior enum variants | WebFetch (docs.rs) | 1 | 1 |
| StreamUpdates oneof variants (codebase) | Codebase analysis | 1 | 1 |
| UpdatePlaylist sub-message pattern (codebase) | Codebase analysis | 1 | 1 |
| UpdateEvents enum Rust definition (codebase) | Codebase analysis | 1 | 1 |
| podcast_db.rs update_podcast SQL (codebase) | Codebase analysis | 1 | 1 |
| migration.rs user_version pattern (codebase) | Codebase analysis | 1 | 1 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-023 | lib/proto/player.proto — StreamUpdates oneof and UpdatePlaylist sub-message pattern | Codebase | 2026-06-24 | Fresh | High |
| SRC-024 | lib/src/player.rs — UpdatePlaylistEvents nested enum with From impl for protobuf | Codebase | 2026-06-24 | Fresh | High |
| SRC-025 | DeepWiki: tokio-rs/tokio — DelayQueue vs global interval for per-entity scheduling | AI Documentation | 2026-06-25 | Fresh | Medium |
| SRC-026 | DeepWiki: miniflux/v2 — POLLING_FREQUENCY global wake with next_check_at filtering | AI Documentation | 2026-06-25 | Fresh | Medium |
| SRC-027 | tokio-util::time::DelayQueue documentation — docs.rs/tokio-util/latest | Official Docs | 2026-06-25 | Fresh | High |
| SRC-028 | lib/src/podcast/db/podcast_db.rs — PodcastDBInsertable::update_podcast SQL pattern | Codebase | 2026-06-24 | Fresh | High |
| SRC-029 | lib/src/podcast/db/migration.rs — DB migration via user_version pragma | Codebase | 2026-06-24 | Fresh | High |
| SRC-030 | DeepWiki: grpc/grpc — oneof expansion patterns, nested vs flat | AI Documentation | 2026-06-25 | Fresh | Medium |
| SRC-031 | tokio::time::interval_at documentation — first tick at start, MissedTickBehavior | Official Docs | 2026-06-25 | Fresh | High |
| SRC-032 | server/src/podcast_sync.rs — current sync_once and start_podcast_sync_task implementation | Codebase | 2026-06-24 | Fresh | High |

---

## Per-Issue Analysis

### ISS-007: StreamUpdates oneof Expansion Strategy

**Prior Understanding**: The `StreamUpdates` oneof in `player.proto` currently has 8 variants. Adding podcast sync progress variants will increase this. The question was whether to add multiple top-level variants or use a sub-message with inner oneof.

**Investigation Summary**: Analysis of the existing codebase revealed a critical precedent that definitively answers this question:

1. **Existing precedent in the codebase**: The `UpdatePlaylist` message (lines 176-184 of player.proto) already uses exactly the sub-message pattern in question. It has its own inner `oneof type` with 6 variants (`add_track`, `remove_track`, `cleared`, `loop_mode`, `swap_tracks`, `shuffled`), and occupies a single slot (`playlist_changed = 7`) in the outer `StreamUpdates` oneof (SRC-023).

2. **Rust-side implementation**: The `UpdatePlaylistEvents` enum in `lib/src/player.rs` (line 277) is a nested enum that maps 1:1 to the proto sub-message, with a `From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist` impl (SRC-024). This pattern is clean and well-established.

3. **gRPC/protobuf guidance**: The grpc/grpc repository uses both flat and nested oneofs. For categorically-related updates (like "all podcast events"), nesting is recommended to preserve organization and prevent namespace pollution (SRC-030). The `Security` message in channelz.proto demonstrates nested oneofs with a `Tls` sub-message containing its own `cipher_suite` oneof.

4. **Proto naming convention**: The existing codebase uses the `Update` prefix for stream update messages (e.g., `UpdateVolumeChanged`, `UpdatePlaylist`). The podcast variant should follow suit: `UpdatePodcastSync`.

**Resolution Status**: RESOLVED — clear design path with strong codebase precedent.

**Resolution**: Use the sub-message pattern, identical to how `UpdatePlaylist` is structured.

```protobuf
// Add ONE variant to the StreamUpdates oneof:
message StreamUpdates {
  oneof type {
    // ... existing 8 variants (1-8) ...
    UpdatePodcastSync podcast_sync = 9;
  }
}

// New sub-message with its own inner oneof for podcast sync events:
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

Rust-side implementation follows the same pattern as `UpdatePlaylistEvents`:

```rust
// In lib/src/player.rs:
#[derive(Debug, Clone, PartialEq)]
pub enum UpdatePodcastSyncEvents {
    Started { total_podcasts: u64 },
    Progress { podcast_title: String, episodes_found: u64, episodes_downloaded: u64 },
    Complete(SyncPassStats),
    Error { podcast_title: String, error_message: String },
}

// Add variant to UpdateEvents:
pub enum UpdateEvents {
    // ... existing variants ...
    PodcastSync(UpdatePodcastSyncEvents),
}
```

---

### ISS-008: Per-Podcast Scheduler Wake Strategy

**Prior Understanding**: Two strategies were identified: (A) wake at global interval and check which podcasts are due, or (B) compute exact next-due time and sleep until then. The tradeoffs between simplicity and efficiency were unclear.

**Investigation Summary**: Three approaches were evaluated with concrete research:

1. **Global interval wake-and-check (Miniflux approach)**: Miniflux uses `POLLING_FREQUENCY` (default 60 minutes) as a global wake interval. On each wake, it queries the database for feeds where `next_check_at` is in the past, batches them, and dispatches to a worker pool. This approach is battle-tested for RSS aggregation with thousands of feeds (SRC-026).

2. **tokio-util::time::DelayQueue**: This utility manages per-entity timers efficiently. It uses an internal timer wheel and registers only ONE `Sleep` future with the runtime for its next-expiring item. Methods include `insert(value, timeout)`, `reset(key, timeout)`, and it implements `Stream` for expired items. The `time` feature is required (SRC-025, SRC-027). It is already in the workspace dependency (`tokio-util = "0.7.18"`) but would need the `time` feature added.

3. **Per-entity sleep_until**: Each podcast gets its own `Sleep` future via `tokio::time::sleep_until`. The tokio runtime's timer wheel handles these efficiently at O(1) per registration. However, managing 50+ individual futures and handling additions/removals is complex (SRC-025).

**Key factors for termusic**:
- Typical podcast count: 5-50 (not thousands like Miniflux)
- Current architecture: single `interval_at` loop with `sync_once` processing all podcasts (SRC-032)
- Reviewer expectation: per-podcast `last_checked` tracking for scheduling, not a fundamentally new scheduler architecture
- The codebase already opens a fresh DB connection per sync pass (SRC-032 line 53), making SQL-based filtering natural

**Resolution Status**: RESOLVED.

**Resolution**: Use the global-interval wake-and-check pattern (matching Miniflux). The scheduler wakes at the configured global interval, queries podcasts where `(now - last_checked) >= effective_interval`, and processes only those that are due.

```rust
pub async fn sync_once(
    config: &SharedServerSettings,
    cmd_tx: &PlayerCmdSender,
    db_path: &Path,
) -> Result<SyncPassStats> {
    // ...
    let podcasts = db.get_podcasts()?;
    let now = Utc::now();
    let global_interval = config.read().settings.podcast.synchronization.interval;
    
    // Filter to podcasts that are due for a check
    let due_podcasts: Vec<_> = podcasts.iter().filter(|p| {
        let effective_interval = p.check_interval
            .map(|secs| Duration::from_secs(secs as u64))
            .unwrap_or(global_interval);
        let elapsed = now.signed_duration_since(p.last_checked);
        elapsed >= chrono::Duration::from_std(effective_interval).unwrap_or(chrono::Duration::max_value())
    }).collect();
    
    // Process only due podcasts
    for podcast in &due_podcasts {
        // ... feed fetch and download ...
    }
}
```

**Why NOT DelayQueue for now**:
- Adds feature dependency (`tokio-util/time`)
- Requires managing `Key` handles for insert/reset/remove on podcast add/remove
- Requires persisting timer state across server restarts (DelayQueue is in-memory only, so initial population from DB is needed anyway)
- For 5-50 podcasts, the SQL filter costs microseconds — no measurable benefit from DelayQueue
- The global-interval pattern is simpler, matches Miniflux's proven approach, and aligns with the existing `interval_at` architecture

**Future path**: If termusic ever needs sub-minute precision or supports 1000+ podcasts, DelayQueue can replace the SQL-filter approach without changing the external API.

---

### ISS-009: DB API Addition for Lightweight last_checked Update

**Prior Understanding**: The existing `update_podcast` method updates ALL fields (title, url, description, author, explicit, last_checked) from fetched feed data. On the failure path, we only need to update `last_checked` without requiring full podcast data.

**Investigation Summary**: 

1. **Existing API pattern**: The `PodcastDBInsertable` struct (SRC-028) provides `update_podcast` which takes an ID and updates all fields via a single SQL UPDATE statement with named parameters. This is appropriate for the success path where `parse_feed_data` returns fresh metadata.

2. **Current last_checked column**: The `podcasts` table in 001.sql (SRC-029) already has `last_checked INTEGER` (nullable). The existing `update_podcast` SQL is: `UPDATE podcasts SET title = :title, url = :url, description = :description, author = :author, explicit = :explicit, last_checked = :last_checked WHERE id = :id` (SRC-028 lines 96-99).

3. **Gap identified**: There is no standalone method to update only `last_checked`. On the failure path (feed fetch error), calling `update_podcast` would require constructing a full `PodcastDBInsertable` with stale data just to set the timestamp — this is wasteful and semantically incorrect.

4. **Existing analogues**: The `delete_podcast` function (SRC-028 line 116) is a standalone function (not on `PodcastDBInsertable`) that operates on the connection directly. The new `update_last_checked` should follow this pattern.

**Resolution Status**: RESOLVED.

**Resolution**: Add a standalone function `update_last_checked` in `podcast_db.rs`:

```rust
/// Update only the `last_checked` timestamp for a podcast.
///
/// This is used on both success and failure paths during sync to record
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

**Design decisions**:
- Standalone function (not on `PodcastDBInsertable`) — follows `delete_podcast` pattern (SRC-028)
- Takes `PodcastDBId` and `DateTime<Utc>` — minimal parameters for the operation
- Uses `prepare_cached` — same as other DB methods for performance
- Stores as Unix timestamp (INTEGER) — matches existing `last_checked` column type
- Returns `Result<usize, rusqlite::Error>` — consistent with `delete_podcast` return type

**Integration in sync_once**:

```rust
// After processing each podcast (success OR failure):
match message {
    PodcastSyncResult::SyncData((pod_id, pod_data)) => {
        // Success: update_podcast already updates last_checked via full data
        db.update_podcast(pod_id, &pod_data)?;
        // ... process episodes ...
    }
    PodcastSyncResult::Error(feed) => {
        // Failure: update only last_checked so scheduler advances
        if let Some(pod_id) = feed.id {
            if let Err(err) = update_last_checked(pod_id, Utc::now(), db.conn()) {
                warn!("Failed to update last_checked for podcast {pod_id}: {err}");
            }
        }
    }
}
```

---

## Options Comparison

### ISS-007: StreamUpdates Oneof Expansion

| Criterion | Option A: Sub-message (inner oneof) | Option B: Multiple top-level variants | Option C: Separate streaming RPC |
|-----------|--------------------------------------|---------------------------------------|----------------------------------|
| Maturity | 5 | 5 | 4 |
| Community/Support | 5 | 5 | 4 |
| Performance | 5 | 5 | 3 |
| Bundle Size / Footprint | 5 | 4 | 3 |
| Learning Curve | 4 | 5 | 3 |
| Maintenance Burden | 5 | 3 | 3 |
| Project Fit | 5 | 3 | 2 |
| Innovation/Momentum | 4 | 3 | 4 |
| **TOTAL** | **38** | **33** | **26** |

#### Option A: Sub-message with Inner Oneof — RECOMMENDED

**Summary**: Add one `UpdatePodcastSync` variant to the outer `StreamUpdates` oneof. The `UpdatePodcastSync` message contains its own inner `oneof type` with variants for started/progress/complete/error.

- **Strengths**: Follows the exact pattern already used by `UpdatePlaylist` in this codebase (SRC-023, SRC-024). Keeps the outer `StreamUpdates` oneof from growing unboundedly. Groups semantically related events. Allows podcast sync events to evolve independently without touching the top-level message. Recommended by gRPC community for categorically-related events (SRC-030).
- **Weaknesses**: Adds one level of nesting in client code. Slightly more verbose proto definition.
- **Best For**: This project — matches existing conventions exactly.

#### Option B: Multiple Top-Level Variants

**Summary**: Add 3-4 new variants directly to the `StreamUpdates` oneof: `podcast_sync_started`, `podcast_sync_progress`, `podcast_sync_complete`, `podcast_sync_error`.

- **Strengths**: Flat structure is simpler to understand initially. No extra nesting in generated code. Direct access to each variant (SRC-030).
- **Weaknesses**: Grows the outer oneof from 8 to 12 variants. Pollutes the namespace. Diverges from the sub-message pattern already established by `UpdatePlaylist` (SRC-023). Makes it harder to add future event categories without further bloat. Inconsistent with project conventions.
- **Best For**: Small services with few event types that will not grow.

#### Option C: Separate Streaming RPC for Podcast Updates

**Summary**: Add a new `rpc SubscribePodcastUpdates(Empty) returns (stream PodcastSyncUpdate)` endpoint separate from `SubscribeServerUpdates`.

- **Strengths**: Complete isolation of podcast updates from player updates. Clients can subscribe to only what they need (SRC-030).
- **Weaknesses**: Duplicates the broadcast channel infrastructure. Clients must manage two streams. Significantly more implementation work. Not how the existing architecture works — `SubscribeServerUpdates` is the single event bus (SRC-023, SRC-032). The reviewer would likely reject this as over-engineering for a TUI app.
- **Best For**: Microservice architectures with many independent subscribers.

### ISS-008: Per-Podcast Scheduler Wake Strategy

| Criterion | Option A: Global interval wake-and-check | Option B: DelayQueue per-podcast | Option C: Per-podcast sleep_until |
|-----------|------------------------------------------|----------------------------------|-----------------------------------|
| Maturity | 5 | 4 | 4 |
| Community/Support | 5 | 4 | 4 |
| Performance | 4 | 5 | 5 |
| Bundle Size / Footprint | 5 | 4 | 5 |
| Learning Curve | 5 | 3 | 3 |
| Maintenance Burden | 5 | 3 | 2 |
| Project Fit | 5 | 3 | 2 |
| Innovation/Momentum | 3 | 5 | 4 |
| **TOTAL** | **37** | **31** | **29** |

#### Option A: Global Interval Wake-and-Check — RECOMMENDED

**Summary**: The existing `interval_at` timer wakes at the configured global interval. On each wake, `sync_once` queries all podcasts and filters to those where `(now - last_checked) >= effective_interval`. Only due podcasts are processed.

- **Strengths**: Minimal change to existing architecture (SRC-032). Matches Miniflux's proven approach for feed scheduling (SRC-026). No new dependencies or features needed. SQL-based filtering is trivial for 5-50 podcasts. Easy to reason about — one wake, one filter, one batch. Handles server restarts gracefully (just checks elapsed time on next wake). Aligns with reviewer expectations for incremental improvement (SRC-001 from prior reports).
- **Weaknesses**: Potential delay of up to `global_interval` before a newly-due podcast is processed. For 1-hour intervals with varied per-podcast intervals, some podcasts may wait up to 59 extra minutes. Not optimal for sub-minute precision requirements.
- **Best For**: This project — simple, proven, minimal architecture change.

#### Option B: DelayQueue Per-Podcast

**Summary**: Use `tokio_util::time::DelayQueue` to manage per-podcast timers. Each podcast is inserted with its effective interval. When expired, the podcast is processed and re-inserted with a fresh timeout.

- **Strengths**: Optimal wake precision — each podcast processed exactly when due (SRC-025, SRC-027). Only one internal Sleep registered with the runtime (efficient). Clean API for managing delayed items. Supports dynamic podcast additions/removals via `insert`/`remove`.
- **Weaknesses**: Requires adding `time` feature to `tokio-util` in server/Cargo.toml. In-memory state requires population from DB on startup. Handling server restarts means re-computing all deadlines anyway (same as Option A). More complex lifecycle management (Key tracking, reset on config change). Over-engineered for 5-50 podcasts where the SQL filter approach costs microseconds.
- **Best For**: Systems with 1000+ entities, sub-minute scheduling precision, or real-time requirements.

#### Option C: Per-Podcast sleep_until Futures

**Summary**: Spawn a separate task per podcast, each using `tokio::time::sleep_until` for its next-due time. Tasks independently process their podcast and reschedule.

- **Strengths**: Maximum scheduling precision per podcast (SRC-025). Each podcast is independently managed.
- **Weaknesses**: N tasks for N podcasts — resource overhead. Complex lifecycle management (adding/removing podcasts requires spawning/cancelling tasks). Shared TaskPool access from multiple tasks adds synchronization concerns. Harder to implement ordering guarantees between podcasts. No single point of control for graceful shutdown (need CancellationToken per task or shared). Fundamentally different architecture from existing code.
- **Best For**: Systems where each entity has radically different lifecycle requirements.

### ISS-009: DB API for Lightweight last_checked Update

| Criterion | Option A: Standalone function | Option B: Method on Database struct | Option C: Generic update_field helper |
|-----------|-------------------------------|-------------------------------------|--------------------------------------|
| Maturity | 5 | 5 | 3 |
| Community/Support | 5 | 5 | 3 |
| Performance | 5 | 5 | 4 |
| Bundle Size / Footprint | 5 | 5 | 4 |
| Learning Curve | 5 | 5 | 3 |
| Maintenance Burden | 5 | 5 | 3 |
| Project Fit | 5 | 4 | 2 |
| Innovation/Momentum | 3 | 3 | 4 |
| **TOTAL** | **38** | **37** | **26** |

#### Option A: Standalone Function — RECOMMENDED

**Summary**: A free function `update_last_checked(id, timestamp, conn)` following the same pattern as `delete_podcast(id, conn)`.

- **Strengths**: Matches existing code pattern — `delete_podcast` is a standalone function taking `(id, conn)` (SRC-028). Minimal API surface. Clear, single-purpose. No changes to existing types or traits. Easy to test in isolation.
- **Weaknesses**: Does not go through `PodcastDBInsertable` — but that is appropriate since this operation does not require podcast data.
- **Best For**: This project — matches the existing API style exactly.

#### Option B: Method on Database Struct

**Summary**: Add `pub fn update_last_checked(&self, id: PodcastDBId, timestamp: DateTime<Utc>) -> Result<()>` to the `Database` struct (wherever it wraps the Connection).

- **Strengths**: Groups all DB operations on one struct. Discoverable via IDE autocomplete on `db.`.
- **Weaknesses**: The existing `insert_podcast`/`update_podcast`/`delete_podcast` are NOT methods on a `Database` struct — they are on `PodcastDBInsertable` or standalone functions (SRC-028). Adding a method here would be inconsistent with the existing pattern.
- **Best For**: Projects that use a repository/DAO pattern with a single database object.

#### Option C: Generic update_field Helper

**Summary**: A generic `update_podcast_field(id, column_name, value, conn)` function that can update any single column.

- **Strengths**: Reusable for future single-column updates. Generic over column types.
- **Weaknesses**: SQL injection risk if column name is dynamic (must be hardcoded or validated). Over-engineered for a single use case. Harder to type-check at compile time. Not how the rest of the DB layer works. The reviewer would reject this as unnecessary abstraction.
- **Best For**: ORMs or query-builder patterns, not hand-written SQL.

---

## Best Practices

### BP-012: Sub-Message Pattern for Grouped StreamUpdate Events

- **Pattern**: When adding a new category of events to an existing `oneof`-based streaming message, create a sub-message with its own inner `oneof` and add one variant to the outer message.
- **Rationale**: Preserves bounded growth of the outer oneof. Groups semantically related events. Allows independent evolution of each category. This is already the pattern used by `UpdatePlaylist` in this codebase (SRC-023, SRC-024). The grpc/grpc repo demonstrates the same pattern in channelz.proto (SRC-030).
- **Source**: SRC-023, SRC-024, SRC-030
- **Confidence**: High
- **Example**:
```protobuf
// WRONG: growing the outer oneof for every new event type
message StreamUpdates {
  oneof type {
    // 8 existing + 4 new = 12 variants (and growing)
    UpdatePodcastSyncStarted podcast_started = 9;
    UpdatePodcastSyncProgress podcast_progress = 10;
    UpdatePodcastSyncComplete podcast_complete = 11;
    UpdatePodcastSyncError podcast_error = 12;
  }
}

// CORRECT: one variant with inner grouping
message StreamUpdates {
  oneof type {
    // 8 existing + 1 new = 9 variants (bounded)
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
```

### BP-013: Global-Interval Wake with SQL Filter for Per-Entity Scheduling

- **Pattern**: Use a single periodic timer at the global interval. On each tick, query the database for entities where `(now - last_checked) >= effective_interval`. Process only due entities.
- **Rationale**: Proven pattern used by Miniflux for thousands of RSS feeds (SRC-026). Handles server restarts gracefully (no in-memory timer state to reconstruct). SQL filtering is O(n) but for small N (5-50 podcasts) is measured in microseconds. Simpler to implement and debug than in-memory timer queues. State is always in the database — single source of truth.
- **Source**: SRC-026, SRC-031, SRC-032
- **Confidence**: High
- **Example**:
```rust
// In sync_once: filter podcasts by due time
let due_podcasts: Vec<_> = all_podcasts.iter().filter(|p| {
    let interval = p.check_interval
        .map(|s| Duration::from_secs(s as u64))
        .unwrap_or(global_interval);
    let elapsed = (now - p.last_checked).num_seconds();
    elapsed >= interval.as_secs() as i64
}).collect();
```

### BP-014: Dedicated Single-Column Update Methods for Hot Paths

- **Pattern**: When a specific column is updated frequently or independently of other columns (like `last_checked` on every sync pass), provide a dedicated lightweight update method rather than requiring the full entity update.
- **Rationale**: Avoids constructing full entity data structures for a single-field update. Reduces data transfer between application and DB. Makes the failure-path update clean (no stale data needed). Follows the principle of least privilege — the method only touches what it needs (SRC-028, SRC-029).
- **Source**: SRC-028, SRC-029
- **Confidence**: High
- **Example**:
```rust
// Lightweight: only touches one column
pub fn update_last_checked(id: PodcastDBId, ts: DateTime<Utc>, con: &Connection) -> Result<usize, rusqlite::Error> {
    con.prepare_cached("UPDATE podcasts SET last_checked = ? WHERE id = ?;")?
       .execute(params![ts.timestamp(), id])
}

// vs. Full update: requires all fields populated
pub fn update_podcast(&self, id: PodcastDBId, con: &Connection) -> Result<usize, rusqlite::Error> {
    // needs title, url, description, author, explicit, last_checked
}
```

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| Growing outer oneof unboundedly | Proto schema becomes unmanageable; code generation produces massive switch/match statements; every new feature pollutes shared namespace | Sub-message with inner oneof — one variant per category | SRC-023, SRC-030 |
| In-memory timer queue without DB backing for persistent entity scheduling | Timer state lost on restart; requires complex reconstruction logic; diverges from DB as source of truth | Database-backed scheduling with SQL filter on wake | SRC-026, SRC-032 |
| Calling full `update_podcast` with stale data on failure path | Semantically incorrect (could overwrite valid data with stale cache); requires unnecessary data construction | Dedicated `update_last_checked` method for the specific column | SRC-028 |
| Per-entity spawned tasks for scheduling | N tasks for N entities creates resource pressure; complex lifecycle management; harder to implement ordering guarantees and shared resource access | Single scheduler task with wake-and-filter | SRC-025, SRC-032 |

---

## Implementation Considerations

### Performance

- The global-interval wake-and-check approach adds at most `global_interval` seconds of latency before a due podcast is processed. For a default 1-hour interval, a podcast that becomes due 1 second after the last check will wait up to 59 minutes 59 seconds. This is acceptable for podcast sync (feeds rarely update more than hourly) (SRC-026).
- The SQL filter `WHERE (now - last_checked) >= interval` for 50 podcasts costs microseconds — negligible compared to network I/O for feed fetching (SRC-032).
- `prepare_cached` in the new `update_last_checked` avoids repeated SQL parsing on the hot path (SRC-028).

### Security

- No new security concerns from these changes. The `update_last_checked` method uses parameterized queries (no SQL injection risk). Proto changes do not expose new attack surface since UDS socket provides access control (SRC-023).

### Compatibility

- Adding `podcast_sync = 9` to the `StreamUpdates` oneof is backward-compatible in protobuf — old clients will ignore unknown fields (SRC-023).
- The Rust `UpdateEvents` enum addition requires updating the `From` impl and any match statements in the TUI, but this is a compile-time-caught change.
- `update_last_checked` is a new function — no breaking changes to existing API (SRC-028).
- The `tokio-util/time` feature is NOT needed for the recommended approach (Option A for ISS-008), avoiding dependency changes.

---

## Contradictions Found

| Topic | Position A | Position B | Assessment |
|-------|-----------|-----------|------------|
| DelayQueue vs global interval | DeepWiki (tokio-rs/tokio) recommends DelayQueue as "most efficient" for per-entity scheduling (SRC-025) | Miniflux uses global interval wake-and-check for thousands of feeds successfully (SRC-026) | Both are valid at different scales. For termusic's 5-50 podcasts, efficiency difference is immeasurable. Miniflux proves the simpler approach works at scale. The complexity cost of DelayQueue outweighs its efficiency benefit at this scale. Recommend global interval for now with DelayQueue as future optimization path. |

---

## Issues and Ambiguities

All three input issues (ISS-007, ISS-008, ISS-009) are now fully resolved. No new blocking issues identified.

- **ISS-010** (minor, non-blocking): The `UpdatePodcastSync` sub-message design assumes the TUI will want real-time progress updates during sync. If the TUI does not need sync progress (only completion notification), the sub-message could be simplified to just `PodcastSyncComplete`. This is a UX decision that can be deferred — start with the full sub-message and omit sending progress events until the TUI implements display for them.

---

## References

### Primary Sources (Official Documentation)

- SRC-027: tokio-util::time::DelayQueue — https://docs.rs/tokio-util/latest/tokio_util/time/struct.DelayQueue.html
- SRC-031: tokio::time::interval_at + MissedTickBehavior — https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html

### Secondary Sources (AI Documentation)

- SRC-025: DeepWiki: tokio-rs/tokio — DelayQueue vs global interval per-entity scheduling analysis
- SRC-026: DeepWiki: miniflux/v2 — POLLING_FREQUENCY global wake with next_check_at filtering architecture
- SRC-030: DeepWiki: grpc/grpc — oneof expansion patterns, nested vs flat, with channelz.proto examples

### Codebase Sources

- SRC-023: lib/proto/player.proto — StreamUpdates oneof (8 variants) and UpdatePlaylist sub-message pattern
- SRC-024: lib/src/player.rs — UpdatePlaylistEvents nested enum with From impl
- SRC-028: lib/src/podcast/db/podcast_db.rs — PodcastDBInsertable::update_podcast and delete_podcast patterns
- SRC-029: lib/src/podcast/db/migration.rs — user_version pragma migration infrastructure
- SRC-032: server/src/podcast_sync.rs — current sync_once and start_podcast_sync_task implementation
