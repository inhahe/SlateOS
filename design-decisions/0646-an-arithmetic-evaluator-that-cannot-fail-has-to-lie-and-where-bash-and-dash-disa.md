## 646. An arithmetic evaluator that cannot fail has to lie, and where bash and dash disagree about the lie, dash is right

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** kshell can do arithmetic — `expr 2 + 3`, `$((count * 2))`,
`if [ "$n" -gt 5 ]`, `for ((i=0; i<10; i=i+1))`. The code doing it had no way to
say "I couldn't". So when it met something it could not work out — a word that
isn't a number, a division by zero, a missing bracket — it answered **0** and
reported success. Zero is a perfectly ordinary answer to a sum, so neither a
person nor a script could tell a real zero from a failed one. This entry records
giving it a way to say "I couldn't", and the two places where the obvious
reference implementation (bash) does the wrong thing and we follow dash instead.

### What was wrong

`eval_arithmetic` returned an `i64` — a plain number, with no room in the return
value for "no answer". Every failure therefore had to be spelled as some number,
and the number chosen was `0`:

| You typed | It printed | It should have said |
|---|---|---|
| `expr 1O + 1` (letter O for zero) | `1` | `1O` is not a number |
| `expr 1 / 0` | `0` | division by zero |
| `expr 2 * (3 + 4` | `14` | missing `)` |
| `expr 6 & 3` | `6` | `&` is not an operator here |
| `expr 1 2` | `1` | unexpected trailing operand |
| `x=abc; echo $((x * 100))` | `0` | `abc` is not a number |
| `echo $((99999999999999999999))` | `0` | doesn't fit in a 64-bit integer |

The exit status was `0` in every one of those rows too, so the failure was
invisible to a script as well as to a reader. The `1 / 0` row is the sharpest:
`0` is exactly what a *correct* division would produce for a zero numerator, so
the wrong answer is indistinguishable from the right one in the case most likely
to arise.

This is design-decisions.md §600's prohibited shape — substituting an invented
value for a word that could not be read — but sitting under the shell's entire
expression language rather than under one command's option.

### The decision

Every function in the evaluator returns `Result<i64, ArithError>`, and each of
the eleven call sites decides what a failure means *there*:

| Caller | On failure |
|---|---|
| `$((…))` expansion | substitute nothing (not `0`, which is how `rm /tmp/x$((n))` became `rm /tmp/x0`) |
| `((…))` command | print, status 1 |
| `let` | print, status 1, and **leave the variable alone** rather than overwriting it with the guess |
| `expr` | print, status **2** |
| `test` / `[` | print, status **2** |
| `for ((…))` | print; don't start the loop, or stop it |
| `${x:off:len}` | print, expand to nothing |

### Why `test` and `expr` exit 2 rather than 1

Because `1` already means something else. For `test`, `1` is *the condition was
false* — so if a malformed expression also exited 1, then `if [ "$n" -gt 5 ]`
with a mistyped `$n` would take the else-branch silently, which is the exact
outcome an operator would never think to check. bash and dash both reserve 2 for
this and GNU `expr` does the same. Collapsing the two is how a typo becomes a
skipped branch.

### Where bash and dash disagree

Two cases, and we follow dash both times.

**A variable holding something that is not a number** — `x=abc; echo $((x))`.
bash prints `0`; dash refuses with `Illegal number: abc`. bash's answer is
literally the invented value §600 exists to prohibit, and it is the one that
turns a misspelled configuration value into a plausible-looking measurement, so
dash wins. (bash's actual rule is that it re-evaluates the *value* as an
expression, which is a recursive-evaluation feature this shell does not have and
should not grow on the strength of one edge case.)

**A literal too large for `i64`** — `$((99999999999999999999))`. bash wraps to
`7766279631452241919`; dash saturates to `9223372036854775807`. Neither produces
`0`, and **they do not agree with each other** — which is the clearest available
evidence that there is no correct value to substitute. Refused.

**An *unset* variable stays `0`**, which is not a guess but the documented rule
in every shell, and `$((count + 1))` on a fresh variable has to mean `1`.

### Alternatives considered

| Option | Why not |
|---|---|
| Keep returning `i64`, add a separate "did it fail" flag the callers may check | A flag callers *may* check is a flag callers *will* forget; the type system should make the failure unignorable. This is what `Result` is for. |
| Return `i64` but make failures return a sentinel like `i64::MIN` | A sentinel is a guessed value with extra steps, and §600 already has a named sub-shape for "sentinels chosen as cannot-exist". `i64::MIN` is a legal result of `-(2**63)`. |
| Follow bash exactly, including its `0` for non-numeric variables | Bug-compatibility with the reference is a real argument — scripts are written against bash. But this shell is a *debug* shell with no script corpus, nothing in-tree runs kshell scripts, and adopting the one bash behaviour that is itself the defect under burn-down would be perverse. |
| Refuse the unset variable too, for consistency | Would break `$((count + 1))` on a fresh variable, which is the single commonest use of shell arithmetic, and disagrees with *every* shell rather than one of them. |

### Cost

`test`/`expr` returning 2 where they previously returned 0 or 1 is a
user-visible behaviour change, and a script that tested `$? -eq 1` to mean
"false" would now see 2 for malformed input. That is the intended effect — it is
the difference the change exists to expose — and there is no such script in the
tree: nothing in-tree runs kshell scripts, there are no shipped `.ksh` files and
no `include_str!` embeds, and before rung 111 there was no self-test rung
touching `expr`, `let`, `test`, `$((…))` or C-style `for` at all. That absence is
also why the defect survived forty-two batches: there was nothing observing the
evaluator to notice that every failure looked like a success.

### How to reverse it

Change the signatures back to `i64` and drop `ArithError`, `report_arith` and
rung 111. Note what reversing reinstates: not a noisy behaviour but a **silent**
one — the shell would go back to answering `0` and exiting `0`, with nothing in
the output to indicate it had failed. That is what the rung is there to prevent.
