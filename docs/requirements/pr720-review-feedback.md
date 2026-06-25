# PR #720 Review Feedback — Podcast Synchronization

> Reviewer: **@hasezoey** (Collaborator) · Status: **CHANGES_REQUESTED**
> PR: https://github.com/tramhao/termusic/pull/720
> Date: 2026-06-24

---

## Overall Review Summary

> "Thanks for trying your hand at this feature, but i think the current design is not quite right."

### Major Design Concerns (from review body)

1. **Per-podcast tracking** — Podcasts themselves should store a `last_checked` (or `next_check_at`) date and potentially a `check_interval`, instead of one global interval.
2. **Auto-enqueue behaviour** — The reviewer doesn't like that new episodes get automatically added to the playlist because:
   - They can appear in any order between any podcast that gets synchronized.
   - If an episode is skipped (e.g. network error), subsequent episodes still get appended.
   - There is currently no way to turn this off.
3. **Prerequisite: move existing podcast sync to server** — Before this feature is added, the whole existing podcast sync should be moved from the TUI to the server.
4. **Single global TaskPool** — There should only be one global TaskPool for network (or at least podcast) tasks, sharing `concurrent_downloads_max`.
5. **Commit message style** — The scope format doesn't match what the project uses. See recent commits and [CONTRIBUTING.md](https://github.com/tramhao/termusic/blob/master/CONTRIBUTING.md).

---

## Inline Review Comments (59 total)

### Config: `lib/src/config/v2/server/mod.rs`

| Line | Comment |
|------|---------|
| 36 | "If that is podcast related, it should be under `podcast`" — The `[synchronization]` section should be nested under or merged with the existing `[podcast]` config section. |

### Config: `lib/src/config/v2/server/synchronization.rs`

| Line | Comment |
|------|---------|
| 23 | `enable` + `interval` can be condensed into one field where `interval = 0` means disabled. |
| 34 | `refresh_on_startup` should be disableable. Consider using an enum. |
| 41 | Add a human-readable comment explaining what the default duration value is (e.g. "1 hour"). |
| 142 | "Why is this necessary?" — Questioning the need for a specific implementation. |

### Config Tests: `lib/src/config/v2/server/synchronization_tests.rs`

| Line | Comment |
|------|---------|
| 5 | "What does `AC` stand for?" — Unclear abbreviation in test constants. |
| 27 | Defaults don't need to be repeated in every test. |
| 40 | Use `indoc` crate to make multiline strings indent-friendly. |
| 97 | Two tests could be combined since defaults are already tested above. |
| 162 | Test should verify the string format is `5h15m30s`, not just the total seconds. |
| 180 | Error tests should assert the actual error/message, not just `is_err()`. Applies to all similar tests. |
| 262 | Test is duplicative — same as having the section omitted entirely. |

### Player: `lib/src/player.rs`

| Line | Comment |
|------|---------|
| 485 | `new_append_single`/`new_append_vec` should just delegate to `new_single`/`new_vec` with the sentinel value, instead of redefining everything. Docs should mention they're aliases. |

### Player Tests: `lib/src/player_playlist_add_track_tests.rs`

| Line | Comment |
|------|---------|
| 10 | "What does `AC` and `T` stand for?" — Unclear abbreviations. |
| 76 | Multiple tests could be condensed into one (personal preference). |

### Podcast Sync: `server/src/podcast_sync.rs`

#### Documentation & Style

| Line | Comment |
|------|---------|
| 2 | Use suggested module doc: `//! Podcast synchronization module.\n//! Implements the sync pass logic and task lifecycle for periodic podcast feed refresh and download.` |

#### Architecture & Logic

| Line | Comment |
|------|---------|
| 50 | Should be a `Default` impl instead of a manual constructor. |
| 68 | "This line could be inlined." |
| 134 | Use existing `lib::utils::create_podcast_dir` function (though it's sync). |
| 142 | Should also filter out already-played (and locally deleted) episodes. |
| 142 | The `max_new_episodes` limit should be applied here (early). |
| 173 | **BLOCKING read in async** — The read should happen only once before the loop, not every iteration. Also it's a sync (blocking) operation inside an async block. |
| 173 | Use `Result::and_then` / `Option::and_then` instead of converting between types. |
| 173 | **Use `tokio::fs::read_dir`** — `std::fs::read_dir` wrapped in `tokio::spawn_blocking` is unnecessary when async `tokio::fs::read_dir` exists for this exact purpose. |
| 195 | **WRONG**: Even if the episode is downloaded, it needs to be `PlaylistTrackSource::PodcastUrl` with the episode URL. |
| 215 | Reuse existing `ep` and modify it instead of repeating all fields. |
| 226 | There should only be one task pool that shares `concurrent_downloads_max`. |
| 257 | **WRONG** (repeated): Even if downloaded, must use `PlaylistTrackSource::PodcastUrl` with the episode URL. |
| 284 | Download should be a separate task instead of blocking the podcast feed update receiver. |
| 291 | Too much nesting — extract to a separate function. |
| 309 | Consistency suggestion: reformat the warn/counter block. |
| 334 | Function arguments are too cluttered — import shared types instead of passing everything individually. |
| 382 | `refresh_on_startup` + periodic loop can be combined by adjusting `interval_at` start time to be immediate. |
| 429 | Formatting suggestion for `UnboundedReceiver` type. |

#### Tests (server/src/podcast_sync.rs)

| Line | Comment |
|------|---------|
| 459 | "Tests basic Rust functionality, which is unnecessary." |
| 504 | "Should be a helper factory." |
| 553 | Use `localhost` in test URLs to prevent unwanted calls to outside sources. |
| 808 | "Tests basic Rust functionality, unnecessary." (no function calls/constructors involved) |
| 854 | "Test does nothing (does NOT send commands), so the test name is misleading." |
| 868 | "Not using `sync_once`, name and description misleading. Also duplicate of test in another file." |
| 882 | "Does not use `sync_once` or anything — practically tests nothing." |
| 919 | "Wasn't this tested already?" |
| 971 | Use `Iter::find` instead of manual loop. |
| 999 | "Does a lot of setup but does not get any different result it wants to test." |
| 1063 | Use existing config test creation facility and only modify the specific value. |
| 1095 | "`sync_once` may read it, but it does not cause anything observable in this test." |
| 1134 | Formatting suggestion for `start_podcast_sync_task` call. |
| 1160 | Formatting suggestion for `start_podcast_sync_task` call. |
| 1211 | "Does not actually test the non-execute of the main function; rest already handled elsewhere." |
| 1307 | "Should be in the `= true` case test as it is an empty database." |
| 1361 | "Does not test anything other tests haven't done already (timer drift not actually tested)." |
| 1442 | "Wasn't testing the arguments already a test above?" |
| 1506 | "Wasn't this literally a test above?" |
| 1530 | "If the date is static, it may cause inconsistent sorting (insert order ≠ real behavior)." |
| 1555 | "Static content could be a static slice." |
| 1815 | "Should also have a test that 1 podcast has been checked." |
| 1931 | "Should also test the correct episode is downloaded." |
| 2016 | "Does not actually test anything new, especially not that the player actually starts." |
| 2432 | "Does not test anything new." |
| 2636 | "Should use a spy to confirm it was never hit." |

---

## Action Items (Prioritized)

### P0 — Blocking / Design Changes Required

1. **Move existing podcast sync from TUI to server first** — This is a prerequisite before adding the periodic sync feature.
2. **Per-podcast `last_checked` / `check_interval`** — Redesign to store per-podcast timing instead of one global interval.
3. **Fix `PlaylistTrackSource`** — Must use `PodcastUrl` with episode URL even when file is downloaded (lines 195, 257).
4. **Remove auto-enqueue or make it configurable** — Episodes appearing in arbitrary order and no opt-out is unacceptable.
5. **Single global TaskPool** — Share `concurrent_downloads_max` across all podcast operations.
6. **Fix blocking read in async** — Line 173 reads sync (blocking) in async context and does it every iteration.
7. **Use `tokio::fs::read_dir`** — Replace `std::fs::read_dir` wrapped in `spawn_blocking` with native async `tokio::fs::read_dir`.

### P1 — Architecture / Code Quality

7. **Config should live under `[podcast]` section**, not a separate `[synchronization]` section.
8. **Condense `enable` + `interval`** into single field (`interval = 0` disables).
9. **`new_append_single`/`new_append_vec`** should delegate to existing constructors with sentinel.
10. **Extract nested logic** at line 291 into a separate function.
11. **Download as separate task** — Don't block the feed update receiver (line 284).
12. **Combine startup + periodic** by adjusting `interval_at` start time.
13. **Filter out already-played/deleted episodes** in dedup logic.
14. **Reuse `lib::utils::create_podcast_dir`** instead of reimplementing.

### P2 — Tests

15. **Remove ~20+ redundant/useless tests** that test basic Rust functionality or duplicate other tests.
16. **Use `indoc`** for multiline string literals in tests.
17. **Assert actual error messages**, not just `is_err()`.
18. **Use `localhost`** in test URLs to prevent external calls.
19. **Clarify abbreviations** (`AC`, `T` → use full names).
20. **Use helper factories** for test setup instead of repeating config boilerplate.
21. **Test observable outcomes** — verify correct episodes, correct ordering, actual command sending.

### P3 — Style / Minor

22. **Fix commit message scope** per CONTRIBUTING.md conventions.
23. **Add human-readable comments** for duration defaults.
24. **Use `Iter::find`** instead of manual loop (line 971).
25. **Various formatting suggestions** (code style, inlining single-use variables).

---

## Recommended Next Steps

Given the reviewer's feedback, particularly the requirement that **existing podcast sync should move from TUI to server first**, the recommended path is:

1. **Close or significantly redesign this PR.**
2. **Phase 1** — Move existing TUI podcast sync logic into the server crate (new prerequisite PR).
3. **Phase 2** — Redesign the periodic sync with:
   - Per-podcast `last_checked`/`next_check_at` fields in the podcast DB.
   - Optional per-podcast `check_interval` overrides.
   - Configurable enqueue behavior (opt-in, ordering guarantees).
   - Shared task pool with existing download infrastructure.
4. **Phase 3** — Implement the redesigned periodic sync (much smaller diff after Phase 1).
