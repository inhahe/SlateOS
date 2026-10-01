#!/usr/bin/env bash
# sum-diff.sh — compare our `sum` against GNU's, inside WSL.
#
# ## What this is checking
#
#   * **both algorithms** over inputs chosen for their arithmetic: empty, one
#     byte, the 0xff bytes that carry, and the block edges -- 512, 513, 1024
#     and 1025 bytes -- where the rounded-up block count changes;
#   * **the name rule** -- printed whenever any operand is given, `-` too,
#     never for a bare `sum`; printed as its bytes, spaces and all;
#   * **-r and -s**, and which wins;
#   * **failures** -- a missing file, a directory, one bad operand among good
#     ones (every operand is still attempted, and the status is 1) -- and a
#     write error.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='sum'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
INPUT=/dev/null

fx=$DIFF_TMP/fx
mkdir -p "$fx/dir" || exit 1
cd "$fx" || exit 1
printf '' > empty
printf 'x' > one
printf 'hello\n' > hello
head -c 512 /dev/zero | tr '\0' '\377' > ff512
head -c 513 /dev/zero | tr '\0' '\377' > ff513
awk 'BEGIN { for (i = 0; i < 1024; i++) printf "%c", 65 + i % 26 }' > b1024
awk 'BEGIN { for (i = 0; i < 1025; i++) printf "%c", 65 + i % 26 }' > b1025
awk 'BEGIN { for (i = 0; i < 70000; i++) printf "%c", i % 251 }' > big
printf 'spaced\n' > 'a name'
printf 'dash\n' > ./-r

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 env PATH="$bindir/$side" sum "$@" ) <"$INPUT" >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 env PATH="$bindir/$side" sum "$@" ) <"$INPUT" >"$out" 2>"$err"
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
  printf 'sum %s%s%s' "$*" "${INPUT:+ <$(basename "$INPUT")}" "${TO_FULL:+  [>/dev/full]}"
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
  TO_FULL=; INPUT=/dev/null
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
  TO_FULL=; INPUT=/dev/null
  return 0
}

# --- both algorithms over every fixture, from a file and from stdin ----------------
for f in empty one hello ff512 ff513 b1024 b1025 big; do
  run_case "$f"
  run_case -s "$f"
  INPUT=$fx/$f; run_case
  INPUT=$fx/$f; run_case -s
done

# --- the name rule -------------------------------------------------------------------
INPUT=$fx/hello; run_case -
INPUT=$fx/hello; run_case -s -
INPUT=$fx/hello; run_case - one
run_case hello big
run_case -s hello big empty
run_case 'a name'
run_case -- -r
run_case ./-r

# --- -r, -s, and the long spelling -----------------------------------------------------
run_case -r hello
run_case -r -s hello
run_case -s -r hello
run_case --sysv hello
run_case --sys hello
run_case -rs hello
run_case -sr hello

# --- failures --------------------------------------------------------------------------
run_case nosuch
run_case -s nosuch
run_case dir
run_case hello nosuch big
run_case nosuch dir
run_case -x
run_case --nope
run_case --sysv=1 hello
TO_FULL=1; run_case hello
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
