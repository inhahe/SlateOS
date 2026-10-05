### BUG-OILS-EVAL-DISCARD-SCOPE. A `Flow::Discard` raised inside `eval` aborts only the current *line* of the eval'd string, where bash unwinds the whole `eval` and the rest of the caller's list — 2026-07-27 — ✅ RESOLVED 2026-07-28

**Symptom.** Two of bash's `jump_to_top_level(DISCARD)` sites unwind to
different depths, and osh models both with a single `Flow::Discard` that
the top-level item loop (`exec_program_top`) catches one *line* at a
time. Measured against bash 5.2:

```sh
# (a) word-expansion error — both shells agree
$ bash -c 'eval "echo a; echo \$((1/0)); echo b
echo c"; echo done=$?'
a
bash: line 2: 1/0: division by 0 (error token is "0")
c
done=0
# osh: identical.

# (b) special-builtin usage error — they diverge
$ bash -c 'eval "for i in 1; do break 1 2; done
echo next"; echo done=$?'
bash: line 2: break: too many arguments        # eval AND `echo done` both discarded
$ osh -c 'eval "for i in 1; do break 1 2; done
echo next"; echo done=$?'
osh: line 2: break: too many arguments
next                                            # eval continued …
done=0                                          # … and so did the caller
```

In bash the difference comes from `no_args()` calling
`top_level_cleanup()` before `jump_to_top_level(DISCARD)`, which unwinds
past `parse_and_execute`'s handler, whereas an expansion error is caught
by it. osh has no equivalent distinction.

**Where.** `userspace/oils/src/interp.rs` — the `Flow::Discard` variant
(~line 509) and its only resumption point, `exec_program_top`. The
too-many-arguments site is `loop_count_arg`.

**Impact.** Very narrow: it needs a special-builtin *usage* error
(`break`/`continue` with two operands is the only one osh currently
raises this way) inside a multi-line `eval`/`source`. Nothing in the
corpus depends on it; `tests/corpus/break-continue-args.sh` deliberately
tests the discard scope at the script's own top level, where the shells
agree, and says so in a comment.

**Proper fix.** Split `Flow::Discard` into two variants — one caught by
the nearest read-eval loop (`eval`, `.`/`source`, the top-level item
loop) and one that unwinds past `eval`/`source` to the *outermost*
read-eval loop — and audit every raise site against bash to decide which
it is. Worth doing together with a survey of bash's other
`top_level_cleanup()` callers (`no_args`, `sh_needarg` in special
builtins, `assignment_error` under POSIX mode) so the classification is
made once rather than one site at a time.

**✅ RESOLVED 2026-07-28.** Done as described: `Flow::Abort` joins
`Flow::Discard`, and `loop_count_arg`'s too-many-arguments site — bash's
`no_args()`, the one `top_level_cleanup()` caller osh currently reaches —
raises it. `exec_items` catches it only at a top level that is *not*
inside an `eval`/`source` (`Shell::at_outermost_read_eval`); everywhere
else it propagates like `Exit`. Because `eval` and `.`/`source` are
builtins that can only return an `i32`, each re-raises it through the new
`Shell::pending_abort` side channel, which the post-builtin teardown turns
back into `Flow::Abort` — the same shape `pending_builtin_exit` already
used for `Exit`.

Two things the original write-up did not anticipate, both found by
measuring rather than reasoning:

*The unit is not the line.* `Discard` approximates "rest of the current
top-level parse unit" by skipping items sharing the offending item's
source line. That is wrong for this case: the `eval` and the `echo` after
it are one unit but two lines, because the newline between them is inside
the eval'd string. `Abort` therefore abandons the whole remaining
`Program` instead of filtering by line.

*`-c` is not a script.* bash's jump lands in whatever read-eval loop is
left, and a `-c` shell has none — `parse_and_execute` *is* its top level
and the cleanup popped it — so the shell exits with the error status,
while a script or stdin resumes with the next unit. Measured:

```sh
$ bash    -c 'eval "…break 1 2…"; echo done=$?
echo after'          # → error only, exit 1   (osh now identical)
$ bash script                                 # → error, then `after`, exit 0
$ … | bash                                    # → like the script, not like -c
```

`Shell::command_mode` already distinguished the two, so the catch arm
returns `Flow::Exit(last_status)` there and `Flow::Next` otherwise.
Subshell and command-substitution boundaries needed no change: `Abort`
falls into the same catch-alls `Exit` does, so the child fails with status
1 and the parent carries on, which is what bash does.

**Coverage.** `tests/corpus/eval-discard-scope.sh` pins the expansion-error
scope (unchanged), the usage-error scope through both `eval` and `source`,
the subshell boundary, and the plain top-level discard. Full differential
corpus: 107 matched, 0 failed; 840 unit tests pass. Two things are not
corpus-testable and are therefore recorded above instead: the `-c`/stdin
split (the harness runs every case as a script file), and the
command-substitution shape (bash's line number for it is off the end of
the file — TD-OILS-CMDSUB-ABORT-LINENO).
