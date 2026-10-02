### TD-OILS-STDIN-RESYNCS-AFTER-A-SYNTAX-ERROR — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::parse_error_flow` and the
new `Shell::took_syntax_abandon`; `userspace/oils/src/main.rs` — `repl`, the
non-interactive stdin read loop, which kept parsing after a syntax error where
the file and `-c` routes correctly abandoned.

**What.** The same script abandoned from a file and continued from stdin:

```sh
[[ a
== b ]]
echo tail
```

```text
osh FILE:  3 diagnostic lines, then nothing        — matches bash
osh -c:    3 diagnostic lines, then nothing        — matches bash
osh <FILE: 3 diagnostic lines, then `line 2: ==: command not found`, then `tail`
bash<FILE: 3 diagnostic lines, then nothing
```

With a leading `echo one` the statuses diverged too: bash exited 2, osh 0.

**Why.** bash's `report_syntax_error` does not merely score the error. Unless
the shell is interactive it finishes with `jump_to_top_level (FORCE_EOF)`,
which sets `EOF_Reached` and so stops `reader_loop` outright — the rest of the
input is never read, whichever reader supplied it.

osh had the behaviour, but only by accident of shape: a script file and a `-c`
string are each **one** `run_source_at` call, so `parse_error_flow` returning
`Flow::Next` already abandoned everything there was. Stdin is read a *logical
command* at a time, so the same `Flow::Next` abandoned only the current buffer
and the loop came back for the next line — which is exactly bash's
*interactive* resynchronisation, applied to a non-interactive shell.

**Fixed by** making the abandonment explicit rather than incidental.
`parse_error_flow` now latches `Shell::syntax_abandon` when the error reaches
the outermost read-eval loop and the shell is not interactive — after the
backtick-body case returns, since a `` ` … ` `` body's own loop ends only
itself. `repl` polls it with `Shell::took_syntax_abandon` (take-and-clear)
right beside its existing `exit_requested` check and returns
`Shell::last_status`. The flag never carries into a subshell clone.

Everything else keeps its behaviour: an interactive shell still resynchronises
and prompts again (`osh -i` runs the command after the bad one, as bash does),
`eval` and `.`/`source` still end only their own string or file, an incomplete
construct is still accumulated rather than reported, and a *runtime* error is
still not a syntax error.

**Pinned by**
`tests/corpus/a-syntax-error-abandons-the-rest-of-the-input-on-every-reader.sh`
— the same script through all three readers, nine error shapes, two
unterminated constructs, three multi-line commands that are *not* errors, plus
the runtime-error and `eval`/`source` non-cases and the surviving exit status.
