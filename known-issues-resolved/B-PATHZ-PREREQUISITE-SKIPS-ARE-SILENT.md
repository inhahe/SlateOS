### B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT. 26 Path-Z self-test rungs (every `tcc` rung, Parts 35–60) have been no-opping on every boot while the boot test reported PASSED — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/proc/spawn.rs`, `kernel/src/main.rs`, `scripts/boot-test.sh`)

**Verified fixed 2026-08-13.** `rootfs.ext4` was rebuilt with tinycc present and a
full boot test confirms all 26 tcc rungs now run and pass under CR4.SMAP
(compile → glibc-link → ld.so → ring 3, including signal, setjmp, varargs, SSE,
struct-by-value, x87 long double, TLS and ctor/dtor), ending in
`[spawn] Path-Z prerequisites: complete — 0 rungs skipped`. A boot that *does*
lose coverage now prints one `SKIP:` line per rung naming the missing file, a
nonzero summary count, and a `=== PATH-Z COVERAGE INCOMPLETE ===` block from
`boot-test.sh` — so the silence that caused this can no longer recur.

**Symptom.** `rootfs.ext4` contains no TinyCC:

```
$ grep -c TinyCC rootfs.ext4
0
```

and the serial log of a *passing* boot ends its Path-Z sequence at Part 34:

```
[spawn] Running REAL GNU make (ring 3, Path Z) test...
[spawn]   REAL GNU make (...): OK
[hunt] Path-Z checkpoint: ...          <- straight on to the next subsystem
```

Parts 35–60 — every rung that compiles C on the target (`self_test_linux_
real_glibc_cc`, `..._cc_hosted`, `..._cc_hosted_stdio`, `..._cc_separate`,
`..._make_cc`, and the twenty-odd codegen rungs after them: signal, setjmp,
varargs, SSE, struct-by-value, x87 long double, bitfields, indirect call,
computed goto, unions, function-local statics, VLAs, inline asm, `_Atomic`,
statement expressions, `_Generic`, switch jump tables, TLS, ctor/dtor) — print
**nothing at all** and return `Ok(())`.

**Root cause.** Every Path-Z rung opens with a prerequisite guard of the shape

```rust
// kernel/src/proc/spawn.rs:25781
if !crate::fs::Vfs::exists(SRC_TCC) {
    return Ok(());
}
```

There are **40** such guards in `spawn.rs`. They exist for a good reason — the
rungs need artifacts staged into `rootfs.ext4` by `scripts/create-ext4-rootfs.sh`
and the image is optional — but the skip is *silent*: no marker, no counter, no
difference in the boot log between "ran and passed" and "never ran". A rung that
never runs is indistinguishable from a rung that passed, so `boot-test.sh` exits
0 either way.

**Why the artifact went missing.** `create-ext4-rootfs.sh` takes tcc from `PATH`
or from a cached source build at `/tmp/tccinstall/bin/tcc` (tcc is not on a
default Ubuntu install and `apt install tcc` needs root). `/tmp` was cleared at
some point on the WSL build host, so a later rootfs rebuild found neither, hit
the script's `else` branch, and produced an image without `/bin/tcc`. The script
*does* warn at build time — but that warning scrolled past months ago and
nothing downstream ever noticed.

**Consequence beyond the lost coverage.** This silently invalidated the planned
Q43 option-E experiment. B-KNULLJUMP is only ever observed during the
**tcc-signal Path-Z** rung; a 250-boot soak of the current image would have
sampled a population in which the trigger never executes, and a clean result
would have been read — wrongly — as evidence the `B-NO-CLD-ON-INTERRUPT-ENTRY`
fix landed. The soak was caught before it started only because the archived
soak serial log was grepped for `tcc` and came back empty.

**Lesson (the same one as `B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP`,
from the other direction).** There, enforcement that was *off* hid a bug until
it was turned on. Here, a test that was *skipped* hid its own absence. Both are
the same failure of instrumentation: a green result that carries no information.
**A skip must be at least as loud as a failure**, because a silent skip is
strictly worse than a failure — a failure gets investigated.

**Fix.**
1. Route every prerequisite guard through one helper that logs a `[spawn] SKIP:`
   line naming the rung and the missing path, and bumps a counter.
2. Print a summary line after the Path-Z sequence, and make `boot-test.sh`
   surface a nonzero skip count.
3. Rebuild `rootfs.ext4` with `/bin/tcc` staged.
