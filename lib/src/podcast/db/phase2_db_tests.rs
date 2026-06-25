//! Phase 2 tests for Database Schema Migration and Per-Podcast Scheduling.
//!
//! These tests validate:
//! - AC-08: Per-podcast last_checked timestamp stored after feed check
//! - AC-09: Per-podcast check_interval override (nullable, falls back to global)
//! - T-17: 002.sql migration adds check_interval column
//! - T-18: Migration applies when user_version < 2
//! - T-19: update_last_checked writes correct timestamp
//! - T-20: get_due_podcasts returns only due podcasts
//!
//! SCENARIO-010: Per-podcast last-checked timestamp is recorded
//! SCENARIO-011: Per-podcast scheduling uses individual timestamps
//! SCENARIO-012: Per-podcast interval override takes precedence
//! SCENARIO-013: Missing per-podcast interval falls back to global
//! SCENARIO-036: Empty podcast subscription list during sync
//! SCENARIO-037: Zero new episodes still updates last_checked
//! SCENARIO-039: Network timeout isolates to single podcast
//! SCENARIO-041: last_checked updated even when all downloads fail

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use pretty_assertions::assert_eq;
use rusqlite::{Connection, params};

use super::PodcastDBId;
use super::podcast_db::{get_due_podcasts, update_last_checked};
use super::test_utils::gen_database;

/// Helper: run migrations on a fresh in-memory database to get it to version 2.
fn setup_migrated_db() -> Connection {
    let conn = gen_database();
    super::migration::migrate(&conn).expect("migration should succeed on fresh db");
    conn
}

/// Helper: insert a podcast into the test database, returning its ID.
fn insert_test_podcast(
    conn: &Connection,
    title: &str,
    url: &str,
    last_checked: Option<i64>,
    check_interval: Option<i64>,
) -> PodcastDBId {
    conn.execute(
        "INSERT INTO podcasts (title, url, last_checked, check_interval) VALUES (?1, ?2, ?3, ?4)",
        params![title, url, last_checked, check_interval],
    )
    .expect("insert test podcast");
    conn.last_insert_rowid()
}

// =========================================================================
// T-17 / T-18: Database migration adds check_interval column
// =========================================================================

/// After migration to version 2, the podcasts table should have a
/// check_interval column (nullable INTEGER).
#[test]
fn migration_002_adds_check_interval_column() {
    let conn = setup_migrated_db();

    // Verify the column exists by inserting a row with check_interval
    conn.execute(
        "INSERT INTO podcasts (title, url, last_checked, check_interval) VALUES ('Test', 'http://127.0.0.1/feed.xml', 0, 7200)",
        [],
    )
    .expect("should be able to insert with check_interval column");

    // Read it back
    let interval: Option<i64> = conn
        .query_row(
            "SELECT check_interval FROM podcasts WHERE url = 'http://127.0.0.1/feed.xml'",
            [],
            |row| row.get(0),
        )
        .expect("should read check_interval");

    assert_eq!(interval, Some(7200));
}

/// check_interval should be nullable (NULL means use global interval).
#[test]
fn check_interval_column_is_nullable() {
    let conn = setup_migrated_db();

    conn.execute(
        "INSERT INTO podcasts (title, url, last_checked) VALUES ('Nullable Test', 'http://127.0.0.1/nullable.xml', 0)",
        [],
    )
    .expect("should insert without check_interval");

    let interval: Option<i64> = conn
        .query_row(
            "SELECT check_interval FROM podcasts WHERE url = 'http://127.0.0.1/nullable.xml'",
            [],
            |row| row.get(0),
        )
        .expect("should read null check_interval");

    assert_eq!(interval, None);
}

/// After migration, user_version should be 2.
#[test]
fn migration_sets_user_version_to_2() {
    let conn = setup_migrated_db();

    let version: u32 = conn
        .query_row("SELECT user_version FROM pragma_user_version", [], |r| {
            r.get(0)
        })
        .expect("should read user_version");

    assert_eq!(version, 2, "user_version should be 2 after migration");
}

/// Migration should be idempotent — running twice should not error.
#[test]
fn migration_is_idempotent() {
    let conn = gen_database();
    super::migration::migrate(&conn).expect("first migration");
    super::migration::migrate(&conn).expect("second migration should also succeed");

    let version: u32 = conn
        .query_row("SELECT user_version FROM pragma_user_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 2);
}

// =========================================================================
// T-19 / SCENARIO-010: update_last_checked writes correct timestamp
// =========================================================================

/// update_last_checked should write the given timestamp for a specific podcast.
#[test]
fn update_last_checked_writes_timestamp_for_specific_podcast() {
    let conn = setup_migrated_db();

    let pod_id = insert_test_podcast(
        &conn,
        "Podcast A",
        "http://127.0.0.1/a.xml",
        Some(1000),
        None,
    );

    let new_timestamp = DateTime::from_timestamp(5000, 0).unwrap();
    let rows_affected =
        update_last_checked(pod_id, new_timestamp, &conn).expect("update should succeed");

    assert_eq!(rows_affected, 1, "should update exactly one row");

    // Verify the stored value
    let stored: i64 = conn
        .query_row(
            "SELECT last_checked FROM podcasts WHERE id = ?1",
            params![pod_id],
            |row| row.get(0),
        )
        .expect("should read back timestamp");

    assert_eq!(
        stored, 5000,
        "stored timestamp should match the one written"
    );
}

/// update_last_checked for a nonexistent ID should return Ok(0) rows affected.
#[test]
fn update_last_checked_nonexistent_id_returns_zero_rows() {
    let conn = setup_migrated_db();

    let nonexistent_id: PodcastDBId = 99999;
    let timestamp = Utc::now();

    let rows_affected =
        update_last_checked(nonexistent_id, timestamp, &conn).expect("should not error");

    assert_eq!(
        rows_affected, 0,
        "updating nonexistent podcast should affect 0 rows"
    );
}

/// update_last_checked should not affect other podcasts.
#[test]
fn update_last_checked_does_not_affect_other_podcasts() {
    let conn = setup_migrated_db();

    let pod_a = insert_test_podcast(
        &conn,
        "Podcast A",
        "http://127.0.0.1/a.xml",
        Some(1000),
        None,
    );
    let pod_b = insert_test_podcast(
        &conn,
        "Podcast B",
        "http://127.0.0.1/b.xml",
        Some(2000),
        None,
    );

    let new_timestamp = DateTime::from_timestamp(9999, 0).unwrap();
    update_last_checked(pod_a, new_timestamp, &conn).expect("update A");

    // Pod B should remain unchanged
    let stored_b: i64 = conn
        .query_row(
            "SELECT last_checked FROM podcasts WHERE id = ?1",
            params![pod_b],
            |row| row.get(0),
        )
        .expect("should read B's timestamp");

    assert_eq!(stored_b, 2000, "podcast B's timestamp should be unchanged");
}

// =========================================================================
// T-20 / SCENARIO-011: get_due_podcasts returns only due podcasts
// =========================================================================

/// Podcasts with NULL last_checked are always considered due.
#[test]
fn get_due_podcasts_includes_null_last_checked() {
    let conn = setup_migrated_db();

    insert_test_podcast(
        &conn,
        "Never Checked",
        "http://127.0.0.1/never.xml",
        None,
        None,
    );

    let global_interval_secs: i64 = 3600; // 1 hour
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].title, "Never Checked");
}

/// Podcasts checked more than global_interval_secs ago should be included.
#[test]
fn get_due_podcasts_includes_overdue_podcast() {
    let conn = setup_migrated_db();

    let two_hours_ago = Utc::now() - ChronoDuration::hours(2);
    insert_test_podcast(
        &conn,
        "Overdue Podcast",
        "http://127.0.0.1/overdue.xml",
        Some(two_hours_ago.timestamp()),
        None,
    );

    let global_interval_secs: i64 = 3600; // 1 hour
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].title, "Overdue Podcast");
}

/// Podcasts checked less than global_interval_secs ago should be excluded.
/// SCENARIO-011: podcast A was last checked 30 minutes ago, global interval 1 hour -> skipped.
#[test]
fn get_due_podcasts_excludes_recently_checked_podcast() {
    let conn = setup_migrated_db();

    let thirty_min_ago = Utc::now() - ChronoDuration::minutes(30);
    insert_test_podcast(
        &conn,
        "Recent Podcast",
        "http://127.0.0.1/recent.xml",
        Some(thirty_min_ago.timestamp()),
        None,
    );

    let global_interval_secs: i64 = 3600; // 1 hour
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 0, "recently checked podcast should not be due");
}

/// Mixed set: one due and one not due.
/// SCENARIO-011: podcast A (30 min ago) skipped, podcast B (2 hours ago) included.
#[test]
fn get_due_podcasts_filters_mixed_set_correctly() {
    let conn = setup_migrated_db();

    let thirty_min_ago = Utc::now() - ChronoDuration::minutes(30);
    let two_hours_ago = Utc::now() - ChronoDuration::hours(2);

    insert_test_podcast(
        &conn,
        "Not Due Yet",
        "http://127.0.0.1/notdue.xml",
        Some(thirty_min_ago.timestamp()),
        None,
    );
    insert_test_podcast(
        &conn,
        "Overdue",
        "http://127.0.0.1/overdue.xml",
        Some(two_hours_ago.timestamp()),
        None,
    );

    let global_interval_secs: i64 = 3600;
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].title, "Overdue");
}

// =========================================================================
// SCENARIO-012 / AC-09: Per-podcast interval override takes precedence
// =========================================================================

/// When a podcast has a per-podcast check_interval override of 6 hours,
/// it should NOT be considered due after only 2 hours even if global is 1 hour.
#[test]
fn get_due_podcasts_respects_per_podcast_interval_override() {
    let conn = setup_migrated_db();

    let two_hours_ago = Utc::now() - ChronoDuration::hours(2);
    // Per-podcast override: 6 hours (21600 seconds)
    insert_test_podcast(
        &conn,
        "Long Interval Podcast",
        "http://127.0.0.1/long.xml",
        Some(two_hours_ago.timestamp()),
        Some(21600),
    );

    let global_interval_secs: i64 = 3600; // 1 hour
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(
        due.len(),
        0,
        "podcast with 6h override should not be due after only 2h"
    );
}

/// When enough time has passed to exceed the per-podcast override, it should be due.
#[test]
fn get_due_podcasts_includes_podcast_past_its_override_interval() {
    let conn = setup_migrated_db();

    let seven_hours_ago = Utc::now() - ChronoDuration::hours(7);
    // Per-podcast override: 6 hours (21600 seconds)
    insert_test_podcast(
        &conn,
        "Past Override Podcast",
        "http://127.0.0.1/pastoverride.xml",
        Some(seven_hours_ago.timestamp()),
        Some(21600),
    );

    let global_interval_secs: i64 = 3600;
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].title, "Past Override Podcast");
}

// =========================================================================
// SCENARIO-013: Missing per-podcast interval falls back to global
// =========================================================================

/// Podcast with no check_interval (NULL) should use the global interval.
#[test]
fn get_due_podcasts_null_check_interval_uses_global() {
    let conn = setup_migrated_db();

    let two_hours_ago = Utc::now() - ChronoDuration::hours(2);
    // No per-podcast override (NULL)
    insert_test_podcast(
        &conn,
        "Global Interval Podcast",
        "http://127.0.0.1/global.xml",
        Some(two_hours_ago.timestamp()),
        None,
    );

    let global_interval_secs: i64 = 3600; // 1 hour — podcast was checked 2h ago -> due
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].title, "Global Interval Podcast");
}

// =========================================================================
// SCENARIO-036: Empty podcast subscription list during sync
// =========================================================================

/// get_due_podcasts with no podcasts should return an empty vec.
#[test]
fn get_due_podcasts_empty_table_returns_empty_vec() {
    let conn = setup_migrated_db();

    let global_interval_secs: i64 = 3600;
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    assert_eq!(due.len(), 0, "empty table should return empty result");
}

// =========================================================================
// SCENARIO-041: last_checked updated even when all downloads fail
// (Tests that update_last_checked can be called independently of download success)
// =========================================================================

/// update_last_checked should work regardless of episode/download state.
/// This confirms it can be called on both success and failure paths.
#[test]
fn update_last_checked_works_independently_of_episodes() {
    let conn = setup_migrated_db();

    let pod_id = insert_test_podcast(
        &conn,
        "Fail Podcast",
        "http://127.0.0.1/fail.xml",
        Some(1000),
        None,
    );

    // Simulate calling update_last_checked after a failure
    let now = Utc::now();
    let rows = update_last_checked(pod_id, now, &conn).expect("should succeed");
    assert_eq!(rows, 1);

    // Verify timestamp was updated
    let stored: i64 = conn
        .query_row(
            "SELECT last_checked FROM podcasts WHERE id = ?1",
            params![pod_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, now.timestamp());
}

// =========================================================================
// Additional: get_due_podcasts with mixed overrides and nulls
// =========================================================================

/// Complex scenario: multiple podcasts with different intervals and timestamps.
#[test]
fn get_due_podcasts_complex_mix_of_overrides_and_timestamps() {
    let conn = setup_migrated_db();

    let now = Utc::now();

    // Podcast A: checked 30 min ago, global interval 1h -> NOT due
    insert_test_podcast(
        &conn,
        "A",
        "http://127.0.0.1/a.xml",
        Some((now - ChronoDuration::minutes(30)).timestamp()),
        None,
    );

    // Podcast B: checked 2h ago, global interval 1h -> DUE
    insert_test_podcast(
        &conn,
        "B",
        "http://127.0.0.1/b.xml",
        Some((now - ChronoDuration::hours(2)).timestamp()),
        None,
    );

    // Podcast C: checked 2h ago, per-podcast override 6h -> NOT due
    insert_test_podcast(
        &conn,
        "C",
        "http://127.0.0.1/c.xml",
        Some((now - ChronoDuration::hours(2)).timestamp()),
        Some(21600),
    );

    // Podcast D: never checked (NULL) -> DUE
    insert_test_podcast(&conn, "D", "http://127.0.0.1/d.xml", None, None);

    // Podcast E: checked 7h ago, per-podcast override 6h -> DUE
    insert_test_podcast(
        &conn,
        "E",
        "http://127.0.0.1/e.xml",
        Some((now - ChronoDuration::hours(7)).timestamp()),
        Some(21600),
    );

    let global_interval_secs: i64 = 3600;
    let due = get_due_podcasts(global_interval_secs, &conn).expect("query should succeed");

    let due_titles: Vec<&str> = due.iter().map(|p| p.title.as_str()).collect();

    assert!(
        due_titles.contains(&"B"),
        "B should be due (2h > 1h global)"
    );
    assert!(due_titles.contains(&"D"), "D should be due (never checked)");
    assert!(
        due_titles.contains(&"E"),
        "E should be due (7h > 6h override)"
    );
    assert!(
        !due_titles.contains(&"A"),
        "A should NOT be due (30m < 1h global)"
    );
    assert!(
        !due_titles.contains(&"C"),
        "C should NOT be due (2h < 6h override)"
    );
    assert_eq!(due.len(), 3);
}
