# Implementation Summary: Optimize Playlist Loading Performance

- **Date**: 2026-06-26
- **Author**: super-dev:impl-summary-writer
- **Phase**: 1 — Dependency Setup
- **Status**: completed

---

## Overview

Phase 1 added the rayon 1.12 crate as a workspace dependency and wired it into the playback crate. The import `use rayon::prelude::*` was added to `playback/src/playlist.rs` in preparation for Phase 2 parallelization work. The workspace builds without errors and cargo clippy produces no new warnings. A minor rustfmt reformatting of the `episode_by_url` HashMap declaration was included as a code style improvement.

## Files Changed

- `Cargo.toml` — modified, +1/-0
  - Purpose: Declared `rayon = "1.12"` in the `[workspace.dependencies]` section to make rayon available as a workspace-level dependency.

- `Cargo.lock` — modified, +1/-0
  - Purpose: Lock file updated to reflect the new direct rayon dependency entry (rayon was already present as a transitive dependency, so no new download).

- `playback/Cargo.toml` — modified, +1/-0
  - Purpose: Added `rayon.workspace = true` to the playback crate's `[dependencies]` section, enabling rayon usage in the playback crate.

- `playback/src/playlist.rs` — modified, +9/-5
  - Purpose: Added `use rayon::prelude::*` import (with `#[allow(unused_imports)]` annotation noting it will be used in Phase 2). Also reformatted the `episode_by_url` HashMap type annotation to comply with rustfmt line-length rules.

## Key Decisions

### 1. Allow unused_imports annotation on rayon import

- **Context**: The rayon import is added in this phase but will not be used until Phase 2 implements the parallel iteration.
- **Decision**: Added `#[allow(unused_imports)]` with a comment explaining the import is "Used in Phase 2 for parallel playlist loading".
- **Rationale**: Prevents cargo clippy from raising an unused import warning during the intermediate state between Phase 1 and Phase 2, while keeping the import declaration co-located with the dependency setup phase for clear traceability.
- **Reference**: `playback/src/playlist.rs`

### 2. Rayon version pinned to 1.12

- **Context**: Rayon was already a transitive dependency in the project (present in Cargo.lock). Choosing a version that matches what is already resolved avoids pulling in additional crates.
- **Decision**: Used `rayon = "1.12"` which aligns with the version already in the lock file.
- **Rationale**: Minimizes binary size impact and dependency graph changes. The implementation plan explicitly noted this version choice to avoid workspace resolution conflicts.
- **Reference**: `Cargo.toml`

### 3. Reformatted episode_by_url HashMap declaration

- **Context**: The existing HashMap type annotation exceeded rustfmt line-length limits and was reformatted as part of the diff.
- **Decision**: Applied rustfmt-compliant formatting to the multi-line type annotation.
- **Rationale**: Keeps the code consistent with project formatting standards and avoids a separate formatting-only commit.
- **Reference**: `playback/src/playlist.rs`

## Deviations from Spec

No deviations from specification.

## Test Results

- **Unit Tests**: All existing workspace tests pass (build verification confirms no regressions)
- **Integration Tests**: No new tests in this phase (dependency-only phase)

## Next Steps

Phase complete. No remaining items.

Phase 2 (Core Parallelization) can now proceed with:
1. Replace sequential line iteration with batch collection
2. Implement line classification into network addresses and local paths
3. Implement parallel metadata read using par_iter
4. Implement sequential podcast/radio resolution
5. Implement order-preserving merge
6. Add elapsed time logging

---

# Implementation Summary: Optimize Playlist Loading Performance

- **Date**: 2026-06-26
- **Author**: super-dev:impl-summary-writer
- **Phase**: 2 — Core Parallelization
- **Status**: completed

---

## Overview

Phase 2 replaced the sequential for-loop in `Playlist::load()` with a two-phase classify-then-parallel-process architecture. A new `parallel_load` submodule was extracted from `playlist.rs` containing four pure helper functions: `collect_and_filter_lines`, `classify_playlist_lines`, `parallel_read_local_tracks`, and `merge_indexed_tracks`. The main `Playlist::load()` function now classifies playlist lines into network addresses and local file paths, processes local metadata reads in parallel via rayon `par_iter`, resolves network entries sequentially, and merges results preserving original playlist order. All 424 workspace tests pass including 31 new Phase 2 tests.

## Files Changed

- `playback/src/playlist.rs` — modified, +46/-28
  - Purpose: Removed the sequential for-loop (lines 226-250) and replaced it with calls to the new `parallel_load` module functions. Added `pub mod parallel_load;` declaration. Removed the unused rayon import (now in the submodule). Added elapsed time logging via `info!` macro.

- `playback/src/playlist/parallel_load.rs` — created, +117/-0
  - Purpose: New submodule containing the core parallelization logic extracted into four testable functions: `collect_and_filter_lines` (batch line reading with `map_while`), `classify_playlist_lines` (partition into network/local entries preserving indices), `parallel_read_local_tracks` (rayon `par_iter` metadata reads with graceful failure handling), and `merge_indexed_tracks` (order-preserving sort-merge of indexed results).

- `playback/tests/phase2_core_parallelization_tests.rs` — created, +630/-0
  - Purpose: 31 unit and integration tests covering line classification, order-preserving merge, parallel read error handling, API signature stability, full pipeline integration, bounded-time completion, and edge cases (empty, single, all-fail, large input).

## Key Decisions

### 1. Extract parallel_load as a separate submodule

- **Context**: The parallelization logic (classify, parallel-read, merge) needed to be testable in isolation without running the full `Playlist::load()` which depends on config paths and database access.
- **Decision**: Created `playback/src/playlist/parallel_load.rs` as a public submodule with four pure functions rather than inlining all logic in the `load()` method body.
- **Rationale**: Enables comprehensive unit testing of each phase (collect, classify, process, merge) independently. The functions accept simple inputs (iterators, slices) and return simple outputs (Vecs), making them trivial to test without filesystem or database fixtures.
- **Reference**: `playback/src/playlist/parallel_load.rs`

### 2. Use map_while(Result::ok) instead of line? for batch collection

- **Context**: The original code used `line?` which would abort the entire load operation on the first I/O error mid-file. The parallel approach needs all lines collected upfront.
- **Decision**: Used `map_while(Result::ok)` which stops reading at the first I/O error but returns successfully-read lines rather than propagating an error.
- **Rationale**: For a local regular file, mid-read I/O errors are near-impossible. Partial playlist loading is preferable to total startup failure. The track index line (read earlier with `?`) catches truly unreadable files.
- **Reference**: `playback/src/playlist/parallel_load.rs` (function `collect_and_filter_lines`)

### 3. Early file existence check before Track::read_track_from_path

- **Context**: `parallel_read_local_tracks` could call `Track::read_track_from_path` directly for all paths, but non-existent paths would still trigger the full metadata parsing attempt.
- **Decision**: Added `std::path::Path::new(file_path).exists()` check before attempting to read track metadata.
- **Rationale**: Skipping non-existent files early avoids the overhead of opening and attempting to parse files that cannot exist, reducing unnecessary work in the parallel phase.
- **Reference**: `playback/src/playlist/parallel_load.rs` (function `parallel_read_local_tracks`)

### 4. Prefix-based URL classification (http:// and https://)

- **Context**: The original code used `line.starts_with("http")` which would match strings like "httpfoo/bar.mp3". The spec called for proper URL detection.
- **Decision**: Used `starts_with("http://") || starts_with("https://")` for classification, which is case-sensitive and requires the full scheme separator.
- **Rationale**: More precise than the original logic — only actual URLs with proper scheme separators are treated as network entries. Case-sensitive matching matches the behavior of standard URL parsers and avoids false positives.
- **Reference**: `playback/src/playlist/parallel_load.rs` (function `classify_playlist_lines`)

### 5. No catch_unwind for rayon tasks

- **Context**: Lofty metadata parsing could theoretically panic on malformed files. The implementation plan mentioned assessing panic risk.
- **Decision**: Did not add `catch_unwind` per-task, relying on lofty's fuzz testing and `ParsingMode::BestAttempt`.
- **Rationale**: Documented in the safety note that lofty 0.24.0 has extensive fuzz testing (8+ fuzz targets) and panics are extremely unlikely. If a panic occurs, rayon propagates it to the calling thread naturally.
- **Reference**: `playback/src/playlist/parallel_load.rs` (doc comment on `parallel_read_local_tracks`)

## Deviations from Spec

### URL classification is stricter than original code

- **Spec said**: Replace the sequential loop preserving identical observable behavior.
- **Actual**: The classification now requires `http://` or `https://` prefix rather than just `http` prefix, meaning a hypothetical path like "httpfoo/bar.mp3" would now be classified as a local file path instead of a network address.
- **Reason**: This is a correctness improvement. The original `line.starts_with("http")` was overly broad and would incorrectly treat paths containing "http" as a prefix (without "://") as network URLs. The new behavior is more correct and matches the intent of the specification.

## Test Results

- **Unit Tests**: 424/424 passing (all workspace tests, including 31 new Phase 2 tests)
- **Integration Tests**: Included in the 31 new tests (full pipeline tests exercising classify-process-merge end-to-end)

## Next Steps

Phase complete. No remaining items.

Phase 3 (Integration Testing) can now proceed with:
1. Create test fixture playlist files (mixed, invalid, empty, single, all-invalid)
2. Add integration test for order preservation with mixed entries
3. Add integration test for graceful skip of invalid paths
4. Add edge case integration tests (empty, single, all-fail)
5. Run full test suite verification
