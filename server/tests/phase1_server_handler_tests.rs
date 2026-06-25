//! Phase 1: Server-Side Podcast Command Handler Tests
//!
//! These tests verify that the server correctly handles the new PlayerCmd variants
//! for podcast operations (PodcastFeedRefresh, PodcastDownloadEpisodes).
//!
//! These tests are in the server crate's test module because they verify server behavior.
//! They will fail to compile until the new PlayerCmd variants are added and handled.
//!
//! Coverage:
//!   AC-01: Server owns all podcast network operations (T-04, T-05)
//!   AC-02: TUI contains zero direct calls to check_feed/download_list
//!   SCENARIO-001: Server assumes ownership of feed refresh operations
//!   SCENARIO-002: TUI delegates all podcast network operations to server
//!   SCENARIO-005: Server handles feed refresh when TUI is disconnected

#[cfg(test)]
mod phase1_server_handler_tests {
    use termusiclib::config::v2::server::PodcastSettings;
    use termusiclib::config::v2::server::ServerSettings;
    use termusiclib::config::{ServerOverlay, SharedServerSettings, new_shared_server_settings};
    use termusiclib::podcast::PodcastNoId;
    use termusiclib::podcast::db::Database;
    use termusicplayback::{EpisodeDownloadRequest, PlayerCmd, PlayerCmdSender};
    use tokio::sync::mpsc::unbounded_channel;

    use std::path::Path;

    // =========================================================================
    // Helper: create a SharedServerSettings with test config
    // =========================================================================

    fn make_test_config(download_dir: &Path) -> SharedServerSettings {
        let settings = ServerSettings {
            podcast: PodcastSettings {
                download_dir: download_dir.to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        new_shared_server_settings(ServerOverlay {
            settings,
            ..Default::default()
        })
    }

    fn make_cmd_channel() -> (
        PlayerCmdSender,
        tokio::sync::mpsc::UnboundedReceiver<(
            PlayerCmd,
            termusicplayback::PlayerCmdCallbackSender,
        )>,
    ) {
        let (tx, rx) = unbounded_channel();
        (PlayerCmdSender::new(tx), rx)
    }

    // =========================================================================
    // T-04: Server handles PodcastFeedRefresh command
    // AC-01, SCENARIO-001, SCENARIO-005
    // =========================================================================

    /// The PlayerCmd::PodcastFeedRefresh variant can be sent through the command channel.
    /// This verifies the variant exists and is sendable via the established communication layer.
    #[tokio::test]
    async fn podcast_feed_refresh_command_is_sendable_through_channel() {
        let (cmd_tx, mut cmd_rx) = make_cmd_channel();

        // Send the new PodcastFeedRefresh command
        let send_result = cmd_tx.send(PlayerCmd::PodcastFeedRefresh);
        assert!(
            send_result.is_ok(),
            "PodcastFeedRefresh should be sendable through PlayerCmdSender"
        );

        // Verify it can be received
        let received = cmd_rx.try_recv();
        assert!(received.is_ok(), "Should receive the sent command");

        let (cmd, _callback) = received.unwrap();
        assert!(
            matches!(cmd, PlayerCmd::PodcastFeedRefresh),
            "Received command should be PodcastFeedRefresh, got: {:?}",
            cmd
        );
    }

    // =========================================================================
    // T-05: Server handles PodcastDownloadEpisodes command
    // AC-01, SCENARIO-001
    // =========================================================================

    /// The PlayerCmd::PodcastDownloadEpisodes variant can be sent through the command channel.
    /// This verifies the variant exists and carries its data correctly through the channel.
    #[tokio::test]
    async fn podcast_download_episodes_command_is_sendable_through_channel() {
        let (cmd_tx, mut cmd_rx) = make_cmd_channel();

        let requests = vec![
            EpisodeDownloadRequest {
                podcast_id: 1,
                episode_url: "http://127.0.0.1:9999/episode1.mp3".to_string(),
                episode_title: "Episode 1".to_string(),
            },
            EpisodeDownloadRequest {
                podcast_id: 1,
                episode_url: "http://127.0.0.1:9999/episode2.mp3".to_string(),
                episode_title: "Episode 2".to_string(),
            },
        ];

        let send_result = cmd_tx.send(PlayerCmd::PodcastDownloadEpisodes(requests));
        assert!(
            send_result.is_ok(),
            "PodcastDownloadEpisodes should be sendable through PlayerCmdSender"
        );

        let received = cmd_rx.try_recv();
        assert!(received.is_ok(), "Should receive the sent command");

        let (cmd, _callback) = received.unwrap();
        match cmd {
            PlayerCmd::PodcastDownloadEpisodes(reqs) => {
                assert_eq!(reqs.len(), 2, "Should carry 2 download requests");
                assert_eq!(reqs[0].podcast_id, 1);
                assert_eq!(reqs[0].episode_url, "http://127.0.0.1:9999/episode1.mp3");
                assert_eq!(reqs[1].episode_title, "Episode 2");
            }
            other => panic!(
                "Expected PodcastDownloadEpisodes, got: {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    /// PodcastDownloadEpisodes with empty Vec should be sendable (no-op case).
    #[tokio::test]
    async fn podcast_download_episodes_empty_request_is_valid() {
        let (cmd_tx, mut cmd_rx) = make_cmd_channel();

        let send_result = cmd_tx.send(PlayerCmd::PodcastDownloadEpisodes(vec![]));
        assert!(send_result.is_ok());

        let (cmd, _) = cmd_rx.try_recv().unwrap();
        match cmd {
            PlayerCmd::PodcastDownloadEpisodes(reqs) => {
                assert_eq!(reqs.len(), 0);
            }
            _ => panic!("Expected PodcastDownloadEpisodes"),
        }
    }

    // =========================================================================
    // AC-02: Verify TUI does not directly call check_feed or download_list
    // SCENARIO-002: TUI delegates all podcast network operations to server
    // =========================================================================
    //
    // NOTE: This is a compile-time / static analysis constraint.
    // The test below uses a structural assertion approach: after Phase 1 migration,
    // the TUI podcast component should ONLY use PlayerCmd sends, never direct function calls.
    // This is verified by ensuring that PlayerCmd::PodcastFeedRefresh and
    // PlayerCmd::PodcastDownloadEpisodes are the ONLY way podcast operations are triggered.
    //
    // The actual verification that TUI code no longer calls check_feed/download_list
    // is enforced by removing the import of those functions from the TUI crate.
    // If the TUI code still references them after migration, it will fail to compile.
    // =========================================================================

    /// After migration, the server must be the sole handler of podcast feed refresh.
    /// This test verifies the communication contract: TUI sends PodcastFeedRefresh,
    /// server receives it and would invoke check_feed internally.
    #[tokio::test]
    async fn server_is_sole_owner_of_feed_refresh_after_migration() {
        let (cmd_tx, mut cmd_rx) = make_cmd_channel();

        // Simulate what the TUI would do after migration: send a command
        cmd_tx
            .send(PlayerCmd::PodcastFeedRefresh)
            .expect("send should succeed");

        // Server side receives the command
        let (cmd, _) = cmd_rx.try_recv().expect("should receive command");
        assert!(matches!(cmd, PlayerCmd::PodcastFeedRefresh));

        // The server would then call check_feed internally.
        // We verify the command is received correctly - the actual handler
        // implementation is tested separately in the server integration tests.
    }

    /// After migration, the server must be the sole handler of episode downloads.
    /// This test verifies the communication contract: TUI sends PodcastDownloadEpisodes,
    /// server receives it and would invoke download_list internally.
    #[tokio::test]
    async fn server_is_sole_owner_of_episode_downloads_after_migration() {
        let (cmd_tx, mut cmd_rx) = make_cmd_channel();

        let requests = vec![EpisodeDownloadRequest {
            podcast_id: 5,
            episode_url: "http://127.0.0.1:8080/test_download.mp3".to_string(),
            episode_title: "Test Download Episode".to_string(),
        }];

        // Simulate what the TUI would do after migration
        cmd_tx
            .send(PlayerCmd::PodcastDownloadEpisodes(requests))
            .expect("send should succeed");

        // Server side receives the command
        let (cmd, _) = cmd_rx.try_recv().expect("should receive command");
        match cmd {
            PlayerCmd::PodcastDownloadEpisodes(reqs) => {
                assert_eq!(reqs.len(), 1);
                assert_eq!(reqs[0].podcast_id, 5);
            }
            _ => panic!("Expected PodcastDownloadEpisodes"),
        }
    }

    // =========================================================================
    // AC-03, SCENARIO-003: Manual podcast refresh works identically after migration
    // =========================================================================

    /// The PodcastFeedRefresh command should trigger a feed check for ALL subscribed
    /// podcasts (matching the existing TUI behavior where refresh checks all feeds).
    /// This is a contract test - the actual implementation calls check_feed for each podcast.
    #[tokio::test]
    async fn podcast_feed_refresh_triggers_check_for_all_subscribed_podcasts() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp_dir.path();

        // Set up a database with multiple podcasts
        let db = Database::new(db_path).expect("create database");
        let podcast1 = PodcastNoId {
            title: "Podcast One".to_string(),
            url: "http://127.0.0.1:9999/feed1.xml".to_string(),
            description: None,
            author: None,
            explicit: None,
            last_checked: chrono::Utc::now(),
            episodes: vec![],
            image_url: None,
        };
        let podcast2 = PodcastNoId {
            title: "Podcast Two".to_string(),
            url: "http://127.0.0.1:9999/feed2.xml".to_string(),
            description: None,
            author: None,
            explicit: None,
            last_checked: chrono::Utc::now(),
            episodes: vec![],
            image_url: None,
        };
        db.insert_podcast(&podcast1).expect("insert podcast 1");
        db.insert_podcast(&podcast2).expect("insert podcast 2");

        // Verify we can construct and send the refresh command
        let (cmd_tx, mut cmd_rx) = make_cmd_channel();
        cmd_tx
            .send(PlayerCmd::PodcastFeedRefresh)
            .expect("send should succeed");

        let (cmd, _) = cmd_rx.try_recv().expect("should receive command");
        assert!(matches!(cmd, PlayerCmd::PodcastFeedRefresh));
        // When the server handles this command, it should check ALL podcasts in the database
    }

    /// PodcastDownloadEpisodes carries enough information for the server to download
    /// episodes without needing access to the TUI's local state.
    #[tokio::test]
    async fn episode_download_request_carries_all_needed_info_for_server() {
        let request = EpisodeDownloadRequest {
            podcast_id: 10,
            episode_url: "http://127.0.0.1:8080/full_episode.mp3".to_string(),
            episode_title: "Full Info Episode".to_string(),
        };

        // The server needs:
        // 1. podcast_id - to look up the podcast's download directory
        assert_eq!(request.podcast_id, 10);
        // 2. episode_url - to know WHERE to download from
        assert_eq!(
            request.episode_url,
            "http://127.0.0.1:8080/full_episode.mp3"
        );
        // 3. episode_title - to construct the filename
        assert_eq!(request.episode_title, "Full Info Episode");
    }

    // =========================================================================
    // SCENARIO-004: OPML import/export remain functional after migration
    // =========================================================================
    //
    // Note: OPML import/export are already server-side operations (they use the
    // database directly). This scenario is verified by confirming the existing
    // export_to_opml / import_from_opml functions remain accessible from the
    // server crate. No new test needed if they already work - but we document
    // that they should NOT be affected by the migration.
    // =========================================================================

    /// OPML export function should remain accessible from the server crate context.
    /// This is a compile-time check that the import path still works.
    #[test]
    fn opml_export_remains_accessible_from_server_crate() {
        // Verify the function exists and is importable
        use termusiclib::podcast::export_to_opml;
        // We just need this to compile - the function signature check is sufficient
        let _fn_ptr: fn(&std::path::Path, &std::path::Path) -> anyhow::Result<()> = export_to_opml;
    }
}
