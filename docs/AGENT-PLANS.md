# Agent plan boundary

The core now stores exact recycling manifests. Desktop review, the local approval broker and executor integration are pending. No new agent file mutation endpoint is enabled by this unit.

A manifest identifies its connection, Windows user, source snapshot, root identity, selected file identities, bytes, timestamps, risk, action and expiry. Byte quantities and native identities are strings, preserving exact values in JavaScript clients. Recycling reports zero immediate freed bytes; potential allocation recovery is a separate value and excludes multiply linked files.

The public manifest includes a BLAKE3 hash of private native evidence. That evidence records every parent identity and exact exclusions without exposing ancestor identities or excluded path names in the public response. The manifest itself has a separate BLAKE3 hash. These hashes detect changed evidence; the local database and audit are not claimed to resist the Windows account owner editing them.

Policies are rechecked on every call. An opaque binding includes the current credential digest and complete policy. Rotation, revocation and changed policy invalidate prior authority. Registry, plan store and approver must belong to the same pinned state directory. Known credential locations, Git internals, virtual disks, protected system directories and the state directory are blocked. User-defined notes and descendant protections remain a required later integration before agent execution is enabled.

Plans expire after 15 minutes. A human approval lasts at most two minutes, bounded by the plan expiry. Human approval requires an exclusive OS lease and an in-memory grant bound to the manifest hash, connection and policy. An `approved` SQLite row, even with the current issuer identifier, cannot produce an execution ticket. The agent transport has no approval endpoint.

The lifecycle is `draft`, `awaiting_approval`, `approved`, `executing`, then a terminal result. Rejection, expiry and changed material evidence are terminal. Transactions allow one opaque execution ticket; concurrent or repeated calls report the existing operation. Finishing requires that ticket and its private nonce. Starting a new human session discards old grants. Explicit recovery returns old approvals to review and marks abandoned execution `failed` with `recovery_required`, without repeating uncertain I/O.

The store does not execute files. Before executor integration, the entire selection must pass native preflight under retained handles, and journal entries must be linked to the plan before original files move. The existing desktop executor's ability to proceed with a valid subset must not be reused for an immutable agent selection.

Six owned-fixture tests cover forged database approval, wrong hashes, expiry, restart, changed files, changed hard-link count, principal isolation, rotation, limits, rejection, protected selections and eight competing requests. These tests issue and consume execution tickets without cleaning any user data. They do not establish native UI acceptance or actual approved recycling through CLI/MCP.
