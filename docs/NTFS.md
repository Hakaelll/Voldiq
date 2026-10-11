# NTFS reader and integration

The reader uses existing Windows filesystem APIs. It never creates a journal, changes permissions or requests elevation. `VolumeReader` owns its native handle through `std::fs::File`, so early errors and cancellation close handles. FFI is confined to volume discovery, `CreateFileW`, `DeviceIoControl` and `OpenFileById`.

## Implemented reader

The reader resolves the volume mount point and GUID, verifies NTFS, queries journal bounds, and enumerates records with a reusable 256 KiB buffer. It opens metadata by file ID with read-attribute access and reparse-point handling. It reads journal changes only when volume identity, journal ID and saved cursor remain continuous. Record limits and cancellation return distinct states without a completed checkpoint.

The parser reads little-endian fields from checked slices. It requires V2 records, aligned lengths, bounded UTF-16 names and dynamic offsets. V2 minor extensions retain the recorded filename offset. Unsupported major versions return an error for resynchronization or conventional fallback. No binary record is cast to an unchecked Rust pointer.

USN records provide identity, parent and names; they do not supply logical or allocated sizes. The live index obtains sizes from opened metadata handles, reconstructs parents, enumerates hard-link aliases and aggregates actual allocation once per file identity. Returning USN names alone is not a storage scan. See Microsoft's [USN record layout](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_record_v2), [MFT enumeration](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_enum_usn_data), [metadata by file ID](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-openfilebyid) and [hard-link enumeration](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfilenamew).

## Checkpoint persistence

A complete NTFS scan retains its volume serial, journal ID and next USN only when the final journal check confirms no changes during indexing. Cancellation, errors and incomplete coverage do not produce a checkpoint. Conventional scans do not produce one either.

SQLite schema version 2 stores this evidence in `snapshot_journals`, in the same transaction as its snapshot. Volume and journal IDs use eight-byte blobs to preserve the full unsigned range. Each checkpoint belongs to one capture and is removed when that capture is deleted. Version 1 databases migrate transactionally without changing existing captures. Reading persisted evidence verifies the volume identity and complete coverage recorded with the capture.

Loading a snapshot still creates a historical index without an active checkpoint or current file identity stamps. The stored cursor alone does not turn that index into a current one. A separate resume path reconstructs native evidence privately and validates the journal before returning a current index.

Three database tests cover restart, full-width journal IDs, cancellation rollback, deletion, volume mismatch, invalid cursors, incomplete or historical inputs, tampered evidence and version 1 migration. These tests use controlled checkpoint values; they do not certify a privileged raw-volume scan on this host.

## Live journal replay

Repeating a scan of the same volume root with the same exclusions now attempts journal replay when the in-memory NTFS index has a completed checkpoint. The reader verifies volume, journal ID and cursor continuity. The replay engine rereads metadata for changed file references, resolves current names and hard-link aliases, updates moved directories and removes deleted files or directory subtrees. It rebuilds bounded query rows and ancestor totals without rereading unchanged files. Directory entries retain native identities so moves keep their existing descendants attached.

Publication replaces the index only after the complete journal batch, root identity and final unchanged journal bounds pass validation. Candidate construction does not alter the previous index. Cancellation retains the previous published index and labels it as previous data in the scan status; it does not start another scan. Changed journal bounds, lost continuity, access failures and row limits discard the candidate and use the normal scan path. A new included directory requires full enumeration, so it triggers full resynchronization. Moving a directory into an exclusion prunes its descendants while preserving allocation owned by surviving hard-link names.

Four tests use real Windows filesystem fixtures and metadata handles for file growth, deletion, file rename, directory move, new files, deleted subtrees, hard links, exclusions, cancellation, checkpoint reset, final verification failure, row limits and full-scan fallback. One fixture verifies that replay opens exactly the changed references and does not reopen an unchanged descendant. Journal records and checkpoint values are controlled test inputs; these tests do not certify raw journal reads on this host.

Replay currently rebuilds rows in memory, with temporary storage proportional to index size. End-to-end raw-volume and large-index replay measurements remain open. Historical captures are not promoted into current indexes by ordinary history loading.

## Resume after restart

SQLite schema version 3 stores native volume, file reference and change-time stamps in each entry version as a checked 24-byte blob. Adding or changing native evidence creates a new version. Comparisons use identity differences only when both captures have stamps, so migration does not falsely report unchanged legacy files as changed. Versions 1 and 2 migrate in one transaction; old captures remain available, with absent native evidence left unknown.

With no in-memory index, the native scan command searches for the latest completed checkpoint capture matching the requested volume path, native root identity and exclusions. Resume loading validates a directory root, directory-only parents and native entry volumes, and checks cancellation and the requested row limit. The candidate remains private until native journal replay and final root/journal verification succeed. Missing stamps, unavailable journal access, lost continuity or invalid evidence use a full scan. Cancelling before any index is published reports no results and skips fallback.

Two additional replay fixtures reopen SQLite before applying real file growth and directory movement, verify that historical reads expose no active stamps, reject changed final journal bounds, cancellation, oversized candidates, malformed roots/parents and tampered volume evidence, and verify version 2 migration fallback. A persistence fixture detects a real file replacement whose size and modification time match the previous file. The fixtures supply controlled journal records; native startup resume on a journal-readable volume still needs acceptance testing.

## Verification limits

Native hard-link enumeration preserves UTF-16 names, grows bounded buffers for long paths and closes search handles on every exit. A real two-name fixture with a Unicode path longer than 260 characters verifies both aliases and their shared identity. Handle-based final-path resolution is also verified. These helpers are ready for index integration; they do not change the scan method by themselves.

Four parser and continuity tests pass. Five integration tests use real metadata handles and controlled filesystem fixtures to verify size parity, hard-link aliases including an excluded primary name, hierarchy validation, changed identities, missing parents, limits, cancellation and bounded drive-root fallback. The records in these fixtures are constructed from actual file identities; they do not certify raw MFT enumeration.

The native scan command attempts NTFS at volume roots and falls back on capability or enumeration-limit failures before publishing non-root entries. Folder scans use directory enumeration. Metadata is acquired outside index write locks; partial results remain queryable. Unresolved parents, name or identity changes, limits and reparse coverage mark the index incomplete. A changed journal during construction also marks it incomplete.

The host raw-volume probe reports access denied. No elevation or journal modification was attempted. Elevated enumeration, volume performance comparison, real journal replay and native cross-restart acceptance remain open. The current implementation does not claim measured NTFS performance gains.
