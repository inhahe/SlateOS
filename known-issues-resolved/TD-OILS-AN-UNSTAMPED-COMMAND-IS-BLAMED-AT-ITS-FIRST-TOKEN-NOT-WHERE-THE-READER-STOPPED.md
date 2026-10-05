### TD-OILS-AN-UNSTAMPED-COMMAND-IS-BLAMED-AT-ITS-FIRST-TOKEN-NOT-WHERE-THE-READER-STOPPED. bash's `line_number` is a register the *reader* seeds, and osh seeds it per command — 2026-08-08 — ✅ RESOLVED 2026-08-08

**Where:** `userspace/oils/src/interp.rs:8275` — the list driver does
`self.current_line = item.line.saturating_sub(self.line_bias)` before every
`Item`, where `Item::line` is the item's **first** token. bash never does that:
its `line_number` is a single shell register that the *reader* leaves at the
last line of the parse unit it just read, and that only individual executors
overwrite (and restore) around themselves.

**What.** Every executor that raises a diagnostic *while it still owns the
line* stamps itself first — a simple command
(`SET_LINE_NUMBER (simple_command->line)`), `case`/`for`/`select`/`for (( ))`/
`[[ … ]]` (see TD-OILS-A-COMPOUND-COMMANDS-LINE-IS-STAMPED-AT-ITS-KEYWORD).
So the register's *inherited* value is invisible almost everywhere. Two windows
leave it exposed:

- **`for`/`select`'s identifier check.** `execute_for_command` calls
  `check_identifier (for_command->name, 1)` at execute_cmd.c:2884 — *before*
  `line_number = for_command->line` at 2897 (identically for `select` at
  3401/3405). A bad loop variable is therefore blamed on the inherited line.
- **Redirections on a compound command.** A group, `while`, `until` or `if`
  never stamps itself, so a failing expansion in a redirection attached to one
  is blamed on the inherited line too.

Measured (bash 5.2.37, `bash FILE`):

```sh
echo one                        # 1
for \                           # 2
  'a[0]' \                      # 3
  in x; do :; done              # 4
# bash: line 4    osh: line 2
```

```sh
echo one                        # 1
( {                             # 2
  :                             # 3
} > "${nope?bad}" )             # 4
# bash: line 4    osh: line 2
```

The inherited value is the line the **reader stopped on**, not a leftover from
the previous command: `echo two; for \⏎ 'a[0]' \⏎ in x; do :; done` still
reports the `done` line, because `execute_command_internal` saves and restores
`line_number` around the simple command (execute_cmd.c:648, 703). Inside a
function it is the function *body*'s line instead — `execute_function` does
`line_number = function_line_number = tc->line` (execute_cmd.c:5205) — so the
same `for` inside `f() {` on line 2 reports line 2, not the `done`.

**The proper fix** is to stop seeding `current_line` from `Item::line` and
model bash's register honestly: record on each top-level parse unit the line
its **last** token sits on, seed `current_line` from that when the unit starts
executing, and let the already-correct per-command stamps (`SimpleCommand::line`,
`CaseClause::line`, `Shell::on_control_line`, …) save/restore over it as they
already do. A function call then seeds it from the body group's line, matching
execute_cmd.c:5205. Nothing should read `Item::line` for this purpose
afterwards.

**Impact.** Narrow but sharp-edged: only the two windows above are observable,
and only when the construct spans lines — but both are diagnostics, so the
divergence is a wrong line number in user-visible output. Everything reached
through a stamped executor (which is every expansion in a simple command, a
`case`, a loop header, a conditional or an arithmetic command) is already
right.

**Found by** the probe matrix for
TD-OILS-A-COMPOUND-COMMANDS-LINE-IS-STAMPED-AT-ITS-KEYWORD (probes `m2`,
`e1`–`e7`).

**Resolved 2026-08-08.** `Item::line` is gone — the field, its two parser stamp
sites and the `exec_items` assignment — and bash's register is modelled
directly:

- `Shell::run_source_flow_units` seeds `current_line` from
  `IncrementalParser::last_unit_end_line()` before each unit runs, which is
  where the reader left it.
- `Shell::exec_simple` brackets the simple command's own stamp with a
  save/restore, mirroring `cm_simple`'s
  `save_line_number = line_number; … line_number = save_line_number;`
  (execute_cmd.c:849, 863, 867). Without the restore a preceding command on the
  same unit would leave its line behind for the `for` after it.
- `Command::Subshell` grew a line of its own (`SubshellClause::line`, the `)`'s
  line — make_cmd.c:824, installed at execute_cmd.c:650) and runs through
  `Shell::on_control_line`.
- `FunctionDef::body_line` records where the body's `{` opened
  (`function_bstart`, parse.y:3271 → make_cmd.c:791); `Shell::call_function`
  installs it as bash's `line_number = function_line_number = tc->line`
  (execute_cmd.c:5205), before the entry DEBUG trap at 5238. The existing
  `call_line_stack` pop is the matching `unwind_protect_int (line_number)`
  (execute_cmd.c:5095).

Seeding `current_line` from the unit's end exposed one thing that had been
riding on the old per-item stamp: `declare -F NAME` under `extdebug` reports the
line a function was *defined* on, and `exec_function_def` was reading it from
`self.current_line`, which is now the unit's last line — so `g()⏎{⏎:⏎}` reported
the closing brace. Fixed properly by giving `FunctionDef` its own `line`, stamped
where bash stamps `function_dstart`: on a `)` closing a `(` after a WORD
(parse.y:3580), or on the word after the `function` keyword (parse.y:5349), the
later write winning. Measured against bash 5.2.37 for all five spellings
(`w()`, `w \⏎ ()`, `w( \⏎ )`, `function \⏎ w`, `function w \⏎ ()`); covered by
the extended `declare-F-under-extdebug` corpus case.

Two corrections to the hypotheses recorded above:

1. The claim that `execute_command_internal` "saves and restores `line_number`
   around the simple command (execute_cmd.c:648, 703)" named the wrong lines —
   648/703 are the **subshell** bracket. The simple-command bracket is 849/863/
   867 in the `cm_simple` arm. Both exist; the conclusion was right for the
   wrong reason.
2. The restores are **bare assignments**, not unwind-protects, so a
   `jump_to_top_level` flies past them. Implementing them unconditionally broke
   `a_discard_out_of_a_compound_command_loses_a_line`, which measures exactly
   that drift. `Shell::on_control_line` and `Shell::exec_simple` now consult the
   new `LineJump` trait and skip the restore for `Flow::Discard`/`Flow::Abort`.
   The function-call restore is *not* skipped, because bash's really is an
   unwind-protect — which is why a discard inside a function costs the caller
   only what the call's own surroundings cost.

**Regression cover:**
`userspace/oils/tests/corpus/an-unstamped-command-is-blamed-where-the-reader-stopped-not-at-its-first-token.sh`
— both windows, an enclosing group/`if`/`while`, three subshell shapes, three
function-definition shapes (including `g()⏎{` where the `{` is on its own line,
which discriminates the body line from the definition line), and the simple
command's own reduce-time stamp.
