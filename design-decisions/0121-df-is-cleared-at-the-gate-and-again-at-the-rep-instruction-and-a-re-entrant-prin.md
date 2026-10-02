## §121 — DF is cleared at the gate and again at the `rep` instruction, and a re-entrant print garbles rather than blocks

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Two small tradeoffs settled while fixing
`B-NO-CLD-ON-INTERRUPT-ENTRY`; mine to revisit. The bug itself had no decision
in it — a missing `cld` on interrupt entry is simply wrong, and there is no
case for the other side.

**Context.** No IDT stub cleared the direction flag. An IDT gate clears TF, NT,
RF and VM but leaves DF exactly as the interrupted context had it (Intel SDM
Vol. 3A §6.12.1), so ring 3 could hand the kernel DF = 1 with a one-byte `std`
and every `rep`-string operation — including every compiler-emitted
`memset`/`memcpy` — would then write *before* its intended destination instead
of at it. The SYSCALL half was already covered by `IA32_FMASK` bit 10. Details,
evidence and the B-KNULLJUMP hypothesis are in `known-issues.md`.

Three questions did have two defensible answers.

**Decision 1 — belt *and* braces: `rawmem::fill_u8` clears DF itself, even
though entry now guarantees it.**

*For:* the helper stops depending on an invariant established somewhere else
entirely. `fill_u8` is a memory-*debugging* primitive: it runs in the exact
situations where other invariants are already suspect, and a backwards fill
would corrupt the bytes *before* a redzone — which is to say, it would
manufacture exactly the kind of corruption the surrounding subsystem exists to
detect, and blame it on someone else. Clearing DF can only ever repair state,
never damage it, because DF = 0 is what all compiled code requires anyway; so
the redundancy carries no risk of its own. The cost is one instruction on a path
that is not hot.

*Against:* it is genuinely redundant now, and redundancy invites the reader to
wonder which of the two mechanisms is the real one — the failure mode §118 kept
hitting, where a defence that looks sufficient is not. Mitigated by saying so
explicitly at both sites: the `idt.rs` comment names `rawmem::fill_u8` as a
dependant, and the `fill_u8` comment says entry already clears DF and that this
is deliberate belt-and-braces.

*Consequence:* `options(preserves_flags)` had to come off `fill_u8`'s `asm!`
block, since it now writes DF. Negligible — the compiler just reloads flags it
almost never has live across a bulk fill.

**Decision 2 — a re-entrant print garbles the output rather than dropping the
line or blocking.**

The console lock cannot be taken twice by one CPU, and `cli` does not help
because exceptions ignore it. Three options:

- **Block (the old behaviour).** Deadlock. Not really an option; it is the bug.
- **Drop the nested line.** *For:* output stays clean and parseable, and the
  boot-log greps in `boot-test.sh` keep working unchanged. *Against:* it drops
  precisely the highest-value line in the log. Nested prints are not routine —
  a print inside a print means a fault fired during a fault report, which is the
  most interesting event in the entire boot.
- **Chosen: write the nested line through the lock-free emergency port,
  interleaved.** *For:* nothing is lost. The wedge that motivated this
  (`B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT`) is a case where
  the operator needed one token — the faulting RIP — and the failure mode
  deleted it. *Against:* a line can now be cut in half by another line, so any
  future log parser must tolerate that. Accepted: interleaving happens only when
  something has already gone wrong, so it degrades the pretty case never and the
  broken case from "silence" to "readable but ugly".

**Decision 3 — claim the per-CPU re-entrancy flag before taking the lock, not
after.**

*For:* setting it after acquiring leaves a window — one instruction, but real —
where the CPU holds the lock and a nested exception would not recognise that,
and would block forever. The whole point is to have no such window. *Against:*
during the interval where this CPU is merely *waiting* for the lock, the flag
claims it is "inside" when it is not. That turns out to be the correct reading
anyway: the flag's meaning is "this CPU is somewhere inside `_print`", which is
exactly the condition under which a nested call must not block. And it is
per-CPU, so no other CPU's view is affected.

**Where it lives.** `kernel/src/idt.rs` (the `cld` in all three `global_asm!`
stub macros, the "`cld` on entry" comment, `df_on_entry_self_test`, and the
`BP_ENTRY_DF` observation in `handle_breakpoint`), `kernel/src/mm/rawmem.rs`
(`fill_u8`), `kernel/src/serial.rs` (`_print`, `IN_PRINT`,
`reentrancy_self_test`), `kernel/src/main.rs` (both self-test calls).

**How to reverse.** Each piece is independent. Removing `fill_u8`'s `cld`
(decision 1) leaves the entry `cld` load-bearing and is safe as long as it stays
— restore `preserves_flags` if so. Decisions 2 and 3 are confined to `_print`;
reverting to the blocking form restores the deadlock, so it should only be done
alongside some other escape. The entry `cld` itself should not be reverted.
