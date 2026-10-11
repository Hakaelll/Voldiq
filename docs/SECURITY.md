# Filesystem safety

Scanning is read-only and uses the user's current Windows permissions. Voldiq does not elevate the main application or install a resident service.

Reparse points are not traversed. The scanner rejects a reparse-point scan root and records child reparse points as incomplete coverage. Metadata handles request file attributes, allow normal sharing and use `FILE_FLAG_OPEN_REPARSE_POINT`.

Duplicate analysis rejects offline and recall-marked files before opening content and rechecks the opened handle. Windows content handles also use `FILE_FLAG_OPEN_NO_RECALL`, following the [native open flag contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew). A controlled fixture sets the offline attribute on two files and verifies zero content bytes read and no duplicate group. This does not certify every third-party cloud provider.

Physical allocation comes from Windows file information. If a handle cannot be opened or its metadata cannot be read, the error is reported and the result becomes incomplete.

The webview loads local packaged resources. Content Security Policy excludes remote scripts, frames and objects. The command bridge owns the filesystem paths; the interface cannot execute arbitrary filesystem or shell commands.

No destructive command exists yet. Cleanup previews now retain backend-owned plans with scanned identity/change-time evidence, native protected-directory exclusions and reviewed selections. Verification handles pin ancestors and block leaf mutation while held. Confirmed recycle execution and concurrent redirection tests remain pending; these preview checks do not certify a deletion transaction. See [cleanup scope and guarantees](CLEANUP.md). Tests for destructive operations must use controlled fixtures.

Explorer reveal uses a fixed executable with one argument derived from the index. It does not pass user text through a command interpreter.
