## TD-A-DF-IS-THE-SEVENTH-COPY-OF-THE-FATAL-FAULT-GUARD (lane A)

**Found 2026-09-05** while extending `begin_fatal_kernel_fault` to `#TS`/`#NP`/
`#SS`. Not fixed in that commit on purpose: `#DF`'s guard is subtly different
from its siblings' and changing it is a separate judgement, not a rename.

**What.** `handle_double_fault` (`kernel/src/idt.rs`, vector 8) does not call
`begin_fatal_kernel_fault`. It open-codes the second half of it —
`FATAL_FAULT_IN_PROGRESS.swap(true, Relaxed)` with its own `NESTED #DF …`
one-liner and `halt_loop()` — and omits the first half, the `cpu::cli()`.

With `#TS`/`#NP`/`#SS` now routed through the shared function, vector 8 is the
**only** fatal handler still carrying its own copy, which is precisely the
shape `begin_fatal_kernel_fault`'s own doc comment argues against ("a shared
entry point is what stops the fourth handler from having to learn it a fourth
time").

**Why it was not simply folded in.**

1. *The message differs, and the difference is load-bearing.* `#DF`'s nested
   line carries `err={:#x}`; the shared one prints only `rip`/`rsp`. On a
   double fault the error code is architecturally always zero, so this is
   probably droppable — but "probably" is not a thing to establish in a commit
   about three other vectors.
2. *The ordering differs.* `#DF` takes the guard **before** `log_exception(8,…)`;
   the siblings log first and guard after. That is arguably correct here — a
   `#DF` arriving during fatal diagnostics is the death spiral the guard exists
   for, and the cheapest possible response is right — but it means the call
   cannot simply be substituted at the top.
3. *The missing `cli()` looks harmless and is not obviously so.* Every entry in
   this IDT is a 64-bit **interrupt** gate (`IdtEntry::new`, `type_attr =
   0x80 | dpl<<5 | 0x0E`), and an interrupt gate clears `IF` on entry, so
   interrupts are already masked when `handle_double_fault` starts. That would
   make the `cli()` pure belt-and-braces — except that the shared function's doc
   records it being added *because* the fatal path was observed running
   interruptible on 2026-07-16 ("a timer tick faulted mid-report, tripped the
   guard, and halted us before the diagnostics printed"). Those two facts do not
   sit comfortably together, and the discrepancy is the actual open question
   here: something re-enables interrupts between gate entry and the fatal branch,
   and until it is known what, "the gate already did it" is not a safe reason to
   keep omitting the `cli()`.

**Proper fix.** Resolve (3) first — find what re-enabled `IF` on 2026-07-16, or
establish that the lesson was mis-attributed and the real fix was something
else. Then either give `begin_fatal_kernel_fault` an optional error-code
argument and call it from `#DF` too, or record in `#DF` why it is deliberately
the exception. Either outcome removes the seventh copy; leaving it undecided is
what this entry is against.

**Severity.** Low. `#DF` today is correct in the case that matters (it halts,
and its guard does stop the recursion). This is a maintenance/consistency debt
plus one genuinely unresolved question about interrupt state on the fatal path.
