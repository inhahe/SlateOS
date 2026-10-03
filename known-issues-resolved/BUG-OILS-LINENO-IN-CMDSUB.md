### BUG-OILS-LINENO-IN-CMDSUB. `$LINENO` restarts at 1 inside `$( … )` — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Where:** `userspace/oils/src/lexer.rs` (`Seg::CmdSub`, `Lexer::cur_line`) and
`userspace/oils/src/parser.rs` (`parse_cmdsub_body`, `CmdSubLineMap`). Corpus
case: `userspace/oils/tests/corpus/lineno-cmdsub.sh` (it was waived with
`# EXPECT-DIFF` while this was open; the waiver went stale the moment the fix
landed and the harness said so, which is exactly what that mechanism is for).

**Symptom.** For this script:

```
1  echo "L1=$LINENO"
2  v=$(
3  echo "a=$LINENO"
4  echo "b=$LINENO"
5  )
6  echo "$v"
7  echo "L7=$LINENO"
```

bash prints `L1=1`, `a=5`, `b=6`, `L7=7`; osh prints `a=1`, `b=2` — the
substitution body restarts its own numbering. It shows up in diagnostics too:
`v=$(echo x > /nodir/f)` says `line 1:` in osh where bash names the real line.

**bash's actual rule** (reverse-engineered from 11 probes against bash 5.x, not
from the manual — it is an artifact of bash re-parsing the body after the outer
command has already been scanned):

> Inside `$( … )`, `$LINENO` = *(source line of the closing paren)* + *(0-based
> rank of the body line among the body's **command-bearing** lines)*.

The second term is a rank, **not** an offset: blank lines inside the body do not
advance it. Both halves are needed — each of these was measured:

| Body | bash |
|---|---|
| `x=$(echo $LINENO)` on line 1 | `1` (rank 0, close line 1) |
| `y=$(echo $LINENO` line 3 / `echo $LINENO)` line 4 | `4`, `5` — both offset from the *close* line, not the start |
| `z=$(` line 6 / `echo $LINENO)` line 7 | `7` — the empty remainder of line 6 does not count |
| `z=$(` line 1 / blank line 2 / `echo $LINENO)` line 3 | `3` — the blank line does not count either |
| `q=$(echo $LINENO` line 5 / blank / `echo $LINENO)` line 7 | `7`, `8` — ranks 0 and 1, not 0 and 2 |
| `r=$(echo $LINENO; echo $LINENO` line 1 / `echo $LINENO)` line 2 | `2`, `2`, `3` — two commands on one line share a rank |

Any fix must reproduce *this*, not the intuitive "line where the body actually
is" — byte-fidelity with bash is the whole point of the differential harness.

**Fix.** `Seg::CmdSub` now carries the source line of its closing delimiter as a
second field, and `seg_to_part` parses the body through `parse_cmdsub_body`,
which rebases the per-token `lines` vector `tokenize_spanned` produced:
`CmdSubLineMap` assigns each distinct line carrying a non-`Newline` token the
number `close_line + rank`, and every other line (blank lines, and the
continuation lines of a multi-line token) takes the nearest preceding
command-bearing line's number. One mechanism reproduces both the base and the
blank-line collapse.

One trap worth recording: the recipe originally written here assumed
`Lexer::line` was already the close-paren line at the `Seg::CmdSub` push sites.
It is not — `line` is advanced only once per `run` iteration, in `stamp_lines`,
so mid-token it still names the line the *word* started on. Hence the new
`Lexer::cur_line`, which adds the newlines consumed since `Lexer::iter_start`
(the character index at which the current iteration began).

Nested substitutions compose because the rebasing rewrites the close lines of
any `Seg::CmdSub` nested inside the body's tokens *before* handing them to the
recursive parse, so an inner `$( … )` is numbered relative to the outer body's
already-rebased lines rather than to its own first line.

**Verified:** `x=$(echo $LINENO)`, the two-line and blank-line forms, two
commands sharing a body line, backticks, `$( $( … ) )`, a substitution inside a
function body, a comment-only body line, and a `$( … )` inside a here-doc body
all now print byte-identically to bash 5.x — as do the diagnostics raised inside
a body (`v=$(\nnosuchcommand 2>&1\n)` says `line 3:`, the close-paren line, in
both). Regression tests:
`interp::tests::lineno_inside_command_substitution_counts_from_the_closing_paren`
plus the un-waived corpus case. Suite 729 + 15 pass, clippy clean, 33/33 corpus
cases match.
