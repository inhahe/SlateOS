## A-KSHELL-A-RUNG-THAT-SPELLS-A-DIAGNOSTIC-FAILS-THE-KERNEL-FOR-IMPROVING-IT — ✅ FIXED 2026-08-26 (lane A)

**In short:** the boot test panicked and the kernel died — not because the
kernel was wrong, but because a self-test had written the *exact wording* of an
error message into its assertion, and the burn-down then improved that wording.
`vd remove` with no operand used to print `Usage: vd remove <id>`; after being
converted to the shared operand helper it prints `vd: remove: missing desktop
id`, which names what is missing instead of reprinting a synopsis. That is the
outcome the whole burn-down exists to produce. The rung failed it anyway,
because it asserted the six characters `Usage:`.

**Symptom.** `scripts/boot-test.sh` on `adddc7459`:

```
!! `a missing operand is reported`: output lacked text it must contain
   expected somewhere in the output:  Usage:
   actual output (31 bytes):          vd: remove: missing desktop id
!!! KERNEL PANIC !!!  panicked at kernel\src\kshell.rs:446:5
```

**Where.** Two tables in `kshell::self_test`, both written during the
`} else {` blind-spot repair:

| Table | Rung | Cases affected |
|---|---|---|
| `arity_cases` | 67 | `vd remove`, `tile add` — converted to `required_id`/`required_num` |
| `refusal_cases` | 67 | `wsnap snap 999 zznosuch` — converted to name the word |

Both looped `assert_output_contains(…, b"Usage:")` over every row.

**Fix.** Each table gained a third column: the fragment the complaint must
contain, per case. The loop asserts that fragment for the failing invocation
and its *absence* for the paired control, so the control half keeps working
too. Nothing about the rungs' coverage changed; what changed is that they now
pin the rule — *a missing or unusable operand is reported, and the status
reaches the caller* — rather than one wording of it.

**Why it is worth an entry.** This is a trap the burn-down manufactures at a
steady rate. Every batch converts more commands from "print a synopsis" to
"name the word", and there are now eight rungs' worth of assertions written
against the old wording. Two rules follow, and they are the reason this is
written down rather than just fixed:

1. **Assert the fragment that carries the rule, not the sentence.** `is not a
   layout` and `missing window id` are rules; `Usage:` and the full synopsis
   text are formatting. A rung should fail when a command stops refusing, not
   when it starts refusing more clearly.
2. **A table-driven rung needs a per-row expectation.** The two tables here
   were uniform when written and stopped being uniform the moment one row's
   command was improved. A shared literal in a loop is a coupling between rows
   that have nothing to do with each other.

**Severity.** Low as a defect — the kernel was correct throughout and the boot
test is exactly the thing that caught it, at the first opportunity, before the
lane merged to `main`. Notable as a process finding: the gates (`cargo check`,
all eight kshell scanners) were green on the broken tree, because none of them
runs the shell. Only the QEMU boot does. That is the argument for booting every
burn-down batch rather than batching several and booting once.

**Follow-up 2026-08-26 — the class is now caught statically, and it was not the
only instance.** The two rules above were written for a human to remember, which
is the weakest possible enforcement for a defect whose only detector is an
eleven-minute boot. `scripts/check-selftest-wording.py` now enforces them: for
every `assert_output_contains`/`_lacks` in `self_test` it resolves the captured
command through the dispatch table to its `cmd_*` function, narrows to the
`match` arm the subcommand selects, and requires the fragment to be producible
from a literal that arm — or anything it calls in `kshell.rs` — passes to a
print macro. `boot-test.sh` runs it, and runs its `--self-test` fixture first.
Rationale and the over-approximation argument: design-decisions §604.

Running it on the current tree found **two more dead guards of the same class**,
both in the `lacks` direction — the half of the defect this entry named but did
not go looking for:

| Rung | Asserted the absence of | Why it could never fire |
|---|---|---|
| 74, `wsnap` | `Left half` | `cmd_winsnap` names the position through `SnapPosition::label()`, which spells it `left`. No code in the tree produces `Left half`. |
| 67, `bright` | `Usage:` | The `set` arm was converted to `required_num` and says `missing percentage` instead. The guard died in the same kind of conversion that caused this entry's bug. |

These are the *mirror image* of what happened here: a `contains` on stale
wording fails a correct kernel and is loud, while a `lacks` on stale wording
passes forever and is silent. The silent one is the worse of the two — the rung
still reads as a guarantee, and the regression it was written to catch has been
unguarded ever since. Fixed in `c87e74627`; each now names a sentence the
command still owns.
