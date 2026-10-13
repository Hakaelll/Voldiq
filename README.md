# Voldiq

<p align="center"><img src="frontend/public/wordmark.png" width="420" alt="Voldiq" /></p>
<p align="center">A local storage explorer for Windows, built with Rust and Tauri.</p>

Voldiq maps the files on a drive or in a folder so you can understand where space goes. Its Rust engine owns the filesystem index; a compact Svelte interface requests bounded pages and draws a custom Canvas treemap.

> **Development status:** Voldiq is under active construction. The current source includes real volume discovery, live conventional scanning, search, a virtual file list, a proportional treemap, BLAKE3 duplicate analysis and persistent SQLite snapshots. The complete product acceptance checklist remains open. See [NEXT_TODOS.md](NEXT_TODOS.md).

## Principles

- Local and offline. No account, cloud service, advertising or telemetry.
- Real measurements. No sample data presented as system information.
- Read-only analysis by default. Permanent file removal requires preview and explicit confirmation.
- Precise terminology. Logical file sizes differ from unique physical allocation and volume usage.
- Small, reusable engine. Heavy analysis runs in Rust, outside the interface thread.

## Build on Windows

Install the stable Rust MSVC toolchain, Visual Studio C++ Build Tools with the Windows SDK, Node.js 22 (version 22.12 or later) or Node.js 24+, pnpm, and Microsoft Edge WebView2. Windows 10 and 11 x86-64 are the initial targets.

```powershell
pnpm install
pnpm tauri dev
```

The browser-only preview, `pnpm dev`, intentionally cannot scan your computer. Run the native app for filesystem access.

```powershell
pnpm check
pnpm test
cargo test -p voldiq-core
cargo clippy --workspace --all-targets -- -D warnings
pnpm tauri build
```

The release configuration targets an NSIS installer and includes the offline WebView2 installer. Build output is in `target/release/bundle/nsis`; run `./scripts/verify_distribution.ps1` on Windows after building to check artifact metadata and generated packaging configuration. A distributable download is not published yet. See [Windows distribution](docs/DISTRIBUTION.md) for build evidence and remaining installer acceptance checks.

## Source map

| Location | Responsibility |
|---|---|
| `crates/voldiq-core` | Scanning, Windows metadata, index queries, duplicate analysis and snapshot storage |
| `src-tauri` | Native window and a narrow command bridge |
| `frontend` | Svelte interface, virtual table, custom treemap and CSS tokens |
| `assets` | Original supplied logo and interface references |
| `docs/reference` | Complete specification and supplied design inspiration |
| `scripts/prepare_assets.py` | Reproducible PNG and multi-resolution ICO preparation |

## Scan accuracy

Drive-root scans attempt read-only NTFS enumeration when current permissions allow it. Opened metadata handles provide actual sizes; USN records provide names and parents. All hard-link aliases are resolved, and journal changes during construction mark coverage incomplete. Capability and enumeration-limit failures fall back before non-root IDs are published. Folder scans use iterative directory enumeration. See [NTFS integration and verification limits](docs/NTFS.md).

Settings offers **Automatic** and **Directory enumeration** scan modes. Automatic keeps NTFS enumeration and verified journal refresh/resume where available. Directory enumeration always creates a fresh conventional index, bypassing NTFS and saved journal checkpoints. Both modes retain exclusions, cancellation, entry limits and measured progress; the choice applies to the next scan and is saved locally.

The saved **Concurrency limit** controls 1–8 simultaneous metadata workers in directory enumeration. Its default of 1 limits background interference. Higher limits use bounded queues and preserve result ordering, allocation deduplication and cancellation; NTFS and journal paths retain their existing execution model. See [concurrency measurements and their limits](docs/PERFORMANCE.md#bounded-scan-concurrency).

Both paths tolerate inaccessible files, report errors and support cancellation. They record reparse points without following them, which prevents junction loops and avoids reading cloud-only contents.

**Ctrl+K** opens Explorer at the scan root, clears extension/date filters and focuses the search text. It searches the loaded index, including a historical capture, without scanning again. Typing in Overview's search field opens the matching file list for its current scope. The shortcut is disabled during confirmation dialogs and operations that disable navigation.

Changing a query or closing the file list cancels obsolete filtering and ranking work. Each query family keeps one running request and only its latest pending request; the overview and file list remain independent. Reloading the frontend creates a native query session that invalidates older requests. Cancellation is cooperative and does not return partial query pages.

The file list supports arrow keys, Home, End, Page Up and Page Down from a focused filename. Enter opens the item; Space selects it. Distant keyboard targets fetch a bounded page before receiving focus. Large indices use a bounded scroll range while retaining normal row spacing and wheel steps.

Selecting a file type searches the current folder's indexed subtree, matching the overview totals. **No extension** matches regular files such as `README` and `.gitignore`; folders remain excluded. The same filter is available in Explorer and Large files and clears with Reset filters or Ctrl+K.

Logical size includes each visible hard-link name. Unique allocated size counts a file identity once across the scan. Allocation within an individual folder therefore reflects the link encountered first. Windows volume usage includes metadata and inaccessible or unindexed content; it should not equal the logical total.

Progress reports actual file counts and bytes. Partial results can be explored while scanning; cancellation retains the measured index. No estimated percentage appears when the total is unknown. The initial index has a five-million-entry limit. Reaching the limit yields explicitly incomplete results.

The status strip shows the native analysis date for completed scans and retained partial results. A cancelled refresh that keeps previous results keeps their original date. Clearing the analysis removes both the index and its date. Historical views show their separate snapshot capture date.

Five scans of a real million-file fixture verified exact counts and sizes with zero errors. See [performance measurements](docs/PERFORMANCE.md#large-real-filesystem-fixture) for raw results, reproduction commands and the limits of this warm-cache scanner baseline.

The current release also has a [native/WebView2 resource observation](docs/PERFORMANCE.md#native-application-process-observation), with per-process memory and CPU counters. Its scope and remaining idle/rendering checks are documented separately from scanner benchmarks.

## Duplicate analysis

Duplicate discovery groups indexed files by size, samples at most 8 KiB and verifies matching content with complete BLAKE3 hashes. Locked, changed and vanished files produce reported issues. Hard links to the same file identity are represented once. Repeated analysis reuses hashes only after validating file identity, size, modification time and native change time.

Potential allocation conservatively retains the largest independently allocated copy. Files with multiple hard links contribute no reclaimable estimate, because links may exist outside the scan. This estimate does not imply that moving files to the Recycle Bin frees volume space. Recycling is still pending.

## File management

Cleanup lists indexed `.tmp` and `.temp` files at least seven days old in the Windows user temporary directory. Select files manually, then review their actual paths and sizes. Unscanned and partial coverage remain visible.

Choose **Project temporary files** to review the same age and extension criteria within indexed Cargo, Node.js, CMake and .NET build artifacts. Project manifests must be present and complete in the scan. Recognized dependency trees and configured global caches remain excluded, including those nested inside build output. Source files and unrelated directories are omitted; files still require explicit selection and a freshly verified preview.

Choose **Old files** to review regular files unchanged for at least 90 days within the current live scan. Selection is manual; age alone does not establish that a file is unused. Protected Windows locations and configured exclusions are omitted, and preview checks current identities and metadata. Historical captures cannot produce management plans.

The Recycle Bin panel queries Windows for the selected accessible local drive's current item count and size, independently of the scan. Refresh updates the read-only summary. Unavailable results remain distinct from an empty bin; size is not an estimate of physical space recovered.

Duplicate selections retain a copy from each group and reread full BLAKE3 content before showing selected and retained paths. The review is cancellable. Recycling remains under development.

Select regular files in Explorer or Large files and use Review selection to inspect their real paths, sizes and potential allocation. Rust retains immutable plans and checks scanned identities, change times, exclusions and native Windows protected locations. Historical captures cannot produce management previews.

In Large files, **Items** switches between files and folders. Folder rankings use indexed subtree logical sizes and the existing scope, search, size and date filters. Open a folder to narrow the scope or inspect its details. Folder totals can overlap through ancestry; rows are rankings rather than an additive space-recovery estimate. Management selection continues to accept regular files only.

The advanced **Permanently delete…** action requires typing `DELETE` and confirming the irreversible operation. Each file is verified through retained native handles, its intent is committed before removal, and its result is recorded afterward. Duplicate selections also pin and freshly hash one preserved copy per group throughout execution, then freshly hash every selected file before removal. Results distinguish removal, failure, cancellation and unrecorded outcomes. Closing the result triggers a fresh analysis. This action does not use the Recycle Bin and has no undo; interrupted work is never retried automatically. Recycling is still pending. See [cleanup guarantees and remaining work](docs/CLEANUP.md).

Cleanup activity lists saved per-file results and operation start times, including interrupted outcomes after restart. It is read-only and paginated; unknown results never imply successful removal or trigger retries.

## Preferences

Settings persist the default drive, excluded paths, theme, size metric, reduced motion and navigation width locally. The default drive applies at startup and stays independent of the current drive selection. **Recent** offers the ten most recently completed scan paths for an explicit new analysis; failed and cancelled scans are omitted. Clear recent paths in Settings to remove that list. Stored paths never start a scan automatically or carry cleanup authority.

**Clear current analysis** releases the loaded index, duplicate results, cached hashes and pending cleanup previews. Files, saved snapshots, cleanup activity and preferences remain available. Active operations must finish or be cancelled first. A new scan or explicitly loaded snapshot is required to explore an index again.

## Developer storage

The developer view classifies indexed Cargo targets, Node.js dependencies and build output, Python bytecode caches, Gradle project caches and .NET build directories. It recognizes current-user Cargo, npm, pnpm, pip, Gradle and NuGet locations, including supported absolute environment overrides. npm discovery also reads the supported user and explicitly selected global `.npmrc` cache settings. NuGet installed packages and pnpm shared links are labelled Dependencies, separate from their caches. Project manifests identify ambiguous names such as `target`, `bin` and `build`. Nested candidates belong to their outer artifact so category totals do not count the same subtree twice. Results use bounded pages and expose logical size, allocation and incomplete coverage.

Docker analysis queries the local Engine API independently of filesystem scans. It reports images, containers, persistent volumes and build cache, preserving unknown sizes and shared-layer accounting. Management actions and additional configuration discovery remain pending. See [developer analysis and configuration limits](docs/DEVELOPER.md).

## Snapshot history

Snapshots remain local in the application's data directory. SQLite stores a baseline and new versions of changed entries, so unchanged captures reuse existing entry data. Captures use transactions; cancellation and integer overflow roll back the entire write. Original Windows UTF-16 names survive persistence.

Complete, stable NTFS scans can retain journal checkpoints with their captures. Repeating the same volume scan can apply journal changes to the live index. After restart, the first volume scan can resume from a matching capture with native identities. Both paths rescan fully when continuity or scope cannot be verified. Existing history databases migrate transactionally, and ordinary snapshot loading remains historical. Raw-volume and native restart acceptance checks remain open.

Saved captures can be reopened and compared after restarting. Comparisons require the same root identity and exclusions. Partial coverage marks missing entries as unobserved rather than claiming they were deleted. The history view includes recorded size, paged changes, capture removal and database compaction. Opening a capture labels the explorer as historical; rescan before saving a new capture.

## Project documentation

[Implementation ledger](NEXT_TODOS.md) · [Product brief](PRODUCT.md) · [Requirements](docs/reference/requirements.md) · [Interface reference](docs/reference/design-inspiration.md)

## License

Original project code is licensed under the [MIT license](LICENSE). Dependencies retain their respective licenses. Supplied artwork remains attributed to its source in the asset metadata.
