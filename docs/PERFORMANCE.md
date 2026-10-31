# Performance measurements

## Extension grouping allocation

Extension grouping borrows lowercase ASCII extensions from the existing index strings instead of allocating a lowercase string for every file. Uppercase and non-ASCII extensions retain the existing Unicode lowercase conversion. Returned groups still own their strings; index layout, grouping, ranking, cancellation checkpoints and JSON fields are unchanged. No persistent normalization cache is introduced.

The [interleaved comparison](../benchmarks/overview-comparison-20261031-100849-092.provenance.json) runs five release processes per version, alternating version order. Each process uses the same harness and performs five calls per method/shape after warmup. The [summary](../benchmarks/overview-comparison-20261031-100849-092.summary.json) reports the median of those five process medians; ranges below are across process medians. All 450 measured calls and the separate 45-row candidate diagnostic completed with exact reference IDs and totals. ASCII/Unicode normalization checks also passed.

| Complete overview, one million files | Before median, range | After median, range |
|---|---:|---:|
| 16 sibling folders, equal sizes | 175.204 ms, 172.130–198.132 ms | 110.908 ms, 93.677–123.960 ms |
| 1,000 sibling folders, varied sizes | 160.877 ms, 114.389–170.501 ms | 106.337 ms, 98.863–124.339 ms |
| 64 levels, selected depth 32 | 358.436 ms, 342.742–392.108 ms | 244.934 ms, 228.430–285.583 ms |

The equal-size and deep shapes have non-overlapping process-median ranges, with observed median time reductions of 36.7% and 31.7%. The varied-width median is lower but its ranges overlap. Branch-only and serialization ranges also overlap, so no change is established for those paths. Background desktop CPU load was uncontrolled, at 40% before and 51% after; these generated lowercase-extension workloads do not establish a general application speedup.

The candidate diagnostic still identifies extension grouping as the dominant component: 111.331, 91.827 and 241.526 ms respectively. Complete-overview median thread CPU changes from 171.875 to 109.375 ms for the equal-size shape and from 359.375 to 250.000 ms for the deep shape. This supports CPU work on the synchronous scan/grouping path as the remaining limiter. Thread counters are quantized. The full JSON payload stays bounded at 20,190, 66,254 and 7,615 bytes respectively. Native IPC, concurrent indexing, Canvas, total-app memory and interaction latency are unmeasured here.

To reproduce the paired comparison, build baseline revision `f0f9c9b` with the command below, then capture its executable before returning to the optimized revision:

```powershell
cargo build --offline --release -p voldiq --example overview_benchmark
New-Item -ItemType Directory -Force target/probes | Out-Null
[IO.File]::Copy("$PWD/target/release/examples/overview_benchmark.exe", "$PWD/target/probes/overview-before.exe", $false)
$baseline = @{
  executableSha256 = (Get-FileHash target/probes/overview-before.exe).Hash
  coreIndexSha256 = (Get-FileHash crates/voldiq-core/src/index.rs).Hash
  harnessSha256 = (Get-FileHash src-tauri/examples/overview_benchmark.rs).Hash
  coreSourceCommit = (git rev-parse HEAD)
}
$baseline | ConvertTo-Json | Set-Content -LiteralPath target/probes/overview-before.json
```

After restoring the optimized source revision, build the same example and run `./benchmarks/run_overview_comparison.ps1`. It checks the baseline and shared harness hashes before running. A failed or incomplete run stops the sequence. Temporary baseline copies can be removed after retaining the reports. The retained core tests and strict workspace Clippy pass; the automated test inventory remains eight cases.

## Release overview baseline

The production overview composition and bounded branch pass now live in `src-tauri/src/overview.rs`. The native command and `overview_benchmark` example compile that same module. Both function bodies were checked against their previous implementation after whitespace normalization. This separation changes no query algorithm or response fields.

Build and run on Windows:

```powershell
cargo build --offline --release -p voldiq --example overview_benchmark
./benchmarks/run_overview_benchmark.ps1
```

The [initial release record](../benchmarks/overview-release-20261031-100410-322.provenance.json) links 45 measured rows and a separate 45-row component diagnostic. Each shape contains one million generated in-memory files. Construction, an independent full-sort reference and warmup occur outside query timers. Five repetitions alternate complete overview and branch-only order. Exact returned IDs, byte/file totals, page bounds and decoded serialization results are checked. The current harness additionally checks uppercase ASCII, Unicode, missing and empty extensions outside the timers. No declared test was added.

| Shape | Overview median, range | Branch pass median | JSON bytes |
|---|---:|---:|---:|
| 16 sibling folders, equal sizes | 160.876 ms, 130.369–203.750 ms | 11.410 ms | 20,190 |
| 1,000 sibling folders, varied sizes | 169.459 ms, 138.716–208.790 ms | 12.512 ms | 66,254 |
| 64 levels, selected depth 32 | 344.137 ms, 238.349–437.709 ms | 8.700 ms | 7,615 |

Component diagnostics identify extension grouping as the dominant synchronous component, with medians of 183.513, 161.591 and 294.920 ms respectively. They run separately and must not be summed against another process's overview sample. Thread CPU is close to wall time for the complete overview and extension grouping, supporting CPU work on one thread as the limiter. Short counters advance in roughly 15.625 ms steps; zero CPU samples do not mean zero work. Desktop CPU load was uncontrolled, at 34% before and 29% after. This is a Rust query/serialization baseline, not a filesystem scan, native IPC, concurrent indexing, Canvas, startup or whole-app result.

## Filename search normalization

Filename filtering now searches the original string when its name is already lowercase ASCII. It retains the existing Unicode lowercase conversion for uppercase or non-ASCII names. Query normalization, matching semantics, ranking, page bounds and cancellation checkpoints are unchanged. This avoids constructing a temporary lowercase string for the common lowercase ASCII case without caching a second filename for every indexed entry.

The retained snapshot integration case checks literal expected results for uppercase queries, uppercase filenames, Unicode names and missing results. It exercises both query methods against a real scan and the reopened snapshot. The core's six retained tests pass in release, as does strict all-target core Clippy. The total automated inventory remains eight cases.

The [2026-10-30 raw queries](../benchmarks/search-copy-2026-10-30.csv) and [provenance](../benchmarks/search-copy-2026-10-30.json) record 50 completed queries through the changed release core. The existing benchmark checks counts, page sizes and returned-ID parity. No controlled before/after comparison completed, so these results do not establish a speedup. Allocator counts, varied filename distributions and native search latency remain unmeasured.

## Current million-entry query baseline

The 2026-10-25 release run at `e3d8bb2` completed all 50 queries in the existing `query_benchmark` example. It constructs 1,000,000 generated file entries and 1,001 directories in memory. Each query/method runs five times, alternating the standard and cancellable method order. Both methods finish normally; no cancellation is requested.

```powershell
cargo build --release -p voldiq-core --example query_benchmark
./target/release/examples/query_benchmark.exe
```

| Query | Standard median, range (ms) | Cancellable median, range (ms) |
|---|---:|---:|
| Root children by size | 5.851, 5.472–9.909 | 5.513, 5.097–6.660 |
| All files by size | 36.777, 33.054–61.677 | 22.988, 19.641–34.848 |
| Name, offset 500,000 | 231.921, 223.603–368.587 | 236.482, 227.918–296.466 |
| All files by modification time | 40.009, 33.455–46.024 | 32.220, 31.388–40.174 |
| Name search, no matches | 82.867, 72.978–101.349 | 86.322, 71.660–109.832 |

The [raw CSV](../benchmarks/query-release-2026-10-25.csv) contains all repetitions. The example checks exact result counts, the 100-row page bound and matching returned IDs between the two methods. Root queries match 1,000 folders; global queries match 1,000,000 files; the chosen search matches zero files. These assertions run after each query timer. They do not independently establish that every returned page has the correct ordering.

The [provenance record](../benchmarks/query-release-2026-10-25.provenance.json) identifies the source and executable hashes, host, successful exit and process counters. The Intel i7-1165G7 host reported 34% CPU load before and 43% after the run. Compilation had finished, but other host work was uncontrolled. The process consumed 4,296.875 ms of CPU during 5,278.282 ms elapsed, including index construction and teardown; the individual query timers sum to 4,173.109 ms. These aggregate counters are consistent with substantial CPU work in this single-threaded executable, but do not identify a per-query bottleneck. No speedup, regression or method preference follows from this run.

This updates the query baseline for the current core. It measures neither filesystem scanning nor loaded native interaction, IPC latency, requested cancellation, whole-app memory or frame times. The generated shape is shallow and names are ASCII. The older full-sort comparison below remains historical evidence; it is not a controlled comparison with this host session.

## Native application process observation

### Current release, 2026-10-31

The executable with persistent duplicate hashes, SHA-256 `8A0921EB496CE6EF5A93E77664149C75695A072DCB001B0D18EB51278C3E5F0E`, was observed at checkout `c98a680` through the unchanged read-only process observer. One baseline and five five-second intervals retain seven stable native/WebView2 process identities. All 42 per-process rows independently reproduce the aggregate memory sums and surviving CPU deltas, with zero read errors or availability changes. See [raw group counters](../benchmarks/app-processes-20261031-current.csv), [per-process counters](../benchmarks/app-processes-20261031-current.processes.csv), [checked summary](../benchmarks/app-processes-20261031-current.summary.json) and [provenance](../benchmarks/app-processes-20261031-current.provenance.json).

| Counter | Median of five | Range |
|---|---:|---:|
| Total private bytes | 161,210,368 B | 161,148,928–161,210,368 B |
| Native private bytes | 4,186,112 B | 4,124,672–4,186,112 B |
| WebView2 private bytes | 157,024,256 B | 157,024,256–157,028,352 B |
| Summed working sets | 375,713,792 B | 375,681,024–375,713,792 B |

Intervals lasted 5.131–5.183 seconds. None of the surviving CPU counters advanced during this capture. This does not prove continuous zero CPU. The native process accounts for about 2.6% of median private bytes; WebView2 accounts for the rest. Summed working sets can count shared pages repeatedly and are not unique physical memory.

The application workload and user input were not inspected or controlled. No build, scan benchmark or automated UI input was initiated during capture. Host CPU load was 10% before and 50% after, and the runtime was WebView2 156.0.4314.50. These observations establish current process counters, not controlled idle or loaded-index budgets, a comparison with the older runtime, startup latency or 60 FPS. The private-byte result falls within the specification's approximate 150–200 MB target for this observation only. Controlled workload acceptance remains open.

### Historical observation, 2026-10-13

The release executable built from application sources at `87aa6cb` was opened on 2026-10-13. A read-only observer collected Voldiq and associated WebView2 descendants over one baseline and five intervals of 5.162–5.192 seconds. Seven process identities remained measurable in every sample, with zero counter-read errors. No compilation, scan benchmark or automated UI input ran during capture. User interaction, current UI state and host background load were not inspected, so this is an observed release-process baseline rather than a controlled idle or populated-analysis acceptance test.

```powershell
./scripts/measure_app_processes.ps1 -TargetPid <VoldiqPID> -OutputPath "$PWD/benchmarks/app-processes.csv" -IntervalSeconds 5 -Samples 5
```

The target must already be running. The observer does not launch, terminate or control it. Root identity changes abort capture; output paths stay inside the repository and existing files are preserved. Float fields use invariant formatting. [Summary rows](../benchmarks/app-processes-2026-10-13.csv), [per-process counters](../benchmarks/app-processes-2026-10-13.processes.csv) and [provenance](../benchmarks/app-processes-2026-10-13.provenance.json) allow independent recalculation. The executable hash matches the distribution verifier record.

| Counter | Median of five | Range |
|---|---:|---:|
| Total private bytes | 173,699,072 B | 173,633,536–181,952,512 B |
| Native private bytes | 5,775,360 B | 5,775,360–5,775,360 B |
| WebView2 private bytes | 167,923,712 B | 167,858,176–176,177,152 B |
| Summed working sets | 396,005,376 B | 395,898,880–417,546,240 B |

Private bytes describe allocations that cannot be shared between processes. Working sets include resident pages; adding them can count shared pages repeatedly. These counters are not interchangeable, and the summed working set is not a unique physical-memory total. See Microsoft's [private-byte definition](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.privatememorysize64) and [working-set definition](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.workingset64).

CPU counters did not advance over any of the five intervals. The observer reports surviving-process CPU seconds divided by elapsed seconds, with 100% representing one core. It does not capture CPU consumed by processes that exit between observations. The availability-change columns describe newly measured or no-longer-measured identities, including failed reads, rather than confirmed lifecycle events. All interval availability changes were zero here. This observation does not establish continuous zero CPU, loaded-index memory, startup latency, 60 FPS or completion of the full memory/idle targets.

## Bounded virtual file list

The file list requests at most 80 rows per page. Its physical spacer height is capped at 8,000,000 CSS pixels, while logical coordinates cover the full indexed row count. Visible rows retain 36-pixel spacing. Scrollbar position maps to logical index position; pure vertical wheel pixel, line and page deltas map back to normal logical steps. Diagonal/horizontal gestures and Ctrl/Shift-modified events retain native browser behavior. Clipping prevents stale pages from extending the physical scroll range while a new query is pending.

Geometry tests cover the first and last rows of a five-million-row list, intermediate positions, ordinary uncompressed lists, keyboard endpoints and wheel steps. These tests prove coordinate calculations, not native DOM rendering, focus timing, 60 FPS or real WebView pixel precision. No browser automation or visual inspection was performed. Manual large-index and keyboard acceptance remain open.

## Treemap canvas reuse

Treemap hover and selection redraws now retain the canvas dimensions when CSS size and device-pixel ratio are unchanged. Each draw replaces the transform with the current HiDPI transform rather than accumulating scaling on the reused context. Source review checked balanced clipping state and painting; Svelte diagnostics, existing geometry tests and production compilation pass. Canvas frame time, allocations and native HiDPI presentation remain unmeasured, so this change carries no quantified speedup or FPS claim.

## Cooperative query cancellation

Selected-item details now use a third independent request family. The frontend keeps one running and one latest pending detail request; selecting/opening another item cancels the obsolete one. Closing the pane, Escape, scan terminal events, analysis replacement and unmount invalidate both its queue and presentation token. Files, overview and details share one native worker lifecycle while retaining separate registry slots. Each command fixes its own family, preventing a file-query argument from selecting the detail slot. Native transport latency remains unmeasured.

Overview requests now return directory details, the child page and file types from one blocking worker holding one index read guard. Supersession cancels the complete request. File-type aggregation checks cancellation during entry filtering, ancestry traversal, group conversion and rank selection; it selects at most 30 groups before sorting them. The ranker supports owned extension strings without copying a pivot. Cancellation discards the complete response rather than publishing a partial overview. Directory details now check cancellation every 256 parent visits, ancestor-ordering swaps and path components, plus before validation and after string conversion. Ordinary path/detail calls retain the same native-name and ancestor semantics. Cancellation still has no fixed wall-time bound, including string allocation and disposal of accumulated output. This change has not been benchmarked for native IPC latency.

The core now exposes `Index::query_cancellable`, which returns `None` when its caller cancels. It checks during filtering, ancestor traversal and rank selection, and never returns a partial page. Filtering and partitioning check every 256 steps. Heap fallback checks every 256 roots or extractions; each sift is bounded by heap depth. Final sorting and cloning cover at most 500 entries. These are cooperative checkpoints, not a measured wall-time cancellation guarantee. Existing core `query` callers retain standard-library rank selection. The frontend's overview and file list now use the cancellable native command with independent request ownership and bounded latest-request queues. Native runtime cancellation latency remains unmeasured.

```powershell
cargo run --release -p voldiq-core --example query_benchmark
```

Five rounds compare both methods on the same synthetic in-memory index of 1,000,000 files and 1,000 folders. Method order alternates. Each round asserts exact totals, page lengths and identical ordered IDs. The cancellable method reads an unset atomic flag. The name-search case matches zero entries, so it measures filtering without ranking. [Raw results](../benchmarks/query-cancellation-2026-10-13.csv) contain all 50 samples from 2026-10-13 on the Windows 11/i7-1165G7 host with Rust 1.97.1/MSVC.

| Query | Standard median, range | Cancellable median, range |
|---|---:|---:|
| Root size | 5.680 ms, 5.401–7.067 | 6.173 ms, 5.420–6.839 |
| Global size | 34.466 ms, 32.159–50.738 | 23.354 ms, 20.229–25.575 |
| Global name | 214.876 ms, 205.225–222.990 | 242.544 ms, 225.208–255.455 |
| Global modified date | 39.983 ms, 31.352–42.453 | 33.582 ms, 30.849–37.378 |
| Name search | 88.276 ms, 76.776–104.752 | 93.400 ms, 79.782–99.505 |

The cancellable name ranking is slower on this fixture. The other differences do not establish a general speedup. No compilation or test suite ran during timing, but host background load remained uncontrolled. This comparison excludes filesystem scanning, IPC, UI rendering and actual cancellation latency. The rank algorithm uses median-of-three partitioning with a bounded-depth heap fallback and constant auxiliary ranking memory; the matched-entry vector still scales with the number of matches.

## Large real-filesystem fixture

The Windows `benchmark` example accepts 100 to 1,000,000 files and performs five scans with the production default metadata concurrency of one. It creates real 128-byte files in 100 owned temporary folders, checks available space before creation, and removes its own directory when finished. The space allowance is 8,192 bytes per file plus 512 MiB; this is a preflight safety check, not a guarantee against concurrent disk consumption. Fixture creation is excluded from scan timings and warms filesystem metadata.

```powershell
cargo run --release -p voldiq-core --example benchmark -- 1000000
```

Every scan must have the exact file, entry and logical-byte totals, complete coverage, known allocation and zero errors. Root paging and a matching global search must return the expected counts. Output includes actual allocation, elapsed scan/query times, entry layout, and Windows private bytes, working set and lifetime peak working set sampled after queries. These are scanner-process counters, including allocator retention; they exclude the native app and WebView2. The peak includes fixture creation and previous runs, so it is not a per-scan peak.

Five small-fixture runs and strict Clippy passed on 2026-10-13. The large fixture completed five measured scans with exactly 1,000,000 files, 1,000,101 entries, 128,000,000 logical and allocated bytes and zero errors in every run. Root queries returned 100 folders; the name search matched 10,000 files and returned a bounded page of 100. [Raw results](../benchmarks/real-million-2026-10-13.csv) retain each run.

The process exited successfully after cleanup, and a separate filesystem check confirmed its owned temporary directory no longer existed. Read-only review checked all six medians and ranges against the CSV rows.

| Measurement | Median of five | Range |
|---|---:|---:|
| Scan | 107,341.048 ms | 107,076.542–158,272.273 ms |
| Root page | 5.534 ms | 5.355–8.857 ms |
| Global name search | 67.530 ms | 59.831–74.683 ms |
| Private bytes after queries | 153,669,632 B | 153,264,128–153,985,024 B |
| Working set after queries | 149,319,680 B | 149,254,144–149,561,344 B |
| Lifetime peak working set | 190,713,856 B | 190,578,688–190,914,560 B |

This is a verified warm-directory-scanner baseline on the Windows 11/i7-1165G7/NVMe host described below, not a comparison with another implementation. The scan sources were at `324fd02` with the benchmark changes later committed as `9f22b69`; the measured executable predates the optional kind filter added in `4c3c656`. Its SHA-256 is `25CA3B286FC4F8C9E368C96AA5117B8FC43B106902BD3D3B8599F0DA332B3B8C`. Fixture creation and cleanup are outside timing. The app and other host processes remained open, CPU load was not isolated, and the first scan was slower. No cold-cache control, syscall profile, NTFS fast-path comparison or native UI timing was performed. These measurements do not establish disk bandwidth or a universal scan speed. Previous synthetic million-entry query results below remain distinct.

## Automatic concurrency on the current scanner

The [2026-10-31 raw samples](../benchmarks/concurrency-auto-20261031-114406-133.csv), [summary](../benchmarks/concurrency-auto-20261031-114406-133.summary.json) and [provenance](../benchmarks/concurrency-auto-20261031-114406-133.provenance.json) measure the current directory scanner on 100,000 owned 128-byte files in 100 folders. Five rounds rotate fixed 1/2/4/8 and Automatic. Every complete scan verifies 100,101 entries, 100,000 files, 12,800,000 logical bytes, known allocation, complete coverage and zero errors. Newly written metadata is warm. Creation, output validation and fixture removal are outside the scan timers.

| Setting | Scan median ms | Scan range ms | Process CPU median ms |
|---|---:|---:|---:|
| 1 | 16,291.779 | 15,650.718–19,711.802 | 16,109.375 |
| 2 | 10,017.676 | 9,479.703–10,987.595 | 19,609.375 |
| 4 | 5,780.052 | 5,613.171–6,745.765 | 20,921.875 |
| 8 | 4,306.039 | 4,100.044–4,563.832 | 21,359.375 |
| Automatic | 9,806.502 | 9,471.246–10,629.044 | 19,156.250 |

Automatic's separate preflight probe reports no seek penalty and two workers. The scanner resolves its own capability inside the timed call; the CSV preflight count is not thread telemetry. Automatic and fixed-two spreads overlap. Single-worker CPU time is close to elapsed time, while multiple workers consume more aggregate CPU to reduce elapsed time. This supports CPU execution of warm metadata work as the relevant resource, without isolating a particular syscall. No file contents are read during scanning. The result does not establish disk bandwidth or a universal fastest policy.

The existing conservative Automatic policy remains unchanged. Eight workers finish this fixture sooner, but that does not justify maximum parallelism on HDDs or unknown devices. The host has four cores/eight logical processors; background CPU load was uncontrolled at 60% before and 57% after. No project compilation ran during the timed process. This is not a cold-cache, HDD, network, raw-NTFS or whole-app measurement.

A separate workload in the same executable cancels five scans per setting from a live progress callback after at least 10,000 published files. All 25 return partial, internally consistent indexes with `Cancelled` and zero errors. Automatic's request-to-return median is 0.314 ms, range 0.264–0.521 ms, including scanner return after worker joining. The callback runs between metadata batches, so this proves batch-boundary cancellation. It does not measure interruption of an outstanding native metadata request, UI event delivery or cancellation under an independently timed signal. CPU counters are quantized; short samples cannot establish zero CPU cost.

Build with `cargo build --offline --release -p voldiq-core --example concurrency_benchmark`, then run `./benchmarks/run_concurrency_benchmark.ps1 -FileCount 100000`. The wrapper preserves raw CSV, source/image hashes and host observations, validates all 50 rows and does not kill or restart a timed-out process. The executable checks a disk-space safety reserve before fixture creation and explicitly verifies removal of its owned temporary directory. No declared test was added.

## Independent scanner cancellation

The subsequent [raw samples](../benchmarks/concurrency-auto-20261031-115443-329.csv), [summary](../benchmarks/concurrency-auto-20261031-115443-329.summary.json) and [provenance](../benchmarks/concurrency-auto-20261031-115443-329.provenance.json) cover a cancellation signal sent by a separate thread. A progress callback notifies that thread after at least 10,000 published files. The callback returns, the scanner continues, and the signalling thread waits one millisecond before writing the shared cancellation flag. The measured interval starts at that independent request and ends when the scanner returns after joining metadata workers. Joining the signalling thread is outside that interval.

Five rotating samples per setting use 100,000 newly written real files. All 25 return `Cancelled`, with zero errors, a nonempty incomplete index, fewer than 100,000 published files and exact logical totals for the retained files. The fixture directory is explicitly removed and its absence checked. This run does not repeat the full-scan matrix.

| Setting | Request-to-return median ms | Range ms |
|---|---:|---:|
| 1 | 0.343 | 0.129–0.437 |
| 2 | 0.485 | 0.268–1.432 |
| 4 | 0.438 | 0.423–0.640 |
| 8 | 0.710 | 0.500–0.828 |
| Automatic | 0.450 | 0.366–0.634 |

The Automatic preflight uses the same canonical path form as the scanner and reports two workers. Host CPU load was uncontrolled at 59% before and 51% after. These are warm local metadata requests. The run does not instrument individual native syscalls, prove interruption of a stalled driver request, measure frontend event delivery or establish whole-app cancellation latency. It is not a speed comparison between worker settings.

Reproduce with `cargo build --offline --release -p voldiq-core --example concurrency_benchmark`, then `./benchmarks/run_concurrency_benchmark.ps1 -FileCount 100000 -CancelOnly`. The current default runs the full matrix followed by independent cancellation. The earlier callback-boundary dataset remains valid for its recorded source revision and scope. No production scanner policy or declared test count changed; example Clippy, release compilation and fixture assertions pass.

## Queries during live index publication

The [raw live-query observations](../benchmarks/live-query-20261031-123016-889.jsonl), [per-run summary](../benchmarks/live-query-20261031-123016-889.summary.json) and [provenance](../benchmarks/live-query-20261031-123016-889.provenance.json) cover five alternating pairs of directory scans over 100,000 owned 128-byte files in 100 folders. One pass has no reader; the other uses the actual shared `scan_live` index and rotates root listing, global file search and the production overview composition. The overview module is imported directly from the desktop source. Pages contain at most 100 entries; the overview retains its production bounds.

All ten final indexes have 100,101 entries, 100,000 files, exact logical totals, known allocation, complete coverage and zero errors. All 515 query replies pass consistency checks against the locked snapshot they read, including published file totals, search counts, category byte/file totals, bounded map branches and JSON output. The fixture is removed and its absence verified.

The table describes samples taken with 50,000–100,000 files already published. Each run's median and nearest-rank p95 are calculated separately before aggregating across the five runs, rather than treating correlated requests as independent repetitions.

| Query | Late samples | Median of run medians ms | Median of run p95 ms | Run p95 range ms |
|---|---:|---:|---:|---:|
| Root listing | 85 | 0.661 | 1.009 | 0.781–1.680 |
| Global file search | 85 | 4.901 | 8.319 | 6.787–16.023 |
| Overview | 84 | 11.157 | 17.246 | 13.119–21.384 |

These intervals run from read-lock request through JSON serialization. Read-lock wait never exceeds 0.007 ms in the late samples; query computation dominates response time, with serialization much smaller. Lock hold includes bounded consistency assertions, and overview answer boxing is part of the query measurement. JSON decoding checks run after the response timer. The observer sleeps 100 ms after each completed response, so it generates at most ten requests per second rather than an exact fixed rate.

Quiet scan median is 10,212.972 ms, range 9,874.110–12,511.147. Observed scan median is 10,477.387 ms, range 10,237.246–11,624.291. Paired differences range from -1,115.954 to +1,238.143 ms. The spans overlap and do not establish a clear throughput change attributable to this observer. The observed workload includes test-only validation and response decoding. Host load was uncontrolled at 40% before and 46% after; no project compilation ran during capture.

Reproduce with `cargo build --offline --release -p voldiq --example live_query_benchmark`, then `./benchmarks/run_live_query_benchmark.ps1 -FileCount 100000`. Fixture creation, final validation, observer joining, diagnostic output and removal are outside scan timers. No Tauri IPC, frontend event delivery, DOM, Canvas, frame rate, whole-app memory, cold storage or raw-NTFS result is established. These async backend timings must not be treated as UI frame times. No production behavior or declared test count changes.

## Bounded scan concurrency

The current UI adds Automatic while preserving saved numeric limits. Before conventional enumeration, Automatic queries the local fixed volume's [seek-penalty descriptor](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-device_seek_penalty_descriptor) through a zero-access handle and read-only storage property IOCTL. Relative, UNC and non-fixed paths are unclassified. Unavailable or malformed descriptors and reported seek penalties select one worker. A valid no-penalty descriptor selects at most two, bounded by available processors. It is a device capability hint, not an SSD brand/type or speed measurement. No file contents are read by this probe; driver latency and cancellation of the synchronous IOCTL have not been measured.

Two workers are a conservative policy choice informed by the warm SSD fixture below, rather than its fastest result. Automatic does not benchmark each user's drive or guarantee improvement. Runtime calibration, cold/HDD/network comparisons and host-load adaptation remain pending. Native options omitted by callers still use one worker, and fixed 1–8 requests bypass the storage probe. NTFS/journal paths do not use this metadata pool.

An already cancelled scan skips automatic device resolution and enters the existing cancelled enumeration path. Cancellation arriving during a synchronous driver request cannot abort that request immediately. On 2026-10-13, the owned temporary fixture's actual Windows volume returned `Some(false)` for seek penalty; its files remained unchanged. This is one capability observation, not a timing result or a universal device classification.

Settings now saves a real metadata-concurrency limit from 1 to 8. One worker runs directly on the scanning thread; higher values use a persistent scoped pool. Job and result channels each hold at most `workers × 16` items, with batches constrained by remaining index capacity. Directory iteration and index publication stay on the coordinator, and metadata results are applied in enumeration order. This retains exact limits, hard-link allocation deduplication and consistent live queries. Cancellation stops scheduling and publication, drains already started metadata work and joins workers before returning. NTFS enumeration and journal replay keep their existing execution model. No file content is read by this pool.

The historical 2026-10-13 `concurrency_benchmark -- 10000` run created 10,000 actual 128-byte files in 100 owned temporary folders. Five rounds rotated the order of 1, 2, 4 and 8 workers. The current example also includes Automatic and a separate independent cancellation workload. The earlier callback-boundary run and its successor are described above. Timing excludes fixture creation and validates exact entry, logical-byte and file totals, known allocation, complete coverage and zero errors after every scan. Windows CPU milliseconds include process user and kernel time; unavailable CPU measurement on other platforms is reported as NaN. [Raw results](../benchmarks/concurrency-2026-10-13.csv) were collected on 2026-10-13 with the i7-1165G7 (4 cores, 8 logical processors), Windows 11, Rust 1.97.1/MSVC and an NVMe CA5-8D256 SSD.

| Workers | Median wall time | Wall range | Median process CPU |
|---:|---:|---:|---:|
| 1 | 1,216.734 ms | 898.289–1,707.280 ms | 1,218.750 ms |
| 2 | 923.460 ms | 573.310–1,118.408 ms | 1,765.625 ms |
| 4 | 533.897 ms | 400.964–612.296 ms | 1,843.750 ms |
| 8 | 368.672 ms | 326.170–508.490 ms | 1,812.500 ms |

Every run measured 10,101 entries, 10,000 files, 1,280,000 logical and allocated bytes, with zero errors. The fixture was newly written and subsequent runs reused warm metadata; this is not disk-bandwidth throughput. More workers reduced elapsed time on this fixture while using more CPU. The host was running the app and development builds, and a before-run CPU-load baseline was unavailable; rotation reduces but does not eliminate that noise. No isolated syscall profile, HDD/USB/network fixture, cold-cache or million-file concurrency comparison has been established. These numbers do not choose a universal fastest setting. At the time of this measurement, the default was one worker. The current Automatic capability policy is described above; runtime calibration remains pending.

## First scanner baseline

Measured on 2026-10-11 with an Intel Core i7-1165G7, 4 cores and 8 logical processors, approximately 15.8 GiB of visible memory, Windows 11 Home 10.0.26200, Rust 1.97.1, x86-64 MSVC. The fixture lives on the host's temporary volume. Its storage model was not recorded, so these results should not be generalized to HDDs or other systems.

```powershell
cargo run --release -p voldiq-core --example benchmark -- 10000
```

The runner creates 10,000 real 128-byte files in 100 folders, checks exact file and byte totals, then scans the same fixture three times. The first run is not a cold-cache measurement because fixture creation has just touched the files. Metadata handles collect actual allocation and file identity for each file. No content hashing is included.

| Run | Entries | Errors | Scan | Root page | Global name search |
|---|---:|---:|---:|---:|---:|
| 1 | 10,101 | 0 | 868.573 ms | 0.060 ms | 0.907 ms |
| 2 | 10,101 | 0 | 824.673 ms | 0.033 ms | 0.551 ms |
| 3 | 10,101 | 0 | 762.810 ms | 0.041 ms | 0.574 ms |

This fixture proves correct counts on one small workload. It does not establish whole-drive speed, throughput on millions of files or superiority to an NTFS scanner.

The entry structure is 80 bytes on this target, before heap-allocated names, vector capacity, the directory stack and physical-identity hash table. This is a structure measurement, not total process memory. The initial native host process showed 32,964,608 bytes of working set after startup; WebView2 processes were not included, so it is not a total application memory result.

## Frontend bundle

After live partial-index publication, the same 10,000-file benchmark measured 1,935.958, 1,385.146 and 1,245.608 ms, with zero errors. This run was slower than the initial baseline. Incremental ancestor updates and synchronization were added, and host/cache conditions were not controlled between runs. Treat the change as an unresolved performance concern, not a measured regression attributable to one cause. Large-workload and paired comparisons remain required.

The first production build produces approximately 70.01 kB of JavaScript and 13.20 kB of CSS, or 27.11 kB and 3.35 kB respectively after gzip. Local logo files are separate. These are frontend bundle sizes, not executable or installer sizes.

## Duplicate fixture

`cargo run --release -p voldiq-core --example duplicate_benchmark` creates 200 real 64 KiB files in four content groups. Their sampled first and last blocks match, while middle bytes distinguish the four groups; full BLAKE3 verification is therefore required. Both passes check four groups, 200 member files and zero errors. On this host the first pass took 266.486 ms and read 14,745,600 bytes. The second pass took 19.845 ms, reused 200 validated hashes and read zero content bytes. Metadata was still checked through newly opened handles.

The fixture was just written, cache conditions are warm, and only two passes were measured. This demonstrates effective content-read avoidance for unchanged files, not a general throughput or speedup claim.

## Snapshot fixture

`cargo run --release -p voldiq-core --example snapshot_benchmark` generates 10,000 real 128-byte files in 100 directories. The database lives in a separate temporary directory. It captures the initial index, captures it unchanged, then doubles ten files and scans again. The runner checks restored totals and a 1,280-byte root growth after restarting entry reads from SQLite.

| Capture | Save | Load | Compare | Entry versions | Database, WAL and SHM |
|---|---:|---:|---:|---:|---:|
| Initial | 91.318 ms | 10.785 ms | 12.295 ms | 10,101 | 1,598,376 B |
| Unchanged | 34.249 ms | 15.003 ms | 17.345 ms | 10,101 | 1,610,736 B |
| Ten files doubled | 42.775 ms | 12.327 ms | 19.038 ms | 10,122 | 1,713,736 B |

The last comparison reports 21 changed entries: ten files, their ten parent directories and the root. File creation and scans are outside the save/load timings. This is one warm fixture run per condition, not a throughput guarantee or a million-file result. Storage includes SQLite journal files before compaction. It proves that unchanged captures reuse entry versions.

The history production bundle is 83.22 kB of JavaScript and 16.20 kB of CSS, or 31.36 kB and 3.84 kB after gzip.

## Million-entry query fixture

`cargo run --release -p voldiq-core --example query_benchmark` builds a synthetic in-memory index of 1,000 directories and 1,000,000 generated file entries. It measures Rust queries only. It does not create a million filesystem files, scan a drive, call native IPC or measure whole-process memory. Each query returns at most 100 entries. Name ranking requests an offset of 500,000, testing a middle page rather than only the largest entries. The generated search has zero matches.

| Query | Full-sort baseline, three runs | Page partition, three runs |
|---|---|---|
| Root children by size | 4.137 / 4.011 / 3.955 ms | 4.001 / 5.486 / 4.074 ms |
| All files by size | 246.718 / 137.665 / 253.327 ms | 28.478 / 34.378 / 28.292 ms |
| Name, offset 500,000 | 2883.448 / 2835.201 / 2847.068 ms | 201.529 / 190.708 / 160.718 ms |
| All files by modification time | 295.668 / 283.421 / 303.477 ms | 36.592 / 36.025 / 34.223 ms |
| Name search, no matches | 71.196 / 71.418 / 72.615 ms | 100.261 / 76.656 / 74.058 ms |

Baseline and final runs use the same deterministic index, in release builds on the host described above. They were separate warm runs without cold-cache controls. An intermediate measurement overlapped functional tests and is excluded from this table. The final run had no parallel test workload. Root filtering and no-match search did not improve consistently. Global ranking avoids a full sort; empty search avoids lowercase allocation; ASCII name comparison avoids per-comparison strings. Matching references still require a temporary linear-sized vector. Deep scopes and populated native interaction remain separate measurements.

## Open performance gates

### Historical bounded descendant query, debug diagnostic

The diagnostic test formerly run with `cargo test -p voldiq benchmark_million_entry_overview -- --ignored --nocapture` was removed on 2026-10-21. The historical measurements below used a synthetic in-memory index with 16 folders and 1,000,000 one-byte file entries. Construction was outside the timed region. Five runs alternated the complete synchronous overview and its descendant helper. Each result was consumed and checked for exact totals, 12 branches, 96 children and stable retained IDs. No disk scan, IPC, Canvas or whole-app memory is measured.

| Current path | Median of five runs | Range | Median thread CPU |
|---|---:|---:|---:|
| Complete overview | 381.740 ms | 377.328–476.004 ms | 375.000 ms |
| Descendant helper | 70.123 ms | 68.854–84.434 ms | 78.125 ms |

The [current raw runs](../benchmarks/overview-million-debug-after-2026-10-14.txt) use the unoptimized development/test profile. [GetThreadTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes) supplies kernel/user CPU ticks for the executing thread. Observed counters advance in 15.625 ms steps, so CPU can slightly exceed a short wall interval. Comparable wall and CPU times support CPU work on one synchronous thread as the dominant cost; they do not distinguish instruction execution from memory stalls.

The helper now rejects candidates that cannot beat the weakest of its retained eight before sorting. Logical-size and stable-ID ordering remain unchanged. [Earlier diagnostic runs](../benchmarks/overview-million-debug-before-2026-10-14.txt) measured the same generated shape before this rejection. The two source versions ran in sequential batches rather than randomized interleaving, and complete-overview ranges overlap. These records therefore do not establish a controlled application speedup. The release measurements above now cover varied sizes and a deeper scope on generated indexes. Concurrent scanning and native rendering remain open gates.

On 2026-10-12, the updated 10,000-file runner verified 10,101 entries, exact logical totals and zero errors in all three runs. Scan times were 699.856, 658.685 and 1,167.019 ms; root page times were 0.078, 0.062 and 0.072 ms; global search times were 0.788, 0.742 and 0.814 ms. The runner measured the current entry layout as 112 bytes. Fixture creation warmed filesystem state; host background work and cache state were not controlled. The spread and historical changes cannot be attributed to a specific optimization. This updates the small-fixture evidence, not the million-file or whole-drive gates.

Cleanup identity baselines increase the measured x64 `Entry` layout from 80 to 112 bytes. This excludes heap allocations and is not total process memory. Historical million-entry query measurements above predate that layout and the additional stable metadata reads; they must not be presented as measurements of the updated scanner. Updated million-entry query measurements remain part of the performance gate.

The pending performance gate includes additional large workloads and broad/deep directory shapes, query latency during indexing, NTFS versus fallback scans, hard-link and sparse-file workloads, total application memory, cold and warm startup, idle CPU, cancellation latency and Canvas frame times. The real-million fixture above establishes one scanner baseline, not these wider gates. No 60 FPS or sub-two-second startup guarantee has been validated yet.
