# Developer storage

Developer analysis classifies measured directories from the current index. It uses the same logical size, allocation and incomplete-coverage evidence as Explorer. Nested candidates belong to the outer directory so totals do not count a subtree twice. Pages contain at most 100 directories. Analysis does not execute tools or remove files.

## Project artifacts

Cargo targets require a sibling `Cargo.toml`; Node.js dependencies and build output require `package.json`. CMake builds require `CMakeLists.txt`. Gradle project caches require a Gradle build or settings file. .NET build directories require a C# or F# project. Python bytecode directories use their conventional `__pycache__` name.

## Cargo and npm cache locations

Global-cache analysis resolves the current Windows profile and local-data directory through Tauri's native path resolver. An absolute `CARGO_HOME` overrides the profile's `.cargo` location. Only its `registry` and `git` directories are classified as cache; installed tools, configuration and credentials outside those directories are excluded. These locations are described in the [Cargo home documentation](https://doc.rust-lang.org/cargo/guide/cargo-home.html).

An absolute `NPM_CONFIG_CACHE` overrides the Windows `%LocalAppData%\npm-cache` default described in the [npm cache documentation](https://docs.npmjs.com/cli/v7/commands/npm-cache/). Project and user `.npmrc` files are not read; paths configured only there are not discovered automatically. Relative overrides are omitted because the application's working directory cannot establish the tool's intended base.

Locations must exist and resolve to indexed directories. The query uses their recorded sizes rather than scanning again. A scan or query rooted inside a cache reports only that measured subtree. Historical indexes omit live global-cache classification because the locations are not recorded with snapshots. Their project classifications continue to use recorded rows. Persisting cache-location evidence remains pending.

## Verification status

On 2026-10-12, all three developer-storage integration tests and the native configuration test passed. Formatting and strict workspace Clippy also passed. A source-only review found and verified fixes for scoped cache descendants and historical isolation. Populated native UI acceptance remains pending.

Real-filesystem tests cover configured paths, unrelated namesakes, exclusion of Cargo credentials and installed binaries, nested dependencies, totals, paging, scoped cache descendants, incomplete subtree scans and historical rejection of live cache roots. The native unit test covers default, empty, absolute and relative overrides without modifying process environment variables. Reproduce the checks with:

```powershell
cargo fmt --all
cargo test -p voldiq-core --test developer
cargo test -p voldiq --bin voldiq developer_commands::tests
cargo clippy --workspace --all-targets -- -D warnings
```

pnpm stores, pip and global Gradle caches, .NET caches and Docker analysis remain pending. Docker storage must be inspected through its supported commands or APIs. Its internal folders must not be deleted directly. Cleanup execution and explicit confirmation remain separate unfinished requirements.
