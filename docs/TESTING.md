# Testing

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

Five duplicate tests verify full BLAKE3 content, matching samples with different middle bytes, unique-size exclusion, sample-only rejection, validated hash reuse, same-size changes, hard-link deduplication, locked and vanished files, and cancellation before full hashing. All data is generated in temporary fixtures.

## Manual application checks

```powershell
pnpm tauri dev
```

Choose a disposable fixture folder. Check drive capacity, scan progress, root totals, folder navigation, search, extension filtering, size ranking, file details and Explorer reveal. Cancel an in-progress scan and verify that the resulting index is marked incomplete. Scan a folder with an inaccessible child and verify that access issues are visible. Resize the window, collapse navigation, use Ctrl+K and keyboard controls, then check both themes and reduced motion.

Browser preview checks verify layout and preferences only. The preview deliberately has no filesystem bridge. Native interaction checks cannot be replaced with injected browser data.

## Release acceptance

The installer, offline startup, actual native interaction, duplicate handling, cleanup confirmation, snapshots and NTFS fast path must pass before the project is considered complete. A build passing today does not close those pending checks.
