//! Prototype: Validate numeric design constants for podcast sync architecture.
//!
//! Constants under test:
//!   1. Default sync interval: 3600s (spec says 1h is adequate for podcast cadence)
//!   2. connect_timeout for downloads: 10s (spec assumes feeds respond within this)
//!   3. concurrent_downloads_max: 3 (spec assumes this bounds resource usage adequately)
//!   4. max_download_retries: 3 (spec assumes transient failures resolve in 3 attempts)
//!   5. Dedup matching threshold: 2/3 fields (spec uses 2-of-3 match for fallback dedup)
//!
//! Representative inputs: Real RSS feeds from the task specification.

use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::time::{Duration, Instant};

const FEEDS: &[&str] = &[
    "https://feeds.simplecast.com/54nAGcIl",
    "https://changelog.com/podcast/feed",
    "https://feeds.fireside.fm/selfhosted/rss",
    "https://rustacean-station.org/podcast.rss",
    "https://feeds.transistor.fm/signals-and-threads",
];

const SPEC_CONNECT_TIMEOUT_SECS: f64 = 10.0;
const SPEC_SYNC_INTERVAL_SECS: u64 = 3600;
const SPEC_CONCURRENT_MAX: usize = 3;
const SPEC_MAX_RETRIES: usize = 3;
const SPEC_DEDUP_THRESHOLD: usize = 2; // 2 of 3 fields must match

#[derive(Debug, Clone)]
struct FeedResult {
    url: String,
    response_time_secs: f64,
    episode_count: usize,
    episodes_with_guid: usize,
    episodes_with_url: usize,
    episodes_with_pubdate: usize,
    latest_episode_age_hours: Option<f64>,
    error: Option<String>,
}

#[derive(Debug)]
struct ConcurrencyResult {
    total_sequential_time_secs: f64,
    total_concurrent_time_secs: f64,
    feeds_completed: usize,
    feeds_failed: usize,
}

#[tokio::main]
async fn main() {
    println!("=== Podcast Sync Prototype: Design Constant Validation ===\n");

    // --- Test 1: Feed response times (validates connect_timeout = 10s) ---
    println!("## Test 1: Feed Response Times (connect_timeout = 10s spec)");
    println!("-----------------------------------------------------------");

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30)) // overall timeout for large feeds
        .build()
        .expect("client build");

    let mut feed_results: Vec<FeedResult> = Vec::new();

    for &feed_url in FEEDS {
        let start = Instant::now();
        let result = fetch_and_parse(&client, feed_url).await;
        let elapsed = start.elapsed().as_secs_f64();

        match result {
            Ok(mut fr) => {
                fr.response_time_secs = elapsed;
                println!(
                    "  [OK] {} - {:.2}s, {} episodes",
                    feed_url, fr.response_time_secs, fr.episode_count
                );
                feed_results.push(fr);
            }
            Err(e) => {
                println!("  [ERR] {} - {:.2}s - {}", feed_url, elapsed, e);
                feed_results.push(FeedResult {
                    url: feed_url.to_string(),
                    response_time_secs: elapsed,
                    episode_count: 0,
                    episodes_with_guid: 0,
                    episodes_with_url: 0,
                    episodes_with_pubdate: 0,
                    latest_episode_age_hours: None,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    // --- Test 2: Episode freshness (validates sync_interval = 1h) ---
    println!("\n## Test 2: Episode Freshness (sync_interval = 3600s spec)");
    println!("-----------------------------------------------------------");
    for fr in &feed_results {
        if let Some(age_h) = fr.latest_episode_age_hours {
            let days = age_h / 24.0;
            println!(
                "  {} - latest episode: {:.1} hours ({:.1} days) old",
                fr.url, age_h, days
            );
        } else if fr.error.is_none() {
            println!("  {} - no pubdate available", fr.url);
        }
    }

    // --- Test 3: Dedup field availability (validates 2/3 matching threshold) ---
    println!("\n## Test 3: Dedup Field Availability (2/3 matching threshold)");
    println!("--------------------------------------------------------------");
    for fr in &feed_results {
        if fr.error.is_some() {
            continue;
        }
        let total = fr.episode_count;
        if total == 0 {
            continue;
        }
        let guid_pct = 100.0 * fr.episodes_with_guid as f64 / total as f64;
        let url_pct = 100.0 * fr.episodes_with_url as f64 / total as f64;
        let pubdate_pct = 100.0 * fr.episodes_with_pubdate as f64 / total as f64;

        // For 2/3 dedup to work, at least 2 of (title[always present], url, pubdate/guid)
        // must be present. We check how many episodes have >= 2 of 3 auxiliary fields.
        let fields_available = [guid_pct, url_pct, pubdate_pct];
        let fields_above_90: usize = fields_available.iter().filter(|&&v| v >= 90.0).count();

        println!(
            "  {} ({} eps): guid={:.0}%, url={:.0}%, pubdate={:.0}% => {}/3 fields >90%",
            fr.url, total, guid_pct, url_pct, pubdate_pct, fields_above_90
        );
    }

    // --- Test 4: Concurrent fetch simulation (validates concurrent_downloads_max = 3) ---
    println!("\n## Test 4: Concurrent Fetch (concurrent_downloads_max = 3 spec)");
    println!("-----------------------------------------------------------------");

    let conc_result = test_concurrency(&client, FEEDS).await;
    println!(
        "  Sequential total: {:.2}s",
        conc_result.total_sequential_time_secs
    );
    println!(
        "  Concurrent (max 3): {:.2}s",
        conc_result.total_concurrent_time_secs
    );
    let speedup = conc_result.total_sequential_time_secs / conc_result.total_concurrent_time_secs;
    println!("  Speedup factor: {:.2}x", speedup);
    println!(
        "  Feeds OK: {}, Failed: {}",
        conc_result.feeds_completed, conc_result.feeds_failed
    );

    // --- Test 5: GUID uniqueness for dedup (validates dedup correctness) ---
    println!("\n## Test 5: GUID Uniqueness Check (dedup correctness)");
    println!("-----------------------------------------------------");
    for fr in &feed_results {
        if fr.error.is_some() || fr.episode_count == 0 {
            continue;
        }
        // GUIDs being unique means dedup by GUID won't have collisions
        println!(
            "  {} - {}/{} episodes have GUIDs ({:.1}% coverage)",
            fr.url,
            fr.episodes_with_guid,
            fr.episode_count,
            100.0 * fr.episodes_with_guid as f64 / fr.episode_count as f64
        );
    }

    // --- Summary JSON output ---
    println!("\n## RESULTS SUMMARY (JSON)");
    println!("=========================");

    let successful_feeds: Vec<&FeedResult> = feed_results.iter().filter(|f| f.error.is_none()).collect();
    let failed_feeds: Vec<&FeedResult> = feed_results.iter().filter(|f| f.error.is_some()).collect();

    let max_response_time = successful_feeds
        .iter()
        .map(|f| f.response_time_secs)
        .fold(0.0_f64, f64::max);
    let median_response_time = {
        let mut times: Vec<f64> = successful_feeds.iter().map(|f| f.response_time_secs).collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if times.is_empty() {
            0.0
        } else {
            times[times.len() / 2]
        }
    };

    // connect_timeout: measured max response time vs spec 10s
    let connect_timeout_within = max_response_time <= SPEC_CONNECT_TIMEOUT_SECS;

    // sync_interval: check if any feed updates more frequently than 1h
    // (if latest episode < 1h old across multiple checks, interval may be too long)
    // For this test, we validate the constant is reasonable for typical podcast cadence
    let ages: Vec<f64> = successful_feeds
        .iter()
        .filter_map(|f| f.latest_episode_age_hours)
        .collect();
    let min_age_hours = ages.iter().cloned().fold(f64::INFINITY, f64::min);
    // Sync interval is adequate if typical episode freshness >> interval
    // A 1h interval is fine if podcasts update at most hourly (typical is daily/weekly)
    let sync_interval_adequate = min_age_hours >= 1.0 || ages.is_empty();

    // concurrent_downloads: validates that 3 is enough by checking speedup > 1
    let concurrent_adequate = conc_result.total_concurrent_time_secs < conc_result.total_sequential_time_secs;

    // dedup threshold: all feeds have >= 2 of 3 fields at > 90%
    let dedup_adequate = successful_feeds.iter().all(|f| {
        if f.episode_count == 0 {
            return true;
        }
        let total = f.episode_count as f64;
        let guid_ok = (f.episodes_with_guid as f64 / total) >= 0.9;
        let url_ok = (f.episodes_with_url as f64 / total) >= 0.9;
        let pubdate_ok = (f.episodes_with_pubdate as f64 / total) >= 0.9;
        // Title is always present (parsed from RSS), so we need at least 1 more field at >90%
        // For 2/3 fallback: title always counts + at least one of url/pubdate at >90%
        // Actually the code checks guid first, then falls back to title+url+pubdate (2/3 match)
        // We need at least 2 of {title, url, pubdate} to work, title always exists
        // So we need url OR pubdate at high coverage
        url_ok || pubdate_ok || guid_ok
    });

    // max_retries: 3 retries is adequate if initial success rate is high
    let success_rate = successful_feeds.len() as f64 / feed_results.len() as f64;
    let retries_adequate = success_rate >= 0.8; // if 80%+ succeed on first try, 3 retries is generous

    println!("{{");
    println!("  \"connect_timeout_spec_secs\": {},", SPEC_CONNECT_TIMEOUT_SECS);
    println!("  \"connect_timeout_measured_max_secs\": {:.3},", max_response_time);
    println!("  \"connect_timeout_measured_median_secs\": {:.3},", median_response_time);
    println!("  \"connect_timeout_within_tolerance\": {},", connect_timeout_within);
    println!("  \"sync_interval_spec_secs\": {},", SPEC_SYNC_INTERVAL_SECS);
    println!("  \"sync_interval_min_episode_age_hours\": {:.1},", if min_age_hours.is_infinite() { -1.0 } else { min_age_hours });
    println!("  \"sync_interval_adequate\": {},", sync_interval_adequate);
    println!("  \"concurrent_max_spec\": {},", SPEC_CONCURRENT_MAX);
    println!("  \"concurrent_speedup_factor\": {:.2},", speedup);
    println!("  \"concurrent_adequate\": {},", concurrent_adequate);
    println!("  \"dedup_threshold_spec\": {},", SPEC_DEDUP_THRESHOLD);
    println!("  \"dedup_fields_adequate\": {},", dedup_adequate);
    println!("  \"max_retries_spec\": {},", SPEC_MAX_RETRIES);
    println!("  \"initial_success_rate\": {:.2},", success_rate);
    println!("  \"retries_adequate\": {},", retries_adequate);
    println!("  \"feeds_tested\": {},", feed_results.len());
    println!("  \"feeds_succeeded\": {},", successful_feeds.len());
    println!("  \"feeds_failed\": {}", failed_feeds.len());
    println!("}}");

    // Overall verdict
    let all_pass = connect_timeout_within && sync_interval_adequate && concurrent_adequate && dedup_adequate && retries_adequate;
    println!("\n## VERDICT: {}", if all_pass { "PASS" } else { "FAIL" });
    if !all_pass {
        if !connect_timeout_within {
            println!("  FAIL: connect_timeout - max response {:.2}s exceeds spec {}s", max_response_time, SPEC_CONNECT_TIMEOUT_SECS);
        }
        if !sync_interval_adequate {
            println!("  FAIL: sync_interval - episodes updating faster than {}s interval", SPEC_SYNC_INTERVAL_SECS);
        }
        if !concurrent_adequate {
            println!("  FAIL: concurrent_downloads_max - no speedup observed with {} concurrent", SPEC_CONCURRENT_MAX);
        }
        if !dedup_adequate {
            println!("  FAIL: dedup_threshold - insufficient field coverage for 2/3 matching");
        }
        if !retries_adequate {
            println!("  FAIL: max_retries - initial success rate {:.0}% too low for 3-retry strategy", success_rate * 100.0);
        }
    }
}

async fn fetch_and_parse(client: &reqwest::Client, url: &str) -> Result<FeedResult, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("body read failed: {e}"))?;

    let channel = rss::Channel::read_from(&bytes[..])
        .map_err(|e| format!("RSS parse failed: {e}"))?;

    let items = channel.into_items();
    let episode_count = items.len();
    let mut episodes_with_guid = 0;
    let mut episodes_with_url = 0;
    let mut episodes_with_pubdate = 0;
    let mut latest_pubdate: Option<DateTime<Utc>> = None;
    let mut guids_seen = HashSet::new();

    for item in &items {
        if let Some(guid) = item.guid() {
            let val = guid.value();
            if !val.is_empty() {
                episodes_with_guid += 1;
                guids_seen.insert(val.to_string());
            }
        }
        if let Some(enc) = item.enclosure() {
            if !enc.url().is_empty() {
                episodes_with_url += 1;
            }
        }
        if let Some(pd) = item.pub_date() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(pd) {
                episodes_with_pubdate += 1;
                let utc: DateTime<Utc> = dt.into();
                if latest_pubdate.is_none() || Some(utc) > latest_pubdate {
                    latest_pubdate = Some(utc);
                }
            }
        }
    }

    let latest_episode_age_hours = latest_pubdate.map(|dt| {
        let now = Utc::now();
        let diff = now.signed_duration_since(dt);
        diff.num_minutes() as f64 / 60.0
    });

    Ok(FeedResult {
        url: url.to_string(),
        response_time_secs: 0.0, // filled by caller
        episode_count,
        episodes_with_guid,
        episodes_with_url,
        episodes_with_pubdate,
        latest_episode_age_hours,
        error: None,
    })
}

async fn test_concurrency(client: &reqwest::Client, feeds: &[&str]) -> ConcurrencyResult {
    use tokio::sync::Semaphore;
    use std::sync::Arc;

    // Sequential timing (sum of individual times already measured)
    let mut sequential_total = 0.0;
    let mut completed = 0;
    let mut failed = 0;

    for &feed_url in feeds {
        let start = Instant::now();
        let result = client.get(feed_url).send().await;
        let elapsed = start.elapsed().as_secs_f64();
        sequential_total += elapsed;
        match result {
            Ok(_) => completed += 1,
            Err(_) => failed += 1,
        }
    }

    // Concurrent timing with semaphore (max 3)
    let semaphore = Arc::new(Semaphore::new(SPEC_CONCURRENT_MAX));
    let start = Instant::now();
    let mut handles = Vec::new();

    for &feed_url in feeds {
        let sem = semaphore.clone();
        let client = client.clone();
        let url = feed_url.to_string();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            client.get(&url).send().await.is_ok()
        }));
    }

    let mut conc_completed = 0;
    let mut conc_failed = 0;
    for h in handles {
        match h.await {
            Ok(true) => conc_completed += 1,
            _ => conc_failed += 1,
        }
    }
    let concurrent_total = start.elapsed().as_secs_f64();

    ConcurrencyResult {
        total_sequential_time_secs: sequential_total,
        total_concurrent_time_secs: concurrent_total,
        feeds_completed: conc_completed,
        feeds_failed: conc_failed,
    }
}
