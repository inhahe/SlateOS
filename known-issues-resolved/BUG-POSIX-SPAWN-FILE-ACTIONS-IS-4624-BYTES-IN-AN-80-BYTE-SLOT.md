## BUG-POSIX-SPAWN-FILE-ACTIONS-IS-4624-BYTES-IN-AN-80-BYTE-SLOT (found by lane A, 2026-08-21) — FIXED 2026-08-21 by lane B

**Status:** FIXED — see "Resolution" at the end of this entry. **Owner: lane B**
(`posix/src/spawn.rs`). Filed as
`requests/a-b-posix-spawn-file-actions-init-smashes-the-callers-stack.md`,
which carries the disassembly, the layout table and the recommended fix.

**What.** Our `PosixSpawnFileActionsT` (`posix/src/spawn.rs:220`) is **4,624
bytes** — a `usize` count plus sixteen 288-byte `FileActionSlot`s, each
carrying an inline `[u8; 256]` path. Every C program that uses `posix_spawn`
is compiled against a `<spawn.h>` that declares the same type as **80 bytes**
(musl and glibc agree; only the field names differ). So
`posix_spawn_file_actions_init` zeroes 4,544 bytes past the end of the
caller's object — its spilled locals, its saved registers, its return address,
and roughly 4 KiB of its callers' frames. Every `add*` call writes one
288-byte slot into the same 80 bytes.

**How it shows up.** The Path-Z real-`make` rung. GNU make 4.4.1
(`build/spike/make-slateos.elf`) dies in ring 3 at
`child_execute_job`/`job.c:2422`, `for (pp = child->environment; …)`, because
`child` — spilled to `rbp-0x48`, one slot past an 80-byte file-actions object
at `rbp-0x98` — has been zeroed:

```
[exception] User page fault (task 325) at 0x10315d4, addr=0x8 (not-present, read)
[spawn]   FAIL: real make — exit code=Some(-8), expected 0
```

`exit code=Some(-8)` is exception 8 negated, not an exit status.

**Why nothing caught it.** The two sides are compiled from different headers
in different languages, so no compiler sees both. `cargo test -p posix` cannot
see it either: every test allocates the object as a Rust
`PosixSpawnFileActionsT`, which is by construction big enough. Only a size
assertion finds this — and one *exists* for the sibling `posix_spawnattr_t`
(`test_spawnattr_matches_musl_layout`, `:1877`), whose doc comment spells out
this exact failure mode. The file-actions object never got the same treatment.

**Masked until now.** make never reached `child_execute_job`: it died earlier
in the remake pass on the EACCES covered by
`FIXED-A-PATH-Z-REAL-MAKE-STAT-OF-MAKEFILE-RETURNS-EACCES` above. Fixing that
grant is what exposed this.

**Fix, in outline.** 80 bytes is not negotiable — it is what every compiled
object already believes. The actions must move out of line behind the pointer
at offset 8, where musl's `void *__actions` and glibc's
`struct __spawn_action *__actions` both sit, with `malloc`/`realloc` growth
and a `strdup`'d path instead of 256 inline bytes. Add the 80-byte assertion
as a `const { assert!(…) }` rather than a `#[test]`, for the reason
`kernel/src/cap/rights.rs`'s aliasing check gives: the two halves live in
different crates compiled from different headers, so no single diff contains
both, and it should be impossible to *build* rather than merely detectable on
a test run.

**Audit done at the same time.** Every other C-visible opaque struct in
`posix/src` was checked for the smashing direction (ours larger than the
header's). `posix_spawnattr_t` 336/336, `pthread_mutex_t` 40/40,
`pthread_cond_t` 48/48, `pthread_rwlock_t` 56/56, `pthread_barrier_t` 32/32 —
all exact. `sem_t` (32→4), `regex_t` (64→16) and `glob_t` (72→24) are
*undersized*, which does not smash anything; noted in the request as a
lower-priority correctness point, not a safety one. `posix_spawn_file_actions_t`
is the only one in the dangerous direction.

### Resolution — 2026-08-21, lane B

The struct is now the 80 bytes, with the actions out of line behind the pointer
at offset 8 exactly as musl and glibc do it:

```rust
#[repr(C)]
pub struct PosixSpawnFileActionsT {
    allocated: i32,                 //  0
    used: i32,                      //  4
    actions: *mut FileActionSlot,   //  8
    _pad: [i32; 16],                // 16
}
```

`init` zeroes and NULLs; `destroy` frees and re-NULLs so it is idempotent; the
five `add*` entry points share one `push` that allocates on first use.

**Both guards, not either.** A `const _: () = { assert!(size_of::<…>() == 80); … }`
so the mistake cannot be *built*, which is what lane A asked for and is the
right call — plus `test_file_actions_matches_musl_layout` pinning the offsets
0/4/8/16, which a const block cannot express.

**Extended to the whole table lane A audited**, since its point was that no
Rust-side test can ever see these mismatches. `posix_spawnattr_t`,
`pthread_mutex_t`, `pthread_cond_t`, `pthread_rwlock_t`, `pthread_barrier_t`,
`sem_t`, `regex_t` and `glob_t` all now carry a `const` assertion. The bound is
`size_of::<T>() <= <header size>` rather than `==`, because that is the actual
safety property — **undersized cannot smash, oversized always can** — so it is
true today for the three undersized types *and* still fires the day one of them
grows past its slot. `pthread.rs`'s module doc carries the reasoning once and
the other files point at it.

**Two of lane A's suggestions were not taken**, both because of local facts its
report could not have known; the reasoning is in `push`'s doc comment and in the
reply appended to the request file.

- *The path stays inline at 256 bytes rather than being `strdup`ed.* Shrinking
  the slot from 288 to ~24 bytes is right for a conventional malloc and
  backwards for ours: `posix/src/malloc.rs` is one mapping per allocation
  rounded up to a 16 KiB `REGION_ALIGN`, so a `strdup`ed path costs a **whole
  16 KiB region each**. Sixteen opens would take 256 KiB where the flat array
  takes one region. We are charged for allocation *count*, not slot size.
- *`MAX_FILE_ACTIONS = 16` stays.* The constant is not private to this type: it
  also sizes `OpenedHandles::handles` and backs the assertion
  `MAX_FD_MAP >= 3 + MAX_FILE_ACTIONS` against the **fixed-width fd map passed
  to the kernel's spawn syscall**. Uncapping the list would overrun that map or,
  via `OpenedHandles::push`'s own bounds check, silently *leak* every handle past
  the sixteenth. Lifting the cap is a widening of a kernel interface in lane A's
  tree. Not urgent: glibc's `_add*` also returns `ENOMEM` when it cannot grow, so
  a capped `ENOMEM` conforms, and nothing we ship approaches sixteen.

**Verified** by `cargo test -p posix --target x86_64-pc-windows-gnu` (20 490
tests). The ring-3 proof is the Path-Z real-`make` rung the bug was reported
from, once the sysroot and the four spike ELFs are relinked.

**What fixing it uncovered.** Moving the actions to the heap turned eleven spawn
tests into `ENOMEM` simultaneously, which is how
`B-HOST-MALLOC-NEVER-WORKED-SO-EVERY-ALLOCATING-TEST-PASSED-ON-ITS-OOM-PATH`
below was found. That is a much larger hole than this one and it was invisible
until an allocation was added to a path that had not needed one.
