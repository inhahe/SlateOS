### TD-OILS20. `$LINENO` counts only top-level newline tokens — lines inside multi-line quotes/substitutions/here-docs are undercounted — ✅ RESOLVED 2026-08-08 (embedded-newline undercount fixed 2026-07-19; both sub-gaps closed since)

**Status (2026-08-08, RESOLVED):** both sub-gaps recorded below are closed.

- **(a) per-command granularity.** `SimpleCommand::line` was added and is what
  `Shell::exec_simple_inner` stamps from, so a multi-line pipeline's failing
  stage reports the stage's line. `Item::line` is now gone entirely — see
  TD-OILS-AN-UNSTAMPED-COMMAND-IS-BLAMED-AT-ITS-FIRST-TOKEN-NOT-WHERE-THE-READER-STOPPED,
  which removed it because bash has no per-item stamp at all.
- **(b) function-relative numbering.** Re-measured against bash 5.2.37 and the
  premise is wrong: bash does **not** renumber `$LINENO` relative to a
  function's definition. `echo "top $LINENO"⏎f() {⏎  echo "in $LINENO"⏎}⏎f⏎echo
  "after $LINENO"` gives `1 / 3 / 6` in a script file, under `-c`, and on stdin
  alike, and a function defined inside an `eval` reports the eval string's own
  numbering (`in 4` for a 4-line string). osh answers identically in all four.
  (bash's `function_line_number` exists, but it feeds `declare -F` under
  extdebug and the DEBUG trap's `showing_function_line`, not `$LINENO`.)

**Status (2026-07-19, FIXED):** the embedded-newline undercount is resolved. The
lexer now tracks a running source line and stamps every token with its true
starting line (`tokenize_spanned` returns a parallel `Vec<u32>`; see
`Lexer::stamp_lines`), and the parser reads each `Item`'s line from that vector
(`Parser::cur_line`) instead of counting top-level `Newline` tokens. `$LINENO`
and error line numbers are now exact across newlines swallowed inside
here-document bodies, multi-line quoted strings, and command substitutions
(verified against bash). **Two narrow sub-gaps remain OPEN:** (a) per-*command*
granularity — osh tracks line per top-level `Item`, whereas bash advances
`$LINENO` per simple command, so a multi-line pipeline's failing stage or a
command with an embedded-newline argument reports the item's start line rather
than the stage/command line (logged in todo.txt with the exact fix: add a
`line` to the command node and set `current_line` per stage). (b) `$LINENO` is
not reset to be relative to a function's definition the way bash does — ours is
absolute to the parsed unit.

**Where:** `userspace/oils/src/lexer.rs` (`Lexer.line`/`stamp_lines`/
`tokenize_spanned`), `userspace/oils/src/parser.rs` (`Parser.lines`/`cur_line`),
`userspace/oils/src/ast.rs` (`SimpleCommand.line` and the compound clauses'
`line` fields; `Item.line` no longer exists),
`userspace/oils/src/interp.rs` (`Shell.current_line`, seeded per parse unit in
`run_source_flow_units`, read in `param_value` as `"LINENO"`).

**What:** `$LINENO` is implemented by having the parser count the top-level
`Tok::Newline` tokens it consumes and stamp the current 1-based line onto each
parsed `Item`; the interpreter sets `self.current_line = item.line` before
running each item and returns it for `$LINENO`. This is accurate for ordinary
multi-line scripts, blank/comment lines, and semicolon-joined commands. It
diverges from bash when a **single logical line spans multiple physical lines
via a construct whose interior newlines are not top-level `Newline` tokens** —
e.g. newlines inside a double-quoted string, a `$(…)`/`` `…` `` command
substitution body, an arithmetic `$(( … ))` spanning lines, or a here-document
body. Those interior newlines are absorbed by the lexer into a single segment/
token, so the parser's line counter does not advance across them, and a
`$LINENO` appearing *after* such a construct reports a line number lower than
bash would (bash counts every physical newline the reader consumes). The
counter also does not reset per function body the way bash resets `$LINENO` to
be relative to the function's definition — ours is absolute to the parsed unit.

**Proper fix:** track physical line numbers in the **lexer** (attach a source
line to every `Tok`, counting newlines even inside quoted/substitution/heredoc
segments) and thread that through to `Item.line`, rather than counting
top-level `Newline` tokens in the parser. That makes `$LINENO` exact for all
constructs. Deferred because it requires adding position info to the token
stream (a lexer-wide change) and the current approximation is correct for the
overwhelming majority of real scripts; the discrepancy only appears after
embedded multi-line quotes/substitutions/here-docs.
