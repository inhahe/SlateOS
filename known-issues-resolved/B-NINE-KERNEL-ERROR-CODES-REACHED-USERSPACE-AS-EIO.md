## B-NINE-KERNEL-ERROR-CODES-REACHED-USERSPACE-AS-EIO (lane B, 2026-08-30) — FIXED

**In short:** the kernel has a list of error numbers, and the POSIX layer has a
hand-written copy of that list saying which C `errno` each one becomes. Nine
numbers had been added to the kernel's list and never copied across. Every one
of them arrived in userspace as `EIO` — "I/O error" — which is a real answer
that other codes legitimately give, so nothing looked broken. Among the nine:
the code that means "a non-blocking connect is still in progress", which a
network client is supposed to read as *keep waiting* and instead read as *this
socket is dead*.

**Where.** `posix/src/errno.rs`, `mod native` and `translate` (now
`errno_for`); and a second, worse copy in `posix/src/socket.rs`,
`translate_net_error`.

**The nine.**

| Code | `KernelError` | Should be | Was | Who produces it |
|---|---|---|---|---|
| -512 | `CrossDevice` | `EXDEV` | `EIO` | `vfs.rs` rename/exchange across a mount, `handle.rs` |
| -513 | `StaleHandle` | `ESTALE` | `EIO` | the pinned `*at` calls 662-664 (lane A, same day) |
| -700 | `ConnectionRefused` | `ECONNREFUSED` | `EIO` | `net/socket.rs:476` |
| -701 | `NotConnected` | `ENOTCONN` | `EIO` | `net/netstack_client.rs` ×5 |
| -702 | `InProgress` | `EINPROGRESS` | `EIO` | non-blocking `connect` |
| -703 | `ConnectAlready` | `EALREADY` | `EIO` | `net/socket.rs:462` |
| -704 | `BrokenPipe` | `EPIPE` | `EIO` | `netstack_client.rs:377` |
| -705 | `AddrInUse` | `EADDRINUSE` | `EIO` | `netstack_client.rs:506` |
| -706 | `MsgSize` | `EMSGSIZE` | `EIO` | `netstack_client.rs` ×4 |

`-401 InvalidCapability` was a tenth, acknowledged in a comment but never
mapped; it is now `EACCES`, the same as an outright denial.

**Why it went unnoticed for so long.** Two reasons, and the second is the one
worth remembering.

1. The header on `mod native` said *"MUST stay in sync with
   kernel/src/error.rs — any mismatch causes wrong errno values throughout the
   entire POSIX layer."* That is exactly right, and it is a comment. Comments
   do not fail builds. Lane A added the `-700` range for the netstack and had
   no reason to know a mirror existed two crates away.
2. `translate` ends in `_ => EIO`. A catch-all cannot distinguish *a code the
   kernel does not define* from *a code the kernel defines and this table has
   not caught up with*, so a missing row does not produce an error, it produces
   a plausible wrong answer. Had the catch-all been `unreachable!()` the first
   `connect` would have said so; had it been a distinct sentinel errno the
   first log line would have. `EIO` is the one choice that hides.

   This is the same shape as the `all-diff.sh` skip bug fixed the same day: a
   default that is indistinguishable from a real result. It is worth treating
   "what does the catch-all arm mean, and can it be told apart from a real
   answer?" as a standing question about any lookup table that mirrors
   something else.

**The second copy.** `socket.rs`'s `translate_net_error` was a full second
transcription of the same enum — eighteen codes behind, including all of the
`-700` range, which exists for sockets and nothing else. It differed from
`translate` in exactly two rows: `AlreadyExists` → `EADDRINUSE` (a socket
creates nothing by name but a local binding) and `InvalidCapability` →
`EACCES`. The second of those was not socket-specific at all, merely present
in one table and absent from the other. So the fix is not "update both": it is
one table, plus the one override that is genuinely about sockets.

**The fix.**

- `translate` split into `errno_for(code) -> i32` (the table) and `translate`
  (which sets errno and returns -1). Nine rows added, plus `-401`.
- `translate_net_error` reduced to `ALREADY_EXISTS => EADDRINUSE, other =>
  errno_for(other)`. It keeps its name and its callers; only the body went.
- Two tests, `kernel_error_codes_are_all_accounted_for` and
  `every_declared_code_is_actually_translated`, which read
  `kernel/src/error.rs` and this file's own source and require every kernel
  discriminant to be either mapped or explicitly written down as
  deliberately-unmapped. A source-text check, because the posix crate cannot
  depend on the kernel crate (different target, `no_std` the other way), so
  there is no type to reflect on.

**Both tests were verified to fail.** Deleting `STALE_HANDLE` and
`CONNECTION_REFUSED` produces

    kernel/src/error.rs defines 2 error code(s) that `mod native` in
    posix/src/errno.rs has never heard of:
      StaleHandle (-513)
      ConnectionRefused (-700)

and adding an unreferenced constant produces `these `native` constants are
declared but never consulted by `errno_for``. The first attempt at the
accounting check *passed* with `STALE_HANDLE` deleted: the section divider
`// --- Filesystem (500 range: -500 to -513) ---` contains the number, and a
range's endpoints are precisely the codes most likely to be the new ones.
Dividers are now excluded from the searched text. A sync check that cannot be
made to fail is not a sync check.

**Not fixed, and deliberately.** Several utilities carry their own small
code → *message* tables — `userspace/chown` (six rows),
`userspace/{mount,mkfs,fsck,diskutil}` (two to five each). They are not errno
translations; they render a diagnostic for the handful of failures that
specific tool expects, and a code they do not list falls through to a generic
"error N" that names the number. That is a legible default rather than a
disguised one, so they are not the same defect. They are still five more
copies of a subset of the same enum, and if one of them ever starts claiming
to be exhaustive it should be pointed at `errno_for` instead.
