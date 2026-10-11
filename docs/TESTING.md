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

Two query tests verify complete, stable pages over 1,038 actual files, deterministic size/name/date order, case-insensitive filters, zero results beyond the final page, the 500-entry cap and ASCII/Unicode filename ordering.

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
