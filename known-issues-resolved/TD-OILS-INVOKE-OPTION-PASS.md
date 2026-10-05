### TD-OILS-INVOKE-OPTION-PASS. Four bugs in the single-letter invocation pass — `-o`/`-O` could not bundle, `-c`/`-s` ended the walk, `-cs` was last-wins, and bare `-` meant `-s` — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/main.rs`, the letter loop in `run()`.

**What.** Rewriting the command line for TD-OILS-NO-STARTUP-FILES needed the
letter pass measured properly, and doing so exposed four pre-existing
divergences from bash's `parse_shell_options`. All four came from the same
misreading: that the pass is a *loop over words that stops at the first
non-option*, when in fact it is a **cursor** that individual letters may advance
and that only `--`, a bare `-`, or a word starting with neither `-` nor `+`
stops.

1. **`-o`/`-O` could not be bundled and did not take the next word.** osh had a
   standalone `"-o" | "+o" | "-O" | "+O"` arm, so `osh -eo pipefail` and
   `osh -oc pipefail cmd` failed. In bash they are letters like any other, each
   taking `argv[next_arg]` — the *next word*, never the rest of its own cluster
   — and each one seen advancing the cursor. That cursor is also what decides
   where a bundled `-c`'s command string starts, which is why `-oc pipefail cmd`
   runs `cmd`. With no word left they **list** the options (`-o` columns, `+o`
   re-inputtable `set ±o name`, `-O`/`+O` the `shopt` equivalents) and the shell
   *carries on* rather than failing.
2. **`-c`/`-s` wrongly ended option processing.** bash's `case 'c'`/`case 's'`
   only set a flag, so the walk continues into the *next word*: `bash -c -x
   'echo hi'` traces the command instead of trying to run `-x`, and `bash -s -x`
   applies `-x` rather than making it `$1`. osh broke out of the loop, so both
   were wrong.
3. **`-cs` was last-wins.** `-c` and `-s` are independent flags, not one
   setting, and bash looks for a pending command string *before* it looks at
   `read_from_stdin` — so `-c` wins in either order. osh let the later letter
   win, making `osh -sc cmd` read stdin.
4. **A bare `-` was treated as `-s`.** It is purely end-of-options, identical to
   `--` (both have an empty tail, so the walk stops). A shell started that way
   reads stdin only because nothing is *left* to run as a script, which is why
   the word after it is a **filename**: `bash - s.sh a b` runs `s.sh` with
   `$1=a`. osh read stdin and made `s.sh` into `$1`.

**Fix.** One cursor (`next_arg`) threaded through the cluster loop, `'o' | 'O'`
handled as an ordinary letter that consumes the next word (falling back to
`Shell::format_named_option_list` / `Shell::format_shopt_list` when there is
none), `'c'`/`'s'` setting flags without breaking, `-c` resolved before `-s` in
the mode decision, and `"-"` merged into the `"--"` end-of-options arm.

**Not reproduced.** bash prefixes the `-o`/`-O` **bad name** diagnostics with
`line 0: ` and, for `-o`, repeats the shell name
(`bash: line 0: bash: bogus: invalid option name` vs osh's
`osh: bogus: invalid option name`). That is `builtin_error` firing with
`this_command_name` set and no line number yet; it is cosmetic, affects only the
error path, and is not worth threading a fake line number through for. The
corpus case pins the *status* (2) and the fact that the command does not run.

**Tests.** `tests/corpus/invoke-options.sh` (new, byte-identical to bash 5.2)
pins all four, plus `$-`'s letters and the "unrecognised letter aborts having
applied the ones before it" rule; 10 new tests in `tests/cli_options.rs` drive
the same shapes through the real binary.
