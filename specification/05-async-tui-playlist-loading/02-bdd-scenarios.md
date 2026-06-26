# BDD Scenarios: Async TUI Playlist Loading

- **Feature**: Async TUI Playlist Loading
- **Date**: 2026-06-26
- **Source**: `specification/05-async-tui-playlist-loading/01-requirements.md`
- **Quality Score**: quality_score: 8.75, specificity: 9, independence: 9, coverage: 8, testability: 9

---

## Coverage Summary

| Metric | Value |
|--------|-------|
| Total ACs analyzed | 10 |
| ACs with strong coverage (3+ scenarios) | 4 (AC-01, AC-03, AC-05, AC-08) |
| ACs with adequate coverage (1-2 scenarios) | 4 (AC-02, AC-04, AC-07, AC-09) |
| ACs with weak coverage (happy path only) | 2 (AC-06, AC-10) |
| Edge cases: null/empty | 2 |
| Edge cases: boundary values | 3 |
| Edge cases: concurrent access | 1 |
| Edge cases: timeout | 0 |
| Edge cases: permission/authorization | 0 |
| Edge cases: data overflow | 1 |
| Edge cases: invalid state transitions | 1 |

**Weak coverage justification**: AC-06 (protobuf extension with backward wire compatibility) is a schema constraint verified by protobuf compilation and wire format inspection — no behavioral edge cases beyond the happy path apply. AC-10 (existing playlist operations continue working) is a regression constraint verified by the existing test suite; the operations themselves are not changed, only the data source.

---

## TUI Event Loop Responsiveness

### SCENARIO-001: TUI remains responsive during playlist loading for a large playlist
**Priority**: high
**Refs**: AC-01

**Given** a server with a 1000-track playlist loaded in memory
**And** the TUI is connected to the server
**When** the TUI receives the full playlist data from the server
**Then** the TUI main event loop is never blocked for more than 100ms during processing
**And** the TUI continues to accept user input throughout the playlist population

### SCENARIO-002: TUI remains responsive during playlist loading for a small playlist
**Priority**: medium
**Refs**: AC-01

**Given** a server with a 50-track playlist loaded in memory
**And** the TUI is connected to the server
**When** the TUI receives the full playlist data from the server
**Then** the TUI main event loop is never blocked for more than 100ms during processing

### SCENARIO-003: TUI event loop is not blocked when receiving a shuffled playlist event
**Priority**: high
**Refs**: AC-01, AC-05

**Given** a server that has completed async background loading of a 500-track playlist
**And** the TUI is connected and displaying a previous playlist state
**When** the server sends a shuffle event with full playlist data
**Then** the TUI main event loop is never blocked for more than 100ms while processing the event

---

## Playlist Rendering Speed

### SCENARIO-004: Playlist displays with metadata within 200ms of data receipt
**Priority**: high
**Refs**: AC-02

**Given** a server with a 500-track playlist containing title, artist, and duration metadata
**And** the TUI is connected to the server
**When** the TUI receives the full playlist data
**Then** the playlist view displays track names (title or filename), artist, and duration within 200ms of data arrival

### SCENARIO-005: Playlist displays track titles from metadata when available
**Priority**: medium
**Refs**: AC-02, AC-07

**Given** a server playlist where tracks have title metadata populated
**When** the TUI receives and processes the playlist data
**Then** the playlist view shows the track title as the display name (not the raw file path)

---

## Server Provides Full Display Metadata

### SCENARIO-006: Server includes title, artist, album, and duration in playlist data
**Priority**: high
**Refs**: AC-03

**Given** a server with tracks that have full metadata (title, artist, album, duration)
**When** the server provides playlist data to the TUI
**Then** the transmitted data includes title, artist, album, and duration for each track alongside the track identifier

### SCENARIO-007: Server includes full metadata in playlist shuffle stream events
**Priority**: high
**Refs**: AC-03, AC-05

**Given** a server that has completed background loading and holds full track metadata
**When** the server emits a playlist shuffled stream event
**Then** the event payload includes title, artist, album, and duration for every track in the shuffled playlist

### SCENARIO-008: Server includes full metadata in individual track addition events
**Priority**: high
**Refs**: AC-03

**Given** a server where a user adds a new track to the playlist
**When** the server emits a track addition event to the TUI
**Then** the event includes title, artist, album, and duration for the added track

### SCENARIO-009: Server populates title that was previously always empty
**Priority**: high
**Refs**: AC-03, AC-07

**Given** a server with tracks that have title metadata parsed from audio file tags
**When** the server builds any playlist-related message
**Then** the title is populated with the track's parsed title (not left empty)

---

## TUI Constructs Tracks Without Disk I/O

### SCENARIO-010: TUI constructs track objects directly from server-provided metadata
**Priority**: high
**Refs**: AC-04

**Given** a TUI that receives playlist data containing full display metadata for each track
**When** the TUI processes the data to build its internal playlist model
**Then** track objects are constructed from the provided metadata without reading any file from disk
**And** no filesystem operations are performed during playlist population

### SCENARIO-011: TUI does not invoke file-based metadata parsing during playlist load
**Priority**: high
**Refs**: AC-04

**Given** a TUI connected to a server with a 200-track playlist
**When** the TUI receives and processes the full playlist data
**Then** the file-based metadata parsing path is never invoked
**And** no audio file tags are read from disk

---

## Shuffle Event Processing Without Disk I/O

### SCENARIO-012: Shuffle event is processed without re-reading metadata from disk
**Priority**: high
**Refs**: AC-05

**Given** a TUI displaying a playlist with 500 tracks
**And** the server has completed async background loading
**When** the server sends a shuffle event containing full metadata for all tracks
**Then** the TUI rebuilds its playlist model from the event metadata
**And** no disk I/O is performed during the rebuild

### SCENARIO-013: Multiple rapid shuffle events are each processed without disk I/O
**Priority**: medium
**Refs**: AC-05

**Given** a TUI connected to a server
**When** the server sends two shuffle events in rapid succession (both with full metadata)
**Then** both events are processed by constructing tracks from the provided metadata
**And** no disk reads occur for either event

---

## Protobuf Schema Extension

### SCENARIO-014: Protobuf message includes artist and album with backward wire compatibility
**Priority**: high
**Refs**: AC-06

**Given** the protobuf schema for track addition messages
**When** the schema is extended with optional artist and album string attributes
**Then** existing numbering remains unchanged
**And** the new attributes use numbers that do not conflict with existing definitions
**And** a message serialized by the new schema can be deserialized by a reader unaware of the new attributes without error

---

## Server Populates Title

### SCENARIO-015: Server sends track title instead of empty value
**Priority**: high
**Refs**: AC-07

**Given** a server with tracks whose audio file tags contain title metadata
**When** the server serializes track data for any playlist-related message
**Then** the title contains the parsed title string
**And** the title is not empty or absent

### SCENARIO-016: Server sends filename-derived title when tag-based title is missing
**Priority**: medium
**Refs**: AC-07, AC-08

**Given** a server with a track whose audio file has no title tag
**When** the server serializes that track's data for a playlist message
**Then** the title contains a display name derived from the filename (without file extension)

---

## Graceful Fallback for Missing Metadata

### SCENARIO-017: TUI displays filename fallback when metadata is absent
**Priority**: high
**Refs**: AC-08

**Given** a server sends a track entry with only the path identifier and no title, artist, or album
**When** the TUI processes this track entry
**Then** the playlist view displays a human-readable name derived from the file path (e.g., the filename without extension)
**And** no error or crash occurs

### SCENARIO-018: Server sends partial metadata when file cannot be parsed
**Priority**: high
**Refs**: AC-08

**Given** a server playlist contains a track whose audio file is corrupted or in an unsupported format
**When** the server builds the playlist data for transmission
**Then** the track is included with its path identifier and any available attributes (duration if known)
**And** unavailable metadata attributes are left empty rather than causing the track to be omitted

### SCENARIO-019: TUI handles track with missing duration gracefully
**Priority**: medium
**Refs**: AC-08

**Given** a server sends a track entry with title and artist but no duration
**When** the TUI displays this track in the playlist view
**Then** the track is shown with title and artist
**And** the duration column shows a dash indicator or is left blank rather than displaying an error

### SCENARIO-020: Server does not crash when track has no metadata at all
**Priority**: high
**Refs**: AC-08

**Given** a server playlist contains a track file that cannot be found on disk
**When** the server builds the playlist data including this track
**Then** the server includes the track with its path identifier
**And** the server does not terminate or skip the entire playlist transmission

---

## Playlist Sync Table Performance

### SCENARIO-021: Table building completes within 50ms for a 1000-track playlist
**Priority**: high
**Refs**: AC-09

**Given** a TUI with a fully populated in-memory playlist model containing 1000 tracks
**When** the table view is rebuilt from the in-memory data
**Then** the table-building operation completes within 50ms
**And** no disk I/O occurs during table construction

### SCENARIO-022: Table building scales linearly with track count
**Priority**: medium
**Refs**: AC-09

**Given** a TUI with in-memory playlist models of 100, 500, and 1000 tracks
**When** the table view is rebuilt for each size
**Then** the time taken scales approximately linearly with track count
**And** no operation exceeds 50ms for 1000 tracks

---

## Existing Playlist Operations Compatibility

### SCENARIO-023: All playlist mutations continue working with metadata-carrying protocol
**Priority**: high
**Refs**: AC-10

**Given** a TUI connected to a server using the extended metadata-carrying protocol
**When** the user performs playlist operations (add track, remove track, swap positions, shuffle, clear)
**Then** each operation completes successfully
**And** the playlist state is consistent between server and TUI after each operation

---

## Edge Case Scenarios

### SCENARIO-024: Empty playlist is handled without error
**Priority**: medium
**Refs**: AC-01, AC-02, AC-08

**Given** a server with an empty playlist (zero tracks)
**When** the TUI receives the empty playlist data
**Then** the playlist view shows an empty state without error
**And** no disk I/O or metadata parsing is attempted

### SCENARIO-025: Playlist with all tracks missing metadata displays successfully
**Priority**: medium
**Refs**: AC-08, AC-02

**Given** a server playlist where every track has only a path identifier (no title, artist, album, or duration)
**When** the TUI receives and displays this playlist
**Then** every track is shown using a filename-derived fallback name
**And** the playlist view is usable (tracks are selectable, playable)

### SCENARIO-026: Very large playlist (5000 tracks) does not exceed 100ms event loop block
**Priority**: medium
**Refs**: AC-01, AC-09

**Given** a server with a 5000-track playlist fully loaded with metadata
**When** the TUI receives and processes the full playlist data
**Then** the event loop is never blocked for more than 100ms
**And** the playlist displays within a reasonable time without consuming excessive memory

### SCENARIO-027: Concurrent playlist reload during shuffle event does not corrupt state
**Priority**: medium
**Refs**: AC-01, AC-05, AC-10

**Given** a TUI connected to a server
**When** the TUI initiates a playlist reload at the same moment the server sends a shuffle event
**Then** the playlist state resolves to a consistent final state (either the reload result or the shuffle result)
**And** no partial or corrupted playlist is displayed

### SCENARIO-028: Track with extremely long title and artist metadata is handled without overflow
**Priority**: low
**Refs**: AC-03, AC-08

**Given** a server sends a track with a title exceeding 500 characters and an artist name exceeding 300 characters
**When** the TUI receives and displays this track
**Then** the track is shown with the metadata truncated or wrapped appropriately
**And** no buffer overflow or display error occurs

---

## Traceability Matrix

| AC-ID | Scenarios |
|-------|-----------|
| AC-01 | SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-024, SCENARIO-026, SCENARIO-027 |
| AC-02 | SCENARIO-004, SCENARIO-005, SCENARIO-024, SCENARIO-025 |
| AC-03 | SCENARIO-006, SCENARIO-007, SCENARIO-008, SCENARIO-009, SCENARIO-028 |
| AC-04 | SCENARIO-010, SCENARIO-011 |
| AC-05 | SCENARIO-003, SCENARIO-007, SCENARIO-012, SCENARIO-013, SCENARIO-027 |
| AC-06 | SCENARIO-014 |
| AC-07 | SCENARIO-005, SCENARIO-009, SCENARIO-015, SCENARIO-016 |
| AC-08 | SCENARIO-017, SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024, SCENARIO-025, SCENARIO-028 |
| AC-09 | SCENARIO-021, SCENARIO-022, SCENARIO-026 |
| AC-10 | SCENARIO-023, SCENARIO-027 |

### Non-Behavioral Constraints (not suited to Given/When/Then)

| AC-ID | Type | Verification Method |
|-------|------|-------------------|
| AC-06 | Schema constraint | Protobuf compilation and wire format compatibility test |
| AC-10 | Regression constraint | Existing test suite execution |

---

## Metadata

- **Total scenarios**: 28
- **Feature areas covered**: 9 (TUI Event Loop Responsiveness, Playlist Rendering Speed, Server Metadata Provision, TUI Track Construction, Shuffle Event Processing, Protobuf Schema, Title Population, Graceful Fallback, Table Performance)
- **Non-functional requirements addressed**: Performance (SCENARIO-001, SCENARIO-002, SCENARIO-003, SCENARIO-021, SCENARIO-022, SCENARIO-026), Reliability (SCENARIO-017, SCENARIO-018, SCENARIO-019, SCENARIO-020, SCENARIO-024, SCENARIO-025, SCENARIO-027), Observability (implied via timing verification in SCENARIO-004, SCENARIO-021)
- **Ambiguous items**: None — open questions from requirements (album modeling, backward compatibility with old server, podcast metadata, file_type) are documented as deferred concerns and do not affect current AC definitions
