### BUG-OILS-EXIT-NOT-HONOURED-AT-TOP-LEVEL. `exit` from stdin never ended the shell; an `exit` in an EXIT trap or a `mapfile -C` callback was swallowed — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Three more instances of the same defect class as
`BUG-OILS-SOURCE-FRAME-SEMANTICS` — a `Flow::Exit` collapsed into a mere status
by a caller that then carried on. Each row was checked against bash 5.x
(`C:/Program Files/Git/usr/bin/bash.exe`) before being fixed:

| Case | bash | osh (before) |
|---|---|---|
| `printf 'echo one\nexit 3\necho two\n' \| osh` | prints `one`, exits 3 | printed `one` **and** `two`, exited 0 |
| `trap 'exit 9' EXIT; exit 2` (script or `-c`) | exits 9 | exited 2 |
| `( trap 'exit 9' EXIT; exit 2 ); echo $?` | 9 | 2 |
| `cb() { echo cb; exit 5; }; mapfile -C cb -c 1 arr < f` | one callback, shell exits 5, `arr` unset | callback ran for every line, shell continued |

**Root causes.**

1. `repl()` (`main.rs`) called `Shell::run_source`, which collapses `Flow::Exit`
   into a status, and then looped back for the next prompt — so the *only* way
   the REPL/stdin driver ever stopped was EOF. (`-c` and script mode were fine:
   there the whole buffer is one `run_source`, and `exec_program_top` already
   stops at the `exit`.)
2. `run()` returned the mode's own `code`, ignoring the EXIT trap. The trap's
   `exit N` *was* recorded in `last_status`, but the driver could not simply
   read that instead: some statuses (e.g. 127 for an unreadable script) never
   pass through `last_status` at all.
3. The `Command::Subshell` arm fired the subshell's EXIT trap and *then*
   overwrote `last_status` from the body's `Flow::Exit`, clobbering the
   handler's override — and firing the handler before the body's status landed
   meant it also saw the wrong `$?`.
4. The `mapfile -C` callback ran through `run_source_out`, discarding its flow.

**Fix.**

* New `Shell::exit_requested` flag, latched by `run_source_out` (the outermost
  entry point, the only place a `Flow::Exit` means "everything has unwound") and
  exposed as `Shell::exit_requested()`. `repl()` polls it after each command and
  returns. Nested shell code is untouched: it still unwinds via `Flow::Exit` and
  only reaches the flag if it propagates all the way out — so `( exit 4 )` does
  not latch it.
* `run_exit_trap{,_out}` now return `Option<i32>` — `Some(code)` iff the handler
  ran `exit N` — and `run()` does `sh.run_exit_trap().unwrap_or(code)`.
* The subshell arm lands the body's `Flow::Exit` in `sub.last_status` *before*
  firing the EXIT trap (so the handler sees it as `$?`) and no longer re-applies
  it afterwards.
* The `mapfile` callback uses `run_source_flow_out` and routes an `exit` through
  `pending_builtin_exit`, stopping the read loop so the array is never assigned
  (bash does the same).

**Tests.** `exit_trap_reports_its_own_exit_to_the_driver`,
`subshell_exit_trap_exit_replaces_body_status`,
`top_level_exit_latches_exit_requested`, `mapfile_callback_exit_unwinds_shell`
(interp), plus `exit_from_stdin_ends_the_read_loop` and
`exit_trap_exit_replaces_the_shells_status` driving the real binary
(`tests/cli_options.rs`). All six failed before the change. Suite: 722 + 15
pass, `cargo clippy -p oils --all-targets` clean.
