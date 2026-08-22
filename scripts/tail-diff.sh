#!/usr/bin/env bash
# Differential test: our tail against GNU tail.
#
# `tail` copies bytes, so stdout is compared byte for byte with `od -An -c`. A
# comparison that trimmed whitespace would be blind to the two things this
# implementation most had to get right: that a final line with no terminator is
# copied *without* one being added (and still counts as one of the N), and that
# `-z` changes what a terminator is.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `tail` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `head-diff.sh`.
#
# ## Why every invocation is wrapped in a timeout, and why the signal is KILL
#
# `-f` does not terminate, and `-f` is reachable from more places than it looks:
# the obsolete word hides one (`tail -2f`), and so does a long-option
# abbreviation (`tail --fo`). Rather than maintain a list of which cases may
# hang, *every* case on both sides runs under `timeout`, so a case that hangs on
# one side and not the other shows up as the status difference it is instead of
# stopping the harness.
#
# The signal must be `KILL`, and that is not a preference. Our binary is a
# *native Windows* process launched from MSYS, and Cygwin can only deliver a
# signal to a non-Cygwin child by calling `TerminateProcess`, which it does for
# `SIGKILL` and for nothing else. A plain `timeout 3` therefore expires, sends a
# SIGTERM that goes nowhere, and leaves our `tail -f` running forever — measured:
# the first `-f` case pinned the harness for five minutes with the child still
# accumulating memory. `timeout -s KILL 3` tears it down in three seconds.
#
# The two sides then report that kill differently — MSYS's bash gives 137
# (128 + SIGKILL) and `wsl.exe` gives the bare 9 — so `norm_rc` folds both onto
# `timeout`'s own 124. No real `tail` status is 9 or 137, so nothing else is
# conflated. What is *not* normalised is stdout: both implementations flush
# before they start following, so a killed `-f` still has to have printed the
# same bytes, and that is compared as strictly as any other case.
#
# The locale is `C.UTF-8` throughout. Nothing `tail` does is locale-dependent —
# it never decodes a byte — and the quote marks gnulib puts round a bad number
# agree here too: since §351 ours are U+2018/U+2019 in every locale, which is
# what GNU prints under any UTF-8 one. The number diagnostics used to be
# referenced under `LC_ALL=C`, back when ours stayed ASCII
# (`open-questions.md` → B-Q2, since answered); `C` is now the setting in which
# the reference would be wrong.
set -u

# Our tail is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

# Built here, from the package named, rather than picked up out of `target/`.
# A harness that only *runs* that path measures whatever was written there
# last, which need not be current and need not even be this crate — see
# `scripts/diff-subject.sh`.
. "$(dirname "$0")/diff-subject.sh"
OURS=$(subject_binary coreutils tail "${OURS:-}") || exit 1
GNU=${GNU:-"wsl -e timeout -s KILL 3 env LC_ALL=C.UTF-8 tail"}
export LC_ALL=${LC_ALL:-C.UTF-8}

# A case that had to be killed, however the two shells chose to say so.
norm_rc() { case $1 in 9|137) printf '124\n' ;; *) printf '%s\n' "$1" ;; esac; }

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands
# on the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'probe\n' > .probe
if [ "$($GNU -n1 .probe 2>/dev/null)" = "probe" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "tail-diff: glibc tail not reachable in this directory (tried: $GNU); skipping"
fi
rm -f .probe

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
# Larger than one read block, so that the backwards block scan has to take more
# than one step and the ragged-first-block arithmetic is exercised.
seq 1 40000                                     > big.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 ref=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(tail | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  # The subshell is not decoration: bash announces a foreground child killed by
  # a signal ("… Killed …") on *its own* stderr, which no redirection of the
  # child's streams can catch. Running it one level down puts that announcement
  # on the subshell's stderr, where `2>/dev/null` reaches it, and every `-f`
  # case is killed by construction.
  if [ "$stdin" = "-" ]; then
    ( timeout -s KILL 3 "$OURS_ABS" "$@" </dev/null >"$o_bin" 2>"$o_err" ) 2>/dev/null; o_rc=$?
    $ref "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    ( printf '%b' "$stdin" | timeout -s KILL 3 "$OURS_ABS" "$@" >"$o_bin" 2>"$o_err" ) 2>/dev/null
    o_rc=$?
    printf '%b' "$stdin" | $ref "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_rc=$(norm_rc "$o_rc"); g_rc=$(norm_rc "$g_rc")
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of
  # the getopt module is that the sentences match, so a harness that only asked
  # "did it complain?" would pass on every wording this exists to fix.
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - "$GNU" "$@"; report "tail $*"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" "$GNU" "$@"
  report "printf '$input' | tail $*"
}

xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local reason="$1"; shift
  compare - "$GNU" "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL tail %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS tail %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
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
  x=$( ( timeout -s KILL 3 "$OURS_ABS" $a </dev/null 2>&1 ) 2>/dev/null ); xr=$(norm_rc $?)
  y=$( ( timeout -s KILL 3 "$OURS_ABS" $b </dev/null 2>&1 ) 2>/dev/null ); yr=$(norm_rc $?)
  if [ "$x" = "$y" ] && [ "$xr" = "$yr" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   tail %s == tail %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF tail %s != tail %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

# --- the default: ten lines --------------------------------------------------
run_case five.txt
run_case twelve.txt
run_case empty.txt
run_case blanks.txt
run_case unterminated.txt
run_case big.txt
run_stdin 'a\nb\nc\n'
run_stdin ''
run_stdin 'no newline'

# --- -n, from the end --------------------------------------------------------
run_case -n1 five.txt
run_case -n 1 five.txt
run_case -n0 five.txt
run_case -n99 five.txt
run_case -n4 twelve.txt
run_case -n1 unterminated.txt
run_case -n2 unterminated.txt
run_case -n1 blanks.txt
run_case -n1 empty.txt
run_case -n1 crlf.txt
run_case -n1 badbytes.txt
run_case -n3 big.txt
run_case -n 20000 big.txt
run_stdin 'a\nb' -n1
run_stdin 'a\nb' -n2
run_stdin 'a\nb\n' -n1
run_stdin '\n' -n1
run_stdin 'a\nb\nc\n' -n0

# --- -n +N, from the start ---------------------------------------------------
# `+0` and `+1` are the same thing, which is upstream's stated concession to
# Unix compatibility and not an accident of the arithmetic.
run_case -n +0 five.txt
run_case -n +1 five.txt
run_case -n +2 five.txt
run_case -n +5 five.txt
run_case -n +6 five.txt
run_case -n +99 five.txt
run_case -n +2 unterminated.txt
run_case -n +39998 big.txt
run_stdin 'a\nb\nc\n' -n +2
run_stdin 'a\nb' -n +2
# `+` is sticky: nothing clears it, so the second `-n` here still counts from
# the start.
run_case -n +2 -n 2 five.txt
run_case -n 2 -n +2 five.txt

# --- -c ----------------------------------------------------------------------
run_case -c1 five.txt
run_case -c 3 five.txt
run_case -c0 five.txt
run_case -c99 five.txt
run_case -c 4 unterminated.txt
run_case -c +3 five.txt
run_case -c +1 five.txt
run_case -c +99 five.txt
run_case -c 100000 big.txt
run_stdin 'abcdef' -c2
run_stdin 'abcdef' -c +3
run_stdin '' -c2
# The last option wins, and switching unit does not reset the count.
run_case -n2 -c3 five.txt
run_case -c3 -n2 five.txt

# --- -z ----------------------------------------------------------------------
run_case -z -n1 nul.txt
run_case -z -n2 nul.txt
run_case -z -n1 nul-unterminated.txt
run_case -z -n1 five.txt
run_case --zero-terminated -n2 nul.txt

# --- headers -----------------------------------------------------------------
run_case five.txt w1.txt
run_case -n1 five.txt w1.txt twelve.txt
run_case -q -n1 five.txt w1.txt
run_case -v -n1 five.txt
run_case -v -n1 five.txt w1.txt
run_case --verbose -n1 five.txt
run_case --quiet -n1 five.txt w1.txt
run_case --silent -n1 five.txt w1.txt
# A file that fails to open gets no banner, so the next one that succeeds is
# still the first and takes no leading blank line.
run_case -n1 nope.txt five.txt
run_case -n1 five.txt nope.txt
run_case -v -n1 nope.txt five.txt
run_case -n1 nope.txt alsonope.txt
run_case -n1 nope.txt
# `-n0` returns before a single banner is printed, even under `-v`.
run_case -v -n0 five.txt
run_case -v -n0 five.txt w1.txt
run_case -v -c0 five.txt
# ... but `+0` does not, because it is `from_start`.
run_case -v -n +0 five.txt

# --- operands ----------------------------------------------------------------
run_case -n1 -- five.txt
run_case -n1 five.txt five.txt
run_stdin 'a\nb\nc\n' -n1 -
run_stdin 'a\nb\nc\n' -n1 - w1.txt
run_case -n1 ''
# A directory is `xfail_case`d at the foot of this file — the host build cannot
# open one, so it reports a different sentence there and the same one on target.

# --- the obsolete form -------------------------------------------------------
# Recognised only in the three command-line shapes upstream allows.
run_case -3 five.txt
run_case -3
run_case -3 -- five.txt
run_case +3 five.txt
run_case -0 five.txt
run_case -1 twelve.txt
# Out of shape: two operands, or an option anywhere near it.
run_case -3 five.txt w1.txt
run_case -q -3 five.txt
run_case -3 -q five.txt
run_case -3 -- five.txt w1.txt
run_case -3x five.txt
# The letters, which are units and a multiplier rather than the modern flags.
run_case -2b big.txt
run_case -b big.txt
run_case -2c five.txt
run_case -2l five.txt
run_case -l five.txt
run_case -2lb five.txt
run_case -2cb five.txt
# A bare `-` is standard input and a bare `-c` wants an argument; neither is
# the obsolete form.
run_stdin 'a\nb\nc\n' -
run_case -c
# `-c` with a following word takes it as the count, so this is a number
# diagnostic and belongs under the C locale like the rest of them.
run_case -c five.txt
# `f` inside the obsolete word means follow, which is why these time out.
run_case -2f five.txt
run_case -2lf five.txt
run_case -f five.txt
run_case -99999999999999999999999b big.txt
run_case -99999999999999999999999 big.txt

# --- getopt's sentences ------------------------------------------------------
run_case -x five.txt
run_case -n
run_case -s
run_case --li
run_case --lines
run_case --bytes
run_case --pid
run_case --verbose=1
run_case --quiet=x
run_case --retry=x
# The hidden options really do take three dashes.
run_case ---presume-input-pipe -n1 five.txt
run_case ---p -n1 five.txt
run_case --presume-input-pipe -n1 five.txt
run_case ---disable-inotify -n1 five.txt
# An empty prefix matches every option, printing the table in declaration order.
run_case --=x
# Ambiguities, whose candidate lists are in declaration order rather than
# alphabetical.
run_case --v five.txt
run_case --s five.txt
# `--p` resolves to `--pid`, so its operand is a PID and the diagnostic is a
# number one — under the C locale with the others.
run_case --p five.txt
# Abbreviations, every one of which the hand-written parser refused.
run_case --li 2 five.txt
run_case --by 4 five.txt
run_case --verb -n1 five.txt
run_case --zero -n1 nul.txt
run_case --q -n1 five.txt w1.txt
run_case --sil -n1 five.txt w1.txt
run_case --max-unchanged-stats=2 -n1 five.txt
run_case --max 2 -n1 five.txt

# --- --follow ----------------------------------------------------------------
# `--follow` takes an *optional* argument, so the value must be attached; a
# detached one is an operand.
run_case --follow=name -n1 five.txt
run_case --follow=descriptor -n1 five.txt
run_case --follow=d -n1 five.txt
run_case --follow=n -n1 five.txt
# `argmatch`'s two sentences quote with gnulib's `quote()`, so like the number
# diagnostics they come out curly on both sides under this file's `C.UTF-8`.
run_case --follow=x -n1 five.txt
run_case --follow= -n1 five.txt
run_case -F -n1 five.txt
run_case --retry -n1 five.txt
run_case --retry --follow=name -n1 five.txt
run_case --retry -f -n1 five.txt
run_case --pid=1 -n1 five.txt
run_case --follow=name -n1 -
run_case -F -n1 -
# `--pid` naming a process that cannot exist: the loop notices and stops,
# which is the one way a `-f` run ends on its own.
run_case --pid=2147483647 -f -n1 five.txt
run_case --pid=2147483647 -F -n1 nope.txt

# --- the number, which is gnulib's xdectoumax --------------------------------
run_case -n x five.txt
run_case -c x five.txt
run_case -n 1x five.txt
run_case -n 0x10 five.txt
run_case -n '' five.txt
run_case -n - five.txt
run_case -n + five.txt
run_case -n -- -2 five.txt
run_case -n '2 ' five.txt
run_case -n ' ' five.txt
run_case -n ' -2' five.txt
run_case -n ' +2' five.txt
run_case -n ' K' five.txt
run_case -n '+K' five.txt
run_case -n K five.txt
run_case -n +x five.txt
run_case -n -x five.txt
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
# the way C does and not the way a shell would.
run_case -n "a'b" five.txt
run_case -n 'a\b' five.txt
run_case -n 'a"b' five.txt
run_case -n 'a b' five.txt
# The suffixes gnulib knows but `tail`'s list does not include.
for bad in 1w 1c 1B 1g 1t 1D; do run_case -n "$bad" five.txt; done
# The ones it does, checked for value rather than validity.
run_case -c 1K big.txt
run_case -c 1kB big.txt
run_case -c 1KiB big.txt
run_case -c 2b big.txt
run_case -n 1K big.txt
run_case -c 1M big.txt

# --- --pid and --max-unchanged-stats, which take a different parser ----------
run_case --pid=x -f five.txt
run_case --pid= -f five.txt
run_case --pid=-1 -f five.txt
run_case --pid=5k -f five.txt
run_case --pid=1b -f five.txt
run_case --pid=99999999999999999999 -f five.txt
run_case --pid=2147483648 -f five.txt
run_case '--pid= 5' -n1 five.txt
run_case --max-unchanged-stats=x -f five.txt
run_case --max-unchanged-stats=-1 -f five.txt
run_case --max-unchanged-stats=1k -f five.txt
run_case --max-unchanged-stats=99999999999999999999 -f five.txt

# --- -s, which is strtod rather than strtoumax -------------------------------
run_case -s x -f five.txt
run_case -s '' -f five.txt
run_case -s -1 -f five.txt
run_case -s nan -f five.txt
run_case -s 1x -f five.txt
run_case -s 1e -f five.txt
run_case --sleep-interval=x -f five.txt
run_case -s 0x -f five.txt

# --- the warnings, which change nothing but are printed ----------------------
run_case --retry -n1 five.txt
run_case --pid=1 -n1 five.txt
run_case --retry --pid=1 -n1 five.txt
run_case -F -n1 five.txt
run_case --retry --follow=descriptor -n1 five.txt

# --- what differs on purpose -------------------------------------------------
# `--help`'s body is GNU's except for two clauses about `inotify`, an interface
# this implementation does not have and never uses: it always polls, so
# `--max-unchanged-stats` is never "rarely useful" and `--pid` is not checked
# "at least once every N seconds". What follows the body differs too — GNU
# closes with links to gnu.org, the Translation Project and
# `info '(coreutils) tail invocation'`, which name an upstream project this is
# not and documentation this does not ship.
xfail_case help-describes-a-polling-implementation-and-does-not-refer-to-gnu --h
xfail_case version-names-slateos-coreutils-not-gnu-coreutils --vers
# The abbreviations above still have to resolve, which the comparison cannot
# show while the outputs are expected to differ.
selfsame --h --help
selfsame --vers --version
selfsame --hel --help
# A directory: POSIX lets it open and fails the *read*, which is what GNU
# reports. Our host build is Windows, where `File::open` of a directory fails
# outright, so the sentence is `cannot open` rather than `error reading`. On
# SlateOS — which presents as `linux-musl` — this case takes the POSIX path and
# agrees.
xfail_case windows-refuses-to-open-a-directory-where-posix-fails-the-read -n1 .

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
