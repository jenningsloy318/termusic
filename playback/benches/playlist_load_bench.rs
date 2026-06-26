//! Phase 4: Performance Validation Benchmark (RED Phase)
//!
//! This criterion benchmark measures the wall-clock time of parallel vs sequential
//! playlist loading with 200+ audio files to verify the minimum 3x speedup
//! requirement on a 4+ core machine.
//!
//! Coverage:
//!   T-17: Criterion benchmark measuring `Playlist::load()` performance
//!   SCENARIO-001: Large playlist loads metadata in parallel achieving proportional speedup
//!   SCENARIO-002: Parallel loading scales with available CPU cores
//!   SCENARIO-003: Small playlist loading incurs negligible parallelization overhead
//!   AC-01: Wall-clock speedup proportional to available CPU cores (min 3x on 4-core)
//!   AC-08: Peak RSS increase bounded to thread pool overhead (~8MB for 8 threads)
//!
//! This benchmark will FAIL to compile until `sequential_read_local_tracks` is
//! added to the `parallel_load` module as a baseline reference implementation.
//! The sequential function is needed specifically for benchmarking comparison —
//! it is NOT part of the production code path.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use termusicplayback::playlist::parallel_load::{
    load_playlist_from_path, parallel_read_local_tracks, sequential_read_local_tracks,
};

/// Create `count` minimal audio files in the given directory for benchmark use.
/// Returns the list of created file paths.
fn create_benchmark_audio_files(dir: &std::path::Path, count: usize) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(count);
    for i in 0..count {
        let file_path = dir.join(format!("bench_track_{i:04}.mp3"));
        // Write minimal content — large enough to simulate real metadata parsing overhead.
        // A real audio file would take ~20ms to parse; these fake files exercise the file
        // open + attempt-to-parse + fallback-to-default path.
        fs::write(&file_path, vec![0u8; 1024]).unwrap();
        paths.push(file_path);
    }
    paths
}

/// Benchmark: Compare parallel vs sequential loading for 200+ local tracks.
///
/// SCENARIO-001: Wall-clock load time is at least 3x faster than sequential.
/// SCENARIO-002: Speedup scales with available CPU cores.
/// AC-01: Minimum 3x improvement on a 4-core machine with 200+ tracks.
fn bench_parallel_vs_sequential_200_tracks(c: &mut Criterion) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for benchmark");
    let dir = temp_dir.path();

    let track_count = 200;
    let audio_files = create_benchmark_audio_files(dir, track_count);

    let local_entries: Vec<(usize, String)> = audio_files
        .iter()
        .enumerate()
        .map(|(i, path)| (i, path.to_string_lossy().to_string()))
        .collect();

    let mut group = c.benchmark_group("playlist_load_200_tracks");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("sequential", |b| {
        b.iter(|| {
            let result = sequential_read_local_tracks(&local_entries);
            assert_eq!(result.len(), track_count);
        });
    });

    group.bench_function("parallel", |b| {
        b.iter(|| {
            let result = parallel_read_local_tracks(&local_entries);
            assert_eq!(result.len(), track_count);
        });
    });

    group.finish();
}

/// Benchmark: Compare parallel vs sequential loading for 500 local tracks.
///
/// SCENARIO-002: Total metadata read time is approximately 500 / `core_count`.
/// AC-01: Speedup proportional to available CPU cores.
fn bench_parallel_vs_sequential_500_tracks(c: &mut Criterion) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for benchmark");
    let dir = temp_dir.path();

    let track_count = 500;
    let audio_files = create_benchmark_audio_files(dir, track_count);

    let local_entries: Vec<(usize, String)> = audio_files
        .iter()
        .enumerate()
        .map(|(i, path)| (i, path.to_string_lossy().to_string()))
        .collect();

    let mut group = c.benchmark_group("playlist_load_500_tracks");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    group.bench_function("sequential", |b| {
        b.iter(|| {
            let result = sequential_read_local_tracks(&local_entries);
            assert_eq!(result.len(), track_count);
        });
    });

    group.bench_function("parallel", |b| {
        b.iter(|| {
            let result = parallel_read_local_tracks(&local_entries);
            assert_eq!(result.len(), track_count);
        });
    });

    group.finish();
}

/// Benchmark: Verify small playlists do not regress from parallelization overhead.
///
/// SCENARIO-003: Small playlist (< 50 tracks) load time is not measurably worse
/// than sequential processing.
fn bench_small_playlist_overhead(c: &mut Criterion) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for benchmark");
    let dir = temp_dir.path();

    let small_sizes = [1, 5, 10, 25, 49];

    let mut group = c.benchmark_group("playlist_load_small");
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(5));

    for &size in &small_sizes {
        let audio_files = create_benchmark_audio_files(dir, size);
        let local_entries: Vec<(usize, String)> = audio_files
            .iter()
            .enumerate()
            .map(|(i, path)| (i, path.to_string_lossy().to_string()))
            .collect();

        group.bench_with_input(
            BenchmarkId::new("sequential", size),
            &local_entries,
            |b, entries| {
                b.iter(|| {
                    let result = sequential_read_local_tracks(entries);
                    assert_eq!(result.len(), size);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parallel", size),
            &local_entries,
            |b, entries| {
                b.iter(|| {
                    let result = parallel_read_local_tracks(entries);
                    assert_eq!(result.len(), size);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Full pipeline via `load_playlist_from_path` with mixed content.
///
/// This measures the end-to-end loading time including file reading,
/// classification, parallel metadata reads, and order-preserving merge.
///
/// SCENARIO-001: Overall performance validation.
/// SCENARIO-021: Mixed entries performance.
fn bench_full_pipeline_mixed_entries(c: &mut Criterion) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for benchmark");
    let dir = temp_dir.path();

    // Create 200 real audio files
    let audio_files = create_benchmark_audio_files(dir, 200);

    // Build a playlist file with interleaved local and network entries (300 total)
    // Pattern: 2 local files then 1 network URL, repeated
    let playlist_path = dir.join("bench_playlist.log");
    let mut content = String::from("0\n"); // track index line
    let mut local_idx = 0;
    for i in 0..300 {
        if i % 3 == 2 {
            // Network URL
            writeln!(content, "http://example.com/podcast/ep{i}.mp3").unwrap();
        } else if local_idx < audio_files.len() {
            // Local file
            writeln!(content, "{}", audio_files[local_idx].display()).unwrap();
            local_idx += 1;
        }
    }
    fs::write(&playlist_path, &content).unwrap();

    let mut group = c.benchmark_group("playlist_load_full_pipeline");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("load_playlist_from_path_mixed_300", |b| {
        b.iter(|| {
            let (_, tracks) =
                load_playlist_from_path(&playlist_path).expect("Benchmark load should succeed");
            // Should have 200 local tracks + 100 network tracks = 300 total
            assert!(tracks.len() >= 200, "Should load at least 200 tracks");
        });
    });

    group.finish();
}

/// Benchmark: Scaling test across different track counts to show proportional speedup.
///
/// SCENARIO-002: Speedup is proportional to core count.
/// AC-01: Verification across multiple data sizes.
fn bench_scaling_with_track_count(c: &mut Criterion) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for benchmark");
    let dir = temp_dir.path();

    let track_counts = [50, 100, 200, 500];

    let mut group = c.benchmark_group("playlist_load_scaling");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for &count in &track_counts {
        let sub_dir = dir.join(format!("tracks_{count}"));
        fs::create_dir_all(&sub_dir).unwrap();
        let audio_files = create_benchmark_audio_files(&sub_dir, count);

        let local_entries: Vec<(usize, String)> = audio_files
            .iter()
            .enumerate()
            .map(|(i, path)| (i, path.to_string_lossy().to_string()))
            .collect();

        group.bench_with_input(
            BenchmarkId::new("parallel", count),
            &local_entries,
            |b, entries| {
                b.iter(|| {
                    let result = parallel_read_local_tracks(entries);
                    assert_eq!(result.len(), count);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sequential", count),
            &local_entries,
            |b, entries| {
                b.iter(|| {
                    let result = sequential_read_local_tracks(entries);
                    assert_eq!(result.len(), count);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parallel_vs_sequential_200_tracks,
    bench_parallel_vs_sequential_500_tracks,
    bench_small_playlist_overhead,
    bench_full_pipeline_mixed_entries,
    bench_scaling_with_track_count,
);
criterion_main!(benches);
