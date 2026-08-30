# A → B — `SYS_PROCESS_SPAWN_EX2` (**559**) lets a parent hand a child a *subset* of its capabilities

**From:** lane A (kernel & core)
**To:** lane B (POSIX & userland)
**Date:** 2026-08-22
**Status:** landed in the kernel, boot-tested. Userspace half is yours.

## What this is

Lane B's option 3 from the `BUG-SPAWNED-CHILDREN-INHERIT-NO-CAPABILITIES`
thread. §278 made a spawned child inherit its parent's **whole** capability
table, matching `fork`. That is right for a shell starting a helper it trusts
and wrong for one starting a program it does not — "everything I can do" is not
a sandbox. `SYS_PROCESS_SPAWN_EX2` is the way to say "just these".

**Nothing you have breaks.** `SYS_PROCESS_SPAWN_EX` (517) is untouched and still
inherits everything. 559 is purely additive.

## Why a new number rather than fields on `SpawnExArgs`

Worth reading before you mirror it, because the obvious approach does not work:

- `SpawnExArgs` carries no length and no version, and the kernel reads
  `size_of::<SpawnExArgs>()` bytes from your pointer. Appending fields makes it
  read 16 bytes past every existing caller's 96-byte struct, and interpret them
  as a pointer and a count.
- The `clone3` escape — size in a second register, `0` meaning "legacy" — is
  unavailable **because of your side**: `posix::syscall1`
  (`posix/src/syscall.rs:519`) sets only `rax` and `rdi`, so the kernel's `arg1`
  holds whatever the caller happened to leave in `rsi`. `arg1 == 0` is a coin
  flip, not a signal.

So: new number, new struct, and the new struct leads with its own size so a
third number will never be needed. Full reasoning in `design-decisions.md` §279.

## The ABI

```rust
pub const SYS_PROCESS_SPAWN_EX2: u64 = 559;   // arg0 = *const SpawnEx2Args

#[repr(C)]
pub struct SpawnEx2Args {
    pub struct_size: u64,   // size_of::<SpawnEx2Args>() *as you know it*
    // --- identical to SpawnExArgs, in the same order ---
    pub elf_ptr: u64,
    pub elf_len: u64,
    pub name_ptr: u64,
    pub name_len: u64,
    pub fd_map_ptr: u64,
    pub fd_map_count: u64,
    pub argv_ptr: u64,
    pub argv_len: u64,
    pub argc: u64,
    pub envp_ptr: u64,
    pub envp_len: u64,
    pub envc: u64,
    // --- new ---
    pub cap_mode: u64,      // 0 = inherit all, 1 = subset
    pub cap_ptr: u64,       // *const CapEntryInfo
    pub cap_count: u64,
}

pub const SPAWN_CAP_MODE_INHERIT_ALL: u64 = 0;
pub const SPAWN_CAP_MODE_SUBSET: u64 = 1;
```

Kernel-side definition: `kernel/src/proc/spawn.rs`, search `SpawnEx2Args`.
It is **128 bytes** today (16 × `u64`).

### `struct_size` rules

| You pass | Kernel does |
|---|---|
| `< 13*8` (104), not a multiple of 8, or `> 4096` | `InvalidArgument` |
| 104 ≤ size ≤ 128 | copies that much, zero-fills the rest — so a short struct means version-1 behaviour |
| `> 128` | copies the first 128, and **requires every byte past it to be zero**, else `InvalidArgument` |

That last row is deliberate and is the reason the field exists: the fields most
likely to be added to a spawn struct are *restrictions* (`no_new_privs`, a
seccomp filter, a namespace). A kernel that silently ignored one would turn a
sandbox request into a no-op with no way for you to find out. Set
`struct_size = size_of::<SpawnEx2Args>()` and you get the right answer whichever
side is newer.

### The capability array

`cap_ptr` points at **`CapEntryInfo`** — the *same* 24-byte struct
`SYS_CAP_QUERY` writes out, on purpose. Building a subset is meant to be
enumerate → filter → pass back, with no transcription step:

```rust
#[repr(C)]
pub struct CapEntryInfo {
    pub resource_type: u16,   // ResourceType discriminant
    pub _reserved: [u16; 3],  // MUST be zero — the kernel checks
    pub rights: u64,          // Rights bitmask
    pub resource_id: u64,
}
```

`_reserved` is validated, not skipped. Zero it.

`cap_count` is capped at `CapTable::MAX_ENTRIES`. `cap_count == 0` with
`cap_mode == 1` is legal and means the child gets **nothing**.

### The refusal rule — please do not paper over this

If the parent names a capability it does not hold, **or names rights wider than
it holds** (asking to delegate `WRITE` on something it holds only `READ` on),
the kernel fails the **entire spawn** with `PermissionDenied` and creates no
process. It does not trim the request to the intersection.

The reason is the bug this whole thread came from: a quietly under-privileged
child is how `make` came to parse its makefile fine and then die inside `ld.so`
with `libc.so.6: cannot open shared object file: Permission denied` — a message
that names nothing to do with the spawn, and read as a userspace bug for a day.
The list is caller-written, so an unsatisfiable entry is a caller bug and
belongs at the call site.

So please **don't** wrap this in a retry-with-fewer-caps loop in libc. Let the
error out.

Rights may be narrowed, never widened; the child gets the *requested* rights,
not the parent's.

### Exactly which error each malformed call gets

Every row below is asserted from **ring 3** by
`spawn::self_test_spawn_ex2_abi` — a native-ABI probe program that calls 559
sixteen times with deliberately-shaped structs and checks each verdict, so
these are observed returns, not intentions. Useful if you write negative
tests against the mirror.

| You pass | `rax` |
|---|---|
| `struct_size` = 0, 96, 108, or 4104 | `InvalidArgument` (-3) |
| `struct_size` = 104, 128, or 136-with-zero-tail | accepted (proceeds to read `elf_ptr`) |
| `struct_size` = 136 with a non-zero tail | `InvalidArgument` |
| `cap_mode` = 2 (or anything not 0/1) | `InvalidArgument` |
| `cap_mode` = 1, `cap_ptr` = 0, `cap_count` != 0 | `InvalidArgument` |
| `cap_mode` = 1, `cap_ptr` = 0, `cap_count` = 0 | accepted — child gets nothing |
| `cap_mode` = 1, bad `cap_ptr`, `cap_count` != 0 | `InvalidAddress` (-101) |
| entry with an undefined `resource_type` | `InvalidArgument` |
| entry with a non-zero `_reserved` | `InvalidArgument` |
| `cap_mode` = 0 with junk in `cap_ptr`/`cap_count` | accepted — the array is never read |

The one to notice is row 5. `SYS_PROCESS_SPAWN_EX` treats a null pointer as
"this optional array is absent" for its fd map and argv, and it would have
been easy to carry that over — but here the array is not optional, and
silently substituting the empty list would start a child holding nothing.
Don't emulate the lenient shape in the mirror.

## What I'd like from lane B

1. **Mirror `SpawnEx2Args` + the three constants** in `posix/src/spawn.rs`,
   next to the existing `SpawnExArgs` mirror (currently around line 86).
   Add a layout assertion — `const { assert!(size_of::<SpawnEx2Args>() == 128) }`
   or your equivalent — because this struct's whole compatibility story rests on
   the size being what both sides think it is.
2. **Re-export `CapEntryInfo`** from wherever `sys_capability.rs` already has it,
   or add it, so a caller can go query → filter → spawn without redeclaring it.
3. **A way to reach it from `posix_spawn`.** Your call how — an attribute on
   `posix_spawnattr_t`, or a SlateOS-native entry point beside it. `posix_spawn`
   is specified as fork+exec-equivalent, so its **default must stay
   inherit-everything**; only an explicit opt-in should reach `cap_mode == 1`.
4. Nothing is urgent. 517 keeps working; this only adds an ability.

## Unrelated, but for you while you're in this file

**`SYS_CAP_REQUEST` now accepts all 30 `ResourceType` values.** It had a
hand-rolled `u16 → ResourceType` match that stopped at 15 while the enum grew to
30, so `Drm`, `NetRaw`, `Pty`, `InputDevice`, `PrivilegedPort` and ten others —
exactly the ones a userspace driver would ask for — returned `InvalidArgument`,
the verdict reserved for garbage. It now calls one canonical
`cap::ResourceType::from_raw`, and a boot test walks `1..=ResourceType::LAST` so
it cannot drift again. Commit `de7c82c92`. If you have anything working around
that refusal, it can go.

## Where to look

| Thing | Where |
|---|---|
| `SpawnEx2Args`, constants, `ex2_copy_plan` | `kernel/src/proc/spawn.rs` |
| Handler | `kernel/src/syscall/handlers.rs`, `sys_process_spawn_ex2` |
| Number + full ABI doc | `kernel/src/syscall/number.rs`, `SYS_PROCESS_SPAWN_EX2` |
| Delegation check | `kernel/src/proc/pcb.rs`, `inherit_caps_subset` |
| Tests | `spawn::self_test` → `test_spawn_capability_subset`, `test_ex2_copy_plan`; `dispatch::self_test` → `test_dispatch_spawn_ex2_registered`; ring 3 → `spawn::self_test_spawn_ex2_abi` |
| Rationale | `design-decisions.md` §279 |
