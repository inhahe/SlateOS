### BUG-OILS-LEX-ERROR-LINE. An unterminated quote/substitution was always blamed on line 1, and suppressed every command before it — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** A script whose last line opens a quote it never closes:

```sh
echo one
echo two
v='abc
```

```
bash: one / two / case.sh: line 3: unexpected EOF while looking for matching `'' / status 2
osh : case.sh: line 1: unexpected EOF while looking for matching `''  / status 2
```

Two independent divergences: the line number was always 1, and `one`/`two`
never ran even though bash had already executed them.

**Root cause.** `LexError` was a bare `pub struct LexError(pub String)` — no
line at all — so `format_parse_error` fell back to `self.current_line`, which
is still 1 because nothing has executed yet. And `IncrementalParser::new`
tokenized the *whole* source up front and returned `Err` on any lex failure,
so the incremental parse/execute model (landed as
BUG-OILS-WHOLE-SCRIPT-PREPARSE) never got to hand out the complete lines
that precede the bad one. bash instead reads, parses and executes one logical
line at a time, so it only discovers the unclosed construct when it reaches
that line — by which point everything before it has run.

**bash's line-attribution rules** (measured over ~30 probes against bash
5.2.37, not assumed):

| construct | line reported |
|---|---|
| `'…`, `"…`, `` `… ``, `$'…`, `${…`, `$[…`, `$((…`, `a=(…` | the line the construct **opened** on |
| `$( … )` and `<( … )` / `>( … )` | **end of input** (one past the last source line) |
| nested constructs | the **innermost** one that hit EOF wins |

The `$( … )` exception is structural: bash scans the substitution body
without parsing it and re-parses it only after the outer word is complete, so
the missing `)` surfaces at EOF rather than at the `$(`. Execution
granularity is the *logical line*: everything on complete lines before the
bad one has run, and nothing on the bad line runs
(`echo four; echo five 'abc` prints neither).

**Fix.**
* `lexer.rs`: `LexError` became `{ msg: String, line: Option<u32> }` with
  `new()` and an `at(line)` that only ever *fills* an empty line — so as the
  error unwinds through nested scanners the innermost construct keeps its
  own line. Every scanner (`read_single_quote`, `read_ansi_c_quote`,
  `read_double_quote`, `read_dollar_brace` and its nested `'`/`"`/`` ` ``
  loops, `read_arith`, `read_backtick`, `try_array_assign`, the `$[` and
  `${…}`-nested `read_balanced` callers) captures its opening line and stamps
  it; the `$( )` and `<( )`/`>( )` callers deliberately stamp
  `self.cur_line()` — the EOF line — instead.
* `lexer.rs`: `Lexer::run` split into `run` + `run_into(&mut out, &mut lines)`
  so a failed lex keeps the tokens produced so far. New `tokenize_deferred`
  returns `Tokenized { toks, lines, err }`, truncating the token stream back
  to the last complete `Newline` before the failure (and before any pending
  here-doc), so what remains is exactly the lines bash would already have run.
* `parser.rs`: `From<LexError> for ParseError` carries the line across (only
  for whole-source lexers — the fragment lexers restart at line 1 and keep
  dropping it). `IncrementalParser::new` is now infallible: it uses
  `tokenize_deferred` and parks the error in `pending_lex_err`, which
  `next_unit` surfaces only once every complete preceding unit has been handed
  out. A grammar error that only happened *because* the stream was truncated
  (`if true; then` with its body cut off) yields to the parked lex error —
  gated on the parser having run out of tokens, so a genuine earlier grammar
  error (`echo one )` on line 1) still wins.

**Regression tests.** Corpus cases `lex-error-line-quote.sh` (opening-line
rule + both execution-granularity rules), `lex-error-line-cmdsub.sh` (the
`$( )` end-of-input exception) and `lex-error-line-nested.sh`
(innermost-wins); unit tests
`interp::tests::lexer_error_carries_the_constructs_opening_line` and
`interp::tests::lex_error_is_deferred_until_earlier_lines_have_run`.

**Two adjacent gaps this uncovered, tracked separately below:**
BUG-OILS-REPL-LINE-BASE and BUG-OILS-EVAL-LINE-BASE.
