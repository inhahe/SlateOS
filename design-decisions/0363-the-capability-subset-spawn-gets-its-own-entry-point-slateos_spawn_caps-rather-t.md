## §363 — The capability-subset spawn gets its own entry point, `slateos_spawn_caps`, rather than an attribute on `posix_spawnattr_t`

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Where it bites:** `posix/src/spawn.rs` — `slateos_spawn_caps`, `spawn_impl`,
`spawn_ex2_args`, the `SpawnEx2Args` mirror; `posix/src/syscall.rs`
`SYS_PROCESS_SPAWN_EX2`.

**In short:** The kernel gained a way to start a program holding only *some* of
the permissions the parent holds, instead of all of them (lane A's syscall 559
— see §279 for why it is a new number). Userspace needed a way to ask for that.
The question was where to put the new argument: hidden inside the existing
`posix_spawn` attributes object, or in a new function beside `posix_spawn`. We
added a new function. `posix_spawn` itself is untouched and still hands the
child everything, which is what the POSIX standard requires it to do.

### The options

| | *What changes* |
|---|---|
| **A. New entry point** (chosen) | A caller that wants to narrow permissions calls `slateos_spawn_caps(...)` instead of `posix_spawn(...)`; the extra list is a visible argument. |
| **B. Attribute on `posix_spawnattr_t`** | A caller sets a field on the attributes object it already passes, and keeps calling `posix_spawn`. |

### Why A

**`posix_spawnattr_t` is a value, and a pointer stored in it outlives nothing.**
The object is 336 bytes of plain data with a C ABI (`test_spawnattr_matches_musl_layout`
pins the layout); callers copy it, keep it in a struct, reuse it across several
spawns, and memcpy it. A capability array is variable-length, so the attribute
could only ever be a *pointer plus a count* stored in the padding — and every
one of those ordinary uses would then carry a pointer to a list that may have
been dropped. A dangling pointer smuggled through a struct nobody thinks of as
holding references is the kind of bug that shows up as a spawn with the wrong
authority, which is the exact failure this feature exists to prevent.

**A sandbox request should not be invisible at the call site.** With B, the
difference between "this child gets everything I have" and "this child gets
three capabilities" is a line somewhere earlier that set a field. With A it is
the name of the function being called. Reviewing an attribute-carried policy
means finding every mutation of the attr object; reviewing A means reading the
call.

**POSIX pins the default anyway.** `posix_spawn` is specified as
fork+exec-equivalent, and `fork` gives the child the parent's whole authority,
so `posix_spawn`'s default must stay inherit-everything under either option.
Given that, B's only advantage — reusing a familiar signature — buys nothing a
portable program can use: any code setting the attribute is already SlateOS-only.

**Cost of A, stated honestly.** Two exported symbols where C libraries
conventionally have one, and a second signature to keep in step with
`posix_spawn` if it ever grows a parameter. Mitigated by both being thin
wrappers over one `spawn_impl`, so ELF loading, fd-map construction, argv
packing and the `munmap`/`close_all` cleanup exist once.

### Two smaller calls made inside this one

**`None` goes to syscall 517, not to 559 with `cap_mode == 0`.** Both mean
"inherit everything" and the kernel treats them identically. Routing the
untouched case down the untouched syscall means adding this feature cannot
regress `posix_spawn`, which every existing caller uses; the alternative would
have put every spawn in the system through new code to gain nothing.

**A delegation refusal is reported as `EPERM`, not the table's `EACCES`.** The
shared kernel-error mapping sends `PermissionDenied` to `EACCES`, and
`load_elf` earlier in the same function can also return `EACCES` for a binary
it could not read. On the subset path those two would be indistinguishable, so
a caller whose capability list was rejected would go and look at the file —
a smaller copy of the bug lane A's request describes, where a quietly
under-privileged child surfaced as `libc.so.6: cannot open shared object file`
and was read as a userspace bug for a day. So from `slateos_spawn_caps`,
`EPERM` means "the kernel refused on capability grounds" and `EACCES` means
"the binary was unreadable". The divergence is confined to this entry point;
the shared table and `posix_spawn` are unchanged. The cost is that one kernel
error now has two POSIX spellings depending on which function you called, which
is a real inconsistency — accepted because the alternative is an ambiguity in
precisely the answer this feature exists to make legible.

### What is *not* in the mirror, on purpose

No retry-with-fewer-capabilities loop. The kernel fails the whole spawn if the
parent names authority it does not hold, rather than trimming to the
intersection, and libc does not soften that: the list is caller-written, so an
unsatisfiable entry is a caller bug and belongs at the call site.
