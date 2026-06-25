//! Phase 2 tests for UpdatePodcastSyncEvents and protobuf integration.
//!
//! These tests validate:
//! - T-22/T-23/T-24: UpdatePodcastSyncEvents enum and protobuf conversion
//! - AC-08: Progress reporting for podcast sync operations
//! - SCENARIO-005: Server reports sync progress via StreamUpdates
//!
//! The UpdatePodcastSyncEvents enum and its From impls are added in Phase 2
//! to support streaming sync progress from server to TUI.

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::player::{
        PodcastSyncCompleteStats, StreamUpdates, UpdateEvents, UpdatePodcastSyncEvents,
    };

    // =========================================================================
    // T-23: UpdatePodcastSyncEvents enum exists with correct variants
    // =========================================================================

    /// UpdatePodcastSyncEvents::Started should contain total_podcasts count.
    #[test]
    fn update_podcast_sync_events_started_variant() {
        let event = UpdatePodcastSyncEvents::Started { total_podcasts: 5 };

        if let UpdatePodcastSyncEvents::Started { total_podcasts } = event {
            assert_eq!(total_podcasts, 5);
        } else {
            panic!("expected Started variant");
        }
    }

    /// UpdatePodcastSyncEvents::Progress should contain podcast_title and counts.
    #[test]
    fn update_podcast_sync_events_progress_variant() {
        let event = UpdatePodcastSyncEvents::Progress {
            podcast_title: "My Podcast".to_string(),
            episodes_found: 10,
            episodes_downloaded: 3,
        };

        if let UpdatePodcastSyncEvents::Progress {
            podcast_title,
            episodes_found,
            episodes_downloaded,
        } = event
        {
            assert_eq!(podcast_title, "My Podcast");
            assert_eq!(episodes_found, 10);
            assert_eq!(episodes_downloaded, 3);
        } else {
            panic!("expected Progress variant");
        }
    }

    /// UpdatePodcastSyncEvents::Complete should contain aggregate statistics.
    #[test]
    fn update_podcast_sync_events_complete_variant() {
        let stats = PodcastSyncCompleteStats {
            podcasts_checked: 10,
            podcasts_failed: 1,
            episodes_downloaded: 25,
            episodes_enqueued: 20,
        };
        let event = UpdatePodcastSyncEvents::Complete(stats.clone());

        if let UpdatePodcastSyncEvents::Complete(s) = event {
            assert_eq!(s.podcasts_checked, 10);
            assert_eq!(s.podcasts_failed, 1);
            assert_eq!(s.episodes_downloaded, 25);
            assert_eq!(s.episodes_enqueued, 20);
        } else {
            panic!("expected Complete variant");
        }
    }

    /// UpdatePodcastSyncEvents::Error should contain podcast_title and error_message.
    #[test]
    fn update_podcast_sync_events_error_variant() {
        let event = UpdatePodcastSyncEvents::Error {
            podcast_title: "Broken Feed".to_string(),
            error_message: "connection timeout".to_string(),
        };

        if let UpdatePodcastSyncEvents::Error {
            podcast_title,
            error_message,
        } = event
        {
            assert_eq!(podcast_title, "Broken Feed");
            assert_eq!(error_message, "connection timeout");
        } else {
            panic!("expected Error variant");
        }
    }

    // =========================================================================
    // T-24: PodcastSync variant in UpdateEvents enum
    // =========================================================================

    /// UpdateEvents should have a PodcastSync variant wrapping UpdatePodcastSyncEvents.
    #[test]
    fn update_events_has_podcast_sync_variant() {
        let sync_event = UpdatePodcastSyncEvents::Started { total_podcasts: 3 };
        let event = UpdateEvents::PodcastSync(sync_event);

        if let UpdateEvents::PodcastSync(inner) = event {
            if let UpdatePodcastSyncEvents::Started { total_podcasts } = inner {
                assert_eq!(total_podcasts, 3);
            } else {
                panic!("expected Started inner variant");
            }
        } else {
            panic!("expected PodcastSync outer variant");
        }
    }

    // =========================================================================
    // T-24: From<UpdateEvents> for protobuf::StreamUpdates (PodcastSync path)
    // =========================================================================

    /// Converting UpdateEvents::PodcastSync(Started) to protobuf should not panic.
    #[test]
    fn update_events_podcast_sync_started_converts_to_protobuf() {
        let event =
            UpdateEvents::PodcastSync(UpdatePodcastSyncEvents::Started { total_podcasts: 7 });

        let proto: StreamUpdates = event.into();
        // The protobuf should have the podcast_sync field set (field 9)
        assert!(
            proto.r#type.is_some(),
            "protobuf StreamUpdates should have type set"
        );
    }

    /// Converting UpdateEvents::PodcastSync(Complete) to protobuf should preserve stats.
    #[test]
    fn update_events_podcast_sync_complete_converts_to_protobuf() {
        let stats = PodcastSyncCompleteStats {
            podcasts_checked: 5,
            podcasts_failed: 0,
            episodes_downloaded: 12,
            episodes_enqueued: 12,
        };
        let event = UpdateEvents::PodcastSync(UpdatePodcastSyncEvents::Complete(stats));

        let proto: StreamUpdates = event.into();
        assert!(proto.r#type.is_some());
    }

    /// Converting UpdateEvents::PodcastSync(Error) to protobuf should work.
    #[test]
    fn update_events_podcast_sync_error_converts_to_protobuf() {
        let event = UpdateEvents::PodcastSync(UpdatePodcastSyncEvents::Error {
            podcast_title: "Test Pod".to_string(),
            error_message: "feed parse error".to_string(),
        });

        let proto: StreamUpdates = event.into();
        assert!(proto.r#type.is_some());
    }

    /// Roundtrip: UpdateEvents::PodcastSync -> protobuf -> UpdateEvents should preserve data.
    #[test]
    fn update_events_podcast_sync_protobuf_roundtrip() {
        let original = UpdateEvents::PodcastSync(UpdatePodcastSyncEvents::Complete(
            PodcastSyncCompleteStats {
                podcasts_checked: 3,
                podcasts_failed: 1,
                episodes_downloaded: 8,
                episodes_enqueued: 6,
            },
        ));

        let proto: StreamUpdates = original.clone().into();
        let roundtripped: UpdateEvents =
            proto.try_into().expect("should convert back from protobuf");

        assert_eq!(original, roundtripped);
    }

    // =========================================================================
    // PodcastSyncCompleteStats struct validation
    // =========================================================================

    /// PodcastSyncCompleteStats should implement Clone, PartialEq, and Eq.
    #[test]
    fn podcast_sync_complete_stats_clone_and_eq() {
        let stats = PodcastSyncCompleteStats {
            podcasts_checked: 10,
            podcasts_failed: 2,
            episodes_downloaded: 30,
            episodes_enqueued: 28,
        };

        let cloned = stats.clone();
        assert_eq!(stats, cloned);
    }
}
