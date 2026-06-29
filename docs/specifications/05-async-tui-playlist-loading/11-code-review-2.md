# Code Review: Async TUI Playlist Loading

- **Date**: 2026-06-27
- **Author**: super-dev:code-reviewer
- **Verdict**: Approved
- **Files Reviewed**: 14

---

## Verdict: Approved

The implementation correctly eliminates all TUI-side disk I/O during playlist loading by extending the gRPC protocol with full display metadata and rewriting `load_from_grpc` to use a pure in-memory `Track::from_grpc_metadata` constructor. All 10 acceptance criteria are met. The only findings are Low/Info severity (observability gap and stale annotation) which do not impact correctness, security, or performance.

## Severity Counts

- Critical: 0
- High: 0
- Medium: 0
- Low: 2
- **Total**: 2

---

## Findings

### F-01: LoadStats computed but never logged at call sites

- Severity: Low
- File: `tui/src/ui/model/update.rs`
- Line: 1129
- Category: observability

The spec (Section 7.3) requires: "TUI logs timing of playlist response processing at INFO level: Processed {count} tracks in {elapsed_ms}ms". The `load_from_grpc` method correctly computes and returns `LoadStats { track_count, elapsed }`, but both production call sites discard the `Ok(LoadStats)` value:
1. `update.rs:1129` uses `if let Err(err) = ...` which discards the success case
2. `components/playlist.rs:520` uses `?` which propagates error but discards success

The timing data is never actually emitted to the log.

**Recommendation**: At both call sites, capture the `LoadStats` and log it:
```rust
match self.playback.load_from_grpc(playlist_tracks) {
    Ok(stats) => info!("Processed {} tracks in {:?}", stats.track_count, stats.elapsed),
    Err(err) => self.mount_error_popup(err),
}
```

### F-02: Stale dead_code annotation on insert_track_at

- Severity: Low
- File: `tui/src/ui/model/playlist.rs`
- Line: 24
- Category: maintainability

The `#[allow(dead_code)]` annotation and its comment "Used in Phase 3 (TUI playlist loading rewrite)" is stale. Phase 3 has been completed and `insert_track_at` IS now called from production code (`tui/src/ui/components/playlist.rs:460`). The annotation suppresses a compiler warning that would no longer fire.

**Recommendation**: Remove `#[allow(dead_code)] // Used in Phase 3 (TUI playlist loading rewrite)` since the method has an active caller.

---

## Dimension Scores

| Dimension | Score | Notes |
|-----------|-------|-------|
| Correctness | 5 | All logic paths verified correct; Track construction handles all three source variants; edge cases (empty playlist, missing metadata, usize::MAX index) properly handled |
| Security | 5 | No new attack surface; all data flows over existing gRPC channel between co-located processes; no user-facing input validation gaps |
| Performance | 5 | Zero disk I/O achieved; sub-millisecond processing for 1000+ tracks validated by tests; Vec pre-allocation used correctly |
| Maintainability | 4 | Clear naming, good documentation on constructor. Minor: stale dead_code annotation. Overall well-structured with clean separation of concerns |
| Testability | 5 | Excellent test coverage (131 new tests across 4 test modules); LoadStats return type enables programmatic verification; non-existent path test proves zero-I/O structurally |
| Error Handling | 4 | Proper bail!/context usage; graceful fallback for missing metadata; observability gap (LoadStats not logged) is the only minor gap |
| Concurrency | 5 | No shared mutable state; TUI playlist is single-threaded event-loop model; no race conditions possible in the new code |
| Data Integrity | 5 | Playlist replacement is atomic (set_tracks); individual insertions use insert_track_at with bounds checking; no partial update risk |
| Observability | 3 | LoadStats is computed but not logged at call sites; pre-existing INFO log exists but doesn't include timing/count data; spec requirement partially unmet |

---

## BDD Scenario Coverage

- **SCENARIO-001**: Covered (perf_load_from_grpc_1000_tracks_under_100ms)
- **SCENARIO-002**: Covered (perf_small_playlist_50_tracks)
- **SCENARIO-003**: Covered (perf_shuffle_event_1000_tracks_under_100ms)
- **SCENARIO-004**: Covered (perf_combined_load_and_sync_1000_tracks_under_200ms)
- **SCENARIO-005**: Covered (e2e_title_from_metadata_preferred_over_path)
- **SCENARIO-006**: Covered (e2e_server_proto_output_to_load_from_grpc_preserves_all_metadata)
- **SCENARIO-007**: Covered (serialization_round_trip_playlist_shuffled_preserves_metadata)
- **SCENARIO-008**: Covered (e2e_individual_track_add_event_full_metadata)
- **SCENARIO-009**: Covered (e2e_server_populates_title_not_none)
- **SCENARIO-010**: Covered (e2e_server_proto_output_to_load_from_grpc_preserves_all_metadata)
- **SCENARIO-011**: Covered (structural_no_disk_access_nonexistent_paths)
- **SCENARIO-012**: Covered (e2e_shuffle_event_reorders_playlist_from_metadata)
- **SCENARIO-013**: Covered (e2e_multiple_rapid_shuffles_no_disk_io)
- **SCENARIO-014**: Covered (serialization_round_trip_with_absent_new_fields)
- **SCENARIO-015**: Covered (e2e_server_populates_title_not_none)
- **SCENARIO-016**: Covered (e2e_filename_derived_title_from_server)
- **SCENARIO-017**: Covered (e2e_all_missing_metadata_filename_fallback)
- **SCENARIO-018**: Covered (e2e_partial_metadata_path_and_duration_only)
- **SCENARIO-019**: Covered (e2e_missing_duration_displays_gracefully)
- **SCENARIO-020**: Covered (e2e_partial_metadata_path_and_duration_only)
- **SCENARIO-021**: Covered (perf_playlist_sync_data_access_1000_tracks_under_50ms)
- **SCENARIO-022**: Covered (perf_playlist_sync_linear_scaling)
- **SCENARIO-023**: Covered (regression_mixed_operations_sequence)
- **SCENARIO-024**: Covered (e2e_empty_playlist_no_error)
- **SCENARIO-025**: Covered (e2e_all_missing_metadata_filename_fallback)
- **SCENARIO-026**: Covered (perf_load_from_grpc_5000_tracks_under_100ms)
- **SCENARIO-027**: Covered (e2e_sequential_reload_and_shuffle_consistent_final_state)
- **SCENARIO-028**: Covered (e2e_extremely_long_metadata_no_overflow)

## Files Changed

- `lib/proto/player.proto` — modified, +5/-0
- `lib/src/lib.rs` — modified, +3/-0
- `lib/src/player.rs` — modified, +13/-0
- `lib/src/track.rs` — modified, +52/-0
- `playback/src/playlist.rs` — modified, +49/-4
- `tui/src/ui/model/mod.rs` — modified, +75/-27
- `tui/src/ui/model/playlist.rs` — modified, +12/-80
- `lib/src/async_tui_phase1_tests.rs` — created, +617/-0
- `tui/src/ui/model/async_tui_phase1_playlist_tests.rs` — created, +191/-0
- `playback/tests/phase2_server_metadata_population_tests.rs` — created, +854/-0
- `tui/src/ui/model/update.rs` — modified, +1/-4
- `tui/src/ui/components/playlist.rs` — modified, +23/-14
- `tui/src/ui/model/async_tui_phase3_tests.rs` — created, +896/-0
- `tui/src/ui/model/async_tui_loading_tests.rs` — created, +1537/-0

## Checklist

- [x] Code compiles without errors
- [x] All tests pass
- [x] No security vulnerabilities introduced
- [x] Naming conventions followed
- [x] Architecture patterns respected
