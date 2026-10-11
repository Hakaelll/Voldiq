# Windows distribution

Voldiq builds an x64 Windows NSIS installer from the supplied artwork. Build artifacts stay in the ignored `target` directory; no download has been published.

## Build

Use Windows with the Rust MSVC toolchain, Visual Studio C++ Build Tools, Node.js 22 (version 22.12 or later) or Node.js 24+, and the pnpm version recorded in `package.json`.

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm test
cargo test -p voldiq-core
cargo clippy --workspace --all-targets -- -D warnings
pnpm tauri build --bundles nsis
```

The Tauri command builds the frontend before compiling the native release executable. Its output directory is `target/release/bundle/nsis`. A running development window can stay open during the release build.

`src-tauri/tauri.conf.json` selects `offlineInstaller` for WebView2. The bundler downloads Microsoft's standalone runtime installer during the build and embeds it in the setup executable. Installation therefore does not need to download that runtime. Tauri's default NSIS mode installs for the current user. See the [official Windows installer documentation](https://v2.tauri.app/distribute/windows-installer/).

## Artwork

`scripts/prepare_assets.py` derives the packaging PNGs and the 16, 24, 32, 48, 64, 128 and 256 pixel ICO frames from the supplied icon. The native executable, installer and uninstaller use this artwork. Original artwork stays in `assets`; the derived PNG metadata records its source. The script requires Pillow.

## Acceptance still required

Check the produced installer on a clean Windows machine with networking disabled and WebView2 absent. Verify installation, first launch, native filesystem access on a disposable fixture, installed icons, uninstall and retained user data. This check must use the actual installer; a native development build or embedded-runtime configuration cannot substitute for it.

Release signing and a public download are not configured. Packaging does not close the remaining product acceptance checks listed in `NEXT_TODOS.md` and `docs/TESTING.md`.

## Build evidence, 2026-10-12

The application at source commit `cbd1728`, with the packaging changes accompanying this record, built using Rust/Cargo 1.97.1 and Tauri CLI 2.12.1. `pnpm tauri build --bundles nsis` completed successfully. After setting installer and uninstaller icons, `pnpm tauri bundle --bundles nsis` regenerated the final installer from that release executable. A frozen-lockfile install also passed after the Node engine declaration changed.

| Artifact | Evidence |
|---|---|
| `target/release/voldiq.exe` | PE machine `0x8664` (x64), product version `0.1.0`, 11,966,464 bytes |
| `target/release/bundle/nsis/Voldiq_0.1.0_x64-setup.exe` | 220,364,517 bytes (210.16 MiB), unsigned |
| Installer SHA-256 | `0EDB810F5C82A47B51DAC03644EDFF626A044AE7FD8621E197F2B9C354C4C22E` |
| Embedded Microsoft runtime installer | 214,090,448 bytes; Authenticode status `Valid`, signer Microsoft Corporation |
| Runtime installer SHA-256 | `2A6ADD76C37BFA872EB8C2B22D45B3216B8E2A1F193E0774006340FF62AC5FA0` |

Generated `target/release/nsis/x64/installer.nsi` sets `INSTALLMODE` to `currentUser`, `INSTALLWEBVIEW2MODE` to `offlineInstaller` and both icon paths to the supplied ICO. Its active offline branch embeds the standalone runtime using the NSIS `File` instruction. Binary inspection of the committed ICO confirms all seven documented frames. A source-only reviewer checked the corrected configuration and prerequisites. No installation or visual acceptance was performed.

These hashes identify this local build, not every future rebuild. Tauri's runtime download can change; verify and record the downloaded vendor installer again for each release.
