# Deep Research Report: Async TUI Playlist Loading (Iteration 3)

- **Date**: 2026-06-27
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-26 to 2026-06-27
- **Technologies**: Rust, prost 0.14.4, tonic 0.14.6, protobuf3, lofty 0.24.0, tokio 1.52
- **Freshness**: Fresh (< 6mo)

---

## Executive Summary

- ISS-006 (insert_track_at event loop concern) is **resolved**: The TUI's `TUIPlaylist` struct is a pure data container (`Vec<Track>`, `Option<usize>`, `LoopMode`) with NO `stream_tx` field and NO event emission capability. Inserting a track at any index cannot trigger an event loop because there is no event emission mechanism in the TUI playlist (SRC-025, SRC-026).
- ISS-007 (has_localfile podcast indicator deferral) is **resolved**: The current codebase already exhibits the same limitation — after a podcast download completes, the `[D]` indicator does NOT update on existing Track objects in the playlist until a full reload. The `episode_update_playlist()` method only calls `playlist_sync()` (re-renders table) without updating the Track objects. Deferring `has_localfile` from gRPC introduces no regression from current behavior (SRC-027, SRC-028).
- **Recommendation** (High confidence): For ISS-006, add an `insert_track_at(index: usize, track: Track)` method on `TUIPlaylist` that performs a direct `Vec::insert` — it is safe with zero risk of event loops. For ISS-007, omit `has_localfile` from the gRPC protocol; instead, populate it in the `Track::from_grpc_metadata` constructor by checking `PodcastTrackSource::localfile` from the server's Track data, transmitted as an optional field.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| TUIPlaylist struct fields and event emission patterns | DeepWiki (tramhao/termusic) | 1 | 1 |
| podcast has_localfile indicator TUI playlist display | DeepWiki (tramhao/termusic) | 1 | 1 |
| PCMsg::DLComplete podcast download handling code path | DeepWiki (tramhao/termusic) | 1 | 1 |
| TUIPlaylist struct definition (tui/src/ui/model/playlist.rs) | Codebase read | 1 | 1 |
| handle_playlist_add calling add_tracks (tui/src/ui/components/playlist.rs:448-461) | Codebase read | 1 | 1 |
| episode_download_complete and episode_update_playlist code paths | Codebase grep + read | 3 | 2 |
| playlist_sync_podcasts has_localfile usage | Codebase read | 1 | 1 |
| stream_tx references in TUI crate | Codebase grep | 0 | 0 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-025 | termusic codebase: `tui/src/ui/model/playlist.rs:14-20` — TUIPlaylist struct definition with only `tracks: Vec<Track>`, `current_track_idx: Option<usize>`, `loop_mode: LoopMode` | Codebase | 2026-06 | Fresh | High |
| SRC-026 | termusic codebase: `tui/src/ui/model/mod.rs:106-108` — Playback struct confirms `pub playlist: playlist::TUIPlaylist` (not server Playlist) | Codebase | 2026-06 | Fresh | High |
| SRC-027 | termusic codebase: `tui/src/ui/components/podcast.rs:774-777` — `episode_update_playlist()` only calls `self.playlist_sync()` without updating Track objects | Codebase | 2026-06 | Fresh | High |
| SRC-028 | termusic codebase: `tui/src/ui/components/playlist.rs:585-588` — `playlist_sync_podcasts` reads `track.as_podcast().is_some_and(PodcastTrackData::has_localfile)` from existing Track objects | Codebase | 2026-06 | Fresh | High |
| SRC-029 | termusic codebase: `tui/src/ui/components/podcast.rs:714-730` — `episode_download_complete` updates DB and refreshes `self.podcast.podcasts` but NOT `self.playback.playlist.tracks` | Codebase | 2026-06 | Fresh | High |
| SRC-030 | termusic codebase: `playback/src/playlist.rs:51,59` — Server-side Playlist has `stream_tx: StreamTX` field; TUI's TUIPlaylist has NO such field | Codebase | 2026-06 | Fresh | High |
| SRC-031 | termusic codebase: `lib/src/track.rs:39-89` — PodcastTrackData struct with `localfile: Option<PathBuf>` and `has_localfile()` method | Codebase | 2026-06 | Fresh | High |
| SRC-032 | termusic codebase: `lib/src/track.rs:199-224` — `Track::from_podcast_episode` sets localfile from `ep.path.take_if(|v| v.exists())` | Codebase | 2026-06 | Fresh | High |
| SRC-033 | DeepWiki: tramhao/termusic — TUI playlist is NOT the server's Playlist; handle_playlist_add processes events received FROM server, not sent TO server | AI Documentation | 2026-06 | Fresh | Medium |

---

## Per-Issue Deep Dive

### ISS-006: TUI handle_playlist_add Needs insert_track_at Without Event Loop Risk

**Prior Understanding**: The `handle_playlist_add` refactoring will construct a Track from gRPC metadata and insert it at a specific index in the TUI playlist. The concern was whether this could trigger an infinite event loop if the TUI playlist has an event emission mechanism like the server's `stream_tx`.

**Investigation Summary**: Examined the `TUIPlaylist` struct definition, verified absence of `stream_tx` in the TUI crate, traced the `handle_playlist_add` -> `add_tracks` code path, and confirmed the architectural separation between TUI and server playlist types.

**Resolution Status**: RESOLVED (no risk)

**Evidence**:

1. **TUIPlaylist has NO event emission capability** (SRC-025): The struct is defined as:
```rust
pub struct TUIPlaylist {
    tracks: Vec<Track>,
    current_track_idx: Option<usize>,
    loop_mode: LoopMode,
}
```
There is no `stream_tx`, no broadcast channel, no event sender of any kind.

2. **TUIPlaylist is a separate type from the server Playlist** (SRC-026, SRC-030): The TUI's `Playback` struct at `tui/src/ui/model/mod.rs:106-108` uses `pub playlist: playlist::TUIPlaylist`, while the server's playlist at `playback/src/playlist.rs:51` has `stream_tx: StreamTX`. These are completely different types in different crates.

3. **No stream_tx references in entire TUI crate** (SRC-025): `grep -rn "stream_tx" tui/` returns zero results. The TUI crate never imports or uses any event broadcast mechanism for its playlist data.

4. **Event flow is unidirectional** (SRC-033): Stream events flow FROM server TO TUI. When the TUI receives a `PlaylistAddTrack` event, it calls `handle_playlist_add` to update its local state. The TUI never sends events back in response to local state mutations.

5. **The current `TUIPlaylist::add_tracks` method** (SRC-025, line 123-154) already performs direct `Vec::push` and `Vec::insert` operations with no side effects beyond modifying the local `tracks` vector.

**Resolution Path**:

Add a simple `insert_track_at` method on `TUIPlaylist`:

```rust
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

This is safe because:
- `TUIPlaylist` has no event emission (SRC-025)
- `Vec::insert` is a pure data operation with no callbacks
- The existing `add_tracks` method already does the same thing but with filesystem access mixed in

**Risk Assessment**: Zero. There is no mechanism by which a `Vec::insert` on the TUI's local data could propagate back to the server or re-enter the event handling loop.

---

### ISS-007: has_localfile Podcast Indicator Deferral Validation

**Prior Understanding**: The prior report recommended deferring `has_localfile` from the gRPC protocol. The concern was that this could cause visual inconsistency when switching to Podcast layout if the indicator is not populated via gRPC.

**Investigation Summary**: Traced the complete `has_localfile` lifecycle: how it is set during Track construction, how it is used in display, and what happens after a download completes. Discovered that the current behavior is already limited in the same way.

**Resolution Status**: RESOLVED (no regression from current behavior)

**Evidence**:

1. **Current behavior already has the same limitation** (SRC-027, SRC-029): When a podcast download completes, `episode_download_complete()` updates the database and calls `episode_update_playlist()`, but `episode_update_playlist()` only calls `self.playlist_sync()` which re-renders the table from EXISTING Track objects. It does NOT update the Track objects' `localfile` field.

2. **Track objects are immutable once loaded** (SRC-028): The `playlist_sync_podcasts()` function reads `track.as_podcast().is_some_and(PodcastTrackData::has_localfile)` from the Track objects in `self.playback.playlist.tracks()`. These Track objects retain whatever `localfile` value they had when initially constructed during `load_from_grpc`.

3. **The `[D]` indicator reflects load-time state, not real-time state** (SRC-032): `Track::from_podcast_episode` sets localfile based on `ep.path.as_ref().take_if(|v| v.exists())` — it only reflects whether the file existed at the moment the Track was constructed, not the current filesystem state.

4. **No code in TUI updates localfile on existing Tracks** (SRC-028): `grep -rn "localfile\|set_localfile" tui/` only finds the one read-only access in `playlist_sync_podcasts`. There is no setter or mutation of the localfile field after Track construction.

5. **Full playlist reload is required to refresh download status**: The only way the `[D]` indicator updates is through a full `load_from_grpc` call (e.g., on shuffle event or explicit reload), which re-constructs all Track objects.

**Conclusion**: Deferring `has_localfile` from the gRPC protocol causes ZERO regression from current behavior because:
- Currently: Track is loaded via `load_from_grpc` -> `track_from_podcasturi` -> `from_podcast_episode` which checks `path.exists()` at load time
- With new approach: Track is loaded via `Track::from_grpc_metadata` which would need `has_localfile` from the server to show the `[D]` indicator at load time

The key insight is that `has_localfile` data IS available on the server's Track objects (the server's podcast Tracks have `localfile` populated). The question is whether to transmit it.

**Resolution Path — Three sub-options for ISS-007**:

**Sub-option 7A (Recommended)**: Include `has_localfile` as `optional bool has_local_file = 7` in proto. The server populates it from `track.as_podcast().map_or(false, |p| p.has_localfile())`. The TUI's `Track::from_grpc_metadata` constructor uses it to set `PodcastTrackData.localfile` to a sentinel (`Some(PathBuf::new())`) when true, or `None` when false. This preserves the `[D]` indicator behavior at zero additional I/O cost.

**Sub-option 7B**: Omit from proto entirely. The `[D]` indicator disappears from the podcast layout until a separate on-demand query populates it. This is technically a regression from current behavior (where it IS shown on initial load).

**Sub-option 7C**: Include as a string field (`optional string local_file_path = 7`) to transmit the actual path. This enables the TUI to check file existence if needed, but adds unnecessary data for a mere boolean indicator.

**Recommendation**: Sub-option 7A. Adding a single boolean field to the proto is trivial (1 byte on wire), preserves existing behavior perfectly, and requires no additional queries. The server already has this data in memory.

---

## Options Comparison

REQUIRED: Compare 3-5 viable options for the `insert_track_at` + `has_localfile` design integration.

| Criterion | Option A: Simple insert_track_at + bool proto field | Option B: Refactor add_tracks to accept pre-built Track + defer has_localfile | Option C: New method + sentinel localfile path | Option D: New method + on-demand DB query for podcast indicator |
|-----------|------------------------------------------------------|-----------------------------------------------------------------------------|-----------------------------------------------|---------------------------------------------------------------|
| Maturity | 5 | 4 | 4 | 3 |
| Community/Support | 5 | 4 | 4 | 3 |
| Performance | 5 | 4 | 5 | 3 |
| Bundle Size / Footprint | 5 | 5 | 4 | 4 |
| Learning Curve | 5 | 4 | 3 | 3 |
| Maintenance Burden | 5 | 4 | 3 | 2 |
| Project Fit | 5 | 4 | 4 | 3 |
| Innovation/Momentum | 4 | 4 | 3 | 3 |
| **TOTAL** | **39** | **33** | **30** | **24** |

### Option A: Simple insert_track_at + bool proto field (Recommended)

Add a minimal `insert_track_at(index, track)` method on `TUIPlaylist` for direct insertion. Add `optional bool has_local_file = 7` to the proto. Server populates it; TUI uses it to set sentinel localfile on `PodcastTrackData`.

- **Strengths**: Simplest possible implementation — one new method (3 lines of logic) for insertion (SRC-025). Single boolean proto field for podcast indicator (SRC-031). No refactoring of existing methods needed. Preserves current `[D]` indicator behavior exactly (SRC-028). Zero disk I/O. The sentinel pattern (`localfile = Some(PathBuf::new())`) works because `has_localfile()` only checks `is_some()` (SRC-031). Wire cost: 1 byte per podcast track, 0 bytes for non-podcast tracks (proto optional semantics).
- **Weaknesses**: Creates a parallel insertion path (`insert_track_at` vs `add_tracks`). The sentinel empty PathBuf is semantically impure (it means "exists but path unknown"). If future code calls `localfile()` expecting a real path, it gets an empty PathBuf rather than None.
- **Best For**: This project — minimal change with full behavioral preservation.

### Option B: Refactor add_tracks to Accept Pre-Built Track + Defer has_localfile

Modify `TUIPlaylist::add_tracks` to accept an enum: either a `PlaylistTrackSource` (old behavior, reads from disk) or a pre-constructed `Track` (new behavior). Defer `has_localfile` — the `[D]` indicator only appears after a full playlist reload.

- **Strengths**: Single method handles both paths (SRC-025). No new proto field needed for podcast indicator. Clean enum dispatch pattern.
- **Weaknesses**: More invasive refactoring of `add_tracks` (SRC-025, line 123-154). Regression: `[D]` indicator disappears on initial load until full reload, which is a behavioral change from current code where it IS shown (SRC-028, SRC-032). The enum adds complexity for callers. Still needs `db_pod` parameter for the legacy code path.
- **Best For**: Projects prioritizing minimal proto changes over display fidelity.

### Option C: New insert Method + Sentinel Localfile Path String from Proto

Add `insert_track_at` on `TUIPlaylist`. Instead of a bool, add `optional string local_file_path = 7` to the proto, transmitting the actual path string. Use it directly in `PodcastTrackData.localfile`.

- **Strengths**: Full fidelity — the TUI has the actual download path, not just a boolean (SRC-031). Future code that calls `localfile()` gets the real path. No sentinel needed.
- **Weaknesses**: Transmits unnecessary data (full path string, ~50-200 bytes per podcast track) when only a boolean check is needed (SRC-028). Exposes filesystem paths over the protocol, which is conceptually leaky if remote TUI scenarios ever materialize. Requires `PathBuf::from(path_string)` conversion on TUI side. The path might be stale if the file is deleted between server load and TUI display.
- **Best For**: Future scenarios where the TUI needs the actual podcast download file path for playback without server mediation.

### Option D: New insert Method + On-Demand DB Query for Podcast Indicator

Add `insert_track_at` on `TUIPlaylist`. Do NOT include `has_localfile` in proto. When the TUI enters Podcast layout, query the podcast database for episode download status and update Track objects accordingly.

- **Strengths**: Clean protocol — no podcast-specific fields in generic track message. Database is the source of truth for download status.
- **Weaknesses**: Requires database access during layout switch — reintroduces the I/O we are trying to eliminate (SRC-028). The TUI already has a reference to `db_podcast` but using it on layout switch adds latency. Requires mutating existing Track objects in the playlist or rebuilding them — complex and error-prone. Current code does NOT do this (SRC-027), so it is new functionality rather than preserving existing behavior.
- **Best For**: Scenarios where download status changes frequently and must always be current (not the case here — downloads are infrequent).

---

## Deprecation Warnings

No deprecation concerns identified for current stack.

- prost 0.14.4 and tonic 0.14.6 are current stable releases.
- lofty 0.24.0 is the current stable release.
- The `Vec::insert` API is stable Rust standard library — no deprecation risk.

---

## Best Practices

### BP-008: Use Direct Vec Operations for Event-Free Data Insertion

- **Pattern**: When a struct is purely a data container without event emission, use direct `Vec::push` / `Vec::insert` operations rather than routing through methods that have side effects (disk I/O, event dispatch).
- **Rationale**: The `TUIPlaylist` struct is a pure data container with no side effects. The existing `add_tracks` method conflates two concerns: (1) constructing a Track from a source identifier (with disk I/O) and (2) inserting it into the Vec. Separating these concerns allows the caller to provide a pre-constructed Track directly (SRC-025, SRC-026).
- **Source**: SRC-025, SRC-026
- **Confidence**: High
- **Example**:
```rust
/// Insert a pre-constructed Track at a specific index.
pub fn insert_track_at(&mut self, index: usize, track: Track) {
    if index >= self.tracks.len() {
        self.tracks.push(track);
    } else {
        self.tracks.insert(index, track);
    }
}
```

### BP-009: Use Boolean Proto Fields for Status Indicators Rather Than Full Data

- **Pattern**: When the consumer only needs a boolean check (exists / does not exist), transmit a boolean over the protocol rather than the full underlying data (e.g., a file path).
- **Rationale**: The `has_localfile()` method only checks `self.localfile.is_some()` (SRC-031). Transmitting the full file path is wasteful and leaky. A boolean field costs 1 byte on wire and provides exactly the information needed for the `[D]` display indicator (SRC-028).
- **Source**: SRC-028, SRC-031
- **Confidence**: High
- **Example**:
```protobuf
message PlaylistAddTrack {
  // ... existing fields 1-6 ...
  optional bool has_local_file = 7;  // podcast download indicator
}
```

### BP-010: Sentinel Values for Boolean-Only Checks on Path Fields

- **Pattern**: When a struct field is `Option<PathBuf>` but downstream code only calls `.is_some()` on it, use a sentinel value (empty `PathBuf`) to indicate "present but path unknown" when constructing from a protocol that only transmits presence.
- **Rationale**: The `PodcastTrackData.has_localfile()` method returns `self.localfile.is_some()`. When constructing from gRPC with only a boolean, setting `localfile = Some(PathBuf::new())` satisfies the check without requiring the actual path. This avoids refactoring the existing `PodcastTrackData` struct to add a separate boolean field (SRC-031, SRC-032).
- **Source**: SRC-031, SRC-032
- **Confidence**: Medium
- **Caveat**: Document the sentinel clearly. If future code calls `localfile()` expecting a real path, it must handle the empty PathBuf case.

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|--------------|-------------|-------------|--------|
| Conflating Track construction with Track insertion in a single method | The current `TUIPlaylist::add_tracks` does disk I/O (reading metadata) AND vector insertion in one method, making it impossible to insert a pre-constructed Track without side effects | Separate the concerns: provide `insert_track_at` for pure insertion and keep `add_tracks` as a convenience wrapper for the legacy path | SRC-025 |
| Assuming TUI playlist has event emission because the server playlist does | DeepWiki (SRC-033) incorrectly suggested the TUI uses the server's Playlist struct with stream_tx. Direct code analysis proves the TUI uses a separate `TUIPlaylist` type with no event mechanism | Always verify architectural claims against actual codebase structure rather than assuming shared types | SRC-025, SRC-026, SRC-030 |
| Using database queries during UI rendering/layout switches for status indicators | Querying `db_podcast` on every layout switch to determine download status would reintroduce I/O latency that the feature is designed to eliminate | Precompute and transmit status indicators via the protocol; update only on explicit events (download complete -> full reload) | SRC-027, SRC-028 |

---

## Implementation Considerations

### Performance

- `insert_track_at` is O(n) for middle insertions (Vec shifts elements), but this is only called for individual track-add events (one track at a time), not bulk operations. For a 1000-track playlist, a single insert takes ~microseconds — negligible (SRC-025).
- The boolean `has_local_file` proto field adds 0 bytes for non-podcast tracks (absent optional) and 1 byte per podcast track (varint-encoded bool). For a mixed playlist with 50 podcast episodes, this is 50 bytes additional — negligible (SRC-031).
- The sentinel `PathBuf::new()` allocates a zero-length OsString on the heap (~24 bytes). For N podcast tracks, this is 24*N bytes. For 100 podcast tracks: 2.4KB — negligible (SRC-031).

### Security

- No new attack surface. The boolean field carries no user-controlled data. The sentinel PathBuf is never used for filesystem operations — only for `is_some()` checks (SRC-031).

### Compatibility

- Adding `optional bool has_local_file = 7` to `PlaylistAddTrack` is wire-compatible: older deserializers ignore unknown fields, newer deserializers treat absent fields as `None` (which maps to "not downloaded") (SRC-031).
- The `insert_track_at` method is purely internal to the TUI crate and has no wire/protocol implications (SRC-025).
- Both changes maintain full backward compatibility with existing tests and behavior (SRC-028).

---

## Contradictions Found

| Topic | Position A (SRC-033) | Position B (SRC-025, SRC-026, SRC-030) | Assessment |
|-------|---------------------|----------------------------------------|------------|
| Whether TUI playlist has stream_tx event emission | DeepWiki stated the TUI's `Model.playlist` is `termusicplayback::Playlist` with `stream_tx` field, suggesting event emission risk | Direct codebase analysis confirms `Model.playback.playlist` is `playlist::TUIPlaylist` (a separate type in `tui/src/ui/model/playlist.rs`) with NO stream_tx field | Position B is correct. DeepWiki's information is outdated or incorrect — likely based on an older version where the TUI may have used the server's Playlist type directly. The current codebase clearly shows two separate types. Verified by: (1) struct definition at line 14-20, (2) import at mod.rs:40, (3) zero grep results for stream_tx in entire tui/ crate. |

---

## Issues and Ambiguities

All prior issues are now resolved. Two minor implementation notes remain (non-blocking):

- **ISS-008** (Informational): The sentinel `PathBuf::new()` pattern for `localfile` should be documented with a code comment explaining that it represents "file exists on server but path is not transmitted." If a future feature needs the actual download path on the TUI side (e.g., for local playback without server mediation), `optional string local_file_path` could be added as field 8 at that time. This is a deferred concern with no current impact.

- **ISS-009** (Informational): After this feature is implemented, the `track_from_podcasturi` method on `TUIPlaylist` (line 186-191) becomes dead code for the `load_from_grpc` and `handle_playlist_add` paths. It may still be needed for TUI-initiated podcast episode additions (where the TUI adds a podcast episode to the playlist before the server processes it). Verify during implementation whether any caller still uses `add_tracks` with `PodcastUrl` source after the refactoring. If not, mark it for future cleanup.

---

## References

### Primary Sources (Codebase)

- SRC-025: termusic `tui/src/ui/model/playlist.rs:14-20` — TUIPlaylist struct definition (tracks, current_track_idx, loop_mode only)
- SRC-026: termusic `tui/src/ui/model/mod.rs:106-108` — Playback struct with `pub playlist: playlist::TUIPlaylist`
- SRC-027: termusic `tui/src/ui/components/podcast.rs:774-777` — episode_update_playlist only calls playlist_sync
- SRC-028: termusic `tui/src/ui/components/playlist.rs:585-588` — playlist_sync_podcasts reads has_localfile from Track objects
- SRC-029: termusic `tui/src/ui/components/podcast.rs:714-730` — episode_download_complete updates DB, not Track objects
- SRC-030: termusic `playback/src/playlist.rs:51,59` — Server Playlist has stream_tx; TUI TUIPlaylist does not
- SRC-031: termusic `lib/src/track.rs:39-89` — PodcastTrackData struct, has_localfile() checks is_some()
- SRC-032: termusic `lib/src/track.rs:199-224` — from_podcast_episode sets localfile from ep.path.take_if(exists)

### Secondary Sources (AI Documentation)

- SRC-033: DeepWiki: tramhao/termusic — TUI playlist architecture (partially incorrect regarding struct type, corrected by direct codebase analysis)
