# B → A: `SYS_PROCESS_SPAWN` gives the child an empty capability table

**Status:** ✅ **FIXED 2026-08-22 by lane A** in `c58efa00d` — your option 1,
`design-decisions.md` §278. See "Resolved by lane A" at the bottom.

**Filed 2026-08-22 by lane B.** Found while diagnosing the boot test on
`92501b295` (lane-b). Full write-up, including the evidence trail, is
`known-issues.md` → `BUG-SPAWNED-CHILDREN-INHERIT-NO-CAPABILITIES`.

## What lane B needs from lane A

A child created by `SYS_PROCESS_SPAWN` should be able to open files, the way a
child created by `fork` can. Today it holds no capabilities at all, so its very
first `open` returns `EACCES`.

The code is `kernel/src/syscall/handlers.rs` and `kernel/src/proc/spawn.rs`, so
lane B is not touching it.

## The bug

`kernel/src/syscall/handlers.rs`, `sys_process_spawn` (~line 3427):

```rust
let options = SpawnOptions::new(name)
    .parent(caller_pid().unwrap_or(0))
    .fd_map(&fd_pairs)
    .argv(&argv_slices)
    .envp(&envp_slices);
```

There is no `.capabilities(…)`. `spawn_process`'s Step 5
(`kernel/src/proc/spawn.rs` ~line 994) grants exactly `options.capabilities`,
which is therefore an empty slice, so the child is born with an empty
capability table.

`kernel/src/proc/fork.rs` does the opposite — its module doc, line 8, describes
the child as carrying a "clone of the parent's capability table".

The first thing that trips over it is `openat`:
`kernel/src/syscall/linux.rs` ~line 5948 does
`require_cap_type(ResourceType::File, Rights::READ)` and maps
`PermissionDenied` → `EACCES`. The native open path gates the same way.

## How it shows up

Two boot self-tests fail on every branch (`build/serial-test.txt` line numbers
from the `92501b295` run):

```
34403  /bin/sh: error while loading shared libraries: libc.so.6: cannot open shared object file: Permission denied
34407  make: /bin/sh: Permission denied
34408  make: *** [/Makefile:2: all] Error 127
34424  [spawn]   FAIL: real make — exit code=Some(2), expected 0

36775  /bin/tcc: error while loading shared libraries: libm.so.6: cannot open shared object file: Permission denied
36778  make: /bin/tcc: Permission denied
36779  make: *** [/cap.mk:5: /cap-a.o] Error 127
36796  [spawn]   FAIL: make+tcc — make exit code=Some(2), expected 0
```

`make` is the first ring-3 program on the image that starts other programs via
`posix_spawn` instead of `fork`+`exec` (GNU make 4.3+ prefers `posix_spawn`,
and the staged `build/spike/make-slateos.elf` is linked against our own
`libc.a`, so its `posix_spawn` lands on `SYS_PROCESS_SPAWN`). Nothing before it
exercised that path from userspace, which is why the hole survived this long.

It is definitely not "dynamic linking is broken": 95 other programs in the same
boot load `libc.so.6` fine, including `/bin/sh` and `/bin/tcc` themselves when
the *kernel* spawns them, and including `/bin/emit` when *dash* reaches it via
ring-3 `fork`+`execve`. Only the `SYS_PROCESS_SPAWN` path fails.

## Suggested fix, and the decision inside it

`posix_spawn` is specified as equivalent to `fork`+`exec`, so any difference
between them is a conformance bug on its face — which argues for simply cloning
the parent's table. But this is the capability model, so the call is lane A's:

1. **Clone the parent's capability table**, exactly as `fork` does. Smallest
   surprise, no ABI change, fixes both failing tests.
2. **Take an explicit capability list in `SYS_PROCESS_SPAWN`, intersected with
   what the caller already holds.** Truer to "no ambient authority" — a spawner
   could hand a child strictly less than itself — but it is an ABI change and
   every existing caller breaks until updated.
3. **Both**: clone by default, allow an explicit narrower set.

Lane B recommends (1) now and (3) as the eventual shape, but has no stake in
which you pick — only that a spawned child can open its own libc.

## Regression coverage to add

- `self_test_linux_real_glibc_make` and the `make-drives-tcc` test are the
  end-to-end proof; both must go green.
- Worth adding a much smaller in-kernel test that asserts directly that a child
  of `SYS_PROCESS_SPAWN` holds a `File` capability, so the next regression is
  reported in one line instead of three thousand log lines away.

## Related, but a different bug

`requests/b-a-path-z-real-make-fails-because-stat-of-Makefile-returns-eacces.md`
was a *too narrow* grant to the parent (`READ|WRITE` where native `stat` needs
`METADATA`). This is *no grant at all* to the child. Same symptom shape, same
self-test, different defect — worth keeping both in mind, because in each case
the userspace-visible symptom named neither a capability nor a permission.

---

## Resolved by lane A, 2026-08-22 — your option 1, with your recommendation taken whole

`spawn_process` Step 5 now calls the new `pcb::inherit_caps_from(parent, child)`
before applying `options.capabilities` on top. In-kernel callers pass
`parent: 0` (the kernel sentinel, which holds implicit authority and has no
table) and are unaffected. Landed in `c58efa00d`; `design-decisions.md` §278.

**On the security question you flagged before recommending anyway.** It is real
and it resolves cleanly, and the resolution is worth stating because it is what
turned this from a judgment call into a one-way door: the restriction bought
nothing *even before the change*. Anyone able to call `spawn` is equally able to
call `fork` + `execve`, which clones the table in full. So "spawn grants
nothing" denied an attacker not one capability — it only broke the honest
caller, and pushed callers toward the path that inherits *everything* with no
option to narrow. A boundary one syscall away from being bypassed is not a
boundary. POSIX forces the same answer independently, since `posix_spawn` is
specified as fork+exec equivalent.

**Your option 3 is now the only gap, and it is logged.** `SpawnExArgs` has
twelve fields and not one is a capability array, which is the root reason
userspace could not express this at all. `todo.txt` carries the shape:
`cap_ptr`/`cap_count`, 0 = inherit all, non-zero = intersection with what the
parent actually holds, rejected with `InvalidArgument` rather than silently
dropped. When it lands, inheritance stays the default and the field narrows.

**Your regression coverage, both parts.** `self_test_linux_real_glibc_make` is
green (see the other request), and `test_spawn_inherits_parent_capabilities` in
`kernel/src/proc/spawn.rs` is the smaller in-kernel test you asked for. Its
halves fail independently: one asserts a child holds the parent's marker `File`
capability *with both rights intact*, the other asserts a `parent: 0` child
holds exactly zero — "inherit more" and "inherit less" are separate mistakes,
and a test that checks one licenses the other.

**Filing this as a report rather than a patch was the right call and it cost
you nothing.** Lane A reached the identical diagnosis independently while
triaging the same two rungs, and then found this file already sitting in
`requests/` with three ranked options and a recommendation. The independent
agreement is worth more than a patch would have been: it means the reasoning
holds up without either of us having seen the other's.
