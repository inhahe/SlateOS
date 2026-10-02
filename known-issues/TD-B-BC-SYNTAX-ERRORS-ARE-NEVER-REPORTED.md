## TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED (lane B, 2026-08-24) -- **Status: FIXED** 2026-09-16

**In short:** feed our `bc` a program with a mistake in it — a stray `)`, a
character that is not part of the language, a quote that is never closed — and
it says nothing at all, computes something, and exits 0. GNU `bc` names the
file and the line and says `syntax error`. So a script with a typo in it
silently produces a *wrong answer* on ours, and a shell pipeline that checks
`bc`'s exit status is told everything went fine. This is the most damaging of
the differences the new differential harness found, because every other one is
a wrong *message* where this one is a wrong *number* with no message.

**Where:** `userspace/coreutils/src/bin/bc.rs`. Four places conspire, and all
four are the same decision — *recover silently and keep going* — taken without
anywhere to record that recovery happened:

| Site | What it does now |
|---|---|
| `Parser::parse_primary`, the `_` arm | returns `Expr::Number("0")` for any token that cannot start an expression, without advancing |
| `Parser::parse_stmt`, the `_` arm | advances past an unexpected token and returns `None` |
| `Parser::expect` | returns `false` and carries on when the required token is absent |
| `Lexer::next_token` | has no "illegal character" token; unknown bytes are skipped |

**Measured**, GNU `bc` 1.07.1 under WSL, every case both as a file operand and
on standard input (`scripts/bc-diff.sh`, marked `known_bug` with this key):

| Input | GNU stderr | GNU stdout | Ours |
|---|---|---|---|
| `print )` | `prog.bc 1: syntax error` | *(none)* | *(nothing at all, exit 0)* |
| `1 $ 2` | `illegal character: $` then `syntax error` | *(none)* | `1` and `2`, exit 0 |
| `print "abc` *(unterminated)* | `illegal character: "` | *(none)* | `abc`, exit 0 |
| `if (1) {` / `print "A"` *(unclosed)* | `syntax error` | *(none)* | `A`, exit 0 |
| `print )` then `print "after\n"` | `syntax error` | `after` | `after`, no diagnostic |
| `print "A"` with **no trailing newline** | `syntax error` | *(none)* | `A`, exit 0 |

Note the last row: GNU requires the final newline and treats its absence as a
syntax error, which is worth knowing before "fixing" it in the obvious
direction.

Note also that GNU **keeps going** after an error — row 5 still prints `after`
— so this is not "stop at the first problem"; it is "say so, then continue".

**The proper fix.** The parser needs an error channel, which it has never had.
Concretely:

1. `Lexer` gains a `Token::Illegal(u8)` for a byte that starts no token, and
   reports an unterminated string the same way rather than running to end of
   input. GNU's wording is `illegal character: X`.
2. `Parser` accumulates a `Vec<SyntaxError>` carrying a line number, rather
   than silently recovering. `parse_primary`'s zero, `parse_stmt`'s skip and
   `expect`'s `false` each record one first.
3. The chunk runner writes them, prefixed as GNU prefixes them — the **file
   name** for a file operand (`prog.bc 1: syntax error`) and the literal
   `(standard_in)` for the session on stdin — and then runs the chunk anyway,
   because that is what GNU does.
4. The line number counts within the *whole input*, not within the chunk, so
   `Chunker` has to carry a running line count.

The exit status stays 0: GNU exits 0 after a syntax error, which was measured
and is the one part of the current behaviour that is already right.

**Blast radius:** the six harness rows above, plus every `parse_primary`
fallback taken in a program that is actually valid — of which there should be
none. So an implementation that is too eager shows up immediately as some
other, currently-green harness row turning red.

### Fixed 2026-09-16

Done as prescribed above, with one structural difference and two corrections to
the measurements in this entry.

`Token::Illegal(u8)` now exists, the scanner counts lines in `Lexer::bump` —
the one place the cursor ever moves, so the three separate branches that
consume newlines cannot disagree — and `Parser` carries a `Vec<SyntaxError>`
that `parse_primary`, `parse_stmt` and `expect` all write to. `Chunker` carries
the running line base, so a diagnostic names its line in the *file* rather than
in the unit.

**The structural difference: a unit with an error in it does not run at all.**
The entry proposed printing the errors and running the chunk anyway. That is
not what GNU does, and the difference is the whole severity of this bug.
Measured: `1; $ 2` prints *nothing* — not even the `1`, which is a finished
statement sitting before the mistake — while `1\n$ 2\n` prints `1`, because
there the good statement is on its own line. So the thing discarded is the
whole unit. `Feed` grew a `Failed(Vec<SyntaxError>)` variant carrying no
statements, which makes "a statement with a mistake in it does not run" a
property of the type rather than a rule every caller has to remember.

**Two corrections to the table above, both found by re-measuring rather than
by reading it.** They are recorded because the table was written from a real
run and was still wrong, which is the more useful lesson:

1. `illegal character` **does** carry the `NAME LINE: ` prefix —
   `prog.bc 1: illegal character: "`. The table shows it bare. Implementing
   from the table would have produced a diagnostic missing its prefix on
   exactly the two rows that have one.
2. Rows 3 and 6 report `(standard_in) 1:` **even for a file operand**, and
   even when the construct opens on line 4. That is not "the line the
   construct started on" — it was checked with a 5-line file. At end of input
   GNU's reporter reads a file handle and a line counter it has already torn
   down, so it names neither the right file nor the right line.

**Not copied, and now marked `differs_by_design` in the harness rather than
`known_bug`:** that at-EOF misnaming, and GNU's treatment of a missing final
newline as a syntax error. Both are GNU bugs. Filing them as known bugs would
have put two entries in this file describing work nobody intends to do.

**What is left is a different bug**, split out as
`TD-B-BC-STATEMENTS-NEED-NO-SEPARATOR`: `1 $ 2` gets `illegal character: $`
but not the `syntax error` GNU also prints, because with the `$` dropped our
grammar happily accepts `1 2` where GNU refuses it. That is a grammar gap, not
a reporting gap, and it is the only row of the six still marked `known_bug`.

**Evidence.** `scripts/bc-diff.sh` went from `129 passed, 17 known bugs` to
`135 passed, 13 known bugs, 8 differ on purpose`, with the six rows for this
key reported `KFIXED` in between — the harness fails on its own stale markers,
which is how the closing of this entry was forced rather than remembered. The
discrimination check the harness documents, `OURS=/usr/bin/bc`, turns all 13
remaining known bugs into `KFIXED` and all 8 deliberate differences into
`XPASS` and nothing else, so the suite can still tell agreement from
disagreement. 91 unit tests in `bc.rs`, including a control asserting that nine
*correct* programs produce no diagnostics at all — every other test here
asserts that `bc` complains, and all of them would pass on a `bc` that
complained about everything.
