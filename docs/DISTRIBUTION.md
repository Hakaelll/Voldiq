# Windows distribution

Voldiq builds an x64 Windows NSIS installer from the supplied artwork. Build artifacts stay in the ignored `target` directory; no download has been published.

The executable and offline NSIS installer were refreshed on 2026-10-31 with persistent duplicate hashes, native index ownership, historical restoration review and the gated recycling integration. The [native launch record](../benchmarks/native-snapshot-cancellation-preview-20261031.json) identifies the opened executable, and the [distribution record](../benchmarks/distribution-2026-10-31-snapshot-cancellation.json) identifies the current package. Default recycling remains disabled pending the deferred combined UAC fixture. Clean-machine installation remains pending and will be checked by the user.

## Build

Use Windows with the Rust MSVC toolchain, Visual Studio C++ Build Tools, Node.js 22 (version 22.12 or later) or Node.js 24+, and the pnpm version recorded in `package.json`.

```powershell
pnpm install --frozen-lockfile
cargo fetch --locked
./scripts/verify_project.ps1
pnpm tauri build --bundles nsis
./scripts/verify_distribution.ps1
```

The Tauri command builds the frontend before compiling the native release executable. Its output directory is `target/release/bundle/nsis`. Open release previews from the separate hash-addressed copy described below so that packaging can update the canonical executable.

### Open a release preview

Prepare a separate copy before opening a release build. Running `target/release/voldiq.exe` directly can prevent the bundler from updating its metadata.

```powershell
$preview = ./scripts/prepare_release_preview.ps1
# Close any existing Voldiq window before launching another instance.
Start-Process -FilePath $preview.Path
```

The script only prepares the executable; it never launches or closes a process. It stores one verified copy per SHA-256 in ignored `target/preview`, reuses matching copies, and refuses to overwrite a mismatched executable. This keeps the release path available to the bundler. Remove obsolete preview directories only after their application instances have closed; retain the current preview while it is running.

`src-tauri/tauri.conf.json` selects `offlineInstaller` for WebView2. The bundler downloads Microsoft's standalone runtime installer during the build and embeds it in the setup executable. Installation therefore does not need to download that runtime. Tauri's default NSIS mode installs for the current user. See the [official Windows installer documentation](https://v2.tauri.app/distribute/windows-installer/).

## Artwork

`scripts/prepare_assets.py` derives the packaging PNGs and the 16, 24, 32, 48, 64, 128 and 256 pixel ICO frames from the supplied icon. The native executable, installer and uninstaller use this artwork. Original artwork stays in `assets`; the derived PNG metadata records its source. The script requires Pillow.

## Acceptance still required

Check the produced installer on a clean Windows machine with networking disabled and WebView2 absent. Verify installation, first launch, native filesystem access on a disposable fixture, installed icons, uninstall and retained user data. This check must use the actual installer; a native development build or embedded-runtime configuration cannot substitute for it.

Release signing and a public download are not configured. Packaging does not close the remaining product acceptance checks listed in `NEXT_TODOS.md` and `docs/TESTING.md`.

## Latest build, 2026-10-31

The native lifecycle working tree based on `caf391a` passes the [six-stage production source gate](../benchmarks/project-verification-native-lifecycle-20261031.json) with eight declared tests. `cargo build --offline --release -p voldiq --features production --bin voldiq` completes. The [actual application observation](../benchmarks/native-application-lifecycle-20261031.json) verifies embedded assets, normal close with exit code 0 and a reopened responding window. The optional lifecycle trace observes the existing shutdown path; it does not change that behavior. History retains cancellation for native capture/loading. The user confirmed that the earlier corrected standalone interface displays correctly; detailed interaction acceptance remains manual.

`pnpm tauri bundle --bundles nsis` replaces the installer from that verified production executable. The [distribution record](../benchmarks/distribution-2026-10-31-native-lifecycle.json) passes executable-level decoded JS/CSS matching plus architecture, version, offline/current-user settings, vendor signature and icon checks. The runtime download was reused. Native image and setup remain unsigned. Installation and installer payload extraction remain unverified.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0 | 13,240,320 | `05DE0A0493232167C0AD312E67F4FA79D604A413CE531A884580A93927B764F3` |
| NSIS installer, version 0.1.0 | 221,638,136 | `DF85A82ABD226212A1FF7FB1A04C6D6F206188FB2AE7D6D1CAA9D35263816257` |
| Microsoft offline runtime source, version 1.3.307.15 | 215,017,168 | `62863F0F0611A16CCF124B4F15650C3B66624504EBA74D7CDE1E2AE5D951249B` |

The previous window accepted a normal close request, but PID 16504 remained running with no main-window handle. Its in-use preview executable is retained until that process exits. No forced termination or deletion of its image was performed. The cause of this shutdown delay is not established. See the current launch record for exact process and image identities.

The later PID 17000 and traced PID 10304 both terminated after normal window close. PID 22500 is the reopened current preview. This does not reproduce or explain PID 16504's delay, and does not establish shutdown under active work. The exited PID 17000's 13,216,256-byte [verified obsolete preview](../benchmarks/native-lifecycle-preview-cleanup-20261031.json) was removed; the two in-use previews remain retained.

The user confirmed that the corrected standalone window displays the interface. This confirms visible startup after the development-URL fix; it does not establish acceptance of every interaction, final visual design or a clean-machine installation.

### User-owned clean-machine check

1. Copy the installer to a clean x64 Windows environment and verify its SHA-256 above. Record the Windows version and whether WebView2 is initially present. Test the absent-runtime case to verify the embedded runtime installer.
2. Disconnect networking, install for the current user and launch from the installed shortcut without development tools.
3. Scan a disposable folder with known contents. Check totals, search, folder navigation, Explorer reveal and the installed icons. Windows Recycle Bin access should open the system bin; this does not test recycling from Voldiq.
4. Close the application and uninstall it. Check shortcut/executable removal and the chosen user-data retention behavior.
5. Report the initial environment and the install, offline launch, native scan and uninstall results. These checks remain pending until that report is available.

## Earlier standalone correction, 2026-10-31

The working tree based on `3d29649` builds with `cargo build --offline --release -p voldiq --features production --bin voldiq`. The [production source gate](../benchmarks/project-verification-standalone-ui-20261031.json) passes all six stages and eight retained tests. A no-feature optimized desktop check now fails with exit 101 at the compile-time guard. The [correction record](../benchmarks/standalone-ui-fix-20261031.json) includes exact source hashes and the rejected old-image check.

The [actual executable probe](../benchmarks/embedded-ui-production-20261031.json) reports production mode and decoded runtime hashes for embedded HTML, JavaScript, CSS and supplied images. Its JS/CSS match current `dist/assets`. The corrected preview opens with no server listening on port 1420. The prior defective preview closed normally and its 12,737,536-byte executable was removed. Native visual interaction remains user-owned.

`pnpm tauri bundle --bundles nsis` replaced the existing installer from that production image. The [distribution verifier](../benchmarks/distribution-2026-10-31-standalone-ui.json) repeats the executable probe alongside architecture, version, current-user installation, offline runtime, vendor signature and icon checks. The canonical image still matches the opened preview after packaging. The offline runtime was reused. The native image and installer remain unsigned, and installer payload extraction and installation remain unverified.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0 | 13,210,624 | `ABE628085159BDDB345991A50931437F53116FB247A6D859CBF4094FCDB2430A` |
| NSIS installer, version 0.1.0 | 221,626,350 | `29E95CB877711D918E8E8171D0A0DAF2A81CF33B08BC77F54C95CA67BBB04637` |
| Microsoft offline runtime source, version 1.3.307.15 | 215,017,168 | `62863F0F0611A16CCF124B4F15650C3B66624504EBA74D7CDE1E2AE5D951249B` |

The package includes snapshot schema 5 and finite pointer-driven treemap navigation. Older schema-4 executables cannot reopen an upgraded history database. Default recycling stays disabled while the combined UAC fixture is deferred.

## Correction to earlier build evidence

The development-server connection failure reported by the user invalidates the standalone-UI acceptance of the Cargo-built images below. Their static metadata and responding-window checks did not detect the missing Tauri production protocol. The earlier static records remain historical evidence, not proof of a working standalone interface. Preview and distribution preparation now require an executable-level embedded-asset probe. The corrected package and executable-level evidence are recorded above.

## Earlier build evidence, 2026-10-31 before production-mode correction

The persistent-hash working tree based on `2e770fd` passes the [complete source gate](../benchmarks/project-verification-persistent-hashes-20261031.json), builds its native release executable and opens a responding Voldiq window. The [cache verification record](../benchmarks/persistent-hashes-20261031.json) hashes the application sources. `pnpm tauri bundle --bundles nsis` then replaces the previous installer without recompilation. The canonical executable still matches the opened preview after packaging.

The [static distribution record](../benchmarks/distribution-2026-10-31-persistent-hashes.json) passes architecture, version, current-user installation, offline runtime configuration, the Microsoft runtime signature and all seven icon sizes. The runtime download was reused, and the previous owned preview executable was removed after its process closed normally. Only one preview directory remains. Application and installer are unsigned. Embedded-payload extraction and actual installation remain unverified.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0 | 12,737,536 | `8A0921EB496CE6EF5A93E77664149C75695A072DCB001B0D18EB51278C3E5F0E` |
| NSIS installer, version 0.1.0 | 221,168,127 | `D603D8ADDA82FBFF5B7FC677C89C78D019981F86B65795677B5CB2B1B5C21167` |
| Microsoft offline runtime source, version 1.3.307.15 | 215,017,168 | `62863F0F0611A16CCF124B4F15650C3B66624504EBA74D7CDE1E2AE5D951249B` |

Opening this version migrates snapshot storage to schema 5 without rewriting saved captures. Earlier executables that support only schema 4 cannot reopen an upgraded database. This development package keeps default recycling disabled while the combined UAC fixture is deferred.

## Earlier build evidence, 2026-10-31 before persistent hashes

`pnpm tauri bundle --bundles nsis` completed at checkout `efee62e`, reusing the release executable from the [completed source gate](../benchmarks/project-verification-index-ownership-20261031.json). No additional Rust compilation or target directory was created. The canonical executable still matches the hash of the responding native preview after packaging.

The [static verification record](../benchmarks/distribution-2026-10-31-current.json) passes architecture, version, current-user installation, offline runtime configuration, the Microsoft runtime signature and all seven icon sizes. The executable and installer remain unsigned. The verifier does not extract the embedded payload or execute installation, and the executable contains no verified Git commit.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0 | 12,723,712 | `0B3AEAA86B4B6B921F8F6DC42F0F4275F4F96C12B2D10D16A20710675E1490DA` |
| NSIS installer, version 0.1.0 | 221,158,256 | `2999C2719450957C9D47AA55EF47AAF775CFF0911C008FC3C73729FDDC79998C` |
| Microsoft offline runtime source, version 1.3.307.15 | 215,017,168 | `62863F0F0611A16CCF124B4F15650C3B66624504EBA74D7CDE1E2AE5D951249B` |

The previous installer was replaced at the same output path. The old WebView2 download matched the previous distribution receipt and was not running. Its 214,999,248-byte cache file and empty containing directory were removed without recursive deletion. The current runtime and running preview were retained. See [cache cleanup evidence](../benchmarks/packaging-cache-cleanup-20261031.json).

## Earlier build evidence, 2026-10-30

`pnpm tauri bundle --bundles nsis` completed at checkout `6c3fc8a`, reusing the already verified native executable without recompilation. The [static verification record](../benchmarks/distribution-2026-10-30-current.json) passes the architecture/version, current-user/offline configuration, vendor runtime signature and seven icon-size checks. The Voldiq executable and installer remain unsigned. The verifier does not extract the embedded payload or prove installation, and the executable contains no verified Git commit.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0 | 12,440,064 | `21962F69D95DCA07407344BFB938FA9CC9B8DE55E30845C0B7E0992C88816783` |
| NSIS installer, version 0.1.0 | 221,087,507 | `8D89E2E31A2873F04F3956CB93A5F012FA166531A716CAD7946E955766A43A4C` |
| Microsoft offline runtime source, version 1.3.307.15 | 214,999,248 | `F9D45CF7BEF9576D6DF50C7A7525E0BC8C26B6C3BC2C667B494AAFB062A35B6F` |

The application stayed open during packaging. Its in-use image was retained at `target/release/voldiq.running-6c3fc8a.exe`, while an identical writable copy at the ordinary release path let the bundler apply its package metadata and restore the original file afterward. After that instance closed, the holding image was removed. The same release was subsequently opened from its hash-addressed preview copy; its window responded and the canonical release executable remained writable. The previous installer was replaced, rather than archived.

## Earlier build evidence, 2026-10-15

`pnpm tauri bundle --bundles nsis` regenerated the existing installer from the updated release executable. Application sources include the snapshot-cache persistence committed as `5400c84`; the bundle session ran at `d4934ef`, whose subsequent changes contain only the WinRT probe and cleanup documentation. This package includes the graphical interface changes, cleanup ancestor pinning, local Docker context discovery and snapshot schema version 4. The older installer was replaced rather than archived.

The [static verification record](../benchmarks/distribution-2026-10-15-current.json) passed architecture/version, generated offline/current-user configuration, vendor runtime signature and icon checks. The native executable does not embed a verified Git commit, and the verifier does not extract the installer's payload or execute installation.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Native x64 executable, version 0.1.0, unsigned | 12,858,368 | `8DC9EDD25F8CA27AE28CB2E0836E6FF0368A7F855679790EB281B0A01AC12A9E` |
| NSIS installer, version 0.1.0, unsigned | 220,613,604 | `1F0712E1978A0A7D8CB8040E2459F8AB0162BDF36CDED087BDB93232E44211E3` |

The embedded-runtime source installer retains its valid Microsoft signature and the recorded 214,090,448-byte hash. The current Windows 11 Home host has no available Windows Sandbox or configured VM command-line tool, so no isolated installation was attempted. Clean Windows installation without networking/WebView2, installed operation, uninstall and signing remain pending. Opening the new application migrates history to schema version 4, which older application binaries cannot reopen.

## Earlier build evidence, 2026-10-14

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
