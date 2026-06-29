# Specification Review: PR #720 Podcast Synchronization — Review Feedback Remediation (Second Review)

- **Date**: 2026-06-25
- **Reviewer**: spec-inspector
- **Specification**: ./09-specification.md
- **Implementation Plan**: ./10-implementation-plan.md
- **Task List**: ./11-task-list.md
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md
- **Architecture**: ./07-architecture.md

---

## Verdict: APPROVED

The specification has been revised to address all findings from the first review. It is internally consistent, correctly grounded against the codebase, and provides complete coverage of all acceptance criteria and BDD scenarios. The remaining issues are minor and will not block or misdirect implementation.

---

## Quantitative Summary

| Metric | Value |
|--------|-------|
| ACs Addressed | 31/31 (100%) |
| BDD Scenarios Covered | 42/42 (100%) |
| Grounding Score | 97% (29/30 verified references) |
| Traceability Completeness | 98% |
| Critical Findings | 0 |
| High Findings | 0 |
| Medium Findings | 1 |
| Low Findings | 2 |

---

## D1 Completeness

**Score: 97%**

All 31 acceptance criteria have corresponding spec sections. All 42 BDD scenarios are mapped in Section 6.4 (testing strategy). Error handling is comprehensively specified per-boundary in the architecture document's Error Handling Strategy table and Section 8 of the spec. NFRs are covered in Section 7 (Performance, Reliability, Backward Compatibility, Security).

AC-07 (human-readable comments on Duration defaults) is addressed as a task-list item (T-13) rather than a behavioral spec section, which is appropriate for a source-code style constraint.

---

## D2 Consistency

**Score: 95%**

The first review's critical finding (contradictory default interval) has been resolved. The specification now correctly sets `interval: Duration::ZERO` in the Default impl (Section 3.1, line 110) with an explicit comment: "Duration::ZERO means disabled -- absent config means sync is off (AC-05, SCENARIO-008)." This is consistent with Section 7.3 (Backward Compatibility), AC-05, and SCENARIO-008.

The phase numbering inconsistency between requirements (Phase 0-4) and implementation plan (Phase 1-5) is addressed with an explicit mapping table in both the specification (Appendix: Phase Numbering Mapping) and the implementation plan (Phase Numbering Mapping table at line 23-29). This eliminates implementer confusion.

Data model names are consistent throughout: `SynchronizationSettings`, `PodcastSettings`, `ExistingFilesMap`, `SyncPassStats`, `UpdatePodcastSyncEvents` are used uniformly across spec, implementation plan, and task list.

Remaining minor issue: The architecture document's "Numeric Constants" table (line 639) still lists "Default sync interval | 3600s" while the specification correctly uses Duration::ZERO. This is an inter-document inconsistency where the architecture doc was not updated to match the revised specification.

---

## D3 Feasibility

**Score: 95%**

The architecture fits well within existing project patterns. All proposed dependencies are already in the workspace:
- tokio 1.52 (async runtime, interval_at, spawn_blocking)
- rusqlite (DB migrations, user_version pragma)
- humantime-serde (Duration deserialization)
- tonic/prost (protobuf, StreamUpdates extension)
- tokio-util (CancellationToken)
- sanitize-filename (filename derivation)
- indoc (test multiline strings)

The global-interval wake-and-check pattern is proven and appropriate for 5-50 podcasts. The proto extension follows the existing UpdatePlaylist sub-message pattern exactly. No circular dependencies or external dependency risks identified.

The phased approach with clear prerequisites is architecturally sound and each phase leaves the system in a compilable, backward-compatible state.

---

## D4 Testability

**Score: 95%**

Testing strategy is comprehensive with clear unit/integration/E2E separation (Section 6). BDD scenarios map to concrete test approaches in Section 6.4. The TestHarness builder pattern provides good infrastructure for test isolation.

All ACs have measurable pass/fail criteria. Performance constraints are appropriately qualitative for this domain (podcast sync latency is not a hard constraint). AC-30 (nesting depth) is acknowledged as a code-review verification, which is appropriate.

The spy-channel approach for verifying observable outcomes (AC-27, SCENARIO-032) is well-designed and testable.

---

## D5 Traceability

**Score: 98%**

Strong traceability chains exist:
- AC to spec section: Complete (all 31 ACs mapped, Section 6.4 and Appendix tables)
- SCENARIO to testing strategy: Complete (Section 6.4 maps all 42 scenarios with test type)
- Spec sections to implementation plan phases: Complete (Phase Numbering Mapping table resolves offset)
- Implementation plan to task list: Complete (all 47 tasks have phase assignments, file references, and AC/SCENARIO cross-references)
- Task dependencies: Well-defined (T-01 through T-47 with explicit depends-on chains)

No orphan tasks or broken chains identified.

---

## D6 Grounding

**Score: 97%**

Verified 29 of 30 codebase references against actual files:

**Verified correct (sample):**
- `playback/src/lib.rs` — PlayerCmd enum at line 104 (CORRECT)
- `lib/src/config/v2/server/mod.rs` — ServerSettings.synchronization field (CONFIRMED)
- `lib/src/config/v2/server/synchronization.rs` — SynchronizationSettings with enable/interval/refresh_on_startup (CONFIRMED)
- `lib/proto/player.proto` — StreamUpdates oneof fields 1-8, field 9 available (CONFIRMED)
- `lib/src/podcast/db/migrations/001.sql` — exists (CONFIRMED)
- `lib/src/podcast/db/migration.rs` — DB_VERSION = 1, user_version-based migration (CONFIRMED)
- `lib/src/taskpool.rs` — TaskPool::new(n_tasks: usize) (CONFIRMED)
- `lib/src/player.rs:403` — PlaylistTrackSource::PodcastUrl (CONFIRMED)
- `lib/src/player.rs:454` — AT_END = u64::MAX (CONFIRMED)
- `lib/src/player.rs:457` — new_single(at_index: u64, track: PlaylistTrackSource) (CONFIRMED)
- `lib/src/player.rs:471` — new_append_single(track) (CONFIRMED)
- `lib/src/utils.rs:111` — create_podcast_dir(config: &ServerOverlay, pod_title: String) -> Result<PathBuf> (CONFIRMED)
- `sanitize-filename` workspace dependency (CONFIRMED)
- `humantime-serde` workspace dependency (CONFIRMED)
- `indoc` workspace dependency (CONFIRMED)
- `EpisodeDB.played: bool` in episode_db.rs (CONFIRMED)
- `EpisodeDB.pubdate: Option<DateTime<Utc>>` (CONFIRMED)
- `tui/src/ui/components/podcast.rs` — has check_feed()/download_list() calls (CONFIRMED)
- `server/src/podcast_sync.rs` — sync_once, start_podcast_sync_task (CONFIRMED)
- `SharedServerSettings = Arc<RwLock<ServerOverlay>>` (CONFIRMED)

**One unverifiable reference:**
- Spec Section 5.7 references `should_download_episode` taking `episode: &EpisodeDB` — the design uses EpisodeDB which has `played` but no `path` field. The spec correctly handles this by using the pre-scanned HashSet for file-existence detection. However, the actual sync workflow will need episodes from `get_episodes()` (which returns `Vec<Episode>` with path info via JOIN) or from feed results. The spec should clarify which episode source provides the `played` field for the filter — this is a minor design ambiguity, not a grounding error.

**Previously broken references now fixed:**
- T-01/T-02/T-03 now correctly reference `playback/src/lib.rs` (was `lib/src/player.rs`)
- create_podcast_dir usage now shows correct signature and call pattern
- new_single parameter order correctly documented (at_index first)

---

## D7 Complexity

**Score: 92%**

The spec proposes 47 tasks across 5 implementation phases for a large-scope refactoring effort addressing 59 reviewer comments. This is proportionate to scope.

The phased decomposition is well-justified with clear dependencies. No unnecessary abstractions are introduced. The AutoEnqueue enum with two variants (Enabled/Disabled) is appropriately simple.

One minor complexity concern: Phase 4 consolidates 7 distinct test-cleanup activities into 2 tasks (T-44 and T-45). T-45 in particular combines TestHarness creation, URL replacement, error assertion fixes, indoc usage, and name abbreviation replacement into a single "large effort" task. This is acceptable for implementation planning but may warrant further decomposition during execution.

---

## D8 Ambiguity

**Score: 92%**

API schemas are fully defined with input/output types and error cases (Sections 4.1-4.4). The protobuf definition is complete and unambiguous. State transitions for the sync loop are explicit (Section 5.11). Error responses are specified per boundary.

The spec clearly defines:
- When sync is disabled (interval = Duration::ZERO or absent config)
- Episode filtering logic (4 states: played/unplayed x file-exists/file-deleted)
- Enqueue ordering (oldest-first within per-podcast groups)
- Error isolation (per-podcast, per-episode boundaries)
- Pre-scan timing (before async loop, via spawn_blocking)

Minor remaining ambiguity: The spec does not specify what `MissedTickBehavior::Delay` means for the at-most-once guarantee (SCENARIO-038) when a sync pass takes longer than the interval. The code sets this behavior at Section 5.11, and the semantics are that the next tick is delayed (not skipped), but this could be stated more explicitly for implementers unfamiliar with tokio's interval semantics.

---

## Coverage Matrix

### AC to Spec Section

| AC-ID | Spec Section | Status |
|-------|-------------|--------|
| AC-01 | 2.1, 5.1 | Covered |
| AC-02 | 2.1, 5.1 | Covered |
| AC-03 | 2.1 | Covered |
| AC-04 | 3.2, 5.2 | Covered |
| AC-05 | 3.1 (interval=ZERO default) | Covered |
| AC-06 | 3.1 (refresh_on_startup=false) | Covered |
| AC-07 | Task T-13 | Covered (process constraint) |
| AC-08 | 3.3, 4.2, 4.3 | Covered |
| AC-09 | 3.3, 4.3, 5.4 | Covered |
| AC-10 | 5.6 | Covered |
| AC-11 | 3.1, 5.10 | Covered |
| AC-12 | 5.10 | Covered |
| AC-13 | 5.7 | Covered |
| AC-14 | 5.8 | Covered |
| AC-15 | 5.5 | Covered |
| AC-16 | 5.6 | Covered |
| AC-17 | 5.9 | Covered |
| AC-18 | 5.8 | Covered |
| AC-19 | 5.11 | Covered |
| AC-20 | 6.1 | Covered |
| AC-21 | 6.1 | Covered |
| AC-22 | 6.1 | Covered |
| AC-23 | 6.1 | Covered |
| AC-24 | 6.1 | Covered |
| AC-25 | 6.1 | Covered |
| AC-26 | 6.1 | Covered |
| AC-27 | 6.2 | Covered |
| AC-28 | Impl Plan cross-cutting | Covered |
| AC-29 | 6.1, Phase 5 | Covered |
| AC-30 | 5.13, Phase 5 | Covered |
| AC-31 | Phase 5 | Covered |

---

## Findings

### Finding 1: Architecture Document Stale Constant Table

- **Severity**: medium
- **Section**: Architecture doc (07-architecture.md), Numeric Constants table (line 639)
- **Issue**: The architecture document's "Numeric Constants" table lists "Default sync interval | 3600s (1 hour) | SynchronizationSettings::default()" but the specification correctly defines the default interval as Duration::ZERO (disabled). The architecture doc was not updated when the specification resolved the first review's Finding 1. An implementer consulting the architecture doc's constants table could be misled about the intended default behavior.
- **Recommendation**: Update the architecture document's Numeric Constants table entry for "Default sync interval" from "3600s (1 hour)" to "0 (disabled)" with rationale "Duration::ZERO means disabled by default for backward compatibility (AC-05)."

### Finding 2: AC-18 Satisfaction Ambiguity

- **Severity**: low
- **Section**: Spec Section 5.8; Task T-43; Requirements AC-18
- **Issue**: AC-18 states "new_append_single/new_append_vec playlist helpers delegate to new_single/new_vec with a sentinel value rather than redefining all fields." The actual existing code at lib/src/player.rs:471-484 constructs `Self { at_index: Self::AT_END, tracks: ... }` directly — it does NOT call `new_single(Self::AT_END, track)`. The spec's Section 5.8 shows the existing code as-is (direct construction). Task T-43 says "confirm existing implementation is correct." This creates ambiguity: does AC-18 require refactoring the code to literally delegate, or is the current pattern (which uses the same fields without duplication) acceptable?
- **Recommendation**: Add a clarifying note in T-43 that the current implementation is accepted as satisfying AC-18's intent (single source of field definitions via AT_END sentinel) even though it does not literally call the `new_single` function. The struct has only 2 fields, so the duplication concern from AC-18 is minimal.

### Finding 3: PodcastDB Visibility Not Addressed

- **Severity**: low
- **Section**: Spec Section 4.3 (get_due_podcasts returns Vec<PodcastDB>)
- **Issue**: The proposed `get_due_podcasts` function returns `Vec<PodcastDB>`, but `PodcastDB` is currently imported with a private `use` in `lib/src/podcast/db/mod.rs` (line 14: `use podcast_db::{PodcastDB, PodcastDBInsertable};`). It is not re-exported as `pub use`. The spec's File Inventory (Files to Modify section) lists `lib/src/podcast/db/mod.rs` for re-exporting `update_last_checked` and `get_due_podcasts` but does not mention making `PodcastDB` itself public. Implementers would hit a visibility error when trying to use the returned type from outside the module.
- **Recommendation**: Add a note in Task T-21 (re-export task) that `PodcastDB` must also be made `pub use` from `lib/src/podcast/db/mod.rs` for the `get_due_podcasts` return type to be usable by `server/src/podcast_sync.rs`.

---

## Anti-Pattern Check

| Anti-Pattern | Status | Notes |
|--------------|--------|-------|
| YAGNI violations | None detected | All modules serve stated ACs |
| Premature optimization | None detected | Design choices match project scale |
| Untestable requirements | None detected | All ACs have verification methods |
| Missing error paths | None detected | Error handling table is comprehensive |
| Gold-plating | None detected | Spec stays within AC boundaries |

---

## Dimension Summary

| Dimension | Score | Notes |
|-----------|-------|-------|
| D1 Completeness | 97% | All ACs and scenarios fully covered |
| D2 Consistency | 95% | Internal contradiction resolved; minor arch doc stale entry |
| D3 Feasibility | 95% | Architecture fits project patterns, all deps verified |
| D4 Testability | 95% | Comprehensive test strategy with clear verification methods |
| D5 Traceability | 98% | Strong cross-references, mapping table resolves phase offset |
| D6 Grounding | 97% | 29/30 references verified against codebase (above 90% threshold) |
| D7 Complexity | 92% | Proportionate to scope; T-45 is large but acceptable |
| D8 Ambiguity | 92% | APIs well-defined; minor MissedTickBehavior semantics gap |

---

## Recommendation

The specification is ready for implementation. The three findings are all low-to-medium severity and will not block a competent implementer:

1. The architecture document stale constant (Finding 1) should be corrected as a housekeeping update but does not affect the spec or implementation plan.
2. AC-18's delegation requirement (Finding 2) is satisfied by the existing code pattern; a clarifying note avoids confusion.
3. PodcastDB visibility (Finding 3) is a minor oversight that will be caught at compile time during implementation.

All critical and high-severity findings from the first review have been addressed. The spec demonstrates strong codebase grounding, complete AC coverage, and clear implementation guidance.
