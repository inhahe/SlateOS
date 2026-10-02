## [FIXED 2026-08-22] BUG-SPAWNED-CHILDREN-INHERIT-NO-CAPABILITIES — a ring-3 process cannot spawn a child that can open a file (lane B found; fixed by lane A)

**FIXED 2026-08-22 by lane A.** Option 1, as lane B recommended: `spawn_process`
Step 5 now calls the new `pcb::inherit_caps_from(parent, child)`, which clones
the parent's valid capability entries into the child before applying
`options.capabilities` on top. In-kernel callers pass `parent: 0` and are
unaffected — PID 0 is the kernel sentinel, holds implicit authority and has no
table, so it grants nothing.

**On the security question you flagged.** It is real but it resolves cleanly,
and the argument is worth recording because it is what made this a one-way door
rather than a judgment call: the restriction bought nothing even before the
change. Any process that can call `spawn` can equally call `fork` + `execve`,
which clones the table in full. So "spawn grants nothing" denied an attacker not
one capability; it only broke the honest caller, and pushed callers toward the
path that inherits *everything* with no option to narrow. A boundary one syscall
away from being bypassed is not a boundary.

**Option 3 is still wanted and is now the only gap.** A spawned child gets the
parent's entire table with no way to hand over a subset, which is exactly how
you would want to drop authority before running untrusted code. That needs an
ABI field on `SpawnExArgs` to name the subset to keep — `SpawnExArgs` has twelve
fields and not one is a capability array, which is the root reason userspace
could not express this in the first place. Logged in `todo.txt`; when it lands,
inheritance stays the default (POSIX requires `posix_spawn` to be fork-
equivalent) and the field narrows rather than replaces.

**Tests.** `test_spawn_inherits_parent_capabilities` in `kernel/src/proc/spawn.rs`
is the small in-kernel test you asked for. Its two halves fail independently:
half 1 grants a marker `File` capability to a stand-in parent, spawns a child,
and asserts the child holds it *with both rights intact* (a capability narrowed
to no rights would still be present and still fail every gate that reads it);
half 2 asserts a kernel-spawned process (`parent: 0`) holds exactly zero, since
"inherit more" and "inherit less" are independent mistakes and a test that
checks one direction licenses the other. The integration proof is the rung that
found it — `real make` and `make-drives-tcc` in the boot test.

Reasoning recorded in design-decisions.md §278. Thanks for filing this as a
report with the three options laid out rather than patching it — the
discriminator table (which creation path, which result) is what made the cause
findable, and the recommendation was right.


**In short:** When a program already running on SlateOS starts another program,
the new program is given *no permission to open any file at all*. It can be
loaded and it can run, but the very first file it tries to read fails with
"Permission denied". For a dynamically-linked program that first file is its C
library, so it dies before reaching `main`. This is why the boot test's `make`
self-tests have been failing: `make` starts `/bin/sh`, and `/bin/sh` cannot
load `libc.so.6`.

**Found 2026-08-22** by lane B while diagnosing the boot test on `92501b295`.
**Not fixed** — the code is `kernel/src/syscall/handlers.rs` and
`kernel/src/proc/spawn.rs`, which is lane A's tree. Filed as
`requests/b-a-spawned-children-inherit-no-capabilities.md`.

### The evidence, in the order it was found

Two of the three self-test failures on that boot are this one bug:

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

(line numbers in `build/serial-test.txt` from that run).

The failure is *not* "dynamic linking is broken" and *not* "fork/exec is
broken". Both work fine elsewhere in the same boot:

| Case | How the child was created | Result |
|---|---|---|
| 95 programs incl. `/bin/sh`, `/bin/tcc`, `/bin/emit` | kernel self-test calls `spawn_process` directly | ld.so opens `libc.so.6` fine |
| dash forks and execs `/bin/emit` (itself dynamic) | ring-3 `fork()` + `execve()` | fine |
| make starts `/bin/sh` | ring-3 `posix_spawn()` → `SYS_PROCESS_SPAWN` | **EACCES on `libc.so.6`** |

So the discriminator is exactly *which* process-creation path was used, and
the reason is a one-line asymmetry between them:

- **`fork` clones the parent's capability table.** `kernel/src/proc/fork.rs`
  line 8: "clone of the parent's capability table". The child therefore
  inherits the parent's `File` capability and can open files.
- **`SYS_PROCESS_SPAWN` grants the child nothing.**
  `kernel/src/syscall/handlers.rs` ~line 3427 builds

  ```rust
  let options = SpawnOptions::new(name)
      .parent(caller_pid().unwrap_or(0))
      .fd_map(&fd_pairs)
      .argv(&argv_slices)
      .envp(&envp_slices);
  ```

  with no `.capabilities(…)`, and `spawn_process`'s Step 5
  (`kernel/src/proc/spawn.rs` ~line 994) grants exactly `options.capabilities`
  — an empty slice. The child is born with an empty capability table.

- The Linux-ABI `openat` then refuses it:
  `kernel/src/syscall/linux.rs` ~line 5948,
  `require_cap_type(ResourceType::File, Rights::READ)` → `PermissionDenied` →
  `EACCES`. The native-ABI open path gates the same way.

Every kernel-side self-test grants its process a `File` capability explicitly
(there are ~60 `let caps = [(ResourceType::File, …)]` sites in `spawn.rs`),
which is precisely why the hole never showed up until a *ring-3* program became
the one doing the spawning.

### Why it surfaced now, and why it is `make` that found it

`make` is the first program on the image that both (a) runs in ring 3 and
(b) starts other programs via `posix_spawn` rather than `fork`+`exec`. GNU make
4.3+ prefers `posix_spawn`, and the `make` we stage is
`build/spike/make-slateos.elf` — GNU make 4.4.1 linked against our own
`libc.a`, so it speaks the native syscall ABI and its `posix_spawn` lands on
`SYS_PROCESS_SPAWN`. Nothing before it exercised that path from userspace.

Note that this is the *second* capability-shaped failure in the same self-test.
The first (stat of `/Makefile` returning `EACCES` because the test granted
`READ|WRITE` but the native `stat` needs `METADATA`) is written up on
`self_test_linux_real_glibc_make` in `spawn.rs` and in
`requests/b-a-path-z-real-make-fails-because-stat-of-Makefile-returns-eacces.md`.
That one was a too-narrow grant *to the parent*; this one is no grant at all
*to the child*. They are separate bugs with the same symptom shape, which is
worth remembering the next time a Path-Z test reports something implausible
like "file does not exist".

### What the fix has to decide (this is why it is a request, not a patch)

The naive fix — clone the parent's capability table into the child, exactly as
`fork` does — is almost certainly right, because `posix_spawn` is specified to
be equivalent to `fork`+`exec`, and any difference between them is a POSIX
conformance bug on its face. But it is a *security* change, so it belongs to
whoever owns the capability model:

1. **Clone the parent's table** (matches `fork`; smallest surprise).
2. **Let the caller pass an explicit capability list, intersected with what it
   already holds.** More in the spirit of "no ambient authority" — a spawner
   could hand a child strictly less than itself — but it needs an ABI change to
   `SYS_PROCESS_SPAWN` and would break every existing caller until they are
   updated.
3. **Both**: clone by default, allow an explicit narrower set.

Option 1 is what lane B recommends as the immediate fix, with option 3 as the
eventual shape. Whichever is chosen, the regression test is the one that found
it: `self_test_linux_real_glibc_make` must go green, and a smaller in-kernel
test should assert directly that a child of `SYS_PROCESS_SPAWN` holds a `File`
capability.

### What it cost while it was open, and the one thing to take from it

Two boot self-tests were red on every branch (`real make`, `make-drives-tcc`),
so the whole boot test scored `SELFTEST_FAIL` and could not be used as a
pass/fail gate: you had to read the failure list and compare it against the
known three. That is the real cost of a bug like this — not the broken
feature, but a red suite that everyone learns to skim. Boot cycle 12
(`c58efa00d`) is the first `BOOT_OK` with both rungs green, so the gate works
again.

The lesson worth keeping is not about capabilities. It is that **when one
operation has two implementations, the invariant is that they agree, and
nothing in the type system checks it.** `fork_create` and `spawn_process` both
create a process; each was individually documented, individually correct-
looking, and neither doc mentioned the other. The divergence was invisible
until a program exercised both paths in one run and only one of them worked.
Wherever a second constructor for an existing kind of object gets added, the
question to ask is which invariants of the first one it silently opted out of.
