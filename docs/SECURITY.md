# Filesystem safety

Scanning is read-only and uses the user's current Windows permissions. Voldiq does not elevate the main application or install a resident service.

Reparse points are not traversed. The scanner rejects a reparse-point scan root and records child reparse points as incomplete coverage. Metadata handles request file attributes, allow normal sharing and use `FILE_FLAG_OPEN_REPARSE_POINT`.

Physical allocation comes from Windows file information. If a handle cannot be opened or its metadata cannot be read, the error is reported and the result becomes incomplete.

The webview loads local packaged resources. Content Security Policy excludes remote scripts, frames and objects. The command bridge owns the filesystem paths; the interface cannot execute arbitrary filesystem or shell commands.

No destructive command exists in this first delivery. Cleanup will require backend-owned plans, route and identity validation, critical-directory exclusions, a reviewed selection and explicit confirmation. Tests for destructive operations must use controlled fixtures.

Explorer reveal uses a fixed executable with one argument derived from the index. It does not pass user text through a command interpreter.
