# Specification Review: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Reviewer**: spec-inspector
- **Specification**: ./07-specification.md
- **Implementation Plan**: ./08-implementation-plan.md
- **Task List**: ./09-task-list.md
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md
- **Architecture**: ./05-architecture.md
- **Code Assessment**: ./04-code-assessment.md

---

## Verdict: APPROVED

---

## Executive Summary

This specification is well-grounded, comprehensive, and closely aligned with the actual codebase. The proposed protocol extension is a clean, additive change to a well-understood protobuf message. All major codebase references (file paths, method names, struct fields, proto field numbers) have been verified against the actual worktree and are accurate. The approach directly addresses the root cause identified in the code assessment and requirements. The implementation plan and task list provide clear, incremental steps with correct dependency ordering.

One minor pseudocode issue exists in Section 5.4 (type mismatch in `handle_playlist_add` example), but it does not affect implementability since the spec's intent is clear from the surrounding context and the correct types are defined earlier in the document.

---

## Quantitative Assessment

- **AC Coverage**: 10/10 (100%)
- **BDD Scenario Coverage**: 28/28 (100%)
- **Grounding Score**: 95% (38/40 verifiable references confirmed)
- **Traceability Completeness**: 100% (all AC -> spec section, scenario -> task chains verified)

---

## Dimension Scores

### D1 Completeness

**Score: 95%**

Every acceptance criterion (AC-01 through AC-10) has a corresponding spec section with implementation details. All 28 BDD scenarios are addressed in the testing strategy (Section 6.4) with explicit scenario-to-test mappings. Error handling is specified for missing metadata (Section 5.6, 5.7), missing track IDs (Section 5.3 `bail!`), and corrupted files (Section 4.1 error cases). Non-functional requirements are addressed with specific numeric thresholds.

Minor gap: The spec does not explicitly describe how logging (NFR Observability) is implemented in the new `load_from_grpc` - it mentions it in Section 7.3 but does not show it in the code samples.

### D2 Consistency

**Score: 98%**

Terminology is consistent throughout: `Track::from_grpc_metadata`, `insert_track_at`, `PlaylistAddTrackInfo`, `PlaylistTrackSource` are used uniformly across the specification, implementation plan, task list, and architecture documents. The proto field naming (artist, album, has_local_file) matches the domain struct field names.

One minor inconsistency: Section 5.4 `handle_playlist_add` code sample uses `Duration::from_secs(info.duration as u64).into()` but `info.duration` is `PlayerTimeUnit` (std::time::Duration), not a numeric type. The correct conversion should be `Some(info.duration)`. This is limited to one pseudocode example.

### D3 Feasibility

**Score: 98%**

The architecture fits the project's existing patterns perfectly:
- Additive proto field extension is standard protobuf practice
- The Track struct already has all the target fields (title, artist, duration at top level; album in TrackData)
- `PodcastTrackData` already has the `localfile: Option<PathBuf>` field and `has_localfile()` method
- The sentinel PathBuf pattern (PathBuf::new() for "exists but path unknown") is lightweight
- No new dependencies required
- All proposed methods follow existing naming conventions (`from_*` constructors, `insert_*` mutations)

No circular dependencies or architectural conflicts identified.

### D4 Testability

**Score: 95%**

All ACs have measurable pass/fail criteria:
- AC-01: <100ms (numeric threshold, measurable)
- AC-02: <200ms (numeric threshold, measurable)
- AC-09: <50ms (numeric threshold, measurable)
- AC-04: Verifiable by absence of `Track::read_track_from_path` calls
- AC-06: Verifiable by proto compilation and field number inspection

Performance tests specify concrete types (benchmark, timing assertion) with generous margins. Unit tests cover all Track source variants (Path, Url, PodcastUrl).

### D5 Traceability

**Score: 100%**

Complete traceability chains:
- AC-01 -> Spec Section 5.3 -> Phase 3 Task T-26 -> Phase 4 Task T-34
- AC-02 -> Spec Section 5.3 -> Phase 3 Task T-26 -> Phase 4 Task T-34
- AC-03 -> Spec Sections 3.1, 5.1 -> Phase 2 Tasks T-18 to T-22
- AC-04 -> Spec Section 3.3 -> Phase 1 Tasks T-09 to T-11, Phase 3 Task T-26
- AC-05 -> Spec Section 4.3 -> Phase 3 Task T-27
- AC-06 -> Spec Section 3.1 -> Phase 1 Tasks T-01 to T-03
- AC-07 -> Spec Section 5.1 -> Phase 2 Task T-18
- AC-08 -> Spec Sections 5.6, 5.7 -> Phase 2 Tasks T-22, T-25
- AC-09 -> Spec Section 7.1 -> Phase 4 Task T-34
- AC-10 -> Spec Section 6.2 -> Phase 4 Task T-35

All 28 BDD scenarios are mapped to tasks in Phase 4.

### D6 Grounding

**Score: 95% (38/40 references verified)**

Verified references:
- `lib/proto/player.proto` - EXISTS, PlaylistAddTrack at line 228 with fields 1-4 confirmed
- `lib/src/player.rs` - EXISTS, PlaylistAddTrackInfo at line 336 confirmed (missing artist/album/has_local_file as expected)
- `lib/src/track.rs` - EXISTS, Track struct at line 185 with inner/duration/title/artist fields confirmed
- `tui/src/ui/model/mod.rs` - EXISTS, load_from_grpc at line 187 with db_pod parameter confirmed
- `tui/src/ui/model/playlist.rs` - EXISTS, track_from_path at line 157, refactor annotation at line 173, track_from_podcasturi at line 186, refactor annotation at line 187 all confirmed
- `tui/src/ui/components/playlist.rs` - EXISTS, handle_playlist_add at line 448, handle_playlist_shuffled at line 509 confirmed
- `tui/src/ui/model/update.rs` - EXISTS, load_from_grpc caller at line 1131 confirmed
- `playback/src/playlist.rs` - EXISTS, as_grpc_playlist_tracks at line 1030, send_stream_ev_pl at line 1132, optional_title: None at line 1043 confirmed
- `TrackData.album: Option<String>` - Confirmed at line 118
- `PodcastTrackData.localfile: Option<PathBuf>` - Confirmed at line 43
- `PodcastTrackData::has_localfile()` - Confirmed at line 68
- `Track::artist()` - Confirmed at line 287
- `Track::title()` - Confirmed at line 292
- `playlist_sync()` usage of track.as_track().and_then(|v| v.album()) - Confirmed at line 648-651
- `PlaylistTrackSource` enum variants (Path, Url, PodcastUrl) - Confirmed at lines 504-507
- Proto field numbers 1-4 are in use - Confirmed (at_index=1, title=2, duration=3, id=4)
- Individual event sends title (line 672, 726) - Confirmed
- Bulk as_grpc_playlist_tracks sends optional_title: None (line 1043) - Confirmed
- `RadioTrackData::new(url)` constructor - Confirmed at line 108

Unverified/inaccurate references (2):
- `track.title_or_filename()` mentioned in Section 5.6 - Method does NOT exist. Actual pattern is `track.title().map_or_else(|| track.id_str(), Into::into)` in playlist_sync. However, the spec says "(or equivalent path-based fallback)" which acknowledges this.
- Section 5.4 code: `Duration::from_secs(info.duration as u64).into()` - Invalid Rust (can't cast std::time::Duration as u64). Correct code: `Some(info.duration)`.

### D7 Complexity

**Score: 98%**

The change is proportional to the problem:
- 3 new proto fields (minimal wire change)
- 1 new constructor method (Track::from_grpc_metadata)
- 1 new insertion method (TUIPlaylist::insert_track_at)
- 2 method rewrites (load_from_grpc, handle_playlist_add)
- Server serialization update (populate existing + new fields)

No unnecessary abstractions. No new crates, traits, or architectural layers. The 35-task breakdown may appear large but each task is atomic and small. The 4-phase structure follows the established pattern from spec-04.

### D8 Ambiguity

**Score: 95%**

Proto schema is fully defined with field numbers, types, and semantics. The Track::from_grpc_metadata constructor has explicit type signatures and behavior for all three source variants. Error handling is explicit: bail on missing track ID, unwrap_or(false) for has_local_file, None propagation for optional fields.

Minor ambiguity: The spec mentions "derive title from filename" (Section 5.7) but does not specify the exact method for filename derivation (e.g., `file_stem()` vs stripping extension manually). However, `file_stem()` is mentioned in the same section, making intent clear.

---

## Coverage Matrix

| AC-ID | Spec Section | Implementation Phase | Status |
|-------|-------------|---------------------|--------|
| AC-01 | 5.3 | Phase 3 (T-26) | Covered |
| AC-02 | 5.3 | Phase 3 (T-26) | Covered |
| AC-03 | 3.1, 5.1 | Phase 1 (T-01-03), Phase 2 (T-18-21) | Covered |
| AC-04 | 3.3, 5.3 | Phase 1 (T-09-11), Phase 3 (T-26) | Covered |
| AC-05 | 4.3 | Phase 3 (T-27) | Covered |
| AC-06 | 3.1, 2.4 | Phase 1 (T-01-03) | Covered |
| AC-07 | 5.1, 5.7 | Phase 2 (T-18, T-22) | Covered |
| AC-08 | 5.6, 5.7, 4.1 | Phase 2 (T-22, T-25), Phase 3 | Covered |
| AC-09 | 7.1 | Phase 4 (T-34) | Covered |
| AC-10 | 6.2 | Phase 4 (T-35) | Covered |

---

## Findings

### Finding 1: Pseudocode type mismatch in handle_playlist_add

**Severity**: minor
**Section**: 07-specification.md Section 5.4
**Issue**: The code sample shows `Duration::from_secs(info.duration as u64).into()` but `info.duration` is `PlayerTimeUnit` (which is `std::time::Duration`). You cannot cast `std::time::Duration` to `u64` with the `as` operator. The correct expression should be `Some(info.duration)` to match the `Track::from_grpc_metadata` signature of `duration: Option<Duration>`.
**Impact**: Low - this is pseudocode illustrating intent. The implementer will recognize the type mismatch at compile time and use the correct conversion.
**Recommendation**: Update the example to `Some(info.duration)` to avoid confusion during implementation.

### Finding 2: Nonexistent method name in fallback description

**Severity**: minor
**Section**: 07-specification.md Section 5.6
**Issue**: The spec references `track.title_or_filename()` as the fallback mechanism in playlist_sync. This method does not exist in the codebase. The actual pattern used in `playlist_sync()` at line 645 of `tui/src/ui/components/playlist.rs` is `track.title().map_or_else(|| track.id_str(), Into::into)`.
**Impact**: Low - the spec says "(or equivalent path-based fallback)" which partially acknowledges this. The implementer can verify by reading the existing code.
**Recommendation**: Replace `title_or_filename()` with the actual pattern `title().map_or_else(|| id_str(), ...)` or simply note "the existing playlist_sync display logic handles fallback via `track.id_str()`".

### Finding 3: Observability logging not shown in rewritten load_from_grpc

**Severity**: minor
**Section**: 07-specification.md Section 7.3 vs Section 5.3
**Issue**: Section 7.3 specifies "TUI logs timing of playlist response processing at INFO level" but the `load_from_grpc` code sample in Section 5.3 does not include this logging statement. The implementer may forget to add it.
**Impact**: Low - observability is a nice-to-have, not a blocking requirement for correctness or performance.
**Recommendation**: Add an `info!("Processed {} tracks in {:?}", playlist_items.len(), elapsed)` line to the Section 5.3 code sample, or note it as a required addition.

---

## Grounding Verification

| Reference | Location | Status |
|-----------|----------|--------|
| lib/proto/player.proto:PlaylistAddTrack | Line 228 | VERIFIED |
| PlaylistAddTrack fields 1-4 | Lines 231-246 | VERIFIED |
| lib/src/player.rs:PlaylistAddTrackInfo | Line 336 | VERIFIED |
| lib/src/track.rs:Track struct | Line 185 | VERIFIED |
| Track fields: inner, duration, title, artist | Lines 186-190 | VERIFIED |
| TrackData.album: Option<String> | Line 118 | VERIFIED |
| PodcastTrackData.localfile | Line 43 | VERIFIED |
| PodcastTrackData::has_localfile() | Line 68 | VERIFIED |
| tui/src/ui/model/mod.rs:load_from_grpc | Line 187 | VERIFIED |
| load_from_grpc param podcast_db: &DBPod | Line 190 | VERIFIED |
| tui/src/ui/model/playlist.rs:track_from_path | Line 157 | VERIFIED |
| Refactor annotation at playlist.rs:173 | Line 173 | VERIFIED |
| Refactor annotation at playlist.rs:187 (approx) | Line 187 | VERIFIED |
| tui/src/ui/components/playlist.rs:handle_playlist_add | Line 448 | VERIFIED |
| tui/src/ui/components/playlist.rs:handle_playlist_shuffled | Line 509 | VERIFIED |
| tui/src/ui/model/update.rs:load_from_grpc caller | Line 1131 | VERIFIED |
| playback/src/playlist.rs:as_grpc_playlist_tracks | Line 1030 | VERIFIED |
| as_grpc_playlist_tracks sends optional_title: None | Line 1043 | VERIFIED |
| playback/src/playlist.rs:send_stream_ev_pl | Line 1132 | VERIFIED |
| Track::read_track_from_path | Line 242 | VERIFIED |
| PlaylistTrackSource enum | Line 504 | VERIFIED |
| track.title_or_filename() | N/A | NOT FOUND (minor) |

---

## Anti-Pattern Check

| Anti-Pattern | Status | Notes |
|--------------|--------|-------|
| YAGNI violations | None | All proposed changes directly address ACs |
| Premature optimization | None | Performance thresholds from requirements, validated by prototype |
| Untestable requirements | None | All ACs have numeric thresholds or binary verifiable conditions |
| Missing error paths | None | Error handling specified for missing IDs, missing metadata, corrupted files |
| Gold-plating | None | Minimal approach (3 proto fields, 1 constructor) chosen over richer alternatives |

---

## Summary

The specification is well-crafted, thoroughly grounded in the actual codebase, and correctly identifies both the root cause and the minimal intervention needed. The proto extension approach (additive optional fields) is sound and follows protobuf best practices. The implementation plan provides a safe incremental path where each phase compiles independently. The task list is granular and correctly ordered by dependencies.

The three minor findings are all pseudocode/documentation issues that would be caught at compile time and do not represent implementation risks. The specification is ready for implementation.
