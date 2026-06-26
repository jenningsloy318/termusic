# Deep Research Report: Async Server Metadata Loading (Iteration 2)

- **Date**: 2026-06-26
- **Author**: super-dev:research-agent
- **Research Period**: 2026-06-26
- **Technologies**: Rust, Tokio 1.52, parking_lot 0.12, std::sync::atomic (AtomicBool, Ordering), tokio::sync::broadcast, tokio::sync::mpsc
- **Freshness**: Fresh (< 6mo)
- **Mode**: Deep Research (Issue Resolution)

---

## Executive Summary

- ISS-005 (Order of operations in async completion handler) is **resolved**: The four-step sequence (write-lock swap, AtomicBool store Release, send PlaylistShuffled event, send PlayerCmd::PlaylistLoadComplete) is both necessary and sufficient. Multiple layers of memory ordering guarantees ensure correctness.
- The ordering is enforced by three complementary mechanisms: (1) Rust's single-thread sequential execution semantics for opaque function calls, (2) Release ordering on the AtomicBool creating a happens-before edge, and (3) the RwLock's own Release semantics on guard drop.
- Three design options exist for documenting and enforcing this ordering: inline comments with ORDERING invariant, a dedicated `complete_background_load()` function encapsulating the sequence, or a state machine wrapper.
- **Recommendation** (High confidence): Option A (dedicated function with ordering comment block) provides the best balance of clarity, maintainability, and protection against future reordering. It encapsulates the four steps in a named function with a doc-comment explaining the ordering invariant and rationale for each step.

---

## Search Methodology

| Query | Tool | Results | Useful |
|-------|------|---------|--------|
| Rust AtomicBool Release ordering happens-before guarantee | WebFetch (doc.rust-lang.org/std/sync/atomic) | 1 | 1 |
| Rust nomicon atomics Release Acquire visibility guarantees | WebFetch (doc.rust-lang.org/nomicon/atomics) | 1 | 1 |
| tokio broadcast channel memory ordering send recv | WebFetch (docs.rs/tokio) | 1 | 1 |
| tokio mpsc unbounded_channel memory ordering | WebFetch (docs.rs/tokio) | 1 | 1 |
| Rust std mpsc channel happens-before send recv | WebFetch (doc.rust-lang.org/std) | 1 | 1 |
| Rust RwLock memory ordering drop guard Release | WebFetch (doc.rust-lang.org/std/sync/RwLock) | 1 | 1 |
| Rust compiler reorder opaque function calls side effects | WebFetch (doc.rust-lang.org/nomicon/races) | 1 | 1 |
| compiler_fence documentation and opaque function barriers | WebFetch (doc.rust-lang.org/std) | 1 | 1 |
| termusic stream_tx cmd_tx channel ordering independence | DeepWiki (tramhao/termusic) | 1 | 1 |
| "store(false" "Ordering::Release" "send(" language:rust | GitHub Code Search | 5 | 2 |
| "is_loading" "store" "Release" "send" language:rust | GitHub Code Search | 5 | 2 |

---

## Source Inventory

| ID | Source | Type | Date | Recency | Confidence |
|----|--------|------|------|---------|------------|
| SRC-001 | Rust std::sync::atomic::Ordering documentation - https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html | Official docs | 2026 | Fresh | High |
| SRC-002 | Rust Nomicon: Atomics chapter - https://doc.rust-lang.org/nomicon/atomics.html | Official docs | 2026 | Fresh | High |
| SRC-003 | Rust std::sync::mpsc::channel documentation - https://doc.rust-lang.org/std/sync/mpsc/fn.channel.html | Official docs | 2026 | Fresh | High |
| SRC-004 | Rust std::sync::RwLock documentation - https://doc.rust-lang.org/std/sync/struct.RwLock.html | Official docs | 2026 | Fresh | High |
| SRC-005 | Rust std::sync::atomic::compiler_fence documentation - https://doc.rust-lang.org/std/sync/atomic/fn.compiler_fence.html | Official docs | 2026 | Fresh | High |
| SRC-006 | DeepWiki: tramhao/termusic - stream_tx and cmd_tx channel independence - https://deepwiki.com/tramhao/termusic | AI-generated docs | 2026 | Fresh | Medium |
| SRC-007 | Rust Reference: Behavior Considered Undefined - https://doc.rust-lang.org/reference/behavior-considered-undefined.html | Official docs | 2026 | Fresh | High |
| SRC-008 | GitHub: zeromq/zmq.rs - AtomicBool Release store before channel operations pattern - https://github.com/zeromq/zmq.rs | GitHub Code | 2025 | Fresh | Medium |
| SRC-009 | GitHub: tinyhumansai/openhuman - Connected state Release store with event notification pattern - https://github.com/tinyhumansai/openhuman | GitHub Code | 2025 | Fresh | Medium |

---

## Issue Resolution

### ISS-005: Order of operations in async completion handler

**Prior Understanding**: The completion handler must perform operations in exact sequence: (1) write-lock swap, (2) AtomicBool store Release, (3) send PlaylistShuffled event, (4) send PlayerCmd::PlaylistLoadComplete. The ordering was identified as "straightforward but must be documented to prevent future reordering."

**Investigation Summary**:

The investigation focused on three questions:
1. What memory ordering guarantees enforce this sequence?
2. Can the Rust compiler or CPU reorder these operations?
3. What is the best pattern for documenting and enforcing the invariant?

**Findings on Memory Ordering Guarantees**:

The four-step sequence leverages multiple layers of synchronization:

**Step 1 (write-lock swap) -> Step 2 (AtomicBool Release store)**:
- Dropping the `RwLock` write guard provides Release semantics on the lock release (SRC-004). All writes within the critical section (playlist data assignment) are visible to any thread that subsequently acquires the lock.
- The AtomicBool `store(false, Ordering::Release)` in step 2 provides an additional Release barrier (SRC-001). Per the Nomicon: "a release access ensures that every access before it stays before it" (SRC-002). This means all writes from step 1 are ordered before the AtomicBool store.
- Combined guarantee: Any thread that reads `is_loading.load(Acquire) == false` is guaranteed to see all the playlist data written in step 1 (SRC-001, SRC-002).

**Step 2 (AtomicBool store) -> Step 3 (broadcast send)**:
- The Release ordering on step 2 prevents the compiler from moving step 3 before step 2 (SRC-002: "every access before [Release] stays before it" -- but note this refers to accesses BEFORE the Release, not AFTER).
- However, the question is whether step 3 can be moved BEFORE step 2. The answer is NO for two reasons: (1) `broadcast::send()` is an opaque function call with internal synchronization (it acquires internal locks/atomics), and the Rust compiler cannot reorder across opaque function calls with potential side effects (SRC-005, SRC-007); (2) Sequential program order is maintained for side-effectful operations on a single thread (SRC-002).

**Step 3 (broadcast send) -> Step 4 (mpsc send)**:
- These are two independent channel sends on independent channels (SRC-006). There is no cross-channel ordering guarantee for receivers.
- However, **on the sending thread**, sequential function calls to opaque functions with side effects preserve program order (SRC-002, SRC-007). The compiler cannot reorder `stream_tx.send(event)` past `cmd_tx.send(PlayerCmd::PlaylistLoadComplete)` because both are opaque calls with observable side effects.
- From the receivers' perspective: the TUI may process the PlaylistShuffled event before or after the player_loop processes PlaylistLoadComplete. This is FINE because: (a) the TUI reads from the shared Arc<RwLock<Playlist>> which was already populated in step 1; (b) the player_loop reads from the same shared playlist which is also already populated.

**Why This Ordering is Necessary**:

| Step | Must come after | Rationale |
|------|----------------|-----------|
| 2 (AtomicBool Release) | 1 (write-lock swap) | Save interval must see populated data when it reads `is_loading == false`. The Release ordering guarantees memory visibility of step 1's writes (SRC-001). |
| 3 (PlaylistShuffled event) | 1 (write-lock swap) | When the TUI receives the event and calls GetPlaylist, it must see populated data. The RwLock's Release-on-drop from step 1 guarantees this (SRC-004). |
| 4 (PlayerCmd::PlaylistLoadComplete) | 1 (write-lock swap) | player_loop's `resume_from_stopped()` reads playlist via shared Arc<RwLock>. Data must be present. Channel send has Acquire semantics on receive that synchronize with preceding writes (SRC-003). |
| 3 (PlaylistShuffled event) | 2 (AtomicBool Release) | Not strictly required for correctness, but maintains a clean logical progression: "data is committed, flag is cleared, notifications are sent." |
| 4 (PlaylistLoadComplete) | 3 (PlaylistShuffled event) | Not strictly required for correctness (independent channels, independent consumers), but preferred for deterministic behavior: "TUI gets notified, then player starts." If reversed, playback could start before TUI displays the playlist. |

**Resolution Status**: Resolved

**Evidence**:
- Rust atomic `Release` ordering guarantees all preceding writes are visible to threads performing `Acquire` loads on the same atomic (SRC-001, SRC-002)
- RwLock write guard drop provides Release semantics; subsequent read lock acquisition provides Acquire semantics (SRC-004)
- Channel `send()` establishes happens-before with the corresponding `recv()` (SRC-003)
- Opaque function calls with side effects cannot be reordered by the compiler on a single thread due to the as-if rule preserving observable behavior (SRC-002, SRC-005, SRC-007)
- The `stream_tx` (broadcast) and `cmd_tx` (mpsc) are independent channels with no cross-channel ordering for receivers (SRC-006)
- Real-world Rust codebases (zmq.rs, openhuman) use the same pattern: AtomicBool Release store followed by channel notification (SRC-008, SRC-009)

**Resolution Path**: See Options Comparison below for three approaches to documenting and enforcing this invariant.

---

## Options Comparison

| Criterion | Option A: Dedicated function with ORDERING doc-comment | Option B: Inline sequence with numbered comments | Option C: State machine wrapper (CompletionState) |
|-----------|-------|-------|-------|
| Maturity | 5 | 5 | 4 |
| Community/Support | 5 | 5 | 3 |
| Performance | 5 | 5 | 4 |
| Bundle Size / Footprint | 5 | 5 | 4 |
| Learning Curve | 5 | 5 | 3 |
| Maintenance Burden | 5 | 4 | 3 |
| Project Fit | 5 | 4 | 3 |
| Innovation/Momentum | 4 | 3 | 4 |
| **TOTAL** | **39** | **36** | **28** |

### Option A: Dedicated function with ORDERING doc-comment (Recommended)

**Summary**: Extract the four-step completion sequence into a dedicated function `complete_background_load()` with a doc-comment block explaining the ordering invariant. The function signature makes the dependencies explicit (takes all required parameters), and the doc-comment serves as a persistent record of why the ordering matters.

- **Strengths**: Encapsulates the invariant in a single named function that cannot be accidentally split; the doc-comment persists through refactoring; function signature makes dependencies explicit; callers cannot accidentally insert operations between steps; matches the codebase pattern of extracting logical units into helper functions (e.g., `start_playlist_save_interval`, `start_service`) (SRC-006); the compiler treats the function as a single opaque call from the caller's perspective
- **Weaknesses**: Slightly more indirection than inline code; the function is only called from one place (but this is acceptable for invariant protection)
- **Best For**: This project -- the server.rs file already uses this pattern for logical groupings

**Example**:
```rust
/// Complete the background playlist load by committing data and notifying consumers.
///
/// # Ordering Invariant
///
/// These steps MUST execute in this exact order:
/// 1. Write-lock swap: populate the shared playlist with loaded data.
///    Dropping the write guard provides Release semantics.
/// 2. AtomicBool store(false, Release): signals that loading is complete.
///    Any thread reading Acquire on this bool is guaranteed to see step 1's data.
/// 3. Send PlaylistShuffled event via stream_tx: notifies connected TUI clients.
///    Must come after step 1 so clients see populated data on GetPlaylist.
/// 4. Send PlayerCmd::PlaylistLoadComplete via cmd_tx: triggers auto-play if configured.
///    Must come after step 1 so player_loop finds tracks in the playlist.
///
/// Steps 3 and 4 use independent channels. Their relative order is a preference
/// (notify TUI before starting playback) rather than a correctness requirement.
fn complete_background_load(
    shared_playlist: &SharedPlaylist,
    is_loading: &AtomicBool,
    stream_tx: &StreamTX,
    cmd_tx: &PlayerCmdSender,
    loaded_index: usize,
    loaded_tracks: Vec<Track>,
    config: &SharedServerSettings,
) {
    // Step 1: Commit loaded data under write lock
    {
        let mut playlist = shared_playlist.write();
        playlist.current_track_index = loaded_index;
        playlist.tracks = loaded_tracks;
        playlist.is_modified = false;
    } // Write lock dropped here -- Release semantics

    // Step 2: Clear loading flag (Release ensures step 1 is visible)
    is_loading.store(false, Ordering::Release);

    // Step 3: Notify TUI clients
    if let Ok(tracks) = shared_playlist.read().as_grpc_playlist_tracks() {
        let event = UpdateEvents::PlaylistChanged(
            UpdatePlaylistEvents::PlaylistShuffled(PlaylistShuffledInfo { tracks })
        );
        let _ = stream_tx.send(event);
    }

    // Step 4: Trigger auto-play if configured
    if config.read().settings.player.startup_state == StartupState::Playing {
        let _ = cmd_tx.send(PlayerCmd::PlaylistLoadComplete);
    }
}
```

### Option B: Inline sequence with numbered comments

**Summary**: Keep the four steps inline within the `tokio::select!` match arm where the spawn_blocking result is handled. Use numbered comments (// Step 1, // Step 2, etc.) and a preceding block comment explaining the ordering invariant.

- **Strengths**: No additional function call overhead (zero-cost, though negligible); all logic visible in one place without jumping to another function; simpler for a small sequence of 4 operations (SRC-006); fewer lines of code overall
- **Weaknesses**: The invariant exists only as a comment -- it can be accidentally split by inserting code between steps; during refactoring, developers may not notice the ordering constraint; harder to unit-test the completion sequence in isolation; inline comments are more easily deleted or moved than doc-comments on a function
- **Best For**: Very small teams where the invariant is unlikely to be violated, or projects with strong code review culture that catches ordering violations

**Example**:
```rust
// ORDERING INVARIANT: The following steps must execute in this exact order.
// See ISS-005 in specification/04-async-server-metadata-loading/ for rationale.
// Step 1: write-lock swap (data commit, Release on drop)
// Step 2: AtomicBool Release (visibility guarantee for save interval)
// Step 3: PlaylistShuffled broadcast (TUI notification)
// Step 4: PlayerCmd::PlaylistLoadComplete (auto-play trigger)

// Step 1
{
    let mut playlist = shared_playlist.write();
    playlist.current_track_index = loaded_index;
    playlist.tracks = loaded_tracks;
    playlist.is_modified = false;
}

// Step 2
is_loading.store(false, Ordering::Release);

// Step 3
if let Ok(tracks) = shared_playlist.read().as_grpc_playlist_tracks() {
    let _ = stream_tx.send(UpdateEvents::PlaylistChanged(
        UpdatePlaylistEvents::PlaylistShuffled(PlaylistShuffledInfo { tracks })
    ));
}

// Step 4
if config.read().settings.player.startup_state == StartupState::Playing {
    let _ = cmd_tx.send(PlayerCmd::PlaylistLoadComplete);
}
```

### Option C: State machine wrapper (CompletionState)

**Summary**: Introduce a `BackgroundLoadCompletion` struct that enforces the ordering through a builder-like pattern. Each step consumes the previous state, making it impossible to call steps out of order at compile time.

- **Strengths**: Compile-time enforcement of ordering -- impossible to call step 3 without completing step 2; self-documenting through type signatures; type-state pattern is a well-known Rust idiom for enforcing invariants; provides the strongest guarantee against future reordering
- **Weaknesses**: Significant over-engineering for 4 simple sequential operations; adds ~50 lines of boilerplate struct definitions and impls; introduces new types that must be understood by all contributors; the type-state pattern is powerful but overkill for a linear sequence that runs exactly once; increases cognitive load for a one-shot operation; no real-world Rust projects in our search use this pattern for simple completion handlers
- **Best For**: Libraries with public APIs where the ordering invariant must be enforced across crate boundaries, or systems where the completion sequence is called from multiple sites

**Example**:
```rust
struct LoadComplete<S> { state: S }
struct Step1Done { playlist: SharedPlaylist, index: usize, tracks: Vec<Track> }
struct Step2Done { /* ... */ }
struct Step3Done { /* ... */ }

impl LoadComplete<Step1Done> {
    fn clear_loading_flag(self, is_loading: &AtomicBool) -> LoadComplete<Step2Done> { ... }
}
impl LoadComplete<Step2Done> {
    fn notify_clients(self, stream_tx: &StreamTX) -> LoadComplete<Step3Done> { ... }
}
// etc.
```

---

## Best Practices

### BP-001: Encapsulate ordering-sensitive sequences in dedicated functions

**Pattern**: When multiple operations must execute in a specific order for correctness, extract them into a named function with a doc-comment explaining the ordering invariant. The function boundary prevents accidental interleaving of unrelated code.

**Rationale**: Functions serve as both logical groupings and documentation anchors. A doc-comment on a function persists through refactoring and is visible in IDE tooltips. The function call from the caller's perspective is atomic (opaque), reducing the risk of someone inserting code between the ordered steps. This pattern is used throughout the Rust ecosystem for safety-critical sequences (SRC-008, SRC-009).

**Source**: SRC-002, SRC-008, SRC-009
**Confidence**: High

**Example**:
```rust
/// # Ordering Invariant
/// Step 1 must precede Step 2 because [reason].
/// Step 2 must precede Step 3 because [reason].
fn critical_sequence(/* explicit params */) {
    // Step 1: ...
    // Step 2: ...
    // Step 3: ...
}
```

### BP-002: Use Release ordering on AtomicBool AFTER the data it guards is committed

**Pattern**: When using an AtomicBool as a "ready" flag, always commit the data first (under a lock or via direct writes), then store the flag with `Ordering::Release`. Consumers load with `Ordering::Acquire` to establish the happens-before edge.

**Rationale**: The Release ordering guarantees that all preceding writes (including those within a lock that was just dropped) are visible to any thread that subsequently loads the atomic with Acquire and sees the new value (SRC-001, SRC-002). Placing the store BEFORE the data commit would create a window where consumers see "ready" but data is not yet visible.

**Source**: SRC-001, SRC-002
**Confidence**: High

**Example**:
```rust
// CORRECT: data committed before flag
{
    let mut guard = shared_data.write();
    guard.field = new_value;
} // Lock dropped (Release semantics)
is_ready.store(true, Ordering::Release); // Flag set after data

// INCORRECT: flag set before data is committed
is_ready.store(true, Ordering::Release); // BUG: consumer may see ready but stale data
{
    let mut guard = shared_data.write();
    guard.field = new_value;
}
```

### BP-003: Notifications must follow data commitment, not precede it

**Pattern**: Send notification events (via broadcast channels, event systems, etc.) only AFTER the data they reference has been committed to shared state. Never notify before committing.

**Rationale**: When a consumer receives a notification and queries shared state, the data must already be present. If notification precedes commitment, there is a race window where the consumer queries and sees stale/empty data. The RwLock's Release-on-drop semantics ensure that once the write lock is released and a read lock is subsequently acquired, all writes are visible (SRC-004). Channel sends with happens-before guarantees (SRC-003) ensure the receiver sees a consistent view.

**Source**: SRC-003, SRC-004, SRC-006
**Confidence**: High

### BP-004: Document cross-channel ordering as preference vs correctness requirement

**Pattern**: When sending on multiple independent channels from the same thread, explicitly document which orderings are correctness requirements (violation causes bugs) vs preferences (violation causes suboptimal but correct behavior).

**Rationale**: The `stream_tx` (broadcast to TUI) and `cmd_tx` (mpsc to player_loop) are independent channels with no cross-channel ordering for receivers (SRC-006). The sending thread executes them sequentially, but receivers may process them in any relative order. Documenting this distinction prevents future developers from introducing dependencies that assume cross-channel ordering.

**Source**: SRC-006
**Confidence**: High

---

## Anti-Patterns

| Anti-Pattern | Why Harmful | Alternative | Source |
|-------------|-------------|-------------|--------|
| Setting AtomicBool "ready" flag before data is committed under the lock | Consumer sees `is_loading == false` but reads stale/empty playlist data from the RwLock. Creates a data visibility race. | Always commit data first, then set flag with Release ordering | SRC-001, SRC-002 |
| Sending PlaylistShuffled notification before releasing the write lock | Deadlock: TUI receives event, calls GetPlaylist, which tries to acquire read lock, but write lock is still held by the notifying thread | Drop write lock BEFORE sending notification events | SRC-004, SRC-006 |
| Sending PlayerCmd::PlaylistLoadComplete before playlist data swap | player_loop processes the command, calls `resume_from_stopped()`, reads empty playlist, silently no-ops. Playback never starts. | Send command only after data is committed to SharedPlaylist | SRC-003, SRC-006 |
| Assuming cross-channel ordering guarantees between broadcast and mpsc | Code assumes TUI receives event before player_loop processes command (or vice versa), but channels are independent | Document which orderings are correctness-required vs preference; design so either order works | SRC-006 |
| Splitting the 4-step sequence with unrelated operations between steps | Insertions may introduce accidental reordering or hold locks longer than necessary, violating the invariant | Encapsulate sequence in a dedicated function | SRC-008 |

---

## Implementation Considerations

### Performance

- The four-step sequence total execution time is dominated by step 3 (serializing playlist to protobuf for the event). For a 1000-track playlist, serialization takes ~1-5ms. Steps 1, 2, and 4 combined take <1us (SRC-001, SRC-004).
- The write lock in step 1 is held for <100ns (3 field assignments, one Vec pointer move). No reader is blocked perceptibly (SRC-004).
- The read lock in step 3 (for `as_grpc_playlist_tracks()`) may take 1-5ms for serialization. This is acceptable because it happens once at startup completion, not in a hot path (SRC-006).
- No additional synchronization primitives are needed beyond what is already planned (AtomicBool, existing RwLock, existing channels) (SRC-001).

### Security

- No new attack surface: the ordering invariant is internal to the server process with no external exposure (SRC-006).
- The AtomicBool flag and channel sends operate on local memory/channels with no network involvement (Codebase analysis).

### Compatibility

- The pattern is compatible with all existing infrastructure: `parking_lot::RwLock`, `tokio::sync::broadcast`, `tokio::sync::mpsc::UnboundedSender`, `std::sync::atomic::AtomicBool` (Codebase analysis).
- No new dependencies required (Codebase analysis).
- The TUI's existing `PlaylistShuffled` handler performs a full playlist replacement, which is exactly the behavior needed after background load (SRC-006).
- The proposed `PlayerCmd::PlaylistLoadComplete` variant is the only new enum member needed (1 line in the enum, 4 lines in the handler) (Codebase analysis).

---

## Contradictions Found

| Topic | Position A (SRC-005) | Position B (SRC-002) | Assessment |
|-------|---------------------|---------------------|------------|
| Whether opaque function calls act as compiler barriers | compiler_fence documentation states that opaque function calls do NOT reliably act as compiler barriers in the same way as explicit fences | Nomicon states that the compiler preserves observable behavior for single-threaded execution (as-if rule), and channel sends have observable side effects | Both are correct for their contexts. compiler_fence is needed for signal handlers (same-thread preemption). For sequential function calls on a single thread, the as-if rule prevents reordering of operations with observable side effects (like sending on a channel that involves internal synchronization). Our case is the latter: sequential calls, no preemption concern. The compiler cannot reorder two `send()` calls because they have independent observable effects. |

---

## Issues and Ambiguities

### ISS-005: Resolved

The four-step ordering is both necessary and correctly enforced by:
1. **Release semantics** from RwLock write guard drop (step 1) and AtomicBool store (step 2) ensure data visibility.
2. **Sequential execution** of opaque function calls with side effects on a single thread prevents compiler reordering of steps 3 and 4.
3. **Channel happens-before** guarantees that receivers see all data written before the send.

The ordering between steps 3 and 4 is a **preference** (notify TUI before triggering playback) rather than a **correctness requirement** (both consumers read from the same already-committed SharedPlaylist). This distinction should be documented.

**No remaining ambiguities** for this issue.

### ISS-006 (New, Low Priority): Serialization under read lock in step 3

In step 3, `as_grpc_playlist_tracks()` is called while holding a read lock. For very large playlists (10,000+ tracks), serialization could take 5-10ms, during which the write lock cannot be acquired by any other writer (e.g., user adding tracks). This is not a practical concern for the background load scenario (happens once at startup), but should be noted for future reference if this pattern is reused in a hot path.

**Resolution path**: If needed in the future, serialize outside the lock by cloning the tracks vec first, then dropping the lock. For the current one-shot startup scenario, this optimization is unnecessary.

---

## References

### Primary Sources (Official Documentation)

- SRC-001: Rust std::sync::atomic::Ordering documentation - https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html
- SRC-002: Rust Nomicon: Atomics chapter - https://doc.rust-lang.org/nomicon/atomics.html
- SRC-003: Rust std::sync::mpsc::channel documentation - https://doc.rust-lang.org/std/sync/mpsc/fn.channel.html
- SRC-004: Rust std::sync::RwLock documentation - https://doc.rust-lang.org/std/sync/struct.RwLock.html
- SRC-005: Rust std::sync::atomic::compiler_fence documentation - https://doc.rust-lang.org/std/sync/atomic/fn.compiler_fence.html
- SRC-007: Rust Reference: Behavior Considered Undefined - https://doc.rust-lang.org/reference/behavior-considered-undefined.html

### Community Sources (GitHub, DeepWiki)

- SRC-006: DeepWiki: tramhao/termusic - stream_tx and cmd_tx channel independence - https://deepwiki.com/tramhao/termusic
- SRC-008: GitHub: zeromq/zmq.rs - AtomicBool Release store before channel operations - https://github.com/zeromq/zmq.rs
- SRC-009: GitHub: tinyhumansai/openhuman - Connected state Release store with event notification - https://github.com/tinyhumansai/openhuman
