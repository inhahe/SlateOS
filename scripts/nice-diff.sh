#!/usr/bin/env bash
# Differential test: our `nice` against GNU coreutils'.
#
# ## What this program's answer actually consists of
#
# Three things, and a harness that checks only the first two would run green
# over most of what `nice` is for:
#
#   1. What it printed — its own diagnostics, and, in the query form, a number.
#   2. What it exited with — 125 for its own failures, 126/127 for the
#      command's, and otherwise the command's own status, which it must not
#      touch.
#   3. **What niceness the command actually ran at.** The first two can be
#      right while the third is silently wrong: a version that parsed `-n 5`,
#      reported nothing and ran the command unchanged is indistinguishable
#      from a correct one by text and status alone. That is not hypothetical —
#      it is the defect this port replaced.
#
# So the command run by most cases below is `nice` itself. Run with no
# arguments `nice` is a query: it prints the niceness it inherited. `nice -n 5
# nice` therefore prints `5` if and only if the adjustment was applied, and the
# comparison of stdout becomes a comparison of the scheduling parameter.
#
# ## Descriptors closed on purpose
#
# `nice >&-` is not an odd corner: it is the case in which gnulib's
# `close_stdout` earns its keep, and the case in which a Rust port is silently
# wrong by default (the runtime reopens closed standard descriptors on
# `/dev/null` before `main`, and then swallows `EBADF` from writes to them).
# The `sh_case` cases below hand the program a closed or full descriptor and
# compare the status: measured, GNU exits 125 with `nice: write error: Bad file
# descriptor` when something was owed to a closed stdout, 0 when nothing was,
# and — the one that catches an over-eager port — **125 rather than 127** for
# `nice /nope 2>&-`, because a diagnostic that could not be delivered is itself
# a failure.
#
# ## Why both sides run inside WSL
#
# The same reasons as `cmp-diff.sh`, `tee-diff.sh` and `nohup-diff.sh`, whose
# headers spell them out, plus one specific to this program: it is built on
# `nice(2)`, `getpriority(2)` and `exec`, none of which the Windows host has.
# The build lands in `$HOME/.cache/slateos-diff-target` inside WSL, shared with
# the other harnesses (`design-decisions.md` §374).
#
# ## Cases that differ on purpose
#
# Two, both the family's: `--help` omits the GNU project's `Report bugs to:`
# block, and `--version` names SlateOS.
#
# Run `OURS=/usr/bin/nice ./scripts/nice-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `nice` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
#
# `$bindir/$side` is *prepended* to `PATH` below rather than replacing it,
# because this harness runs real commands (`nice` itself, `sh`, `printf`)
# through the program under test and they have to be findable. `argv[0]` is the
# bare word `nice` on both sides either way, so the `nice: ` prefix matches; and
# `nice nice` reaches the *same* side's binary, which is what makes the niceness
# readback below a test of one implementation rather than a mixture.
DIFF_PROG=nice
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"

case_no=0

# --- knobs, reset after every case -------------------------------------------

# Compare *which* of help/version/nothing came out rather than its full text.
# Used by the abbreviation cases, whose question is which option a prefix
# resolves to, not what that option prints — comparing the text there would
# fail on the difference already recorded as an xfail below, for every case,
# which is how a known difference stops being a record and becomes noise.
KIND=
# The niceness to run the program at, for the cases about *relative*
# adjustment. `nice`'s argument is added to what it inherited, so a port that
# ignored the inherited value and set an absolute one would agree with GNU at
# 0 and disagree here.
BASE=
reset_knobs() { KIND=; BASE=; }

classify() {
  local first
  first=$(head -c 200 "$1" | head -1)
  if [ ! -s "$1" ]; then echo empty
  elif [ "${first#Usage: nice }" != "$first" ]; then echo help
  elif [ "${first#nice \(}" != "$first" ]; then echo version
  else echo other
  fi
}

# --- running one side --------------------------------------------------------

# Direct exec: stdin from /dev/null, stdout and stderr to separate files. The
# arguments arrive as this function's arguments, so a byte that is not valid
# UTF-8 and a word containing a space both reach the program untouched.
run_direct() {
  local side=$1 out=$2 err=$3 rcf=$4; shift 4
  if [ -n "$BASE" ]; then
    ( PATH="$bindir/$side:$PATH" timeout -k 2 30 \
        "$gnu_real" -n "$BASE" nice "$@" </dev/null >"$out" 2>"$err" )
  else
    ( PATH="$bindir/$side:$PATH" timeout -k 2 30 \
        nice "$@" </dev/null >"$out" 2>"$err" )
  fi
  echo $? >"$rcf"
  return 0
}

# A shell snippet, for the cases whose subject is a descriptor: `nice >&-`
# cannot be expressed as an argument list. The snippet's own status is written
# to descriptor 9, which the snippet cannot reach and therefore cannot close —
# `echo $?` through stdout would be lost by exactly the cases that matter.
run_snippet() {
  local side=$1 out=$2 err=$3 rcf=$4 snippet=$5
  ( PATH="$bindir/$side:$PATH" timeout -k 2 30 \
      sh -c "$snippet"'; echo $? >&9' </dev/null ) \
    >"$out" 2>"$err" 9>"$rcf"
  return 0
}

# --- comparing the two sides -------------------------------------------------

judge() {
  local o_out=$1 g_out=$2 o_err=$3 g_err=$4 o_rc=$5 g_rc=$6 label=$7
  local o_show g_show
  if [ -n "$KIND" ]; then
    o_show="class $(classify "$o_out")"; g_show="class $(classify "$g_out")"
  else
    o_show=$(cat "$o_out"); g_show=$(cat "$g_out")
  fi
  local o_e g_e o_r g_r
  o_e=$(cat "$o_err"); g_e=$(cat "$g_err")
  o_r=$(cat "$o_rc"); g_r=$(cat "$g_rc")

  if [ "$o_show" = "$g_show" ] && [ "$o_e" = "$g_e" ] && [ "$o_r" = "$g_r" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: rc=%s out{%s} err{%s}\n  gnu : rc=%s out{%s} err{%s}' \
    "$o_r" "$(printf '%s' "$o_show" | tr '\n' '|')" "$(printf '%s' "$o_e" | tr '\n' '|')" \
    "$g_r" "$(printf '%s' "$g_show" | tr '\n' '|')" "$(printf '%s' "$g_e" | tr '\n' '|')")
  LABEL=$label
}

compare_direct() {
  case_no=$((case_no+1))
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no  g_rc=$work/gr$case_no
  run_direct ours "$o_out" "$o_err" "$o_rc" "$@"
  run_direct gnu  "$g_out" "$g_err" "$g_rc" "$@"
  judge "$o_out" "$g_out" "$o_err" "$g_err" "$o_rc" "$g_rc" \
    "nice${BASE:+ [base=$BASE]} $*"
  reset_knobs
}

compare_snippet() {
  case_no=$((case_no+1))
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no  g_rc=$work/gr$case_no
  run_snippet ours "$o_out" "$o_err" "$o_rc" "$1"
  run_snippet gnu  "$g_out" "$g_err" "$g_rc" "$1"
  judge "$o_out" "$g_out" "$o_err" "$g_err" "$o_rc" "$g_rc" "[sh] $1"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case()    { compare_direct "$@"; report; }
sh_case()     { compare_snippet "$1"; report; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare_direct "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

echo "nice-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. The query form
# =============================================================================
# No adjustment and no command: print the niceness we were started at. The
# `BASE` cases prove the number is read rather than assumed — a port that
# printed a constant 0 would pass the first case and fail the rest.

run_case
BASE=3;  run_case
BASE=7;  run_case
BASE=19; run_case

# =============================================================================
# 2. The adjustment is applied to the command
# =============================================================================
# `nice N nice` prints the niceness the inner one inherited, which is the only
# way to see from outside that the outer one did anything at all. The default
# with no `-n` is +10, which `--help` promises.

run_case nice
run_case -n 5 nice
run_case -n 0 nice
run_case -n 1 nice
run_case -n 19 nice
BASE=5; run_case nice                # 5 + 10, capped at 19
BASE=5; run_case -n 3 nice           # relative, not absolute: 8
BASE=9; run_case -n 4 nice           # 13
BASE=19; run_case -n 5 nice          # already at the floor of the scheduler

# Over the top of the range, both in the argument and in the sum. GNU clamps
# the *adjustment* to ±(2*NZERO-1) and the kernel clamps the *result* to 19,
# so these agree only if both clampings happen.
run_case -n 39 nice
run_case -n 40 nice
run_case -n 1000 nice
run_case -n 99999999999999999999 nice   # past what a long can hold
BASE=10; run_case -n 39 nice

# =============================================================================
# 3. The obsolete `-NUM` syntax
# =============================================================================
# The reason the scan is interleaved rather than a pre-pass: this form cannot
# be described to getopt, so each word is tested for it before getopt is asked
# for the next item. `-5`, `--5` and `-+5` are all adjustments; `--` is not.

run_case -5 nice
run_case -0 nice
run_case -19 nice
run_case --5 nice                    # the second dash is the sign: -5
run_case -+5 nice                    # and an explicit plus is +5
run_case -40 nice                    # clamped like `-n`
run_case --40 nice

# Trailing rubbish is an *invalid adjustment*, not an unknown option: the word
# was claimed by the digit test before getopt ever saw it.
run_case -5x nice
run_case --5x nice
run_case -+5x nice
run_case -5.5 nice

# =============================================================================
# 4. Which adjustment wins
# =============================================================================
# The last one, across both spellings, because each assignment overwrites the
# previous rather than accumulating.

run_case -n 3 -n 7 nice
run_case -5 -3 nice
run_case -n 3 -5 nice
run_case -5 -n 3 nice
run_case --adjustment=7 -n 2 nice
run_case -n 2 --adjustment 7 nice
run_case -n2 nice                    # joined
run_case --adjustment=4 nice
run_case --adjustment 4 nice

# A negative value belongs to the option that asked for it — this is what a
# pre-pass over the whole command line would get wrong, taking the `-5` for a
# second obsolete adjustment.
run_case -n -5 nice
run_case --adjustment=-5 nice
run_case --adjustment -5 nice

# =============================================================================
# 5. Refusing to lower the niceness
# =============================================================================
# Unprivileged, a negative adjustment is refused — and the refusal is a
# *warning*: the command still runs, at the niceness it already had, and the
# status is the command's. A port that treated it as fatal would be caught
# here rather than in production.

run_case -n -1 nice
run_case -n -20 nice
BASE=5; run_case -n -1 nice
run_case -n -5 sh -c 'exit 42'

# =============================================================================
# 6. Numbers that are not numbers
# =============================================================================
# `invalid adjustment` carries no `Try 'nice --help'` referral — upstream
# prints it with `error (EXIT_CANCELED, …)`, which does not call `usage`.

run_case -n abc nice
run_case -n '' nice
run_case -n ' ' nice
run_case -n ' 5' nice                # strtol skips leading blanks: accepted
run_case -n '5 ' nice
run_case -n 0x10 nice                # base 10 only
run_case -n +5 nice
run_case -n -- nice
run_case -n abc                      # no command either: which complaint wins
run_case --adjustment=abc nice

# =============================================================================
# 7. An adjustment with nothing to apply it to
# =============================================================================
# The other complaint, and the only one of `nice`'s own that *does* carry the
# referral.

run_case -n 5
run_case -5
run_case --adjustment=5
run_case -n 5 --

# =============================================================================
# 8. Options getopt itself rejects
# =============================================================================

run_case -x nice
run_case -xy nice
run_case --nope nice
run_case -n                          # missing argument, short
run_case --adjustment                # missing argument, long
run_case --help=x
run_case --version=x
run_case --=x                        # names every long option, in table order

# =============================================================================
# 9. `--` and what follows it
# =============================================================================

run_case --
run_case -- nice
run_case -- -n 5                     # the command is named `-n`
run_case -n 5 -- nice
run_case -n 5 -- -n

# =============================================================================
# 10. Everything after the command belongs to the command
# =============================================================================
# The `+` in the short-option string. Without it these would be `nice`'s own
# options and would be rejected — and the option most likely to be typed after
# a command is exactly `--help`.

run_case printf '[%s]' a -n
run_case echo -n hi
run_case sh -c 'echo "$1"' sh --nope
run_case sh -c 'echo "$1"' sh -n 5
run_case -n 5 sh -c 'echo "$1"' sh --help

# =============================================================================
# 11. The command's own status, and the two failures to run it
# =============================================================================
# 127 when the name resolved to nothing, 126 when it resolved to something that
# could not be executed. Anything else is the command's own and must arrive
# untouched.

run_case sh -c 'exit 0'
run_case sh -c 'exit 42'
run_case sh -c 'exit 255'
run_case false
run_case /nope/nope                  # 127
run_case nosuchcommandanywhere       # 127, via PATH
run_case /etc                        # 126: a directory
run_case /etc/hostname               # 126: found, not executable
run_case ''                          # the empty name
run_case -                           # a command literally called `-`
run_case ./nice                      # relative, and not there

# =============================================================================
# 12. Bytes that are not valid UTF-8
# =============================================================================
# The finding that brought this program up for conversion: the old version held
# argv as `Vec<String>` and panicked here before doing anything at all.

run_case "$(printf 'na\377me')"                     # 127, and the name is quoted
run_case -n "$(printf '\377')" nice                 # invalid adjustment, quoted
run_case -n 5 printf '[%s]' "$(printf 'na\377me')"  # passed through untouched
run_case -n 5 printf '[%s]' "$(printf '\377')" "$(printf '\200\201')"
run_case "$(printf '\377\376')"

# =============================================================================
# 13. --help and --version
# =============================================================================
# Both win over an adjustment that would otherwise be refused, because upstream
# validates the number only after the whole scan.

KIND=1; run_case --h
KIND=1; run_case --he
KIND=1; run_case --v
KIND=1; run_case --ver
KIND=1; run_case --a                 # unambiguous: only `--adjustment`
run_case --adj 5 nice                # ...and takes its argument like the whole
run_case --adjust=5 nice
KIND=1; run_case --help -n abc
KIND=1; run_case -n abc --help
KIND=1; run_case --version -n abc

xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# =============================================================================
# 14. Descriptors closed or full
# =============================================================================
# See the header. These are the cases in which a Rust port is wrong by
# default, and they are all status-only: there is by construction nothing to
# read back from a descriptor that is closed.

# Nothing was owed to stdout, so closing it is not an error.
sh_case 'nice true >&-'
sh_case 'nice -n 5 true >&-'
sh_case 'nice -n 5 sh -c "exit 42" >&-'

# The query form owes stdout a number, and cannot deliver it.
sh_case 'nice >&-'
sh_case 'nice 2>&-'
sh_case 'nice >/dev/full'

# A diagnostic that could not be delivered is itself a failure, and it replaces
# the status the command would have produced: 125, not 127.
sh_case 'nice /nope 2>&-'
sh_case 'nice /nope 2>/dev/full'
sh_case 'nice -n abc 2>&-'
sh_case 'nice -n 5 2>&-'
sh_case 'nice --nope 2>&-'

# A warning that could not be delivered, likewise.
sh_case 'nice -n -5 true 2>&-'
sh_case 'nice -n -5 true 2>/dev/full'

# stdout closed but the failure is on stderr, and the other way round.
sh_case 'nice /nope >&-'
sh_case 'nice -n abc >&-'
sh_case 'nice --help >&-'
sh_case 'nice --version >&-'
sh_case 'nice --help >/dev/full'
# stdout survives here, so what it says is the text that differs on purpose:
# compare which of the two came out, as the abbreviation cases do.
KIND=1; sh_case 'nice --version 2>&-'
KIND=1; sh_case 'nice --help 2>&-'
sh_case 'nice true >&- 2>&-'
sh_case 'nice >&- 2>&-'

# =============================================================================
# Summary
# =============================================================================
total=$((pass+fail+xfail+xpass))
printf 'nice         %d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
  "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
