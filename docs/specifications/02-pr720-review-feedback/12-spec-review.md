# Specification Review: PR #720 Podcast Synchronization — Review Feedback Remediation

- **Date**: 2026-06-25
- **Reviewer**: spec-inspector
- **Specification**: ./09-specification.md
- **Implementation Plan**: ./10-implementation-plan.md
- **Task List**: ./11-task-list.md
- **Requirements**: ./01-requirements.md
- **BDD Scenarios**: ./02-bdd-scenarios.md
- **Architecture**: ./07-architecture.md

---

## Verdict: REVISIONS NEEDED

The specification is comprehensive, well-structured, and demonstrates strong traceability between requirements, BDD scenarios, and implementation tasks. However, it contains several grounding errors (wrong file paths, incorrect function signatures, wrong parameter order), a critical internal contradiction about default configuration behavior, and a task count inconsistency. These issues would cause implementation confusion if not corrected.

---

## Quantitative Summary

| Metric | Value |
|--------|-------|
| ACs Addressed | 31/31 (100%) |
| BDD Scenarios Covered | 42/42 (100%) |
| Grounding Score | 87% (26/30 verified references) |
| Traceability Completeness | 95% |
| Critical Findings | 0 |
| High Findings | 3 |
| Medium Findings | 3 |
| Low Findings | 1 |

---

## D1 Completeness

**Score: 95%**

All 31 acceptance criteria have corresponding spec sections. All 42 BDD scenarios are mapped to testing strategy entries. Error handling is specified per-boundary (Section 8, Architecture Error Handling Strategy table). NFRs are covered in Section 7 (Performance, Reliability, Backward Compatibility, Resource Usage).

The only gap is AC-07 (human-readable comments on Duration defaults), which is mentioned in the implementation plan (Phase 2, Task T-15) but has no explicit spec section — this is acceptable as it is a source-code style constraint, not a behavioral requirement.

---

## D2 Consistency

**Score: 85%**

**Finding 1 (High)**: The specification has an internal contradiction about the default behavior when the `[podcast.synchronization]` section is absent from config:

- Section 3.1 defines the default `interval` as 3600s (1 hour), which means sync is ENABLED.
- Section 7.3 (Backward Compatibility) states: "existing config files without `[podcast.synchronization]` section get defaults (sync disabled)".
- SCENARIO-008 states: "Absent interval setting disables periodic sync."
- AC-05 says: "interval = 0 (or absence) means disabled."

These statements cannot all be true simultaneously. If the Default impl sets interval to 3600s, then absence means enabled (via serde default). If absence should mean disabled, then the Default impl must set interval to Duration::ZERO.

**Finding 2 (Medium)**: Phase numbering is inconsistent across documents:
- Requirements and BDD use "Phase 0, 1, 2, 3, 4"
- Implementation Plan uses "Phase 1, 2, 3, 4, 5, 6, 7" (where impl Phase 1 = spec Phase 0)
- The spec itself uses "Phase 0, 1, 2, 3, 4" in section headings

This could confuse implementers tracking work across documents.

**Finding 3 (Low)**: Task list header states "Total Tasks: 42" but the summary and actual task count is 52.

---

## D3 Feasibility

**Score: 92%**

The architecture fits well within existing project patterns. All proposed dependencies (tokio, rusqlite, humantime-serde, tonic/prost, tokio-util) are already in the workspace. The global-interval wake-and-check pattern is sound for the expected scale (5-50 podcasts). The proto extension using the existing UpdatePlaylist sub-message pattern is clean.

The `indoc` crate is already a workspace dependency (v2.0.7), confirming feasibility of Phase 6 test cleanup.

No circular dependency risks identified. The phased approach with clear prerequisites is architecturally sound.

---

## D4 Testability

**Score: 93%**

Testing strategy is comprehensive with clear unit/integration/E2E separation. BDD scenarios map to concrete test approaches. Performance thresholds are not numeric (acceptable for this domain — podcast sync latency is not a hard constraint). The TestHarness builder pattern in Phase 6 provides good infrastructure for test isolation.

All ACs have measurable pass/fail criteria. The weakest area is AC-30 (nesting depth <= 3) which requires manual inspection or custom lint — but this is acknowledged in the BDD document as a code-review verification.

---

## D5 Traceability

**Score: 95%**

Strong traceability chains exist:
- AC → spec section: Complete (all 31 ACs mapped in Section 2 references)
- SCENARIO → testing strategy: Complete (Section 6.4 maps all 42 scenarios)
- Spec sections → implementation plan phases: Complete
- Implementation plan → task list: Complete (all phases decomposed into tasks)

The one gap: the implementation plan's phase numbering offset (1-7 vs 0-4) requires mental mapping but does not break traceability since task IDs (T-01 through T-52) are unambiguous.

---

## D6 Grounding

**Score: 87%**

Verified 26 of 30 codebase references. Four grounding errors found:

**Finding 4 (High)**: Tasks T-01, T-02, T-03 specify modifying `lib/src/player.rs` to add `PlayerCmd` variants. However, `PlayerCmd` is defined in `playback/src/lib.rs` (line 104), not `lib/src/player.rs`. The `lib/src/player.rs` file contains `UpdateEvents`, `PlaylistAddTrack`, and related types — but NOT `PlayerCmd` or `PlayerCmdSender`. This wrong file reference appears in the task list (Phase 1) and spec section 5.1.

**Finding 5 (High)**: Spec section 5.11 shows `create_podcast_dir` called as `create_podcast_dir(&podcast.title, &base_download_dir)`, but the actual function signature is `pub fn create_podcast_dir(config: &ServerOverlay, pod_title: String) -> Result<PathBuf>`. The function takes the full `ServerOverlay` config reference (to derive the download path internally), not a base directory path. The spec's proposed usage would not compile.

**Finding 6 (Medium)**: Spec section 5.10 shows `PlaylistAddTrack::new_single(source, AT_END)` but the actual function signature is `pub fn new_single(at_index: u64, track: PlaylistTrackSource)` — parameters are in reverse order. Correct call would be `Self::new_single(Self::AT_END, track)`.

**Finding 7 (Medium)**: Spec section 5.8 (`should_download_episode`) references `episode.path` as a field on `EpisodeDB`. However, `EpisodeDB` does not have a `path` field. File paths are stored in a separate `files` table via the `FileDB` struct, accessed through `Database::insert_file`/`remove_file` methods. The episode-file association requires a JOIN query or separate lookup, not a direct field access.

**Verified references (correct)**:
- `lib/src/config/v2/server/mod.rs` — ServerSettings with `synchronization` field on top level (confirmed)
- `lib/src/config/v2/server/synchronization.rs` — SynchronizationSettings with `enable` boolean (confirmed)
- `lib/proto/player.proto` — StreamUpdates oneof with fields 1-8 (confirmed, field 9 available)
- `lib/src/taskpool.rs` — TaskPool::new(n_tasks: usize) (confirmed)
- `lib/src/podcast/db/migrations/001.sql` — exists (confirmed)
- `lib/src/podcast/db/migration.rs` — user_version-based migration, DB_VERSION = 1 (confirmed)
- `PlaylistTrackSource::PodcastUrl` variant (confirmed at player.rs:403)
- `AT_END` constant (confirmed at player.rs:454)
- `humantime-serde` workspace dependency (confirmed)
- `tokio-util` workspace dependency (confirmed)
- `indoc` workspace dependency (confirmed)
- `PodcastDB.last_checked: DateTime<Utc>` (confirmed)
- `PodcastDBId = i64` (confirmed)
- `UpdatePlaylist` sub-message pattern with inner oneof (confirmed)

---

## D7 Complexity

**Score: 90%**

The spec proposes 52 tasks across 7 implementation phases for a large-scope refactoring effort — this is proportionate to addressing 59 reviewer comments spanning architecture, logic, tests, and style.

The phased decomposition is well-justified. No unnecessary abstractions are introduced. The `AutoEnqueue` enum with two variants (Enabled/Disabled) is appropriately simple rather than over-engineered (a bitflag or complex mode enum would be YAGNI).

The single `SyncPassStats` struct is sufficient for progress reporting without introducing unnecessary observer patterns.

---

## D8 Ambiguity

**Score: 88%**

API schemas are fully defined with input/output types and error cases (Sections 4.1-4.5). The protobuf definition is complete and unambiguous. State transitions for the sync loop are explicit (Section 5.9). Error responses are specified per boundary.

Remaining ambiguities:
- The contradictory default interval value (addressed in D2 finding) creates ambiguity about intended behavior for new installations.
- The spec does not explicitly state what happens if `concurrent_downloads_max` is changed via config reload while a sync pass is in progress (minor — current pass would use the old value).

---

## Coverage Matrix

### AC → Spec Section

| AC-ID | Spec Section | Status |
|-------|-------------|--------|
| AC-01 | 2.1 | Covered |
| AC-02 | 2.1, 5.1 | Covered |
| AC-03 | 2.1 | Covered |
| AC-04 | 2.2, 3.2, 5.2 | Covered |
| AC-05 | 2.2, 3.1 | Covered (but default contradicts) |
| AC-06 | 2.2, 3.1, 5.9 | Covered |
| AC-07 | Impl Plan Phase 2 T-15 | Covered (process constraint) |
| AC-08 | 2.3, 3.3, 4.2 | Covered |
| AC-09 | 2.3, 4.3 | Covered |
| AC-10 | 2.4, 5.5 | Covered |
| AC-11 | 2.7, 3.1, 5.7 | Covered |
| AC-12 | 2.7, 5.7 | Covered |
| AC-13 | 5.8 | Covered |
| AC-14 | 2.7, 5.6 | Covered |
| AC-15 | 2.6, 5.4 | Covered |
| AC-16 | 2.7, 5.5 | Covered |
| AC-17 | 5.11 | Covered (wrong signature) |
| AC-18 | 5.10 | Covered (wrong param order) |
| AC-19 | 5.9 | Covered |
| AC-20 | 2.8, 6.1 | Covered |
| AC-21 | 2.8, 6.1 | Covered |
| AC-22 | 2.8, 6.1 | Covered |
| AC-23 | 2.8, 6.1 | Covered |
| AC-24 | 2.8 | Covered |
| AC-25 | 2.8 | Covered |
| AC-26 | 2.8, 6.1 | Covered |
| AC-27 | 2.8, 6.2 | Covered |
| AC-28 | 2.9 | Covered |
| AC-29 | 2.9 | Covered |
| AC-30 | 2.9, 5.9 | Covered |
| AC-31 | 2.9, 4.4 | Covered |

---

## Findings

### Finding 1: Default Interval Contradicts Backward Compatibility Claim

- **Severity**: high
- **Section**: Spec 3.1, 7.3; BDD SCENARIO-008; Requirements AC-05
- **Issue**: The specification defines the default `interval` as 3600s (Section 3.1) but claims that config files without a `[podcast.synchronization]` section will have "sync disabled" (Section 7.3). With `#[serde(default)]` and a 3600s default, absent config means sync is ENABLED with a 1-hour interval. This directly contradicts SCENARIO-008 and AC-05's "absence means disabled" requirement.
- **Recommendation**: Change the Default impl to use `Duration::ZERO` for `interval` (making absence = disabled). Or change the backward compatibility claim to state that absent config means sync enabled with 1-hour default. The requirements clearly state absence should mean disabled, so the Default interval should be zero.

### Finding 2: PlayerCmd File Location Wrong in Task List

- **Severity**: high
- **Section**: Task List T-01, T-02, T-03; Spec Section 5.1
- **Issue**: Tasks T-01 through T-03 specify `Files: lib/src/player.rs` for adding `PlayerCmd` variants. However, `PlayerCmd` is defined in `playback/src/lib.rs` (line 104). `lib/src/player.rs` contains `UpdateEvents`, `PlaylistAddTrack`, and playlist helper types — not `PlayerCmd`.
- **Recommendation**: Change the file reference in T-01, T-02, T-03 from `lib/src/player.rs` to `playback/src/lib.rs`.

### Finding 3: create_podcast_dir Signature Mismatch

- **Severity**: high
- **Section**: Spec Section 5.11; Task T-36
- **Issue**: The spec shows `create_podcast_dir(&podcast.title, &base_download_dir)` but the actual function signature is `create_podcast_dir(config: &ServerOverlay, pod_title: String)`. It takes the full config reference (internally deriving the download path), not a base directory argument. Code written to match the spec would not compile.
- **Recommendation**: Update Section 5.11 to show the correct usage: `create_podcast_dir(&config.read(), podcast.title.clone())`. Alternatively, if the design intends to change the function signature for testability, document that as a new task.

### Finding 4: new_single Parameter Order Reversed in Spec

- **Severity**: medium
- **Section**: Spec Section 5.10
- **Issue**: Section 5.10 shows `PlaylistAddTrack::new_single(source, AT_END)` but the actual signature is `new_single(at_index: u64, track: PlaylistTrackSource)` — at_index comes first. The delegation should be `Self::new_single(Self::AT_END, track)`.
- **Recommendation**: Fix the code example in Section 5.10 to use the correct parameter order.

### Finding 5: episode.path Field Does Not Exist on EpisodeDB

- **Severity**: medium
- **Section**: Spec Section 5.8 (should_download_episode)
- **Issue**: The `should_download_episode` function references `episode.path` as a direct field on `EpisodeDB`. However, `EpisodeDB` has no `path` field. File paths are stored in a separate `files` table via the `FileDB` struct. Determining if a file exists for an episode requires either a JOIN query or a separate lookup through `Database::insert_file`/`remove_file` infrastructure.
- **Recommendation**: Redesign the `should_download_episode` filter to work with the pre-scanned `HashSet<String>` of existing filenames (already described in Section 5.4) combined with the episode URL/title for filename derivation, rather than assuming a `.path` field. The function signature should accept the `existing_files: &HashSet<String>` and derive expected filename from episode metadata.

### Finding 6: Phase Numbering Inconsistency Across Documents

- **Severity**: medium
- **Section**: Implementation Plan (all phases); Requirements; BDD Scenarios
- **Issue**: Requirements and BDD use Phase 0-4 numbering. The implementation plan uses Phase 1-7 (with a different decomposition: spec's Phase 1 maps to impl plan's Phases 2+3+4). This creates a naming collision where "Phase 1" means different things in different documents.
- **Recommendation**: Align the implementation plan to use the same phase numbering as requirements/BDD (0-4), or add an explicit mapping table at the top of the implementation plan clarifying the relationship.

### Finding 7: Task Count Header Mismatch

- **Severity**: minor
- **Section**: Task List header
- **Issue**: The task list header states "Total Tasks: 42" but the document contains 52 tasks (T-01 through T-52) and the summary section correctly states 52.
- **Recommendation**: Update the header to "Total Tasks: 52".

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
| D1 Completeness | 95% | All ACs and scenarios covered |
| D2 Consistency | 85% | Default interval contradiction is significant |
| D3 Feasibility | 92% | Architecture fits project patterns well |
| D4 Testability | 93% | Comprehensive test strategy |
| D5 Traceability | 95% | Strong cross-references throughout |
| D6 Grounding | 87% | 4 incorrect codebase references (below 90% threshold) |
| D7 Complexity | 90% | Proportionate to scope |
| D8 Ambiguity | 88% | Default contradiction creates ambiguity |

---

## Recommendation

Address the 3 high-severity findings before implementation begins:
1. Resolve the default interval contradiction (Finding 1) — this is a design decision that affects behavior.
2. Fix the PlayerCmd file location (Finding 2) — implementers will waste time looking in the wrong file.
3. Fix the create_podcast_dir signature (Finding 3) — code examples should compile.

The medium-severity findings (parameter order, missing field, phase numbering) should be corrected but will not block a competent implementer who reads the actual codebase.
