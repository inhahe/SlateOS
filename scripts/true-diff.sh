#!/usr/bin/env bash
# Differential test: our `true` and `false` against GNU coreutils 9.4's.
#
# One harness for both because upstream is one program for both: `false.c`
# is `#define EXIT_STATUS EXIT_FAILURE` and `#include "true.c"`. What there is
# to measure is small and easy to get wrong:
#
#   * every argument is ignored -- options too, `-x` and `--nosuch` alike --
#     except `--help` and `--version`, and those only when spelled out in full
#     and *alone*: `true --he`, `true --help x` and `true x --help` all do
#     nothing.
#   * `--help` and `--version` are the only paths that write, and they are
#     followed by `close_stdout`, so a full or closed standard output turns
#     even `true --help` into `write error`, status 1.
#   * `false --help` prints the help and still exits 1.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='true'
DIFF_BINS='true false'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# `run_side PROG SIDE REDIR BIN ERR ARGS...`: one side, with one descriptor as
# REDIR says.
run_side() {
  local prog=$1 side=$2 redir=$3 bin=$4 err=$5; shift 5
  case $redir in
    '')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        "$prog" "$@" </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        "$prog" "$@" </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        "$prog" "$@" </dev/null >&- 2>"$err" ;;
    '<&-')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        "$prog" "$@" <&- >"$bin" 2>"$err" ;;
    '2>/dev/full')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        "$prog" "$@" </dev/null >"$bin" 2>/dev/full ;;
    # Both full: the `write error` itself has nowhere to go.
    '>/dev/full 2>/dev/full')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        "$prog" "$@" </dev/null >/dev/full 2>/dev/full ;;
    '2>&-')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        /bin/sh -c 'p=$1; shift; exec 2>&-; exec "$p" "$@"' sh "$prog" "$@" \
        </dev/null >"$bin" ;;
    *) echo "true-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
}

compare_with() {
  local prog=$1 redir=$2; shift 2
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  run_side "$prog" ours "$redir" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side "$prog" gnu  "$redir" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s {%s}\n  gnu  (rc=%s): %s {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-120)" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-120)" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

# `run_case PROG ARGS...` and `fd_case PROG REDIR ARGS...`.
run_case() { local prog=$1; shift; compare_with "$prog" '' "$@"; report "$prog $*"; }
fd_case() {
  local prog=$1 redir=$2; shift 2
  compare_with "$prog" "$redir" "$@"
  report "$prog $* $redir"
}

xfail_case() {
  local why=$1 prog=$2; shift 2
  compare_with "$prog" '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s %s -- expected to differ (%s) and did not\n' "$prog" "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s %s (%s)\n' "$prog" "$*" "$why"
  fi
  return 0
}

for prog in true false; do
  # --- arguments, all ignored -----------------------------------------------------------
  run_case "$prog"
  run_case "$prog" x
  run_case "$prog" x y z
  run_case "$prog" -x
  run_case "$prog" -h
  run_case "$prog" -v
  run_case "$prog" -
  run_case "$prog" --
  run_case "$prog" ''
  run_case "$prog" --nosuch
  run_case "$prog" --he
  run_case "$prog" --hel
  run_case "$prog" --vers
  run_case "$prog" --HELP
  run_case "$prog" --help=x
  run_case "$prog" --help x
  run_case "$prog" x --help
  run_case "$prog" --help --help
  run_case "$prog" --version x
  run_case "$prog" -- --help
  run_case "$prog" --help --version
  run_case "$prog" "$(printf 'bad\377arg')"

  # --- the descriptors, quiet ------------------------------------------------------------
  fd_case "$prog" '>/dev/full'
  fd_case "$prog" '>&-'
  fd_case "$prog" '<&-'
  fd_case "$prog" '2>/dev/full'
  fd_case "$prog" '2>&-'
  fd_case "$prog" '>/dev/full' x --help

  # --- the descriptors, with something to write -------------------------------------------
  fd_case "$prog" '>/dev/full' --help
  fd_case "$prog" '>&-' --help
  fd_case "$prog" '>/dev/full' --version
  fd_case "$prog" '>&-' --version
  fd_case "$prog" '>/dev/full 2>/dev/full' --help
  fd_case "$prog" '>/dev/full 2>/dev/full' --version

  # --- the text, which is ours ---------------------------------------------------------------
  xfail_case "our help text, not the GNU project's" "$prog" --help
  xfail_case "our version string, not the GNU project's" "$prog" --version
done

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
