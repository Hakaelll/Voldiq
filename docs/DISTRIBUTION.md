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
./scripts/verify_distribution.ps1
```

The Tauri command builds the frontend before compiling the native release executable. Its output directory is `target/release/bundle/nsis`. A running development window can stay open during the release build.

`src-tauri/tauri.conf.json` selects `offlineInstaller` for WebView2. The bundler downloads Microsoft's standalone runtime installer during the build and embeds it in the setup executable. Installation therefore does not need to download that runtime. Tauri's default NSIS mode installs for the current user. See the [official Windows installer documentation](https://v2.tauri.app/distribute/windows-installer/).

## Artwork

`scripts/prepare_assets.py` derives the packaging PNGs and the 16, 24, 32, 48, 64, 128 and 256 pixel ICO frames from the supplied icon. The native executable, installer and uninstaller use this artwork. Original artwork stays in `assets`; the derived PNG metadata records its source. The script requires Pillow.

## Acceptance still required

Check the produced installer on a clean Windows machine with networking disabled and WebView2 absent. Verify installation, first launch, native filesystem access on a disposable fixture, installed icons, uninstall and retained user data. This check must use the actual installer; a native development build or embedded-runtime configuration cannot substitute for it.

Release signing and a public download are not configured. Packaging does not close the remaining product acceptance checks listed in `NEXT_TODOS.md` and `docs/TESTING.md`.

## Latest build evidence, 2026-10-14

`pnpm tauri build` completed with application sources at `bf54c5b`. This installer includes navigation that expands on hover, bounded nested storage maps and the native query changes. It predates the subsequent Overview welcome redesign. The [static verification record](../benchmarks/distribution-2026-10-14-current.json) passed architecture/version, offline WebView2 configuration, Microsoft runtime signature and icon checks. The checkout reference records the build session; the executable does not embed a verified Git commit.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0, unsigned | 12,776,960 | `F269FF73FF721945E8B32CECA16233E4751AE1ECE3A62F72CE4A63F7B439F632` |
| NSIS installer, version 0.1.0, unsigned | 220,575,946 | `0CA30F43062481FC30981A94315C26A3B716B39EE033F9D6C10F24B8CB105C7E` |

The verifier did not execute the installer or extract its embedded payload. Clean-machine offline installation, uninstall and signing remain pending.

## Earlier build evidence, 2026-10-13

`pnpm tauri build` completed with application sources at `87aa6cb`. This build includes query cancellation, bounded keyboard navigation, extensionless filtering and native analysis dates. The [static verification record](../benchmarks/distribution-2026-10-13-current.json) passed architecture/version, offline WebView2 configuration, runtime signature and supplied icon checks. Its checkout reference records the build session; the executable does not embed a verified Git commit.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0, unsigned | 12,568,576 | `C99B86844EC0189CA3CA4A283082CE1D7428F1E4C2E1A46306E6E4A6C0A4422A` |
| NSIS installer, version 0.1.0, unsigned | 220,514,926 | `CF4FAE447DE036A73A0803A22031F36BBD7ECC63539F88834E39F7C7C208C6EE` |

The new release executable was launched and remained running through the [native/WebView2 process measurement](PERFORMANCE.md#native-application-process-observation). No installer execution or UI automation took place. Clean-machine offline installation, populated native interaction and uninstall still require acceptance.

## Earlier build evidence, 2026-10-13

`pnpm tauri build` completed with the application sources at checkout `4b70b68`. The static verifier passed after the build. This checkout reference records the build session; the executable does not embed a verified Git commit.

| Artifact | Evidence |
|---|---|
| Native executable | x64 (`0x8664`), product version `0.1.0`, 12,524,544 bytes, unsigned |
| Native SHA-256 | `F557E469F03B704521AF31D32ABCA018EAD9185B70C10F94D1C583D6FEB5A0AC` |
| NSIS installer | 220,500,483 bytes, product version `0.1.0`, unsigned |
| Installer SHA-256 | `64C953721B1A51191555BE8EB3170C75EE037FFD165D0F609987DFDB4DADBD25` |
| Microsoft runtime installer | 214,090,448 bytes, valid Microsoft Corporation signature |
| Runtime SHA-256 | `2A6ADD76C37BFA872EB8C2B22D45B3216B8E2A1F193E0774006340FF62AC5FA0` |

`scripts/verify_distribution.ps1` reads artifact metadata, hashes and signatures. It checks the native architecture/version, generated NSIS current-user/offline configuration, executable and icon source paths, runtime embedding instruction, and seven ICO directory sizes with payload bounds. The NSIS and runtime bootstrap executables themselves report x86 headers; the application payload is x64. The script does not execute the installer, extract its embedded payload, decode icon artwork or verify installation. Clean-machine acceptance remains pending.

## Previous build evidence, 2026-10-12

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
