# Requirements: PR #720 Podcast Synchronization — Review Feedback Remediation

- **Date**: 2026-06-25
- **Author**: super-dev:requirements-clarifier
- **Type**: refactor
- **Priority**: high
- **Status**: draft

---

## Executive Summary

PR #720 introduced periodic podcast synchronization for termusic but received substantial review feedback (59 inline comments, CHANGES_REQUESTED status) from collaborator @hasezoey. The core concerns are architectural: the sync config placement is wrong, podcast tracking should be per-podcast rather than global, auto-enqueue behavior lacks user control, blocking I/O exists in async context, and the existing TUI podcast sync must migrate to the server before this feature can land. This requirements document captures all reviewer feedback as actionable acceptance criteria for a redesigned implementation.

## The Real Need (Root Cause Analysis)

### Surface Request

Address all review feedback on PR #720 to bring the podcast synchronization feature up to project standards and gain reviewer approval.

### 5 Whys Analysis

1. **Why**: PR #720 was rejected with CHANGES_REQUESTED — the design and implementation do not meet project standards.
2. **Why**: The feature was built on an incomplete foundation — existing podcast sync lives in the TUI, but the new periodic sync was added to the server without first migrating the base logic.
3. **Why**: The architecture grew organically from the TUI side, and adding server-side automation on top of a TUI-owned workflow creates duplication and inconsistency.
4. **Why**: There was no phased design plan that established prerequisites (server migration) before building the periodic layer.
5. **Why**: The root cause is attempting to build an advanced feature (periodic sync) before the foundational architecture (server-owned podcast operations) is in place.

### Job to Be Done

When I subscribe to podcasts and want new episodes automatically
I want the server to periodically check feeds, download new episodes, and optionally enqueue them
So I can listen to new content without manual intervention while maintaining control over what enters my playlist.

- **Functional**: Automatic periodic feed checking, downloading, and playlist management
- **Emotional**: Confidence that the system works reliably in the background without surprising behavior
- **Social**: Clean, reviewable code that passes collaborator review and aligns with project conventions

## Stakeholders

- **End User**: Wants reliable, configurable background podcast sync without unexpected playlist disruption
- **Reviewer (@hasezoey)**: Requires architectural correctness, code quality, test quality, and adherence to project conventions
- **Maintainer (@tramhao)**: Needs the feature to integrate cleanly with existing architecture without creating tech debt

## Workflow Context

### Before (Current State)

- Podcast sync (feed refresh) is triggered manually from the TUI
- PR #720 adds periodic sync in the server but: uses a global interval for all podcasts, auto-enqueues in arbitrary order with no opt-out, creates per-sync TaskPools instead of sharing the global one, places config in a top-level `[synchronization]` section instead of under `[podcast]`, and has blocking filesystem reads inside async code
- 20+ tests are redundant or test basic Rust functionality rather than meaningful behavior

### After (Desired State)

- Existing podcast sync logic lives entirely in the server crate (prerequisite migration complete)
- Per-podcast `last_checked`/`next_check_at` tracking with optional per-podcast `check_interval` override
- Configurable enqueue behavior: opt-in, with ordering guarantees (per-podcast chronological)
- Single shared TaskPool for all podcast network operations
- Config nested under `[podcast.synchronization]`
- Zero blocking I/O in async contexts
- Clean, non-redundant test suite with meaningful assertions

## Solution Options

### Option 1: Phased Redesign (Recommended)

Decompose into 3 phases: (1) migrate existing TUI sync to server, (2) redesign periodic sync architecture with per-podcast tracking and configurable enqueue, (3) implement the periodic sync on the solid foundation.

- **Pros**: Each phase is independently reviewable; foundation is solid before automation is layered on; smaller diffs per PR
- **Cons**: Takes longer to deliver the full feature; requires 3 separate PRs
- **Effort**: high (total), but each phase is medium individually

### Option 2: Single PR Rework

Address all 59 comments in one large PR rewrite.

- **Pros**: Feature ships in one pass
- **Cons**: Massive diff; hard to review; high risk of additional rounds of feedback; mixes prerequisite migration with new feature work
- **Effort**: high

## Acceptance Criteria

### Phase 0: Prerequisites and Migration

- **AC-01**: All existing podcast sync logic (feed refresh, download dispatch) is moved from the TUI crate to the server crate, with the TUI calling the server via the existing gRPC/UDS communication layer.
- **AC-02**: After migration, the TUI contains zero direct calls to `check_feed()` or `download_list()` — all podcast network operations route through the server.
- **AC-03**: Existing podcast functionality (manual refresh, manual download, OPML import/export) continues to work identically from the user's perspective after migration.

### Phase 1: Architecture and Config Redesign

- **AC-04**: The `[synchronization]` config section is removed from the top level and its settings are nested under `[podcast]` (e.g., `[podcast.synchronization]` or merged into `PodcastSettings`).
- **AC-05**: The `enable` + `interval` fields are condensed into a single field where `interval = 0` (or absence) means disabled, eliminating the boolean flag.
- **AC-06**: `refresh_on_startup` is represented as an enum or optional value that allows disabling (not just a hardcoded `true` default).
- **AC-07**: Duration default values include a human-readable comment in the source (e.g., `// 1 hour`).
- **AC-08**: Each podcast stores a `last_checked` (or `next_check_at`) timestamp in the database, enabling per-podcast scheduling.
- **AC-09**: An optional per-podcast `check_interval` override field exists in the podcast DB schema, falling back to the global interval when unset.
- **AC-10**: A single global `TaskPool` is used for all podcast network operations (feed fetches AND downloads), sharing `concurrent_downloads_max` from `PodcastSettings`.

### Phase 2: Sync Logic Correctness

- **AC-11**: Auto-enqueue behavior is configurable — users can disable automatic playlist addition of new episodes entirely.
- **AC-12**: When enqueue is enabled, episodes from the same podcast are added in chronological order (oldest first), and episodes from different podcasts do not interleave arbitrarily.
- **AC-13**: Episodes already played AND whose local file was deleted are excluded from the sync pass (not re-downloaded).
- **AC-14**: `PlaylistTrackSource::PodcastUrl` with the episode URL is used for enqueued tracks even when the file is already downloaded locally — never `PlaylistTrackSource::Path` alone for podcast episodes.
- **AC-15**: No blocking (synchronous) filesystem operations occur inside async task contexts. The `read_dir` for existing-file detection happens once before the async loop, not inside it.
- **AC-16**: Download operations run as separate tasks and do not block the feed update receiver channel processing.
- **AC-17**: `lib::utils::create_podcast_dir` (or equivalent existing utility) is reused for directory creation instead of reimplementing.
- **AC-18**: The `new_append_single`/`new_append_vec` playlist helpers delegate to `new_single`/`new_vec` with a sentinel value rather than redefining all fields.
- **AC-19**: The `refresh_on_startup` + periodic loop logic is combined by adjusting `interval_at` start time to `Instant::now()` (immediate first tick) rather than maintaining two separate code paths.

### Phase 3: Test Quality

- **AC-20**: All tests that merely verify basic Rust language semantics (Option/Result behavior, Vec operations) are removed.
- **AC-21**: Duplicate tests (same assertion, same setup, different name) are consolidated.
- **AC-22**: Test URLs use `localhost` or `127.0.0.1` to prevent any network calls to external hosts.
- **AC-23**: Error assertion tests check the actual error variant or message string, not just `is_err()`.
- **AC-24**: Multiline string literals in tests use the `indoc` crate for readability.
- **AC-25**: Abbreviations in test names and constants are replaced with full descriptive names (no unexplained `AC`, `T`).
- **AC-26**: Test helper factories exist for common setup (config creation, episode creation, podcast creation) — no boilerplate repetition across tests.
- **AC-27**: Tests verify observable outcomes: correct episodes downloaded, correct ordering, actual command sending confirmed via spy/mock.

### Phase 4: Style and Conventions

- **AC-28**: All commit messages follow the project's scope format as documented in CONTRIBUTING.md (reference: recent commits like `fix(podcast-sync):`, `feat(server):`).
- **AC-29**: Module documentation uses `//!` doc comments with the approved format from the review.
- **AC-30**: Deeply nested logic (3+ levels) is extracted into named helper functions.
- **AC-31**: Function signatures that accept many individual config values are refactored to accept shared types or config struct references.

## Non-Functional Requirements

- **Performance** (high): Sync operations must not block the player event loop or audio playback. The single shared TaskPool with `concurrent_downloads_max` bounds ensures bounded resource usage. Filesystem scanning for existing files must happen outside async contexts.
- **Security** (low): No new attack surface introduced — all network operations use existing `reqwest` client with timeouts. Test URLs must not leak to external hosts.
- **Accessibility** (low): Not applicable — this is a background server operation with no direct UI interaction beyond configuration.
- **Reliability** (high): Per-podcast error isolation — one failed feed must not abort the entire sync pass. Network errors must be logged and retried per existing retry policy. Partial failures (some episodes fail, some succeed) must not corrupt database state.

## Open Questions

- Should the per-podcast `check_interval` override be exposed in the TOML config file, or only manageable via a future CLI/TUI command?
- What is the correct behavior when a podcast feed returns fewer episodes than previously known (feed truncation)? Should those episodes be marked as unavailable?
- Should the enqueue behavior support a "download only, never enqueue" mode in addition to "download and enqueue" and "disabled"?
- What is the ordering guarantee when multiple podcasts have new episodes in the same sync pass? Per-podcast groups in subscription order, or interleaved by pubdate?
- Is there an existing migration mechanism for the podcast SQLite database, or does the `last_checked`/`check_interval` schema change need a new migration file in `lib/src/podcast/db/migrations/`?

## Recommendations

1. **Phase the work into 3 separate PRs**: The reviewer explicitly stated that moving existing podcast sync to the server is a prerequisite. Attempting to do everything in one PR will result in another CHANGES_REQUESTED round. Phase 1 (migration) can land independently and provides value by itself.
2. **Start with the database schema change**: Adding `last_checked` and `check_interval` columns to the podcast table (via a new `002.sql` migration) is a small, safe change that unblocks the per-podcast scheduling design.
3. **Use `PlaylistTrackSource::PodcastUrl` consistently**: This was flagged twice as WRONG in the review. Podcast episodes must always carry their feed URL for proper resume/re-download behavior, even when a local file exists.
4. **Audit the test suite with a deletion-first pass**: Remove the ~20 redundant tests before writing new ones. A smaller, meaningful test suite is easier to review and maintain.

## Assumptions

- The existing `TaskPool` implementation in `lib/src/taskpool.rs` can be shared across multiple callers (feed fetch + download) without modification.
- The podcast SQLite database schema supports migrations (evidence: `lib/src/podcast/db/migrations/001.sql` exists).
- The gRPC/UDS communication layer between TUI and server already supports sending podcast-related commands (at least partially, given existing `PlayerCmd` variants).
- The reviewer will re-review after the prerequisite migration PR lands — the periodic sync PR should not be submitted until Phase 1 is merged.
- `concurrent_downloads_max` in `PodcastSettings` is the single source of truth for bounding all podcast network concurrency.
