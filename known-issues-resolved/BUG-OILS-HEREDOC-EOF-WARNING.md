### BUG-OILS-HEREDOC-EOF-WARNING. An unterminated here-document produced no warning at all — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/lexer.rs` (`collect_heredocs`, `Lexer::fetched_line`,
`HeredocEof`, `Tokenized::heredoc_eof`), `userspace/oils/src/parser.rs`
(`IncrementalParser::take_heredoc_eof`) and `userspace/oils/src/interp.rs`
(`Shell::err_prefix_at`, the emission point in `run_source_flow_out`).

**Symptom.** When input ran out before a here-document's delimiter was seen, bash
warned and then ran the command with the partial body; osh collected the same body
and ran the same command in silence.

```
$ bash -c 'cat <<EOF
body'
bash: line 2: warning: here-document at line 1 delimited by end-of-file (wanted `EOF')
body
```

**The two line numbers turned out to be one rule.** This entry originally described
them as different things — "where input ran out" and "the line whose newline began
the body" — and gave the second an awkward off-by-one definition ("one before the
body's first line"). Measuring more shapes showed both are the *same* quantity
sampled at two moments: **the last input line bash had fetched**, once when body
collection began and once when the input ran out. bash reads a whole line at a
time, so:

* a cursor anywhere *within* a line means that line has been fetched;
* a cursor sitting exactly at a line's start has only consumed the previous line's
  newline — the line it is poised on has not been asked for yet.

That single rule (`Lexer::fetched_line` = `cur_line()`, minus one at a line
boundary) reproduces every measured case, including the ones the old description
could not:

| input | prefix line | message line | why |
|---|---|---|---|
| `cat <<EOF` / `body` | 2 | 1 | ends mid-line 2; body began at line 2's start |
| `cat <<EOF` / `body` / `` | 2 | 1 | the trailing newline leaves the cursor at a boundary, so the empty line past the end is never fetched |
| `echo hi` / `echo ho` / `cat <<EOF` | **3** | **3** | empty body: the operator's line is the last one either way — the case proving the prefix line is *not* the message line plus one |
| `cat <<A <<B` / `one` / `A` / `two` | 4 | 3 | B's body began where A's terminator left the cursor |
| `cat <<A <<B` / `one` | 2 | 1 (A), **2** (B) | A stops at EOF *mid-line*, so B's body "begins" on line 2 |
| `cat <<A <<B` / `one` / `` | 2 | 1 (A), 2 (B) | same answer, by the boundary rule instead |
| `cat <<EOF` / `body` / `EOF` / `cat <<X` / `more` | 5 | 4 | absolute, not relative to the operator |

Note this is deliberately **not** `Lexer::eof_line`'s rule (one *past* the last
line). bash bumps `line_number` for an input line it asks for and does not get,
which is why an unclosed `$( … )` is blamed one past the end — but a
here-document's reader has already stopped by then and reports the line it last
had.

**Timing is observable in two directions,** because the warning comes from bash's
*reader* rather than from the parser or the command:

* it must land **after** the output of earlier lines (`echo hi` / `cat <<EOF` /
  `body` prints `hi` first) and after a `set -v` echo of the lines it read, but
  **before** the unit runs and before a syntax error the same unit also carries
  (`f() { cat <<EOF` warns, *then* reports the unexpected end of file);
* it must **not be printed at all** when a syntax error on an earlier line means
  bash never reads that far: `echo one )` followed by an unterminated here-document
  prints only the syntax error.

osh lexes a whole input up front, so neither falls out for free. The lexer records
each cut-off here-document (`HeredocEof`, carrying the delimiter, both lines, and
the `tok_index` of its placeholder token); `IncrementalParser` holds the records
back and releases one only when the parse unit containing that token is handed out,
and *discards* them when a parse error makes it abandon the rest of the input. The
reader-level driver `run_source_flow_out` drains and prints them between the
`set -v` echo and the unit's execution.

The prefix is `Shell::err_prefix`'s shape, not `syntax_error_prefix`'s: bash prints
`bash: line 2: warning: here-document …` with **no** `-c`/`eval` input-source
token, even from `-c` and even inside an `eval` (the token belongs to bash's
`parser_error`; this is a plain `internal_warning`). `err_prefix` was refactored
into `err_prefix_at(line)`, since the warning's line is the reader's rather than
the currently-executing command's.

**Verified** byte-identical to MSYS bash 5.2 across eighteen contexts: `-c`, a
script file, `.`/`source`, `eval`, a plain `( … )` subshell, piped stdin, CRLF
input, `<<"EOF"`, `<<-EOF`, `cat <<EOF; echo after`, two here-documents (one
terminated, both terminated, neither), an empty body, an unterminated function
body, an unterminated loop body, under `set -v`, under `set -x`, and under
`set -e`. Tests: `unterminated_heredoc_records_both_warning_lines` (lexer, the
line table above), `heredoc_eof_warning_is_released_with_its_own_unit` (parser, the
release timing including the suppression case), and the
`tests/corpus/heredoc-eof-warning.sh` corpus case, whose every probe is an `eval`
or a sourced file because each needs an input of its own that *ends*.

**Found** while measuring TD-OILS-SYNTAX-ERR-ERREXIT-ECHO, which needed a
single-line diagnostic to check that errexit leaves those alone; `cat <<EOF` was
the example chosen, and it produced no line at all.
