#!/usr/bin/env bash
# Differential test: our paste against GNU paste.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the reasons. The reference has to be glibc's: the
# host's `paste` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll` rather
# than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`), so a harness pointed
# at it certifies sentences no GNU/Linux system prints (`known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`). This file already
# avoided that by reaching for `wsl -e env LC_ALL=C.UTF-8 paste`, at the cost of
# a WSL process per case and a probe to check that `wsl`'s inherited Windows cwd
# landed on the same bytes under `/mnt/...`.
#
# The subject moving with it is the part that changed an answer — see the two
# directory-operand cases at the foot of this file, which were expected
# differences only because a Windows `File::open` refuses a directory outright.
#
# Run `OURS=/usr/bin/paste ./scripts/paste-diff.sh` to confirm the harness still
# discriminates: every expected difference should turn into an XPASS.
#
# ## Why `od -An -c`
#
# `paste`'s output *is* its delimiters — which include a tab by default, a NUL
# under `-z`, and whatever `-d` was handed, up to and including bytes that are
# not text. A comparison that trimmed or collapsed whitespace would agree with
# almost every wrong implementation, so stdout goes through `od -An -c` byte for
# byte. The empty-file cases in particular differ from the correct output by
# exactly one byte.
#
# ## The two modes have different failure rules, and both are tested
#
# Parallel opens every operand before writing anything and dies on the first one
# that will not open; serial opens them as it reaches them, names every bad one
# and keeps going. So `paste A nosuch B` and `paste -s A nosuch B` differ in
# their stdout, their stderr *and* the order of the two, which is why the
# missing-file section runs each case both ways.
#
# ## The two cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's.
#
# One further asymmetry is not tested here at all. With standard input closed,
# `paste -s - A 0<&-` prints `paste: -: Bad file descriptor` **twice**: once
# from the per-file read and once from `main`'s closing `fclose (stdin)`. Rust
# has no `fclose (stdin)` and surfaces a read error where the read happens, so
# the second line has no analogue. Injecting a closed descriptor through this
# harness would also mean bypassing `compare`'s redirections, so the case lives
# in `known-issues.md` rather than here.
#
# The locale is `C.UTF-8` throughout, including for the diagnostics that pass
# an argument through gnulib's `quote()`. Those used to be referenced under
# `LC_ALL=C`, because that was the only locale in which GNU's quote marks were
# ASCII like ours; §351 made ours U+2018/U+2019 in every locale, which is what
# GNU prints under any UTF-8 locale, so `C` is now the setting in which the
# reference would be wrong.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `paste` so `argv[0]` matches. See `scripts/diff-wsl.sh`.  `timeout` is
# named because every invocation below is bounded with it; see `run_side`.
DIFF_PROG=paste
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# Every invocation is bounded, on both sides. `paste`'s parallel loop terminates
# only when the last file closes, so an implementation that failed to close a
# file that had hit end of file would spin emitting delimiters forever. That is
# the specific failure this timeout exists for, and it is why the *reference* is
# wrapped too: a harness that only bounded our side would hang on the day the
# reference was the buggy one.
# One invocation of one side. `$1` is `ours` or `gnu`; each is reached through
# a symlink named `paste` in a directory that is the whole of `PATH` for that
# one invocation, so `argv[0]` is the bare word on both sides.
run_side() { local side=$1; shift; timeout -k 2 30 env PATH="$bindir/$side" paste "$@"; }

# --- fixtures ----------------------------------------------------------------
# Three files of decreasing length, so every combination of "which column ran
# out first" is reachable by permuting them.
printf 'a1\na2\na3\n'      > a.txt
printf 'b1\nb2\n'          > b.txt
printf 'c1\n'              > c.txt
# Empty: nothing at all in parallel mode, a bare line delimiter in serial.
printf ''                  > empty.txt
# No final newline, which POSIX says must gain one.
printf 'x1\nx2'            > unterm.txt
# A single unterminated line, so the very first thing read is also the last.
printf 'solo'              > solo.txt
# Blank lines, which are lines and must keep their columns.
printf '\n\n\n'            > blanks.txt
# NUL-separated, for `-z`; note it also holds newlines, which under `-z` are
# ordinary content and must survive untouched.
printf 'p\nq\0r\0s'        > nuls.txt
printf 'z1\0z2\0'          > nulterm.txt
# Bytes that are not text at all, in the data.
printf '\xff\xfe\n\x80\x01\n' > bad.txt
# A file whose lines contain the delimiter characters the -d cases use, so a
# delimiter is never confusable with content by accident alone.
printf 'a,b\na;b\n'        > commas.txt
# Long enough to cross the reader's buffer boundary, which is where a
# one-byte-lookahead serial merge would go wrong if it were written per chunk
# rather than per byte.
{ for i in $(seq 1 5000); do printf 'line%d\n' "$i"; done; } > long.txt
mkdir subdir

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(paste | od)` the recorded status
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

run_case()  { compare - "$@"; report "paste $*"; }
run_stdin() {
  local input="$1"; shift
  compare "$input" "$@"
  report "printf '$input' | paste $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  local why="$1"; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS paste %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL paste %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- parallel, the default ----------------------------------------------------
# The shipped parser read `--`, `-z` and every long option as filenames, and its
# delimiter list did not cycle, so a good many of these fail against it before
# any of the padding rules are involved.
for f in a.txt b.txt c.txt empty.txt unterm.txt solo.txt blanks.txt bad.txt long.txt; do
  run_case "$f"
done
run_case a.txt b.txt
run_case b.txt a.txt
run_case a.txt b.txt c.txt
run_case c.txt b.txt a.txt
run_case a.txt c.txt c.txt
run_case c.txt c.txt a.txt
run_case a.txt a.txt a.txt
run_case a.txt b.txt c.txt empty.txt
run_case empty.txt a.txt
run_case a.txt empty.txt
run_case empty.txt empty.txt
run_case empty.txt a.txt empty.txt
run_case unterm.txt a.txt
run_case a.txt unterm.txt
run_case unterm.txt unterm.txt
run_case solo.txt solo.txt
run_case blanks.txt a.txt
run_case a.txt blanks.txt
run_case bad.txt a.txt
run_case long.txt a.txt
run_case long.txt long.txt

# --- serial -------------------------------------------------------------------
for f in a.txt b.txt c.txt empty.txt unterm.txt solo.txt blanks.txt bad.txt long.txt; do
  run_case -s "$f"
  run_case --serial "$f"
done
run_case -s a.txt b.txt
run_case -s empty.txt a.txt
run_case -s a.txt empty.txt
run_case -s unterm.txt a.txt
run_case -s a.txt b.txt c.txt empty.txt unterm.txt
run_case -s long.txt long.txt

# --- the delimiter list, and its cycle ---------------------------------------
# A one-character list is the easy case; the point of `-d` is that a longer one
# cycles *within* a line and restarts at the next.
for d in , : ',;' ',;:' 'xyz' '..' ; do
  run_case -d "$d" a.txt b.txt c.txt
  run_case -d "$d" a.txt b.txt
  run_case -s -d "$d" a.txt b.txt
  run_case -s -d "$d" long.txt
done
run_case -d , commas.txt commas.txt
run_case -s -d , commas.txt
# Every spelling of the option, including the separated long form: its absence
# from expand-diff.sh once certified a parser that rejected `--tabs 4` outright.
run_case -d, a.txt b.txt
run_case -d , a.txt b.txt
run_case --delimiters=, a.txt b.txt
run_case --delimiters , a.txt b.txt
run_case --delim=, a.txt b.txt
run_case --d , a.txt b.txt
# The last one wins.
run_case -d , -d : a.txt b.txt c.txt
run_case -d , --delimiters=: a.txt b.txt c.txt
# A cluster ends at `-d`, because the rest of it is the argument.
run_case -sd, a.txt b.txt
run_case -ds, a.txt b.txt
run_case -zd, a.txt b.txt
run_case -dz, a.txt b.txt

# --- the escapes, and the empty position -------------------------------------
# `\0` is "no delimiter here", and the position is still spent — so `x\0y` is
# not the same list as `xy`.
run_case -d '\0' a.txt b.txt
run_case -d 'x\0y' a.txt b.txt c.txt
run_case -d 'x\0y' a.txt b.txt c.txt a.txt
run_case -s -d '\0' a.txt
run_case -s -d 'x\0y' a.txt
# An empty argument is rewritten to `\0`, so it means the same thing.
run_case -d '' a.txt b.txt
run_case -d '' a.txt b.txt c.txt
run_case -s -d '' a.txt
run_case --delimiters= a.txt b.txt
# The named escapes, each of which is a byte a shell argument could not carry
# literally without the harness mangling it.
for d in '\n' '\t' '\r' '\b' '\f' '\v' '\\' ; do
  run_case -d "$d" a.txt b.txt
  run_case -s -d "$d" a.txt
done
# An unrecognised escape is the character itself, the backslash dropped.
run_case -d '\q' a.txt b.txt
run_case -d '\-' a.txt b.txt
run_case -d '\,\;' a.txt b.txt c.txt
# An even run of backslashes is a delimiter; an odd one is the fatal case.
run_case -d 'a\\' a.txt b.txt c.txt
run_case -d '\\\\' a.txt b.txt
# Bytes that are not text, as delimiters.
run_case -d 'é' a.txt b.txt
run_case -d '中' a.txt b.txt c.txt

# --- -z, which changes what a line is -----------------------------------------
run_case -z a.txt b.txt
run_case -z nuls.txt
run_case -z nulterm.txt
run_case -z nulterm.txt nulterm.txt
run_case -z nuls.txt nulterm.txt
run_case -z empty.txt
run_case -z empty.txt nulterm.txt
run_case -z unterm.txt
run_case --zero-terminated nulterm.txt a.txt
run_case -sz a.txt b.txt
run_case -zs nulterm.txt
run_case -zs empty.txt
run_case -s -z nuls.txt nulterm.txt
run_case -z -d , nulterm.txt nulterm.txt
run_case -zsd, nulterm.txt
run_case --zero nulterm.txt

# --- standard input -----------------------------------------------------------
# Every `-` is the same stream, so `paste - -` deals one stream into two columns.
run_stdin 'p\nq\n' -
run_stdin 'p\nq\nr\n' - -
run_stdin 'p\nq\nr\ns\n' - - -
run_stdin 'p\nq\n' a.txt -
run_stdin 'p\nq\n' - a.txt
run_stdin 'p\nq\nr\n' -s -
run_stdin 'p\nq\nr\n' -s - a.txt
run_stdin 'p\nq\nr\n' -s a.txt -
run_stdin 'p\nq\n' -d , - -
run_stdin 'p\nq' -
run_stdin '' -
run_stdin 'p\nq\n'
run_stdin 'p\nq\n' -s
run_stdin 'p\nq\n' -- -

# --- operands that cannot be opened ------------------------------------------
# Parallel is fatal at the first bad operand and prints nothing at all; serial
# names every one and keeps going. Both orders of good and bad, both ways.
run_case nosuch.txt
run_case a.txt nosuch.txt
run_case nosuch.txt a.txt
run_case a.txt nosuch.txt b.txt
run_case nosuch.txt nosuch2.txt
run_case -s nosuch.txt
run_case -s a.txt nosuch.txt
run_case -s nosuch.txt a.txt
run_case -s a.txt nosuch.txt b.txt
run_case -s nosuch.txt nosuch2.txt
run_case -s nosuch.txt a.txt nosuch2.txt

# --- the delimiter diagnostic -------------------------------------------------
# A list ending in an odd number of backslashes is fatal, and the argument is
# quoted in `c_maybe` style — the raw argument, before collapsing, with the
# quotes off unless some byte forces them on.
run_case -d '\' a.txt
run_case -d 'a\' a.txt
run_case -d '\\\' a.txt
run_case -d ',\' a.txt
run_case -d '\t\' a.txt
run_case --delimiters='x\' a.txt
run_case -s -d 'a\' a.txt
# It is raised after the whole command line is read, so a later `-d` rescues an
# earlier bad one and a getopt error anywhere preempts it…
run_case -d 'a\' -d , a.txt b.txt
run_case -d 'a\' -Q a.txt
run_case -Q -d 'a\' a.txt
# …but it comes before the files are opened, so a missing operand loses to it.
run_case -d '\' nosuch.txt
run_case -s -d '\' nosuch.txt

# --- getopt diagnostics ------------------------------------------------------
run_case -Q a.txt
run_case -sQ a.txt
run_case -Qs a.txt
run_case -d
run_case -sd
run_case --delimiters
run_case --nope a.txt
run_case --serial=x a.txt
run_case --zero-terminated=x a.txt
run_case --help=x a.txt
run_case --version=x a.txt
run_case --=x a.txt
run_case -- -Q
# Abbreviations, which the shipped parser did not accept at all. No two long
# options share a first letter, so every one of them abbreviates to one letter.
run_case --s a.txt b.txt
run_case --se a.txt b.txt
run_case --z nulterm.txt
run_case --zero-term nulterm.txt
run_case --deli , a.txt b.txt

# --- operands and options interleave -----------------------------------------
run_case a.txt -s
run_case a.txt -d , b.txt
run_case -s a.txt -d , b.txt
run_case -- a.txt b.txt
run_case -s -- a.txt
run_case -- -d
run_case a.txt -- -d

# A directory operand in each mode, which used to be an expected difference and
# is not one any more: opening a directory succeeds on POSIX and the *read*
# fails, so GNU says `subdir: Is a directory` — with status 1 and, in serial
# mode, a bare newline already on stdout — and so do we. They differed only
# while the subject was a Windows build, where `File::open` refuses a directory
# outright and the errno was the host's; see the same trap in `filekind.rs`,
# where a Windows `is_file()` calls a pipe a regular file. Moving both sides
# into WSL deleted them, as it did the identical case in `cut-diff.sh`,
# `fold-diff.sh`, `expand-diff.sh`, `unexpand-diff.sh` and `uniq-diff.sh`.
run_case subdir
run_case -s subdir

# --- differ on purpose -------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The abbreviated spellings are here too, and as xfails rather than as ordinary
# cases: they reach the same two texts, so a `run_case` row for them would
# report the *body* difference we already accept and hide the thing they are
# actually for — that `--h` and `--v` resolve at all rather than being rejected
# as unknown. An XPASS on either would mean the resolution broke in the
# direction that makes them print something else entirely.
xfail_case 'an abbreviation of --help reaches our help text' --h a.txt
xfail_case 'an abbreviation of --version reaches our version text' --v a.txt

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
