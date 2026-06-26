# Requirements: Async TUI Playlist Loading

- **Date**: 2026-06-26
- **Author**: super-dev:requirements-clarifier
- **Type**: enhancement
- **Priority**: high
- **Status**: draft

---

## Executive Summary

After the TUI connects to the server, it requests the playlist and then blocks the main event loop for several seconds while re-reading file metadata (title, artist, album, duration) from disk for every track via `Track::read_track_from_path`. This happens because the gRPC protocol only transmits track identifiers (file paths/URLs) and duration — not the full display metadata (title, artist, album). The TUI must independently parse each audio file's tags (via lofty) to populate the playlist view, causing a multi-second freeze on the main thread where no rendering or user input processing occurs. This requirement specifies eliminating this blocking behavior by either offloading metadata reading to a background thread or — more efficiently — extending the gRPC protocol to transmit all display metadata from the server (which already has it loaded).

## The Real Need (Root Cause Analysis)

### Surface Request

After TUI starts, it takes several seconds to read/load the playlist and show playlist content. Investigate async reading of playlist data while waiting for server start and while TUI initializes.

### 5 Whys Analysis

1. **Why**: The TUI takes several seconds after startup to display the playlist content.
2. **Why**: The TUI's `load_from_grpc` method calls `Track::read_track_from_path` for every track, which parses audio file metadata via lofty — this is I/O-bound and executed on the main event loop thread.
3. **Why**: The gRPC `PlaylistTracks` response only contains track identifiers (file paths) and duration, but NOT the display metadata (title, artist, album) needed to render the playlist view.
4. **Why**: The protobuf schema (`PlaylistAddTrack`) currently only has fields for `at_index`, `optional_title`, `duration`, and `id` — and even the title field is not populated by the server (`optional_title: None`). Artist and album are absent from the protocol entirely.
5. **Why**: The original architecture assumed TUI and server both have local filesystem access, so metadata was read independently. But this creates duplicated work — the server already parses all metadata during its startup loading (now async via spec-04) and holds it in memory. The TUI redundantly re-reads the same files.

### Job to Be Done

When I launch termusic and the TUI connects to the server,
I want to see the playlist contents rendered immediately (or with minimal delay),
So I can start browsing and playing music without waiting for a multi-second metadata parsing freeze.

- **Functional**: Eliminate the blocking metadata parse on the TUI main thread during playlist load
- **Emotional**: The app feels instant and responsive — no unexplained freezes after the connection establishes
- **Social**: N/A (single-user terminal application)

## Stakeholders

- **End user (music listener with large playlists)**: Experiences a frozen/blank TUI for several seconds after connection; primary beneficiary
- **TUI main event loop**: Currently blocked by synchronous I/O during `load_from_grpc`; must remain responsive for keyboard input and rendering
- **Server**: Already holds all display metadata in memory; could transmit it to eliminate redundant TUI-side disk reads
- **Termusic maintainers**: Must balance protocol changes vs minimal diff; existing TODO comments in code already acknowledge this design debt

## Workflow Context

### Before (Current State)

1. TUI connects to server (fast, ~100ms since spec-04)
2. TUI sends `SelfReloadPlaylist` command
3. `ServerRequestActor` calls `get_playlist()` gRPC → server returns `PlaylistTracks` containing: `current_track_index` + repeated `PlaylistAddTrack {at_index, duration, id(path/url), optional_title: None}`
4. TUI receives the response in `ServerReqResponse::FullPlaylist`
5. **BOTTLENECK**: `load_from_grpc` iterates over all tracks and calls `Track::read_track_from_path(path)` for each one — this calls `parse_metadata_from_file` via lofty, performing disk I/O and tag parsing for album, artist, title, duration
6. For 200+ tracks this takes 2-5 seconds; during this time the TUI event loop is blocked — no rendering, no input handling
7. After `load_from_grpc` completes, `playlist_sync()` builds the table data and the UI finally renders

The same bottleneck occurs when the server completes async loading (spec-04) and sends a `PlaylistShuffled` stream event — the TUI again calls `load_from_grpc` which re-reads all metadata from disk.

### After (Desired State)

1. TUI connects to server (fast, ~100ms)
2. TUI sends `SelfReloadPlaylist` command
3. Server returns full display metadata (title, artist, album, duration) alongside track identifiers
4. TUI constructs `Track` objects directly from the gRPC response data — no disk I/O needed
5. Playlist renders immediately (<100ms after receiving the response)
6. The TUI event loop remains unblocked throughout

Alternative (if protocol changes are deferred): metadata loading happens on a background thread, with the TUI showing tracks progressively or a loading indicator.

## Solution Options

### Option 1: Extend gRPC Protocol to Include Full Display Metadata (Recommended)

Add `artist` and `album` fields to the `PlaylistAddTrack` protobuf message. Have the server populate `optional_title`, duration, artist, and album when building the gRPC response. Modify the TUI's `load_from_grpc` to construct `Track` objects from the gRPC data directly — zero disk I/O.

- **Pros**: Eliminates the root cause (redundant disk I/O); zero disk reads on TUI side; server already has all metadata in memory; proto already has TODO comments requesting this change; minimal runtime overhead (data is already in memory on server side, just needs serialization); works even if TUI is on a different machine (future remote scenario)
- **Cons**: Requires protobuf schema change (new fields); slightly larger gRPC messages (~100-200 bytes per track for artist/album strings); requires careful handling of the `Track` construction to work without a path-based read
- **Effort**: medium

### Option 2: Background Thread for TUI-Side Metadata Loading

Keep the gRPC protocol unchanged. Move the `load_from_grpc` processing to a background thread (or rayon pool). Show file paths in the playlist immediately, then update with metadata as it resolves. Use the existing `tx_to_main` pattern (like `library_scan`) to send completed tracks back.

- **Pros**: No protocol changes; follows existing pattern used by music library scanner; TUI remains responsive immediately
- **Cons**: Still performs redundant disk I/O (server already has this data); adds complexity to the TUI model (partial playlist state, progressive updates); race conditions if user interacts with playlist during loading; file paths shown briefly before metadata resolves (visual flash)
- **Effort**: medium

### Option 3: Hybrid — Protocol Extension + Graceful Fallback

Extend the protocol (Option 1) and implement a fallback path: if the server provides metadata in the gRPC response, use it directly; if not (e.g., old server version), fall back to background thread loading (Option 2).

- **Pros**: Best of both worlds; backward compatible; future-proof
- **Cons**: More code to maintain; the fallback path may rarely be exercised
- **Effort**: high

## Acceptance Criteria

- **AC-01**: The TUI main event loop MUST NOT be blocked for more than 100ms during playlist loading, regardless of playlist size (100, 500, 1000+ tracks).
- **AC-02**: The playlist view MUST render with track display names (title or filename), artist, and duration within 200ms of receiving the server's playlist response.
- **AC-03**: The server's `GetPlaylist` gRPC response and `PlaylistShuffled`/`PlaylistAddTrack` stream events MUST include sufficient display metadata (title, artist, album, duration) for the TUI to render the playlist without additional disk I/O.
- **AC-04**: The TUI's `load_from_grpc` MUST construct `Track` objects from gRPC-provided metadata without calling `Track::read_track_from_path` or any filesystem operation.
- **AC-05**: When the server's async background loading (spec-04) completes and sends a `PlaylistShuffled` event, the TUI MUST process it without re-reading metadata from disk — the event payload MUST contain full display metadata.
- **AC-06**: The protobuf `PlaylistAddTrack` message MUST be extended with optional `artist` and `album` string fields while maintaining backward wire compatibility with existing field numbers.
- **AC-07**: The server MUST populate the `optional_title` field (currently always `None`) with the track's title when building gRPC playlist responses.
- **AC-08**: For tracks where metadata is unavailable (file not found, parse error), the server MUST send the track with available fields (at minimum: the path/URL identifier and duration if known) and the TUI MUST display a reasonable fallback (e.g., filename derived from path).
- **AC-09**: The `playlist_sync()` table-building step MUST complete within 50ms for a 1000-track playlist (it iterates in-memory data only, no I/O).
- **AC-10**: All existing playlist operations (add, remove, swap, shuffle, clear) MUST continue to work correctly with the new metadata-carrying protocol.

## Non-Functional Requirements

- **Performance** (critical): Playlist must render within 200ms of receiving server response. Zero disk I/O on the TUI side for playlist population. Server serialization overhead for metadata must be <50ms for 1000 tracks (in-memory string copies only). Memory increase from carrying metadata in gRPC messages is bounded: ~200 bytes/track * 1000 tracks = ~200KB additional transfer — negligible for local IPC.
- **Security** (low): No new attack surface — all data flows over the existing gRPC channel between co-located processes.
- **Accessibility** (low): N/A beyond existing terminal UI requirements.
- **Reliability** (high): If metadata fields are missing/empty in the gRPC response (forward compatibility), the TUI must gracefully degrade to showing the filename. The server must not crash if Track metadata is partially populated.
- **Observability** (medium): Log timing of playlist response processing in the TUI (INFO level: "Processed N tracks in Xms"). Existing server-side logging from spec-04 is sufficient for the server path.

## Open Questions

1. **Album field usage in TUI**: The TUI's `playlist_sync()` reads `track.as_track().and_then(|v| v.album())` for the table display. Should the album be a first-class field on all `Track` variants (currently only `MediaTypes::Track` has it), or should it remain track-specific? Recommendation: keep album as track-specific, send it as an optional field in proto.
2. **Backward compatibility**: If a newer TUI connects to an older server that doesn't send metadata, should we fall back to disk reads? Recommendation: Yes, but this is a deferred concern — both binaries are built together from the same repo. Document the assumption that client and server are same version.
3. **Podcast episode metadata**: For `PlaylistTrackSource::PodcastUrl`, the TUI currently queries the podcast SQLite database. Should the server also send podcast episode metadata via gRPC? Recommendation: Yes, include title and duration for podcast episodes to fully eliminate TUI-side database queries during playlist load.
4. **Track file_type field**: The `Track` struct currently stores `file_type` (from metadata parsing). This is used in some TUI logic. Should `file_type` also be sent over gRPC? Recommendation: Defer — `file_type` can be inferred from the file extension in the path without disk I/O.

## Recommendations

1. **Implement Option 1 (Protocol Extension)**: This addresses the root cause rather than working around it. The server already has all display metadata in memory (loaded during spec-04's async startup). Transmitting it over gRPC eliminates 100% of the TUI-side disk reads during playlist load. The protobuf already has a TODO comment requesting exactly this change. The code has a TODO at `playlist.rs:173` saying "refactor to have everything necessary send over grpc instead of having the TUI reading too".
2. **Populate the existing `optional_title` field immediately**: The server already computes titles but sends `None`. This is a quick win that can be done in Phase 1.
3. **Add `artist` and `album` as optional proto fields**: These are the remaining display fields needed by the TUI's `playlist_sync()` table builder. Making them optional ensures backward compatibility.
4. **Construct `Track` from gRPC data without path-based read**: Create a new `Track` constructor (e.g., `Track::from_grpc_metadata(path, title, artist, album, duration)`) that populates the struct without touching the filesystem. This replaces the `Track::read_track_from_path` call path entirely for the playlist loading use case.
5. **Defer Option 2 (background thread) as unnecessary**: If the server sends full metadata, there is nothing for the TUI to load asynchronously. The background-thread approach is a workaround, not a fix. It should only be implemented as a fallback if protocol extension is somehow blocked.

## Assumptions

- The server has all display metadata (title, artist, album, duration) in memory for every track after spec-04 background loading completes — verified by examining `Track` struct fields and `as_grpc_playlist_tracks` implementation.
- The protobuf field numbers in `PlaylistAddTrack` have room for new optional fields (currently uses 1-4, can add 5+).
- The TUI and server are always the same version (built from same repo commit) — no cross-version compatibility concern for v1.
- Adding ~200 bytes per track to gRPC messages is negligible for local inter-process communication (Unix Domain Socket or localhost TCP).
- The `playlist_sync()` table-building is already fast when operating on in-memory data — the bottleneck is exclusively the metadata disk I/O, not the table construction itself.
- The TODO comments in the codebase (`playlist.rs:173`, proto file comments) confirm this is an acknowledged design debt that the maintainers intend to fix.
