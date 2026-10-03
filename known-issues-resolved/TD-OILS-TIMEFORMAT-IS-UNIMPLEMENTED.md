### TD-OILS-TIMEFORMAT-IS-UNIMPLEMENTED, so `time`'s report has only its two built-in shapes — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Resolved:** `Shell::format_time_report` is gone, replaced by
`Shell::time_format` (which picks the format string: `-p`'s POSIX one, else a
set `$TIMEFORMAT`, else the posix shell-times or the default form) and
`Shell::render_time_report` + `Shell::mkfmt` (which render it). Everything
described below is implemented, including the empty-format case, the
`builtin_error` (not `report_error`) severity of a bad directive, and the
posix-mode bare `time` reporting the shell's *lifetime* through a `%R` — for
which `Shell` gained a `birth` instant that, unlike `seconds_anchor`,
`SECONDS=0` cannot rebase. Covered by `time_report_formatting`,
`timeformat_directives_and_their_modifiers`,
`timeformat_is_read_after_the_command_and_never_fatal`,
`posix_mode_bare_time_reports_the_shell` and the corpus case
`the-time-report-is-rendered-from-timeformat.sh`. `%U`, `%S` and `%P` read zero
no longer either: TD-OILS10 was resolved immediately afterwards and they now
carry real CPU figures on a Windows host.

Two details the probing added to the description below: only **one** precision
digit is ever read (so `%99R` fails on its second `9`), and `%P` is matched
*before* the precision/`l` scan, so it accepts neither (`%0P` is an error, not
a two-place `%P`). The named character in the diagnostic is whatever bash's
cursor found, so a format that runs out mid-directive names the NUL.

**Where:** `userspace/oils/src/interp.rs`, `Shell::format_time_report` and its
caller in `exec_pipeline`.

**What:** bash formats a `time` report from `$TIMEFORMAT`, and osh has no such
variable — it picks between two hard-coded shapes (bash's default, and `-p`'s
POSIX one) and ignores `$TIMEFORMAT` entirely.

**Reproduce:**

```
$ bash --norc -c "TIMEFORMAT='R=%R U=%U S=%S'; time sleep 0.05"
R=0.078 U=0.000 S=0.015
$ osh -c "TIMEFORMAT='R=%R U=%U S=%S'; time sleep 0.05"

real	0m0.062s
user	0m0.000s
sys	0m0.000s
```

**What the format is** (probed against the reference bash, 5.2.37):

* `%%` is a literal `%`. Everything else that is not a directive is copied
  through, newlines included, and a trailing newline is always added.
* The directives are `%R` (elapsed), `%U` (user CPU), `%S` (system CPU) and
  `%P` (CPU percentage, `(U+S)/R*100`).
* Each takes an optional precision `0`–`3` (default `3`) and an optional `l`
  for the long `%dm%.*fs` form, in that order: `%2lR` → `0m0.07s`, `%0R` → `0`,
  `%lR` → `0m0.078s`. `%P`'s default precision is `2`.
* An unrecognised directive is an error — `TIMEFORMAT: `t': invalid format
  character` — and suppresses the report.
* An **empty** `TIMEFORMAT` prints nothing at all; an **unset** one falls back
  to bash's default `\nreal\t%3lR\nuser\t%3lU\nsys\t%3lS`.
* `time -p` overrides `$TIMEFORMAT` completely and always prints the POSIX
  three-line form.

**It also hides a refinement of the posix-mode null-command report.** A bare
`time` in posix mode reports the *shell's* own times (see
TD-OILS-POSIX-MODE-BEHAVIOURS-ARE-NOT-IMPLEMENTED), and the reason no `real`
line appears is simply that the format bash uses for that case has no `%R` in
it — with `TIMEFORMAT='R=%R U=%U S=%S'` a bare `time` does print an `R`, and it
is the shell's elapsed *lifetime*: `R=0.006` at the top of a script and
`R=0.338` after a `sleep 0.3`. osh cannot show that until both this and
TD-OILS10 are done.

**Proper fix:** parse `$TIMEFORMAT` into a small directive list and render it,
replacing the two hard-coded shapes with the default format string. The
precision/`l` handling and the `%P` computation are the whole of the work; the
values themselves come from the same places they do now (and user/sys stay zero
until TD-OILS10 is fixed, which is why this was not done alongside the
posix-mode `time` work — a `$TIMEFORMAT` that could only ever report zeros for
two of its four directives is not worth a corpus case yet).
