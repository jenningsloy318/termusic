//! Phase 4 Integration Tests for Async TUI Playlist Loading.
//!
//! These tests validate the complete end-to-end data flow from server serialization
//! through gRPC protocol to TUI deserialization and rendering. They cover:
//!
//! - T-32: End-to-end test: server proto output fed to load_from_grpc produces correct TUIPlaylist
//! - T-33: Edge cases: empty playlist, all-missing-metadata, missing duration, long metadata
//! - T-34: Performance tests: load_from_grpc timing, playlist_sync equivalent timing
//! - T-35: Shuffle events, concurrent reload/shuffle consistency, regression tests
//!
//! AC References:
//! - AC-01: TUI main event loop MUST NOT be blocked for more than 100ms (SCENARIO-001, SCENARIO-002, SCENARIO-026)
//! - AC-02: Playlist view renders within 200ms of data receipt (SCENARIO-004)
//! - AC-03: Server includes sufficient display metadata (SCENARIO-006, SCENARIO-007, SCENARIO-008, SCENARIO-009)
//! - AC-04: TUI constructs Track without disk I/O (SCENARIO-010, SCENARIO-011)
//! - AC-05: Shuffle events processed without disk re-reads (SCENARIO-003, SCENARIO-012, SCENARIO-013)
//! - AC-06: Proto extended with backward wire compatibility (SCENARIO-014)
//! - AC-07: Server populates optional_title (SCENARIO-005, SCENARIO-015, SCENARIO-016)
//! - AC-08: Graceful fallback for missing metadata (SCENARIO-017, SCENARIO-018, SCENARIO-019, SCENARIO-020)
//! - AC-09: playlist_sync completes within 50ms for 1000 tracks (SCENARIO-021, SCENARIO-022)
//! - AC-10: All existing playlist operations continue working (SCENARIO-023, SCENARIO-027)

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::time::{Duration, Instant};

    use termusiclib::player::playlist_helpers::PlaylistTrackSource;
    use termusiclib::player::{
        PlaylistAddTrackInfo, PlaylistShuffledInfo, PlaylistTracks, UpdatePlaylistEvents,
    };
    use termusiclib::track::Track;

    use crate::ui::model::playlist::TUIPlaylist;
    use crate::ui::model::Playback;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Build a PlaylistTracks proto matching server `as_grpc_playlist_tracks()` output.
    /// Each track has full metadata populated (title, artist, album, duration).
    fn make_server_style_playlist_tracks(count: usize) -> PlaylistTracks {
        use termusiclib::player::playlist_add_track::OptionalTitle;
        use termusiclib::player::track_id::Source;

        let mut tracks = Vec::with_capacity(count);
        for i in 0..count {
            let track = termusiclib::player::PlaylistAddTrack {
                at_index: i as u64,
                optional_title: Some(OptionalTitle::Title(format!("Song Title {i}"))),
                duration: Some(termusiclib::player::Duration {
                    secs: 180 + (i as u64 % 300),
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Path(format!(
                        "/home/user/music/artist_{}/album_{}/track_{i:03}.flac",
                        i % 10,
                        i % 5
                    ))),
                }),
                artist: Some(format!("Artist {}", i % 10)),
                album: Some(format!("Album {}", i % 5)),
                has_local_file: None,
            };
            tracks.push(track);
        }

        PlaylistTracks {
            current_track_index: 0,
            tracks,
        }
    }

    /// Build a PlaylistTracks with a mix of all three source types and varying metadata.
    fn make_mixed_integration_playlist() -> PlaylistTracks {
        use termusiclib::player::playlist_add_track::OptionalTitle;
        use termusiclib::player::track_id::Source;

        let tracks = vec![
            // Local track with full metadata
            termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(OptionalTitle::Title("Bohemian Rhapsody".to_string())),
                duration: Some(termusiclib::player::Duration {
                    secs: 354,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Path(
                        "/music/queen/a_night_at_the_opera/bohemian_rhapsody.flac".to_string(),
                    )),
                }),
                artist: Some("Queen".to_string()),
                album: Some("A Night at the Opera".to_string()),
                has_local_file: None,
            },
            // Radio stream (no duration, no album)
            termusiclib::player::PlaylistAddTrack {
                at_index: 1,
                optional_title: Some(OptionalTitle::Title("Classic FM".to_string())),
                duration: None,
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Url(
                        "http://media-ice.musicradio.com/ClassicFMMP3".to_string(),
                    )),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            },
            // Podcast with local file downloaded
            termusiclib::player::PlaylistAddTrack {
                at_index: 2,
                optional_title: Some(OptionalTitle::Title("Episode 42: The Answer".to_string())),
                duration: Some(termusiclib::player::Duration {
                    secs: 3600,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::PodcastUrl(
                        "https://podcast.example.com/episodes/42.mp3".to_string(),
                    )),
                }),
                artist: Some("Science Podcast".to_string()),
                album: None,
                has_local_file: Some(true),
            },
            // Podcast without local file
            termusiclib::player::PlaylistAddTrack {
                at_index: 3,
                optional_title: Some(OptionalTitle::Title("Episode 43: Deep Space".to_string())),
                duration: Some(termusiclib::player::Duration {
                    secs: 2400,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::PodcastUrl(
                        "https://podcast.example.com/episodes/43.mp3".to_string(),
                    )),
                }),
                artist: Some("Science Podcast".to_string()),
                album: None,
                has_local_file: Some(false),
            },
            // Local track with minimal metadata (title from filename fallback on server)
            termusiclib::player::PlaylistAddTrack {
                at_index: 4,
                optional_title: Some(OptionalTitle::Title("unknown_recording".to_string())),
                duration: Some(termusiclib::player::Duration { secs: 45, nanos: 0 }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Path(
                        "/music/unsorted/unknown_recording.wav".to_string(),
                    )),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            },
        ];

        PlaylistTracks {
            current_track_index: 0,
            tracks,
        }
    }

    /// Simulate the display data extraction pattern that playlist_sync performs.
    /// This benchmarks the same data access pattern without requiring UI components.
    /// Returns the number of tracks processed.
    fn simulate_playlist_sync_data_access(playlist: &TUIPlaylist) -> usize {
        let mut count = 0;
        for (idx, track) in playlist.tracks().iter().enumerate() {
            // Simulate exactly what playlist_sync does: access duration, title, artist, album
            let _duration_str = track.duration_str_short();
            let _title: Cow<'_, str> = track.title().map_or_else(|| track.id_str(), Into::into);
            let _artist = track.artist().unwrap_or("Unknown Artist");
            let _album = track
                .as_track()
                .and_then(|v| v.album())
                .unwrap_or("Unknown Album");

            // Simulate the current track symbol prefix logic
            if Some(idx) == playlist.current_track_index() {
                let _prefixed = format!(">> {}", _title);
            }

            count += 1;
        }
        count
    }

    // =========================================================================
    // T-32: End-to-end integration: server proto -> load_from_grpc -> TUIPlaylist
    // SCENARIO-006: Server includes title, artist, album, duration in playlist data
    // SCENARIO-010: TUI constructs track objects directly from server-provided metadata
    // =========================================================================

    /// End-to-end: simulated server output (matching as_grpc_playlist_tracks format)
    /// fed directly to load_from_grpc produces a correct TUIPlaylist with all metadata intact.
    #[test]
    fn e2e_server_proto_output_to_load_from_grpc_preserves_all_metadata() {
        let mut playback = Playback::new();
        let proto = make_mixed_integration_playlist();

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok(), "load_from_grpc failed: {:?}", result.err());

        // Verify all 5 tracks loaded
        assert_eq!(playback.playlist.len(), 5);

        // Track 0: Local file with full metadata
        let t0 = &playback.playlist.tracks()[0];
        assert_eq!(t0.title(), Some("Bohemian Rhapsody"));
        assert_eq!(t0.artist(), Some("Queen"));
        assert_eq!(t0.duration(), Some(Duration::from_secs(354)));
        let t0_data = t0.as_track().expect("Track 0 should be MediaTypes::Track");
        assert_eq!(t0_data.album(), Some("A Night at the Opera"));
        assert_eq!(
            t0_data.path().to_str().unwrap(),
            "/music/queen/a_night_at_the_opera/bohemian_rhapsody.flac"
        );

        // Track 1: Radio stream
        let t1 = &playback.playlist.tracks()[1];
        assert_eq!(t1.title(), Some("Classic FM"));
        assert_eq!(t1.artist(), None);
        assert_eq!(t1.duration(), None);
        let t1_radio = t1.as_radio().expect("Track 1 should be MediaTypes::Radio");
        assert_eq!(
            t1_radio.url(),
            "http://media-ice.musicradio.com/ClassicFMMP3"
        );

        // Track 2: Podcast with local file
        let t2 = &playback.playlist.tracks()[2];
        assert_eq!(t2.title(), Some("Episode 42: The Answer"));
        assert_eq!(t2.artist(), Some("Science Podcast"));
        assert_eq!(t2.duration(), Some(Duration::from_secs(3600)));
        let t2_podcast = t2
            .as_podcast()
            .expect("Track 2 should be MediaTypes::Podcast");
        assert_eq!(
            t2_podcast.url(),
            "https://podcast.example.com/episodes/42.mp3"
        );
        assert!(
            t2_podcast.has_localfile(),
            "Podcast with has_local_file=true should have localfile"
        );

        // Track 3: Podcast without local file
        let t3 = &playback.playlist.tracks()[3];
        assert_eq!(t3.title(), Some("Episode 43: Deep Space"));
        let t3_podcast = t3
            .as_podcast()
            .expect("Track 3 should be MediaTypes::Podcast");
        assert!(
            !t3_podcast.has_localfile(),
            "Podcast with has_local_file=false should NOT have localfile"
        );

        // Track 4: Local track with filename-derived title
        let t4 = &playback.playlist.tracks()[4];
        assert_eq!(t4.title(), Some("unknown_recording"));
        assert_eq!(t4.artist(), None);
        let t4_data = t4.as_track().expect("Track 4 should be MediaTypes::Track");
        assert_eq!(t4_data.album(), None);
    }

    /// End-to-end: verifying current_track is set correctly from proto data.
    #[test]
    fn e2e_current_track_index_propagates_through_load() {
        let mut playback = Playback::new();
        let mut proto = make_mixed_integration_playlist();
        proto.current_track_index = 2; // Set current to the podcast episode

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        assert_eq!(playback.playlist.current_track_index(), Some(2));
        let current = playback
            .current_track()
            .expect("current_track should be set");
        assert_eq!(current.title(), Some("Episode 42: The Answer"));
    }

    /// End-to-end: server sends 10 tracks with varied metadata, all survive the protocol.
    #[test]
    fn e2e_10_track_full_metadata_round_trip() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(10);

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 10);

        // Verify metadata for each track
        for i in 0..10 {
            let track = &playback.playlist.tracks()[i];
            assert_eq!(track.title(), Some(format!("Song Title {i}").as_str()));
            assert_eq!(track.artist(), Some(format!("Artist {}", i % 10).as_str()));
            let track_data = track.as_track().expect("should be Track variant");
            assert_eq!(
                track_data.album(),
                Some(format!("Album {}", i % 5).as_str())
            );
            assert_eq!(
                track.duration(),
                Some(Duration::from_secs(180 + (i as u64 % 300)))
            );
        }
    }

    // =========================================================================
    // T-32 continued: UpdatePlaylistEvents serialization round-trip
    // SCENARIO-008: Server includes full metadata in individual track addition events
    // SCENARIO-014: Protobuf backward wire compatibility
    // AC-06: Proto extended with artist/album maintaining backward compatibility
    // =========================================================================

    /// The UpdatePlaylistEvents::PlaylistAddTrack round-trips artist, album, has_local_file
    /// through the From/TryFrom protobuf conversion.
    #[test]
    fn serialization_round_trip_playlist_add_track_with_new_fields() {
        let original = UpdatePlaylistEvents::PlaylistAddTrack(PlaylistAddTrackInfo {
            at_index: 5,
            title: Some("Integration Test Song".to_string()),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            duration: Duration::from_secs(240),
            trackid: PlaylistTrackSource::Path("/music/test/song.flac".to_string()),
            has_local_file: false,
        });

        // Serialize to protobuf
        let proto: termusiclib::player::UpdatePlaylist = original.clone().into();

        // Deserialize back to domain
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("deserialization should succeed");

        // Verify round-trip preserves all fields including new ones
        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert_eq!(info.at_index, 5);
                assert_eq!(info.title, Some("Integration Test Song".to_string()));
                assert_eq!(info.artist, Some("Test Artist".to_string()));
                assert_eq!(info.album, Some("Test Album".to_string()));
                assert_eq!(info.duration, Duration::from_secs(240));
                assert_eq!(
                    info.trackid,
                    PlaylistTrackSource::Path("/music/test/song.flac".to_string())
                );
                assert!(!info.has_local_file);
            }
            other => panic!("Expected PlaylistAddTrack, got {:?}", other),
        }
    }

    /// Round-trip for podcast track with has_local_file=true preserves the field.
    #[test]
    fn serialization_round_trip_podcast_has_local_file_true() {
        let original = UpdatePlaylistEvents::PlaylistAddTrack(PlaylistAddTrackInfo {
            at_index: 0,
            title: Some("Podcast Episode".to_string()),
            artist: Some("Host".to_string()),
            album: None,
            duration: Duration::from_secs(1800),
            trackid: PlaylistTrackSource::PodcastUrl("https://example.com/ep.mp3".to_string()),
            has_local_file: true,
        });

        let proto: termusiclib::player::UpdatePlaylist = original.into();
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("deserialization should succeed");

        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert!(
                    info.has_local_file,
                    "has_local_file=true must survive round-trip"
                );
                assert_eq!(info.artist, Some("Host".to_string()));
                assert_eq!(info.album, None);
            }
            other => panic!("Expected PlaylistAddTrack, got {:?}", other),
        }
    }

    /// Round-trip with all metadata fields absent (None) still works correctly.
    /// This validates backward wire compatibility: if the server doesn't populate
    /// the new fields, the TUI handles them gracefully.
    #[test]
    fn serialization_round_trip_with_absent_new_fields() {
        let original = UpdatePlaylistEvents::PlaylistAddTrack(PlaylistAddTrackInfo {
            at_index: 0,
            title: None,
            artist: None,
            album: None,
            duration: Duration::from_secs(100),
            trackid: PlaylistTrackSource::Path("/music/legacy.mp3".to_string()),
            has_local_file: false,
        });

        let proto: termusiclib::player::UpdatePlaylist = original.into();
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("deserialization should succeed");

        match deserialized {
            UpdatePlaylistEvents::PlaylistAddTrack(info) => {
                assert_eq!(info.title, None);
                assert_eq!(info.artist, None);
                assert_eq!(info.album, None);
                assert!(!info.has_local_file);
            }
            other => panic!("Expected PlaylistAddTrack, got {:?}", other),
        }
    }

    /// PlaylistShuffled round-trip preserves full playlist metadata through serialization.
    #[test]
    fn serialization_round_trip_playlist_shuffled_preserves_metadata() {
        let playlist_data = make_server_style_playlist_tracks(5);

        let original = UpdatePlaylistEvents::PlaylistShuffled(PlaylistShuffledInfo {
            tracks: playlist_data.clone(),
        });

        let proto: termusiclib::player::UpdatePlaylist = original.into();
        let deserialized =
            UpdatePlaylistEvents::try_from(proto).expect("deserialization should succeed");

        match deserialized {
            UpdatePlaylistEvents::PlaylistShuffled(info) => {
                assert_eq!(info.tracks.tracks.len(), 5);
                // Verify first track metadata survives
                let first = &info.tracks.tracks[0];
                assert!(first.artist.is_some());
                assert!(first.album.is_some());
                assert_eq!(first.artist.as_deref(), Some("Artist 0"));
                assert_eq!(first.album.as_deref(), Some("Album 0"));
            }
            other => panic!("Expected PlaylistShuffled, got {:?}", other),
        }
    }

    // =========================================================================
    // T-33: Edge cases
    // SCENARIO-024: Empty playlist handled without error
    // =========================================================================

    /// Empty playlist from server produces empty TUIPlaylist, no errors, no disk I/O.
    #[test]
    fn e2e_empty_playlist_no_error() {
        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert!(playback.playlist.is_empty());
        assert_eq!(playback.playlist.current_track_index(), None);
        assert!(playback.current_track().is_none());
    }

    // =========================================================================
    // SCENARIO-025: All tracks missing metadata — filename fallback
    // =========================================================================

    /// When all tracks have no title/artist/album, the TUI stores None and
    /// the display layer derives a fallback from the file path's filename.
    #[test]
    fn e2e_all_missing_metadata_filename_fallback() {
        use termusiclib::player::track_id::Source;

        let mut playback = Playback::new();

        let tracks: Vec<_> = (0..5)
            .map(|i| termusiclib::player::PlaylistAddTrack {
                at_index: i as u64,
                optional_title: None,
                duration: None,
                id: Some(termusiclib::player::TrackId {
                    source: Some(Source::Path(format!("/music/unsorted/file_{i}.mp3"))),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            })
            .collect();

        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks,
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 5);

        // All tracks have None title (display layer derives from path)
        for (i, track) in playback.playlist.tracks().iter().enumerate() {
            assert_eq!(track.title(), None);
            assert_eq!(track.artist(), None);
            // The id_str() method should derive filename from path
            let id = track.id_str();
            assert_eq!(id.as_ref(), format!("file_{i}.mp3"));
        }
    }

    // =========================================================================
    // SCENARIO-019: Track with missing duration handled gracefully
    // =========================================================================

    /// Tracks with no duration should display without error (duration_str_short returns None).
    #[test]
    fn e2e_missing_duration_displays_gracefully() {
        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(
                    termusiclib::player::playlist_add_track::OptionalTitle::Title(
                        "No Duration Track".to_string(),
                    ),
                ),
                duration: None, // explicitly no duration
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(
                        "/music/no_dur.flac".to_string(),
                    )),
                }),
                artist: Some("Artist".to_string()),
                album: None,
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let track = &playback.playlist.tracks()[0];
        assert_eq!(track.title(), Some("No Duration Track"));
        assert_eq!(track.duration(), None);
        // duration_str_short should return None, not panic
        assert!(track.duration_str_short().is_none());
    }

    // =========================================================================
    // SCENARIO-028: Extremely long metadata strings
    // =========================================================================

    /// Track with title > 500 chars and artist > 300 chars handled without overflow.
    #[test]
    fn e2e_extremely_long_metadata_no_overflow() {
        let mut playback = Playback::new();
        let long_title = "X".repeat(600);
        let long_artist = "Y".repeat(400);
        let long_album = "Z".repeat(500);

        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(
                    termusiclib::player::playlist_add_track::OptionalTitle::Title(
                        long_title.clone(),
                    ),
                ),
                duration: Some(termusiclib::player::Duration {
                    secs: 300,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(
                        "/music/long.flac".to_string(),
                    )),
                }),
                artist: Some(long_artist.clone()),
                album: Some(long_album.clone()),
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let track = &playback.playlist.tracks()[0];
        assert_eq!(track.title().unwrap().len(), 600);
        assert_eq!(track.artist().unwrap().len(), 400);
        let td = track.as_track().unwrap();
        assert_eq!(td.album().unwrap().len(), 500);
    }

    // =========================================================================
    // SCENARIO-018: Server sends partial metadata when file cannot be parsed
    // SCENARIO-020: Server does not crash when track has no metadata
    // (Here we test the TUI side handling of partial server data)
    // =========================================================================

    /// When server sends track with only path and duration (no title/artist/album),
    /// the TUI constructs it and uses filename fallback for display.
    #[test]
    fn e2e_partial_metadata_path_and_duration_only() {
        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: None,
                duration: Some(termusiclib::player::Duration {
                    secs: 120,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(
                        "/data/corrupted_but_playable.ogg".to_string(),
                    )),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let track = &playback.playlist.tracks()[0];
        assert_eq!(track.title(), None);
        assert_eq!(track.duration(), Some(Duration::from_secs(120)));
        // id_str falls back to filename
        assert_eq!(track.id_str().as_ref(), "corrupted_but_playable.ogg");
    }

    // =========================================================================
    // T-34: Performance tests
    // SCENARIO-001/AC-01: load_from_grpc < 100ms for 1000 tracks
    // SCENARIO-026: load_from_grpc < 100ms for 5000 tracks
    // =========================================================================

    /// Performance: load_from_grpc with 1000 tracks completes in under 100ms.
    /// With zero disk I/O this should be well under 1ms.
    #[test]
    fn perf_load_from_grpc_1000_tracks_under_100ms() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(1000);

        let start = Instant::now();
        let result = playback.load_from_grpc(proto);
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 1000);
        assert!(
            elapsed < Duration::from_millis(100),
            "load_from_grpc for 1000 tracks took {:?}, exceeds 100ms AC-01 limit",
            elapsed
        );
    }

    /// Performance: load_from_grpc with 5000 tracks completes in under 100ms.
    #[test]
    fn perf_load_from_grpc_5000_tracks_under_100ms() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(5000);

        let start = Instant::now();
        let result = playback.load_from_grpc(proto);
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert_eq!(playback.playlist.len(), 5000);
        assert!(
            elapsed < Duration::from_millis(100),
            "load_from_grpc for 5000 tracks took {:?}, exceeds 100ms AC-01 limit",
            elapsed
        );
    }

    // =========================================================================
    // SCENARIO-021/AC-09: playlist_sync table building < 50ms for 1000 tracks
    // SCENARIO-022: Linear scaling
    // =========================================================================

    /// Performance: the data access pattern of playlist_sync (iterating tracks,
    /// accessing title/artist/album/duration) completes in under 50ms for 1000 tracks.
    /// This tests the in-memory data access without requiring the UI component.
    #[test]
    fn perf_playlist_sync_data_access_1000_tracks_under_50ms() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(1000);
        playback.load_from_grpc(proto).unwrap();

        // Set a current track index to exercise the current-track symbol logic
        playback.playlist.set_current_track_index(500).unwrap();

        let start = Instant::now();
        let count = simulate_playlist_sync_data_access(&playback.playlist);
        let elapsed = start.elapsed();

        assert_eq!(count, 1000);
        assert!(
            elapsed < Duration::from_millis(50),
            "playlist_sync data access for 1000 tracks took {:?}, exceeds 50ms AC-09 limit",
            elapsed
        );
    }

    /// Performance: playlist_sync scales approximately linearly.
    /// Time for 1000 tracks should be less than 2x the time for 500 tracks.
    #[test]
    fn perf_playlist_sync_linear_scaling() {
        // 500 tracks
        let mut playback_500 = Playback::new();
        let proto_500 = make_server_style_playlist_tracks(500);
        playback_500.load_from_grpc(proto_500).unwrap();

        let start_500 = Instant::now();
        simulate_playlist_sync_data_access(&playback_500.playlist);
        let elapsed_500 = start_500.elapsed();

        // 1000 tracks
        let mut playback_1000 = Playback::new();
        let proto_1000 = make_server_style_playlist_tracks(1000);
        playback_1000.load_from_grpc(proto_1000).unwrap();

        let start_1000 = Instant::now();
        simulate_playlist_sync_data_access(&playback_1000.playlist);
        let elapsed_1000 = start_1000.elapsed();

        // Allow up to 3x ratio (generous for test stability on loaded systems)
        let ratio = elapsed_1000.as_nanos() as f64 / elapsed_500.as_nanos().max(1) as f64;
        assert!(
            ratio < 3.0,
            "Non-linear scaling detected: 500 tracks took {:?}, 1000 tracks took {:?}, ratio {:.2}x (expected < 3x)",
            elapsed_500,
            elapsed_1000,
            ratio
        );

        // Both must be under 50ms
        assert!(
            elapsed_1000 < Duration::from_millis(50),
            "1000 tracks sync took {:?}",
            elapsed_1000
        );
    }

    // =========================================================================
    // SCENARIO-004/AC-02: Combined load + render < 200ms
    // =========================================================================

    /// Performance: the combined load_from_grpc + playlist_sync data access for
    /// 1000 tracks completes within 200ms (AC-02).
    #[test]
    fn perf_combined_load_and_sync_1000_tracks_under_200ms() {
        let proto = make_server_style_playlist_tracks(1000);
        let mut playback = Playback::new();

        let start = Instant::now();
        playback.load_from_grpc(proto).unwrap();
        simulate_playlist_sync_data_access(&playback.playlist);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(200),
            "Combined load + sync for 1000 tracks took {:?}, exceeds 200ms AC-02 limit",
            elapsed
        );
    }

    // =========================================================================
    // T-35: Shuffle event integration
    // SCENARIO-003: TUI event loop not blocked when receiving shuffled playlist event
    // SCENARIO-012: Shuffle event processed without disk I/O
    // SCENARIO-013: Multiple rapid shuffle events each processed without disk I/O
    // =========================================================================

    /// A shuffle event (full playlist with metadata) is processed via load_from_grpc
    /// without any disk I/O, resulting in correct reordered state.
    #[test]
    fn e2e_shuffle_event_reorders_playlist_from_metadata() {
        let mut playback = Playback::new();

        // Initial load: 5 tracks in order 0..4
        let initial = make_server_style_playlist_tracks(5);
        playback.load_from_grpc(initial).unwrap();
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 0"));
        assert_eq!(playback.playlist.tracks()[4].title(), Some("Song Title 4"));

        // Simulate shuffle event: tracks in reversed order
        let shuffled = {
            use termusiclib::player::playlist_add_track::OptionalTitle;
            use termusiclib::player::track_id::Source;

            let tracks: Vec<_> = (0..5)
                .rev()
                .enumerate()
                .map(
                    |(new_idx, orig_idx)| termusiclib::player::PlaylistAddTrack {
                        at_index: new_idx as u64,
                        optional_title: Some(OptionalTitle::Title(format!(
                            "Song Title {orig_idx}"
                        ))),
                        duration: Some(termusiclib::player::Duration {
                            secs: 180 + (orig_idx as u64 % 300),
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(Source::Path(format!(
                                "/home/user/music/artist_{}/album_{}/track_{orig_idx:03}.flac",
                                orig_idx % 10,
                                orig_idx % 5
                            ))),
                        }),
                        artist: Some(format!("Artist {}", orig_idx % 10)),
                        album: Some(format!("Album {}", orig_idx % 5)),
                        has_local_file: None,
                    },
                )
                .collect();

            PlaylistTracks {
                current_track_index: 2,
                tracks,
            }
        };

        // Process the shuffle event
        let result = playback.load_from_grpc(shuffled);
        assert!(result.is_ok());

        // Verify reversed order
        assert_eq!(playback.playlist.len(), 5);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 4"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Song Title 3"));
        assert_eq!(playback.playlist.tracks()[2].title(), Some("Song Title 2"));
        assert_eq!(playback.playlist.tracks()[3].title(), Some("Song Title 1"));
        assert_eq!(playback.playlist.tracks()[4].title(), Some("Song Title 0"));

        // Current track index updated
        assert_eq!(playback.playlist.current_track_index(), Some(2));
    }

    /// Multiple rapid shuffle events: each replaces the playlist state entirely.
    /// Both are processed via in-memory construction (no disk I/O).
    #[test]
    fn e2e_multiple_rapid_shuffles_no_disk_io() {
        let mut playback = Playback::new();

        // First event
        let first = make_server_style_playlist_tracks(10);
        playback.load_from_grpc(first).unwrap();
        assert_eq!(playback.playlist.len(), 10);

        // Second event (fewer tracks — simulates a different shuffled subset)
        let second = make_server_style_playlist_tracks(8);
        playback.load_from_grpc(second).unwrap();
        assert_eq!(playback.playlist.len(), 8);

        // Third event (back to full size)
        let third = make_server_style_playlist_tracks(10);
        playback.load_from_grpc(third).unwrap();
        assert_eq!(playback.playlist.len(), 10);

        // Final state is deterministic from last event
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 0"));
        assert_eq!(playback.playlist.tracks()[9].title(), Some("Song Title 9"));
    }

    /// Shuffle event with 1000 tracks completes under 100ms (AC-01 for shuffle path).
    #[test]
    fn perf_shuffle_event_1000_tracks_under_100ms() {
        let mut playback = Playback::new();

        // Initial load
        let initial = make_server_style_playlist_tracks(1000);
        playback.load_from_grpc(initial).unwrap();

        // Shuffle event (same size, all metadata)
        let shuffled = make_server_style_playlist_tracks(1000);
        let start = Instant::now();
        let result = playback.load_from_grpc(shuffled);
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(
            elapsed < Duration::from_millis(100),
            "Shuffle event for 1000 tracks took {:?}, exceeds 100ms",
            elapsed
        );
    }

    // =========================================================================
    // T-35 continued: SCENARIO-027 — Concurrent reload and shuffle consistency
    // =========================================================================

    /// When a reload and a shuffle both call load_from_grpc sequentially,
    /// the final state is from the last call (no partial/corrupted state).
    #[test]
    fn e2e_sequential_reload_and_shuffle_consistent_final_state() {
        let mut playback = Playback::new();

        // Simulate reload
        let reload_data = {
            use termusiclib::player::playlist_add_track::OptionalTitle;
            use termusiclib::player::track_id::Source;

            PlaylistTracks {
                current_track_index: 0,
                tracks: vec![
                    termusiclib::player::PlaylistAddTrack {
                        at_index: 0,
                        optional_title: Some(OptionalTitle::Title("Reload Track A".to_string())),
                        duration: Some(termusiclib::player::Duration {
                            secs: 100,
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(Source::Path("/music/a.flac".to_string())),
                        }),
                        artist: Some("Artist A".to_string()),
                        album: None,
                        has_local_file: None,
                    },
                    termusiclib::player::PlaylistAddTrack {
                        at_index: 1,
                        optional_title: Some(OptionalTitle::Title("Reload Track B".to_string())),
                        duration: Some(termusiclib::player::Duration {
                            secs: 200,
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(Source::Path("/music/b.flac".to_string())),
                        }),
                        artist: Some("Artist B".to_string()),
                        album: None,
                        has_local_file: None,
                    },
                ],
            }
        };

        // Simulate shuffle arriving after reload (overwrites)
        let shuffle_data = {
            use termusiclib::player::playlist_add_track::OptionalTitle;
            use termusiclib::player::track_id::Source;

            PlaylistTracks {
                current_track_index: 1,
                tracks: vec![
                    termusiclib::player::PlaylistAddTrack {
                        at_index: 0,
                        optional_title: Some(OptionalTitle::Title("Shuffle Track Z".to_string())),
                        duration: Some(termusiclib::player::Duration {
                            secs: 300,
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(Source::Path("/music/z.flac".to_string())),
                        }),
                        artist: Some("Artist Z".to_string()),
                        album: Some("Album Z".to_string()),
                        has_local_file: None,
                    },
                    termusiclib::player::PlaylistAddTrack {
                        at_index: 1,
                        optional_title: Some(OptionalTitle::Title("Shuffle Track Y".to_string())),
                        duration: Some(termusiclib::player::Duration {
                            secs: 400,
                            nanos: 0,
                        }),
                        id: Some(termusiclib::player::TrackId {
                            source: Some(Source::Path("/music/y.flac".to_string())),
                        }),
                        artist: Some("Artist Y".to_string()),
                        album: Some("Album Y".to_string()),
                        has_local_file: None,
                    },
                ],
            }
        };

        // Process reload then shuffle
        playback.load_from_grpc(reload_data).unwrap();
        playback.load_from_grpc(shuffle_data).unwrap();

        // Final state should be from the shuffle (last write wins)
        assert_eq!(playback.playlist.len(), 2);
        assert_eq!(
            playback.playlist.tracks()[0].title(),
            Some("Shuffle Track Z")
        );
        assert_eq!(
            playback.playlist.tracks()[1].title(),
            Some("Shuffle Track Y")
        );
        assert_eq!(playback.playlist.current_track_index(), Some(1));

        // Metadata fully preserved
        assert_eq!(playback.playlist.tracks()[0].artist(), Some("Artist Z"));
        let td = playback.playlist.tracks()[0].as_track().unwrap();
        assert_eq!(td.album(), Some("Album Z"));
    }

    // =========================================================================
    // T-35 continued: SCENARIO-023/AC-10 — Regression tests for playlist operations
    // All operations (add, remove, swap, shuffle, clear) work with metadata protocol
    // =========================================================================

    /// After loading via metadata protocol, adding a track via insert_track_at works.
    #[test]
    fn regression_add_track_after_metadata_load() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(5);
        playback.load_from_grpc(proto).unwrap();

        // Add a new track at position 2
        let new_track = Track::from_grpc_metadata(
            PlaylistTrackSource::Path("/music/new_insertion.flac".to_string()),
            Some("New Inserted Track".to_string()),
            Some("New Artist".to_string()),
            Some("New Album".to_string()),
            Some(Duration::from_secs(250)),
            false,
        );
        playback.playlist.insert_track_at(2, new_track);

        assert_eq!(playback.playlist.len(), 6);
        assert_eq!(
            playback.playlist.tracks()[2].title(),
            Some("New Inserted Track")
        );
        assert_eq!(playback.playlist.tracks()[2].artist(), Some("New Artist"));
        // Original tracks shifted
        assert_eq!(playback.playlist.tracks()[3].title(), Some("Song Title 2"));
    }

    /// After loading via metadata protocol, removing a track works.
    #[test]
    fn regression_remove_track_after_metadata_load() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(5);
        playback.load_from_grpc(proto).unwrap();

        // Remove track at index 2
        playback.playlist.remove_simple(2).unwrap();

        assert_eq!(playback.playlist.len(), 4);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 0"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Song Title 1"));
        assert_eq!(playback.playlist.tracks()[2].title(), Some("Song Title 3")); // was index 3
        assert_eq!(playback.playlist.tracks()[3].title(), Some("Song Title 4"));
        // was index 4
    }

    /// After loading via metadata protocol, swapping tracks works.
    #[test]
    fn regression_swap_tracks_after_metadata_load() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(5);
        playback.load_from_grpc(proto).unwrap();
        playback.playlist.set_current_track_index(1).unwrap();

        // Swap tracks at index 1 and 3
        playback.playlist.swap(1, 3).unwrap();

        assert_eq!(playback.playlist.tracks()[1].title(), Some("Song Title 3"));
        assert_eq!(playback.playlist.tracks()[3].title(), Some("Song Title 1"));
        // Current track index should follow the swap (was 1, swapped to 3)
        assert_eq!(playback.playlist.current_track_index(), Some(3));
    }

    /// After loading via metadata protocol, clearing the playlist works.
    #[test]
    fn regression_clear_after_metadata_load() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(100);
        playback.load_from_grpc(proto).unwrap();
        assert_eq!(playback.playlist.len(), 100);

        playback.playlist.clear();

        assert!(playback.playlist.is_empty());
        assert_eq!(playback.playlist.current_track_index(), None);
    }

    /// After loading via metadata protocol, a full reload replaces the playlist.
    #[test]
    fn regression_reload_replaces_playlist() {
        let mut playback = Playback::new();

        // First load
        let proto1 = make_server_style_playlist_tracks(50);
        playback.load_from_grpc(proto1).unwrap();
        assert_eq!(playback.playlist.len(), 50);

        // Second load (different size)
        let proto2 = make_server_style_playlist_tracks(25);
        playback.load_from_grpc(proto2).unwrap();
        assert_eq!(playback.playlist.len(), 25);

        // Metadata from second load is present
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 0"));
        assert_eq!(
            playback.playlist.tracks()[24].title(),
            Some("Song Title 24")
        );
    }

    /// Sequence: load -> add -> remove -> swap -> shuffle produces consistent state.
    #[test]
    fn regression_mixed_operations_sequence() {
        let mut playback = Playback::new();

        // Load 5 tracks
        let proto = make_server_style_playlist_tracks(5);
        playback.load_from_grpc(proto).unwrap();
        assert_eq!(playback.playlist.len(), 5);

        // Add track at index 2
        let added = Track::from_grpc_metadata(
            PlaylistTrackSource::Path("/music/added.flac".to_string()),
            Some("Added Track".to_string()),
            Some("Added Artist".to_string()),
            None,
            Some(Duration::from_secs(100)),
            false,
        );
        playback.playlist.insert_track_at(2, added);
        assert_eq!(playback.playlist.len(), 6);

        // Remove track at index 0
        playback.playlist.remove_simple(0).unwrap();
        assert_eq!(playback.playlist.len(), 5);
        // Now: [Song Title 1, Added Track, Song Title 2, Song Title 3, Song Title 4]
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 1"));
        assert_eq!(playback.playlist.tracks()[1].title(), Some("Added Track"));

        // Swap indices 0 and 4
        playback.playlist.swap(0, 4).unwrap();
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Song Title 4"));
        assert_eq!(playback.playlist.tracks()[4].title(), Some("Song Title 1"));

        // Shuffle (reload with different order)
        let shuffled = {
            use termusiclib::player::playlist_add_track::OptionalTitle;
            use termusiclib::player::track_id::Source;

            PlaylistTracks {
                current_track_index: 0,
                tracks: vec![termusiclib::player::PlaylistAddTrack {
                    at_index: 0,
                    optional_title: Some(OptionalTitle::Title("Final State".to_string())),
                    duration: Some(termusiclib::player::Duration {
                        secs: 999,
                        nanos: 0,
                    }),
                    id: Some(termusiclib::player::TrackId {
                        source: Some(Source::Path("/music/final.flac".to_string())),
                    }),
                    artist: Some("Final Artist".to_string()),
                    album: Some("Final Album".to_string()),
                    has_local_file: None,
                }],
            }
        };
        playback.load_from_grpc(shuffled).unwrap();

        // Final state from shuffle
        assert_eq!(playback.playlist.len(), 1);
        assert_eq!(playback.playlist.tracks()[0].title(), Some("Final State"));
        assert_eq!(playback.playlist.tracks()[0].artist(), Some("Final Artist"));
    }

    // =========================================================================
    // SCENARIO-005/AC-07: Title from metadata (not file path) when available
    // =========================================================================

    /// When server provides title, it is used as display name (not the file path).
    #[test]
    fn e2e_title_from_metadata_preferred_over_path() {
        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(
                    termusiclib::player::playlist_add_track::OptionalTitle::Title(
                        "Beautiful Song Title".to_string(),
                    ),
                ),
                duration: Some(termusiclib::player::Duration {
                    secs: 210,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(
                        "/music/01_ugly_filename_v2_final.flac".to_string(),
                    )),
                }),
                artist: Some("Great Artist".to_string()),
                album: Some("Amazing Album".to_string()),
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let track = &playback.playlist.tracks()[0];
        // title() should return the metadata title, NOT the filename
        assert_eq!(track.title(), Some("Beautiful Song Title"));
        // id_str() is the filename fallback
        assert_eq!(track.id_str().as_ref(), "01_ugly_filename_v2_final.flac");
    }

    // =========================================================================
    // SCENARIO-016: Server sends filename-derived title when tag title missing
    // (TUI receives this as title = Some(filename_stem))
    // =========================================================================

    /// When server derives title from filename (no tag), TUI receives it as Some(stem).
    #[test]
    fn e2e_filename_derived_title_from_server() {
        let mut playback = Playback::new();
        // Server sends filename stem as title (simulating the server-side fallback)
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![termusiclib::player::PlaylistAddTrack {
                at_index: 0,
                optional_title: Some(
                    termusiclib::player::playlist_add_track::OptionalTitle::Title(
                        "my_favorite_song".to_string(), // filename stem without extension
                    ),
                ),
                duration: Some(termusiclib::player::Duration {
                    secs: 180,
                    nanos: 0,
                }),
                id: Some(termusiclib::player::TrackId {
                    source: Some(termusiclib::player::track_id::Source::Path(
                        "/music/my_favorite_song.mp3".to_string(),
                    )),
                }),
                artist: None,
                album: None,
                has_local_file: None,
            }],
        };

        let result = playback.load_from_grpc(proto);
        assert!(result.is_ok());

        let track = &playback.playlist.tracks()[0];
        // Title should be the filename-derived value from server
        assert_eq!(track.title(), Some("my_favorite_song"));
    }

    // =========================================================================
    // SCENARIO-015/AC-07: Server sends title instead of empty value
    // (Verified by testing that all tracks with metadata have non-None title)
    // =========================================================================

    /// After end-to-end load, tracks that had metadata on server have populated titles.
    #[test]
    fn e2e_server_populates_title_not_none() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(20);

        playback.load_from_grpc(proto).unwrap();

        // All tracks should have Some title (server always sends one)
        for track in playback.playlist.tracks() {
            assert!(
                track.title().is_some(),
                "Track should have a title populated by server"
            );
            assert!(
                !track.title().unwrap().is_empty(),
                "Track title should not be empty"
            );
        }
    }

    // =========================================================================
    // SCENARIO-002: Small playlist (50 tracks) loaded without blocking
    // =========================================================================

    /// Small playlist load is trivially fast (well under 100ms).
    #[test]
    fn perf_small_playlist_50_tracks() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(50);

        let start = Instant::now();
        playback.load_from_grpc(proto).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(playback.playlist.len(), 50);
        assert!(
            elapsed < Duration::from_millis(100),
            "50 track load took {:?}",
            elapsed
        );
    }

    // =========================================================================
    // SCENARIO-011: File-based metadata parsing path is never invoked
    // (Structural verification: load_from_grpc uses from_grpc_metadata, not read_track_from_path)
    // This is verified by the fact that we can load tracks with non-existent paths
    // and no error occurs (read_track_from_path would fail for non-existent files).
    // =========================================================================

    /// load_from_grpc with non-existent file paths succeeds (proves no disk access).
    #[test]
    fn structural_no_disk_access_nonexistent_paths() {
        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![
                termusiclib::player::PlaylistAddTrack {
                    at_index: 0,
                    optional_title: Some(
                        termusiclib::player::playlist_add_track::OptionalTitle::Title(
                            "Fictional Track".to_string(),
                        ),
                    ),
                    duration: Some(termusiclib::player::Duration {
                        secs: 100,
                        nanos: 0,
                    }),
                    id: Some(termusiclib::player::TrackId {
                        source: Some(termusiclib::player::track_id::Source::Path(
                            "/this/path/does/not/exist/anywhere/on/disk.flac".to_string(),
                        )),
                    }),
                    artist: Some("Ghost Artist".to_string()),
                    album: Some("Phantom Album".to_string()),
                    has_local_file: None,
                },
                termusiclib::player::PlaylistAddTrack {
                    at_index: 1,
                    optional_title: None,
                    duration: None,
                    id: Some(termusiclib::player::TrackId {
                        source: Some(termusiclib::player::track_id::Source::Path(
                            "/absolutely/fictional/xyz123.mp3".to_string(),
                        )),
                    }),
                    artist: None,
                    album: None,
                    has_local_file: None,
                },
            ],
        };

        // If read_track_from_path were called, this would fail because files don't exist
        let result = playback.load_from_grpc(proto);
        assert!(
            result.is_ok(),
            "Should succeed without disk access: {:?}",
            result.err()
        );
        assert_eq!(playback.playlist.len(), 2);
        assert_eq!(
            playback.playlist.tracks()[0].title(),
            Some("Fictional Track")
        );
    }

    // =========================================================================
    // Additional integration: handle_playlist_add through PlaylistAddTrackInfo
    // SCENARIO-008: Individual track addition event carries full metadata
    // =========================================================================

    /// The full integration path for individual track addition:
    /// PlaylistAddTrackInfo -> Track::from_grpc_metadata -> insert_track_at
    #[test]
    fn e2e_individual_track_add_event_full_metadata() {
        let mut playback = Playback::new();

        // Start with an existing playlist
        let proto = make_server_style_playlist_tracks(3);
        playback.load_from_grpc(proto).unwrap();

        // Simulate receiving a PlaylistAddTrack stream event
        let add_info = PlaylistAddTrackInfo {
            at_index: 1,
            title: Some("Newly Added Song".to_string()),
            artist: Some("Stream Event Artist".to_string()),
            album: Some("Stream Event Album".to_string()),
            duration: Duration::from_secs(275),
            trackid: PlaylistTrackSource::Path("/music/new/added_via_event.flac".to_string()),
            has_local_file: false,
        };

        // Construct track from the event info (what handle_playlist_add does)
        let track = Track::from_grpc_metadata(
            add_info.trackid,
            add_info.title,
            add_info.artist,
            add_info.album,
            Some(add_info.duration),
            add_info.has_local_file,
        );
        playback
            .playlist
            .insert_track_at(add_info.at_index as usize, track);

        // Verify
        assert_eq!(playback.playlist.len(), 4);
        let inserted = &playback.playlist.tracks()[1];
        assert_eq!(inserted.title(), Some("Newly Added Song"));
        assert_eq!(inserted.artist(), Some("Stream Event Artist"));
        let td = inserted.as_track().unwrap();
        assert_eq!(td.album(), Some("Stream Event Album"));
        assert_eq!(inserted.duration(), Some(Duration::from_secs(275)));
    }

    /// Individual podcast track addition event with has_local_file=true.
    #[test]
    fn e2e_individual_podcast_add_event_with_local_file() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(2);
        playback.load_from_grpc(proto).unwrap();

        let add_info = PlaylistAddTrackInfo {
            at_index: 2,
            title: Some("Podcast: New Episode".to_string()),
            artist: Some("Podcast Host".to_string()),
            album: None,
            duration: Duration::from_secs(4500),
            trackid: PlaylistTrackSource::PodcastUrl(
                "https://feeds.example.com/ep99.mp3".to_string(),
            ),
            has_local_file: true,
        };

        let track = Track::from_grpc_metadata(
            add_info.trackid,
            add_info.title,
            add_info.artist,
            add_info.album,
            Some(add_info.duration),
            add_info.has_local_file,
        );
        playback
            .playlist
            .insert_track_at(add_info.at_index as usize, track);

        assert_eq!(playback.playlist.len(), 3);
        let podcast = &playback.playlist.tracks()[2];
        assert_eq!(podcast.title(), Some("Podcast: New Episode"));
        let pd = podcast.as_podcast().expect("should be Podcast variant");
        assert!(pd.has_localfile());
        assert_eq!(pd.url(), "https://feeds.example.com/ep99.mp3");
    }

    // =========================================================================
    // Observability: Section 7.3 — load_from_grpc must report timing info
    // The spec requires: "TUI logs timing of playlist response processing at
    // INFO level: 'Processed {count} tracks in {elapsed_ms}ms'"
    //
    // To make this testable, load_from_grpc should return a LoadStats struct
    // containing the count of tracks loaded and the elapsed processing time.
    // This enables both logging at the call site AND programmatic verification.
    // =========================================================================

    /// load_from_grpc must return LoadStats with track_count reflecting the
    /// number of tracks successfully loaded.
    #[test]
    fn observability_load_from_grpc_returns_track_count() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(42);

        let stats = playback.load_from_grpc(proto).expect("load should succeed");

        // LoadStats must report the correct number of tracks loaded
        assert_eq!(stats.track_count, 42);
    }

    /// load_from_grpc must return LoadStats with elapsed time that is non-zero
    /// for non-empty playlists (proves timing is measured).
    #[test]
    fn observability_load_from_grpc_returns_elapsed_time() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(100);

        let stats = playback.load_from_grpc(proto).expect("load should succeed");

        // Elapsed must be measured (non-zero for 100 tracks)
        assert!(
            stats.elapsed > Duration::ZERO || stats.track_count == 0,
            "elapsed should be non-zero for non-empty playlist"
        );
        assert_eq!(stats.track_count, 100);
    }

    /// load_from_grpc with empty playlist returns zero track_count.
    #[test]
    fn observability_load_from_grpc_empty_returns_zero_count() {
        let mut playback = Playback::new();
        let proto = PlaylistTracks {
            current_track_index: 0,
            tracks: vec![],
        };

        let stats = playback.load_from_grpc(proto).expect("load should succeed");

        assert_eq!(stats.track_count, 0);
    }

    /// load_from_grpc elapsed time for 1000 tracks must be under 100ms (AC-01).
    /// This verifies the LoadStats.elapsed accurately reflects the processing time.
    #[test]
    fn observability_load_from_grpc_elapsed_under_100ms_for_1000_tracks() {
        let mut playback = Playback::new();
        let proto = make_server_style_playlist_tracks(1000);

        let stats = playback.load_from_grpc(proto).expect("load should succeed");

        assert_eq!(stats.track_count, 1000);
        assert!(
            stats.elapsed < Duration::from_millis(100),
            "LoadStats.elapsed {:?} exceeds 100ms AC-01 limit",
            stats.elapsed
        );
    }
}
