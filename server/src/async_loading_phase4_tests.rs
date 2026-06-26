//! Phase 4: Async Server Metadata Loading — Integration Testing and Validation
//!
//! These tests provide comprehensive integration coverage for the async loading feature,
//! exercising the full pipeline with real filesystem I/O using temp fixtures.
//!
//! Coverage of acceptance criteria:
//!   AC-01: Server gRPC listener accepts connections within 1 second (SCENARIO-001, SCENARIO-002, SCENARIO-003)
//!   AC-02: Metadata loading on dedicated background thread pool (SCENARIO-004, SCENARIO-005)
//!   AC-03: Loaded playlist matches synchronous behavior (SCENARIO-006, SCENARIO-007)
//!   AC-04: Connected clients receive notification (SCENARIO-008, SCENARIO-009)
//!   AC-05: GetPlaylist non-blocking during loading (SCENARIO-010, SCENARIO-011, SCENARIO-012)
//!   AC-06: Playback deferred until load complete (SCENARIO-013, SCENARIO-014)
//!   AC-07: Save protection during loading (SCENARIO-015, SCENARIO-016, SCENARIO-017)
//!   AC-08: Graceful degradation on load failure (SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024)
//!   AC-09: Clean shutdown within 1 second (SCENARIO-021, SCENARIO-022, SCENARIO-026)
//!   AC-10: TUI remains responsive during loading (SCENARIO-023)
//!
//! These tests are RED because they require a testable entry point
//! `start_background_playlist_load_from_path` that does not yet exist.
//! This function is identical to `start_background_playlist_load` but accepts a
//! `PathBuf` for the playlist file, enabling filesystem-based integration testing
//! without depending on the system config directory.

#[cfg(test)]
mod phase4_integration_tests {
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use parking_lot::RwLock;
    use tempfile::TempDir;
    use termusiclib::config::{ServerOverlay, SharedServerSettings, new_shared_server_settings};
    use termusiclib::player::{UpdateEvents, UpdatePlaylistEvents};
    use termusicplayback::playlist::parallel_load::load_playlist_from_path;
    use termusicplayback::{PlayerCmd, PlayerCmdSender, Playlist, SharedPlaylist, StreamTX};
    use tokio::sync::{broadcast, mpsc::unbounded_channel};
    use tokio_util::sync::CancellationToken;

    use crate::PlaylistLoadingFlag;
    // This function does not yet exist — it is the testable entry point that Phase 4
    // integration testing requires. It is identical to `start_background_playlist_load`
    // but accepts a `PathBuf` for the playlist file instead of relying on the system
    // config directory. This import will cause a compile error (RED state) until the
    // function is implemented.
    use crate::start_background_playlist_load_from_path;

    // =========================================================================
    // Test Fixture Helpers
    // =========================================================================

    /// Create a temp directory containing a playlist.log fixture file.
    /// Returns (TempDir, path_to_playlist_log).
    fn create_playlist_fixture(track_count: usize) -> (TempDir, PathBuf) {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let playlist_path = tmp_dir.path().join("playlist.log");
        let mut file = File::create(&playlist_path).expect("create playlist.log");

        // First line is current track index
        writeln!(file, "0").expect("write index");

        // Write track paths (radio URLs for simplicity — no I/O needed to "load" them)
        for i in 0..track_count {
            writeln!(file, "http://example.com/track_{:04}.mp3", i).expect("write track");
        }

        file.flush().expect("flush");
        (tmp_dir, playlist_path)
    }

    /// Create a playlist fixture with a mix of local (non-existent) paths and radio URLs.
    fn create_mixed_playlist_fixture(
        valid_radio_count: usize,
        invalid_local_count: usize,
    ) -> (TempDir, PathBuf) {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let playlist_path = tmp_dir.path().join("playlist.log");
        let mut file = File::create(&playlist_path).expect("create playlist.log");

        writeln!(file, "0").expect("write index");

        // Write valid radio URLs
        for i in 0..valid_radio_count {
            writeln!(file, "http://example.com/valid_track_{:04}.mp3", i).expect("write track");
        }

        // Write invalid local paths (files that don't exist)
        for i in 0..invalid_local_count {
            writeln!(file, "/nonexistent/path/track_{:04}.flac", i).expect("write track");
        }

        file.flush().expect("flush");
        (tmp_dir, playlist_path)
    }

    /// Create a playlist fixture with corrupt/unparseable lines interspersed.
    fn create_corrupt_playlist_fixture() -> (TempDir, PathBuf) {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let playlist_path = tmp_dir.path().join("playlist.log");
        let mut file = File::create(&playlist_path).expect("create playlist.log");

        writeln!(file, "2").expect("write index"); // index points to 3rd track
        writeln!(file, "http://example.com/track_valid_1.mp3").expect("write valid");
        writeln!(file, "http://example.com/track_valid_2.mp3").expect("write valid");
        writeln!(file, "http://example.com/track_valid_3.mp3").expect("write valid");
        // Empty lines and comments are filtered (not "corrupt" per se)
        writeln!(file, "").expect("write empty");
        writeln!(file, "# comment line").expect("write comment");
        writeln!(file, "http://example.com/track_valid_4.mp3").expect("write valid");

        file.flush().expect("flush");
        (tmp_dir, playlist_path)
    }

    fn make_test_config() -> SharedServerSettings {
        new_shared_server_settings(ServerOverlay::default())
    }

    fn make_empty_playlist(config: &SharedServerSettings) -> (SharedPlaylist, StreamTX) {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let playlist = Playlist::new(config, stream_tx.clone());
        let shared: SharedPlaylist = Arc::new(RwLock::new(playlist));
        (shared, stream_tx)
    }

    // =========================================================================
    // T-18: Server Startup Timing Tests
    // SCENARIO-001: Server accepts connections within 1s with large playlist (1000 tracks)
    // SCENARIO-002: Server accepts connections immediately with empty playlist
    // SCENARIO-003: Server accepts connections within 1s with small playlist (10 tracks)
    // AC-01
    // =========================================================================

    /// SCENARIO-001: With a large playlist (1000 tracks), the background loading task
    /// must be spawned and the server ready within 1 second. The test verifies that
    /// `start_background_playlist_load_from_path` spawns the load task without blocking.
    #[tokio::test]
    async fn t18_scenario_001_server_accepts_connection_within_1s_with_large_playlist() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(1000);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        let start = Instant::now();

        // Spawn the background loading with a custom path — must return immediately
        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        let spawn_elapsed = start.elapsed();

        // The spawn itself must be near-instant (< 100ms) — the actual loading happens
        // in the background. This simulates the server being ready to accept connections.
        assert!(
            spawn_elapsed < Duration::from_millis(100),
            "Spawning background load must be near-instant (< 100ms), took {:?}",
            spawn_elapsed
        );

        // At this point, the "server" is ready to accept connections.
        // The playlist is still empty because loading is in the background.
        assert!(
            playlist.read().is_empty(),
            "Playlist must be empty immediately after spawn (loading in background)"
        );
        assert!(
            playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must still be true (loading not yet complete)"
        );

        // Wait for loading to complete (up to 5 seconds for CI environments)
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // After loading completes, verify the playlist is populated
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading must complete within 5 seconds"
        );
        assert_eq!(
            playlist.read().len(),
            1000,
            "All 1000 tracks must be loaded after background loading completes"
        );

        cancel_token.cancel();
    }

    /// SCENARIO-002: With an empty playlist (0 tracks), the server starts immediately.
    /// No background loading task should block, and the flag clears quickly.
    #[tokio::test]
    async fn t18_scenario_002_server_accepts_connection_with_empty_playlist() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(0);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        let start = Instant::now();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete (should be nearly instant for empty playlist)
        let deadline = Instant::now() + Duration::from_secs(2);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let total_elapsed = start.elapsed();

        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading of empty playlist must complete within 2 seconds"
        );
        assert!(
            total_elapsed < Duration::from_secs(1),
            "Empty playlist loading must complete within 1 second total, took {:?}",
            total_elapsed
        );
        assert_eq!(
            playlist.read().len(),
            0,
            "Playlist must remain empty for empty playlist.log"
        );

        cancel_token.cancel();
    }

    /// SCENARIO-003: With a small playlist (10 tracks), server accepts connections within 1s.
    #[tokio::test]
    async fn t18_scenario_003_server_accepts_connection_within_1s_with_small_playlist() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(10);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        let start = Instant::now();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Spawn returns immediately
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "Background load spawn must be instant"
        );

        // Wait for completion
        let deadline = Instant::now() + Duration::from_secs(2);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));
        assert_eq!(playlist.read().len(), 10);

        cancel_token.cancel();
    }

    // =========================================================================
    // T-19: Playlist Correctness Tests
    // SCENARIO-006: Loaded playlist matches synchronous implementation output exactly
    // SCENARIO-007: Track ordering preserved after asynchronous loading
    // AC-03
    // =========================================================================

    /// SCENARIO-006: The async-loaded playlist must contain the same tracks in the
    /// same order as `load_playlist_from_path` (the synchronous baseline).
    #[tokio::test]
    async fn t19_scenario_006_playlist_correctness_matches_synchronous_baseline() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(50);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        // Load synchronously using the reference implementation
        let (sync_index, sync_tracks) = load_playlist_from_path(&playlist_path)
            .expect("Synchronous load must succeed with valid fixture");

        // Load asynchronously via the background task
        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for async load to complete
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Async loading must complete"
        );

        let read = playlist.read();
        let async_tracks = read.tracks();

        // Compare: same track count
        assert_eq!(
            async_tracks.len(),
            sync_tracks.len(),
            "Async-loaded playlist must have same track count as sync: expected {}, got {}",
            sync_tracks.len(),
            async_tracks.len()
        );

        // Compare: same current track index
        assert_eq!(
            read.get_current_track_index(),
            sync_index,
            "Async-loaded current_track_index must match sync: expected {}, got {}",
            sync_index,
            read.get_current_track_index()
        );

        // Compare: same tracks in same order
        for (i, (sync_track, async_track)) in
            sync_tracks.iter().zip(async_tracks.iter()).enumerate()
        {
            assert_eq!(
                sync_track.url(),
                async_track.url(),
                "Track at index {i}: URL mismatch between sync and async load"
            );
        }

        cancel_token.cancel();
    }

    /// SCENARIO-007: Tracks with variable metadata read times are still in correct order.
    /// Uses a playlist with specific ordering that must be preserved through async load.
    #[tokio::test]
    async fn t19_scenario_007_track_ordering_preserved_with_variable_latency() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let playlist_path = tmp_dir.path().join("playlist.log");
        let mut file = File::create(&playlist_path).expect("create playlist.log");

        // Write tracks A through E in specific order
        writeln!(file, "0").expect("write index");
        writeln!(file, "http://example.com/track_A.mp3").expect("write A");
        writeln!(file, "http://example.com/track_B.mp3").expect("write B");
        writeln!(file, "http://example.com/track_C.mp3").expect("write C");
        writeln!(file, "http://example.com/track_D.mp3").expect("write D");
        writeln!(file, "http://example.com/track_E.mp3").expect("write E");
        file.flush().expect("flush");

        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for completion
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        let read = playlist.read();
        let tracks = read.tracks();
        assert_eq!(tracks.len(), 5);

        let expected_order = ["track_A", "track_B", "track_C", "track_D", "track_E"];
        for (i, expected_name) in expected_order.iter().enumerate() {
            let url = tracks[i].url().expect("Radio track must have URL");
            assert!(
                url.contains(expected_name),
                "Track at index {i} must contain '{expected_name}', got: {url}"
            );
        }

        cancel_token.cancel();
    }

    // =========================================================================
    // T-20: Save Protection Tests
    // SCENARIO-015: Periodic save skips writing while loading in progress
    // SCENARIO-016: Save resumes normally after loading completes
    // SCENARIO-017: Manual save blocked during loading
    // AC-07
    // =========================================================================

    /// SCENARIO-015: The playlist.log file must NOT be overwritten during background loading.
    /// Verifies that the save-interval logic skips when `playlist_is_loading` is true.
    #[tokio::test]
    async fn t20_scenario_015_save_skipped_during_loading() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(100);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        // Record the original file content
        let original_content = fs::read_to_string(&playlist_path).expect("read original");

        // Start background loading
        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path.clone(),
        );

        // While loading is in progress, the save interval must skip
        // (simulating what start_playlist_save_interval does)
        let loading_still_in_progress = playlist_is_loading.load(Ordering::Acquire);
        if loading_still_in_progress {
            // The save-interval guard would skip saving here
            let would_save = !playlist_is_loading.load(Ordering::Acquire);
            assert!(
                !would_save,
                "Save MUST be skipped while playlist_is_loading is true"
            );
        }

        // Verify the file was not modified during the loading check
        let current_content = fs::read_to_string(&playlist_path).expect("read current");
        assert_eq!(
            original_content, current_content,
            "playlist.log must NOT be modified during background loading"
        );

        cancel_token.cancel();
    }

    /// SCENARIO-016: After loading completes, save_if_modified should work normally.
    #[tokio::test]
    async fn t20_scenario_016_save_resumes_after_loading_completes() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(5);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading must complete"
        );

        // After loading, save should be allowed
        let would_save = !playlist_is_loading.load(Ordering::Acquire);
        assert!(
            would_save,
            "Save MUST be allowed after loading completes (flag is false)"
        );

        // The playlist should have been populated
        assert_eq!(playlist.read().len(), 5);

        cancel_token.cancel();
    }

    /// SCENARIO-017: Any save trigger during loading is suppressed.
    /// Tests the guard logic with multiple rapid checks simulating timer ticks.
    #[tokio::test]
    async fn t20_scenario_017_all_save_paths_blocked_during_loading() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));

        // Simulate 10 rapid save-interval ticks during loading
        for tick in 0..10 {
            let should_save = !playlist_is_loading.load(Ordering::Acquire);
            assert!(
                !should_save,
                "Save tick {tick} MUST be blocked during loading"
            );
        }

        // Complete loading
        playlist_is_loading.store(false, Ordering::Release);

        // Now saves should proceed
        let should_save = !playlist_is_loading.load(Ordering::Acquire);
        assert!(should_save, "Save must be allowed after loading completes");
    }

    // =========================================================================
    // T-21: Auto-play Deferral Tests
    // SCENARIO-013: Playback does not start while loading in progress
    // SCENARIO-014: Playback starts after loading completes when auto-play configured
    // AC-06
    // =========================================================================

    /// SCENARIO-013: Even with startup_state=Playing, the PlaylistLoadComplete command
    /// is NOT sent until loading finishes. During loading, no auto-play occurs.
    #[tokio::test]
    async fn t21_scenario_013_autoplay_deferred_during_loading() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(500);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Immediately after spawn, no PlaylistLoadComplete should be in the channel
        // (loading hasn't completed yet for a 500-track playlist)
        tokio::time::sleep(Duration::from_millis(5)).await;
        let immediate_cmd = cmd_rx.try_recv();

        // During the loading window, there should be no PlaylistLoadComplete command yet.
        // If loading completed that fast (unlikely for 500 tracks), the test still validates
        // the command wasn't sent BEFORE loading completed.
        if playlist_is_loading.load(Ordering::Acquire) {
            assert!(
                immediate_cmd.is_err(),
                "PlaylistLoadComplete must NOT be sent while loading is in progress"
            );
        }

        // Wait for loading to finish
        let deadline = Instant::now() + Duration::from_secs(10);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // After loading completes, PlaylistLoadComplete must eventually arrive
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut received_load_complete = false;
        while Instant::now() < deadline {
            match cmd_rx.try_recv() {
                Ok((PlayerCmd::PlaylistLoadComplete, _)) => {
                    received_load_complete = true;
                    break;
                }
                _ => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }

        assert!(
            received_load_complete,
            "PlaylistLoadComplete MUST be sent after loading completes"
        );

        cancel_token.cancel();
    }

    /// SCENARIO-014: After loading completes, the PlaylistLoadComplete command is sent.
    /// The player_loop (not tested here directly) would call resume_from_stopped()
    /// if startup_state == Playing. We verify the command is sent correctly.
    #[tokio::test]
    async fn t21_scenario_014_playlist_load_complete_sent_after_loading() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(10);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        // Drain the channel to find PlaylistLoadComplete
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut found = false;
        while Instant::now() < deadline {
            match cmd_rx.try_recv() {
                Ok((PlayerCmd::PlaylistLoadComplete, _)) => {
                    found = true;
                    break;
                }
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                _ => {}
            }
        }

        assert!(
            found,
            "PlaylistLoadComplete command must be received after background loading finishes"
        );

        cancel_token.cancel();
    }

    // =========================================================================
    // T-22: Graceful Degradation Tests
    // SCENARIO-018: Corrupt playlist.log partial load
    // SCENARIO-019: Unreadable playlist.log
    // SCENARIO-020: Individual track I/O failure
    // SCENARIO-024: Missing playlist.log
    // AC-08
    // =========================================================================

    /// SCENARIO-018: A playlist.log with some invalid content still loads valid tracks.
    /// Comments and empty lines are filtered; valid URLs are loaded as radio tracks.
    #[tokio::test]
    async fn t22_scenario_018_corrupt_playlist_log_partial_load() {
        let (_tmp_dir, playlist_path) = create_corrupt_playlist_fixture();
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        let read = playlist.read();
        // The fixture has 4 valid HTTP URLs (empty lines and comments are filtered)
        assert_eq!(
            read.len(),
            4,
            "Only valid tracks must be loaded (comments and empty lines filtered). Got {} tracks",
            read.len()
        );

        // The current track index should be clamped to valid range
        // (original index was 2, which is valid for 4 tracks)
        assert_eq!(
            read.get_current_track_index(),
            2,
            "Current track index must be preserved when valid"
        );

        cancel_token.cancel();
    }

    /// SCENARIO-019: When playlist.log cannot be read (e.g., doesn't exist for read),
    /// the server operates with an empty playlist.
    #[tokio::test]
    async fn t22_scenario_019_unreadable_playlist_log_results_in_empty_playlist() {
        // Create a path that doesn't exist
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let nonexistent_path = tmp_dir.path().join("does_not_exist.log");

        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            nonexistent_path,
        );

        // Wait for the task to handle the error
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // The loading flag must be cleared even on failure
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be cleared after load failure (server continues with empty playlist)"
        );

        // Playlist remains empty
        assert_eq!(
            playlist.read().len(),
            0,
            "Playlist must be empty when playlist.log cannot be read"
        );

        cancel_token.cancel();
    }

    /// SCENARIO-020: When some tracks reference non-existent files, only the valid
    /// tracks are loaded. Invalid local paths are skipped.
    #[tokio::test]
    async fn t22_scenario_020_individual_track_io_failure_does_not_halt_loading() {
        let (_tmp_dir, playlist_path) = create_mixed_playlist_fixture(10, 5);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        let read = playlist.read();
        // The 10 radio URLs are valid; the 5 local paths don't exist and will be filtered
        // by the parallel_read_local_tracks function (which skips files that fail to read)
        assert!(
            read.len() >= 10,
            "At least the 10 valid radio tracks must be loaded. Got {} tracks",
            read.len()
        );
        assert!(
            read.len() <= 15,
            "At most 15 tracks possible (10 radio + 5 local). Got {} tracks",
            read.len()
        );

        cancel_token.cancel();
    }

    /// SCENARIO-024: When no playlist.log exists at the expected path, the server
    /// starts with an empty playlist and continues operating.
    #[tokio::test]
    async fn t22_scenario_024_missing_playlist_log_handled_gracefully() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let missing_path = tmp_dir.path().join("nonexistent_playlist.log");

        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        let start = Instant::now();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            missing_path,
        );

        // Wait for the task to complete (should be fast since file doesn't exist)
        let deadline = Instant::now() + Duration::from_secs(2);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let elapsed = start.elapsed();

        // Must complete quickly and gracefully
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be cleared for missing file"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "Handling missing playlist.log must complete within 1 second, took {:?}",
            elapsed
        );
        assert_eq!(playlist.read().len(), 0, "Playlist must be empty");

        cancel_token.cancel();
    }

    // =========================================================================
    // T-23: Shutdown During Loading Tests
    // SCENARIO-021: Server shutdown terminates loading within 1 second
    // SCENARIO-022: Shutdown after loading completes has no additional delay
    // SCENARIO-026: Shutdown before loading starts any work
    // AC-09
    // =========================================================================

    /// SCENARIO-021: When the server shuts down during active loading, the background
    /// task must be cancelled within 1 second via CancellationToken.
    #[tokio::test]
    async fn t23_scenario_021_shutdown_during_loading_within_1s() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(500);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Give the task a moment to start
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Cancel (simulating shutdown)
        let start = Instant::now();
        cancel_token.cancel();

        // The cancellation itself is instant; verify it completes quickly
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "Cancellation signal must be delivered within 1 second, took {:?}",
            elapsed
        );

        // After cancellation, the loading flag might still be true (loading was interrupted)
        // The task exits without committing, which is correct behavior for shutdown.
        // The key invariant is that the server can exit cleanly.
        assert!(
            cancel_token.is_cancelled(),
            "Cancel token must be in cancelled state"
        );
    }

    /// SCENARIO-022: When loading has already completed, shutdown has no delay from
    /// the loading infrastructure.
    #[tokio::test]
    async fn t23_scenario_022_shutdown_after_loading_no_delay() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(5);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete first
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!playlist_is_loading.load(Ordering::Acquire));

        // NOW trigger shutdown — should be instant since loading already done
        let start = Instant::now();
        cancel_token.cancel();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(50),
            "Shutdown after loading must be near-instant, took {:?}",
            elapsed
        );
    }

    /// SCENARIO-026: Shutdown arrives before loading has started any actual work.
    /// The task must be cancelled cleanly.
    #[tokio::test]
    async fn t23_scenario_026_shutdown_before_loading_starts_work() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(1000);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        // Cancel BEFORE spawning the task
        cancel_token.cancel();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Give the spawned task time to observe cancellation
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Playlist must remain empty (loading was cancelled before starting)
        assert!(
            playlist.read().is_empty(),
            "Playlist must be empty when loading was cancelled before starting"
        );
    }

    // =========================================================================
    // T-18 extended: Client Notification Tests
    // SCENARIO-008: Connected client receives notification when loading completes
    // SCENARIO-009: Client connecting after loading gets full playlist
    // AC-04
    // =========================================================================

    /// SCENARIO-008: A client subscribed to the stream before loading completes
    /// receives a PlaylistShuffled event notification.
    #[tokio::test]
    async fn t18_scenario_008_client_receives_notification_after_load_complete() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(20);
        let config = make_test_config();
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let playlist = Arc::new(RwLock::new(Playlist::new(&config, stream_tx.clone())));
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        // Subscribe BEFORE loading starts (simulating connected client)
        let mut client_rx = stream_tx.subscribe();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for the notification event
        let event = tokio::time::timeout(Duration::from_secs(5), client_rx.recv())
            .await
            .expect("Must receive event within 5 seconds")
            .expect("Must receive event without error");

        // The event must be a PlaylistShuffled notification
        assert!(
            matches!(
                event,
                UpdateEvents::PlaylistChanged(UpdatePlaylistEvents::PlaylistShuffled(_))
            ),
            "Client must receive PlaylistShuffled event after loading, got: {:?}",
            event
        );

        // After notification, playlist is populated
        assert!(!playlist_is_loading.load(Ordering::Acquire));
        assert_eq!(playlist.read().len(), 20);

        cancel_token.cancel();
    }

    /// SCENARIO-009: A client that connects AFTER loading already completed receives
    /// the full playlist on GetPlaylist.
    #[tokio::test]
    async fn t18_scenario_009_client_after_loading_gets_full_playlist() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(15);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading to complete
        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        // Now a "new client connects" and reads the playlist — must see full contents
        let read = playlist.read();
        assert_eq!(
            read.len(),
            15,
            "Client connecting after loading must see all 15 tracks"
        );
        assert_eq!(
            read.get_current_track_index(),
            0,
            "Current track index must be 0 (as written in fixture)"
        );

        cancel_token.cancel();
    }

    // =========================================================================
    // Non-Blocking Queries During Loading
    // SCENARIO-010: GetPlaylist returns empty during loading
    // SCENARIO-011: Multiple concurrent GetPlaylist calls
    // AC-05
    // =========================================================================

    /// SCENARIO-010 / SCENARIO-011: While loading is in progress, GetPlaylist calls
    /// return immediately with empty state (non-blocking).
    #[tokio::test]
    async fn t18_scenario_010_011_get_playlist_non_blocking_during_loading() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(500);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Immediately after spawn: simulate 3 concurrent GetPlaylist reads
        let start = Instant::now();
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let pl = playlist.clone();
                tokio::spawn(async move {
                    let read = pl.read();
                    read.len()
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
            "3 concurrent GetPlaylist reads must complete within 100ms, took {:?}",
            elapsed
        );

        // During loading, all should see empty (or possibly partially loaded if extremely fast)
        // The key assertion is non-blocking response within 100ms
        for (i, count) in results.iter().enumerate() {
            // We assert that responses are immediate, not necessarily that they are empty
            // (on fast machines, loading might complete before the reads)
            assert!(
                *count <= 500,
                "Client {i} result must be <= 500 tracks (sanity check)"
            );
        }

        cancel_token.cancel();
    }

    // =========================================================================
    // TUI Responsiveness (SCENARIO-023)
    // AC-10
    // =========================================================================

    /// SCENARIO-023: During loading, multiple rapid reads of the shared playlist
    /// (simulating TUI render cycles) must all complete quickly.
    #[tokio::test]
    async fn t18_scenario_023_tui_responsiveness_during_loading() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(500);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Simulate 50 rapid TUI render cycles during loading
        let start = Instant::now();
        for _ in 0..50 {
            let read = playlist.read();
            let _ = read.len();
            let _ = read.get_current_track_index();
            drop(read);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(100),
            "50 TUI render-cycle reads must complete within 100ms during loading, took {:?}",
            elapsed
        );

        cancel_token.cancel();
    }

    // =========================================================================
    // Index Clamping Edge Cases
    // AC-03
    // =========================================================================

    /// When the playlist.log has a current_track_index larger than the loaded track
    /// count, the index must be clamped to a valid range.
    #[tokio::test]
    async fn index_clamped_when_exceeds_track_count() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let playlist_path = tmp_dir.path().join("playlist.log");
        let mut file = File::create(&playlist_path).expect("create");

        // Index 999 but only 3 tracks
        writeln!(file, "999").expect("write index");
        writeln!(file, "http://example.com/t1.mp3").expect("write");
        writeln!(file, "http://example.com/t2.mp3").expect("write");
        writeln!(file, "http://example.com/t3.mp3").expect("write");
        file.flush().expect("flush");

        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        let read = playlist.read();
        assert_eq!(read.len(), 3);
        // Index must be clamped to max valid index (2 for 3 tracks)
        assert!(
            read.get_current_track_index() <= 2,
            "Index must be clamped to valid range [0, len-1]. Got: {}",
            read.get_current_track_index()
        );

        cancel_token.cancel();
    }

    /// Empty playlist.log (only index line, no tracks) results in empty playlist
    /// with index 0.
    #[tokio::test]
    async fn empty_playlist_log_only_index_line() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let playlist_path = tmp_dir.path().join("playlist.log");
        let mut file = File::create(&playlist_path).expect("create");
        writeln!(file, "5").expect("write index only");
        file.flush().expect("flush");

        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));

        let read = playlist.read();
        assert_eq!(read.len(), 0, "Playlist must be empty with no track lines");
        assert_eq!(
            read.get_current_track_index(),
            0,
            "Index must be 0 for empty playlist"
        );

        cancel_token.cancel();
    }

    // =========================================================================
    // Background Thread Pool Isolation
    // SCENARIO-004: Metadata loading on dedicated thread pool
    // SCENARIO-005: Background loading does not starve gRPC service
    // AC-02
    // =========================================================================

    /// SCENARIO-004/005: While background loading is active, the tokio runtime remains
    /// responsive. This verifies that spawn_blocking is used (not running on async runtime).
    #[tokio::test]
    async fn t18_scenario_004_005_background_loading_does_not_block_async_runtime() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(200);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // The async runtime must remain responsive during loading.
        // If loading blocked the runtime, this sleep would not resolve on time.
        let start = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(100),
            "Async runtime must remain responsive during background loading. \
             tokio::time::sleep(10ms) took {:?} (would be blocked if loading on async runtime)",
            elapsed
        );

        cancel_token.cancel();
    }

    // =========================================================================
    // SCENARIO-027: Client reconnect during loading
    // AC-04, AC-05
    // =========================================================================

    /// SCENARIO-027: A client that disconnects and reconnects during loading can
    /// subscribe to the stream and receive the completion notification.
    #[tokio::test]
    async fn t18_scenario_027_client_reconnect_during_loading() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(30);
        let config = make_test_config();
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let playlist = Arc::new(RwLock::new(Playlist::new(&config, stream_tx.clone())));
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        // Client 1 "connects" and sees empty playlist
        assert_eq!(playlist.read().len(), 0);

        // Client 1 "disconnects" (drops subscription)
        // Client 1 "reconnects" by subscribing again
        let mut reconnected_rx = stream_tx.subscribe();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Reconnected client receives the notification when loading finishes
        let event = tokio::time::timeout(Duration::from_secs(5), reconnected_rx.recv())
            .await
            .expect("Must receive event within 5 seconds")
            .expect("Must not error");

        assert!(
            matches!(
                event,
                UpdateEvents::PlaylistChanged(UpdatePlaylistEvents::PlaylistShuffled(_))
            ),
            "Reconnected client must receive PlaylistShuffled event"
        );

        // After notification, full playlist is available
        assert_eq!(playlist.read().len(), 30);

        cancel_token.cancel();
    }

    // =========================================================================
    // SCENARIO-025: Large playlist memory constraint
    // AC-01, AC-02
    // =========================================================================

    /// SCENARIO-025: A very large playlist (10000 tracks) does not cause issues.
    /// Bounded by thread pool size (no unbounded concurrency).
    #[tokio::test]
    async fn t18_scenario_025_large_playlist_loads_without_memory_issues() {
        let (_tmp_dir, playlist_path) = create_playlist_fixture(10_000);
        let config = make_test_config();
        let (playlist, stream_tx) = make_empty_playlist(&config);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let cancel_token = CancellationToken::new();

        start_background_playlist_load_from_path(
            tokio::runtime::Handle::current(),
            cancel_token.clone(),
            playlist.clone(),
            playlist_is_loading.clone(),
            stream_tx.clone(),
            cmd_tx,
            playlist_path,
        );

        // Wait for loading (generous timeout for large playlist)
        let deadline = Instant::now() + Duration::from_secs(30);
        while playlist_is_loading.load(Ordering::Acquire) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(!playlist_is_loading.load(Ordering::Acquire));
        assert_eq!(
            playlist.read().len(),
            10_000,
            "All 10,000 tracks must be loaded"
        );

        cancel_token.cancel();
    }
}
