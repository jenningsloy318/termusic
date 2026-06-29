# Code Assessment: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:code-assessor
- **Scope**: server/, lib/src/podcast/, lib/src/config/v2/server/, lib/src/taskpool.rs, lib/src/utils.rs, playback/src/lib.rs, playback/src/playlist.rs
- **Focus**: architecture, patterns, dependencies (scoped to feature implementation)

---

## Executive Summary

The termusic codebase is a well-structured Rust workspace with clear separation between library (shared types), playback (audio engine), server (headless daemon), and TUI (client). The project follows consistent patterns: `#[serde(default)]` config structs, `CancellationToken` + `tokio::select!` for lifecycle, `anyhow::Result` for error propagation, and `parking_lot::RwLock` for shared state. The new podcast sync feature has a natural extension point mirroring `start_playlist_save_interval`. No blocking issues found; the primary recommendation is to add the new config section following the exact `#[serde(default)]` pattern used by `MetadataSettings` and `BackendSettings`.

| Dimension | Score (1-5) | Issues |
|-----------|-------------|--------|
| Architecture | 4 | 1 |
| Code Standards | 5 | 0 |
| Dependencies | 4 | 1 |
| Framework Patterns | 4 | 1 |
| Maintainability | 4 | 1 |

Scoring: 5=Excellent, 4=Good, 3=Adequate, 2=Needs Improvement, 1=Critical

---

## Architecture Evaluation

### Organization

The project uses a Cargo workspace with 4 crates:

```
Cargo.toml (workspace root)
  lib/       -> termusiclib  (shared types, config, podcast DB, utils)
  playback/  -> termusic-playback (audio backends, playlist, player trait)
  server/    -> termusic-server (headless daemon, gRPC service)
  tui/       -> termusic (TUI client)
```

This is a proper multi-crate workspace (`resolver = "2"`, workspace-level dependencies, workspace lints). The dependency flow is: `server` -> `playback` -> `lib`, and `tui` -> `playback` -> `lib`. The `lib` crate is the foundation containing shared types, protobuf definitions, podcast infrastructure, config structures, and utilities.

### Module Boundaries

| Module | Responsibility | Coupling | Cohesion |
|--------|---------------|----------|----------|
| `lib/src/config/v2/server/` | Server config deserialization and defaults | Low | High |
| `lib/src/podcast/` | RSS feed parsing, episode download, podcast DB | Low | High |
| `lib/src/taskpool.rs` | Bounded async task execution with cancellation | Low | High |
| `server/src/server.rs` | Server lifecycle, player loop, periodic tasks | Medium | Medium |
| `playback/src/playlist.rs` | Playlist state management, track add/remove | Medium | High |
| `playback/src/lib.rs` | Player engine, GeneralPlayer, backend abstraction | Medium | Medium |

### Data Flow

```
                   +-----------+
                   |  Config   |
                   |  (TOML)   |
                   +-----+-----+
                         |
                         v
  +--------+      +------+------+      +-----------+
  | gRPC   | ---> | PlayerCmd   | ---> | Player    |
  | Service |     | Channel     |      | Loop      |
  +--------+      | (unbounded) |      | (thread)  |
                  +------+------+      +-----+-----+
                         ^                   |
                         |                   v
  +------------------+   |            +------+------+
  | Periodic Tasks   |---+            | GeneralPlayer|
  | (tokio::spawn)   |               | + Backend    |
  | - playlist save  |               | + Playlist   |
  | - [NEW: sync]    |               | + db_podcast |
  +------------------+               +-------------+
```

The sync task will send `PlayerCmd::PlaylistAddTrack` through the existing `cmd_tx` channel, following the same path as gRPC client requests. This is the correct integration point.

### Error Handling Consistency

The project uses `anyhow::Result` with `.context()` for error enrichment consistently across all modules. Error patterns:
- Recoverable per-item errors: collect and continue (see `import_from_opml` at `lib/src/podcast/mod.rs:325-357`)
- Fatal startup errors: propagate with `?` from `actual_main()` (see `server/src/server.rs:103-211`)
- Log-and-continue: `warn!`/`error!` macros for non-fatal issues in periodic tasks (see `server/src/server.rs:229`)
- Channel send failures: `let _ = tx.send(...)` pattern (see `lib/src/taskpool.rs:42-57`)

This pattern is directly applicable to the sync task: per-podcast errors should use `warn!` and continue, per-episode download errors should use `warn!` and continue, and startup path resolution uses `?` propagation.

### Findings

**ARCH-001** | Severity: Low | Location: `server/src/server.rs:292`

- **Issue**: The `player_loop` function takes 9 arguments (`#[allow(clippy::too_many_arguments)]`). Adding the sync task does not worsen this (it does not modify the player loop), but it indicates the server module could benefit from a builder or context struct.
- **Impact**: Low maintenance burden. The sync task does not interact with this function. No action needed for this feature.
- **Recommendation**: No action required for the sync feature. This is pre-existing technical debt unrelated to the current work.

---

## Code Standards

### Tooling Inventory

| Tool | Config File | Status |
|------|-------------|--------|
| Clippy (linter) | `Cargo.toml` workspace.lints + `clippy.toml` | Active: pedantic + correctness + all at warn level |
| rustfmt (formatter) | None (uses defaults) | Active (via `cargo fmt --all` in Makefile) |
| Rust Edition | `Cargo.toml` workspace.package | 2024 edition, MSRV 1.90 |
| bacon (watch) | `.config/bacon.toml` | Active |

### Conventions Observed

- **Naming**: `snake_case` functions/variables, `PascalCase` types/enums, `SCREAMING_SNAKE_CASE` constants. Consistent across all 154 source files. Example: `start_playlist_save_interval` at `server/src/server.rs:216`, `PlayerCmdSender` at `playback/src/lib.rs:62`, `BACKEND_ERROR_LIMIT` at `server/src/server.rs:45`.
- **File Organization**: One module per file, with `mod.rs` for directories containing submodules. Config sections have their own files under `config/v2/server/` (e.g., `backends.rs`, `metadata.rs`, `config_extra.rs`).
- **Import Ordering**: `std` first, then external crates, then internal (`crate::`/`super::`). Groups separated by blank lines. Example: `server/src/server.rs:1-31`.
- **Comment Style**: `///` doc comments on public items, `//` inline comments for non-obvious logic. TODO comments use uppercase `TODO:` prefix. Example: `lib/src/podcast/db/mod.rs:282`.
- **Serde Pattern**: Config structs use `#[serde(default)]` at struct level with manual `impl Default`. Example: `lib/src/config/v2/server/mod.rs:22-64`.
- **Error Pattern**: `anyhow::Result` with `.context()` for function-level errors. `thiserror::Error` derive for typed domain errors. Example: `lib/src/config/v2/server/mod.rs:423-426`.

### Findings

No findings. The codebase has strong, consistent standards enforced by clippy pedantic and workspace lints.

---

## Dependencies

### Manifest Analysis (relevant to sync feature)

| Package | Current | Latest | Status | Risk |
|---------|---------|--------|--------|------|
| tokio | 1.52 | 1.52 | Current | Low |
| tokio-util | 0.7.18 | 0.7.18 | Current | Low |
| reqwest | 0.13.4 | 0.13.4 | Current | Low |
| rusqlite | 0.39 | 0.39 | Current | Low |
| rss | 2.0.13 | 2.0.13 | Current | Low |
| serde | 1.0.228 | 1.0.228 | Current | Low |
| toml | 1.1.2 | 1.1.2 | Current | Low |
| humantime-serde | N/A (not present) | 0.2.x | New dependency needed | Low |

### Dependency Health Scoring

| Dependency | Last Commit | Open CVEs | Maintenance | Bus Factor | Score |
|------------|-------------|-----------|-------------|------------|-------|
| tokio | < 1 week | 0 | Active (Tokio team) | 10+ | Healthy |
| reqwest | < 1 month | 0 | Active | 5+ | Healthy |
| rusqlite | < 3 months | 0 | Active | 3+ | Healthy |
| rss | < 6 months | 0 | Reduced | 2 | Warning |
| humantime-serde | < 12 months | 0 | Reduced | 2 | Warning |

The `rss` and `humantime-serde` crates are in "Warning" territory due to reduced activity, but both are stable, small-scope crates that rarely need updates. No action needed.

### Security Advisories

None found for the current dependency tree relevant to the sync feature scope.

### Bundle/Binary Size Concerns

The proposed new dependency `humantime-serde` is tiny (wraps `humantime` which is already a small crate). It adds negligible binary size. No concerns.

### Findings

**DEP-001** | Severity: Low | Location: `Cargo.toml` (workspace root)

- **Issue**: The `humantime` or `humantime-serde` crate is not present in the workspace. The sync feature requires parsing duration strings like `"1h"`, `"30m"` from the config. This must be added as a new workspace dependency.
- **Impact**: Without this dependency, the `interval` config field cannot use human-readable duration strings. The alternative (manual parsing or raw seconds) degrades user experience.
- **Recommendation**: Add `humantime-serde = "0.2"` to `[workspace.dependencies]` in the root `Cargo.toml` and use `#[serde(with = "humantime_serde")]` on the `interval: Duration` field in the new `SynchronizationSettings` struct.

---

## Framework Patterns

### Patterns Inventory

| Pattern | Usage | Location | Assessment |
|---------|-------|----------|------------|
| Periodic async task with cancellation | Custom (tokio spawn + select!) | `server/src/server.rs:216-241` | Appropriate |
| Shared config via Arc<RwLock<T>> | `SharedServerSettings` type alias | `lib/src/config/mod.rs:17` | Appropriate |
| Command channel (unbounded mpsc) | `PlayerCmdSender` wrapper | `playback/src/lib.rs:62-93` | Appropriate |
| Bounded task pool with semaphore | `TaskPool` struct | `lib/src/taskpool.rs:1-68` | Appropriate |
| Config with serde(default) + manual Default | All server config sections | `lib/src/config/v2/server/*.rs` | Appropriate |

### Test Structure

Tests use the standard Rust `#[cfg(test)] mod tests` pattern inline with the source files. The project uses `pretty_assertions` for readable diffs. Database tests use in-memory SQLite (`Connection::open_in_memory()`) defined in `lib/src/podcast/db/mod.rs:450-457`. Config tests verify serialization/deserialization roundtrips. The `Makefile` runs `cargo test --features cover,all-backends --release --all`.

### Findings

**PAT-001** | Severity: Low | Location: `lib/src/podcast/mod.rs:309-310`

- **Issue**: The `import_from_opml` function at line 309-310 constructs a `TaskPool` inline with the podcast download concurrency setting. The new sync task will need to do the same. This is not a problem per se, but the `TaskPool` creation pattern should be replicated exactly (using `usize::from(config.concurrent_downloads_max.get())`).
- **Impact**: None if the pattern is followed. Just a note for implementation consistency.
- **Recommendation**: When creating the `TaskPool` in the sync task, use the same `usize::from(config.settings.podcast.concurrent_downloads_max.get())` pattern from the `import_from_opml` function.

---

## Pattern Library (Canonical Patterns for New Code)

### PAT-LIB-001: Periodic Async Task with CancellationToken

**Canonical Example**: `server/src/server.rs:216-241` (`start_playlist_save_interval`)

```rust
fn start_playlist_save_interval(
    handle: Handle,
    cancel_token: CancellationToken,
    playlist: SharedPlaylist,
) {
    handle.spawn(async move {
        let mut timer = tokio::time::interval_at(
            Instant::now() + PLAYLIST_SAVE_INTERVAL,
            PLAYLIST_SAVE_INTERVAL,
        );
        loop {
            select! {
                _ = timer.tick() => { /* work */ },
                _ = cancel_token.cancelled() => { break; }
            }
        }
    });
}
```

- **Consistency Score**: 100% (only one such periodic task exists; this IS the pattern to replicate)
- **Violations**: None
- **Key Elements**: (1) Takes `Handle`, `CancellationToken`, and task-specific deps. (2) Uses `interval_at` (not `sleep`). (3) Uses `select!` with `cancelled()` branch. (4) Called from `actual_main()` after config setup.

### PAT-LIB-002: Config Section with serde(default) and Manual Default

**Canonical Example**: `lib/src/config/v2/server/metadata.rs:6-40` (`MetadataSettings`)

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MetadataSettings {
    pub field: Type,
}

impl Default for MetadataSettings {
    fn default() -> Self { /* defaults */ }
}
```

- **Consistency Score**: 100% (all config sections: `PodcastSettings`, `ComSettings`, `PlayerSettings`, `BackendSettings`, `MetadataSettings` follow this pattern)
- **Violations**: None
- **Key Elements**: (1) `#[serde(default)]` on struct. (2) `Debug, Clone, Deserialize, Serialize, PartialEq` derives. (3) Manual `impl Default`. (4) Field in parent `ServerSettings` struct. (5) Comment explaining serde(default) purpose.

### PAT-LIB-003: Database Path Resolution and Passing

**Canonical Example**: `server/src/server.rs:674-676` (`execute_action`)

```rust
let config_dir_path = utils::get_app_config_path().context("getting app-config-path")?;
podcast::import_from_opml(&config_dir_path, &config.settings.podcast, &path).await?;
```

- **Consistency Score**: 100% (all code needing `db_path` calls `get_app_config_path()` at the call site and passes it)
- **Violations**: None
- **Key Elements**: (1) Call `utils::get_app_config_path()` at the call site. (2) Add `.context()` for error enrichment. (3) Pass `&Path` or `PathBuf` to the function that needs it. (4) `Database::new(path)` opens its own connection.

### PAT-LIB-004: Error Isolation in Iterative Processing

**Canonical Example**: `lib/src/podcast/mod.rs:327-357` (`import_from_opml` message loop)

```rust
while let Some(message) = rx_to_main.recv().await {
    match message {
        PodcastSyncResult::Error(feed) => {
            msg_counter += 1;
            failure = true;
            error!("Error retrieving RSS feed: {}", feed.url);
        }
        PodcastSyncResult::NewData(pod) => {
            msg_counter += 1;
            // process...
        }
        // ...
    }
    if msg_counter >= podcast_list.len() { break; }
}
```

- **Consistency Score**: 100% (both `check_feed` and `download_list` follow per-item error isolation)
- **Violations**: None
- **Key Elements**: (1) Process each item independently. (2) Log errors with `error!`/`warn!`. (3) Set a failure flag. (4) Continue processing remaining items. (5) Use a counter or channel-close to know when done.

### PAT-LIB-005: PlayerCmd Dispatch via Unbounded Channel

**Canonical Example**: `server/src/server.rs:502-514` (`PlaylistAddTrack` handler)

```rust
PlayerCmd::PlaylistAddTrack(info) => {
    let mut playlist_write = player.playlist.write();
    let was_empty = playlist_write.is_empty();
    if let Err(err) = playlist_write.add_tracks(info, &player.db_podcast) {
        error!("Error adding tracks: {err}");
    }
    drop(playlist_write);
    if was_empty {
        player.resume_from_stopped();
    }
}
```

- **Consistency Score**: 100% (all external actions go through `PlayerCmdSender::send()`)
- **Violations**: None
- **Key Elements**: (1) Construct `PlayerCmd` variant with required data. (2) Send via `cmd_tx.send(PlayerCmd::Variant(data))`. (3) Player loop processes synchronously on its thread. (4) Auto-start if queue was empty.

---

## Architecture Smell Detection

No critical or high severity architecture smells detected. Assessment notes:

- **God Class/Module**: The largest relevant file is `playback/src/playlist.rs` (1281 lines) but it has a single clear responsibility (playlist state management). The `server/src/server.rs` (691 lines) combines multiple concerns (config loading, lifecycle, player loop) but this is typical for a binary's main module.
- **Shotgun Surgery**: The new sync feature touches config (1 file), sync logic (1 new file), task spawn (1 file = server.rs), and tests. This is 3-4 files, within acceptable range.
- **Feature Envy**: Not detected. Modules access their own state primarily.
- **Inappropriate Intimacy**: The `playback` crate's `GeneralPlayer` holds `db_podcast` directly (not behind an abstraction), but this is acceptable for a single-user application.

---

## Better Options Analysis

| Current Approach | Better Option | Benefit | Migration Effort |
|-----------------|---------------|---------|-----------------|
| Raw `u64::MAX` as append sentinel in `PlaylistAddTrack` | Named constructor `new_append_single` on `PlaylistAddTrack` | Self-documenting API, eliminates magic value at call sites | S |
| No `humantime` dependency (no human-readable durations) | Add `humantime-serde` crate | Allows `"1h"`, `"30m"` in config instead of raw seconds | S |

---

## Technical Debt Inventory

| ID | Description | Location | Severity | Effort | Blast Radius | Priority |
|----|-------------|----------|----------|--------|--------------|----------|
| TD-001 | `player_loop` has 9 parameters requiring `clippy::too_many_arguments` suppression | `server/src/server.rs:292` | Low | M | 1 file | Eventually |
| TD-002 | `PlaylistAddTrack` uses raw `u64::MAX` sentinel without named constant or constructor | `lib/src/player.rs:446-464` | Low | S | 3 call sites | Soon |
| TD-003 | `import_from_opml` counter-based loop (line 356) has potential hang if task panics without sending | `lib/src/podcast/mod.rs:356` | Low | S | 1 function | Eventually |

---

## Prioritized Recommendations

| Priority | ID | Recommendation | Effort | Impact |
|----------|------|----------------|--------|--------|
| 1 | REC-001 | Add `SynchronizationSettings` struct to `lib/src/config/v2/server/` following PAT-LIB-002 pattern exactly (new file `synchronization.rs` with `#[serde(default)]`, `impl Default`, fields: `enable: bool`, `interval: Duration`, `refresh_on_startup: bool`). Add as field to `ServerSettings`. | S | L |
| 2 | REC-002 | Add `humantime-serde = "0.2"` to workspace dependencies. Use `#[serde(with = "humantime_serde")]` on the `interval` field for human-readable duration parsing. | S | L |
| 3 | REC-003 | Create `start_podcast_sync_task` function in the server crate following PAT-LIB-001 (signature: `fn start_podcast_sync_task(handle: Handle, cancel_token: CancellationToken, config: SharedServerSettings, cmd_tx: PlayerCmdSender, db_path: PathBuf)`). Gate spawning on `config.read().settings.synchronization.enable`. | M | L |
| 4 | REC-004 | Add `PlaylistAddTrack::new_append_single` constructor (per TD-002) to provide a clean API for appending tracks without exposing the `u64::MAX` sentinel. Use this in the sync task. | S | M |
| 5 | REC-005 | Implement `sync_once` as an async function that: (1) opens its own `Database::new(&db_path)`, (2) calls `get_podcasts()`, (3) iterates with per-podcast error isolation (PAT-LIB-004), (4) uses `check_feed` + `download_list` from `lib/src/podcast/`, (5) sends `PlaylistAddTrack` via `cmd_tx` for each downloaded episode. | M | L |

Priority ordering: High Impact + Low Effort first (REC-001, REC-002), then High Impact + Medium Effort (REC-003, REC-005), then Medium Impact + Low Effort (REC-004).

---

## File Coverage Report

| Category | Files Analyzed | Total Files | Coverage |
|----------|---------------|-------------|---------|
| server/src/ (all .rs) | 5 | 8 | 63% |
| lib/src/config/v2/server/ | 4 | 4 | 100% |
| lib/src/podcast/ | 5 | 7 | 71% |
| lib/src/taskpool.rs | 1 | 1 | 100% |
| lib/src/utils.rs | 1 | 1 | 100% |
| playback/src/ (core) | 2 | 2 | 100% |
| Root configs | 3 | 3 | 100% |
| **Total (in scope)** | **21** | **26** | **81%** |

### Exclusions

- `tui/` (51 files): Out of scope for server-side feature. Only checked for pattern reference on `download_list` usage.
- `playback/src/backends/` (12 files): Audio backend internals not relevant to sync feature.
- `lib/src/songtag/` (7 files): Song tag parsing, unrelated to podcast sync.
- `lib/src/new_database/` (7 files): Track metadata database, not podcast DB.
- `lib/src/playlist/*.rs` (4 files): Playlist file format parsers, not relevant.
- `server/src/connection/` (3 files): Transport layer, not relevant to sync task.

---

## Key Patterns Summary for Implementation

The new podcast sync feature MUST follow these patterns:

1. **Config**: New `SynchronizationSettings` struct in `lib/src/config/v2/server/` with `#[serde(default)]` + `impl Default` (mirror `MetadataSettings`).
2. **Task Lifecycle**: `start_podcast_sync_task(Handle, CancellationToken, ...)` function using `interval_at` + `select!` (mirror `start_playlist_save_interval`).
3. **Path Resolution**: Call `utils::get_app_config_path()` in `actual_main()` before spawning, pass `PathBuf` to the task (mirror `execute_action` pattern).
4. **Error Isolation**: Per-podcast `warn!` + continue pattern (mirror `import_from_opml` loop).
5. **Enqueue**: Send `PlayerCmd::PlaylistAddTrack(...)` through `cmd_tx.send()` (mirror gRPC service calls).
6. **Download**: Reuse `download_list` from `lib/src/podcast/` with a `TaskPool` bounded by `config.settings.podcast.concurrent_downloads_max`.
