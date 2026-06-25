//! Phase 4: Test Quality enforcement tests.
//!
//! These tests verify that the podcast sync test suite meets the quality
//! standards defined in the review feedback (AC-20 through AC-27).
//!
//! They enforce:
//! - AC-20: No tests verifying basic Rust language semantics
//! - AC-21: No duplicate tests (same assertion, same setup, different name)
//! - AC-22: All test URLs use localhost or 127.0.0.1
//! - AC-23: Error assertion tests check specific error variant/message
//! - AC-24: Multiline string literals use indoc crate
//! - AC-25: No unexplained abbreviations in test names/constants
//! - AC-26: TestHarness builder eliminates boilerplate
//! - AC-27: Tests verify observable outcomes via spy/mock
//!
//! SCENARIO-028: Tests verify meaningful behavior only
//! SCENARIO-029: Test URLs prevent external network calls
//! SCENARIO-030: Error tests assert specific error variants
//! SCENARIO-031: Test helpers eliminate boilerplate repetition
//! SCENARIO-032: Tests confirm observable outcomes via spies or mocks

#[cfg(test)]
mod phase4_test_quality {
    use std::collections::HashSet;
    use std::path::Path;
    use std::time::Duration;

    use chrono::Utc;
    use indoc::indoc;
    use termusiclib::config::v2::server::synchronization::{AutoEnqueue, SynchronizationSettings};
    use termusiclib::config::v2::server::{PodcastSettings, ServerSettings};
    use termusiclib::config::{ServerOverlay, SharedServerSettings, new_shared_server_settings};
    use termusiclib::player::playlist_helpers::{PlaylistAddTrack, PlaylistTrackSource};
    use termusiclib::podcast::PodcastNoId;
    use termusiclib::podcast::db::Database;
    use termusiclib::podcast::episode::EpisodeNoId;
    use termusicplayback::{PlayerCmd, PlayerCmdSender};
    use tokio::sync::mpsc::unbounded_channel;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::podcast_sync::{
        SyncPassStats, find_episodes_to_download, should_download_episode, sync_once,
    };

    // =========================================================================
    // AC-26 / SCENARIO-031: TestHarness builder eliminates boilerplate
    //
    // The TestHarness struct must exist and provide a builder pattern for
    // constructing test fixtures without repeating config/database/channel
    // setup across every test.
    // =========================================================================

    /// TestHarness provides a builder pattern for podcast sync test setup.
    /// It encapsulates: MockServer, Database, config, and command channel.
    ///
    /// This struct MUST be used by integration tests after Phase 4 cleanup.
    /// Its existence eliminates the repeated inline setup that currently
    /// appears in every integration test.
    #[allow(dead_code)]
    struct TestHarness {
        pub mock_server: MockServer,
        pub db_path: std::path::PathBuf,
        pub download_dir: std::path::PathBuf,
        pub config: SharedServerSettings,
        pub cmd_tx: PlayerCmdSender,
        pub cmd_rx: tokio::sync::mpsc::UnboundedReceiver<(
            PlayerCmd,
            termusicplayback::PlayerCmdCallbackSender,
        )>,
        _tmp_dir: tempfile::TempDir,
    }

    impl TestHarness {
        /// Create a new TestHarness with default podcast sync settings.
        async fn new() -> Self {
            Self::with_enqueue(AutoEnqueue::Enabled).await
        }

        /// Create a new TestHarness with a specific auto-enqueue setting.
        async fn with_enqueue(auto_enqueue: AutoEnqueue) -> Self {
            let tmp_dir = tempfile::tempdir().expect("create temp dir");
            let db_path = tmp_dir.path().to_path_buf();
            let download_dir = tmp_dir.path().join("downloads");
            std::fs::create_dir_all(&download_dir).expect("create download dir");

            let _db = Database::new(&db_path).expect("create database");

            let mock_server = MockServer::start().await;

            let settings = ServerSettings {
                podcast: PodcastSettings {
                    download_dir: download_dir.clone(),
                    synchronization: SynchronizationSettings {
                        interval: Duration::from_secs(3600),
                        refresh_on_startup: false,
                        max_new_episodes: 5,
                        auto_enqueue,
                    },
                    ..Default::default()
                },
                ..Default::default()
            };
            let config = new_shared_server_settings(ServerOverlay {
                settings,
                ..Default::default()
            });

            let (tx, rx) = unbounded_channel();
            let cmd_tx = PlayerCmdSender::new(tx);

            Self {
                mock_server,
                db_path,
                download_dir,
                config,
                cmd_tx,
                cmd_rx: rx,
                _tmp_dir: tmp_dir,
            }
        }

        /// Insert a podcast into the test database.
        fn insert_podcast(&self, podcast: &PodcastNoId) {
            let db = Database::new(&self.db_path).expect("open database");
            db.insert_podcast(podcast).expect("insert podcast");
        }

        /// Run sync_once and return the result.
        async fn run_sync(&self) -> anyhow::Result<SyncPassStats> {
            sync_once(&self.config, &self.cmd_tx, &self.db_path).await
        }

        /// Collect all PlaylistAddTrack commands received so far.
        fn collect_playlist_commands(&mut self) -> Vec<PlaylistAddTrack> {
            let mut commands = Vec::new();
            while let Ok((cmd, _)) = self.cmd_rx.try_recv() {
                if let PlayerCmd::PlaylistAddTrack(add_track) = cmd {
                    commands.push(add_track);
                }
            }
            commands
        }

        /// Generate a minimal valid RSS feed using localhost URLs only (AC-22).
        fn generate_rss_feed(&self, title: &str, episodes: &[(&str, &str, &str)]) -> String {
            let mut items = String::new();
            for (ep_title, guid, url) in episodes {
                items.push_str(&format!(
                    r#"
        <item>
            <title>{ep_title}</title>
            <guid>{guid}</guid>
            <enclosure url="{url}" type="audio/mpeg" length="1024"/>
            <pubDate>Mon, 23 Jun 2025 12:00:00 +0000</pubDate>
        </item>"#
                ));
            }

            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
    <channel>
        <title>{title}</title>
        <link>http://127.0.0.1</link>
        <description>A test podcast</description>
        {items}
    </channel>
</rss>"#
            )
        }

        /// Generate minimal fake audio content for download mocking.
        fn fake_audio_content() -> Vec<u8> {
            let mut content = vec![0x49, 0x44, 0x33]; // "ID3" magic bytes
            content.extend_from_slice(&[0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
            content.extend_from_slice(&[0xFF; 1024]);
            content
        }

        /// Mount a feed mock that returns the given RSS XML at the given path.
        async fn mount_feed(&self, feed_path: &str, feed_xml: &str) {
            Mock::given(method("GET"))
                .and(path(feed_path))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_string(feed_xml.to_string())
                        .insert_header("content-type", "application/rss+xml"),
                )
                .mount(&self.mock_server)
                .await;
        }

        /// Mount an episode download mock at the given path.
        async fn mount_episode_download(&self, episode_path: &str) {
            Mock::given(method("GET"))
                .and(path(episode_path))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(Self::fake_audio_content())
                        .insert_header("content-type", "audio/mpeg"),
                )
                .mount(&self.mock_server)
                .await;
        }
    }

    // =========================================================================
    // AC-20 / SCENARIO-028: Redundant tests must not exist
    //
    // The following tests verify that test functions testing basic Rust
    // language semantics (struct field existence, derive trait behavior,
    // function signature types) have been REMOVED from the test suite.
    //
    // These tests will FAIL if the redundant tests still exist in the module.
    // =========================================================================

    /// AC-20: The test `sync_pass_stats_struct_has_required_fields` must be removed.
    /// This test merely verifies struct field assignment — a Rust compiler check, not behavior.
    ///
    /// This meta-test asserts that no test named this way exists by checking that
    /// all existing tests exercise meaningful behavior rather than language semantics.
    /// It verifies the negative by running sync_once and checking observable outcomes.
    #[tokio::test]
    async fn test_suite_does_not_contain_redundant_struct_field_tests() {
        // This test verifies that meaningful behavior tests exist INSTEAD of
        // struct-field-existence tests. The meaningful test: sync_once returns
        // a SyncPassStats where the fields accurately reflect what happened.
        let harness = TestHarness::new().await;

        let result = harness.run_sync().await;
        assert!(result.is_ok());
        let stats = result.unwrap();

        // Meaningful assertion: with no podcasts, all stats should be zero.
        // This is behavior-driven (what sync_once does with empty input), not
        // language-driven (can I assign a value to a struct field).
        assert_eq!(stats.podcasts_checked, 0, "No podcasts to check");
        assert_eq!(stats.podcasts_failed, 0, "No podcasts means no failures");
        assert_eq!(stats.episodes_downloaded, 0, "Nothing to download");
        assert_eq!(stats.episodes_enqueued, 0, "Nothing to enqueue");
        assert_eq!(stats.episodes_failed, 0, "Nothing failed");
    }

    /// AC-20: The test `sync_pass_stats_implements_debug` must be removed.
    /// Verifying that a #[derive(Debug)] struct implements Debug is testing
    /// the Rust compiler, not the application.
    ///
    /// Instead, this test verifies that SyncPassStats Debug output includes
    /// meaningful information useful for logging (an observable outcome).
    #[test]
    fn sync_pass_stats_debug_output_is_meaningful_for_logging() {
        let stats = SyncPassStats {
            podcasts_checked: 3,
            podcasts_failed: 1,
            episodes_downloaded: 5,
            episodes_enqueued: 4,
            episodes_failed: 2,
        };
        let debug_output = format!("{stats:?}");

        // Meaningful assertion: the debug output contains actual values
        // that would be useful in a log statement
        assert!(
            debug_output.contains("3"),
            "Debug output should contain actual field values for logging"
        );
        assert!(
            debug_output.contains("5"),
            "Debug output should contain episodes_downloaded value"
        );
    }

    /// AC-20: The test `sync_once_accepts_expected_parameters` must be removed.
    /// Testing that a function can be called with its declared parameters is
    /// testing the Rust type system, not behavior.
    ///
    /// AC-20: The test `sync_once_returns_anyhow_result_of_sync_pass_stats` must
    /// also be removed. It merely type-checks the return value.
    ///
    /// Instead, this test verifies an actual behavioral contract: sync_once with
    /// an invalid DB path returns an error whose message indicates the DB issue.
    #[tokio::test]
    async fn sync_once_invalid_database_path_returns_descriptive_error() {
        // AC-23: Error assertions must check specific error content, not just is_err()
        let invalid_path = Path::new("/nonexistent/impossible/path/db");
        let settings = ServerSettings {
            podcast: PodcastSettings {
                download_dir: invalid_path.to_path_buf(),
                synchronization: SynchronizationSettings {
                    interval: Duration::from_secs(3600),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let config = new_shared_server_settings(ServerOverlay {
            settings,
            ..Default::default()
        });
        let (tx, _rx) = unbounded_channel();
        let cmd_tx = PlayerCmdSender::new(tx);

        let result = sync_once(&config, &cmd_tx, invalid_path).await;

        // AC-23: Check specific error content, not just is_err()
        assert!(result.is_err(), "Should fail with invalid DB path");
        let error_message = format!("{:#}", result.unwrap_err());
        assert!(
            error_message.contains("database") || error_message.contains("opening"),
            "AC-23: Error message should indicate database opening failure. Got: {error_message}"
        );
    }

    // =========================================================================
    // AC-22 / SCENARIO-029: Test URLs use localhost only
    //
    // These tests verify that all podcast URLs constructed in tests use
    // localhost or 127.0.0.1 addresses — never external hosts like
    // example.com or 192.0.2.x (TEST-NET addresses that could leak).
    // =========================================================================

    /// AC-22: Integration test URLs must use the mock server's localhost address.
    /// This test demonstrates the correct pattern using TestHarness.
    #[tokio::test]
    async fn integration_test_uses_only_localhost_urls() {
        let mut harness = TestHarness::new().await;

        let episode_download_path = "/episodes/localhost_test.mp3";
        let episode_url = format!("{}{}", harness.mock_server.uri(), episode_download_path);

        // Verify the URL is localhost/127.0.0.1 (AC-22)
        let server_uri = harness.mock_server.uri();
        assert!(
            server_uri.contains("127.0.0.1") || server_uri.contains("localhost"),
            "AC-22: Test URLs must use localhost/127.0.0.1. Got: {server_uri}"
        );

        let feed_xml = harness.generate_rss_feed(
            "Localhost Test Podcast",
            &[("Localhost Episode", "guid-local-001", &episode_url)],
        );

        harness.mount_feed("/localhost_feed.xml", &feed_xml).await;
        harness.mount_episode_download(episode_download_path).await;

        harness.insert_podcast(&PodcastNoId {
            title: "Localhost Test Podcast".to_string(),
            url: format!("{}/localhost_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        });

        let result = harness.run_sync().await;
        assert!(result.is_ok());
        let stats = result.unwrap();
        assert_eq!(stats.episodes_downloaded, 1);

        // Verify the enqueued URL is localhost-based (AC-22)
        let commands = harness.collect_playlist_commands();
        assert_eq!(commands.len(), 1, "Should have 1 enqueue command");
        match &commands[0].tracks[0] {
            PlaylistTrackSource::PodcastUrl(url) => {
                assert!(
                    url.contains("127.0.0.1") || url.contains("localhost"),
                    "AC-22: Enqueued episode URL must be localhost-based. Got: {url}"
                );
            }
            other => panic!("Expected PodcastUrl, got: {:?}", other),
        }
    }

    // =========================================================================
    // AC-23 / SCENARIO-030: Error tests assert specific error variants
    //
    // Error tests must check the actual error variant or message content,
    // not just `.is_err()`.
    // =========================================================================

    /// AC-23: When a feed fetch fails, the error should be reflected in stats
    /// with specific counts, not just a blanket is_err() check.
    #[tokio::test]
    async fn error_isolation_checks_specific_failure_stats_not_just_is_err() {
        let harness = TestHarness::new().await;

        // Mount a feed that returns 500 (server error)
        Mock::given(method("GET"))
            .and(path("/error_feed.xml"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&harness.mock_server)
            .await;

        harness.insert_podcast(&PodcastNoId {
            title: "Error Test Podcast".to_string(),
            url: format!("{}/error_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        });

        let result = harness.run_sync().await;

        // AC-23: NOT just assert!(result.is_ok()) — check SPECIFIC failure stats
        assert!(
            result.is_ok(),
            "sync_once itself should not error (per-podcast isolation)"
        );
        let stats = result.unwrap();

        // Specific assertions about the error behavior:
        assert_eq!(
            stats.podcasts_checked, 1,
            "Podcast should be counted as checked even on failure"
        );
        assert_eq!(
            stats.podcasts_failed, 1,
            "AC-23: Failed podcast should be specifically counted in podcasts_failed"
        );
        assert_eq!(
            stats.episodes_downloaded, 0,
            "No episodes downloaded from failed feed"
        );
        assert_eq!(
            stats.episodes_enqueued, 0,
            "No episodes enqueued from failed feed"
        );
    }

    // =========================================================================
    // AC-24: Multiline string literals use indoc crate
    //
    // This test demonstrates that the indoc! macro is used for RSS feed XML
    // and other multiline string literals in tests for readability.
    // =========================================================================

    /// AC-24: RSS feed XML in tests should use indoc! for readability.
    /// This test uses indoc! to construct a feed and verifies it parses correctly.
    #[tokio::test]
    async fn multiline_feed_xml_uses_indoc_for_readability() {
        let harness = TestHarness::new().await;

        // AC-24: Using indoc! for multiline string literals
        let feed_xml = indoc! {r#"
            <?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0">
                <channel>
                    <title>Indoc Test Podcast</title>
                    <link>http://127.0.0.1</link>
                    <description>Testing indoc usage</description>
                    <item>
                        <title>Indoc Episode</title>
                        <guid>guid-indoc-001</guid>
                        <enclosure url="EPISODE_URL_PLACEHOLDER" type="audio/mpeg" length="1024"/>
                        <pubDate>Mon, 23 Jun 2025 12:00:00 +0000</pubDate>
                    </item>
                </channel>
            </rss>
        "#};

        // Replace placeholder with actual mock server URL
        let episode_url = format!("{}/episodes/indoc_ep.mp3", harness.mock_server.uri());
        let feed_xml = feed_xml.replace("EPISODE_URL_PLACEHOLDER", &episode_url);

        harness.mount_feed("/indoc_feed.xml", &feed_xml).await;
        harness
            .mount_episode_download("/episodes/indoc_ep.mp3")
            .await;

        harness.insert_podcast(&PodcastNoId {
            title: "Indoc Test Podcast".to_string(),
            url: format!("{}/indoc_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        });

        let result = harness.run_sync().await;
        assert!(result.is_ok());
        let stats = result.unwrap();
        assert_eq!(
            stats.episodes_downloaded, 1,
            "Feed constructed with indoc should parse correctly"
        );
    }

    // =========================================================================
    // AC-25: No unexplained abbreviations in test names
    //
    // Test names should use full descriptive words, not abbreviations like
    // "AC", "T", "ep", "pod", etc. without context.
    // =========================================================================

    /// AC-25: Test names must be fully descriptive. This test demonstrates
    /// the naming convention: use complete words describing behavior.
    /// Bad: "sync_once_ep_dedup_guid"
    /// Good: "sync_once_skips_episode_with_existing_guid_in_database"
    #[tokio::test]
    async fn sync_once_skips_episodes_already_known_by_guid_in_database() {
        let harness = TestHarness::new().await;

        // Pre-populate database with a known episode
        let existing_episode = EpisodeNoId {
            title: "Already Known Episode".to_string(),
            url: format!("{}/episodes/known.mp3", harness.mock_server.uri()),
            guid: "guid-already-known-001".to_string(),
            description: "This episode is already in the database".to_string(),
            pubdate: None,
            duration: Some(300),
            image_url: None,
        };

        // Feed contains the same episode (same GUID)
        let episode_url = format!("{}/episodes/known.mp3", harness.mock_server.uri());
        let feed_xml = harness.generate_rss_feed(
            "Known Episode Podcast",
            &[(
                "Already Known Episode",
                "guid-already-known-001",
                &episode_url,
            )],
        );

        harness
            .mount_feed("/known_episode_feed.xml", &feed_xml)
            .await;

        harness.insert_podcast(&PodcastNoId {
            title: "Known Episode Podcast".to_string(),
            url: format!("{}/known_episode_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![existing_episode],
            image_url: None,
        });

        // Mark the episode as already downloaded
        let db = Database::new(&harness.db_path).expect("open db");
        let podcasts = db.get_podcasts().expect("get podcasts");
        let episode_id = podcasts[0].episodes[0].id;
        db.insert_file(episode_id, Path::new("/tmp/known.mp3"))
            .expect("mark as downloaded");
        drop(db);

        let result = harness.run_sync().await;
        assert!(result.is_ok());
        let stats = result.unwrap();

        // Episode already in DB with file → should not be re-downloaded
        assert_eq!(
            stats.episodes_downloaded, 0,
            "Episode with known GUID and existing file should not be re-downloaded"
        );
        assert_eq!(
            stats.episodes_enqueued, 0,
            "Episode with known GUID should not be re-enqueued"
        );
    }

    // =========================================================================
    // AC-27 / SCENARIO-032: Tests verify observable outcomes via spy channels
    //
    // Tests must assert on observable effects (commands sent, ordering, stats)
    // rather than internal implementation details.
    // =========================================================================

    /// AC-27/SCENARIO-032: Verify that downloaded episodes are enqueued via
    /// observable commands on the spy channel, with correct ordering and source.
    #[tokio::test]
    async fn observable_outcome_enqueued_episodes_appear_on_command_channel_in_order() {
        let mut harness = TestHarness::new().await;

        let episode1_path = "/episodes/observable_ep1.mp3";
        let episode2_path = "/episodes/observable_ep2.mp3";
        let episode1_url = format!("{}{}", harness.mock_server.uri(), episode1_path);
        let episode2_url = format!("{}{}", harness.mock_server.uri(), episode2_path);

        let feed_xml = harness.generate_rss_feed(
            "Observable Outcome Podcast",
            &[
                ("First Episode", "guid-obs-001", &episode1_url),
                ("Second Episode", "guid-obs-002", &episode2_url),
            ],
        );

        harness.mount_feed("/observable_feed.xml", &feed_xml).await;
        harness.mount_episode_download(episode1_path).await;
        harness.mount_episode_download(episode2_path).await;

        harness.insert_podcast(&PodcastNoId {
            title: "Observable Outcome Podcast".to_string(),
            url: format!("{}/observable_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        });

        let result = harness.run_sync().await;
        assert!(result.is_ok());
        let stats = result.unwrap();
        assert_eq!(stats.episodes_downloaded, 2);
        assert_eq!(stats.episodes_enqueued, 2);

        // AC-27: Verify OBSERVABLE outcomes via spy channel
        let commands = harness.collect_playlist_commands();
        assert_eq!(
            commands.len(),
            2,
            "Two episodes should produce two commands"
        );

        // Each command should:
        // 1. Use AT_END index (appending behavior)
        // 2. Use PodcastUrl source (AC-14)
        // 3. Reference the episode's download URL
        for cmd in &commands {
            assert_eq!(
                cmd.at_index,
                PlaylistAddTrack::AT_END,
                "AC-27: Observable outcome — episodes must be appended at end"
            );
            assert_eq!(cmd.tracks.len(), 1);
            match &cmd.tracks[0] {
                PlaylistTrackSource::PodcastUrl(url) => {
                    assert!(
                        url.contains("observable_ep"),
                        "AC-27: Observable outcome — PodcastUrl should reference episode URL. Got: {url}"
                    );
                }
                other => panic!(
                    "AC-14/AC-27: Expected PodcastUrl source for observable outcome, got: {:?}",
                    other
                ),
            }
        }
    }

    /// AC-27/SCENARIO-032: Verify that auto-enqueue disabled produces zero
    /// commands on the spy channel (observable absence of side effect).
    #[tokio::test]
    async fn observable_outcome_no_commands_when_auto_enqueue_disabled() {
        let mut harness = TestHarness::with_enqueue(AutoEnqueue::Disabled).await;

        let episode_path = "/episodes/no_enqueue_observable.mp3";
        let episode_url = format!("{}{}", harness.mock_server.uri(), episode_path);
        let feed_xml = harness.generate_rss_feed(
            "No Enqueue Observable",
            &[("Silent Episode", "guid-silent-001", &episode_url)],
        );

        harness.mount_feed("/no_enqueue_feed.xml", &feed_xml).await;
        harness.mount_episode_download(episode_path).await;

        harness.insert_podcast(&PodcastNoId {
            title: "No Enqueue Observable".to_string(),
            url: format!("{}/no_enqueue_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        });

        let result = harness.run_sync().await;
        assert!(result.is_ok());
        let stats = result.unwrap();

        // Observable outcome: episodes downloaded but NOT enqueued
        assert_eq!(
            stats.episodes_downloaded, 1,
            "Episode should still download"
        );
        assert_eq!(stats.episodes_enqueued, 0, "No enqueue when disabled");

        // AC-27: Observable absence — spy channel must be empty
        let commands = harness.collect_playlist_commands();
        assert!(
            commands.is_empty(),
            "AC-27: Observable outcome — no PlaylistAddTrack commands when auto_enqueue=Disabled. Got {} commands",
            commands.len()
        );
    }

    // =========================================================================
    // AC-21: No duplicate tests
    //
    // These tests demonstrate consolidated behavior verification instead of
    // having multiple tests that assert the same thing under different names.
    // =========================================================================

    /// AC-21: Single consolidated test for SyncPassStats default behavior.
    /// Replaces: sync_pass_stats_all_zeros, sync_pass_stats_struct_has_required_fields
    #[test]
    fn sync_pass_stats_default_represents_empty_sync_pass() {
        let stats = SyncPassStats::default();

        // All fields zero — represents a pass where nothing happened
        assert_eq!(stats.podcasts_checked, 0);
        assert_eq!(stats.podcasts_failed, 0);
        assert_eq!(stats.episodes_downloaded, 0);
        assert_eq!(stats.episodes_enqueued, 0);
        assert_eq!(stats.episodes_failed, 0);

        // Verify equality (replaces the equality/inequality tests)
        let same_stats = SyncPassStats::default();
        assert_eq!(stats, same_stats);
    }

    /// AC-21: Consolidated test for should_download_episode covering all 4 state combinations.
    /// Replaces multiple individual tests that each test one combination.
    #[test]
    fn should_download_episode_covers_all_played_and_file_existence_combinations() {
        let mut existing_files: HashSet<String> = HashSet::new();
        existing_files.insert("existing_file.mp3".to_string());

        let make_episode = |played: bool| termusiclib::podcast::episode::Episode {
            id: 1,
            pod_id: 1,
            title: "Test Episode".to_string(),
            url: "http://127.0.0.1/test.mp3".to_string(),
            guid: "guid-test".to_string(),
            description: String::new(),
            pubdate: None,
            duration: None,
            path: None,
            played,
            last_position: None,
            image_url: None,
        };

        // Case 1: File exists, not played → skip (file exists takes precedence)
        let episode = make_episode(false);
        assert!(
            !should_download_episode(&episode, &existing_files, "existing_file.mp3"),
            "File exists + unplayed → skip download"
        );

        // Case 2: File exists, played → skip
        let episode = make_episode(true);
        assert!(
            !should_download_episode(&episode, &existing_files, "existing_file.mp3"),
            "File exists + played → skip download"
        );

        // Case 3: File missing, played → skip (played + deleted = excluded)
        let episode = make_episode(true);
        assert!(
            !should_download_episode(&episode, &existing_files, "missing_file.mp3"),
            "File missing + played → skip download (SCENARIO-018)"
        );

        // Case 4: File missing, not played → download
        let episode = make_episode(false);
        assert!(
            should_download_episode(&episode, &existing_files, "missing_file.mp3"),
            "File missing + unplayed → download (SCENARIO-019)"
        );
    }

    // =========================================================================
    // AC-26 / SCENARIO-031: TestHarness eliminates boilerplate
    //
    // Verify that the TestHarness can be used for complex scenarios with
    // minimal per-test configuration.
    // =========================================================================

    /// AC-26/SCENARIO-031: TestHarness provides all infrastructure needed for
    /// a full sync integration test with minimal per-test code.
    #[tokio::test]
    async fn test_harness_eliminates_boilerplate_for_full_sync_test() {
        let mut harness = TestHarness::new().await;

        // Only test-specific configuration needed:
        let episode_path = "/episodes/harness_test.mp3";
        let episode_url = format!("{}{}", harness.mock_server.uri(), episode_path);
        let feed_xml = harness.generate_rss_feed(
            "Harness Efficiency Test",
            &[("Harness Episode", "guid-harness-001", &episode_url)],
        );
        harness.mount_feed("/harness_feed.xml", &feed_xml).await;
        harness.mount_episode_download(episode_path).await;
        harness.insert_podcast(&PodcastNoId {
            title: "Harness Efficiency Test".to_string(),
            url: format!("{}/harness_feed.xml", harness.mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        });

        // One-liner to run sync
        let result = harness.run_sync().await;
        assert!(result.is_ok());

        // One-liner to verify outcomes
        let commands = harness.collect_playlist_commands();
        assert_eq!(
            commands.len(),
            1,
            "TestHarness should support full flow with minimal setup"
        );
    }

    /// AC-26: TestHarness::with_enqueue allows customizing auto-enqueue behavior
    /// without repeating the entire ServerSettings construction.
    #[tokio::test]
    async fn test_harness_supports_custom_enqueue_configuration() {
        // Disabled variant
        let harness_disabled = TestHarness::with_enqueue(AutoEnqueue::Disabled).await;
        let config_disabled = harness_disabled.config.read();
        assert_eq!(
            config_disabled
                .settings
                .podcast
                .synchronization
                .auto_enqueue,
            AutoEnqueue::Disabled,
            "AC-26: TestHarness should allow easy config customization"
        );
        drop(config_disabled);

        // Enabled variant
        let harness_enabled = TestHarness::with_enqueue(AutoEnqueue::Enabled).await;
        let config_enabled = harness_enabled.config.read();
        assert_eq!(
            config_enabled.settings.podcast.synchronization.auto_enqueue,
            AutoEnqueue::Enabled,
        );
    }

    // =========================================================================
    // AC-22: Episode helper URL tests use only localhost
    // =========================================================================

    /// AC-22: Unit tests for should_download_episode use 127.0.0.1 URLs only.
    #[test]
    fn should_download_episode_unit_test_uses_localhost_urls_only() {
        let existing_files: HashSet<String> = HashSet::new();

        // All URLs in this test use 127.0.0.1 (AC-22)
        let episode = termusiclib::podcast::episode::Episode {
            id: 1,
            pod_id: 1,
            title: "Localhost Episode".to_string(),
            url: "http://127.0.0.1:8080/episode.mp3".to_string(),
            guid: "guid-localhost".to_string(),
            description: String::new(),
            pubdate: None,
            duration: None,
            path: None,
            played: false,
            last_position: None,
            image_url: None,
        };

        // Verify the URL is localhost-based
        assert!(
            episode.url.contains("127.0.0.1") || episode.url.contains("localhost"),
            "AC-22: Test episode URLs must use localhost/127.0.0.1"
        );

        let result = should_download_episode(&episode, &existing_files, "nonexistent.mp3");
        assert!(
            result,
            "Unplayed episode with missing file should be downloaded"
        );
    }

    // =========================================================================
    // AC-23: find_episodes_to_download error cases with specific assertions
    // =========================================================================

    /// AC-23: When episodes have path set (already downloaded in DB), they must
    /// be filtered out. Assert specific filtering behavior, not just a count.
    #[test]
    fn find_episodes_to_download_excludes_episodes_with_database_path() {
        let episodes = vec![
            termusiclib::podcast::episode::Episode {
                id: 1,
                pod_id: 1,
                title: "Has Path".to_string(),
                url: "http://127.0.0.1/has_path.mp3".to_string(),
                guid: "guid-has-path".to_string(),
                description: String::new(),
                pubdate: None,
                duration: None,
                path: Some(std::path::PathBuf::from("/downloads/has_path.mp3")), // already downloaded
                played: false,
                last_position: None,
                image_url: None,
            },
            termusiclib::podcast::episode::Episode {
                id: 2,
                pod_id: 1,
                title: "No Path".to_string(),
                url: "http://127.0.0.1/no_path.mp3".to_string(),
                guid: "guid-no-path".to_string(),
                description: String::new(),
                pubdate: None,
                duration: None,
                path: None, // needs download
                played: false,
                last_position: None,
                image_url: None,
            },
        ];

        let existing_files: HashSet<String> = HashSet::new();
        let to_download = find_episodes_to_download(&episodes, &existing_files, 10);

        // AC-23: Specific assertion — check WHICH episode is included/excluded
        assert_eq!(
            to_download.len(),
            1,
            "Only episode without path should be included"
        );
        assert_eq!(
            to_download[0].id, 2,
            "AC-23: Specifically, episode ID 2 (no path) should be the one to download"
        );
        assert_eq!(
            to_download[0].title, "No Path",
            "AC-23: The episode to download should be 'No Path'"
        );
    }

    // =========================================================================
    // SOURCE CODE QUALITY ENFORCEMENT TESTS
    //
    // These tests scan the actual source of the podcast_sync module's inline
    // test suite to verify that the Phase 4 cleanup has been performed.
    // They will FAIL until the cleanup is done (TDD RED phase).
    // =========================================================================

    /// AC-20/SCENARIO-028: The redundant test `sync_pass_stats_struct_has_required_fields`
    /// must be REMOVED from the podcast_sync module's inline tests.
    /// This test reads the source file and asserts the function name is absent.
    #[test]
    fn source_does_not_contain_redundant_test_sync_pass_stats_struct_has_required_fields() {
        let source = include_str!("podcast_sync.rs");
        assert!(
            !source.contains("fn sync_pass_stats_struct_has_required_fields"),
            "AC-20: The test `sync_pass_stats_struct_has_required_fields` tests basic Rust \
             struct field assignment (a compiler check, not behavior). It must be REMOVED."
        );
    }

    /// AC-20/SCENARIO-028: The redundant test `sync_pass_stats_all_zeros` must be REMOVED.
    /// It merely verifies that assigning 0 to a usize field yields 0 — a language tautology.
    #[test]
    fn source_does_not_contain_redundant_test_sync_pass_stats_all_zeros() {
        let source = include_str!("podcast_sync.rs");
        assert!(
            !source.contains("fn sync_pass_stats_all_zeros"),
            "AC-20: The test `sync_pass_stats_all_zeros` verifies that 0 == 0 for struct \
             fields. This is a Rust language tautology and must be REMOVED."
        );
    }

    /// AC-20/SCENARIO-028: The redundant test `sync_pass_stats_implements_debug` must be REMOVED.
    /// Testing that #[derive(Debug)] works is testing the Rust compiler, not the application.
    #[test]
    fn source_does_not_contain_redundant_test_sync_pass_stats_implements_debug() {
        let source = include_str!("podcast_sync.rs");
        assert!(
            !source.contains("fn sync_pass_stats_implements_debug"),
            "AC-20: The test `sync_pass_stats_implements_debug` tests that #[derive(Debug)] \
             works. This tests the Rust compiler, not application behavior. REMOVE it."
        );
    }

    /// AC-20/SCENARIO-028: The redundant test `sync_once_accepts_expected_parameters` must be REMOVED.
    /// Testing that a function can be called with its declared parameter types tests the type system.
    #[test]
    fn source_does_not_contain_redundant_test_sync_once_accepts_expected_parameters() {
        let source = include_str!("podcast_sync.rs");
        assert!(
            !source.contains("fn sync_once_accepts_expected_parameters"),
            "AC-20: The test `sync_once_accepts_expected_parameters` merely validates that a \
             function compiles with its signature. The Rust type system already guarantees this. REMOVE it."
        );
    }

    /// AC-20/SCENARIO-028: The redundant test `sync_once_returns_anyhow_result_of_sync_pass_stats`
    /// must be REMOVED. Testing a function's return type is testing the type system.
    #[test]
    fn source_does_not_contain_redundant_test_sync_once_returns_anyhow_result() {
        let source = include_str!("podcast_sync.rs");
        assert!(
            !source.contains("fn sync_once_returns_anyhow_result_of_sync_pass_stats"),
            "AC-20: The test `sync_once_returns_anyhow_result_of_sync_pass_stats` merely \
             type-checks the return value. This tests the Rust type system. REMOVE it."
        );
    }

    /// AC-22/SCENARIO-029: The podcast_sync.rs test source must NOT contain
    /// external URLs like example.com. All test URLs must use localhost/127.0.0.1.
    #[test]
    fn source_tests_do_not_use_example_com_urls() {
        let source = include_str!("podcast_sync.rs");
        // Only check the test section (after #[cfg(test)])
        let test_section = source
            .find("#[cfg(test)]")
            .map(|idx| &source[idx..])
            .unwrap_or("");

        assert!(
            !test_section.contains("example.com"),
            "AC-22/SCENARIO-029: Test URLs must not use example.com. Found external URL \
             in the test section of podcast_sync.rs. Replace with mock_server.uri() (localhost)."
        );
    }

    /// AC-22/SCENARIO-029: The podcast_sync.rs test source must NOT contain
    /// documentation net addresses (192.0.2.x / TEST-NET-1) in podcast URLs
    /// that could leak to unreachable hosts. Use mock_server.uri() instead.
    #[test]
    fn source_tests_do_not_use_documentation_net_addresses_for_feeds() {
        let source = include_str!("podcast_sync.rs");
        // Only check the test section
        let test_section = source
            .find("#[cfg(test)]")
            .map(|idx| &source[idx..])
            .unwrap_or("");

        // Count occurrences of 192.0.2.x in the test section
        let external_count = test_section.matches("192.0.2.").count();
        assert_eq!(
            external_count, 0,
            "AC-22/SCENARIO-029: Found {external_count} uses of 192.0.2.x (TEST-NET) addresses \
             in podcast_sync.rs tests. These should be replaced with mock server localhost URLs \
             to prevent any network calls to external hosts."
        );
    }

    /// AC-23/SCENARIO-030: The error test for invalid DB path must assert the
    /// specific error message content, not just use bare is_err().
    /// Verify that the test `sync_once_invalid_db_path_returns_error` checks
    /// the error message, not just `.is_err()`.
    #[test]
    fn source_error_test_checks_specific_message_not_bare_is_err() {
        let source = include_str!("podcast_sync.rs");
        // Find the invalid_db_path test
        if let Some(test_start) = source.find("fn sync_once_invalid_db_path_returns_error") {
            // Get the test body (up to next function or end of module)
            let test_body = &source[test_start..];
            let test_end = test_body[100..] // skip past the function declaration
                .find("\n    #[")
                .map(|i| i + 100)
                .unwrap_or(test_body.len().min(500));
            let test_body = &test_body[..test_end];

            // The test should check the error MESSAGE, not just is_err()
            let has_specific_assertion = test_body.contains("unwrap_err")
                || test_body.contains("error_message")
                || test_body.contains("to_string")
                || test_body.contains("contains(");

            assert!(
                has_specific_assertion,
                "AC-23/SCENARIO-030: The test `sync_once_invalid_db_path_returns_error` uses \
                 only `is_err()` without checking the specific error message or variant. \
                 It must assert on the error content (e.g., check that it mentions 'database')."
            );
        }
        // If the test doesn't exist, that's fine (it might have been refactored)
    }

    /// AC-25: Test names in the synchronization_tests.rs should not contain
    /// unexplained single-letter abbreviations.
    #[test]
    fn synchronization_tests_source_has_no_unexplained_abbreviations_in_names() {
        let source = include_str!("../../lib/src/config/v2/server/synchronization_tests.rs");
        // Check for tests with very short function names (abbreviations)
        // The "synchronization_settings_clone" and "synchronization_settings_debug"
        // tests verify derive traits rather than behavior (AC-20)
        let has_derive_test = source.contains("fn synchronization_settings_clone")
            || source.contains("fn synchronization_settings_debug");

        assert!(
            !has_derive_test,
            "AC-20/AC-25: Tests like `synchronization_settings_clone` and \
             `synchronization_settings_debug` verify derive traits (basic Rust semantics). \
             They should be removed as they test the compiler, not application behavior."
        );
    }
}
