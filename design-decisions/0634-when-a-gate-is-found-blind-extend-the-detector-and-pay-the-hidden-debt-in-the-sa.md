## 634. When a gate is found blind, extend the detector and pay the hidden debt in the same commit — never in two

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane A)
**Lane:** A

**In short:** we keep a script (`scripts/check-option-refusal.py`) whose job is
to count one specific kind of bug in the shell — places that read a word you
typed, fail to understand it, and quietly substitute some other value. The count
is published in `known-issues.md` and is only allowed to go down. This week the
script turned out to have been **missing nineteen of them all along**, because
it only recognised one of the two ways Rust spells "convert text to a number".
The question was what to do in the moment of discovery: fix the script now and
the nineteen sites later (which publishes a sudden *rise* in the count), or fix
both together (one commit, count still falls). I chose both together.

**What was actually wrong.** The detector required a literal `.parse()` call, so
the decimal spelling of the defect was caught and the hexadecimal one —
`u16::from_str_radix(v, 16).unwrap_or(0)` — was not. Same bug, same
consequence, invisible. Among the nineteen were `sysrq mask`, where the
substituted value enables *every* magic SysRq category including reboot and
crash, and `kprobes register`, which installed a probe at the null address and
reported a real probe id for it.

**The options.**

| Option | *What changes:* |
|---|---|
| **A. Fix the detector now, file the sites as a new backlog item** | the published count jumps 346 → 365 with no code having changed, then falls later. |
| **B. Fix the sites first, quietly, then fix the detector** | the count never rises; the detector stays blind for the length of the intervening work, and a new site added meanwhile is still missed. |
| **C (chosen). One commit: extended detector + all nineteen sites + the rest of each function they were in** | the count falls 365 → 335; the ledger gains no new entry; the gate is never green over code it cannot see. |

**Why C.** The ledger's whole value is the promise in its own header — "the
number is visible and can only go down". Option A honours the letter of that and
breaks its spirit: a reader seeing 346 → 365 has no way to tell a regression
from a measurement correction, and the file would need a paragraph explaining
that the rise is good news, which is precisely the sort of asterisk that trains
people to stop reading the number. Option B is worse than it looks: it leaves a
window in which the gate is knowingly blind, and the reason we have the gate is
that nobody is watching this code by hand.

C costs a larger-than-usual commit — 30 sites across 8 functions rather than the
usual 4 — and that is the accepted price. It is the only option under which no
published number is ever wrong, and under which the sentence "the gate is green"
means the same thing before and after.

**The corollary, which is the durable part.** *A gate's own output cannot audit
its coverage.* The total looked healthy at 346 and would have looked healthy at
any number. What exposed this was picking two functions that **ought** to appear
in the ledger and finding they were not merely un-exempted but **absent**. The
checker already reports an entry claiming *more* sites than exist (its `stale`
arm); it structurally cannot report an entry that was never written. So when a
counted-debt gate is inherited or extended, the check is to name a function you
believe is dirty and confirm the gate agrees — not to read the total.

This is the second time the same gate has been found under-counting for a reason
that had nothing to do with the code it inspects: see
`A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR`, where it
matched against a *line* and `cargo fmt` had wrapped the statements it was
looking for, hiding 466 sites. Both times the gate was green over code it was
written to catch, and both times the total gave no hint. It is the same shape as
§630's shellcheck floor — a check reporting success because it could not run.

**How to reverse.** The detector change is two alternatives in one regex in
`scripts/check-option-refusal.py` (`D1`); narrowing it back would re-hide the
class, and would also make the `stale` arm fire for every function fixed here,
so the ledger would have to be edited back in step. There is no reason to.

**Where it is.** `scripts/check-option-refusal.py` (`D1`);
`scripts/option-refusal-ledger.txt` (four entries removed);
`kernel/src/kshell.rs` (`required_hex`, `required_key`, and the eight `cmd_*`
functions listed in the thirty-eighth burn-down entry); self-test rung 102.
