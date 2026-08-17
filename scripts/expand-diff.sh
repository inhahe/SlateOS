#!/usr/bin/env bash
# Differential test: our expand against GNU expand.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `expand` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `head-diff.sh`, `wc-diff.sh`, `cut-diff.sh`, `uniq-diff.sh`
# and `nl-diff.sh`.
#
# Run `OURS=/usr/bin/expand ./scripts/expand-diff.sh` to confirm the harness
# still discriminates: it should report dozens of differences, not zero.
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
# ## Two cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's.
#
# The locale is `C.UTF-8`, with the diagnostics that pass an argument through
# gnulib's `quote()` referenced under `LC_ALL=C` so the quote marks are ASCII on
# both sides — which is every tab-stop diagnostic and every getopt one. See
# `open-questions.md` → B-Q2.
set -u

# Our expand is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/expand.exe"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# Every invocation is bounded. `expand` is one of the few utilities that can be
# asked, in perfectly valid syntax, to produce more output than the universe has
# room for — `-t 18446744073709551615` turns one tab into 2**64-1 spaces — and a
# case that does so does not fail, it wedges the run and fills the disk. No case
# here should ever come near this, so a timeout that fires is itself a bug
# report: it shows up as a status difference rather than as a hung harness.
run_ours() { timeout -k 2 30 "$OURS_ABS" "$@"; }
run_gnu()  { local loc=$1; shift; timeout -k 2 30 wsl -e env "LC_ALL=$loc" expand "$@"; }

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands on
# the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'a\tb\n' > .probe
if [ "$(run_gnu C.UTF-8 -t2 .probe 2>/dev/null)" = "$(printf 'a b')" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "expand-diff: glibc expand not reachable in this directory; skipping"
fi
rm -f .probe

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
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(expand | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_gnu "$loc" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_gnu "$loc" "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C.UTF-8 "$@"; report "expand $*"; }
# The diagnostics that pass an argument through gnulib's `quote()`. Under a
# UTF-8 locale gnulib switches to U+2018/U+2019 and ours does not — one open
# question, not one per case. See B-Q2. Every diagnostic `expand` can print
# quotes something, so every error case here is `run_ascii`.
run_ascii() { [ "$HAVE_GNU" = yes ] || return 0; compare - C "$@"; report "expand $* [C]"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" C.UTF-8 "$@"
  report "printf '$input' | expand $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1"; shift
  compare - C.UTF-8 "$@"
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
run_ascii nosuch.txt
run_ascii plain.txt nosuch.txt
run_ascii nosuch.txt plain.txt
run_ascii nosuch.txt nosuch2.txt
run_ascii -t3 plain.txt nosuch.txt plain.txt

# --- tab-stop diagnostics ----------------------------------------------------
run_ascii -t 0 plain.txt
run_ascii -t0 plain.txt
run_ascii -0 plain.txt
run_ascii -t 0,4 plain.txt
run_ascii -t 4,0 plain.txt
run_ascii -t 4,4 plain.txt
run_ascii -t 4,2 plain.txt
run_ascii -t 4 -t 2 plain.txt
run_ascii -t 4 -t 4 plain.txt
run_ascii -t x plain.txt
run_ascii -t oops plain.txt
run_ascii -t 4,5x plain.txt
run_ascii -t 1x2 plain.txt
run_ascii -t '4;5' plain.txt
run_ascii -t -- plain.txt
run_ascii -t=4 plain.txt
run_ascii --tabs=x plain.txt
# `/` and `+` misplaced. The parse continues after one of these, so `1/2/3`
# reports twice — and the message quotes the *rest* of the argument, not the
# character.
run_ascii -t 1/2 plain.txt
run_ascii -t 1/2/3 plain.txt
run_ascii -t 1+2 plain.txt
run_ascii -t 1+2+3 plain.txt
run_ascii -t 1/2+3 plain.txt
# Two prefixes of the same kind in one list, which is the "only allowed with the
# last value" pair.
run_ascii -t /2,/4 plain.txt
run_ascii -t +2,+4 plain.txt
run_ascii -t /2 -t /4 plain.txt
run_ascii -t +2 -t +4 plain.txt
# One of each, which is caught at the end rather than during the parse.
run_ascii -t /2,+4 plain.txt
run_ascii -t +2,/4 plain.txt
run_ascii -t 1,/2,+4 plain.txt
# Overflow. The whole digit run is named, including the digits that had already
# been accumulated, and the rest of it is skipped so one number gives one
# message.
run_ascii -t 99999999999999999999999 plain.txt
run_ascii -t 18446744073709551616 plain.txt
# The largest *accepted* value, reached only through a case that fails before
# any conversion happens. A bare `-t 18446744073709551615` is valid, and both
# implementations honour it: one tab becomes 2**64-1 spaces, written at about
# 11 MB/s forever. An earlier revision of this file had exactly that case and
# it wedged the run. Pairing it with a descending stop makes `finalize` refuse
# the list, which is the only part of it worth testing.
run_ascii -t 18446744073709551615,4 plain.txt
run_ascii -t 99999999999999999999999,4 plain.txt
run_ascii -t 4,99999999999999999999999x plain.txt
run_ascii -99999999999999999999999 plain.txt

# --- getopt diagnostics ------------------------------------------------------
run_ascii -q plain.txt
run_ascii -iq plain.txt
run_ascii -qi plain.txt
run_ascii -t plain.txt
run_ascii -it
run_ascii --tabs
run_ascii --zz plain.txt
run_ascii --initial=4 plain.txt
run_ascii --help=x plain.txt
run_ascii --version=x plain.txt
run_ascii --=x plain.txt
run_ascii -- -q
# A tab-stop diagnostic exits where it is found, so which of two errors gets
# reported depends only on argv order.
run_ascii -t x -q plain.txt
run_ascii -q -t x plain.txt
run_ascii -t 0 -q plain.txt
run_ascii -q -t 0 plain.txt
# Abbreviations, which the shipped parser did not accept at all.
run_case --in plain.txt
run_case --ta=3 plain.txt
run_case --tab=3 plain.txt
run_case --i plain.txt
run_ascii --t=3 plain.txt
run_ascii --ini=3 plain.txt

# --- operands and options interleave -----------------------------------------
run_case plain.txt -t3
run_case plain.txt -i leading.txt
run_case -t3 plain.txt -i
run_ascii -- -t3
run_ascii plain.txt -- -t3

# --- differ on purpose -------------------------------------------------------
# On SlateOS, and on any POSIX host, opening a directory succeeds and the *read*
# fails, so GNU says `.: Is a directory` and so do we. This harness runs a
# Windows build, where `File::open` of a directory fails outright and the errno
# is the host's. The difference is the host's, not the code's — see the same
# trap in `cut-diff.sh` and in `filekind.rs`, where a Windows `is_file()` calls
# a pipe a regular file.
xfail_case 'a directory operand cannot be opened on a Windows host' .

xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1
