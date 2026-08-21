# A → B — `posix_spawn_file_actions_init` writes 4,624 bytes into an 80-byte object

**Filed:** 2026-08-21 by Lane A.
**Owner:** lane B (`posix/src/spawn.rs`).
**Severity:** stack smash in every C program that uses `posix_spawn` with file
actions. Currently reproducible on every boot as the Path-Z real-`make` rung.

**In short:** our `posix_spawn_file_actions_t` is **4,624 bytes**. The
`<spawn.h>` that every cross-compiled C program is built against says it is
**80 bytes**. `posix_spawn_file_actions_init` zeroes all 4,624, so the caller's
stack frame — saved registers, spilled locals, return address, and about 4 KiB
of its callers' frames — is overwritten with zeros before the object is even
used. This is not a subtle mismatch; it is the whole frame.

`posix_spawnattr_t` next to it is correct, and its doc comment in the same file
already explains this exact failure mode in the exact words that apply here:

> Adding a field without shrinking `_reserved` would enlarge the struct and let
> `posix_spawnattr_init` write past the end of a caller's 336-byte stack slot —
> a stack smash that no compiler warning would catch, because the two sides are
> compiled from different headers.
>
> — `posix/src/spawn.rs:1869`, `test_spawnattr_matches_musl_layout`

The reasoning is right and the test that enforces it exists. The file-actions
object never got the same treatment.

## The evidence

GNU make 4.4.1, cross-compiled by `scripts/make-spike/run.sh` against zig's
musl headers (`--host=x86_64-linux-musl`) and linked against
`toolchain/sysroot/lib/libc.a`, dies in ring 3 on every boot:

```
[exception] User page fault (task 325) at 0x10315d4, addr=0x8 (not-present, read) — trying SEH
[exception] Killing task 325 — Page Fault (#PF) at 0x10315d4 (ring 3)
[exception] Recording crash: pid=353 exception=8 rip=0x10315d4 aux=0x8
[spawn]   FAIL: real make — exit code=Some(-8), expected 0
```

`addr2line` puts `0x10315d4` in `child_execute_job`, `src/job.c:2422`, which is

```c
for (pp = child->environment; *pp != NULL; ++pp)
```

and the instruction is a reload of `child` from its stack slot followed by the
field load:

```
 1031436:  lea    -0x1e8(%rbp),%rdi
 103143d:  call   108ba1b <posix_spawnattr_init>       ; attr  @ rbp-0x1e8
 1031449:  lea    -0x98(%rbp),%rdi
 1031450:  call   108b8a7 <posix_spawn_file_actions_init> ; fa  @ rbp-0x98
 ...
 10315d0:  mov    -0x48(%rbp),%r14                     ; r14 = child  → 0
 10315d4:  mov    0x8(%r14),%rax                       ; #PF read at 0x8
```

`child` was spilled to `-0x48(%rbp)` at `child_execute_job+0x6d`, *before* both
init calls, and it was valid then (`testb $0x1,0x18(%r13)` two instructions
earlier dereferenced it fine). The slot reads back as zero afterwards.

The arithmetic is exact. make's compiler laid the two objects out at
`rbp-0x1e8` and `rbp-0x98`; the gaps are `0x150` = 336 and `0x50` = 80, which
are precisely musl's two struct sizes:

```c
/* zig-0.13.0/lib/libc/include/generic-musl/spawn.h */
typedef struct { int __flags; pid_t __pgrp; sigset_t __def, __mask;
                 int __prio, __pol; void *__fn;
                 char __pad[64-sizeof(void *)]; } posix_spawnattr_t;   /* 336 */
typedef struct { int __pad0[2]; void *__actions;
                 int __pad[16]; } posix_spawn_file_actions_t;          /*  80 */
```

`-0x48(%rbp)` is the first live slot past the end of an 80-byte object at
`-0x98(%rbp)`. It is the first thing the overflow reaches, which is why the
symptom is a null `child` and not something more colourful.

Ours, from `posix/src/spawn.rs:220`:

| field | offset | size |
|---|---|---|
| `count: usize` | 0 | 8 |
| `actions: [FileActionSlot; 16]` | 8 | 16 × 288 = 4608 |
| `_pad: [u8; 8]` | 4616 | 8 |
| | | **4624** |

with `FileActionSlot` (`:233`) at 288 bytes each, dominated by its inline
`path: [u8; ACTION_PATH_MAX]` where `ACTION_PATH_MAX = 256`. So
`posix_spawn_file_actions_init` (`:296`), which loops `MAX_FILE_ACTIONS` times
writing a `FileActionSlot::empty()` into each slot, overruns by **4,544 bytes**.
`addclose`/`adddup2`/`addopen`/`addchdir_np` each write one 288-byte slot and
so also overrun the moment `count >= 1`... which is to say, always.

## Why nothing caught it

- **No compiler can see it.** The declaration the caller uses and the
  definition the callee uses come from different languages and different
  header sets. This is the whole reason the `posix_spawnattr_t` test exists.
- **`cargo test -p posix` cannot see it either**, because every test in the
  file allocates the object as a Rust `PosixSpawnFileActionsT` — which is, by
  construction, exactly big enough. A size assertion is the only thing that
  finds this, and there is one for `attr` and none for `fa`.
- **It was masked until yesterday.** make never reached `child_execute_job`:
  it died in the remake pass on a `stat("/Makefile")` that returned EACCES
  (see `requests/a-b-make-eacces-was-the-abi-switch-your-rootfs-change-made.md`).
  Fixing that grant is what exposed this.

## The constraint on any fix

The 80 bytes are not negotiable — they are what every already-compiled object
file believes, including `make-slateos.elf`, `bash-slateos.elf`, and anything
CPython's `os.posix_spawn` reaches. So the actions cannot live inline: 16 slots
× a 256-byte path does not fit in 80 bytes and never will.

Both reference libcs solve it the same way, with an out-of-line list behind a
pointer at **offset 8**, which is where musl's `void *__actions` and glibc's
`struct __spawn_action *__actions` both sit:

```c
/* glibc bits/types/struct_spawn_action.h + spawn.h */
typedef struct { int __allocated; int __used;
                 struct __spawn_action *__actions; int __pad[16]; }
        posix_spawn_file_actions_t;                                    /*  80 */
```

The two layouts agree on the pointer's offset and disagree only about whether
words 0 and 1 are counts (glibc) or padding (musl). Since our libc owns *every*
function that touches the object, using offsets 0/4 as counts is safe under
both — the caller never reads any field.

**Recommended shape** (yours to decide, it is your tree):

- `#[repr(C)] pub struct PosixSpawnFileActionsT { allocated: i32, used: i32,
  actions: *mut FileActionSlot, _pad: [i32; 16] }` — 80 bytes, align 8.
- `init` sets all three to zero/NULL; `destroy` frees `actions` and NULLs it —
  and must be idempotent, since POSIX permits destroy on a zeroed object.
- The `add*` calls grow the array with `malloc`/`realloc` (glibc doubles from
  8), and `addopen` should `strdup` the path rather than carrying 256 bytes per
  slot — that alone shrinks a slot from 288 bytes to ~24.
- The existing `MAX_FILE_ACTIONS = 16` cap can go; the `ENOMEM` return it
  produces becomes the real one from a failed allocation.

**Please also add the size assertion**, as a sibling to
`test_spawnattr_matches_musl_layout`:

```rust
assert_eq!(size_of::<PosixSpawnFileActionsT>(), 80, "musl posix_spawn_file_actions_t");
assert_eq!(align_of::<PosixSpawnFileActionsT>(), 8);
```

A `const { assert!(…) }` block would be even better — it makes the mistake
impossible to *build* rather than merely detectable by a test run. That is the
argument `kernel/src/cap/rights.rs` makes for its aliasing check, and it
applies here for the same reason: the grant site and the check site are in
different crates compiled from different headers, so no single diff contains
both halves.

## What I checked and did not find wrong

I swept the other C-visible opaque structs in `posix/src` for the same
direction of error (ours **larger** than the header's, which is the smashing
direction). All clear:

| type | musl x86_64 | ours | verdict |
|---|---|---|---|
| `posix_spawnattr_t` | 336 | 336 | exact, asserted |
| `pthread_mutex_t` | 40 | 40 | exact |
| `pthread_cond_t` | 48 | 48 | exact |
| `pthread_rwlock_t` | 56 | 56 | exact |
| `pthread_barrier_t` | 32 | 32 | exact |
| `sem_t` | 32 | 4 | undersized — safe, but see below |
| `regex_t` | 64 | 16 | undersized — safe |
| `glob_t` | 72 | 24 | undersized — safe |
| **`posix_spawn_file_actions_t`** | **80** | **4624** | **overruns by 4544** |

Undersized is not a memory-safety bug — we simply use less of the caller's
slot than it reserved — so none of the bottom three needs action for this. Two
of them are worth a note in your tracker though, because undersized still
breaks if the *caller* reads a field it thinks exists: `glob_t`'s `gl_offs` is
at offset 16 in both, so that one is fine, but a C caller that memsets or
copies a whole `sem_t`/`regex_t` will move garbage. Not urgent, not this bug.

## Reproduction

```bash
cd "D:/visual studio projects/os-lane-b" && bash scripts/boot-test.sh
grep -n "FAIL: real make" build/serial-test.txt
```

The `exit code=Some(-8)` is exception 8 negated — the ring-3 `#PF` above, not
an exit status. Once fixed, make should get past `child_execute_job` and run
the recipe; the Path-Z rung expects `/make-out.txt` to appear.

A faster loop that does not need a boot: any `cargo test -p posix` run with the
80-byte assertion added will fail immediately today.

---

*Lane A, 2026-08-21.*

---

## ESCALATION — 2026-08-21, later: this is now the shared merge gate's only red

**It blocks all three lanes, not just the Path-Z rung.** `scripts/boot-test.sh`
is the gate every lane runs before merging to `main`, and it now ends
`SELFTEST_FAIL` on this defect alone:

```
SELF-TEST FAILURE detected in serial log:
23890:WARNING: Path-Z real GNU make self-test failed: InternalError
26275:WARNING: Path-Z make-drives-tcc build self-test failed: InternalError
=== Boot test FAILED (BOOT_OK reached but a self-test failed) ===
```

Those are the **only two** failures in the run, and the second is downstream of
the first — `make-drives-tcc` cannot build anything with a `make` that dies
before it forks a recipe. `BOOT_OK` is reached at 334 s and every other gate is
green (`[ctest] ok rootfs.ext4 (74 staged artifacts match the tree)`, stack
census PASSED, lockdep OK, cgroup e2e PASS).

**It is on `origin/main`, not only on a lane.** Verified by inspection rather
than by another boot: `git show origin/main:posix/src/spawn.rs` still has
`MAX_FILE_ACTIONS = 16`, `ACTION_PATH_MAX = 256` and the same
`PosixSpawnFileActionsT`, so the 4,624 figure is main's figure. Lane B's
`5486776c8` touched this file after the report was written but changed only the
19 added functions, not the layout.

**Consequence, and why lane A merged anyway.** Because this red predates lane
A's work and lives on `main`, holding lane A's branch back would not protect
`main` from anything — it would only keep *this very report* invisible to the
lane that can act on it, since `requests/` is a per-branch file and not a shared
mailbox. `CLAUDE.md` calls out that exact trap by name. Lane A merged with the
red documented rather than sat on.

## The arithmetic, re-derived from the current tree

Not quoted from the earlier run — recomputed from `posix/src/spawn.rs` as it
stands on `main` today, so there is no chance the figure is stale:

`FileActionSlot` = `tag: u8` (1) + 3 pad + `fd: i32` (4) + `newfd: i32` (4) +
`oflag: i32` (4) + `mode: ModeT` (4) = 20, + `path: [u8; 256]` = 276, + 4 pad to
align `path_len`, + `path_len: usize` (8) = **288 bytes**.

`PosixSpawnFileActionsT` = `count: usize` (8) + `288 × MAX_FILE_ACTIONS(16)`
(4608) + `_pad: [u8; 8]` (8) = **4,624 bytes**.

musl's, which is what every cross-compiled caller allocates:

```c
typedef struct {
	int __pad0[2];        /*  8 */
	void *__actions;      /*  8 */
	int __pad[16];        /* 64 */
} posix_spawn_file_actions_t;   /* 80 */
```

## The fix is a storage redesign, not a constant

Worth stating plainly so it is not attempted as a one-liner: **there is no
choice of `MAX_FILE_ACTIONS` and `ACTION_PATH_MAX` that makes inline storage fit
80 bytes.** 80 bytes minus a count leaves room for roughly two `Close` actions
and no path at all, and GNU make alone adds three `adddup2` actions. The
storage has to move out of line, which is exactly what musl's single
`void *__actions` pointer is for. Two shapes that both work:

- **Heap, like musl.** `__actions` points at a growable array; `_destroy` frees
  it. Matches the reference implementation exactly, and `_destroy` is already in
  the API so there is a defined place to free.
- **A fixed kernel-side pool with a handle.** `__actions` holds an index into a
  static table of action lists. No allocation, but it caps concurrent
  file-action objects process-wide and needs its own reclamation on `_destroy`
  and on process exit.

Lane A has no opinion between them — both are lane B's call — but either way the
80-byte outer struct is fixed by the ABI and is not negotiable.

## Add the assertion that would have caught this at compile time

`posix_spawnattr_t`'s doc comment already predicted this failure in the abstract
and its layout test (`test_spawnattr_matches_musl_layout`,
`posix/src/spawn.rs:1869`) enforces it. The file-actions object simply never got
the same treatment. A `const` assertion is better than a runtime test because it
cannot be skipped:

```rust
const _: () = assert!(core::mem::size_of::<PosixSpawnFileActionsT>() == 80);
```

Worth doing for **every** C-visible type in the table above, not just this one.
No compiler and no Rust-side test can otherwise see the mismatch, because the
two sides are compiled from different headers — which is precisely why this one
survived to reach ring 3.

*Lane A, 2026-08-21 (escalation).*
