# Code Review: Server-Side Podcast Synchronization

- **Date**: 2026-06-23
- **Author**: super-dev:code-reviewer
- **Verdict**: Approved
- **Files Reviewed**: 11

---

## Verdict: Approved

The implementation correctly delivers all 11 acceptance criteria and covers all 23 BDD scenarios. The code compiles cleanly, passes clippy with zero warnings, and all tests pass. The implementation follows established codebase patterns (mirrors `start_playlist_save_interval`), uses proper error isolation, and maintains backward compatibility. No critical or high-severity issues found. Two low-severity findings are noted as non-blocking recommendations for future improvement: a minor inefficiency with duplicate config reads and a feed concurrency bound reusing the download limit. Neither is blocking — approved as-is.

## Severity Counts

- Critical: 0
- High: 0
- Medium: 0
- Low: 2
- **Total**: 2

---

## Findings

### F-01: Two separate config reads at task startup could be consolidated

- Severity: Low
- File: `server/src/podcast_sync.rs`
- Line: 235
- Category: maintainability

At lines 235-236, `config.read()` is called twice in succession to read `interval` and `refresh_on_startup`:
```rust
let interval_duration = config.read().settings.synchronization.interval;
let refresh_on_startup = config.read().settings.synchronization.refresh_on_startup;
```
While not a correctness bug (config is effectively immutable at this point during startup), this acquires and releases the RwLock twice when a single read guard could extract both values. This is a minor inefficiency and readability concern.

**Recommendation**: Consolidate into a single read guard:
```rust
let (interval_duration, refresh_on_startup) = {
    let settings = &config.read().settings.synchronization;
    (settings.interval, settings.refresh_on_startup)
};
```

### F-02: Feed fetch TaskPool reuses concurrent_downloads_max for feed concurrency

- Severity: Low
- File: `server/src/podcast_sync.rs`
- Line: 74
- Category: correctness

The `feed_taskpool` at line 74 uses `concurrent_downloads_max` to bound feed fetch concurrency. While this is a reasonable default, the spec (Section 7.1) states "Download concurrency is bounded by `podcast.concurrent_downloads_max`" -- referring specifically to downloads, not feed fetches. Feed fetches are lightweight HTTP GETs for RSS XML, while downloads are large audio files. Reusing the same bound is conservative and safe but could unnecessarily limit feed throughput when `concurrent_downloads_max` is set to 1.

**Recommendation**: This is acceptable for the initial implementation. A future enhancement could add a separate `concurrent_feed_checks_max` config field if feed throughput becomes a concern.

---

## Dimension Scores

| Dimension | Score | Notes |
|-----------|-------|-------|
| Correctness | 5 | All logic paths verified correct. Deduplication via GUID/URL, error isolation per-podcast, channel-drain pattern, and graceful cancellation are all correctly implemented. |
| Security | 5 | No new attack surface. Feed URLs from user's own DB. Downloads restricted to configured directory. No network listeners added. |
| Performance | 4 | Minor: feed concurrency shares the download limit. All I/O is async. Timer uses interval_at to prevent drift. DB connection per-pass avoids lock contention. |
| Maintainability | 5 | Clean separation of concerns. Config in lib, sync logic in server module. Doc comments on all public API. Follows established codebase patterns. |
| Testability | 5 | Comprehensive test suite with mock HTTP servers (wiremock), temp databases, proper DI via parameters, clean interfaces. |
| Error Handling | 5 | Per-podcast and per-episode error isolation. Fatal errors propagated with context. Logging at appropriate levels (warn for recoverable, error for fatal). |

---

## BDD Scenario Coverage

- **SCENARIO-001**: Covered (synchronization_tests: default_config_when_synchronization_section_absent)
- **SCENARIO-002**: Covered (synchronization_tests: explicit_non_default_values_deserialized_correctly)
- **SCENARIO-003**: Covered (synchronization_tests: serialization_roundtrip_preserves_all_fields)
- **SCENARIO-004**: Covered (synchronization_tests: invalid_duration_string_produces_error)
- **SCENARIO-005**: Covered (podcast_sync tests: sync_task_not_spawned_when_disabled)
- **SCENARIO-006**: Covered (podcast_sync tests: start_podcast_sync_task_executes_startup_sync_when_enabled, integration_startup_sync_with_mock_server)
- **SCENARIO-007**: Covered (podcast_sync tests: start_podcast_sync_task_skips_startup_sync_when_disabled)
- **SCENARIO-008**: Covered (podcast_sync tests: start_podcast_sync_task_fires_periodic_sync_at_interval)
- **SCENARIO-009**: Covered (podcast_sync tests: start_podcast_sync_task_exits_on_cancellation, start_podcast_sync_task_cancellation_interrupts_interval_wait)
- **SCENARIO-010**: Covered (podcast_sync tests: integration_full_flow_fetches_downloads_and_enqueues_new_episodes, integration_downloads_only_new_episodes_when_some_already_exist)
- **SCENARIO-011**: Covered (podcast_sync tests: sync_once_skips_episodes_with_existing_guid, integration_deduplication_across_multiple_sync_passes)
- **SCENARIO-012**: Covered (podcast_sync tests: integration_deduplication_by_enclosure_url_fallback)
- **SCENARIO-013**: Covered (podcast_sync tests: sync_once_does_not_reenqueue_already_downloaded_episode, integration_deduplication_across_multiple_sync_passes)
- **SCENARIO-014**: Covered (podcast_sync tests: integration_full_flow_fetches_downloads_and_enqueues_new_episodes -- verifies files exist in download dir)
- **SCENARIO-015**: Covered (podcast_sync tests: playlist_add_track_for_sync_uses_at_end, integration_full_flow -- verifies AT_END index)
- **SCENARIO-016**: Covered (podcast_sync tests: integration_enqueue_format_enables_autostart_on_empty_queue)
- **SCENARIO-017**: Covered (podcast_sync tests: sync_once_unreachable_feed_increments_failed_continues, integration_http_500_on_one_feed_does_not_abort_others)
- **SCENARIO-018**: Covered (podcast_sync tests: integration_malformed_feed_xml_does_not_crash)
- **SCENARIO-019**: Covered (podcast_sync tests: integration_one_episode_download_fails_others_succeed)
- **SCENARIO-020**: Covered (podcast_sync tests: start_podcast_sync_task_mirrors_playlist_save_pattern -- verifies signature and spawn pattern)
- **SCENARIO-021**: Covered (podcast_sync tests: sync_once_no_podcasts_returns_ok_with_zero_stats, integration_empty_feed_completes_without_downloads)
- **SCENARIO-022**: Covered (podcast_sync tests: integration_sync_during_playback_appends_at_end)
- **SCENARIO-023**: Covered (podcast_sync tests: start_podcast_sync_task_fires_periodic_sync_at_interval -- uses interval_at semantics)

## Files Changed

- `Cargo.toml` -- modified, +3/-0
- `lib/Cargo.toml` -- modified, +1/-0
- `lib/src/config/v2/server/mod.rs` -- modified, +8/-0
- `lib/src/config/v2/server/synchronization.rs` -- created, +113/-0
- `lib/src/config/v2/server/synchronization_tests.rs` -- created, +350/-0
- `lib/src/lib.rs` -- modified, +3/-0
- `lib/src/player.rs` -- modified, +22/-0
- `lib/src/player_playlist_add_track_tests.rs` -- created, +269/-0
- `server/Cargo.toml` -- modified, +5/-0
- `server/src/podcast_sync.rs` -- created, +2551/-0
- `server/src/server.rs` -- modified, +15/-0

## Checklist

- [x] Code compiles without errors
- [x] All tests pass
- [x] No security vulnerabilities introduced
- [x] Naming conventions followed
- [x] Architecture patterns respected
