### TD-OILS-PROMPT-COMMAND-NUMBER-AND-SHELL-NAME. `\#` never left 1, and `\s` answered with `$0` — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::prompt_decode`'s `'#'` and
`'s'` arms, which had no state of their own to read: `'#'` shared the `'!'` arm's
history number and `'s'` expanded `$0`.

**What.** The two escapes name the shell and the command it is running, and
neither is derivable from anything osh already tracked.

```text
$ p='\#'; echo "${p@P}"; echo "${p@P}"   # …as two lines of a script
bash: 5 / 6      # once per top-level command the reader begins
osh : 1 / 1      # the history number, which a script never advances

$ bash d/s.sh    # with s='\s' inside
bash: bash       # the name the shell was started under
osh : d/s.sh     # $0 — the name of the script it is running
```

`\#` is bash's `current_command_number` (`shell.c:174` seeds it to 1;
`eval.c:166` advances it in `reader_loop`, between parsing a command and running
it). Everything reached *from inside* a command therefore shares that command's
number — a function body, a loop body, a group, a subshell, `$( … )`, an `eval`
string, a sourced file, a trap action — and two commands on one line share one
number while a ten-line compound has one. It is not a count of commands executed
and not a count of lines. The counter also runs one ahead of what is shown:
`parse.y:5976` subtracts one back off everywhere except PS0/PS1/PS2, so `${p@P}`
and `PS4` name the command they are expanded for and agree with each other. A
`-c` string goes to `parse_and_execute` and never through the reader loop, so
`\#` is 0 for every command in it.

`\s` is `base_pathname(shell_name)`, and `shell_name` is argv[0] at startup, the
`-c` *name* operand, or a `BASH_ARGV0=` assignment — but **not** the script path,
which moves `$0` alone. (`\!` was already right: it is `history_number()`, which
`parse.y:5646` leaves unadjusted at 1, so a shell that has recorded nothing shows
1 where `$HISTCMD` shows 0.)

**Fix (2026-08-06).** Two new `Shell` fields. `command_number` starts at 1, is
advanced in `run_source_flow_units` only for `HistRead::Reader` units outside
command mode — the shell's own top-level reader, not `eval`/`source`/traps/`$( )`
— and is inherited unadvanced by a subshell, as a fork would. `shell_name` is
seeded from argv[0], set by `set_name` (which `-c` uses) and by a `BASH_ARGV0=`
assignment, but left alone by the new `set_script_name`, which is what opening a
script calls so that only `$0` moves.

**Found by** auditing `${x@P}` against bash escape by escape. The audit also
turned up TD-OILS-A-TRAILING-SEMICOLON-OR-AMPERSAND-JOINS-A-LINE-TO-THE-NEXT-ONE
below, whose symptom was `\#` running one short per `&` in the script.

**Tests.**
`tests/corpus/the-prompt-escapes-that-name-the-shell-and-its-command.sh` — the
number's granularity across ten containing constructs, its agreement with `PS4`,
the history number beside `$HISTCMD`, and `\s` against `$0` and `BASH_ARGV0`.
The invocations a corpus case cannot reach are in `tests/cli_options.rs`:
`prompt_command_number_counts_only_the_readers_own_commands` (0 throughout a
`-c` string, counted from stdin) and
`prompt_shell_name_is_the_name_the_shell_started_under`.
