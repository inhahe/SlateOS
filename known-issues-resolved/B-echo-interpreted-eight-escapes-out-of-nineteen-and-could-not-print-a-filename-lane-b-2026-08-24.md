## `echo` interpreted eight escapes out of nineteen, and could not print a filename (lane B, 2026-08-24)

**In short:** `echo` is the smallest utility we ship and it was missing more
rules than any of the big ones. It knew `-n` and `-e` but not `-E`; it knew
eight backslash escapes but not the other eleven; it had no `--help` and no
`--version`; and handing it an argument containing a byte that is not valid
UTF-8 — a perfectly legal filename on this OS, and `echo` is the tool you would
reach for to print one — made it panic before it printed anything. It also
threw away the result of its own write, so `echo hi > /dev/full` on a full disk
said nothing and exited 0, telling any script that asked that the write had
happened. Rewritten from GNU coreutils 9.4's `src/echo.c`, and checked against
the real GNU binary case by case.

### Why "it's just echo" was exactly the problem

Every earlier port here started from a bug report. This one started from
`scripts/argv-utf8.py`, which lists the binaries whose `main` collects argv into
a `Vec<String>`. That collection panics on a non-UTF-8 byte, so its presence is
a reliable marker for a program nobody ever exercised against real input. `echo`
was on that list. It is 285 lines and looks self-evidently finished, which is
precisely why nothing had ever been checked against upstream — and the count
came out at eleven missing escapes, three missing options, and a discarded
`Result`.

### What was missing

1. **`-E`** — the option that turns escapes back off. Absent entirely, so
   `echo -e -E 'a\tb'` printed a tab where GNU prints `a\tb`. The last of
   `-e`/`-E` wins, within a bundle and across words, which is the only thing
   that distinguishes those two command lines.
2. **`\c`** — ends the *program*: no trailing newline, and every later argument
   dropped. Upstream is literally `case 'c': return EXIT_SUCCESS;`. We printed
   `\c`.
3. **`\e`** (escape, 0x1B) and **`\v`** (vertical tab) — both absent.
4. **`\xHH`** — the entire hexadecimal family. One or two digits; with no hex
   digit following, upstream `goto not_an_escape` and prints `\x` literally,
   which is not the same as printing nothing.
5. **Octal beyond one digit.** We handled `\0` as a NUL and stopped. Upstream
   takes up to three octal digits, and — not documented in its own `--help` —
   has switch arms for `'1'` through `'7'` as well as `'0'`, so `\101` is `A`.
   The arithmetic is `unsigned char`, so it wraps: `\0777` is `\xFF`, not a
   clamp and not an error.
6. **`--help` and `--version`** — neither existed. Upstream recognises them only
   when they are the *entire* command line (`allow_options && argc == 2`) and
   never abbreviated; it declines `parse_long_options` and says why in a
   comment: *"in order to avoid accepting abbreviations."* So `echo --help x`
   prints `--help x`, and `echo --hel` prints `--hel`.
7. **`POSIXLY_CORRECT`.** Unread. It changes `echo` more than any option does:
   the printing loop is entered on `do_v9 || posixly_correct`, so escapes are
   decoded with no `-e` at all and `-E` cannot switch them back off; and
   `allow_options` goes false, so `-e` stops being an option and becomes a word
   to print. The single exception is a first argument of exactly `-n`, which
   keeps option parsing alive for the whole command line.
8. **A bare `-`, and any word with a character outside `{e,E,n}`,** are text and
   stop the option scan for good. We had no such rule, so `echo -en1 'a\tb'`
   was parsed as options where GNU prints `-en1 a\tb`.
9. **Non-UTF-8 argv** — `Vec<String>` panicked. Now `OsString` throughout, with
   the bytes carried to the syscall unchanged.
10. **`let _ = out.write_all(...)` and `let _ = out.flush()`** — both discarded.
    GNU registers `close_stdout` with `atexit`, so a failed write is diagnosed
    once and the status is 1. We now write the whole output in one call and
    report `echo: write error: <strerror>` with status 1, which is byte-identical
    on the four `/dev/full` cases in the harness.

### The one structural difference from upstream, and why it is safe

Upstream `putchar`s into a buffered stream and lets `atexit(close_stdout)`
notice the failure. We build the output bytes first and write them once. The two
observable properties that matter — exactly one diagnostic, and exit status 1 —
are the same, and the harness checks them. The rearrangement also makes `\c`
need no special case: by the time it ends the program it has already contributed
everything it was going to contribute.

### How it was checked: 93 cases, and two controls

`scripts/echo-diff.sh` builds this binary for Linux inside WSL and runs it
against the real `/usr/bin/echo`, comparing stdout as bytes (`od -An -c`, so
`\xff` survives the comparison), stderr, and exit status. **93 passed, 0
differed, 2 differ on purpose.**

Zero differences on a first run is the kind of result that is more often a
broken harness than a correct program, so it was not taken on trust. Two
controls:

- **`OURS=/usr/bin/echo`** — point the harness at the reference itself. Result:
  93 pass and exactly the two `xfail`s flip to `XPASS`. That proves no case is
  *wrong about GNU*; a case encoding a mistaken expectation would show up here
  as a diff against GNU itself.
- **`OURS=` a dash-builtin wrapper** — point it at an `echo` known to be
  different. Result: **59 of the 93 cases caught it.** That proves the cases
  discriminate rather than all agreeing vacuously.

Both were necessary. The first alone would pass against a harness that compared
nothing; the second alone would pass against a harness full of wrong
expectations.

### Deliberate divergences, both recorded as `xfail`

| Case | Why |
|---|---|
| `--help` | Omits the GNU project's ancillary block (bug-report address, home page, `info` reference), as every converted utility here does. The two upstream NOTEs are kept — the first is the only warning a user gets that the `echo` they typed was almost certainly their shell's builtin. |
| `--version` | Names SlateOS rather than GNU coreutils. |

Nothing else. In particular the closed-descriptor divergence that `tee` has does
not arise for `echo`, which never reads.
