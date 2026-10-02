### TD-OILS-A-FORCE-EOF-REPARSE-ABORT-KEEPS-ITS-SCRIPT-STATUS-UNDER-DASH-C. `bash -c` ends a re-parse abort with 127; osh ends it with 2 — 2026-08-08 — ✅ RESOLVED 2026-08-08

**Where:** `userspace/oils/src/interp.rs` — the two sites that arm a
FORCE_EOF-class `FatalAbort` for a *re-parse* failure:
`command_sub_body_inner`'s `nested` branch (~28428) and
`apply_assignment_inner`'s `array_assign_reparse_error` hook (~13665). Both
compute `let status = if self.at_outermost_read_eval() { 2 } else { 1 };`,
which is the script/stdin rule only.

**What.** A re-parse that fails inside `parse_comsub` is
`jump_to_top_level (FORCE_EOF)` (parse.y:4185), and `-c`'s own handler scores
that class 127 unconditionally:

```c
    case FORCE_EOF:
      ...
      return last_command_exit_value = 127;      /* shell.c:1446, run_one_command */
```

A script's `reader_loop` has no such promotion, so it keeps whatever
`last_command_exit_value` the syntax error left. Measured, both sites alike:

| context | bash | osh |
|---|---|---|
| `bash -c 'a=( "p$(⏎!⏎)q" )'` | **127** | 2 |
| `bash -c "eval 'a=( … )'"` | **127** | 1 |
| `bash -c '{ a=( … ); }'` / a function body | **127** | 2 |
| `bash script` | 2 | 2 ✓ |
| `script`, under `eval` | 1 | 1 ✓ |
| `bash -c '( a=( … ) )'` (real subshell) | 2 | 2 ✓ |
| `bash -c "eval '( a=( … ) )'"` | **1** | 2 |
| under `-e`, anywhere | 2 | 2 ✓ |

So the rule is the interception rule osh already knows, with a FORCE_EOF twist:

```rust
if self.errexit { 2 }
else if self.command_mode && self.abort_catch == 0 { 127 }
else if self.at_outermost_read_eval() { 2 }
else { 1 }
```

— i.e. the 127 belongs to `-c`'s handler and only reaches the exit where
nothing catches the jump first (`abort_catch == 0`); a brace group and a
function body install no handler and so keep it, a `( … )` and a `$( … )` do
install one and fall back to the script rule. Note this is *not* the shape
`parse_error_flow` computes for a reader syntax error (that one keys the
outermost case on `command_mode` alone), so it wants its own helper rather
than a reuse.

**Two more divergences at the same two sites**, found in the same measurement:

- **Under `set -e` bash drops the echoed source line**, printing only
  `… : syntax error near unexpected token \`)'`. osh prints both lines.
  `Shell::format_parse_error` already implements this for reader syntax
  errors; these two sites format their own message and do not.
- **Inside a backtick body's read-eval the status is 1, not 2.**
  ``x=`a=( "p$(⏎!⏎)q" )` `` leaves `$?` at 1 in bash and 2 in osh.
  `parse_error_flow`'s `at_comsub_read_eval` branch already has this rule
  (`if e.fatal { 1 } else { 2 }`); the `FatalAbort` path bypasses it.

**The proper fix** is one helper — call it `Shell::force_eof_status` — next to
`fatal_abort_status`, carrying the table above plus the backtick case, used by
both sites; and moving the errexit line-drop out of `format_parse_error` into
something both the reader and these two can call.

**Impact.** Only the *status* (and one diagnostic line under `-e`) of a
re-parse abort, which needs a body whose re-print will not parse back — in
practice one ending in a bare `!` or `time`. The diagnostics themselves are
already byte-exact.

**Found by** the measurement pass for
TD-OILS-A-COMPOUND-ARRAY-ASSIGNMENT-IS-RE-PARSED-UNDER-ITS-OWN-INPUT-NAME.

**Resolved.** `Shell::force_eof_status` carries the table above and is used by
both sites; the errexit line-drop is now `Shell::errexit_first_line`, factored
out of `format_parse_error` and called by both as well. Covered by
`tests/corpus/a-force-eof-reparse-abort-is-scored-127-by-a-c-string.sh` (22
shapes).

**The errexit row was understated.** It is not only that the status is 2 and
one diagnostic line is dropped: under errexit *neither* jump is reached at
all, because `parser_error` (error.c) ends with a **direct** call, made before
the class of the failure has been decided —

```c
    if (exit_immediately_on_error)
      exit_shell (last_command_exit_value = 2);
```

— so there is nothing for a `||` to catch. `set -e; v=p$(⏎!⏎)q || echo B; echo
C` prints no `B` and no `C` and exits 2, where osh printed both and exited 0.
That applies to the *recoverable* DISCARD shape too, which is why the fix
touches three branches and not two.
