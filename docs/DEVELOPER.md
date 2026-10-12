# Developer storage

Developer analysis classifies measured directories from the current index. It uses the same logical size, allocation and incomplete-coverage evidence as Explorer. Nested candidates belong to the outer directory so totals do not count a subtree twice. Pages contain at most 100 directories. Analysis does not execute tools or remove files.

## Project artifacts

Cargo targets require a sibling `Cargo.toml`; Node.js dependencies and build output require `package.json`. CMake builds require `CMakeLists.txt`. Gradle project caches require a Gradle build or settings file. .NET build directories require a C# or F# project. Python bytecode directories use their conventional `__pycache__` name.

## Global tool locations

Global-cache analysis resolves the current Windows profile and local-data directory through Tauri's native path resolver. An absolute `CARGO_HOME` overrides the profile's `.cargo` location. Only its `registry` and `git` directories are classified as cache; installed tools, configuration and credentials outside those directories are excluded. These locations are described in the [Cargo home documentation](https://doc.rust-lang.org/cargo/guide/cargo-home.html).

An absolute `NPM_CONFIG_CACHE` overrides the Windows `%LocalAppData%\npm-cache` default described in the [npm cache documentation](https://docs.npmjs.com/cli/v7/commands/npm-cache/). Project and user `.npmrc` files are not read; paths configured only there are not discovered automatically. Relative overrides are omitted because the application's working directory cannot establish the tool's intended base.

Locations must exist and resolve to indexed directories. The query uses their recorded sizes rather than scanning again. A scan or query rooted inside a cache reports only that measured subtree. Historical indexes omit live global-cache classification because the locations are not recorded with snapshots. Their project classifications continue to use recorded rows. Persisting cache-location evidence remains pending.

The Windows resolver also recognizes these locations and their absolute environment overrides:

| Tool | Default location | Override | Category |
|---|---|---|---|
| pnpm | Versioned directories beneath `%LocalAppData%\pnpm\store` | `PNPM_CONFIG_STORE_DIR`, then `PNPM_HOME\store`, then `XDG_DATA_HOME\pnpm\store` | Cache / Dependencies |
| pip | `%LocalAppData%\pip\Cache` | `PIP_CACHE_DIR` | Cache |
| Gradle | `%UserProfile%\.gradle\caches` | `GRADLE_USER_HOME`, then `caches` | Cache |
| NuGet packages | `%UserProfile%\.nuget\packages` | `NUGET_PACKAGES` | Dependencies |
| NuGet HTTP | `%LocalAppData%\NuGet\v3-cache` | `NUGET_HTTP_CACHE_PATH` | Cache |
| NuGet plugins | `%LocalAppData%\NuGet\plugins-cache` | `NUGET_PLUGINS_CACHE_PATH` | Cache |

These locations follow [pip caching](https://pip.pypa.io/en/stable/topics/caching/) and its [cache-directory option](https://pip.pypa.io/en/stable/cli/pip/), [Gradle directory layout](https://docs.gradle.org/current/userguide/directory_layout.html) and [NuGet package/cache management](https://learn.microsoft.com/en-us/nuget/consume-packages/managing-the-global-packages-and-cache-folders). NuGet projects can use packages directly from the global packages directory, so it is labelled Dependencies. Gradle configuration, daemon files, installed JDKs and wrapper distributions are outside the selected `caches` directory.

Tool configuration files and command-line overrides are not inspected. When configured locations overlap, a scoped query uses the most specific matching root. Coincident roots retain a dependency category when one is declared. A wider scan still aggregates nested locations into their outer candidate without counting bytes twice.

pnpm discovery follows its documented [store settings](https://pnpm.io/settings/store). `PNPM_HOME` and `XDG_DATA_HOME` are considered before the Windows default. Conventional locations recognize `v3`, `v10` and `v11`. The explicit `PNPM_CONFIG_STORE_DIR` override follows the inspected pnpm 11.25 source: append `v11` unless the path already ends in `v11`. Relative overrides are omitted, and a missing configured store does not fall back to a different existing store.

Only each version's `files` and `index` directories are classified as Cache. `links` is classified separately as Dependencies because the [global virtual store](https://pnpm.io/global-virtual-store) can serve installed packages directly to projects. No outer store candidate absorbs these categories. Sibling binaries, configuration and unrelated files are excluded. Standalone bookkeeping files such as `index.db` remain visible in Explorer but are not included in these developer categories.

This is known-location analysis, not a claim that the discovered store is active for every project. YAML/legacy npmrc configuration, custom global virtual-store paths, stores selected for other disks, project-local store fallback and unrecognized future formats are not discovered automatically. No pnpm command is executed.

## Verification status

On 2026-10-12, four developer-storage integration tests and three native resolver tests passed, with formatting and strict workspace Clippy. The expanded native fixture verifies eight tool locations with measured totals, three absolute overrides and exclusion of Gradle configuration and runtimes. The pnpm fixture checks environment precedence, version suffixes, missing/relative overrides, unrelated sibling exclusion and separate shared dependencies. Source-only review verified scoped cache descendants, historical isolation, overlapping-root categories and pnpm versioned layouts. Populated native UI acceptance remains pending.

Real-filesystem tests cover configured paths, unrelated namesakes, exclusion of Cargo credentials and installed binaries, nested dependencies, totals, paging, scoped cache descendants, incomplete subtree scans and historical rejection of live cache roots. The native unit test covers default, empty, absolute and relative overrides without modifying process environment variables. Reproduce the checks with:

```powershell
cargo fmt --all
cargo test -p voldiq-core --test developer
cargo test -p voldiq --bin voldiq developer_commands::tests
cargo clippy --workspace --all-targets -- -D warnings
```

Configuration-file and per-disk pnpm discovery, custom global virtual-store locations and bookkeeping classification remain pending. Cleanup execution and explicit confirmation remain separate unfinished requirements.

## Docker Engine analysis

The Docker panel performs only `GET /version` and a negotiated `GET /v1.x/system/df` on a local Windows named pipe. It defaults to `\\.\pipe\docker_engine` and accepts a local `npipe:////./pipe/NAME` or `npipe://./pipe/NAME` through `DOCKER_HOST`. TCP, SSH and remote pipes are rejected. Docker CLI contexts and configuration files are not read; the panel identifies the actual queried pipe. No Docker command, project script, service startup or folder deletion is performed.

The client supports Engine API 1.31–1.56, selecting the lower of the daemon's advertised version and 1.56 and checking its minimum version. Newer responses use `ImageUsage`, `ContainerUsage`, `VolumeUsage` and `BuildCacheUsage`. Legacy responses use `LayersSize` for images, `SizeRw` for containers, volume `UsageData.Size` and unshared build-cache records. Shared cache records remain in the item count. This follows the [current Engine schema](https://docs.docker.com/reference/api/engine/version/v1.56.yaml), [API changes](https://docs.docker.com/reference/api/engine/version-history/) and Docker's [legacy summary adapter](https://github.com/moby/moby/blob/master/client/system_disk_usage.go).

Missing measurements and the daemon's `-1` unknown-size sentinel remain Unknown. Invalid negative values, overflow and values beyond JavaScript's exact integer range are rejected. Categories are not added to a grand total: sizes may overlap and are Engine measurements, not Windows physical allocation or promised reclaimable bytes. Volumes are explicitly labelled persistent data.

The complete request sequence has a 15-second deadline. Headers, body and wire buffering are bounded; HTTP failures, truncation, unsupported transfer encoding and ambiguous framing return errors. The panel remains usable before a drive scan and has no management actions.

On 2026-10-12, seven native tests passed, including four Docker tests covering current/legacy accounting, version and endpoint validation, HTTP framing, and an actual Windows named-pipe GET exchange plus silent-server timeout. Svelte checks, production frontend build and strict workspace Clippy passed. Source-only review found and corrected legacy shared-cache overcounting. The default local Docker pipe was unavailable in a read-only connection probe; acceptance with a running Docker Engine remains pending. No visual automation was used.

```powershell
cargo test -p voldiq --bin voldiq docker_commands::tests
```
