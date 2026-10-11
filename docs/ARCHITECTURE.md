# Architecture

Voldiq has one reusable engine crate and one desktop adapter. The engine can run without Tauri through its scan example and integration tests.

## Engine

`scanner` performs an iterative directory walk with cancellation and at most one metadata operation in flight. This conservative I/O policy is the initial baseline, not a claim of optimal performance on every drive. It excludes requested paths and records reparse points without following them.

The index stores compact 32-bit node IDs, parent IDs and names. Full paths are reconstructed only when requested. Each insertion propagates its totals through existing ancestors, so partial directory totals are immediately consistent. Allocation deduplication uses volume identity and file identity. Logical totals retain each path name.

Queries return at most 500 entries. The frontend renders at most 80 table rows per virtual window and requests at most 250 direct children for the Canvas. Unrepresented bytes appear in a smaller-items region linked to the complete virtual list.

Queries filter references into the in-memory index, partition at the requested offset and limit, then sort only the returned page. They retain exact deterministic ranking without sorting every match. Empty searches avoid allocating lowercase names, ASCII name comparisons allocate nothing, and Unicode names retain lowercase ordering. Root scope checks are constant time. Progress is emitted at initialization, the first insertion, every 100 ms and completion.

## Desktop adapter

Tauri exposes volume discovery, native folder selection, scanning, cancellation, bounded index and duplicate queries, details, file types, analysis issues and Explorer reveal. No arbitrary frontend filesystem API or shell command is exposed.

Scanning and queries run in background tasks. The engine publishes one shared index behind `Arc<RwLock<Index>>` before initial progress. Filesystem calls happen outside the write lock; inserting entries and updating ancestors uses a short write section. Queries hold a read guard and return bounded results. The interface refreshes measured partial results at most once per second, plus terminal progress, without polling. A second scan cannot start until the first releases the active operation slot.

## Interface

Svelte owns interaction state, preferences and bounded query results. A custom squarified treemap redraws when geometry, selection or hover changes. There is no persistent animation loop or idle polling. Layout uses Windows system fonts and local artwork.

## Duplicate analysis

`duplicates` filters unique sizes before content reads, samples the first and last 4 KiB for large files, and fully hashes matching samples with a reusable 64 KiB buffer. Small files are read once. Candidate identities exclude hard-link aliases. Windows content handles allow shared readers while excluding writers and deletion, and do not follow final reparse points. Cache stamps include identity, length, modification time and native change time. Hashing validates the opened handle again after reading. Cancellation is checked between candidates and buffer reads.

Duplicate results retain the exact shared scan index that produced their IDs. Groups and members use bounded pages. One active-operation slot prevents scanning and hashing from competing. The in-memory cache has a 250,000-identity cap; persistence remains a snapshot-store task.

## Snapshot history

The bundled SQLite database uses WAL, foreign keys and transactional schema initialization. A root key combines the native root path, filesystem identity and exact exclusion coverage. Stable paths store native name bytes and parent IDs. Entry versions use half-open snapshot intervals; unchanged captures do not copy every entry. Snapshot deletion retains versions required by surviving captures, and compaction checkpoints the WAL and vacuums the database.

Loading reconstructs the Rust index and validates parent order, node limits, filename components and checked integer conversions. Snapshot comparisons page actual additions, omissions and changed directory totals. Incomplete coverage never confirms missing entries as deleted. Database work runs in blocking tasks. The active-operation gate prevents capture and loading from racing a scan or duplicate analysis.

## Pending architecture work

NTFS enumeration, cleanup plans, persistent hash-cache reuse and incremental journal validation remain tracked in `NEXT_TODOS.md`. Large queries can delay index writes while holding their read guard; this is part of the large-workload performance gate. No production behavior is simulated for pending modules.
