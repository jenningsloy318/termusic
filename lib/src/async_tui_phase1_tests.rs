//! Phase 1 tests for Async TUI Playlist Loading — Protocol Extension and Domain Struct Updates.
//!
//! These tests validate:
//! - T-09/T-10/T-11: Track::from_grpc_metadata constructor for all source variants
//! - T-13/T-14/T-15: Unit tests for Track::from_grpc_metadata behavior
//! - T-05/T-06/T-07: PlaylistAddTrackInfo new fields and serialization round-trip
//! - T-17: PlaylistAddTrackInfo serialization round-trip with artist, album, has_local_file
//!
//! AC References:
//! - AC-06: Protobuf extended with artist, album, has_local_file (backward wire compatibility)
//! - AC-04: TUI constructs Track from gRPC-provided metadata without disk I/O
//! - AC-08: Graceful fallback when metadata is absent
//!
//! BDD Scenario References:
//! - SCENARIO-014: Protobuf message includes artist and album with backward wire compatibility
//! - SCENARIO-010: TUI constructs track objects directly from server-provided metadata
//! - SCENARIO-017: TUI displays filename fallback when metadata is absent

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use crate::player::playlist_helpers::PlaylistTrackSource;
    use crate::player::{PlaylistAddTrackInfo, UpdatePlaylistEvents};
    use crate::track::Track;

    // =========================================================================
    // T-09: Track::from_grpc_metadata — Path source variant
    // SCENARIO-010: TUI constructs track objects directly from server-provided metadata
    // =========================================================================

    /// Track::from_grpc_metadata with Path source should create a MediaTypes::Track
    /// containing the correct path and album.
    #[test]
    fn from_grpc_metadata_path_creates_track_variant() {
        let source = PlaylistTrackSource::Path("/home/user/music/song.mp3".to_string());
        let title = Some("My Song".to_string());
        let artist = Some("My Artist".to_string());
        let album = Some("My Album".to_string());
        let duration = Some(Duration::from_secs(240));

        let track = Track::from_grpc_metadata(source, title, artist, album, duration, false);

        // Should be a Track variant (local file)
        assert!(
            track.as_track().is_some(),
            "Expected MediaTypes::Track variant"
        );
        assert!(track.as_radio().is_none());
        assert!(track.as_podcast().is_none());
    }

    /// Track::from_grpc_metadata with Path source should store the path correctly.
    #[test]
    fn from_grpc_metadata_path_stores_path() {
        let source = PlaylistTrackSource::Path("/home/user/music/song.mp3".to_string());

        let track =
            Track::from_grpc_metadata(source, Some("Title".to_string()), None, None, None, false);

        let track_data = track.as_track().expect("Expected Track variant");
        assert_eq!(
            track_data.path(),
            PathBuf::from("/home/user/music/song.mp3")
        );
    }

    /// Track::from_grpc_metadata with Path source should store album in TrackData.
    #[test]
    fn from_grpc_metadata_path_stores_album() {
        let source = PlaylistTrackSource::Path("/music/song.flac".to_string());
        let album = Some("Greatest Hits".to_string());

        let track = Track::from_grpc_metadata(source, None, None, album, None, false);

        let track_data = track.as_track().expect("Expected Track variant");
        assert_eq!(track_data.album(), Some("Greatest Hits"));
    }

    /// Track::from_grpc_metadata with Path source should store title correctly.
    #[test]
    fn from_grpc_metadata_path_stores_title() {
        let source = PlaylistTrackSource::Path("/music/song.flac".to_string());
        let title = Some("Beautiful Day".to_string());

        let track = Track::from_grpc_metadata(source, title, None, None, None, false);

        assert_eq!(track.title(), Some("Beautiful Day"));
    }

    /// Track::from_grpc_metadata with Path source should store artist correctly.
    #[test]
    fn from_grpc_metadata_path_stores_artist() {
        let source = PlaylistTrackSource::Path("/music/song.flac".to_string());
        let artist = Some("The Band".to_string());

        let track = Track::from_grpc_metadata(source, None, artist, None, None, false);

        assert_eq!(track.artist(), Some("The Band"));
    }

    /// Track::from_grpc_metadata with Path source should store duration correctly.
    #[test]
    fn from_grpc_metadata_path_stores_duration() {
        let source = PlaylistTrackSource::Path("/music/song.flac".to_string());
        let duration = Some(Duration::from_secs(180));

        let track = Track::from_grpc_metadata(source, None, None, None, duration, false);

        assert_eq!(track.duration(), Some(Duration::from_secs(180)));
    }

    /// Track::from_grpc_metadata with Path source and all fields populated stores everything.
    #[test]
    fn from_grpc_metadata_path_all_fields_populated() {
        let source = PlaylistTrackSource::Path("/home/user/Bohemian Rhapsody.mp3".to_string());
        let title = Some("Bohemian Rhapsody".to_string());
        let artist = Some("Queen".to_string());
        let album = Some("A Night at the Opera".to_string());
        let duration = Some(Duration::from_secs(354));

        let track = Track::from_grpc_metadata(source, title, artist, album, duration, false);

        assert_eq!(track.title(), Some("Bohemian Rhapsody"));
        assert_eq!(track.artist(), Some("Queen"));
        assert_eq!(
            track.as_track().unwrap().album(),
            Some("A Night at the Opera")
        );
        assert_eq!(track.duration(), Some(Duration::from_secs(354)));
        assert_eq!(
            track.as_track().unwrap().path(),
            PathBuf::from("/home/user/Bohemian Rhapsody.mp3")
        );
    }

    // =========================================================================
    // T-10: Track::from_grpc_metadata — Url source variant (Radio)
    // =========================================================================

    /// Track::from_grpc_metadata with Url source should create a MediaTypes::Radio variant.
    #[test]
    fn from_grpc_metadata_url_creates_radio_variant() {
        let source = PlaylistTrackSource::Url("http://stream.example.com/radio.mp3".to_string());

        let track = Track::from_grpc_metadata(source, None, None, None, None, false);

        assert!(
            track.as_radio().is_some(),
            "Expected MediaTypes::Radio variant"
        );
        assert!(track.as_track().is_none());
        assert!(track.as_podcast().is_none());
    }

    /// Track::from_grpc_metadata with Url source should store the radio URL.
    #[test]
    fn from_grpc_metadata_url_stores_url() {
        let source = PlaylistTrackSource::Url("http://stream.example.com/radio.mp3".to_string());

        let track = Track::from_grpc_metadata(source, None, None, None, None, false);

        let radio_data = track.as_radio().expect("Expected Radio variant");
        assert_eq!(radio_data.url(), "http://stream.example.com/radio.mp3");
    }

    /// Track::from_grpc_metadata with Url source should store title for radio.
    #[test]
    fn from_grpc_metadata_url_stores_title() {
        let source = PlaylistTrackSource::Url("http://radio.example.com/jazz".to_string());
        let title = Some("Jazz FM".to_string());

        let track = Track::from_grpc_metadata(source, title, None, None, None, false);

        assert_eq!(track.title(), Some("Jazz FM"));
    }

    // =========================================================================
    // T-11: Track::from_grpc_metadata — PodcastUrl source variant
    // T-14: PodcastUrl variant with has_local_file sentinel logic
    // =========================================================================

    /// Track::from_grpc_metadata with PodcastUrl source should create a MediaTypes::Podcast variant.
    #[test]
    fn from_grpc_metadata_podcast_creates_podcast_variant() {
        let source =
            PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep1.mp3".to_string());

        let track = Track::from_grpc_metadata(source, None, None, None, None, false);

        assert!(
            track.as_podcast().is_some(),
            "Expected MediaTypes::Podcast variant"
        );
        assert!(track.as_track().is_none());
        assert!(track.as_radio().is_none());
    }

    /// Track::from_grpc_metadata with PodcastUrl source should store the URL.
    #[test]
    fn from_grpc_metadata_podcast_stores_url() {
        let source =
            PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep1.mp3".to_string());

        let track = Track::from_grpc_metadata(source, None, None, None, None, false);

        let podcast_data = track.as_podcast().expect("Expected Podcast variant");
        assert_eq!(podcast_data.url(), "http://podcast.example.com/ep1.mp3");
    }

    /// Track::from_grpc_metadata with PodcastUrl and has_local_file=true should produce
    /// a PodcastTrackData with localfile=Some (sentinel PathBuf).
    #[test]
    fn from_grpc_metadata_podcast_has_local_file_true_produces_sentinel() {
        let source =
            PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep2.mp3".to_string());

        let track = Track::from_grpc_metadata(
            source,
            Some("Episode 2".to_string()),
            None,
            None,
            Some(Duration::from_secs(3600)),
            true, // has_local_file
        );

        let podcast_data = track.as_podcast().expect("Expected Podcast variant");
        assert!(
            podcast_data.has_localfile(),
            "Expected has_localfile() to be true when has_local_file=true"
        );
        // The sentinel should be Some(PathBuf::new()) — an empty path
        assert_eq!(podcast_data.localfile(), Some(PathBuf::new().as_path()));
    }

    /// Track::from_grpc_metadata with PodcastUrl and has_local_file=false should produce
    /// a PodcastTrackData with localfile=None.
    #[test]
    fn from_grpc_metadata_podcast_has_local_file_false_produces_none() {
        let source =
            PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep3.mp3".to_string());

        let track = Track::from_grpc_metadata(source, None, None, None, None, false);

        let podcast_data = track.as_podcast().expect("Expected Podcast variant");
        assert!(
            !podcast_data.has_localfile(),
            "Expected has_localfile() to be false when has_local_file=false"
        );
        assert_eq!(podcast_data.localfile(), None);
    }

    /// Track::from_grpc_metadata with PodcastUrl stores title and duration correctly.
    #[test]
    fn from_grpc_metadata_podcast_stores_metadata() {
        let source =
            PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep4.mp3".to_string());
        let title = Some("Episode 4: The Finale".to_string());
        let duration = Some(Duration::from_secs(2700));

        let track = Track::from_grpc_metadata(source, title, None, None, duration, false);

        assert_eq!(track.title(), Some("Episode 4: The Finale"));
        assert_eq!(track.duration(), Some(Duration::from_secs(2700)));
    }

    // =========================================================================
    // T-15: Track::from_grpc_metadata — None metadata fields
    // SCENARIO-017: TUI displays filename fallback when metadata is absent
    // =========================================================================

    /// Track::from_grpc_metadata with all metadata None should still create a valid Track.
    #[test]
    fn from_grpc_metadata_all_metadata_none() {
        let source = PlaylistTrackSource::Path("/music/unknown_track.ogg".to_string());

        let track = Track::from_grpc_metadata(source, None, None, None, None, false);

        assert_eq!(track.title(), None);
        assert_eq!(track.artist(), None);
        assert_eq!(track.duration(), None);
        assert!(track.as_track().is_some());
        assert_eq!(track.as_track().unwrap().album(), None);
    }

    /// Track::from_grpc_metadata with None album for Path source should store None in TrackData.
    #[test]
    fn from_grpc_metadata_path_none_album() {
        let source = PlaylistTrackSource::Path("/music/single.mp3".to_string());

        let track = Track::from_grpc_metadata(
            source,
            Some("Single Track".to_string()),
            Some("Solo Artist".to_string()),
            None, // no album
            Some(Duration::from_secs(200)),
            false,
        );

        assert_eq!(track.title(), Some("Single Track"));
        assert_eq!(track.artist(), Some("Solo Artist"));
        assert_eq!(track.as_track().unwrap().album(), None);
    }

    /// Track::from_grpc_metadata with empty string title should store Some("").
    #[test]
    fn from_grpc_metadata_empty_string_title() {
        let source = PlaylistTrackSource::Path("/music/track.mp3".to_string());

        let track = Track::from_grpc_metadata(source, Some(String::new()), None, None, None, false);

        // An empty string is still Some(""), not None
        assert_eq!(track.title(), Some(""));
    }

    // =========================================================================
    // T-13: Track::from_grpc_metadata uses file_type: None
    // (file_type is deferred — not transmitted over gRPC)
    // =========================================================================

    /// Track::from_grpc_metadata should set file_type to None (it's inferred from extension if needed).
    #[test]
    fn from_grpc_metadata_path_file_type_is_none() {
        let source = PlaylistTrackSource::Path("/music/song.flac".to_string());

        let track = Track::from_grpc_metadata(
            source,
            Some("Song".to_string()),
            None,
            None,
            Some(Duration::from_secs(300)),
            false,
        );

        let track_data = track.as_track().expect("Expected Track variant");
        assert_eq!(
            track_data.file_type(),
            None,
            "file_type should be None for gRPC-constructed tracks"
        );
    }

    // =========================================================================
    // T-05: PlaylistAddTrackInfo domain struct extended with artist, album, has_local_file
    // =========================================================================

    /// PlaylistAddTrackInfo should have artist, album, and has_local_file fields.
    #[test]
    fn playlist_add_track_info_has_new_fields() {
        let info = PlaylistAddTrackInfo {
            at_index: 0,
            title: Some("Track Title".to_string()),
            artist: Some("Track Artist".to_string()),
            album: Some("Track Album".to_string()),
            duration: Duration::from_secs(180),
            trackid: PlaylistTrackSource::Path("/music/track.mp3".to_string()),
            has_local_file: false,
        };

        assert_eq!(info.artist, Some("Track Artist".to_string()));
        assert_eq!(info.album, Some("Track Album".to_string()));
        assert!(!info.has_local_file);
    }

    /// PlaylistAddTrackInfo with has_local_file=true for podcast tracks.
    #[test]
    fn playlist_add_track_info_podcast_with_local_file() {
        let info = PlaylistAddTrackInfo {
            at_index: 5,
            title: Some("Podcast Episode".to_string()),
            artist: None,
            album: None,
            duration: Duration::from_secs(3600),
            trackid: PlaylistTrackSource::PodcastUrl(
                "http://podcast.example.com/ep.mp3".to_string(),
            ),
            has_local_file: true,
        };

        assert!(info.has_local_file);
        assert_eq!(info.title, Some("Podcast Episode".to_string()));
    }

    /// PlaylistAddTrackInfo with all optional fields as None.
    #[test]
    fn playlist_add_track_info_all_optional_none() {
        let info = PlaylistAddTrackInfo {
            at_index: 10,
            title: None,
            artist: None,
            album: None,
            duration: Duration::from_secs(0),
            trackid: PlaylistTrackSource::Path("/music/unknown.mp3".to_string()),
            has_local_file: false,
        };

        assert_eq!(info.artist, None);
        assert_eq!(info.album, None);
        assert_eq!(info.title, None);
    }

    // =========================================================================
    // T-06/T-07/T-17: Serialization round-trip for PlaylistAddTrackInfo with new fields
    // SCENARIO-014: Protobuf backward wire compatibility
    // =========================================================================

    /// PlaylistAddTrackInfo with artist, album, has_local_file should round-trip
    /// correctly through protobuf serialization (From -> TryFrom).
    #[test]
    fn playlist_add_track_info_roundtrip_with_artist_album() {
        let original = PlaylistAddTrackInfo {
            at_index: 3,
            title: Some("Roundtrip Song".to_string()),
            artist: Some("Roundtrip Artist".to_string()),
            album: Some("Roundtrip Album".to_string()),
            duration: Duration::from_secs(210),
            trackid: PlaylistTrackSource::Path("/music/roundtrip.mp3".to_string()),
            has_local_file: false,
        };

        // Serialize to protobuf
        let proto: crate::player::UpdatePlaylist =
            UpdatePlaylistEvents::PlaylistAddTrack(original.clone()).into();

        // Deserialize back
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("Should successfully deserialize");

        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert_eq!(info.at_index, original.at_index);
                assert_eq!(info.title, original.title);
                assert_eq!(info.artist, original.artist);
                assert_eq!(info.album, original.album);
                assert_eq!(info.duration, original.duration);
                assert_eq!(info.trackid, original.trackid);
                assert_eq!(info.has_local_file, original.has_local_file);
            }
            other => panic!("Expected PlaylistAddTrack, got {other:?}"),
        }
    }

    /// PlaylistAddTrackInfo round-trip with None artist and album preserves None.
    #[test]
    fn playlist_add_track_info_roundtrip_none_fields() {
        let original = PlaylistAddTrackInfo {
            at_index: 0,
            title: None,
            artist: None,
            album: None,
            duration: Duration::from_secs(60),
            trackid: PlaylistTrackSource::Url("http://radio.example.com/stream".to_string()),
            has_local_file: false,
        };

        let proto: crate::player::UpdatePlaylist =
            UpdatePlaylistEvents::PlaylistAddTrack(original.clone()).into();
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("Should successfully deserialize");

        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert_eq!(info.artist, None);
                assert_eq!(info.album, None);
                assert_eq!(info.title, None);
                assert!(!info.has_local_file);
            }
            other => panic!("Expected PlaylistAddTrack, got {other:?}"),
        }
    }

    /// PlaylistAddTrackInfo round-trip with has_local_file=true for podcast.
    #[test]
    fn playlist_add_track_info_roundtrip_has_local_file_true() {
        let original = PlaylistAddTrackInfo {
            at_index: 7,
            title: Some("Downloaded Episode".to_string()),
            artist: None,
            album: None,
            duration: Duration::from_secs(1800),
            trackid: PlaylistTrackSource::PodcastUrl(
                "http://podcast.example.com/episode7.mp3".to_string(),
            ),
            has_local_file: true,
        };

        let proto: crate::player::UpdatePlaylist =
            UpdatePlaylistEvents::PlaylistAddTrack(original.clone()).into();
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("Should successfully deserialize");

        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert!(
                    info.has_local_file,
                    "has_local_file should round-trip as true"
                );
                assert_eq!(info.title, Some("Downloaded Episode".to_string()));
                assert_eq!(info.trackid, original.trackid);
            }
            other => panic!("Expected PlaylistAddTrack, got {other:?}"),
        }
    }

    /// PlaylistAddTrackInfo round-trip with only artist (no album) preserves correctly.
    #[test]
    fn playlist_add_track_info_roundtrip_artist_only() {
        let original = PlaylistAddTrackInfo {
            at_index: 2,
            title: Some("Single".to_string()),
            artist: Some("Solo Singer".to_string()),
            album: None,
            duration: Duration::from_secs(195),
            trackid: PlaylistTrackSource::Path("/music/single.mp3".to_string()),
            has_local_file: false,
        };

        let proto: crate::player::UpdatePlaylist =
            UpdatePlaylistEvents::PlaylistAddTrack(original.clone()).into();
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("Should successfully deserialize");

        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert_eq!(info.artist, Some("Solo Singer".to_string()));
                assert_eq!(info.album, None);
            }
            other => panic!("Expected PlaylistAddTrack, got {other:?}"),
        }
    }

    // =========================================================================
    // Proto field verification: artist, album, has_local_file exist in generated proto
    // AC-06: backward wire compatibility (new optional fields 5, 6, 7)
    // =========================================================================

    /// The protobuf PlaylistAddTrack message should have an artist field (field 5).
    #[test]
    fn proto_playlist_add_track_has_artist_field() {
        let proto_msg = crate::player::PlaylistAddTrack {
            at_index: 0,
            optional_title: None,
            duration: None,
            id: None,
            artist: Some("Test Artist".to_string()),
            album: None,
            has_local_file: None,
        };

        assert_eq!(proto_msg.artist, Some("Test Artist".to_string()));
    }

    /// The protobuf PlaylistAddTrack message should have an album field (field 6).
    #[test]
    fn proto_playlist_add_track_has_album_field() {
        let proto_msg = crate::player::PlaylistAddTrack {
            at_index: 0,
            optional_title: None,
            duration: None,
            id: None,
            artist: None,
            album: Some("Test Album".to_string()),
            has_local_file: None,
        };

        assert_eq!(proto_msg.album, Some("Test Album".to_string()));
    }

    /// The protobuf PlaylistAddTrack message should have a has_local_file field (field 7).
    #[test]
    fn proto_playlist_add_track_has_local_file_field() {
        let proto_msg = crate::player::PlaylistAddTrack {
            at_index: 0,
            optional_title: None,
            duration: None,
            id: None,
            artist: None,
            album: None,
            has_local_file: Some(true),
        };

        assert_eq!(proto_msg.has_local_file, Some(true));
    }

    /// Protobuf PlaylistAddTrack with all new fields as None should remain valid
    /// (backward wire compatibility — older readers ignore unknown fields).
    #[test]
    fn proto_playlist_add_track_new_fields_optional() {
        let proto_msg = crate::player::PlaylistAddTrack {
            at_index: 5,
            optional_title: Some(crate::player::playlist_add_track::OptionalTitle::Title(
                "Song".to_string(),
            )),
            duration: Some(crate::player::Duration {
                secs: 120,
                nanos: 0,
            }),
            id: Some(crate::player::TrackId {
                source: Some(crate::player::track_id::Source::Path(
                    "/music/song.mp3".to_string(),
                )),
            }),
            artist: None,
            album: None,
            has_local_file: None,
        };

        // Old-style message (without new fields) should still be fully valid
        assert_eq!(proto_msg.at_index, 5);
        assert!(proto_msg.artist.is_none());
        assert!(proto_msg.album.is_none());
        assert!(proto_msg.has_local_file.is_none());
    }
}
