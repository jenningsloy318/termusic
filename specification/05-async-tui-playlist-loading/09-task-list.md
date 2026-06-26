# Task List: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Author**: super-dev:spec-writer
- **Specification**: ./07-specification.md
- **Implementation Plan**: ./08-implementation-plan.md
- **Total Tasks**: 35

---

## Phase 1: Protocol Extension and Domain Struct Updates

**Milestone**: Extended proto schema, new Track constructor, updated domain struct — all additive, zero behavioral change

- [ ] **T-01**: Add `optional string artist = 5` field to PlaylistAddTrack message in player.proto
  - Files: lib/proto/player.proto
  - Type: modify
  - Effort: small
  - Depends on: None

- [ ] **T-02**: Add `optional string album = 6` field to PlaylistAddTrack message in player.proto
  - Files: lib/proto/player.proto
  - Type: modify
  - Effort: small
  - Depends on: None

- [ ] **T-03**: Add `optional bool has_local_file = 7` field to PlaylistAddTrack message in player.proto
  - Files: lib/proto/player.proto
  - Type: modify
  - Effort: small
  - Depends on: None

- [ ] **T-04**: Run cargo build to regenerate proto bindings and verify compilation
  - Files: lib/proto/player.proto (verification)
  - Type: verify
  - Effort: small
  - Depends on: T-01, T-02, T-03

- [ ] **T-05**: Add artist, album, has_local_file fields to PlaylistAddTrackInfo domain struct
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-04

- [ ] **T-06**: Update From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist to serialize artist, album, has_local_file
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-05

- [ ] **T-07**: Update TryFrom<protobuf::UpdatePlaylist> for UpdatePlaylistEvents to deserialize artist, album, has_local_file
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-05

- [ ] **T-08**: Update all existing PlaylistAddTrackInfo constructors in playback crate to populate new fields (artist from track.artist(), album from track.as_track().album(), has_local_file from track.as_podcast().has_localfile())
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-05

- [ ] **T-09**: Create Track::from_grpc_metadata constructor for Path source variant
  - Files: lib/src/track.rs
  - Type: modify
  - Effort: small
  - Depends on: T-04

- [ ] **T-10**: Extend Track::from_grpc_metadata for Url source variant
  - Files: lib/src/track.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09

- [ ] **T-11**: Extend Track::from_grpc_metadata for PodcastUrl source variant with sentinel PathBuf logic
  - Files: lib/src/track.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09

- [ ] **T-12**: Add TUIPlaylist::insert_track_at method with bounds-checking
  - Files: tui/src/ui/model/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: None

- [ ] **T-13**: Unit tests for Track::from_grpc_metadata — Path variant (title, artist, album populated correctly)
  - Files: lib/src/track.rs
  - Type: modify
  - Effort: small
  - Depends on: T-09

- [ ] **T-14**: Unit tests for Track::from_grpc_metadata — PodcastUrl variant (has_local_file sentinel logic)
  - Files: lib/src/track.rs
  - Type: modify
  - Effort: small
  - Depends on: T-11

- [ ] **T-15**: Unit tests for Track::from_grpc_metadata — Url variant and None metadata fields
  - Files: lib/src/track.rs
  - Type: modify
  - Effort: small
  - Depends on: T-10

- [ ] **T-16**: Unit tests for TUIPlaylist::insert_track_at (beginning, middle, end, beyond-length)
  - Files: tui/src/ui/model/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-12

- [ ] **T-17**: Unit tests for PlaylistAddTrackInfo serialization round-trip with new fields
  - Files: lib/src/player.rs
  - Type: modify
  - Effort: small
  - Depends on: T-06, T-07

---

## Phase 2: Server-Side Metadata Population

**Milestone**: Server sends full display metadata (title, artist, album, has_local_file) in all playlist messages

- [ ] **T-18**: Update as_grpc_playlist_tracks to populate optional_title from Track::title() (replacing the current None)
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-04

- [ ] **T-19**: Update as_grpc_playlist_tracks to populate artist field from Track::artist()
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-18

- [ ] **T-20**: Update as_grpc_playlist_tracks to populate album field from Track::as_track().album()
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-18

- [ ] **T-21**: Update as_grpc_playlist_tracks to populate has_local_file from Track::as_podcast().has_localfile()
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-18

- [ ] **T-22**: Add title-from-filename fallback in as_grpc_playlist_tracks when Track::title() returns None
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-18

- [ ] **T-23**: Update individual stream event emission (send_stream_ev_pl paths: insert_track, add_track_back, add_track_front) to populate artist, album, has_local_file
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-08

- [ ] **T-24**: Unit tests for server serialization — verify all metadata fields populated for Track with full metadata
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-19, T-20, T-21

- [ ] **T-25**: Unit tests for server serialization — verify partial metadata (missing title uses filename, missing artist/album are None)
  - Files: playback/src/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-22

---

## Phase 3: TUI Playlist Loading Rewrite

**Milestone**: TUI loads and displays playlist with zero filesystem access — pure in-memory transformation from gRPC data

- [ ] **T-26**: Rewrite Playback::load_from_grpc to use Track::from_grpc_metadata — remove all Track::read_track_from_path calls and db_pod parameter
  - Files: tui/src/ui/model/mod.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-09, T-10, T-11

- [ ] **T-27**: Update all callers of load_from_grpc to remove db_pod argument (update.rs FullPlaylist handler, handle_playlist_shuffled)
  - Files: tui/src/ui/model/update.rs, tui/src/ui/components/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-26

- [ ] **T-28**: Rewrite handle_playlist_add to construct Track via from_grpc_metadata and call insert_track_at
  - Files: tui/src/ui/components/playlist.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-12, T-09

- [ ] **T-29**: Deprecate or remove TUIPlaylist::track_from_path and track_from_podcasturi (verify no remaining callers)
  - Files: tui/src/ui/model/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-26, T-28

- [ ] **T-30**: Remove resolved TODO comments at playlist.rs:173 and playlist.rs:187
  - Files: tui/src/ui/model/playlist.rs
  - Type: modify
  - Effort: small
  - Depends on: T-29

- [ ] **T-31**: Verify full workspace compilation and all existing tests pass (cargo test --workspace)
  - Files: (workspace-wide)
  - Type: verify
  - Effort: small
  - Depends on: T-26, T-27, T-28

---

## Phase 4: Integration Testing and Validation

**Milestone**: Comprehensive test suite proving zero-I/O loading, correctness, and performance compliance with all ACs

- [ ] **T-32**: Create integration test file and implement end-to-end test: server proto output fed to load_from_grpc produces correct TUIPlaylist (covers SCENARIO-006, SCENARIO-010)
  - Files: tui/src/ui/model/async_tui_loading_tests.rs
  - Type: create
  - Effort: medium
  - Depends on: T-31

- [ ] **T-33**: Integration tests for edge cases: empty playlist (SCENARIO-024), all-missing-metadata (SCENARIO-025), missing duration (SCENARIO-019), long metadata strings (SCENARIO-028)
  - Files: tui/src/ui/model/async_tui_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-32

- [ ] **T-34**: Performance tests: load_from_grpc with 1000 and 5000 tracks under 100ms (AC-01, SCENARIO-001, SCENARIO-026); playlist_sync under 50ms for 1000 tracks (AC-09, SCENARIO-021)
  - Files: tui/src/ui/model/async_tui_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-32

- [ ] **T-35**: Integration tests for shuffle event processing (SCENARIO-012, SCENARIO-013), concurrent reload/shuffle consistency (SCENARIO-027), and regression tests for all playlist operations (SCENARIO-023, AC-10)
  - Files: tui/src/ui/model/async_tui_loading_tests.rs
  - Type: modify
  - Effort: medium
  - Depends on: T-32

---

## Summary

- Phase 1: Protocol Extension and Domain Struct Updates — 17 tasks, small effort
- Phase 2: Server-Side Metadata Population — 8 tasks, small effort
- Phase 3: TUI Playlist Loading Rewrite — 6 tasks, medium effort
- Phase 4: Integration Testing and Validation — 4 tasks, medium effort
- **Total**: 35 tasks, 2 days estimated effort
