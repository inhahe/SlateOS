#!/usr/bin/env bash
# Differential test: our unexpand against GNU unexpand.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `unexpand` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `head-diff.sh`, `wc-diff.sh`, `cut-diff.sh`, `uniq-diff.sh`,
# `nl-diff.sh` and `expand-diff.sh`.
#
# Run `OURS=/usr/bin/unexpand ./scripts/unexpand-diff.sh` to confirm the harness
# still discriminates: it should report dozens of differences, not zero.
#
# ## Why `od -An -c`
#
# `unexpand`'s entire output is whitespace placement, and the interesting part
# of it is precisely the difference between a tab and a run of spaces — which a
# comparison that collapsed whitespace could not see at all. So stdout goes
# through `od -An -c` byte for byte. That also catches the two things the old
# implementation got wrong invisibly: a stripped CR and an appended final
# newline.
#
# ## Bounded invocations
#
# `unexpand` cannot be made to emit more than it reads — it only ever shortens —
# so it has no runaway case of `expand -t 18446744073709551615`'s kind. The
# timeout is kept anyway, at the same 30 s: it costs nothing, and a case that
# hits it is a deadlock report rather than a filled disk.
#
# ## Three cases that differ on purpose
#
# A directory operand (a Windows-host artefact, see the note at the bottom),
# `--help` and `--version`, whose text is ours rather than the GNU project's.
#
# The locale is `C.UTF-8` throughout, including for the diagnostics that pass
# an argument through gnulib's `quote()`. Those used to be referenced under
# `LC_ALL=C`, because that was the only locale in which GNU's quote marks were
# ASCII like ours; §351 made ours U+2018/U+2019 in every locale, which is what
# GNU prints under any UTF-8 locale, so `C` is now the setting in which the
# reference would be wrong.
set -u

# Our unexpand is a native Windows binary, so MSYS would rewrite an argument
# that looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

# Built here, from the package named, rather than picked up out of `target/`.
# A harness that only *runs* that path measures whatever was written there
# last, which need not be current and need not even be this crate — see
# `scripts/diff-subject.sh`.
. "$(dirname "$0")/diff-subject.sh"
OURS=$(subject_binary coreutils unexpand "${OURS:-}") || exit 1
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

run_ours() { timeout -k 2 30 "$OURS_ABS" "$@"; }
run_gnu()  { local loc=$1; shift; timeout -k 2 30 wsl -e env "LC_ALL=$loc" unexpand "$@"; }

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands on
# the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf '  a\n' > .probe
if [ "$(run_gnu C.UTF-8 -t2 .probe 2>/dev/null)" = "$(printf '\ta')" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "unexpand-diff: glibc unexpand not reachable in this directory; skipping"
fi
rm -f .probe

# --- fixtures ----------------------------------------------------------------
# Every leading-blank width from 0 to 17, so an off-by-one at either of the
# first two stops shows up.
: > ramp.txt
for n in 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17; do
  printf '%*sz\n' "$n" '' >> ramp.txt
done
# The same widths *after* a non-blank, which only `-a` touches.
: > inner.txt
for n in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16; do
  printf 'a%*sb\n' "$n" '' >> inner.txt
done
# A single blank at each column, which is the `one_blank_before_tab_stop` case:
# one blank is never worth a tab even when it lands exactly on a stop.
: > single.txt
for n in 0 1 2 3 4 5 6 7 8 9 10 11 12; do
  printf '%*s x\n' "$n" '' >> single.txt
done
printf '        text\n\t\tmore\n'          > leading.txt
printf 'a        b        c\n'             > wide.txt
printf '\n \n  \n\t\n'                     > blanks.txt
printf ''                                  > empty.txt
# Tabs already in the input, mixed with blanks — a tab folds the blanks before
# it, which the old implementation never did.
printf 'x \ty\nx  \t y\n\t \tz\n'          > tabs.txt
# Trailing blanks, decided by nothing, so flushed verbatim.
printf 'a   \nb        \n   \n'            > trail.txt
# Backspaces: before a run, at the start of a line, and more than there are
# columns to give back.
printf 'ab\b        c\n\b\b        x\nabcdefgh\b\b        z\n' > back.txt
# A line with no terminator, whose pending blanks must still be flushed.
printf 'a        b'                        > unterm.txt
printf '        '                          > untermblank.txt
# Two halves of one line, to prove the operands are a single stream.
printf 'x   '                              > half1.txt
printf '    y\n'                           > half2.txt
# Bytes that are not text at all, and CRLF, which must survive verbatim.
printf 'a\xff        b\n\xfe\xfd        c\n' > badbytes.txt
printf 'a        \r\n        b\r\n'        > crlf.txt
# Multibyte text: `é` is two bytes and therefore two columns.
printf '\xc3\xa9      x\n\xe4\xb8\xad     y\n' > utf8.txt
# Already-unexpanded input, which must be a fixed point.
printf '\ta\n\t\tb\n'                      > done.txt
# Runs that reach columns 12 and 24, which is what tells the obsolete form's
# two readings apart: `-1 -2` is one stop every twelve and folds these, while
# `-1,2` is stops at 1 and 2 and leaves them alone. On a file whose blanks are
# all near column 8 the two agree, which is how a weak fixture would certify a
# parser that read the digits the wrong way.
printf 'abcdefghij  l\n0123456789ab  n\nabcdefghijkl          x\n' > twelve.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(unexpand | od)` the recorded
  # status is od's, and `PIPESTATUS` is set in the substitution's subshell where
  # it cannot be read. See the same note in cat-diff.sh.
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C.UTF-8 "$@"; report "unexpand $*"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" C.UTF-8 "$@"
  report "printf '$input' | unexpand $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1"; shift
  compare - C.UTF-8 "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS unexpand %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL unexpand %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default, which is leading blanks and a stop every eight columns ------
# The shipped parser read `-t8`, `--tabs=8`, `--all` and `--first-only` as
# filenames, so several of these fail against it before any grammar is involved.
for f in ramp.txt inner.txt single.txt leading.txt wide.txt blanks.txt empty.txt \
         tabs.txt trail.txt done.txt; do
  run_case "$f"
done

# --- -a and --all, which convert every run -----------------------------------
for f in ramp.txt inner.txt single.txt wide.txt tabs.txt trail.txt done.txt; do
  run_case -a "$f"
  run_case --all "$f"
done
# The single blank that lands on a stop, which stays a blank because a tab
# would not be shorter. This is the rule the old implementation approximated
# with `space_count > 1`, and it is not the same rule.
run_stdin 'abcdefg h\n' -a
run_stdin 'abcdef  h\n' -a
run_stdin 'abcde   h\n' -a
run_stdin 'a b c d e f g h i\n' -a
run_stdin 'a  b  c  d  e  f\n' -a

# --- --first-only, which cancels -a from either side -------------------------
run_case --first-only inner.txt
run_case -a --first-only inner.txt
run_case --first-only -a inner.txt
run_case --first-only -t4 inner.txt
run_case -t4 --first-only inner.txt
run_case -a -t4 --first-only inner.txt

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
run_case -a --tabs 4 inner.txt
run_case --tab 4 ramp.txt

# `-t` implies `-a`, which is visible only on a file with interior blanks.
for size in 2 3 4 8; do
  run_case -t$size inner.txt
  run_case -t$size wide.txt
done

# --- the obsolete digit form, which is NOT expand's --------------------------
# Eleven no-argument options accumulating one digit at a time, with `,`
# flushing. So `-1 -2` is a single stop at twelve, and `-1,3` is two stops.
for size in 1 2 3 4 7 8 9; do
  run_case -$size ramp.txt
  run_case -$size inner.txt
done
run_case -12 ramp.txt
run_case -16 ramp.txt
run_case -1 -2 ramp.txt
run_case -1 -6 inner.txt
run_case -1,3 ramp.txt
run_case -1,3,5 ramp.txt
run_case -2,4 inner.txt
# The three spellings that separate an accumulator from a list parser: on
# `twelve.txt`, `-1 -2` and `-12` fold and `-1,2` does not.
run_case -a -12 twelve.txt
run_case -a -1 -2 twelve.txt
run_case -a -1,2 twelve.txt
run_case -a -t12 twelve.txt
run_case -a -t 1,2 twelve.txt
run_case -a -2 -4 twelve.txt
run_case -a -1 -2 -4 twelve.txt
run_case -, ramp.txt
run_case -,, ramp.txt
run_case -4, ramp.txt
run_case -,4 ramp.txt
run_case -1, -3 ramp.txt
run_case -a4 ramp.txt
run_case -4a ramp.txt
run_case ramp.txt -4
run_case -4 ramp.txt --
# Mixed with `-t`, which does not disturb the accumulator.
run_case -t3 -6 ramp.txt
run_case -3 -t6 ramp.txt
run_case -1 -t4 -2 ramp.txt

# --- explicit lists ----------------------------------------------------------
run_case -t 1,3,5 ramp.txt
run_case -t 2,4,6,8 ramp.txt
run_case -t 4,8,12 ramp.txt
run_case -t 3,6 inner.txt
run_case -t '1 3 5' ramp.txt
run_case -t '1,,3' ramp.txt
run_case -t ',,1,,3,,' ramp.txt
run_case -t ' ' ramp.txt
run_case -t '' ramp.txt
run_case -t ',' ramp.txt
# Every occurrence appends rather than replacing.
run_case -t1 -t3 ramp.txt
run_case -t 1 -t 3 -t 5 ramp.txt
run_case -t4 --tabs=8 ramp.txt
# Past the last explicit stop, `unexpand` gives up on the rest of the line —
# where `expand` would go on treating each tab as one space.
run_case -a -t 2,4 wide.txt
run_case -a -t 2,4 inner.txt
run_case -a -t 4,8 wide.txt

# --- the two prefixes --------------------------------------------------------
run_case -t 2,/4 ramp.txt
run_case -t 2,+4 ramp.txt
run_case -a -t 2,/4 wide.txt
run_case -a -t 2,+4 wide.txt
run_case -t 1,3,/5 ramp.txt
run_case -t 1,3,+5 ramp.txt
run_case -t /4 ramp.txt
run_case -t +4 ramp.txt
# A zero-valued prefix is *no prefix*, so these are the default 8.
run_case -t /0 ramp.txt
run_case -t +0 ramp.txt
run_case -t / ramp.txt
run_case -t + ramp.txt

# --- backspaces --------------------------------------------------------------
run_case back.txt
run_case -a back.txt
run_case -t4 back.txt
run_case -a -t 2,4 back.txt
run_stdin '\b        x\n'
run_stdin 'abcdefghij\b\b\b        x\n' -a
run_stdin 'a\b\b\b\b        x\n' -t3
run_stdin 'ab\b \b \b x\n' -a

# --- the operands are one stream ---------------------------------------------
# `half1.txt` has no newline, so its line continues into `half2.txt` and the
# column count carries across the join.
run_case half1.txt half2.txt
run_case -a half1.txt half2.txt
run_case -t3 half1.txt half2.txt
run_case unterm.txt unterm.txt
run_case ramp.txt ramp.txt
run_case empty.txt ramp.txt
run_case ramp.txt empty.txt
# `-` is standard input and is an operand, not an option.
run_stdin '        q\n' -
run_stdin '        q\n' -t3 -

# --- nothing is added to a line that had no newline --------------------------
run_case unterm.txt
run_case untermblank.txt
run_case -a unterm.txt
run_stdin 'a        b'
run_stdin '        '
run_stdin '  '
run_stdin ''
run_stdin '\t'

# --- bytes, not characters ---------------------------------------------------
run_case badbytes.txt
run_case -a badbytes.txt
run_case crlf.txt
run_case -a crlf.txt
run_case utf8.txt
run_case -a utf8.txt
run_stdin 'a\xff        b\n' -a

# --- operands that cannot be opened ------------------------------------------
# The run continues and the status is 1 at the end, so the good file is still
# converted.
run_case nosuch.txt
run_case ramp.txt nosuch.txt
run_case nosuch.txt ramp.txt
run_case nosuch.txt nosuch2.txt
run_case -t3 ramp.txt nosuch.txt ramp.txt

# --- tab-stop diagnostics ----------------------------------------------------
run_case -t 0 ramp.txt
run_case -t0 ramp.txt
run_case -0 ramp.txt
run_case -t 0,4 ramp.txt
run_case -t 4,0 ramp.txt
run_case -t 4,4 ramp.txt
run_case -t 4,2 ramp.txt
run_case -t 4 -t 2 ramp.txt
run_case -4 -t 2 ramp.txt
run_case -t 2 -4 ramp.txt
run_case -4,2 ramp.txt
run_case -t x ramp.txt
run_case -t oops ramp.txt
run_case -t 4,5x ramp.txt
run_case -t 1x2 ramp.txt
run_case -t=4 ramp.txt
run_case --tabs=x ramp.txt
run_case -t 1/2 ramp.txt
run_case -t 1/2/3 ramp.txt
run_case -t 1+2+3 ramp.txt
run_case -t /2,/4 ramp.txt
run_case -t +2,+4 ramp.txt
run_case -t /2,+4 ramp.txt
run_case -t 99999999999999999999999 ramp.txt
run_case -t 18446744073709551616 ramp.txt
# The obsolete form has its own overflow message — `tab stop value is too
# large`, from `unexpand.c` rather than from the shared list parser, and it is
# reached one digit at a time so it fires on the digit that overflows.
run_case -99999999999999999999999 ramp.txt
run_case -18446744073709551616 ramp.txt
run_case -1,99999999999999999999999 ramp.txt
# The largest *accepted* value, which nonetheless converts nothing: upstream
# sizes its pending-blank buffer at `max_column_width` with `xmalloc`, so a
# stop 2**64-1 columns wide is answered `memory exhausted` and status 1. Safe
# to run as-is — this is the one place `unexpand` differs usefully from
# `expand`, which would happily start emitting 2**64-1 spaces instead.
run_case -t 18446744073709551615 ramp.txt
run_case -18446744073709551615 ramp.txt
# …and the allocation happens *after* the first operand opens, so a command
# line that opens nothing never reaches it.
run_case -t 18446744073709551615 nosuch.txt
run_case -t 18446744073709551615 empty.txt

# --- getopt diagnostics ------------------------------------------------------
run_case -q ramp.txt
run_case -aq ramp.txt
run_case -qa ramp.txt
run_case -t ramp.txt
run_case -at
run_case --tabs
run_case --zz ramp.txt
run_case --all=x ramp.txt
run_case --first-only=x ramp.txt
run_case --help=x ramp.txt
run_case --version=x ramp.txt
run_case --=x ramp.txt
run_case -- -q
# A tab-stop diagnostic exits where it is found, so which of two errors gets
# reported depends only on argv order.
run_case -t x -q ramp.txt
run_case -q -t x ramp.txt
run_case -t 0 -q ramp.txt
run_case -q -t 0 ramp.txt
# Abbreviations, which the shipped parser did not accept at all.
run_case --al ramp.txt
run_case --a inner.txt
run_case --ta=3 ramp.txt
run_case --tab=3 ramp.txt
run_case --first ramp.txt
run_case --f inner.txt
run_case --t=3 ramp.txt
run_case --al=3 ramp.txt

# --- operands and options interleave -----------------------------------------
run_case ramp.txt -t3
run_case ramp.txt -a inner.txt
run_case -t3 ramp.txt -a
run_case -- -t3
run_case ramp.txt -- -t3

# --- differ on purpose -------------------------------------------------------
# On SlateOS, and on any POSIX host, opening a directory succeeds and the *read*
# fails, so GNU says `.: Is a directory` and so do we. This harness runs a
# Windows build, where `File::open` of a directory fails outright and the errno
# is the host's. The difference is the host's, not the code's — see the same
# trap in `cut-diff.sh`, `expand-diff.sh` and in `filekind.rs`, where a Windows
# `is_file()` calls a pipe a regular file.
xfail_case 'a directory operand cannot be opened on a Windows host' .

xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1
