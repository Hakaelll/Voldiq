# NTFS reader and integration

The reader uses existing Windows filesystem APIs. It never creates a journal, changes permissions or requests elevation. `VolumeReader` owns its native handle through `std::fs::File`, so early errors and cancellation close handles. FFI is confined to volume discovery, `CreateFileW`, `DeviceIoControl` and `OpenFileById`.

## Implemented reader

The reader resolves the volume mount point and GUID, verifies NTFS, queries journal bounds, and enumerates records with a reusable 256 KiB buffer. It opens metadata by file ID with read-attribute access and reparse-point handling. It reads journal changes only when volume identity, journal ID and saved cursor remain continuous. Record limits and cancellation return distinct states without a completed checkpoint.

The parser reads little-endian fields from checked slices. It requires V2 records, aligned lengths, bounded UTF-16 names and dynamic offsets. V2 minor extensions retain the recorded filename offset. Unsupported major versions return an error for resynchronization or conventional fallback. No binary record is cast to an unchecked Rust pointer.

USN records provide identity, parent and names; they do not supply logical or allocated sizes. The live index obtains sizes from opened metadata handles, reconstructs parents, enumerates hard-link aliases and aggregates actual allocation once per file identity. Returning USN names alone is not a storage scan. See Microsoft's [USN record layout](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_record_v2), [MFT enumeration](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_enum_usn_data), [metadata by file ID](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-openfilebyid) and [hard-link enumeration](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfilenamew).

## Checkpoint persistence

A complete NTFS scan retains its volume serial, journal ID and next USN only when the final journal check confirms no changes during indexing. Cancellation, errors and incomplete coverage do not produce a checkpoint. Conventional scans do not produce one either.

SQLite schema version 2 stores this evidence in `snapshot_journals`, in the same transaction as its snapshot. Volume and journal IDs use eight-byte blobs to preserve the full unsigned range. Each checkpoint belongs to one capture and is removed when that capture is deleted. Version 1 databases migrate transactionally without changing existing captures. Reading persisted evidence verifies the volume identity and complete coverage recorded with the capture.

Loading a snapshot still creates a historical index without an active checkpoint or current file identity stamps. The stored cursor alone does not turn that index into a current one. Journal replay, reconstruction of current metadata and publication of a validated refreshed index remain pending.

Three database tests cover restart, full-width journal IDs, cancellation rollback, deletion, volume mismatch, invalid cursors, incomplete or historical inputs, tampered evidence and version 1 migration. These tests use controlled checkpoint values; they do not certify a privileged raw-volume scan on this host.

## Verification limits

Native hard-link enumeration preserves UTF-16 names, grows bounded buffers for long paths and closes search handles on every exit. A real two-name fixture with a Unicode path longer than 260 characters verifies both aliases and their shared identity. Handle-based final-path resolution is also verified. These helpers are ready for index integration; they do not change the scan method by themselves.

Four parser and continuity tests pass. Five integration tests use real metadata handles and controlled filesystem fixtures to verify size parity, hard-link aliases including an excluded primary name, hierarchy validation, changed identities, missing parents, limits, cancellation and bounded drive-root fallback. The records in these fixtures are constructed from actual file identities; they do not certify raw MFT enumeration.

The native scan command attempts NTFS at volume roots and falls back on capability or enumeration-limit failures before publishing non-root entries. Folder scans use directory enumeration. Metadata is acquired outside index write locks; partial results remain queryable. Unresolved parents, name or identity changes, limits and reparse coverage mark the index incomplete. A changed journal during construction also marks it incomplete.

The host raw-volume probe reports access denied. No elevation or journal modification was attempted. Elevated enumeration, volume performance comparison and journal replay remain open. The current implementation does not claim NTFS performance gains or resumable indexing.
