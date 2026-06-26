# Implementation Summary: Async TUI Playlist Loading

---

## Phase 1 — Protocol Extension and Domain Struct Updates

- **Date**: 2026-06-27
- **Author**: super-dev:impl-summary-writer
- **Phase**: 1 — Protocol Extension and Domain Struct Updates
- **Status**: completed

---

### Overview

Phase 1 extended the gRPC protocol with three new optional fields (artist, album, has_local_file) on the PlaylistAddTrack message, created the `Track::from_grpc_metadata` constructor for zero-I/O Track assembly on the TUI side, added the `TUIPlaylist::insert_track_at` method, and updated all `PlaylistAddTrackInfo` serialization paths. All changes are purely additive with no behavioral regressions — 588 workspace tests pass including 38 new Phase 1 unit tests.

### Files Changed

- `lib/proto/player.proto` — modified, +5/-0
  - Purpose: Added optional string artist (field 5), optional string album (field 6), and optional bool has_local_file (field 7) to the PlaylistAddTrack proto message for zero-I/O TUI rendering metadata.

- `lib/src/lib.rs` — modified, +3/-0
  - Purpose: Registered the new `async_tui_phase1_tests` test module for compilation under `#[cfg(test)]`.

- `lib/src/player.rs` — modified, +13/-0
  - Purpose: Extended PlaylistAddTrackInfo struct with artist, album, has_local_file fields. Updated the From<UpdatePlaylistEvents> serializer and TryFrom<protobuf::UpdatePlaylist> deserializer to handle the new fields with correct Option/bool semantics.

- `lib/src/track.rs` — modified, +52/-0
  - Purpose: Implemented the `Track::from_grpc_metadata` constructor which builds a Track from pre-parsed gRPC fields without disk I/O. Handles Path (TrackData), Url (RadioTrackData), and PodcastUrl (PodcastTrackData with sentinel PathBuf for has_local_file) source variants.

- `playback/src/playlist.rs` — modified, +27/-0
  - Purpose: Updated all four PlaylistAddTrackInfo construction sites (add_podcast_track, add_track_to_back, add_tracks_from_paths with insert and append) to populate the new artist, album, and has_local_file fields from Track metadata. Also updated as_grpc_playlist_tracks to include the new fields as None for wire compatibility.

- `tui/src/ui/model/mod.rs` — modified, +4/-1
  - Purpose: Made the playlist module public (`pub mod playlist`) so tests can access TUIPlaylist. Registered the `async_tui_phase1_playlist_tests` test module.

- `tui/src/ui/model/playlist.rs` — modified, +11/-0
  - Purpose: Added the `TUIPlaylist::insert_track_at` method with bounds-checking (appends when index >= len, inserts otherwise).

- `lib/src/async_tui_phase1_tests.rs` — created, +320/-0
  - Purpose: 30 unit tests covering Track::from_grpc_metadata (all three source variants, sentinel PathBuf logic, None metadata, empty strings), PlaylistAddTrackInfo struct fields, serialization round-trip for artist/album/has_local_file, and proto field existence verification.

- `tui/src/ui/model/async_tui_phase1_playlist_tests.rs` — created, +130/-0
  - Purpose: 8 unit tests covering TUIPlaylist::insert_track_at at beginning, middle, end, beyond-length, usize::MAX, multiple sequential insertions at same index, and mixed Track type preservation.

### Key Decisions

#### 1. Sentinel PathBuf for has_local_file in PodcastTrackData

- **Context**: The PodcastTrackData struct uses `localfile: Option<PathBuf>` to signal whether a downloaded file exists. The actual file path is not needed on the TUI side, only the boolean presence indicator.
- **Decision**: Use `Some(PathBuf::new())` (empty path) as a sentinel value when `has_local_file=true`, and `None` when false.
- **Rationale**: This reuses the existing has_localfile() method (`localfile.is_some()`) without introducing a new field or breaking the existing data model. The empty PathBuf is never used for I/O on the TUI side.
- **Reference**: `lib/src/track.rs`

#### 2. has_local_file serialized as Option<bool> with None meaning false

- **Context**: The proto field `optional bool has_local_file = 7` maps to `Option<bool>` in Rust. The domain struct uses a plain `bool`.
- **Decision**: Serialize `true` as `Some(true)` and `false` as `None` (omitted on wire). Deserialize with `.unwrap_or(false)`.
- **Rationale**: This minimizes wire overhead for the common case (non-podcast tracks where has_local_file is always false) and maintains backward wire compatibility — older readers simply ignore the absent field.
- **Reference**: `lib/src/player.rs`

#### 3. Album populated from podcast tracks as None

- **Context**: The `add_podcast_track` method in playback constructs a PlaylistAddTrackInfo for podcast episodes. Podcast episodes do not have an album concept.
- **Decision**: Explicitly set `album: track.as_podcast().and(None::<String>)` — always None for podcast tracks.
- **Rationale**: This makes the semantic difference clear in code: podcast episodes have no album, while regular tracks derive album from TrackData.
- **Reference**: `playback/src/playlist.rs`

#### 4. Made playlist module public in TUI

- **Context**: The TUIPlaylist struct needed to be accessible from the new test module which lives at the same level as the playlist module.
- **Decision**: Changed `mod playlist` to `pub mod playlist` in `tui/src/ui/model/mod.rs`.
- **Rationale**: Enables test access and will also be needed by Phase 3 which rewrites callers that reference TUIPlaylist directly. The struct was already effectively public through re-exports.
- **Reference**: `tui/src/ui/model/mod.rs`

### Deviations from Spec

No deviations from specification.

### Test Results

- **Unit Tests**: 588/588 passing (38 new Phase 1 tests across 2 test modules)
- **Integration Tests**: 0/0 (scheduled for Phase 4)

### Next Steps

Phase 1 complete. All 17 tasks (T-01 through T-17) are implemented and verified. Ready to proceed to Phase 2 (Server-Side Metadata Population) which will populate the new proto fields with actual Track metadata in as_grpc_playlist_tracks and stream events.
