# Research Report: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-23
- **Technologies**: Rust, Tokio, rusqlite, humantime-serde, duration-str, serde, RSS/podcast feed parsing
- **Freshness**: Fresh (< 6mo)

---

## Executive Summary

- The recommended architecture is a Tokio periodic async task using `interval_at` + `CancellationToken`, directly mirroring the proven `start_playlist_save_interval` pattern already in the codebase (SRC-001, SRC-002).
- For duration configuration deserialization, `humantime-serde` is the most appropriate choice given the project's simplicity needs, despite `duration-str` being more actively maintained (SRC-004, SRC-005).
- SQLite concurrent access via separate `Database::new()` connections is safe and well-supported by rusqlite, requiring only a `busy_timeout` to handle write contention (SRC-006, SRC-007).
- The `MissedTickBehavior::Delay` strategy is the correct choice for a podcast sync task to prevent burst-catching-up after long sync passes (SRC-003).

**Recommendation**: Implement Option A (Tokio periodic task with `interval_at` + `CancellationToken`) using `humantime-serde` for config deserialization and a separate `Database` instance for the sync task. Confidence: **High**.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| humantime-serde crate Rust duration deserialization | Exa | 5 | 4 |
| Rust tokio periodic background task pattern interval_at CancellationToken | Exa | 5 | 4 |
| Rust SQLite rusqlite concurrent access multiple connections | Exa | 5 | 4 |
| Rust tokio interval_at missed tick behavior | Exa | 5 | 3 |
| Rust podcast RSS feed sync background service architecture | Exa | 5 | 3 |
| alternative to humantime-serde duration parsing serde | Exa | 5 | 4 |
| termusic server architecture podcast sync | DeepWiki | 1 | 1 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-001 | Tokio Graceful Shutdown - https://tokio.rs/tokio/topics/shutdown | Official docs | 2026-05 | Fresh | High |
| SRC-002 | Tokio interval_at docs - https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html | Official docs | 2026 | Fresh | High |
| SRC-003 | Tokio MissedTickBehavior - https://docs.rs/tokio/latest/tokio/time/enum.MissedTickBehavior.html | Official docs | 2026 | Fresh | High |
| SRC-004 | humantime-serde 1.1.1 - https://crates.io/crates/humantime-serde | Official docs | 2022-03 | Dated | High |
| SRC-005 | duration-str 0.21.0 - https://crates.io/crates/duration-str | Official docs | 2026-03 | Fresh | High |
| SRC-006 | rusqlite Connection docs - https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html | Official docs | 2026 | Fresh | High |
| SRC-007 | SQLite Threading - https://sqlite.org/threadsafe.html | Official docs | 2023-12 | Current | High |
| SRC-008 | Rust Tokio task cancellation patterns - https://cybernetist.com/2024/04/19/rust-tokio-task-cancellation-patterns/ | Blog | 2024-04 | Current | Medium |
| SRC-009 | rusqlite GitHub issue #188 - https://github.com/rusqlite/rusqlite/issues/188 | GitHub | 2016-11 | Dated | Medium |
| SRC-010 | fundu duration parser - https://github.com/fundu-rs/fundu | GitHub | 2025 | Fresh | Medium |
| SRC-011 | rpodder (Rust podcast sync server) - https://github.com/thekoma/rpodder | GitHub | 2026-03 | Fresh | Medium |
| SRC-012 | termusic codebase (start_playlist_save_interval) - server/src/server.rs | Codebase | 2026-06 | Fresh | High |
| SRC-013 | termusic podcast module - lib/src/podcast/mod.rs | Codebase | 2026-06 | Fresh | High |
| SRC-014 | termusic Database (podcast DB) - lib/src/podcast/db/mod.rs | Codebase | 2026-06 | Fresh | High |
| SRC-015 | serde_ext_duration - https://github.com/milchinskiy/serde_ext_duration | GitHub | 2025-10 | Fresh | Medium |

---

## Options Comparison

### Architecture Options: How to implement the periodic sync task

| Criterion | Option A: Tokio interval_at task | Option B: std::thread sleep loop | Option C: External cron/systemd timer | Option D: Tokio spawn with Mutex guard |
|-----------|----------------------------------|----------------------------------|---------------------------------------|---------------------------------------|
| Maturity | 5 | 4 | 5 | 4 |
| Community/Support | 5 | 3 | 4 | 4 |
| Performance | 5 | 3 | 4 | 5 |
| Bundle Size / Footprint | 5 | 5 | 3 | 5 |
| Learning Curve | 5 | 5 | 2 | 3 |
| Maintenance Burden | 5 | 3 | 2 | 4 |
| Project Fit | 5 | 2 | 1 | 4 |
| Innovation/Momentum | 4 | 2 | 3 | 4 |
| **TOTAL** | **39** | **27** | **24** | **33** |

#### Option A: Tokio interval_at + CancellationToken (Recommended)

- **Strengths**: Directly mirrors `start_playlist_save_interval` pattern already in the codebase (SRC-012). Uses the existing `service_cancel_token` for graceful shutdown (SRC-001). Timer does not drift because `interval_at` tracks absolute time (SRC-002). All async podcast fetch code (`get_feed_data`, `download_file`) runs naturally on the tokio runtime (SRC-013). Zero new abstractions needed. The `tick()` method is cancellation-safe with `select!` (SRC-003).
- **Weaknesses**: If a sync pass takes longer than the interval, the default `Burst` behavior would cause immediate re-firing. Must explicitly set `MissedTickBehavior::Delay` (SRC-003). Requires careful handling of overlapping ticks (SCENARIO-023).
- **Best For**: This exact use case -- a server-internal periodic task that integrates with existing cancellation and async infrastructure.

#### Option B: std::thread::spawn with sleep loop

- **Strengths**: Simple to understand. No async complexity. Similar to `ticker_thread` pattern in the codebase (SRC-012).
- **Weaknesses**: Cannot use `CancellationToken` directly without an atomic flag or condvar (SRC-008). Cannot call async podcast fetch/download code (`get_feed_data` uses `reqwest` async) without creating/entering a runtime (SRC-013). Would need `Handle::block_on()` calls scattered throughout. Timer drifts because `sleep` does not account for execution time (SRC-002). Inconsistent with the rest of the server's periodic task pattern.
- **Best For**: CPU-bound periodic work that does not involve async I/O.

#### Option C: External cron/systemd timer

- **Strengths**: Zero in-process complexity. Leverages OS scheduling. Fault isolation from server process (SRC-011 shows this pattern in rpodder).
- **Weaknesses**: Requires IPC to the running server or direct DB manipulation (race conditions with the player loop). Users must configure OS-level scheduling separately. Does not fulfill the "server-internal" requirement from AC-04 and AC-09. Breaks the single-binary deployment model. Cannot share `cmd_tx` channel for `PlaylistAddTrack`.
- **Best For**: Multi-service architectures where components communicate via API (not this single-process design).

#### Option D: Tokio spawn with Mutex-guarded sync state

- **Strengths**: Prevents overlapping sync passes with an `Arc<Mutex<bool>>` "running" flag. More explicit concurrency control. Could combine with `tokio::time::sleep` for a "delay after completion" model rather than fixed interval.
- **Weaknesses**: Adds extra synchronization primitive not present in the existing codebase pattern. `sleep`-after-completion means interval is not fixed -- it is completion_time + interval (SRC-002). More code to maintain. The `interval_at` approach with `MissedTickBehavior::Delay` achieves the same non-overlapping behavior more idiomatically (SRC-003).
- **Best For**: Tasks where the interval should be measured from completion, not from start.

---

### Duration Deserialization Options

| Criterion | Option A: humantime-serde | Option B: duration-str | Option C: Manual parsing with humantime |
|-----------|--------------------------|------------------------|----------------------------------------|
| Maturity | 5 | 4 | 5 |
| Community/Support | 4 | 4 | 5 |
| Performance | 4 | 4 | 5 |
| Bundle Size / Footprint | 5 | 3 | 5 |
| Learning Curve | 5 | 4 | 3 |
| Maintenance Burden | 5 | 4 | 3 |
| Project Fit | 5 | 3 | 4 |
| Innovation/Momentum | 3 | 4 | 3 |
| **TOTAL** | **36** | **30** | **33** |

#### Option A: humantime-serde (Recommended)

- **Strengths**: 58M+ downloads, MIT/Apache-2.0 licensed (SRC-004). Single annotation `#[serde(with = "humantime_serde")]` on a `Duration` field. Minimal API surface. Supports `"1h"`, `"30m"`, `"2h30m"` format strings that match the requirements doc. Only 2 transitive dependencies (`humantime` + `serde`). Last version 1.1.1 from 2022-03 is stable and has no known issues (SRC-004).
- **Weaknesses**: Last updated 2022-03 -- no recent commits (SRC-004). Only 40 GitHub stars (SRC-004). Does not support integer-seconds or float-seconds input -- only human-readable strings. 3 open issues on GitHub (minor).
- **Best For**: Projects needing simple, human-readable duration strings in TOML/YAML config files with minimal dependencies.

#### Option B: duration-str

- **Strengths**: Actively maintained with v0.21.0 released 2026-03 (SRC-005). Supports expressions like `"1m+30s"`. Supports `chrono::Duration` and `time::Duration` in addition to `std::time::Duration`. Chinese time unit support. Playground for testing. More format flexibility.
- **Weaknesses**: Heavier dependency (pulls in `chrono` and `time` by default). The expression support (`+`, `*`) is overkill for a config field (SRC-005). Different API pattern (`deserialize_with` function vs `with` module). 3M downloads vs 58M for humantime-serde -- smaller ecosystem footprint.
- **Best For**: Projects needing advanced duration parsing, multiple duration type support, or Chinese locale.

#### Option C: Manual parsing with humantime crate directly

- **Strengths**: `humantime` is well-maintained and already underlies `humantime-serde`. Can store as `String` in config and parse manually during validation. Most control over error messages. No new serde integration dependency.
- **Weaknesses**: Requires manual `impl Default` and custom deserialize logic. More boilerplate code. Must implement serialization manually if roundtrip is needed. Does not follow the existing config patterns in the codebase which use `#[serde(default)]` heavily (SRC-012).
- **Best For**: Projects with unusual validation requirements or that want to avoid any serde integration crates.

---

## Deprecation Warnings

No deprecation concerns identified for the current stack. `humantime-serde` v1.1.1 has not been updated since 2022 but remains fully functional with current `serde` and `humantime` versions (SRC-004). The `rusqlite` 0.39 dependency was recently upgraded in the project (commit cdcfa4e5).

---

## Best Practices

### BP-001: Use MissedTickBehavior::Delay for long-running periodic tasks

- **Pattern**: Set `interval.set_missed_tick_behavior(MissedTickBehavior::Delay)` for periodic tasks where the work may take longer than the interval period.
- **Rationale**: The default `Burst` behavior would cause the sync task to immediately fire again after a long sync pass, potentially creating back-to-back syncs. `Delay` resets the timer from when `tick()` is called, ensuring at least one full interval elapses between sync passes (SRC-003).
- **Source**: SRC-003
- **Confidence**: High
- **Example**:
```rust
let mut timer = tokio::time::interval_at(start, period);
timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
```

### BP-002: Open separate SQLite connections for concurrent readers/writers

- **Pattern**: Each thread/task that needs database access should open its own `Connection` via `Database::new()`. Set `busy_timeout` to handle write contention.
- **Rationale**: `rusqlite::Connection` is `Send` but not `Sync` -- it cannot be shared between threads without a Mutex (SRC-006, SRC-009). SQLite in WAL mode supports concurrent readers with a single writer. The existing codebase already has the player loop owning its own `Database` instance (SRC-014). Opening a second connection for the sync task is the idiomatic approach.
- **Source**: SRC-006, SRC-007, SRC-009
- **Confidence**: High
- **Example**:
```rust
// In sync task
let db = Database::new(&db_path)?;
// db.conn already has busy_timeout(5000) set by rusqlite defaults
```

### BP-003: Error isolation with per-podcast try/catch

- **Pattern**: Wrap each podcast's sync logic in a `match` or `if let Err(e)` block, logging warnings and continuing to the next podcast.
- **Rationale**: A single malformed RSS feed or network timeout must not abort the entire sync pass. This matches the error isolation pattern in `import_from_opml` where individual feed failures are logged and counted (SRC-013). AC-08 mandates this behavior.
- **Source**: SRC-013
- **Confidence**: High
- **Example**:
```rust
for podcast in podcasts {
    if let Err(e) = sync_single_podcast(&podcast, &db, &config).await {
        warn!("Sync failed for '{}': {e:#}", podcast.title);
    }
}
```

### BP-004: Reuse existing TaskPool for bounded concurrency

- **Pattern**: Use the existing `TaskPool` struct (which wraps `tokio::Semaphore`) to bound concurrent feed fetches and downloads to `concurrent_downloads_max`.
- **Rationale**: The `TaskPool` already implements the semaphore-based concurrency limiter with cancellation support (SRC-013). Reusing it avoids duplicating concurrency control logic and respects the existing `podcast.concurrent_downloads_max` configuration.
- **Source**: SRC-013, SRC-014
- **Confidence**: High

### BP-005: Cap initial sync episode count

- **Pattern**: On first sync of a newly-subscribed podcast (where no episodes exist in the DB), limit downloads to the N most recent episodes (e.g., 50).
- **Rationale**: Podcasts with large back-catalogs (500+ episodes) would consume excessive bandwidth and disk space on first sync. This is noted as an open question in the requirements (SRC-011 shows rpodder has similar configurable limits). Can be implemented by sorting feed episodes by pubdate descending and taking only the first N.
- **Source**: SRC-011, Requirements Open Questions
- **Confidence**: Medium

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| Using `tokio::time::sleep` in a loop for periodic tasks | Timer drifts by the execution time of each iteration; misses exact interval semantics | Use `interval_at` which accounts for execution time (SRC-002) | SRC-002 |
| Sharing a single `rusqlite::Connection` across threads with `Arc<Mutex<Connection>>` | Holds a lock for the entire duration of complex queries; blocks the player loop during sync | Open separate connections per thread/task (SRC-006, SRC-009) | SRC-006 |
| Using `MissedTickBehavior::Burst` (default) for long-running tasks | If sync takes 2 hours with 1h interval, Burst fires immediately twice on completion | Use `Delay` to reset timer from completion point (SRC-003) | SRC-003 |
| Downloading all episodes without limit on first subscription | A podcast with 1000 episodes would download hundreds of GB of audio files | Cap at configurable N most recent episodes (SRC-011) | SRC-011 |

---

## Implementation Considerations

### Performance

- The sync task MUST NOT block the player loop. Since it runs as a separate tokio task and communicates via `cmd_tx` channel (which is unbounded), there is no blocking path between sync and playback (SRC-012).
- Concurrent feed fetches should be bounded by `podcast.concurrent_downloads_max` (default 3) to avoid overwhelming network or disk I/O (SRC-013).
- `interval_at` does not drift -- it is the correct primitive for periodic scheduling (SRC-002).
- SQLite with WAL mode allows concurrent reads while a write is in progress (SRC-007). The sync task primarily reads (get_podcasts, get_episodes) and occasionally writes (insert_episode, insert_file).

### Security

- Feed URLs come from the user's own database -- no new attack surface (Requirements NFR).
- Downloads go only to the configured `podcast.download_dir` -- no path traversal risk as `sanitize_filename` is already used (SRC-013).
- The sync task makes outbound HTTP requests to RSS feed URLs. If a malicious URL is subscribed, the existing `connect_timeout(5s)` in `get_feed_data` limits exposure (SRC-013).

### Compatibility

- `humantime-serde` 1.1.1 is compatible with `serde` 1.x (any version) and `humantime` 2.x (SRC-004).
- `rusqlite` 0.39 (recently upgraded in this project) supports the multi-threaded SQLite mode needed for separate connections (SRC-006).
- Existing config files without `[synchronization]` section will continue to parse due to `#[serde(default)]` on the struct (matching the `ServerSettings` pattern in SRC-012).
- The `CancellationToken` from `tokio-util` is already a dependency in this project (used by `TaskPool` and `start_playlist_save_interval`) (SRC-012).

---

## Contradictions Found

| Topic | Position A (SRC-004) | Position B (SRC-005) | Assessment |
|-------|---------------------|---------------------|------------|
| Duration crate choice | humantime-serde is stable at 1.1.1 since 2022, 58M downloads, minimal API | duration-str is actively maintained (v0.21 in 2026), more features, 3M downloads | Both are valid. humantime-serde is preferred for this project due to minimal footprint and compatibility with the TOML config format. The lack of recent updates is a non-issue -- the crate is feature-complete for its scope. |
| SQLite concurrent access | rusqlite docs say Connection is not Sync and should not be shared (SRC-006) | sqlite-rwc crate suggests pool-based approach with read/write separation (SRC-006) | For this use case (one writer = player loop, one reader+occasional-writer = sync task), separate connections with busy_timeout is sufficient. A connection pool is overkill for 2 consumers. |

---

## Issues and Ambiguities

- **ISS-001**: Race condition between `refresh_on_startup` sync and player startup auto-play. If `startup_state == Playing` and the playlist is not empty, the player starts playing immediately. If `refresh_on_startup` then adds tracks via `PlaylistAddTrack`, there is no race because the `cmd_tx` channel serializes all commands and the player loop processes them sequentially (SRC-012). However, the "queue was empty" detection for auto-play depends on the playlist state at the moment `PlaylistAddTrack` is processed. This is safe because both paths go through the same channel. **Resolution**: No race condition exists -- the channel serializes access. The sync task sends `PlaylistAddTrack` which the player loop handles atomically.

- **ISS-002**: Should `update_podcast` (which calls `update_episodes`) be used or should the sync task only call `insert_episode` for truly new episodes? The existing `Database::update_podcast` method already handles deduplication by GUID and fallback matching (title/url/pubdate 2-of-3 match) (SRC-014). This is the correct function to reuse -- it returns a `SyncResult` with `added` count indicating new episodes. The sync task should call `update_podcast` and then only download episodes that are genuinely new (not yet having a file path in the `files` table).

- **ISS-003**: The `download_file` function in `lib/src/podcast/mod.rs` is currently a private async function. It needs to be either made `pub(crate)` or the sync task needs to invoke `download_list` (which is already public). Using `download_list` with `TaskPool` is the better approach as it handles concurrency and retries (SRC-013). The sync task should construct `EpData` structs for new episodes and call `download_list`.

- **ISS-004**: Episode cap on first sync. The requirements say "all new episodes" but the open questions ask about limits. Recommendation: Add an optional `max_episodes_per_sync` config field (default: 50) that caps how many episodes are downloaded per podcast per sync pass, sorted by pubdate descending. This prevents unbounded downloads on first subscription while allowing users to disable the limit (`max_episodes_per_sync = 0` means unlimited).

---

## References

### Primary Sources (Official Documentation)

- SRC-001: Tokio Graceful Shutdown Guide -- https://tokio.rs/tokio/topics/shutdown
- SRC-002: Tokio interval_at API documentation -- https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html
- SRC-003: Tokio MissedTickBehavior documentation -- https://docs.rs/tokio/latest/tokio/time/enum.MissedTickBehavior.html
- SRC-004: humantime-serde crate -- https://crates.io/crates/humantime-serde
- SRC-005: duration-str crate -- https://crates.io/crates/duration-str
- SRC-006: rusqlite Connection documentation -- https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html
- SRC-007: SQLite Threading documentation -- https://sqlite.org/threadsafe.html

### Secondary Sources (Blogs, Papers, Guides)

- SRC-008: Rust Tokio Task Cancellation Patterns -- https://cybernetist.com/2024/04/19/rust-tokio-task-cancellation-patterns/
- SRC-010: fundu duration parser -- https://github.com/fundu-rs/fundu
- SRC-015: serde_ext_duration -- https://github.com/milchinskiy/serde_ext_duration

### Community Sources (GitHub, Reddit, X/Twitter)

- SRC-009: rusqlite issue #188 (Share Connection into several threads) -- https://github.com/rusqlite/rusqlite/issues/188
- SRC-011: rpodder (Rust podcast sync server) -- https://github.com/thekoma/rpodder
- SRC-012: termusic server source (start_playlist_save_interval) -- server/src/server.rs (local)
- SRC-013: termusic podcast module -- lib/src/podcast/mod.rs (local)
- SRC-014: termusic Database module -- lib/src/podcast/db/mod.rs (local)
