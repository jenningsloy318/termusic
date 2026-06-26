<document type="debug-analysis">

<metadata>
  <field name="title">Debug Analysis: Server blocks on synchronous metadata loading preventing TUI connection</field>
  <field name="date">2026-06-26T14:00:00+08:00</field>
  <field name="author">super-dev:debug-analyzer</field>
  <field name="status">Hypotheses Pending</field>
  <field name="severity">Critical</field>
</metadata>

<section title="Issue Summary">
  <table>
    <row header="true">
      <cell>Field</cell>
      <cell>Value</cell>
    </row>
    <row><cell>Symptom</cell><cell>TUI displays "Connecting is taking more time than expected..." followed by "Error: Server output during start" with lofty metadata warnings in stderr. Server takes too long to start accepting gRPC connections.</cell></row>
    <row><cell>Expected Behavior</cell><cell>Server should accept gRPC connections within 1 second of process start, regardless of playlist size. Metadata loading should happen asynchronously in the background after the server is connectable.</cell></row>
    <row><cell>Actual Behavior</cell><cell>Server blocks on `Playlist::new_shared()` at server.rs:148-149, which calls `load_apply()` synchronously. The gRPC listener (`start_service()`) is not started until ALL track metadata has been parsed by lofty. For large playlists, this takes 1.5-10+ seconds, exceeding the TUI's 5-second warning threshold.</cell></row>
    <row><cell>First Observed</cell><cell>Always present since the current architecture; exacerbated after spec-03 (parallel loading sped up metadata reads but did not decouple from server readiness)</cell></row>
    <row><cell>Frequency</cell><cell>Always — deterministic for playlists with 100+ local audio files requiring metadata I/O</cell></row>
    <row><cell>Environment</cell><cell>Linux 7.0.13-1-liquorix-amd64, termusic v0.13.2-102-gd171d85c, Rust workspace</cell></row>
  </table>
</section>

<section title="Evidence Collected">
  <subsection title="Error Messages">
    <code lang="text">
Connecting is taking more time than expected...
Error: Server output during start:
---STDOUT---
Logging to file "/tmp/termusic-server.log"

---STDERR---
[2026-06-26T13:35:29.041+08:00 INFO termusic_server::logger]: Termusic-server version v0.13.2-102-gd171d85c[g]
[2026-06-26T13:35:29.041+08:00 INFO termusic_server]: Server starting...
[2026-06-26T13:35:30.035+08:00 WARN lofty::mpeg::properties]: MPEG: Using bitrate to estimate duration
[2026-06-26T13:35:30.035+08:00 WARN lofty::mp4::atom_info]: Encountered an atom with invalid characters, stopping
[2026-06-26T13:35:30.039+08:00 WARN lofty::iff::chunk]: Chunk exceeds reader size, stopping
... (20+ lofty warnings continuing for ~1.6+ seconds from 13:35:30.035 to at least 13:35:30.711)
    </code>
  </subsection>

  <subsection title="Logs">
    <code lang="text">
Timeline analysis:
- T+0.000s (13:35:29.041): Server starting...
- T+0.994s (13:35:30.035): First lofty metadata read warning (still loading)
- T+1.670s (13:35:30.711): Last visible lofty warning (loading continues beyond log cutoff)
- T+5.000s: TUI prints "Connecting is taking more time than expected..." (WAIT_MESSAGE_TIME=5s)
- The log is truncated — loading likely continues well past the 5s mark for this playlist
    </code>
  </subsection>

  <subsection title="Visual Evidence">
    <paragraph>N/A — terminal application, evidence is in log output above</paragraph>
  </subsection>

  <subsection title="Context">
    <list type="unordered">
      <item name="Recent Changes">Commit d171d85c merged spec-03 (parallel metadata loading via rayon), which improved loading speed but did not change the server startup architecture. The server still synchronously blocks on `Playlist::new_shared()` before calling `start_service()`.</item>
      <item name="Affected Scope">All users with playlists containing 100+ local audio files. More tracks = longer delay. The rayon parallelization helps but cannot reduce wall-clock time below the I/O-bound minimum for the slowest track metadata read.</item>
      <item name="Related Issues">This is the architectural problem that spec-04 (async server metadata loading) aims to solve. The requirements doc proposes Option 1: start server with empty playlist, load metadata in background.</item>
    </list>
  </subsection>
</section>

<section title="Reproduction Strategy">
  <field name="technique">CLI invocation</field>
  <field name="deterministic">Yes</field>

  <subsection title="Steps to Reproduce">
    <list type="ordered">
      <item>Ensure `playlist.log` contains 200+ local audio file paths (typical user library)</item>
      <item>Run `termusic` (or directly run `termusic-server` and observe startup timing)</item>
      <item>Observe that "Server starting..." appears immediately but "Server listening on ..." does not appear until all metadata is loaded (1.5-10+ seconds later)</item>
      <item>The TUI times out or shows the warning message during this gap</item>
    </list>
  </subsection>

  <subsection title="Minimal Reproduction">
    <paragraph>Run `termusic-server` with a playlist.log containing 500+ local audio file paths. Time the gap between "Server starting..." and "Server listening on ...". Any gap exceeding 1 second confirms the blocking behavior.</paragraph>
  </subsection>

  <subsection title="Reproduction Confirmation">
    <paragraph>Confirmed from user-provided logs: 1.67+ seconds of visible metadata loading activity before the server can possibly reach start_service(). The log cutoff suggests total loading time significantly exceeds 5 seconds (the TUI message threshold was triggered).</paragraph>
  </subsection>
</section>

<section title="Code Execution Path">
  <diagram type="ascii">
actual_main() → get_config() → Playlist::new_shared() [BLOCKS HERE] → start_service() → gRPC listener
                                       ↓
                              load_apply() → load()
                                       ↓
                              parallel_read_local_tracks() [rayon, but still synchronous call]
                                       ↓
                              lofty metadata reads (1.5-10+ seconds for 500+ files)
  </diagram>

  <subsection title="Trace">
    <table>
      <row header="true">
        <cell>Step</cell>
        <cell>Location</cell>
        <cell>Action</cell>
        <cell>Data State</cell>
      </row>
      <row>
        <cell>1</cell>
        <cell>server/src/server.rs:121</cell>
        <cell>info!("Server starting...")</cell>
        <cell>Config loaded, no playlist yet</cell>
      </row>
      <row>
        <cell>2</cell>
        <cell>server/src/server.rs:148-149</cell>
        <cell>Playlist::new_shared(&config, stream_tx.clone())</cell>
        <cell>BLOCKS — calls load_apply() which reads all file metadata synchronously</cell>
      </row>
      <row>
        <cell>3</cell>
        <cell>playback/src/playlist.rs:83</cell>
        <cell>playlist.load_apply()</cell>
        <cell>Calls Self::load() which does parallel_read_local_tracks via rayon</cell>
      </row>
      <row>
        <cell>4</cell>
        <cell>playback/src/playlist.rs:245</cell>
        <cell>parallel_load::parallel_read_local_tracks(&classified.local_entries)</cell>
        <cell>Rayon par_iter processes all local files — BLOCKING for 1.5-10+ seconds</cell>
      </row>
      <row>
        <cell>5</cell>
        <cell>server/src/server.rs:175</cell>
        <cell>start_service(&config, music_player_service, ...)</cell>
        <cell>Only NOW does the gRPC listener start — too late for TUI's timeout</cell>
      </row>
    </table>
  </subsection>
</section>

<section title="Hypotheses">
  <paragraph>Ranked by likelihood. Each hypothesis has a falsifiable prediction. These hypotheses address WHY the server startup is slow and WHERE the fix should be applied.</paragraph>

  <subsection title="HYP-001: Synchronous metadata loading blocks gRPC listener startup">
    <field name="likelihood">High</field>
    <field name="confidence">0.95</field>
    <field name="prediction">If `Playlist::new_shared()` is replaced with creating an empty SharedPlaylist (skipping `load_apply()`), and `start_service()` is called immediately after, the TUI will connect within 1 second even with a 1000+ track playlist.log file present.</field>
    <field name="supporting-evidence">Log timestamps show ~1.67s of visible metadata activity before log is truncated, and TUI's 5s warning fires — meaning start_service() has not been reached. Code at server.rs:148-175 shows new_shared() is called synchronously before start_service(). The 5-Whys analysis in the requirements doc confirms this architectural coupling.</field>
    <field name="contradicting-evidence">None</field>
    <field name="verification-method">Trace the code path: confirm that no gRPC listener or socket is opened before line 175 (start_service). Alternatively, instrument with a timestamp log before and after new_shared() to measure the blocking duration.</field>
    <field name="result">UNVERIFIED</field>
    <field name="result-evidence">Pending code investigation</field>
  </subsection>

  <subsection title="HYP-002: Rayon thread pool contention causes metadata loading to be slower than expected">
    <field name="likelihood">Medium</field>
    <field name="confidence">0.35</field>
    <field name="prediction">If the PLAYLIST_POOL (or global rayon pool) is being contended with other startup tasks, increasing the thread pool size or ensuring exclusive pool access during startup will reduce the metadata loading wall-clock time by 20%+. Conversely, if there is no contention, changing pool size will have negligible effect on loading time.</field>
    <field name="supporting-evidence">The rayon parallelization from spec-03 introduced a PLAYLIST_POOL (LazyLock ThreadPool). If this pool shares resources with other rayon work items, or if the pool size is too small for the I/O-bound metadata reads, loading could be slower than optimal.</field>
    <field name="contradicting-evidence">The logs show metadata reads happening rapidly (many warnings within 0.5-0.7s), suggesting the parallel pool IS working. The fundamental issue is architectural (blocking before server start) rather than performance of the loading itself. Even a fast parallel load of 500+ files takes 1-3 seconds due to disk I/O.</field>
    <field name="verification-method">Compare metadata loading time with different PARALLEL_THRESHOLD values and pool sizes. If pool size changes do not significantly affect wall-clock time, this hypothesis is refuted for the "too slow" aspect. The core issue remains that ANY synchronous loading > 1s blocks server readiness.</field>
    <field name="result">UNVERIFIED</field>
    <field name="result-evidence">Pending performance measurement</field>
  </subsection>

  <subsection title="HYP-003: Lofty metadata parsing of corrupt/unusual files causes disproportionate slowdown">
    <field name="likelihood">Medium</field>
    <field name="confidence">0.30</field>
    <field name="prediction">If the specific files triggering lofty warnings ("Chunk exceeds reader size", "atom with invalid characters", "Using bitrate to estimate duration") are removed from the playlist, loading time will decrease significantly (by 50%+). Conversely, if these files are NOT disproportionately slow, removing them will have minimal impact on total loading time.</field>
    <field name="supporting-evidence">The user's logs show 20+ lofty warnings for different file types (MPEG, MP4, IFF). These could represent files that lofty spends extra time on (scanning for valid data, retrying, falling back to estimation). The "Chunk exceeds reader size, stopping" warnings suggest lofty is encountering invalid data and must do extra processing to recover.</field>
    <field name="contradicting-evidence">Lofty typically exits quickly on corrupt files (the "stopping" messages suggest it gives up early). The timestamps show all warnings clustered within ~0.7s (30.035 to 30.711), suggesting these are fast failures rather than slow hangs. The problem is likely the VOLUME of files, not individual slow files.</field>
    <field name="verification-method">Profile individual file read times. Identify the 10 slowest files and check if they correlate with the warning-emitting files. If the slow files are NOT the warning files, this is refuted.</field>
    <field name="result">UNVERIFIED</field>
    <field name="result-evidence">Pending profiling</field>
  </subsection>

  <subsection title="HYP-004: TUI timeout/warning thresholds are too aggressive for the server's architectural design">
    <field name="likelihood">Low</field>
    <field name="confidence">0.15</field>
    <field name="prediction">If WAIT_MESSAGE_TIME is increased from 5s to 15s and WAIT_TIMEOUT from 30s to 60s, the user will no longer see the warning message for typical playlists (under 1000 tracks). However, this does NOT fix the underlying problem — the server still cannot serve gRPC requests until loading completes.</field>
    <field name="supporting-evidence">The TUI prints "Connecting is taking more time than expected..." at 5s (WAIT_MESSAGE_TIME). The server may successfully start at 6-8s for a moderately large playlist. Increasing thresholds would mask the symptom.</field>
    <field name="contradicting-evidence">This is a band-aid, not a fix. The requirement states the server MUST accept connections within 1 second (AC-01). Even with relaxed timeouts, the user experiences dead time where the TUI is frozen. The user explicitly asked for async metadata loading, not longer timeouts.</field>
    <field name="verification-method">Increase WAIT_MESSAGE_TIME to 30s. If the error disappears but startup still takes 5-10s of blank screen, this confirms the timeout is not the root cause — the architecture is.</field>
    <field name="result">UNVERIFIED</field>
    <field name="result-evidence">Pending — likely to be rejected as root cause (it is a contributing symptom amplifier, not the cause)</field>
  </subsection>

  <subsection title="HYP-005: The server startup sequence has no mechanism for deferred/async playlist initialization">
    <field name="likelihood">High</field>
    <field name="confidence">0.92</field>
    <field name="prediction">If the server is refactored to: (1) create an empty SharedPlaylist, (2) call start_service() immediately, (3) spawn background metadata loading on a separate thread pool, and (4) atomically swap the playlist data when complete — then the TUI will connect within 1 second AND the fully loaded playlist will be available within the same total wall-clock time as before.</field>
    <field name="supporting-evidence">The code at server.rs:148-175 shows a strict sequential dependency: new_shared() THEN start_service(). There is no architectural pattern in the current code for deferred initialization. The requirements doc Option 1 proposes exactly this decoupling. The existing `Playlist::load()` returns `(usize, Vec<Track>)` which is already structured for a swap-in-later pattern.</field>
    <field name="contradicting-evidence">None — this is the architectural gap identified by the requirements analysis. The deep research report (ISS-005) already designed the completion handler ordering for this exact fix.</field>
    <field name="verification-method">Verify that (1) no code between Playlist creation and start_service() depends on the playlist being populated, and (2) the player_loop can handle receiving an initially-empty playlist without crashing. If both are true, the deferred loading approach is validated.</field>
    <field name="result">UNVERIFIED</field>
    <field name="result-evidence">Pending code verification of dependencies between playlist state and service initialization</field>
  </subsection>
</section>

<section title="Verified Root Cause">
  <field name="confirmed-hypothesis">HYP-001 (pending formal verification, but evidence is overwhelming)</field>
  <field name="root-cause">The server's `actual_main()` function calls `Playlist::new_shared()` synchronously at server.rs:148-149, which blocks on disk I/O (lofty metadata parsing via rayon) for 1.5-10+ seconds for large playlists, before calling `start_service()` at line 175 to open the gRPC listener. The TUI cannot connect until the listener is open, causing timeout failures.</field>
  <field name="location">server/src/server.rs:148-149 (blocking call) and server/src/server.rs:175 (deferred service start)</field>

  <subsection title="Evidence Chain">
    <list type="ordered">
      <item>Log timestamp gap: "Server starting..." at T+0s, first metadata activity at T+1s, lofty warnings continuing past T+1.67s, TUI warning at T+5s — server is still loading metadata when TUI gives up waiting</item>
      <item>Code structure: server.rs line 148-149 calls new_shared() which calls load_apply() which calls parallel_read_local_tracks() — all synchronous, all before line 175 start_service()</item>
      <item>Architecture: No mechanism exists to start the gRPC listener independently of playlist state. The MusicPlayerService requires a SharedPlaylist at construction time (line 153-159), and start_service() requires the MusicPlayerService</item>
    </list>
  </subsection>

  <subsection title="Why It Wasn't Caught">
    <paragraph>The original architecture assumed playlists would be small enough to load within 1 second. As users accumulated larger libraries (500+ tracks), the synchronous loading time exceeded the TUI connection timeout. The spec-03 parallel loading optimization improved throughput but did not address the architectural coupling between server readiness and playlist completeness. No integration test measures time-to-first-connection as a function of playlist size.</paragraph>
  </subsection>
</section>

<section title="Recommended Fix">
  <subsection title="Fix Approach">
    <paragraph>Implement Option 1 from the requirements document: Deferred Loading with Immediate Server Start. Create an empty SharedPlaylist, start the gRPC service immediately, then spawn Playlist::load() on the existing PLAYLIST_POOL thread pool. When loading completes, atomically swap the data into the SharedPlaylist and notify connected clients via the PlaylistShuffled event. Add an AtomicBool `is_loading` flag to prevent the save interval from overwriting playlist.log during loading, and to defer auto-play until loading completes.</paragraph>
  </subsection>

  <subsection title="Code Locations">
    <table>
      <row header="true">
        <cell>File</cell>
        <cell>Line</cell>
        <cell>Change</cell>
      </row>
      <row>
        <cell>server/src/server.rs</cell>
        <cell>148-149</cell>
        <cell>Replace `Playlist::new_shared()` with creating an empty SharedPlaylist via `Arc::new(RwLock::new(Playlist::new(&config, stream_tx.clone())))` (skip load_apply)</cell>
      </row>
      <row>
        <cell>server/src/server.rs</cell>
        <cell>175 (after start_service)</cell>
        <cell>Spawn background loading task: `tokio::task::spawn_blocking` or use PLAYLIST_POOL to run `Playlist::load()`, then swap results into SharedPlaylist</cell>
      </row>
      <row>
        <cell>server/src/server.rs</cell>
        <cell>240-264 (start_playlist_save_interval)</cell>
        <cell>Add `is_loading` AtomicBool check — skip save if loading is in progress</cell>
      </row>
      <row>
        <cell>server/src/server.rs</cell>
        <cell>333-335 (startup_state Playing check)</cell>
        <cell>Move auto-play logic to after loading completes (triggered by new PlayerCmd::PlaylistLoadComplete)</cell>
      </row>
      <row>
        <cell>playback/src/lib.rs (PlayerCmd enum)</cell>
        <cell>TBD</cell>
        <cell>Add `PlayerCmd::PlaylistLoadComplete` variant for post-load auto-play trigger</cell>
      </row>
    </table>
  </subsection>

  <subsection title="Alternative Approaches">
    <list type="unordered">
      <item>Option 2 (Progressive Loading): Insert placeholder tracks immediately, enrich metadata in background. Higher complexity, marginal UX benefit over 1-3s of empty playlist. Deferred to follow-up.</item>
      <item>Option 4 (Increase TUI timeout): Band-aid that masks the symptom without fixing the architectural problem. Rejected per AC-01 requirement.</item>
      <item>Tokio spawn_blocking instead of PLAYLIST_POOL: Simpler but uses tokio's blocking thread pool which may interfere with async tasks. Prefer dedicated PLAYLIST_POOL from spec-03.</item>
    </list>
  </subsection>
</section>

<section title="Regression Test Strategy">
  <subsection title="Test Seam">
    <paragraph>Integration test level: measure time from server process start to gRPC connection acceptance. The test creates a large playlist.log fixture (500+ entries), starts the server, and asserts connection is accepted within 1 second. This tests the architectural decoupling without needing to mock lofty.</paragraph>
  </subsection>

  <subsection title="Test Cases">
    <table>
      <row header="true">
        <cell>Test Name</cell>
        <cell>Input</cell>
        <cell>Expected Output</cell>
        <cell>Verifies</cell>
      </row>
      <row>
        <cell>test_server_accepts_connection_within_1s_large_playlist</cell>
        <cell>playlist.log with 1000 local file paths</cell>
        <cell>gRPC connection accepted within 1 second of process start</cell>
        <cell>HYP-001/HYP-005: server readiness decoupled from metadata loading</cell>
      </row>
      <row>
        <cell>test_playlist_populated_after_background_load</cell>
        <cell>playlist.log with known tracks</cell>
        <cell>GetPlaylist returns full playlist (correct order, metadata) after loading notification</cell>
        <cell>AC-03: correctness preserved after async loading</cell>
      </row>
      <row>
        <cell>test_save_interval_skipped_during_loading</cell>
        <cell>Trigger save interval while is_loading is true</cell>
        <cell>playlist.log file is NOT overwritten during loading</cell>
        <cell>AC-07: save protection during background load</cell>
      </row>
      <row>
        <cell>test_autoplay_deferred_until_load_complete</cell>
        <cell>startup_state=Playing, background load in progress</cell>
        <cell>No playback until PlaylistLoadComplete is received</cell>
        <cell>AC-06: playback deferred until valid playlist</cell>
      </row>
    </table>
  </subsection>
</section>

<section title="Prevention Recommendations">
  <list type="unordered">
    <item name="Process">Add a CI performance gate that measures server time-to-connection with a 500-track fixture playlist. Fail the build if connection takes longer than 2 seconds. This catches any future regression that re-introduces synchronous blocking on the startup path.</item>
    <item name="Monitoring">Add structured logging at server startup: log elapsed time between "Server starting..." and "Server listening on ..." as a dedicated metric. Alert if this exceeds 1 second.</item>
    <item name="Architecture">Establish the pattern that all I/O-bound initialization (metadata loading, database queries, network fetches) MUST be performed after the service listener is ready. Document this as an architectural invariant in the server module doc-comments. The server's "readiness" is defined by gRPC listener availability, not by data completeness.</item>
  </list>
</section>

</document>
