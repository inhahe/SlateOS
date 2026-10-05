## §279 — Delegating a *subset* of authority to a child gets its own syscall number and a self-describing argument struct, and an impossible request fails the spawn rather than being trimmed

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** §278 fixed a program started with `spawn` receiving *no*
permissions by giving it *all* of its parent's. That is right for a shell
starting a helper it trusts, and wrong for a shell starting a program it does
not: "everything I can do" is not a sandbox. This adds a second way to spawn, in
which the parent lists exactly which permissions the child should get — a subset
of its own, and never more. Three decisions were needed to build it: how a new
field reaches the kernel without breaking the programs already calling the old
one; what shape the list takes; and what happens when a parent asks to hand over
something it does not itself have.

### 1. A new syscall number (559), not a wider `SpawnExArgs`

`SpawnExArgs` is a bare `#[repr(C)]` struct with no length field and no version
field, and `sys_process_spawn_ex` reads `size_of::<SpawnExArgs>()` bytes from
the pointer it is handed. Appending fields to it makes that read run 16 bytes
past the end of every *existing* caller's 96-byte struct — and those bytes are
then interpreted as a user pointer and a count.

The obvious escape is Linux's `clone3` trick: pass the struct's size in a second
register, with `0` meaning "the old, fixed-size struct". That is unavailable
here, and the reason is worth writing down because it is not visible from the
kernel side. `posix::syscall1` (`posix/src/syscall.rs:519`) sets only `rax` and
`rdi` before `syscall`; `rsi` — which the kernel reads as `arg1` — holds
whatever the caller happened to leave there. So `arg1 == 0` is not a reliable
signal of an old caller, it is a coin flip.

| Option | *What changes:* |
|---|---|
| Extend `SpawnExArgs` in place | Every `posix`/`userspace` caller must be recompiled in lockstep across three lanes, or read garbage as a pointer |
| Size in `arg1`, `clone3`-style | Nothing works: `arg1` is uninitialised for existing callers |
| **New number 559 + `SpawnEx2Args`** (chosen) | Old callers keep working untouched; new ones opt in |

`design.txt` mandates versioned syscall tables for exactly this situation, and a
syscall number costs one `u64` in a 1100-entry table.

### 2. `struct_size` is field 0, so there is never a `SPAWN_EX3`

Having paid for a new number, the struct is made self-describing so the next
field is free. `struct_size` leads the struct, which means the kernel can read
it before trusting any other byte:

- **Shorter than this kernel knows** — an older caller. The missing tail is
  zero-filled, and every field's zero value is defined to be its version-1
  behaviour (`SPAWN_CAP_MODE_INHERIT_ALL` is `0` precisely for this).
- **Longer than this kernel knows** — a newer caller. Accepted only if the
  unknown tail is all zero. Otherwise `InvalidArgument`.

That last rule is the one with teeth. The tempting behaviour is to ignore
trailing bytes you have no names for; the trouble is that the fields most likely
to be added to a *spawn* struct are restrictions — `no_new_privs`, a seccomp
filter, a namespace. Silently ignoring a field named `no_new_privs` is how a
sandbox stops being one, and the caller has no way to find out. Failing loudly
turns a silent privilege escalation into a startup error.

The size gate is a separate `const fn` (`spawn::ex2_copy_plan`) rather than
inline in the handler, because it is the part whose failure mode is
memory-safety-adjacent — an off-by-one means copying past the end of the
caller's struct — and inline it would be reachable only from ring 3. As pure
arithmetic on one integer it is swept exhaustively by `test_ex2_copy_plan`.

### 3. The request list reuses `CapEntryInfo`, the type `SYS_CAP_QUERY` returns

The natural way to build a subset is: ask what you hold, drop what the child
should not have, pass the rest. Reusing the enumeration type makes that a
filter. A bespoke request struct would have made it a transcription — and a
transcription step between "what I hold" and "what I delegate" is a place to get
a field wrong in the direction of granting too much.

The cost is that `CapEntryInfo` carries a `_reserved: [u16; 3]` the caller must
zero. It is checked rather than skipped: a reserved field that is never
validated can never later be given a meaning, because by the time you want to
use it, callers are already putting junk in it.

### 4. An unsatisfiable request fails the whole spawn

If the parent asks to delegate something it does not hold — or asks to hand over
`WRITE` on a resource it holds only `READ` on — the entire spawn fails with
`PermissionDenied` and no process is created.

| Option | *What changes:* |
|---|---|
| Silently drop the entries that cannot be satisfied | A process starts, runs for a while, and dies somewhere else for a reason that never mentions the spawn |
| **Fail the spawn** (chosen) | The caller gets an error at the call that was wrong |

The argument for dropping is that it keeps a partly-working child alive. The
argument against is `BUG-SPAWNED-CHILDREN-INHERIT-NO-CAPABILITIES` itself, which
is what a quietly under-privileged child looks like from the outside: `make`
parsed its makefile, spawned `/bin/sh`, and the failure surfaced as
`libc.so.6: cannot open shared object file: Permission denied` from inside
`ld.so`. Nothing in that message names the spawn, and it read as a userspace bug
for a day. The capability list is *caller-written*, so an unsatisfiable entry is
a caller bug, and a caller bug should be reported at the call.

Rights may be **narrowed, never widened**: `inherit_caps_subset` inserts the
*requested* rights, not the parent's, and requires `parent_rights.contains(
requested)`. That asymmetry is what makes this delegation rather than granting.
It also means undefined `Rights` bits need no explicit mask — no parent ever
holds one, so a request for one is already `PermissionDenied`.

### What this does not do

`cap_mode` is validated against the two defined values and an unknown one is
rejected, never clamped to the permissive one. "Unrecognised" and "written
against a newer kernel" are indistinguishable from inside the kernel, and the
safe reading of both is "no".

### How the argument rules are actually tested

Three tests, because the syscall has three layers and no single test reaches
all of them:

| Layer | Test | Runs where |
|---|---|---|
| The size arithmetic (`ex2_copy_plan`) | `spawn::self_test` → `test_ex2_copy_plan` — an exhaustive sweep of every multiple of 8 up to 4096 | pure math, no process |
| The delegation policy | `spawn::self_test` → `test_spawn_capability_subset` | kernel; calls `spawn_process_with_caps` directly |
| The copy-in path — reading `struct_size` from a user pointer, the tail check, the `cap_mode` dispatch, the `CapEntryInfo` decode | `spawn::self_test_spawn_ex2_abi` (`elf::build_spawn_ex2_abi_test_elf`) | **ring 3** |

The third exists because the second cannot reach it: calling
`spawn_process_with_caps` from the kernel skips the entire argument struct,
which is where every rule in this section lives. The probe program pins each
rule down with an accept/reject pair differing in one field, using a
deliberately unmapped `elf_ptr` so that an accepted call returns
`InvalidAddress` while a rejected one returns `InvalidArgument`. That
difference is the whole trick — it makes it possible to tell *which* check
fired, rather than only that something did.

Writing it found a real defect. The subset arm had inherited
`SYS_PROCESS_SPAWN_EX`'s lenience toward null *optional* array pointers, so
`cap_ptr == 0` with `cap_count == 3` silently became "no capabilities" — the
exact silent under-privileging this section argues against, reintroduced by
copied code a few lines from where the argument against it is written down.
Fixed in `0b78b6a01`; probe `0x1A` is now the regression test.
