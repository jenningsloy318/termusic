# Research Report: Async TUI Playlist Loading

- **Date**: 2026-06-26
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-26
- **Technologies**: Rust, prost 0.14.4, tonic 0.14.6, protobuf3, tokio 1.52, lofty 0.24.0, gRPC streaming
- **Freshness**: Fresh (< 6mo)

---

## Executive Summary

- The TUI's `load_from_grpc` method currently calls `Track::read_track_from_path` for every track, performing redundant disk I/O when the server already holds all metadata in memory. This is the root cause of the multi-second freeze (SRC-001, SRC-002).
- Protobuf3 guarantees full backward wire compatibility when adding new optional fields with unused field numbers. The existing `PlaylistAddTrack` message uses field numbers 1-4, leaving room for `artist` (5) and `album` (6) without breaking existing serialized data (SRC-003, SRC-004).
- The recommended approach is extending the gRPC protocol to transmit full display metadata (title, artist, album, duration) from server to TUI, eliminating 100% of TUI-side disk reads. This follows established patterns from other Rust music player projects (SRC-005, SRC-006).
- **Recommendation** (High confidence): Implement Option A (Protocol Extension) as the primary approach. The server already has all metadata in memory; transmitting it over gRPC is the minimal-overhead, root-cause solution that aligns with existing TODO comments in the codebase.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| protobuf optional fields backward compatibility best practices 2025 | DeepWiki (tokio-rs/prost) | 1 | 1 |
| tonic streaming server updates patterns | DeepWiki (hyperium/tonic) | 1 | 1 |
| termusic TUI playlist loading gRPC metadata | DeepWiki (tramhao/termusic) | 1 | 1 |
| ratatui async non-blocking event loop patterns | DeepWiki (ratatui/ratatui) | 1 | 1 |
| optional string artist album proto3 music player | GitHub Code Search | 2 | 2 |
| QSync musync.proto Track message | WebFetch (GitHub raw) | 1 | 1 |
| spotifatius service.proto Track | WebFetch (GitHub raw) | 1 | 1 |
| prost optional fields documentation | WebFetch (docs.rs) | 1 | 1 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-001 | termusic codebase: `tui/src/ui/model/mod.rs:187-232` (load_from_grpc) | Codebase | 2026-06 | Fresh | High |
| SRC-002 | termusic codebase: `tui/src/ui/model/playlist.rs:157-178` (track_from_path calling Track::read_track_from_path) | Codebase | 2026-06 | Fresh | High |
| SRC-003 | prost documentation — proto3 optional field handling: generates `Option<String>` for optional string fields | Official docs | 2026 | Fresh | High |
| SRC-004 | DeepWiki: tokio-rs/prost — backward compatibility with new optional fields in proto3 | AI Documentation | 2026-06 | Fresh | High |
| SRC-005 | QSync (Discreater/QSync) — `musync.proto` Track message with optional artist, album, duration fields | GitHub | 2023 | Dated | Medium |
| SRC-006 | spotifatius (AndreasBackx/spotifatius) — `service.proto` Track message with optional artist, album, title | GitHub | 2023 | Dated | Medium |
| SRC-007 | DeepWiki: hyperium/tonic — server-side streaming patterns with ResponseStream trait | AI Documentation | 2026-06 | Fresh | High |
| SRC-008 | DeepWiki: ratatui/ratatui — async patterns using channels (mpsc) to communicate between background tasks and main event loop | AI Documentation | 2026-06 | Fresh | High |
| SRC-009 | termusic codebase: `lib/proto/player.proto:228-247` (PlaylistAddTrack message, field numbers 1-4) | Codebase | 2026-06 | Fresh | High |
| SRC-010 | termusic codebase: `playback/src/playlist.rs:1030-1053` (as_grpc_playlist_tracks sets optional_title: None) | Codebase | 2026-06 | Fresh | High |
| SRC-011 | termusic codebase: `lib/src/track.rs:184-191` (Track struct with inner, duration, title, artist fields) | Codebase | 2026-06 | Fresh | High |
| SRC-012 | termusic codebase: `tui/src/ui/components/orx_music_library/scanner.rs:25-35` (library_scan background thread pattern using std::thread + tx_to_main channel) | Codebase | 2026-06 | Fresh | High |
| SRC-013 | DeepWiki: tramhao/termusic — TUI playlist loading architecture, gRPC client-server model | AI Documentation | 2026-06 | Fresh | High |

---

## Options Comparison

REQUIRED: Compare 3-5 viable options identified during research.

| Criterion | Option A: Protocol Extension (Full Metadata in gRPC) | Option B: Background Thread Loading | Option C: Hybrid (Protocol + Fallback) | Option D: Chunked Progressive Loading |
|-----------|------------------------------------------------------|--------------------------------------|----------------------------------------|---------------------------------------|
| Maturity | 5 | 4 | 4 | 3 |
| Community/Support | 5 | 4 | 4 | 3 |
| Performance | 5 | 3 | 5 | 3 |
| Bundle Size / Footprint | 4 | 4 | 3 | 4 |
| Learning Curve | 4 | 4 | 3 | 3 |
| Maintenance Burden | 5 | 3 | 2 | 2 |
| Project Fit | 5 | 4 | 4 | 3 |
| Innovation/Momentum | 4 | 3 | 4 | 3 |
| **TOTAL** | **37** | **29** | **29** | **24** |

### Option A: Protocol Extension (Full Metadata in gRPC)

Extend the `PlaylistAddTrack` protobuf message with `optional string artist = 5` and `optional string album = 6`. Populate `optional_title` (currently always `None`) on the server side. Construct `Track` objects directly from gRPC data in the TUI without any disk I/O.

- **Strengths**: Eliminates the root cause entirely (SRC-001, SRC-010). Zero disk I/O on TUI side. Server already holds all metadata in memory (SRC-011). Proto already has TODO comments requesting this exact change (SRC-002, line 173). Follows established patterns from other music player projects (SRC-005, SRC-006). Wire-compatible with existing field numbers (SRC-004, SRC-009). Minimal runtime overhead — data is already in memory, just needs serialization. ~200 bytes/track additional transfer is negligible for local IPC.
- **Weaknesses**: Requires protobuf schema change (SRC-009). Requires new `Track` constructor that skips filesystem access. Slightly larger gRPC messages (~200KB for 1000 tracks). All-or-nothing — if server has not finished loading metadata, empty fields are sent.
- **Best For**: This project — the exact scenario of same-version client/server with shared filesystem and server-side metadata already in memory.

### Option B: Background Thread Loading

Keep gRPC protocol unchanged. Move `load_from_grpc` processing to a background thread (following the `library_scan` pattern in `scanner.rs`). Show file paths or placeholder names in the playlist immediately, then update with full metadata as tracks are parsed. Send completed tracks back via the existing `tx_to_main` channel.

- **Strengths**: No protocol changes required (SRC-009 unchanged). Follows existing pattern used by library scanner (SRC-012). TUI event loop remains responsive immediately. Can be implemented incrementally.
- **Weaknesses**: Still performs redundant disk I/O — server already has this data (SRC-010, SRC-011). Adds complexity (partial playlist state, progressive updates, race conditions). Brief visual flash of raw file paths before metadata resolves. Does not fix the root cause — just moves the symptom. User might interact with partially-loaded playlist causing inconsistency.
- **Best For**: When protocol changes are blocked (e.g., cross-version compatibility needed) or as a transitional step.

### Option C: Hybrid (Protocol Extension + Background Thread Fallback)

Implement Option A as the primary path. Add detection: if the server provides metadata in the gRPC response (artist/title fields present), use it directly. If not (e.g., connecting to older server), fall back to Option B's background thread loading.

- **Strengths**: Best of both worlds (SRC-004). Backward compatible with hypothetical older server versions. Future-proofs against version mismatches.
- **Weaknesses**: More code to maintain — two code paths that must both be tested (SRC-001, SRC-012). The fallback path is unlikely to be exercised since both binaries are built from the same repo. Increased cognitive load for maintainers. Over-engineering for a single-binary project.
- **Best For**: Projects where client and server may be different versions, or multi-platform deployments.

### Option D: Chunked Progressive Loading via Streaming

Replace the single `GetPlaylist` unary RPC with a server-side streaming RPC that sends tracks in batches (e.g., 50 at a time). The TUI renders each batch as it arrives, providing progressive visual feedback.

- **Strengths**: Immediate visual feedback as first batch renders quickly (SRC-007). Natural backpressure via gRPC streaming. Could handle arbitrarily large playlists without memory spikes.
- **Weaknesses**: Requires significant protocol redesign — changes from unary to streaming RPC (SRC-009). Does not eliminate TUI-side disk I/O unless combined with Option A's metadata inclusion. Adds complexity to playlist state management (partial loads, batch merging). Existing stream subscription pattern (`SubscribeServerUpdates`) is already used for events, not bulk data. Over-engineered for the actual problem — playlists are typically <5000 tracks and fit in one message.
- **Best For**: Extremely large playlists (10,000+) where even serialization time is a concern — not this project's primary use case.

---

## Deprecation Warnings

No deprecation concerns identified for current stack.

- prost 0.14.4 and tonic 0.14.6 are current stable releases.
- lofty 0.24.0 is the current stable release.
- tokio 1.52 is actively maintained.
- protobuf3 syntax is the current recommended proto syntax.

---

## Best Practices

### BP-001: Add New Protobuf Fields as Optional with Unused Field Numbers

- **Pattern**: When extending a protobuf3 message, add new fields using the `optional` keyword and unused field numbers. Never reuse or reorder existing field numbers.
- **Rationale**: Proto3 guarantees that unknown fields are preserved and ignored by older deserializers. Adding optional fields with new numbers ensures full wire-format backward and forward compatibility. The `optional` keyword in proto3 generates `Option<T>` in prost, allowing explicit presence tracking (SRC-003, SRC-004).
- **Source**: SRC-003, SRC-004
- **Confidence**: High
- **Example**:
```protobuf
// Existing message (fields 1-4 used)
message PlaylistAddTrack {
  uint64 at_index = 1;
  oneof optional_title { string title = 2; }
  Duration duration = 3;
  TrackId id = 4;
  // New fields for display metadata
  optional string artist = 5;
  optional string album = 6;
}
```

### BP-002: Construct Domain Objects from gRPC Data Without Filesystem Access

- **Pattern**: Create a dedicated constructor (e.g., `Track::from_grpc_metadata`) that builds the domain object purely from in-memory data provided by the gRPC response, bypassing any file system access.
- **Rationale**: Separates the "metadata reading" concern from the "Track construction" concern. Enables testing without filesystem. Eliminates the I/O bottleneck identified in the TUI (SRC-001, SRC-002). Follows the pattern used by `Track::new_radio` and `Track::from_podcast_episode` which already construct tracks without reading files (SRC-011).
- **Source**: SRC-001, SRC-011
- **Confidence**: High
- **Example**:
```rust
impl Track {
    /// Create a Track from gRPC-provided metadata without filesystem access.
    pub fn from_grpc_metadata(
        path: PathBuf,
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        duration: Option<Duration>,
    ) -> Self {
        let track_data = TrackData { path, album, file_type: None };
        Self { inner: MediaTypes::Track(track_data), duration, title, artist }
    }
}
```

### BP-003: Server Should Always Populate Available Metadata in gRPC Responses

- **Pattern**: When the server builds gRPC responses, it should include all available metadata fields rather than sending `None` and forcing the client to re-derive data. Even partial metadata is better than none.
- **Rationale**: The server already parses and holds all display metadata (title, artist, album, duration) for every track in its `Playlist` (SRC-010, SRC-011). Sending `optional_title: None` when the title is known is a missed optimization. Populating fields eliminates redundant client-side work (SRC-001).
- **Source**: SRC-010, SRC-011
- **Confidence**: High

### BP-004: Use Channels for Background-to-Main Communication in TUI Apps

- **Pattern**: When background work must communicate results to a TUI event loop, use an unbounded channel (`tokio::sync::mpsc::UnboundedSender` or equivalent) to send messages that the event loop polls during its tick cycle.
- **Rationale**: This pattern is already established in the termusic codebase via `tx_to_main` (SRC-012) and is the recommended approach for ratatui-based applications needing async updates (SRC-008). It avoids blocking the render loop while allowing background tasks to deliver results asynchronously.
- **Source**: SRC-008, SRC-012
- **Confidence**: High

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| Blocking the TUI event loop with synchronous disk I/O | Freezes the entire UI for seconds; no rendering or input handling occurs during the block. For 200+ tracks with lofty metadata parsing, this manifests as a 2-5 second hang. | Move disk I/O to a background thread, or better yet, eliminate it by receiving data from the server that already has it. | SRC-001, SRC-002 |
| Server sending `None` for metadata it already possesses | Forces the client to independently re-read the same files from disk, duplicating work and creating the exact performance bottleneck this feature eliminates. | Always populate available fields in gRPC responses. | SRC-010 |
| Using `oneof` for simple optional fields in proto3 | The codebase uses `oneof optional_title { string title = 2; }` as a workaround for older protobuf versions lacking the `optional` keyword. This creates unnecessary nesting in the generated code (`playlist_add_track::OptionalTitle::Title(v)`). | For new fields, use the `optional` keyword directly (supported since protobuf 3.15, and prost handles it natively). Keep existing `oneof` for backward compatibility. | SRC-003, SRC-009 |
| Re-reading metadata from database for podcast episodes during playlist load | The TUI queries the podcast SQLite database for each podcast track during `load_from_grpc`, which is I/O that could be eliminated if the server sends episode metadata. | Include podcast episode title and duration in the gRPC response alongside the podcast URL. | SRC-002, SRC-013 |

---

## Implementation Considerations

### Performance

- Server serialization overhead for metadata is negligible: ~200 bytes/track (artist + album strings) * 1000 tracks = ~200KB additional gRPC payload. This is well within acceptable limits for local IPC over Unix Domain Socket (SRC-009, SRC-010).
- The `playlist_sync()` table-building function iterates in-memory data only. With the protocol extension, it will operate on pre-populated `Track` objects requiring zero I/O, easily meeting the 50ms target for 1000 tracks (SRC-001).
- Prost's generated code uses zero-copy deserialization where possible, and `String` fields are moved rather than cloned during message consumption (SRC-003).

### Security

- No new attack surface introduced. All data flows over the existing gRPC channel between co-located processes (same machine, same user). The metadata fields (artist, album, title) are strings that were already parsed from the filesystem by the server — no user-controlled external input is introduced (SRC-009).

### Compatibility

- Adding optional fields 5 and 6 to `PlaylistAddTrack` is wire-compatible: older deserializers ignore unknown field numbers, and newer deserializers treat absent optional fields as `None` (SRC-003, SRC-004).
- The project builds both client and server from the same repository commit, so cross-version concerns are theoretical rather than practical. The requirements document explicitly states this assumption (SRC-001).
- The `oneof optional_title` pattern in the existing proto uses field number 2. New fields using numbers 5+ will not conflict (SRC-009).

---

## Contradictions Found

No contradictions found across sources.

All sources consistently agree that:
1. Adding optional fields to proto3 messages is backward-compatible (SRC-003, SRC-004, SRC-005, SRC-006).
2. The server already holds all necessary metadata in memory (SRC-010, SRC-011, SRC-013).
3. The TUI-side disk I/O during `load_from_grpc` is the root cause of the freeze (SRC-001, SRC-002, SRC-013).

---

## Issues and Ambiguities

- **ISS-001**: The existing `oneof optional_title` uses field number 2, while a straight `optional string title` would also naturally use `Option<String>` in prost. Should the existing `oneof` pattern be migrated to `optional` for consistency with the new `artist` and `album` fields? Changing the wire encoding of an existing field would break compatibility, so the answer is likely "no — keep `oneof` for title, use `optional` for new fields." This creates an inconsistency in the generated Rust code (title accessed via enum unwrap, artist/album via direct `Option<String>`), but it preserves wire stability.

- **ISS-002**: The `PlaylistAddTrackInfo` struct (used for stream events from server to TUI) currently only carries `at_index`, `title`, `duration`, and `trackid`. It lacks `artist` and `album` fields. This struct must be extended to carry the new metadata fields so that individual `PlaylistAddTrack` stream events also avoid TUI-side disk reads. The question is whether to add these as `Option<String>` fields or restructure the info type.

- **ISS-003**: The `file_type` field on `TrackData` is currently populated by `parse_metadata_from_file` and used in some TUI logic. If we construct `Track` from gRPC metadata without filesystem access, `file_type` will be `None`. Need to verify what TUI logic depends on `file_type` and whether inferring it from the file extension (which is available from the path string) is sufficient.

- **ISS-004**: For podcast episodes, the TUI currently calls `podcast_db.get_episode_by_url` to construct a `Track::from_podcast_episode`. The podcast track stores a `localfile` path and `image_url`. Should the gRPC protocol also transmit these podcast-specific fields, or is title + duration + URL sufficient for playlist display purposes? The `playlist_sync()` function only uses `title()`, `artist()`, and `duration()` for display, suggesting title + duration is sufficient for the playlist view specifically.

- **ISS-005**: The `as_grpc_playlist_tracks` function is called from both the `get_playlist` RPC handler and the `shuffle` event emission. Both paths must be updated to populate the new metadata fields. Additionally, individual `add_track` events in the `add_episode` and `add_track` methods on the server-side `Playlist` must also populate artist/album when building `PlaylistAddTrackInfo`.

---

## References

### Primary Sources (Official Documentation)

- SRC-003: prost documentation (docs.rs/prost) — proto3 optional field handling and code generation
- SRC-004: DeepWiki: tokio-rs/prost — backward compatibility rules for optional fields in proto3

### Secondary Sources (Blogs, Papers, Guides)

- SRC-007: DeepWiki: hyperium/tonic — server-side streaming patterns and message size management
- SRC-008: DeepWiki: ratatui/ratatui — async data loading patterns for terminal applications
- SRC-013: DeepWiki: tramhao/termusic — TUI playlist loading architecture and gRPC communication

### Community Sources (GitHub, Reddit, X/Twitter)

- SRC-005: QSync (Discreater/QSync) musync.proto — https://github.com/Discreater/QSync/blob/master/protos/musync.proto
- SRC-006: spotifatius (AndreasBackx/spotifatius) service.proto — https://github.com/AndreasBackx/spotifatius/blob/main/proto/service.proto

### Codebase Sources

- SRC-001: termusic `tui/src/ui/model/mod.rs:187-232` — load_from_grpc function showing Track::read_track_from_path calls
- SRC-002: termusic `tui/src/ui/model/playlist.rs:157-178` — track_from_path with TODO comment about gRPC refactor
- SRC-009: termusic `lib/proto/player.proto:228-247` — PlaylistAddTrack message definition (fields 1-4)
- SRC-010: termusic `playback/src/playlist.rs:1030-1053` — as_grpc_playlist_tracks sending optional_title: None
- SRC-011: termusic `lib/src/track.rs:184-191` — Track struct definition with all metadata fields
- SRC-012: termusic `tui/src/ui/components/orx_music_library/scanner.rs:25-35` — background thread pattern with tx_to_main channel
