//! SCENARIO-011: Per-podcast scheduling uses individual timestamps
//!
//! This test verifies that `sync_once` calls `get_due_podcasts` (DEF-001)
//! to filter podcasts based on their individual `last_checked` timestamps
//! rather than processing all subscribed podcasts unconditionally.
//!
//! The scenario: podcast A was last checked 30 minutes ago and podcast B was
//! last checked 2 hours ago. With a global sync interval of 1 hour, only
//! podcast B should be included in the sync pass.

#[cfg(test)]
mod scenario_011_per_podcast_scheduling {
    use std::path::Path;
    use std::time::Duration;

    use chrono::Utc;
    use termusiclib::config::v2::server::synchronization::{AutoEnqueue, SynchronizationSettings};
    use termusiclib::config::v2::server::{PodcastSettings, ServerSettings};
    use termusiclib::config::{ServerOverlay, SharedServerSettings, new_shared_server_settings};
    use termusiclib::podcast::PodcastNoId;
    use termusiclib::podcast::db::Database;
    use termusicplayback::{PlayerCmd, PlayerCmdSender};
    use tokio::sync::mpsc::unbounded_channel;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::podcast_sync::sync_once;

    // =========================================================================
    // Helpers
    // =========================================================================

    fn make_config_with_interval(download_dir: &Path, interval_secs: u64) -> SharedServerSettings {
        let settings = ServerSettings {
            podcast: PodcastSettings {
                download_dir: download_dir.to_path_buf(),
                synchronization: SynchronizationSettings {
                    interval: Duration::from_secs(interval_secs),
                    refresh_on_startup: false,
                    max_new_episodes: 5,
                    auto_enqueue: AutoEnqueue::Enabled,
                },
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

    fn generate_rss_feed(title: &str, episodes: &[(&str, &str, &str)]) -> String {
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

    fn fake_audio_content() -> Vec<u8> {
        let mut content = vec![0x49, 0x44, 0x33]; // "ID3" magic bytes
        content.extend_from_slice(&[0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        content.extend_from_slice(&[0xFF; 1024]);
        content
    }

    // =========================================================================
    // SCENARIO-011: Per-podcast scheduling uses individual timestamps
    //
    // Given podcast A was last checked 30 minutes ago and podcast B was last
    // checked 2 hours ago, and the global sync interval is 1 hour:
    // When the periodic sync evaluator runs,
    // Then podcast B is included in the sync pass,
    // And podcast A is skipped until its next eligible check time.
    //
    // This test FAILS because sync_once currently calls get_podcasts() (which
    // returns ALL podcasts) instead of get_due_podcasts() (which filters by
    // last_checked timestamps). DEF-001 requires get_due_podcasts to be used.
    // =========================================================================

    /// SCENARIO-011: sync_once must use per-podcast scheduling to skip podcast A
    /// (checked 30min ago, within 1h interval) and only process podcast B
    /// (checked 2h ago, past 1h interval).
    ///
    /// This test verifies that sync_once calls get_due_podcasts (DEF-001) to
    /// filter podcasts by individual last_checked timestamps. The test fails if
    /// sync_once fetches ALL podcasts regardless of scheduling.
    #[tokio::test]
    async fn sync_once_uses_get_due_podcasts_to_filter_by_individual_timestamps() {
        let mock_server = MockServer::start().await;

        // Podcast A's feed — should NOT be fetched (checked 30 min ago, within 1h interval)
        let feed_a_xml = generate_rss_feed(
            "Podcast A Recently Checked",
            &[(
                "A Episode",
                "guid-a-recent",
                &format!("{}/episodes/a_recent.mp3", mock_server.uri()),
            )],
        );
        Mock::given(method("GET"))
            .and(path("/feed_a_recent.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(feed_a_xml)
                    .insert_header("content-type", "application/rss+xml"),
            )
            .expect(0) // MUST NOT be called — podcast A is not due
            .named("Feed A (should NOT be fetched)")
            .mount(&mock_server)
            .await;

        // Podcast B's feed — SHOULD be fetched (checked 2h ago, past 1h interval)
        let feed_b_xml = generate_rss_feed(
            "Podcast B Overdue",
            &[(
                "B Episode",
                "guid-b-overdue",
                &format!("{}/episodes/b_overdue.mp3", mock_server.uri()),
            )],
        );
        Mock::given(method("GET"))
            .and(path("/feed_b_overdue.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(feed_b_xml)
                    .insert_header("content-type", "application/rss+xml"),
            )
            .expect(1) // MUST be called exactly once — podcast B is overdue
            .named("Feed B (should be fetched)")
            .mount(&mock_server)
            .await;

        // Episode download mock for podcast B
        Mock::given(method("GET"))
            .and(path("/episodes/b_overdue.mp3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(fake_audio_content())
                    .insert_header("content-type", "audio/mpeg"),
            )
            .mount(&mock_server)
            .await;

        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp_dir.path();
        let download_dir = tmp_dir.path().join("downloads");
        std::fs::create_dir_all(&download_dir).expect("create download dir");

        // Insert both podcasts with different last_checked times
        let db = Database::new(db_path).expect("create database");

        // Podcast A: last checked 30 minutes ago (within 1-hour interval → NOT due)
        let podcast_a = PodcastNoId {
            title: "Podcast A Recently Checked".to_string(),
            url: format!("{}/feed_a_recent.xml", mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::minutes(30),
            episodes: vec![],
            image_url: None,
        };
        db.insert_podcast(&podcast_a).expect("insert podcast A");

        // Podcast B: last checked 2 hours ago (exceeds 1-hour interval → DUE)
        let podcast_b = PodcastNoId {
            title: "Podcast B Overdue".to_string(),
            url: format!("{}/feed_b_overdue.xml", mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(2),
            episodes: vec![],
            image_url: None,
        };
        db.insert_podcast(&podcast_b).expect("insert podcast B");
        drop(db);

        // Global interval = 1 hour (3600 seconds)
        let config = make_config_with_interval(&download_dir, 3600);
        let (cmd_tx, _cmd_rx) = make_cmd_channel();

        let result = sync_once(&config, &cmd_tx, db_path).await;
        assert!(
            result.is_ok(),
            "sync_once should succeed: {:?}",
            result.err()
        );
        let stats = result.unwrap();

        // SCENARIO-011 assertion: Only podcast B should be checked.
        // Podcast A was checked 30 minutes ago (within the 1-hour interval) and
        // should be SKIPPED by get_due_podcasts filtering.
        assert_eq!(
            stats.podcasts_checked, 1,
            "SCENARIO-011/DEF-001: sync_once must call get_due_podcasts to filter \
             by individual timestamps. Only podcast B (checked 2h ago) should be \
             processed. Podcast A (checked 30min ago) should be skipped. \
             Got podcasts_checked={}, expected 1.",
            stats.podcasts_checked
        );

        // Podcast B should have been successfully processed (not failed)
        assert_eq!(
            stats.podcasts_failed, 0,
            "Podcast B should succeed (valid mock feed)"
        );

        // Podcast B had one new episode that should be downloaded
        assert_eq!(
            stats.episodes_downloaded, 1,
            "Podcast B's episode should be downloaded"
        );
    }

    /// SCENARIO-011 (complementary): When BOTH podcasts are overdue, both should
    /// be processed. This verifies that get_due_podcasts does not inadvertently
    /// exclude podcasts that ARE due.
    #[tokio::test]
    async fn sync_once_processes_all_overdue_podcasts_via_get_due_podcasts() {
        let mock_server = MockServer::start().await;

        // Both podcasts have feeds and episodes
        let feed_a_xml = generate_rss_feed(
            "Overdue Podcast A",
            &[(
                "A Episode",
                "guid-both-a",
                &format!("{}/episodes/both_a.mp3", mock_server.uri()),
            )],
        );
        let feed_b_xml = generate_rss_feed(
            "Overdue Podcast B",
            &[(
                "B Episode",
                "guid-both-b",
                &format!("{}/episodes/both_b.mp3", mock_server.uri()),
            )],
        );

        Mock::given(method("GET"))
            .and(path("/feed_both_a.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(feed_a_xml)
                    .insert_header("content-type", "application/rss+xml"),
            )
            .expect(1) // Both should be fetched
            .named("Feed A (overdue, should be fetched)")
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/feed_both_b.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(feed_b_xml)
                    .insert_header("content-type", "application/rss+xml"),
            )
            .expect(1) // Both should be fetched
            .named("Feed B (overdue, should be fetched)")
            .mount(&mock_server)
            .await;

        for ep in ["/episodes/both_a.mp3", "/episodes/both_b.mp3"] {
            Mock::given(method("GET"))
                .and(path(ep))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(fake_audio_content())
                        .insert_header("content-type", "audio/mpeg"),
                )
                .mount(&mock_server)
                .await;
        }

        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp_dir.path();
        let download_dir = tmp_dir.path().join("downloads");
        std::fs::create_dir_all(&download_dir).expect("create download dir");

        let db = Database::new(db_path).expect("create database");

        // Both podcasts checked 3 hours ago (both exceed 1-hour interval → both DUE)
        let podcast_a = PodcastNoId {
            title: "Overdue Podcast A".to_string(),
            url: format!("{}/feed_both_a.xml", mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(3),
            episodes: vec![],
            image_url: None,
        };
        let podcast_b = PodcastNoId {
            title: "Overdue Podcast B".to_string(),
            url: format!("{}/feed_both_b.xml", mock_server.uri()),
            description: None,
            author: None,
            explicit: None,
            last_checked: Utc::now() - chrono::Duration::hours(5),
            episodes: vec![],
            image_url: None,
        };
        db.insert_podcast(&podcast_a).expect("insert podcast A");
        db.insert_podcast(&podcast_b).expect("insert podcast B");
        drop(db);

        // Global interval = 1 hour
        let config = make_config_with_interval(&download_dir, 3600);
        let (cmd_tx, _cmd_rx) = make_cmd_channel();

        let result = sync_once(&config, &cmd_tx, db_path).await;
        assert!(
            result.is_ok(),
            "sync_once should succeed: {:?}",
            result.err()
        );
        let stats = result.unwrap();

        // Both podcasts are overdue, so both should be processed
        assert_eq!(
            stats.podcasts_checked, 2,
            "SCENARIO-011: Both overdue podcasts should be processed via get_due_podcasts. \
             Got podcasts_checked={}, expected 2.",
            stats.podcasts_checked
        );
        assert_eq!(stats.podcasts_failed, 0);
        assert_eq!(
            stats.episodes_downloaded, 2,
            "Both episodes should be downloaded"
        );
    }
}
