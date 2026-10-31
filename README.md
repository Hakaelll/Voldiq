# Voldiq

<p align="center"><img src="frontend/public/wordmark.png" width="420" alt="Voldiq" /></p>
<p align="center">A local storage explorer for Windows, built with Rust and Tauri.</p>

Voldiq maps the files on a drive or in a folder so you can understand where space goes. Its Rust engine owns the filesystem index; a compact Svelte interface requests bounded pages and draws a custom Canvas treemap.

> **Development status:** Voldiq is under active construction. The current source includes real volume discovery, live conventional scanning, search, a virtual file list, a proportional treemap, BLAKE3 duplicate analysis and persistent SQLite snapshots. The complete product acceptance checklist remains open. See [NEXT_TODOS.md](NEXT_TODOS.md).

The [product acceptance record](docs/ACCEPTANCE.md) tracks the original requirements, available evidence and remaining Windows verification. Run `./scripts/verify_project.ps1` for the complete automated source gate.

## Principles

- Local and offline. No account, cloud service, advertising or telemetry.
- Real measurements. No sample data presented as system information.
- Read-only analysis by default. Permanent file removal requires preview and explicit confirmation.
- Precise terminology. Logical file sizes differ from unique physical allocation and volume usage.
- Small, reusable engine. Heavy analysis runs in Rust, outside the interface thread.

## Build on Windows

Install the stable Rust MSVC toolchain, Visual Studio C++ Build Tools with the Windows SDK, Node.js 22 (version 22.12 or later) or Node.js 24+, pnpm, and Microsoft Edge WebView2. Windows 10 and 11 x86-64 are the initial targets.

```powershell
pnpm install --frozen-lockfile
cargo fetch --locked
pnpm tauri dev
```

The browser-only preview, `pnpm dev`, intentionally cannot scan your computer. Run the native app for filesystem access.

The complete source gate reuses release artifacts and offline Cargo dependencies by default. Development builds use limited debug information and disable incremental compilation to reduce disk cache growth. Build artifacts still accumulate under `target`; remove `target/debug` only after development builds and the development app have stopped. Keep `target/release` when retaining a built app or installer.

```powershell
pnpm check
pnpm test
cargo test --offline --release --workspace --features voldiq/production
cargo clippy --offline --release --workspace --all-targets --features voldiq/production -- -D warnings
pnpm tauri build
```

The automated suite retains 8 focused tests, reduced from 178 on 2026-10-21. See [Testing](docs/TESTING.md) for coverage and manual checks.

Build the desktop release with `pnpm tauri build`. A direct Cargo build must include `--features production`; `--release` alone changes optimization but does not select the embedded Tauri interface. The application refuses that invalid release mode. Preview and distribution verification also probe the actual executable and match its embedded JavaScript and CSS to the current frontend build.

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

The saved **Concurrency limit** offers Automatic or 1–8 metadata workers in directory enumeration. New UI preferences use Automatic: one worker for devices reporting seek penalties or unavailable information, and at most two for a local fixed device reporting no seek penalty, bounded by available processors. Existing manual preferences remain unchanged; omitted native options still use one worker. Bounded queues preserve ordering, allocation deduplication and cancellation. NTFS and journal paths retain their existing execution model. See [concurrency measurements and their limits](docs/PERFORMANCE.md#bounded-scan-concurrency).

Both paths tolerate inaccessible files, report errors and support cancellation. They record reparse points without following them, which prevents junction loops and avoids reading cloud-only contents.

**Ctrl+K** opens Explorer at the scan root, clears extension/date filters and focuses the search text. It searches the loaded index, including a historical capture, without scanning again. Typing in Overview's search field opens the matching file list for its current scope. The shortcut is disabled during confirmation dialogs and operations that disable navigation.

Changing a query or closing the file list cancels obsolete filtering and ranking work. Each query family keeps one running request and only its latest pending request; the overview and file list remain independent. Reloading the frontend creates a native query session that invalidates older requests. Cancellation is cooperative and does not return partial query pages. Indexed requests also carry their originating analysis identity. Loading another analysis clears the previous selection and rejects late replies, even when both analyses reuse the same local file IDs.

The file list supports arrow keys, Home, End, Page Up and Page Down from a focused filename. Enter opens the item; Space selects it. Select the Name, Logical size or Modified header to order the indexed results; a chevron marks the active order. Distant keyboard targets fetch a bounded page before receiving focus. Large indices use a bounded scroll range while retaining normal row spacing and wheel steps.

Selecting a file type searches the current folder's indexed subtree, matching the overview totals. **No extension** matches regular files such as `README` and `.gitignore`; folders remain excluded. The same filter is available in Explorer and Large files and clears with Reset filters or Ctrl+K.

The file-type bar shows the eight largest groups and **Other**, using measured logical bytes from all regular files in the current scope. Other includes groups outside the displayed list and reports their combined file count.

Logical size includes each visible hard-link name. Unique allocated size counts a file identity once across the scan. Allocation within an individual folder therefore reflects the link encountered first. Windows volume usage includes metadata and inaccessible or unindexed content; it should not equal the logical total.

Progress reports actual file counts and bytes. Partial results can be explored while scanning; cancellation retains the measured index. No estimated percentage appears when the total is unknown. The initial index has a five-million-entry limit. Reaching the limit yields explicitly incomplete results.

The status strip shows the native analysis date for completed scans and retained partial results. A cancelled refresh that keeps previous results keeps their original date. Clearing the analysis removes both the index and its date. Historical views show their separate snapshot capture date.

Five scans of a real million-file fixture verified exact counts and sizes with zero errors. See [performance measurements](docs/PERFORMANCE.md#large-real-filesystem-fixture) for raw results, reproduction commands and the limits of this warm-cache scanner baseline.

The [current standalone resource observation](docs/PERFORMANCE.md#verified-production-window-observation-2026-10-31) includes production-mode verification and independently checked native/WebView2 counters. Controlled idle/loaded workloads, startup and frame-rate acceptance remain open. The earlier development-protocol capture remains withdrawn as an interface baseline. Scanner benchmarks remain separate evidence.

## Duplicate analysis

Duplicate discovery groups indexed files by size, samples at most 8 KiB and verifies matching content with complete BLAKE3 hashes. Locked, changed and vanished files produce reported issues. Hard links to the same file identity are represented once. Repeated analysis reuses hashes only after validating file identity, size, modification time and native change time. A bounded SQLite cache preserves those hashes across restarts. The cache loads when duplicate analysis starts and stores no file paths or content.

Potential allocation conservatively retains the largest independently allocated copy. Files with multiple hard links contribute no reclaimable estimate, because links may exist outside the scan. This estimate does not imply that moving files to the Recycle Bin frees volume space. Recycling is still pending.

## File management

Cleanup's **Windows storage** button opens the system's Storage settings for Windows-managed cleanup. Windows controls the operation and its confirmations; rescan afterward to refresh Voldiq's results.

Cleanup lists indexed `.tmp` and `.temp` files at least seven days old in the Windows user temporary directory. Select files manually, then review their actual paths and sizes. Unscanned and partial coverage remain visible.

Choose **Project temporary files** to review the same age and extension criteria within indexed Cargo, Node.js, CMake and .NET build artifacts. Project manifests must be present and complete in the scan. Recognized dependency trees and configured global caches remain excluded, including those nested inside build output. Source files and unrelated directories are omitted; files still require explicit selection and a freshly verified preview.

Choose **Old files** to review regular files unchanged for at least 90 days within the current live scan. Selection is manual; age alone does not establish that a file is unused. Protected Windows locations and configured exclusions are omitted, and preview checks current identities and metadata. Historical captures cannot produce management plans.

Choose **Browser HTTP cache** to review external Blockfile files unchanged for at least seven days in standard Chrome and Edge profiles. Close the browser before review; cached pages may need downloading again. Cookies, sessions, shared blocks, cache indexes, Simple Cache streams and custom profiles are excluded. Selection and the advanced permanent confirmation remain explicit.

The Recycle Bin panel queries Windows for the selected accessible local drive's current item count and size, independently of the scan. Refresh updates the read-only summary. **Open in Windows** opens the full Windows Recycle Bin, where Windows controls restore and empty-bin actions. Unavailable results remain distinct from an empty bin; size is not an estimate of physical space recovered.

Duplicate selections retain a copy from each group and reread full BLAKE3 content before showing selected and retained paths. The review is cancellable. Recycling remains under development.

Select regular files in Explorer or Large files and use Review selection to inspect their real paths, sizes and potential allocation. Rust retains immutable plans and checks scanned identities, change times, exclusions and native Windows protected locations. Historical captures cannot produce management previews.

In Large files, **Items** switches between files and folders. Folder rankings use indexed subtree logical sizes and the existing scope, search, size and date filters. Open a folder to narrow the scope or inspect its details. Folder totals can overlap through ancestry; rows are rankings rather than an additive space-recovery estimate. Management selection continues to accept regular files only.

The advanced **Permanently delete…** action requires typing `DELETE` and confirming the irreversible operation. Each file is verified through retained native handles, its intent is committed before removal, and its result is recorded afterward. Duplicate selections also pin and freshly hash one preserved copy per group throughout execution, then freshly hash every selected file before removal. Results distinguish removal, failure, cancellation and unrecorded outcomes. Closing the result triggers a fresh analysis. This action does not use the Recycle Bin and has no undo; interrupted work is never retried automatically. The default **Move to Recycle Bin** action remains unavailable pending its combined native verification. See [cleanup guarantees and remaining work](docs/CLEANUP.md).

Cleanup activity lists saved per-file results and operation start times, including interrupted outcomes after restart. Compatible recycled receipts offer **Restore**. Review the verified original location and confirm before moving a file back. Existing files are never overwritten. Unknown results remain unresolved and cannot be retried through this action. Reviews expire after two minutes; prepare a new review if needed.

## Preferences

Settings persist the default drive, excluded paths, theme, size metric and reduced motion locally. The default drive applies at startup and stays independent of the current drive selection. **Recent** offers the ten most recently completed scan paths for an explicit new analysis; failed and cancelled scans are omitted. Clear recent paths in Settings to remove that list. Stored paths never start a scan automatically or carry cleanup authority.

**Clear current analysis** releases the loaded index, duplicate results, cached hashes and pending cleanup previews. Files, saved snapshots, cleanup activity and preferences remain available. Active operations must finish or be cancelled first. A new scan or explicitly loaded snapshot is required to explore an index again.

## Developer storage

The category chart shows measured logical bytes, percentages and directory counts. Select Dependencies, Build artifacts or Cache directly in its legend to filter the indexed list; select the same category again or All categories to reset. Global category totals remain visible while browsing a filtered page.

Filter indexed directories by Dependencies, Build artifacts or Cache, and order them by size or name. Filters apply before paging. The category bar retains totals for all classified directories; the list count follows the selected category.

The developer view classifies indexed Cargo targets, Node.js dependencies and build output, Python bytecode caches, Gradle project caches and .NET build directories. It recognizes current-user Cargo, npm, pnpm, pip, Gradle and NuGet locations, including supported absolute environment overrides. npm discovery reads supported `.npmrc` cache settings. pnpm discovery reads supported user-wide YAML settings for custom stores and metadata caches, with environment overrides taking precedence. pip discovery reads supported Windows `pip.ini` sources and combines configured caches for recognized commands, with native folder resolution and explicit/environment precedence. NuGet installed packages and pnpm shared links are labelled Dependencies, separate from their caches. Project manifests identify ambiguous names such as `target`, `bin` and `build`. Nested candidates belong to their outer artifact so category totals do not count the same subtree twice. Results use bounded pages and expose logical size, allocation and incomplete coverage.

Docker analysis queries the local Engine API independently of filesystem scans. It discovers supported local CLI contexts and reports images, containers, persistent volumes and build cache, preserving unknown sizes and shared-layer accounting. Management actions remain pending. See [developer analysis and configuration limits](docs/DEVELOPER.md).

## Snapshot history

Snapshots remain local in the application's data directory. SQLite stores a baseline and new versions of changed entries, so unchanged captures reuse existing entry data. Captures use transactions; cancellation and integer overflow roll back the entire write. Original Windows UTF-16 names survive persistence. History exposes Cancel while a native capture or load is running. Cancellation before publication keeps the previous analysis. Capture or load results that have already committed remain completed.

New captures also preserve the supported developer-cache classifications for their indexed directories. Opening history uses that saved classification and measured size, independently of current tool settings. Older captures keep their original evidence without inferred cache labels.

Complete, stable NTFS scans can retain journal checkpoints with their captures. Repeating the same volume scan can apply journal changes to the live index. After restart, the first volume scan can resume from a matching capture with native identities. Both paths rescan fully when continuity or scope cannot be verified. Existing history databases migrate transactionally, and ordinary snapshot loading remains historical. Raw-volume and native restart acceptance checks remain open.

Saved captures can be reopened and compared after restarting. Comparisons require the same root identity and exclusions. Partial coverage marks missing entries as unobserved rather than claiming they were deleted. The history view includes recorded size, paged changes, capture removal and database compaction. Opening a capture labels the explorer as historical; rescan before saving a new capture.

## Project documentation

The nine [interface references](interface/README.md) supplied by the user define the current visual direction. Overview and the shared shell are being adapted to their composition with measured engine data; the images are references rather than screenshots of shipped behavior.

[Implementation ledger](NEXT_TODOS.md) · [Product brief](PRODUCT.md) · [Requirements](docs/reference/requirements.md) · [Interface reference](docs/reference/design-inspiration.md)

## License

Original project code is licensed under the [MIT license](LICENSE). Dependencies retain their respective licenses. Supplied artwork remains attributed to its source in the asset metadata.
