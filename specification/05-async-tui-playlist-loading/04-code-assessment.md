# Code Assessment: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Author**: super-dev:code-assessor
- **Scope**: Full workspace (lib, playback, server, tui) with focus on playlist loading path, gRPC protocol, Track construction, and TUI event handling
- **Focus**: architecture, patterns, dependencies

---

## Executive Summary

The termusic codebase is well-structured as a Rust workspace with clear crate boundaries (lib, playback, server, tui). The primary bottleneck identified by the requirements -- TUI-side synchronous metadata disk I/O during `load_from_grpc` -- is confirmed at `tui/src/ui/model/mod.rs:209-210` and `playback/src/playlist.rs:333`. The server already holds all display metadata in memory but sends only track identifiers via gRPC. The codebase has explicit code annotations acknowledging this design debt. The existing patterns for protobuf serialization, Track construction, and event handling are consistent and well-suited for the proposed protocol extension.

| Dimension | Score (1-5) | Issues |
|-----------|-------------|--------|
| Architecture | 4 | 2 |
| Code Standards | 4 | 1 |
| Dependencies | 5 | 0 |
| Framework Patterns | 4 | 2 |
| Maintainability | 3 | 3 |

Scoring: 5=Excellent, 4=Good, 3=Adequate, 2=Needs Improvement, 1=Critical

---

## Architecture Evaluation

### Organization

The workspace uses a clean 4-crate structure with clear responsibilities:

```
termusic/
  lib/          -> shared types, protobuf definitions, track model, config, podcast DB
  playback/     -> server-side playlist management, audio backends, parallel loading
  server/       -> gRPC service, server binary, async loading orchestration
  tui/          -> terminal UI, TUI playlist model, event handling
```

The protobuf `.proto` file lives in `lib/proto/player.proto` with `build.rs` in lib generating Rust code via `tonic-prost-build`. Both `server/` and `tui/` consume types from `lib/`.

### Module Boundaries

| Module | Responsibility | Coupling | Cohesion |
|--------|---------------|----------|----------|
| `lib/src/track.rs` | Track data model, metadata parsing, MediaTypes enum | Low | High |
| `lib/src/player.rs` | Protobuf bindings, event types, proto-to-domain conversions | Medium | High |
| `playback/src/playlist.rs` | Server playlist state, track operations, gRPC serialization | Medium | Medium |
| `tui/src/ui/model/playlist.rs` | TUI playlist state (Vec<Track>), add/remove/swap operations | Low | High |
| `tui/src/ui/components/playlist.rs` | Playlist view rendering (table), user interaction handlers | Medium | Medium |
| `tui/src/ui/model/update.rs` | TUI event dispatch, response handling | High | Low |
| `server/src/music_player_service.rs` | gRPC endpoint implementations | Low | High |

### Data Flow

```
[Server Playlist (playback crate)]
     |
     | as_grpc_playlist_tracks() -- serializes Track -> PlaylistAddTrack (proto)
     | PROBLEM: sends optional_title: None, no artist/album fields
     v
[gRPC Wire (protobuf PlaylistTracks)]
     |
     | get_playlist() RPC / PlaylistShuffled stream event
     v
[TUI Playback::load_from_grpc (tui/src/ui/model/mod.rs:187)]
     |
     | For each track: Track::read_track_from_path(path) -- DISK I/O BOTTLENECK
     v
[TUIPlaylist.tracks: Vec<Track>]
     |
     | playlist_sync() -- builds TableBuilder from in-memory Track data
     v
[Rendered Playlist Table (tuirealm Table component)]
```

### Error Handling Consistency

Error handling follows a consistent pattern across the codebase:
- Functions return `anyhow::Result<T>` for fallible operations
- The TUI uses `mount_error_popup(err.context("description"))` to display errors to users
- gRPC conversions use `unwrap_msg()` helper for consistent Option-to-Result conversion
- The `bail!()` macro is used for early-return error conditions
- Track metadata parse failures are non-fatal: logged at debug level, resulting in `TrackMetadata::default()` (see `lib/src/track.rs:260-269`)

### Findings

**ARCH-001** | Severity: High | Location: `tui/src/ui/model/mod.rs:209-210`

- **Issue**: `Playback::load_from_grpc` calls `Track::read_track_from_path(v)` for every local file track, performing synchronous disk I/O on the TUI main thread. This blocks the event loop for 2-5 seconds with 200+ tracks.
- **Impact**: Complete UI freeze during playlist load. User cannot interact with the application. This is the primary target of spec-05.
- **Recommendation**: Replace with `Track::from_grpc_metadata(...)` constructor that builds Track from server-provided metadata fields without disk I/O. Requires protocol extension (add artist/album to proto).

**ARCH-002** | Severity: Medium | Location: `playback/src/playlist.rs:1039-1044`

- **Issue**: `as_grpc_playlist_tracks()` sets `optional_title: None` despite the server Track objects having title populated. The server discards metadata it already has when serializing for gRPC.
- **Impact**: Forces the TUI to re-read metadata from disk (the root cause of ARCH-001). The server has `track.title()`, `track.artist()`, `track.duration()` all in memory but does not serialize them.
- **Recommendation**: Populate `optional_title` with `track.title()`, and add artist/album fields to the proto message.

---

## Code Standards

### Tooling Inventory

| Tool | Config File | Status |
|------|-------------|--------|
| Clippy (linter) | `Cargo.toml` workspace.lints.clippy, `clippy.toml` | Active (pedantic + all + correctness) |
| rustfmt (formatter) | None found (uses defaults) | Active (default config) |
| Rust edition 2024 | `Cargo.toml` workspace.package.edition | Active |
| unsafe_code = "deny" | `Cargo.toml` workspace.lints.rust | Active |

### Conventions Observed

- **Naming**: snake_case for functions/fields, PascalCase for types/enums, SCREAMING_SNAKE for constants. Example: `PlaylistAddTrackInfo` (type) at `lib/src/player.rs:336`, `current_track_index` (field) at `playback/src/playlist.rs:42`.
- **File Organization**: One module per file, test modules either inline (`#[cfg(test)] mod tests`) or separate `*_tests.rs` files included via `#[cfg(test)] mod`. Phase-based test files (e.g., `async_loading_phase4_tests.rs`) for integration tests.
- **Import Ordering**: std library first, then external crates, then local crate imports. Grouped with blank lines between categories. Example at `tui/src/ui/components/playlist.rs:1-46`.
- **Comment Style**: `///` doc comments on public API, `//` for inline explanations. `// NOTE:` prefixes for annotations and `// <verb>:` markers for acknowledged design debt. Protobuf uses `//` comments.
- **Error Pattern**: `anyhow::Result` for general errors, `bail!()` for early returns, `.context()` for error chain building.

### Findings

**STD-001** | Severity: Low | Location: `tui/src/ui/model/playlist.rs:173,187`

- **Issue**: Code annotations explicitly acknowledge design debt: "refactor to have everything necessary send over grpc instead of having the TUI reading too" and "refactor to have everything necessary send over grpc instead of having the TUI access to the database". These had been present for an extended period.
- **Impact**: Design debt remains unfixed, causing the performance issue this spec targets.
- **Recommendation**: Resolve these code annotations as part of spec-05 implementation. Remove the annotations once the protocol is extended.

---

## Dependencies

### Manifest Analysis

| Package | Current | Latest | Status | Health |
|---------|---------|--------|--------|--------|
| prost | 0.14.4 | 0.14.4 | Current | Healthy |
| tonic | 0.14.6 | 0.14.6 | Current | Healthy |
| lofty | 0.24.0 | 0.24.0 | Current | Healthy |
| tokio | 1.52 | 1.52 | Current | Healthy |
| anyhow | 1.0.102 | 1.0.102 | Current | Healthy |
| parking_lot | 0.12.5 | 0.12.5 | Current | Healthy |
| rayon | 1.12 | 1.12 | Current | Healthy |
| tuirealm | 4.1.0 | 4.1.0 | Current | Warning (niche) |
| rusqlite | 0.39 | 0.39 | Current | Healthy |

### Security Advisories

None found. All key dependencies (tonic, prost, tokio, lofty) are at current stable versions.

### Bundle/Binary Size Concerns

No concerns relevant to this feature. The protocol extension adds minimal wire overhead (~200 bytes/track for artist/album strings).

### Findings

No dependency findings. The dependency stack is appropriate and well-maintained for this feature's needs. The prost/tonic combination is the standard Rust gRPC stack with active maintenance.

---

## Framework Patterns

### Patterns Inventory

| Pattern | Usage | Location | Assessment |
|---------|-------|----------|------------|
| gRPC Streaming (server->TUI) | tonic + BroadcastStream | `server/src/music_player_service.rs:27`, `tui/src/ui/model/ports/stream_events.rs:17` | Appropriate |
| TUI Event Loop | tuirealm async ports | `tui/src/ui/model/ports/stream_events.rs:34-69` | Appropriate |
| Command Pattern (TUI->Server) | ServerRequestActor with mpsc channel | `tui/src/ui/server_req_actor.rs:12-180` | Appropriate |
| Protobuf Serialization | prost + tonic-prost-build | `lib/build.rs:1-4`, `lib/src/player.rs:6-8` | Appropriate |
| Server Playlist (shared state) | Arc<RwLock<Playlist>> (parking_lot) | `playback/src/playlist.rs:37`, `server/src/music_player_service.rs:28` | Appropriate |
| TUI Playlist (local state) | Plain struct with Vec<Track> | `tui/src/ui/model/playlist.rs:14-20` | Appropriate |

### Test Structure

- **Framework**: Standard Rust `#[test]` and `#[tokio::test]` attributes
- **Organization**: Phase-based test files (`async_loading_phase1_tests.rs` through `phase4_tests.rs`) for feature integration tests, inline `#[cfg(test)] mod tests` for unit tests
- **Coverage**: ~316 test annotations across the workspace; comprehensive phase-based testing established by spec-04
- **Pattern**: Tests use `tempfile` for filesystem fixtures, `pretty_assertions` for comparison output, in-module test helpers. BDD-style naming with scenario numbering.

### Findings

**PAT-001** | Severity: High | Location: `tui/src/ui/components/playlist.rs:448-461`

- **Issue**: `handle_playlist_add` currently re-reads track metadata from disk for individual track additions received via stream events. It calls `self.playback.playlist.add_tracks(...)` which invokes `track_from_path()` -> `Track::read_track_from_path(path)`. This same disk I/O pattern exists for both bulk load and individual track events.
- **Impact**: Even after fixing `load_from_grpc`, individual `PlaylistAddTrack` stream events will still perform disk I/O unless also updated. The `PlaylistAddTrackInfo` struct already has a `title` field but not artist/album.
- **Recommendation**: Extend `PlaylistAddTrackInfo` with `artist: Option<String>` and `album: Option<String>`. Create a `Track::from_grpc_metadata()` constructor and use it in `handle_playlist_add` instead of re-reading from disk.

**PAT-002** | Severity: Medium | Location: `playback/src/playlist.rs:314-349` and `tui/src/ui/model/mod.rs:187-232`

- **Issue**: `load_from_grpc` is duplicated between the `playback` crate (server-side, used during `start_background_playlist_load`) and the `tui` crate (TUI-side, used for rendering). Both contain nearly identical logic for converting proto data to Track objects. The TUI version calls `Track::read_track_from_path`; the server version also calls it.
- **Impact**: Violates DRY; any protocol changes must be applied in two places. However, the duplication is necessary because the server version mutates `self.tracks` + `self.current_track_index` + `self.is_modified` (server fields), while the TUI version calls `self.playlist.set_tracks(...)`.
- **Recommendation**: When implementing the protocol extension, only the TUI's `load_from_grpc` needs to change (to use metadata from proto instead of disk). Document that the server's `load_from_grpc` remains for backward-compatibility scenarios only. Consider marking the server's version with a note that it is only used for loading from the playlist.log file (not from gRPC responses the server receives).

---

## Pattern Library (Canonical Patterns)

### Pattern 1: Proto-to-Domain Conversion via From/TryFrom

- **Canonical example**: `lib/src/player.rs:392-430` -- `From<UpdatePlaylistEvents> for protobuf::UpdatePlaylist`
- **Consistency score**: 95% -- All protobuf-to-domain conversions use `From`/`TryFrom` traits consistently
- **Violations**: None found
- **Notes for new code**: New proto fields MUST have corresponding `From`/`TryFrom` implementations in `lib/src/player.rs`. The `unwrap_msg()` helper at line 481 should be used for optional proto fields.

### Pattern 2: Track Construction (Factory Methods)

- **Canonical example**: `lib/src/track.rs:202-224` -- `Track::from_podcast_episode(&Episode) -> Self`
- **Consistency score**: 100% -- All Track creation uses named constructors (`from_podcast_episode`, `new_radio`, `read_track_from_path`)
- **Violations**: None
- **Notes for new code**: A new `Track::from_grpc_metadata(...)` constructor MUST follow this same pattern -- named constructor, documented with `///` doc comment, takes typed parameters, returns `Self` (not `Result` since all data is pre-validated).

### Pattern 3: TUI Event Handling (Stream -> Model mutation -> View sync)

- **Canonical example**: `tui/src/ui/components/playlist.rs:509-534` -- `handle_playlist_shuffled`
- **Consistency score**: 90% -- All playlist event handlers follow: receive info struct -> mutate `self.playback.playlist` -> call `self.playlist_sync()`
- **Violations**: None significant; all handlers follow the pattern
- **Notes for new code**: New/modified playlist handlers MUST call `self.playlist_sync()` after mutation. If the change affects the current track, also call `self.handle_current_track_index(...)`.

### Pattern 4: Error Propagation (Context + Mount Popup)

- **Canonical example**: `tui/src/ui/model/update.rs:1129-1134` -- `self.playback.load_from_grpc(...).mount_error_popup(err)`
- **Consistency score**: 95% -- Errors in the TUI update loop are caught with `if let Err(err) = ...` and displayed via `mount_error_popup`
- **Violations**: None relevant
- **Notes for new code**: Fallible operations in event handlers MUST use this pattern. Do NOT silently drop errors.

### Pattern 5: Proto Field Extension (Backward Compatibility)

- **Canonical example**: `lib/proto/player.proto:228-247` -- `PlaylistAddTrack` message with `oneof optional_title` for backward compat
- **Consistency score**: 100% -- All optional proto fields use `oneof` wrapper or proto3 `optional`
- **Violations**: None
- **Notes for new code**: New fields MUST use sequential field numbers (5, 6, 7...) after existing fields. Use `optional` keyword for new string fields (proto3 syntax). Do NOT reuse or renumber existing fields.

---

## Architecture Smell Detection

### Duplicated Responsibility (Medium Severity)

- **Location**: `playback/src/playlist.rs:314-349` (server) and `tui/src/ui/model/mod.rs:187-232` (TUI)
- **Smell**: Both contain `load_from_grpc` with near-identical iteration logic
- **Blast radius**: 2 files. Low blast radius since both serve different crates with different struct contexts.
- **Assessment**: Intentional duplication due to different ownership semantics (server Playlist vs TUI Playback). Not a blocking smell for this feature.

### Data Clumps (Low Severity)

- **Location**: `PlaylistAddTrackInfo` fields (`at_index`, `title`, `duration`, `trackid`) at `lib/src/player.rs:336-344` repeat a subset of Track fields
- **Blast radius**: 3 files (lib, playback, tui)
- **Assessment**: This is the natural consequence of having a separate "info" struct for event transport. Adding `artist` and `album` here extends the clump but follows the existing pattern.

---

## Better Options Analysis

| Current Approach | Better Option | Benefit | Migration Effort |
|-----------------|---------------|---------|-----------------|
| TUI reads metadata from disk per track | Server sends full display metadata via gRPC | Eliminates 100% of TUI disk I/O; renders immediately | M |
| `optional_title: None` in as_grpc_playlist_tracks | Populate title from Track::title() | Quick win: titles display without disk I/O | S |
| No artist/album in proto | Add optional string artist/album fields | Complete display metadata without disk I/O | S |
| TUI db_podcast query for podcast track metadata | Server sends podcast metadata via gRPC | Eliminates TUI-side database dependency for load | M |

---

## Technical Debt Inventory

| ID | Description | Location | Severity | Effort | Blast Radius | Priority |
|----|-------------|----------|----------|--------|--------------|----------|
| TD-001 | Server discards title when building gRPC response (sets None) | `playback/src/playlist.rs:1043` | High | S (1h) | 3 files (playback, lib proto, tui) | Now |
| TD-002 | Proto lacks artist/album fields needed for TUI display | `lib/proto/player.proto:228-247` | High | S (2h) | 4 files (proto, player.rs, playback, tui) | Now |
| TD-003 | TUI `load_from_grpc` does synchronous disk I/O on main thread | `tui/src/ui/model/mod.rs:209-210` | Critical | M (4h) | 2 files (tui model, tui playlist) | Now |
| TD-004 | TUI `handle_playlist_add` does disk I/O for individual track events | `tui/src/ui/components/playlist.rs:448-461` | High | M (3h) | 3 files (player.rs, playback, tui) | Now |
| TD-005 | TUI podcast track construction queries database instead of using gRPC data | `tui/src/ui/model/playlist.rs:186-191` | Medium | M (3h) | 2 files (tui playlist, proto) | Soon |
| TD-006 | Duplicated `load_from_grpc` between playback and TUI crates | `playback/src/playlist.rs:314` and `tui/src/ui/model/mod.rs:187` | Low | M (2h) | 2 files | Eventually |
| TD-007 | `playlist_sync()` rebuilds entire table on every change (no partial updates) | `tui/src/ui/components/playlist.rs:620-690` | Low | L (8h) | 1 file (tui components) | Never |

---

## Prioritized Recommendations

| Priority | ID | Recommendation | Effort | Impact |
|----------|-----|---------------|--------|--------|
| 1 | REC-001 | Extend `PlaylistAddTrack` proto with `optional string artist = 5` and `optional string album = 6` fields. Update `as_grpc_playlist_tracks()` to populate title, artist, album from server Track objects. | S | L |
| 2 | REC-002 | Create `Track::from_grpc_metadata(source, title, artist, album, duration)` constructor in `lib/src/track.rs` that builds a Track without disk I/O. | S | L |
| 3 | REC-003 | Rewrite TUI `Playback::load_from_grpc` to use `Track::from_grpc_metadata()` instead of `Track::read_track_from_path()`. Eliminate all disk I/O from the TUI playlist loading path. | M | L |
| 4 | REC-004 | Add `artist: Option<String>` and `album: Option<String>` to `PlaylistAddTrackInfo` struct. Update server stream event emission to populate these from Track metadata. Update TUI `handle_playlist_add` to use the new constructor. | M | L |
| 5 | REC-005 | Add `optional bool has_local_file = 7` to proto for podcast download indicator. Populate from server. Use sentinel PathBuf in TUI-side Track construction. | S | S |
| 6 | REC-006 | Add `insert_track_at(index: usize, track: Track)` method on `TUIPlaylist` for clean pre-built Track insertion (replacing the `add_tracks` path that does disk I/O). | S | M |

Priority ordering: High Impact + Low Effort first (REC-001, REC-002), then High Impact + Medium Effort (REC-003, REC-004), then Low Impact + Low Effort (REC-005, REC-006).

---

## File Coverage Report

| Category | Files Analyzed | Total Files | Coverage |
|----------|---------------|-------------|----------|
| lib/src (core types, proto, track) | 8 | 62 | 13% |
| playback/src (playlist, parallel_load) | 3 | 22 | 14% |
| server/src (music_player_service) | 2 | 17 | 12% |
| tui/src (model, playlist, update, server_req_actor) | 8 | 66 | 12% |
| Proto files | 1 | 1 | 100% |
| Config files (Cargo.toml, clippy.toml) | 6 | 6 | 100% |
| **Total (scope-relevant)** | **28** | **167** | **17%** |

### Exclusions

- `lib/src/songtag/`: Song tag search providers, not relevant to playlist loading protocol
- `lib/src/new_database/`: Music library database, not relevant to gRPC protocol changes
- `lib/src/podcast/db/`: Podcast database internals (only the interface via `db_podcast.get_episode_by_url` was relevant)
- `lib/src/config/`: Configuration system, not relevant to this feature
- `playback/src/backends/`: Audio playback backends, not relevant to playlist metadata transfer
- `tui/src/ui/components/config_editor/`: Config editor UI, not relevant
- `tui/src/ui/components/tag_editor/`: Tag editor UI, not relevant
- `tui/src/ui/components/orx_music_library/`: Music library browser, not relevant
- `server/src/*_tests.rs`: Test files for prior specs, reviewed for test pattern only (not content)

### Coverage Justification

The 17% file coverage reflects targeted analysis of the specific code paths affected by this feature: protobuf schema, Track construction, gRPC serialization/deserialization, TUI playlist loading, and TUI event handling. Files outside the playlist-loading data flow were excluded as they are unaffected by the proposed changes.
