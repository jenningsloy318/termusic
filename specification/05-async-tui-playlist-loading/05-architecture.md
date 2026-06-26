# Architecture Design: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Author**: super-dev:architecture-improver
- **Scope**: gRPC protocol (lib/proto), Track construction (lib/src/track.rs), server playlist serialization (playback/src/playlist.rs), TUI playlist loading (tui/src/ui/model), TUI event handling (tui/src/ui/components/playlist.rs)

---

## Friction Analysis

The core architectural friction is a **shallow protocol boundary** between the server and TUI. The gRPC `PlaylistAddTrack` message acts as a pass-through identifier relay rather than a deep module that encapsulates the data needed for rendering. The server discards metadata it already holds in memory, forcing the TUI to re-derive the same information from disk. This violates locality (knowledge about tracks is scattered across server memory, disk files, and podcast database) and provides zero leverage at the protocol seam (callers must do as much work after crossing the seam as the server did before it).

### Shallow Modules Identified

| Module | Files | Symptom | Deletion Test |
|--------|-------|---------|---------------|
| `PlaylistAddTrack` proto message | `lib/proto/player.proto:228-247` | Pass-through: carries only identifiers (path/url) and duration, forcing TUI to re-derive title/artist/album from disk. Interface is nearly as complex as the value it delivers (4 fields, but omits the 3 fields callers actually need). | Fail: if deleted, TUI would need direct server memory access or its own loading mechanism (which it already has). The message earns zero leverage because it forces re-work on the receiver. |
| `TUIPlaylist::track_from_path` | `tui/src/ui/model/playlist.rs:157-178` | Shallow wrapper: 3 lines of logic around `Track::read_track_from_path` with a TODO comment acknowledging it should not exist. No value added beyond error context. | Pass: if deleted and replaced by gRPC-supplied metadata, callers simplify (no disk I/O, no error handling for file reads). |
| `TUIPlaylist::track_from_podcasturi` | `tui/src/ui/model/playlist.rs:186-191` | Shallow wrapper: 2 lines calling `db_pod.get_episode_by_url` then `Track::from_podcast_episode`. The TUI should not need database access for display data. | Pass: if deleted and podcast metadata arrives via gRPC, the TUI removes its `DBPod` dependency for the playlist loading path. |
| `Playback::load_from_grpc` (TUI) | `tui/src/ui/model/mod.rs:187-232` | Leaky abstraction: named "load from gRPC" but actually loads from disk via `Track::read_track_from_path`. The gRPC response is just an index into the filesystem. | Pass: if it received complete Track data from gRPC, the method becomes a trivial Vec construction (high leverage, zero I/O). |

### Depth Assessment

| Module | Interface Width | Implementation Depth | Leverage | Verdict |
|--------|----------------|---------------------|----------|---------|
| `PlaylistAddTrack` (proto) | 4 fields (at_index, title, duration, id) | ~0 logic (wire format only) | Low — callers must still read files | Shallow |
| `as_grpc_playlist_tracks` (server) | 1 entry point | ~30 lines (iteration + serialization) | Low — discards metadata, requires TUI re-work | Shallow |
| `TUIPlaylist::add_tracks` | 1 entry point | ~30 lines (iteration + disk I/O + db queries) | Medium — but wrong kind of depth (I/O that should not exist) | Borderline (deep implementation, but wrong work) |
| `Track::read_track_from_path` | 1 entry point | ~40 lines (file open, lofty parse, metadata extraction) | High — genuinely earns its keep on the server side where metadata must be read from disk | Deep (but used in wrong context on TUI side) |
| `playlist_sync()` | 1 entry point | ~70 lines (table building) | High — single call produces full rendered table | Deep |

---

## Deepening Candidates

### CAND-001: Deepen the gRPC Protocol Seam to Carry Full Display Metadata

- **Files**: `lib/proto/player.proto:228-247`, `lib/src/player.rs:336-344,392-470`, `playback/src/playlist.rs:1030-1053,669-830`, `tui/src/ui/model/mod.rs:187-232`, `tui/src/ui/model/playlist.rs:123-191`, `tui/src/ui/components/playlist.rs:448-461`
- **Problem**: The gRPC protocol seam is shallow — it passes identifiers without the display data callers need. The `PlaylistAddTrack` message has 4 fields but the TUI requires 7 data points (path, title, artist, album, duration, track_type, has_localfile). This forces the TUI to cross a second seam (filesystem/database) to complete its work, destroying locality. The server already computed all metadata; the protocol discards it.
- **Dependency Category**: In-process (proto serialization is pure computation; the gRPC channel is local IPC between co-located processes)
- **Solution**: Extend `PlaylistAddTrack` with `artist`, `album`, and `has_local_file` optional fields. Have the server populate all metadata fields from its in-memory Track objects. Create a `Track::from_grpc_metadata()` constructor that builds Track without disk I/O. Rewrite TUI's `load_from_grpc` and `handle_playlist_add` to use the new constructor exclusively.
- **Benefits**: Locality — all track display knowledge concentrates in the server's Track object and flows through a single seam (gRPC). Leverage — callers receive a fully-formed Track from one gRPC message with no additional work. Tests improve — TUI playlist loading becomes testable without filesystem fixtures (pure data transformation from proto to domain).
- **Effort**: M
- **Impact**: L
- **Risk**: Low

### CAND-002: Create a Deep `Track::from_grpc_metadata` Constructor Module

- **Files**: `lib/src/track.rs:184-284`, `tui/src/ui/model/mod.rs:209-216`, `tui/src/ui/model/playlist.rs:157-191`
- **Problem**: Track construction is split across multiple shallow factory methods (`track_from_path`, `track_from_podcasturi`, `track_from_uri`) that each cross different seams (filesystem, database, string parsing). The TUI side has 3 parallel paths with different error modes, yet the output is always the same `Track` struct. This scatter violates locality — a change to Track display requirements touches 3+ methods across 2 crates.
- **Dependency Category**: In-process (Track construction from metadata is pure computation — no I/O when metadata is pre-provided)
- **Solution**: Create `Track::from_grpc_metadata(source: PlaylistTrackSource, title: Option<String>, artist: Option<String>, album: Option<String>, duration: Option<Duration>, has_local_file: bool)` that unifies all 3 TUI-side construction paths into one pure function with no I/O dependencies.
- **Benefits**: Locality — all Track-from-gRPC logic in one place. Leverage — one constructor replaces 3 methods + error handling. Tests — the constructor is a pure function testable without filesystem fixtures or database connections.
- **Effort**: S
- **Impact**: L
- **Risk**: Low

### CAND-003: Deepen Server-Side Track Serialization (Eliminate Title: None)

- **Files**: `playback/src/playlist.rs:1030-1053`, `playback/src/playlist.rs:669-830`
- **Problem**: The server's `as_grpc_playlist_tracks()` sets `optional_title: None` despite holding title in memory. Individual track addition events DO send title (line 672, 726, 793), but bulk playlist responses do not. This inconsistency means the protocol seam provides different leverage depending on which message type is received — callers cannot trust the interface to behave uniformly.
- **Dependency Category**: In-process (pure in-memory string serialization)
- **Solution**: Populate all metadata fields (title, artist, album, duration) consistently in both `as_grpc_playlist_tracks()` (bulk) and `send_stream_ev_pl` (individual events). Ensure uniform behavior across the seam.
- **Benefits**: Locality — serialization logic is consistent in one place. Leverage — TUI receives complete data from every protocol path, no special-casing needed. Tests — bulk and individual paths can be verified identically.
- **Effort**: S
- **Impact**: L
- **Risk**: Low

### CAND-004: Deepen TUI `load_from_grpc` Into a Pure Data Transformation

- **Files**: `tui/src/ui/model/mod.rs:187-232`, `tui/src/ui/model/playlist.rs:123-191`
- **Problem**: `load_from_grpc` crosses 3 seams (gRPC data, filesystem, podcast DB) when it should cross one. Its interface requires a `DBPod` parameter for what should be a pure data transformation. The method name lies — it does not "load from gRPC" but "load from gRPC identifiers + disk + database". This forces tests to provide filesystem fixtures and database state.
- **Dependency Category**: Local-substitutable (the podcast DB has an in-memory test stand-in, but the filesystem dependency has no substitute — it must be eliminated, not substituted)
- **Solution**: Once the protocol carries full metadata (CAND-001), rewrite `load_from_grpc` to accept metadata directly and construct Tracks via `Track::from_grpc_metadata()`. Remove the `db_pod` parameter. The method becomes a pure O(n) in-memory transformation.
- **Benefits**: Locality — all loading logic in one method with no external calls. Leverage — callers pass one message, get a fully-populated playlist. Tests — no filesystem fixtures, no database setup, just construct proto data and verify Track output.
- **Effort**: M
- **Impact**: L
- **Risk**: Low

### CAND-005: Add `TUIPlaylist::insert_track_at` for Pre-Built Track Insertion

- **Files**: `tui/src/ui/model/playlist.rs:123-154`
- **Problem**: `TUIPlaylist::add_tracks` conflates two concerns — constructing a Track from a source identifier (with I/O) and inserting it into the Vec. There is no way to insert a pre-constructed Track without going through the I/O path. This makes the module shallow: its interface requires `DBPod` and performs disk reads even though the caller may already have a fully-constructed Track.
- **Dependency Category**: In-process (Vec::insert is pure computation)
- **Solution**: Add `insert_track_at(index: usize, track: Track)` method that performs a direct Vec insertion with bounds-checking. This is the I/O-free counterpart to `add_tracks` for use when Track data arrives from gRPC.
- **Benefits**: Locality — insertion is a single operation with clear semantics. Leverage — callers with pre-built Tracks avoid unnecessary I/O path. Tests — testable without filesystem or database dependencies.
- **Effort**: S
- **Impact**: M
- **Risk**: Low

---

## Selected Candidate

**Selected**: CAND-001 (Deepen the gRPC Protocol Seam to Carry Full Display Metadata)

**Rationale**: CAND-001 is the root-cause fix that enables all other candidates. CAND-002 through CAND-005 are natural consequences of deepening the protocol seam — once metadata flows through gRPC, the Track constructor (CAND-002), server serialization (CAND-003), load_from_grpc rewrite (CAND-004), and insert_track_at (CAND-005) all follow mechanically. Addressing CAND-001 first provides maximum leverage: one protocol change eliminates all TUI-side I/O, enables pure-function Track construction, and removes the DBPod dependency from playlist loading. The effort is Medium but the blast radius covers all 5 candidates simultaneously.

---

## Interface Alternatives (Design It Twice)

### Option A: Minimal Protocol Extension (Flat Fields on Existing Message)

- **Interface Shape**: Extend existing `PlaylistAddTrack` message with 3 new optional fields. Single `Track::from_grpc_metadata()` constructor. `Playback::load_from_grpc(info: PlaylistTracks)` drops the `db_pod` parameter.
  ```protobuf
  message PlaylistAddTrack {
    uint64 at_index = 1;
    oneof optional_title { string title = 2; }
    Duration duration = 3;
    TrackId id = 4;
    // NEW fields
    optional string artist = 5;
    optional string album = 6;
    optional bool has_local_file = 7;
  }
  ```
  ```rust
  impl Track {
      pub fn from_grpc_metadata(
          source: PlaylistTrackSource,
          title: Option<String>,
          artist: Option<String>,
          album: Option<String>,
          duration: Option<Duration>,
          has_local_file: bool,
      ) -> Self { ... }
  }
  ```
- **Usage Example**: Server calls `as_grpc_playlist_tracks()` which populates all fields from in-memory Track data. TUI calls `Track::from_grpc_metadata(source, title, artist, album, duration, has_local_file)` for each proto track.
- **What Implementation Hides**: All metadata parsing logic (lofty, tag reading, file type detection) remains on server only. TUI never touches filesystem for playlist display. Sentinel PathBuf logic for podcast indicator is hidden inside the constructor.
- **Dependency Strategy**: Filesystem I/O eliminated from TUI playlist path. DBPod dependency removed from `load_from_grpc`. Proto serialization remains in-process.
- **Trade-offs**: The `PlaylistAddTrack` message grows in responsibility (carries display data). The `oneof optional_title` pattern is inconsistent with the new `optional string artist` pattern (legacy vs modern proto3 optional). The existing `PlaylistAddTrackInfo` domain struct must also grow to carry artist/album.

### Option B: Separate Track Metadata Message (Maximum Flexibility)

- **Interface Shape**: Create a new `TrackDisplayMetadata` proto message. Reference it from `PlaylistAddTrack` and from `UpdateTrackChanged`. Multiple constructors for different Track creation scenarios.
  ```protobuf
  message TrackDisplayMetadata {
    optional string title = 1;
    optional string artist = 2;
    optional string album = 3;
    optional Duration duration = 4;
    optional bool has_local_file = 5;
  }
  
  message PlaylistAddTrack {
    uint64 at_index = 1;
    TrackId id = 2;  // renumbered for clean layout
    TrackDisplayMetadata metadata = 3;
  }
  
  message UpdateTrackChanged {
    uint64 current_track_index = 1;
    TrackId id = 2;
    TrackDisplayMetadata metadata = 3;
    PlayerTime progress = 4;
  }
  ```
  ```rust
  impl Track {
      pub fn from_display_metadata(source: PlaylistTrackSource, meta: TrackDisplayMetadata) -> Self { ... }
      pub fn update_metadata(&mut self, meta: TrackDisplayMetadata) { ... }
  }
  ```
- **Usage Example**: Any message that needs to convey track information embeds `TrackDisplayMetadata`. Future messages (search results, library items) reuse the same sub-message. The TUI has a single code path for all metadata-carrying messages.
- **What Implementation Hides**: All metadata derivation on server side. All metadata caching decisions. The shared sub-message means a single deserialization path handles all use cases.
- **Dependency Strategy**: Same as Option A for runtime dependencies. Additionally enables future extension (e.g., adding `genre`, `year`, `bitrate`) without touching parent messages.
- **Trade-offs**: Breaking wire compatibility — renumbering `PlaylistAddTrack` fields means old clients cannot decode new messages. Requires version-bumping all messages that carry track info. More invasive proto change. The `UpdateTrackChanged` message currently uses field numbers 1,3,4 (2 is unused) — renumbering would break existing deployments. Over-designed for a same-version-binary project.

### Option C: Rich Track Domain Object Over gRPC (Optimize for Common Caller)

- **Interface Shape**: Single `Track::from_proto(proto_track: ProtoPlaylistTrack)` that handles all variants. The proto carries everything the TUI needs including variant-specific data.
  ```protobuf
  message PlaylistAddTrack {
    uint64 at_index = 1;
    oneof optional_title { string title = 2; }
    Duration duration = 3;
    TrackId id = 4;
    optional string artist = 5;
    optional string album = 6;
    optional bool has_local_file = 7;
  }
  ```
  ```rust
  impl Track {
      /// One-stop constructor from any PlaylistAddTrack proto message.
      /// Handles path/url/podcast variants internally.
      pub fn from_proto(proto: &protobuf::PlaylistAddTrack) -> Result<Self> { ... }
  }
  
  impl Playback {
      /// Zero-I/O playlist loading. No db_pod parameter needed.
      pub fn load_from_grpc(&mut self, info: PlaylistTracks) -> Result<()> { ... }
  }
  ```
- **Usage Example**: TUI receives proto message, calls `Track::from_proto(&proto_track)` — one line per track, no variant matching needed by the caller. The `load_from_grpc` method becomes 10 lines of iteration.
- **What Implementation Hides**: PlaylistTrackSource variant matching (path vs url vs podcast). Sentinel PathBuf logic. Fallback title derivation from filename. Duration parsing. All hidden inside `Track::from_proto`.
- **Dependency Strategy**: Same wire format as Option A (additive fields only). The implementation decision (flat fields vs sub-message) is hidden from callers. The `from_proto` method owns all conversion logic.
- **Trade-offs**: Couples `Track` construction to proto types (lib crate must import proto types). If proto shape changes, Track constructor must change. But this is a natural coupling — the proto IS the track's wire representation. One-stop convenience makes the common case trivial (just call `from_proto`).

### Option D: Event-Sourced Playlist Updates (Maximize Separation)

- **Interface Shape**: TUI maintains no Track objects at all. Instead, `playlist_sync()` reads directly from a flat metadata table derived from stream events. No Track construction needed.
  ```rust
  struct PlaylistEntry {
      title: String,
      artist: String,
      album: String,
      duration_str: String,
      is_current: bool,
      has_local_file: bool,
  }
  
  struct TUIPlaylist {
      entries: Vec<PlaylistEntry>,
  }
  
  impl TUIPlaylist {
      pub fn apply_event(&mut self, event: PlaylistEvent) { ... }
  }
  ```
- **Usage Example**: Every gRPC event (add, remove, shuffle, etc.) is applied directly to the entries Vec. No Track objects, no MediaTypes enum. The table builder reads `PlaylistEntry` fields directly.
- **What Implementation Hides**: All Track domain complexity (MediaTypes, TrackData, PodcastTrackData) is irrelevant to the TUI display path. The TUI becomes a thin projection of server state.
- **Dependency Strategy**: Complete decoupling from `lib/src/track.rs` for the display path. Track objects only created for operations that need them (playback commands).
- **Trade-offs**: Radical redesign — breaks all existing TUI code that uses `Track` methods (title(), artist(), path(), as_podcast()). Requires duplicating any logic that currently accesses Track fields (e.g., m3u export, tag editor context). Two representations of the same data. High migration cost for marginal benefit beyond Option C.

### Comparison

| Criterion | Option A | Option B | Option C | Option D |
|-----------|----------|----------|----------|----------|
| Depth (leverage per entry point) | 4 | 5 | 5 | 3 |
| Locality (change concentration) | 4 | 3 | 5 | 2 |
| Seam Placement | 5 | 4 | 5 | 3 |
| Testability | 4 | 4 | 5 | 4 |
| Migration Cost | 5 | 2 | 4 | 1 |
| **TOTAL** | **22** | **18** | **24** | **13** |

---

## Recommended Interface

**Selected Option**: Option C (Rich Track Domain Object Over gRPC)

**Rationale**: Option C scores highest because it maximizes both depth and locality. The `Track::from_proto` constructor hides all variant-matching logic behind a single entry point (high leverage). The caller's common case is trivial: one function call per track, no branching. Locality is maximized because all proto-to-domain conversion logic concentrates in one method. Migration cost is moderate because the proto wire format is identical to Option A (additive optional fields), while the Rust-side refactoring replaces scattered match arms with a single constructor. Option A is nearly as good but leaves variant-matching scattered across callers. Option B over-designs the proto for a same-version project. Option D requires a ground-up rewrite with high risk.

### Interface Definition

```protobuf
// lib/proto/player.proto — Extended PlaylistAddTrack
message PlaylistAddTrack {
  uint64 at_index = 1;
  oneof optional_title {
    string title = 2;
  }
  Duration duration = 3;
  TrackId id = 4;
  // NEW: display metadata fields
  optional string artist = 5;
  optional string album = 6;
  optional bool has_local_file = 7;
}
```

```rust
// lib/src/track.rs — New constructor
impl Track {
    /// Construct a Track from gRPC-provided metadata without any disk I/O.
    ///
    /// This is the primary constructor for TUI-side Track creation when
    /// receiving playlist data from the server.
    ///
    /// # Arguments
    /// * `source` - The track identifier (path, URL, or podcast URL)
    /// * `title` - Display title (falls back to filename if None)
    /// * `artist` - Display artist (None if unknown)
    /// * `album` - Display album (None if unknown)
    /// * `duration` - Track duration (None if unknown)
    /// * `has_local_file` - Whether a podcast episode has been downloaded locally
    #[must_use]
    pub fn from_grpc_metadata(
        source: PlaylistTrackSource,
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        duration: Option<Duration>,
        has_local_file: bool,
    ) -> Self {
        let inner = match source {
            PlaylistTrackSource::Path(path) => MediaTypes::Track(TrackData {
                path: PathBuf::from(path),
                album: album.clone(),
                file_type: None, // not needed for display
            }),
            PlaylistTrackSource::Url(url) => MediaTypes::Radio(RadioTrackData::new(url)),
            PlaylistTrackSource::PodcastUrl(url) => {
                let localfile = if has_local_file {
                    Some(PathBuf::new()) // sentinel: "exists but path not transmitted"
                } else {
                    None
                };
                MediaTypes::Podcast(PodcastTrackData {
                    url,
                    localfile,
                    image_url: None,
                })
            }
        };

        Self {
            inner,
            duration,
            title,
            artist,
        }
    }
}
```

```rust
// lib/src/player.rs — Extended domain struct
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistAddTrackInfo {
    pub at_index: u64,
    pub title: Option<String>,
    pub artist: Option<String>,   // NEW
    pub album: Option<String>,    // NEW
    pub duration: PlayerTimeUnit,
    pub trackid: playlist_helpers::PlaylistTrackSource,
    pub has_local_file: bool,     // NEW
}
```

```rust
// tui/src/ui/model/mod.rs — Rewritten load_from_grpc
impl Playback {
    /// Load a full playlist from gRPC response. Zero disk I/O.
    ///
    /// # Errors
    /// - when converting from u64 grpc values to usize fails
    /// - when there is no track-id
    pub fn load_from_grpc(&mut self, info: PlaylistTracks) -> anyhow::Result<()> {
        let current_track_index = usize::try_from(info.current_track_index)
            .context("convert current_track_index(u64) to usize")?;
        let mut playlist_items = Vec::with_capacity(info.tracks.len());

        for (idx, proto_track) in info.tracks.into_iter().enumerate() {
            let at_index_usize =
                usize::try_from(proto_track.at_index).context("convert at_index(u64) to usize")?;
            if idx != at_index_usize {
                error!("Non-matching \"index\" and \"at_index\"!");
            }

            let Some(id) = proto_track.id else {
                bail!("Track does not have an id, which is required to load!");
            };

            let source = PlaylistTrackSource::try_from(id)?;
            let title = proto_track.optional_title.map(|v| {
                let protobuf::playlist_add_track::OptionalTitle::Title(v) = v;
                v
            });
            let duration = proto_track.duration.map(|d| Duration::from(d));
            let artist = proto_track.artist;
            let album = proto_track.album;
            let has_local_file = proto_track.has_local_file.unwrap_or(false);

            let track = Track::from_grpc_metadata(
                source, title, artist, album, duration, has_local_file,
            );
            playlist_items.push(track);
        }

        self.playlist.set_tracks(playlist_items);

        if !self.playlist.is_empty() {
            self.playlist.set_current_track_index(current_track_index)?;
        }

        self.set_current_track_from_playlist();
        Ok(())
    }
}
```

```rust
// tui/src/ui/model/playlist.rs — New insertion method
impl TUIPlaylist {
    /// Insert a pre-constructed Track at a specific index.
    /// If the index >= len, the track is appended at the end.
    pub fn insert_track_at(&mut self, index: usize, track: Track) {
        if index >= self.tracks.len() {
            self.tracks.push(track);
        } else {
            self.tracks.insert(index, track);
        }
    }
}
```

---

## Migration Path

Incremental steps from current state to target architecture. Each step compiles and passes all existing tests independently.

1. **Step 1: Extend protobuf schema with new optional fields.** Add `optional string artist = 5`, `optional string album = 6`, `optional bool has_local_file = 7` to `PlaylistAddTrack` in `lib/proto/player.proto`. Run `cargo build` to regenerate proto bindings. No behavioral change — new fields are `None` everywhere. All existing tests pass unchanged.

2. **Step 2: Extend `PlaylistAddTrackInfo` domain struct with new fields.** Add `artist: Option<String>`, `album: Option<String>`, `has_local_file: bool` to `PlaylistAddTrackInfo` in `lib/src/player.rs`. Update `From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist` to serialize the new fields. Update `TryFrom<protobuf::UpdatePlaylist> for UpdatePlaylistEvents` to deserialize the new fields. All callers that construct `PlaylistAddTrackInfo` (in `playback/src/playlist.rs`) are updated to populate artist/album from `track.artist()` / `track.as_track().and_then(|t| t.album())`. All existing tests pass.

3. **Step 3: Populate metadata in `as_grpc_playlist_tracks()`.** Update `playback/src/playlist.rs:1039-1044` to set `optional_title` from `track.title()`, set `artist` from `track.artist()`, set `album` from `track.as_track().and_then(|t| t.album())`, set `has_local_file` from `track.as_podcast().is_some_and(|p| p.has_localfile())`. The server now sends full metadata over gRPC. TUI still ignores the new fields (reads from disk as before). All existing tests pass.

4. **Step 4: Create `Track::from_grpc_metadata()` constructor.** Add the new constructor to `lib/src/track.rs` as defined in the interface section above. Add unit tests for the constructor (all 3 variants: path, url, podcast). No callers use it yet. All existing tests pass.

5. **Step 5: Rewrite `Playback::load_from_grpc` to use metadata from proto.** Remove the `podcast_db: &DBPod` parameter. Replace `Track::read_track_from_path` / `Track::from_podcast_episode` with `Track::from_grpc_metadata`. Update all callers of `load_from_grpc` (in `tui/src/ui/model/update.rs` and `tui/src/ui/components/playlist.rs:handle_playlist_shuffled`). All existing tests pass — the TUI no longer performs disk I/O during playlist load.

6. **Step 6: Add `insert_track_at` and rewrite `handle_playlist_add`.** Add `TUIPlaylist::insert_track_at(index, track)` method. Rewrite `handle_playlist_add` in `tui/src/ui/components/playlist.rs` to construct a Track via `Track::from_grpc_metadata` using the `PlaylistAddTrackInfo` fields, then call `insert_track_at`. Remove the call to `add_tracks` (which does disk I/O). All existing tests pass.

7. **Step 7: Clean up dead code and TODO comments.** Mark `TUIPlaylist::track_from_path` and `TUIPlaylist::track_from_podcasturi` as `#[deprecated]` or remove if no remaining callers. Remove the `db_pod` parameter from `TUIPlaylist::add_tracks` if no callers remain. Remove TODO comments at `playlist.rs:173` and `playlist.rs:187`. Add integration tests verifying zero disk I/O and timing constraints (AC-01: <100ms, AC-02: <200ms, AC-09: <50ms).

### Dependency Handling

| Dependency | Category | Strategy |
|------------|----------|----------|
| Filesystem (lofty metadata parsing) | In-process | Eliminate from TUI path entirely (server retains it) |
| Podcast database (DBPod) | Local-substitutable | Remove from `load_from_grpc` parameter; server sends metadata via proto |
| gRPC channel (tonic) | Remote-owned (local IPC) | Accept as-is; the seam is deepened by carrying more data |
| Proto code generation (prost/tonic-build) | In-process | Accept as-is; additive field changes require no build system changes |

---

## Test Replacement Strategy

Replace shallow tests, don't layer new tests on top:

| Current Test | Problem | Replacement | Tests At |
|--------------|---------|-------------|----------|
| Any test of `load_from_grpc` requiring filesystem fixtures (audio files with metadata) | Tests cross two seams (gRPC deserialization + filesystem I/O) — if file parsing breaks, playlist tests break | Test `load_from_grpc` with constructed `PlaylistTracks` proto objects containing metadata fields. Verify resulting `TUIPlaylist` contents match expected Track fields. Zero filesystem dependency. | `Playback::load_from_grpc` interface (pure data transformation) |
| Any test of `handle_playlist_add` requiring DB setup | Tests cross gRPC + database seams — testing too much for a single-track insertion | Test `handle_playlist_add` with `PlaylistAddTrackInfo` containing all metadata fields. Verify Track inserted at correct index with correct fields. No DB needed. | `handle_playlist_add` + `TUIPlaylist::insert_track_at` interface |
| Existing `as_grpc_playlist_tracks` tests (if any) that only check id/duration | Tests verify incomplete serialization (title=None) — will fail after fix | Update to verify all 7 fields are populated. Test with Track objects containing various metadata combinations (full, partial, missing). | `Playlist::as_grpc_playlist_tracks` interface (server-side serialization) |
| Integration tests requiring large audio file collections for timing verification | Slow, flaky, environment-dependent | Replace with benchmark tests using `Track::from_grpc_metadata` constructor (no files needed). Verify O(n) linear scaling and sub-100ms processing for 1000 constructed proto messages. | `load_from_grpc` timing tests (pure computation, deterministic) |

---

## Numeric Constants

The following numeric values are specified in this architecture and should be validated empirically during prototype-runner (Stage 6.5):

| Constant | Value | Context | Validation Method |
|----------|-------|---------|-------------------|
| Event loop block ceiling | 100ms | AC-01: TUI main event loop must not block longer than this during playlist loading | Measure wall-clock time of `load_from_grpc` with 1000-track proto payload |
| Playlist render latency | 200ms | AC-02: Time from receiving server response to rendered playlist view | End-to-end timing from proto receipt to `playlist_sync()` completion |
| Table build ceiling | 50ms | AC-09: `playlist_sync()` must complete within this for 1000 tracks | Benchmark `playlist_sync()` with 1000 in-memory Track objects |
| Wire overhead per track | ~200 bytes | Additional gRPC message size from artist/album strings | Measure serialized `PlaylistAddTrack` size with typical metadata |
| Sentinel PathBuf allocation | 24 bytes | Per-podcast-track heap cost for empty PathBuf sentinel | Verify with `std::mem::size_of_val` |
| Proto field numbers | 5, 6, 7 | New field numbers for artist, album, has_local_file | Verify no conflicts with existing field numbering |

---

## Modules Declared

The following modules are created or significantly modified by this architecture:

1. **`PlaylistAddTrack` proto message** — Extended with artist (field 5), album (field 6), has_local_file (field 7)
2. **`Track::from_grpc_metadata`** — New constructor in `lib/src/track.rs` for I/O-free Track creation
3. **`PlaylistAddTrackInfo`** — Extended domain struct in `lib/src/player.rs` with artist, album, has_local_file
4. **`Playlist::as_grpc_playlist_tracks`** — Modified server serialization in `playback/src/playlist.rs`
5. **`Playback::load_from_grpc`** — Rewritten TUI method in `tui/src/ui/model/mod.rs` (no db_pod param)
6. **`TUIPlaylist::insert_track_at`** — New method in `tui/src/ui/model/playlist.rs`
7. **`handle_playlist_add`** — Rewritten event handler in `tui/src/ui/components/playlist.rs`
