# Architecture

Voldiq has one reusable engine crate, a desktop adapter and a separate headless CLI/MCP adapter. The headless binary depends on `voldiq-core` and the official MCP Rust SDK, without Tauri or a renderer.

## Authorized headless services

`agents` owns connection policies and digest-only credential storage. Windows authorization opens metadata handles from the volume root to the target, refuses reparse points and ambiguous path syntax, pins ancestor names and compares native root identities. Capabilities and credentials are rechecked on every call. Exclusions also match directory identity so a renamed excluded directory does not become an allowed metadata path.

`jobs` stores bounded scan status and cancellation in SQLite. An OS-released `scan.lock` serializes scans across desktop and headless processes using the same state directory. Interrupted jobs are marked when the next lease is acquired and are never retried automatically. The desktop retains its existing `IndexId` publication rules; its in-memory progress is not yet mirrored into the durable job table.

`service::AgentService` scans only permitted roots with verified exclusions, persists compact snapshots through the existing store and serves bounded queries from one cached index. Connections can read shared desktop/agent snapshots only after a small scope query verifies native root identity and sufficient exclusion coverage. Unauthorized roots fail before loading the index. Filtered history pages use one SQLite read view and expose no unfiltered totals. Jobs remain owner-scoped. `voldiq-headless` implements versioned CLI output and eight `rmcp` stdio analysis tools, with bounded protocol input/output and worker concurrency. Human provisioning is a separate desktop command path with native review; agents cannot provision, rotate or approve through their transport. Agent plan approval and file execution are still pending. See [MCP](MCP.md) and [remaining coverage](WORKBENCH-COVERAGE.md).

## Engine

`scanner` performs an iterative directory walk with cancellation and bounded metadata work. Manual concurrency accepts one to eight workers. Automatic mode uses up to two workers when Windows reports no seek penalty, and one when a seek penalty is present or unknown. It excludes requested paths and records reparse points without following them.

The index stores compact 32-bit node IDs, parent IDs and names. Full paths are reconstructed only when requested. Each insertion propagates its totals through existing ancestors, so partial directory totals are immediately consistent. Allocation deduplication uses volume identity and file identity. Logical totals retain each path name.

Queries return at most 500 entries. The frontend renders at most 80 table rows per virtual window. Overview requests up to 250 direct children, plus the eight largest direct entries beneath each of up to twelve visible directories. The descendant pass checks cancellation every 256 index entries. Unrepresented bytes appear in a smaller-items region linked to the complete virtual list.

Queries filter references into the in-memory index, partition at the requested offset and limit, then sort only the returned page. They retain exact deterministic ranking without sorting every match. Empty searches avoid allocating lowercase names, ASCII name comparisons allocate nothing, and Unicode names retain lowercase ordering. Root scope checks are constant time. Progress is emitted at initialization, the first insertion, every 100 ms and completion.

## Desktop adapter

Tauri exposes volume discovery, native folder selection, scanning, cancellation, bounded index and duplicate queries, details, file types, analysis issues, Explorer reveal, history, developer analysis and confirmed cleanup. Fixed Windows launch commands open Storage settings and the Recycle Bin. No arbitrary frontend filesystem API or shell command is exposed.

Scanning and queries run in background tasks. The engine publishes one shared index behind `Arc<RwLock<Index>>` before initial progress. Filesystem calls happen outside the write lock; inserting entries and updating ancestors uses a short write section. Queries hold a read guard and return bounded results. The interface refreshes measured partial results at most once per second, plus terminal progress, without polling. A second scan cannot start until the first releases the active operation slot.

## Interface

Svelte owns interaction state, preferences and bounded query results. A custom squarified treemap redraws when geometry, selection or hover changes. There is no persistent animation loop or idle polling. Pointer hierarchy navigation can schedule a finite 160 ms map transition; it cancels on new input, replacement, resize, reduced motion, hiding or unmounting. Layout uses Windows system fonts and local artwork.

## Duplicate analysis

`duplicates` filters unique sizes before content reads, samples the first and last 4 KiB for large files, and fully hashes matching samples with a reusable 64 KiB buffer. Small files are read once. Candidate identities exclude hard-link aliases. Windows content handles allow shared readers while excluding writers and deletion, and do not follow final reparse points. Cache stamps include identity, length, modification time and native change time. Hashing validates the opened handle again after reading. Cancellation is checked between candidates and buffer reads.

Duplicate results retain the exact shared scan index that produced their IDs. Groups and members use bounded pages. One active-operation slot prevents scanning and hashing from competing. The cache has a 250,000-identity cap and persists in snapshot database schema 5. Duplicate analysis lazily loads fixed-size identity, stamp and digest fields, then transactionally replaces the bounded table. No paths or file content are stored. It still opens the real file and validates its complete stamp before accepting a hit. Invalid rows are ignored before hashing; database failures become reported analysis issues and ordinary hashing remains available. Cancellation rolls back cache replacement. Cache memory belongs to the operation and is released afterward.

## Snapshot history

The bundled SQLite database uses WAL, foreign keys and transactional schema initialization. A root key combines the native root path, filesystem identity and exact exclusion coverage. Stable paths store native name bytes and parent IDs. Entry versions use half-open snapshot intervals; unchanged captures do not copy every entry. Snapshot deletion retains versions required by surviving captures, and compaction checkpoints the WAL and vacuums the database.

Snapshot schema 8 adds per-group retention preferences and transactional history/configuration revisions. A desktop review retains at most 100 exact oldest captures; its fingerprint binds their metadata, the group, preference and revision. Apply rechecks that fingerprint under an immediate transaction and the shared native operation lease, preserving the configured newest captures. Cancellation/failure rolls back the batch and its interval garbage collection. Preferences default to Keep all; saving one does not run retention. No CLI/MCP history-removal endpoint exists. These revisions detect application-managed changes, not privileged tampering with local state.

Schema version 4 adds per-capture developer-cache evidence referencing those stable paths. It stores tool/category labels only for indexed directories, including an indexed root beneath a known cache. Historical queries reconstruct cache scopes from saved node IDs and never resolve current tool settings. Index loading reads entries and cache evidence in one SQLite transaction. Live scan and NTFS replay indexes start without historical cache evidence.

Schema version 5 adds the independent bounded duplicate-hash table. Versions 0 through 4 migrate transactionally without rewriting stored snapshot entries. Older executables that support only schema 4 cannot reopen an upgraded database.

Loading reconstructs the Rust index and validates parent order, node limits, filename components and checked integer conversions. Snapshot comparisons page actual additions, omissions and changed directory totals. Incomplete coverage never confirms missing entries as deleted. Database work runs in blocking tasks. The active-operation gate prevents capture and loading from racing a scan or duplicate analysis. Capture/loading commands acknowledge their operation IDs through channels so History can use the existing owner-specific cancellation endpoint. Loading checks cancellation before its read transaction, between rows and before returning. Final publication holds the active-operation mutex while checking cancellation and swapping the loaded index, so publication completes before a cancellation acknowledgement or retains the previous analysis. Capture keeps its transactional pre-commit check and returns its committed ID on success.

History invalidates late channel acknowledgements by action generation and cancels owned work on unmount. Its Cancel control ends when the native result arrives, before frontend refresh or adoption. Removed-button focus returns to the original connected trigger when focus has not moved elsewhere. Cancellation remains cooperative: an already running SQLite call or connection-mutex wait must return before the worker finishes. Comparison, deletion and compaction do not use this capture/load control.

## NTFS scanning and refresh

Eligible volume-root scans try native NTFS enumeration before directory walking. Capability failures fall back before publishing non-root IDs. Incremental refresh checks the saved volume and journal checkpoint, constructs a new candidate index, and publishes only after final continuity checks. Invalid continuity or unsupported changes trigger resynchronization; cancellation keeps the previous index. Raw-volume acceptance on this host remains unverified. See [NTFS scope](NTFS.md).

## Cleanup

The backend owns a single-use plan tied to the live index. Preview validates exclusions, native identity and metadata; execution pins ancestors and files, and duplicate cleanup also retains a freshly verified copy. Advanced permanent removal commits per-item intent to a separate exclusive SQLite journal before native handle-based disposition. Result-write failure stops further work. On restart, in-flight items become unknown and are never retried automatically.

Default recycling is not implemented. Separate fixtures establish sealed-directory admission with elevation and Shell recycling/restoration with a retained content guard. They do not establish the combined transaction. The [recycling transaction design](RECYCLE-TRANSACTION.md) defines the remaining privilege, handoff and recovery boundaries; [cleanup evidence](CLEANUP.md) records what has actually run.

## Pending architecture work

Combined safe recycling and whole-application performance acceptance remain open. Large queries can delay index writes while holding their read guard; this is part of the large-workload performance gate. See [current acceptance](ACCEPTANCE.md) rather than treating historical entries in `NEXT_TODOS.md` as current module status. No production behavior is simulated for pending modules.

## Navigation during scan refresh

The overview queue normally keeps only the latest request. Explicit folder navigation now owns a short frontend refresh interval while its details and overview are pending. Scan-progress refreshes defer during that interval and coalesce into one refresh of the committed destination afterward. A stale navigation completion cannot release a newer owner, and obsolete navigation overview replies or errors cannot commit after another interaction. Interaction invalidation clears abandoned ownership.

This prevents a progress update for the old committed scope from superseding a pending folder transition. It adds no polling or visible copy. Native index replacement has its own ownership rule below. The retained frontend regression covers deferral, coalescing, stale completion and reset, while native pointer/keyboard acceptance remains manual. See [the source verification](../benchmarks/live-navigation-refresh-20261031.json).

The [rebuilt native preview](../benchmarks/native-live-navigation-preview-20261031-124936-267.json) has a responding Voldiq window and records the executable/frontend hashes. The previous owned preview was closed normally and its 12,699,648-byte executable removed. This metadata does not establish pointer/keyboard interaction acceptance.

## Index publication ownership

Each loaded `SharedIndex` has an opaque `IndexId` paired with its Arc and analysis date under the engine mutex. Updating the date for the same Arc preserves its identity. Replacing or clearing the publication invalidates that identity, including when a previously removed Arc is loaded again. Terminal scan events capture identity and date together before releasing operation ownership.

Every node- or duplicate-group-scoped IPC request carries the identity of its originating analysis. The native adapter validates it before interpreting local IDs. Cancellable reads also check the captured Arc before returning. Cleanup execution retains its existing one-use plan and exact-Arc check. Snapshot database IDs and cleanup journal receipt IDs have independent ownership.

The frontend captures identity when scheduling queries and discards late management replies after replacement. Adopting a new publication clears selection, details and folder scope, cancels obsolete queries and remounts index-bound views. An unscanned cleanup rule may request static metadata with no identity; it cannot resolve indexed files that way.

The retained native case scans two real folders whose local node IDs overlap and proves that the first identity cannot resolve the second index. The frontend case verifies a delayed IPC reply against an injected transport. Both counterexample mutations fail their retained case, and the restored source passes the [complete source gate](../benchmarks/project-verification-index-ownership-20261031.json). These checks add no declared tests or native interaction acceptance. See [ownership verification](../benchmarks/index-ownership-20261031.json).

The [current native preview](../benchmarks/native-index-ownership-preview-20261031.json) opens a responding Voldiq window from the rebuilt executable. Its receipt records the executable and frontend hashes. The previous owned preview closed normally and its 12,699,648-byte executable was removed. No visual or pointer/keyboard acceptance is inferred from this metadata.

## Standalone interface loading

Optimized native binaries must enable Tauri's production protocol. The project `production` feature forwards `tauri/custom-protocol`, and a compile-time guard rejects a non-test optimized binary in Tauri dev mode. The Tauri CLI production build also enables the underlying protocol. A direct Cargo `--release` flag alone is insufficient.

The `--verify-embedded-ui` diagnostic constructs the compiled context and exits before application setup. It reports decoded runtime asset lengths and SHA-256 digests. Preview preparation and distribution verification run it and match JavaScript and CSS to current `dist/assets`. Raw storage iterators may contain compressed data; the diagnostic obtains content with `Assets::get`, the runtime decoding path. The source gate builds frontend assets before checking native production mode. This establishes compiled payload and mode, while actual desktop interaction and clean-machine installation remain separate.
