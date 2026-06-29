# BDD Scenarios: Server-Side Podcast Synchronization

- **Feature**: Server-Side Podcast Synchronization
- **Date**: 2026-06-23
- **Updated**: 2026-06-23
- **Requirements Source**: `01-requirements.md`
- **Status**: verified (all 23 scenarios covered by passing tests)

---

## Feature Area 1: Configuration

### SCENARIO-001: Default synchronization config applied when section absent
**AC Reference**: AC-01, AC-10  
**Priority**: high  
**Given** an existing server configuration file that has no `[synchronization]` section  
**When** the server loads the configuration  
**Then** synchronization is enabled with a 1-hour interval and refresh-on-startup active  
**And** no configuration parsing error occurs  

### SCENARIO-002: Explicit synchronization configuration honored
**AC Reference**: AC-01  
**Priority**: high  
**Given** a server configuration file with `synchronization.enable = false`, `interval = "30m"`, and `refresh_on_startup = false`  
**When** the server loads the configuration  
**Then** the synchronization settings reflect the explicitly specified values  

### SCENARIO-003: Configuration roundtrip preserves all fields
**AC Reference**: AC-01, AC-10  
**Priority**: medium  
**Given** a synchronization configuration with non-default values for all fields  
**When** the configuration is serialized and then deserialized  
**Then** all field values are identical after the roundtrip  

### SCENARIO-004: Invalid interval duration string rejected
**AC Reference**: AC-01  
**Priority**: medium  
**Given** a server configuration file with `synchronization.interval` set to a non-parseable duration string  
**When** the server attempts to load the configuration  
**Then** a configuration parse error is reported  
**And** the server does not start  

---

## Feature Area 2: Sync Task Lifecycle

### SCENARIO-005: Sync task not spawned when disabled
**AC Reference**: AC-02  
**Priority**: high  
**Given** synchronization is configured with `enable = false`  
**When** the server starts  
**Then** no synchronization task is running  
**And** the server operates identically to its behavior without the synchronization feature  

### SCENARIO-006: Immediate sync on startup when refresh_on_startup enabled
**AC Reference**: AC-03  
**Priority**: high  
**Given** synchronization is enabled with `refresh_on_startup = true`  
**And** there are subscribed podcasts with new episodes available in their feeds  
**When** the server starts  
**Then** a full sync pass executes before the periodic cycle begins  
**And** new episodes are downloaded and enqueued  

### SCENARIO-007: No immediate sync when refresh_on_startup disabled
**AC Reference**: AC-03  
**Priority**: medium  
**Given** synchronization is enabled with `refresh_on_startup = false`  
**And** `interval` is set to `"1h"`  
**When** the server starts  
**Then** no sync pass occurs until the first interval elapses  

### SCENARIO-008: Periodic sync executes at configured interval
**AC Reference**: AC-04  
**Priority**: high  
**Given** synchronization is enabled with `interval = "1h"`  
**And** the server has been running  
**When** 1 hour elapses since the last sync pass  
**Then** a new sync pass begins refreshing all subscribed podcast feeds  

### SCENARIO-009: Graceful shutdown cancels the sync task
**AC Reference**: AC-09  
**Priority**: high  
**Given** the synchronization task is running and currently mid-sync  
**When** the server receives a shutdown signal (cancel token is triggered)  
**Then** the sync task exits cleanly without completing the current pass  
**And** no resources are leaked  

---

## Feature Area 3: Episode Detection and Deduplication

### SCENARIO-010: New episode identified by GUID absence
**AC Reference**: AC-05  
**Priority**: high  
**Given** a subscribed podcast feed contains an episode with a GUID not present in the podcast database  
**When** a sync pass processes that podcast  
**Then** the episode is identified as new  
**And** it is scheduled for download  

### SCENARIO-011: Episode with existing GUID is skipped
**AC Reference**: AC-05  
**Priority**: high  
**Given** a subscribed podcast feed contains an episode whose GUID already exists in the podcast database  
**When** a sync pass processes that podcast  
**Then** the episode is not downloaded  
**And** it is not added to the play queue  

### SCENARIO-012: Fallback deduplication by enclosure URL when GUID absent
**AC Reference**: AC-05  
**Priority**: medium  
**Given** a subscribed podcast feed contains an episode without a GUID but with an enclosure URL already present in the podcast database  
**When** a sync pass processes that podcast  
**Then** the episode is recognized as already known  
**And** it is not re-downloaded  

### SCENARIO-013: Episode already in play queue is not re-added
**AC Reference**: AC-05  
**Priority**: medium  
**Given** a podcast episode has been downloaded and is already present in the play queue  
**When** a subsequent sync pass encounters the same episode in the feed  
**Then** the episode is not added to the play queue again  

---

## Feature Area 4: Download and Enqueue

### SCENARIO-014: New episode downloaded to podcast directory
**AC Reference**: AC-06  
**Priority**: high  
**Given** a new episode has been identified during a sync pass  
**When** the episode is processed for download  
**Then** the episode audio file is saved to the configured podcast download directory  
**And** the episode metadata is inserted into the podcast database  

### SCENARIO-015: Downloaded episode appended to end of play queue
**AC Reference**: AC-07  
**Priority**: high  
**Given** a new episode has been successfully downloaded  
**And** the play queue already contains existing tracks  
**When** the episode is enqueued  
**Then** the episode appears at the end of the play queue after all existing tracks  

### SCENARIO-016: Playback auto-starts when queue was empty
**AC Reference**: AC-07  
**Priority**: high  
**Given** the play queue is empty  
**And** a new episode has been successfully downloaded  
**When** the episode is added to the play queue  
**Then** playback begins automatically  

---

## Feature Area 5: Error Isolation

### SCENARIO-017: Network error on one feed does not abort sync pass
**AC Reference**: AC-08  
**Priority**: high  
**Given** multiple podcasts are subscribed  
**And** one podcast feed is unreachable due to a network error  
**When** a sync pass processes all subscribed podcasts  
**Then** the unreachable feed is skipped with a warning logged  
**And** all other podcasts are processed normally  

### SCENARIO-018: Malformed RSS feed does not crash the server
**AC Reference**: AC-08  
**Priority**: high  
**Given** a subscribed podcast has a feed that returns malformed content  
**When** a sync pass attempts to parse that feed  
**Then** a warning is logged for the malformed feed  
**And** the sync pass continues with remaining podcasts  
**And** the server remains operational  

### SCENARIO-019: Download failure for one episode does not block others
**AC Reference**: AC-08  
**Priority**: medium  
**Given** a podcast has three new episodes available  
**And** one episode's download fails due to a broken link  
**When** the sync pass processes that podcast  
**Then** the failed episode download is logged as a warning  
**And** the other two episodes are downloaded and enqueued successfully  

---

## Feature Area 6: Task Integration Pattern

### SCENARIO-020: Sync task follows established spawn pattern
**AC Reference**: AC-11  
**Priority**: medium  
**Given** the server is configured with synchronization enabled  
**When** the server starts in `actual_main()`  
**Then** the sync task is spawned adjacent to the existing playlist save interval task  
**And** it receives a handle and the service cancel token  

---

## Edge Cases

### SCENARIO-021: First sync with no subscribed podcasts
**AC Reference**: AC-04  
**Priority**: low  
**Given** synchronization is enabled  
**And** the user has no subscribed podcasts  
**When** a sync pass executes  
**Then** the pass completes immediately with no errors  
**And** no downloads are attempted  

### SCENARIO-022: Sync pass during ongoing playback does not disrupt audio
**AC Reference**: AC-07, AC-09  
**Priority**: medium  
**Given** the server is actively playing a track from the queue  
**And** a sync pass downloads new episodes  
**When** the new episodes are appended to the play queue  
**Then** the currently playing track is not interrupted  
**And** new episodes appear after the current queue contents  

### SCENARIO-023: Concurrent sync tick arrives while previous pass is still running
**AC Reference**: AC-04  
**Priority**: medium  
**Given** a sync pass is still in progress when the next interval tick fires  
**When** the interval timer elapses  
**Then** the timer does not drift due to the overrun  
**And** the task does not spawn duplicate concurrent sync operations  

---

## Traceability Matrix

| AC-ID  | Scenarios                                     | Coverage Level |
|--------|-----------------------------------------------|----------------|
| AC-01  | SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-004 | Strong |
| AC-02  | SCENARIO-005                                  | Adequate |
| AC-03  | SCENARIO-006, SCENARIO-007                    | Strong |
| AC-04  | SCENARIO-008, SCENARIO-021, SCENARIO-023      | Strong |
| AC-05  | SCENARIO-010, SCENARIO-011, SCENARIO-012, SCENARIO-013 | Strong |
| AC-06  | SCENARIO-014                                  | Adequate |
| AC-07  | SCENARIO-015, SCENARIO-016, SCENARIO-022      | Strong |
| AC-08  | SCENARIO-017, SCENARIO-018, SCENARIO-019      | Strong |
| AC-09  | SCENARIO-009, SCENARIO-022                    | Strong |
| AC-10  | SCENARIO-001, SCENARIO-003                    | Adequate |
| AC-11  | SCENARIO-020                                  | Adequate |

---

## Coverage Summary

- **Total ACs analyzed**: 11
- **ACs with strong coverage (3+ scenarios)**: 6 (AC-01, AC-03, AC-04, AC-05, AC-07, AC-08)
- **ACs with adequate coverage (1-2 focused scenarios)**: 5 (AC-02, AC-06, AC-09, AC-10, AC-11)
- **ACs with weak coverage**: 0
- **Edge case scenarios by dimension**:
  - Null/Empty inputs: 1 (SCENARIO-021 -- no subscribed podcasts)
  - Boundary values: 1 (SCENARIO-004 -- invalid duration string)
  - Concurrent access: 1 (SCENARIO-023 -- overlapping sync ticks)
  - Timeouts: 0 (timeout behavior subsumed by network error isolation in SCENARIO-017)
  - Permission boundaries: 0 (N/A -- single-user server application)
  - Data overflow: 0 (noted in Open Questions; not specified as AC)
  - Invalid state transitions: 1 (SCENARIO-022 -- sync during active playback)

---

## Quality Self-Assessment

```
quality_score: 8.5, specificity: 8, independence: 9, coverage: 9, testability: 8
```

- **Specificity (8/10)**: Scenarios describe concrete conditions and outcomes. A few (e.g., SCENARIO-020) are slightly more structural than behavioral but remain verifiable.
- **Independence (9/10)**: Each scenario can be validated in isolation with appropriate test fixtures. No scenario depends on another scenario's side effects.
- **Coverage breadth (9/10)**: All 11 acceptance criteria are exercised. Error paths, deduplication logic, and lifecycle behaviors are all represented. Edge cases cover empty state, concurrency, and active-playback interaction.
- **Testability (8/10)**: All scenarios describe deterministic pass/fail conditions. Some (SCENARIO-023 timer drift) may require time-manipulation utilities but remain automatable.

---

## Constraints Noted (Non-Functional)

The following non-functional requirements inform scenario design but are not directly expressed as Given/When/Then:

- **Performance**: Sync must not block the player loop; network I/O is async; concurrent feed fetches bounded by `concurrent_downloads_max`; timer must not drift.
- **Reliability**: Error isolation per podcast is mandatory (covered by SCENARIO-017, SCENARIO-018, SCENARIO-019).
- **Backward Compatibility**: Existing configs without `[synchronization]` must parse without error (covered by SCENARIO-001).
- **Minimal Dependencies**: Only `humantime`/`humantime-serde` expected as new dependency.
