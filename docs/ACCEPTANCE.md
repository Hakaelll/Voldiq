# Product acceptance

Voldiq 0.1.0 implements the original [specification](reference/requirements.md). The user reported that all outstanding checks were complete and requested final delivery and a GitHub release. This closes user-owned manual acceptance; it is not a new machine-generated benchmark or a claim that deferred diagnostic scripts were executed.

## Acceptance record

| # | Required outcome | Delivery evidence |
|---|---|---|
| 1 | Install and launch without development tools | Offline current-user NSIS installer; clean Windows checks accepted by the user. |
| 2 | Detect real drives | Native discovery of volumes, filesystems, capacity and accessibility; manual acceptance. |
| 3 | Scan a drive or folder | Real filesystem fixtures, directory scanning, NTFS integration and cancellation; manual acceptance. |
| 4 | Show measured storage usage | Distinct volume, logical and unique allocated sizes; aggregation and hard-link fixtures. |
| 5 | Navigate the treemap | Custom Canvas geometry, bounded descendant queries, pointer and keyboard navigation; manual acceptance. |
| 6 | Select folders and inspect details | Indexed paths, ancestors, direct children and cancellable details; manual acceptance. |
| 7 | Search files | Native filters, sorting, paging, keyboard navigation and Ctrl+K. |
| 8 | Find large files | Native file/folder rankings and scoped filters; manual acceptance. |
| 9 | Detect real duplicates | Complete BLAKE3 verification, identity checks, hard-link handling, cancellation and persisted hash reuse. |
| 10 | Review cleanup candidates | Explicit selections, immutable native previews, supported rules, exclusions and bounded pages. |
| 11 | Execute permitted removal with confirmation | Recycling and recorded-item restoration enabled for delivery; separate typed permanent removal, retained-handle validation and durable outcomes. |
| 12 | Cancel long operations | Scan, query, duplicate, cleanup and History cancellation; operation ownership rejects stale results. |
| 13 | Recover from access errors | Partial coverage, reported issues and error boundaries; manual acceptance. |
| 14 | Save and compare snapshots | Local SQLite persistence, reopened comparisons, growth and file changes; manual acceptance. |
| 15 | Remain usable during analysis | Filesystem work outside the UI thread, bounded results and concurrent live-index fixtures; manual acceptance. |
| 16 | Work offline | Local services and embedded offline WebView2 installer; manual acceptance. |
| 17 | Use Voldiq artwork | Supplied wordmark and application, installer and uninstaller icons. |
| 18 | Main tests pass | Final [six-stage source verification](../benchmarks/project-verification-release-010.json), retaining eight tests. |
| 19 | Produce a distribution build | Final [production distribution verification](../benchmarks/distribution-release-010.json) and [GitHub release](https://github.com/Hakaelll/Voldiq/releases/tag/v0.1.0). |
| 20 | Reproducible documentation and benchmarks | Architecture, security, testing, performance, subsystem references and archived measurements. |

## Evidence boundaries

Manual acceptance is the user's report. The final automation verifies source checks, native production assets and packaging. It does not invent measured frame rates, hardware-independent performance guarantees or a clean-machine report with environment details.

The earlier combined privileged recycling diagnostic was deferred and was not rerun during finalization. Its historical records retain that fact. Final activation follows the user's acceptance and instruction to finish delivery; it does not weaken runtime path, identity, confirmation, cancellation or no-overwrite checks.

The older windowless-process incident remains in the diagnostic history with no established root cause. Later normal exits and final manual acceptance do not retrospectively diagnose that instance. In-use images are never force-deleted as cache cleanup.

Performance targets remain hardware-dependent engineering targets. Recorded workloads, limitations and the withdrawn development-protocol observation are documented in [Performance](PERFORMANCE.md). Unsupported tool configurations, browser cache formats and filesystem behavior remain documented subsystem limits, rather than claims of universal support.

## Verification references

- [Testing and the retained eight-test inventory](TESTING.md)
- [Cleanup guarantees and experiments](CLEANUP.md)
- [Recycling transaction evidence](RECYCLE-TRANSACTION.md)
- [Windows packaging and release](DISTRIBUTION.md)
- [Original requirements](reference/requirements.md)
