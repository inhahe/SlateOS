#!/usr/bin/env bash
# Differential test: our `sleep` against GNU coreutils 9.4's.
#
# `sleep`'s output is a pause, so beside what each side printed and said and
# its status, every case compares how long it took: in tenths of a second,
# within two tenths of each other, which is far tighter than the shortest
# difference that matters (an operand ignored, a suffix read as seconds) and
# loose enough for a busy machine.
#
# ## What it covers
#
#   * **the number** -- read as `strtod` reads one: decimals, exponents, hex,
#     leading whitespace, `inf`; and refused as upstream refuses it, every bad
#     operand named before one referral.
#   * **the suffixes** -- `s`, `m`, `h`, `d`, and what is not one.
#   * **several operands** -- summed.
#   * **the options** -- `--help` and `--version` wherever they appear; any
#     other option refused; `--` alone a zero pause.
#   * **the descriptors** -- standard output full and closed (`sleep` writes
#     nothing there but its help), standard error full and closed.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='sleep'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
# Seconds a case may run before `timeout` ends it: `sleep inf` is meant to
# reach this, as both sides should.
CASE_LIMIT=20

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

now_ds() { date +%s%N | cut -c1-11; }   # deciseconds since the epoch

# `run_side SIDE REDIR BIN ERR ARGS...`: one side, with one descriptor as REDIR
# says, leaving its run time in tenths of a second in ELAPSED.
run_side() {
  local side=$1 redir=$2 bin=$3 err=$4; shift 4
  local start rc
  start=$(now_ds)
  case $redir in
    '')
      diff_run timeout -k 2 "$CASE_LIMIT" env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        sleep "$@" </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      diff_run timeout -k 2 "$CASE_LIMIT" env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        sleep "$@" </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      diff_run timeout -k 2 "$CASE_LIMIT" env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        sleep "$@" </dev/null >&- 2>"$err" ;;
    '<&-')
      diff_run timeout -k 2 "$CASE_LIMIT" env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        sleep "$@" <&- >"$bin" 2>"$err" ;;
    '2>/dev/full')
      diff_run timeout -k 2 "$CASE_LIMIT" env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        sleep "$@" </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      diff_run timeout -k 2 "$CASE_LIMIT" env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        /bin/sh -c 'exec 2>&-; exec sleep "$@"' sh "$@" </dev/null >"$bin" ;;
    *) echo "sleep-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
  rc=$?
  ELAPSED=$(( $(now_ds) - start ))
  return "$rc"
}

compare_with() {
  local redir=$1; shift
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin o_t g_t
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$redir" "$o_bin" "$o_err" "$@"; o_rc=$?; o_t=$ELAPSED
  run_side gnu  "$redir" "$g_bin" "$g_err" "$@"; g_rc=$?; g_t=$ELAPSED
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  local dt=$(( o_t - g_t ))
  [ "$dt" -lt 0 ] && dt=$(( -dt ))
  # Only the time disagrees: run both again, twice at most, and compare each
  # side's quickest. A busy machine can only make a run slower -- one case
  # once took GNU 3.8 s for `sleep 0.1 -- 0.1` while another lane wrote a
  # disk image in WSL -- so the minimum is the measurement, and a real
  # difference (an operand ignored) survives every retry.
  local try t scratch_out scratch_err
  for try in 1 2; do
    [ "$dt" -le 2 ] && break
    [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ] || break
    scratch_out=$(mktemp); scratch_err=$(mktemp)
    run_side ours "$redir" "$scratch_out" "$scratch_err" "$@"; t=$ELAPSED
    [ "$t" -lt "$o_t" ] && o_t=$t
    run_side gnu "$redir" "$scratch_out" "$scratch_err" "$@"; t=$ELAPSED
    [ "$t" -lt "$g_t" ] && g_t=$t
    rm -f "$scratch_out" "$scratch_err"
    dt=$(( o_t - g_t ))
    [ "$dt" -lt 0 ] && dt=$(( -dt ))
  done
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ] \
     && [ "$dt" -le 2 ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s, %s.%ss): %s {%s}\n  gnu  (rc=%s, %s.%ss): %s {%s}' \
    "$o_rc" "$(( o_t / 10 ))" "$(( o_t % 10 ))" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" \
    "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(( g_t / 10 ))" "$(( g_t % 10 ))" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" \
    "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

run_case() { compare_with '' "$@"; report "sleep $*"; }

fd_case() {
  local redir=$1; shift
  compare_with "$redir" "$@"
  report "sleep $* $redir"
}

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS sleep %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail sleep %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the number ----------------------------------------------------------------------
for n in 0 0.0 .2 0.2 0.25 2e-1 2E-1 0x0.4p0 0X.4P0 0x0 ' 0.1' '	0.1' +0.1 00.1 \
         0.1000000000000000000000001 1e-400 0e999999 -0 -0.0 '+.1' 0.; do
  run_case "$n"
done
# Not numbers, or not ones `sleep` takes.
for n in '' ' ' abc 0.1x 1..2 -1 -0.1 1e 0x 0xg . + - nan NaN -inf 0.1s1 '0.1 ' \
         0,1 1/2 0x1p 0b1 ０.1; do
  run_case -- "$n"
done
run_case -- -1 abc 0.1
run_case abc def

# --- the suffixes ------------------------------------------------------------------------
for n in 0.2s 0s 0m 0h 0d 0.003m 0.00005h 0.000002d .2S 0.1ms 0.1q 0.1M 0.1x 1e-2m 0x0.4ps; do
  run_case -- "$n"
done

# --- several operands --------------------------------------------------------------------
run_case 0.1 0.1
run_case 0.1 0.001m 0
run_case 0 0 0 0 0
run_case 0.1 abc 0.1
run_case 0.1 -- 0.1

# --- forever ---------------------------------------------------------------------------
CASE_LIMIT=1 run_case inf
CASE_LIMIT=1 run_case INF
CASE_LIMIT=1 run_case infinity
CASE_LIMIT=1 run_case 0 inf
CASE_LIMIT=1 run_case infs
CASE_LIMIT=1 run_case 1e999

# --- the options ---------------------------------------------------------------------------
run_case
run_case --
run_case -- --
run_case -x
run_case -1
run_case --nosuch
run_case --help=x
run_case abc --nosuch
run_case 0.1 --nosuch

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' 0
fd_case '>&-' 0
fd_case '<&-' 0.1
fd_case '2>/dev/full' 0
fd_case '2>/dev/full' abc
fd_case '2>/dev/full' -x
fd_case '2>&-' 0
fd_case '2>&-' abc
fd_case '>/dev/full' --help
fd_case '>&-' --help
fd_case '>/dev/full' --version
fd_case '>&-' --version

# --- the ones whose text is ours -----------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version
xfail_case "our help text: --help wins wherever it is" 0.1 0.2 --help
xfail_case "our help text: --help wins over a bad operand" abc --help
xfail_case "our version string: the first of the two wins" --version --help
xfail_case "our help text: --he is --help" --he
xfail_case "our version string: --vers is --version" --vers

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
