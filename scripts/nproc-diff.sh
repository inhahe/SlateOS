#!/usr/bin/env bash
# nproc-diff.sh — compare our `nproc` against GNU's, inside WSL.
#
# ## What this is checking
#
# gnulib's `num_processors` is the whole program, so the cases vary what it
# reads rather than files:
#
#   * **the affinity mask** -- run under `taskset` pinned to one CPU, two, and
#     a range, so "available to this process" and "online" come apart. The
#     crate this replaced counted `/sys` ranges and would have said 12 to all
#     of them.
#   * **the OpenMP variables** -- `OMP_NUM_THREADS` alone, with a nesting list,
#     with white space, invalid, zero, overflowing; `OMP_THREAD_LIMIT` alone
#     and as a cap on each; and that `--all` ignores both.
#   * **`--ignore`** -- subtraction, the floor of 1, and every way gnulib's
#     `xdectoumax` refuses an argument (sign, suffix, overflow, empty), which
#     also decides the order an invalid number and an unknown option are
#     noticed in.
#   * **the command line** -- prefixes, a value on `--all`, a missing value,
#     operands.
#
# Every case runs with the OpenMP variables cleared first, so the caller's own
# environment cannot make both sides agree by accident.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='nproc'
DIFF_GNU_SOURCE=9.4
DIFF_NEED='taskset'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
# Set for one case, then cleared: the environment and CPU set it runs with.
SETENV=()
PIN=''

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  local -a pin=()
  [ -n "$PIN" ] && pin=(taskset -c "$PIN")
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 "${pin[@]}" env -u OMP_NUM_THREADS -u OMP_THREAD_LIMIT "${SETENV[@]}" \
          PATH="$bindir/$side" nproc "$@" ) >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 "${pin[@]}" env -u OMP_NUM_THREADS -u OMP_THREAD_LIMIT "${SETENV[@]}" \
          PATH="$bindir/$side" nproc "$@" ) >"$out" 2>"$err"
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
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf '%s%snproc %s%s' \
    "${PIN:+[cpus $PIN] }" "${SETENV[*]:+${SETENV[*]} }" "$*" "${TO_FULL:+  [>/dev/full]}"
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
  TO_FULL=; SETENV=(); PIN=''
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
  TO_FULL=; SETENV=(); PIN=''
  return 0
}

# --- the plain answer and --all -------------------------------------------------
run_case
run_case --all

# --- the affinity mask ------------------------------------------------------------
PIN=0;     run_case
PIN=0,2;   run_case
PIN=1-3;   run_case
PIN=0;     run_case --all
PIN=0-1;   run_case --ignore=1
PIN=0-2;   run_case --ignore=5

# --- the OpenMP variables ---------------------------------------------------------
SETENV=(OMP_NUM_THREADS=3);                         run_case
SETENV=(OMP_NUM_THREADS=3);                         run_case --all
SETENV=('OMP_NUM_THREADS=3,2,1');                   run_case
SETENV=('OMP_NUM_THREADS= 3 ');                     run_case
SETENV=('OMP_NUM_THREADS=3 ,2');                    run_case
SETENV=(OMP_NUM_THREADS=3x);                        run_case
SETENV=(OMP_NUM_THREADS=0);                         run_case
SETENV=(OMP_NUM_THREADS=);                          run_case
SETENV=(OMP_NUM_THREADS=-3);                        run_case
SETENV=(OMP_NUM_THREADS=+3);                        run_case
SETENV=(OMP_NUM_THREADS=99999999999999999999999);  run_case
SETENV=(OMP_NUM_THREADS=500);                       run_case
SETENV=(OMP_THREAD_LIMIT=2);                        run_case
SETENV=(OMP_THREAD_LIMIT=2);                        run_case --all
SETENV=(OMP_THREAD_LIMIT=500);                      run_case
SETENV=(OMP_THREAD_LIMIT=0);                        run_case
SETENV=(OMP_THREAD_LIMIT=x);                        run_case
SETENV=(OMP_NUM_THREADS=8 OMP_THREAD_LIMIT=3);      run_case
SETENV=(OMP_NUM_THREADS=2 OMP_THREAD_LIMIT=3);      run_case
SETENV=(OMP_NUM_THREADS=3);                         run_case --ignore=1
SETENV=(OMP_NUM_THREADS=3);                         run_case --ignore=5
PIN=0; SETENV=(OMP_THREAD_LIMIT=4);                 run_case
PIN=0; SETENV=(OMP_NUM_THREADS=6);                  run_case

# --- --ignore -----------------------------------------------------------------------
run_case --ignore=0
run_case --ignore=1
run_case --ignore 1
run_case --ignore=1000
run_case --ignore=18446744073709551615
run_case --ignore=18446744073709551616
run_case --ignore=-1
run_case --ignore=+1
run_case --ignore=abc
run_case --ignore=
run_case --ignore=' 1'
run_case --ignore='1 '
run_case --ignore=1k
run_case --ignore=1K
run_case --ignore=0x10
run_case --ignore=010
run_case --all --ignore=3
run_case --ignore=2 --ignore=1
run_case --ignore=x --nope
run_case --nope --ignore=x
run_case --ignore=x extra

# --- the command line ----------------------------------------------------------------
run_case --a
run_case --al
run_case --i=2
run_case --ig 2
run_case --all=1
run_case --ignore
run_case extra
run_case extra --all
run_case -- x
run_case --
run_case -x
run_case -a
run_case --nope
run_case --help=1
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --help omits the GNU ancillary block' --he
xfail_case 'our --help omits the GNU ancillary block' extra --help
xfail_case 'our --version names SlateOS' --version

# --- write errors ---------------------------------------------------------------------
TO_FULL=1; run_case
TO_FULL=1; run_case --all

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
