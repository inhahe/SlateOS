### TD-OILS-CMDSUB-ARITH-VS-SUBSHELL. `$((` is committed to arithmetic with no backtrack, so `$(( cmd ) | cmd )` fails to parse — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/lexer.rs` — `read_arith`, reached because
`read_dollar` commits to `$((` on sight.

**Symptom.** `$(( ` and `$( (` are ambiguous until the close, and bash backtracks:

```
$ bash -c 'x=$(((1 << 3)); echo "x=[$x]"'
bash: -c: line 1: unexpected EOF while looking for matching `)'
$ osh -c '…'
osh: line 1: syntax error: malformed arithmetic expansion
```

Both reject that one and both score 2, so it looked message-only at first. It is not.
A second shape, found while measuring the `case` fix, is ordinary shell that bash
**runs** — a subshell as the first element of a pipeline inside a substitution:

```
$ bash -c 'x=$(( echo a; echo b ) | tr a-z A-Z); echo "x=[$x]"'
x=[A
B]
$ osh -c '…'
osh: -c: line 1: syntax error: malformed arithmetic expansion
```

With a space (`$( (`) osh gets it right, so the whole divergence is the missing
backtrack. (The first guess when logging this was that bash rejects the un-spaced
form too — measuring it is what showed otherwise. The `$(((1 << 3))` case above
*is* rejected by both, but only because the arithmetic reading also fails.)

**Impact.** Any `$((` that is really a substitution containing a parenthesised group:
`$(( cmd ) | cmd )`, `$(( cmd ) && cmd )`, `$(( cmd ); cmd )`. bash runs all of them.

**Fixed.** `read_dollar`'s `$((` arm now does bash's backtrack: it records the
position of the *inner* `(`, reads the text as arithmetic, and on failure rewinds
there and reads it again as a substitution body. Only `self.pos` has to be
restored — `cur_line()` is derived from it and the arithmetic scan records nothing
else — which is what makes the rewind safe rather than merely convenient.

Two details came out of measuring rather than reasoning:

- **What does *not* backtrack.** `$(( echo a ))` reaches its `))`, so it stays
  arithmetic and fails at *evaluation* (`echo a : syntax error in expression`),
  exactly as in bash. Reaching the end is the whole decision procedure; looking
  shell-like has nothing to do with it. Add one paren — `$(( echo a ) )` — and
  the scan runs off the end, so it rewinds and runs.
- **Where an unterminated one is blamed.** A plain `$( … )` is reported one line
  *past* the last, because its body is re-parsed after the outer scan. A
  backtracked `$((` is reported on its *opening* line: bash has already failed
  the arithmetic reading by then and stamps that failure at the `$((`. So the
  rewind captures `cur_line()` before the attempt and uses `.at(open)`.

Covered by `lexer.rs::dollar_paren_paren_backtracks_to_a_substitution` and the
corpus case `tests/corpus/arith-vs-subshell.sh`.

**Two shapes deliberately left divergent**, both logged separately below:
`TD-OILS-ARITH-GROUP-ERROR-TOKEN` (an arithmetic error token differs when the
expression contains a parenthesised group — pre-existing, unrelated to the
backtrack) and `TD-OILS-CMDSUB-ARITH-CASE-BASH-BUG` (bash cannot read
`$(( case … ) | cat)`, which osh runs).
