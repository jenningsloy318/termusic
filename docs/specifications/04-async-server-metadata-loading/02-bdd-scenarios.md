# BDD Scenarios: Async Server Metadata Loading

- **Feature**: Async Server Metadata Loading
- **Date**: 2026-06-26
- **Source**: `specification/04-async-server-metadata-loading/01-requirements.md`
- **Quality Score**: quality_score: 8.75, specificity: 9, independence: 9, coverage: 8, testability: 9

---

## Coverage Summary

| Metric | Value |
|--------|-------|
| Total ACs analyzed | 10 |
| ACs with strong coverage (3+ scenarios) | 4 (AC-01, AC-05, AC-07, AC-08) |
| ACs with adequate coverage (1-2 scenarios) | 5 (AC-02, AC-03, AC-04, AC-06, AC-09) |
| ACs with weak coverage (happy path only) | 1 (AC-10) |
| Edge cases: null/empty | 2 |
| Edge cases: boundary values | 2 |
| Edge cases: concurrent access | 2 |
| Edge cases: timeout | 1 |
| Edge cases: permission/authorization | 0 |
| Edge cases: data overflow | 1 |
| Edge cases: invalid state transitions | 2 |

**Weak coverage justification**: AC-10 (TUI responsiveness during loading) is a UX constraint verified by the TUI remaining interactive and displaying a meaningful state. The behavior is binary (responsive vs. frozen) and edge cases would duplicate scenarios from AC-04 and AC-05.

---

## Server Startup Readiness

### SCENARIO-001: Server accepts connections within 1 second regardless of playlist size
**Priority**: high
**Refs**: AC-01

**Given** a playlist file containing 1000 local audio file paths
**And** each metadata read takes approximately 20ms of blocking I/O
**When** the server process is started
**Then** the gRPC listener accepts incoming connections within 1 second of process start
**And** metadata loading has not yet completed at the time of connection acceptance

### SCENARIO-002: Server accepts connections immediately with an empty playlist
**Priority**: medium
**Refs**: AC-01

**Given** a playlist file containing zero track entries
**When** the server process is started
**Then** the gRPC listener accepts incoming connections within 1 second of process start
**And** no background metadata loading task is spawned

### SCENARIO-003: Server accepts connections within 1 second with a small playlist
**Priority**: medium
**Refs**: AC-01

**Given** a playlist file containing 10 local audio file paths
**When** the server process is started
**Then** the gRPC listener accepts incoming connections within 1 second of process start

---

## Background Thread Pool Isolation

### SCENARIO-004: Metadata loading executes on a dedicated thread pool separate from the async runtime
**Priority**: high
**Refs**: AC-02

**Given** the server has started its gRPC listener
**And** background metadata loading is in progress
**When** multiple client calls arrive concurrently
**Then** client call handling is not blocked or delayed by metadata I/O
**And** the metadata loading thread pool operates independently of the tokio async runtime

### SCENARIO-005: Background loading does not starve the gRPC service of resources
**Priority**: medium
**Refs**: AC-02

**Given** a playlist file containing 1000 local audio file paths
**And** the background metadata loading thread pool is fully utilized
**When** a client sends a playlist retrieval call during peak loading activity
**Then** the call is answered within 100ms
**And** the service threads are not contending with the metadata loading threads

---

## Playlist Correctness After Loading

### SCENARIO-006: Loaded playlist matches synchronous implementation output exactly
**Priority**: high
**Refs**: AC-03

**Given** a playlist file containing a mix of local audio paths, podcast addresses, and radio streams in a specific order
**When** background metadata loading completes
**Then** the shared playlist contains the same tracks in the same order as the previous synchronous implementation would produce
**And** all track metadata (title, artist, duration, album) is identical to synchronous loading output

### SCENARIO-007: Track ordering is preserved after asynchronous loading
**Priority**: high
**Refs**: AC-03

**Given** a playlist file with tracks listed as [track-A, track-B, track-C, track-D, track-E]
**And** track-B metadata takes significantly longer to read than the others
**When** background metadata loading completes
**Then** the shared playlist contains tracks in the exact order [track-A, track-B, track-C, track-D, track-E]

---

## Client Notification on Playlist Availability

### SCENARIO-008: Connected client receives notification when playlist loading completes
**Priority**: high
**Refs**: AC-04

**Given** a TUI client is connected to the server
**And** background metadata loading is in progress
**When** metadata loading completes and the playlist is fully populated
**Then** the connected client receives an update event indicating the playlist is now available
**And** the client can retrieve the full playlist contents

### SCENARIO-009: Client connecting after loading completes receives the full playlist
**Priority**: medium
**Refs**: AC-04

**Given** background metadata loading has already completed
**When** a new TUI client connects and asks for the playlist
**Then** the client receives the complete playlist with all tracks and metadata

---

## Non-Blocking Playlist Queries During Loading

### SCENARIO-010: GetPlaylist returns empty state while loading is in progress
**Priority**: high
**Refs**: AC-05

**Given** a TUI client is connected to the server
**And** background metadata loading has started but not completed
**When** the client asks for the current playlist
**Then** the server responds immediately with the current state (empty playlist)
**And** the answer does not block waiting for metadata loading to finish

### SCENARIO-011: Multiple concurrent GetPlaylist calls during loading all return promptly
**Priority**: medium
**Refs**: AC-05

**Given** background metadata loading is in progress
**When** three clients simultaneously ask for the current playlist
**Then** all three clients receive an answer within 100ms
**And** none of the answers block on metadata completion

### SCENARIO-012: GetPlaylist returns fully populated playlist after loading completes
**Priority**: medium
**Refs**: AC-05

**Given** background metadata loading has completed
**And** the shared playlist has been fully populated
**When** a client asks for the current playlist
**Then** the server responds with the complete playlist including all tracks and metadata

---

## Playback Deferred Until Load Complete

### SCENARIO-013: Playback does not start while playlist is still loading even if configured to auto-play
**Priority**: high
**Refs**: AC-06

**Given** the server configuration has `startup_state` set to Playing
**And** background metadata loading is in progress
**When** the server evaluates whether to begin playback
**Then** playback does not start
**And** the server waits until loading completes and a valid current track index exists before initiating playback

### SCENARIO-014: Playback starts automatically after loading completes when auto-play is configured
**Priority**: high
**Refs**: AC-06

**Given** the server configuration has `startup_state` set to Playing
**And** background metadata loading has just completed with a valid playlist
**When** the server evaluates the post-load state
**Then** playback begins at the saved track index
**And** the current track is valid and fully loaded with metadata

---

## Save Protection During Loading

### SCENARIO-015: Periodic save skips writing while metadata loading is in progress
**Priority**: high
**Refs**: AC-07

**Given** the server's periodic playlist save interval triggers
**And** background metadata loading has not yet completed
**When** the save mechanism evaluates whether to write playlist.log
**Then** the save is skipped without writing to disk
**And** the existing playlist.log file remains unmodified

### SCENARIO-016: Save resumes normally after metadata loading completes
**Priority**: high
**Refs**: AC-07

**Given** background metadata loading has completed
**And** the shared playlist is fully populated
**When** the periodic playlist save interval triggers
**Then** the save mechanism writes the current playlist to disk normally
**And** the saved file contains all loaded tracks

### SCENARIO-017: Manual save operation is also blocked during loading
**Priority**: medium
**Refs**: AC-07

**Given** background metadata loading is in progress
**When** a save operation is triggered (periodic or by any other mechanism)
**Then** the save is suppressed to protect the existing playlist.log
**And** no partial playlist data is written to disk

---

## Graceful Degradation on Load Failure

### SCENARIO-018: Corrupt playlist.log results in partial load with error logging
**Priority**: high
**Refs**: AC-08

**Given** the playlist.log file contains some valid entries and some corrupt/unparseable lines
**When** background metadata loading processes the file
**Then** all valid tracks are loaded successfully into the shared playlist
**And** errors are logged for the corrupt entries
**And** the server continues operating with the successfully loaded tracks

### SCENARIO-019: Completely unreadable playlist.log results in empty playlist with logged error
**Priority**: high
**Refs**: AC-08

**Given** the playlist.log file cannot be read (file permissions error or file system failure)
**When** background metadata loading attempts to process the file
**Then** an error is logged describing the failure
**And** the server continues operating with an empty playlist
**And** the server does not crash or become unresponsive

### SCENARIO-020: Individual track file I/O failure does not halt loading of remaining tracks
**Priority**: high
**Refs**: AC-08

**Given** a playlist.log file references 100 tracks where 5 tracks have become inaccessible (deleted or moved)
**When** background metadata loading processes all entries
**Then** 95 tracks are loaded successfully into the shared playlist
**And** 5 failures are logged individually
**And** the relative order of the 95 successful tracks matches their original order in the file

---

## Clean Shutdown

### SCENARIO-021: Server shutdown terminates background loading within 1 second
**Priority**: high
**Refs**: AC-09

**Given** background metadata loading is in progress (partially complete)
**When** the server receives a Quit signal
**Then** the background metadata thread pool is shut down within 1 second
**And** any tracks that were fully loaded before shutdown are not lost
**And** the server process exits cleanly

### SCENARIO-022: Server shutdown after loading completes does not block on the thread pool
**Priority**: medium
**Refs**: AC-09

**Given** background metadata loading has already completed
**And** the dedicated thread pool is idle
**When** the server receives a Quit signal
**Then** the thread pool is cleaned up immediately without delay
**And** the server process exits within normal shutdown time

---

## TUI Responsiveness During Loading Period

### SCENARIO-023: TUI remains interactive between connection and playlist availability
**Priority**: high
**Refs**: AC-10

**Given** the TUI client has connected to the server
**And** background metadata loading has not yet completed
**When** the user interacts with the TUI (switching views, accessing settings)
**Then** the TUI responds to user input without delay
**And** the playlist area displays an appropriate empty or loading state rather than appearing frozen

---

## Edge Case Scenarios

### SCENARIO-024: Server starts with missing playlist.log file
**Priority**: medium
**Refs**: AC-01, AC-08

**Given** no playlist.log file exists at the expected path
**When** the server process is started
**Then** the gRPC listener accepts connections within 1 second
**And** the server operates with an empty playlist
**And** a warning is logged about the missing playlist file

### SCENARIO-025: Extremely large playlist does not cause excessive memory during loading
**Priority**: medium
**Refs**: AC-01, AC-02

**Given** a playlist file containing 10,000 local audio file paths
**When** background metadata loading is in progress
**Then** memory usage does not exceed 2x the steady-state usage
**And** the loading process bounds its concurrency to the thread pool size

### SCENARIO-026: Shutdown signal arrives before background loading has started any work
**Priority**: medium
**Refs**: AC-09

**Given** the server has just started
**And** the background metadata loading task has been spawned but has not begun processing any tracks
**When** the server receives a Quit signal immediately
**Then** the background task is cancelled cleanly
**And** the server exits within 1 second without errors

### SCENARIO-027: Client disconnects and reconnects during metadata loading
**Priority**: medium
**Refs**: AC-04, AC-05

**Given** a TUI client is connected during background metadata loading
**When** the client disconnects and reconnects before loading completes
**Then** the reconnected client can retrieve the current playlist state (empty or partial)
**And** the client receives the full playlist notification when loading eventually completes

---

## Traceability Matrix

| AC-ID | Scenarios |
|-------|-----------|
| AC-01 | SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-024, SCENARIO-025 |
| AC-02 | SCENARIO-004, SCENARIO-005, SCENARIO-025 |
| AC-03 | SCENARIO-006, SCENARIO-007 |
| AC-04 | SCENARIO-008, SCENARIO-009, SCENARIO-027 |
| AC-05 | SCENARIO-010, SCENARIO-011, SCENARIO-012, SCENARIO-027 |
| AC-06 | SCENARIO-013, SCENARIO-014 |
| AC-07 | SCENARIO-015, SCENARIO-016, SCENARIO-017 |
| AC-08 | SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024 |
| AC-09 | SCENARIO-021, SCENARIO-022, SCENARIO-026 |
| AC-10 | SCENARIO-023 |

### Non-Behavioral Constraints (not suited to Given/When/Then)

| Constraint | Type | Verification Method |
|------------|------|-------------------|
| Memory usage during loading <= 2x steady-state | Performance | Profiling / memory measurement during integration test |
| Metadata loading throughput must not regress vs current parallel implementation | Performance | Benchmark comparison with spec-03 baseline |
| No new attack surface introduced | Security | Code review — same files, same permissions |
| Log start/completion of background loading with timing | Observability | Log output inspection during integration test |

---

## Metadata

- **Total scenarios**: 27
- **Feature areas covered**: 8 (Server Startup Readiness, Thread Pool Isolation, Playlist Correctness, Client Notification, Non-Blocking Queries, Playback Deferral, Save Protection, Graceful Degradation, Clean Shutdown, TUI Responsiveness)
- **Non-functional requirements addressed**: Performance (SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-025), Reliability (SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024), Observability (SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024)
- **Ambiguous items**: None — open questions from requirements (loading indicator style, progressive vs atomic swap, PLAYLIST_POOL reuse) are addressed by the recommended approach (atomic swap, reuse existing pool) and do not alter AC definitions
