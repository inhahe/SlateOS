#!/usr/bin/env bash
# hostid-diff.sh — compare our `hostid` against GNU's, inside WSL.
#
# ## What this is checking
#
# `hostid` prints `gethostid()` as eight hex digits. On Linux both programs
# call glibc's `gethostid`, so the answer itself is compared as well as the
# command line: `parse_gnu_standard_options_only` (the first option decides),
# `extra operand`, and a write error.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='hostid'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 env PATH="$bindir/$side" hostid "$@" ) >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 env PATH="$bindir/$side" hostid "$@" ) >"$out" 2>"$err"
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
  local label="hostid $*${TO_FULL:+  [>/dev/full]}"
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
  local label="hostid $*"
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

# --- operands: gnulib reads one option, then counts --------------------------------
run_case x
run_case x y
run_case -- x
run_case -
run_case ''
run_case x --nope
run_case --nope x

# --- options -----------------------------------------------------------------------
run_case -x
run_case -v
run_case --nope
run_case --help=1
run_case --version=1
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --help omits the GNU ancillary block' --h
xfail_case 'our --help omits the GNU ancillary block' x --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --version names SlateOS' --v

# --- write errors --------------------------------------------------------------------
TO_FULL=1; run_case

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
