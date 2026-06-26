# Handoff: Async TUI Playlist Loading

<document type="handoff">

<metadata>
  <field name="title">Handoff: Async TUI Playlist Loading</field>
  <field name="date">2026-06-27</field>
  <field name="spec">specification/05-async-tui-playlist-loading</field>
  <field name="status">Partial -- commit stage pending</field>
  <field name="commits">4 commits on branch 05-async-tui-playlist-loading</field>
</metadata>

## 1. Objective

Eliminate the multi-second TUI freeze during playlist loading by extending the gRPC protocol with full display metadata so the TUI constructs Track objects from server-provided data without any disk I/O. "Done" = commit merged to master, TUI loads 1000-track playlists in <50ms with zero filesystem access.

AC reference: See `01-requirements.md` Acceptance Criteria section (AC-01 through AC-10).

## 2. Progress

| Stage | Status | Notes |
|-------|--------|-------|
| Requirements & BDD | done | 10 ACs, 20+ BDD scenarios |
| Research | done | 3 reports (codebase, proto patterns, test strategies) |
| Specification | done | `07-specification.md` |
| Architecture | done | Protocol seam deepening approach |
| Prototype | done | Validated sentinel PathBuf pattern |
| Implementation | done | 4 phases, 35/35 tasks, 676 tests pass |
| Code Review | Approved | 0 critical/high/medium, 2 low |
| Adversarial Review | PASS | 0 critical/high/medium, 4 low |
| Documentation | done | All docs updated |
| Validation | done | All tests pass, clippy clean |
| **Commit** | **pending** | Final stage — needs commit + PR |

## 3. Key Decisions

- **Protocol seam deepening over async spawn**: Extended gRPC wire format with 3 optional fields (artist, album, has_local_file) rather than offloading disk I/O to background tasks -- eliminates I/O entirely instead of just moving it
- **Sentinel PathBuf for has_local_file**: PodcastTrackData stores an empty PathBuf when has_local_file=true -- downstream code only calls `is_some()` on it (verified safe, see `03-research-report.md` SRC-028/031)
- **LoadStats return type**: `load_from_grpc` returns timing/count stats for testability and future observability -- currently unused at call sites (low-priority finding)
- **Dead code removal**: Removed `add_tracks`, `track_from_path`, `track_from_podcasturi` from TUI -- zero callers after rewrite

See `07-specification.md` Section 2 for full architecture rationale.

## 4. Unfinished Items

### P0: Critical
- **Commit and PR creation** -- All code is implemented and reviewed but the commit stage has not been executed. The 4 existing commits on the branch need to be squashed or a PR opened against master.

### P1: Important
- None. All 10 ACs are satisfied. All review findings are low-severity code hygiene.

### P2: Nice-to-Have
- **Log LoadStats at call sites** -- `update.rs:1129` and `playlist.rs:520` discard the LoadStats return value; spec Section 7.3 wants INFO-level timing logs. See adversarial review S-01.
- **Remove stale `#[allow(dead_code)]` on `insert_track_at`** -- `tui/src/ui/model/playlist.rs:24` has annotation but method is actively called. See adversarial review S-02.
- **Normalize has_local_file serialization** -- Minor asymmetry between bulk vs stream paths (both deserialize correctly). See adversarial review A-01.

## 5. Risks and Gotchas

- **Sentinel PathBuf**: The empty PathBuf in PodcastTrackData must never be opened/read -- only checked via `is_some()`. If future code adds `File::open` on that path, it will fail silently or error. Document this invariant.
- **Proto backward compatibility**: Fields 5,6,7 are optional -- older servers omit them and TUI falls back to filename-derived titles. Do not make these fields required.
- **Worktree location**: Code lives in `.worktree/05-async-tui-playlist-loading`, not the main repo root.

Directions NOT worth pursuing: Async spawn approach for TUI-side metadata loading was considered and rejected -- adds complexity without eliminating I/O (see `03-research-report.md` Section 3).

## 6. Read These First

1. `specification/05-async-tui-playlist-loading/07-specification.md` -- Full technical design and AC mapping
2. `specification/05-async-tui-playlist-loading/11-implementation-summary.md` -- Phase-by-phase change log
3. `specification/05-async-tui-playlist-loading/12-adversarial-review-2.md` -- Final review with 4 low findings
4. `specification/05-async-tui-playlist-loading/08-implementation-plan.md` -- Phase breakdown and file targets
5. `specification/05-async-tui-playlist-loading/05-async-tui-playlist-loading-workflow-tracking.json` -- Machine-readable status

## 7. Next Steps

1. Run `cd /home/jenningsl/development/osc/terminals/termusic/.worktree/05-async-tui-playlist-loading && cargo test --workspace` to confirm all 676 tests still pass
2. Generate commit message using `@.claude/skills/generating-commit-messages` and create a single squash commit (or open PR with the 4 phase commits as-is)
3. Open a PR against `master` with title "feat: async TUI playlist loading -- eliminate disk I/O during playlist sync" referencing spec-05
4. Optionally address P2 items (LoadStats logging, dead_code annotation removal) in a follow-up commit before merging
5. After merge, delete worktree: `git worktree remove .worktree/05-async-tui-playlist-loading`

</document>

## AC Coverage Assessment

### ACs met as planned
AC-01, AC-02, AC-03, AC-04, AC-05, AC-06, AC-07, AC-08, AC-09, AC-10 -- all 10 acceptance criteria satisfied exactly as described in the specification. No pivots occurred, no alternative mechanisms needed.

### ACs met by alternative mechanism
None.

### ACs superseded
None.
