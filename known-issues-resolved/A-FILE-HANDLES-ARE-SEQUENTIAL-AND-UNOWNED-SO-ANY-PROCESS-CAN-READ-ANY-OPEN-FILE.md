## A-FILE-HANDLES-ARE-SEQUENTIAL-AND-UNOWNED-SO-ANY-PROCESS-CAN-READ-ANY-OPEN-FILE

**Status:** FIXED 2026-08-30 (`b51e2d237` the gate, `e601c5bc5` the probe's
buffer) · **Lane:** A · **Filed:** 2026-08-30 · **Severity:** high
(cross-process confidentiality and integrity)

**How it was fixed.** `require_file_handle_owner` in `syscall/handlers.rs`
checks `pcb::owns_ipc_handle(caller, ResourceType::File, handle)` and is
applied to all eight consumers (`read`, `write`, `close`, `seek`, `fstat`,
`ftruncate`, `handle_path`, `dup`) plus `spawn`'s `fd_map` FILE arm. The
refusal is `InvalidHandle`, deliberately not `PermissionDenied`, so the errno
cannot be used as an oracle for "is some other process holding handle N?".
Kernel callers (`caller_pid() == None`) bypass, because they hold handles
that were never registered against a pid and no user process is claiming
authority — the same condition the pty arm uses.

**How we know it is actually fixed**, rather than merely that a refusal was
added: `proc::spawn::self_test_file_handle_ownership` performs the attack
from ring 3 over the native ABI against a handle the kernel holds, and
carries a *control* — it reads its own file first and checks the byte — so a
kernel that refused everything would fail the test rather than pass it. Exit
code `1` is reserved for "the foreign read succeeded" so the security failure
can never be confused with a fixture problem. Verified green on the
2026-08-30 boot: `[spawn]   file handle ownership (ring 3: foreign handle
refused InvalidHandle, own handle still works, fd_map arm gated): OK`, with
process 162 exiting 0 and process 163 logging `Failed to dup handle 59
(type=0) for fd 0: InvalidHandle`.

**The gate has to be a ring-3 test.** It is a deliberate no-op when
`caller_pid()` is `None`, so a kernel-context test would exercise only the
bypass and pass unconditionally.

**In short:** When a program opens a file, the kernel hands it back a number —
a "handle" — and the program uses that number to read and write the file.
Those numbers are handed out in plain counting order: 1, 2, 3, and so on, for
the whole machine rather than per program. Nothing checks that the program
using a number is the one that was given it. So any program can simply try
number 1, number 2, number 3, and read — or overwrite, or truncate — whatever
files other programs happen to have open, including files it would never have
been allowed to open itself.

**Why the design says this must not happen.** `CLAUDE.md` states the rule as
"capability-based security from day one. Every kernel object accessed via
unforgeable handles. No ambient authority." A small counting number that any
program can guess is the textbook example of a *forgeable* handle, and being
able to act on it merely by naming it is the textbook example of *ambient
authority* (permission you get by existing, rather than by holding a token
someone gave you).

**The kernel already knows this rule and already applies it — just not here.**
`pcb::owns_ipc_handle` (`kernel/src/proc/pcb.rs:6430`) exists precisely for
this, and its own doc comment states the premise the file path violates:

> Most IPC handle values in this kernel are treated as self-authorising — the
> handle *is* the capability — **which is sound when the value is
> unguessable.** It is not sound for a `ResourceType::Pty` handle, whose raw
> form is `(tty_id << 1) | end` and therefore trivially enumerable […]

That reasoning is exactly right, and it transfers verbatim to files. A pty
handle is enumerable because it is derived from a small id. A **file** handle
is enumerable because `NEXT_HANDLE` (`kernel/src/fs/handle.rs:191`) is an
`AtomicU64::new(1)` advanced by `fetch_add(1)` — it is *literally* a counter.
The stated soundness condition ("unguessable") is not met, so the
self-authorising treatment is not justified for `ResourceType::File`.

**Measured scope.** No fs syscall that consumes a handle checks either
ownership or a capability. Audited across `kernel/src/syscall/handlers.rs`:

| Syscall | capability check | ownership check |
|---|---|---|
| `sys_fs_read` | no | no |
| `sys_fs_write` | no | no |
| `sys_fs_close` | no | no |
| `sys_fs_seek` | no | no |
| `sys_fs_fstat` | no | no |
| `sys_fs_ftruncate` | no | no |
| `sys_fs_dup` | no | no |

`grep -rn "owns_ipc_handle" kernel/src | grep -i file` returns nothing: the
predicate is used for `Pty` (`handlers.rs:5684`, `spawn.rs:32064`) and in
`spawn.rs:1675`, and never for files.

**The `require_cap_type` gate does not cover this**, which is the part most
likely to be misread. `sys_fs_open_mode` does gate on
`require_cap_type(File, READ)` — but that gates *opening*. An attacker
exercising this bug never opens anything; it calls `sys_fs_read` directly on a
guessed number, and `sys_fs_read` has no gate at all. So holding no File
capability whatsoever is not a defence.

**Reproduce:** from a ring-3 program, loop `h` over 1..64 calling
`SYS_FS_READ(h, buf, 64)` and print every handle that returns > 0. Any file a
service left open — a config, a key, a log being appended — reads back. The
same loop with `SYS_FS_WRITE` or `SYS_FS_FTRUNCATE` corrupts them. (No such
test exists yet; writing it is part of the fix, and it belongs in ring 3 for
the same reason the `openat2` test does — kernel context has no fd table.)

**Proper fix:** gate every handle-consuming fs syscall on
`pcb::owns_ipc_handle(pid, ResourceType::File, handle)`, returning
`InvalidHandle` (**not** `PermissionDenied`) on failure, so the reply cannot be
used to distinguish "someone else's live handle" from "no such handle" — the
same reasoning that puts `beneath_fragment_ok` before the dirfd lookup in
`sys_openat_beneath`. The check must be conditional on `caller_pid()` being
`Some`, because kernel-context callers legitimately hold handles that were
never registered to a pid.

**Why it is not a one-line change**, and what has to be verified first: the
ownership records must already be complete on every path that legitimately
transfers a handle, or the gate will break working programs. Specifically
`fork` (`pcb::snapshot_ipc_handles` refcount-duplicates — looks correct),
`spawn`'s `fd_map`, and `sys_fs_dup` (which returns a *new* handle and must
register it to the caller — worth checking that it does). Each needs
confirming before the gate goes in, and the whole change needs a boot test,
since a false refusal here breaks every program in lanes B and C at once.

**Relationship to `SYS_FS_OPENAT2`** (lane B's pending request): this bug is
load-bearing on that design. Its `dirfd` argument is the *containment base* for
`RESOLVE_BENEATH`, so an unowned handle means a walk confined under a directory
the caller was never granted — which succeeds, returns a valid descriptor, and
looks exactly like working confinement. That is the identical failure mode lane
B asked for the ring-3 marshalling test to rule out, arriving through a
different door. The gate should therefore land before, or with, that syscall.
