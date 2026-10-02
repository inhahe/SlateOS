### TD-OILS-SYNTAX-ERR-ERREXIT-ECHO. bash drops the echoed source line from *every* syntax error once `set -e` is on, and forces the status to 2 — 2026-07-28 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` `Shell::format_parse_error` (the
`syntax error near ` second line) and `Shell::parse_error_flow`.

**What:** bash normally reports a syntax error on two lines — the message, then
the offending source line echoed back:

```
bash: -c: line 1: syntax error near unexpected token `newline'
bash: -c: line 1: `for'
```

With `set -e` in effect the *second* line disappears, for every syntax error,
not just command-substitution ones. It is not about the error being fatal:
`bash -e -c 'for'` and `set -e` at the top of a script both lose it, and a
plain `bash -c 'for'` keeps it. On top of that, the fatal
command-substitution status (see `TD-OILS-CMDSUB-ERR-FATALITY` item 2) collapses
to 2 under errexit wherever it would otherwise be 127 or 1 — *except* inside a
command substitution, where it stays 1 and the echoed line comes back.

**Reproduce:**
```sh
bash    -c 'for'                    # two lines, status 2
bash -e -c 'for'                    # ONE line, status 2
bash    -c 'eval "echo \$( ! )"'    # two lines, status 127
bash -e -c 'eval "echo \$( ! )"'    # ONE line, status 2
bash -e -c 'x=$(eval "echo \$( ! )"); echo st=$?'   # two lines again, st=1
```

**Assessment:** this looks like a bash bug rather than a designed behaviour —
`-e` has no business changing how a *parse* diagnostic is worded, and the
inconsistency inside a command substitution is the tell (bash almost certainly
frees or resets `shell_input_line` on the errexit path before
`report_syntax_error` reaches the echo). osh currently keeps the echoed line
and its own status in all of these. Before emulating it, decide whether we
want bug-for-bug parity here at all; if we do, the change belongs in
`format_parse_error` (suppress the echo when `errexit`) and in
`parse_error_flow` (status 2 when `errexit` and not inside a command
substitution), and needs a corpus file of its own.

**✅ RESOLVED 2026-07-30.** The assessment above was wrong on both counts, and
re-measuring is what showed it. This is **not** a bash bug and it is **not**
about the echoed source line.

*It is not about the echo.* The rule is "**keep only the first line of the
diagnostic**". Multi-line diagnostics discriminate the two models, and every one
of them collapses to a single line:

| source | plain | under errexit |
|---|---|---|
| `for` | message + echoed source line | message only |
| `[[ -n a` | `unexpected EOF while looking for `]]'` + `syntax error: unexpected end of file` | first only |
| `[[ a b ]]` | `conditional binary operator expected` + `syntax error near `b'` + echo | first only |
| `[[ (` | `unexpected token `EOF' …` + `expected `)'` + `unexpected end of file` | first only |
| `echo "` | one line | unchanged |

Two of those dropped lines are follow-on *messages*, not echoes, so "suppress
the source echo" cannot explain them.

*It is not arbitrary.* The condition is simply errexit's state **at the moment
the error is reported**, and every apparent inconsistency follows from that:

* `-c 'set -e; for'` keeps all its lines — `set -e` and `for` are one
  `;`-separated list, so they are parsed together and `set -e` has not run yet.
  The same two commands on separate *lines* lose them, because input is parsed
  and executed command by command. (`-e -c 'set +e; for'` is truncated for the
  mirror-image reason.) This also disposes of the "`-e` changes how a parse
  diagnostic is worded" objection: the option is read when the *report* is
  emitted, not when the text is parsed.
* the "tell" — `-e -c 'x=$(eval "for")'` getting the full form back while
  `-e -c '(eval "for")'` does not — is because **errexit is not in effect inside
  a command substitution**. Measured directly rather than guessed: `$-` is
  `hBc` (no `e`) inside `$( … )` against `ehBc` inside `( … )`, `set -o` reports
  `errexit off` there, and `-e -c 'x=$(false; echo reached)'` prints `reached`.
  That is the default-off `inherit_errexit` shopt, which osh already modelled —
  so this case needed no special-casing at all, and `shopt -s inherit_errexit`
  makes the command substitution truncate like the subshell in both shells.
* it is errexit *specifically*, not "any option": `-u`, `-x`, `-f`, `-n`, `-E`
  and `-o pipefail` all leave the diagnostic alone; only `-e` / `-o errexit` do
  this.

*The status.* A syntax error's own `$?` is 2. Where a parse error is **fatal**
(the command-substitution-body case, `TD-OILS-CMDSUB-ERR-FATALITY` item 2) the
shell normally leaves by that fatality path carrying 127 (in `-c`) or 1; under
errexit it leaves by errexit's own exit instead, which carries the 2. So the
collapse is not a special case for parse errors — it is which of two exit
mechanisms fires.

**Fix shipped.** Two small changes, both keyed on `self.errexit`:

* `interp.rs` `format_parse_error` — truncate the assembled diagnostic at its
  first newline. Doing it at the end, after the echo has been appended, is what
  makes one line of code cover all four shapes above.
* `interp.rs` `parse_error_flow` — a fatal parse error scores 2 under errexit
  instead of the invocation-mode 127/1. Nothing was needed for the
  command-substitution case, since errexit is already unset there.

**Tests.** `interp::tests::errexit_keeps_only_the_first_line_of_a_syntax_error`
(asserts the terse form *equals the full form's first line* for four shapes, so
it cannot drift) and `tests/corpus/errexit-syntax-error.sh` (both forms of each
shape, the `;`-vs-newline timing pair, `set -e; set +e`, the command-substitution
/ subshell / `inherit_errexit` trio, and the 127→2 collapse).

**Left out of the corpus deliberately:** the `[[ -n a` pair, because osh blames a
different line number for it than bash — the unrelated residue recorded under
TD-OILS-CASE-PATTERN-EOF below. The unit test covers that shape instead, where
the line number is not what is asserted.
