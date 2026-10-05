### TD-OILS-A-PIPELINE-STAGE-COUNTS-AS-A-SUBSHELL-FOR-THE-FATAL-ABORT-STATUS. A `set -u` abort in a pipeline stage yields 1 where bash yields 127 — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::fatal_abort_status`, which
reads `subshell_depth`, and `Shell::enter_stage_subshell`, which raises it for a
pipeline stage.

**What.** `fatal_abort_status` gives the invocation-mode status (127 for
nounset/`:?` under `bash -c`) only at `subshell_depth == 0`, and 1 otherwise.
Its doc comment lists "pipeline stage" among the contexts that take 1. That is
wrong for a stage that is a *simple command*: bash does not count one as a
subshell level, and the higher status comes through.

```text
$ bash -c 'set -u; echo a | echo "$nope"; echo "rc=$?"'   rc=127
$ osh  -c 'set -u; echo a | echo "$nope"; echo "rc=$?"'   rc=1
```

Found while fixing TD-OILS-NOUNSET-IN-A-REDIRECTION-WORD, which routes its new
redirect-word statuses through the same `fatal_abort_status` and so inherits the
skew (`echo a | cat > $nope` is 127 in bash, 1 in osh). It is a **pre-existing**
divergence of the plain word-expansion path, not something that fix introduced,
which is why it is logged on its own.

**Impact.** Wrong `$?` (1 instead of 127) for a fatal expansion error in a
simple-command pipeline stage under `bash -c`. Narrow, but it is exactly the
status a `set -u` script would branch on.

**Fixed 2026-08-06.** The obvious fix — "use the same notion of depth that
`BASH_SUBSHELL` reports" — was measured and **disproved** before it was written.
Depth is not the question; *interception* is.

A fatal word-expansion error is not a status any command returns. It is
`jump_to_top_level (FORCE_EOF)`, a `longjmp`, and the status is decided entirely
by whichever `setjmp (top_level)` catches it. bash installs one in exactly three
places, and only the first has a 127 to give:

```c
shell.c:1446  run_one_command                 /* the -c reader */
  code = setjmp_nosigs (top_level);
  … case FORCE_EOF: return last_command_exit_value = 127;
    case DISCARD:   return last_command_exit_value = 1;      /* shell.c:1467 */

execute_cmd.c:1703  execute_in_subshell
  else if (result)
    return_code = (last_command_exit_value == EXECUTION_SUCCESS)
                  ? EXECUTION_FAILURE : last_command_exit_value;   /* → 1 */

execute_cmd.c:5379  execute_subshell_builtin_or_function
  result = setjmp_nosigs (top_level);
  … else if (result) subshell_exit (EXECUTION_FAILURE);            /* → 1 */
```

The third is the one that kills the `BASH_SUBSHELL` theory: it is the
**builtin** branch only. The `execute_function` call standing beside it in the
same function has no `setjmp` in front of it. So a pipeline stage that is an
`eval`/`.`/`source` answers 1 while a stage that is a *function call* answers
127 — even though `$BASH_SUBSHELL` reads 1 in both. Likewise a plain async
simple command (`echo "$nope" &`) is `BASH_SUBSHELL` 1 and keeps 127, because
`execute_simple_command` has already forked (`already_forked = 1`) before it
calls `expand_words`, so nothing is between its failure and the top.

`execute_in_subshell` is entered for a `( … )`, for anything flagged
`CMD_WANT_SUBSHELL|CMD_FORCE_SUBSHELL`, and for a `shell_control_structure`
(`execute_cmd.c:431` — `{ … }`, `if`, `for`, `while`, `case`, `[[`, `((`) that
is being piped or run asynchronously. The one place an and-or list differs from
a simple command is `&`: `execute_connection` sets `CMD_FORCE_SUBSHELL` on an
asynchronous `AND_AND`/`OR_OR`, so `a && b &` is caught (1) where `b &` alone is
not (127).

The fix therefore adds a **third** counter beside the two osh already had
(`subshell_depth`, the true nesting; `subshell_level`/`subshell_base`, what
`$BASH_SUBSHELL` reports): `Shell::abort_catch`, counting only the handlers that
are actually in the way. `Shell::fatal_abort_status` reads that instead of
`subshell_depth`. A `StageKind` enum classifies each pipeline stage at
`exec_command`, `stage_pending_catch`/`settle_stage` arm the counter once the
stage's shape is known (the `Builtin` settle arms it, the `Function` settle does
not), and the `&`-job path applies the `CMD_FORCE_SUBSHELL` rule. A `$( … )`
body parse error goes through `parse_error_flow`, which counts the same way.

None of this is visible when the shell reads a *file* — `reader_loop`'s handler
answers with the last command's status, so every case would be 1 — so the corpus
case re-invokes `$BASH --norc -c`, the only reader with a 127 to give. Locked by
`userspace/oils/tests/corpus/the-status-of-a-fatal-expansion-abort-belongs-to-whoever-caught-it.sh`
(45 probes across five sections: nothing in the way, `( … )`/`eval`, pipeline
stage by shape, `&` jobs, and `$(for)` parse errors).
