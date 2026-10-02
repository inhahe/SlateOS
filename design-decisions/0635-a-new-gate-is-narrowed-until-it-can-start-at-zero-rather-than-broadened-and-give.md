## 635. A new gate is narrowed until it can start at zero, rather than broadened and given a backlog

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane A)
**Lane:** A

**In short:** we found a new kind of shell bug — a command that reads a number
you typed, cannot make sense of it, and then does *nothing at all*: no message,
no error status, no action, so it looks exactly like success. To stop it coming
back we added an automatic check. The check could either be written broadly (it
would catch all 22 real bugs, plus 17 places where the same code shape is
perfectly correct, so it would have to ship with a 17-line list of exceptions),
or narrowly (it catches 20 of the 22 and none of the 17, and needs no exception
list at all). I chose narrow. A check that starts clean stays meaningful; a
check that starts with a list of exceptions teaches everyone to add to the list.

**The defect.** `if let Ok(id) = w.parse::<u32>()` nested inside
`if let Some(w) = parts.get(1)` with no `else` on the inner test. The `else`
that exists belongs to the *outer* test, so it covers the missing argument only.
An unreadable one falls out of both branches. `ptime enable zzz` printed
nothing and exited 0.

**Why the obvious detector cannot be used.** The bare shape — "`if let Ok` on a
parse, with no `else`" — matched 39 sites in `kshell.rs`. Only 22 were defects.
The other 17 were the same shape used *correctly*, to try one reading and fall
through to another:

| Site | Falls through to |
|---|---|
| `resolve_container_ref` | a numeric reference, else a container name |
| `parse_datetime_to_ns` | epoch seconds, else `YYYY-MM-DD` |
| `cmd_useracct info`, `cmd_template` | a uid/id, else a name — as its usage line offers |
| `execute_select` | POSIX `select`: non-numeric input leaves the variable empty |
| `expand_brace_expr` | bash's own `${x:abc}` |

Nothing about the syntax distinguishes these. Only the *destination* of the
fall-through does.

### Decision

D4 requires the nesting **and** that the inner `if let Ok` be the **sole**
statement of the `if let Some(w) = parts.get(N)` block. Soleness is a
mechanical stand-in for "the fall-through has nowhere to go": if there is
anything after the inner block, the code is trying an alternative; if there is
not, control leaves the command having done nothing.

*What changes:* the gate reports 20 of the 22 defects, 0 of the 17 correct
sites, and carries no exception list.

### Alternatives considered

**Broad detector plus a 17-entry allowlist.** Would have caught all 22 and the
two `container create` sites D4 cannot reach. Rejected because the allowlist is
the failure mode, not the cost: this file already carries a 332-line ledger for
D1, and the reason that ledger is tolerable is that it was inherited, not
created. A *new* gate arriving with 17 pre-granted exceptions establishes the
exception as the normal way to interact with it. The first person to hit a
false positive adds an eighteenth entry; nobody re-reads the first seventeen.

**No gate at all, on the grounds that the 22 sites are fixed.** Rejected for the
reason §600's whole burn-down exists: this defect was written 22 times by the
same hand that knew better each time. Shapes recur.

### The part that generalises

**D4's first zero was a false one, and only a fixture could say so.** It landed
at zero on `kshell.rs` immediately — while being broken. `close_brace` counted
braces per line, so `} else {` netted zero, a block with an `else` never
appeared to close, and the detector was silently skipping most of what it
targets. The file's own green output could not distinguish "nothing to find"
from "cannot find anything", and never will be able to; that is §634's
corollary. The self-test fixture reported it in the first second it existed:
two defective fixtures unreported, one correct fixture reported.

So the rule this adds to §634: **write the fixture before trusting the zero,
not after the blindness costs something.** Four of the five gates found blind
here in four days were found afterwards, by someone going to look at a specific
function. D4 was found before shipping, by a fixture that took ten minutes.

### Where it lives

`scripts/check-option-refusal.py` — `D4_OUTER`, `D4_INNER`,
`silent_operand_sites`, `close_brace`, and `--self-test` with `_D4_FIXTURE`
(three defective shapes, five correct ones);
`kernel/src/kshell.rs` — `readable_num`/`readable_hex` and self-test rung 103.

### How to reverse

Widen `D4_INNER`/`D4_OUTER` and add an `ALLOWED` entry per false positive. The
two `container create` sites are the honest argument for doing so: they are real
instances of the defect that D4 does not reach, because their word comes from
`strip_prefix` inside a loop rather than from `parts.get`, leaving no positional
nesting and so no soleness test. If a third such site appears outside the
`parts.get` shape, that is the trigger to reconsider — the narrowing is a bet
that the defect concentrates in positional operands, and two known exceptions is
where the bet still pays.
