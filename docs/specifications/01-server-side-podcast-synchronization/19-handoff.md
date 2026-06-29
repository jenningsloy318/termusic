# Handoff: Server-Side Podcast Synchronization

## Metadata

| Field   | Value |
|---------|-------|
| Date    | 2026-06-23 |
| Spec    | specification/01-server-side-podcast-synchronization |
| Status  | Partial -- finalization and commit squash/PR remaining |
| Commits | 5 commits on branch `01-server-side-podcast-synchronization` |
| Tests   | 79 passing (19 config + 20 playlist API + 20 sync logic + 9 lifecycle + 11 integration) |

---

## 1. Objective

Add a server-internal periodic task that refreshes subscribed podcast RSS feeds, downloads new episodes (deduplicated by GUID/URL), and appends them to the play queue -- enabling fully headless podcast sync independent of the TUI. See `01-requirements.md` Section "Acceptance Criteria" (AC-01 through AC-11).

---

## 2. Progress

| Stage | Status | Notes |
|-------|--------|-------|
| Requirements | done | 11 ACs defined and all satisfied |
| BDD Scenarios | done | 23 scenarios, all covered by tests |
| Research | done | 2 deep-research reports + code assessment |
| Specification | done | Architecture + full technical spec |
| Prototype | done | Validated approach in isolation |
| Implementation Plan | done | 5-phase plan, all phases executed |
| Task List | done | 26 tasks (T-01 through T-26), all complete |
| Implementation | done | 5 atomic commits, 79 tests passing, clippy clean |
| Code Review | Approved | 0 findings on 3rd iteration |
| Adversarial Review | PASS | 7 low-severity findings, all acceptable |
| Documentation | done | Spec/requirements/BDD updated to reflect final state |
| Finalization | pending | Handoff written; commit squash and PR not yet done |

---

## 3. Key Decisions

- **Tokio periodic task (Option 1)** -- mirrors proven `start_playlist_save_interval` pattern; reuses `CancellationToken` and `Handle::spawn`.
- **humantime-serde v1.1** -- latest stable; provides `#[serde(with = "humantime_serde")]` for Duration deserialization from strings like `"1h"`.
- **Separate DB connection per sync pass** -- avoids cross-thread sharing of non-Send rusqlite Connection; dropped after each pass.
- **Custom Deserialize impl for SynchronizationSettings** -- dual-path parsing handles both standalone TOML sections and nested struct deserialization. See `09-specification.md` Section 2.2.
- **Sequential per-podcast processing** -- ensures download concurrency never exceeds `concurrent_downloads_max` globally.

---

## 4. Unfinished Items

### P0: Critical

- None.

### P1: Important

- **Finalize commit history** -- decide whether to squash the 5 phase commits into 1 feature commit or keep them atomic per phase before opening a PR.
- **Open PR against `master`** -- branch `01-server-side-podcast-synchronization` is ready but no PR exists yet.

### P2: Nice-to-Have

- **Back-catalog limit** (`max_episodes_per_sync` config field) -- documented in `01-requirements.md` "Open Questions" and adversarial review S-03. Not implemented for MVP.
- **Mid-pass cancellation** -- adversarial review S-01 notes that `sync_once` is not cancellation-aware within a pass. Acceptable for MVP given bounded concurrent downloads.
- **32-bit compile guard** -- adversarial review S-02 notes `usize::try_from(u64::MAX).unwrap()` would panic on 32-bit. Pre-existing pattern; a future `compile_error!` macro could guard this.

---

## 5. Risks and Gotchas

- **Large back-catalog subscription**: A podcast with hundreds of episodes will download all on first sync. Bounded only by `concurrent_downloads_max=3`. Monitor disk usage.
- **SQLite contention**: Sync task opens its own connection. If the player loop is also writing heavily, WAL mode may introduce brief lock waits. Not observed in testing.
- **humantime-serde v1.1 is the correct version**: The implementation plan referenced v0.2 which was wrong. Do not downgrade.
- **Direction NOT worth pursuing**: Shared DB connection between sync task and player -- rejected due to rusqlite Connection being non-Send.

---

## 6. Read These First

1. `specification/01-server-side-podcast-synchronization/09-specification.md` -- full technical design and architecture
2. `server/src/podcast_sync.rs` -- core implementation (task lifecycle + sync_once logic)
3. `lib/src/config/v2/server/synchronization.rs` -- configuration struct with serde
4. `specification/01-server-side-podcast-synchronization/18-adversarial-review-3.md` -- final review findings
5. `specification/01-server-side-podcast-synchronization/01-requirements.md` -- ACs and rationale

---

## 7. Next Steps

1. Run `cd /home/jenningsl/development/osc/terminals/termusic/.worktree/01-server-side-podcast-synchronization && cargo test --workspace` to confirm green build before finalizing.
2. Decide commit strategy: either keep 5 atomic phase commits or squash into a single feature commit. Use `git log --oneline master..HEAD` to review.
3. Open a PR from branch `01-server-side-podcast-synchronization` against `master` with title `feat(server): add periodic podcast synchronization task`. Reference AC-01 through AC-11 in the PR description.
4. After merge, consider filing a follow-up issue for `max_episodes_per_sync` config field (P2 item from adversarial review S-03).
5. Delete the worktree after PR merge: `git worktree remove .worktree/01-server-side-podcast-synchronization`.

---

## AC Coverage Assessment

### ACs met as planned

AC-01, AC-02, AC-03, AC-04, AC-05, AC-06, AC-07, AC-08, AC-09, AC-10, AC-11 -- all 11 acceptance criteria satisfied exactly as specified. No deviations in mechanism or outcome.

### ACs met by alternative mechanism

None.

### ACs superseded

None.
