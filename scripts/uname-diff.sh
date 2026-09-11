#!/usr/bin/env bash
# Differential test: our `uname` against GNU uname.
#
# ## Why this one is cheap and worth doing anyway
#
# `uname` has no input, no files and nine flags, so its whole behaviour is a
# function of `argv` and the host. That makes it one of the few subjects where a
# harness can be close to exhaustive rather than representative -- every
# single-flag case, every pairing that matters, and the canonical-order rule
# below, in a hundred-odd cases that run in seconds.
#
# It is one of the fifteen duplicate pairs left under §1005, and the corrected
# survey puts `coreutils` well ahead (2337 lines against 526). That is a
# prediction, not a measurement, and this tree has had five such predictions
# come back wrong -- twice inverted. So it gets measured like the others.
#
# ## The rule implementations get wrong: output order is CANONICAL
#
# `uname -m -s` and `uname -s -m` print the same thing, because the fields come
# out in a fixed order -- kernel-name, nodename, kernel-release, kernel-version,
# machine, processor, hardware-platform, operating-system -- regardless of the
# order the flags were given in. An implementation that appends as it parses
# looks perfect until someone writes the flags the other way round. Most of the
# pairings below exist to test exactly that, in both orders.
#
# ## The other rule: `-a` is not "all the flags"
#
# `uname -a` omits processor and hardware-platform **when they are unknown**,
# while `uname -p` alone prints `unknown`. So `-a` is not the union of the
# single flags, and an implementation that defines it that way prints two extra
# `unknown` fields on every machine where those are not known -- which is most
# of them, including this one.
#
# ## Why the reference is built rather than installed
#
# `uname` is GNU coreutils, so `DIFF_GNU_SOURCE` can supply it, and §726 says to
# use that: WSL's installed coreutils is Ubuntu's `9.4-3ubuntu6.3` and carries
# behavioural patches, so a green run against it would certify agreement with
# Debian rather than with GNU.
#
# ## Why `od -An -c`
#
# The fields are separated by single spaces and terminated by one newline, and
# an implementation that emitted a trailing space or a second newline would be
# invisible to any comparison that trimmed. The whole output is one short line,
# so there is no cost to comparing it byte for byte.
set -u

DIFF_PROG='uname'
DIFF_GNU_SOURCE=9.4
# Every invocation below is bounded with it, on both sides. `uname` should never
# take measurable time, which is exactly why an unbounded one that does would
# stop the run with no clue as to why.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# One invocation of one side, reached through a symlink named `uname` in a
# directory that is the whole of `PATH`, so `argv[0]` is the bare word and the
# `uname: ` prefix on every diagnostic matches.
run_side() {
  local side=$1; shift
  diff_run timeout -k 2 10 env LC_ALL=C.UTF-8 PATH="$bindir/$side" uname "$@"
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
  run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report "uname $*"; }

# The operating-system field is `SlateOS` here and `GNU/Linux` on the reference,
# and that is the correct answer rather than a defect: this is not GNU/Linux.
# Every case that asks for that field therefore differs on purpose, and saying
# so is not bookkeeping -- it is the difference between a harness that reports
# 36 failures and one that reports 11. A case routed here is still RUN and still
# compared; an `xfail` that starts agreeing is reported as an XPASS, so if the
# string ever changes to match GNU this harness says so rather than going quiet.
os_case() {
  xfail_case "the operating-system field is SlateOS, not GNU/Linux" "$@"
}

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS uname %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail uname %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- no options, which is `-s` and nothing else -------------------------------
run_case

# --- each flag on its own ------------------------------------------------------
for f in -s -n -r -v -m -p -i; do
  run_case "$f"
done
os_case -a
os_case -o

# --- each long spelling --------------------------------------------------------
os_case --all
run_case --kernel-name
run_case --nodename
run_case --kernel-release
run_case --kernel-version
run_case --machine
run_case --processor
run_case --hardware-platform
os_case --operating-system

# --- long-option abbreviation, which GNU accepts when unambiguous ---------------
run_case --kernel-n
run_case --mach
run_case --all-of-it
run_case --k
os_case --o
run_case --proc
run_case --hard

# --- CANONICAL ORDER: every pairing, both ways round ----------------------------
# If these agree in one order and differ in the other, the implementation is
# appending as it parses rather than emitting a fixed field order.
for a in -s -n -r -v -m -p -i -o; do
  for b in -s -n -r -v -m -p -i -o; do
    [ "$a" = "$b" ] && continue
    # A pairing that includes -o carries the operating-system field.
    case "$a$b" in
      *-o*) os_case "$a" "$b" ;;
      *)    run_case "$a" "$b" ;;
    esac
  done
done

# --- repeats, which must not duplicate a field -----------------------------------
run_case -s -s
run_case -s -s -s
run_case -m -m -n -n
os_case -a -a
run_case -s --kernel-name

# --- clustered short options ------------------------------------------------------
run_case -sn
run_case -ns
run_case -snrvm
run_case -mvrns
os_case -sa
os_case -as

# --- `-a` against the union of its parts, which it is NOT -------------------------
os_case -a
os_case -snrvmpio
os_case -a -p
os_case -a -i
run_case -p -i

# --- refusals and operands ---------------------------------------------------------
run_case -Z
run_case -x
run_case --nosuchoption
run_case --kernel
run_case extra
run_case -s extra
run_case -- -s
run_case --
run_case -

# --- the two whose text is ours -----------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
