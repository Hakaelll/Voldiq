# Voldiq

<p align="center"><img src="frontend/public/wordmark.png" width="420" alt="Voldiq" /></p>
<p align="center">A local storage explorer for Windows, built with Rust and Tauri.</p>

Voldiq maps the files on a drive or in a folder so you can understand where space goes. Its Rust engine owns the filesystem index; a compact Svelte interface requests bounded pages and draws a custom Canvas treemap.

> **Development status:** Voldiq is under active construction. The current source includes real volume discovery, live conventional scanning, search, a virtual file list, a proportional treemap, BLAKE3 duplicate analysis and persistent SQLite snapshots. The complete product acceptance checklist remains open. See [NEXT_TODOS.md](NEXT_TODOS.md).

## Principles

- Local and offline. No account, cloud service, advertising or telemetry.
- Real measurements. No sample data presented as system information.
- Read-only analysis by default. File management will require preview and confirmation.
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

The release configuration targets an NSIS installer and includes the offline WebView2 installer. Build output is in `target/release/bundle/nsis`; a distributable download is not published yet. See [Windows distribution](docs/DISTRIBUTION.md) for build instructions and remaining installer acceptance checks.

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

Both paths tolerate inaccessible files, report errors and support cancellation. They record reparse points without following them, which prevents junction loops and avoids reading cloud-only contents.

Logical size includes each visible hard-link name. Unique allocated size counts a file identity once across the scan. Allocation within an individual folder therefore reflects the link encountered first. Windows volume usage includes metadata and inaccessible or unindexed content; it should not equal the logical total.

Progress reports actual file counts and bytes. Partial results can be explored while scanning; cancellation retains the measured index. No estimated percentage appears when the total is unknown. The initial index has a five-million-entry limit. Reaching the limit yields explicitly incomplete results.

## Duplicate analysis

Duplicate discovery groups indexed files by size, samples at most 8 KiB and verifies matching content with complete BLAKE3 hashes. Locked, changed and vanished files produce reported issues. Hard links to the same file identity are represented once. Repeated analysis reuses hashes only after validating file identity, size, modification time and native change time.

Potential allocation conservatively retains the largest independently allocated copy. Files with multiple hard links contribute no reclaimable estimate, because links may exist outside the scan. This estimate does not imply that moving files to the Recycle Bin frees volume space. Recycling is still pending.

## File management previews

Cleanup lists indexed `.tmp` and `.temp` files at least seven days old in the Windows user temporary directory. Select files manually, then review their actual paths and sizes. Unscanned and partial coverage remain visible.

Duplicate selections retain a copy from each group and reread full BLAKE3 content before showing selected and retained paths. The review is cancellable. Recycling remains under development.

Select regular files in Explorer or Large files and use Review selection to inspect their real paths, sizes and potential allocation. Rust retains immutable plans and checks scanned identities, change times, exclusions and native Windows protected locations. Historical captures cannot produce management previews. Verification tests cover replacement and mutation with controlled files. Recycling and permanent deletion are not exposed yet; see [cleanup guarantees and remaining work](docs/CLEANUP.md).

## Developer storage

The developer view classifies indexed Cargo targets, Node.js dependencies and build output, Python bytecode caches, Gradle project caches and .NET build directories. It recognizes current-user Cargo, npm, pnpm, pip, Gradle and NuGet locations, including supported absolute environment overrides. NuGet installed packages and pnpm shared links are labelled Dependencies, separate from their caches. Project manifests identify ambiguous names such as `target`, `bin` and `build`. Nested candidates belong to their outer artifact so category totals do not count the same subtree twice. Results use bounded pages and expose logical size, allocation and incomplete coverage. Management actions, additional configuration discovery and Docker analysis remain pending. See [developer analysis and configuration limits](docs/DEVELOPER.md).

## Snapshot history

Snapshots remain local in the application's data directory. SQLite stores a baseline and new versions of changed entries, so unchanged captures reuse existing entry data. Captures use transactions; cancellation and integer overflow roll back the entire write. Original Windows UTF-16 names survive persistence.

Complete, stable NTFS scans can retain journal checkpoints with their captures. Repeating the same volume scan can apply journal changes to the live index. After restart, the first volume scan can resume from a matching capture with native identities. Both paths rescan fully when continuity or scope cannot be verified. Existing history databases migrate transactionally, and ordinary snapshot loading remains historical. Raw-volume and native restart acceptance checks remain open.

Saved captures can be reopened and compared after restarting. Comparisons require the same root identity and exclusions. Partial coverage marks missing entries as unobserved rather than claiming they were deleted. The history view includes recorded size, paged changes, capture removal and database compaction. Opening a capture labels the explorer as historical; rescan before saving a new capture.

## Project documentation

[Implementation ledger](NEXT_TODOS.md) · [Product brief](PRODUCT.md) · [Requirements](docs/reference/requirements.md) · [Interface reference](docs/reference/design-inspiration.md)

## License

Original project code is licensed under the [MIT license](LICENSE). Dependencies retain their respective licenses. Supplied artwork remains attributed to its source in the asset metadata.
