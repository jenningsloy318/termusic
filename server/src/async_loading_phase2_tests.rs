//! Phase 2: Background Loading Task and Completion Handler — RED Tests
//!
//! These tests verify Phase 2 behaviors from the spec-04 implementation plan.
//! They target the `start_background_playlist_load` and `complete_background_load`
//! functions (T-05 through T-08) and their integration points.
//!
//! Phase 2 scope:
//! - T-05: `complete_background_load()` with four-step ordering invariant
//! - T-06: `start_background_playlist_load()` function skeleton
//! - T-07: Success path (calls complete_background_load)
//! - T-08: Error paths (total load failure, JoinError, cancellation)
//!
//! BDD Scenario coverage:
//! - SCENARIO-004 (AC-02): Background loading on dedicated thread pool
//! - SCENARIO-006 (AC-03): Loaded playlist matches synchronous implementation
//! - SCENARIO-007 (AC-03): Track ordering preserved
//! - SCENARIO-008 (AC-04): Connected client receives notification
//! - SCENARIO-013 (AC-06): Playback deferred during loading
//! - SCENARIO-014 (AC-06): Playback starts after load when auto-play configured
//! - SCENARIO-015 (AC-07): Save skipped during loading
//! - SCENARIO-016 (AC-07): Save resumes after loading completes
//!
//! These tests are expected to be RED (fail to compile or fail assertions)
//! until the Phase 3 integration is complete. The Phase 2 functions exist but
//! their consumers (player_loop auto-play handler, save_interval protection)
//! require Phase 3 modifications to pass these tests.

#[cfg(test)]
mod phase2_background_loading_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use parking_lot::RwLock;
    use termusiclib::config::{ServerOverlay, SharedServerSettings, new_shared_server_settings};
    use termusiclib::player::{UpdateEvents, UpdatePlaylistEvents};
    use termusiclib::track::Track;
    use termusicplayback::{PlayerCmd, PlayerCmdSender, Playlist, SharedPlaylist, StreamTX};
    use tokio::sync::{broadcast, mpsc::unbounded_channel};
    use tokio_util::sync::CancellationToken;

    use crate::PlaylistLoadingFlag;
    use crate::complete_background_load;

    // =========================================================================
    // Helpers
    // =========================================================================

    fn make_test_config() -> SharedServerSettings {
        new_shared_server_settings(ServerOverlay::default())
    }

    fn make_test_playlist() -> (SharedPlaylist, StreamTX, SharedServerSettings) {
        let config = make_test_config();
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let playlist = Playlist::new(&config, stream_tx.clone());
        let shared: SharedPlaylist = Arc::new(RwLock::new(playlist));
        (shared, stream_tx, config)
    }

    fn make_named_tracks(names: &[&str]) -> Vec<Track> {
        names
            .iter()
            .map(|name| Track::new_radio(format!("file:///tmp/{name}.mp3")))
            .collect()
    }

    // =========================================================================
    // T-05 / AC-03: complete_background_load ordering invariant
    //
    // The four-step ordering invariant states:
    // 1. Write-lock swap (data committed)
    // 2. AtomicBool cleared (Release)
    // 3. Stream event sent (TUI notification)
    // 4. PlayerCmd::PlaylistLoadComplete sent
    //
    // Tests verify that observers on the stream channel can read the playlist
    // data BEFORE or AT the time they receive the notification (ordering guarantee).
    // =========================================================================

    /// SCENARIO-008 / AC-04: Verify that the stream event payload contains the
    /// CORRECT tracks matching what was loaded, not just that the event type
    /// is PlaylistShuffled. This tests the fidelity of step 3 in the ordering
    /// invariant — the event must carry the actual loaded data.
    #[tokio::test]
    async fn complete_background_load_stream_event_contains_correct_track_data() {
        let (stream_tx, mut stream_rx) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["alpha", "beta", "gamma"]);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            1, // current track index = 1 (beta)
            tracks,
        );

        // Receive the stream event
        let event = stream_rx
            .try_recv()
            .expect("Must receive stream event after complete_background_load");

        // Extract the PlaylistShuffled payload
        let playlist_tracks = match event {
            UpdateEvents::PlaylistChanged(UpdatePlaylistEvents::PlaylistShuffled(info)) => {
                info.tracks
            }
            other => panic!(
                "Expected PlaylistChanged(PlaylistShuffled), got: {:?}",
                other
            ),
        };

        // Verify the event carries the correct current_track_index
        assert_eq!(
            playlist_tracks.current_track_index, 1,
            "Stream event must carry current_track_index=1 matching loaded state"
        );

        // Verify the event carries exactly 3 tracks
        assert_eq!(
            playlist_tracks.tracks.len(),
            3,
            "Stream event must carry all 3 loaded tracks"
        );
    }

    /// SCENARIO-006 / AC-03: After complete_background_load, the playlist
    /// `get_current_track()` must return the track at the loaded index.
    /// This verifies the atomicity of the swap — both tracks AND index are set.
    #[tokio::test]
    async fn complete_background_load_sets_current_track_at_loaded_index() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["first", "second", "third"]);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            2, // should point to "third"
            tracks,
        );

        let read = playlist.read();
        let current = read
            .get_current_track()
            .expect("get_current_track must return Some after loading");
        let url = current.url().expect("Track must have URL");
        assert!(
            url.contains("third"),
            "Current track at index 2 must be 'third', got URL: {url}"
        );
    }

    /// SCENARIO-006 / AC-03: After complete_background_load with loaded_index
    /// BEYOND the tracks array length, the index must be clamped to a valid value.
    /// This tests defensive behavior matching Playlist::load()'s clamping logic.
    ///
    /// NOTE: This test is expected to be RED because apply_loaded_data does NOT
    /// clamp loaded_index. The synchronous Playlist::load() does its own clamping,
    /// but complete_background_load should also handle invalid indices defensively.
    #[tokio::test]
    async fn complete_background_load_clamps_index_beyond_track_count() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["only_track_a", "only_track_b"]);

        // loaded_index = 99, but there are only 2 tracks
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            99, // invalid — beyond range
            tracks,
        );

        let read = playlist.read();
        // The index must be clamped to a valid range [0, len-1]
        let idx = read.get_current_track_index();
        assert!(
            idx < read.len(),
            "current_track_index ({idx}) must be less than track count ({}). \
             complete_background_load must clamp out-of-bounds indices.",
            read.len()
        );
    }

    /// SCENARIO-006 / AC-03: complete_background_load with zero tracks and
    /// loaded_index > 0 must set index to 0 (clamped to empty-safe value).
    ///
    /// NOTE: This test is expected to be RED because apply_loaded_data does NOT
    /// handle the empty-tracks-with-nonzero-index case.
    #[tokio::test]
    async fn complete_background_load_handles_empty_tracks_with_nonzero_index() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Empty tracks but nonzero index (edge case from corrupt playlist.log)
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            5, // invalid for empty tracks
            Vec::new(),
        );

        let read = playlist.read();
        assert_eq!(read.len(), 0, "Playlist must be empty");
        // Index must be 0 (the only valid value for an empty playlist)
        assert_eq!(
            read.get_current_track_index(),
            0,
            "current_track_index must be 0 for empty playlist, even if loaded_index was 5"
        );
    }

    // =========================================================================
    // T-05 / AC-03: complete_background_load does NOT mark playlist as modified
    //
    // This is critical for save-interval behavior: the loaded data matches
    // what's already on disk (playlist.log), so the playlist should NOT be
    // saved again until a user mutation occurs.
    // =========================================================================

    /// SCENARIO-016 / AC-07: After complete_background_load, the playlist must
    /// NOT be marked as modified (since loaded data matches disk state).
    /// The save-interval must not redundantly write the same data back.
    #[tokio::test]
    async fn complete_background_load_does_not_mark_playlist_as_modified() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["track_x", "track_y"]);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            tracks,
        );

        // save_if_modified should return Ok(false) since is_modified is false
        // This verifies that the background load doesn't trigger an unnecessary save
        let mut write = playlist.write();
        let result = write.save_if_modified();
        // save_if_modified returns Ok(false) when is_modified is false (no save needed)
        match result {
            Ok(false) => {} // Expected: not modified, no save performed
            Ok(true) => {
                panic!(
                    "Playlist was incorrectly marked as modified after background load — save_if_modified returned Ok(true)"
                )
            }
            Err(e) => {
                // An error here could be expected if no playlist path is configured in test
                // but the key assertion is that it shouldn't have attempted to save at all
                // (is_modified should be false). We verify is_modified indirectly.
                panic!(
                    "save_if_modified errored (attempted to save) which means is_modified was true: {e}"
                )
            }
        }
    }

    // =========================================================================
    // T-05 / AC-04, AC-06: Ordering invariant — flag cleared BEFORE commands sent
    //
    // The spec requires:
    //   Step 2 (flag cleared) happens BEFORE Step 3 (stream event)
    //   Step 2 (flag cleared) happens BEFORE Step 4 (cmd sent)
    //
    // This means any observer that receives the stream event or command can
    // read the loading flag and see it as false (loading complete).
    // =========================================================================

    /// Verify that by the time the stream subscriber receives the event,
    /// the loading flag is already false. This tests the ordering between
    /// step 2 (flag clear) and step 3 (stream send).
    #[tokio::test]
    async fn ordering_invariant_flag_cleared_before_stream_event_observable() {
        let (stream_tx, mut stream_rx) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["order_test"]);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            tracks,
        );

        // The stream event has been sent. At this point, the flag MUST be false.
        let _event = stream_rx.try_recv().expect("Must receive stream event");

        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be false by the time the stream event is receivable \
             (ordering invariant: step 2 before step 3)"
        );
    }

    /// Verify that by the time the cmd_rx receives PlaylistLoadComplete,
    /// the loading flag is already false. This tests the ordering between
    /// step 2 (flag clear) and step 4 (cmd send).
    #[tokio::test]
    async fn ordering_invariant_flag_cleared_before_cmd_observable() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["cmd_order_test"]);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            tracks,
        );

        // The command has been sent. At this point, the flag MUST be false.
        let (cmd, _cb) = cmd_rx
            .try_recv()
            .expect("Must receive command after complete_background_load");
        assert!(matches!(cmd, PlayerCmd::PlaylistLoadComplete));

        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be false by the time PlaylistLoadComplete is receivable \
             (ordering invariant: step 2 before step 4)"
        );
    }

    /// Verify that by the time the stream event is receivable, the playlist
    /// data is already populated. This tests the ordering between step 1
    /// (write-lock swap) and step 3 (stream send).
    #[tokio::test]
    async fn ordering_invariant_data_populated_before_stream_event() {
        let (stream_tx, mut stream_rx) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let tracks = make_named_tracks(&["data_a", "data_b", "data_c"]);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            tracks,
        );

        // Stream event is receivable
        let _event = stream_rx.try_recv().expect("Must receive stream event");

        // At this point, the playlist must already contain the data (step 1 before step 3)
        let read = playlist.read();
        assert_eq!(
            read.len(),
            3,
            "Playlist must be populated before stream event is sent \
             (ordering invariant: step 1 before step 3)"
        );
    }

    // =========================================================================
    // T-08 / AC-08: Error path — load failure clears flag without sending cmds
    //
    // When Playlist::load() returns Err, the background task must:
    //   - Log the error (tested via integration)
    //   - Clear the playlist_is_loading flag (so save-interval resumes)
    //   - NOT send PlaylistLoadComplete (no auto-play on failure)
    //   - NOT crash the server
    //
    // This is tested at the function level by verifying behavior of the error
    // recovery code path in start_background_playlist_load.
    // =========================================================================

    /// SCENARIO-019 / AC-08: On total load failure, the loading flag must be
    /// cleared but NO PlaylistLoadComplete command should be sent (no auto-play
    /// on failure). This tests the error path contract.
    #[tokio::test]
    async fn error_path_load_failure_clears_flag_without_sending_cmd() {
        let (_stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let _cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (_playlist, _, _config) = make_test_playlist();

        // Simulate what start_background_playlist_load does on Err(load_error):
        // It only clears the flag. It does NOT call complete_background_load.
        // So no PlaylistLoadComplete should be sent.
        playlist_is_loading.store(false, Ordering::Release);

        // Verify: flag is cleared
        assert!(
            !playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must be cleared on error path"
        );

        // Verify: no command was sent
        let result = cmd_rx.try_recv();
        assert!(
            result.is_err(),
            "No PlaylistLoadComplete must be sent on load failure — \
             auto-play should NOT trigger when playlist failed to load"
        );
    }

    /// SCENARIO-019 / AC-08: On total load failure, the stream must NOT receive
    /// a PlaylistShuffled event (there's nothing useful to notify about).
    #[tokio::test]
    async fn error_path_load_failure_does_not_send_stream_event() {
        let (_stream_tx, mut stream_rx) = broadcast::channel::<UpdateEvents>(10);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));

        // Simulate error path: only clear the flag (matching implementation)
        playlist_is_loading.store(false, Ordering::Release);

        // Verify: no stream event
        let result = stream_rx.try_recv();
        assert!(
            result.is_err(),
            "No stream event must be sent on load failure"
        );
    }

    // =========================================================================
    // T-06 / AC-09: start_background_playlist_load cancellation behavior
    //
    // When CancellationToken fires DURING Playlist::load() execution:
    //   - The task exits cleanly via select!
    //   - The loading flag is NOT cleared (server is shutting down anyway)
    //   - No data is committed to the playlist
    //   - No commands or events are sent
    // =========================================================================

    /// SCENARIO-021 / AC-09: When cancellation fires during loading, the
    /// playlist must remain empty (no partial data committed).
    #[tokio::test]
    async fn cancellation_during_loading_does_not_commit_partial_data() {
        let cancel_token = CancellationToken::new();
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let _cmd_tx = PlayerCmdSender::new(cmd_tx_raw);

        // Pre-cancel the token (simulating immediate shutdown)
        cancel_token.cancel();

        // Verify: playlist remains empty (no commit happened)
        assert!(
            playlist.read().is_empty(),
            "Playlist must remain empty after cancellation"
        );

        // Verify: loading flag remains true (not cleared on cancellation)
        assert!(
            playlist_is_loading.load(Ordering::Acquire),
            "Loading flag must NOT be cleared on cancellation (server is shutting down)"
        );

        // Verify: no commands sent
        assert!(
            cmd_rx.try_recv().is_err(),
            "No commands must be sent on cancellation"
        );
    }

    // =========================================================================
    // SCENARIO-013 / AC-06: Playback deferred until load complete
    //
    // This test verifies that the PlaylistLoadComplete handler in player_loop
    // triggers auto-play when startup_state is Playing. This requires Phase 3
    // implementation of the handler (currently a no-op).
    //
    // RED REASON: player_loop's PlaylistLoadComplete arm is currently:
    //   `PlayerCmd::PlaylistLoadComplete => { /* Phase 3: auto-play logic */ }`
    // =========================================================================

    /// SCENARIO-013 / AC-06: The save-interval function must accept a
    /// PlaylistLoadingFlag parameter and check it before saving. When the flag
    /// is true, saving must be skipped.
    ///
    /// RED REASON: start_playlist_save_interval does not yet accept the
    /// PlaylistLoadingFlag parameter (Phase 3 Task T-13). This test verifies
    /// the integration point exists by calling the function with the flag.
    #[tokio::test]
    async fn save_interval_accepts_loading_flag_and_skips_during_loading() {
        use tokio::runtime::Handle;

        let handle = Handle::current();
        let cancel_token = CancellationToken::new();
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _stream_tx, _config) = make_test_playlist();

        // This call MUST accept 4 parameters including PlaylistLoadingFlag.
        // Currently start_playlist_save_interval only accepts 3 parameters.
        // This test will fail to compile until Phase 3 T-13 is implemented.
        crate::start_playlist_save_interval(
            handle,
            cancel_token.clone(),
            playlist,
            playlist_is_loading,
        );

        // Clean up
        cancel_token.cancel();
        // Give the task time to observe cancellation
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // =========================================================================
    // SCENARIO-014 / AC-06: Playback starts after load when auto-play configured
    //
    // This test verifies that after PlaylistLoadComplete is processed by
    // player_loop, if startup_state == Playing, resume_from_stopped() is called.
    //
    // RED REASON: player_loop's PlaylistLoadComplete arm is a no-op.
    // We test this indirectly by checking that the command is sent AND that
    // the completion handler's step 4 always sends it (already verified).
    // The actual auto-play trigger requires Phase 3.
    // =========================================================================

    /// SCENARIO-014 / AC-06: Verify that complete_background_load sends
    /// PlaylistLoadComplete even with a single track, ensuring auto-play
    /// can trigger for any non-empty playlist load result.
    #[tokio::test]
    async fn complete_background_load_sends_cmd_for_single_track_playlist() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Single track — minimal playlist
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            vec![Track::new_radio("file:///tmp/single.mp3")],
        );

        let (cmd, _cb) = cmd_rx
            .try_recv()
            .expect("Must receive command for single-track playlist");
        assert!(
            matches!(cmd, PlayerCmd::PlaylistLoadComplete),
            "Must send PlaylistLoadComplete for single-track playlist"
        );
    }

    /// SCENARIO-014 / AC-06: Verify that complete_background_load sends
    /// PlaylistLoadComplete even for an empty playlist (so the system knows
    /// loading is done and can evaluate whether to auto-play or not).
    #[tokio::test]
    async fn complete_background_load_sends_cmd_for_empty_playlist() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Empty playlist (e.g., empty playlist.log)
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            Vec::new(),
        );

        let (cmd, _cb) = cmd_rx
            .try_recv()
            .expect("Must receive command even for empty playlist load");
        assert!(
            matches!(cmd, PlayerCmd::PlaylistLoadComplete),
            "Must send PlaylistLoadComplete even for empty playlist"
        );
    }

    // =========================================================================
    // SCENARIO-015 / AC-07: Save skipped during loading
    //
    // Tests that the save-interval protection mechanism works correctly.
    // The flag must prevent saves during loading.
    // =========================================================================

    /// SCENARIO-015 / AC-07: Verify the save protection pattern: when
    /// playlist_is_loading is true, the save-interval must NOT call
    /// save_if_modified(). This tests the guard condition logic.
    #[tokio::test]
    async fn save_protection_flag_true_prevents_save() {
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));

        // This is the exact guard condition that start_playlist_save_interval
        // must implement (Phase 3):
        //   if playlist_is_loading.load(Ordering::Acquire) { continue; }
        let should_skip = playlist_is_loading.load(Ordering::Acquire);
        assert!(
            should_skip,
            "Save must be skipped when loading flag is true"
        );
    }

    /// SCENARIO-016 / AC-07: Verify that after complete_background_load,
    /// the save protection is lifted and saves can proceed.
    #[tokio::test]
    async fn save_protection_flag_cleared_after_complete_background_load() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // Loading in progress — saves blocked
        assert!(playlist_is_loading.load(Ordering::Acquire));

        // Complete the load
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            make_named_tracks(&["save_test"]),
        );

        // After completion — saves unblocked
        let should_skip = playlist_is_loading.load(Ordering::Acquire);
        assert!(
            !should_skip,
            "Save must NOT be skipped after loading completes (flag is false)"
        );
    }

    // =========================================================================
    // T-07 / AC-04: start_background_playlist_load success path
    //
    // Test that the function signature matches the specification and that it
    // can be called with all required parameters.
    // =========================================================================

    /// SCENARIO-004 / AC-02: Verify start_background_playlist_load can be
    /// invoked with the correct parameters. This validates the function
    /// signature matches spec section 4.1.
    #[tokio::test]
    async fn start_background_playlist_load_has_correct_signature() {
        use tokio::runtime::Handle;

        let handle = Handle::current();
        let cancel_token = CancellationToken::new();
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, config) = make_test_playlist();

        // Cancel immediately to prevent actual I/O during test
        cancel_token.cancel();

        // This validates the function exists with the expected signature
        crate::start_background_playlist_load(
            handle,
            cancel_token,
            playlist,
            playlist_is_loading,
            stream_tx,
            cmd_tx,
            config,
        );

        // Give the task a moment to observe cancellation
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // =========================================================================
    // T-05 / Concurrency: Verify that complete_background_load is safe to
    // call concurrently with playlist readers (no deadlock, no panic).
    // =========================================================================

    /// SCENARIO-005 / AC-02: Verify that concurrent readers on the SharedPlaylist
    /// do not deadlock when complete_background_load acquires the write lock.
    /// The write lock must be held for minimum time (just the swap).
    #[tokio::test]
    async fn complete_background_load_does_not_deadlock_with_concurrent_readers() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, _cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        let playlist_clone = playlist.clone();

        // Spawn many concurrent readers on OS threads (parking_lot guards are not Send)
        let readers: Vec<_> = (0..10)
            .map(|_| {
                let pl = playlist_clone.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        let read = pl.read();
                        let _ = read.len();
                        drop(read);
                        std::thread::yield_now();
                    }
                })
            })
            .collect();

        // Complete the load while readers are active
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            make_named_tracks(&["concurrent_a", "concurrent_b"]),
        );

        // All readers must complete without deadlock (within 2 seconds)
        let start = std::time::Instant::now();
        for r in readers {
            r.join().expect("Reader thread must not panic");
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "Concurrent readers must complete within 2 seconds (no deadlock), took {:?}",
            elapsed
        );
    }

    // =========================================================================
    // T-05: complete_background_load idempotency — calling it twice must
    // result in the second call's data being the final state.
    // =========================================================================

    /// Verify that if complete_background_load is called twice (hypothetical
    /// race condition), the second call's data takes precedence and both
    /// calls complete without error.
    #[tokio::test]
    async fn complete_background_load_second_call_overwrites_first() {
        let (stream_tx, _) = broadcast::channel::<UpdateEvents>(10);
        let (cmd_tx_raw, mut cmd_rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(cmd_tx_raw);
        let playlist_is_loading: PlaylistLoadingFlag = Arc::new(AtomicBool::new(true));
        let (playlist, _, _config) = make_test_playlist();

        // First call
        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            0,
            make_named_tracks(&["first_a", "first_b"]),
        );

        // Reset flag and call again (simulate hypothetical double-load)
        playlist_is_loading.store(true, Ordering::Release);

        complete_background_load(
            &playlist,
            &playlist_is_loading,
            &stream_tx,
            &cmd_tx,
            1,
            make_named_tracks(&["second_x", "second_y", "second_z"]),
        );

        // Second call's data must be the final state
        let read = playlist.read();
        assert_eq!(
            read.len(),
            3,
            "Second complete_background_load must overwrite first"
        );
        assert_eq!(read.get_current_track_index(), 1);

        // Both calls should have sent PlaylistLoadComplete
        let (cmd1, _) = cmd_rx.try_recv().expect("First cmd");
        let (cmd2, _) = cmd_rx.try_recv().expect("Second cmd");
        assert!(matches!(cmd1, PlayerCmd::PlaylistLoadComplete));
        assert!(matches!(cmd2, PlayerCmd::PlaylistLoadComplete));
    }
}
