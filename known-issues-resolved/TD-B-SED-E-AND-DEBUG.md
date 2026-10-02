## TD-B-SED-E-AND-DEBUG — the `e` command and `--debug` are still missing (lane B, 2026-08-24) — ✅ **FIXED 2026-08-24**

**Fixed** in sed tranche 2d. `e`, `e COMMAND` and `s///e` all go through
`coreutils::shell::shell_bytes`, which is the one place in the tree that decides
what "the shell" is — so the policy question this entry parked turned out to
have been answered already, by `awk`'s `system()` and `split --filter`. All
three are refused by `--sandbox`, and
`the_sandbox_refuses_every_command_that_reaches_outside_the_script` now names
them.

`--debug` is implemented in full: the `SED PROGRAM:` dump (built during parsing,
into `Command::dump`, because that is the only moment the script's text still
exists) and the runtime trace (`INPUT:`, `PATTERN:`, `HOLD:`, `COMMAND:`,
`MATCHED REGEX REGISTERS`, `END-OF-CYCLE:`). Forty-odd `--debug` cases in
`scripts/sed-diff.sh` compare it against GNU byte for byte; one case differs on
purpose, for the reason in `design-decisions.md` §379 — GNU sign-extends a byte
≥ 0x80 into `\o37777777600`, and we print `\o200`.

Two unrelated defects surfaced while wiring the trace's `INPUT:` line, and were
fixed with it:

- `Job::in_place` handed a `-` operand to `Input`, which mapped `-` to standard
  input unconditionally — contradicting the `File::open` immediately above it,
  so `sed -i 's/a/A/' -- -` half-edited a file and half-read a stream. `Input`
  now carries `dash_is_stdin`.
- `Input::fill` reads one line ahead, and updated `cur_name` when it opened the
  *next* file — so at a file boundary `F` and `INPUT:` named the wrong file.
  Each `Line` now carries its own `Rc<OsString>` name.

The original entry follows.

---


**What it is.** Two gaps are left in `sed` after TD-B-SED-MISSING-COMMANDS
closed the other five:

| Missing | What GNU does | What ours does |
|---|---|---|
| `e` / `e COMMAND` / `s///e` | runs the pattern space (or COMMAND) through the shell and substitutes the output | ``unknown command: `e'`` (status 1) |
| `--debug` | prints an annotated trace: the parsed program, then `INPUT:`/`PATTERN:`/`COMMAND:`/`MATCHED REGEX REGISTERS`/`END-OF-CYCLE:` per cycle | accepted and ignored, so the run's ordinary output appears instead |

**Why they are still out.** `e` needs a shell to hand the command to, and the
one question it raises — *which* shell, and what happens when there is none —
is a policy question rather than a coding one on an OS that does not have
`/bin/sh` at a fixed path yet. `--debug` is a large, exactly-specified output
format whose only consumer is a person reading it; it is worth doing, but it is
worth doing after the wording tranches (2c) that share its error-reporting
machinery.

**Reproduce.** `bash scripts/sed-diff.sh` — the case `sed --debug s/a/A/`
reports DIFF, showing our ordinary output against GNU's trace. `e` has no
harness case because there is nothing yet to compare.

**The proper fix.**

1. `e` — spawn the shell with the pattern space as its command, replace the
   pattern space with its stdout, and drop one trailing separator. `s///e` does
   the same to the *result* of the substitution. Both must be refused by
   `Parser::deny_in_sandbox`, which already exists and already carries the
   message naming them; add the two call sites and extend
   `the_sandbox_refuses_every_command_that_reaches_outside_the_script`, which
   names this entry in a comment.
2. `--debug` — a flag on `Exec` and a writer that renders the compiled program
   back to text. The rendering is the bulk of it: it must round-trip every
   command, which is a useful check on the parser in its own right.

Scoped as sed tranche 2d.
