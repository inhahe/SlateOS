## 604. The self-test wording gate decides what a command can print by an over-approximating call-graph closure, and treats the rung's own text as a witness rather than a wording

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel shell's self-test checks commands by running them and
asserting their output contains, or does not contain, some piece of text. That
text goes stale: a command's wording is improved, the assertion still names the
old wording, and the test either fails a *correct* kernel or quietly stops
guarding anything. Because the shell self-test can only run inside a booted
QEMU, either outcome costs an eleven-minute boot to discover. `scripts/check-selftest-wording.py`
now checks the assertions against the source before the build. The decision
recorded here is how it answers "can this command print this text?", because
every plausible answer is wrong in one direction or the other, and the choice
of which direction to be wrong in is the whole design.

**The defect it exists for.** On `adddc7459` a table of nine commands asserted
that each one's complaint about a missing operand contained `Usage:`. `vd
remove` was then converted to `required_id` and began saying `vd: remove:
missing desktop id` — strictly better wording — and the rung panicked the
kernel *for the improvement*. The class is not "someone typed `Usage:`"; it is
**an assertion whose expected text no longer belongs to the command under
test**, and it has two directions:

| Direction | What it costs |
|---|---|
| `assert_output_contains` names text the command cannot print | the rung fails a correct kernel — a boot lost per occurrence |
| `assert_output_lacks` names text the command cannot print | the assertion can never fire, so the regression it was written to catch is unguarded, silently |

Both were live in the tree when the gate was written: `wsnap snap abc left`
asserted the absence of `Left half`, wording that stopped existing when the
position started being named by `SnapPosition::label()` (which spells it
`left`), and `bright set 50` asserted the absence of `Usage:` after that arm had
been converted to `required_num` and stopped saying `Usage:` at all.

**The problem.** To check the assertion the gate has to know what text a command
can produce, and it is a regex scanner, not a compiler. Three sub-decisions:

**1. How wide is "the command"?** Not the whole `cmd_*` function: `cmd_vdesktop`
prints `Usage:` in plenty of arms, just not the one `remove` reaches, and a
whole-function pool declares the stale marker producible and reports nothing.
The pool is narrowed to the depth-1 `match` arm named by the command's *second*
word, plus everything reachable from it. It deliberately does **not** narrow to
nested matches, which would require knowing which operand the rung passed.

**2. How does a call get followed?** By name, over every definition of that
name in the file — free functions, associated functions and methods alike.
Following methods is not optional: `cut`, `sed`, `fold` and `base64` put every
diagnostic they own behind `err.report()`, and without it their pools held none
of their own error wording and every refusal rung in four commands was reported
as broken. But a name is all the scanner has, and `fn report` is a method on
four different error enums, so it unions all four.

*This over-approximates on purpose.* A pool larger than the truth lets a real
defect through; a pool smaller than the truth accuses correct code. Only the
second gets the gate switched off, so that is the direction not to err in. The
same reasoning sets `MIN_FIXED_RUN` in `producible`: a fragment matches a
format literal only if some *contiguous* run of the literal's fixed text, four
bytes or more, survives in it — enough that `` `abc' is not a horizontal
coordinate `` matches `` `{}' is not a {} `` on the eleven bytes of `' is not a `,
and `Usage:` matches nothing in the `vd remove` arm, whose best run is a lone
colon.

**3. What about text the command never owned in the first place?** Most of
these rungs assert on *data*, not wording: `cut -d: -f2` fed `zz_a:zz_b:zz_c`
prints `zz_b`, and no pool of `cut`'s literals can ever vouch for it. Checking
them anyway produced 121 findings that were all correct code.

So the gate tracks a **witness**: everything the rung has put into the system in
this block — every command line it ran, every byte it piped, every literal in a
statement that asserts nothing (rungs plant witnesses through `Vfs::write_file`
as readily as through a command). A fragment drawn from the witness is skipped,
because text the rung supplied is not a wording the command owns. This is what
these rungs' `zz_`/`selftest` prefix convention has always meant informally; the
gate just makes it decidable.

Three limits keep that from swallowing the rule it is meant to serve:

- The witness is **rung-scoped**, dropped at brace depth 0 between rungs. Accumulated over all 74 rungs it would eventually contain a substring of nearly anything.
- Assertion fragments and table markers are **excluded** — they are the thing under test. This is exactly what keeps `b"Usage:"` checkable.
- Only **whitespace-free** plain literals are planted. A path or a file name is one token by construction; the third argument of `assert_eq!(last_exit(), 1, "find: an unreadable size is an error")` is prose *about* the command and would blind the gate to half its own vocabulary.

**The alternative considered and rejected** for the witness was splitting the
fragment on whitespace and skipping it if every token appears in the witness.
It would have cleared two more findings, and it is far too weak: a marker like
`no such desktop` would be skipped the moment `no`, `such` and `desktop` each
turned up somewhere in the block.

**What is left over is exempted, not analysed away.** Five assertions remain
that the gate cannot derive: output a filter *rearranged* out of the rung's data
(`cut -d: -f3,1` → `zz_a:zz_c`, `cut -c1-3,7` → `zz_d`, `sed s/zz_dup/zz_one/g`
→ `zz_one zz_one`), `od`'s formatted offset column, and `pmgr`'s `Disk #{}: {}
{}GB` whose literal frame between two operands is one space and two letters.
They live in `ALLOWED` with a reason each, and an entry that stops matching is
itself reported — an exemption that no longer fires is either debt that was paid
or an exemption now covering something it was never written for.

**The gate is graded before it grades.** `--self-test` runs the whole analysis
over an embedded fixture — a miniature `kshell.rs` that *is* the `adddc7459` bug:
a table of arity cases, a loop over it, and one `b"Usage:"` marker true of one
row and false of the other. `boot-test.sh` runs it first, for the same reason
`check-recursive-locks.py` and `check-selftest-skips.py` are run against their
fixtures first: **every collapse mode of this analysis makes findings disappear,
never appear**, and a gate that has stopped grading anything reports zero
findings in exactly the same words as a clean tree. Building the fixture on the
real bug rather than on synthetic cases means a gate that has lost the ability to
catch the one thing it was written for cannot pass its own test.

The fixture was then **mutation-tested** rather than trusted for passing. Six
mutants — arm narrowing disabled, the witness never planted, `piped` unrecognised
as a capture, the call graph not followed, the loop not bound to its table,
`MIN_FIXED_RUN` removed — were each injected into a copy of the gate, and all six
turned the fixture red. A fixture that passes proves nothing until a broken gate
makes it fail; the passing run and the six failing ones together are the evidence.
That is also why the analysis returns a `Report` rather than printing: the fixture
asserts on the verdict, so rewording an error message cannot silently pass it.

**What it does not do, learned on the first boot after it landed.** The gate
answers *can this command print this text*, never *will this run print it*. The
very first assertion written on its advice — `bright set 50` expecting
`Brightness → 50%` — passed the gate and panicked the kernel, because
`brightness::init_defaults()` is called only from the `show` arm and a fresh boot
has no display 1 to set; the command printed `Error: NotFound`. The rung was
missing a `bright show` opener, the way rung 74 opens with `tile init`. Worth
recording because it is the *old* guard's epitaph too: `Error: NotFound` lacks
`Usage:` exactly as happily as a working run does, so the assertion that gate
replaced had been passing on the error path for as long as it existed. The
lesson is that this gate narrows what a rung may *claim*; only a boot establishes
that the claim holds — and note the direction, since it yields a rung that fails
loudly on a correct kernel rather than one that passes while testing nothing.

**Cost of being wrong.** If the over-approximation is too generous the gate
misses a stale assertion and the tree is exactly where it was before the gate
existed: one boot to find out. If it were too strict it would block builds over
correct code, and the pressure would be to delete it. The asymmetry is the
argument.

**Where it came from.** The `adddc7459` regression, found the expensive way
during the `A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`
burn-down. Same shape as §299: the gate fires on the category — "this text
belongs to some other command" — not on the word `Usage`.
