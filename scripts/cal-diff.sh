#!/usr/bin/env bash
# Differential test: our `cal` against util-linux `cal`.
#
# ## Why `cal` is worth a harness even though nothing depends on it
#
# It is pure computation. Every byte of output is a function of the arguments
# and of a calendar reform that happened in 1752, so unlike `date` or `uptime`
# there is nothing volatile to work around and a disagreement is always a defect
# in one side. That makes it the cheapest possible place to find out whether an
# implementation was written from the spec or from memory.
#
# And the spec has teeth. September 1752 is missing eleven days in the British
# reform, 1900 is not a leap year while 2000 is, ISO week numbers disagree with
# US ones at both ends of a year, and `-j` renumbers every cell. An
# implementation that gets an ordinary month right can be wrong about all five.
#
# ## The reference
#
# `/usr/bin/cal` from util-linux (Debian ships it in the `ncal` package, which
# is why `dpkg -S` names `ncal`). §726's caveat about heavily patched coreutils
# does not apply -- this is not coreutils -- but the general form does: a green
# run certifies agreement with Ubuntu's util-linux.
#
# Its option set is read from the program rather than assumed: `--reform`,
# `--iso`, `-1`/`-3`, `--months`, `--span`, `--vertical`, `--columns` and
# `--color` are util-linux's, and are not the same list BSD `cal` carries.
#
# ## Why `od -An -c`
#
# `cal` pads every line to a fixed width with TRAILING SPACES, and the width
# depends on `-j` and on the number of months shown. A comparison that stripped
# them would call a three-column layout equal to a one-column one for any month
# whose text happened to match. The whole output is a few hundred bytes.
#
# ## The one case that is not a pure function of the arguments
#
# Bare `cal` shows the current month. It changes once a month rather than once a
# second, so unlike `date` it is not a flake source worth excluding -- the two
# sides would have to straddle midnight on the first of a month. It is included,
# and if it ever fails alone that is the first thing to suspect.
set -u

DIFF_PROG='cal'
# Every invocation is bounded, both sides. `cal 12 9999` and `-n 1000` ask for a
# lot of output from a loop over months, which is the shape that runs away.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

run_side() {
  local side=$1; shift
  # TERM and the absence of a tty both matter: `cal` highlights today, and it
  # must not do so when its output is a pipe. Pinned so neither side can be
  # colouring on the strength of the harness's own environment.
  diff_run timeout -k 2 15 env TZ=UTC LC_ALL=C.UTF-8 TERM=dumb \
    PATH="$bindir/$side" cal "$@"
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

run_case() { compare "$@"; report "cal $*"; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS cal %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail cal %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the current month, the one case that is not purely argument-driven --------
run_case

# --- ordinary months, to establish the layout ----------------------------------
run_case 1 2021
run_case 2 2021
run_case 6 2021
run_case 12 2021

# --- THE GREGORIAN REFORM, which is the whole reason this program is hard -------
run_case 9 1752
run_case 1752
run_case 8 1752
run_case 10 1752
run_case 2 1752
run_case 1751
run_case 1753

# --- leap years, including the century rule --------------------------------------
run_case 2 2000
run_case 2 1900
run_case 2 2024
run_case 2 2023
run_case 2 1600
run_case 2 2100

# --- the ends of the range ---------------------------------------------------------
run_case 1 1
run_case 12 9999
run_case 1 1000
run_case 9999

# --- first day of the week ----------------------------------------------------------
run_case -s 6 2021
run_case -m 6 2021
run_case --sunday 6 2021
run_case --monday 6 2021
run_case -s 9 1752
run_case -m 9 1752

# --- day-of-year numbering ------------------------------------------------------------
run_case -j 1 2021
run_case -j 12 2021
run_case -j 9 1752
run_case -j 2 2000
run_case --julian 6 2021
run_case -j 2021

# --- one, three and N months ------------------------------------------------------------
run_case -1 6 2021
run_case -3 6 2021
run_case -3 1 2021
run_case -3 12 2021
run_case -3 9 1752
run_case --three 6 2021
run_case -n 5 6 2021
run_case --months 5 6 2021
run_case -n 1 6 2021
run_case -n 0 6 2021
run_case -S -3 6 2021
run_case --span -n 4 6 2021

# --- whole years --------------------------------------------------------------------------
run_case -y 2021
run_case -y
run_case --year 2021
run_case -Y 2021
run_case --twelve 2021
run_case -y -j 2021
run_case -y -m 2021

# --- week numbers ---------------------------------------------------------------------------
run_case -w 6 2021
run_case --week 6 2021
run_case -w 1 2021
run_case -w 12 2021
run_case -w -m 1 2021
run_case --week=4 6 2021
run_case -w 9 1752

# --- the reform knob -------------------------------------------------------------------------
run_case --reform=1752 9 1752
run_case --reform=gregorian 9 1752
run_case --reform=julian 9 1752
run_case --reform=iso 9 1752
run_case --iso 9 1752
run_case --iso 2 1900
run_case --reform=nosuch 9 1752
run_case --reform

# --- layout knobs ------------------------------------------------------------------------------
run_case -v 6 2021
run_case --vertical 6 2021
run_case -v -j 6 2021
run_case -c 40 -3 6 2021
run_case --columns=40 -3 6 2021
run_case -c 0 6 2021
run_case --color=never 6 2021
run_case --color=always 6 2021
run_case --color=auto 6 2021
run_case --color=nosuch 6 2021

# --- the day/month/year form and names ------------------------------------------------------------
run_case 15 9 1752
run_case 1 1 2021
run_case 31 12 2021
run_case 32 12 2021

# --- refusals -------------------------------------------------------------------------------------
run_case 0 2021
run_case 13 2021
run_case -1 2021
run_case 6 0
run_case 6 10000
run_case notamonth 2021
run_case 6 notayear
run_case 1 2 3 4
run_case -Q
run_case --nosuchoption
run_case -n
run_case -c

# --- long-option abbreviation -------------------------------------------------------------------------
run_case --mon 6 2021
run_case --sund 6 2021
run_case --jul 6 2021
run_case --thr 6 2021
run_case --ye 2021
run_case --ver 6 2021
run_case --col=40 -3 6 2021

# --- the two whose text is ours ---------------------------------------------------------------------------
# NOT an xfail: measured, and the help text matches util-linux's exactly.
# It was written here as an xfail on the assumption every other harness's help
# case holds -- and the XPASS reported that the assumption was wrong rather than
# letting it stand. An exemption that has stopped being true is worth more as a
# noisy failure than as a quiet allowance.
run_case -h
run_case --help
xfail_case "our version string, not util-linux's" -V
xfail_case "our version string, not util-linux's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
