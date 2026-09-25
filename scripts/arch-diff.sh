#!/usr/bin/env bash
# arch-diff.sh — compare our `arch` against GNU's, inside WSL.
#
# ## What this is checking
#
# Upstream's `arch` is `src/uname.c` built in arch mode: no short options,
# `--help` and `--version` by a full `getopt_long` loop, `extra operand` for
# anything else, then `uname -m`'s answer. So the cases are mostly the command
# line -- the order in which an operand and an option are noticed, prefixes, a
# value on a flag -- plus the answer itself, a write error, and one check that
# is not against GNU at all: that OUR `arch` and OUR `uname -m` agree, since
# the whole point of building both from one constant is that they cannot
# drift.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='arch'
DIFF_BINS='arch uname'
DIFF_GNU_SOURCE=9.4
# Upstream builds `arch` only when asked: it is one of `no_install__progs`.
DIFF_GNU_EXTRA='arch'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 env PATH="$bindir/$side" arch "$@" ) >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 env PATH="$bindir/$side" arch "$@" ) >"$out" 2>"$err"
    fi
    rc=$?
    : >>"$out"
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
    fi
    : >"$out"
  done
  TO_FULL=
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

run_case() {
  local label="arch $*${TO_FULL:+  [>/dev/full]}"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

xfail_case() {
  local why=$1; shift
  local label="arch $*"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- the answer ------------------------------------------------------------------
run_case
run_case --

# --- operands: noticed only after every option ---------------------------------
run_case x
run_case x y
run_case -- x
run_case -
run_case ''
run_case x --nope
run_case --nope x

# --- options -------------------------------------------------------------------
run_case -m
run_case -x
run_case -mx
run_case --nope
run_case --machine
run_case --help=1
run_case --version=1
run_case --he=1
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --help omits the GNU ancillary block' --h
xfail_case 'our --help omits the GNU ancillary block' x --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --version names SlateOS' --v

# --- write errors -----------------------------------------------------------------
TO_FULL=1; run_case

# --- ours against ours: `arch` is `uname -m` ---------------------------------------
o_arch=$(env PATH="$bindir/ours" arch)
o_um=$(env PATH="$bindir/ours" uname -m)
if [ -n "$o_arch" ] && [ "$o_arch" = "$o_um" ]; then
  pass=$((pass+1))
  [ -n "${VERBOSE:-}" ] && printf 'OK   our arch = our uname -m (%s)\n' "$o_arch"
else
  fail=$((fail+1))
  printf 'DIFF our arch {%s} is not our uname -m {%s}\n' "$o_arch" "$o_um"
fi

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
