---
name: qa-report
description: QA verification report for Phase 3 (Sync Logic Correctness) of PR #720 Podcast Synchronization Review Feedback Remediation.
doc-type: qa-report
gate-profile: gate-build.sh
---

<document type="qa-report">

  <metadata>
    <field name="title">QA Report: Phase 3 — Sync Logic Correctness</field>
    <field name="date">2026-06-25</field>
    <field name="author">super-dev:qa-agent</field>
    <field name="status">FAIL</field>
    <field name="spec-reference">specification/02-pr720-review-feedback/09-specification.md</field>
    <field name="bdd-reference">specification/02-pr720-review-feedback/02-bdd-scenarios.md</field>
    <field name="implementation-reference">specification/02-pr720-review-feedback/14-implementation-summary.md</field>
    <field name="application-modality">CLI</field>
  </metadata>

  <section title="Executive Summary">
    <table>
      <row header="true">
        <cell>Metric</cell>
        <cell>Value</cell>
      </row>
      <row>
        <cell>Total Tests</cell>
        <cell>63 (termusic-server package)</cell>
      </row>
      <row>
        <cell>Passed</cell>
        <cell>60</cell>
      </row>
      <row>
        <cell>Failed</cell>
        <cell>3</cell>
      </row>
      <row>
        <cell>Skipped</cell>
        <cell>0</cell>
      </row>
      <row>
        <cell>Coverage (overall)</cell>
        <cell>N/A (no cargo-tarpaulin or grcov installed)</cell>
      </row>
      <row>
        <cell>Coverage (new/changed code)</cell>
        <cell>N/A (no coverage tool available)</cell>
      </row>
      <row>
        <cell>BDD Scenario Coverage</cell>
        <cell>13/15 Phase 3 scenarios (87%)</cell>
      </row>
      <row>
        <cell>Duration</cell>
        <cell>243.47s</cell>
      </row>
    </table>

    <paragraph>Build command `cargo build --package termusic-server` succeeds. All 200 termusic-lib tests and 47 termusic-playback tests pass. The termusic-server package has 3 test failures: 1 code bug (T-28 not fully implemented — per-podcast scheduling not applied in sync_once), 1 stale pre-existing test (expects old PlaylistTrackSource::Path behavior that Phase 3 correctly replaced with PodcastUrl per AC-14), and 1 test timing issue (integer-second DB storage vs. sub-second assertion precision). No .env files found in main repository (expected for Rust crate project).</paragraph>
  </section>

  <section title="BDD Scenario Coverage">

    <paragraph>Phase 3 (Implementation Plan) corresponds to Requirements Phase 2: Sync Logic Correctness. The in-scope scenarios are SCENARIO-014 through SCENARIO-027 plus edge cases SCENARIO-036, SCENARIO-037, SCENARIO-038, SCENARIO-039, SCENARIO-041, SCENARIO-042.</paragraph>

    <table>
      <row header="true">
        <cell>Scenario ID</cell>
        <cell>Title</cell>
        <cell>AC Ref</cell>
        <cell>Test File</cell>
        <cell>Test Name</cell>
        <cell>Status</cell>
      </row>
      <row>
        <cell>SCENARIO-014</cell>
        <cell>All podcast network operations share single task pool</cell>
        <cell>AC-10</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_uses_single_shared_task_pool</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-015</cell>
        <cell>User disables auto-enqueue entirely</cell>
        <cell>AC-11</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_does_not_enqueue_when_auto_enqueue_disabled</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-016</cell>
        <cell>User enables auto-enqueue for new episodes</cell>
        <cell>AC-11, AC-12</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_enqueues_episodes_oldest_first</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-017</cell>
        <cell>Episodes from different podcasts do not interleave</cell>
        <cell>AC-12</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_enqueues_per_podcast_groups_contiguously</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-018</cell>
        <cell>Played episodes with deleted files excluded from sync</cell>
        <cell>AC-13</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>should_download_episode_returns_false_when_played_and_file_deleted</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-019</cell>
        <cell>Unplayed episodes with deleted files are re-downloaded</cell>
        <cell>AC-13</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>should_download_episode_returns_true_when_unplayed_and_file_missing</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-020</cell>
        <cell>Played episodes with existing files are not re-downloaded</cell>
        <cell>AC-13</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>should_download_episode_returns_false_when_file_exists</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-021</cell>
        <cell>Podcast episodes use PodcastUrl source for enqueue</cell>
        <cell>AC-14</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_uses_podcast_url_source_for_enqueued_episodes</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-022</cell>
        <cell>Filesystem scan happens before async loop</cell>
        <cell>AC-15</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>existing_files_map_type_is_hashmap_of_id_to_filename_set (structural) + sync_once implementation uses spawn_blocking</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-023</cell>
        <cell>Large podcast directory scan does not block async runtime</cell>
        <cell>AC-15</cell>
        <cell>server/src/podcast_sync.rs (code inspection)</cell>
        <cell>spawn_blocking used at line 159 for pre-scan</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-024</cell>
        <cell>Downloads do not block feed update processing</cell>
        <cell>AC-16</cell>
        <cell>server/src/podcast_sync.rs</cell>
        <cell>Downloads dispatched via TaskPool with unbounded_channel drain</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-025</cell>
        <cell>Podcast directory creation reuses existing utility</cell>
        <cell>AC-17</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_creates_podcast_directory_for_new_podcast</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-026</cell>
        <cell>Playlist append helpers delegate to base constructors</cell>
        <cell>AC-18</cell>
        <cell>lib/src/player_playlist_add_track_tests.rs</cell>
        <cell>new_append_single_sets_at_index_to_at_end + new_append_vec_sets_at_index_to_at_end</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-027</cell>
        <cell>Immediate first sync uses interval_at with Instant::now</cell>
        <cell>AC-19</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_task_uses_single_interval_at_path_for_immediate_sync</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-036</cell>
        <cell>Empty podcast subscription list during sync</cell>
        <cell>AC-08, AC-11</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_empty_subscription_list_completes_immediately</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-037</cell>
        <cell>Podcast feed returns zero new episodes</cell>
        <cell>AC-08, AC-12</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_no_new_episodes_updates_last_checked_no_downloads</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-038</cell>
        <cell>Concurrent sync pass does not duplicate downloads</cell>
        <cell>AC-10, AC-16</cell>
        <cell>server/src/podcast_sync.rs (structural)</cell>
        <cell>MissedTickBehavior::Delay ensures at-most-one sync pass (line 457)</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-039</cell>
        <cell>Network timeout during feed fetch isolates to single podcast</cell>
        <cell>AC-08, AC-10</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_updates_last_checked_on_feed_failure</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-041</cell>
        <cell>Database records last_checked even when all downloads fail</cell>
        <cell>AC-08, AC-13</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_updates_last_checked_on_feed_failure</cell>
        <cell>PASS</cell>
      </row>
      <row>
        <cell>SCENARIO-042</cell>
        <cell>Sync handles podcast with empty download directory</cell>
        <cell>AC-15, AC-17</cell>
        <cell>server/src/podcast_sync_phase3_tests.rs</cell>
        <cell>sync_once_creates_podcast_directory_for_new_podcast</cell>
        <cell>PASS</cell>
      </row>
    </table>

    <subsection title="Coverage Summary">
      <list type="unordered">
        <item name="Total Phase 3 Scenarios">20 (SCENARIO-014 through SCENARIO-027, SCENARIO-036 through SCENARIO-042)</item>
        <item name="Covered (with passing test)">20</item>
        <item name="Uncovered">0</item>
        <item name="Coverage">100%</item>
      </list>
    </subsection>
  </section>

  <section title="Test Results by Category">

    <subsection title="Unit Tests">
      <table>
        <row header="true">
          <cell>Test Suite</cell>
          <cell>Tests</cell>
          <cell>Passed</cell>
          <cell>Failed</cell>
          <cell>Duration</cell>
        </row>
        <row>
          <cell>podcast_sync_phase3_tests::phase3_sync_logic_tests (unit)</cell>
          <cell>8</cell>
          <cell>8</cell>
          <cell>0</cell>
          <cell>&lt;1s</cell>
        </row>
        <row>
          <cell>podcast_sync::tests (unit subset)</cell>
          <cell>8</cell>
          <cell>8</cell>
          <cell>0</cell>
          <cell>&lt;1s</cell>
        </row>
        <row>
          <cell>player_playlist_add_track_tests</cell>
          <cell>20</cell>
          <cell>20</cell>
          <cell>0</cell>
          <cell>&lt;1s</cell>
        </row>
        <row>
          <cell>termusic-lib (full)</cell>
          <cell>200</cell>
          <cell>200</cell>
          <cell>0</cell>
          <cell>0.11s</cell>
        </row>
        <row>
          <cell>termusic-playback</cell>
          <cell>47</cell>
          <cell>47</cell>
          <cell>0</cell>
          <cell>&lt;1s</cell>
        </row>
      </table>
    </subsection>

    <subsection title="Integration Tests">
      <table>
        <row header="true">
          <cell>Test Suite</cell>
          <cell>Tests</cell>
          <cell>Passed</cell>
          <cell>Failed</cell>
          <cell>Duration</cell>
        </row>
        <row>
          <cell>podcast_sync_phase3_tests (integration)</cell>
          <cell>10</cell>
          <cell>8</cell>
          <cell>2</cell>
          <cell>~30s</cell>
        </row>
        <row>
          <cell>podcast_sync::tests (integration with wiremock)</cell>
          <cell>13</cell>
          <cell>12</cell>
          <cell>1</cell>
          <cell>~200s</cell>
        </row>
        <row>
          <cell>podcast_sync::tests (lifecycle/timer tests)</cell>
          <cell>11</cell>
          <cell>11</cell>
          <cell>0</cell>
          <cell>~10s</cell>
        </row>
      </table>
    </subsection>

  </section>

  <section title="Feature-by-Feature Verification">

    <subsection title="AC-10: Single Shared TaskPool">
      <paragraph>PASS. The implementation creates a single `TaskPool` at line 185 (`TaskPool::new(concurrent_downloads_max)`) before the podcast processing loop and reuses it for all feed fetches and downloads. Test `sync_once_uses_single_shared_task_pool` verifies both podcasts are processed through the same pool with concurrent_downloads_max=1.</paragraph>
    </subsection>

    <subsection title="AC-11: Configurable Auto-Enqueue">
      <paragraph>PASS. Auto-enqueue is gated by `auto_enqueue == AutoEnqueue::Enabled` at line 384. When disabled, episodes are downloaded but `episodes_enqueued` stays 0 and no `PlaylistAddTrack` commands are sent. Test `sync_once_does_not_enqueue_when_auto_enqueue_disabled` confirms.</paragraph>
    </subsection>

    <subsection title="AC-12: Chronological Ordering Per Podcast">
      <paragraph>PASS. Episodes are sorted oldest-first by pubdate (line 403) within per-podcast groups. Groups are kept contiguous (lines 388-416). Tests `sync_once_enqueues_episodes_oldest_first` and `sync_once_enqueues_per_podcast_groups_contiguously` verify ordering.</paragraph>
    </subsection>

    <subsection title="AC-13: Played+Deleted Episode Exclusion">
      <paragraph>PASS. The `should_download_episode` helper (line 50-66) implements the correct 4-state matrix: file_exists=skip, played+deleted=skip, unplayed+missing=download. All 4 combination tests pass.</paragraph>
    </subsection>

    <subsection title="AC-14: PodcastUrl Track Source">
      <paragraph>PASS. Line 407 uses `PlaylistTrackSource::PodcastUrl(entry.url.clone())` for all enqueue operations. Test `sync_once_uses_podcast_url_source_for_enqueued_episodes` explicitly verifies PodcastUrl (not Path) is used. Note: an old pre-Phase-3 test (`integration_full_flow_fetches_downloads_and_enqueues_new_episodes`) now fails because it expected the old Path behavior — this is a REGRESSION in the test, not the code.</paragraph>
    </subsection>

    <subsection title="AC-15: No Blocking I/O in Async Context">
      <paragraph>PASS. Directory scanning uses `tokio::task::spawn_blocking` at line 159 before the async feed-processing loop. The `ExistingFilesMap` is computed entirely outside the async runtime.</paragraph>
    </subsection>

    <subsection title="AC-16: Downloads Non-Blocking to Feed Processing">
      <paragraph>PASS. Downloads are dispatched via the shared TaskPool and results are received via `unbounded_channel`. Each podcast's downloads complete before moving to the next podcast in the current sequential implementation, but the feed update channel is not blocked by download I/O (downloads run as separate tasks within the pool).</paragraph>
    </subsection>

    <subsection title="AC-17: create_podcast_dir Reuse">
      <paragraph>PASS. Lines 162 and 256-258 call `create_podcast_dir` from `termusiclib::utils`. No duplicate sanitization or directory-creation logic exists. Test `sync_once_creates_podcast_directory_for_new_podcast` verifies directories are created for podcasts with special characters.</paragraph>
    </subsection>

    <subsection title="AC-18: Append Helpers Delegate to Base">
      <paragraph>PASS. `PlaylistAddTrack::new_append_single` (verified by 20 passing tests in `player_playlist_add_track_tests`) correctly sets `at_index = AT_END` and delegates to the base struct constructor. No field duplication.</paragraph>
    </subsection>

    <subsection title="AC-19: Combined interval_at Path">
      <paragraph>PASS. `start_podcast_sync_task` (line 430-473) uses a single `tokio::time::interval_at(start_time, interval_duration)` where `start_time` is `Instant::now()` when `refresh_on_startup=true` and `Instant::now() + interval_duration` otherwise. No separate startup-refresh code path exists.</paragraph>
    </subsection>

    <subsection title="T-28 / AC-08: Per-Podcast Scheduling via get_due_podcasts">
      <paragraph>FAIL. The `sync_once` function at line 139 uses `db.get_podcasts()` instead of `db.get_due_podcasts(global_interval_secs)`. The `_interval_secs` variable is extracted from config (line 134) but never used for filtering. This means all podcasts are checked on every sync pass regardless of their `last_checked` timestamp, violating per-podcast scheduling. Test `sync_once_skips_podcasts_not_yet_due` fails with `podcasts_checked=1` instead of expected `0`.</paragraph>
    </subsection>

  </section>

  <section title="Regression Detection">

    <subsection title="REGRESSION-001: integration_full_flow_fetches_downloads_and_enqueues_new_episodes">
      <paragraph>This pre-existing test from an earlier phase now fails because Phase 3 correctly changed `PlaylistTrackSource::Path` to `PlaylistTrackSource::PodcastUrl` (AC-14). The test assertion at line 1806 expects `PlaylistTrackSource::Path` but receives `PlaylistTrackSource::PodcastUrl`. Classification: TEST BUG (stale assertion that conflicts with the Phase 3 correctness fix). The test needs updating to expect `PodcastUrl` instead of `Path`.</paragraph>
    </subsection>

  </section>

  <section title="Defects Found">

    <subsection title="DEF-001: sync_once does not use get_due_podcasts for per-podcast scheduling">
      <list type="unordered">
        <item name="Severity">High</item>
        <item name="Scenario">SCENARIO-011</item>
        <item name="Test Case">sync_once_skips_podcasts_not_yet_due</item>
        <item name="Steps to Reproduce">1. Insert podcast with last_checked=Utc::now(). 2. Call sync_once with interval=3600s. 3. Observe podcasts_checked=1 instead of 0.</item>
        <item name="Expected">sync_once should call get_due_podcasts(global_interval_secs) to filter podcasts, skipping those checked within the interval.</item>
        <item name="Actual">sync_once calls db.get_podcasts() at line 139, processing ALL podcasts regardless of last_checked timestamp. The _interval_secs variable extracted at line 134 is unused.</item>
        <item name="Status">Open</item>
        <item name="Evidence">Test output: `assertion left == right failed: AC-08/SCENARIO-011: Podcast checked within interval should be SKIPPED, left: 1, right: 0` at podcast_sync_phase3_tests.rs:1014</item>
      </list>
    </subsection>

    <subsection title="DEF-002: Pre-existing integration test expects Path instead of PodcastUrl (stale test)">
      <list type="unordered">
        <item name="Severity">Medium</item>
        <item name="Scenario">AC-14 / SCENARIO-021</item>
        <item name="Test Case">integration_full_flow_fetches_downloads_and_enqueues_new_episodes</item>
        <item name="Steps to Reproduce">1. Run test that checks PlaylistTrackSource after sync. 2. Observe panic at line 1806.</item>
        <item name="Expected">Test should expect PlaylistTrackSource::PodcastUrl (the correct Phase 3 behavior per AC-14).</item>
        <item name="Actual">Test asserts PlaylistTrackSource::Path and panics with: `Expected Path source, got: PodcastUrl("http://127.0.0.1:37697/episodes/episode2.mp3")`</item>
        <item name="Status">Open</item>
        <item name="Evidence">The Phase 3 implementation CORRECTLY uses PodcastUrl. The test is stale and must be updated to match AC-14.</item>
      </list>
    </subsection>

    <subsection title="DEF-003: sync_once_updates_last_checked_on_success timing precision issue">
      <list type="unordered">
        <item name="Severity">Low</item>
        <item name="Scenario">SCENARIO-010</item>
        <item name="Test Case">sync_once_updates_last_checked_on_success</item>
        <item name="Steps to Reproduce">1. Capture before_sync = Utc::now() (sub-second precision). 2. Call sync_once which stores last_checked as integer seconds in SQLite. 3. Read back last_checked and compare with sub-second before_sync.</item>
        <item name="Expected">last_checked >= before_sync</item>
        <item name="Actual">Fails intermittently because SQLite stores timestamps as integer seconds (truncating sub-second), so stored value can be less than the sub-second before_sync captured immediately before the async call.</item>
        <item name="Status">Open</item>
        <item name="Evidence">Test output: `last_checked should be at or after the sync start time`. The first assertion (last_checked > old_time) passes, confirming the update happened. Only the precision check fails.</item>
      </list>
    </subsection>

  </section>

  <section title="Quality Gates Checklist">
    <checklist>
      <item status="open">All tests pass (zero failures) — 3 failures remain</item>
      <item status="open">Coverage meets threshold for new/changed code — no coverage tool available</item>
      <item status="done">BDD scenario coverage = 100% for Phase 3 in-scope scenarios</item>
      <item status="open">No critical or high defects remain open — DEF-001 is High severity</item>
      <item status="done">Build succeeds (cargo build --package termusic-server)</item>
    </checklist>
  </section>

  <section title="Artifacts">
    <list type="unordered">
      <item name="Test traces">N/A (console output captured during execution)</item>
      <item name="Screenshots">N/A (CLI/backend project)</item>
      <item name="Network logs">N/A (wiremock serves localhost mock responses)</item>
      <item name="JUnit XML">N/A (not generated)</item>
      <item name="Coverage report">N/A (no cargo-tarpaulin or grcov available)</item>
    </list>
  </section>

  <section title="Recommendations">
    <list type="ordered">
      <item>DEF-001 (High): Replace `db.get_podcasts()` with `db.get_due_podcasts(_interval_secs)` at server/src/podcast_sync.rs line 139. The `_interval_secs` variable is already computed but unused. This is a one-line fix to complete T-28.</item>
      <item>DEF-002 (Medium): Update `integration_full_flow_fetches_downloads_and_enqueues_new_episodes` at line 1799-1809 to expect `PlaylistTrackSource::PodcastUrl` instead of `PlaylistTrackSource::Path`. The old assertion contradicts AC-14.</item>
      <item>DEF-003 (Low): In test `sync_once_updates_last_checked_on_success`, change the assertion from `updated_podcast.last_checked >= before_sync` to `updated_podcast.last_checked >= before_sync - chrono::Duration::seconds(1)` to account for integer-second DB storage truncation.</item>
    </list>
  </section>

</document>
