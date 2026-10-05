### BUG-OILS-EVAL-LINE-BASE. `eval` numbers its string's lines from 1 instead of continuing the caller's — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.**

```sh
echo x
eval "echo a
v='abc"
```
```
bash: x / a / case.sh: eval: line 4: unexpected EOF while looking for matching `''
osh : x / a / case.sh: eval: line 2: unexpected EOF while looking for matching `''
```

**bash's rule** (measured over 7 probes): line 1 of the eval string *is* the
script line on which the `eval` command **ended**, i.e. reported line =
`LINENO(eval) - 1 + line_within_string`. Confirmed independently by `$LINENO`
inside the string: `eval "echo A$LINENO\necho B$LINENO"` on script lines 1–2
prints `2` then `3` in bash, `1` then `2` in osh.

| eval at script lines | line in eval string | bash reports |
|---|---|---|
| 1 | 1 | 1 |
| 1–2 | 2 | 3 |
| 3 | 1 | 3 |
| 3–5 | 3 | 7 |
| 1 | EOF (`$(`, 1-line string) | 2 |
| 1–2 | EOF (`$(`, 2-line string) | 4 |

**Second, separable divergence this exposed:** osh's `$LINENO` for a command
spanning several physical lines was the **first** line; bash's is (almost
always) the **last**. That is what makes the offset `LINENO(eval) - 1` rather
than `first_line(eval) - 1`, so both had to be fixed together.

**bash's per-command line rule** (measured over ~25 further probes). bash
stamps a simple command with `line_number` as it stands when its grammar
reduces the command's *first element*, and `line_number` names the last input
line bash has **fetched** — i.e. the line the token just read *ends* on:

* A leading plain word forces **one token of lookahead**, because bash cannot
  yet tell a simple command from a function definition (`WORD '(' ')' …`). So
  the number comes from the token *after* the command word.
* A leading assignment (`v=1 cmd …`, `a=(1) cmd …`) reduces on sight and is
  numbered by its own last line.

| source | bash `$LINENO` |
|---|---|
| `echo "a⏎b" $LINENO` | 2 |
| `echo $LINENO "x⏎y"` | 1 |
| `echo A$LINENO \⏎B` | 1 |
| `echo \⏎$LINENO` | 2 |
| `v=1 echo "a⏎b" $LINENO` | 1 |
| `v="a⏎b"; echo $LINENO` | 2 |
| `nosuchcmd` alone on line 2 | 2 |

The last row is why the recorded line must be the line of a token's *last
character* rather than the line after it: reading a token that ends at a
newline never forces the next line to be fetched.

**Fix (3 parts).**

1. `userspace/oils/src/lexer.rs` `Lexer::stamp_lines` — the per-token `lines`
   vector now records the line each token **ends** on (newlines strictly before
   the iteration's final character), not the one it starts on. This alone fixes
   assignment-led and `Item`-level numbering.
2. `userspace/oils/src/parser.rs` `Parser::simple_command_line` — new helper
   used by `parse_simple`: the line of the token at `pos + 1` unless the first
   token is an assignment word / `ArrayAssign`, in which case `pos`. This
   reproduces bash's function-definition lookahead.
3. `userspace/oils/src/interp.rs` — the `eval` builtin passes
   `self.current_line - 1` as `line_base` to `run_source_flow_out` instead of
   `0`. `current_line` is already absolute, so nested evals compose.

Plus `Lexer::eof_line` (new): the "end of input" line used for an unterminated
`$( … )` / `<( … )` is one past the last line **counting a final line with no
newline of its own** — which every `eval` string is.

**Regression tests.** `tests/corpus/lineno-multiline.sh` (14 measured
expectations covering all of the above end-to-end);
`parser::tests::simple_command_line_follows_bashs_lookahead_rule` (9 rows);
`interp::tests::lexer_error_carries_the_constructs_opening_line` (the two
no-trailing-newline EOF rows).
