### BUG-OILS-REPL-BACKSLASH-JOIN. The REPL spliced `\<newline>` itself, corrupting quotes, here-docs and escaped backslashes — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Five distinct wrong answers, all when the input came from stdin
(a script *file* was already correct, which is what pinned the read loop as
the culprit):

| stdin | bash | osh (before) |
|---|---|---|
| `echo a\` / `b` | `ab` | `a` then `line 2: b: command not found` |
| `echo 'x\` / `y'` | `x\` / `y` | `x` / `y` (backslash eaten inside quotes) |
| `echo "x\` / `y"` | `xy` | `x` / `y` |
| `echo a\\` / `echo done` | `a\` / `done` | `aecho done` |
| `cat <<E` / `a\` / `b` / `E` | `ab` | `a` / `b` |

**Root cause.** `repl()` in `userspace/oils/src/main.rs` did
`if let Some(cont) = trimmed.strip_suffix('\\') { buffer.push_str(cont);
buffer.push('\n'); … }` — it removed the backslash and put a *bare newline*
in its place. That is wrong three ways: the newline turns one command into
two; a backslash that is literal (inside `'…'`) is destroyed; and an even
run of backslashes (`\\`, an escaped backslash) is misread as a continuation
so the next line is glued on.

**Fix.** The read loop now only decides *whether more input is coming* — an
odd number of trailing backslashes leaves a live `\<newline>` pair — and
pushes the line verbatim, newline included. The lexer, which already handles
`\<newline>` correctly for script files, does the splicing and is the only
thing that knows the context-dependent rules.

**Regression test.** `tests/cli_options.rs`
`stdin_repl_leaves_line_continuations_to_the_lexer` — all five rows above.
