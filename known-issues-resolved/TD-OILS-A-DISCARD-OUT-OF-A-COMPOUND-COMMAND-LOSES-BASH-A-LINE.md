### TD-OILS-A-DISCARD-OUT-OF-A-COMPOUND-COMMAND-LOSES-BASH-A-LINE. Every line number for the rest of the script is one low per compound level unwound — 2026-08-08 — ✅ **RESOLVED 2026-08-08**

**Where:** `userspace/oils/src/interp.rs` — `Shell::line_bias` and the two places
a recorded line becomes the current one (`Shell::exec_items`,
`Shell::exec_simple_inner`).

**What.** bash's `line_number` (`parse.y:1749`) is **one** global shared by the
lexer and the executor, and a script is read one parse unit at a time — so the
counter the parser is using when it records `simple_command->line` for the *next*
unit is whatever the *previous* unit's execution left behind.

Normally that is harmless, because each compound-command executor saves the
counter on entry and puts it back on exit — `execute_for_command` at
`execute_cmd.c:2883` / `:3003`, `execute_select_command` at `:3404` / `:3444`,
and so on. The saved value is where the lexer stopped (the compound's last
line); the body drives the global *backwards* to each simple command's own line;
the restore undoes that.

A `jump_to_top_level(DISCARD)` from inside the body is a `longjmp` straight to
`reader_loop`, and these are plain assignments rather than unwind-protects — so
the restore never runs. Two constructs *do* use unwind-protects, and a `longjmp`
runs those: a function call (`unwind_protect_int (line_number)`,
`execute_cmd.c:5095`) and `parse_and_execute`, i.e. `eval`/`source`
(`builtins/evalstring.c:232`). The rule that falls out:

> After an abort, bash's counter is left at **the line the innermost command of
> the current shell frame was on**, with the function-call and `eval` restores
> applied — instead of **the last line of the parse unit it was in**. Everything
> parsed afterwards is low by the difference, and it never recovers.

"Current shell frame" is what makes a subshell and a command substitution lose
nothing: bash forks for both, so the abort perturbs the *child's* copy of the
global. A top-level abort loses nothing either, because for it the aborting line
*is* the unit's last line.

Measured 2026-08-08 against bash 5.2.37. The abort is a malformed `-i` value,
which is the deepest of bash's two discards (see
`TD-OILS-INT-BIND-DISCARD-STOPS-AT-EVAL`):

```sh
declare -i m
if true; then
  m=1+          # line 3 — both shells say 3
fi
echo "A $LINENO"  # line 5 — bash says 4, and osh said 5 before the fix
```

Each shape below aborts inside a compound whose last line is given, and asks
`$LINENO` on the next line. The loss is *unit's last line − leftover counter*,
and it accumulates across aborts:

| shape around the abort | leftover is | loss |
|---|---|---|
| none (top level) | the aborting line, which is also the unit's last | 0 |
| `while … done`, `{ … }`, `if … fi` | the aborting line, one above `done`/`}`/`fi` | 1 |
| `if … if … fi fi` | the aborting line, two above | 2 |
| a function call, called at top level | the *call site* — restored by the unwind-protect | 0 |
| a function call, called from `if … fi` | the call site, one above `fi` | 1 |
| a function call, called from `if … if … fi fi` | the call site, two above | 2 |
| `eval "m=1+"` inside `if … fi` | the `eval`'s own line — `parse_and_execute` restores it | 1 |
| `( m=1+ )` inside `if … fi` | nothing: bash forked, the parent's counter is untouched | 0 |
| `x=$( m=1+ )` inside `if … fi` | same | 0 |
| an `if` built inside an `eval` string | same — the restore covers the whole body | 0 |
| two aborts, each out of one `if` | — | 2 by the script's end |

The function rows are why the first draft of this entry was wrong: it said an
abort out of a function loses *nothing*, reasoning only from the unwind-protect.
The unwind-protect restores the *call site*, not the unit's last line, so a call
made from inside a compound drifts exactly as far as the compound does. A
later-defined function is affected too, since its body's lines are recorded by
the already-perturbed parser: after a loss of 1, a function whose body sits on
line 5 reports `LINENO` 4 from it.

**How it bit.** It cost a rewrite of
`a-builtins-signature-is-taken-away-by-running-a-command.sh`, whose first draft
put four probes in a `for` loop: every probe aborts, so from the loop onward the
two shells disagreed about every line number in the case while agreeing about
every diagnostic it was actually testing. **A corpus case that aborts must keep
the aborting command at top level** unless the drift is the thing being
measured. (bash also abandons the whole `for` there, so only the first iteration
ever ran — the loop was buying nothing.) That rule is now only about *reading* a
case: osh reproduces the drift, so an aborting case inside a compound still
matches — it just measures this entry as well as its own subject.

**Why osh did not have it.** osh's `Shell::current_line` was set from `sc.line` —
the parser's exact record, taken with no shared mutable counter between parsing
and execution — so there was nothing for an abort to perturb. osh was *right* and
bash is wrong; byte-fidelity means reproducing bash anyway.

**Fixed in `HEAD`.** bash's counter is carried as a bias rather than by
recreating its shared global:

- `Shell::line_bias` holds how far the counter has fallen behind the true source
  line. `Shell::exec_items` and `Shell::exec_simple_inner` subtract it when a
  recorded line becomes the current one, so `current_line` is itself held
  *biased* — exactly as bash holds its counter — and every downstream reader
  (`$LINENO`, `err_prefix_at`, `format_parse_error`) needed no change at all.
- `Shell::exec_items` sets `unit_discarded` in the two arms that absorb a
  discard, and `Shell::run_source_flow_units` remeasures the bias after such a
  unit: `line_bias = unit_end - current_line`, where `unit_end` comes from the
  new `IncrementalParser::last_unit_end_line()` (built from the parser's
  `orig_lines`, since `UnitLine` carries no line number). The measurement is
  *absolute*, not accumulated, because `current_line` is already biased by the
  old value — which is what makes successive aborts add up on their own.
- The two unwind-protected restores are reproduced where bash has them:
  `run_source_flow_result` — osh's `parse_and_execute` — saves and restores both
  halves of the counter, and the function-call teardown restores `current_line`
  from `call_line_stack`. Neither is observable except through this drift, since
  every command after them sets `current_line` itself.
- A `LineMap`'s *base* is the one place that needs the true line back, because a
  map's output is a recorded line that will be biased again on the way in. The
  new `Shell::source_line()` adds the drift back for the five bases (`eval`, the
  two command-substitution forms, a trap body, `compgen -C`). Without it an
  `eval` reached after a loss of 1 reported one line lower than bash — caught by
  the differential probe, not by reasoning.

Covered by the lib test `a_discard_out_of_a_compound_command_loses_a_line` and by
`tests/corpus/a-discard-out-of-a-compound-command-loses-a-line.sh`, whose running
total climbs 0 → 1 → 2 → 4 → 5 → 6 → 7 across the sections.
