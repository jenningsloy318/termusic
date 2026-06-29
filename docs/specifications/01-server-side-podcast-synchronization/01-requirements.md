# Requirements: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Updated**: 2026-06-23
- **Author**: super-dev:requirements-clarifier
- **Type**: feature
- **Priority**: high
- **Status**: implemented

---

## Executive Summary

Add a server-internal scheduled job that periodically refreshes subscribed podcast RSS feeds, downloads new episodes (deduplicated by GUID or enclosure URL), and appends them to the play queue. This enables fully headless podcast synchronization independent of the TUI, keeping subscribed podcasts current even when no client is connected.

## The Real Need (Root Cause Analysis)

### Surface Request

Implement a periodic podcast synchronization task in the server process that refreshes feeds, downloads new episodes, and enqueues them automatically.

### 5 Whys Analysis

1. **Why**: Users want their podcast subscriptions to stay current without manual intervention.
2. **Why**: The current architecture requires TUI interaction to refresh feeds and download new episodes -- when the TUI is closed, nothing happens.
3. **Why**: The server process (which owns the playlist and podcast DB) has no autonomous refresh capability; it is purely reactive to client commands.
4. **Why**: The original design assumed a user would always be interacting via the TUI to trigger podcast updates.
5. **Why**: The server was designed as a playback engine first, with podcast management added later as a TUI-driven workflow rather than a server-autonomous concern.

### Job to Be Done

When I subscribe to podcasts and leave the termusic server running,
I want new episodes to be automatically fetched, downloaded, and queued,
So I can start listening to fresh content immediately when I connect a TUI client.

- **Functional**: Automatically detect, download, and enqueue new podcast episodes on a configurable schedule.
- **Emotional**: Confidence that subscriptions are always up-to-date without manual babysitting.
- **Social**: N/A (single-user application).

## Stakeholders

- **Server operator (self-hosted user)**: Primary beneficiary. Gains hands-free podcast updates.
- **TUI client user**: Sees new episodes appear in the play queue without triggering a refresh manually.
- **Upstream maintainers**: Must review code that adds a new server-side task; concerned about complexity, error handling, and config backward compatibility.

## Workflow Context

### Before (Current State)

1. User opens TUI, navigates to podcast section.
2. User manually triggers "Refresh" to fetch RSS feeds.
3. User selects episodes to download.
4. User adds downloaded episodes to the queue.
5. If TUI is closed, no podcasts are refreshed -- subscriptions go stale.

### After (Desired State)

1. Server starts and (if `refresh_on_startup` is true) immediately syncs all subscribed podcasts.
2. Every `interval` (default 1h), the server re-checks all RSS feeds.
3. New episodes not yet in `db_podcast` are downloaded automatically.
4. Downloaded episodes are appended to the end of the play queue via `PlaylistAddTrack`.
5. If the queue was empty, playback auto-starts (existing `PlaylistAddTrack` behavior).
6. All of this happens regardless of whether a TUI client is connected.

## Solution Options

### Option 1: Tokio periodic task mirroring start_playlist_save_interval (Recommended)

Spawn a new async task from `actual_main()` using the existing `service_cancel_token` and `Handle::spawn` pattern. The task uses `tokio::time::interval_at` + `select!` on `CancellationToken::cancelled`. Each tick calls a `sync_once(...)` function that iterates podcasts, fetches feeds, deduplicates, downloads, and enqueues.

- **Pros**: Minimal new abstraction. Mirrors proven pattern already in the codebase (`start_playlist_save_interval`). Uses existing `CancellationToken` for graceful shutdown. Reuses `termusiclib::podcast` feed fetching and download infrastructure.
- **Cons**: None significant. This is the natural extension point.
- **Effort**: medium

### Option 2: Separate background thread with sleep loop

Use `std::thread::spawn` with a `std::thread::sleep` loop (like `ticker_thread`).

- **Pros**: Simple to implement.
- **Cons**: Cannot cleanly cancel via `CancellationToken` (would need atomic flag). Cannot use async podcast fetch/download code without entering a runtime. Breaks from the async pattern the rest of the server uses for periodic tasks.
- **Effort**: medium

### Option 3: External cron/systemd timer triggering a CLI command

Add a `termusic-server sync` subcommand and let the OS scheduler handle periodicity.

- **Pros**: Zero in-process complexity. Leverages OS scheduling.
- **Cons**: Requires IPC to the running server or direct DB/playlist manipulation (risky). Users must configure OS-level scheduling separately. Does not fulfill the "server-internal" requirement.
- **Effort**: high

## Acceptance Criteria

- **AC-01**: A new `[synchronization]` section exists in the server config (`lib/src/config/v2/server/`) with fields `enable` (bool, default `true`), `interval` (humantime duration string, default `"1h"`), and `refresh_on_startup` (bool, default `true`). The section deserializes correctly with `#[serde(default)]` when absent from existing config files (backward compatible).
- **AC-02**: When `synchronization.enable == false`, no sync task is spawned at server startup. The server starts and operates identically to today.
- **AC-03**: When `synchronization.enable == true` and `refresh_on_startup == true`, one full sync pass executes immediately on server startup before entering the periodic cycle.
- **AC-04**: When `synchronization.enable == true`, a tokio task runs every `interval`, refreshing all subscribed podcasts by fetching their RSS feeds.
- **AC-05**: New episodes are identified by absence from `db_podcast`, keyed by GUID first with fallback to enclosure URL. Episodes already present in the database or already in the play queue are not re-added.
- **AC-06**: Each new episode is downloaded to the local podcast directory (reusing existing download infrastructure from `termusiclib::podcast`) and inserted into `db_podcast`.
- **AC-07**: After download, each new track is appended to the end of the play queue by sending `PlayerCmd::PlaylistAddTrack` over the existing `cmd_tx` channel. If the queue was previously empty, playback auto-starts (existing behavior of the `PlaylistAddTrack` handler).
- **AC-08**: A failure (network error, RSS parse error, download error) on one podcast is logged at `warn` level and does not abort the current sync pass or prevent other podcasts from being processed.
- **AC-09**: The sync task respects `service_cancel_token` -- it exits cleanly when the token is cancelled (server shutdown), using `select!` on `CancellationToken::cancelled()` mirroring `start_playlist_save_interval`.
- **AC-10**: Config serialization roundtrip tests exist verifying that the `[synchronization]` section serializes and deserializes correctly, including default values when the section is missing.
- **AC-11**: The sync task function signature follows the pattern of `start_playlist_save_interval(handle: Handle, cancel_token: CancellationToken, ...)` and is called from `actual_main()` adjacent to the existing `start_playlist_save_interval` call, gated by `synchronization.enable`.

## Non-Functional Requirements

- **Performance** (medium): The sync task must not block the player loop or gRPC service. All network I/O is async. Concurrent feed fetches are bounded by the existing `podcast.concurrent_downloads_max` setting. The interval timer must not drift (use `interval_at`, not repeated `sleep`).
- **Security** (low): No new attack surface. Feed URLs come from the user's own database. Downloads go to the configured podcast directory only.
- **Accessibility** (low): N/A -- this is a server-side background task with no UI component.
- **Reliability** (high): Error isolation per podcast is mandatory. A single malformed feed or unreachable host must never crash the server or prevent other podcasts from syncing. The task must survive transient network failures gracefully (log and continue).
- **Backward Compatibility** (high): Existing config files without a `[synchronization]` section must continue to parse without error, using defaults. No new `PlayerCmd` variants are introduced.
- **Minimal Dependencies** (medium): The `humantime` crate (or equivalent serde-compatible duration parser) is the only expected new dependency. No other new crates unless strictly necessary.

## Open Questions (Resolved)

- ~~Should there be a limit on the number of episodes downloaded per podcast per sync pass?~~ **Resolved**: No limit implemented for MVP. Documented as an accepted risk (S-03 in adversarial review). The `concurrent_downloads_max` setting provides natural throttling. A future `max_episodes_per_sync` config field can address this.
- ~~Should the sync task share the `db_podcast` instance from `GeneralPlayer`, or open its own connection?~~ **Resolved**: Opens its own `Database::new(db_path)` connection per sync pass. Dropped after each pass to minimize SQLite lock contention.
- ~~How should the `humantime` duration be integrated?~~ **Resolved**: Used `humantime-serde` v1.1 with `#[serde(with = "humantime_serde")]` for direct deserialization.
- ~~If `refresh_on_startup` runs concurrently with the player starting up, is there a race condition?~~ **Resolved**: No race condition. The `PlayerCmdSender` is an unbounded mpsc channel that serializes commands. Startup sync runs synchronously before the periodic loop begins.

## Recommendations

1. **Use Option 1 (tokio periodic task)**: It directly mirrors the proven `start_playlist_save_interval` pattern, reuses the existing cancellation infrastructure, and keeps all async podcast code on the tokio runtime where it belongs.
2. **Add `humantime-serde` as dependency**: This crate provides `#[serde(with = "humantime_serde")]` for clean Duration deserialization from strings like `"1h"`, `"30m"`, `"2h30m"`. It is well-maintained and avoids manual parsing.
3. **Open a separate `db_podcast` connection in the sync task**: The existing `DBPod::new()` opens a SQLite connection. Since the player loop thread owns its own instance, the sync task should open its own to avoid cross-thread sharing of a non-Send rusqlite Connection.
4. **Cap initial sync at recent episodes**: Consider a soft limit (e.g., 50 most recent episodes per podcast) on the first sync to avoid downloading entire back-catalogs. This can be a future config field but should have a sensible default.
5. **Implement in 5 atomic commits** as outlined in the spec: Config, sync logic, task, wiring, tests. Each commit must compile and pass tests independently.

## Assumptions

- The existing `termusiclib::podcast::get_feed_data` and `download_file` functions are reusable from an async context outside the TUI without modification.
- The `PlayerCmd::PlaylistAddTrack` handler in the player loop is safe to receive commands from any thread/task at any time (the channel is unbounded and already used by multiple senders).
- SQLite connections via `DBPod::new()` can be opened independently by multiple threads (SQLite supports concurrent readers; the sync task primarily reads and occasionally inserts).
- The existing `Playlist::add_tracks(&db_podcast)` correctly handles being called while playback is active (it already does, as the TUI can add tracks at any time).
- The `podcast.download_dir` configuration is already validated/created by the time the server starts (existing behavior).
