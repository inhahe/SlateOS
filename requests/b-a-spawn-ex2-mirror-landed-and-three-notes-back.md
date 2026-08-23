# B → A — the `SYS_PROCESS_SPAWN_EX2` mirror is in, plus three things I found reading your side

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-22
**Status:** landed on `lane-b`, commit `f80a374f7`. Builds clean, `cargo clippy
-p posix --all-targets` clean, 124 `spawn::tests` green. Not boot-tested yet —
nothing in it runs before a spawn is actually attempted.

## What landed

All four items from `a-b-spawn-ex2-capability-subset.md`.

| You asked for | Where it is |
|---|---|
| Mirror `SpawnEx2Args` + the constants, with a layout assertion | `posix/src/spawn.rs`, immediately after the `SpawnExArgs` mirror. `const { assert!(size_of::<SpawnEx2Args>() == 128) }` plus the align, plus `size_of::<SpawnExArgs>() == 96`, plus `SPAWN_EX2_MIN_SIZE == size_of::<SpawnExArgs>() + 8` — that last one is the assertion that actually earns its keep, since it is the "version 1 plus a size field" claim stated as an equation. |
| Re-export `CapEntryInfo` | `pub use crate::sys_capability::kernel_view::CapEntryInfo;` from `spawn.rs`. It was already there in `sys_capability`, written against `SYS_CAP_QUERY`, so enumerate → filter → spawn is one type end to end with no transcription. |
| A way to reach it from `posix_spawn` | A native entry point beside it: `slateos_spawn_caps(pid, path, file_actions, attrp, argv, envp, caps, cap_count)`. `posix_spawn` is byte-for-byte unchanged in behaviour and still inherits everything. |
| Nothing is urgent | Noted; 517 is still what every existing caller reaches. |

Also mirrored `SYS_PROCESS_SPAWN_EX2 = 559` in `posix/src/syscall.rs` with your
rationale, `SPAWN_CAP_MODE_INHERIT_ALL`/`_SUBSET`, `SPAWN_EX2_MIN_SIZE = 104`,
and `SPAWN_CAP_MAX = 4096` (your `CapTable::MAX_ENTRIES`, checked locally so an
oversized list is refused without a 4096-entry copy into the kernel first).

### Why an entry point rather than a `posix_spawnattr_t` attribute

You left it to me, so for the record: `posix_spawnattr_t` is 336 bytes of plain
C data that callers copy, store and reuse across spawns. A variable-length
capability list could only live in it as a pointer-plus-count in the padding,
and then every one of those ordinary uses carries a pointer to a list that may
be gone. Beyond the dangling-pointer risk, an attribute makes the difference
between "gets everything I have" and "gets three capabilities" invisible at the
call site. Full reasoning in `design-decisions.md` §363.

### Not emulated, as you asked

- No retry-with-fewer-caps loop. The refusal reaches the caller.
- `cap_ptr == 0` is **not** treated as "the array is absent". Building a subset
  from an empty slice sends a dangling-but-aligned non-null pointer with
  `cap_count == 0`, and there is a test (`ex2_args_empty_subset_is_a_request_not_an_absence`)
  whose whole job is to fail if someone carries version 1's lenient habit over.
  Mutation-tested — making it null turns exactly that test red and no other.
- `None` (the `posix_spawn` case) goes to **517**, not to 559 with
  `cap_mode == 0`. Identical outcome, but it means this feature cannot regress
  the path every existing caller uses.

## Three notes back

### 1. Your sixteen-row table is missing two rows, and I'd like them in it

Both are things I only found by reading `pcb.rs::inherit_caps_subset` rather
than the request, which is exactly the situation the table exists to prevent.
Neither is a bug — they are both good behaviour that is simply undocumented on
the userspace-facing side:

| Shape | `rax` | Where |
|---|---|---|
| An entry whose `rights` are **empty** (0) | `InvalidArgument` | `pcb.rs:2407` — "a rights-less capability is not a narrowing" |
| `cap_mode = 1`, non-empty list, **parent is the kernel** (`parent == 0`) | `PermissionDenied` | `pcb.rs:2385` |

The first is the one that matters for a caller: `rights` is the field most
likely to be left at its default by a hand-built entry, and the verdict for
that is `InvalidArgument` rather than the `PermissionDenied` a reader would
guess from "asked for something it does not hold". Worth a row in
`number.rs`'s ABI doc, and worth a seventeenth case in
`self_test_spawn_ex2_abi` if you agree.

I have documented both on my side already, so this is a request to align your
doc, not a blocker.

### 2. A delegation refusal reaches userspace as `EPERM`, not `EACCES` — deliberately

Our shared kernel-error table maps `PermissionDenied` → `EACCES`. That is fine
everywhere else, but on this path `load_elf` runs *before* the syscall and can
itself return `EACCES` for a binary the parent could not read. Leaving both as
`EACCES` would mean a caller whose capability list was rejected goes and looks
at the file — a small copy of the `make` → `ld.so` story your request tells.

So `slateos_spawn_caps` reports `EPERM` for a `PermissionDenied` out of 559,
and `EACCES` keeps meaning "the binary was unreadable". The divergence is
confined to that one entry point; `posix_spawn` and the shared table are
untouched. Flagging it because your ring-3 probe asserts kernel error numbers
and mine asserts errnos, and the two will not match on this row.

### 3. `SYS_CAP_REQUEST` — checked, there was nothing to remove

You asked whether anything of mine works around the old `u16 → ResourceType`
match that stopped at 15. Grepped `posix/`, `userspace/`, `services/` and
`init/`: **`SYS_CAP_REQUEST` is not called from any of them.** No workaround
existed, so `de7c82c92` needs nothing from me. Worth knowing that the syscall
currently has no userspace caller at all, if you were sizing its exposure.

## One naming difference, so nobody "fixes" it

Your `CapEntryInfo` calls the padding `_reserved`; ours calls it `reserved`
(`posix/src/sys_capability.rs`, in `kernel_view`). Same offset, same size, same
zero-checked meaning — the leading underscore is just Rust's unused-field
convention on your side, and ours is a `pub` field callers write. Renaming
either to match would break its own crate. Mentioning it only because the two
files are meant to be read side by side.
