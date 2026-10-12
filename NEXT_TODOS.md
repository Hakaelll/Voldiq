# Implementation ledger

The core now has a durable cleanup-operation journal with schema/application identification, FULL-sync transactions, exclusive ownership and exact native identity/stamp storage. Session-bound operation tokens and guarded per-item transitions reject repeat work. Restart marks unresolved attempts unknown and unstarted items cancelled, never retrying deletion. Four real SQLite/owned Windows fixture tests cover actual deletion followed by failed result persistence, replacement preservation, mixed outcomes, intent-write failure, ownership, rollback and incompatible databases; strict Clippy passes. Application initialization, confirmation/executor wiring, result history and index refresh remain pending. See `docs/CLEANUP.md`.

A handle-bound permanent-disposition primitive now consumes verified leaf/ancestor authority and rechecks the full stamp before a POSIX deletion request. It never invokes recycling or a path-based fallback and rejects read-only files. Four controlled Windows/NTFS fixture tests cover hard links, existing readers, substitution, read-only targets and post-acquisition metadata changes; nine existing cleanup/rule/duplicate-selection tests pass. This is not exposed through IPC/UI. Explicit advanced-action confirmation, durable intent/result logging, interruption recovery, keep-copy authority and index refresh are still required. Default recycling remains unresolved. See `docs/CLEANUP.md`.

The manually selected old-file rule now lists complete regular files unchanged for at least 90 days in the live indexed scope. A minimal Cleanup selector switches between it and user temporary files; changing rules clears selection and invalidates pending query results. Native protected Windows scopes are omitted and immutable preview revalidates scanned authority. Three real-filesystem cleanup-rule tests, Svelte diagnostics, production build and strict workspace Clippy pass. Age is not proof of disuse and no deletion executes. Safe recycling, official cache cleanup and result/recovery logging remain pending. See `docs/CLEANUP.md`.

Cleanup now includes an independent read-only Windows Recycle Bin summary for the explicitly selected accessible local drive. Three native tests pass, including the actual `C:\` query, invalid scopes and numeric boundaries. Svelte diagnostics, frontend production build and strict workspace Clippy pass. Unavailable totals remain distinct from zero; selected-drive changes discard stale results. Populated-bin acceptance, enumeration, emptying/restoration and safe recycling execution remain pending. See `docs/CLEANUP.md`.

Native cleanup verification now has controlled post-acquisition junction mutation coverage. An empty pinned directory can become a junction, but relative acquisition fails closed with Win32 1921; a populated parent with its leaf pinned refuses conversion with 145. Original and substitute contents survive. Four native helper tests and strict workspace Clippy pass. Tests are separated from production code; wider filesystem/filter behavior and the actual recycling/recovery transaction remain pending. See `docs/CLEANUP.md`.

Cleanup verification now opens the filesystem root once and acquires each ancestor and leaf relative to the previously verified native directory handle. Open-only, no-follow and no-recall policy is preserved, with single-component/ADS validation and owned handle lifetimes. Two native helper tests and eight cleanup/rule/duplicate integration tests pass, including directory-name substitution retaining original authority and a replaced junction parent being refused. Strict workspace Clippy passes. Concurrent ancestor metadata changes and the recycling/recovery transaction remain pending; see `docs/CLEANUP.md`.

A controlled Shell file-ID probe now reproduces namespace substitution on Windows 10.0.26200.0/NTFS. Despite `IFileSystemBindData2` getters being called, both an existing Shell item and a newly parsed item with the original ID read replacement content; a missing original name returns file-not-found. The retained original remains unchanged. This rules out using parsed identity metadata as immutable stream authority, without claiming a result for `IFileOperation` deletion. Recycling handoff, durable recovery and execution remain pending. See `docs/CLEANUP.md` and `benchmarks/shell-file-id-probe.ps1`.

Docker storage now has an independent read-only local Engine API panel with version negotiation, four category summaries, persistent-volume labels, unknown-size preservation, shared-cache accounting, bounded HTTP transport and a 15-second deadline. Seven native tests pass, including a real Windows named-pipe fixture and timeout. Svelte checks, production build and strict workspace Clippy pass. The default Docker pipe is unavailable locally, so live Engine acceptance remains pending. Context-file discovery and management actions are not implemented. See `docs/DEVELOPER.md`.

Voldiq is under active development. A task becomes DONE only after its acceptance checks pass. The supplied specification is preserved in `docs/reference/requirements.md`.

| ID | Task | Priority | Status | Dependencies | Acceptance criteria | Related files |
|---|---|---|---|---|---|---|
| V01 | Windows app and build foundation | P0 | DONE | None | Svelte checks, Rust build, real Windows window | src-tauri, frontend |
| V02 | Native volumes and conventional scan | P0 | IN_PROGRESS | V01 | Real fixtures, cancellation, errors, links, aggregation, bounded queries, partial results, calibrated concurrency | crates/voldiq-core |
| V03 | NTFS enumeration and metadata | P0 | IN_PROGRESS | V02 | Validated binary records, hierarchy, metadata, hard-link names, fallback, volume comparison benchmark | crates/voldiq-core/src/ntfs.rs |
| V04 | Visual explorer and search | P0 | IN_PROGRESS | V02 | Canvas navigation, virtual rows, real IPC data, keyboard navigation | frontend |
| V05 | Large files and duplicates | P1 | IN_PROGRESS | V02 | Filters, BLAKE3 content verification, cancellation, hard-link handling, reviewed selections, native interaction | crates/voldiq-core, frontend/components/Duplicates.svelte |
| V06 | Safe cleanup and developer storage | P0 | IN_PROGRESS | V05 | Preview, exclusions, identity validation, confirmation, controlled fixture execution | crates/voldiq-core |
| V07 | Persistent snapshots | P1 | IN_PROGRESS | V02 | SQLite transaction, restart, additions/deletions/growth comparison, native interaction | crates/voldiq-core, frontend/components/History.svelte |
| V08 | Incremental NTFS index | P1 | IN_PROGRESS | V03, V07 | Journal continuity validation and full resynchronization | crates/voldiq-core |
| V09 | Preferences and accessibility | P1 | IN_PROGRESS | V04 | Persistent preferences, themes, reduced motion, keyboard checks | frontend, src-tauri |
| V10 | Benchmarks and quality audits | P0 | IN_PROGRESS | V02-V09 | Reproducible measurements, Ponytail review and audit, resolved defects | benchmarks, docs |
| V11 | Windows distribution | P0 | IN_PROGRESS | V10 | Installer build, offline smoke check, final icon, documentation | src-tauri, README.md |

## Delivery policy

Commit verified functional units with English messages and push to the configured repository. Preserve pending work and measured limitations. Never claim that the full acceptance list has passed while items remain open.

## Windows installer delivery

The x64 release executable and NSIS installer build successfully with the offline WebView2 runtime embedded. Installer and uninstaller now use the supplied multi-resolution Voldiq icon. Generated NSIS source confirms current-user installation and offline runtime embedding; the downloaded runtime has a valid Microsoft Authenticode signature. The Node prerequisite matches the locked frontend tooling. See `docs/DISTRIBUTION.md` for artifact evidence and build instructions. Clean-machine offline installation, startup and uninstall remain open.

## First verified delivery, 2026-10-11

The native executable builds and starts a real window titled Voldiq. Six Rust filesystem tests, two treemap tests, Svelte diagnostics and workspace Clippy pass. The release scanner benchmark checks 10,000 generated files over three passes with no errors. Its observed scan range is 762.810-868.573 ms on this host; see `docs/PERFORMANCE.md` for methodology and limits.

The frontend includes a real command bridge, custom map, virtual explorer, search, large-file filters and persisted visual preferences. Browser verification covers initial and settings states; populated native interaction still needs verification. There are no duplicate, cleanup, snapshot or NTFS simulations.

Live partial queries now share one Rust index and update ancestor totals on insertion. A seventh Rust test checks live reads, partial totals and cancellation. Visible UI copy was reduced at the user's request. The updated benchmark is slower; its measured values and uncertainty are recorded in `docs/PERFORMANCE.md`.

Next: validate native IPC on controlled fixtures, calibrate scanner concurrency, and implement NTFS enumeration with metadata acquisition and explicit fallback. Duplicate analysis, cleanup, snapshots, incremental indexing and distribution remain open.

## Duplicate analysis delivery

The real BLAKE3 engine filters unique sizes, samples at most 8 KiB, verifies complete content, reuses stamps validated with file identity, modification time and native change time, and reports locked or vanished files. Five tests cover matching samples with different middle content, cache reuse and invalidation, hard links, unique sizes, sampling, cancellation and locked files. Duplicate groups and members cross IPC in bounded pages. Duplicate results retain their original index to avoid reinterpreting IDs after a rescan.

The duplicate view uses minimal labels and measured data. Management selection, confirmation and recycling remain part of the safe-cleanup work; no deletion command is exposed yet. Native populated interactions and cross-volume fixtures remain open.

## Snapshot history delivery

SQLite stores native paths and version intervals, with transactional captures and checked integers. Five tests cover restart comparisons, changed directory totals, unchanged entry reuse, rollback, capture deletion, compaction, exclusions and native UTF-16 names. Incomplete coverage reports unobserved entries rather than confirmed deletions. The history component exposes bounded lists, saved-index loading, comparisons and storage management, using minimal copy. Native populated interaction and large-history measurements remain open.

## Ranked query delivery

Queries partition matching references at the requested offset and limit and sort only the page. ASCII name comparison avoids temporary strings, empty searches avoid lowercase allocation, and root scope checks are constant time. Two real-filesystem tests check complete deterministic paging, filters, caps and Unicode sorting. A clearly labelled synthetic million-file benchmark records baseline and final rankings in `docs/PERFORMANCE.md`. It does not close the actual million-file scan, total-memory or native UI gates.

## NTFS reader foundation

The Windows reader opens an existing NTFS volume read-only, queries its journal, enumerates bounded MFT records, opens metadata by file ID and reads changes only with verified journal continuity. The binary parser checks record versions, lengths, alignment, dynamic filename offsets and component lengths without unsafe pointer casts. Four tests cover native UTF-16, every truncation of a fixture record, malformed offsets, minor-version extensions and journal reset/gap/volume checks. Limited and cancelled results cannot carry a completed checkpoint.

The host probe returned access denied for the raw NTFS volume. Its conventional two-hard-link fixture passed. No elevation or journal modification was attempted. Integration of NTFS records into the live index, complete hard-link name enumeration, persistent journal checkpoints and elevated-drive comparison remain open.

Native alias enumeration and final-path helpers now pass a real Unicode long-path fixture. Duplicate hashing excludes offline and recall-marked content; a Windows offline-attribute fixture verifies zero bytes read. All 26 core tests and strict workspace Clippy pass. Index integration remains pending.

## NTFS index integration

Drive-root scans now attempt the read-only NTFS path; unsupported capability and bounded enumeration fall back before non-root IDs are published. Native metadata populates parent-first directories and all hard-link aliases. Five integration tests cover real size parity, an excluded primary hard-link name, unique allocation, changed identities, unresolved parents, malformed names, limits, cancellation and bounded volume fallback. Raw enumeration on an elevated drive, journal replay, checkpoint persistence and volume performance comparison remain open. The source reviewer could not run because its account usage limit was reached; source review and functional checks were performed directly.

## NTFS checkpoint persistence

Stable, complete NTFS scans now retain their volume serial, journal ID and next USN. SQLite schema version 2 binds checkpoints to captures in the capture transaction and cascades their deletion. Three database tests cover restart, rollback, full-width IDs, invalid and tampered evidence, incomplete or historical inputs, deletion and migration from version 1. Historical loading does not promote checkpoint evidence into current-index authority. Journal replay, current metadata reconstruction, continuity-loss resynchronization and real raw-volume acceptance remain open.

## Live NTFS replay

The native scan command now attempts journal refresh for an eligible in-memory volume index with matching scope and exclusions. Changed identities provide fresh metadata and current aliases; unchanged files are retained without reopening them. Candidate rows and totals are published only after final journal/root verification. Four real-filesystem tests cover growth, deletion, renames, moves, new files, subtree deletion, hard-link allocation after exclusion changes, cancellation, reset, row limits, final verification and conventional rescan fallback. Newly included directories require full resynchronization. Raw-volume acceptance, cross-restart identity persistence and replay memory/performance measurements remain open.

## Persisted NTFS resume

Schema version 3 records native entry identity/change-time evidence in version rows. The first volume scan with an empty engine attempts a capture matching native root identity and exclusions, restores evidence privately and returns current rows only after verified replay. Old databases migrate transactionally; absent legacy stamps require a full rescan. New fixtures verify SQLite restart replay with actual metadata, cancellation, bounded loading, historical isolation, malformed root/parent rejection, volume tampering, version 2 fallback, unchanged migration comparisons and matching-size/time replacement history. Native raw-journal restart acceptance and large-index measurements remain open.

## Developer storage analysis

A real-filesystem test verifies manifest-based Cargo, Node.js and .NET detection, Python caches, nested artifact deduplication and deterministic paging. The native command exposes bounded indexed results. A compact developer view reports categories, sizes and partial coverage, with details and paging. Svelte diagnostics, frontend production build and strict workspace Clippy pass. No cleanup execution is exposed yet; plans, confirmations, guarded recycling, official tool integrations and cache discovery remain open.

Global Cargo and npm cache analysis now resolves documented current-user locations and absolute environment overrides, then classifies indexed directories without reading cache contents. Cargo installed tools and credentials are excluded. Scoped cache descendants retain measured subtree coverage; historical indexes omit live global roots. All three developer integration tests, the native configuration test, formatting and strict workspace Clippy pass. Source-only review verified both scope and historical fixes. Other tool caches, persisted cache-location evidence, Docker analysis and cleanup execution remain open; see `docs/DEVELOPER.md`.

Global analysis now includes pip, Gradle and NuGet. NuGet packages are Dependencies; HTTP and plugin locations are Cache. Gradle selects only its caches directory. Most-specific matching roots preserve scoped categories when configurations overlap, and coincident dependency locations take precedence. Four core integration tests, two native tests, formatting and strict workspace Clippy pass; source-only review confirmed the overlap fix. pnpm store discovery, persisted location evidence, Docker analysis and cleanup execution remain pending.

pnpm analysis now resolves conventional versioned stores and the supported absolute environment override. Files/index directories are Cache; global shared links are Dependencies, with no outer candidate hiding the distinction. A real-scanned native fixture verifies precedence, the pnpm 11 suffix, already-versioned paths, missing/relative configuration and unrelated sibling exclusion. Three native tests and strict workspace Clippy pass. Configuration-file/per-disk discovery, custom virtual-store paths, standalone bookkeeping classification, Docker analysis and cleanup execution remain pending.

## Manual management previews

The first cleanup rule now discovers and pages indexed user `.tmp`/`.temp` files aged seven days or more. Two real-filesystem tests cover exact totals, age, extension, paging, explicit selection, changed files, historical rejection, exclusions, unscanned locations and partial subtree coverage. Its descriptor records the full rule contract. The Cleanup view supports scanning the actual temporary location and manual preview. Recycling and unused-file execution remain pending.

Native recycling handoff research now includes an isolated Windows capability probe. Atomic sealed staging prevents new children, but the created handle lacks the requested DACL capability and native enrollment is denied on this host. See `docs/CLEANUP.md`; source-handle release followed by path deletion remains rejected. The updated 10,000-file scanner fixture verifies exact totals, zero errors and the 112-byte entry layout; timings are recorded in `docs/PERFORMANCE.md`.

Duplicate review now rejects complete-group selections, verifies selected content and a retained copy with fresh BLAKE3 reads, exposes cancellation and records retained paths in the preview. A real-filesystem test covers successful retention, all-copy rejection, unknown/duplicate IDs, cancellation, digest mismatch and a changed retained file. Execution-time retention and recycling remain pending.

The explorer and large-file list now support explicit file selection and a backend-owned read-only preview. Scans retain file identity/change time; native known-folder APIs resolve protected locations. Four cleanup tests verify real sizes, replacement before preview with matching timestamps, changed identity after preview, exclusions, historical rejection, hard-link estimates, native protected paths and handles that block mutation while held. All 36 core tests, Svelte diagnostics, production frontend build and strict workspace Clippy pass. A source-only reviewer confirmed both accepted safety findings resolved. Recycling, duplicate keep-copy validation, cleanup rules, result logging and concurrency/transaction checks remain open. Entry storage is now 112 bytes on x64, and updated scanner performance requires measurement.
