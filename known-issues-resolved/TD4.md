### TD4. Monolithic `syscall::linux::self_test()` has an unbounded boot-stack frame — RESOLVED 2026-06-14

**Resolution (2026-06-14):** The split is complete. Every self-contained
validation block in `self_test()` is now wrapped in its own
`#[inline(never)]` nested helper (`fn self_test_NAME() -> KernelResult<()>`,
called via `?`), so each sub-frame is allocated and freed transiently around
its call and no single frame is the sum of all batches. The body went from one
monolithic ~1.4 MB frame to ~80 small per-block helpers. Three earlier helpers
that had grown to wrap multiple sibling blocks (`getrusage_sysinfo_times` = 5
blocks, `capget_capset` = 2, `sched_affinity` = 2) were peeled apart so each
block gets its own frame. A structural scan confirms **zero** bare top-level
blocks remain. The technique used throughout (Technique B): insert a 5-line
header — `self_test_NAME()?;` + `#[inline(never)] fn self_test_NAME() -> …
{ use crate::serial_println;` — immediately before the block's leading
comment, and a 2-line footer — `Ok(())` + `}` — immediately after the block's
closing brace; the block body is never reproduced or re-indented, so the wrap
is safe for arbitrarily large blocks. A non-inlined nested fn cannot capture
enclosing locals, which acts as a compile-time safety net against
mis-scoping. Every wrap was individually boot-tested (BOOT_OK) and committed.
This removes the F10 (`.bss`/`FPU_STRATEGY` silent-corruption) failure class
at its root rather than merely deferring it behind the boot-stack canary.

**Progress (2026-06-13):** Began the incremental `#[inline(never)]` split. The
two leading self-contained check groups were extracted into standalone
functions — `self_test_errno_mapping()` (errno round-trips + the `check_errno!`
macro, used nowhere else) and `self_test_native_translation()` (the
`linux_from_native` round-trips). Both are guaranteed behaviour-preserving:
their locals never escape the extracted region. `self_test()` now calls them
via `?`. This establishes the repeatable extraction pattern (cut a contiguous
region whose locals don't cross the boundary, lift to an `#[inline(never)] fn
… -> KernelResult<()>`, replace inline with a `?` call, build+boot-test).
Continue opportunistically: the safe cut points are regions that don't share a
reused local (e.g. the early checks share `args`/`r`, so a larger contiguous
run ending at the last use of those must be lifted as one unit). Remaining
work is the bulk of the ~40 k-line body.


**What:** `kernel/src/syscall/linux.rs::self_test()` is a single ~1.4 MB
function (~39 k lines, opens near line 35858, closes near line 75298) whose
body is one giant 4-space enclosing block. Each ABI-fidelity batch (536 and
counting) appends its own locals inside that block. In the unoptimized
debug build (`opt-level=0`, no LLVM stack-slot coloring), the compiler does
**not** reuse stack slots across the lexically-disjoint per-batch sub-blocks,
so the function's single frame is the *sum* of every batch's locals
(~480 KiB as of batch 536 and growing monotonically). It runs directly on
the guardless boot stack — this is exactly what caused F10 (silent
`.bss`/`FPU_STRATEGY` corruption when the frame overran the old 512 KiB
stack).

**Why it's debt, not a bug now:** F10's fix (2 MiB boot stack + 64 KiB
redzone canary) gives ~1000+ batches of runway and converts any future
overrun into a clean `FATAL: boot stack overflow detected` halt instead of
silent corruption. So the system is correct and self-diagnosing. But the
frame grows ~1 KiB/batch, so this only defers the wall; it does not remove
it.

**Proper fix:** split `self_test()` into many small `#[inline(never)]`
sub-functions (e.g. one per batch or per logical group, `fn self_test_b536()
-> Result<…>` …) called in sequence from a thin driver, so each sub-frame is
allocated and freed around its call and no single frame is large. This caps
the boot-stack frame regardless of batch count and is the real removal of
the F10 failure class.

**Why deferred:** the function is one giant 4-space block; a hand-split risks
silently mis-scoping locals shared across batch boundaries (a local defined
in an early batch and read in a later one would stop compiling, or worse,
shadow). Doing it safely means iterating in small chunks with a build after
each (~50 s/cycle), and the canary makes it non-urgent. **Trigger to do it
properly:** before the boot-stack usage (reported by the canary scan / a
future high-water mark print) crosses ~50 % of the 2 MiB stack, or
opportunistically when next touching the self-test scaffolding.
