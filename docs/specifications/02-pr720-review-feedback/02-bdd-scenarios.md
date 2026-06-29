# BDD Scenarios: PR #720 Podcast Synchronization — Review Feedback Remediation

- **Feature**: PR #720 Podcast Synchronization — Review Feedback Remediation
- **Date**: 2026-06-25
- **Source**: `specification/02-pr720-review-feedback/01-requirements.md`
- **Quality Score**: quality_score: 8.25, specificity: 8, independence: 9, coverage: 8, testability: 8

---

## Coverage Summary

| Metric | Value |
|--------|-------|
| Total ACs analyzed | 31 |
| ACs with strong coverage (3+ scenarios) | 6 (AC-01, AC-08, AC-11, AC-12, AC-13, AC-15) |
| ACs with adequate coverage (1-2 scenarios) | 19 (AC-02 through AC-07, AC-09, AC-10, AC-14, AC-16 through AC-19, AC-22, AC-23, AC-26, AC-27) |
| ACs with weak coverage (happy path only) | 6 (AC-20, AC-21, AC-24, AC-25, AC-28 through AC-31) |
| Edge cases: null/empty | 3 |
| Edge cases: boundary values | 3 |
| Edge cases: concurrent access | 2 |
| Edge cases: timeout | 1 |
| Edge cases: permission/authorization | 0 |
| Edge cases: data overflow | 1 |
| Edge cases: invalid state transitions | 2 |

**Weak coverage justification**: AC-20, AC-21, AC-24, AC-25, AC-28–AC-31 are code-quality/style constraints that are binary pass/fail. They do not have meaningful error paths or boundary conditions — a test either violates the convention or does not.

---

## Phase 0: Prerequisites and Migration

### SCENARIO-001: Server assumes ownership of feed refresh operations
**Priority**: high
**Refs**: AC-01

**Given** podcast sync logic (feed refresh, download dispatch) resides in the TUI crate
**When** the migration is complete
**Then** all feed refresh logic executes within the server crate
**And** the server exposes podcast sync operations through the existing communication layer

### SCENARIO-002: TUI delegates all podcast network operations to server
**Priority**: high
**Refs**: AC-02

**Given** the server owns all podcast sync operations
**When** the TUI initiates any podcast network action (refresh, download)
**Then** the TUI sends a command via the communication layer to the server
**And** the TUI contains zero direct invocations of feed-check or download-list functions

### SCENARIO-003: Manual podcast refresh works identically after migration
**Priority**: high
**Refs**: AC-03

**Given** a user with subscribed podcasts
**When** the user triggers a manual feed refresh after migration
**Then** the refresh results (new episodes discovered, metadata updated) are identical to pre-migration behavior

### SCENARIO-004: OPML import and export remain functional after migration
**Priority**: medium
**Refs**: AC-03

**Given** a user with an OPML file containing podcast subscriptions
**When** the user imports or exports podcast subscriptions after migration
**Then** the import produces the same subscriptions and the export produces the same OPML output as before

### SCENARIO-005: Server handles feed refresh when TUI is disconnected
**Priority**: medium
**Refs**: AC-01, AC-02

**Given** the server is running but no TUI client is connected
**When** a periodic sync interval elapses
**Then** the server performs the feed refresh independently
**And** results are persisted for retrieval when a client reconnects

---

## Phase 1: Architecture and Config Redesign

### SCENARIO-006: Sync config nested under podcast section
**Priority**: high
**Refs**: AC-04

**Given** a configuration file with podcast synchronization settings
**When** the configuration is parsed
**Then** synchronization settings are read from the `[podcast.synchronization]` section
**And** no top-level `[synchronization]` section is recognized

### SCENARIO-007: Interval value of zero disables periodic sync
**Priority**: high
**Refs**: AC-05

**Given** a configuration with `interval` set to zero
**When** the server evaluates whether to run periodic sync
**Then** periodic synchronization is disabled
**And** no separate boolean enable setting is consulted

### SCENARIO-008: Absent interval setting disables periodic sync
**Priority**: medium
**Refs**: AC-05

**Given** a configuration where the `interval` setting is omitted entirely
**When** the server evaluates whether to run periodic sync
**Then** periodic synchronization is disabled by default

### SCENARIO-009: Refresh-on-startup can be explicitly disabled
**Priority**: medium
**Refs**: AC-06

**Given** a configuration with `refresh_on_startup` set to the disabled variant
**When** the server starts
**Then** no feed refresh is triggered at startup

### SCENARIO-010: Per-podcast last-checked timestamp is recorded
**Priority**: high
**Refs**: AC-08

**Given** a podcast with no previous sync history
**When** the server completes a feed check for that podcast
**Then** a `last_checked` timestamp is stored in the persistent store for that specific podcast

### SCENARIO-011: Per-podcast scheduling uses individual timestamps
**Priority**: high
**Refs**: AC-08, AC-09

**Given** podcast A was last checked 30 minutes ago and podcast B was last checked 2 hours ago
**And** the global sync interval is 1 hour
**When** the periodic sync evaluator runs
**Then** podcast B is included in the sync pass
**And** podcast A is skipped until its next eligible check time

### SCENARIO-012: Per-podcast interval override takes precedence
**Priority**: medium
**Refs**: AC-09

**Given** the global sync interval is 1 hour
**And** podcast C has a per-podcast override interval of 6 hours
**When** 2 hours have elapsed since podcast C was last checked
**Then** podcast C is not included in the sync pass

### SCENARIO-013: Missing per-podcast interval falls back to global
**Priority**: medium
**Refs**: AC-09

**Given** podcast D has no per-podcast interval override set
**And** the global sync interval is 1 hour
**When** more than 1 hour has elapsed since podcast D was last checked
**Then** podcast D is included in the sync pass using the global interval

### SCENARIO-014: All podcast network operations share a single task pool
**Priority**: high
**Refs**: AC-10

**Given** the server is performing feed fetches and episode downloads concurrently
**When** the total concurrent operations reach `concurrent_downloads_max`
**Then** additional operations wait in the shared task pool queue
**And** no separate task pool is created per sync cycle

---

## Phase 2: Sync Logic Correctness

### SCENARIO-015: User disables auto-enqueue entirely
**Priority**: high
**Refs**: AC-11

**Given** a user has set auto-enqueue to disabled in configuration
**When** new episodes are discovered and downloaded during a sync pass
**Then** the episodes are stored locally but not added to the playlist

### SCENARIO-016: User enables auto-enqueue for new episodes
**Priority**: high
**Refs**: AC-11, AC-12

**Given** a user has enabled auto-enqueue
**When** new episodes from podcast X are discovered during sync
**Then** those episodes are added to the playlist in chronological order (oldest first)

### SCENARIO-017: Episodes from different podcasts do not interleave arbitrarily
**Priority**: high
**Refs**: AC-12

**Given** auto-enqueue is enabled
**And** podcast X has 3 new episodes and podcast Y has 2 new episodes
**When** the sync pass completes and enqueues episodes
**Then** all episodes from the same podcast appear as a contiguous group in the playlist
**And** within each group episodes are ordered oldest to newest

### SCENARIO-018: Played episodes with deleted files are excluded from sync
**Priority**: high
**Refs**: AC-13

**Given** an episode that has been marked as played
**And** its local audio file has been deleted
**When** the sync pass processes the episode's podcast feed
**Then** that episode is not re-downloaded

### SCENARIO-019: Unplayed episodes with deleted files are re-downloaded
**Priority**: medium
**Refs**: AC-13

**Given** an episode that has NOT been marked as played
**And** its local audio file has been deleted
**When** the sync pass processes the episode's podcast feed
**Then** that episode is re-downloaded

### SCENARIO-020: Played episodes with existing files are not re-downloaded
**Priority**: medium
**Refs**: AC-13

**Given** an episode that has been marked as played
**And** its local audio file still exists on disk
**When** the sync pass processes the episode's podcast feed
**Then** that episode is skipped (no download, no enqueue)

### SCENARIO-021: Podcast episodes use PodcastUrl source for enqueue
**Priority**: high
**Refs**: AC-14

**Given** an episode has been downloaded and its local file exists
**When** that episode is enqueued to the playlist
**Then** the playlist track source is set to the podcast episode feed address
**And** the local file path alone is never used as the track source

### SCENARIO-022: Filesystem scan for existing files happens before async loop
**Priority**: high
**Refs**: AC-15

**Given** the sync process is about to check for already-downloaded episodes
**When** the directory listing for existing files is performed
**Then** the listing completes as a single operation before any async download tasks begin
**And** no synchronous filesystem call occurs inside the async task context

### SCENARIO-023: Large podcast directory scan does not block async runtime
**Priority**: medium
**Refs**: AC-15

**Given** a podcast download directory contains thousands of episode files
**When** the existing-file detection is performed
**Then** the scan is performed outside the async runtime (or via async filesystem primitives)
**And** the async event loop remains responsive during the scan

### SCENARIO-024: Downloads do not block feed update processing
**Priority**: high
**Refs**: AC-16

**Given** the server is processing feed updates from multiple podcasts
**When** a download task for a large episode file is in progress
**Then** the feed update processing continues without waiting for the download to complete

### SCENARIO-025: Podcast directory creation reuses existing utility
**Priority**: medium
**Refs**: AC-17

**Given** a newly subscribed podcast needs a download directory
**When** the sync process prepares to store episodes
**Then** the existing `create_podcast_dir` utility is invoked
**And** no duplicate directory-creation logic is introduced

### SCENARIO-026: Playlist append helpers delegate to base constructors
**Priority**: medium
**Refs**: AC-18

**Given** the playlist system has `new_single`/`new_vec` base constructors
**When** an `append_single` or `append_vec` operation is performed for podcast episodes
**Then** the append variant delegates to the base constructor with an append sentinel
**And** property definitions are not duplicated

### SCENARIO-027: Immediate first sync uses interval_at with Instant::now
**Priority**: medium
**Refs**: AC-19

**Given** the server starts with `refresh_on_startup` behavior desired
**When** the periodic sync loop is initialized
**Then** a single code path handles both immediate-first-tick and periodic behavior via start-time configuration
**And** no separate startup-refresh code path exists

---

## Phase 3: Test Quality

### SCENARIO-028: Tests verify meaningful behavior only
**Priority**: high
**Refs**: AC-20, AC-21

**Given** the test suite for podcast synchronization
**When** a test is evaluated for inclusion
**Then** tests verifying basic language semantics (Option unwrap, Vec push) are absent
**And** no two tests assert the same behavior under different names

### SCENARIO-029: Test URLs prevent external network calls
**Priority**: high
**Refs**: AC-22

**Given** a test that exercises feed-fetching or download logic
**When** the test constructs a podcast feed address
**Then** only localhost or loopback addresses are used
**And** no test makes outbound connections to external hosts

### SCENARIO-030: Error tests assert specific error variants
**Priority**: medium
**Refs**: AC-23

**Given** a test that verifies error handling behavior
**When** the test asserts on the error result
**Then** the assertion checks the specific error variant or message content
**And** a bare `is_err()` check is never the sole assertion

### SCENARIO-031: Test helpers eliminate boilerplate repetition
**Priority**: medium
**Refs**: AC-26

**Given** multiple tests need similar setup (config, episodes, podcasts)
**When** test setup is performed
**Then** shared factory functions provide the common objects
**And** individual tests configure only the variations relevant to their scenario

### SCENARIO-032: Tests confirm observable outcomes via spies or mocks
**Priority**: high
**Refs**: AC-27

**Given** a test verifying that correct episodes are downloaded in correct order
**When** the sync operation completes
**Then** the test asserts on observable effects (commands sent, files created, ordering confirmed)
**And** internal implementation details are not asserted directly

---

## Phase 4: Style and Conventions

### SCENARIO-033: Module documentation uses doc-comment format
**Priority**: medium
**Refs**: AC-29

**Given** a module within the podcast sync implementation
**When** the module's documentation is inspected
**Then** it uses `//!` doc-comment syntax in the approved format

### SCENARIO-034: Deeply nested logic is extracted to named helpers
**Priority**: medium
**Refs**: AC-30

**Given** sync logic that requires conditional branching
**When** nesting would exceed 3 levels
**Then** the inner logic is extracted into a named helper function with a descriptive name

### SCENARIO-035: Functions accept config struct references over individual values
**Priority**: medium
**Refs**: AC-31

**Given** a function that needs multiple configuration values
**When** the function signature is defined
**Then** it accepts a shared config type or struct reference
**And** individual config fields are not passed as separate parameters

---

## Edge Case Scenarios

### SCENARIO-036: Empty podcast subscription list during sync
**Priority**: medium
**Refs**: AC-08, AC-11

**Given** a user has no podcast subscriptions
**When** the periodic sync interval elapses
**Then** the sync pass completes immediately with no errors
**And** no unnecessary work (directory scans, task pool allocation) is performed

### SCENARIO-037: Podcast feed returns zero new episodes
**Priority**: medium
**Refs**: AC-08, AC-12

**Given** a subscribed podcast with all known episodes already downloaded
**When** the feed is refreshed during a sync pass
**Then** the `last_checked` timestamp is updated
**And** no download or enqueue operations are triggered

### SCENARIO-038: Concurrent sync pass does not duplicate downloads
**Priority**: high
**Refs**: AC-10, AC-16

**Given** a sync pass is currently in progress for podcast A
**When** a second sync trigger fires before the first pass completes
**Then** episodes already being downloaded are not queued for a duplicate download
**And** the shared task pool prevents resource exhaustion

### SCENARIO-039: Network timeout during feed fetch isolates to single podcast
**Priority**: high
**Refs**: AC-08, AC-10

**Given** the sync pass is checking feeds for podcasts A, B, and C
**When** the feed fetch for podcast B exceeds the configured timeout
**Then** podcasts A and C complete their sync successfully
**And** podcast B's failure is logged without aborting the entire pass

### SCENARIO-040: Sync interval set to maximum boundary value
**Priority**: low
**Refs**: AC-05, AC-09

**Given** a configuration with the sync interval set to an extremely large value (e.g., 30 days)
**When** the server parses and schedules the interval
**Then** the interval is accepted without overflow or scheduling errors
**And** the next sync fires at the correct future time

### SCENARIO-041: Database records last_checked even when all episodes fail download
**Priority**: medium
**Refs**: AC-08, AC-13

**Given** a podcast feed check discovers 5 new episodes
**When** all 5 download attempts fail due to network errors
**Then** the `last_checked` timestamp is still updated for that podcast
**And** the episodes remain eligible for retry on the next sync pass

### SCENARIO-042: Sync handles podcast with empty download directory
**Priority**: medium
**Refs**: AC-15, AC-17

**Given** a podcast subscription exists but its download directory does not yet exist
**When** the sync pass runs the existing-file detection for that podcast
**Then** the directory is created using the existing utility
**And** the file scan returns an empty set without error

---

## Traceability Matrix

| AC-ID | Scenarios |
|-------|-----------|
| AC-01 | SCENARIO-001, SCENARIO-005 |
| AC-02 | SCENARIO-002, SCENARIO-005 |
| AC-03 | SCENARIO-003, SCENARIO-004 |
| AC-04 | SCENARIO-006 |
| AC-05 | SCENARIO-007, SCENARIO-008, SCENARIO-040 |
| AC-06 | SCENARIO-009 |
| AC-07 | [CONSTRAINT: Human-readable comment in source — verified by code review, not behavioral scenario] |
| AC-08 | SCENARIO-010, SCENARIO-011, SCENARIO-036, SCENARIO-037, SCENARIO-039, SCENARIO-041 |
| AC-09 | SCENARIO-011, SCENARIO-012, SCENARIO-013, SCENARIO-040 |
| AC-10 | SCENARIO-014, SCENARIO-038, SCENARIO-039 |
| AC-11 | SCENARIO-015, SCENARIO-016, SCENARIO-036 |
| AC-12 | SCENARIO-016, SCENARIO-017, SCENARIO-037 |
| AC-13 | SCENARIO-018, SCENARIO-019, SCENARIO-020 |
| AC-14 | SCENARIO-021 |
| AC-15 | SCENARIO-022, SCENARIO-023, SCENARIO-042 |
| AC-16 | SCENARIO-024, SCENARIO-038 |
| AC-17 | SCENARIO-025, SCENARIO-042 |
| AC-18 | SCENARIO-026 |
| AC-19 | SCENARIO-027 |
| AC-20 | SCENARIO-028 |
| AC-21 | SCENARIO-028 |
| AC-22 | SCENARIO-029 |
| AC-23 | SCENARIO-030 |
| AC-24 | [CONSTRAINT: Style convention — verified by code review, not behavioral scenario] |
| AC-25 | [CONSTRAINT: Naming convention — verified by code review, not behavioral scenario] |
| AC-26 | SCENARIO-031 |
| AC-27 | SCENARIO-032 |
| AC-28 | [CONSTRAINT: Commit message format — verified by CI/git hooks, not behavioral scenario] |
| AC-29 | SCENARIO-033 |
| AC-30 | SCENARIO-034 |
| AC-31 | SCENARIO-035 |

### Non-Behavioral Constraints (not suited to Given/When/Then)

| AC-ID | Type | Verification Method |
|-------|------|-------------------|
| AC-07 | Source code comment | Code review / linter rule |
| AC-24 | Style (indoc crate usage) | Code review / clippy lint |
| AC-25 | Naming convention | Code review / naming lint |
| AC-28 | Commit message format | Git hook / CI check |

---

## Metadata

- **Total scenarios**: 42
- **Phases covered**: 5 (Phase 0–4)
- **Non-functional requirements addressed**: Performance (SCENARIO-014, SCENARIO-022, SCENARIO-023, SCENARIO-024, SCENARIO-038), Reliability (SCENARIO-039, SCENARIO-041)
- **Ambiguous items**: None — all open questions from requirements are noted but do not block scenario generation for stated ACs
