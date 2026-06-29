# Requirements: Async Server Metadata Loading

- **Date**: 2026-06-26
- **Author**: super-dev:requirements-clarifier
- **Type**: enhancement
- **Priority**: critical
- **Status**: draft

---

## Executive Summary

The termusic server blocks on synchronous playlist metadata loading during startup, preventing the gRPC listener from accepting connections until all audio file tags are parsed. With large playlists (500+ tracks), lofty metadata reads take 1.5-10+ seconds even with the rayon parallelization from spec-03, causing the TUI client to display "Connecting is taking more time than expected..." or outright timeout. This requirement specifies decoupling server readiness from metadata loading by starting the gRPC server immediately with an empty/skeleton playlist, then loading metadata asynchronously in a dedicated thread pool, progressively populating the playlist as tracks are resolved.

## The Real Need (Root Cause Analysis)

### Surface Request

Make the server start accepting connections immediately by moving playlist metadata loading to a separate background thread pool, so the TUI does not timeout waiting for the server.

### 5 Whys Analysis

1. **Why**: The TUI shows "Connecting is taking more time than expected..." and may timeout on startup.
2. **Why**: The server does not start its gRPC listener until `Playlist::new_shared()` completes, which performs full metadata I/O for every track.
3. **Why**: `Playlist::new_shared()` at `server.rs:148-149` calls `load_apply()` synchronously — the entire server startup is sequentially gated on metadata resolution.
4. **Why**: The previous optimization (spec-03, rayon parallelization) sped up metadata reading itself but did not decouple it from server readiness — the server still blocks on `new_shared()` returning.
5. **Why**: The architectural assumption was that the playlist must be fully populated before the server can serve any requests — but the TUI only needs connectability at startup, and can receive playlist data progressively.

### Job to Be Done

When I launch termusic and the server needs to load a large playlist,
I want the server to become connectable within 1 second regardless of playlist size,
So I can start interacting with the TUI immediately while tracks load in the background.

- **Functional**: Decouple gRPC listener startup from metadata loading; load metadata asynchronously in a dedicated thread pool; populate the shared playlist progressively as tracks resolve
- **Emotional**: Eliminate the frustrating wait and uncertainty of "is it stuck or working?"
- **Social**: N/A (single-user terminal application)

## Stakeholders

- **End user (power user with large podcast/music libraries)**: Experiences startup delays and timeouts; primary beneficiary
- **TUI client**: Must handle receiving playlist data progressively (may initially see empty or partial playlist)
- **Termusic maintainers**: Must ensure correctness of concurrent playlist access during background loading

## Workflow Context

### Before (Current State)

1. User launches `termusic` (TUI), which spawns `termusic-server`
2. Server calls `Playlist::new_shared()` at `server.rs:148-149`
3. `load_apply()` → `load()` → opens `playlist.log`, reads all lines, calls `parallel_read_local_tracks()` via rayon
4. Even with rayon parallelization, reading 500+ files through lofty takes 1.5-10+ seconds
5. Only after ALL tracks are loaded does `start_service()` begin the gRPC listener
6. TUI's `wait_till_connected()` polls every 100ms; after 5s prints warning; after 30s hard timeouts
7. User sees: `"Connecting is taking more time than expected..."` followed by failure with server stderr showing lofty warnings

### After (Desired State)

1. User launches `termusic` (TUI), which spawns `termusic-server`
2. Server creates an empty `SharedPlaylist` (no metadata loading)
3. Server starts gRPC listener immediately (within ~100ms of process start)
4. TUI connects successfully within 1 second
5. Server spawns a background metadata loading task on a dedicated thread pool
6. As tracks are loaded (in batches or individually), they are inserted into the `SharedPlaylist`
7. The server sends `UpdatePlaylist` stream events to notify connected TUI clients of new tracks
8. TUI progressively displays tracks as they arrive via the existing update stream
9. Full playlist is available within the same wall-clock time as before (parallel loading), but user-perceived startup is near-instant

## Solution Options

### Option 1: Deferred Loading with Immediate Server Start (Recommended)

Start the gRPC server with an empty playlist. Spawn the existing `Playlist::load()` logic on a dedicated thread pool (reusing the `PLAYLIST_POOL` from `parallel_load.rs` or creating a new one). Once loading completes, atomically swap the empty playlist with the loaded one and send a full-playlist-update event to connected clients.

- **Pros**: Minimal changes to existing code; leverages existing parallel_load infrastructure; single atomic swap avoids partial-state complexity; TUI already handles `FullPlaylist` responses via `SelfReloadPlaylist`
- **Cons**: TUI sees an empty playlist for 1-3 seconds until loading completes; no progressive feedback during load
- **Effort**: low

### Option 2: Progressive Loading with Streaming Updates

Start the gRPC server with an empty playlist. Parse the playlist file to extract file paths first (fast, no I/O beyond reading the text file). Insert placeholder tracks (path only, no metadata). Then spawn background metadata resolution that updates tracks in-place and sends per-track or batched update events.

- **Pros**: TUI shows track filenames immediately; metadata (title/artist/duration) fills in progressively; feels more responsive
- **Cons**: Requires new "track metadata updated" event type; more changes to Track, Playlist, and protobuf; partial state management adds complexity; placeholder tracks need careful handling in the player loop
- **Effort**: high

### Option 3: Two-Phase Load (File List + Deferred Metadata)

Read only file paths from `playlist.log` (Phase 1, ~1ms). Create Track entries with path-only data (using existing `TrackData::new(path)`). Start the server immediately with these path-only tracks. Spawn background metadata enrichment that calls `parse_metadata_from_file` per track and updates the shared Track data.

- **Pros**: Immediate playlist visibility with track count and filenames; metadata enrichment is purely additive
- **Cons**: Track struct currently stores metadata at construction time — requires making title/artist/duration mutable or using an interior-mutability pattern; need to handle "track playing but metadata not yet loaded" edge case; player may try to play before metadata confirms file validity
- **Effort**: medium

## Acceptance Criteria

- **AC-01**: The server gRPC listener MUST accept connections within 1 second of process start, regardless of playlist size (0, 100, 500, 1000+ tracks).
- **AC-02**: Metadata loading MUST occur in a dedicated background thread pool, separate from the tokio runtime and the gRPC service threads.
- **AC-03**: After metadata loading completes, the shared playlist MUST contain the same tracks (in the same order) as the current synchronous implementation produces.
- **AC-04**: Connected TUI clients MUST receive notification (via the existing `UpdatePlaylist` stream or a new event) when the playlist becomes available after background loading.
- **AC-05**: If a `GetPlaylist` gRPC request arrives while metadata is still loading, the server MUST respond with the current state (empty or partially loaded) without blocking.
- **AC-06**: Playback MUST NOT start (even if `startup_state == Playing`) until the playlist has been fully loaded and the current track index is valid.
- **AC-07**: The playlist save-on-interval mechanism MUST NOT save an empty/partial playlist over a valid `playlist.log` file while background loading is in progress.
- **AC-08**: If metadata loading fails (e.g., file I/O error, corrupt playlist.log), the server MUST log the error and continue operating with whatever tracks loaded successfully (matching current graceful-degradation behavior).
- **AC-09**: The background metadata thread pool MUST be cleanly shut down on server `Quit`, without losing completed work or blocking the shutdown path for more than 1 second.
- **AC-10**: The TUI MUST remain responsive during the period between connection and playlist availability — it should display an appropriate state (empty playlist or loading indicator) rather than appearing frozen.

## Non-Functional Requirements

- **Performance** (critical): Server must accept connections within 1 second of process start. Metadata loading throughput must not regress compared to the current parallel implementation. Memory usage during loading should not exceed 2x the steady-state usage (no full duplicate playlist in memory during swap).
- **Security** (low): No new attack surface introduced — background loading operates on the same local files with the same permissions as the current implementation.
- **Accessibility** (low): N/A for terminal UI beyond what already exists.
- **Reliability** (high): Background loading failures must not crash the server. Partial success (some tracks loaded, some failed) must be handled gracefully. The server must never serve a stale or corrupt playlist state.
- **Observability** (medium): Log the start and completion of background metadata loading with timing information. Log the number of tracks successfully loaded vs failed. Use existing log levels (INFO for milestones, WARN for partial failures, DEBUG for per-track details).

## Open Questions

1. **Loading indicator in TUI**: Should the TUI show a "Loading playlist..." indicator while waiting for background metadata to complete, or is an empty playlist display sufficient? (Affects whether we need a new gRPC event type for "loading in progress" state.)
2. **Playback startup timing**: If `startup_state == Playing` is configured, should the server wait for full playlist load before starting playback, or attempt to play the first track as soon as it is resolved? (Recommendation: wait for full load to preserve current behavior and avoid edge cases.)
3. **Existing `PLAYLIST_POOL`**: Should the background loading reuse the existing `PLAYLIST_POOL` LazyLock thread pool from `parallel_load.rs`, or create a new dedicated pool? (Recommendation: reuse existing pool — it is already configured for playlist I/O workloads.)
4. **Atomic swap vs progressive**: Should the playlist be populated all-at-once (atomic swap after loading completes) or progressively (tracks inserted as they load)? (Recommendation: atomic swap for v1 — simpler correctness model, fewer edge cases with the player loop, and the TUI already handles full-playlist responses.)

## Recommendations

1. **Option 1 (Deferred Loading with Immediate Server Start)**: This is the recommended approach. It requires minimal code changes, reuses existing infrastructure, and the TUI already handles `GetPlaylist` returning an empty playlist gracefully (it just shows nothing). The atomic swap after loading completes triggers a full update event that the TUI processes through the existing `SelfReloadPlaylist` path.
2. **Add a "playlist loading" state flag**: Introduce a simple `AtomicBool` or similar flag on the `Playlist` (or alongside it) that indicates loading is in progress. The save-interval task checks this flag and skips saving until loading is complete. The `startup_state == Playing` logic checks this flag before attempting playback.
3. **Defer progressive loading to a follow-up**: Option 2/3 (progressive updates) adds significant complexity for marginal UX improvement (1-3 seconds of empty playlist vs. seeing filenames without metadata). If user feedback indicates this matters, it can be implemented as a separate enhancement on top of the deferred-loading foundation.

## Assumptions

- The TUI handles receiving an empty playlist from `GetPlaylist` without crashing (verified: `load_from_grpc` with 0 tracks should work as tracks vec is just empty)
- The existing `UpdatePlaylist` stream events (specifically the `PlaylistAddTrack` or a reload trigger) can be used to notify the TUI when loading completes
- The rayon `PLAYLIST_POOL` from `parallel_load.rs` is suitable for reuse in the background loading task (it already handles playlist I/O workloads)
- The `playlist.log` file is not modified by external processes during background loading
- A 1-3 second window of empty playlist display is acceptable UX (vs. not being able to connect at all)
