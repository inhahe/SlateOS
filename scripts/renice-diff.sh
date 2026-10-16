#!/usr/bin/env bash
# Differential test: our `renice` against util-linux 2.39.3's.
#
# `renice` acts on running processes, so each side gets a target of its own:
# a `sleep` started at niceness 2 in a session of its own (`setsid`, so that
# its process-group ID is its PID and `-g` reaches nothing else). `@PID@` in a
# case's arguments is that target's PID. Compared: what each side printed and
# said, with the target's PID written as `PID`; its status; and the niceness
# the target was left with.
#
# Starting at 2 makes three things observable: an absolute `-n 5` (5) against
# a relative one (7); `+3` read as relative or absolute; and the refusal to
# lower a niceness without privilege (`renice 0` is `Permission denied`).
#
# ## What it never does
#
# `-u` with a real user: that renices every process the user owns -- this
# session's included. `-u` is exercised only on names that do not exist.
#
# ## The reference
#
# util-linux 2.39.3 as the distribution installs it (design-decisions §622
# says why util-linux rather than BSD).
set -u

DIFF_PROG='renice'
DIFF_REF='/usr/bin/renice'
DIFF_NEED='timeout setsid nice ps'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "renice-diff: refusing to run as root, where the refusals it measures" >&2
  echo "  are granted." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
CASE_ENV=

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# A target: `sleep` at niceness 2, leader of its own session. Its PID on
# stdout.
spawn_target() {
  nice -n 2 setsid sleep 60 </dev/null >/dev/null 2>&1 &
  local pid=$!
  # `setsid` execs `sleep` in place when it is not a group leader already,
  # so `$!` is the sleep itself; wait until it is visibly asleep at 2.
  local tries=0
  while [ "$(ps -o ni= -p "$pid" 2>/dev/null | tr -d ' ')" != 2 ] && [ "$tries" -lt 50 ]; do
    tries=$((tries+1)); sleep 0.02
  done
  printf '%s' "$pid"
}

# Kill only the target this harness started, by its PID.
reap_target() {
  kill "$1" 2>/dev/null
  wait "$1" 2>/dev/null
}

# `run_side SIDE REDIR BIN ERR PID ARGS...`: one side against target PID.
run_side() {
  local side=$1 redir=$2 bin=$3 err=$4 pid=$5; shift 5
  local args=() a
  for a in "$@"; do args+=("${a//@PID@/$pid}"); done
  case $redir in
    '')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 ${CASE_ENV:+"$CASE_ENV"} PATH="$bindir/$side" \
        renice "${args[@]}" </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        renice "${args[@]}" </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        renice "${args[@]}" </dev/null >&- 2>"$err" ;;
    '2>/dev/full')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        renice "${args[@]}" </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        /bin/sh -c 'exec 2>&-; exec renice "$@"' sh "${args[@]}" </dev/null >"$bin" ;;
    # Both into one file: util-linux's `warn` does not flush standard output
    # first, as gnulib's `error` does, so a failure lands before the success
    # lines its buffer still holds.
    '2>&1')
      diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        renice "${args[@]}" </dev/null >"$bin" 2>&1 ;;
    *) echo "renice-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
}

compare_with() {
  local redir=$1; shift
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin o_pid g_pid o_ni g_ni
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  o_pid=$(spawn_target)
  run_side ours "$redir" "$o_bin" "$o_err" "$o_pid" "$@"; o_rc=$?
  o_ni=$(ps -o ni= -p "$o_pid" 2>/dev/null | tr -d ' ')
  reap_target "$o_pid"
  g_pid=$(spawn_target)
  run_side gnu "$redir" "$g_bin" "$g_err" "$g_pid" "$@"; g_rc=$?
  g_ni=$(ps -o ni= -p "$g_pid" 2>/dev/null | tr -d ' ')
  reap_target "$g_pid"
  # The target's PID differs between the sides; it is the one thing that may.
  o_out=$(sed "s/\\b$o_pid\\b/PID/g" "$o_bin" | od -An -c)
  g_out=$(sed "s/\\b$g_pid\\b/PID/g" "$g_bin" | od -An -c)
  local o_msg g_msg
  o_msg=$(sed "s/\\b$o_pid\\b/PID/g" "$o_err"); g_msg=$(sed "s/\\b$g_pid\\b/PID/g" "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ] \
     && [ "$o_ni" = "$g_ni" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s, ni=%s): %s {%s}\n  gnu  (rc=%s, ni=%s): %s {%s}' \
    "$o_rc" "$o_ni" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-200)" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$g_ni" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-200)" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

run_case() { compare_with '' "$@"; report "${CASE_ENV:+$CASE_ENV }renice $*"; }
fd_case() { local redir=$1; shift; compare_with "$redir" "$@"; report "renice $* $redir"; }

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS renice %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail renice %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- setting a priority -------------------------------------------------------------------
for prio in 5 2 19 20 2147483647 0 1 -5 ' 5' +5 '' 99999999999999999999 -99999999999999999999 007; do
  run_case "$prio" @PID@
  run_case "$prio" -p @PID@
done
run_case 5 -g @PID@
run_case 5 --pgrp @PID@
run_case 5 --pid @PID@
run_case 5 -g -p @PID@
run_case 5 -p -g @PID@
run_case 5 @PID@ @PID@
run_case --priority 5 @PID@
run_case --priority=5 @PID@
run_case --prio 5 @PID@

# --- relative and absolute ------------------------------------------------------------------
run_case -n 5 @PID@
run_case -n 3 -p @PID@
run_case -n -1 @PID@
run_case --relative 3 @PID@
run_case --relative=3 @PID@
run_case --rel 3 @PID@
run_case --relative -1 @PID@
CASE_ENV=POSIXLY_CORRECT=1 run_case -n 5 @PID@
CASE_ENV=POSIXLY_CORRECT=1 run_case -n 3 -g @PID@
CASE_ENV=POSIXLY_CORRECT=1 run_case 5 @PID@

# --- what is refused -------------------------------------------------------------------------
for prio in abc ' ' 0x10 1.5 5x -- -h -x; do
  run_case "$prio" @PID@
done
run_case 5 abc
run_case 5 -g abc
run_case 5 -u nosuchuser_slateos
run_case 5 --user nosuchuser_slateos
run_case 5 -1
run_case 5 2147483648
run_case 5 99999999
run_case 5 @PID@ 99999999
run_case 5 99999999 @PID@
run_case 0 4294967296
run_case
run_case 5
run_case -n
run_case -n 5
run_case --priority
run_case 5 -p
run_case 5 --nosuch @PID@
run_case --nosuch

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' 5 @PID@
fd_case '>&-' 5 @PID@
fd_case '>/dev/full' 5 99999999
fd_case '2>/dev/full' 5 99999999
fd_case '2>/dev/full' 5 @PID@
fd_case '2>&-' 5 99999999
fd_case '2>&-' 5 @PID@
fd_case '2>&1' 5 @PID@ 99999999
fd_case '2>&1' 5 99999999 @PID@
fd_case '2>&1' 5 @PID@ abc

# --- the help, which is upstream's word for word ------------------------------------------
run_case -h
run_case --help
fd_case '>/dev/full' -h
fd_case '>&-' -h

# --- the ones whose text is ours -----------------------------------------------------------
xfail_case "our version string" -v
xfail_case "our version string" -V
xfail_case "our version string" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
