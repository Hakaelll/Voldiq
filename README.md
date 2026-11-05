# Voldiq

<p align="center"><img src="frontend/public/wordmark.png" width="420" alt="Voldiq" /></p>
<p align="center">Understand your storage.</p>
<p align="center">Windows 10 / 11 · Rust · Tauri · Svelte · Local and offline</p>
<p align="center"><a href="https://github.com/Hakaelll/Voldiq/releases/latest">Download</a> · <a href="docs/ARCHITECTURE.md">Architecture</a> · <a href="docs/ACCEPTANCE.md">Acceptance</a> · <a href="LICENSE">MIT license</a></p>

Voldiq is a Windows storage explorer with a native Rust indexing engine and a compact desktop interface. Explore measured storage usage, locate large files and duplicates, and review files before moving them to the Recycle Bin.

## What you can do

- Scan a drive or folder and explore results while analysis continues.
- Explore the same proportional layout as a Canvas treemap or an isometric Skyline, with synchronized folder details and virtual file lists.
- Search the current index with **Ctrl+K**, filters, sorting and keyboard navigation.
- Find large files and verify duplicates with complete BLAKE3 hashes.
- Review cleanup candidates, move selected files to the Recycle Bin and restore compatible recorded items.
- Inspect developer dependencies, build artifacts, supported tool caches and local Docker storage.
- Save local SQLite snapshots and compare growth, added files and removed files across restarts.
- Choose light or dark appearance, reduced motion, size metrics and scanning preferences.

No account, cloud service, advertising or telemetry. The application uses actual filesystem data and works offline.

## Agent analysis in the current source

The current source adds human-managed connections in Settings, plan review and transfer history in Cleanup, plus a separate Rust CLI and authenticated local stdio MCP server. They scan allowed folders, query measured snapshots and prepare exact recycling plans through the existing core, without loading the desktop renderer. Humans approve specific manifests in Voldiq; agent file execution is still being implemented. These additions are not part of the published 0.2.0 installer yet. See [MCP setup](docs/MCP.md) and [workbench coverage](docs/WORKBENCH-COVERAGE.md).

## Install

Download **Voldiq_0.2.0_x64-setup.exe** from the [latest release](https://github.com/Hakaelll/Voldiq/releases/latest). The current-user installer includes the offline Microsoft WebView2 runtime. Development tools are not required.

The application and installer are unsigned.

## Explore your storage

1. Choose a drive or folder and start an analysis.
2. Select a treemap region or open Explorer to inspect its files.
3. Use Large files, Duplicates or Cleanup to review candidates.
4. Check the selected paths and confirm the action. Recorded recycled items can be reviewed for restoration in Cleanup activity.

Logical size, unique physical allocation and Windows volume usage are separate measurements. Moving a file to the Recycle Bin does not immediately free its occupied space. Partial scans and access failures remain visible.

Analysis is read-only. Cleanup requires an explicit selection and native preview. Voldiq revalidates file identities, protected paths and retained duplicate copies before execution. Restoration never overwrites an existing file. Advanced permanent removal requires typing `DELETE` and cannot be undone. See [cleanup behavior](docs/CLEANUP.md) and [security](docs/SECURITY.md).

## Build on Windows

Use the Rust MSVC toolchain, Visual Studio C++ Build Tools with Windows SDK, Node.js 22.12+ or 24+, Microsoft Edge WebView2, and the pnpm version in `package.json`.

```powershell
pnpm install --frozen-lockfile
cargo fetch --locked
pnpm tauri dev
```

For a production installer:

```powershell
./scripts/verify_project.ps1
pnpm tauri build --features production --bundles nsis
./scripts/verify_distribution.ps1
```

The source gate runs formatting, Svelte diagnostics, frontend and Rust behavior tests, the frontend production build and strict Clippy. It reuses the release target and offline Cargo dependencies. Output installers are in `target/release/bundle/nsis`.

New builds use a small WebView2 bootstrapper: the measured installer is 9.9 MB and the application executable is 25.3 MB. If WebView2 is missing, installation downloads Microsoft's runtime; the application itself works offline. For a complete offline installer, add `--config src-tauri/tauri.offline.conf.json` and verify it with `./scripts/verify_distribution.ps1 -Offline`. The previously published 0.2.0 installer still includes the full runtime.

Rust build and test artifacts can occupy several GB; they are not application contents. After verification and packaging, run `./scripts/clean_build_cache.ps1` to inspect what will be kept, then add `-Apply` to remove inactive release artifacts. It retains the application, CLI, current installer and running previews. The next Rust build regenerates its cache. See [Windows distribution](docs/DISTRIBUTION.md) for the measured footprint and acceptance limits.

A direct native build must use `cargo build --offline --release -p voldiq --features production --bin voldiq`. Release optimization alone does not enable Tauri's embedded interface. Preview and distribution checks execute the embedded-asset probe to verify production mode and the bundled frontend.

`pnpm dev` provides a browser-only frontend preview. Use the native application for filesystem access. See [Windows distribution](docs/DISTRIBUTION.md) for packaging and preview commands.

## Project structure

| Path | Responsibility |
|---|---|
| `crates/voldiq-core` | Filesystem indexing, queries, duplicates, cleanup and snapshots |
| `src-tauri` | Windows integration, IPC, operation ownership and packaging |
| `frontend` | Svelte interface, virtual lists and Canvas treemap |
| `assets` | Supplied artwork and source assets |
| `benchmarks` | Reproducible fixtures, measurements and verification receipts |
| `scripts` | Source, asset and distribution verification |

## Documentation

[Skyline](docs/SKYLINE.md) · [Architecture](docs/ARCHITECTURE.md) · [Design](DESIGN.md) · [Developer storage](docs/DEVELOPER.md) · [NTFS](docs/NTFS.md) · [Performance](docs/PERFORMANCE.md) · [Testing](docs/TESTING.md) · [Product acceptance](docs/ACCEPTANCE.md)

Performance figures describe their recorded hardware and workloads. They are engineering measurements, not universal guarantees. Known filesystem and tool-configuration limits are documented with each subsystem.

## License

[MIT](LICENSE).
