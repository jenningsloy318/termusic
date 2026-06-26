//! Phase 1 tests for TUIPlaylist::insert_track_at method.
//!
//! These tests validate:
//! - T-12: TUIPlaylist::insert_track_at method with bounds-checking
//! - T-16: Unit tests for insert_track_at (beginning, middle, end, beyond-length)
//!
//! AC References:
//! - AC-04: TUI constructs Track from gRPC-provided metadata without disk I/O
//!
//! BDD Scenario References:
//! - SCENARIO-010: TUI constructs track objects directly from server-provided metadata

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use termusiclib::player::playlist_helpers::PlaylistTrackSource;
    use termusiclib::track::Track;

    use crate::ui::model::playlist::TUIPlaylist;

    /// Helper to create a Track from gRPC metadata for testing purposes.
    fn make_test_track(path: &str, title: &str) -> Track {
        Track::from_grpc_metadata(
            PlaylistTrackSource::Path(path.to_string()),
            Some(title.to_string()),
            None,
            None,
            Some(Duration::from_secs(180)),
            false,
        )
    }

    /// Helper to create a TUIPlaylist with some pre-populated tracks.
    fn make_playlist_with_tracks(count: usize) -> TUIPlaylist {
        let mut playlist = TUIPlaylist::default();
        for i in 0..count {
            let track = make_test_track(&format!("/music/track_{i}.mp3"), &format!("Track {i}"));
            playlist.insert_track_at(i, track);
        }
        playlist
    }

    // =========================================================================
    // T-12/T-16: TUIPlaylist::insert_track_at — basic insertion
    // =========================================================================

    /// insert_track_at into an empty playlist should add the track at position 0.
    #[test]
    fn insert_track_at_empty_playlist() {
        let mut playlist = TUIPlaylist::default();
        let track = make_test_track("/music/first.mp3", "First Track");

        playlist.insert_track_at(0, track);

        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.tracks()[0].title(), Some("First Track"));
    }

    /// insert_track_at the beginning (index 0) of a non-empty playlist.
    #[test]
    fn insert_track_at_beginning() {
        let mut playlist = make_playlist_with_tracks(3);
        let new_track = make_test_track("/music/new_first.mp3", "New First");

        playlist.insert_track_at(0, new_track);

        assert_eq!(playlist.len(), 4);
        assert_eq!(playlist.tracks()[0].title(), Some("New First"));
        assert_eq!(playlist.tracks()[1].title(), Some("Track 0"));
        assert_eq!(playlist.tracks()[2].title(), Some("Track 1"));
        assert_eq!(playlist.tracks()[3].title(), Some("Track 2"));
    }

    /// insert_track_at the middle of a playlist.
    #[test]
    fn insert_track_at_middle() {
        let mut playlist = make_playlist_with_tracks(4);
        let new_track = make_test_track("/music/middle.mp3", "Middle Track");

        playlist.insert_track_at(2, new_track);

        assert_eq!(playlist.len(), 5);
        assert_eq!(playlist.tracks()[0].title(), Some("Track 0"));
        assert_eq!(playlist.tracks()[1].title(), Some("Track 1"));
        assert_eq!(playlist.tracks()[2].title(), Some("Middle Track"));
        assert_eq!(playlist.tracks()[3].title(), Some("Track 2"));
        assert_eq!(playlist.tracks()[4].title(), Some("Track 3"));
    }

    /// insert_track_at the end (index == len) should append.
    #[test]
    fn insert_track_at_end() {
        let mut playlist = make_playlist_with_tracks(3);
        let new_track = make_test_track("/music/last.mp3", "Last Track");

        playlist.insert_track_at(3, new_track);

        assert_eq!(playlist.len(), 4);
        assert_eq!(playlist.tracks()[3].title(), Some("Last Track"));
    }

    /// insert_track_at an index beyond the length should append at end (not panic).
    #[test]
    fn insert_track_at_beyond_length_appends() {
        let mut playlist = make_playlist_with_tracks(3);
        let new_track = make_test_track("/music/way_beyond.mp3", "Way Beyond");

        // index 100 is way beyond len()==3
        playlist.insert_track_at(100, new_track);

        assert_eq!(playlist.len(), 4);
        assert_eq!(playlist.tracks()[3].title(), Some("Way Beyond"));
    }

    /// insert_track_at with index == usize::MAX should append (extreme boundary).
    #[test]
    fn insert_track_at_usize_max_appends() {
        let mut playlist = make_playlist_with_tracks(2);
        let new_track = make_test_track("/music/max_index.mp3", "Max Index");

        playlist.insert_track_at(usize::MAX, new_track);

        assert_eq!(playlist.len(), 3);
        assert_eq!(playlist.tracks()[2].title(), Some("Max Index"));
    }

    /// Multiple sequential insertions at the same index shift existing tracks.
    #[test]
    fn insert_track_at_same_index_multiple_times() {
        let mut playlist = make_playlist_with_tracks(2);

        // Insert at index 1 three times
        playlist.insert_track_at(1, make_test_track("/music/a.mp3", "Insert A"));
        playlist.insert_track_at(1, make_test_track("/music/b.mp3", "Insert B"));
        playlist.insert_track_at(1, make_test_track("/music/c.mp3", "Insert C"));

        assert_eq!(playlist.len(), 5);
        // Order should be: Track 0, Insert C, Insert B, Insert A, Track 1
        assert_eq!(playlist.tracks()[0].title(), Some("Track 0"));
        assert_eq!(playlist.tracks()[1].title(), Some("Insert C"));
        assert_eq!(playlist.tracks()[2].title(), Some("Insert B"));
        assert_eq!(playlist.tracks()[3].title(), Some("Insert A"));
        assert_eq!(playlist.tracks()[4].title(), Some("Track 1"));
    }

    /// insert_track_at preserves existing track data integrity.
    #[test]
    fn insert_track_at_preserves_existing_tracks() {
        let mut playlist = TUIPlaylist::default();

        // Add tracks with different types
        let track1 = Track::from_grpc_metadata(
            PlaylistTrackSource::Path("/music/song.mp3".to_string()),
            Some("Song".to_string()),
            Some("Artist".to_string()),
            Some("Album".to_string()),
            Some(Duration::from_secs(200)),
            false,
        );
        let track2 = Track::from_grpc_metadata(
            PlaylistTrackSource::Url("http://radio.example.com/stream".to_string()),
            Some("Radio".to_string()),
            None,
            None,
            None,
            false,
        );
        let track3 = Track::from_grpc_metadata(
            PlaylistTrackSource::PodcastUrl("http://podcast.example.com/ep.mp3".to_string()),
            Some("Podcast".to_string()),
            None,
            None,
            Some(Duration::from_secs(3600)),
            true,
        );

        playlist.insert_track_at(0, track1);
        playlist.insert_track_at(1, track2);
        playlist.insert_track_at(2, track3);

        // Verify all tracks retain their type and metadata
        assert!(playlist.tracks()[0].as_track().is_some());
        assert_eq!(playlist.tracks()[0].artist(), Some("Artist"));
        assert!(playlist.tracks()[1].as_radio().is_some());
        assert!(playlist.tracks()[2].as_podcast().is_some());
        assert!(playlist.tracks()[2].as_podcast().unwrap().has_localfile());
    }
}
