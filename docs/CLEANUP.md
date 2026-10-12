# Cleanup previews

The current delivery supports manual, duplicate, indexed temporary-file and old-file selections with read-only previews. It does not expose recycling or permanent deletion yet. A preview is not approval to execute a destructive operation. The core's handle-bound permanent-disposition primitive remains separate from a confirmed, logged application transaction.

## Manual file rule

| Field | Contract |
|---|---|
| ID | `manual-files` |
| Name | Selected regular files |
| Discovery | Explicit IDs from the current native index |
| Selection | 1–500 distinct, complete regular files |
| Exclusions | Historical captures, root directories, reparse points, recall-marked files, excluded ancestors and protected system locations |
| Risk | Review required |
| Permissions | Current user's metadata access; later execution must obtain delete access |
| Planned policy | Windows Recycle Bin, subject to confirmed execution support |
| Verification | Scanned identity/change time, actual metadata, captured ancestors and native handle checks |

Scans retain file identity and native change time alongside entry data. The evidence stays in memory and does not cross the row IPC boundary or become authority for a loaded historical snapshot. Preview creation rejects replacements since scanning even when size and modification time match. Windows protected locations come from `SHGetKnownFolderPath`; resolution failures abort the preview. Protection compares native directory identities so case aliases do not remove it.

The preview reports logical size and conservative potential allocation. A file with multiple hard links contributes zero potential because removing one visible name may leave other links. Moving a file to the Recycle Bin does not itself free that allocation.

## Verification handles

`Plan::verify` opens and pins ancestors from the volume root inward, validates their recorded identities and rejects redirected or recalled directories. Only the filesystem root is opened by an absolute native name. Every subsequent component, including the selected file, is opened by a single child name relative to the previously verified native directory handle. This follows the [NtCreateFile root-directory contract](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile), avoiding another lookup of the full directory path at each step. Parent handles deny delete sharing; the leaf denies write and delete sharing and compares exact preview identity, size, modification time and change time.

The open-only native call uses `FILE_OPEN`, `FILE_OPEN_REPARSE_POINT`, `FILE_OPEN_NO_RECALL` and synchronous I/O. Reparse and recall attributes are checked on each returned object before traversing it. Relative names reject absolute paths, parent traversal, separators, embedded nulls and alternate data-stream syntax. Native failures propagate as I/O errors. Owned Rust file handles close on every exit; no content read, rename or deletion occurs during verification.

On 2026-10-12, two native helper tests and eight related integration tests passed, with strict workspace Clippy. A controlled directory-substitution test deliberately allows its fixture directory to be renamed, then proves that child acquisition still returns the retained original's identity through the existing parent handle. A separate planned-selection test replaces a parent with a junction and rejects it before opening its child, preserving both original and substitute contents. Existing tests still cover replacement before preview, changes after preview, write/rename sharing restrictions, protected locations, temporary rules and duplicate selections. Disk/UNC name conversion is tested; actual network-share acceptance remains pending.

Two additional native helper tests now exercise mount-point mutation after acquiring the directory handle on this NTFS host. For an empty directory, `FSCTL_SET_REPARSE_POINT` succeeds despite the metadata handle remaining open. A normal path read reaches the replacement fixture, but the relative child open returns `ERROR_CANT_RESOLVE_FILENAME` (1921). For a populated directory with its leaf pinned, installing the mount point fails with `ERROR_DIR_NOT_EMPTY` (145); both original and replacement contents remain unchanged. A guard removes any installed mount-point metadata through the same fixture handle before teardown, including during test unwinding. All four helper tests and strict workspace Clippy pass. The byte layout follows Microsoft's [mount-point reparse data specification](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/ca069dad-ed16-42aa-b057-b6b207f447cc). The tests are kept in `src/windows/cleanup_io_tests.rs`, separate from production acquisition code.

These checks do not yet prove a recycling implementation. A later executor must preserve object binding through the native operation; simply closing the handles and calling a path-based deletion API would reopen a race. It must also handle unsupported recycling without silently deleting permanently, preserve recoverability, record partial failure and refresh the index. The controlled post-acquisition mount-point cases cover the tested NTFS configuration; other reparse/filter behavior, network shares and the complete recycle transaction remain pending. Directory handles must not be described as preventing all metadata mutation.

## Interface and command boundary

The virtual explorer and large-file list expose native checkboxes without automatic selection. Review selection requests an immutable backend plan; the modal shows full paths, logical size, potential allocation and hard-link labels. It contains one Close action. Native dialog behavior supplies modal keyboard focus and Escape dismissal. Its populated visual acceptance remains with the user.

The backend retains one plan session. Discard identifies that session, so a stale discard cannot remove a newer plan. Starting a scan or loading a snapshot clears frontend selection and preview. Before future execution, the backend must also verify that the plan still belongs to the active index and consume the confirmed session once.

## User temporary-file rule

`old-user-temporary-files` discovers the current process user's Windows temporary directory and queries its indexed scope. It admits complete regular files ending in `.tmp` or `.temp`, ignoring case, whose recorded modification time is at least seven days old. Other extensions, recent files and paths outside the temporary scope are excluded. Its descriptor records discovery, selection, exclusions, review risk, permissions, recycle policy and verification. File age does not establish that an application has finished using the file.

Candidates remain unselected until the user chooses them. The native preview rechecks scope, extension and age, then applies the manual plan's scanned identity, metadata, exclusion and protected-path checks. A changed file, historical snapshot or selection outside the rule aborts preview creation. Execution will also need unused-file verification and the complete recycle transaction.

The Cleanup view exposes Scan user temp, bounded candidate pages and Review selection. A temporary directory absent from the current index is shown as Not indexed. An index covering only a child of that directory is labelled Partial. Logical and potential allocation totals cover discovered candidates; hard links and unknown allocation contribute zero potential. No delete or automatic cleanup command is exposed.

## Duplicate selection review

Duplicate checkboxes start empty and remain selected across result pages. The frontend and backend both reject selecting every member of a group. The backend rejects selections outside the current analysis and analysis results from an older index. For every touched group it chooses one unselected copy to retain, reads the full content of that copy and every selected file with BLAKE3, and compares the original group digest. These reads bypass the hash cache and check the scanned file identity and native change time. The review can be cancelled during content reads.

The preview shows retained paths under Keep. This is evidence at preview creation, not a guarantee at a future deletion time. The executor must repeat retained-copy validation and preserve that copy through the destructive transaction. No recycling command is exposed in this delivery.

Additional cleanup rules, compatible cache discovery, developer tool commands, confirmed recycling, result logging and undo integration are still open work.

## Durable operation journal

`cleanup::journal::Journal` records permanent-operation intent and per-file results in a separate, versioned SQLite database. It requires an absolute file path, DELETE journal mode, an application identifier and a supported schema. Foreign and future databases are rejected. Transactions use `synchronous=FULL`; a connection holds [SQLite EXCLUSIVE locking](https://www.sqlite.org/pragma.html#pragma_locking_mode) between commits so another owner cannot classify live work as interrupted.

Preparation commits the rule, explicit permanent policy, timestamp, exact paths and native file identity/change-time blobs plus size and modification time. It accepts the immutable core plan rather than a frontend path list. Per-file transitions are `prepared` → `in_flight` → `succeeded` or `failed`. Starting must commit before filesystem mutation. Failed result writes require the caller to stop; success cannot be inferred from an unrecorded filesystem outcome. Cancellation affects only unstarted items; finishing rejects outstanding items. Repeated transitions, foreign-session operation IDs and stale tokens cannot resume work.

Reopening converts interrupted `in_flight` items to `unknown` and unstarted `prepared` items to `cancelled`, retaining previously recorded results. Recovery updates bookkeeping only. It never deletes, restores or retries a file, and the saved paths/identities do not become new management authority. A fresh scan and confirmation remain necessary for any new action. A finished operation can contain both success and failure; it does not imply total success or recovered allocation.

Four controlled tests use real SQLite files and disposable Windows fixtures. They cover mixed success/failure/cancellation surviving reopen, exact identity serialization, bounded paging, duplicate transition rejection, failure to commit intent, failure to commit a result after actual deletion, replacement-file preservation across repeated recovery, exclusive-owner refusal, numeric rollback and foreign/future database rejection. They simulate interrupted caller state and SQLite write rejection; power-loss/device-failure durability and complete application restart integration are not claimed.

This journal is not yet initialized by the application or exposed through IPC. The confirmed executor, result-history interface, index refresh, duplicate retention and default recycling transaction remain pending.

## Handle-bound permanent-disposition primitive

`VerifiedFile::remove_permanently` is a core primitive for a future explicitly confirmed advanced action. It consumes the retained leaf and ancestor handles, rereads the exact planned identity/size/modification/change-time stamp, rejects read-only files and calls `SetFileInformationByHandle` with SDK `FILE_DISPOSITION_INFO_EX`. Flags request delete, POSIX semantics and an image-section check. There is no filename-based fallback, attribute override, recycling call or IPC executor.

Microsoft documents that [POSIX disposition removes the visible link when the deleting handle closes](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex), while existing handles may retain the data. Consequently this primitive does not report physical space reclaimed or promise that other links disappear. Unsupported filesystems and native failures remain errors.

Four controlled Windows/NTFS fixture tests verify removal of only the approved link, an unchanged retained hard link, continued reads through a preexisting reader, last-link removal while a reader retains data, rejection of substitution before verification, rejection of read-only targets and rejection of metadata changed after verification. Every destructive call targets files freshly created inside the test's owned temporary directory; unrelated fixture files survive. Nine existing cleanup/rule/duplicate-selection integration tests also pass.

This primitive is not a completed cleanup transaction. Before exposing it, the application still needs explicit permanent-deletion confirmation, durable intent/result records, interruption and partial-failure handling, duplicate keep-copy authority and index invalidation/refresh. Recycling remains the intended default and is a separate unresolved handoff. No cleanup action is exposed to users by this change.

The metadata recheck and disposition are separate calls. Later attribute changes can still race with disposition, although the retained handle prevents substitution of another file. These tests do not prove that a file is unused or cover every mapped-section, filesystem or filter configuration. The source-only review found no blocker for this unexposed primitive and confirmed these limits.

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

The rule uses the same bounded paging, conservative hard-link allocation accounting and immutable manual plan as temporary-file review. Preview rechecks the age criterion, scanned identity, size, modification/change time, current root, exclusions and every ancestor. Personal documents and configuration files can appear for explicit manual review, but no rule automatically removes them. Risk is `review` and the intended policy is `recycle`; execution remains unavailable.

A controlled real-filesystem test covers the exact 90-day boundary, recent files, document/configuration filenames, exclusions, oldest-first paging, explicit selection, duplicate/empty selection rejection, missing scan, historical captures, partial coverage, clock underflow and a file changed after scanning. Fixture contents survive. Both existing temporary-rule tests also pass. Rendered interaction and destructive execution are not claimed by these checks.

## Read-only Recycle Bin summary

The Cleanup panel calls [SHQueryRecycleBinW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryrecyclebinw) for the explicitly selected accessible local drive. It uses the Windows SDK structure with `cbSize` initialized and requires `S_OK`. It never reads `$Recycle.Bin` folders, empties the bin, restores items or deletes files. Counts and sizes must be nonnegative and exactly representable by JavaScript; errors remain unavailable results rather than zero totals.

The query runs off the IPC thread. Changing the selected drive or leaving the panel invalidates pending display results. It reports current Shell totals, independently of live or historical scan contents. The reported size does not establish physical allocation or space recovered by emptying the bin.

Three native tests pass, covering numeric boundaries, invalid drive paths and a real read-only query on this Windows host. The live `C:\` query returned zero items and zero bytes. A populated-bin acceptance check remains pending; no user items were created or modified for testing. Emptying, item enumeration, restoration and durable recycling execution remain future work.
