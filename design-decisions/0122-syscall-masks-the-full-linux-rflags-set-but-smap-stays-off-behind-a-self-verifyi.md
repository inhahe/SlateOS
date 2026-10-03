## §122 — SYSCALL masks the full Linux RFLAGS set, but SMAP stays off behind a self-verifying gate rather than shipping an unconditional `clac`

**Date:** 2026-08-12
**Decided by:** Claude (autonomous)

**Context.** Fixing `B-NO-CLD-ON-INTERRUPT-ENTRY` (§121) established that RFLAGS
the kernel inherits from ring 3 at an entry gate is attacker-controlled state.
Auditing the rest of that class turned up `EFLAGS.AC`, which is the SMAP
override and is settable from ring 3 with an unprivileged `popfq`. An IDT gate
does not clear it (measured, not assumed — see
`B-AC-INHERITED-AT-KERNEL-ENTRY`). Three sub-decisions followed.

### 1. Widen `IA32_FMASK` to Linux's full set, not just the bits with a known bug

`FMASK` was `TF | IF | DF`. Adding `AC` alone would have closed the specific
hole found. Instead it now masks what Linux's `MSR_SYSCALL_MASK` masks: AC, NT,
IOPL, RF, ID and the arithmetic flags as well (`0x257fd5`).

*For the narrow fix:* only change what you can justify; every extra masked bit
is a behaviour change on the syscall hot path, and masking arithmetic flags in
particular fixes no known bug.

*For the wide fix (chosen):* the cost is exactly zero — FMASK is applied by the
CPU as part of the instruction, so masking fifteen bits is no slower than
masking three. And the audit that found AC found it only because someone went
looking; NT (a stray `NT` makes a later `iret` attempt a task-switch return) and
IOPL are the same shape of latent problem. Enumerating "which inherited flags are
harmless" is a standing obligation to re-audit on every future change, whereas
masking everything not needed is a one-time decision that stays correct. Linux
reached the same conclusion after its own history of entry-flag bugs, which is
decent evidence about where this ends up.

### 2. Do not put an unconditional `clac` in the ISR stubs

The symmetric fix to §121's `cld` would be a `clac` beside it. Rejected: `clac`
#UDs when CPUID.SMAP is absent, so this would fault at the first interrupt on
any pre-Haswell / pre-Zen CPU — turning a latent hardening gap into a total boot
failure on older hardware. Linux alternatives-patches `ASM_CLAC` to a NOP on such
CPUs; we have no patching framework, so the honest position is that this fix is
*blocked on infrastructure*, not merely unwritten. The alternatives are recorded
in `known-issues.md` rather than half-implemented.

A `pushfq`/`and`/`popfq` sequence would work everywhere with no patching, and was
rejected on cost: ~20+ cycles versus `clac`'s ~2, on every single interrupt. That
is a real, permanent tax on the hottest path in the kernel to fix a bug that is
currently latent (SMAP is off). If a cheap alternatives framework proves
impractical, this becomes the fallback.

### 3. Gate CR4.SMAP on a constant, and make the constant self-verifying

The failure mode being defended against is specific: someone finishes the
STAC/CLAC instrumentation, enables SMAP, and gets a protection that reports
itself ACTIVE while enforcing nothing, because AC is still inherited. Nothing
crashes and no test goes red — the §118 failure mode exactly.

*Rejected — a comment.* The existing code already had a comment explaining why
SMAP was deferred, and it named only the STAC/CLAC prerequisite. The AC one had
never been noticed. Comments do not survive being unread.

*Chosen — a gate plus a cross-check.* `smap_enable_blocker()` refuses to set
CR4.SMAP while `ENTRY_PATHS_CLEAR_AC` is false, and
`idt::ac_on_entry_self_test()` measures what an IDT gate actually does with AC
and asserts it against that constant. The cross-check is what makes this more
than bookkeeping: the constant cannot drift from reality in *either* direction.
Flipping it without adding `clac` fails the boot test; adding `clac` without
flipping it also fails, so the mitigation cannot sit unused after being written.

The cost is a self-test that asserts a currently-*broken* property is broken,
which reads oddly. That is deliberate: it asserts agreement between the code's
belief and the hardware, not that AC is clear, so it keeps passing unchanged
through the eventual fix and only fails if the two disagree.

**How to reverse.** Decision 1 is one literal in `syscall::entry::init()`; the
old `TF|IF|DF` value still boots. Decisions 2 and 3 are the same change viewed
from two sides — deleting the gate re-enables SMAP the moment `features.smap` is
true, which is precisely the silent failure, so the gate should outlive the
`clac` work rather than be removed with it.

**Update (2026-08-13).** Both prerequisites are now met and CR4.SMAP is on.
Decision 2's blocker was resolved by §123's alternatives framework (the ISR
stubs carry a NOP that is patched to `clac` only when CPUID reports SMAP), and
the STAC/CLAC instrumentation finished with
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE`. Decision 3 held up
exactly as intended and is kept: the gate now reads `None`, but it stays as the
one documented switch for turning SMAP back off, and the self-test was rewritten
to assert CR4 and `SMAP_ENABLED` agree with `smap_enable_blocker()` in *both*
directions rather than asserting SMAP is deliberately off — the same
"agreement, not a fixed answer" property, now checked from the other side.

Worth recording what the enforcement caught the moment it went live: a raw
`core::ptr::write` of the SEH exception frame onto the user stack in
`idt::try_dispatch_user_exception`, which three separate greps and two careful
readings had missed because it derives its user address from the ring-3
interrupt frame's RSP rather than from a syscall argument — and which, checked
properly, turned out to be an arbitrary kernel write available to any process
(`B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP`). That is the strongest
available argument for decision 3's premise: an instrumentation claim is not
verifiable by inspection, so the gate had to be flipped and tested, not
reasoned about.
