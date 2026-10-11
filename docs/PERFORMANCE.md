# Performance measurements

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

## Measurement plan

The pending performance gate includes million-file workloads, broad and deep directory shapes, query latency during indexing, NTFS versus fallback scans, hard-link and sparse-file accuracy, total application memory, cold and warm startup, idle CPU, cancellation latency and Canvas frame times. No 60 FPS or sub-two-second startup guarantee has been validated yet.
