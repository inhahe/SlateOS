### TD-OILS3. `osh` compound-command redirections: stderr now honored — RESOLVED 2026-07-18

**Where:** `userspace/oils/src/interp.rs` (`exec_redirected`, `StderrTarget`,
`Shell::stderr_stack`, `emit_stderr`, `errln`, `child_stdio_for_stderr`,
`write_bytes`, `run_external`), `userspace/oils/src/parser.rs`
(`with_redirects`, `at_redirect_start`), `userspace/oils/src/ast.rs`
(`Command::Redirected`).

**What:** Trailing redirections on compound commands are supported
(`while read …; done < file`, `for … done > out`, `{ …; } >> log`,
subshell/`if`/`case`/`(( ))` bodies, pipeline-into-`while read`). Input
is fed via a shared `StdinSrc::Cursor` (`RefCell<io::Cursor<Vec<u8>>>`)
so successive `read`s in the body consume successive lines; the compound's
stdout is captured into a buffer and written to the target file after the
body runs.

**Resolution (2026-07-18):** stderr redirection on a compound command is
now wired via a `stderr_stack: Vec<StderrTarget>` on the `Shell`. Before
running the body, `exec_redirected` pushes a `StderrTarget` (`File` for
`2>`/`2>>`; for `2>&1` with fd 1 not a file, mirrors fd 1's live sink —
`Buffer`+merge for a captured stdout, `Pipe` for a pipe, `Stdout` for
inherit) and pops it after. All fd-2 output — command diagnostics
(`errln`), builtin `>&2` (`write_bytes` → `emit_stderr`), and external
child stderr (`run_external`, precedence: per-command `2> file` > `2>&1` >
`stderr_stack` top > inherit) — consults `stderr_stack.last()`, so the
whole group honours the redirect. `1>&2` (`stdout_to_stderr`) routes the
body's stdout to the current stderr sink. `Arc` (not `Rc`) is used because
subshell clones move into scoped pipeline threads (`clone_for_subshell`
resets the stack to empty). Tests: `compound_stderr_redirect_to_file`,
`compound_stderr_append_redirect`, `compound_stderr_to_stdout_in_capture`,
`compound_stderr_to_stdout_top_level_capture`,
`compound_for_loop_stderr_redirect`. **Best-effort limit:** `2>&1`
interleaving into a captured stdout is not byte-interleaved — the group's
stderr is folded in after the body finishes (documented in `exec_redirected`).
