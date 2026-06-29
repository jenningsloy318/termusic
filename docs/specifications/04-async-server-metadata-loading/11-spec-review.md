# Spec Review: Async Server Metadata Loading

- **Date**: 2026-06-26
- **Reviewer**: spec-inspector
- **Specification**: ./08-specification.md
- **Implementation Plan**: ./09-implementation-plan.md
- **Task List**: ./10-task-list.md
- **Verdict**: APPROVED

---

## Summary

The specification for async server metadata loading is well-structured, technically sound, and closely aligned with the existing codebase patterns. It correctly identifies the blocking `Playlist::new_shared()` call at `server.rs:148-149`, proposes a minimal-change solution using existing infrastructure (`PLAYLIST_POOL`, `CancellationToken`, broadcast channels), and follows the established `start_podcast_sync_task` pattern. All 10 acceptance criteria are addressed, all 27 BDD scenarios are mapped, and the implementation plan decomposes work into reasonable phases. A few minor issues were identified but none block implementation.

---

## Dimensions

### D1 Completeness

**Score**: 95%

All 10 acceptance criteria (AC-01 through AC-10) have corresponding spec sections that address them. All 27 BDD scenarios are mapped to test coverage in Section 6.4. Error handling is specified for total load failure, partial track failures, JoinError/panic, and cancellation. NFRs for performance, reliability, observability, and security are covered in Section 7. The only gap is the missing specification of a new public method on `Playlist` to accept pre-loaded data (see finding F-01).

### D2 Consistency

**Score**: 90%

Terminology is consistent throughout: `PlaylistLoadingFlag`, `SharedPlaylist`, `PlayerCmd::PlaylistLoadComplete`, `complete_background_load`. The spec, implementation plan, and task list use identical naming. One inconsistency exists regarding the empty-playlist edge case behavior (see finding F-02). The spec's parameter description for `config` in `start_background_playlist_load` is slightly inaccurate (described as "needed by Playlist::load for paths" when `Playlist::load()` takes no parameters) but this does not affect implementation correctness since the config may be useful for other purposes in the function.

### D3 Feasibility

**Score**: 92%

The architecture fits the project patterns exactly. The `start_podcast_sync_task` pattern (Handle + CancellationToken + select!) is reused verbatim. All required dependencies (`tokio`, `parking_lot`, `rayon`, `tokio-util`) are already present in the workspace at current versions. No new external dependencies needed. The one feasibility concern is that `Playlist` struct fields are private and no existing public method accepts pre-loaded `(usize, Vec<Track>)` data -- a new method must be added (see finding F-01).

### D4 Testability

**Score**: 95%

ACs have measurable pass/fail criteria: AC-01 specifies "within 1 second", AC-09 specifies "within 1 second shutdown". The testing strategy (Section 6) maps every BDD scenario to a concrete test type (unit/integration/e2e). Performance thresholds are numeric. The test file naming follows the established `phase*_*_tests.rs` pattern. Timing tests acknowledge flakiness risk and specify generous margins.

### D5 Traceability

**Score**: 98%

Traceability chains are well-established:
- AC-01 through AC-10 map to spec Sections 2.1-2.5, 5.1-5.6
- BDD SCENARIO-001 through SCENARIO-027 map to test coverage in Section 6.4
- Implementation plan phases reference spec sections and ACs
- Task list references ACs per task (AC refs field)
- One minor break: the implementation plan Phase 3 scope contradicts spec Section 5.5 on empty-playlist behavior (see F-02)

### D6 Grounding

**Score**: 95% (19/20 verified references)

Verified references against the codebase:

| Reference | Status |
|-----------|--------|
| `Playlist::new_shared()` at server.rs:148-149 | VERIFIED (exact line) |
| `SharedPlaylist = Arc<RwLock<Playlist>>` at playback/src/lib.rs:179 | VERIFIED |
| `PlayerCmd` enum in playback/src/lib.rs:104 | VERIFIED |
| `start_podcast_sync_task` at podcast_sync.rs:476-519 | VERIFIED |
| `start_playlist_save_interval` at server.rs:239 | VERIFIED |
| `PLAYLIST_POOL` static in parallel_load.rs:27 | VERIFIED |
| `Playlist::load() -> Result<(usize, Vec<Track>)>` at playlist.rs:190 | VERIFIED |
| `Playlist::new(&config, stream_tx) -> Self` at playlist.rs:59 | VERIFIED |
| `player_loop` at server.rs:315, auto-play check at lines 332-335 | VERIFIED |
| `resume_from_stopped()` at playback/src/lib.rs:673 | VERIFIED |
| `CancellationToken` import at server.rs:27 | VERIFIED |
| `Handle::current()` at server.rs:177 | VERIFIED |
| `broadcast::channel(10)` at server.rs:146 | VERIFIED |
| `StreamTX = broadcast::Sender<UpdateEvents>` at playback/src/lib.rs:178 | VERIFIED |
| `UpdatePlaylistEvents::PlaylistShuffled` at lib/src/player.rs:387 | VERIFIED |
| `StartupState::Playing` at lib/src/config/v2/server/mod.rs:256 | VERIFIED |
| `parking_lot` in server/Cargo.toml and playback/Cargo.toml | VERIFIED |
| `tokio-util` in server/Cargo.toml | VERIFIED |
| `save_if_modified()` at playback/src/playlist.rs:402 | VERIFIED |
| `config: SharedServerSettings` "needed by Playlist::load for paths" | INACCURATE (Playlist::load takes no params) |

Grounding score: 95% (19 verified, 1 inaccurate description but non-blocking).

### D7 Complexity

**Score**: 95%

The solution is proportional to the problem. Changes touch primarily one file (`server/src/server.rs`) plus one enum variant addition in `playback/src/lib.rs`. Two new internal functions are introduced (`start_background_playlist_load`, `complete_background_load`). One type alias is added. No new modules, crates, or abstractions beyond what the existing pattern library already provides. The test file in Phase 4 is appropriately scoped.

### D8 Ambiguity

**Score**: 92%

API signatures are fully defined with types and doc-comments. The four-step ordering invariant is precisely specified with memory ordering semantics. Error responses are enumerated per function. The `AtomicBool` semantics (Release/Acquire) are explicit. Minor ambiguity: how exactly to "populate the SharedPlaylist" in step 1 of the completion handler given that Playlist fields are private (see F-01). The spec describes the logical operation but not the mechanical API needed.

---

## Findings

### F-01: Missing Playlist Method for Data Population

**Section**: Specification 4.2 (complete_background_load), Implementation Plan Phase 2 Task T-05
**Severity**: minor
**Issue**: The spec describes "Write-lock swap: Populate SharedPlaylist with loaded tracks and index" but the `Playlist` struct fields (`tracks`, `current_track_index`) are private (playlist.rs:39-41). No existing public method accepts pre-loaded `(usize, Vec<Track>)` data. The only similar method is `load_apply()` which re-reads from disk (defeating the purpose). A new public method on `Playlist` (e.g., `populate_from_loaded(index: usize, tracks: Vec<Track>)`) is required but not specified or included in the task list.
**Recommendation**: Add a task to Phase 1 or Phase 2 in the task list: "Add `Playlist::populate_from_loaded(index: usize, tracks: Vec<Track>)` public method in `playback/src/playlist.rs`" that sets `self.tracks = tracks`, `self.current_track_index = index`, `self.is_modified = false`. This is a trivial addition (3 lines) but must be called out explicitly to avoid confusion during implementation.

### F-02: Inconsistency on Empty-Playlist Edge Case Behavior

**Section**: Specification 5.5 vs Implementation Plan Phase 3 Scope
**Severity**: minor
**Issue**: Spec Section 5.5 states: "The background loading task detects this and still follows the full completion sequence (clearing the flag, sending notifications)." The Implementation Plan Phase 3 scope states: "Handle the empty-playlist edge case (playlist.log does not exist or is empty): no background loading spawned, flag set to false immediately." These are contradictory -- either the background task is spawned (spec) or it is not (implementation plan). BDD SCENARIO-002 says "no background metadata loading task is spawned" which aligns with the implementation plan.
**Recommendation**: Resolve in favor of one approach. The spec's approach (always spawn, let it complete quickly) is simpler to implement and test because it avoids a special-case code path. Update either the implementation plan or the spec to be consistent. If the BDD scenario's wording is kept, the spec Section 5.5 should be updated to say "when playlist.log is empty, the loading flag is set to false immediately without spawning the background task."

### F-03: Inaccurate Parameter Description for config in start_background_playlist_load

**Section**: Specification 4.1 (start_background_playlist_load parameters)
**Severity**: minor
**Issue**: The spec states `config: SharedServerSettings` is "needed by Playlist::load for paths". However, `Playlist::load()` is a static method that takes zero parameters (`pub fn load() -> Result<(usize, Vec<Track>)>`) and discovers its path via `get_playlist_path()` internally. The `config` parameter is not needed for calling `Playlist::load()`.
**Recommendation**: Remove the `config` parameter from `start_background_playlist_load` unless it is needed for other purposes within the function (none are currently specified). Alternatively, update the description to accurately state why config is passed (it is not needed). This avoids developer confusion about whether Playlist::load's signature needs to be changed.

---

## Coverage Matrix

### AC Coverage

| AC-ID | Spec Section | Status |
|-------|-------------|--------|
| AC-01 | 2.1, 5.1 | COVERED |
| AC-02 | 2.3, 4.1 | COVERED |
| AC-03 | 2.4, 4.2, 5.5 | COVERED |
| AC-04 | 2.4 step 3, 4.2 | COVERED |
| AC-05 | 5.1 | COVERED |
| AC-06 | 2.5, 5.4 | COVERED |
| AC-07 | 2.2, 5.3 | COVERED |
| AC-08 | 4.1 error cases, 7.2 | COVERED |
| AC-09 | 2.3, 5.6 | COVERED |
| AC-10 | 5.1 | COVERED |

**AC coverage**: 10/10 = 100%

### BDD Scenario Coverage

All 27 scenarios (SCENARIO-001 through SCENARIO-027) are explicitly mapped in spec Section 6.4 with test type and coverage status.

**Scenario coverage**: 27/27 = 100%

### Traceability Chains

| Chain | Status |
|-------|--------|
| Requirements AC -> Spec sections | Complete |
| BDD Scenarios -> Test strategy | Complete |
| Spec sections -> Implementation plan phases | Complete |
| Implementation plan -> Task list | Complete (except F-01 gap) |
| Architecture (code assessment) -> Spec approach | Aligned |

**Traceability completeness**: 95%

---

## Anti-Pattern Check

| Anti-Pattern | Status |
|-------------|--------|
| YAGNI violations | None detected. All functions serve a specific AC. |
| Premature optimization | None. Uses existing PLAYLIST_POOL without new optimizations. |
| Untestable requirements | None. All ACs have numeric thresholds or observable outcomes. |
| Missing error paths | None. All error cases enumerated in Section 4.1 and 4.2. |
| Gold-plating | None. Solution is minimal (Option 1 from requirements). |
| Over-specification | Minor: config parameter may be unnecessary (F-03). |

---

## Verdict Rationale

The specification is technically sound and well-grounded in the actual codebase. All 10 acceptance criteria are fully addressed. All 27 BDD scenarios have explicit test coverage mapping. The architecture reuses established patterns without introducing unnecessary complexity. Three minor findings were identified: (1) a missing task for a trivial Playlist method, (2) an inconsistency between spec and implementation plan on empty-playlist handling, and (3) an inaccurate parameter description. None of these block implementation -- they are easily resolvable during development without requiring a spec rewrite.

**Verdict: APPROVED**
