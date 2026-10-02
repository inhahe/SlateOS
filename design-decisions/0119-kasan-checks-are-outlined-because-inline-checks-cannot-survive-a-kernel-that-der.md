## §119 — KASAN checks are outlined, because inline checks cannot survive a kernel that dereferences user pointers

**Date:** 2026-08-12

**Decided by:** Claude (operator-approved scope). A follow-on to §118, made
while getting the instrumented kernel past its first user-mode fault; mine to
revisit.

**Context.** With the pre-shadow window fixed (§118) the instrumented kernel
booted 1298 serial lines — and then died on an unrecoverable kernel #GP inside
`core::ptr::write::<u64>`, reached from `idt::try_dispatch_user_exception`
writing the SEH `ExceptionContext` onto the faulting thread's **user** stack.
The faulting instruction was `cmpb $0x0, (%rax)`: LLVM's inline shadow compare.

**The mechanism.** Inline instrumentation computes
`shadow = (addr >> 3) + 0xDFFFE00000000000` and dereferences it
*unconditionally*, before anything has had a chance to decide whether the
address is one we shadow. For a kernel address that is fine. For a user address
it is not: `shadow(0x40_0000_0000) = 0xDFFF_E008_0000_0000`, whose bits 63:48
are `0xDFFF` while bit 47 is 1 — not sign-extended, therefore **non-canonical**,
therefore #GP. There is no offset that fixes this; a single-add mapping cannot
cover both halves of a 48-bit split address space, which is why Linux's KASAN
covers only kernel addresses too.

Linux gets away with inline checks because kernel code there *never*
dereferences a user pointer directly — every access goes through hand-written
`uaccess` asm that the compiler does not instrument. Our kernel does dereference
user pointers directly in several places by design (the SEH context above,
`mm::user`'s copy helpers, …), so the same guarantee does not hold, and each
such site is a kernel panic with a backtrace that points at the sanitizer rather
than at the bug being hunted.

**Decision.** Build with `-asan-instrumentation-with-call-threshold=0`, which
makes LLVM emit a **call** to `mm::kasan_rt::__asan_load8_noabort` & co. for
every checked access instead of the inline compare. The entry points already
existed (they were defined defensively for LLVM's 7000-access threshold); they
call `kasan::shadow_allows`, which runs `shadow_of` *first* and returns "no
shadow, therefore allowed" for any address outside the backed window — user
addresses included. The bad dereference is now unreachable by construction.

**For.** It is one rule that covers every site, present and future, instead of
an open-ended hunt for raw user derefs where each miss costs a full boot cycle
(~10 minutes) to find. It also makes the shadow lookup a single place where
policy can be added — the pre-shadow "is the shadow even installed yet"
question, address-space filtering, future quarantine integration — rather than
something baked into thousands of inline sequences. Linux offers exactly this
mode (`CONFIG_KASAN_OUTLINE`) for essentially the same reasons.

**Against.** A call per memory access is substantially slower than a
compare-and-branch, and the instrumented kernel was already `-O0` with a check
on every load and store. Accepted for the *correctness* goal: this is a debug
profile whose entire purpose is localizing one bug, and a boot that finishes
slowly beats a boot that panics in the sanitizer.

**Measured cost, and the consequence nobody costed up front.** The slowdown was
guessed at "roughly doubled" when this was written; measuring it gave something
far worse. A plain debug boot reaches `BOOT_OK` in **~283–318 s**
(`soak-20260723-190300`, 100/100 iterations). The instrumented debug boot ran
975 s to reach the point a plain boot reaches at line 4016 of 23532 — 17 % of
the log — with 48 of the 66 ring-3 spawn tests and 178 MB of the 217 MB of test
ELF still ahead of it. Both extrapolations (line rate, and remaining-ELF ratio)
land between **5500 s and 8500 s**, i.e. a **~20× slowdown**, not 2×. The first
attempt was launched with a 5400 s timeout on the strength of the 2× guess and
had to be abandoned at 17 % rather than yield a truncated answer.

For proving the instrumented kernel *boots*, ~2 h per boot is merely tedious.
For the thing this profile was built for it is disqualifying: B-KNULLJUMP fires
at roughly 1 boot in 120, so a soak that has an even chance of catching it needs
~80 boots — **over a week of wall-clock** at this rate, against ~7 h for the
plain-build soak that has been run repeatedly. So the cost is only "accepted"
for the single validating boot. Making the *hunt* viable needs a separate
decision (an optimized instrumented build being the obvious candidate, since
most of the 20× is `-O0` codegen rather than the sanitizer, but a release kernel
has never been booted here and optimization perturbs exactly the timing a rare
race depends on) — see `open-questions.md` Q43.

**The obligation it creates.** Every checked access now *calls* the runtime, so
nothing on the path from a check entry point down through `shadow_allows` may
perform an instrumented access of its own — it would call the check again,
unbounded, and appear as a stack overflow with no explanation. This is the same
invariant as §118's pre-shadow window with different roots, and it is enforced
the same way: `scripts/kasan-check-preshadow.py` now runs a second walk from the
`__asan_load*`/`__asan_store*` entry points. It caught a real violation on the
first run — `shadow_allows` calling `Option::<u8>::is_some`, a `core` generic
monomorphised into the kernel *with* instrumentation — which is why `byte_bad`
now returns a `u8` sentinel (`0` = addressable) rather than an `Option<u8>`, and
why `get_shadow` loads the mapped-frame bitmap and the shadow byte with `asm!`
and indexes with shifts and masks instead of `/` and `%`.

The *report* path is deliberately exempt from that rule and the walk stops
there: it formats and backtraces, far too much code to keep raw, and it does not
need to be. A report calls instrumented code, whose checks call `shadow_allows`,
which is clean and returns — one level of nesting, not a regress.

**Where it lives.** `scripts/kasan-build.sh` (the flag and its rationale),
`kernel/src/mm/kasan_rt.rs` (the check entry points are now the primary ones),
`kernel/src/mm/kasan.rs` (`get_shadow`, `byte_bad`, `shadow_allows`,
`raw_load_u8`, `raw_shl_u64`), `scripts/kasan-check-preshadow.py` (the
`RUNTIME_ROOT_PREFIXES` walk).

**How to reverse.** Drop the one flag. The check entry points stay defined and
unused, exactly as they were before, and the raw-`asm!` shadow lookup is
semantically identical either way — so reversing costs only the reappearance of
the #GP class this fixed.
