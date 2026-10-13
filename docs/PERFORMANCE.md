# Performance measurements

## Cooperative query cancellation

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

## Bounded scan concurrency

Settings now saves a real metadata-concurrency limit from 1 to 8. One worker runs directly on the scanning thread; higher values use a persistent scoped pool. Job and result channels each hold at most `workers × 16` items, with batches constrained by remaining index capacity. Directory iteration and index publication stay on the coordinator, and metadata results are applied in enumeration order. This retains exact limits, hard-link allocation deduplication and consistent live queries. Cancellation stops scheduling and publication, drains already started metadata work and joins workers before returning. NTFS enumeration and journal replay keep their existing execution model. No file content is read by this pool.

`cargo run --release -p voldiq-core --example concurrency_benchmark -- 10000` creates 10,000 actual 128-byte files in 100 owned temporary folders. Five rounds rotate the order of 1, 2, 4 and 8 workers. Timing excludes fixture creation and validates exact entry, logical-byte and file totals, known allocation, complete coverage and zero errors after every scan. Windows CPU milliseconds include process user and kernel time; unavailable CPU measurement on other platforms is reported as NaN. [Raw results](../benchmarks/concurrency-2026-10-13.csv) were collected on 2026-10-13 with the i7-1165G7 (4 cores, 8 logical processors), Windows 11, Rust 1.97.1/MSVC and an NVMe CA5-8D256 SSD.

| Workers | Median wall time | Wall range | Median process CPU |
|---:|---:|---:|---:|
| 1 | 1,216.734 ms | 898.289–1,707.280 ms | 1,218.750 ms |
| 2 | 923.460 ms | 573.310–1,118.408 ms | 1,765.625 ms |
| 4 | 533.897 ms | 400.964–612.296 ms | 1,843.750 ms |
| 8 | 368.672 ms | 326.170–508.490 ms | 1,812.500 ms |

Every run measured 10,101 entries, 10,000 files, 1,280,000 logical and allocated bytes, with zero errors. The fixture was newly written and subsequent runs reused warm metadata; this is not disk-bandwidth throughput. More workers reduced elapsed time on this fixture while using more CPU. The host was running the app and development builds, and a before-run CPU-load baseline was unavailable; rotation reduces but does not eliminate that noise. No isolated syscall profile, HDD/USB/network fixture, cold-cache or million-file concurrency comparison has been established. These numbers do not choose a universal fastest setting. The default remains 1 to limit background interference; storage-aware automatic calibration remains pending.

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

On 2026-10-12, the updated 10,000-file runner verified 10,101 entries, exact logical totals and zero errors in all three runs. Scan times were 699.856, 658.685 and 1,167.019 ms; root page times were 0.078, 0.062 and 0.072 ms; global search times were 0.788, 0.742 and 0.814 ms. The runner measured the current entry layout as 112 bytes. Fixture creation warmed filesystem state; host background work and cache state were not controlled. The spread and historical changes cannot be attributed to a specific optimization. This updates the small-fixture evidence, not the million-file or whole-drive gates.

Cleanup identity baselines increase the measured x64 `Entry` layout from 80 to 112 bytes. This excludes heap allocations and is not total process memory. Historical million-entry query measurements above predate that layout and the additional stable metadata reads; they must not be presented as measurements of the updated scanner. Updated million-entry query measurements remain part of the performance gate.

The pending performance gate includes additional large workloads and broad/deep directory shapes, query latency during indexing, NTFS versus fallback scans, hard-link and sparse-file workloads, total application memory, cold and warm startup, idle CPU, cancellation latency and Canvas frame times. The real-million fixture above establishes one scanner baseline, not these wider gates. No 60 FPS or sub-two-second startup guarantee has been validated yet.
