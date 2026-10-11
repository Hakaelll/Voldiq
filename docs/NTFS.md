# NTFS reader and integration

The reader uses existing Windows filesystem APIs. It never creates a journal, changes permissions or requests elevation. `VolumeReader` owns its native handle through `std::fs::File`, so early errors and cancellation close handles. FFI is confined to volume discovery, `CreateFileW`, `DeviceIoControl` and `OpenFileById`.

## Implemented reader

The reader resolves the volume mount point and GUID, verifies NTFS, queries journal bounds, and enumerates records with a reusable 256 KiB buffer. It opens metadata by file ID with read-attribute access and reparse-point handling. It reads journal changes only when volume identity, journal ID and saved cursor remain continuous. Record limits and cancellation return distinct states without a completed checkpoint.

The parser reads little-endian fields from checked slices. It requires V2 records, aligned lengths, bounded UTF-16 names and dynamic offsets. V2 minor extensions retain the recorded filename offset. Unsupported major versions return an error for resynchronization or conventional fallback. No binary record is cast to an unchecked Rust pointer.

USN records provide identity, parent and names; they do not supply logical or allocated sizes. The live index integration must obtain sizes from opened metadata handles, reconstruct parents, enumerate all hard-link names and aggregate actual allocation once per file identity. Returning USN names alone is not a storage scan. See Microsoft's [USN record layout](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_record_v2), [MFT enumeration](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_enum_usn_data), [metadata by file ID](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-openfilebyid) and [hard-link enumeration](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfilenamew).

## Verification limits

Four parser and continuity tests pass. The actual host probe reports raw-volume access denied. The conventional temporary fixture still scans correctly. Elevated NTFS enumeration, hierarchy integration, hard-link alias coverage, journal application and volume performance comparison remain pending. The native scan command currently uses directory enumeration.

The next integration stage must retain live partial results, distinguish unsupported capability from indexing errors, and fall back before publishing unrelated IDs. Incomplete or changed journal coverage must never label an old index current.
