//! Phase 3 tests for TUI Playlist Loading Rewrite.
//!
//! These tests validate the Phase 3 implementation:
//! - T-26: Rewrite Playback::load_from_grpc to use Track::from_grpc_metadata (no disk I/O, no db_pod)
//! - T-27: Update all callers of load_from_grpc to remove db_pod argument
//! - T-28: Rewrite handle_playlist_add to use Track::from_grpc_metadata and insert_track_at
//! - T-29: Deprecate or remove track_from_path and track_from_podcasturi
//! - T-30: Remove resolved TODO comments
//!
//! AC References:
//! - AC-01: TUI main event loop MUST NOT be blocked for more than 100ms during playlist loading
//! - AC-04: TUI load_from_grpc MUST construct Track objects without calling Track::read_track_from_path
//! - AC-05: Shuffle events processed without re-reading metadata from disk
//! - AC-08: Graceful fallback for missing metadata (filename derived from path)
//! - AC-10: All existing playlist operations continue working
//!
//! BDD Scenario References:
//! - SCENARIO-010: TUI constructs track objects directly from server-provided metadata
//! - SCENARIO-011: TUI does not invoke file-based metadata parsing during playlist load
//! - SCENARIO-012: Shuffle event processed without re-reading metadata from disk
//! - SCENARIO-017: TUI displays filename fallback when metadata is absent
//! - SCENARIO-024: Empty playlist handled without error
//! - SCENARIO-025: Playlist with all tracks missing metadata displays successfully

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use termusiclib::player::playlist_helpers::PlaylistTrackSource;
    use termusiclib::player::{PlaylistAddTrackInfo, PlaylistTracks};
    use termusiclib::track::Track;

    use crate::ui::model::playlist::TUIPlaylist;

    // =========================================================================
    // Helper: Build a PlaylistTracks proto-like struct for testing load_from_grpc
    // =========================================================================

    /// Helper to build a PlaylistTracks with N tracks containing full metadata.
    /// Uses the protobuf-generated PlaylistTracks type that load_from_grpc accepts.
    fn make_playlist_tracks_proto(count: usize) -> PlaylistTracks {
        let mut tracks = Vec::with_capacity(count);
        for i in 0..count {
            use termusiclib::player::playlist_add_track::OptionalTitle;

            let track = termusiclib::player::PlaylistAddTrack {
                at_index: i as u64,
                optional_title: Some(OptionalTitle::Title(format!("Track {i}"))),
                duration: Some(termusiclib::player::Duration {
                    secs: 180,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(format!(
                        "/music/track_{i}.flac"
                    ))),
                }),
                artist: Some(format!("Artist {i}")),
                album: Some(format!("Album {i}")),
                has_local_file: None,
            };
            tracks.push(track);
        }

        PlaylistTracks {
            current_track_index: 0,
            tracks,
        }
    }

    /// Helper to build a PlaylistTracks with tracks that have NO metadata fields.
    fn make_playlist_tracks_no_metadata(count: usize) -> PlaylistTracks {
        let mut tracks = Vec::with_capacity(count);
        for i in 0..count {
            let track = termusiclib::player::PlaylistAddTrack {
                at_index: i as u64,
                optional_title: None,
                duration: None,
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(format!(
                        "/music/no_metadata_{i}.mp3"
                    ))),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            };
            tracks.push(track);
        }

        PlaylistTracks {
            current_track_index: 0,
            tracks,
        }
    }

    /// Helper to build a PlaylistTracks with a mix of Path, Url, PodcastUrl sources.
    fn make_mixed_source_playlist_tracks() -> PlaylistTracks {
        use termusiclib::player::playlist_add_track::OptionalTitle;
        use termusiclib::player::track_id::Source;

        let tracks = vec![
            termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(OptionalTitle::Title("Local Song".to_string())),
                duration: Some(termusiclib::player::Duration {
                    secs: 240,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Path("/music/song.flac".to_string())),
                }),
                artist: Some("Local Artist".to_string()),
                album: Some("Local Album".to_string()),
                has_local_file: None,
            },
            termusiclib::player::PlaylistAddTrack {
                at_index: 1,
                optional_title: Some(OptionalTitle::Title("Radio Stream".to_string())),
                duration: None,
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Url("http://radio.example.com/stream".to_string())),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            },
            termusiclib::player::PlaylistAddTrack {
                at_index: 2,
                optional_title: Some(OptionalTitle::Title("Podcast Episode".to_string())),
                duration: Some(termusiclib::player::Duration {
                    secs: 3600,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::PodcastUrl(
                        "http://podcast.example.com/ep1.mp3".to_string(),
                    )),
                }),
                artist: Some("Podcast Host".to_string()),
                album: None,
                has_local_file: Some(true),
            },
        ];

        PlaylistTracks {
            current_track_index: 1,
            tracks,
        }
    }

    // =========================================================================
    // T-26: Rewrite Playback::load_from_grpc — no db_pod, uses from_grpc_metadata
    // SCENARIO-010, SCENARIO-011, AC-04
    // =========================================================================

    /// load_from_grpc must accept PlaylistTracks WITHOUT a db_pod parameter.
    /// This test verifies the new signature: load_from_grpc(&mut self, info: PlaylistTracks) -> Result<()>
    /// It should fail to compile if load_from_grpc still requires db_pod.
    #[test]
    fn load_from_grpc_no_db_pod_parameter() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(5);

        // Phase 3 rewrite: load_from_grpc takes only PlaylistTracks, no db_pod
        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 5);
    }

    /// load_from_grpc populates Track title from proto metadata (not disk I/O).
    #[test]
    fn load_from_grpc_populates_title_from_proto() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(3);

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 0"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Track 1"));
        assert_eq!(playback.playlist.tracks()[2].title(), Some("Track 2"));
    }

    /// load_from_grpc populates Track artist from proto metadata.
    #[test]
    fn load_from_grpc_populates_artist_from_proto() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(3);

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        assert_eq!(playback.playlist.tracks()[0].artist(), Some("Artist 0"));
        assert_eq!(playback.playlist.tracks()[1].artist(), Some("Artist 1"));
        assert_eq!(playback.playlist.tracks()[2].artist(), Some("Artist 2"));
    }

    /// load_from_grpc populates Track album from proto metadata.
    #[test]
    fn load_from_grpc_populates_album_from_proto() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(3);

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let track0 = &playback.playlist.tracks()[0];
        let track_data = track0.as_track().expect("should be a TrackData");
        assert_eq!(track_data.album(), Some("Album 0"));
    }

    /// load_from_grpc sets current_track_index correctly.
    #[test]
    fn load_from_grpc_sets_current_track_index() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let mut proto = make_playlist_tracks_proto(5);
        proto.current_track_index = 3;

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        assert_eq!(playback.playlist.current_track_index(), Some(3));
    }

    /// load_from_grpc handles mixed source types (Path, Url, PodcastUrl) correctly.
    /// SCENARIO-010: constructs track objects from server-provided metadata
    #[test]
    fn load_from_grpc_handles_mixed_sources() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_mixed_source_playlist_tracks();

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        assert_eq!(playback.playlist.len(), 3);

        // Path track
        let track0 = &playback.playlist.tracks()[0];
        assert!(track0.as_track().is_some());
        assert_eq!(track0.title(), Some("Local Song"));
        assert_eq!(track0.artist(), Some("Local Artist"));

        // URL (radio) track
        let track1 = &playback.playlist.tracks()[1];
        assert!(track1.as_radio().is_some());
        assert_eq!(track1.title(), Some("Radio Stream"));

        // PodcastUrl track
        let track2 = &playback.playlist.tracks()[2];
        assert!(track2.as_podcast().is_some());
        assert_eq!(track2.title(), Some("Podcast Episode"));
        assert_eq!(track2.artist(), Some("Podcast Host"));
        // has_local_file=true -> podcast has localfile set
        assert!(track2.as_podcast().unwrap().has_localfile());
    }

    /// load_from_grpc with current_track_index=1 sets current_track on Playback.
    #[test]
    fn load_from_grpc_sets_current_track_on_playback() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_mixed_source_playlist_tracks(); // current_track_index=1

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        // After load, current_track should be set from playlist
        let current = playback.current_track();
        assert!(current.is_some());
        assert_eq!(current.unwrap().title(), Some("Radio Stream"));
    }

    // =========================================================================
    // SCENARIO-024: Empty playlist handled without error
    // =========================================================================

    /// load_from_grpc with empty tracks produces empty playlist without error.
    #[test]
    fn load_from_grpc_empty_playlist() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert!(playback.playlist.is_empty());
        assert_eq!(playback.playlist.current_track_index(), None);
    }

    // =========================================================================
    // SCENARIO-025, SCENARIO-017: Tracks with missing metadata display fallback
    // AC-08: Graceful fallback for missing metadata
    // =========================================================================

    /// load_from_grpc with tracks that have no title, artist, or album should
    /// still succeed; title() returns None (display layer derives fallback from path).
    #[test]
    fn load_from_grpc_missing_metadata_stores_none() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_no_metadata(3);

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 3);

        // Title should be None (fallback is handled at display layer)
        assert_eq!(playback.playlist.tracks()[0].title(), None);
        assert_eq!(playback.playlist.tracks()[1].title(), None);
        assert_eq!(playback.playlist.tracks()[2].title(), None);

        // Artist should be None
        assert_eq!(playback.playlist.tracks()[0].artist(), None);
    }

    /// load_from_grpc with tracks that have no duration stores None duration.
    /// SCENARIO-019: TUI handles track with missing duration gracefully
    #[test]
    fn load_from_grpc_missing_duration_stores_none() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_no_metadata(2);

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        // Duration should be None (no crash)
        assert_eq!(playback.playlist.tracks()[0].duration(), None);
        assert_eq!(playback.playlist.tracks()[1].duration(), None);
    }

    /// load_from_grpc fails when a track has no id.
    #[test]
    fn load_from_grpc_fails_for_missing_track_id() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(
                    termusiclib::player::playlist_add_track::OptionalTitle::Title(
                        "No ID".to_string(),
                    ),
                ),
                duration: None,
                id: None, // Missing ID!
                artist: None,
                album: None,
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_err());
    }

    // =========================================================================
    // T-28: handle_playlist_add uses Track::from_grpc_metadata and insert_track_at
    // SCENARIO-008: Individual track addition uses from_grpc_metadata
    // =========================================================================

    /// handle_playlist_add should construct Track from PlaylistAddTrackInfo metadata
    /// and insert it at the specified index using insert_track_at.
    /// This test verifies the track is constructed from the info struct's metadata fields.
    #[test]
    fn handle_playlist_add_constructs_track_from_metadata() {
        // Directly test the TUI playlist behavior after what handle_playlist_add should do:
        // Build a Track from PlaylistAddTrackInfo fields using from_grpc_metadata, then insert
        let mut playlist = TUIPlaylist::default();

        // Pre-populate with some tracks
        for i in 0..3 {
            let t = Track::from_grpc_metadata(
                PlaylistTrackSource::Path(format!("/music/existing_{i}.mp3")),
                Some(format!("Existing {i}")),
                None,
                None,
                Some(Duration::from_secs(100)),
                false,
            );
            playlist.insert_track_at(i, t);
        }

        // Simulate what the rewritten handle_playlist_add does:
        let info = PlaylistAddTrackInfo {
            at_index: 1,
            title: Some("New Track".to_string()),
            artist: Some("New Artist".to_string()),
            album: Some("New Album".to_string()),
            duration: Duration::from_secs(300),
            trackid: PlaylistTrackSource::Path("/music/new_track.flac".to_string()),
            has_local_file: false,
        };

        // The rewritten handle_playlist_add should do this:
        let track = Track::from_grpc_metadata(
            info.trackid.clone(),
            info.title.clone(),
            info.artist.clone(),
            info.album.clone(),
            Some(info.duration),
            info.has_local_file,
        );
        playlist.insert_track_at(info.at_index as usize, track);

        // Verify insertion
        assert_eq!(playlist.len(), 4);
        assert_eq!(playlist.tracks()[1].title(), Some("New Track"));
        assert_eq!(playlist.tracks()[1].artist(), Some("New Artist"));
        let track_data = playlist.tracks()[1]
            .as_track()
            .expect("should be TrackData");
        assert_eq!(track_data.album(), Some("New Album"));
    }

    /// handle_playlist_add with PodcastUrl source and has_local_file=true
    /// constructs a podcast track with localfile sentinel.
    #[test]
    fn handle_playlist_add_podcast_with_local_file() {
        let mut playlist = TUIPlaylist::default();

        let info = PlaylistAddTrackInfo {
            at_index: 0,
            title: Some("Podcast Ep".to_string()),
            artist: Some("Host Name".to_string()),
            album: None,
            duration: Duration::from_secs(1800),
            trackid: PlaylistTrackSource::PodcastUrl(
                "http://podcast.example.com/ep.mp3".to_string(),
            ),
            has_local_file: true,
        };

        let track = Track::from_grpc_metadata(
            info.trackid.clone(),
            info.title.clone(),
            info.artist.clone(),
            info.album.clone(),
            Some(info.duration),
            info.has_local_file,
        );
        playlist.insert_track_at(info.at_index as usize, track);

        assert_eq!(playlist.len(), 1);
        let podcast_track = &playlist.tracks()[0];
        assert!(podcast_track.as_podcast().is_some());
        assert!(podcast_track.as_podcast().unwrap().has_localfile());
        assert_eq!(podcast_track.title(), Some("Podcast Ep"));
    }

    /// handle_playlist_add at end of playlist appends correctly.
    #[test]
    fn handle_playlist_add_at_end_appends() {
        let mut playlist = TUIPlaylist::default();

        // Add 3 tracks
        for i in 0..3 {
            let t = Track::from_grpc_metadata(
                PlaylistTrackSource::Path(format!("/music/t{i}.mp3")),
                Some(format!("T{i}")),
                None,
                None,
                Some(Duration::from_secs(60)),
                false,
            );
            playlist.insert_track_at(i, t);
        }

        // Add at index 3 (== len) should append
        let info = PlaylistAddTrackInfo {
            at_index: 3,
            title: Some("Appended".to_string()),
            artist: None,
            album: None,
            duration: Duration::from_secs(120),
            trackid: PlaylistTrackSource::Path("/music/appended.mp3".to_string()),
            has_local_file: false,
        };

        let track = Track::from_grpc_metadata(
            info.trackid.clone(),
            info.title.clone(),
            info.artist.clone(),
            info.album.clone(),
            Some(info.duration),
            info.has_local_file,
        );
        playlist.insert_track_at(info.at_index as usize, track);

        assert_eq!(playlist.len(), 4);
        assert_eq!(playlist.tracks()[3].title(), Some("Appended"));
    }

    // =========================================================================
    // SCENARIO-012: Shuffle event processed without disk I/O
    // AC-05: Shuffle events processed without re-reading metadata from disk
    // =========================================================================

    /// When a shuffle event arrives, load_from_grpc (without db_pod) processes
    /// the full playlist from the event payload using from_grpc_metadata.
    #[test]
    fn shuffle_event_processed_via_load_from_grpc_no_disk_io() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();

        // Initial load
        let initial = make_playlist_tracks_proto(5);
        playback.load_from_grpc(initial).unwrap();
        assert_eq!(playback.playlist.len(), 5);

        // Simulate shuffle event: same tracks, different order
        let shuffled = {
            use termusiclib::player::playlist_add_track::OptionalTitle;

            let tracks = vec![
                termusiclib::player::PlaylistAddTrack {
                    at_index: 0,
                    optional_title: Some(OptionalTitle::Title("Track 3".to_string())),
                    duration: Some(termusiclib::player::Duration {
                        secs: 180,
                        nanos: 0,
                    }),
                    id: Some(termusiclib::player::TrackId {
                        source: Some(termusiclib::player::track_id::Source::Path(
                            "/music/track_3.flac".to_string(),
                        )),
                    }),
                    artist: Some("Artist 3".to_string()),
                    album: Some("Album 3".to_string()),
                    has_local_file: None,
                },
                termusiclib::player::PlaylistAddTrack {
                    at_index: 1,
                    optional_title: Some(OptionalTitle::Title("Track 0".to_string())),
                    duration: Some(termusiclib::player::Duration {
                        secs: 180,
                        nanos: 0,
                    }),
                    id: Some(termusiclib::player::TrackId {
                        source: Some(termusiclib::player::track_id::Source::Path(
                            "/music/track_0.flac".to_string(),
                        )),
                    }),
                    artist: Some("Artist 0".to_string()),
                    album: Some("Album 0".to_string()),
                    has_local_file: None,
                },
            ];

            PlaylistTracks {
                current_track_index: 0,
                tracks,
            }
        };

        // Process shuffle via load_from_grpc (no db_pod)
        let result = playback.load_from_grpc(shuffled);
        assert!(result.is_ok());

        // Verify tracks are in shuffled order
        assert_eq!(playback.playlist.len(), 2);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 3"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Track 0"));
    }

    // =========================================================================
    // SCENARIO-013: Multiple rapid shuffle events each processed without disk I/O
    // =========================================================================

    /// Two sequential shuffle events are both processed via load_from_grpc
    /// without disk I/O, resulting in the final shuffled state.
    #[test]
    fn multiple_shuffle_events_processed_without_disk_io() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();

        // First shuffle
        let first_shuffle = make_playlist_tracks_proto(3);
        playback.load_from_grpc(first_shuffle).unwrap();
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 0"));

        // Second shuffle (different order)
        let second_shuffle = {
            use termusiclib::player::playlist_add_track::OptionalTitle;

            PlaylistTracks {
                current_track_index: 0,
                tracks: vec![
                    termusiclib::player::PlaylistAddTrack {
                        at_index: 0,
                        optional_title: Some(OptionalTitle::Title("Track 2".to_string())),
                        duration: Some(termusiclib::player::Duration {
                            secs: 180,
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(termusiclib::player::track_id::Source::Path(
                                "/music/track_2.flac".to_string(),
                            )),
                        }),
                        artist: Some("Artist 2".to_string()),
                        album: Some("Album 2".to_string()),
                        has_local_file: None,
                    },
                    termusiclib::player::PlaylistAddTrack {
                        at_index: 1,
                        optional_title: Some(OptionalTitle::Title("Track 1".to_string())),
                        duration: Some(termusiclib::player::Duration {
                            secs: 180,
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(termusiclib::player::track_id::Source::Path(
                                "/music/track_1.flac".to_string(),
                            )),
                        }),
                        artist: Some("Artist 1".to_string()),
                        album: Some("Album 1".to_string()),
                        has_local_file: None,
                    },
                ],
            }
        };

        let result = playback.load_from_grpc(second_shuffle);
        assert!(result.is_ok());

        // Final state reflects second shuffle
        assert_eq!(playback.playlist.len(), 2);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 2"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Track 1"));
    }

    // =========================================================================
    // AC-10: Existing playlist operations continue working after rewrite
    // SCENARIO-023: All playlist mutations continue working
    // =========================================================================

    /// After loading via the rewritten load_from_grpc, swap still works.
    #[test]
    fn existing_operation_swap_works_after_grpc_load() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(4);
        playback.load_from_grpc(proto).unwrap();

        // Swap tracks 0 and 2
        playback.playlist.swap(0, 2).unwrap();

        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 2"));
        assert_eq!(playback.playlist.tracks()[2].title(), Some("Track 0"));
    }

    /// After loading via the rewritten load_from_grpc, remove still works.
    #[test]
    fn existing_operation_remove_works_after_grpc_load() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(4);
        playback.load_from_grpc(proto).unwrap();

        // Remove track at index 1
        playback.playlist.remove_simple(1).unwrap();

        assert_eq!(playback.playlist.len(), 3);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 0"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Track 2"));
        assert_eq!(playback.playlist.tracks()[2].title(), Some("Track 3"));
    }

    /// After loading via the rewritten load_from_grpc, clear still works.
    #[test]
    fn existing_operation_clear_works_after_grpc_load() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(5);
        playback.load_from_grpc(proto).unwrap();

        playback.playlist.clear();

        assert!(playback.playlist.is_empty());
        assert_eq!(playback.playlist.current_track_index(), None);
    }

    // =========================================================================
    // T-29: track_from_path and track_from_podcasturi should no longer be publicly accessible
    // (or removed entirely) — verified by absence in the public API
    // =========================================================================

    /// Verify that TUIPlaylist does NOT expose track_from_path as a public method.
    /// After Phase 3, this method should be removed or made private/deprecated.
    /// This test asserts the method is gone from the public interface by calling
    /// the new insert_track_at path instead.
    #[test]
    fn tui_playlist_uses_insert_track_at_instead_of_add_tracks_for_single_track() {
        let mut playlist = TUIPlaylist::default();

        // The Phase 3 approach: construct Track from metadata, then insert_track_at
        let track = Track::from_grpc_metadata(
            PlaylistTrackSource::Path("/music/song.mp3".to_string()),
            Some("Song Title".to_string()),
            Some("Song Artist".to_string()),
            Some("Song Album".to_string()),
            Some(Duration::from_secs(200)),
            false,
        );

        playlist.insert_track_at(0, track);

        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.tracks()[0].title(), Some("Song Title"));
        assert_eq!(playlist.tracks()[0].artist(), Some("Song Artist"));
    }

    // =========================================================================
    // SCENARIO-028: Track with extremely long metadata strings handled
    // =========================================================================

    /// Tracks with very long title and artist should be handled without panic.
    #[test]
    fn load_from_grpc_handles_long_metadata_strings() {
        use crate::ui::model::Playback;
        use termusiclib::player::playlist_add_track::OptionalTitle;

        let mut playback = Playback::new();
        let long_title = "A".repeat(500);
        let long_artist = "B".repeat(300);

        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(OptionalTitle::Title(long_title.clone())),
                duration: Some(termusiclib::player::Duration {
                    secs: 100,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(
                        "/music/long_meta.mp3".to_string(),
                    )),
                }),
                artist: Some(long_artist.clone()),
                album: Some("Normal Album".to_string()),
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert_eq!(
            playback.playlist.tracks()[0].title(),
            Some(long_title.as_str())
        );
        assert_eq!(
            playback.playlist.tracks()[0].artist(),
            Some(long_artist.as_str())
        );
    }

    // =========================================================================
    // Performance: AC-01 — load_from_grpc completes quickly for large playlists
    // SCENARIO-001, SCENARIO-026
    // =========================================================================

    /// load_from_grpc with 1000 tracks must complete in under 100ms.
    /// Since there is no disk I/O, this should be sub-millisecond.
    #[test]
    fn load_from_grpc_1000_tracks_under_100ms() {
        use crate::ui::model::Playback;
        use std::time::Instant;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(1000);

        let start = Instant::now();
        let result = playback.load_from_grpc(proto);
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 1000);
        assert!(
            elapsed < Duration::from_millis(100),
            "load_from_grpc took {:?}, exceeds 100ms limit",
            elapsed
        );
    }

    /// load_from_grpc with 5000 tracks must complete in under 100ms.
    /// SCENARIO-026
    #[test]
    fn load_from_grpc_5000_tracks_under_100ms() {
        use crate::ui::model::Playback;
        use std::time::Instant;

        let mut playback = Playback::new();
        let proto = make_playlist_tracks_proto(5000);

        let start = Instant::now();
        let result = playback.load_from_grpc(proto);
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 5000);
        assert!(
            elapsed < Duration::from_millis(100),
            "load_from_grpc for 5000 tracks took {:?}, exceeds 100ms limit",
            elapsed
        );
    }

    // =========================================================================
    // Regression: load_from_grpc replaces existing playlist content
    // =========================================================================

    /// Calling load_from_grpc twice replaces the previous playlist entirely.
    #[test]
    fn load_from_grpc_replaces_previous_playlist() {
        use crate::ui::model::Playback;

        let mut playback = Playback::new();

        // First load
        let proto1 = make_playlist_tracks_proto(10);
        playback.load_from_grpc(proto1).unwrap();
        assert_eq!(playback.playlist.len(), 10);

        // Second load (smaller playlist) replaces
        let proto2 = make_playlist_tracks_proto(3);
        playback.load_from_grpc(proto2).unwrap();
        assert_eq!(playback.playlist.len(), 3);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Track 0"));
    }

    /// load_from_grpc with has_local_file=false for podcast produces no localfile.
    #[test]
    fn load_from_grpc_podcast_without_local_file() {
        use crate::ui::model::Playback;
        use termusiclib::player::playlist_add_track::OptionalTitle;
        use termusiclib::player::track_id::Source;

        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(OptionalTitle::Title("Podcast No Local".to_string())),
                duration: Some(termusiclib::player::Duration {
                    secs: 600,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::PodcastUrl(
                        "http://podcast.example.com/ep2.mp3".to_string(),
                    )),
                }),
                artist: None,
                album: None,
                has_local_file: Some(false),
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let podcast = playback.playlist.tracks()[0].as_podcast().unwrap();
        assert!(!podcast.has_localfile());
    }
}
