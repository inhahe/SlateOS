### BUG-OILS-REPL-LINE-BASE. The piped/interactive REPL restarts line numbering at 1 for every line read — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** `printf 'echo one\nnosuchcmd_xyz\n' | osh`:

```
bash: one / bash: line 2: nosuchcmd_xyz: command not found
osh : one / osh: line 1: nosuchcmd_xyz: command not found
```

Not specific to any one diagnostic — `$LINENO`, command-not-found,
`set -u` unbound-variable, and syntax errors are all affected:

| input (piped to the shell) | bash | osh |
|---|---|---|
| `echo $LINENO` on lines 2 and 4 | `2`, `4` | `1`, `1` |
| `nosuchcmd_xyz` on line 2 | `line 2:` | `line 1:` |
| `echo two )` on line 2 | `line 2:` | `line 1:` |
| `echo $undef` (`set -u`) on line 3 | `line 3:` | `line 1:` |

**Where.** `userspace/oils/src/main.rs` `repl()` reads one logical command at
a time and calls `sh.run_source(&buffer)` on each, and `run_source` restarts
`current_line` at 1 for every call because the AST line numbers are relative
to the string it was handed.

**Fix.** The shift is applied at *parse* time, not execution time, mirroring
what `parse_cmdsub_body`/`CmdSubLineMap` already do for `$( … )` bodies: a new
`parser::shift_lines` adds the base to every entry of the lexed `lines` vector
and to each `Seg::CmdSub` close line, so every AST node ends up carrying an
**absolute** line. That is what makes a function body report its *definition*
line no matter where it is later called from (bash's behaviour, verified) — a
runtime `line_base` added at `current_line` assignment would have used the
caller's base instead and needed a per-function saved base.

`IncrementalParser::new` gained a `line_base` parameter (also applied to a
parked lexer error's line); `Shell::run_source_at(src, line_base)` is the new
public entry point and `run_source_out` / `run_source_flow_out` /
`format_parse_error` thread it through. `format_parse_error` subtracts it
again before `nth_source_line`, which indexes the *fragment*. `repl()` in
`main.rs` counts physical lines read per command and passes the running total.
`source`, traps and `mapfile -C` bodies pass 0 (they are their own input
source, numbered from 1 — verified against bash); `eval` passes
`current_line - 1`, see BUG-OILS-EVAL-LINE-BASE.

**Regression test.** `tests/cli_options.rs`
`stdin_repl_numbers_lines_across_the_whole_stream` — `$LINENO` across plain
lines, blank/comment lines, a multi-line compound, a function body, plus
command-not-found, a syntax error with its source echo, and a deferred lexer
error.
