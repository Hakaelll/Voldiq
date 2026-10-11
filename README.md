# Voldiq

<p align="center"><img src="frontend/public/wordmark.png" width="420" alt="Voldiq" /></p>
<p align="center">A local storage explorer for Windows, built with Rust and Tauri.</p>

Voldiq maps the files on a drive or in a folder so you can understand where space goes. Its Rust engine owns the filesystem index; a compact Svelte interface requests bounded pages and draws a custom Canvas treemap.

> **Development status:** Voldiq is under active construction. The current source includes real volume discovery, conventional scanning, size aggregation, search, a virtual file list and a proportional treemap. The complete product acceptance checklist remains open. See [NEXT_TODOS.md](NEXT_TODOS.md).

## Principles

- Local and offline. No account, cloud service, advertising or telemetry.
- Real measurements. No sample data presented as system information.
- Read-only analysis by default. File management will require preview and confirmation.
- Precise terminology. Logical file sizes differ from unique physical allocation and volume usage.
- Small, reusable engine. Heavy analysis runs in Rust, outside the interface thread.

## Build on Windows

Install the stable Rust MSVC toolchain, Visual Studio C++ Build Tools with the Windows SDK, Node.js 22 or newer, pnpm, and Microsoft Edge WebView2. Windows 10 and 11 x86-64 are the initial targets.

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

The release configuration targets an NSIS installer and includes the offline WebView2 installer. A distributable download is not published yet.

## Source map

| Location | Responsibility |
|---|---|
| `crates/voldiq-core` | Filesystem scanning, Windows metadata, index queries and aggregation |
| `src-tauri` | Native window and a narrow command bridge |
| `frontend` | Svelte interface, virtual table, custom treemap and CSS tokens |
| `assets` | Original supplied logo and interface references |
| `docs/reference` | Complete specification and supplied design inspiration |
| `scripts/prepare_assets.py` | Reproducible PNG and multi-resolution ICO preparation |

## Scan accuracy

The conventional scanner walks directories iteratively. It tolerates inaccessible files, reports errors and supports cancellation. It records reparse points without following them, which prevents junction loops and avoids reading cloud-only contents.

Logical size includes each visible hard-link name. Unique allocated size counts a file identity once across the scan. Allocation within an individual folder therefore reflects the link encountered first. Windows volume usage includes metadata and inaccessible or unindexed content; it should not equal the logical total.

Progress reports actual file counts and bytes. No estimated percentage appears when the total is unknown. The initial index has a five-million-entry limit. Reaching the limit yields explicitly incomplete results.

## Project documentation

[Implementation ledger](NEXT_TODOS.md) · [Product brief](PRODUCT.md) · [Requirements](docs/reference/requirements.md) · [Interface reference](docs/reference/design-inspiration.md)

## License

Original project code is licensed under the [MIT license](LICENSE). Dependencies retain their respective licenses. Supplied artwork remains attributed to its source in the asset metadata.
