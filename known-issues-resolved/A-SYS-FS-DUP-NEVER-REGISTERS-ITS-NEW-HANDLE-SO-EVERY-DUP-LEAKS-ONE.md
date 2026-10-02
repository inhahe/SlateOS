## A-SYS-FS-DUP-NEVER-REGISTERS-ITS-NEW-HANDLE-SO-EVERY-DUP-LEAKS-ONE

**Status:** FIXED 2026-08-30 (`b51e2d237`) · **Lane:** A · **Filed:**
2026-08-30 · **Severity:** medium, downgraded to low by the Correction below
(unbounded resource leak; also blocked the fix for
`A-FILE-HANDLES-ARE-SEQUENTIAL-AND-UNOWNED-…`)

**How it was fixed.** `sys_fs_dup` now calls
`pcb::register_ipc_handle(pid, ResourceType::File, new_handle)` on the
duplicate, which is the missing half of the pair `sys_fs_close` already
completes by deregistering. Exit cleanup walks `proc.ipc_handles`, so the
duplicate is now reachable by it.

Note this stopped being merely a leak the moment the ownership gate landed in
the same commit: an unregistered handle is one the *caller itself* cannot
use, so `dup` would have returned a handle that every subsequent `read` on it
refused with `InvalidHandle`. That is why the two fixes had to ship together
and why this entry was recorded as blocking the other.

The independent cursor is still deliberate — see the Correction below before
anyone "fixes" this by switching to `dup_shared`.

**In short:** Duplicating an open file — what a program does when it wants a
second, independent way to refer to the same file — creates a brand-new
bookkeeping entry in the kernel, but the kernel never writes down which process
owns it. Cleanup when a program exits works by walking the list of things that
program was recorded as owning, so a duplicate is never on that list and is
never cleaned up. Every duplicate a program makes therefore stays behind after
it exits, for as long as the machine is up.

**The chain, with the specific lines:**

1. `sys_fs_dup` (`kernel/src/syscall/handlers.rs`) calls
   `crate::fs::handle::dup(handle)` and returns the result. It does **not**
   call `pcb::register_ipc_handle` — compare `sys_fs_open_mode`, which does.
2. `fs::handle::dup` (`kernel/src/fs/handle.rs`) is not a refcounting dup: it
   reads the source entry, drops the lock, and calls `allocate_handle` /
   `allocate_dir_handle`, both of which take a fresh number from `NEXT_HANDLE`.
   So the returned value is a genuinely new `OPEN_FILES` entry, not another
   reference to an existing one.
3. Exit cleanup (`pcb.rs:6241-6249`) does
   `core::mem::take(&mut proc.ipc_handles)` and hands *that list* to
   `ipc::cleanup_handles`. Nothing else closes handles at exit.

An entry that was never added at step 1 is not in the list at step 3. It is
therefore never closed, and `OPEN_FILES` grows monotonically across the boot.

**Note the contrast that shows this is an oversight rather than a policy:**
`sys_fs_close` deliberately calls `pcb::deregister_ipc_handle`, with a comment
explaining it is "so the handle is not double-closed by `cleanup_handles` on
process exit." The close path is written with full awareness that
`ipc_handles` drives exit cleanup. The dup path simply never got the matching
registration.

**A second consequence beyond the leak:** the duplicate also survives its
creator, so its number stays live and readable by anyone — which makes this
strictly worse under
`A-FILE-HANDLES-ARE-SEQUENTIAL-AND-UNOWNED-SO-ANY-PROCESS-CAN-READ-ANY-OPEN-FILE`.
A leaked handle to a file a now-dead privileged service had open is exactly
the thing that bug lets an unprivileged process find by counting.

**Proper fix:** in `sys_fs_dup`, register the returned handle to
`caller_pid()` exactly as `sys_fs_open_mode` does:

```rust
if let Some(pid) = caller_pid() {
    pcb::register_ipc_handle(pid, ResourceType::File, new_handle);
}
```

Conditional on `caller_pid()` because kernel-context callers have no pid, which
is the same condition the open path uses.

**Why this must land before the ownership gate**, not after: with the gate in
and this unfixed, a dup'd handle would be unowned and so the very next
`SYS_FS_READ` on it would be refused — turning a leak into a visible breakage
of every program that dups. Fixing the registration first makes the gate a
no-op for correct programs.

**Also unaudited on the same axis:** `SYS_FS_HANDLE_PATH` consumes a handle
with no ownership check and returns the file's full VFS path, which discloses
the path of any open file in the system even to a caller that cannot read it.
Covered by the gate in the other entry; noted here because it is a disclosure
in its own right, not merely a missing check.

### Correction (2026-08-30, same day): libc does not route `dup(2)` through this

Checked after filing. `posix/src/fdtable.rs` (lane B) implements the whole
POSIX dup family in its own fd table — a new fd entry pointing at the *same*
kernel handle — and its module doc states outright that "the kernel's
`SYS_FS_DUP`, which mints a fresh handle with an independent cursor, is
reserved for cases that genuinely need a separate description; the POSIX dup
family does not use it."

Two things follow, and they cut in opposite directions:

- **Severity is lower than filed.** No libc caller reaches `SYS_FS_DUP`, so
  the leak is currently latent rather than active. Downgrade from medium to
  **low-but-real**: it is a live trap for the first native caller, not a leak
  happening today.
- **The independent cursor is deliberate, not a bug.** My first reading was
  that `sys_fs_dup` calling `dup` instead of `dup_shared` broke POSIX `dup(2)`
  offset sharing. It does not, because it is not what implements `dup(2)`.
  `SYS_FS_DUP` is a "clone this open file with a fresh cursor" primitive and is
  correct as such. Recording this explicitly so the next reader does not
  "fix" it by switching to `dup_shared` and silently changing its meaning.

**The fix is unchanged and still worth making** — one `register_ipc_handle`
call — because the leak is a property of the syscall, not of who calls it, and
because it must be in place before the ownership gate lands or that gate will
refuse the first native dup'd handle.

**Worth noting for whoever implements a shared-description dup later:** neither
existing primitive can serve a userspace `dup(2)` on its own. `dup` gives a new
id but an independent cursor; `dup_shared` shares the cursor but returns *the
same id*, which cannot be a distinct fd. A native shared-description dup would
need a new id mapping to an existing `OpenFile` — an id → description
indirection that `OPEN_FILES` (keyed handle → `OpenFile` directly) does not
have today, even though `OpenFile.refcount` is documented for exactly that
sharing. Lane B's fd table sidesteps this by keeping the indirection in
userspace, which is why the gap has not bitten.

### Addendum (2026-08-30): `spawn`'s `fd_map` reasons about this explicitly and gets files wrong

The strongest single piece of evidence that this is a defect rather than a
deliberate trust model is in `kernel/src/proc/spawn.rs:1615-1692`, where an
`fd_map` entry `(fd_num, handle_type, parent_handle)` is duplicated into a
child. The `PTY` arm gates on the parent's ownership and explains why:

> A pty end is the one handle family in this loop whose raw value is
> *guessable*: it is `(tty_id << 1) | end`, so without an ownership check any
> process could name `2` and have the kernel hand its child a live master […]
> Every `SYS_PTY_*` handler gates on `owns_ipc_handle` for exactly this
> reason, and spawn is another way to obtain the handle, so it gates too.

The `FILE` arm, twenty lines above it, is:

```rust
fd_handle_type::FILE => {
    // Duplicate the file handle — child gets an independent copy.
    crate::fs::handle::dup(parent_handle)
}
```

No check. The claim "the one handle family in this loop whose raw value is
guessable" is false: a `FILE` handle is `NEXT_HANDLE.fetch_add(1)` starting at
1, which is *more* predictable than `(tty_id << 1) | end`, not less. So
`spawn` with `fd_map = [(0, FILE, 3)]` hands a child a duplicate of whatever
open file handle 3 happens to be, no matter which process owns it — a second
route to the same authority, reachable by a caller that never issues a single
`SYS_FS_*` call.

This matters for how the fix is scoped: gating the seven fs syscalls is not
sufficient. `spawn`'s `FILE` arm needs the same `options.parent != 0 &&
!owns_ipc_handle(options.parent, ResourceType::File, parent_handle)` guard the
`PTY` arm already has, and the pty comment needs correcting so it stops
asserting files are safe.

**Prerequisite audit is now complete** (the "must be verified first" list in
the main entry above):

| Path that transfers a File handle | Registers to the receiver? |
|---|---|
| `sys_fs_open` / `sys_fs_open_mode` | yes — `register_ipc_handle` on success |
| `fork` (`fork.rs:494-524`) | yes — snapshot, `dup_one` each, `fork_create` takes ownership |
| `spawn` `fd_map` (`spawn.rs:1716`) | yes — `register_ipc_handle(pid, rt, child_handle)` before push |
| `sys_fs_dup` | **no** — see the sibling entry |

So the ownership gate is safe to add once `sys_fs_dup` is fixed: three of the
four transfer paths already maintain the records the gate would read.
