#!/usr/bin/env bash
# factor-diff.sh — compare our `factor` against GNU's, inside WSL.
#
# ## What this is checking
#
#   * **factorisations** across both of upstream's paths: below 2^127 (its
#     "single-precision" arithmetic) and from 2^127 up (its GMP path), with the
#     boundary itself on either side; primes, prime powers, semiprimes of two
#     large primes, 2^64 and its neighbours, and Mersenne and Fermat numbers
#     that factor quickly;
#   * **reading**: spaces and one `+` skipped, digits only, leading zeros
#     normalised away; tabs, a second sign, trailing garbage and an empty
#     operand refused, the run continuing; and standard input split on space,
#     tab and newline only;
#   * **output order**: a number of each size in one run, where the large one
#     is written first -- upstream flushes its stdio path at once and its own
#     line buffer at exit;
#   * **-h** on both paths, and **write errors** of the three shapes upstream
#     has: the line buffer's at exit, the line buffer's mid-run (reported twice,
#     since the exit handler retries it), and stdio's bare `write error`.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block, `--version` names SlateOS, and
# `---debug` accepts the option without writing upstream's algorithm trace.
set -u

DIFF_PROG='factor'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
STDIN=

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      printf '%b' "$STDIN" | ( timeout -k 2 120 env PATH="$bindir/$side" factor "$@" ) >/dev/full 2>"$err"
    else
      printf '%b' "$STDIN" | ( timeout -k 2 120 env PATH="$bindir/$side" factor "$@" ) >"$out" 2>"$err"
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
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-400)" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-400)" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf 'factor %s%s%s' "$*" "${STDIN:+ <<<$(printf '%s' "$STDIN" | cut -c1-40)}" "${TO_FULL:+  [>/dev/full]}"
}

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  TO_FULL=; STDIN=
  return 0
}

xfail_case() {
  local why=$1; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  TO_FULL=; STDIN=
  return 0
}

# --- factorisations, the single-precision side -------------------------------------
run_case 0 1 2 3 4 5 6 7 8 9 10 11 12
run_case 360 997 1000 1001 65536 65537 1000000 999999
run_case 999985999949             # 999983 * 1000003
run_case 1000002000001            # (10^6 + 1)^2
run_case 18446744073709551615 18446744073709551616 18446744073709551617
run_case 18446744073709551557     # the largest 64-bit prime
run_case 340282366920938463463374607431768211455  # 2^128 - 1: the large path
run_case 170141183460469231731687303715884105727  # 2^127 - 1, prime: the last small
run_case 170141183460469231731687303715884105728  # 2^127: the first large
run_case 170141183460469231731687303715884105729  # 2^127 + 1
run_case 1329227995784915872903807060280344576    # 2^120
run_case 1208925819614629174706176                 # 2^80
run_case 1180591620717411303424                    # 2^70
run_case 1180591620717411303425                    # 2^70 + 1
run_case 6700417 4294967297                         # F5 = 641 * 6700417
run_case 1152921504606846883 1152921504606846869    # 2^60 - 93 and 2^60 - 107, primes
run_case 1208925819335353221265601                 # two 40-bit primes
run_case 99999999999999999999999999999              # 29 nines
run_case 1000000000000000000000000000000000000000   # 10^39: the large path, small factors
run_case 1606938044258990275541962092341162602522202993782792835301376  # 2^200
run_case 3486784401 205891132094649                 # 3^20, 3^30

# --- -h -----------------------------------------------------------------------------
run_case -h 360 1024 1000000 997
run_case --exponents 1606938044258990275541962092341162602522202993782792835301376
run_case -h 1000000000000000000000000000000000000000

# --- reading ------------------------------------------------------------------------
run_case ' 12' '+12' '  +0012' 00 007
run_case '++12'
run_case '12x' x '' 12
run_case "$(printf '\t12')"
run_case -- -5
run_case -1
run_case -x
run_case --nope
run_case --exp 12
STDIN='6 x 9\n\t10\n'; run_case
STDIN='  12  \n\n  15'; run_case
STDIN=''; run_case
STDIN='12\r\n13\n'; run_case
STDIN='+7 ++7 07\n'; run_case
STDIN='1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20\n'; run_case -h

# --- output order: the large path writes first ----------------------------------------
run_case 12 170141183460469231731687303715884105729 15
STDIN='12\n340282366920938463463374607431768211456\n15\n'; run_case

# --- write errors ---------------------------------------------------------------------
TO_FULL=1; run_case 12
# shellcheck disable=SC2046  # one operand per number, on purpose
TO_FULL=1; run_case $(seq 1000 1200)
TO_FULL=1; run_case 170141183460469231731687303715884105729
TO_FULL=1; run_case 12 170141183460469231731687303715884105729
TO_FULL=1; run_case x
TO_FULL=1; run_case --help

xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case "upstream's algorithm trace is not ported" ---debug 12

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
