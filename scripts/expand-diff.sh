#!/usr/bin/env bash
# Differential test: our expand against GNU expand.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the reasons. The reference has to be glibc's: the
# host's `expand` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`), so a harness pointed
# at it certifies sentences no GNU/Linux system prints (`known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`). This file already
# avoided that by reaching for `wsl -e env LC_ALL=C.UTF-8 expand`, at the cost
# of a WSL process per case and a probe to check that `wsl`'s inherited Windows
# cwd landed on the same bytes under `/mnt/...`.
#
# The subject moving with it is the part that changed an answer — see the
# directory-operand case at the foot of this file, which was an expected
# difference only because a Windows `File::open` refuses a directory outright.
#
# Run `OURS=/usr/bin/expand ./scripts/expand-diff.sh` to confirm the harness
# still discriminates: every expected difference should turn into an XPASS.
#
# ## Why `od -An -c`
#
# `expand`'s entire output is whitespace placement. A comparison that collapsed
# or trimmed whitespace would agree with almost every wrong implementation — a
# tab left unconverted and a tab converted to the wrong number of spaces both
# survive it — so stdout goes through `od -An -c` byte for byte. That also
# catches the two things the old implementation got wrong invisibly: a stripped
# CR and an appended final newline.
#
# The corollary is that no case may use a large tab size. `expand -t 2000000000`
# is accepted and really does emit two gigabytes of spaces for one tab; the
# bounds are tested through the *diagnostics* instead.
#
# ## The two cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's.
#
# The locale is `C.UTF-8` throughout, including for the diagnostics that pass
# an argument through gnulib's `quote()`. Those used to be referenced under
# `LC_ALL=C`, because that was the only locale in which GNU's quote marks were
# ASCII like ours; §351 made ours U+2018/U+2019 in every locale, which is what
# GNU prints under any UTF-8 locale, so `C` is now the setting in which the
# reference would be wrong.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `expand` so `argv[0]` matches. See `scripts/diff-wsl.sh`. `timeout` is
# named because every invocation below is bounded with it; see `run_side`.
DIFF_PROG=expand
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# One invocation of one side. `$1` is `ours` or `gnu`; each is reached through
# a symlink named `expand` in a directory that is the whole of `PATH` for that
# one invocation, so `argv[0]` is the bare word on both sides.
#
# Every invocation is bounded. `expand` is one of the few utilities that can be
# asked, in perfectly valid syntax, to produce more output than the universe has
# room for — `-t 18446744073709551615` turns one tab into 2**64-1 spaces — and a
# case that does so does not fail, it wedges the run and fills the disk. No case
# here should ever come near this, so a timeout that fires is itself a bug
# report: it shows up as a status difference rather than as a hung harness.
# `diff_run` keeps bash's own announcement of a child that died of a signal
# out of the stderr the caller captures; `diff-wsl.sh` says why.
run_side() { local side=$1; shift; diff_run timeout -k 2 30 env PATH="$bindir/$side" expand "$@"; }

# --- fixtures ----------------------------------------------------------------
printf 'a\tb\tc\n'                        > plain.txt
printf '\ta\n\t\tb\n'                     > leading.txt
printf '\n\t\n\n'                         > blanks.txt
printf ''                                 > empty.txt
printf '12345678\tX\n1234567\tY\n'        > aligned.txt
# Every column of the first tab stop, so an off-by-one anywhere in the gap
# arithmetic shows up.
printf '\tz\na\tz\nab\tz\nabc\tz\nabcd\tz\nabcde\tz\nabcdef\tz\nabcdefg\tz\nabcdefgh\tz\n' > ramp.txt
# Runs of tabs, which exercise the stop list running out.
printf 'a\t\t\t\tb\n\t\t\t\t\t\t\n'       > runs.txt
# Blanks after a non-blank, which only `-i` treats differently.
printf ' \t x\ty\n\t\tp\tq\n'             > mixed.txt
# Backspaces: before a tab, at the start of a line, and more than there are
# columns to give back.
printf 'ab\b\tc\n\b\b\tx\nabcdefgh\b\b\tz\n' > back.txt
# A line with no terminator, which must not gain one.
printf 'a\tb'                             > unterm.txt
# Two halves of one line, to prove the operands are a single stream.
printf 'x\t'                              > half1.txt
printf 'y\tz\n'                           > half2.txt
# Bytes that are not text at all, and CRLF, which must survive verbatim. The CR
# also occupies a column, which is why `\r\tx` is not `\tx`.
printf 'a\xff\tb\n\xfe\xfd\tc\n'          > badbytes.txt
printf 'a\r\n\tb\r\n'                     > crlf.txt
# Multibyte text: `é` is two bytes and therefore two columns, and a wide CJK
# character is three.
printf '\xc3\xa9\tx\n\xe4\xb8\xad\ty\n'   > utf8.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(expand | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_side ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_side gnu  "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of the
  # getopt module is that the sentences match, so a harness that only asked "did
  # it complain?" would pass on every wording this exists to fix.
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_err" "$g_err"
}

report() {
  local label="$1"; shift
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case()  { compare - "$@"; report "expand $*"; }
run_stdin() {
  local input="$1"; shift
  compare "$input" "$@"
  report "printf '$input' | expand $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  local why="$1"; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS expand %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL expand %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default, which is a stop every eight columns ------------------------
# The shipped parser read `-t8` and `--tabs=8` as filenames, so several of these
# fail against it before any list grammar is involved.
for f in plain.txt leading.txt blanks.txt empty.txt aligned.txt ramp.txt runs.txt mixed.txt; do
  run_case "$f"
done

# --- the five spellings of a tab size -----------------------------------------
# The separated long form is here because it was missing, and its absence
# certified a parser that rejected `--tabs 4` outright: a long option with a
# *required* argument takes the next word when there is no `=`, exactly as the
# short form does. Every harness for a utility with such an option needs this
# row, not only the `=` one.
for size in 1 2 3 4 7 8 9 16; do
  run_case -t$size ramp.txt
  run_case -t "$size" ramp.txt
  run_case --tabs=$size ramp.txt
  run_case --tabs "$size" ramp.txt
done
# A separated argument must be consumed, not left an operand, and the option
# may still be abbreviated when it is written that way.
run_case --tabs 4 ramp.txt plain.txt
run_case --tab 4 ramp.txt

# The obsolete form, which is a short option with an optional argument and so is
# not restricted to the first argument the way `head -5` is.
for size in 1 2 3 4 7 8 9 16; do
  run_case -$size ramp.txt
done
run_case ramp.txt -4
run_case -4 ramp.txt --
run_case -1 runs.txt

# --- explicit lists ----------------------------------------------------------
run_case -t 1,3,5 ramp.txt
run_case -t 2,4,6,8 ramp.txt
run_case -t 3,6 ramp.txt
run_case -t 1 ramp.txt
run_case -t 4 ramp.txt
# Blanks separate exactly as commas do, and an empty entry vanishes.
run_case -t '1 3 5' ramp.txt
run_case -t '1,3 5' ramp.txt
run_case -t '1,,3' ramp.txt
run_case -t ',,1,,3,,' ramp.txt
run_case -t ' ' ramp.txt
run_case -t '' ramp.txt
run_case -t ',' ramp.txt
# The obsolete form carries a whole list, because upstream recovers it with
# `parse_tab_stops (optarg - 1)`.
run_case -1,3 ramp.txt
run_case -1,3,5 ramp.txt
run_case -2,4 runs.txt
# Every occurrence appends rather than replacing.
run_case -t1 -t3 ramp.txt
run_case -t 1 -t 3 -t 5 ramp.txt
run_case -t1 --tabs=3 -5 ramp.txt
run_case -1 -t 3 ramp.txt

# --- the two prefixes --------------------------------------------------------
run_case -t 2,/4 ramp.txt
run_case -t 2,+4 ramp.txt
run_case -t 1,3,/5 ramp.txt
run_case -t 1,3,+5 ramp.txt
run_case -t 2,/4 runs.txt
run_case -t 2,+4 runs.txt
run_case -t '2 /4' ramp.txt
run_case -t2 -t/4 ramp.txt
run_case -t2 -t+4 ramp.txt
# A prefix with no explicit stop is exactly the plain size.
run_case -t /4 ramp.txt
run_case -t +4 ramp.txt
# A zero-valued prefix is *no prefix*, so these are the default 8.
run_case -t /0 ramp.txt
run_case -t +0 ramp.txt
# And a prefix with no number at all stores nothing.
run_case -t / ramp.txt
run_case -t + ramp.txt
run_case -t '/,+' ramp.txt
# The prefix applies to the entry it precedes, not to the argument, so a later
# unprefixed entry is an ordinary stop.
run_case -t '/4,6' ramp.txt
run_case -t '+4,6' ramp.txt

# --- -i, which stops at the first non-blank ----------------------------------
for f in plain.txt leading.txt mixed.txt runs.txt back.txt ramp.txt; do
  run_case -i "$f"
  run_case --initial "$f"
  run_case -i -t3 "$f"
  run_case -it3 "$f"
done
run_case -i -t 2,/4 mixed.txt
# The region reopens at every newline.
run_stdin 'x\ty\n\tz\n' -i
run_stdin ' \t \tx\ty\n' -i
run_stdin '\t \t\n' -i
# A backspace is not blank, so it closes the region even though nothing visible
# has been printed.
run_stdin '\b\tx\n' -i
run_stdin ' \b\tx\n' -i

# --- backspaces --------------------------------------------------------------
run_case back.txt
run_case -t4 back.txt
run_case -t 2,4 back.txt
run_case -t 2,/4 back.txt
run_case -t 2,+4 back.txt
run_stdin 'abc\b\tx\n' -t 2,4
run_stdin 'abcdefghij\b\b\b\tx\n'
run_stdin '\b\b\b\tx\n'
run_stdin 'a\b\b\b\b\tx\n' -t3

# --- the operands are one stream ---------------------------------------------
# `half1.txt` has no newline, so its line continues into `half2.txt` and the
# column count carries across the join.
run_case half1.txt half2.txt
run_case -t3 half1.txt half2.txt
run_case unterm.txt unterm.txt
run_case plain.txt plain.txt plain.txt
run_case -i half1.txt half2.txt
# `-` is standard input and is an operand, not an option.
run_stdin 'q\tr\n' -
run_stdin 'q\tr\n' -t3 -
run_case empty.txt plain.txt
run_case plain.txt empty.txt

# --- nothing is added to a line that had no newline --------------------------
run_case unterm.txt
run_stdin 'a\tb'
run_stdin '\t'
run_stdin ''
run_stdin 'a\tb\n'

# --- bytes, not characters ---------------------------------------------------
run_case badbytes.txt
run_case crlf.txt
run_case utf8.txt
run_case -t3 utf8.txt
run_stdin 'a\xff\tb\n'

# --- operands that cannot be opened ------------------------------------------
# The run continues and the status is 1 at the end, so the good file is still
# converted.
run_case nosuch.txt
run_case plain.txt nosuch.txt
run_case nosuch.txt plain.txt
run_case nosuch.txt nosuch2.txt
run_case -t3 plain.txt nosuch.txt plain.txt

# --- tab-stop diagnostics ----------------------------------------------------
run_case -t 0 plain.txt
run_case -t0 plain.txt
run_case -0 plain.txt
run_case -t 0,4 plain.txt
run_case -t 4,0 plain.txt
run_case -t 4,4 plain.txt
run_case -t 4,2 plain.txt
run_case -t 4 -t 2 plain.txt
run_case -t 4 -t 4 plain.txt
run_case -t x plain.txt
run_case -t oops plain.txt
run_case -t 4,5x plain.txt
run_case -t 1x2 plain.txt
run_case -t '4;5' plain.txt
run_case -t -- plain.txt
run_case -t=4 plain.txt
run_case --tabs=x plain.txt
# `/` and `+` misplaced. The parse continues after one of these, so `1/2/3`
# reports twice — and the message quotes the *rest* of the argument, not the
# character.
run_case -t 1/2 plain.txt
run_case -t 1/2/3 plain.txt
run_case -t 1+2 plain.txt
run_case -t 1+2+3 plain.txt
run_case -t 1/2+3 plain.txt
# Two prefixes of the same kind in one list, which is the "only allowed with the
# last value" pair.
run_case -t /2,/4 plain.txt
run_case -t +2,+4 plain.txt
run_case -t /2 -t /4 plain.txt
run_case -t +2 -t +4 plain.txt
# One of each, which is caught at the end rather than during the parse.
run_case -t /2,+4 plain.txt
run_case -t +2,/4 plain.txt
run_case -t 1,/2,+4 plain.txt
# Overflow. The whole digit run is named, including the digits that had already
# been accumulated, and the rest of it is skipped so one number gives one
# message.
run_case -t 99999999999999999999999 plain.txt
run_case -t 18446744073709551616 plain.txt
# The largest *accepted* value, reached only through a case that fails before
# any conversion happens. A bare `-t 18446744073709551615` is valid, and both
# implementations honour it: one tab becomes 2**64-1 spaces, written at about
# 11 MB/s forever. An earlier revision of this file had exactly that case and
# it wedged the run. Pairing it with a descending stop makes `finalize` refuse
# the list, which is the only part of it worth testing.
run_case -t 18446744073709551615,4 plain.txt
run_case -t 99999999999999999999999,4 plain.txt
run_case -t 4,99999999999999999999999x plain.txt
run_case -99999999999999999999999 plain.txt

# --- getopt diagnostics ------------------------------------------------------
run_case -q plain.txt
run_case -iq plain.txt
run_case -qi plain.txt
run_case -t plain.txt
run_case -it
run_case --tabs
run_case --zz plain.txt
run_case --initial=4 plain.txt
run_case --help=x plain.txt
run_case --version=x plain.txt
run_case --=x plain.txt
run_case -- -q
# A tab-stop diagnostic exits where it is found, so which of two errors gets
# reported depends only on argv order.
run_case -t x -q plain.txt
run_case -q -t x plain.txt
run_case -t 0 -q plain.txt
run_case -q -t 0 plain.txt
# Abbreviations, which the shipped parser did not accept at all.
run_case --in plain.txt
run_case --ta=3 plain.txt
run_case --tab=3 plain.txt
run_case --i plain.txt
run_case --t=3 plain.txt
run_case --ini=3 plain.txt

# --- operands and options interleave -----------------------------------------
run_case plain.txt -t3
run_case plain.txt -i leading.txt
run_case -t3 plain.txt -i
run_case -- -t3
run_case plain.txt -- -t3

# A directory operand, which used to be an expected difference and is not one
# any more: opening a directory succeeds on POSIX and the *read* fails, so GNU
# says `.: Is a directory` and so do we. It differed only while the subject was
# a Windows build, where `File::open` refuses a directory outright and the
# errno was the host's — see the same trap in `filekind.rs`, where a Windows
# `is_file()` calls a pipe a regular file. Moving both sides into WSL deleted
# it, as it did the identical case in `cut-diff.sh` and `fold-diff.sh`.
run_case .

# --- differ on purpose -------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
