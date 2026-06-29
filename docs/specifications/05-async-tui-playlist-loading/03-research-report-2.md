# Deep Research Report: Async TUI Playlist Loading (Iteration 2)

- **Date**: 2026-06-26
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-26
- **Technologies**: Rust, prost 0.14.4, tonic 0.14.6, protobuf3, lofty 0.24.0, tokio 1.52
- **Freshness**: Fresh (< 6mo)

---

## Executive Summary

- ISS-001 (oneof vs optional inconsistency) is **resolved**: Keep existing `oneof optional_title` unchanged for wire stability; add new fields as `optional string artist = 5` and `optional string album = 6`. The Rust access pattern difference is manageable with a simple helper method (SRC-014, SRC-015).
- ISS-002 (PlaylistAddTrackInfo missing artist/album) is **resolved**: Add `artist: Option<String>` and `album: Option<String>` fields to `PlaylistAddTrackInfo` struct, populate them in all three emission sites, and use them in TUI's `handle_playlist_add` to construct Track without disk I/O (SRC-016, SRC-017).
- ISS-003 (file_type will be None) is **resolved**: `file_type` is only used by the tag editor (`TETrack`), which either reads metadata from disk independently when opened, or gracefully handles `None` in the lyric adjustment path. `FileType::from_path()` from lofty can infer the type from extension without I/O if needed in future (SRC-018, SRC-019).
- ISS-004 (podcast episode metadata) is **resolved**: For playlist display, only `title`, `duration`, and `has_localfile` are needed. The server already holds podcast Tracks in memory with all these fields populated. Transmitting title + duration via gRPC is sufficient; `has_localfile` can be sent as a boolean flag (SRC-020, SRC-021).
- ISS-005 (both bulk and individual event emission) is **resolved**: Three code paths need updating: `as_grpc_playlist_tracks()`, `add_track()`, and `add_episode()` (plus the `add_tracks()` batch variant). All have access to the Track's metadata at emission time (SRC-016, SRC-022).
- **Recommendation** (High confidence): Proceed with protocol extension using Option A (proto field addition). All five issues have clear resolution paths with no blocking concerns.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| prost oneof vs optional field access patterns proto3 | DeepWiki (tokio-rs/prost) | 1 | 1 |
| termusic TUI handle_playlist_add track construction | DeepWiki (tramhao/termusic) | 1 | 1 |
| lofty FileType::from_path from_ext without disk IO | DeepWiki (Serial-ATA/lofty-rs) | 1 | 1 |
| file_type usage in TUI tag_editor te_track.rs | Codebase grep | 8 | 3 |
| playlist_sync_podcasts has_localfile display fields | Codebase read | 1 | 1 |
| as_grpc_playlist_tracks add_episode add_track stream emission | Codebase grep | 6 | 4 |
| PlaylistAddTrackInfo struct definition and From impl | Codebase read | 1 | 1 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-014 | DeepWiki: tokio-rs/prost — oneof generates nested enum (`Option<Enum>`), optional generates flat `Option<String>` | AI Documentation | 2026-06 | Fresh | High |
| SRC-015 | termusic codebase: `lib/src/player.rs:397-404` — existing `From<UpdatePlaylistEvents>` unwraps oneof title via `playlist_add_track::OptionalTitle::Title` | Codebase | 2026-06 | Fresh | High |
| SRC-016 | termusic codebase: `playback/src/playlist.rs:669-676,723-730,791-798,817-824` — four sites emitting PlaylistAddTrackInfo, all have track.title() and track.artist() available | Codebase | 2026-06 | Fresh | High |
| SRC-017 | termusic codebase: `tui/src/ui/components/playlist.rs:448-461` — handle_playlist_add calls `add_tracks()` which re-reads from disk; needs refactoring | Codebase | 2026-06 | Fresh | High |
| SRC-018 | termusic codebase: `tui/src/ui/components/tag_editor/te_track.rs:42-68` — TryFrom<&Track> requires file_type but only used by lyric delay adjust (graceful failure) and tag editor (independent disk read) | Codebase | 2026-06 | Fresh | High |
| SRC-019 | DeepWiki: Serial-ATA/lofty-rs — `FileType::from_path()` infers type from file extension without opening file | AI Documentation | 2026-06 | Fresh | High |
| SRC-020 | termusic codebase: `tui/src/ui/components/playlist.rs:570-618` — playlist_sync_podcasts uses only title, duration, and has_localfile for display | Codebase | 2026-06 | Fresh | High |
| SRC-021 | termusic codebase: `lib/src/track.rs:200-224` — from_podcast_episode constructs Track with title, duration, localfile (checking existence), image_url | Codebase | 2026-06 | Fresh | High |
| SRC-022 | termusic codebase: `playback/src/playlist.rs:1030-1053` — as_grpc_playlist_tracks iterates self.tracks() which include already-constructed podcast/radio/music Track objects with all metadata | Codebase | 2026-06 | Fresh | High |
| SRC-023 | termusic codebase: `tui/src/ui/components/tag_editor/view.rs:219` — TETrack::read_metadata_from_file reads independently when tag editor opens | Codebase | 2026-06 | Fresh | High |
| SRC-024 | termusic codebase: `tui/src/ui/components/lyric.rs:384-388` — TETrack::try_from(track) returns early with debug log on failure (no crash) | Codebase | 2026-06 | Fresh | High |

---

## Per-Issue Deep Dive

### ISS-001: oneof vs optional Inconsistency in Proto Access Patterns

**Prior Understanding**: The existing `oneof optional_title { string title = 2; }` uses a pre-proto3.15 workaround for optional fields. New artist/album fields should use `optional` keyword. This creates inconsistency in generated Rust code access patterns.

**Investigation Summary**: Examined prost code generation for both patterns (SRC-014) and the existing conversion code in `player.rs` (SRC-015).

**Resolution Status**: RESOLVED

**Evidence**:
- prost generates `Option<playlist_add_track::OptionalTitle>` for the oneof, requiring pattern matching: `msg.optional_title.map(|v| { let OptionalTitle::Title(t) = v; t })` (SRC-014)
- prost generates `Option<String>` directly for `optional string artist = 5`, accessed simply as `msg.artist` (SRC-014)
- The existing codebase already handles this inconsistency in `player.rs:399-404` and `player.rs:442-446` — the conversion between protobuf and `PlaylistAddTrackInfo` is centralized in two `From`/`TryFrom` impls (SRC-015)
- Wire compatibility is guaranteed because oneof and optional with single variant are wire-equivalent in proto3. The existing title field at number 2 remains unchanged (SRC-014)

**Resolution Path**:
1. Keep `oneof optional_title { string title = 2; }` in the proto file unchanged
2. Add `optional string artist = 5;` and `optional string album = 6;` as new fields
3. The access inconsistency is isolated to two conversion functions in `player.rs` — add artist/album mapping alongside the existing title extraction
4. Optionally add a helper on the generated struct: `fn title_value(&self) -> Option<&str>` to normalize the access pattern for callers

**Code sketch** (proto change):
```protobuf
message PlaylistAddTrack {
  uint64 at_index = 1;
  oneof optional_title { string title = 2; }  // unchanged
  Duration duration = 3;
  TrackId id = 4;
  optional string artist = 5;  // NEW
  optional string album = 6;   // NEW
}
```

**Code sketch** (Rust conversion, `From<UpdatePlaylistEvents>` extension):
```rust
PPlaylistTypes::AddTrack(protobuf::PlaylistAddTrack {
    at_index: vals.at_index,
    optional_title: vals.title.map(protobuf::playlist_add_track::OptionalTitle::Title),
    duration: Some(vals.duration.into()),
    id: Some(vals.trackid.into()),
    artist: vals.artist,  // NEW: direct Option<String>
    album: vals.album,    // NEW: direct Option<String>
})
```

---

### ISS-002: PlaylistAddTrackInfo Struct Missing artist/album Fields

**Prior Understanding**: The `PlaylistAddTrackInfo` struct only has `at_index`, `title`, `duration`, and `trackid`. It lacks artist and album, meaning individual track-add stream events cannot carry full display metadata.

**Investigation Summary**: Examined all emission sites (SRC-016) and the TUI handler (SRC-017).

**Resolution Status**: RESOLVED

**Evidence**:
- `PlaylistAddTrackInfo` is defined at `lib/src/player.rs:336-344` with fields: `at_index`, `title`, `duration`, `trackid` (SRC-016)
- Four emission sites in `playback/src/playlist.rs` all have access to the full Track object at emission time (SRC-016):
  - Line 670: `add_episode()` — has `track.title()`, `track.artist()` available
  - Line 724: `add_track()` — has `track.title()`, `track.artist()` available
  - Line 792: `add_tracks()` at-end branch — has `track.title()`, `track.artist()` available
  - Line 818: `add_tracks()` at-position branch — has `track.title()`, `track.artist()` available
- The album is on `TrackData` (not top-level Track), accessed via `track.as_track().and_then(|v| v.album())` — available at all emission sites (SRC-016)
- TUI handler at `playlist.rs:448-461` currently re-parses from disk because it calls `add_tracks(PlaylistAddTrack { tracks: vec![items.trackid] })` which goes through `source_to_track` -> `track_from_path` (SRC-017)

**Resolution Path**:
1. Add `artist: Option<String>` and `album: Option<String>` fields to `PlaylistAddTrackInfo`
2. Update all four emission sites to populate these fields from the Track object
3. Update `From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist` to map artist/album to proto fields
4. Update `TryFrom<protobuf::UpdatePlaylist> for UpdatePlaylistEvents` to extract artist/album from proto
5. Rewrite TUI `handle_playlist_add` to construct Track directly from event metadata (using new `Track::from_grpc_metadata`) instead of calling `add_tracks` which re-reads from disk

**Code sketch** (struct extension):
```rust
pub struct PlaylistAddTrackInfo {
    pub at_index: u64,
    pub title: Option<String>,
    pub artist: Option<String>,   // NEW
    pub album: Option<String>,    // NEW
    pub duration: PlayerTimeUnit,
    pub trackid: playlist_helpers::PlaylistTrackSource,
}
```

**Code sketch** (TUI handler refactored):
```rust
pub fn handle_playlist_add(&mut self, items: PlaylistAddTrackInfo) -> Result<()> {
    let track = Track::from_grpc_metadata(
        items.trackid,
        items.title,
        items.artist,
        items.album,
        Some(items.duration),
    );
    self.playback.playlist.insert_track_at(items.at_index, track)?;
    self.playlist_sync();
    Ok(())
}
```

---

### ISS-003: TrackData.file_type Will Be None When Constructed from gRPC

**Prior Understanding**: `file_type` is populated by `parse_metadata_from_file` and used in TUI logic. If Track is constructed from gRPC metadata without filesystem access, `file_type` will be `None`.

**Investigation Summary**: Traced all usages of `file_type()` in the TUI crate (SRC-018, SRC-023, SRC-024). Verified lofty's extension-based inference capability (SRC-019).

**Resolution Status**: RESOLVED

**Evidence**:
- `file_type()` is used in exactly two meaningful contexts in the TUI:
  1. **Tag editor `TryFrom<&Track>`** (te_track.rs:50): Only invoked in `lyric.rs:385` for lyric delay adjustment. The code has an explicit `let Ok(mut te_track) = TETrack::try_from(track) else { return; }` guard — failure is non-fatal, just logs a debug message (SRC-024)
  2. **Tag editor `read_metadata_from_file`** (view.rs:219): The tag editor always reads from disk independently when opened, using `TETrack::read_metadata_from_file(path)`. This path does NOT depend on the Track's stored `file_type` — it re-parses the file fresh (SRC-023)
- Other `file_type` references in the TUI are `std::fs::DirEntry::file_type()` (directory traversal), not lofty's `FileType` — different concept entirely
- `file_type` is NOT used by `playlist_sync()` table building. The display columns use only: `duration_str_short()`, `title()`, `artist()`, `as_track().album()`, `id_str()` (SRC-020)
- lofty 0.24 provides `FileType::from_path(&path)` which infers type from file extension without disk I/O (SRC-019). This can be used to populate `file_type` on the new `Track::from_grpc_metadata` constructor if desired

**Resolution Path**:
1. **Primary**: Leave `file_type` as `None` in `Track::from_grpc_metadata`. This is safe because:
   - Playlist display does not use it
   - Tag editor re-reads from disk independently
   - Lyric adjustment gracefully handles None
2. **Optional enhancement**: Populate `file_type` using `FileType::from_path(&path)` in `from_grpc_metadata` for the `MediaTypes::Track` variant — zero disk I/O, pure extension matching. This fixes the lyric delay adjustment edge case.

**Recommendation**: Use `FileType::from_path(&path)` in the constructor. It costs nothing (pure string matching) and preserves the lyric adjustment workflow:

```rust
pub fn from_grpc_metadata(...) -> Self {
    let (inner, file_type_opt) = match &trackid {
        PlaylistTrackSource::Path(p) => {
            let path = PathBuf::from(p);
            let ft = FileType::from_path(&path); // no disk I/O
            (MediaTypes::Track(TrackData { path, album, file_type: ft }), ft)
        }
        PlaylistTrackSource::Url(url) => (MediaTypes::Radio(RadioTrackData::new(url.clone())), None),
        PlaylistTrackSource::PodcastUrl(url) => (MediaTypes::Podcast(PodcastTrackData::new(url.clone())), None),
    };
    Self { inner, duration, title, artist }
}
```

---

### ISS-004: Podcast Episode Metadata for Playlist Display

**Prior Understanding**: Podcast episodes currently use database queries for `localfile` and `image_url`. Need to determine if title + duration + URL is sufficient for playlist display, or if podcast-specific fields need gRPC extension too.

**Investigation Summary**: Examined `playlist_sync_podcasts()` display logic (SRC-020) and the server's in-memory state for podcast tracks (SRC-021, SRC-022).

**Resolution Status**: RESOLVED

**Evidence**:
- `playlist_sync_podcasts()` at tui/src/ui/components/playlist.rs:570-618 uses exactly three pieces of data per track (SRC-020):
  1. `track.duration_str_short()` — duration
  2. `track.title().unwrap_or("Unknown Title")` — title
  3. `track.as_podcast().is_some_and(PodcastTrackData::has_localfile)` — for "[D]" downloaded prefix
- The server's `Playlist` already holds podcast `Track` objects in memory with all this data populated (constructed via `Track::from_podcast_episode` during server-side playlist loading). The `as_grpc_playlist_tracks()` function iterates `self.tracks()` which includes these fully-formed podcast Track objects (SRC-022)
- `image_url` is NOT used in playlist display. It is used in cover art display (a separate, on-demand operation)
- `localfile` path is NOT displayed directly — only its existence is checked (`has_localfile`)

**Resolution Path**:
1. For playlist display: title + duration + `has_localfile` boolean is sufficient
2. The proto already has `TrackId.source` with a `PodcastUrl` variant that carries the URL
3. Add `optional bool has_local_file = 7;` to `PlaylistAddTrack` message (only meaningful for podcast tracks, `None`/false for others)
4. The server populates this from `track.as_podcast().map(|p| p.has_localfile()).unwrap_or(false)`
5. The TUI's `Track::from_grpc_metadata` constructor sets `PodcastTrackData.localfile` to a sentinel value (e.g., `Some(PathBuf::from(""))`) when `has_local_file` is true, or `None` when false. The sentinel works because `has_localfile()` only checks `self.localfile.is_some()`

**Alternative** (simpler): Since `has_localfile` only affects a visual "[D]" prefix, defer this to a later enhancement and show all podcast tracks without the download indicator initially. The indicator will appear once the user navigates to the podcast layout (which could trigger a lightweight query).

**Recommendation**: Use the simpler alternative (omit `has_local_file` from proto). The "[D]" indicator is cosmetic and the TUI can infer download status from the podcast database on-demand when in Podcast layout. This avoids adding a podcast-specific field to a generic track message. Title + duration via gRPC is sufficient.

---

### ISS-005: Both Bulk and Individual Event Emission Must Populate artist/album

**Prior Understanding**: Both `as_grpc_playlist_tracks` (bulk response) and individual `add_track` event emission must be updated.

**Investigation Summary**: Identified all code paths that build gRPC track data (SRC-016, SRC-022).

**Resolution Status**: RESOLVED

**Evidence**:
- **Bulk path**: `as_grpc_playlist_tracks()` at playback/src/playlist.rs:1030-1053 iterates `self.tracks()` and has access to each Track's full metadata: `track.title()`, `track.artist()`, `track.as_track().and_then(|v| v.album())` (SRC-022). Currently sends `optional_title: None` — needs to populate title, artist, and album.
- **Individual paths** (stream events) at playback/src/playlist.rs (SRC-016):
  - `add_episode()` line 670: Track is already constructed, has title/artist available
  - `add_track()` line 724: Track is already constructed via `track_from_path()`, has all metadata
  - `add_tracks()` line 792 (at-end): Track constructed via `source_to_track()`, has all metadata
  - `add_tracks()` line 818 (at-position): Same as above
- All paths have the full Track object available at the point of emitting the event. No additional data fetching is required.

**Resolution Path**:
1. Update `as_grpc_playlist_tracks()`:
```rust
Ok(player::PlaylistAddTrack {
    at_index,
    optional_title: track.title().map(|t| 
        protobuf::playlist_add_track::OptionalTitle::Title(t.to_owned())
    ),
    duration: Some(track.duration().unwrap_or_default().into()),
    id: Some(track_source.into()),
    artist: track.artist().map(ToOwned::to_owned),        // NEW
    album: track.as_track().and_then(|v| v.album()).map(ToOwned::to_owned),  // NEW
})
```

2. Update `PlaylistAddTrackInfo` construction at all four sites to include artist/album:
```rust
PlaylistAddTrackInfo {
    at_index: u64::try_from(self.tracks.len()).unwrap(),
    title: track.title().map(ToOwned::to_owned),
    artist: track.artist().map(ToOwned::to_owned),  // NEW
    album: track.as_track().and_then(|v| v.album()).map(ToOwned::to_owned),  // NEW
    duration: track.duration().unwrap_or_default(),
    trackid: track_location,
}
```

3. The `From`/`TryFrom` impls in `player.rs` handle serialization/deserialization to/from protobuf automatically once the struct and proto are updated.

---

## Options Comparison

REQUIRED: Compare 3-5 viable options for the overall design approach given the resolved issues.

| Criterion | Option A: Flat Optional Fields | Option B: Nested TrackMetadata Message | Option C: Flat Fields + file_type Enum |
|-----------|-------------------------------|---------------------------------------|----------------------------------------|
| Maturity | 5 | 4 | 4 |
| Community/Support | 5 | 4 | 3 |
| Performance | 5 | 4 | 5 |
| Bundle Size / Footprint | 5 | 4 | 4 |
| Learning Curve | 5 | 3 | 3 |
| Maintenance Burden | 4 | 3 | 3 |
| Project Fit | 5 | 4 | 4 |
| Innovation/Momentum | 4 | 4 | 3 |
| **TOTAL** | **38** | **30** | **29** |

### Option A: Flat Optional Fields on PlaylistAddTrack (Recommended)

Add `optional string artist = 5` and `optional string album = 6` directly on the existing `PlaylistAddTrack` message. Keep the existing `oneof optional_title` unchanged. Populate all fields from server.

- **Strengths**: Minimal proto schema change (2 lines added). Wire-compatible with existing messages (SRC-014). No nested message overhead. Matches the existing flat structure of the message. All four emission sites have the data available (SRC-016). Simple `Option<String>` in generated Rust code. No migration needed for existing `oneof` title field (SRC-015).
- **Weaknesses**: Slight inconsistency between title (oneof unwrap) and artist/album (direct Option) access patterns — isolated to two conversion functions (SRC-015). Cannot easily add more metadata fields in future without growing the message (but this is unlikely for playlist display).
- **Best For**: This project — minimal change, maximum compatibility, clear resolution for all five issues.

### Option B: Nested TrackMetadata Sub-Message

Create a new `TrackMetadata` message containing `optional string title`, `optional string artist`, `optional string album`, `optional Duration duration`. Add it as `optional TrackMetadata metadata = 5` on `PlaylistAddTrack`. Deprecate the old `oneof optional_title` and `duration` fields over time.

- **Strengths**: Clean, consistent access pattern (`msg.metadata.as_ref().and_then(|m| m.artist.as_deref())`). Groups related display metadata logically. Enables future extension of metadata without touching the parent message. Eventually allows deprecating the old oneof pattern cleanly (SRC-014).
- **Weaknesses**: Requires creating a new protobuf message type. Adds nesting in generated code (`Option<TrackMetadata>` wrapping `Option<String>` fields — double-unwrap). Breaking semantic change for existing `duration` field (now in two places). Migration period where both old and new fields exist. More complex conversion logic in `player.rs`. Over-engineered for 2 new fields (SRC-015).
- **Best For**: Projects anticipating many future metadata extensions or needing to deprecate the oneof pattern.

### Option C: Flat Fields Including file_type

Same as Option A, but additionally add `optional uint32 file_type = 7` (or a proto enum) to transmit lofty's FileType from server to TUI, eliminating the extension-inference workaround.

- **Strengths**: Fully eliminates any dependency on filesystem for Track construction. Tag editor's `TryFrom<&Track>` would work even without independent disk read. Complete data transfer (SRC-018, SRC-019).
- **Weaknesses**: `file_type` is only used by tag editor which reads from disk anyway (SRC-023). Proto enum must exactly match lofty's enum variants, creating a tight coupling to a specific lofty version. Additional serialization/deserialization code for an unused-in-display field. Over-solves a non-problem since lyric adjustment already handles None gracefully (SRC-024). FileType::from_path already provides zero-cost inference (SRC-019).
- **Best For**: Scenarios where the tag editor must work from gRPC data alone (e.g., remote TUI without filesystem access) — not the current use case.

---

## Deprecation Warnings

No deprecation concerns identified for current stack.

- prost 0.14.4 and tonic 0.14.6 are current stable releases.
- lofty 0.24.0 is the current stable release, providing `FileType::from_path()`.
- The `oneof` pattern for optional fields is not deprecated — it remains wire-equivalent to the `optional` keyword (SRC-014).

---

## Best Practices

### BP-005: Centralize Proto-to-Domain Conversion for Consistent Access

- **Pattern**: When mixing legacy proto patterns (oneof) with modern patterns (optional), centralize the conversion in a single pair of `From`/`TryFrom` implementations. Expose a uniform domain struct with simple `Option<T>` fields regardless of the underlying proto encoding.
- **Rationale**: The `PlaylistAddTrackInfo` domain struct already abstracts away the oneof awkwardness. Adding artist/album as plain `Option<String>` on this domain struct means all downstream code (TUI, tests) never deals with proto specifics. Only the two conversion impls in `player.rs` handle the inconsistency (SRC-015).
- **Source**: SRC-014, SRC-015
- **Confidence**: High

### BP-006: Use FileType::from_path for Zero-Cost Type Inference

- **Pattern**: When constructing a Track from gRPC metadata (no filesystem access), use `lofty::file::FileType::from_path(&path)` to infer the file type from the extension. This is a pure string matching operation with no disk I/O.
- **Rationale**: Preserves functionality for downstream consumers that check `file_type()` (like the lyric adjustment path) without introducing any performance cost. The extension-to-type mapping is deterministic and reliable for supported formats (SRC-019).
- **Source**: SRC-019
- **Confidence**: High
- **Example**:
```rust
use lofty::file::FileType;
let file_type = FileType::from_path(&path); // Returns Option<FileType>, no I/O
```

### BP-007: Defer Podcast Download-Status Indicator to On-Demand Query

- **Pattern**: For metadata that requires a secondary data source (database) and is only used in a specific view, defer its population to when that view becomes active rather than including it in the generic gRPC protocol.
- **Rationale**: The `has_localfile` check for podcasts requires knowing if a downloaded file exists. Adding this to the generic PlaylistAddTrack proto message couples podcast-specific concerns to the general protocol. Since the podcast layout uses a separate `playlist_sync_podcasts()` function, it can query this information on-demand (SRC-020, SRC-021).
- **Source**: SRC-020, SRC-021
- **Confidence**: Medium

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| TUI re-reading track from disk on individual add_track events | Even though the server just read this file and sent the event, the TUI calls `add_tracks()` which goes through `source_to_track()` -> `track_from_path()` -> `Track::read_track_from_path()`, performing redundant disk I/O for every single track addition event | Construct Track directly from event metadata using `Track::from_grpc_metadata()` | SRC-017 |
| Transmitting track identifiers without metadata then expecting the receiver to independently fetch the same metadata | Creates O(N) redundant I/O operations and tightly couples the receiver to having filesystem access. Violates the "tell, don't ask" principle. | Include all display-necessary metadata in the message itself | SRC-016, SRC-022 |
| Adding proto enum fields tightly coupled to external library enums | If lofty changes its FileType enum (adds/removes variants), the proto enum becomes out of sync. Proto enums should represent wire-stable concepts, not library internals. | Use FileType::from_path() on the receiving end for type inference from extension | SRC-019 |

---

## Implementation Considerations

### Performance

- Adding artist + album strings to the proto adds approximately 100-200 bytes per track. For 1000 tracks, this is ~200KB additional payload — negligible for local IPC (SRC-022).
- `FileType::from_path()` is a pure string operation (lowercase extension comparison against a static table). Zero performance impact (SRC-019).
- The TUI `handle_playlist_add` refactoring eliminates one `Track::read_track_from_path` call per individual add event. Each call involves opening a file, parsing audio tags via lofty, and extracting metadata — eliminating this for per-track events prevents noticeable lag when adding multiple tracks rapidly (SRC-017).

### Security

- No new attack surface. All metadata strings originate from the server's lofty parse of local audio files. No user-controlled external input is introduced through this change (SRC-022).

### Compatibility

- Adding optional fields 5 and 6 to PlaylistAddTrack is fully wire-compatible. A message serialized with these fields can be deserialized by code unaware of them (fields are silently ignored). Conversely, a message without these fields deserializes to `None` for the new fields (SRC-014).
- The PlaylistAddTrackInfo struct change is internal to the Rust codebase and does not affect wire format — only the From/TryFrom impls bridge between proto and domain types (SRC-015).
- Both TUI and server are built from the same repo commit, so version mismatch is not a practical concern (SRC-022).

---

## Contradictions Found

No contradictions found across sources.

All sources consistently agree that:
1. The oneof pattern must be preserved for field number 2 to maintain wire compatibility (SRC-014, SRC-015)
2. All emission sites have full Track metadata available at event-send time (SRC-016, SRC-022)
3. file_type is not needed for playlist display (SRC-018, SRC-020)
4. Podcast display needs only title + duration for basic rendering (SRC-020, SRC-021)

---

## Issues and Ambiguities

All five issues from the prior report are resolved. No new blocking issues identified.

- **ISS-006** (Low priority): The `handle_playlist_add` refactoring in the TUI introduces a new concern — the TUI playlist will need a method like `insert_track_at(index, track)` that inserts a pre-constructed Track without triggering a stream event (since the event is what triggered the insertion). The current `add_tracks()` method both constructs tracks AND emits events, creating a potential infinite loop if naively called from the event handler. The current code avoids this because the TUI's playlist instance has `stream_tx: None` (it is not the authoritative playlist). Verify this assumption during implementation.

- **ISS-007** (Low priority): The `has_localfile` podcast indicator decision (defer vs include) should be validated with the maintainer. If podcast users frequently rely on the "[D]" indicator to know which episodes are downloaded, deferring it may cause a brief visual inconsistency when switching to Podcast layout. A simple workaround: always show "[D]" based on the initial gRPC data, then refresh when entering podcast view.

---

## References

### Primary Sources (Official Documentation)

- SRC-014: DeepWiki: tokio-rs/prost — oneof vs optional code generation patterns
- SRC-019: DeepWiki: Serial-ATA/lofty-rs — FileType::from_path and FileType::from_ext API

### Secondary Sources (AI Documentation, Guides)

- SRC-015: termusic codebase: `lib/src/player.rs:393-453` — From/TryFrom impls for UpdatePlaylistEvents
- SRC-018: termusic codebase: `tui/src/ui/components/tag_editor/te_track.rs:42-68` — TryFrom<&Track> file_type usage

### Codebase Sources

- SRC-016: termusic `playback/src/playlist.rs:669-676,723-730,791-798,817-824` — PlaylistAddTrackInfo emission sites
- SRC-017: termusic `tui/src/ui/components/playlist.rs:448-461` — handle_playlist_add disk-read path
- SRC-020: termusic `tui/src/ui/components/playlist.rs:570-618` — playlist_sync_podcasts display fields
- SRC-021: termusic `lib/src/track.rs:200-224` — Track::from_podcast_episode constructor
- SRC-022: termusic `playback/src/playlist.rs:1030-1053` — as_grpc_playlist_tracks server-side builder
- SRC-023: termusic `tui/src/ui/components/tag_editor/view.rs:219` — TETrack independent disk read
- SRC-024: termusic `tui/src/ui/components/lyric.rs:384-388` — graceful failure on missing file_type
