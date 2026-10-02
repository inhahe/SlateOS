## TD-B-BC-RUNTIME-ERROR-WORDING-DIFFERS-FROM-GNU (lane B, 2026-08-24) -- **Status: FIXED** 2026-09-16

**In short:** when a calculation goes wrong — dividing by zero, taking the
square root of a negative number, calling a function that was never defined —
both `bc`s complain and both keep going. They word it differently, and ours
leaves out the two facts GNU includes: *which function* the fault was in, and
*where inside it*. A script that greps `bc`'s stderr, or a user diffing against
a reference output, sees a difference.

**Where:** `userspace/coreutils/src/bin/bc.rs`, `impl Display for RuntimeError`
and the `Runtime error: ` prefix at its call site.

**Measured**, GNU `bc` 1.07.1:

| Input | GNU | Ours |
|---|---|---|
| `1/0` | `Runtime error (func=(main), adr=3): Divide by zero` | `Runtime error: divide by zero` |
| `sqrt(-1)` | `Runtime error (func=(main), adr=4): Square root of a negative number` | `Runtime error: square root of a negative number` |
| `f(1)`, `f` undefined | `Runtime error (func=(main), adr=3): Function f not defined.` | `Runtime error: undefined function f` |

Three separate differences: the missing `(func=…, adr=…)` parenthesis, the
sentence case (GNU capitalises the first word), and the phrasing of the
undefined-function case, which in GNU is a sentence ending in a full stop.

**The proper fix**, and the reason it is not a one-liner: `adr=` is the *byte
offset into GNU's compiled dc program*, and we do not compile to dc — we walk a
tree. There is no honest value to put there. Two ways out:

* **(a) Emit the prefix with a counter of our own** — statements executed within
  the current function, say.
  *What changes:* the shape matches GNU and the number does not; it would be a
  number that looks meaningful and is not.
* **(b) Emit `func=` and drop `adr=`.**
  *What changes:* `Runtime error (func=(main)): Divide by zero` — honest, still
  a divergence, and the divergence is one field rather than a fake value.

Recommendation: **(b)**, plus fixing the capitalisation and the
undefined-function sentence, which are unambiguous. `func=` we can produce
truthfully — the interpreter always knows which function body it is in, or
`(main)`. Then record the missing `adr=` as a deliberate divergence and move
these three harness rows from `known_bug` to `xfail` with that reason.

Do this **after** `TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED`, which builds the
diagnostic plumbing (file name vs `(standard_in)`, line numbers) that this
should reuse rather than duplicate.

### Fixed 2026-09-16

Recommendation (b) taken, and recorded as `design-decisions.md` §1025 —
**omit a field we cannot produce honestly rather than fill it.** Output is now
`Runtime error (func=(main)): Divide by zero`, matching GNU in everything but
the absent `adr=`.

**`adr=` is not a line number, which was measured rather than assumed.** `1/0`
reports `adr=3` whether it is the first line of the file or the fourth, and
`sqrt(-1)` reports `adr=4` — the offsets are into the `dc` program GNU compiles
each *statement* into, and the counter restarts per statement. So the obvious
substitute would have agreed with GNU by coincidence on one-line scripts and
disagreed everywhere else, which is worse than an absent field because nothing
in the output would say so.

**`func=` is produced truthfully and matches GNU in all six cases measured**,
including the two easy to get backwards:

| Script | GNU | Ours |
|---|---|---|
| `1/0` | `func=(main)` | same |
| `f(1)`, `f` undefined | `func=(main)` | same |
| `g` faults, called from `f` | `func=g` *(innermost wins)* | same |
| `g` undefined, called from inside `f` | `func=f` *(callee has no body to be inside of)* | same |
| fault in `f`'s own body | `func=f` | same |
| `g(1)` then `1/0` | `func=g` then `func=(main)` | same |

That last row is why `Interpreter::run` clears `fault_fn` before **every
statement** rather than after printing: without it the second fault inherits
the first one's frame and blames a function it never entered. It has a test of
its own, with a control asserting a script whose only fault *is* in `g` still
says `g` — otherwise the assertion would pass on an implementation stuck on
`(main)`.

**The wording is capitalised, not reworded.** `DecimalError`'s text is shared
with `dc` deliberately — its own comment says so — and GNU words these
differently in the two programs: `bc` says `Divide by zero` and `Square root of
a negative number`, `dc` says `divide by zero` and `square root of negative
number`, without the `a`. The shared strings already match *bc*'s wording apart
from the leading capital, so `bc` capitalises at its own layer and no second
copy of the sentence exists to drift. Only `UndefinedFunction` is restated,
because GNU's is a different sentence ending in a full stop:
`Function f not defined.`

**Evidence.** `scripts/bc-diff.sh`: known bugs 13 -> 7, differ-on-purpose 8 ->
14, 0 differed. The three rows are now `differs_by_design` with the `adr=`
reason rather than `known_bug`, because they describe a divergence nobody
intends to close. The discrimination check `OURS=/usr/bin/bc` turns all 7
remaining known bugs into `KFIXED` and all 14 deliberate differences into
`XPASS` and nothing else. 94 unit tests in `bc.rs`, 0 failed.
