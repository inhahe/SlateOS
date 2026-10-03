## TD-B-SED-HAS-NO-POSIX-MODE-AND-NO-FOLLOW-SYMLINKS (lane B, 2026-10-03)

**Status:** OPEN

**In short:** GNU sed has three levels of strictness -- its default, the one
it switches to when the environment variable `POSIXLY_CORRECT` is set, and
`--posix`, which turns off every GNU extension. Ours accepts `--posix` and
does nothing with it, and reads `POSIXLY_CORRECT` only where option parsing
stops. So a script that GNU refuses under `--posix` runs here, and a few
things behave differently under `POSIXLY_CORRECT` -- `w /dev/stdout` is the
measured one: GNU opens the name as a file there, ours still treats it as
standard output. `--follow-symlinks` is likewise accepted and ignored: `-i`
on a symbolic link should edit the file it points to, and ours replaces the
link with a file.

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

And `--follow-symlinks` (`open_next_file`, `follow_symlink` in `utils.c`):
with `-i`, the file edited and replaced is the link's final target, so the
link survives; without it, `-i` renames a new file over the link.

### Where it bites here

`userspace/coreutils/src/bin/sed.rs`: the option arm
`Opt::Long("binary" | "posix" | "follow-symlinks", _) => {}`, and
`open_wfiles`/`open_rfiles`, which treat the three special names specially
whatever the mode. Measured: `POSIXLY_CORRECT=1 sed 'w /dev/stdout'` on
`a\nb` (no final newline) prints `a\nba\nb` in GNU -- the `w` file is a second
buffer, flushed first at exit -- and `a\na\nbb` here.

### The proper fix

A `Posixicity` enum computed in `main` exactly as GNU computes it, threaded
into the compiler (refusals, address forms, `a/i/c`, escapes, `l N`), the
regex syntax flags (`ere` already has the BRE/ERE GNU-operator switches), the
special-file table, `N` at end of input, and the replacement's case
conversions; `--follow-symlinks` resolving the operand before `-i` opens it.
Each row of the table above becomes a `sed-diff.sh` case under both modes.
