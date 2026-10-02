### TD-A-PCPU-REFILL-AND-DRAIN-REQUIRE-INTERRUPTS-OFF-BUT-NOTHING-CHECKS-IT — 2026-08-25 — ✅ **FIXED** 2026-08-25

**In short:** Two functions in the page allocator only work if the caller has
already turned interrupts off. That requirement is written in a comment and
nowhere else, so a future caller that forgets it introduces a hang that leaves
no trace — the machine simply stops, with nothing on the serial line.

**Where.** `kernel/src/mm/frame.rs`, `fn pcpu_refill` (~line 1184) and
`fn pcpu_drain` (~line 1215). Both open with *"Called with interrupts disabled
and the global lock NOT held"*, then take `allocator.lock()` bare — the only two
of the sixteen `ALLOCATOR` acquisitions in the file that are not wrapped in
`crate::cpu::without_interrupts`. They also index `PCPU_CACHES[cpu]` through
`unsafe`, whose `// SAFETY:` comment cites the same unenforced premise
("interrupts are disabled so no preemption").

**Why it matters.** `frame::stats`'s doc comment records that this exact
class already froze a boot once: *"the frag_history self-test hang in
known-issues.md (observed 2026-06-07 during the post-F1/F2/F3 soak,
`build/soak-hang-run18.txt`)"*. A same-CPU interrupt that re-enters the
allocator while the lock is held spins with `IF=0` and never returns — no panic,
no stall report, no serial output at all. Every other site in the file was fixed
by wrapping; these two were fixed by documenting.

**The fix.** `debug_assert!(!crate::cpu::interrupts_enabled())` at the top of
both, and extend the `unsafe` block's `// SAFETY:` comment to cite the assertion
rather than the caller's good intentions. `crate::cpu::interrupts_enabled()`
already exists (`kernel/src/cpu.rs:164`) and is a single `pushfq`. This turns a
comment into a check that fires on the first wrong call in a debug build, which
is the build the boot test runs.

**Why not `without_interrupts` like the other fourteen.** It would be correct
but wasteful: these are the allocator's batch fast path, called from code that
has *already* disabled interrupts, and `without_interrupts` costs a
`pushfq`/`cli`/`popfq` per call on a path whose reason for existing is to avoid
per-frame overhead. The assertion keeps the fast path and still makes the
premise false-able.

**Fixed.** Both functions now open with
`debug_assert!(!crate::cpu::interrupts_enabled(), ...)`, each `// SAFETY:`
comment cites the assertion rather than the caller's good intentions, and
`pcpu_refill`'s doc comment records why the wrap is omitted so the omission
stays a decision. Boot-tested green (`BOOT_OK` after 370 s) with **neither
assertion firing** — which is the useful half of the result: the precondition
was not merely documented-and-hoped-for, it holds at every call the boot path
makes, and now a future call that breaks it says so instead of hanging.
