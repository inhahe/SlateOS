#!/usr/bin/env bash
# Differential test: our head against GNU head.
#
# `head` copies bytes, so stdout is compared byte for byte with `od -An -c`. A
# comparison that trimmed whitespace would be blind to the two things this
# implementation most had to get right: that a final line with no terminator is
# copied *without* one being added, and that `-z` changes what a terminator is.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the general reasons. This harness's *reference*
# was never the problem — it already ran `wsl -e env LC_ALL=C.UTF-8 head`, so
# the sentences it certified were glibc's, not MSYS2's. (That distinction
# matters elsewhere: the harnesses that referenced `$(command -v head)`-style
# MSYS2 binaries were certifying `getopt` wordings no GNU/Linux system prints —
# `known-issues.md` → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`.)
#
# What the move buys here is four other things.
#
# The subject stops being a Windows build. This file used to carry an expected
# difference for `head -n1 .`: on Windows `File::open` of a directory fails
# outright, where on SlateOS and every POSIX host the open succeeds and the
# *read* fails, so GNU says `error reading '.': Is a directory` and so do we.
# The recorded reason was true and the behaviour it described was the host's,
# not the program's — which is the worst kind of expected difference, because
# it hides the case rather than testing it. Under Linux it is an ordinary case
# and passes. The same move is what puts the real `coreutils::stdfd` under test
# at all: its closed-descriptor and raw-`write` machinery is `#[cfg(target_os =
# "linux")]`, and the build the harness used to compare got the stub `imp` that
# returns `Unsupported` — so every `head` invocation with a closed stdout was
# being certified against code SlateOS will never run.
#
# `argv[0]` matches. Both sides are reached through a symlink named `head` in a
# directory that is the whole of `PATH` for that one invocation, so neither
# side can prefix a diagnostic with a path where the other prints a bare word.
#
# One `wsl` process per run, not one per case. The old reference spawned a
# fresh WSL session for every single comparison; there are 167 of them.
#
# And nothing depends on the Windows working directory happening to map to the
# same bytes under `/mnt/...`. The `.probe` file the old harness created and
# looked for from the other side existed only to check that assumption; with
# both sides inside WSL there is no assumption to check.
#
# The locale is `C.UTF-8`, fixed by the preamble. Nothing `head` does is
# locale-dependent — it never decodes a byte — and the quote marks gnulib puts
# round a bad number agree here too: since §351 ours are U+2018/U+2019 in every
# locale, which is what GNU prints under any UTF-8 one. The number diagnostics
# used to be referenced under `LC_ALL=C`, back when ours stayed ASCII
# (`open-questions.md` → B-Q2, since answered); `C` is now the setting in which
# the reference would be wrong.
#
# Run `OURS=/usr/bin/head ./scripts/head-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `head` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=head
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# --- fixtures ----------------------------------------------------------------
printf 'l1\nl2\nl3\nl4\nl5\n'                   > five.txt
printf 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n'   > twelve.txt
printf 'one two three'                          > unterminated.txt
printf ''                                       > empty.txt
printf '\n\n\n'                                 > blanks.txt
printf 'x\r\ny\r\n'                             > crlf.txt
printf 'a\xffb\n\x80\xfe\n'                     > badbytes.txt
printf 'r1\0r2\0r3\0'                           > nul.txt
printf 'r1\0r2\0r3'                             > nul-unterminated.txt
printf 'q\n'                                    > w1.txt

# One invocation of one side. `$1` is `ours` or `gnu`.
#
# Each side is reached through a symlink named `head` in a directory that is
# the whole of `PATH` for that one invocation, so `argv[0]` is the bare word on
# both sides and the `head: ` prefix on every diagnostic matches.
run_side() {
  local side=$1 stdin=$2 out=$3 err=$4; shift 4
  if [ "$stdin" = "-" ]; then
    env PATH="$bindir/$side" head "$@" </dev/null >"$out" 2>"$err"
  else
    printf '%b' "$stdin" | env PATH="$bindir/$side" head "$@" >"$out" 2>"$err"
  fi
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(head | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$stdin" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$stdin" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of
  # the getopt module is that the sentences match, so a harness that only asked
  # "did it complain?" would pass on every wording this exists to fix. That is
  # only sound because the reference is glibc's — our `errmsg` prints POSIX's
  # strerror strings, which agree with glibc and did not agree with the MSYS2
  # reference this harness used to compare against.
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

run_case()  { compare - "$@"; report "head $*"; }
run_stdin() {
  local input="$1"; shift
  compare "$input" "$@"
  report "printf '$input' | head $*"
}

xfail_case() {
  local reason="$1"; shift
  compare - "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL head %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS head %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# Two of our own invocations compared against each other. The reference cannot
# arbitrate an abbreviation whose long form is *meant* to differ from GNU's, but
# the abbreviation must still resolve to the same option — which is the whole
# point of the getopt module — so that much is checked here.
selfsame() {
  local a="$1" b="$2" x y xr yr
  # shellcheck disable=SC2086  # both are single options by construction
  x=$(env PATH="$bindir/ours" head $a </dev/null 2>&1); xr=$?
  y=$(env PATH="$bindir/ours" head $b </dev/null 2>&1); yr=$?
  if [ "$x" = "$y" ] && [ "$xr" = "$yr" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   head %s == head %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF head %s != head %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

echo "head-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# --- the default: ten lines --------------------------------------------------
run_case five.txt
run_case twelve.txt
run_case empty.txt
run_case blanks.txt
run_case unterminated.txt
run_stdin 'a\nb\nc\n'
run_stdin ''
run_stdin 'no newline'

# --- counting lines ----------------------------------------------------------
run_case -n1 five.txt
run_case -n 1 five.txt
run_case -n0 five.txt
run_case -n99 five.txt
run_case --lines=2 five.txt
run_case --lines 2 five.txt
# A final line with no terminator is a line, and is copied without one.
run_stdin 'a\nb' -n1
run_stdin 'a\nb' -n2
run_stdin 'a\nb' -n9

# --- counting bytes ----------------------------------------------------------
run_case -c1 five.txt
run_case -c 4 five.txt
run_case -c0 five.txt
run_case -c999 five.txt
run_case --bytes=5 five.txt
# `head` copies bytes and never decodes them: an invalid sequence must survive,
# and a split multi-byte character must be split.
run_case -c2 badbytes.txt
run_stdin '\xc3\xa9\xc3\xa9' -c3
run_case -n1 badbytes.txt
# CRLF survives, which the `BufRead::lines` implementation this replaced could
# not manage — it re-emitted `\r\n` as `\n`.
run_case -n1 crlf.txt
run_case crlf.txt

# --- the last option of a kind wins, and it also picks the unit --------------
run_case -n2 -c3 five.txt
run_case -c3 -n2 five.txt
run_case --bytes=3 --lines=2 five.txt

# --- eliding from the end ----------------------------------------------------
run_case -n -1 five.txt
run_case -n-1 five.txt
run_case -n -4 five.txt
run_case -n -5 five.txt
run_case -n -9 five.txt
run_case -n -0 five.txt
run_case -c -1 five.txt
run_case -c -14 five.txt
run_case -c -99 five.txt
run_case -c -0 five.txt
run_case --lines=-2 five.txt
run_case --bytes=-2 five.txt
# The unterminated tail counts as a line to elide, but is never printed.
run_stdin 'a\nb' -n -1
run_stdin 'a\nb\nc' -n -1
run_stdin 'a\nb\nc' -n -2
run_case -n -1 unterminated.txt
run_case -n -1 empty.txt
# The dash is taken off the first byte, before whitespace is skipped.
run_case -n '- 5' five.txt
run_case -n '-  5' five.txt

# --- NUL-terminated records --------------------------------------------------
run_case -z -n1 nul.txt
run_case -z -n2 nul.txt
run_case -z -n9 nul.txt
run_case -z nul-unterminated.txt
run_case -z -n -1 nul.txt
run_case -z -n -1 nul-unterminated.txt
run_case --zero-terminated -n1 nul.txt
# Without `-z` the same file is one very long line.
run_case -n1 nul.txt

# --- headers -----------------------------------------------------------------
run_case -n1 five.txt w1.txt
run_case -n1 five.txt w1.txt twelve.txt
run_case -n1 five.txt
run_case -v -n1 five.txt
run_case -q -n1 five.txt w1.txt
run_case --quiet -n1 five.txt w1.txt
run_case --silent -n1 five.txt w1.txt
run_case --verbose -n1 five.txt
# The last of -q/-v wins.
run_case -q -v -n1 five.txt
run_case -v -q -n1 five.txt w1.txt
# A header still prints when the count is zero, and for an empty file.
run_case -n0 five.txt w1.txt
run_case -v -n0 empty.txt
run_case -v -c0 five.txt
# stdin's header is the words "standard input", not "-".
run_stdin 'a\nb\n' -v -n1
run_stdin 'a\nb\n' -n1 - w1.txt

# --- operands ----------------------------------------------------------------
run_stdin 'a\nb\n' -n1 -
run_case -n1 -- five.txt
run_case -n1 five.txt five.txt
run_case -- -n1 2>/dev/null
# A file that cannot be opened gets no header, so the next one that succeeds is
# still the first and takes no leading blank line.
run_case -n1 nope.txt five.txt
run_case -n1 five.txt nope.txt
run_case -n1 nope.txt
run_case -n1 nope.txt alsonope.txt

# --- the obsolete -NUM form --------------------------------------------------
run_case -3 five.txt
run_case -0 five.txt
run_case -3q five.txt w1.txt
run_case -3v five.txt
run_case -1c five.txt
run_case -2b five.txt
run_case -3l five.txt
run_case -3z nul.txt
run_case -3 -n1 five.txt
# Recognised in exactly one position: the first argument.
run_case -n1 -3 five.txt
run_case -q -3 five.txt
run_case five.txt -3
run_case -3 -3 five.txt
# An unknown trailing letter gets its own sentence, and the letter is unquoted.
run_case -3x five.txt
run_case -3qx five.txt

# --- getopt's five sentences -------------------------------------------------
run_case -x five.txt
run_case -c
run_case -n
run_case --fo
run_case --lines
run_case --bytes
run_case --verbose=1
run_case --quiet=x
# The hidden option really does take three dashes.
run_case ---presume-input-pipe -n1 five.txt
run_case ---p -n1 five.txt
run_case --presume-input-pipe -n1 five.txt
# An empty prefix matches every option, printing the table in declaration order.
run_case --=x
# `--v` is ambiguous between `--verbose` and `--version`, and the candidates are
# listed in declaration order rather than alphabetically.
run_case --v five.txt
# Abbreviations, every one of which the hand-written parser refused.
run_case --li 2 five.txt
run_case --by 4 five.txt
run_case --verb -n1 five.txt
run_case --zero -n1 nul.txt
run_case --q -n1 five.txt w1.txt
run_case --s -n1 five.txt w1.txt

# --- the number, which is gnulib's xdectoumax --------------------------------
run_case -n x five.txt
run_case -c x five.txt
run_case -n 1x five.txt
run_case -n 0x10 five.txt
run_case -n '' five.txt
run_case -n - five.txt
run_case -n -- -5 five.txt
run_case -n '5 ' five.txt
run_case -n ' ' five.txt
run_case -n ' -5' five.txt
run_case -n ' K' five.txt
run_case -n '+K' five.txt
run_case -n 99999999999999999999 five.txt
run_case -n 18446744073709551616 five.txt
run_case -n 18014398509481984K five.txt
run_case -n 99999999999999999999X five.txt
run_case -n 1Ki five.txt
run_case -n 1KiBB five.txt
run_case -n 5K5 five.txt
run_case -n 1Z five.txt
run_case -n 1Q five.txt
# The offending text is echoed back through gnulib's `quote()`, which escapes
# the way C does and not the way a shell would. The two styles agree on
# everything that holds neither a quote nor a backslash, so only these cases
# tell them apart: `quote()` gives 'a\'b' where shell-escaping gives "a'b".
run_case -n "a'b" five.txt
run_case -n 'a\b' five.txt
run_case -n 'a"b' five.txt
run_case -n 'a b' five.txt
run_case -n "$(printf 'a\tb')" five.txt
run_case -c "a'b" five.txt
# The suffixes gnulib knows but `head`'s list does not include.
for bad in 1w 1c 1B 1g 1t 1D; do run_case -n "$bad" five.txt; done
# The ones it does, checked for value rather than validity: five.txt has five
# lines, so any count of 5 or more prints all of it and a smaller one does not.
run_case -n 1K five.txt
run_case -n 1b five.txt
run_case -n K five.txt
run_case -n b five.txt
run_case -c 1kB five.txt
run_case -c 1KiB five.txt
run_case -c 1MD five.txt
run_case -n '  5' five.txt
run_case -n +5 five.txt
run_case -n 0005 five.txt
run_case -n 1E five.txt
run_case -c 1E five.txt

# --- a directory operand -----------------------------------------------------
# Opening a directory succeeds and the *read* fails, so GNU says `error reading
# '.': Is a directory` and so must we. This was an expected difference for as
# long as the subject was a Windows build, where `File::open` of a directory
# fails outright and we said `cannot open '.' for reading: …` instead — a
# difference that belonged to the host and not to the code, and that hid the
# case rather than testing it. It is an ordinary case now.
run_case -n1 .
run_case -c1 .
run_case -n1 . five.txt

# --- differ on purpose -------------------------------------------------------
# `--help`'s body matches GNU's byte for byte; what follows it does not, and
# must not. GNU closes every `--help` with `emit_ancillary_info` — links to
# gnu.org, the Translation Project and `info '(coreutils) head invocation'` —
# which name an upstream project this is not and documentation this does not
# ship. `--version` likewise names SlateOS coreutils rather than GNU coreutils
# 9.4 with its copyright and authors.
xfail_case help-closes-with-a-referral-to-the-gnu-project-which-this-is-not --h
xfail_case version-names-slateos-coreutils-not-gnu-coreutils --vers
# The abbreviations above still have to resolve, which the comparison cannot
# show while the outputs are expected to differ.
selfsame --h --help
selfsame --vers --version
selfsame --hel --help

# --- summary -----------------------------------------------------------------
printf '\n%d passed, %d differed' "$pass" "$fail"
[ "$xfail" -gt 0 ] && printf ', %d differ on purpose' "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d XPASS' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
