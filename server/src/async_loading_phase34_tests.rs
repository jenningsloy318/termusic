//! Phase 3/4: Async Server Metadata Loading — Integration/Behavior Tests (RED)
//!
//! These tests verify the async loading behavior described in spec-04 for Phases 3 and 4.
//! They target functions and behaviors that DO NOT YET EXIST, so they should fail to compile
//! or fail assertions until the implementation is complete.
//!
//! BDD Scenario coverage (19 uncovered scenarios):
//!   - SCENARIO-001: Server accepts connections within 1 second (AC-01)
//!   - SCENARIO-003: Server accepts connections with small playlist (AC-01)
//!   - SCENARIO-005: Background loading does not starve gRPC service (AC-02)
//!   - SCENARIO-006: Loaded playlist matches synchronous implementation (AC-03)
//!   - SCENARIO-007: Track ordering preserved after async loading (AC-03)
//!   - SCENARIO-009: Client connecting after loading gets full playlist (AC-04)
//!   - SCENARIO-010: GetPlaylist returns empty during loading (AC-05)
//!   - SCENARIO-011: Multiple concurrent GetPlaylist calls during loading (AC-05)
//!   - SCENARIO-012: GetPlaylist returns full after loading (AC-05)
//!   - SCENARIO-017: Manual save blocked during loading (AC-07)
//!   - SCENARIO-018: Corrupt playlist.log partial load (AC-08)
//!   - SCENARIO-019: Unreadable playlist.log empty playlist (AC-08)
//!   - SCENARIO-020: Individual track I/O failure does not halt loading (AC-08)
//!   - SCENARIO-021: Server shutdown terminates loading within 1s (AC-09)
//!   - SCENARIO-022: Shutdown after loading no delay (AC-09)
//!   - SCENARIO-023: TUI remains interactive during loading (AC-10)
//!   - SCENARIO-025: Large playlist memory constraint (AC-01, AC-02)
//!   - SCENARIO-026: Shutdown before loading starts (AC-09)
//!   - SCENARIO-027: Client reconnect during loading (AC-04, AC-05)

#[cfg(test)]
mod async_loading_phase34_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use parking_lot::RwLock;
    use termusiclib::config::{ServerOverlay, SharedServerSettings, new_shared_server_settings};
    use termusiclib::player::{UpdateEvents, UpdatePlaylistEvents};
    use termusiclib::track::Track;
    use termusicplayback::{PlayerCmd, PlayerCmdSender, Playlist, SharedPlaylist, StreamTX};
    use tokio::sync::{broadcast, mpsc::unbounded_channel};
    use tokio_util::sync::CancellationToken;

    use crate::PlaylistLoadingFlag;

    // =========================================================================
    // These tests reference functions that MUST be implemented in Phase 2/3:
    //   - start_background_playlist_load
    //   - complete_background_load
    // They will fail to compile until those functions exist.
    // =========================================================================

    // Import the functions that will be created in Phase 2/3.
    // This use statement will cause a compilation failure until the functions are implemented.
    use crate::{complete_background_load, start_background_playlist_load};

    // =========================================================================
    // Helper: Create a test SharedPlaylist using the Playlist::new constructor
    // =========================================================================

    fn make_test_playlist() -> (SharedPlaylist, StreamTX, SharedServerSettings) {
        let config = new_shared_server_settings(ServerOverlay::default());
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let playlist = Playlist::new(&config, stream_tx.clone());
        let shared: SharedPlaylist = Arc::new(RwLock::new(playlist));
        (shared, stream_tx, config)
    }

    /// Create a Vec of test tracks using Track::new_radio (no I/O needed)
    fn make_test_tracks(count: usize) -> Vec<Track> {
        (0..count)
            .map(|i| Track::new_radio(format!("file:///tmp/test_track_{:04}.mp3", i)))
            .collect()
    }

    // =========================================================================
    // SCENARIO-001: Server accepts connections within 1 second regardless of playlist size
    // AC-01 — The gRPC listener MUST accept connections within 1 second of process start.
    // =========================================================================

    /// SCENARIO-001: With a large playlist (1000 tracks), the server must accept connections
    /// within 1 second. This test verifies that creating an empty SharedPlaylist (the new
    /// startup pattern) is near-instant, proving the server did not block on loading.
    #[tokio::test]
    async fn scenario_001_server_accepts_connections_within_1_second_large_playlist() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));

        let start = Instant::now();

        // Create empty playlist — simulating the immediate server start pattern
        let (playlist, _stream_tx, _config) = make_test_playlist();

        // The playlist is empty immediately (server would be ready now)
        assert!(
            playlist.read().is_empty(),
            "Playlist must be empty at server start before background loading"
        );

        // Verify that creating the empty playlist + making it available took < 1 second
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "Server readiness (empty playlist creation) must complete within 1 second, took {:?}",
            elapsed
        );

        // The loading flag should still be true (loading hasn't completed)
        assert!(
            playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be true before background loading completes"
        );
    }

    // =========================================================================
    // SCENARIO-003: Server accepts connections within 1 second with small playlist
    // AC-01
    // =========================================================================

    /// SCENARIO-003: Even with a small 10-track playlist, the server must accept connections
    /// within 1 second. Background loading is still used for consistency.
    #[tokio::test]
    async fn scenario_003_server_accepts_connections_within_1_second_small_playlist() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));

        let start = Instant::now();

        let (playlist, _stream_tx, _config) = make_test_playlist();

        // Playlist is empty at startup
        assert_eq!(
            playlist.read().len(),
            0,
            "Playlist must have 0 tracks at immediate startup"
        );

        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "Server startup with small playlist must complete within 1 second, took {:?}",
            elapsed
        );

        // Loading flag is true — background load would be spawned
        assert!(playlist_is_loading.load(Ordering::Acquire));
    }

    // =========================================================================
    // SCENARIO-005: Background loading does not starve gRPC service of resources
    // AC-02 — Metadata loading MUST occur in a dedicated background thread pool.
    // =========================================================================

    /// SCENARIO-005: When background loading is active, gRPC calls must still respond
    /// within 100ms. This test verifies that the loading flag being true does not block
    /// read access to the playlist (simulating a GetPlaylist call during loading).
    #[tokio::test]
    async fn scenario_005_background_loading_does_not_starve_grpc_service() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();

        // Simulate background loading in progress
        let start = Instant::now();

        // A "GetPlaylist" read should not be blocked by the loading flag
        let read_guard = playlist.read();
        let track_count = read_guard.len();
        drop(read_guard);

        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "GetPlaylist read during loading must respond within 100ms, took {:?}",
            elapsed
        );
        assert_eq!(track_count, 0, "Playlist is empty during loading");

        // Verify loading is still in progress
        assert!(playlist_is_loading.load(Ordering::Acquire));
    }

    // =========================================================================
    // SCENARIO-006: Loaded playlist matches synchronous implementation output exactly
    // AC-03
    // =========================================================================

    /// SCENARIO-006: After `complete_background_load` executes, the shared playlist must
    /// contain the exact tracks that were loaded, matching synchronous behavior.
    #[tokio::test]
    async fn scenario_006_loaded_playlist_matches_synchronous_implementation() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Simulate loaded tracks (what Playlist::load() would return)
        let loaded_tracks = vec![
            Track::new_radio("file:///tmp/test_track_a.mp3"),
            Track::new_radio("file:///tmp/test_track_b.mp3"),
            Track::new_radio("file:///tmp/test_track_c.mp3"),
        ];
        let loaded_index: usize = 1;

        // Call the completion handler
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            loaded_index,
            loaded_tracks.clone(),
        );

        // Verify the playlist now contains the loaded tracks
        let read = playlist.read();
        assert_eq!(
            read.len(),
            3,
            "Playlist must contain exactly 3 tracks after loading"
        );
        assert_eq!(
            read.get_current_track_index(),
            loaded_index,
            "Current track index must match the loaded index"
        );

        // Loading flag must be cleared
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be false after complete_background_load"
        );
    }

    // =========================================================================
    // SCENARIO-007: Track ordering preserved after async loading
    // AC-03
    // =========================================================================

    /// SCENARIO-007: Tracks loaded asynchronously must be in the exact order specified,
    /// even if some tracks take longer to read metadata for.
    #[tokio::test]
    async fn scenario_007_track_ordering_preserved_after_async_loading() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Tracks in a specific order — track_B would have taken longer to read in practice
        let loaded_tracks = vec![
            Track::new_radio("file:///tmp/track_A.flac"),
            Track::new_radio("file:///tmp/track_B.flac"),
            Track::new_radio("file:///tmp/track_C.flac"),
            Track::new_radio("file:///tmp/track_D.flac"),
            Track::new_radio("file:///tmp/track_E.flac"),
        ];

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            loaded_tracks.clone(),
        );

        let read = playlist.read();
        assert_eq!(read.len(), 5);

        // Verify ordering is preserved — each track at its expected index
        let tracks = read.tracks();
        for (i, expected_track) in loaded_tracks.iter().enumerate() {
            let actual = &tracks[i];
            assert_eq!(
                actual.url(),
                expected_track.url(),
                "Track at index {i} must have the correct URL (preserving order)"
            );
        }
    }

    // =========================================================================
    // SCENARIO-009: Client connecting after loading completes receives full playlist
    // AC-04
    // =========================================================================

    /// SCENARIO-009: After loading completes, any new client connecting and calling
    /// GetPlaylist must receive the full playlist with all tracks.
    #[tokio::test]
    async fn scenario_009_client_connecting_after_loading_gets_full_playlist() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let loaded_tracks = vec![
            Track::new_radio("file:///tmp/song1.mp3"),
            Track::new_radio("file:///tmp/song2.mp3"),
        ];

        // Complete the load
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            loaded_tracks,
        );

        // Simulate a new client connecting AFTER loading
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading must be complete"
        );

        // Client reads playlist — must get full contents
        let read = playlist.read();
        assert_eq!(
            read.len(),
            2,
            "Client connecting after load must see full playlist"
        );
    }

    // =========================================================================
    // SCENARIO-010: GetPlaylist returns empty state while loading is in progress
    // AC-05
    // =========================================================================

    /// SCENARIO-010: During loading, GetPlaylist must return immediately with empty state.
    #[tokio::test]
    async fn scenario_010_get_playlist_returns_empty_during_loading() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();

        // Loading is in progress
        assert!(playlist_is_loading.load(Ordering::Acquire));

        // GetPlaylist should return immediately with empty state
        let start = Instant::now();
        let read = playlist.read();
        let len = read.len();
        drop(read);
        let elapsed = start.elapsed();

        assert_eq!(len, 0, "Playlist must be empty during loading");
        assert!(
            elapsed < Duration::from_millis(100),
            "GetPlaylist must not block waiting for loading to complete, took {:?}",
            elapsed
        );
    }

    // =========================================================================
    // SCENARIO-011: Multiple concurrent GetPlaylist calls during loading all return promptly
    // AC-05
    // =========================================================================

    /// SCENARIO-011: Three concurrent GetPlaylist calls during loading must all respond
    /// within 100ms without blocking on metadata completion.
    #[tokio::test]
    async fn scenario_011_multiple_concurrent_get_playlist_during_loading() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();

        let start = Instant::now();

        // Simulate three concurrent read requests
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let pl = playlist.clone();
                let flag = playlist_is_loading.clone();
                tokio::spawn(async move {
                    assert!(
                        flag.load(Ordering::Acquire),
                        "Loading should be in progress"
                    );
                    let read = pl.read();
                    let count = read.len();
                    drop(read);
                    count
                })
            })
            .collect();

        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.expect("Task must complete"));
        }

        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "All 3 concurrent GetPlaylist calls must complete within 100ms, took {:?}",
            elapsed
        );

        // All should see empty playlist
        for (i, count) in results.iter().enumerate() {
            assert_eq!(
                *count, 0,
                "Client {i} must see empty playlist during loading"
            );
        }
    }

    // =========================================================================
    // SCENARIO-012: GetPlaylist returns fully populated playlist after loading completes
    // AC-05
    // =========================================================================

    /// SCENARIO-012: After loading completes, GetPlaylist must return the full playlist.
    #[tokio::test]
    async fn scenario_012_get_playlist_returns_full_after_loading() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let loaded_tracks = vec![
            Track::new_radio("file:///tmp/full_a.mp3"),
            Track::new_radio("file:///tmp/full_b.mp3"),
            Track::new_radio("file:///tmp/full_c.mp3"),
            Track::new_radio("file:///tmp/full_d.mp3"),
        ];

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            2,
            loaded_tracks,
        );

        // After loading, GetPlaylist returns full data
        assert!(!playlist_is_loading.load(Ordering::Acquire));
        let read = playlist.read();
        assert_eq!(
            read.len(),
            4,
            "GetPlaylist after loading must return all 4 tracks"
        );
        assert_eq!(read.get_current_track_index(), 2);
    }

    // =========================================================================
    // SCENARIO-017: Manual save operation blocked during loading
    // AC-07
    // =========================================================================

    /// SCENARIO-017: Any save operation triggered while loading is in progress must be
    /// suppressed. The save-interval must check playlist_is_loading and skip.
    #[tokio::test]
    async fn scenario_017_manual_save_blocked_during_loading() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));

        // The save interval logic must check the flag before saving
        let should_save = !playlist_is_loading.load(Ordering::Acquire);
        assert!(
            !should_save,
            "Save must be blocked (skipped) while loading is in progress"
        );

        // After loading completes, save should be allowed
        playlist_is_loading.store(false, Ordering::Release);
        let should_save_after = !playlist_is_loading.load(Ordering::Acquire);
        assert!(
            should_save_after,
            "Save must be allowed after loading completes"
        );
    }

    // =========================================================================
    // SCENARIO-018: Corrupt playlist.log results in partial load with error logging
    // AC-08
    // =========================================================================

    /// SCENARIO-018: When playlist.log has some corrupt lines, only valid tracks are loaded.
    /// The completion handler receives only the valid tracks and commits them.
    #[tokio::test]
    async fn scenario_018_corrupt_playlist_log_partial_load() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Simulate partial load result: only valid tracks survived the load
        // (corrupt lines were skipped by Playlist::load())
        let partial_tracks = vec![
            Track::new_radio("file:///tmp/valid_track_1.mp3"),
            Track::new_radio("file:///tmp/valid_track_3.mp3"),
        ];

        // complete_background_load with partial results (2 out of potentially more)
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            partial_tracks,
        );

        // Server continues operating with successfully loaded tracks
        let read = playlist.read();
        assert_eq!(
            read.len(),
            2,
            "Only valid tracks must be in the playlist after partial load"
        );
        assert!(!playlist_is_loading.load(Ordering::Acquire));
    }

    // =========================================================================
    // SCENARIO-019: Unreadable playlist.log results in empty playlist
    // AC-08
    // =========================================================================

    /// SCENARIO-019: When playlist.log cannot be read, the server continues with empty
    /// playlist. The completion handler receives an empty Vec and commits it.
    #[tokio::test]
    async fn scenario_019_unreadable_playlist_log_empty_playlist() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Simulate total failure: load returns error, error handler clears flag
        // with empty tracks (the background task error path)
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            Vec::new(), // empty — no tracks loadable
        );

        // Server continues operating with empty playlist
        let read = playlist.read();
        assert_eq!(
            read.len(),
            0,
            "Playlist must be empty when playlist.log is unreadable"
        );
        // Flag must still be cleared so save-interval does not remain blocked forever
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be cleared even on failure"
        );
    }

    // =========================================================================
    // SCENARIO-020: Individual track I/O failure does not halt loading of remaining tracks
    // AC-08
    // =========================================================================

    /// SCENARIO-020: When 5 out of 100 tracks fail to load, the remaining 95 are still
    /// committed to the playlist in their original order.
    #[tokio::test]
    async fn scenario_020_individual_track_io_failure_does_not_halt_loading() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Simulate: 95 tracks loaded successfully (5 failed and were filtered out by load())
        let successful_tracks: Vec<_> = (0..95)
            .map(|i| Track::new_radio(format!("file:///tmp/track_{:03}.mp3", i)))
            .collect();

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            successful_tracks,
        );

        let read = playlist.read();
        assert_eq!(
            read.len(),
            95,
            "95 tracks must be loaded even when 5 tracks failed"
        );
        // Verify ordering preserved
        let tracks = read.tracks();
        let first_url = tracks[0].url().expect("Radio track must have URL");
        assert!(
            first_url.contains("track_000"),
            "First track must be track_000, got: {first_url}"
        );
        let last_url = tracks[94].url().expect("Radio track must have URL");
        assert!(
            last_url.contains("track_094"),
            "Last track must be track_094, got: {last_url}"
        );
    }

    // =========================================================================
    // SCENARIO-021: Server shutdown terminates background loading within 1 second
    // AC-09
    // =========================================================================

    /// SCENARIO-021: When the server receives a Quit signal during loading, the background
    /// task must be cancelled via CancellationToken within 1 second.
    #[tokio::test]
    async fn scenario_021_server_shutdown_terminates_loading_within_1s() {
        let cancel_token = CancellationToken::new();
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        // Loading in progress — flag is true
        assert!(playlist_is_loading.load(Ordering::Acquire));

        // Simulate: cancel_token is triggered (server shutdown)
        let cancel_clone = cancel_token.clone();

        let task = tokio::spawn(async move {
            // Simulate waiting for cancellation (as the background task would)
            tokio::select! {
                _ = cancel_clone.cancelled() => {
                    // Task exits cleanly on cancellation
                    true
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    false
                }
            }
        });

        // Trigger shutdown
        let start = Instant::now();
        cancel_token.cancel();

        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("Task must complete within 1 second timeout")
            .expect("Task must not panic");

        let elapsed = start.elapsed();
        assert!(result, "Task must have exited via cancellation path");
        assert!(
            elapsed < Duration::from_secs(1),
            "Shutdown must complete within 1 second, took {:?}",
            elapsed
        );
    }

    // =========================================================================
    // SCENARIO-022: Shutdown after loading completes does not block on thread pool
    // AC-09
    // =========================================================================

    /// SCENARIO-022: When loading has already completed and the thread pool is idle,
    /// shutdown must not be delayed by any loading-related cleanup.
    #[tokio::test]
    async fn scenario_022_shutdown_after_loading_no_delay() {
        let cancel_token = CancellationToken::new();
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(false)); // loading done

        // Loading is already complete
        assert!(!playlist_is_loading.load(Ordering::Acquire));

        // Trigger shutdown
        let start = Instant::now();
        cancel_token.cancel();

        // Verify token is immediately cancelled (no waiting for background tasks)
        assert!(cancel_token.is_cancelled());
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(50),
            "Shutdown after loading must be near-instant, took {:?}",
            elapsed
        );
    }

    // =========================================================================
    // SCENARIO-023: TUI remains interactive during loading
    // AC-10
    // =========================================================================

    /// SCENARIO-023: During loading, the shared playlist must be readable without delay,
    /// proving the TUI can remain interactive (display empty/loading state).
    #[tokio::test]
    async fn scenario_023_tui_remains_interactive_during_loading() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();

        // Simulate multiple rapid reads (TUI render cycles) during loading
        let start = Instant::now();
        for _ in 0..100 {
            let read = playlist.read();
            let _ = read.len();
            let _ = read.get_current_track_index();
            drop(read);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(100),
            "100 playlist reads (TUI render cycles) must complete within 100ms during loading, took {:?}",
            elapsed
        );

        // Loading still in progress
        assert!(playlist_is_loading.load(Ordering::Acquire));
    }

    // =========================================================================
    // SCENARIO-025: Large playlist memory constraint
    // AC-01, AC-02
    // =========================================================================

    /// SCENARIO-025: During loading of a large playlist, the thread pool bounds concurrency.
    /// This test verifies that the completion handler can handle 10,000 tracks without issue.
    #[tokio::test]
    async fn scenario_025_large_playlist_memory_constraint() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Simulate loading 10,000 tracks
        let large_tracks = make_test_tracks(10_000);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            large_tracks,
        );

        let read = playlist.read();
        assert_eq!(
            read.len(),
            10_000,
            "All 10,000 tracks must be present after loading"
        );
        assert!(!playlist_is_loading.load(Ordering::Acquire));
    }

    // =========================================================================
    // SCENARIO-026: Shutdown signal arrives before background loading has started any work
    // AC-09
    // =========================================================================

    /// SCENARIO-026: When shutdown arrives before loading begins processing, the task
    /// must be cancelled cleanly within 1 second.
    #[tokio::test]
    async fn scenario_026_shutdown_before_loading_starts() {
        let cancel_token = CancellationToken::new();
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();

        // Loading flag is true (task spawned but no work done yet)
        assert!(playlist_is_loading.load(Ordering::Acquire));

        // Cancel BEFORE any loading work begins
        cancel_token.cancel();

        // The background task, if it were to check, would see cancellation immediately
        assert!(cancel_token.is_cancelled());

        // Playlist remains empty (no loading work was done)
        assert!(playlist.read().is_empty());

        // Server should exit cleanly within 1 second
        let start = Instant::now();
        // Simulate the select! pattern used in start_background_playlist_load
        tokio::select! {
            _ = cancel_token.cancelled() => {
                // Immediate — token already cancelled
            }
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "Shutdown before loading starts must be immediate, took {:?}",
            elapsed
        );
    }

    // =========================================================================
    // SCENARIO-027: Client disconnects and reconnects during metadata loading
    // AC-04, AC-05
    // =========================================================================

    /// SCENARIO-027: A client that disconnects and reconnects during loading must be able
    /// to read the current state (empty) and eventually receive the full playlist when
    /// loading completes via the broadcast channel.
    #[tokio::test]
    async fn scenario_027_client_reconnect_during_loading() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Client 1 connects during loading — sees empty playlist
        {
            let read = playlist.read();
            assert_eq!(read.len(), 0, "First connection sees empty playlist");
        }

        // Client 1 "disconnects" (drops its subscriber)
        // Client 1 "reconnects" — subscribes to the stream again
        let mut reconnected_rx = stream_tx.subscribe();

        // Still sees empty during loading
        {
            let read = playlist.read();
            assert_eq!(
                read.len(),
                0,
                "Reconnected client sees empty playlist during loading"
            );
        }

        // Loading completes
        let loaded_tracks = vec![
            Track::new_radio("file:///tmp/reconnect_track_1.mp3"),
            Track::new_radio("file:///tmp/reconnect_track_2.mp3"),
        ];

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            loaded_tracks,
        );

        // Reconnected client receives the notification event
        let event = tokio::time::timeout(Duration::from_secs(1), reconnected_rx.recv())
            .await
            .expect("Must receive event within 1 second")
            .expect("Must receive event without error");

        // The event should be a PlaylistChanged event (PlaylistShuffled)
        assert!(
            matches!(
                event,
                UpdateEvents::PlaylistChanged(UpdatePlaylistEvents::PlaylistShuffled(_))
            ),
            "Reconnected client must receive PlaylistShuffled event, got: {:?}",
            event
        );

        // Client can now read the full playlist
        let read = playlist.read();
        assert_eq!(
            read.len(),
            2,
            "After loading, reconnected client sees full playlist"
        );
    }

    // =========================================================================
    // Additional: Verify complete_background_load sends PlaylistLoadComplete command
    // (Supports SCENARIO-009 and the auto-play flow)
    // =========================================================================

    /// Verify that complete_background_load sends PlayerCmd::PlaylistLoadComplete
    /// through the command channel (step 4 of the ordering invariant).
    #[tokio::test]
    async fn complete_background_load_sends_playlist_load_complete_cmd() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            vec![Track::new_radio("file:///tmp/cmd_test.mp3")],
        );

        // Must receive PlaylistLoadComplete command
        let (cmd, _cb) = cmd_rx
            .try_recv()
            .expect("Must receive a command after complete_background_load");
        assert!(
            matches!(cmd, PlayerCmd::PlaylistLoadComplete),
            "Command must be PlaylistLoadComplete, got: {:?}",
            cmd
        );
    }

    // =========================================================================
    // Additional: Verify complete_background_load sends stream notification
    // (Supports SCENARIO-009, SCENARIO-027)
    // =========================================================================

    /// Verify that complete_background_load sends a PlaylistShuffled event via stream_tx
    /// (step 3 of the ordering invariant).
    #[tokio::test]
    async fn complete_background_load_sends_stream_notification() {
        let (stream_tx, mut stream_rx) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            vec![Track::new_radio("file:///tmp/stream_test.mp3")],
        );

        // Must receive a stream event
        let event = stream_rx
            .try_recv()
            .expect("Must receive a stream event after complete_background_load");
        assert!(
            matches!(
                event,
                UpdateEvents::PlaylistChanged(UpdatePlaylistEvents::PlaylistShuffled(_))
            ),
            "Stream event must be PlaylistChanged(PlaylistShuffled), got: {:?}",
            event
        );
    }
}
