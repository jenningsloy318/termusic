  # Server Podcast Synchronization

  > Status: Draft · Owner: server backend · Last updated: 2026-06-23

  ## 1. Background

  termusic runs a playback **server** (`server` crate) separate from the TUI. The server owns the
  single play queue (`Playlist` / `SharedPlaylist`) and the podcast database (`player.db_podcast`).
  Today there is no automated way to keep subscribed podcasts current — new episodes are only
  fetched through manual TUI actions. When the TUI is closed, nothing refreshes podcasts.

  ## 2. Goal

  Add a **server-internal scheduled job** that periodically refreshes subscribed podcasts,
  downloads new episodes, and appends them (deduplicated) to the end of the play queue.
  It runs entirely inside the server process and is independent of the TUI.

  ## 3. Scope

  ### In scope
  - New `[synchronization]` config section in server settings, designed to be extended.
  - A tokio periodic task started by the server that, each tick:
    1. iterates all subscribed podcasts,
    2. fetches each RSS feed,
    3. detects new episodes not yet in `db_podcast` (dedup by GUID, fallback to enclosure URL),
    4. downloads each new episode's audio,
    5. appends the new tracks to the end of the play queue.
  - `refresh_on_startup`: run one pass immediately on server start, then enter the periodic cycle.
  - `enable`: master switch.

  ### Out of scope
  - Per-podcast schedules (global interval only for now).
  - Any TUI-visible controls or config UI.
  - New `PlayerCmd` variants (reuse `PlaylistAddTrack`).
  - Metadata-only / non-downloaded queue entries.

  ## 4. Configuration

  New section `synchronization` (sibling of the existing `podcast` section in
  `termusiclib/config/v2/server` settings):

  | Field               | Type                  | Default | Description                                                        |
  |---------------------|-----------------------|---------|--------------------------------------------------------------------|
  | `enable`            | bool                  | `true`  | Master switch for the periodic sync task.                          |
  | `interval`          | humantime duration    | `"1h"`  | Global refresh interval for all subscribed podcasts.               |
  | `refresh_on_startup`| bool                  | `true`  | Run one sync immediately on startup before entering the cycle.     |

  Designed for future fields (per-podcast interval, download/retention policy, max episodes).
  Versioning follows the existing `ServerConfigVersionedDefaulted` pattern.

  ## 5. Functional requirements

  - **FR-1 Lifecycle** — The task is spawned from `actual_main()` next to
    `start_playlist_save_interval()`, on the tokio `Handle`, sharing `service_cancel_token`.
    It lives only while the server runs and is independent of the TUI. Not spawned when
    `synchronization.enable == false`.
  - **FR-2 Periodic refresh** — Use `tokio::time::interval_at` + `select!` on
    `CancellationToken::cancelled`, mirroring `start_playlist_save_interval`. Each tick refreshes
    all subscribed podcasts.
  - **FR-3 Refresh-on-startup** — If `refresh_on_startup == true`, run one sync pass immediately at
    startup, then continue with the periodic `interval`.
  - **FR-4 Dedup** — An episode is "new" only if absent from `db_podcast` (keyed by GUID, falling
    back to enclosure URL). Episodes already in the DB or already in the queue are not re-added.
  - **FR-5 Download & enqueue** — Each new episode is downloaded to the local podcast directory
    (reuse existing path in `termusiclib::podcast` / the podcast DB). After download, append the
    track to the queue via `PlayerCmd::PlaylistAddTrack` over the existing `cmd_tx` channel; the
    player-loop handler calls `Playlist::add_tracks(&db_podcast)` and auto-starts playback if the
    queue was empty.
  - **FR-6 Error isolation** — A failure on one podcast (network / parse / download) is logged at
    `warn` and does not abort the pass or other podcasts.

  ## 6. Non-functional

  - Minimal change: reuse `start_playlist_save_interval` structure, existing podcast download
    code, and the existing `PlaylistAddTrack` command. No new abstractions.
  - Each commit compiles and passes existing tests; include tests for new config (de)serialization
    and defaults.
  - No new dependencies unless required.

  ## 7. Implementation outline (atomic commits)

  1. **Config** — add `Synchronization` section + versioning in `termusiclib/config/v2/server`.
  2. **Sync logic** — `sync_once(...)` (refresh feeds → dedup → download → enqueue) in the server
     crate, reusing `termusiclib::podcast` and `db_podcast`.
  3. **Task** — `start_podcast_sync_interval()` mirroring `start_playlist_save_interval`, plus the
     immediate startup pass when `refresh_on_startup`.
  4. **Wire** — call it from `actual_main()`, gated by `synchronization.enable`.
  5. **Tests** — config (de)serialization, defaults, and dedup behavior.

  ## 8. Open / future

  - Per-podcast interval overrides.
  - Download/retention policy (max episodes to keep, auto-delete after play).
  - TUI visibility (read-only status, manual trigger button).
