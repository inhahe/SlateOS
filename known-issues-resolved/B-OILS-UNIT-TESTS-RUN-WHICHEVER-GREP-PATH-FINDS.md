## B-OILS-UNIT-TESTS-RUN-WHICHEVER-GREP-PATH-FINDS — six `osh` unit tests fail when `cargo test` is run from PowerShell on this machine (lane B, 2026-10-01) — **FIXED** 2026-10-01

**Resolution.** Every unit test that piped the shell's output through `grep`
now captures it and keeps the lines in Rust (`lines_where` beside `run` in
`interp.rs`'s tests) -- the ten sites that executed one, including the four
whose answer happened to survive Embarcadero's `grep`. The one child left is
`env`, which is what the test it sits in is about (what reaches a child's
environment); its lines are read in Rust too. From PowerShell, with
Embarcadero's `grep` first on `PATH`, all 1507 pass. (The `tests/corpus`
scripts still use `grep`, as shell scripts do, and are not affected: the
differ runs bash and `osh` under the same `PATH`.)

**In short:** some of `osh`'s unit tests pipe the shell's output through an
external `grep` to pick out the lines under test (22 sites in
`userspace/oils/src/interp.rs`, e.g. `readonly -p | grep ' [ab]='`). The
`grep` is whichever one `PATH` finds. From Git Bash -- where the pre-push hook
and the boot test run -- that is GNU grep and every test passes. From
PowerShell on the development machine the first `grep` on `PATH` is
Embarcadero's (`C:\Program Files (x86)\Embarcadero\Studio\23.0\bin\grep.exe`),
which prints a `STDIN` header and CRLF line ends, and six tests fail:
`readonly_print_lists_vars`, `local_dash_binds_a_variable_no_listing_reports`,
`funcname_is_present_and_empty_outside_a_function`,
`posix_mode_is_the_posixly_correct_variable`,
`a_declaration_without_a_value_still_brings_the_name_into_being` and
`assigning_a_dynamic_variable_stores_the_number_not_the_text`. Nothing is
wrong with the shell; the tests are not hermetic.

**The proper fix:** filter in the test, not in a subprocess -- capture the
whole output and keep the wanted lines in Rust -- so a unit test of the shell
depends on nothing but the shell. Where the pipeline itself is under test, the
test should name its tool (the workspace's own `grep`, built alongside) rather
than take `PATH`'s.

**How to see it:** in PowerShell, `cargo test -p oils --lib --target
x86_64-pc-windows-msvc`; the same command in Git Bash passes.
