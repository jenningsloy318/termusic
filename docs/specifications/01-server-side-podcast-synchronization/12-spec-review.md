# Spec Review: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Reviewer**: spec-inspector (Fagan-style)
- **Specification**: ./09-specification.md
- **Implementation Plan**: ./10-implementation-plan.md
- **Task List**: ./11-task-list.md
- **Verdict**: APPROVED

---

## Verdict Summary

The specification is well-grounded in the actual codebase, with accurate references to existing functions, types, and patterns. All 11 acceptance criteria are addressed, all 23 BDD scenarios are mapped to spec sections and test strategies, and the traceability chains are complete. The single new dependency (`humantime-serde`) is justified and minimal. Two minor findings are noted below but do not require revisions before implementation.

---

## D1 Completeness

**Score: 95%**

All 11 acceptance criteria (AC-01 through AC-11) have corresponding spec sections. All 23 BDD scenarios are addressed in the testing strategy (Section 6.4) with explicit scenario-to-test mapping. Error handling is specified in detail (Section 4.2, Section 5.2). NFRs are covered in Section 7.

The one gap is that the spec notes the "back-catalog flood" risk (Section 8) as a known open question but does not specify a `max_episodes_per_sync` field or workaround. This is explicitly deferred in both the requirements (Open Questions) and the spec (Risks section), so it is acceptable for MVP scope.

---

## D2 Consistency

**Score: 98%**

Terminology is uniform throughout:
- `SynchronizationSettings` is consistent across spec, implementation plan, task list, and architecture
- `PlaylistAddTrack` vs `PlaylistAddTrackInfo`: The spec correctly targets `playlist_helpers::PlaylistAddTrack` (lib/src/player.rs:446) for the new constructors, while the server's `PlayerCmd::PlaylistAddTrack` variant uses this same type. Naming is consistent.
- `sync_once`, `start_podcast_sync_task`, `SyncPassStats` are used consistently

One minor inconsistency: The spec's Section 5.5 states deduplication "falls back to matching by enclosure URL when GUID is absent" but the actual `Database::update_episodes` (lib/src/podcast/db/mod.rs:181-198) uses a 2-of-3 match on title, URL, and pubdate -- not solely enclosure URL. This is a documentation accuracy issue but does not cause implementation failure since the spec delegates to the existing `update_podcast` method rather than reimplementing the logic.

---

## D3 Feasibility

**Score: 98%**

The architecture is directly feasible:
- The `start_playlist_save_interval` pattern at server/src/server.rs:216-241 confirms the spec's proposed task lifecycle is a 1:1 mirror
- `check_feed`, `download_list`, `Database::update_podcast`, `Database::insert_file`, `Database::get_podcasts` all exist with compatible signatures
- `PlayerCmdSender::send(PlayerCmd::PlaylistAddTrack(...))` is the correct send path
- `ServerSettings` derives `Default` and uses `#[serde(default)]`, allowing seamless field addition
- `PlaylistAddTrack` struct has `at_index: u64` and `tracks: Vec<PlaylistTrackSource>` matching the spec's constructor design
- `TaskPool` cancellation on Drop provides safety for in-flight downloads

No circular dependencies or infeasible architecture detected.

---

## D4 Testability

**Score: 92%**

All ACs have measurable pass/fail criteria:
- Config tests: concrete assertions on field values after deserialization (SCENARIO-001 through 004)
- Constructor tests: `AT_END == u64::MAX`, field equality checks
- Integration tests: verifiable via mock HTTP servers and channel inspection

Minor concern: The implementation plan Phase 5 mentions `wiremock` as a testing dependency, but this crate is not in the project's existing dependencies. The plan also offers "inline test server" as alternative, which is achievable without new dependencies using tokio's TCP listener. This is not blocking.

SCENARIO-022 (non-disruption during playback) may be difficult to test deterministically in CI, but the spec correctly identifies this as an E2E test and the channel serialization provides the behavioral guarantee.

---

## D5 Traceability

**Score: 100%**

Complete traceability chains verified:
- **AC -> Spec Section**: All 11 ACs map to spec sections (Section 3.1/5.6 for AC-01/AC-10, Section 5.4 for AC-02/AC-11, Section 5.1 for AC-03/AC-04/AC-09, Section 5.2/5.5 for AC-05, Section 5.2 for AC-06, Section 5.2 for AC-07, Section 5.2 for AC-08)
- **SCENARIO -> Task**: All 23 scenarios have explicit task references in the task list (T-05 through T-08 for config scenarios, T-24 through T-26 for integration scenarios)
- **Plan -> Task List**: 5 phases map cleanly to 26 tasks with correct dependencies
- **Architecture -> Spec**: ADR-001 through ADR-004 align with spec decisions

No orphan requirements, scenarios, or tasks found.

---

## D6 Grounding

**Score: 96% (verified_references: 24/25)**

Codebase verification of all referenced entities:

| Reference | Location | Verified |
|-----------|----------|----------|
| `start_playlist_save_interval` | server/src/server.rs:216 | YES |
| `actual_main()` | server/src/server.rs:103 | YES |
| `CancellationToken` | tokio_util::sync (server/src/server.rs:27) | YES |
| `service_cancel_token` | server/src/server.rs:163 | YES |
| `PlayerCmdSender` | playback/src/lib.rs:62 | YES |
| `PlayerCmd::PlaylistAddTrack` | playback/src/lib.rs:142 | YES |
| `PlaylistAddTrack` struct | lib/src/player.rs:446 | YES |
| `PlaylistTrackSource` | lib/src/player.rs:400 | YES |
| `new_single` / `new_vec` | lib/src/player.rs:453, 461 | YES |
| `lib/src/config/v2/server/mod.rs` | EXISTS | YES |
| `ServerSettings` struct | lib/src/config/v2/server/mod.rs:24 | YES |
| `SharedServerSettings` type | lib/src/config/mod.rs:17 | YES |
| `ServerOverlay` | lib/src/config/server_overlay.rs:8 | YES |
| `Database::new(path)` | lib/src/podcast/db/mod.rs:46 | YES |
| `db.get_podcasts()` | lib/src/podcast/db/mod.rs:282 | YES |
| `db.update_podcast(pod_id, podcast)` | lib/src/podcast/db/mod.rs:129 | YES |
| `db.insert_file(episode_id, path)` | lib/src/podcast/db/mod.rs:98 | YES |
| `check_feed(feed, max_retries, tp, callback)` | lib/src/podcast/mod.rs:90 | YES |
| `download_list(episodes, dest, max_retries, tp, callback)` | lib/src/podcast/mod.rs:467 | YES |
| `PodcastDLResult` enum | lib/src/podcast/mod.rs:453 | YES |
| `TaskPool` | lib/src/taskpool.rs:11 | YES |
| `get_app_config_path()` | lib/src/utils.rs:82 | YES |
| `PodcastSettings.concurrent_downloads_max` | lib/src/config/v2/server/mod.rs:37 | YES |
| `PodcastSettings.max_download_retries` | lib/src/config/v2/server/mod.rs:40 | YES |
| `humantime-serde` in workspace Cargo.toml | NOT YET (to be added) | N/A (new dep) |

Grounding score: 24/24 existing references verified = **100%**. The single unverified item is `humantime-serde` which is a new dependency to be added (not a claim about existing code).

---

## D7 Complexity

**Score: 95%**

The implementation introduces:
- 2 new files (configuration struct, sync logic)
- 5 modified files (workspace Cargo.toml, lib Cargo.toml, config mod, player.rs, server.rs)
- 26 tasks across 5 phases

This is proportional to the feature scope. The design correctly avoids:
- No new abstractions beyond what's needed (reuses existing TaskPool, Database, check_feed, download_list)
- No new enum variants (reuses existing `PlayerCmd::PlaylistAddTrack`)
- No new crates beyond `humantime-serde`
- No changes to the gRPC API or TUI

The 5-phase incremental approach ensures each commit is independently compilable and testable.

---

## D8 Ambiguity

**Score: 93%**

The spec provides:
- Exact Rust struct definitions with concrete types and defaults (Section 3)
- Precise function signatures with parameter types and error semantics (Section 4)
- Complete implementation pseudocode for the task lifecycle (Section 5.1) and sync pass (Section 5.2)
- Explicit error handling for each error category (Section 4.2)
- Numeric defaults: interval=3600s, AT_END=u64::MAX, concurrent_downloads_max=3

One area of slight ambiguity: The spec describes the per-podcast flow (Section 5.2 step 3) using `check_feed` but does not explicitly detail how to handle the fact that `check_feed` is callback-based (spawns a task and calls the closure). The spec references it as "check_feed pattern" and the architecture diagram shows it "via TaskPool". The implementation will need a channel-drain per podcast (similar to how `import_from_opml` works), and while the pattern is well-documented in the codebase reference, the spec could be more explicit about the per-podcast channel setup. This is a minor clarity issue -- an implementer familiar with the codebase (or reading the code assessment) would resolve it immediately.

---

## Findings

### Finding 1 (Minor): Deduplication fallback description oversimplified

- **Section**: Specification Section 5.5, bullet 2
- **Issue**: The spec states "Falls back to matching by enclosure URL when GUID is absent" but the actual `Database::update_episodes` method (lib/src/podcast/db/mod.rs:181-198) uses a 2-of-3 match on title, URL, and pubdate -- not solely enclosure URL. The requirements (AC-05) say "keyed by GUID first with fallback to enclosure URL" which is also oversimplified.
- **Severity**: minor
- **Impact**: No implementation failure -- the spec delegates to `db.update_podcast()` rather than reimplementing the logic. The existing method handles deduplication correctly regardless of how it's described. An implementer calling `update_podcast` gets the correct behavior automatically.
- **Recommendation**: Clarify the fallback description to say "Falls back to the existing multi-field matching logic in `Database::update_episodes` (title, URL, pubdate)" or simply "delegates to `Database::update_podcast` which handles deduplication internally."

### Finding 2 (Minor): Test dependency for integration tests not in project

- **Section**: Implementation Plan Phase 5, Tasks section
- **Issue**: Phase 5 mentions using `wiremock` for mock HTTP servers, but this crate is not in the project's dev-dependencies. The implementation plan hedges with "or inline test server" but does not specify which approach to use.
- **Severity**: minor
- **Impact**: The implementer will need to decide whether to add `wiremock` as a dev-dependency or build a lightweight inline test server. This is a minor implementation decision, not a blocking gap.
- **Recommendation**: Either specify `wiremock` as a dev-dependency to be added (with version), or describe the inline test server approach (e.g., `tokio::net::TcpListener` with hardcoded RSS responses). The existing codebase has no mock HTTP testing infrastructure, so this is new ground.

---

## Coverage Matrix

### AC Coverage (11/11 = 100%)

| AC-ID | Spec Section | Status |
|-------|-------------|--------|
| AC-01 | 3.1, 3.4, 5.6 | Covered |
| AC-02 | 5.4 | Covered |
| AC-03 | 5.1 | Covered |
| AC-04 | 5.1 | Covered |
| AC-05 | 5.2, 5.5 | Covered |
| AC-06 | 5.2 | Covered |
| AC-07 | 5.2, 3.3 | Covered |
| AC-08 | 5.2, 4.2 | Covered |
| AC-09 | 5.1 | Covered |
| AC-10 | 3.1, 5.6 | Covered |
| AC-11 | 5.4 | Covered |

### BDD Scenario Coverage (23/23 = 100%)

All scenarios SCENARIO-001 through SCENARIO-023 are referenced in spec Section 6.4 with explicit test level (unit/integration/e2e) assignments.

---

## Quantitative Summary

| Metric | Value |
|--------|-------|
| ACs addressed | 11/11 (100%) |
| BDD scenarios addressed | 23/23 (100%) |
| Grounding score | 24/24 (100%) |
| New files | 2 |
| Modified files | 5 |
| New dependencies | 1 (humantime-serde) |
| Critical findings | 0 |
| Major findings | 0 |
| Minor findings | 2 |

---

## Dimension Scores

| Dimension | Score |
|-----------|-------|
| D1 Completeness | 0.95 |
| D2 Consistency | 0.98 |
| D3 Feasibility | 0.98 |
| D4 Testability | 0.92 |
| D5 Traceability | 1.00 |
| D6 Grounding | 1.00 |
| D7 Complexity | 0.95 |
| D8 Ambiguity | 0.93 |

---

## Conclusion

The specification is thorough, well-grounded in the actual codebase, and ready for implementation. The two minor findings do not block implementation and can be addressed as documentation clarifications during development. The architecture correctly mirrors proven patterns already in the codebase, the dependency footprint is minimal, and all acceptance criteria and scenarios have clear implementation paths.

**Verdict: APPROVED**
