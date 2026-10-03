## TD-B-A-FUNCTION-THE-SUITE-CANNOT-REACH-IS-A-FUNCTION-NOTHING-CHECKS -- OPEN 2026-09-15

`#[cfg(not(test))]` on anything other than `main` removes that code from the
test build entirely. Not "untested" -- UNREACHABLE. No test can call it,
`cargo test` compiles a binary that does not contain it, and every coverage
signal the project has says nothing about it either way.

**Found because dbus-daemon fabricated for as long as it did.** It printed
"system bus listening at ...", wrote a pid file containing "1", and returned
0 without creating a socket, with 132 tests passing. `run_dbus_daemon` was
`cfg(not(test))`. Its two sibling personalities, `run_dbus_send` and
`run_dbus_monitor`, were gated the same way and were fabricating the same
way -- and the second was found by looking for the gate, not by looking for
the defect.

**The sweep.** Seven crates in `userspace/` gate a function other than
`main`:

| crate | gated functions |
|---|---|
| `dbus` | `parse_send_args`, `run_dbus_send`, `run_dbus_monitor`, `run_main` -- **all fixed and ungated 2026-09-15** |
| `ctags` | `collect_dir`, `extract_tags_from_file`, `read_stdin_filelist`, `read_existing_ctags`, `print_help`, `run_main` |
| `lp` | `get_next_job_id`, `get_default_printer`, `current_username`, `run_lp`, `run_lprm`, `print_help` |
| `lex` | `print_help`, `print_version`, `run` |
| `yacc` | `run_main` |
| `chpasswd` | `print_help`, `print_version` |
| `mesg` | `print_help`, `print_version` |

**Not all of these are equal, and the difference is the point.** `print_help`
and `print_version` behind the gate cost little -- they print a constant and
exit. `collect_dir`, `extract_tags_from_file`, `read_existing_ctags`,
`run_lp` and `run_lprm` are ordinary logic with inputs and outputs, hidden
from the suite for no reason that shows at the call site.

**ALL SEVEN CHECKED, 2026-09-15. Two were fabricating; five were not.** The
cleared rows are here because they are the more useful half: they say the gate
is a testability smell rather than a reliable predictor of a lie, and where
not to look again.

| crate | outcome |
|---|---|
| `dbus` | **FIXED.** All three personalities fabricated. The daemon announced a bus it never listened on and wrote a pid file naming PID 1; `dbus-send` printed a method call "on wire" that went nowhere; `dbus-monitor` claimed to be monitoring and exited 0. All ungated and refusing. **Deleted 2026-10-01** under design-decisions §1049: what the refusals waited for, a path-bound `AF_UNIX` socket, is scheduled by no lane, and D-Bus compatibility is on no roadmap (restore point `707be9dc9`). |
| `lp` | **FIXED.** Reported queued print jobs and never captured the document -- for `-` it drained stdin, measured it, and dropped it. Predicted from this list plus a written-never-read field, which is how it was found. |
| `ctags` | **CLEAN.** A real tool: `File::create`, writes ctags/etags format, reports write errors. Probed end to end -- three source items in, three correct tag lines out, sorted. The gated functions are a testability gap, not a lie. |
| `lex` | **CLEAN.** Two write sites, six refusal messages. Does real work and says so when it cannot. |
| `yacc` | **CLEAN.** Three write sites, four refusal messages. Same. |
| `chpasswd` | **CLEAN.** `print_help` / `print_version` only. |
| `mesg` | **CLEAN.** `print_help` / `print_version` only. |

**What the gate predicted, and what it did not.** Two of seven were
fabricating -- so the gate is a useful place to look and a poor place to
conclude. What sharpened it was the PAIR: a gated entry point *plus* a field
on the written-never-read list. Both crates that had both were lying; none of
the five with only the gate was. That pairing is worth more than either list
alone, and it is cheap to compute.

~~**Still open:** the five clean crates keep code the suite cannot reach.
`ctags`'s `collect_dir`, `extract_tags_from_file` and `read_existing_ctags`
are parsers with inputs and outputs and no tests, which is a real gap even
though nothing is currently wrong behind it.~~

**`ctags` CLOSED 2026-09-16.** `collect_files`, `collect_dir` and
`extract_tags_from_file` are ungated and have five tests between them; 111 ->
116, clippy clean. (`read_existing_ctags` was already ungated and tested --
the row above was wrong about it, which is worth noting because the list was
built by grepping for the attribute and that function does not carry one.)

**The gate's effect, demonstrated rather than argued.** The moment the three
attributes came off, the test build began warning `function is never used` for
all three, with 111 tests passing. That is the entry's claim -- UNREACHABLE,
not untested -- shown by the compiler instead of asserted. The warnings went
away one at a time as each test landed, which is a coverage signal this crate
did not previously have any form of.

**What the tests are actually for**, since "add tests" is not a finding:

* `collect_files` -- the `missing` out-parameter, which exists because
  printing "cannot open" and returning only the successes is what made
  `ctags /nonexistent` exit 0. Its own doc comment says so. Nothing could
  call it to check until now. Tested with a present and an absent path in one
  run, because "missing was flagged" alone passes against a function that
  gives up on the first bad path, and "the good file was tagged" alone passes
  against one that never noticed the bad one.
* `collect_dir` -- sorting, which matters beyond tidiness: `read_dir` order
  is filesystem order, so an unsorted collector produces a tags file that
  differs between machines for no reason anyone can act on. Plus excludes and
  hidden-directory skipping, with a control run collecting the same tree with
  NO excludes -- without it, "target/gen.rs is absent" would pass against a
  collector that found nothing at all.
* `extract_tags_from_file` -- the half `extract_tags_from_content` cannot
  cover: opening the file, and choosing the language from the PATH rather
  than being handed one. The `.txt` case holds valid Rust in a file with the
  wrong extension, which is exactly how a language-by-extension bug hides.

**Still open:** `lex`, `yacc`, `chpasswd` and `mesg`. Those are
`print_help`/`print_version`/`run` -- the cheap end of the list, where the
gate costs a constant and an exit. Worth doing, worth doing last.

**Why the gate is usually there at all.** These crates build `#![no_main]`
for the real target and define a `main` the test harness must not duplicate.
That justifies gating `main`, and nothing else; the rest gets swept along
because it was written next to it. In `dbus` the only casualty of ungating
was two constants and an unused import.

**How to check:**

    grep -Pzo '#\[cfg\(not\(test\)\)\]\s*\n(?:#\[[^\n]*\]\s*\n)*(?:pub )?fn\s+\w+' \
        userspace/*/src/main.rs

or the Python in the commit that opened this entry. It cannot be a gate: a
crate may have a legitimate reason to gate a helper, and the check has no way
to tell one from an accident. It is a list to work through.
