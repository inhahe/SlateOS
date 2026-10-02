### B-AC-INHERITED-AT-KERNEL-ENTRY. An IDT gate does not clear `EFLAGS.AC`, so ring 3 can pre-disable SMAP for every interrupt handler — 2026-08-12 — ✅ FIXED 2026-08-13

**Fix (2026-08-13).** Option 1 below — a real alternatives-patching framework
(`kernel/src/alternatives.rs`, design-decisions §123) — was built and used. Each
of the three ISR stub macros now opens with a 3-byte NOP patch site that
`alternatives::apply()` rewrites to `clac` at boot iff CPUID reports SMAP; the
SYSCALL path was already covered by the widened `IA32_FMASK` (§122). Boot log:

```
[alt] 47 site(s): 47 patched, 0 left at default, 0 error(s)
[alt] Running alternatives self-test...
[alt]   47 patch site(s) in .altinstructions
[alt] Alternatives self-test PASSED
[idt] Running alignment-check-flag (SMAP override) self-test...
EXCEPTION: Breakpoint (#BP) at 0xffffffff8111e73a
[idt]   AC is clear on exception entry: OK (SMAP override closed)
[idt] Alignment-check-flag self-test PASSED
```

That last block is the empirical proof, not an inference from the SDM: the test
sets AC in ring 0, executes `int3`, and the #BP handler observes AC already
clear on entry.

**Still not enabling SMAP.** `smap_enable_blocker()` now reports the *other*
prerequisite — `USER_ACCESSES_ANNOTATED` — and that one turns out to need a
refactor rather than an audit; see
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` in the tech-debt
section. The historical analysis below is kept because the reasoning about
*why* an unconditional `clac` was not an option still explains the design.

---

**Original entry (2026-08-12):**

**What.** `EFLAGS.AC` is the SMAP override: while AC = 1, supervisor-mode
accesses to user pages are permitted and SMAP checks nothing. An interrupt gate
clears only TF, NT, RF and VM (Intel SDM Vol. 3A §6.12.1) — **AC is inherited
verbatim from the interrupted context**, exactly as DF was in
`B-NO-CLD-ON-INTERRUPT-ENTRY` below.

AC is *not* privileged: ring 3 sets it with an ordinary `popfq`. So a user
process can set AC and then simply wait for the next timer tick, and every
kernel interrupt/exception handler from then on runs with SMAP disabled.

**Status: latent, not currently exploitable.** `smep_smap::init()` deliberately
does not set CR4.SMAP, so AC is inert today. The bug is that it becomes a real
hole the moment somebody enables SMAP — and it would fail **open and silently**:
nothing crashes, no test goes red, `smap` in kshell reports `ACTIVE`, and the
protection simply does not work. That is the same failure mode as
design-decisions §118 ("a defence that looks sufficient is not").

**Confirmed empirically, not just from the SDM.** `idt::ac_on_entry_self_test()`
sets AC, executes `int3`, and reads AC back as the handler sees it. Boot log:

```
[idt] Running alignment-check-flag (SMAP override) self-test...
[idt]   AC is inherited on exception entry, as expected while
        B-AC-INHERITED-AT-KERNEL-ENTRY is open (SMAP stays disabled): OK
```

**What is already fixed.** The **SYSCALL** half is closed:
`syscall::entry::init()` now masks AC (bit 18) in `IA32_FMASK`, along with NT,
IOPL, RF, ID and the arithmetic flags — the same set Linux masks in
`MSR_SYSCALL_MASK`. Verified in the boot log as `FMASK=0x257fd5`. Only the
IDT-gate path remains.

**Why it is not simply fixed the same way as `cld`.** `clac` raises #UD when
CPUID.SMAP is absent, so an unconditional `clac` in the ISR stubs would break
every pre-Haswell / pre-Zen CPU. Linux emits `ASM_CLAC` at every entry point and
alternatives-patches it to a 3-byte NOP on CPUs without SMAP
(`arch/x86/include/asm/smap.h`); **we have no alternatives-patching framework**,
which is the actual blocker. Options, in preference order:

1. **Build a minimal alternatives framework** (a section of patch sites, rewritten
   once at boot after CPUID). This is the right long-term answer — SMAP will not
   be the last feature that wants it — and it is the only option with zero
   steady-state cost.
2. **Patch the stubs at SMAP-enable time.** A special case of (1) with much less
   machinery: since we control the single place CR4.SMAP is set, overwrite a
   reserved 3-byte NOP in each stub with `clac` there. Cheap, but bespoke.
3. **`pushfq` / `and` / `popfq` in the stub prologue.** Works on every CPU with
   no patching, but `popfq` costs ~20+ cycles on *every* interrupt versus
   `clac`'s ~2, on the hottest path in the kernel. Rejected unless (1)/(2) prove
   impractical.

**Guard against silently mis-enabling SMAP.** `smep_smap::ENTRY_PATHS_CLEAR_AC`
(currently `false`) gates CR4.SMAP via `smap_enable_blocker()`, and
`idt::ac_on_entry_self_test()` asserts that the constant agrees with what an IDT
gate actually does. So flipping the constant without adding `clac` fails the boot
self-test loudly, and adding `clac` without flipping the constant also fails —
the mitigation cannot sit unused, and SMAP cannot be enabled against an entry
path that disables it.

**Where.** `kernel/src/smep_smap.rs` (`ENTRY_PATHS_CLEAR_AC`,
`USER_ACCESSES_ANNOTATED`, `smap_enable_blocker`, `init`, `init_ap`);
`kernel/src/idt.rs` (`ac_on_entry_self_test`, `BP_ENTRY_AC`, the three stub
macros that need the `clac`); `kernel/src/syscall/entry.rs` (FMASK — done).

**Related.** Second instance of the "ring-3 RFLAGS inherited at an IDT gate" bug
class, after `B-NO-CLD-ON-INTERRUPT-ENTRY`. No longer blocked on test coverage:
`B-QEMU-DEFAULT-CPU-HAS-NO-SMEP-SMAP-UMIP` is fixed, so the boot CPU now
advertises SMAP and a `clac` in the stubs would be live code. The remaining
blocker is the alternatives-patching framework (option 1 above).
