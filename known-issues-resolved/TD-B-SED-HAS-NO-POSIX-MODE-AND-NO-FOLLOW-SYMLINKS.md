## TD-B-SED-HAS-NO-POSIX-MODE-AND-NO-FOLLOW-SYMLINKS (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B); boot-tested on main (394c97655, published as 109a26eec) -- `--follow-symlinks` in `B-SED-I-REWROTE-FILES-WHERE-THEY-STOOD`, the modes in the change described under "Fixed" below.

**In short:** GNU sed has three levels of strictness -- its default, the one
it switches to when the environment variable `POSIXLY_CORRECT` is set, and
`--posix`, which turns off every GNU extension. Ours accepts `--posix` and
does nothing with it, and reads `POSIXLY_CORRECT` only where option parsing
stops. So a script that GNU refuses under `--posix` runs here, and a few
things behave differently under `POSIXLY_CORRECT` -- `w /dev/stdout` is the
measured one: GNU opens the name as a file there, ours still treats it as
standard output. (`--follow-symlinks` was likewise accepted and ignored until
2026-10-03; it is now GNU's.)

### What GNU sed 4.9 does, by mode

`posixicity` is `POSIXLY_EXTENDED` by default, `POSIXLY_CORRECT` when the
variable is set, `POSIXLY_BASIC` under `--posix` (`sed.c`). Every place it is
read:

| where | `POSIXLY_CORRECT` | `--posix` |
|---|---|---|
| `w`/`R` of `/dev/stdin`, `/dev/stdout`, `/dev/stderr` (`get_openfile`) | ordinary files | ordinary files |
| `N` on the last line (`execute.c`) | discards the pattern space | discards it |
| `\n`, `\t`… inside a bracket of a regex (`convert_ANSI`/`match_slash`) | not translated | not translated |
| `RE_UNMATCHED_RIGHT_PAREN_ORD` (`regexp.c`) | a lone `)` in an ERE is a character | the same, plus `RE_NO_GNU_OPS` (no `\w`, `\b`, `` \` ``…) and, in a BRE, `RE_LIMITED_OPS` (no `\|`, `\+`, `\?`) |
| `s///` referring to a group the regex lacks | not refused at compile time | not refused |
| `s` flags `i`/`I`, `m`/`M`, `e` | allowed | `unknown option to 's'` |
| address forms `first~step`, `addr,+N`, `addr,~N`, `0,/re/`, `/re/I`, `/re/M` | allowed | refused |
| commands `e F v z L`, and one-address-only `a i r =` etc. | allowed | refused |
| `a`, `i`, `c` one-liner form (text on the same line) | allowed | `expected \ after 'a', 'c' or 'i'` |
| `l N`, `L N` with a number | allowed | the number is not read |
| replacement `\L \U \l \u \E` | case conversion | the letter itself |
| an incomplete command at the end of the script | allowed | `incomplete command` |
| a regex the DFA warns about, e.g. `[:alpha:]` outside a bracket (`dfawarn`) | accepted -- by default it is refused | refused, as by default (the test is the variable, not the mode) |

### Where it bites here

`userspace/coreutils/src/bin/sed.rs`: the option arm
`Opt::Long("binary" | "posix", _) => {}`, and
`open_wfiles`/`open_rfiles`, which treat the three special names specially
whatever the mode. Measured: `POSIXLY_CORRECT=1 sed 'w /dev/stdout'` on
`a\nb` (no final newline) prints `a\nba\nb` in GNU -- the `w` file is a second
buffer, flushed first at exit -- and `a\na\nbb` here.

### Fixed

`sed.rs`: a `Posixicity` computed as GNU's `main` computes it
(`Mode::from_run`) and threaded through the parser -- every row above --
with `v` switching the rest of the script and the run back to `Extended`;
the special names decided per command by the mode in force there (`Target`);
`N` at the end of the input asking the run's final mode; `ere::sed::
regex_posix` for GNU's bracket-aware `normalize_text`; and the regex syntax
bits through `ere` (`Syntax::unmatched_right_paren_ord`, `BreSyntax::
{no_gnu_ops, unmatched_right_paren_ord, reg_newline}`). An `a` text left
unfinished at the end of one `-e` is `incomplete command` there under
`--posix`, as GNU compiles each fragment on its own.

`scripts/sed-diff.sh`: 44 cases under `--posix` and 19 under
`POSIXLY_CORRECT` (`ENVV`), each row of the table at least once.
