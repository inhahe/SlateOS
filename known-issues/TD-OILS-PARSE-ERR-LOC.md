### TD-OILS-PARSE-ERR-LOC. `osh` syntax-error diagnostics differ from bash in source-token, line number, and partial output — 2026-07-19 — MOSTLY RESOLVED 2026-07-20 (two sub-divergences remain: partial output + token identity)

**Where:** `userspace/oils/src/parser.rs` (`ParseError`, `unexpected_here`,
`cur_line`, `parse_tokens`), `userspace/oils/src/lexer.rs` (`LexError`, which
carries no line), `userspace/oils/src/interp.rs` `format_parse_error` /
`syntax_error_prefix` / `wrap_parse_message` / `nth_source_line` and the
whole-program parse-then-exec model in `run_source*`.

**What:** the original entry listed three divergences on the *syntax-error*
path (all stderr-only; runtime-error messages already match bash):

1. **✅ RESOLVED 2026-07-20 — Missing input-source token.** bash tags a
   syntax error with the current input source between `$0` and `line N`:
   `bash: -c: line N: …` for `-c`, `bash: eval: line N: …` for `eval`, and
   just `<script>: line N: …` for a script file. osh now emits the same
   token via `Shell::syntax_error_prefix`: `-c` when `command_mode`, `eval`
   while `eval_depth > 0` (a new counter bumped around the `eval` builtin,
   winning over the outer token even from a script), and none for a script
   file / interactive input. Runtime (non-parse) diagnostics keep going
   through `err_prefix`, which omits the token — matching bash (`bash: line
   1: cmd: command not found`, no `-c:`).

2. **✅ RESOLVED 2026-07-20 — Wrong line number + second-line echo.**
   `ParseError` now carries `line: Option<u32>`, stamped centrally in
   `parse_tokens` from `p.cur_line()` (pos is not advanced past a failing
   token, so the cursor still points at the error site). bash's *unexpected
   end of file* quirk — it reports the line one past the last token (`if
   true` → line 2, `if true\n\n\n` → line 4) — is reproduced by adding 1
   when `p.peek().is_none()`. For the `near unexpected token` family bash
   also echoes the offending physical source line on a second diagnostic
   line (`osh: -c: line 1: \`echo hello world ('`); `format_parse_error`
   now emits that via `nth_source_line`. Lexer-originated errors (unclosed
   quotes/subs) originally carried no parser line and fell back to
   `current_line`; that gap is closed as of 2026-07-27 — see
   **BUG-OILS-LEX-ERROR-LINE** below. Covered by
   `syntax_error_prefix_carries_source_token_line_and_echo` and
   `lexer_error_carries_the_constructs_opening_line`.

3. **STILL OPEN — No partial output before the error.** bash parses-and-
   executes a script one command at a time, so `echo a; echo b; echo (`
   prints `a` and `b` before the line-3 syntax error (and the same happens
   inside a multi-line `eval` string). osh parses the whole program up
   front, so nothing runs when any part fails to parse. This is an
   architectural change (incremental parse/execute) that interacts with
   here-docs, function definitions spanning commands, and `$LINENO`. The
   second-line echo (from #2) is still emitted correctly even when the
   preceding output is missing, so single-statement syntax errors — the
   common case — now match bash byte-for-byte.

4. **PARTIALLY RESOLVED 2026-07-20 — Token identity.** bash names the
   token its parser cursor sits on (`near unexpected token \`X'`, or
   `unexpected end of file` at EOF). Where osh's cursor *already* sits on
   that same token, the fragment-style message was replaced with
   `unexpected_here()` so the culprit and the second-line echo match bash.
   **Done:** the `case` sites (`case ;`, `case )`, `case x in ;&`, `case x
   in )`, `case x in pat esac`) and subshell close (`( )`, `( echo hi`) —
   all verified equal to bash. Covered by
   `case_and_subshell_errors_name_the_offending_token`. The **redirect
   target** site was also converted (2026-07-20): it now peeks rather than
   `bump()`s, so on a bad target the cursor still sits on the culprit and
   `unexpected_here` names it; `token_display` was extended to spell out
   every redirect operator (`>`, `>>`, `>|`, `>&`, `<&`, `<>`, `&>`, `&>>`,
   `<<`, `<<-`, `<<<`, `|&`). `echo > >`, `echo > >>`, `echo > >&`,
   `echo > <&`, `echo > <>`, `echo > &>`, `echo > &>>`, `echo > <<<`,
   `echo > >|`, `echo > <<`, `echo > |`, `echo > &&`, `echo > ;`,
   `echo > )` all now match bash exactly. Covered by
   `redirect_target_errors_name_the_offending_token`.

   **Still open (cursor is not on bash's culprit):**
   - `(`-in-command-position (`echo a; echo (`, `echo hello (`): bash scans
     past the `(` and reports `near unexpected token \`newline'`; osh errors
     at the `(` itself. Requires bash's implicit-trailing-newline model.
   - Redirect target *at end of line* (`echo <`): bash reports the implicit
     `newline` (`near unexpected token \`newline'`); osh reaches true EOF and
     reports `unexpected end of file`. This is the same implicit-trailing-
     newline model gap as the `(`-in-command-position case — the token-vs-
     token redirect cases above are all resolved.
   - `[[ … ]]` conditional errors — **mostly resolved 2026-07-20.** osh now
     emits bash's conditional-specific phrasing: `syntax error near \`X'` in
     primary position (`[[ ]]`, `[[ a && ]]`, `[[ ! ]]`), `unexpected
     argument \`X' to conditional {unary,binary} operator` after an operator
     (`[[ a == ]]`, `[[ -f ]]`), `unexpected token \`X', expected \`)'` for an
     unclosed sub-expression (`[[ ( a ]]`), and `unexpected EOF while looking
     for \`]]'` for a missing closer at EOF (`[[ a == b`) — each with bash's
     source-line echo where bash emits one. Driven by `CondPos` +
     `cond_operand_error` in `parse_cond*`; covered by
     `cond_expr_errors_match_bash_phrasing`. **Still open:** the *empty*
     sub-expression `[[ ( ]]` (bash prioritises `expected \`)'` over the
     primary near-error — needs paren-context state), and the EOF-after-
     operator forms (`[[ a ==`, `[[ -f`, `[[ a &&`) which use bash's
     implicit-`newline`/`EOF` model (same gap as the redirect/`(` EOL cases).
   - `${}` / `${a[]}`: bash defers these to a *runtime* `${…}: bad
     substitution` (no `-c:`/line prefix), whereas osh raises a parse error.
   - `for (( ))`: bash itself reports a bare `unexpected end of file` here.

**Remaining fix (if pursued):** (3) is a separate incremental-execution
project. The still-open (4) items each need a targeted grammar change
(consume-then-error at the following token, or the implicit-EOL newline
model, or bash's `[[ ]]`/bad-substitution deferral) — behaviour-shifting
and best done one class at a time with a focused test sweep.
