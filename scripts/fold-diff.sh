#!/usr/bin/env bash
# Differential test: our fold against GNU fold.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the reasons. The reference has to be glibc's: the
# host's `fold` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll` rather
# than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`), so a harness pointed
# at it certifies sentences no GNU/Linux system prints (`known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`). This file already
# avoided that by reaching for `wsl -e env LC_ALL=C.UTF-8 fold`, at the cost of
# a WSL process per case and a probe to check that `wsl`'s inherited Windows
# cwd landed on the same bytes under `/mnt/...`.
#
# The subject moving with it is the part that changed an answer — see the
# directory-operand case at the foot of this file, which was an expected
# difference only because a Windows `File::open` refuses a directory outright.
#
# Run `OURS=/usr/bin/fold ./scripts/fold-diff.sh` to confirm the harness still
# discriminates: every expected difference should turn into an XPASS.
#
# ## Why `od -An -c`
#
# `fold`'s entire output is *where the newlines went*, and its inputs are full
# of bytes a shell comparison would mangle: tabs, backspaces, carriage returns,
# trailing blanks that decide where `-s` breaks, and a final line that must not
# gain a terminator it did not have. A comparison that trimmed or collapsed
# whitespace would agree with almost every wrong implementation, so stdout goes
# through `od -An -c` byte for byte.
#
# The corollary is that no case may use a large width: `fold -w 18446744073709551615`
# is perfectly valid and simply never wraps, but it also means no case can
# usefully probe the ceiling through *output*. The bounds are tested through the
# diagnostics instead.
#
# ## The cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's,
# and the two abbreviations that reach them.
#
# The locale is `C.UTF-8` throughout, including for the diagnostics that pass
# an argument through gnulib's `quote()`. Those used to be referenced under
# `LC_ALL=C`, because that was the only locale in which GNU's quote marks were
# ASCII like ours; §351 made ours U+2018/U+2019 in every locale, which is what
# GNU prints under any UTF-8 locale, so `C` is now the setting in which the
# reference would be wrong.
#
# The locale matters for a second reason here, and it is the reason the
# preamble's pin is load-bearing rather than housekeeping: `fold` counts a
# multi-byte character as one column per *byte* (upstream's `isprint` test is
# commented out), so a locale that changed how the reference decoded input
# would change where it wrapped. It does not — but a harness that did not pin
# it could not say so.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `fold` so `argv[0]` matches. See `scripts/diff-wsl.sh`. `timeout` is
# named because every invocation below is bounded with it; see `run_side`.
DIFF_PROG=fold
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# One invocation of one side. `$1` is `ours` or `gnu`; each is reached through
# a symlink named `fold` in a directory that is the whole of `PATH` for that
# one invocation, so `argv[0]` is the bare word on both sides.
#
# Every invocation is bounded, on both sides. A `-s` implementation that got the
# rescan wrong does not produce wrong output — it loops forever emitting empty
# lines, because the remainder it moved to the next line is the same remainder
# it moved last time. That is the specific failure this timeout exists for, and
# it is why the *reference* is wrapped too: a harness that only bounded our side
# would hang on the day the reference was the buggy one.
# `diff_run` keeps bash's own announcement of a child that died of a signal
# out of the stderr the caller captures; `diff-wsl.sh` says why.
run_side() { local side=$1; shift; diff_run timeout -k 2 30 env PATH="$bindir/$side" fold "$@"; }

# --- fixtures ----------------------------------------------------------------
printf 'abcdefghij\n'                     > plain.txt
printf ''                                 > empty.txt
printf '\n\n\n'                           > blanks.txt
# Every length from nothing to past the width, so an off-by-one at the break
# shows up as one wrong line rather than not at all.
printf '\na\nab\nabc\nabcd\nabcde\nabcdef\nabcdefg\nabcdefgh\n' > ramp.txt
# Words and blanks, which is what `-s` is about. The trailing blank on the
# second line is deliberate: it is a break point at the very end of the buffer.
printf 'the quick brown fox jumps\nalpha beta \ngamma\n' > words.txt
# A line whose first word is longer than any width worth testing, so `-s` finds
# no blank and has to fall back to a hard break.
printf 'supercalifragilistic x\n'         > longword.txt
# Blanks are spaces *and* tabs, and a run of them means the break lands after
# the last one.
printf 'aa\t\tbb  cc\ndd \t ee\n'         > tabsblanks.txt
# Tabs, which advance to the next multiple of eight and so can jump the width
# from under it to well past it in one byte.
printf 'a\tb\tc\n\t\t\t\n12345678\tx\n'   > tabs.txt
# Backspaces: mid-line, at the start (where the column floors at zero), and more
# of them than there are columns to give back.
printf 'abc\bd\n\b\bxy\nabcdefgh\b\b\b\bz\n' > back.txt
# Carriage returns, which reset the column to zero — so a CRLF file folds at a
# width the same file with LF endings would not.
printf 'abcde\rfghij\nabc\r\n'            > cr.txt
# A line with no terminator, which must not gain one.
printf 'abcdefghij'                       > unterm.txt
# Two operands, to prove each is its own stream: `half1.txt` ends mid-line.
printf 'abcde'                            > half1.txt
printf 'fghij\n'                          > half2.txt
# Bytes that are not text at all. Each still costs exactly one column.
printf 'a\xff\xfe\xfdb\n\x00\x01\x02\n'   > badbytes.txt
# Multibyte text: `é` is two bytes and therefore two columns, `中` three.
printf '\xc3\xa9\xc3\xa9\xc3\xa9x\n\xe4\xb8\xad\xe4\xb8\xady\n' > utf8.txt
# One very long line with no blanks, which exercises the buffer growing.
printf '%0.sz' $(seq 1 300) > long.txt; printf '\n' >> long.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(fold | od)` the recorded status
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

run_case()  { compare - "$@"; report "fold $*"; }
run_stdin() {
  local input="$1"; shift
  compare "$input" "$@"
  report "printf '$input' | fold $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  local why="$1"; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS fold %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL fold %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default width, which is eighty --------------------------------------
# The shipped parser read `-b`, `-40`, `--width=8` and `--` as filenames, so
# several of these fail against it before any wrapping is involved.
for f in plain.txt empty.txt blanks.txt ramp.txt words.txt tabs.txt back.txt cr.txt long.txt; do
  run_case "$f"
done

# --- the four spellings of a width -------------------------------------------
# The separated long form is here on purpose: its absence from expand-diff.sh
# certified a parser that rejected `--tabs 4` outright. A long option with a
# *required* argument takes the next word when there is no `=`, exactly as the
# short form does.
for w in 1 2 3 4 5 7 8 9 10 11 79 80 81; do
  run_case -w$w ramp.txt
  run_case -w "$w" ramp.txt
  run_case --width=$w ramp.txt
  run_case --width "$w" ramp.txt
done
# A separated argument must be consumed, not left an operand, and the option may
# still be abbreviated when it is written that way.
run_case --width 3 ramp.txt plain.txt
run_case --wid 3 ramp.txt
run_case -w 3 -w 5 ramp.txt

# --- the obsolete digit form -------------------------------------------------
# `-N` is a short option with an *optional* argument whose digits are recovered
# whole, so unlike `unexpand`'s accumulating form each occurrence *replaces* the
# width: `-1 -2` is two, not twelve, and `-12` is twelve.
for w in 1 2 3 4 5 8 12; do
  run_case -$w ramp.txt
done
run_case -1 -2 ramp.txt
run_case -12 ramp.txt
run_case -2 -w 5 ramp.txt
run_case -w 5 -2 ramp.txt
run_case ramp.txt -3
run_case -3 ramp.txt --
run_case -05 ramp.txt

# --- -b, which counts bytes --------------------------------------------------
for f in tabs.txt back.txt cr.txt utf8.txt badbytes.txt; do
  run_case -b -w 4 "$f"
  run_case --bytes -w 4 "$f"
  run_case -bw4 "$f"
  run_case -b "$f"
done
run_case -b -w 1 tabs.txt
run_case -b -w 8 utf8.txt
# `-b` and `-s` together: the blank search is unchanged, only the counting is.
run_case -bs -w 10 words.txt
run_case -sb -w 6 tabsblanks.txt

# --- the column model, which is where `-b` earns its keep ---------------------
# A tab can take the column from under the width to well past it in one byte,
# and the character that then starts the fresh line is *not* measured from zero
# — upstream leaves `column` alone when it emits an over-wide character alone.
for w in 1 2 3 4 7 8 9 16; do
  run_case -w $w tabs.txt
  run_case -w $w back.txt
  run_case -w $w cr.txt
  run_case -w $w utf8.txt
done
run_stdin 'a\tb\n' -w 4
run_stdin 'a\tb\n' -w 8
run_stdin 'a\tb\n' -w 9
run_stdin '\t\t\t\n' -w 8
run_stdin 'abc\b\b\b\bxyz\n' -w 3
run_stdin 'abcde\rxy\n' -w 3
run_stdin '\r\r\rabc\n' -w 2
run_stdin '\b\b\b\n' -w 1

# --- -s, the blank search and the rescan -------------------------------------
for w in 1 2 3 5 6 10 11 20; do
  run_case -s -w $w words.txt
  run_case --spaces -w $w words.txt
  run_case -s -w $w longword.txt
  run_case -s -w $w tabsblanks.txt
done
run_case -sw10 words.txt
run_case -s words.txt
run_case -s -w 4 ramp.txt
run_case -s -w 1 words.txt
# The rescan proper: the remainder moved to the next line is itself too wide, so
# the whole search happens again — and eventually falls through to a hard break.
run_stdin 'a bbbbbbbbbbbb c\n' -s -w 4
run_stdin 'a b cccccccccccc\n' -s -w 3
run_stdin 'aaaa bbbb cccc dddd\n' -s -w 5
# A blank at the very start of the buffer is still a break point, which is what
# makes an empty first line possible.
run_stdin ' abcdef\n' -s -w 3
run_stdin '  abcdef\n' -s -w 2
run_stdin '\tabcdef\n' -s -w 3
# A trailing blank at exactly the width.
run_stdin 'abc def \n' -s -w 4
run_stdin 'abcd \n' -s -w 4

# --- nothing is added to a line that had no newline --------------------------
run_case unterm.txt
run_case -w 3 unterm.txt
run_case -s -w 3 unterm.txt
run_stdin 'abcdef' -w 2
run_stdin 'ab' -w 5
run_stdin '' -w 5
run_stdin 'a' -w 1

# --- each operand is its own stream -------------------------------------------
# `half1.txt` ends mid-line, and unlike `expand` the column does *not* carry
# into the next file: `fold` resets both at every operand.
run_case half1.txt half2.txt
run_case -w 3 half1.txt half2.txt
run_case -w 4 unterm.txt unterm.txt
run_case plain.txt plain.txt plain.txt
run_case -w 3 empty.txt ramp.txt
run_case -w 3 ramp.txt empty.txt
# `-` is standard input and is an operand, not an option.
run_stdin 'abcdef\n' -w 2 -
run_stdin 'abcdef\n' -

# --- bytes, not characters ---------------------------------------------------
run_case badbytes.txt
run_case -w 2 badbytes.txt
run_case utf8.txt
run_case -w 2 utf8.txt
run_case -w 3 utf8.txt
run_case -s -w 3 utf8.txt
run_stdin 'a\xff\xfe\n' -w 2

# --- operands that cannot be opened ------------------------------------------
# The run continues and the status is 1 at the end, so the good file is still
# folded.
run_case nosuch.txt
run_case plain.txt nosuch.txt
run_case nosuch.txt plain.txt
run_case nosuch.txt nosuch2.txt
run_case -w 3 plain.txt nosuch.txt plain.txt

# --- width diagnostics -------------------------------------------------------
# The floor is 1 and the ceiling is `SIZE_MAX - 9`; which of the two `strerror`
# sentences comes out is decided by the *value* against `INT_MAX / 2`, not by
# the bound that was violated, so `0` and a huge number word differently.
run_case -w 0 plain.txt
run_case -w0 plain.txt
run_case -0 plain.txt
run_case --width=0 plain.txt
run_case -w -1 plain.txt
run_case -w x plain.txt
run_case -w '' plain.txt
run_case -w ' ' plain.txt
run_case -w ' 4' plain.txt
run_case -w '4 ' plain.txt
run_case -w +4 plain.txt
run_case -w 4x plain.txt
run_case -w 4.5 plain.txt
run_case -w oops plain.txt
run_case -w=4 plain.txt
run_case --width=x plain.txt
# No suffixes at all, which is what makes `fold -w 1K` an error though
# `head -c 1K` is not: the two pass different suffix lists to one function.
run_case -w 1K plain.txt
run_case -w 1k plain.txt
run_case -w 1b plain.txt
run_case -w 1M plain.txt
run_case -w 1KiB plain.txt
# Out of range, both sides of the heuristic.
run_case -w 2147483647 plain.txt
run_case -w 4294967295 plain.txt
run_case -w 18446744073709551615 plain.txt
run_case -w 18446744073709551616 plain.txt
run_case -w 99999999999999999999999 plain.txt
run_case -w -18446744073709551616 plain.txt
run_case -99999999999999999999999 plain.txt
# The largest *accepted* width, which simply never wraps.
run_case -w 18446744073709551606 ramp.txt
run_case -w 1000000 ramp.txt

# --- getopt diagnostics ------------------------------------------------------
run_case -q plain.txt
run_case -bq plain.txt
run_case -qb plain.txt
run_case -w plain.txt
run_case -bw
run_case --width
run_case --zz plain.txt
run_case --bytes=4 plain.txt
run_case --spaces=4 plain.txt
run_case --help=x plain.txt
run_case --version=x plain.txt
run_case --=x plain.txt
run_case -- -q
# A bad width is an `error (EXIT_FAILURE, …)` inside the option loop, so it
# preempts every option after it and is preempted by every one before it.
run_case -w 0 -q plain.txt
run_case -q -w 0 plain.txt
run_case -w x -q plain.txt
run_case -q -w x plain.txt
run_case -w 0 --zz plain.txt
# Abbreviations, which the shipped parser did not accept at all.
run_case --byt plain.txt
run_case --b plain.txt
run_case --sp plain.txt
run_case --s plain.txt
run_case --wid=3 plain.txt
run_case --w=3 plain.txt

# --- operands and options interleave -----------------------------------------
run_case plain.txt -w 3
run_case plain.txt -b words.txt
run_case -w 3 plain.txt -s
run_case -- -w3
run_case plain.txt -- -w3

# A directory operand, which used to be an expected difference and is not one
# any more: opening a directory succeeds on POSIX and the *read* fails, so GNU
# says `.: Is a directory` and so do we. It differed only while the subject was
# a Windows build, where `File::open` refuses a directory outright and the
# errno was the host's — see the same trap in `filekind.rs`, where a Windows
# `is_file()` calls a pipe a regular file. Moving both sides into WSL deleted
# it, and `cut-diff.sh` lost the identical case for the identical reason.
run_case .

# --- differ on purpose -------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The abbreviated spellings are here too, and as xfails rather than as ordinary
# cases: they reach the same two texts, so an `run_case` row for them would
# report the *body* difference we already accept and hide the thing they are
# actually for — that `--he` and `--vers` resolve at all rather than being
# rejected as unknown. An XPASS on either would mean the resolution broke in the
# direction that makes them print something else entirely.
xfail_case 'an abbreviation of --help reaches our help text' --he plain.txt
xfail_case 'an abbreviation of --version reaches our version text' --vers plain.txt

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
