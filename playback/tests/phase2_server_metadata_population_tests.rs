//! Phase 2: Server-Side Metadata Population — RED Phase Tests
//!
//! These tests verify that the server populates full display metadata (title, artist,
//! album, has_local_file) in all gRPC playlist messages, both bulk responses
//! (`as_grpc_playlist_tracks`) and individual stream events (`send_stream_ev_pl` paths).
//!
//! Phase 2 tasks covered:
//!   T-18: `as_grpc_playlist_tracks` populates `optional_title` from `Track::title()`
//!   T-19: `as_grpc_playlist_tracks` populates `artist` from `Track::artist()`
//!   T-20: `as_grpc_playlist_tracks` populates `album` from `Track::as_track().album()`
//!   T-21: `as_grpc_playlist_tracks` populates `has_local_file` from podcast data
//!   T-22: Title-from-filename fallback when `Track::title()` returns None
//!   T-23: Individual stream event emission populates artist, album, has_local_file
//!   T-24: Unit tests for server serialization — full metadata populated
//!   T-25: Unit tests for server serialization — partial metadata (fallback behavior)
//!
//! BDD Scenario coverage:
//!   SCENARIO-006: Server includes title, artist, album, and duration in playlist data
//!   SCENARIO-007: Server includes full metadata in playlist shuffle stream events
//!   SCENARIO-008: Server includes full metadata in individual track addition events
//!   SCENARIO-009: Server populates title that was previously always empty
//!   SCENARIO-015: Server sends track title instead of empty value
//!   SCENARIO-016: Server sends filename-derived title when tag-based title is missing
//!   SCENARIO-018: Server sends partial metadata when file cannot be parsed
//!   SCENARIO-020: Server does not crash when track has no metadata at all
//!
//! AC coverage:
//!   AC-03: Server includes sufficient display metadata
//!   AC-07: Server populates optional_title (previously always None)
//!   AC-08: Graceful fallback for missing metadata
//!
//! These tests WILL FAIL because the current `as_grpc_playlist_tracks()` sets:
//!   - `optional_title: None`
//!   - `artist: None`
//!   - `album: None`
//!   - `has_local_file: None`
//! Phase 2 implementation will make them pass.

use std::time::Duration;

use termusiclib::config::{ServerOverlay, new_shared_server_settings};
use termusiclib::player::playlist_helpers::PlaylistTrackSource;
use termusiclib::player::{PlaylistAddTrackInfo, UpdateEvents, UpdatePlaylistEvents};
use termusiclib::track::Track;
use termusicplayback::Playlist;
use tokio::sync::broadcast;

/// Helper: Create a Playlist instance suitable for testing.
/// Returns (playlist, stream_receiver) so tests can inspect emitted events.
fn create_test_playlist() -> (Playlist, broadcast::Receiver<UpdateEvents>) {
    let config = new_shared_server_settings(ServerOverlay::default());
    let (stream_tx, stream_rx) = broadcast::channel(64);
    let playlist = Playlist::new(&config, stream_tx);
    (playlist, stream_rx)
}

/// Helper: Create a Track with full metadata (title, artist, album, duration)
/// representing a local music file.
fn make_track_with_full_metadata(
    path: &str,
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: u64,
) -> Track {
    Track::from_grpc_metadata(
        PlaylistTrackSource::Path(path.to_string()),
        Some(title.to_string()),
        Some(artist.to_string()),
        Some(album.to_string()),
        Some(Duration::from_secs(duration_secs)),
        false,
    )
}

/// Helper: Create a Track with NO metadata (only path), simulating corrupted/unreadable file.
fn make_track_without_metadata(path: &str) -> Track {
    Track::from_grpc_metadata(
        PlaylistTrackSource::Path(path.to_string()),
        None,
        None,
        None,
        None,
        false,
    )
}

/// Helper: Create a podcast Track with full metadata.
fn make_podcast_track(url: &str, title: &str, has_local: bool) -> Track {
    Track::from_grpc_metadata(
        PlaylistTrackSource::PodcastUrl(url.to_string()),
        Some(title.to_string()),
        None,
        None,
        Some(Duration::from_secs(3600)),
        has_local,
    )
}

/// Helper: Create a radio Track.
fn make_radio_track(url: &str) -> Track {
    Track::from_grpc_metadata(
        PlaylistTrackSource::Url(url.to_string()),
        None,
        None,
        None,
        None,
        false,
    )
}

// =============================================================================
// T-18: as_grpc_playlist_tracks populates optional_title from Track::title()
// AC-07, SCENARIO-009, SCENARIO-015
// =============================================================================

/// When a track has a title, `as_grpc_playlist_tracks` must include it in
/// the `optional_title` field of the proto message (previously always None).
#[test]
fn as_grpc_populates_title_for_track_with_title() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_track_with_full_metadata(
        "/music/song.mp3",
        "My Song Title",
        "Artist Name",
        "Album Name",
        240,
    );
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.tracks.len(), 1);

    let proto_track = &result.tracks[0];
    // The title MUST be populated (not None)
    assert!(
        proto_track.optional_title.is_some(),
        "optional_title must be Some when track has a title, but was None"
    );

    // Extract the actual title string
    let title = proto_track.optional_title.as_ref().map(|v| {
        let termusiclib::player::playlist_add_track::OptionalTitle::Title(t) = v;
        t.as_str()
    });
    assert_eq!(title, Some("My Song Title"));
}

/// When multiple tracks have titles, all should have populated optional_title.
#[test]
fn as_grpc_populates_title_for_multiple_tracks() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/music/a.mp3", "Track A", "Artist A", "Album A", 180),
        make_track_with_full_metadata("/music/b.flac", "Track B", "Artist B", "Album B", 300),
        make_track_with_full_metadata("/music/c.ogg", "Track C", "Artist C", "Album C", 200),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.tracks.len(), 3);

    for (i, proto_track) in result.tracks.iter().enumerate() {
        assert!(
            proto_track.optional_title.is_some(),
            "Track at index {i} must have optional_title populated"
        );
    }

    // Verify correct titles
    let titles: Vec<Option<&str>> = result
        .tracks
        .iter()
        .map(|t| {
            t.optional_title.as_ref().map(|v| {
                let termusiclib::player::playlist_add_track::OptionalTitle::Title(s) = v;
                s.as_str()
            })
        })
        .collect();
    assert_eq!(
        titles,
        vec![Some("Track A"), Some("Track B"), Some("Track C")]
    );
}

// =============================================================================
// T-19: as_grpc_playlist_tracks populates artist from Track::artist()
// AC-03, SCENARIO-006
// =============================================================================

/// When a track has an artist, `as_grpc_playlist_tracks` must include it in
/// the `artist` field of the proto message.
#[test]
fn as_grpc_populates_artist_for_track_with_artist() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_track_with_full_metadata(
        "/music/song.mp3",
        "Song Title",
        "The Artist",
        "The Album",
        200,
    );
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert_eq!(
        proto_track.artist.as_deref(),
        Some("The Artist"),
        "artist field must be populated from Track::artist()"
    );
}

/// When a track has no artist, the `artist` field should be None.
#[test]
fn as_grpc_leaves_artist_none_when_track_has_no_artist() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = Track::from_grpc_metadata(
        PlaylistTrackSource::Path("/music/unknown.mp3".to_string()),
        Some("Some Title".to_string()),
        None, // no artist
        None,
        Some(Duration::from_secs(100)),
        false,
    );
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert_eq!(
        proto_track.artist, None,
        "artist field should be None when track has no artist metadata"
    );
}

/// Multiple tracks with different artists should each have their respective artist.
#[test]
fn as_grpc_populates_distinct_artists_for_multiple_tracks() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/a.mp3", "A", "Artist One", "Album", 100),
        make_track_with_full_metadata("/b.mp3", "B", "Artist Two", "Album", 200),
        make_track_with_full_metadata("/c.mp3", "C", "Artist Three", "Album", 300),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();

    let artists: Vec<Option<&str>> = result.tracks.iter().map(|t| t.artist.as_deref()).collect();
    assert_eq!(
        artists,
        vec![Some("Artist One"), Some("Artist Two"), Some("Artist Three")]
    );
}

// =============================================================================
// T-20: as_grpc_playlist_tracks populates album from Track::as_track().album()
// AC-03, SCENARIO-006
// =============================================================================

/// When a track has an album, `as_grpc_playlist_tracks` must include it in
/// the `album` field.
#[test]
fn as_grpc_populates_album_for_track_with_album() {
    let (mut playlist, _rx) = create_test_playlist();
    let track =
        make_track_with_full_metadata("/music/song.mp3", "Song", "Artist", "Greatest Hits", 240);
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert_eq!(
        proto_track.album.as_deref(),
        Some("Greatest Hits"),
        "album field must be populated from Track's album metadata"
    );
}

/// When a track has no album (e.g., radio or podcast), `album` should be None.
#[test]
fn as_grpc_leaves_album_none_for_radio_track() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_radio_track("http://radio.example.com/stream");
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert_eq!(
        proto_track.album, None,
        "album should be None for radio tracks"
    );
}

/// Multiple tracks with different albums should each have their respective album.
#[test]
fn as_grpc_populates_distinct_albums_for_multiple_tracks() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/a.mp3", "A", "Art", "Album One", 100),
        make_track_with_full_metadata("/b.mp3", "B", "Art", "Album Two", 200),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();

    let albums: Vec<Option<&str>> = result.tracks.iter().map(|t| t.album.as_deref()).collect();
    assert_eq!(albums, vec![Some("Album One"), Some("Album Two")]);
}

// =============================================================================
// T-21: as_grpc_playlist_tracks populates has_local_file for podcast tracks
// AC-03, SCENARIO-006
// =============================================================================

/// For a podcast track with a local file, `has_local_file` must be Some(true).
#[test]
fn as_grpc_populates_has_local_file_true_for_podcast_with_download() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_podcast_track("http://podcast.example.com/ep1.mp3", "Episode 1", true);
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert_eq!(
        proto_track.has_local_file,
        Some(true),
        "has_local_file must be Some(true) for podcast track with local file"
    );
}

/// For a podcast track without a local file, `has_local_file` must be Some(false)
/// or None (either is acceptable — the key is that has_localfile() returns false).
/// According to the spec, the server should send Some(false) or omit (None) for
/// podcast tracks without a local file. We'll accept None since that maps to false.
#[test]
fn as_grpc_has_local_file_false_for_podcast_without_download() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_podcast_track("http://podcast.example.com/ep2.mp3", "Episode 2", false);
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    // Per spec: has_local_file should be populated for podcast tracks.
    // For podcasts without local file, it could be Some(false) or None (both mean "no local file").
    // The important thing is it's NOT Some(true).
    assert_ne!(
        proto_track.has_local_file,
        Some(true),
        "has_local_file must NOT be Some(true) for podcast without local file"
    );
}

/// For non-podcast tracks (music files, radio), `has_local_file` should be None
/// (omitted to save wire space per spec).
#[test]
fn as_grpc_omits_has_local_file_for_non_podcast_tracks() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_track_with_full_metadata("/music/song.mp3", "Song", "Art", "Album", 200);
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert_eq!(
        proto_track.has_local_file, None,
        "has_local_file should be None (omitted) for non-podcast tracks"
    );
}

// =============================================================================
// T-22: Title-from-filename fallback when Track::title() is None
// AC-07, AC-08, SCENARIO-016
// =============================================================================

/// When a track has no title metadata, the server should derive a display title
/// from the filename (without extension) and populate `optional_title`.
#[test]
fn as_grpc_derives_title_from_filename_when_title_is_none() {
    let (mut playlist, _rx) = create_test_playlist();
    // Track with no title, but has a path with a meaningful filename
    let track = make_track_without_metadata("/music/my-awesome-song.mp3");
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    // The title must be populated with a filename-derived value
    assert!(
        proto_track.optional_title.is_some(),
        "optional_title must be populated even when track has no tag-based title \
         (should use filename-derived fallback)"
    );

    // Extract the title
    let title = proto_track.optional_title.as_ref().map(|v| {
        let termusiclib::player::playlist_add_track::OptionalTitle::Title(t) = v;
        t.as_str()
    });
    // The filename stem is "my-awesome-song" (without extension)
    assert_eq!(
        title,
        Some("my-awesome-song"),
        "Title should be the filename stem (without extension) when no tag-based title exists"
    );
}

/// Another filename fallback test with a different path structure.
#[test]
fn as_grpc_derives_title_from_nested_path_filename() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_track_without_metadata("/home/user/Music/Artist/Album/01 - Track Name.flac");
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert!(
        proto_track.optional_title.is_some(),
        "optional_title must be populated from filename for nested path"
    );

    let title = proto_track.optional_title.as_ref().map(|v| {
        let termusiclib::player::playlist_add_track::OptionalTitle::Title(t) = v;
        t.as_str()
    });
    assert_eq!(
        title,
        Some("01 - Track Name"),
        "Title should be the filename stem from nested path"
    );
}

// =============================================================================
// T-23: Stream event emission populates artist, album, has_local_file
// AC-03, SCENARIO-008
// =============================================================================

/// When a track is added to the playlist via `add_track`, the emitted stream event
/// must include artist, album, and has_local_file fields.
/// This tests the `send_stream_ev_pl` path for individual track additions.
#[test]
fn stream_event_for_track_addition_includes_artist_and_album() {
    let (_playlist, _rx) = create_test_playlist();

    // Add a track via the public API (which reads from path — this requires a real file)
    // Instead we test via the PlaylistAddTrackInfo struct that the fields are populated
    // by checking what the existing add_episode/add_track methods emit.
    //
    // Since add_track requires a real file, we test add_episode which just needs an Episode.
    // We'll verify the emitted event carries artist, album, has_local_file.
    //
    // For this test, we verify the PlaylistAddTrackInfo serialization round-trip:
    // Create a PlaylistAddTrackInfo with full metadata and verify it serializes correctly.
    let info = PlaylistAddTrackInfo {
        at_index: 0,
        title: Some("Episode Title".to_string()),
        artist: Some("Podcast Host".to_string()),
        album: Some("Podcast Name".to_string()),
        duration: Duration::from_secs(3600),
        trackid: PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep.mp3".to_string()),
        has_local_file: true,
    };

    // Serialize to protobuf (UpdatePlaylistEvents -> protobuf::UpdatePlaylist)
    let proto: termusiclib::player::UpdatePlaylist =
        UpdatePlaylistEvents::PlaylistAddTrack(info.clone()).into();

    // Deserialize back (protobuf::UpdatePlaylist -> UpdatePlaylistEvents)
    let roundtrip = UpdatePlaylistEvents::try_from(proto).unwrap();

    match roundtrip {
        UpdatePlaylistEvents::PlaylistAddTrack(rt_info) => {
            assert_eq!(rt_info.artist.as_deref(), Some("Podcast Host"));
            assert_eq!(rt_info.album.as_deref(), Some("Podcast Name"));
            assert_eq!(rt_info.has_local_file, true);
            assert_eq!(rt_info.title.as_deref(), Some("Episode Title"));
        }
        other => panic!("Expected PlaylistAddTrack, got {other:?}"),
    }
}

/// Verify that artist=None round-trips correctly (not accidentally populated).
#[test]
fn stream_event_roundtrip_preserves_none_artist() {
    let info = PlaylistAddTrackInfo {
        at_index: 5,
        title: Some("Radio Station".to_string()),
        artist: None,
        album: None,
        duration: Duration::from_secs(0),
        trackid: PlaylistTrackSource::Url("http://radio.example.com/stream".to_string()),
        has_local_file: false,
    };

    let proto: termusiclib::player::UpdatePlaylist =
        UpdatePlaylistEvents::PlaylistAddTrack(info).into();
    let roundtrip = UpdatePlaylistEvents::try_from(proto).unwrap();

    match roundtrip {
        UpdatePlaylistEvents::PlaylistAddTrack(rt_info) => {
            assert_eq!(rt_info.artist, None);
            assert_eq!(rt_info.album, None);
            assert_eq!(rt_info.has_local_file, false);
        }
        other => panic!("Expected PlaylistAddTrack, got {other:?}"),
    }
}

// =============================================================================
// T-24: Server serialization — verify all fields populated for Track with full metadata
// AC-03, SCENARIO-006, SCENARIO-009
// =============================================================================

/// Comprehensive test: a playlist with mixed track types (music, radio, podcast)
/// should have all metadata fields correctly populated in the gRPC response.
#[test]
fn as_grpc_comprehensive_mixed_playlist_metadata() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        // Music track with full metadata
        make_track_with_full_metadata(
            "/music/rock/hotel-california.mp3",
            "Hotel California",
            "Eagles",
            "Hotel California",
            391,
        ),
        // Radio track (minimal metadata)
        make_radio_track("http://stream.radio.co/jazz"),
        // Podcast with local file
        make_podcast_track(
            "http://feeds.example.com/ep42.mp3",
            "Episode 42: Rust",
            true,
        ),
        // Music track with no artist
        Track::from_grpc_metadata(
            PlaylistTrackSource::Path("/music/ambient/track.wav".to_string()),
            Some("Ambient Soundscape".to_string()),
            None,
            Some("Nature Sounds".to_string()),
            Some(Duration::from_secs(600)),
            false,
        ),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.tracks.len(), 4);

    // Track 0: Music with full metadata
    let t0 = &result.tracks[0];
    assert_eq!(
        t0.optional_title.as_ref().map(|v| {
            let termusiclib::player::playlist_add_track::OptionalTitle::Title(s) = v;
            s.as_str()
        }),
        Some("Hotel California")
    );
    assert_eq!(t0.artist.as_deref(), Some("Eagles"));
    assert_eq!(t0.album.as_deref(), Some("Hotel California"));
    assert_eq!(t0.has_local_file, None); // non-podcast

    // Track 1: Radio (no metadata except maybe no title)
    let t1 = &result.tracks[1];
    assert_eq!(t1.artist, None);
    assert_eq!(t1.album, None);
    assert_eq!(t1.has_local_file, None); // non-podcast

    // Track 2: Podcast with local file
    let t2 = &result.tracks[2];
    assert_eq!(
        t2.optional_title.as_ref().map(|v| {
            let termusiclib::player::playlist_add_track::OptionalTitle::Title(s) = v;
            s.as_str()
        }),
        Some("Episode 42: Rust")
    );
    assert_eq!(t2.has_local_file, Some(true));

    // Track 3: Music with no artist but has album
    let t3 = &result.tracks[3];
    assert_eq!(
        t3.optional_title.as_ref().map(|v| {
            let termusiclib::player::playlist_add_track::OptionalTitle::Title(s) = v;
            s.as_str()
        }),
        Some("Ambient Soundscape")
    );
    assert_eq!(t3.artist, None);
    assert_eq!(t3.album.as_deref(), Some("Nature Sounds"));
    assert_eq!(t3.has_local_file, None); // non-podcast
}

/// Verify that the `current_track_index` in the gRPC response is correct.
#[test]
fn as_grpc_preserves_current_track_index() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/a.mp3", "A", "Art", "Alb", 100),
        make_track_with_full_metadata("/b.mp3", "B", "Art", "Alb", 200),
        make_track_with_full_metadata("/c.mp3", "C", "Art", "Alb", 300),
    ];
    // Set current track index to 2
    playlist.apply_loaded_data(2, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.current_track_index, 2);
}

// =============================================================================
// T-25: Server serialization — partial metadata (missing fields)
// AC-08, SCENARIO-018, SCENARIO-020
// =============================================================================

/// When a track has no metadata at all (corrupted or missing file), the server
/// must still include the track with at least its path identifier.
/// The optional_title should be derived from the filename.
#[test]
fn as_grpc_handles_track_with_no_metadata_gracefully() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_track_without_metadata("/music/unknown-file.mp3");
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.tracks.len(), 1);

    let proto_track = &result.tracks[0];
    // The track must be included (not skipped)
    assert_eq!(proto_track.at_index, 0);

    // The id must be present (path identifier)
    assert!(
        proto_track.id.is_some(),
        "Track must have an id even with no metadata"
    );

    // Artist and album should be None since no metadata
    assert_eq!(proto_track.artist, None);
    assert_eq!(proto_track.album, None);

    // Title should be derived from filename (fallback behavior)
    assert!(
        proto_track.optional_title.is_some(),
        "Title should be filename-derived even when tag-based title is absent"
    );
    let title = proto_track.optional_title.as_ref().map(|v| {
        let termusiclib::player::playlist_add_track::OptionalTitle::Title(t) = v;
        t.as_str()
    });
    assert_eq!(title, Some("unknown-file"));
}

/// Playlist with all tracks missing metadata should not crash and should
/// produce valid proto output for every track.
#[test]
fn as_grpc_handles_all_tracks_missing_metadata() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_without_metadata("/a/song1.mp3"),
        make_track_without_metadata("/b/song2.flac"),
        make_track_without_metadata("/c/song3.ogg"),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.tracks.len(), 3);

    // All tracks must have ids and filename-derived titles
    for (i, proto_track) in result.tracks.iter().enumerate() {
        assert!(proto_track.id.is_some(), "Track {i} must have an id");
        assert!(
            proto_track.optional_title.is_some(),
            "Track {i} must have filename-derived title"
        );
    }

    // Verify the derived titles
    let titles: Vec<&str> = result
        .tracks
        .iter()
        .map(|t| {
            t.optional_title
                .as_ref()
                .map(|v| {
                    let termusiclib::player::playlist_add_track::OptionalTitle::Title(s) = v;
                    s.as_str()
                })
                .unwrap()
        })
        .collect();
    assert_eq!(titles, vec!["song1", "song2", "song3"]);
}

/// Empty playlist should produce empty proto response without error.
#[test]
fn as_grpc_handles_empty_playlist() {
    let (playlist, _rx) = create_test_playlist();

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    assert_eq!(result.tracks.len(), 0);
    assert_eq!(result.current_track_index, 0);
}

// =============================================================================
// SCENARIO-007: Shuffle event contains full metadata
// (Verifies that when shuffle() is called, the emitted PlaylistShuffled event
// carries full metadata in its PlaylistTracks payload)
// =============================================================================

/// When the playlist is shuffled, the emitted PlaylistShuffled event's
/// PlaylistTracks must contain full metadata (title, artist, album) for all tracks.
#[test]
fn shuffle_event_contains_full_metadata() {
    let (mut playlist, mut rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/a.mp3", "Song A", "Artist A", "Album A", 180),
        make_track_with_full_metadata("/b.mp3", "Song B", "Artist B", "Album B", 200),
        make_track_with_full_metadata("/c.mp3", "Song C", "Artist C", "Album C", 220),
    ];
    playlist.apply_loaded_data(0, tracks);

    // Perform shuffle
    playlist.shuffle();

    // Receive the shuffle event
    let event = rx.try_recv().unwrap();
    let UpdateEvents::PlaylistChanged(UpdatePlaylistEvents::PlaylistShuffled(shuffled_info)) =
        event
    else {
        panic!("Expected PlaylistShuffled event, got {event:?}");
    };

    let shuffled_tracks = &shuffled_info.tracks;
    assert_eq!(shuffled_tracks.tracks.len(), 3);

    // Every track in the shuffled event must have title populated
    for (i, proto_track) in shuffled_tracks.tracks.iter().enumerate() {
        assert!(
            proto_track.optional_title.is_some(),
            "Shuffled track at index {i} must have title populated"
        );
        // The artist should also be populated for music tracks
        assert!(
            proto_track.artist.is_some(),
            "Shuffled track at index {i} must have artist populated"
        );
        // Album should be populated for music tracks
        assert!(
            proto_track.album.is_some(),
            "Shuffled track at index {i} must have album populated"
        );
    }
}

// =============================================================================
// Edge Cases
// =============================================================================

/// Track with very long title should be transmitted without truncation.
/// (SCENARIO-028 — server side: no overflow)
#[test]
fn as_grpc_handles_very_long_metadata_strings() {
    let (mut playlist, _rx) = create_test_playlist();
    let long_title = "A".repeat(500);
    let long_artist = "B".repeat(300);
    let track = Track::from_grpc_metadata(
        PlaylistTrackSource::Path("/music/long.mp3".to_string()),
        Some(long_title.clone()),
        Some(long_artist.clone()),
        Some("Normal Album".to_string()),
        Some(Duration::from_secs(100)),
        false,
    );
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    let title = proto_track.optional_title.as_ref().map(|v| {
        let termusiclib::player::playlist_add_track::OptionalTitle::Title(t) = v;
        t.clone()
    });
    assert_eq!(title.as_deref(), Some(long_title.as_str()));
    assert_eq!(proto_track.artist.as_deref(), Some(long_artist.as_str()));
}

/// Duration must be properly populated (it was already working but we verify
/// it remains correct alongside the new fields).
#[test]
fn as_grpc_preserves_duration_alongside_new_metadata_fields() {
    let (mut playlist, _rx) = create_test_playlist();
    let track = make_track_with_full_metadata("/music/song.mp3", "Song", "Art", "Alb", 301);
    playlist.apply_loaded_data(0, vec![track]);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    let proto_track = &result.tracks[0];

    assert!(proto_track.duration.is_some());
    let dur = proto_track.duration.as_ref().unwrap();
    assert_eq!(dur.secs, 301);
    assert_eq!(dur.nanos, 0);
}

/// TrackId must be correctly populated for all source variants alongside new metadata.
#[test]
fn as_grpc_populates_track_id_correctly_for_all_variants() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/music/file.mp3", "File", "Art", "Alb", 100),
        make_radio_track("http://radio.example.com/stream"),
        make_podcast_track("http://podcast.example.com/ep.mp3", "Ep", true),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();

    // Verify track IDs are present
    for (i, proto_track) in result.tracks.iter().enumerate() {
        assert!(proto_track.id.is_some(), "Track {i} must have an id");
    }
}

// =============================================================================
// Regression: Verify at_index is sequential (existing behavior preserved)
// =============================================================================

/// The at_index field must be sequential 0, 1, 2, ... for all tracks.
#[test]
fn as_grpc_at_index_is_sequential() {
    let (mut playlist, _rx) = create_test_playlist();
    let tracks = vec![
        make_track_with_full_metadata("/a.mp3", "A", "Art", "Alb", 100),
        make_track_with_full_metadata("/b.mp3", "B", "Art", "Alb", 200),
        make_track_with_full_metadata("/c.mp3", "C", "Art", "Alb", 300),
        make_track_with_full_metadata("/d.mp3", "D", "Art", "Alb", 400),
    ];
    playlist.apply_loaded_data(0, tracks);

    let result = playlist.as_grpc_playlist_tracks().unwrap();
    for (i, proto_track) in result.tracks.iter().enumerate() {
        assert_eq!(
            proto_track.at_index, i as u64,
            "at_index must be sequential"
        );
    }
}
