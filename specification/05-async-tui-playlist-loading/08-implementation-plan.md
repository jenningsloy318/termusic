# Implementation Plan: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Author**: super-dev:spec-writer
- **Specification**: ./07-specification.md
- **Total Phases**: 4
- **Estimated Effort**: 2 days (medium)

---

## Phase Summary

- Phase 1: Protocol Extension and Domain Struct Updates — Domain: backend, Effort: small, Depends on: None, Parallelizable with: None
- Phase 2: Server-Side Metadata Population — Domain: backend, Effort: small, Depends on: Phase 1, Parallelizable with: None
- Phase 3: TUI Playlist Loading Rewrite — Domain: frontend, Effort: medium, Depends on: Phase 1, Parallelizable with: Phase 2
- Phase 4: Integration Testing and Validation — Domain: testing, Effort: medium, Depends on: Phase 2 and Phase 3, Parallelizable with: None

---

## Phase 1: Protocol Extension and Domain Struct Updates

- **Domain**: backend
- **Effort**: small
- **Objective**: Extend the protobuf schema with artist, album, and has_local_file fields. Create the Track::from_grpc_metadata constructor. Update PlaylistAddTrackInfo domain struct. All changes are additive — existing behavior is unchanged (new fields are unpopulated/None).
- **Depends On**: None
- **Parallelizable With**: None

### Scope

In scope:
- Add `optional string artist = 5`, `optional string album = 6`, `optional bool has_local_file = 7` to `PlaylistAddTrack` proto message
- Create `Track::from_grpc_metadata` constructor in lib/src/track.rs
- Add `artist: Option<String>`, `album: Option<String>`, `has_local_file: bool` fields to `PlaylistAddTrackInfo` in lib/src/player.rs
- Update `From`/`TryFrom` trait implementations for `PlaylistAddTrackInfo` to serialize/deserialize new fields
- Add `TUIPlaylist::insert_track_at` method
- Unit tests for the new constructor and insertion method

Out of scope:
- Server population of new fields (Phase 2)
- TUI consumption of new fields (Phase 3)
- Removing old disk-I/O code paths (Phase 3)

### Tasks

1. Extend PlaylistAddTrack proto message with artist, album, has_local_file fields
   - Files: lib/proto/player.proto
   - Type: modify
2. Add artist, album, has_local_file to PlaylistAddTrackInfo domain struct
   - Files: lib/src/player.rs
   - Type: modify
3. Update From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist to serialize new fields
   - Files: lib/src/player.rs
   - Type: modify
4. Update TryFrom<protobuf::UpdatePlaylist> for UpdatePlaylistEvents to deserialize new fields
   - Files: lib/src/player.rs
   - Type: modify
5. Create Track::from_grpc_metadata constructor
   - Files: lib/src/track.rs
   - Type: modify
6. Add TUIPlaylist::insert_track_at method
   - Files: tui/src/ui/model/playlist.rs
   - Type: modify
7. Unit tests for Track::from_grpc_metadata (all variants) and insert_track_at
   - Files: lib/src/track.rs (inline tests), tui/src/ui/model/playlist.rs (inline tests)
   - Type: modify
8. Update all callers that construct PlaylistAddTrackInfo to populate new fields with defaults
   - Files: playback/src/playlist.rs
   - Type: modify

### Acceptance Criteria

- `cargo build` succeeds with no errors after proto extension
- `Track::from_grpc_metadata` produces correct Track for Path, Url, and PodcastUrl sources (verified by unit tests)
- `Track::from_grpc_metadata` with has_local_file=true produces sentinel PathBuf in PodcastTrackData
- `TUIPlaylist::insert_track_at` inserts correctly at beginning, middle, end, and beyond-length
- `PlaylistAddTrackInfo` serialization round-trips artist, album, has_local_file correctly
- All existing tests continue to pass (no behavioral change)
- Addresses: AC-06 (proto extension with backward compatibility), SCENARIO-014

### Risks

- Proto field number conflicts (mitigated: validated by prototype that fields 5,6,7 are free)

---

## Phase 2: Server-Side Metadata Population

- **Domain**: backend
- **Effort**: small
- **Objective**: Have the server populate title, artist, album, duration, and has_local_file in all gRPC playlist messages (both bulk responses and individual stream events). After this phase, the wire carries full display metadata.
- **Depends On**: Phase 1
- **Parallelizable With**: Phase 3

### Scope

In scope:
- Populate all metadata fields in `as_grpc_playlist_tracks()` (bulk playlist response)
- Populate all metadata fields in individual track addition stream events (send_stream_ev_pl methods: insert_track, add_track_back, add_track_front, etc.)
- Derive title from filename when Track::title() returns None
- Set has_local_file from podcast data (or omit for non-podcast tracks)
- Unit tests for server-side serialization

Out of scope:
- TUI consumption of the populated fields (Phase 3)
- Modifying existing TUI load path (Phase 3)

### Tasks

1. Update as_grpc_playlist_tracks to populate optional_title from Track::title()
   - Files: playback/src/playlist.rs
   - Type: modify
2. Update as_grpc_playlist_tracks to populate artist from Track::artist()
   - Files: playback/src/playlist.rs
   - Type: modify
3. Update as_grpc_playlist_tracks to populate album from Track::as_track().album()
   - Files: playback/src/playlist.rs
   - Type: modify
4. Update as_grpc_playlist_tracks to populate has_local_file from Track::as_podcast().has_localfile()
   - Files: playback/src/playlist.rs
   - Type: modify
5. Add title-from-filename fallback in as_grpc_playlist_tracks when Track::title() is None
   - Files: playback/src/playlist.rs
   - Type: modify
6. Update send_stream_ev_pl track addition events to populate artist, album, has_local_file
   - Files: playback/src/playlist.rs
   - Type: modify
7. Unit tests for server serialization (verify all fields populated for various Track types)
   - Files: playback/src/playlist.rs (inline tests or separate test file)
   - Type: modify

### Acceptance Criteria

- Server `as_grpc_playlist_tracks()` populates title (non-None) for tracks with title metadata (AC-07, SCENARIO-009, SCENARIO-015)
- Server populates filename-derived title when tag-based title is missing (SCENARIO-016)
- Server populates artist and album from Track metadata (AC-03, SCENARIO-006)
- Server populates has_local_file for podcast tracks (SCENARIO-006)
- Server omits has_local_file (None) for non-podcast tracks
- Individual stream events carry full metadata (SCENARIO-008)
- Server does not crash when Track has no metadata (SCENARIO-020)
- Server includes tracks with partial metadata (missing fields are None) (SCENARIO-018)
- All existing tests continue to pass

### Risks

- Server Track objects may have unexpected None values for title/artist (mitigated: fallback to filename for title, None is valid for artist/album)

---

## Phase 3: TUI Playlist Loading Rewrite

- **Domain**: frontend
- **Effort**: medium
- **Objective**: Rewrite the TUI's load_from_grpc and handle_playlist_add to use Track::from_grpc_metadata instead of disk I/O. Remove the db_pod parameter from load_from_grpc. Eliminate all filesystem access from the TUI playlist loading path.
- **Depends On**: Phase 1
- **Parallelizable With**: Phase 2

### Scope

In scope:
- Rewrite Playback::load_from_grpc to use Track::from_grpc_metadata (remove db_pod parameter)
- Update all callers of load_from_grpc to remove db_pod argument
- Rewrite handle_playlist_add to use Track::from_grpc_metadata and insert_track_at
- Update handle_playlist_shuffled to pass through the new load_from_grpc
- Clean up: deprecate or remove track_from_path and track_from_podcasturi methods if no remaining callers
- Remove TODO comments at playlist.rs:173 and playlist.rs:187

Out of scope:
- Server-side metadata population (Phase 2 — but this phase can proceed in parallel because it only needs Phase 1's proto fields to exist, not to be populated; the TUI gracefully handles None fields)
- Performance benchmarking (Phase 4)

### Tasks

1. Rewrite Playback::load_from_grpc to use Track::from_grpc_metadata (no disk I/O)
   - Files: tui/src/ui/model/mod.rs
   - Type: modify
2. Remove db_pod parameter from load_from_grpc signature
   - Files: tui/src/ui/model/mod.rs
   - Type: modify
3. Update all callers of load_from_grpc to remove db_pod argument
   - Files: tui/src/ui/model/update.rs, tui/src/ui/components/playlist.rs
   - Type: modify
4. Rewrite handle_playlist_add to use Track::from_grpc_metadata and insert_track_at
   - Files: tui/src/ui/components/playlist.rs
   - Type: modify
5. Update handle_playlist_shuffled to use the rewritten load_from_grpc
   - Files: tui/src/ui/components/playlist.rs
   - Type: modify
6. Deprecate or remove track_from_path and track_from_podcasturi if no remaining callers
   - Files: tui/src/ui/model/playlist.rs
   - Type: modify
7. Remove resolved TODO comments (playlist.rs:173, playlist.rs:187)
   - Files: tui/src/ui/model/playlist.rs
   - Type: modify
8. Verify all TUI compilation and existing tests pass with rewritten load path
   - Files: (workspace-wide cargo test)
   - Type: verify

### Acceptance Criteria

- load_from_grpc no longer calls Track::read_track_from_path (AC-04, SCENARIO-010, SCENARIO-011)
- load_from_grpc no longer requires db_pod parameter (SCENARIO-010)
- handle_playlist_add uses insert_track_at with pre-built Track (SCENARIO-008)
- Shuffle events processed without disk I/O (AC-05, SCENARIO-012)
- Empty playlist handled without error (SCENARIO-024)
- Tracks with missing metadata display filename fallback (AC-08, SCENARIO-017)
- All existing playlist operations (add, remove, swap, shuffle, clear) continue working (AC-10, SCENARIO-023)
- All existing TUI tests pass
- TODO comments removed

### Risks

- Callers of add_tracks may still exist for non-gRPC paths (e.g., TUI-initiated drag-and-drop or local file addition) — these should retain the disk-I/O path
- Removing db_pod parameter may break compilation in unexpected locations — verify with cargo build before proceeding to each sub-task

---

## Phase 4: Integration Testing and Validation

- **Domain**: testing
- **Effort**: medium
- **Objective**: Comprehensive integration tests verifying the end-to-end data flow from server serialization through TUI deserialization and rendering. Performance benchmarks validating AC-01 (<100ms), AC-02 (<200ms), and AC-09 (<50ms) timing constraints. Regression tests for all existing playlist operations.
- **Depends On**: Phase 2, Phase 3
- **Parallelizable With**: None

### Tasks

1. Integration test: server as_grpc_playlist_tracks output fed to TUI load_from_grpc produces correct Track data
   - Files: tui/src/ui/model/async_tui_loading_tests.rs (create)
   - Type: create
2. Integration test: load_from_grpc with empty playlist (SCENARIO-024)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
3. Integration test: load_from_grpc with all-missing-metadata tracks (SCENARIO-025)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
4. Integration test: handle_playlist_add with full metadata inserts correctly
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
5. Integration test: shuffle event processing without disk I/O (SCENARIO-012, SCENARIO-013)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
6. Performance test: load_from_grpc with 1000-track proto completes under 100ms (SCENARIO-001, AC-01)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
7. Performance test: load_from_grpc with 5000-track proto completes under 100ms (SCENARIO-026)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
8. Performance test: playlist_sync with 1000 in-memory tracks completes under 50ms (SCENARIO-021, AC-09)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
9. Regression test: existing playlist operations (add, remove, swap, shuffle, clear) work with metadata protocol (SCENARIO-023, AC-10)
   - Files: tui/src/ui/model/async_tui_loading_tests.rs
   - Type: modify
10. Integration test: concurrent reload and shuffle resolve to consistent state (SCENARIO-027)
    - Files: tui/src/ui/model/async_tui_loading_tests.rs
    - Type: modify
11. Integration test: track with missing duration handled gracefully (SCENARIO-019)
    - Files: tui/src/ui/model/async_tui_loading_tests.rs
    - Type: modify
12. Verify all workspace tests pass (cargo test --workspace)
    - Files: (workspace-wide)
    - Type: verify

### Acceptance Criteria

- All SCENARIO references (SCENARIO-001 through SCENARIO-028) have at least one test covering them
- Performance tests pass: load_from_grpc < 100ms for 1000 tracks (AC-01)
- Performance tests pass: playlist_sync < 50ms for 1000 tracks (AC-09)
- Performance tests pass: combined load+render < 200ms for 1000 tracks (AC-02)
- All existing workspace tests pass (cargo test --workspace)
- Zero filesystem operations in the TUI playlist loading path (verifiable by absence of Track::read_track_from_path calls)
- All 10 acceptance criteria (AC-01 through AC-10) covered by tests

### Risks

- Performance tests may be flaky on CI due to system load — use generous margins (ceiling is 100ms, expected is sub-1ms)
- Test file creation may conflict with existing test module structure — follow established phase-based test file pattern from spec-04

---

## Cross-Cutting Concerns

### Wire Compatibility

All proto changes are additive (new optional fields with unused field numbers). No existing message behavior changes. Both bulk responses and individual events receive the same field extensions. Wire compatibility is maintained for hypothetical older readers (they ignore unknown fields).

### Error Handling Consistency

All error paths follow the existing pattern: `anyhow::Result` with `.context()` for error chain building, `mount_error_popup` for TUI display. The new `Track::from_grpc_metadata` is infallible (returns Self, not Result) since all inputs are pre-validated by proto deserialization. The `load_from_grpc` method retains its existing error handling for missing track IDs and index conversion failures.

### Dead Code Cleanup

After Phase 3, `TUIPlaylist::track_from_path` and `TUIPlaylist::track_from_podcasturi` may become dead code for the gRPC-driven paths. They should be preserved if any non-gRPC caller exists (e.g., TUI-initiated local file additions), otherwise deprecated/removed. The `add_tracks` method with its `db_pod` parameter should be evaluated for remaining callers.

## Milestone Summary

- **Protocol Foundation**: Phase 1 — Deliverable: Extended proto schema + new constructor + domain struct updates, Verification: cargo build succeeds, unit tests pass, proto compiles with new fields
- **Server Metadata Emission**: Phase 2 — Deliverable: Server sends full display metadata in all playlist messages, Verification: Unit tests confirm all fields populated for various Track types
- **TUI Zero-I/O Loading**: Phase 3 — Deliverable: TUI loads playlist without any filesystem access, Verification: Compilation succeeds, no Track::read_track_from_path in load path, existing tests pass
- **Validated Feature**: Phase 4 — Deliverable: Comprehensive test suite proving correctness and performance, Verification: All integration/performance tests pass, all ACs covered, cargo test --workspace green
