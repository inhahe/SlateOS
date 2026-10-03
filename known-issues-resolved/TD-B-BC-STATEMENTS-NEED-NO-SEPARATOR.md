## TD-B-BC-STATEMENTS-NEED-NO-SEPARATOR (lane B, 2026-09-16) -- **Status: FIXED** 2026-09-16

**In short:** our `bc` accepts two statements written side by side with nothing
between them. `1 2` on one line prints `1` and then `2`; GNU calls it
`syntax error` and prints neither. Nobody writes `1 2` on purpose, so on its
own this is harmless — it matters because it is what stops a *typo* from being
reported. A stray character turns `1 $ 2` into `1 2` once the bad character is
removed, and where GNU then says `syntax error`, we say nothing and compute.

**Where:** `userspace/coreutils/src/bin/bc.rs`, `Parser::parse_stmt` and
`Parser::parse_program`. After a statement is parsed, `skip_terminator` is
called but nothing *requires* that a terminator was actually there, so the
next loop iteration simply starts a new statement wherever the last one
stopped.

**Measured**, GNU bc 1.07.1 under WSL:

| Input | GNU | Ours |
|---|---|---|
| `1 2` | `syntax error`, no output | `1` and `2` |
| `print 1 print 2` | `syntax error`, no output | prints both |
| `1 $ 2` | `illegal character: $` **and** `syntax error` | `illegal character: $` only |
| `1; 2` | prints both | prints both |
| `1\n2` | prints both | prints both |

**Why it is written up rather than fixed in the same change.** It is a
different defect from the one that was being fixed
(`TD-B-BC-SYNTAX-ERRORS-ARE-NEVER-REPORTED`, now closed): that one was *bc had
nowhere to record that a mistake had happened*, this one is *bc's grammar is
too permissive*. Fixing this needs its own measurement round, because the
question "what may legally follow a statement with no separator" has a real
answer that has to be established rather than guessed — `}` certainly may, and
whether `else` and a function body's closing brace may is exactly the sort of
thing that looks obvious and is not. Bolting a guess onto the end of the
reporting change would have risked rejecting valid programs, which is worse
than the bug: the current failure mode is over-acceptance of input nobody
writes, and the failure mode of a wrong fix is refusing input people do write.

**The proper fix.** Establish by measurement which tokens may follow a
statement without a separator, then make `parse_stmt` require one and
`record_error` otherwise. The single harness row
`prog 'bad character' '1 $ 2\n'` turns green when it is right; the control
`a_program_with_no_mistakes_in_it_says_nothing` in `bc.rs` is what catches a
fix that is too eager, and should be extended with whatever the measurement
turns up before the change is made, not after.

**Severity: low.** It is a missing *second* diagnostic in a case that already
gets a first one, and a wrong answer only for programs containing `1 2`, which
no one writes deliberately. It is recorded because the reporting work above
referred to it, and a reference to an entry that does not exist is how a known
gap becomes an unknown one.

### Fixed 2026-09-16

`Parser::require_terminator` replaces `skip_terminator` at every statement end,
refusing anything that cannot legally follow one.

**The followers were measured before a line was written**, exactly as this
entry asked, because the failure mode of an over-strict rule is refusing valid
programs — worse than the over-acceptance being fixed, and invisible to a suite
that only feeds it malformed input:

| after a statement | GNU |
|---|---|
| `;` or newline | the separators themselves |
| `}` | accepted — `{ print "a" }` |
| `else` | accepted — `if (1) print "a" else print "b"` |
| end of input | accepted |
| anything else | `syntax error` |

**Two results were not what reasoning would have produced.** A closing brace
ends a statement but does **not** license a following one: `{ 1 } 2`,
`if (1) { … } 2`, `while (0) { } 2` and `for (…) { } 2` are all refused. And a
**function definition is not a statement** in this sense — `define f() {
return (1) } f()` is accepted — because GNU's grammar makes a definition its
own input item. Guessing either way round would have produced a `bc` that
rejected real programs.

There is a third, learned from a failing test rather than from GNU: a
*braceless* body needs nothing extra, because `parse_stmt` has already consumed
a terminator for the statement it read, and that terminator is the enclosing
`if`'s as well. Requiring a second one rejected `if (1) print "a"` followed by
any next line at all. The braced case is the one that needs it, and it is
applied in `parse_block_or_stmt` where the `}` is consumed.

**This closes the gap the reporting work left.** `1 $ 2` now produces both of
GNU's diagnostics — `illegal character: $` from the scanner and `syntax error`
from the parser, because with the `$` dropped the parser really is looking at
`1 2`. Neither stage can produce the other's finding, which is what the
scanner/parser split was built for.

**Evidence.** `bc-diff.sh` 147 -> 159 passed, 0 differed, known bugs 3 -> 1.
The `bad character` row came back `KFIXED`. Five new rows carry both halves —
the refusals *and* the acceptances — so an over-strict rule cannot pass. The
147 pre-existing rows are themselves the control that no valid program was
refused: they are real `bc` programs and all of them still agree with GNU. 98
unit tests, including a `requiring_a_separator_does_not_reject_valid_programs`
case listing fifteen accepted forms.
