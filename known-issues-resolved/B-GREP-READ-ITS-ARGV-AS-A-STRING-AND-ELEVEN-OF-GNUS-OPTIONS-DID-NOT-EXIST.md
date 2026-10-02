## `B-GREP-READ-ITS-ARGV-AS-A-STRING-AND-ELEVEN-OF-GNUS-OPTIONS-DID-NOT-EXIST` (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** `grep` collected its command line into `Vec<String>`, so a search
whose pattern or file name held a byte that is not valid UTF-8 -- a legal file
name on this OS -- killed the program before it read anything. It also silently
did not have eleven of GNU grep's options, answering each with `unknown option`.
Both are fixed: argv is `OsString`/`&[u8]` end to end, the option table is all
fifty of GNU's long options, and the differential harness went from 467 cases to
498 with none differing.

**Where it was.** `userspace/coreutils/src/bin/grep.rs`, listed in
`scripts/argv-utf8-baseline.txt` as `grep.rs:argv-as-string`. It is now out of
that file; `python scripts/argv-utf8.py --check` prints **10 findings across 9
files** (`diff ed fetch logger more patch ps sh tar`, `sh` carrying two).

### The bytes half

`main` did `env::args_os().map(|a| a.to_string_lossy().into_owned())`, and the
hand-written parser worked in `&str` from there. Three consequences, in
increasing order of how quietly they failed:

1. A non-UTF-8 **operand** became U+FFFD, so `grep pat $'f\xffle'` opened the
   wrong path or none, reporting `No such file or directory` about a file that
   exists.
2. A non-UTF-8 **pattern** was mangled the same way, so the search was not the
   search that was asked for -- and it *succeeded*, which is worse than failing.
3. `search_found` -- the recursive walk's per-file entry point -- called
   `to_string_lossy` on every path it descended into, so the corruption applied
   to files the user never named. It now works on `quote::os_bytes` and rebuilds
   the `OsString` with `quote::os_from_bytes`.

`Prefix::filename` and `Source::filename` changed `&'a str` -> `&'a [u8]`,
which is what carries the name all the way to the write, colouring included.

### The options half

Eleven options were declared to `getopt` for the first time, which is not the
same as implementing them -- each was checked against GNU grep 3.11 and does
what upstream does:

| Option | What it now does |
|---|---|
| `-P`, `--perl-regexp` | prints upstream's own `Perl matching not supported in a --disable-perl-regexp build` and exits 2 -- the right answer for a build without PCRE, rather than `unknown option` |
| `-X`, `--matcher=NAME` | long-hand for `-G`/`-E`/`-F`; `grep`, `egrep`, `fgrep`, `awk`, `gawk`, `posixawk`, `perl` |
| `-I` | `--binary-files=without-match` (see the gap below) |
| `--binary-files=TYPE` | `binary`/`text`/`without-match`, matched with `STREQ` -- **exact**, where `-d` uses argmatch's prefix matching. The asymmetry is upstream's and is now pinned by a case |
| `-U`, `--binary` | accepted and ignored; it is a no-op off MS-DOS |
| `-u`, `--unix-byte-offsets` | warns `grep: warning: --unix-byte-offsets (-u) is obsolete` on stderr, in argv order, and continues |
| `-V`, `--version` | prints a version and exits 0 |
| `--help` | GNU 3.11's help text on stdout, exit 0 |
| `--label=LABEL` | renames standard input only; a named file keeps its name |
| `--line-buffered` | flushes at GNU's three sites: end of `prline` (so `-o` flushes once per *line*, after the last of that line's matches), after the `-c` count, and after the `-l`/`-L` name |
| `--no-ignore-case` | undoes an `-i` that came earlier on the same line |

### What the conversion fixed that was not the point of it

Four cases the harness had recorded as expected-to-differ turned green on their
own, because `coreutils::getopt` produces glibc's and gnulib's diagnostics
rather than our own wording. They were `grep` with no operand, `grep --zzz`,
`grep -m x` and `grep -d bogus`. The last is the interesting one: it exits **1**,
not 2, because gnulib's argmatch dies on its own rather than through grep's
`usage(EXIT_TROUBLE)` -- which is why `Program::argmatch` forces status 1
regardless of the program's usage status.

That grep has **three** error shapes and not one was measured, not read:

| command | output | status |
|---|---|---|
| `grep` | usage summary + `Try ...`, **no `grep: ` line at all** | 2 |
| `grep --zzz` / `grep -Q` / `grep -m` | `grep: <glibc sentence>` + usage + `Try ...` | 2 |
| `grep -d bogus` | argmatch's five lines + usage + `Try ...` | **1** |
| `grep -D bogus` / `-X bogus` / `-m x` / `-A x` / `--binary-files=bogus` / `-E -F` | `grep: <one line>` and nothing else | 2 |

`run_main` reproduces the split with `if e.referral.is_some()`, which is exactly
where `getopt` already draws the line between a `sentence()`-built error and a
`usage()` one. An earlier draft printed the usage summary under *every* error;
the probe is what caught it.

A wrong test was also removed. `parse_empty_errors` asserted the message
`missing PATTERN` -- a string GNU grep has never printed. A bare `grep` is
`usage(EXIT_TROUBLE)` with no diagnostic.

### Gates

- `python scripts/getopt-ambiguity-check.py grep` -> 1 table checked, **0
  disagreements**. grep was previously outside this check entirely, having no
  declared table to check. The 50 long options are kept in GNU's declaration
  order because getopt lists ambiguous candidates in that order, and `grep --l`
  is ambiguous.
- `bash scripts/grep-diff.sh` (in WSL) -> **498 passed, 0 differed, 12 on
  purpose, 0 unexpectedly agreed**, up from 467/0/5/4.
- 108 unit tests pass; `cargo clippy` clean under the crate's
  `deny(clippy::all, clippy::pedantic)`.

### The gap this leaves: `TD-COREUTILS-GREP-NEVER-DETECTS-BINARY-CONTENT` -- **FIXED 2026-08-29**, except for the encoding-error half

*(The section below is the gap as it stood when the conversion landed. The NUL
half of it is now implemented; see* `B-GREP-NEVER-DETECTED-BINARY-CONTENT`
*further down, which also records what was left behind and why.)*

`--binary-files` is one setting with three behaviours and we implement one.
`text` -- print the bytes -- is what we always do, so it agrees with GNU. The
other two need content detection we do not have:

- `binary` (**the default**): GNU writes **nothing to stdout** and prints
  `grep: F: binary file matches` on **stderr**, exiting 0. We print the lines.
- `without-match` / `-I`: GNU treats the file as not matching at all -- nothing
  on either stream, exit 1. We search it normally.

The stream and the wording were measured against the reference host's 3.11
(`grep ary bf > o 2> e`, then `od -c` on both), not read out of the source, and
the measurement contradicts the description this entry first carried. Older grep
printed `Binary file F matches` on **stdout**, and most second-hand accounts of
the feature still say so; 3.x moved it to stderr and lower-cased it. Note also
that command substitution loses it -- `$(grep ary bf 2>&1 >/dev/null)` came back
empty in the first probe, which is what sent the first draft to the wrong
stream. `-c` and `-l` are unaffected: they print `1` and the file name as usual,
because what is suppressed is the *lines*, not the file.

The proper fix is upstream's rule, not a heuristic of our own: a file is binary
if the first buffer holds a NUL, or holds an encoding error in the current
locale. `-a` / `--binary-files=text` bypasses the check, and `-z` changes it
(NUL is the line terminator then, so it cannot be the marker). Four cases in
`scripts/grep-diff.sh` under `# --- binary ---` are marked `!` with this reason
and should all turn XPASS together when it lands.
