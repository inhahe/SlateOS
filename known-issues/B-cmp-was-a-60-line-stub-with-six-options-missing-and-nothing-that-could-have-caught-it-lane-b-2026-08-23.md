## `cmp` was a 60-line stub with six options missing, and nothing that could have caught it (lane B, 2026-08-23)

**In short:** `cmp` answers "are these two files identical, and if not, where do
they first differ?". Scripts use its exit status (0 same, 1 different, 2
trouble) and its one line of output. The shipped one accepted two file names
and nothing else: `-s`, the option that makes it *quiet* and is the reason most
scripts call it at all, was not implemented, so `if cmp -s a b; then` treated
`-s` as a missing file and took the "different" branch every time — for two
identical files. It also panicked outright on a file name that is not valid
UTF-8, which this OS allows. Rewritten as a port of GNU diffutils 3.10's
`src/cmp.c`. Separately, and worth as much: it now has an end-to-end
differential test, and building that test found three further defects that
thirty-three green unit tests had not.

### Why nothing caught this

The build host is Windows. A binary whose body is `#[cfg(unix)] mod imp` —
`cmp` opens `-` as descriptor 0, stats for `st_dev`/`st_ino` to recognise two
names for one file, and seeks to honour `-i` — compiles on the host to a stub
that prints `unix-only utility` and exits 2. So `cargo test` can reach the
parser and the formatter, and *nothing else*: not `open`, not `skip`, not the
same-file shortcut, not the comparison loop's interaction with real files. Most
`scripts/*-diff.sh` harnesses build a native Windows subject and reach into WSL
only for the GNU reference, which for this shape of binary would run the stub
141 times and report a uniform disagreement that says nothing about the
program.

The answer already existed for three other binaries: `du-diff.sh`,
`find-diff.sh` and `ls-diff.sh` build their subject *for Linux inside WSL* and
compare it against GNU there (`design-decisions.md` §365).
`scripts/cmp-diff.sh` is the fourth, and it goes one step further in running
*both* sides inside WSL. 141 cases, 9 recorded as differing on purpose.
`OURS=/usr/bin/cmp scripts/cmp-diff.sh` turns exactly those 9 into XPASS and
nothing else, which is what shows the harness discriminates. It needs a Rust
toolchain under the WSL user's `$HOME` and skips with a message and exit 0 if
it is absent, exactly as its siblings skip when glibc is unreachable.

**Four of the eight unix-only binaries here still have no such harness** —
`chmod`, `chown`, `id`, `stat`. Several entries above end with some form of
"not verified end-to-end; needs QEMU", and that conclusion is wrong: they do
not need QEMU, they need one script each. `cmp` is the evidence — 33 passing
unit tests and clippy-clean on two targets, and the harness found three real
defects on its first run. Promoted from `du`'s special case to the standing
answer in `design-decisions.md` §374, which also merges the four build caches
these scripts had accumulated into one.

### The twelve defects the rewrite fixed

1. **`-s`/`--silent`/`--quiet` did not exist.** The commonest way to call
   `cmp`. Every `cmp -s` in every script silently took the wrong branch.
2. **`-l`/`--verbose` did not exist** — list every differing byte, the other
   reason to call it.
3. **`-i`/`--ignore-initial` and the positional `SKIP1 SKIP2` operands did not
   exist.**
4. **`-n`/`--bytes` did not exist.**
5. **`-b`/`--print-bytes` did not exist.**
6. **`--help` and `--version` did not exist.**
7. **A non-UTF-8 file name panicked.** `let args: Vec<String> = env::args()`
   aborts on the first byte that is not UTF-8; `design.txt` permits every byte
   but `/` and NUL in a name. This is the defect the `scripts/argv-utf8.py`
   sweep exists to find, and it is why the file was opened.
8. **The result line said `char` unconditionally.** GNU picks `byte` or `char`
   from `hard_locale (LC_MESSAGES)`: `char` under `C`/`POSIX`, `byte`
   everywhere else. Anything grepping the line got the wrong word under a real
   locale.
9. **The EOF note did not distinguish ending *on* a newline from ending inside
   a line.** GNU has four wordings — `which is empty`, `after byte N`,
   `after byte N, line M`, `after byte N, in line M+1` — and the last two are a
   real distinction the old code could not express.
10. **`-` did not mean standard input**, so `cmp - a` looked for a file called
    `-`.
11. **Two names for one file were compared byte by byte.** GNU compares
    `st_dev`/`st_ino` and answers immediately; over a disk image that is the
    difference between free and minutes.
12. **A broken pipe panicked.** `cmp -l a b | head -1` is an ordinary end to a
    pipeline; it now exits 1, which is the answer the rows already proved.

### The three the harness found and the unit tests could not

13. **The referral line was missing its `cmp: ` prefix.** GNU coreutils'
    `usage()` writes `Try 'ls --help' …` with a bare `fprintf`, so it is
    unprefixed; diffutils' `try_help` makes a *second* `error()` call and
    `error()` always prefixes, so `cmp` prints `cmp: Try 'cmp --help' …` on the
    second line. `getopt::Error`'s `Display` models the coreutils shape, which
    every other utility here wants, so `cmp` now assembles its own.
14. **`-s` did not silence a *permission* error.** The code special-cased "file
    not found"; upstream silences whatever the open failed with. Measured:
    `cmp -s noperm a` prints nothing and exits 2, exactly as `cmp -s nosuch a`
    does. Under `-s` the status is the whole answer.
15. **A skip larger than the filesystem's maximum file size failed the run.**
    `-i` accepts anything up to `OFF_T_MAX`, but Linux rejects an `lseek` past
    that maximum outright, so `cmp -i 9223372036854775807 a b` — two small
    files, a skip that lands both at EOF and therefore makes them equal — died
    with `Invalid argument` and exit 2 where GNU exits 0. `lseek` leaves the
    offset untouched when it fails, so falling through to a read-and-discard
    loop starts from the right place.

### The sixteenth, found while writing the harness's own cases

16. **`-i` and the positional `SKIP1`/`SKIP2` operands were treated as
    assignments; upstream makes them a maximum.** All four spellings funnel
    through one function, `specify_ignore_initial`, whose last line is
    `if (ignore_initial[f] < val) ignore_initial[f] = val;`. So
    `cmp -i 5 a b 0 0` still skips 5, and `cmp -i 5:6 a b 7 2` skips 7 and 6 —
    each slot independently keeping the larger of the two numbers naming it.
    Measured with `cmp -l`, whose first row names the two bytes actually
    reached and so reads back both skips at once: against `a=ABCDEFGHIJ`,
    `b=abcdefghij`, `cmp -l -i 5 a b 0 0` prints `1 106 146` (a[5], b[5]) where
    an override would have printed `1 101 141`. Silent when wrong: the run
    still succeeds, it just compares the wrong bytes and numbers them from the
    wrong origin. A lone positional `SKIP1` is the one asymmetry — unlike a
    lone `-i`, it says nothing about the second file, so `cmp a b 5` skips 5
    bytes of `a` and none of `b`.

### The seventeenth, found by the pre-push gate

17. **`LONG_OPTIONS` listed `--help` before `--version`; diffutils declares them
    the other way round.** Every coreutils bin in this tree ends its table with
    `GETOPT_HELP_OPTION_DECL` then `GETOPT_VERSION_OPTION_DECL`, and I carried
    that habit into a diffutils program, which writes both entries out by hand
    in the opposite order. The order is not cosmetic: glibc reports `pfound` —
    the first table entry an ambiguous prefix matched — and lists the remaining
    matches after it, so two tables holding the same names in different orders
    name a different option in their diagnostics.

    `scripts/getopt-ambiguity-check.py` caught it at `git push`, which is
    exactly the case it was written for: the prefixes it sweeps are generated
    from *our* names, so a same-names-different-order table survives the sweep
    untouched and only the sequence comparison can see it. Nothing in 138
    differential cases could either — no prefix shorter than the empty one
    matches both `--version` and `--help`. The harness now runs `cmp --=x`,
    whose empty prefix matches every entry and so prints the whole table in
    declaration order, plus `--p` and `--v` for the two ambiguous prefixes that
    do occur; that is the same one-command readout the check script uses, and
    it belongs in any future bin's harness for the same reason.

### Deliberate divergences, all recorded as `xfail` in the harness

* `--help` omits the GNU project's `Report bugs to:` block; `--version` names
  SlateOS.
* **File names in the output are quoted and escaped; GNU emits the raw bytes.**
  Under GNU, `cmp 'sp ace' 'nl<newline>name'` prints something that reads as
  two result lines, and a name that is not text lands on the terminal as-is.
  See `design-decisions.md` §373.
* A rejected `-i`/`-n` value is escaped inside its quotes, for the same reason.
* Under `-l` we flush stdout before writing the `EOF on …` note to stderr, so
  the note follows the rows it summarises. GNU's order is whatever its
  buffering produces — rows first on a terminal, note first into a pipe — so
  nothing can depend on it. Only observable with the two streams merged, which
  is what the harness's `xfail_merged` does and its ordinary cases cannot.

### One thing the harness deliberately does not test

`LC_ALL=en_US.UTF-8` decides `byte` versus `char`, and the case is **skipped
unless the locale is actually generated**. Rust has no `setlocale`, so we read
`LC_ALL`/`LC_MESSAGES`/`LANG` and compare against `C`/`POSIX`; GNU calls
`setlocale`, which *fails* when the named locale was never generated and leaves
the process in `C`. GNU then says `char` where we say `byte`. That is a
property of the system, not a defect in either program: generating the locale
(`localedef -i en_US -f UTF-8 ~/locales/en_US.UTF-8`, `LOCPATH=~/locales`, no
root needed) makes the two agree exactly. Worth knowing because the same
approximation is in every utility here that consults `hard_locale`.
