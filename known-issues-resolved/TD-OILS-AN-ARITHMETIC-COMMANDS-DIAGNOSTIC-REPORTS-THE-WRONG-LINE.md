### TD-OILS-AN-ARITHMETIC-COMMANDS-DIAGNOSTIC-REPORTS-THE-WRONG-LINE. `(( … ))` had no line of its own — FIXED 2026-08-11

**Where:** `userspace/oils/src/ast.rs` (`Command::Arith` carried only the raw
text), `userspace/oils/src/parser.rs` (`parse_shell_command`'s `ArithCmd` arm)
and `userspace/oils/src/interp.rs`'s command dispatch. Every other construct that
stands on its own line — `case`, `[[ … ]]`, a subshell — already had a `line`
field and a `Shell::on_control_line` wrapper; the arithmetic command had neither,
so it inherited whatever line the enclosing construct had reached. That came out
one line **late** inside a brace group and one **early** inside a `for`, an `if`
and a function body.

**The rule — and the recorded hypothesis was wrong.** The original entry guessed
"the line of its own **first** token". Measurement says the **last**:

```sh
(( a =
   LINENO ))      # bash: 2, not 1
```

`make_arith_command` (make_cmd.c:438) stamps `temp->line = line_number` at the
reduction of `arith_command: ARITH_CMD`, and `ARITH_CMD` is a *single* token that
`parse_arith_cmd` scans whole with `parse_matched_pair (0, '(', ')', &ttoklen,
P_ARITH)` (parse.y:4519) — so by the time the rule reduces, the reader has been
carried to wherever the `))` was found. `execute_arith_command`
(execute_cmd.c:3795) installs it around the whole evaluation and puts the
enclosing line back on the way out:

```c
  save_line_number = line_number;
  this_command_name = "((";  /* )) */
  SET_LINE_NUMBER (arith_command->line);
```

The stamp governs both halves of the observation: `$LINENO` read *inside* the
expression, and the line a diagnostic raised by the evaluation is blamed on.

**The fix.** `Command::Arith(Str)` became `Command::Arith(ArithClause)` with an
`expr` and a `line`; the parser fills the line from `Parser::line_of(self.pos)`,
which is the line the token *ends* on (`Parser::lines` is stamped per token by
the lexer at each token's **end**, so a `(( … ))` spread over two lines — or
joined by a `\`-newline — needs no special case); and the dispatch arm wraps
`exec_arith` in `Shell::on_control_line`, exactly as `case` and `[[ … ]]` do. An
arithmetic *expansion* is untouched: it is not a command, so `$(( LINENO ))`
still reads the enclosing item's line.

**Corpus:**
`an-arithmetic-command-stands-on-the-line-its-closing-paren-is-on.sh`, which
covers the six enclosing constructs, the two multi-line spellings, the restore
afterwards, the warning line, and the expansion that is not a command.

**How it was found:** writing the corpus case for the per-context write walk
(`a-write-follows-a-nameref-chain-one-variable-context-at-a-time.sh`), whose
`(( w1 = 5 ))` form put the diagnostic on the wrong line inside a function body.
