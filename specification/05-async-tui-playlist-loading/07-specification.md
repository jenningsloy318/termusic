# Technical Specification: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Author**: super-dev:spec-writer
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md
- **Architecture**: ./05-architecture.md

---

## 1. Overview

This specification defines how to eliminate the multi-second blocking freeze in the termusic TUI during playlist loading. The root cause is that the TUI's `load_from_grpc` method calls `Track::read_track_from_path` for every track in the playlist, performing synchronous disk I/O (lofty metadata parsing) on the main event loop thread. For 200+ tracks this blocks the TUI for 2-5 seconds.

The solution extends the gRPC protocol to transmit full display metadata (title, artist, album, duration, has_local_file) from the server — which already holds this data in memory after spec-04's async background loading — directly to the TUI. A new `Track::from_grpc_metadata` constructor builds Track objects from the protocol data without any filesystem access. This eliminates 100% of TUI-side disk I/O during playlist loading, reducing the operation from seconds to sub-millisecond pure in-memory data transformation.

The approach applies to all playlist-related protocol paths: the initial `GetPlaylist` RPC response, `PlaylistShuffled` stream events (after server background loading completes), and individual `PlaylistAddTrack` stream events. After this change, the TUI never reads audio file metadata from disk for playlist population.

### Acceptance Criteria Coverage

This specification addresses all 10 acceptance criteria:

- **AC-01**: TUI event loop not blocked >100ms — achieved by eliminating disk I/O from load_from_grpc (Section 5.3)
- **AC-02**: Playlist renders within 200ms of response — achieved by pure in-memory Track construction (Section 5.3)
- **AC-03**: Server includes sufficient display metadata — achieved by extending proto and populating fields (Sections 3.1, 5.1)
- **AC-04**: TUI constructs Track without disk I/O — achieved by Track::from_grpc_metadata constructor (Section 3.3)
- **AC-05**: Shuffle events processed without disk re-reads — achieved by same load_from_grpc rewrite (Section 4.3)
- **AC-06**: Proto extended with artist/album while maintaining backward wire compatibility — achieved by additive optional fields 5,6,7 (Section 3.1)
- **AC-07**: Server populates optional_title (previously always None) — achieved by server serialization fix (Section 5.1)
- **AC-08**: Graceful fallback for missing metadata — achieved by filename derivation and None handling (Sections 5.6, 5.7)
- **AC-09**: playlist_sync completes within 50ms for 1000 tracks — achieved by operating on in-memory data only (Section 7.1)
- **AC-10**: All existing playlist operations continue working — achieved by only changing data source (gRPC metadata vs disk), not operation logic (Section 6.2)

## 2. Architecture

### 2.1. Protocol Seam Deepening

The gRPC `PlaylistAddTrack` protobuf message is extended with three new optional fields (`artist`, `album`, `has_local_file`) to carry full display metadata. The server populates all metadata fields from its in-memory Track objects during serialization. The TUI constructs Track objects directly from the deserialized protocol data without crossing any additional seam (filesystem or database).

This transforms the protocol from a shallow identifier relay into a deep module that provides complete track rendering data in a single message. The interface width grows by 3 fields but the implementation depth on the TUI side collapses from 40+ lines of I/O-bound parsing to a single pure-function constructor call.

### 2.2. Data Flow (Target State)

```
[Server Playlist (playback crate)]
     |
     | as_grpc_playlist_tracks() — serializes Track metadata fully
     | Populates: title, artist, album, duration, has_local_file, id
     v
[gRPC Wire (protobuf PlaylistTracks)]
     |
     | get_playlist() RPC / PlaylistShuffled / PlaylistAddTrack stream events
     v
[TUI Playback::load_from_grpc (no db_pod param)]
     |
     | Track::from_grpc_metadata(source, title, artist, album, duration, has_local_file)
     | Pure in-memory construction — zero disk I/O
     v
[TUIPlaylist.tracks: Vec<Track>]
     |
     | playlist_sync() — builds TableBuilder from in-memory Track data
     v
[Rendered Playlist Table]
```

### 2.3. Crate Responsibilities

| Crate | Responsibility After Change |
|-------|---------------------------|
| `lib` | Proto schema (extended PlaylistAddTrack), Track::from_grpc_metadata constructor, PlaylistAddTrackInfo domain struct (extended), proto-to-domain conversion traits |
| `playback` | Server playlist state, as_grpc_playlist_tracks (now populates all metadata), send_stream_ev_pl (populates artist/album in events) |
| `server` | gRPC service endpoints (no change needed — they delegate to playback) |
| `tui` | Playback::load_from_grpc (rewritten, no db_pod), TUIPlaylist::insert_track_at (new), handle_playlist_add (rewritten), playlist_sync (no change) |

### 2.4. Backward Compatibility Strategy

New proto fields use sequential field numbers (5, 6, 7) with `optional` keyword. This maintains wire compatibility: older deserializers ignore unknown fields, newer deserializers treat absent fields as `None`. The TUI and server are always the same version (built from same repo), so cross-version compatibility is a documented non-concern for v1.

## 3. Data Models

### 3.1. PlaylistAddTrack (Protobuf Message — Extended)

The wire-format message transmitted from server to TUI for every track in the playlist.

```protobuf
// lib/proto/player.proto
message PlaylistAddTrack {
  uint64 at_index = 1;
  oneof optional_title {
    string title = 2;
  }
  Duration duration = 3;
  TrackId id = 4;
  // NEW: display metadata fields for zero-I/O TUI rendering
  optional string artist = 5;
  optional string album = 6;
  optional bool has_local_file = 7;
}
```

Field semantics:
- `at_index`: Zero-based insertion position in the playlist
- `optional_title`: Track display title (from audio tags, or filename-derived fallback). Previously always None from server; now populated.
- `duration`: Track length as a Duration message (seconds + nanos)
- `id`: TrackId oneof containing either `path` (string), `url` (string), or `podcast_url` (string)
- `artist` (NEW): Track artist from audio tags. None if unavailable.
- `album` (NEW): Track album from audio tags. None if unavailable.
- `has_local_file` (NEW): Boolean indicating whether a podcast episode has been downloaded locally. Absent (treated as false) for non-podcast tracks.

### 3.2. PlaylistAddTrackInfo (Rust Domain Struct — Extended)

The deserialized domain representation used within the TUI for individual track-addition events.

```rust
// lib/src/player.rs
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistAddTrackInfo {
    pub at_index: u64,
    pub title: Option<String>,
    pub artist: Option<String>,       // NEW
    pub album: Option<String>,        // NEW
    pub duration: PlayerTimeUnit,
    pub trackid: playlist_helpers::PlaylistTrackSource,
    pub has_local_file: bool,         // NEW
}
```

### 3.3. Track (Rust Domain Object — New Constructor)

The core domain object representing a track in the playlist. A new constructor provides I/O-free creation from gRPC-provided metadata.

```rust
// lib/src/track.rs
impl Track {
    /// Construct a Track from gRPC-provided metadata without any disk I/O.
    ///
    /// This is the primary constructor for TUI-side Track creation when
    /// receiving playlist data from the server. The server has already
    /// parsed all metadata from disk; this constructor assembles the
    /// domain object from pre-parsed fields.
    ///
    /// # Arguments
    /// * `source` - The track identifier (Path, Url, or PodcastUrl variant)
    /// * `title` - Display title (falls back to filename derivation if None)
    /// * `artist` - Display artist (None if unavailable)
    /// * `album` - Display album (None if unavailable)
    /// * `duration` - Track duration (None if unavailable)
    /// * `has_local_file` - Whether a podcast episode has a local download
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
                file_type: None, // inferred from extension at display time if needed
            }),
            PlaylistTrackSource::Url(url) => MediaTypes::Radio(RadioTrackData::new(url)),
            PlaylistTrackSource::PodcastUrl(url) => {
                let localfile = if has_local_file {
                    Some(PathBuf::new()) // sentinel: exists but path not transmitted
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

### 3.4. TUIPlaylist (New Method)

```rust
// tui/src/ui/model/playlist.rs
impl TUIPlaylist {
    /// Insert a pre-constructed Track at a specific index without disk I/O.
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

## 4. API Design

### 4.1. GetPlaylist RPC (Existing — Response Extended)

The existing `GetPlaylist` RPC returns a `PlaylistTracks` response. No endpoint signature change is required — only the response payload content changes (server now populates the new metadata fields).

**Response (PlaylistTracks):**

```protobuf
message PlaylistTracks {
  uint64 current_track_index = 1;
  repeated PlaylistAddTrack tracks = 2;
}
```

Each `PlaylistAddTrack` in the response now carries title, artist, album, duration, and has_local_file populated from server in-memory Track objects.

**Error Cases:**

- Track metadata unavailable (corrupted file, parse failure): Server sends the track with path/URL identifier and any available fields (duration if known). Unavailable fields are left absent (None). The TUI displays a filename-derived fallback. (SCENARIO-017, SCENARIO-018, SCENARIO-020)
- Track file not found on disk: Server sends the track with its path identifier and empty metadata. Server does not crash or skip the track. (SCENARIO-020)

### 4.2. Stream Events — PlaylistAddTrack (Extended Payload)

Individual track additions arrive as stream events from server to TUI. The event payload now carries full metadata.

**Event Payload (within UpdatePlaylist):**

```protobuf
// Already defined in PlaylistAddTrack message — same fields apply
// Server populates title, artist, album, duration, has_local_file
// for every track addition event
```

**Error Cases:**

- Metadata unavailable for added track: Server sends available fields, TUI falls back to filename display.

### 4.3. Stream Events — PlaylistShuffled (Full Metadata)

When the server shuffles the playlist (including after spec-04 background loading completes), the entire playlist is re-transmitted with full metadata for all tracks.

**Behavior**: The `PlaylistShuffled` event triggers `handle_playlist_shuffled` on the TUI, which calls `load_from_grpc` with the full playlist data. Since `load_from_grpc` now uses `Track::from_grpc_metadata`, no disk I/O occurs. (SCENARIO-003, SCENARIO-007, SCENARIO-012, SCENARIO-013)

## 5. Implementation Details

### 5.1. Server-Side Metadata Population (as_grpc_playlist_tracks)

Location: `playback/src/playlist.rs` — method `as_grpc_playlist_tracks()`

The server iterates its in-memory `tracks: Vec<Track>` and serializes each Track into a `PlaylistAddTrack` proto message. Currently sets `optional_title: None` and omits artist/album. After modification:

```rust
fn track_to_proto(track: &Track, index: usize) -> protobuf::PlaylistAddTrack {
    protobuf::PlaylistAddTrack {
        at_index: index as u64,
        optional_title: track.title().map(|title_str| {
            protobuf::playlist_add_track::OptionalTitle::Title(title_str.to_string())
        }),
        duration: track.duration().map(protobuf::Duration::from),
        id: Some(track_id_from_track(track)),
        artist: track.artist().map(|artist_str| artist_str.to_string()),
        album: track.as_track().and_then(|track_data| {
            track_data.album().map(|album_str| album_str.to_string())
        }),
        has_local_file: if track.as_podcast().is_some() {
            Some(track.as_podcast().is_some_and(|podcast_data| podcast_data.has_localfile()))
        } else {
            None // omit for non-podcast tracks (saves 1 byte on wire)
        },
    }
}
```

The same population logic applies to individual track addition events in `send_stream_ev_pl` methods.

### 5.2. Server-Side Stream Event Metadata (send_stream_ev_pl)

Location: `playback/src/playlist.rs` — methods that emit `PlaylistAddTrack` stream events

The existing stream event emission already sends `title` for individual additions but not for bulk responses. After this change, ALL paths that emit `PlaylistAddTrack` messages (both bulk `as_grpc_playlist_tracks` and individual events in `insert_track`, `add_track_back`, `add_track_front`, etc.) populate artist, album, and has_local_file from the Track object's in-memory state.

### 5.3. TUI load_from_grpc Rewrite

Location: `tui/src/ui/model/mod.rs` — method `Playback::load_from_grpc`

The `db_pod: &DBPod` parameter is removed. The method becomes a pure data transformation:

```rust
impl Playback {
    /// Load a full playlist from gRPC response. Zero disk I/O.
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
            let duration = proto_track.duration.map(Duration::from);
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

### 5.4. TUI handle_playlist_add Rewrite

Location: `tui/src/ui/components/playlist.rs` — method `handle_playlist_add`

The existing method calls `self.playback.playlist.add_tracks(...)` which invokes `Track::read_track_from_path`. The rewrite constructs a Track from `PlaylistAddTrackInfo` fields and calls `insert_track_at`:

```rust
fn handle_playlist_add(&mut self, info: PlaylistAddTrackInfo) {
    let index = info.at_index as usize;
    let track = Track::from_grpc_metadata(
        info.trackid,
        info.title,
        info.artist,
        info.album,
        Duration::from_secs(info.duration as u64).into(),
        info.has_local_file,
    );
    self.playback.playlist.insert_track_at(index, track);
    self.playlist_sync();
}
```

### 5.5. Proto-to-Domain Conversion Updates (From/TryFrom Traits)

Location: `lib/src/player.rs`

The `From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist` implementation is updated to serialize artist, album, and has_local_file when emitting `PlaylistAddTrack` events. The reverse `TryFrom<protobuf::UpdatePlaylist> for UpdatePlaylistEvents` is updated to deserialize these fields into `PlaylistAddTrackInfo`.

### 5.6. Title Fallback Logic

When `title` is None in the gRPC response (metadata unavailable), the TUI displays a human-readable name derived from the track path. This fallback is handled in the existing `playlist_sync()` table builder which already calls `track.title_or_filename()` (or equivalent path-based fallback). The `Track::from_grpc_metadata` constructor stores `title: None` and the display layer derives the fallback from the path stored in `inner`. (SCENARIO-016, SCENARIO-017, SCENARIO-025)

### 5.7. Server-Side Title Population for Missing Tags

When a track's audio file has no title tag, the server populates `optional_title` with a filename-derived display name (filename without extension). This ensures the TUI always receives a usable display name without needing filesystem access. (SCENARIO-016)

Location: `playback/src/playlist.rs` — within the proto serialization logic. If `track.title()` returns `None`, derive title from `track.path().file_stem()`.

## 6. Testing Strategy

The testing approach verifies both correctness (metadata flows through the protocol and renders correctly) and performance (timing constraints from AC-01, AC-02, AC-09 are met). Tests are structured as unit tests for individual components and integration tests for the end-to-end data flow.

### 6.1. Unit Tests

- `Track::from_grpc_metadata` constructor produces correct Track for all three source variants (Path, Url, PodcastUrl)
- `Track::from_grpc_metadata` with has_local_file=true produces PodcastTrackData with localfile sentinel (Some(PathBuf::new()))
- `Track::from_grpc_metadata` with has_local_file=false produces PodcastTrackData with localfile=None
- `Track::from_grpc_metadata` with title=None stores None (display layer handles fallback)
- `Track::from_grpc_metadata` with all fields populated stores all metadata correctly
- `TUIPlaylist::insert_track_at` inserts at beginning, middle, and end correctly
- `TUIPlaylist::insert_track_at` with index >= len appends at end
- `PlaylistAddTrackInfo` From/TryFrom serialization round-trips artist, album, has_local_file correctly
- Server `track_to_proto` populates title from Track::title()
- Server `track_to_proto` populates artist from Track::artist()
- Server `track_to_proto` populates album from Track::as_track().album()
- Server `track_to_proto` populates has_local_file from Track::as_podcast().has_localfile()
- Server `track_to_proto` omits has_local_file field entirely for non-podcast tracks

### 6.2. Integration Tests

- `load_from_grpc` with 10-track PlaylistTracks proto (mixed sources) produces correct TUIPlaylist without filesystem access
- `load_from_grpc` with empty PlaylistTracks produces empty playlist without error (SCENARIO-024)
- `load_from_grpc` with tracks missing metadata fields displays filename fallback (SCENARIO-025)
- `handle_playlist_add` with full metadata constructs and inserts Track at correct index
- End-to-end: server `as_grpc_playlist_tracks` output deserialized and fed to TUI `load_from_grpc` produces matching Track data
- Server stream event with PlaylistAddTrack carries all metadata fields through serialization

### 6.3. Performance Tests

- `load_from_grpc` with 1000-track proto completes in under 100ms (SCENARIO-001, SCENARIO-026)
- `load_from_grpc` with 5000-track proto completes in under 100ms (SCENARIO-026)
- `playlist_sync` table build with 1000 in-memory tracks completes in under 50ms (SCENARIO-021, SCENARIO-022)
- Serialized PlaylistAddTrack message size is under 300 bytes for typical tracks (wire overhead validation)

### 6.4. BDD Scenario References

- **SCENARIO-001** — performance — Covered (load_from_grpc timing test with 1000 tracks)
- **SCENARIO-002** — performance — Covered (same mechanism; small playlists trivially pass)
- **SCENARIO-003** — integration — Covered (shuffle event uses same load_from_grpc path)
- **SCENARIO-004** — integration — Covered (end-to-end timing from receipt to playlist_sync completion)
- **SCENARIO-005** — unit — Covered (title population in Track::from_grpc_metadata)
- **SCENARIO-006** — integration — Covered (server as_grpc_playlist_tracks includes all fields)
- **SCENARIO-007** — integration — Covered (shuffle event carries full metadata)
- **SCENARIO-008** — integration — Covered (individual add event carries metadata)
- **SCENARIO-009** — unit — Covered (server populates title from Track::title())
- **SCENARIO-010** — integration — Covered (load_from_grpc uses from_grpc_metadata, no disk access)
- **SCENARIO-011** — integration — Covered (no Track::read_track_from_path calls in new path)
- **SCENARIO-012** — integration — Covered (shuffle event processing via load_from_grpc)
- **SCENARIO-013** — integration — Covered (sequential events both use from_grpc_metadata)
- **SCENARIO-014** — unit — Covered (proto compilation test, field number verification)
- **SCENARIO-015** — unit — Covered (server sends title instead of None)
- **SCENARIO-016** — unit — Covered (server sends filename-derived title when tag missing)
- **SCENARIO-017** — integration — Covered (TUI displays fallback when metadata absent)
- **SCENARIO-018** — integration — Covered (server includes track with partial metadata)
- **SCENARIO-019** — integration — Covered (TUI handles missing duration gracefully)
- **SCENARIO-020** — integration — Covered (server does not crash for missing file)
- **SCENARIO-021** — performance — Covered (playlist_sync benchmark with 1000 tracks)
- **SCENARIO-022** — performance — Covered (linear scaling verification)
- **SCENARIO-023** — integration — Covered (existing operation tests with metadata protocol)
- **SCENARIO-024** — integration — Covered (empty playlist handling)
- **SCENARIO-025** — integration — Covered (all tracks missing metadata)
- **SCENARIO-026** — performance — Covered (5000 track load test)
- **SCENARIO-027** — integration — Covered (concurrent reload/shuffle resolves consistently)
- **SCENARIO-028** — integration — Covered (long metadata strings handled)

## 7. Non-Functional Requirements

### 7.1. Performance

- TUI main event loop MUST NOT be blocked for more than 100ms during playlist loading regardless of playlist size (AC-01). Validated by prototype: 0.068ms max for 1000 tracks.
- Playlist view MUST render within 200ms of receiving server response (AC-02). Validated by prototype: 0.130ms max combined load+render for 1000 tracks.
- `playlist_sync()` table build MUST complete within 50ms for 1000 tracks (AC-09). Validated by prototype: 0.062ms max.
- Server serialization overhead for metadata: <50ms for 1000 tracks (in-memory string copies only).
- Wire overhead: average 95.5 bytes per track (measured), maximum 272 bytes for outlier tracks with long metadata strings.

### 7.2. Reliability

- If metadata fields are missing/empty in gRPC response, TUI gracefully degrades to filename display (AC-08).
- Server MUST NOT crash or skip playlist transmission when track files are missing or corrupted (AC-08, SCENARIO-020).
- Concurrent playlist reload and shuffle events MUST resolve to a consistent final state (SCENARIO-027).
- Track with extremely long metadata (500+ char title) is handled without overflow (SCENARIO-028).

### 7.3. Observability

- TUI logs timing of playlist response processing at INFO level: "Processed {count} tracks in {elapsed_ms}ms".
- Server-side logging from spec-04 is sufficient for the server path (already logs track serialization counts).

### 7.4. Memory

- Additional memory from gRPC metadata: ~200 bytes/track * 1000 tracks = ~200KB transfer (negligible for local IPC).
- Sentinel PathBuf allocation: 24 bytes per podcast track (zero heap allocation for PathBuf::new()).
- No persistent memory increase on TUI side — Track objects already store title/artist/album fields (previously populated via disk I/O, now populated from gRPC).

## 8. Risks and Mitigations

- **Risk**: Sentinel PathBuf pattern causes confusion if future code calls `localfile()` expecting a real path
  - Likelihood: low
  - Impact: low
  - Mitigation: Document the sentinel with a code comment. The `has_localfile()` method only checks `is_some()`, which is the sole current usage. If future code needs the actual path, add `optional string local_file_path = 8` to the proto at that time.

- **Risk**: TUI `add_tracks` method still has disk I/O path that could be accidentally called
  - Likelihood: low
  - Impact: medium
  - Mitigation: Mark `track_from_path` and `track_from_podcasturi` as deprecated after migration. Add a compile-time warning. Remove if no callers remain after Phase 3.
  - **Resolution**: Risk eliminated. Methods were fully removed in Phase 3 (commit e96975a9) after confirming zero remaining callers. No disk I/O path remains in TUI playlist loading.

- **Risk**: Proto schema change breaks wire format if field numbers conflict
  - Likelihood: none (validated by prototype)
  - Impact: high
  - Mitigation: Existing fields use numbers 1-4. New fields use 5, 6, 7. Verified no conflicts. Proto3 optional semantics ensure forward/backward compatibility.
  - **Resolution**: Risk confirmed non-existent. Proto compiles cleanly with new fields.

- **Risk**: Server sends empty title for tracks with valid metadata (regression in serialization logic)
  - Likelihood: low
  - Impact: medium
  - Mitigation: Unit tests verify server populates title from Track::title(). Integration tests verify round-trip from server Track to TUI display.
  - **Resolution**: Validated by 38 integration tests in Phase 4 covering all metadata paths.

- **Risk**: Performance regression in server serialization (copying strings for 1000+ tracks)
  - Likelihood: none (validated by prototype)
  - Impact: low
  - Mitigation: String copies from in-memory data are sub-millisecond for 1000 tracks. Measured overhead is negligible.
  - **Resolution**: Performance tests confirm sub-1ms for 1000 tracks, well under 100ms ceiling.

---

## 9. Implementation Deviations

The following deviations from the original specification were identified during implementation and code review.

### 9.1. load_from_grpc Return Type Changed to LoadStats

- **Original Spec (Section 5.3)**: `load_from_grpc` returns `anyhow::Result<()>`
- **Actual**: `load_from_grpc` returns `anyhow::Result<LoadStats>` where `LoadStats { track_count: usize, elapsed: Duration }`
- **Reason**: Section 7.3 requires observability logging ("Processed {count} tracks in {elapsed_ms}ms"). Returning timing data to the caller enables both logging and programmatic test assertions without embedding logging framework calls inside the function.
- **Impact**: Minor. Callers that use `?` or `if let Err` continue to work without modification. LoadStats enables future INFO-level logging at call sites.

### 9.2. Dead Code Removal Instead of Deprecation

- **Original Spec (Section 5.4, Risk #2)**: Suggested deprecating `track_from_path` and `track_from_podcasturi` after migration.
- **Actual**: Methods were fully removed (not deprecated) along with the `add_tracks` method.
- **Reason**: After confirming zero remaining callers in Phase 3, deprecation would leave dead code with no path to removal. Full deletion is cleaner.
- **Impact**: None. No external consumers exist (crate is internal).

### 9.3. T-23 Completed During Phase 1

- **Original Plan**: T-23 (stream event metadata population) was scheduled for Phase 2.
- **Actual**: Completed during Phase 1's T-08 because adding fields to `PlaylistAddTrackInfo` struct required updating all construction sites to compile.
- **Reason**: Rust's exhaustive struct initialization pattern means new required fields must be populated at all construction sites simultaneously.
- **Impact**: None. Phase 2 scope was slightly smaller; correctness unaffected.

### 9.4. has_local_file Serialization Asymmetry

- **Original Spec**: Did not specify serialization behavior distinction between bulk and stream paths.
- **Actual**: In stream events (`From<UpdatePlaylistEvents>`), `has_local_file: false` serializes to `None` (omitted). In bulk responses (`as_grpc_playlist_tracks`), podcast tracks without local files serialize as `Some(false)`.
- **Reason**: Stream events use the domain struct pattern (false -> None to minimize wire overhead). Bulk response uses explicit podcast detection pattern for semantic clarity.
- **Impact**: None. Both deserialize correctly via `unwrap_or(false)`. Identified in adversarial review (A-01, Low severity).

### 9.5. LoadStats Not Yet Logged at Call Sites

- **Original Spec (Section 7.3)**: "TUI logs timing of playlist response processing at INFO level"
- **Actual**: `LoadStats` struct is computed and returned but not logged at either production call site.
- **Reason**: Infrastructure was added during Phase 4 for testability. Logging at call sites was identified as a Low-severity finding (F-01) in code review but not blocking for approval.
- **Impact**: Observability requirement partially unmet. The infrastructure exists; adding the log statement is a one-line change at each call site.
