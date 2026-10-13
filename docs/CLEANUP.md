# Cleanup previews

The current delivery supports manual, duplicate, indexed user/project temporary-file and old-file selections with previews and an explicitly confirmed advanced permanent-removal action. Duplicate execution preserves a verified copy per group throughout removal. Recycling is not exposed, and no destructive operation runs automatically. Preview alone never approves execution.

## Project temporary files

The `old-project-temporary-files` rule lists complete regular `.tmp` and `.temp` files at least seven days old under indexed Cargo, Node.js, CMake and .NET build scopes. Build classification requires a complete project-manifest entry. It shares the Developer view's classifier without its page limit. Age and classification do not establish that a file is unused, so nothing is preselected or removed automatically. Risk is `review`, and intended default recycling remains pending; the existing typed advanced permanent confirmation is separate.

Discovery and preview use the same ancestor predicate. Any recognized dependency or configured cache ancestor excludes the file, even when that location is nested inside an outer build directory. Native code supplies current-user tool locations; client parameters cannot authorize arbitrary cache paths. Developer totals still collapse nested artifacts to their outer candidate, while cleanup keeps raw nested exclusions. Files outside recognized build scopes, configured scan exclusions, protected locations, incomplete entries, unsupported names and historical indexes cannot produce an approved plan. Preview also revalidates age, scanned identity/change time, current metadata and every ancestor through the existing manual-plan boundary.

Two real filesystem tests verify all four tool classifications, exact totals, age boundaries, extension case, hard-link estimates, source and unrelated-file exclusion, cache/dependency trees both around and inside build scopes, incomplete manifests, missing/historical/partial scans, malformed selections and changed files. A fixture with 106 projects proves that discovery and preview extend past the Developer view's first page. Verification leaves fixture content intact. Native visual acceptance remains with the user.

## Manual file rule

| Field | Contract |
|---|---|
| ID | `manual-files` |
| Name | Selected regular files |
| Discovery | Explicit IDs from the current native index |
| Selection | 1–500 distinct, complete regular files |
| Exclusions | Historical captures, root directories, reparse points, recall-marked files, excluded ancestors and protected system locations |
| Risk | Review required |
| Permissions | Current user's metadata access and delete access through the verified native handle |
| Planned policy | Windows Recycle Bin, subject to confirmed execution support |
| Verification | Scanned identity/change time, actual metadata, captured ancestors and native handle checks |

Scans retain file identity and native change time alongside entry data. The evidence stays in memory and does not cross the row IPC boundary or become authority for a loaded historical snapshot. Preview creation rejects replacements since scanning even when size and modification time match. Windows protected locations come from `SHGetKnownFolderPath`; resolution failures abort the preview. Protection compares native directory identities so case aliases do not remove it.

The preview reports logical size and conservative potential allocation. A file with multiple hard links contributes zero potential because removing one visible name may leave other links. Moving a file to the Recycle Bin does not itself free that allocation.

## Verification handles

Identity-only lookups open one metadata handle without following reparses or recalling content. They do not require an ancestor's change time to remain unchanged while other files are created beneath it. Selected-file metadata still uses the before/after identity and change-time guard, and execution still validates its complete stamp. A Windows fixture concurrently creates and removes a sibling 2,000 times while reviewing and verifying the unchanged selected file 100 times; the ancestor identity remains stable while its change time advances. This fixes the reproduced parallel-test rejection on the shared temporary directory without relaxing selected-file checks. All 93 workspace tests pass with default parallel execution, as do five further runs of the 29 core unit tests and six cleanup integration tests; strict workspace Clippy passes.

`Plan::verify` opens and pins ancestors from the volume root inward, validates their recorded identities and rejects redirected or recalled directories. Only the filesystem root is opened by an absolute native name. Every subsequent component, including the selected file, is opened by a single child name relative to the previously verified native directory handle. This follows the [NtCreateFile root-directory contract](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile), avoiding another lookup of the full directory path at each step. Parent handles deny delete sharing; the leaf denies write and delete sharing and compares exact preview identity, size, modification time and change time.

The open-only native call uses `FILE_OPEN`, `FILE_OPEN_REPARSE_POINT`, `FILE_OPEN_NO_RECALL` and synchronous I/O. Reparse and recall attributes are checked on each returned object before traversing it. Relative names reject absolute paths, parent traversal, separators, embedded nulls and alternate data-stream syntax. Native failures propagate as I/O errors. Owned Rust file handles close on every exit; no content read, rename or deletion occurs during verification.

On 2026-10-12, two native helper tests and eight related integration tests passed, with strict workspace Clippy. A controlled directory-substitution test deliberately allows its fixture directory to be renamed, then proves that child acquisition still returns the retained original's identity through the existing parent handle. A separate planned-selection test replaces a parent with a junction and rejects it before opening its child, preserving both original and substitute contents. Existing tests still cover replacement before preview, changes after preview, write/rename sharing restrictions, protected locations, temporary rules and duplicate selections. Disk/UNC name conversion is tested; actual network-share acceptance remains pending.

Two additional native helper tests now exercise mount-point mutation after acquiring the directory handle on this NTFS host. For an empty directory, `FSCTL_SET_REPARSE_POINT` succeeds despite the metadata handle remaining open. A normal path read reaches the replacement fixture, but the relative child open returns `ERROR_CANT_RESOLVE_FILENAME` (1921). For a populated directory with its leaf pinned, installing the mount point fails with `ERROR_DIR_NOT_EMPTY` (145); both original and replacement contents remain unchanged. A guard removes any installed mount-point metadata through the same fixture handle before teardown, including during test unwinding. All four helper tests and strict workspace Clippy pass. The byte layout follows Microsoft's [mount-point reparse data specification](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/ca069dad-ed16-42aa-b057-b6b207f447cc). The tests are kept in `src/windows/cleanup_io_tests.rs`, separate from production acquisition code.

These checks do not yet prove a recycling implementation. A later executor must preserve object binding through the native operation; simply closing the handles and calling a path-based deletion API would reopen a race. It must also handle unsupported recycling without silently deleting permanently, preserve recoverability, record partial failure and refresh the index. The controlled post-acquisition mount-point cases cover the tested NTFS configuration; other reparse/filter behavior, network shares and the complete recycle transaction remain pending. Directory handles must not be described as preventing all metadata mutation.

## Interface and command boundary

The virtual explorer and large-file list expose native checkboxes without automatic selection. Review selection requests an immutable backend plan; the modal shows full paths, logical size, potential allocation and hard-link labels. Permanent removal is a separate advanced choice with irreversible wording and an exact `DELETE` confirmation. During execution, Close is disabled and Escape requests cancellation between files. Native dialog behavior supplies modal focus. Populated visual acceptance remains with the user.

The backend retains one plan session. Discard identifies that session, so a stale discard cannot remove a newer plan. Starting a scan or loading a snapshot clears frontend selection and preview. Execution requires the exact session ID, exact confirmation and the same live index allocation; it consumes the session once. The Engine operation guard prevents concurrent analyses. Index and duplicate authority are invalidated before mutations; the frontend clears stale measurements and starts a fresh scan when the result closes.

The permanent executor commits intent before each filesystem operation, verifies and removes the retained native handle, then commits the result. Changed files remain failures; cancellation affects the next unstarted file. Journal failures stop immediately. Already observed outcomes remain in the report, and results that could not be persisted are explicitly marked unrecorded. No retry action is offered. Native `query_cleanup_history` provides bounded read-only audit rows to Cleanup activity, with saved paths, results and operation start times. The interface displays 50 rows per page, uses one extra row to detect a following page and pauses queries during active work. Requests invalidated by revision changes, busy state or unmount cannot replace current results. Pagination restores keyboard focus to the activity region.

Three real owned-fixture executor tests cover removal, changed-file failure, cancellation, pre-cancelled work, duplicate keep-authority rejection and an actual SQLite result-write rejection after deletion. The latter returns the removed-but-unrecorded item, leaves the next file untouched and recovers to unknown/cancelled on reopening. A native command test rejects missing/lowercase confirmations, wrong IDs, rescanned indexes and second consumption. Workspace tests pass; final targeted cleanup tests, Svelte diagnostics, production build and strict Clippy pass. No personal files were deleted for verification, and no rendered interaction is claimed.

## User temporary-file rule

`old-user-temporary-files` discovers the current process user's Windows temporary directory and queries its indexed scope. It admits complete regular files ending in `.tmp` or `.temp`, ignoring case, whose recorded modification time is at least seven days old. Other extensions, recent files and paths outside the temporary scope are excluded. Its descriptor records discovery, selection, exclusions, review risk, permissions, recycle policy and verification. File age does not establish that an application has finished using the file.

Candidates remain unselected until the user chooses them. The native preview rechecks scope, extension and age, then applies the manual plan's scanned identity, metadata, exclusion and protected-path checks. A changed file, historical snapshot or selection outside the rule aborts preview creation. Execution will also need unused-file verification and the complete recycle transaction.

The Cleanup view exposes Scan user temp, bounded candidate pages and Review selection. A temporary directory absent from the current index is shown as Not indexed. An index covering only a child of that directory is labelled Partial. Logical and potential allocation totals cover discovered candidates; hard links and unknown allocation contribute zero potential. Advanced permanent removal requires the separate typed confirmation and is not classified as low-risk or as proof that files are unused.

## Duplicate selection review

Duplicate checkboxes start empty and remain selected across result pages. The frontend and backend both reject selecting every member of a group. The backend rejects selections outside the current analysis and analysis results from an older index. For every touched group it chooses one unselected copy to retain, reads the full content of that copy and every selected file with BLAKE3, and compares the original group digest. These reads bypass the hash cache and check the scanned file identity and native change time. The review can be cancelled during content reads.

The preview shows retained paths under Keep. The confirmed permanent executor repeats retained-copy validation, holds those native handles through the destructive transaction and freshly hashes selected content through its verified removal handle. No recycling command is exposed in this delivery.

Additional cleanup rules, official developer tool commands, confirmed recycling and recycling undo integration are still open work.

## Retained duplicate-copy authority

Duplicate plans now capture the preserved copy's exact file and ancestor stamps together with its group's full BLAKE3 digest. `verify_retained` opens each preserved leaf with read-data access, no DELETE access and only read sharing, retaining its ancestors and leaf in an opaque RAII guard. It rereads the entire content with cancellation checks, compares the digest and validates the stamp again before returning. Error or cancellation releases every acquired handle. The guard provides no removal method.

Three controlled Windows fixture tests cover write/rename/delete exclusion while ordinary reading remains possible, handle release, changed content and identity, cancellation and incorrect digest rejection. A writable file mapping still rejects acquisition with `ERROR_SHARING_VIOLATION` after its original file handle is closed; unmapping and closing the mapping restores acquisition. This matches the [documented write-sharing rule](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea). Other filesystems and filter drivers are not proved by this NTFS fixture.

The confirmed permanent executor acquires every preserved copy before committing operation intent. Selected digests must match preserved-group digests; selected leaves receive read-data and DELETE access with no write/delete sharing. Every selected content read bypasses the hash cache, checks cancellation, and verifies the full stamp before and after hashing. Preserved-copy stamps are checked again before removal; their handles remain owned until execution returns. Failed acquisition releases any earlier copies and removes nothing. Cancellation before or during selected hashing prevents that removal and subsequent unstarted items remain cancelled. A cancelled in-flight item can be recorded failed with its cancellation reason; this never implies removal.

Three executor tests use disposable real Windows files across two distinct content groups. They prove preserved copies reject write/rename/delete attempts during every result callback, selected links alone disappear, successful results survive reopening, a changed second keeper aborts before journal intent and releases the first, cancellation preserves pending files, and changed selected content remains intact. A wrong digest fails a fresh selected-handle read. Existing failure-to-record and partial-outcome tests continue to pass. Native manual interaction, other filesystem/filter behavior and power-loss durability remain unverified.

## Durable operation journal

`cleanup::journal::Journal` records permanent-operation intent and per-file results in a separate, versioned SQLite database. It requires an absolute file path, DELETE journal mode, an application identifier and a supported schema. Foreign and future databases are rejected. Transactions use `synchronous=FULL`; a connection holds [SQLite EXCLUSIVE locking](https://www.sqlite.org/pragma.html#pragma_locking_mode) between commits so another owner cannot classify live work as interrupted.

Preparation commits the rule, explicit permanent policy, timestamp, exact paths and native file identity/change-time blobs plus size and modification time. It accepts the immutable core plan rather than a frontend path list. Per-file transitions are `prepared` → `in_flight` → `succeeded` or `failed`. Starting must commit before filesystem mutation. Failed result writes require the caller to stop; success cannot be inferred from an unrecorded filesystem outcome. Cancellation affects only unstarted items; finishing rejects outstanding items. Repeated transitions, foreign-session operation IDs and stale tokens cannot resume work.

Reopening converts interrupted `in_flight` items to `unknown` and unstarted `prepared` items to `cancelled`, retaining previously recorded results. Recovery updates bookkeeping only. It never deletes, restores or retries a file, and the saved paths/identities do not become new management authority. A fresh scan and confirmation remain necessary for any new action. A finished operation can contain both success and failure; it does not imply total success or recovered allocation.

Four controlled tests use real SQLite files and disposable Windows fixtures. They cover mixed success/failure/cancellation surviving reopen, exact identity serialization, bounded paging, duplicate transition rejection, failure to commit intent, failure to commit a result after actual deletion, replacement-file preservation across repeated recovery, exclusive-owner refusal, numeric rollback and foreign/future database rejection. They simulate interrupted caller state and SQLite write rejection; power-loss/device-failure durability and complete application restart integration are not claimed.

The application initializes this journal at `cleanup.db` in its local data directory. An unavailable journal disables execution through a reported error while analysis remains usable. The confirmed permanent executor and bounded audit query use it. Cleanup activity keeps `in_flight` results unresolved and recovered `unknown` results unknown; neither becomes success. It has no retry, restore or deletion action. The default recycling transaction remains pending.

## Handle-bound permanent-disposition primitive

`VerifiedFile::remove_permanently` is the core primitive used by the explicitly confirmed advanced action. It consumes the retained leaf and ancestor handles, rereads the exact planned identity/size/modification/change-time stamp, rejects read-only files and calls `SetFileInformationByHandle` with SDK `FILE_DISPOSITION_INFO_EX`. Flags request delete, POSIX semantics and an image-section check. There is no filename-based fallback, attribute override or recycling call.

Microsoft documents that [POSIX disposition removes the visible link when the deleting handle closes](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex), while existing handles may retain the data. Consequently this primitive does not report physical space reclaimed or promise that other links disappear. Unsupported filesystems and native failures remain errors.

Four controlled Windows/NTFS fixture tests verify removal of only the approved link, an unchanged retained hard link, continued reads through a preexisting reader, last-link removal while a reader retains data, rejection of substitution before verification, rejection of read-only targets and rejection of metadata changed after verification. Every destructive call targets files freshly created inside the test's owned temporary directory; unrelated fixture files survive. Nine existing cleanup/rule/duplicate-selection integration tests also pass.

The confirmed permanent executor uses this primitive with durable intent/results, cancellation and index invalidation/refresh, including protected duplicate-copy execution. It is an explicit advanced action and cannot undo permanent removal. Recycling remains a separate unresolved handoff and never silently falls back to this primitive.

The metadata recheck and disposition are separate calls. Later attribute changes can still race with disposition, although the retained handle prevents substitution of another file. These tests do not prove that a file is unused or cover every mapped-section, filesystem or filter configuration. The source-only reviews confirmed these limits; the executor does not claim low-risk removal or reclaimed allocation.

## Native recycling handoff investigation

The documented [Shell deletion interface](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-deleteitem) accepts a Shell item rather than an existing file handle. Keeping a handle that denies delete sharing prevents Shell deletion; releasing it before a path-based operation permits substitution. Ancestor handles pin directory names, but do not by themselves prevent reparse metadata changes. A native disposition call would permanently delete and is not a recycling fallback.

`benchmarks/recycle-binding-probe.ps1` tests one possible sealed staging handoff on a disposable x64 Windows fixture. It creates a directory with a restrictive DACL from its first instant, including owner-right restrictions and denial of child creation, attribute writes and security changes. It then attempts a handle-based rename using an already-open verified source and the staging directory handle. No personal files or Recycle Bin items are touched.

On this host, atomic directory creation succeeded and new children were denied. The requested access was `0x001600a2`, but the returned handle had `0x001200a2`, without the requested DACL modification right. Native relative rename returned `STATUS_ACCESS_DENIED` (`0xc0000022`); the Win32 attempt returned error 87. Native ACL restoration through the created handle also returned access denied. The fixture was removed using its parent permissions. This disproves that enrollment approach on this host; it does not prove that every possible Windows design is unavailable.

Creating an initially writable staging directory and sealing it afterward is also insufficient: previously granted mutation handles remain authorized. A future implementation needs a proven namespace handoff and durable recovery before any source relocation. A staged item also needs an original-path restoration mechanism because Shell would record the staging path. Per-item [PostDeleteItem](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-postdeleteitem) results must contain an actual Recycle Bin item before reporting recycling, and aborted operations and partial failures must remain distinct. This investigation has not authorized a destructive executor.

## Shell file-ID binding experiment

Microsoft documents [IFileSystemBindData2](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifilesystembinddata2) as information supplied to Shell parsing, with [SetFileID](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifilesystembinddata2-setfileid) accepting the file index from `BY_HANDLE_FILE_INFORMATION`. That contract does not establish an immutable handle handoff for recycling.

`benchmarks/shell-file-id-probe.ps1` checks whether a supplied original file ID preserves stream binding after namespace substitution. It creates a unique temporary directory with two controlled contents, implements both binding interfaces with their Windows SDK GUIDs and method order, and runs Shell calls on an STA thread. A successful baseline stream read is required. It then moves the original to a retained fixture path and creates a replacement with a distinct native identity at the original name. Both the previously created Shell item and a new item parsed with the stale ID are opened through `BHID_Stream`. A final read tests the original name after removing only the replacement fixture.

On 2026-10-12, Windows `10.0.26200.0` on NTFS produced:

```text
baseline=original
existing_item_stream=replacement
reparsed_stale_id_stream=replacement
missing_path_stream=HRESULT_0x80070002
find_data_reads=2
file_id_reads=2
file_id_writes=0
```

The original and replacement had different native file indices; the retained original's content remained unchanged. The probe records every returned ID and asserts that each equals the original ID, checks the supplied ID before reparsing, and records setter calls. On this run both returned IDs were the original ID and no setter was invoked. The getter counters show the supplied data was consumed during parsing. Nevertheless, these stream bindings followed the replaced name and later failed when that name disappeared. This is a counterexample to treating the supplied ID or parsed Shell item as immutable object authority. It does not measure `IFileOperation.DeleteItem` behavior, nor prove every Windows recycling design impossible.

The probe never invokes a recycling API. Its cleanup checks the resolved temporary prefix, fixture name and absence of a reparse point, deletes only its named fixture files and removes the directory nonrecursively. Reproduce it with:

```powershell
pwsh -NoProfile -File benchmarks/shell-file-id-probe.ps1
```

The next handoff design must establish binding at the actual filesystem operation, retain approved file and ancestor authority, and separately prove per-item recycling and recovery. Identity metadata supplied to Shell parsing is insufficient evidence for enabling the executor.

## Manually selected old files

The `manually-selected-old-files` rule lists complete regular files whose captured modification time is at least 90 days old, within the current live scan. It never selects them automatically or interprets age as proof of disuse. Native protected Windows directory scopes are omitted from discovery; configured scan exclusions remain excluded. Missing timestamps, future timestamps, incomplete entries and historical captures do not produce candidates. Reparse and cloud-recall safety remains enforced by the scanner and fresh preview validation.

The rule uses the same bounded paging, conservative hard-link allocation accounting and immutable manual plan as temporary-file review. Preview rechecks the age criterion, scanned identity, size, modification/change time, current root, exclusions and every ancestor. Personal documents and configuration files can appear for explicit manual review, but no rule automatically removes them. Risk is `review` and the intended default policy is `recycle`, which remains pending. Permanent removal is a separate explicit advanced confirmation.

A controlled real-filesystem test covers the exact 90-day boundary, recent files, document/configuration filenames, exclusions, oldest-first paging, explicit selection, duplicate/empty selection rejection, missing scan, historical captures, partial coverage, clock underflow and a file changed after scanning. Fixture contents survive. Both existing temporary-rule tests also pass. Rendered interaction and destructive execution are not claimed by these checks.

## Read-only Recycle Bin summary

Cleanup also offers **Windows storage**, which opens the official Windows Storage settings page through the documented [`ms-settings:storagesense` URI](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings). Windows owns its cleanup choices and confirmation. The no-argument native command accepts no arbitrary URI, runs Shell activation off the IPC thread, initializes/uninitializes its COM apartment and reports launch errors. It does not execute cleanup, change settings, empty the Recycle Bin or report recovered bytes. Windows changes require a fresh Voldiq scan to update indexed results. Strict workspace Clippy, Svelte diagnostics and production compilation pass; actual Settings activation remains a manual acceptance check.

The Cleanup panel calls [SHQueryRecycleBinW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryrecyclebinw) for the explicitly selected accessible local drive. It uses the Windows SDK structure with `cbSize` initialized and requires `S_OK`. It never reads `$Recycle.Bin` folders, empties the bin, restores items or deletes files. Counts and sizes must be nonnegative and exactly representable by JavaScript; errors remain unavailable results rather than zero totals.

The query runs off the IPC thread. Changing the selected drive or leaving the panel invalidates pending display results. It reports current Shell totals, independently of live or historical scan contents. The reported size does not establish physical allocation or space recovered by emptying the bin.

Three native tests pass, covering numeric boundaries, invalid drive paths and a real read-only query on this Windows host. The live `C:\` query returned zero items and zero bytes. A populated-bin acceptance check remains pending; no user items were created or modified for testing. Emptying, item enumeration, restoration and durable recycling execution remain future work.
