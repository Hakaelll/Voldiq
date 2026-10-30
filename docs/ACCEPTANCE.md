# Product acceptance

This record follows the original [specification](reference/requirements.md#28-criterios-de-aceptación). It distinguishes implemented behavior and automated fixtures from acceptance of the packaged Windows application. The project remains under development.

Automated fixture references below include historical verification. On 2026-10-21, the suite was reduced from 178 to 8 tests. See [Testing](TESTING.md) for the cases still maintained; earlier fixture results do not imply current automated coverage.

## Required outcomes

| # | Requirement | Current evidence and remaining gate |
|---|---|---|
| 1 | Install and launch without development tools | Offline NSIS packaging is built and statically checked. Clean Windows installation, first launch and uninstall are pending. See [distribution](DISTRIBUTION.md). |
| 2 | Detect real drives | Native volume discovery supplies actual capacity, filesystem and accessibility. Populated native acceptance remains manual. |
| 3 | Scan a drive or folder | Real directory fixtures, cancellation and a million-file benchmark exist. Privileged raw NTFS integration remains unverified on this host. See [NTFS](NTFS.md). |
| 4 | Show measured storage usage | Native volume, logical and unique allocated measurements remain distinct. Fixtures verify aggregation and hard links; presentation remains manual. |
| 5 | Navigate the treemap | Custom Canvas geometry and bounded descendant queries are fixture verified. Native pointer/keyboard routes and rendering remain manual. |
| 6 | Select folders and inspect details | Cancellable native details return paths, ancestors and five direct children. Native focus and presentation remain manual. |
| 7 | Search files | Indexed scope, extension, name and paging fixtures pass. Ctrl+K is connected; native interaction remains manual. |
| 8 | Find large files | Native file/folder ranking, filters, page paths and full-result size scale are fixture verified. Native interaction remains manual. |
| 9 | Detect real duplicates | Real BLAKE3 fixtures verify content, changed files, hard links and cancellation. Populated native workflow remains manual. |
| 10 | Review cleanup candidates | Real temporary/project/old-file fixtures verify scope, age and exclusions. Page selection is bounded to 500. Native review remains manual. |
| 11 | Execute permitted removal with confirmation | Advanced permanent removal has handle-bound validation and durable result fixtures. Default recycling and Undo remain unimplemented. See [cleanup](CLEANUP.md). |
| 12 | Cancel long operations | Scanner, query, duplicate and cleanup fixtures verify cancellation and stale-response isolation. Whole-app latency remains unmeasured. |
| 13 | Recover from access errors | Partial coverage, issues and error boundaries exist and have fixtures. Native recovery presentation remains manual. |
| 14 | Save and compare snapshots | SQLite persistence, comparisons, partial coverage and complete file-change totals are fixture verified. Native save/load/compare workflow remains manual. |
| 15 | Remain usable during analysis | Blocking filesystem work is off the interface thread and results use bounded pages. Loaded native responsiveness and frame timing remain unmeasured. |
| 16 | Work offline | Application services are local and WebView2 is embedded in the installer. Clean-machine installation and native operation with networking disabled remain pending. |
| 17 | Use Voldiq artwork | Supplied icons/wordmark are integrated; static ICO and packaging checks exist. Installed icon presentation remains manual. |
| 18 | Main tests pass | Run [verify_project.ps1](../scripts/verify_project.ps1) for formatting, frontend diagnostics/tests, workspace tests, strict Clippy and frontend build. This gate does not substitute for the other rows. |
| 19 | Produce a distribution build | The installer was regenerated on 2026-10-30 from the current executable, including the filename-search change and Windows Recycle Bin access. Static checks pass; the user will check clean offline Windows installation. The final completed-product build remains pending. See [distribution evidence](DISTRIBUTION.md). |
| 20 | Reproducible documentation and benchmarks | [Architecture](ARCHITECTURE.md), [design](../DESIGN.md), [security](SECURITY.md), [performance](PERFORMANCE.md) and [testing](TESTING.md) document scope and reproduction. New map overhead and remaining whole-app targets still need measurements. |

## Remaining implementation and performance work

The complete source gate passed on 2026-10-14 at checkout `a2dbace`, including all workspace Rust tests and 30 frontend tests. The [verification summary](../benchmarks/project-verification-2026-10-14.json) records all six successful stages. A separate [failure-path check](../benchmarks/project-verification-failure-2026-10-14.json) injected exit code 7 into the first command and proved that no success summary was emitted and the working directory was restored. These records establish the automated source gate, not the remaining native or installer outcomes.

The [2026-10-15 source gate](../benchmarks/project-verification-2026-10-15.json) passed all six stages with the browser-cache changes accompanying this record, including workspace Rust tests, 34 frontend tests and strict Clippy. Its checkout reference identifies the base commit of the tested working tree. The browser filename fixture was then corrected to use a distinct uppercase filename on case-insensitive Windows; all three browser rule tests and Rust formatting passed again. Native browser-cache recovery and visual interaction remain separate acceptance gates.

Default recycling requires a proven namespace handoff, durable per-item results and original-path restoration before Undo can be implemented. The rejected handle/path and staging experiments are recorded in the cleanup document. Advanced permanent removal does not fulfill that default behavior.

Developer analysis recognizes measured projects and supported current-user tool configurations. New captures persist classifications for indexed global-cache roots and historical queries ignore current tool settings. Docker resolves supported local CLI contexts with bounded configuration reads and a real named-pipe fixture. Live Engine acceptance and additional tool configuration discovery remain open. Cleanup now includes explicitly selected seven-day-old external Blockfile HTTP cache files in standard Chrome/Edge profiles, with current native identity checks and disposable-file tests. Real-browser cache recovery, custom profiles, other browser/application caches and developer management remain pending. Docker volumes remain persistent data and are not direct-delete candidates.

The performance document separates warm scanner fixtures, query fixtures and process observations. The [2026-10-25 release query run](../benchmarks/query-release-2026-10-25.provenance.json) updates the current core's million-entry baseline with 50 completed queries and five repetitions per query/method. Background load was uncontrolled; this does not establish a speedup or native responsiveness. Warm startup below two seconds, ordinary 60 FPS interaction, loaded memory budgets, cancellation latency and privileged NTFS versus directory scanning remain acceptance targets. The descendant query adds a bounded-candidate linear pass. Its million-entry debug diagnostic records wall/thread CPU cost; release descendant-query, concurrent and whole-app measurements remain pending.

The user owns visual acceptance and has requested no Computer Use or automated screen inspection. CLI compilation, real fixtures and process/window metadata are allowed and remain separate evidence. No source-level review is described as a completed visual check.

## Next verification order

1. Run the complete source verification and fix failures before adding functionality.
2. Resolve and implement default recycling/Undo with disposable fixtures.
3. Measure loaded native behavior and the new map query without changing filesystem accuracy.
4. Build the final installer and verify clean offline installation, native operation and uninstall.
5. Complete the user's manual visual and interaction acceptance.
