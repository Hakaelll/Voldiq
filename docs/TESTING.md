# Testing

A native registry fixture verifies detail supersession, old completion/cancellation isolation, independence from files/overview and cancellation of all three families on frontend-session reload. A controlled frontend queue fixture checks obsolete detail errors, dismissal, reopening and continued file-list results. Six native query tests, strict workspace Clippy, all 19 frontend tests, Svelte diagnostics and production compilation pass. Source review covers App dismissal, scan terminal and analysis-replacement hooks; no native/WebView interaction was automated.

`tests/details.rs` creates 300 nested folders and a Unicode file. It verifies a complete canonical path, 301 correctly ordered ancestors, parity with ordinary details, cancellation at every observed checkpoint, cancellation before invalid-ID validation and empty root ancestry. Existing unpaired UTF-16 path fixtures also run through the shared path/detail implementation. Native overview calls the cancellable detail path, retaining its complete-response contract. These checks do not measure IPC cancellation latency or allocation/disposal time.

Automatic concurrency policy tests cover seeking, non-seeking and unknown devices against zero, one and many available processors. Native IPC accepts only `auto` or integer limits 1–8. The real concurrency fixture now compares Automatic with manual workers for exact paths, bytes, hard-link allocation, exclusions, entry limits, live queries and cancellation. Storage descriptor tests reject truncated headers and invalid boolean bytes before interpretation; a native probe checks an owned volume without modifying its fixture and rejects UNC/relative inputs. These tests do not establish an optimal worker count or a driver cancellation deadline.

pip resolver fixtures combine native-folder inputs, legacy/current user, virtual-environment and explicit config files with environment overrides. Exact measured totals distinguish the selected caches from existing unrelated and uppercase-section directories, forged APPDATA/PROGRAMDATA locations, credential-bearing configuration and lower-priority caches. Additional cases cover missing/null overrides, DEFAULT inheritance, literal values, relative paths, invalid UTF-8, oversized files and directory inputs. A Windows test reads the actual Known Folder paths without changing environment variables. Existing npm fixtures exercise the shared bounded reader.

File-type totals now accompany the bounded group list. The 80-extension filesystem fixture verifies 80 scoped bytes/files and 82 root bytes across 81 files, despite only 30 groups being returned. Native serialization checks the totals in the overview response. Frontend tests verify that eight visible groups plus Other account for all measured groups, including those outside the IPC page, empty results and zero-byte files. Other has no extension-filter action.

The overview fixture verifies one native response containing consistent directory details, child rows and file-type bytes, plus cancellation before index access. An 80-extension filesystem fixture verifies deterministic top-30 ties, exact file counts and bytes, standard/cancellable parity, extensionless files and cancellation during filtering, ancestry, group conversion and ranking. A controlled frontend composite-response test verifies that an obsolete overview cannot replace the latest scope. Workspace tests, strict Clippy, 16 frontend tests, Svelte diagnostics and the production build pass for this change. These checks do not measure native transport cancellation latency.

`tests/partial_scan.rs` reads the live shared index from progress callbacks, checks directory totals, cancels after the first file and verifies retained partial results. The callback runs outside the index write lock.

Run on Windows with the Rust MSVC toolchain and C++ Build Tools.

```powershell
pnpm install
pnpm check
pnpm test
cargo test -p voldiq-core
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p voldiq
```

The Rust integration tests create temporary fixtures. They check real size aggregation, hard-link allocation deduplication, cancellation, exclusions, bounded pages, Unicode names, a 100-level directory tree, a junction pointing back to its parent and a sparse file with a logical size over 1 GiB. The Windows junction fixture uses `mklink /J`; the sparse fixture uses `fsutil sparse setflag`. They do not inspect or change personal files.

The treemap test checks 100 measured rectangle areas, pairwise non-overlap and viewport bounds. It also checks empty and invalid inputs.

Virtual-list tests verify bounded physical height and final-row reachability for five million logical rows, normal row spacing, keyboard endpoints, page movement and wheel pixel/line/page deltas. FileTable cancels pending keyboard focus when the user changes focus or pointer intent, and uses the current row position after live revisions. Source review covers those lifecycle branches; the tests do not run the component in a WebView. Manual checks should include End/Home across a large result set, moving focus to Search during a pending page and navigation after a live sort refresh.

Query tests verify complete, stable pages over 1,038 actual files, deterministic size/name/date order, case-insensitive filters, zero results beyond the final page, the 500-entry cap and ASCII/Unicode filename ordering. A separate real-file test verifies kind-filtered folder ranking and paging. A 1,500-file fixture cancels a query while it holds a shared-index read lock, verifies no partial page is returned, then acquires the write lock and checks a completed cancellable page. Rank unit tests compare every pivot against sorted ordered, reversed, repeated and scrambled inputs and interrupt both partitioning and heap fallback.

Native query-registry tests verify supersession, cancellation before registration, old completion/cancellation isolation, independent families, safe integer boundaries and frontend reload with a retained native registry. Frontend queue tests use controlled IPC promises to verify one running/one latest pending request, suppression of obsolete errors/results, remount ticket ownership and independent families. They do not exercise the real WebView transport. Manual native cancellation and reload acceptance remain open.

Native analysis-date tests distinguish fresh complete/partial results from retained previous results, absent dates, empty cancellation, failed/in-progress work and clocks before the Unix epoch. The progress JSON keeps its existing fields and adds a nullable date. The real clear-analysis fixture verifies that operation refusal retains the date and successful clearing removes the loaded index/date together. Source review checks publication and snapshot callers; manual date/status presentation remains open.

A real file-type fixture verifies nullable extension keys, dotfiles, nested extensionless files, exact counts and logical bytes, folder exclusion, scope, paging, name/size filters and cancellable-query parity. Native JSON tests verify a defaulted boolean filter and distinct null/string type keys. Frontend query-construction tests ensure file-type selections use the same subtree scope as overview totals, while ordinary browsing retains direct-child scope. Manual legend/filter acceptance remains open.

Five duplicate tests verify full BLAKE3 content, matching samples with different middle bytes, unique-size exclusion, sample-only rejection, validated hash reuse, same-size changes, hard-link deduplication, locked and vanished files, and cancellation before full hashing. All data is generated in temporary fixtures.

## Manual application checks

Four NTFS unit tests validate packed binary records, native UTF-16 preservation, truncation, malformed headers, dynamic filename offsets and journal continuity. `cargo run -p voldiq-core --example ntfs_probe` uses a disposable two-hard-link fixture and probes the existing volume read-only. On this host raw-volume access is denied; the conventional fixture passes with two names and 256 logical bytes. The probe does not elevate or create a journal. Elevated NTFS enumeration and live index integration remain unverified.

Five snapshot tests save actual fixtures, reopen the database, compare additions, removals and directory growth, verify unchanged-version reuse, check rollback and integer overflow, delete intermediate captures, compact storage, preserve native UTF-16 names and reject invalid path components. Incomplete captures report omissions without asserting deletion; captures with different exclusions cannot be compared.

```powershell
pnpm tauri dev
```

Choose a disposable fixture folder. Check drive capacity, scan progress, root totals, folder navigation, search, extension filtering, size ranking, file details and Explorer reveal. Cancel an in-progress scan and verify that the resulting index is marked incomplete. Scan a folder with an inaccessible child and verify that access issues are visible. Resize the window, collapse navigation, use Ctrl+K and keyboard controls, then check both themes and reduced motion.

Browser preview checks verify layout and preferences only. The preview deliberately has no filesystem bridge. Native interaction checks cannot be replaced with injected browser data.

## Release acceptance

The x64 NSIS installer builds; packaging evidence is recorded in `docs/DISTRIBUTION.md`. Clean-machine installation, offline startup, actual native interaction, duplicate management, cleanup confirmation, snapshot interaction and NTFS fast path must pass before the project is considered complete. A build passing today does not close those pending checks.
