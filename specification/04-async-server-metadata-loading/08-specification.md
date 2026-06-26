# Technical Specification: Async Server Metadata Loading

- **Date**: 2026-06-26
- **Author**: super-dev:spec-writer
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md

---

## 1. Overview

This specification defines the technical design for decoupling the termusic server's gRPC listener startup from playlist metadata loading. Currently, the server blocks on synchronous metadata I/O (via `Playlist::new_shared()` at `server.rs:148-149`) before opening the gRPC listener, causing the TUI client to timeout when playlists contain 500+ tracks.

The solution creates an empty `SharedPlaylist`, starts the gRPC listener immediately (satisfying AC-01's sub-1-second connection requirement), then spawns a background metadata loading task on the existing `PLAYLIST_POOL` rayon thread pool. When loading completes, a four-step atomic completion handler populates the shared playlist and notifies connected clients. An `AtomicBool` loading-state flag prevents the save-interval task from overwriting `playlist.log` during loading and defers auto-play until the playlist is fully available.

This approach (Option 1 from the requirements: "Deferred Loading with Immediate Server Start") requires minimal changes to existing code, reuses established infrastructure (`PLAYLIST_POOL`, `CancellationToken`, broadcast channels), and preserves full behavioral compatibility with the current synchronous implementation once loading completes.

## 2. Architecture

### 2.1. Server Startup Sequence (Revised)

The server startup sequence changes from sequential-blocking to parallel-deferred:

**Current (blocking)**:
```
actual_main() -> get_config() -> Playlist::new_shared() [BLOCKS 1.5-10s] -> start_service() -> gRPC ready
```

**New (deferred)**:
```
actual_main() -> get_config() -> create_empty_shared_playlist() [<1ms] -> start_service() -> gRPC ready
                                        |
                                        +-> spawn_background_playlist_load() [runs concurrently]
                                                    |
                                                    +-> complete_background_load() [4-step atomic commit]
```

The gRPC listener becomes available within milliseconds of process start (AC-01). Background metadata loading executes concurrently on the dedicated `PLAYLIST_POOL` thread pool (AC-02), separate from the tokio runtime and gRPC service threads, without blocking any async runtime threads.

### 2.2. Shared State Additions

A new `AtomicBool` flag (`playlist_is_loading`) is introduced as a sibling to the `SharedPlaylist` in the server startup scope. This flag is:
- Set to `true` before spawning the background loading task
- Checked by `start_playlist_save_interval` to skip saves during loading (AC-07, SCENARIO-015, SCENARIO-016, SCENARIO-017)
- Checked by the auto-play logic in `player_loop` to defer playback (AC-06, SCENARIO-013, SCENARIO-014)
- Set to `false` with `Ordering::Release` by the completion handler after data is committed

The flag is NOT placed inside the `Playlist` struct (avoiding lock-scope changes). It lives as an `Arc<AtomicBool>` passed to subsystems that need it.

### 2.3. Background Task Pattern

The background loading task follows the established `start_podcast_sync_task` pattern (Handle + CancellationToken + select!) and satisfies AC-02 (dedicated background thread pool) and AC-09 (clean shutdown). The task:
1. Calls `Playlist::load()` on the `PLAYLIST_POOL` via `tokio::task::spawn_blocking`
2. Awaits the result using `tokio::select!` with the `CancellationToken` for clean shutdown support (AC-09)
3. On success, invokes the `complete_background_load()` four-step completion handler (AC-03, AC-04)
4. On failure, logs the error and clears the loading flag (server continues with empty playlist, AC-08)

### 2.4. Completion Handler Ordering Invariant

The completion handler (`complete_background_load`) executes four steps in strict sequence (per ISS-005 from deep research report):

1. **Write-lock swap**: Populate `SharedPlaylist` with loaded tracks and index. Dropping the write guard provides Release semantics.
2. **AtomicBool Release store**: Set `playlist_is_loading` to `false` with `Ordering::Release`. Any thread loading with `Acquire` is guaranteed to see step 1's data.
3. **Send PlaylistShuffled event via stream_tx**: Notify connected TUI clients that the playlist is now available.
4. **Send PlayerCmd::PlaylistLoadComplete via cmd_tx**: Trigger auto-play in `player_loop` if `startup_state == Playing`.

Steps 3 and 4 are independent channels. Their relative order is a preference (notify TUI before starting playback) rather than a correctness requirement.

### 2.5. PlayerCmd Extension

A new `PlayerCmd::PlaylistLoadComplete` enum variant is added to the `PlayerCmd` enum in `playback/src/lib.rs`. The `player_loop` handles this command by calling `player.resume_from_stopped()` if `startup_state == Playing` and the playlist is non-empty.

## 3. Data Models

### 3.1. PlaylistLoadingState (AtomicBool)

Represents whether background playlist metadata loading is in progress. Lives as an `Arc<AtomicBool>` shared between the background loading task, the save-interval task, and the player_loop.

```rust
/// Type alias for the shared loading-state flag.
/// `true` = loading in progress; `false` = loading complete (or not started for empty playlists).
pub type PlaylistLoadingFlag = Arc<AtomicBool>;
```

### 3.2. PlayerCmd::PlaylistLoadComplete (New Variant)

A new enum variant in the `PlayerCmd` enum signaling that background playlist loading has completed and auto-play may proceed.

```rust
#[derive(Clone, Debug)]
pub enum PlayerCmd {
    // ... existing variants ...

    /// Background playlist metadata loading has completed.
    /// If startup_state is Playing, player_loop should call resume_from_stopped().
    PlaylistLoadComplete,
}
```

### 3.3. BackgroundLoadResult (Return Type)

The result of `Playlist::load()` is already `Result<(usize, Vec<Track>)>` where:
- `usize` = saved current_track_index from `playlist.log`
- `Vec<Track>` = all loaded tracks in playlist order

No new data model is needed; the existing return type is used directly by the completion handler.

## 4. API Design

### 4.1. start_background_playlist_load (Internal Function)

Spawns the background metadata loading task following the established background-task pattern.

```rust
/// Spawn background metadata loading on PLAYLIST_POOL.
///
/// Creates a tokio task that:
/// 1. Calls Playlist::load() via spawn_blocking on the dedicated thread pool
/// 2. On success: invokes complete_background_load() to commit data and notify consumers
/// 3. On failure: logs the error and clears the loading flag
/// 4. Respects CancellationToken for clean shutdown
fn start_background_playlist_load(
    handle: Handle,
    cancel_token: CancellationToken,
    playlist: SharedPlaylist,
    playlist_is_loading: PlaylistLoadingFlag,
    stream_tx: StreamTX,
    cmd_tx: PlayerCmdSender,
    config: SharedServerSettings,
)
```

**Input Parameters:**
- `handle: Handle` - Tokio runtime handle for spawning async task
- `cancel_token: CancellationToken` - For clean shutdown (AC-09)
- `playlist: SharedPlaylist` - The shared playlist to populate after loading
- `playlist_is_loading: PlaylistLoadingFlag` - AtomicBool flag to clear after loading
- `stream_tx: StreamTX` - Broadcast sender for TUI notification events
- `cmd_tx: PlayerCmdSender` - Command sender for PlayerCmd::PlaylistLoadComplete
- `config: SharedServerSettings` - Server configuration (needed by Playlist::load for paths)

**Output:** None (fire-and-forget spawned task; errors are logged internally)

**Error Cases:**
- Playlist file unreadable: Log error at ERROR level, clear loading flag, server continues with empty playlist (AC-08, SCENARIO-019)
- Individual track I/O failure: Handled internally by Playlist::load() which skips failed tracks (AC-08, SCENARIO-020)
- CancellationToken fired during loading: Task exits cleanly within 1 second (AC-09, SCENARIO-021)

### 4.2. complete_background_load (Internal Function)

Executes the four-step atomic completion sequence with documented ordering invariant.

```rust
/// Complete the background playlist load by committing data and notifying consumers.
///
/// # Ordering Invariant
///
/// These steps MUST execute in this exact order:
/// 1. Write-lock swap: populate the shared playlist with loaded data.
///    Dropping the write guard provides Release semantics.
/// 2. AtomicBool store(false, Release): signals that loading is complete.
///    Any thread reading Acquire on this bool is guaranteed to see step 1's data.
/// 3. Send PlaylistShuffled event via stream_tx: notifies connected TUI clients.
///    Must come after step 1 so clients see populated data on GetPlaylist.
/// 4. Send PlayerCmd::PlaylistLoadComplete via cmd_tx: triggers auto-play if configured.
///    Must come after step 1 so player_loop finds tracks in the playlist.
fn complete_background_load(
    playlist: &SharedPlaylist,
    playlist_is_loading: &PlaylistLoadingFlag,
    stream_tx: &StreamTX,
    cmd_tx: &PlayerCmdSender,
    loaded_index: usize,
    loaded_tracks: Vec<Track>,
)
```

**Input Parameters:**
- `playlist: &SharedPlaylist` - Target for data swap (step 1)
- `playlist_is_loading: &PlaylistLoadingFlag` - Flag to clear (step 2)
- `stream_tx: &StreamTX` - For PlaylistShuffled notification (step 3)
- `cmd_tx: &PlayerCmdSender` - For PlaylistLoadComplete command (step 4)
- `loaded_index: usize` - The current_track_index from playlist.log
- `loaded_tracks: Vec<Track>` - All loaded tracks in order

**Output:** None

**Error Cases:**
- PlaylistShuffled event serialization failure: Logged at WARN level, continues to step 4
- cmd_tx send failure (player_loop already exited): Logged at WARN level, no further action

## 5. Implementation Details

### 5.1. Server Startup Modification (server.rs:148-149)

Replace the synchronous `Playlist::new_shared()` call with creating an empty shared playlist:

**Before:**
```rust
let playlist =
    Playlist::new_shared(&config, stream_tx.clone()).context("Failed to load playlist")?;
```

**After:**
```rust
let playlist: SharedPlaylist = Arc::new(RwLock::new(Playlist::new(&config, stream_tx.clone())));
let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
```

The `start_service()` call at line 175 proceeds immediately with the empty playlist. The `MusicPlayerService` already handles empty playlists correctly (returns 0-track responses to `GetPlaylist`). This satisfies AC-05: if a `GetPlaylist` gRPC request arrives while metadata is still loading, the server responds with the current state (empty playlist) without blocking. The TUI remains responsive during this period by displaying the empty state rather than freezing (AC-10, SCENARIO-023).

### 5.2. Background Loading Spawn (after start_service)

After `start_service()` returns and the gRPC listener is ready, spawn the background loading task:

```rust
start_background_playlist_load(
    tokio_handle.clone(),
    service_cancel_token.clone(),
    playlist.clone(),
    playlist_is_loading.clone(),
    stream_tx.clone(),
    cmd_tx.clone(),
    config.clone(),
);
```

### 5.3. Save Interval Protection (start_playlist_save_interval)

Modify `start_playlist_save_interval` to accept the `PlaylistLoadingFlag` and check it before saving:

```rust
fn start_playlist_save_interval(
    handle: Handle,
    cancel_token: CancellationToken,
    playlist: SharedPlaylist,
    playlist_is_loading: PlaylistLoadingFlag,
) {
    handle.spawn(async move {
        let mut timer = tokio::time::interval_at(
            Instant::now() + PLAYLIST_SAVE_INTERVAL,
            PLAYLIST_SAVE_INTERVAL,
        );
        loop {
            select! {
                _ = timer.tick() => {
                    // Skip save if background loading is still in progress (AC-07)
                    if playlist_is_loading.load(Ordering::Acquire) {
                        debug!("Skipping playlist save: background loading in progress");
                        continue;
                    }
                    match playlist.write().save_if_modified() {
                        Err(err) => warn!("Error saving playlist in interval: {err:#?}"),
                        Ok(true) => debug!("Saved playlist in interval"),
                        Ok(false) => ()
                    }
                },
                _ = cancel_token.cancelled() => {
                    break;
                }
            }
        }
    });
}
```

### 5.4. Auto-Play Deferral (player_loop)

Remove the immediate startup-state check at `server.rs:333-335`. Add a handler for the new `PlayerCmd::PlaylistLoadComplete` variant:

**Remove from player_loop entry:**
```rust
// REMOVED: This check ran before playlist was loaded
// if player.config.read().settings.player.startup_state == StartupState::Playing {
//     player.resume_from_stopped();
// }
```

**Add to player_loop match arms:**
```rust
PlayerCmd::PlaylistLoadComplete => {
    info!("Background playlist load complete");
    if player.config.read().settings.player.startup_state == StartupState::Playing {
        player.resume_from_stopped();
    }
}
```

### 5.5. Empty Playlist Edge Case

When `playlist.log` does not exist or is empty, `Playlist::load()` returns `Ok((0, Vec::new()))`. The background loading task detects this and still follows the full completion sequence (clearing the flag, sending notifications). The TUI handles an empty playlist correctly (no tracks displayed). No special case is needed (SCENARIO-002, SCENARIO-024).

### 5.6. Graceful Shutdown During Loading (AC-09)

The `CancellationToken` mechanism provides clean shutdown. When the server receives a Quit signal:
1. `service_cancel_token.cancel()` is called
2. The background loading task's `select!` arm on `cancel_token.cancelled()` fires
3. The `spawn_blocking` task may still be running on `PLAYLIST_POOL`; it completes its current file then the result is dropped (not committed)
4. The loading flag remains `true` (does not matter since server is shutting down)

The `PLAYLIST_POOL` threads are daemon threads (rayon default); they do not block process exit. Total shutdown delay is bounded by the time to complete one metadata read (typically <50ms per file).

## 6. Testing Strategy

The testing approach validates the architectural decoupling (server readiness independent of metadata loading), correctness preservation (loaded playlist matches synchronous behavior), and safety guards (save protection, playback deferral, clean shutdown).

### 6.1. Unit Tests

- Test `complete_background_load` populates playlist correctly with known tracks and index
- Test `complete_background_load` clears `playlist_is_loading` flag after data swap
- Test `PlayerCmd::PlaylistLoadComplete` handler calls `resume_from_stopped()` when startup_state is Playing
- Test `PlayerCmd::PlaylistLoadComplete` handler does NOT call `resume_from_stopped()` when startup_state is Stopped
- Test save-interval logic skips save when `playlist_is_loading` is true
- Test save-interval logic performs save when `playlist_is_loading` is false and playlist is modified

### 6.2. Integration Tests

- Test server accepts gRPC connection within 1 second with a 500-track playlist fixture (SCENARIO-001)
- Test server accepts gRPC connection immediately with an empty playlist (SCENARIO-002)
- Test loaded playlist contains tracks in the exact same order as synchronous loading (SCENARIO-006, SCENARIO-007)
- Test `GetPlaylist` returns empty state during loading and full state after completion (SCENARIO-010, SCENARIO-012)
- Test TUI client receives PlaylistShuffled event when loading completes (SCENARIO-008)
- Test `playlist.log` is NOT overwritten during background loading (SCENARIO-015)
- Test `playlist.log` IS saved normally after loading completes (SCENARIO-016)
- Test server continues with partial playlist when some tracks fail to load (SCENARIO-020)
- Test server continues with empty playlist when `playlist.log` is unreadable (SCENARIO-019)
- Test server shutdown completes within 1 second during active loading (SCENARIO-021)

### 6.3. E2E Tests

- Test full startup flow: launch server, connect TUI, observe empty playlist, wait for load complete event, verify full playlist displayed (SCENARIO-008, SCENARIO-023)
- Test auto-play: configure startup_state=Playing, launch server, verify playback begins only after loading completes (SCENARIO-013, SCENARIO-014)

### 6.4. BDD Scenario References

- **SCENARIO-001** -- integration -- Covered (server connection timing test with large playlist fixture)
- **SCENARIO-002** -- integration -- Covered (server connection timing test with empty playlist)
- **SCENARIO-003** -- integration -- Covered (server connection timing test with small playlist)
- **SCENARIO-004** -- integration -- Covered (concurrent client calls during loading)
- **SCENARIO-005** -- integration -- Covered (gRPC response latency during peak loading)
- **SCENARIO-006** -- integration -- Covered (playlist correctness comparison with synchronous baseline)
- **SCENARIO-007** -- integration -- Covered (ordering preservation with variable-latency tracks)
- **SCENARIO-008** -- integration -- Covered (client receives update event after load)
- **SCENARIO-009** -- integration -- Covered (client connecting after load gets full playlist)
- **SCENARIO-010** -- integration -- Covered (GetPlaylist returns empty during loading)
- **SCENARIO-011** -- integration -- Covered (concurrent GetPlaylist calls during loading)
- **SCENARIO-012** -- integration -- Covered (GetPlaylist returns full after loading)
- **SCENARIO-013** -- unit/integration -- Covered (auto-play deferred during loading)
- **SCENARIO-014** -- unit/integration -- Covered (auto-play triggered after load completes)
- **SCENARIO-015** -- unit/integration -- Covered (save skipped during loading)
- **SCENARIO-016** -- unit/integration -- Covered (save resumes after loading)
- **SCENARIO-017** -- integration -- Covered (all save paths blocked during loading)
- **SCENARIO-018** -- integration -- Covered (partial load with corrupt entries)
- **SCENARIO-019** -- integration -- Covered (unreadable playlist.log results in empty playlist)
- **SCENARIO-020** -- integration -- Covered (individual track failures do not halt loading)
- **SCENARIO-021** -- integration -- Covered (shutdown during loading completes within 1 second)
- **SCENARIO-022** -- integration -- Covered (shutdown after loading has no delay)
- **SCENARIO-023** -- e2e -- Covered (TUI responsive during loading period)
- **SCENARIO-024** -- integration -- Covered (missing playlist.log handled gracefully)
- **SCENARIO-025** -- integration -- Partial (memory constraint verified by code review; bounded by PLAYLIST_POOL size)
- **SCENARIO-026** -- integration -- Covered (immediate shutdown before loading starts)
- **SCENARIO-027** -- integration -- Covered (client reconnect during loading)

## 7. Non-Functional Requirements

### 7.1. Performance

- **Server readiness**: gRPC listener MUST accept connections within 1 second of process start (AC-01). Achieved by creating empty playlist (<1ms) and calling `start_service()` without waiting for metadata.
- **Loading throughput**: Background metadata loading reuses the existing `PLAYLIST_POOL` and `parallel_read_local_tracks()` from spec-03. No throughput regression because the same parallel algorithm executes; only the timing of when it starts changes.
- **Memory**: During loading, the loaded tracks exist in the `spawn_blocking` closure (on the thread pool stack) until the write-lock swap moves them into the `SharedPlaylist`. Peak memory is approximately 1x (steady-state playlist data) since the empty initial playlist has zero allocation and the loaded data replaces it via a Vec pointer swap. This satisfies the 2x steady-state constraint.

### 7.2. Reliability

- **Partial load handling**: If `Playlist::load()` returns tracks where some entries failed, the successfully loaded tracks are committed. The existing behavior (lofty skips corrupt files) is preserved (AC-08, SCENARIO-018, SCENARIO-020).
- **Total load failure**: If `Playlist::load()` returns an error, the server continues with an empty playlist. The error is logged at ERROR level (AC-08, SCENARIO-019).
- **Save protection**: The `playlist_is_loading` flag prevents overwriting `playlist.log` with empty/stale data during loading (AC-07, SCENARIO-015, SCENARIO-017).
- **Playback safety**: Auto-play is deferred until `PlaylistLoadComplete` is received, preventing attempts to play from an empty playlist (AC-06, SCENARIO-013).

### 7.3. Observability

- Log at INFO level: "Starting background playlist metadata loading"
- Log at INFO level: "Background playlist load complete: {track_count} tracks loaded in {elapsed_ms}ms" (includes timing information)
- Log at WARN level: "Background playlist load failed: {error}" (on total failure)
- Log at WARN level: individual track failures (handled by existing Playlist::load() logging)
- Log at DEBUG level: "Skipping playlist save: background loading in progress" (save-interval guard)

### 7.4. Security

No new attack surface is introduced. The background loading task operates on the same local files with the same filesystem permissions as the current synchronous implementation. No new network-facing code paths are added. The `AtomicBool` flag and channel operations are internal to the server process.

## 8. Risks and Mitigations

- **Risk**: TUI displays empty playlist for 1-3 seconds before tracks appear, which may confuse users expecting immediate content.
  - Likelihood: medium
  - Impact: low
  - Mitigation: The TUI already handles empty playlists gracefully (shows nothing in the track list). The PlaylistShuffled event triggers a full UI refresh when tracks arrive. A follow-up enhancement could add a "Loading..." indicator.

- **Risk**: Race condition between completion handler and concurrent playlist mutations (user adds track via gRPC while loading is in progress).
  - Likelihood: low (loading happens during first 1-3 seconds of server life; user interaction is unlikely during this window)
  - Impact: medium (user-added track could be overwritten by the atomic swap)
  - Mitigation: The write-lock in step 1 of the completion handler serializes access. If a user adds a track before loading completes, the swap will overwrite it. This is acceptable because: (1) the window is very short, (2) the user is unlikely to interact before the TUI even displays tracks, and (3) Option 1 (atomic swap) is explicitly chosen for simplicity over progressive loading.

- **Risk**: `PLAYLIST_POOL` thread pool exhaustion if multiple background tasks attempt to use it simultaneously.
  - Likelihood: low (only one background playlist load runs at a time; podcast sync does not use `PLAYLIST_POOL`)
  - Impact: low (rayon queues excess work; throughput degrades but correctness is maintained)
  - Mitigation: `PLAYLIST_POOL` is used exclusively for playlist I/O. No other subsystem shares it.

- **Risk**: Shutdown during loading leaves `playlist_is_loading` as `true` permanently.
  - Likelihood: low (shutdown during the 1-3 second loading window)
  - Impact: none (server is exiting; no code checks the flag after shutdown is initiated)
  - Mitigation: The `CancellationToken` cleanly aborts the background task. Post-shutdown, no code path observes the flag.

- **Risk**: `Playlist::load()` panics inside `spawn_blocking`, crashing the tokio worker thread.
  - Likelihood: very low (Playlist::load uses anyhow::Result throughout; no unsafe code)
  - Impact: medium (unhandled JoinError on the awaiting task)
  - Mitigation: The `select!` branch catching the spawn_blocking result handles `JoinError` (panic case) by logging the error and clearing the loading flag. Server continues with empty playlist.
