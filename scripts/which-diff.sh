#!/usr/bin/env bash
# Differential test: our `which` against GNU which 2.21 (`gnu-which`).
#
# `which`'s inputs are its environment as much as its arguments -- `PATH`,
# `HOME`, the current directory -- and its answers are absolute paths. So each
# case runs both sides one after the other in the same directory, rebuilt
# between them, under an environment the case spells out completely (`env -i`
# plus what the case names), and compares what they printed, what they said
# and their status.
#
# Both are started as `which` (`exec -a`): GNU which names itself from
# `argv[0]` verbatim, so the reference's own path would otherwise be in every
# message.
#
# ## The reference
#
# Not GNU coreutils, so no built reference: the distribution's GNU which 2.21,
# which `gnu-which` installs as `/usr/bin/which.gnu` (Debian's default `which`
# is debianutils' shell script, a different program).
#
# ## Where ours differs on purpose
#
# The departures `known-issues/B-WHICH-DIVERGES-FROM-GNU-IN-FOUR-MEASURED-PLACES.md`
# records, each a case where copying GNU would ship a `which` that lies: an
# absolute command with `PATH` unset, a command directly under `/`, an option
# that is not one (unknown, ambiguous, or given a value it does not take)
# reported and then ignored, and a failed write never checked. They are
# expected differences here, so a change that makes either side agree is
# reported.
set -u

DIFF_PROG='which'
DIFF_REF='/usr/bin/which.gnu'
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built afresh for every run ------------------------------------------
c=$work/case
build_tree() {
  rm -rf "$c"
  mkdir -p "$c/bin/cmd2" "$c/bin2" "$c/home/bin" "$c/homex/bin" "$c/.dot/bin" "$c/sub/bin"
  printf '#!/bin/sh\n' > "$c/bin/cmd";      chmod 755 "$c/bin/cmd"
  printf '#!/bin/sh\n' > "$c/bin/plain";    chmod 644 "$c/bin/plain"
  printf '#!/bin/sh\n' > "$c/bin/xonly";    chmod 111 "$c/bin/xonly"
  mkfifo "$c/bin/fifo";                     chmod 755 "$c/bin/fifo"
  ln -s cmd "$c/bin/link"
  ln -s nowhere "$c/bin/dangling"
  printf '#!/bin/sh\n' > "$c/bin2/cmd";     chmod 755 "$c/bin2/cmd"
  printf '#!/bin/sh\n' > "$c/bin2/only2";   chmod 755 "$c/bin2/only2"
  printf '#!/bin/sh\n' > "$c/home/bin/hcmd"; chmod 755 "$c/home/bin/hcmd"
  printf '#!/bin/sh\n' > "$c/homex/bin/xcmd"; chmod 755 "$c/homex/bin/xcmd"
  printf '#!/bin/sh\n' > "$c/.dot/bin/dcmd"; chmod 755 "$c/.dot/bin/dcmd"
  printf '#!/bin/sh\n' > "$c/sub/bin/scmd"; chmod 755 "$c/sub/bin/scmd"
  printf '#!/bin/sh\n' > "$c/$(printf 'bin/nl\nname')"; chmod 755 "$c/$(printf 'bin/nl\nname')"
}

# `run_side SIDE REDIR BIN ERR ENV-ASSIGNMENTS... -- ARGS...`: one side in the
# case directory, its environment exactly the assignments given.
run_side() {
  local side=$1 redir=$2 bin=$3 err=$4; shift 4
  local envs=()
  while [ "$#" -gt 0 ] && [ "$1" != -- ]; do envs+=("$1"); shift; done
  [ "$#" -gt 0 ] && shift
  local exe="$bindir/$side/which"
  local launch=(env -i LC_ALL=C.UTF-8 "${envs[@]}" /bin/bash -c 'exec -a which "$0" "$@"' "$exe" "$@")
  case $redir in
    '')          ( cd "$c" && diff_run timeout -k 2 20 "${launch[@]}" ) </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full') ( cd "$c" && diff_run timeout -k 2 20 "${launch[@]}" ) </dev/null >/dev/full 2>"$err" ;;
    '>&-')       ( cd "$c" && diff_run timeout -k 2 20 "${launch[@]}" ) </dev/null >&- 2>"$err" ;;
    '2>/dev/full') ( cd "$c" && diff_run timeout -k 2 20 "${launch[@]}" ) </dev/null >"$bin" 2>/dev/full ;;
    *) echo "which-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
}

compare_with() {
  local redir=$1; shift
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  build_tree
  run_side ours "$redir" "$o_bin" "$o_err" "$@"; o_rc=$?
  build_tree
  run_side gnu  "$redir" "$g_bin" "$g_err" "$@"; g_rc=$?
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
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-200)" "$(printf '%s' "$o_msg" | tr '\n' '|' | cut -c1-300)" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-200)" "$(printf '%s' "$g_msg" | tr '\n' '|' | cut -c1-300)")
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

# `run_case ENV... -- ARGS...`, `fd_case REDIR ENV... -- ARGS...`.
run_case() { compare_with '' "$@"; report "$*"; }
fd_case() { local redir=$1; shift; compare_with "$redir" "$@"; report "$* $redir"; }

xfail_case() {
  local why=$1; shift
  xfail_fd_case "$why" '' "$@"
}

# `xfail_fd_case WHY REDIR ENV... -- ARGS...`: an expected difference with
# one descriptor changed.
xfail_fd_case() {
  local why=$1 redir=$2; shift 2
  compare_with "$redir" "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n' "$*" "$why"
  fi
  return 0
}

P="PATH=$c/bin"
P2="PATH=$c/bin:$c/bin2"
H="HOME=$c/home"

# --- finding a command -------------------------------------------------------------------
for cmd in cmd plain xonly fifo cmd2 link dangling nosuch only2 '' . .. "$(printf 'nl\nname')"; do
  run_case "$P" "$H" -- "$cmd"
done
run_case "$P2" "$H" -- cmd
run_case "$P2" "$H" -- -a cmd
run_case "$P2" "$H" -- --all cmd
run_case "$P2" "$H" -- -a cmd only2 nosuch
run_case "$P2" "$H" -- cmd only2
run_case "$P2" "$H" -- nosuch1 nosuch2
run_case "$P2" "$H" -- nosuch1 cmd nosuch2 nosuch3

# --- how PATH is read ----------------------------------------------------------------------
run_case "PATH=" "$H" -- cmd
run_case "PATH=:" "$H" -- cmd
run_case "PATH=::" "$H" -- cmd
run_case "$H" -- cmd
run_case "PATH=bin" "$H" -- cmd
run_case "PATH=./bin" "$H" -- cmd
run_case "PATH=sub/../bin" "$H" -- cmd
run_case "PATH=$c/./bin" "$H" -- cmd
run_case "PATH=$c//bin" "$H" -- cmd
run_case "PATH=/$c/bin" "$H" -- cmd
run_case "PATH=$c/bin/" "$H" -- cmd
run_case "PATH=$c/nosuchdir:$c/bin" "$H" -- cmd
run_case "PATH=$c/bin:" "$H" -- cmd
run_case "PATH=:$c/bin" "$H" -- cmd
run_case "PATH=~/bin" "$H" -- hcmd
run_case "PATH=~" "$H" -- bin
run_case "PATH=~/bin" -- hcmd
run_case "PATH=.dot/bin" "$H" -- dcmd

# --- a command with a slash in it ------------------------------------------------------------
run_case "$P" "$H" -- bin/cmd
run_case "$P" "$H" -- ./bin/cmd
run_case "$P" "$H" -- bin/nosuch
run_case "$P" "$H" -- "$c/bin/cmd"
run_case "$P" "$H" -- "$c/bin/plain"
run_case "$P" "$H" -- bin/
run_case "$P" "$H" -- sub/bin/scmd

# --- the show and skip options --------------------------------------------------------------
run_case "PATH=./bin" "$H" -- --show-dot cmd
run_case "PATH=bin" "$H" -- --show-dot cmd
run_case "PATH=./bin:$c/bin2" "$H" -- --skip-dot cmd
run_case "PATH=bin:$c/bin2" "$H" -- --skip-dot cmd
run_case "PATH=.dot/bin" "$H" -- --skip-dot dcmd
run_case "PATH=~/bin" "$H" -- --skip-tilde hcmd
run_case "PATH=$c/home/bin" "$H" -- --skip-tilde hcmd
run_case "PATH=$c/home/bin" "$H" -- --show-tilde hcmd
run_case "PATH=~/bin" "$H" -- --show-tilde hcmd
run_case "PATH=./bin" "$H" -- --tty-only --show-dot cmd
run_case "PATH=./bin" "$H" -- --show-dot --tty-only cmd
run_case "$P" "$H" -- --skip-alias cmd
run_case "$P" "$H" -- --skip-functions cmd
run_case "$P" "$H" -- --read-alias --skip-alias cmd
run_case "$P" "$H" -- -i --skip-alias cmd

# --- the options ---------------------------------------------------------------------------
run_case "$P" "$H" -- -- cmd
run_case "$P" "$H" -- cmd --
xfail_case "an option given a value it does not take stops the run" "$P" "$H" -- --all=x cmd
xfail_case "an unknown option stops the run" "$P" "$H" -- --nosuch cmd
xfail_case "an ambiguous option stops the run" "$P" "$H" -- --show cmd
xfail_case "an ambiguous option stops the run" "$P" "$H" -- --skip cmd

# --- the descriptors -----------------------------------------------------------------------
xfail_fd_case "a failed write is reported" '>/dev/full' "$P" "$H" -- cmd
xfail_fd_case "a failed write is reported" '>&-' "$P" "$H" -- cmd
fd_case '2>/dev/full' "$P" "$H" -- nosuch
fd_case '>/dev/full' "$P" "$H" -- nosuch

# --- the four departures which.rs documents, and the text that is ours -----------------------
xfail_case "PATH unset: an absolute command is not looked for in PATH" "$H" -- "$c/bin/cmd"
xfail_case "a command under / is searched for in /, not in nothing" "$P" "$H" -- /init
# Once listed as a departure; measured, GNU compares whole components too.
run_case "PATH=$c/homex/bin" "$H" -- --skip-tilde xcmd
xfail_case "an unknown option stops the run" "$P" "$H" -- -Z cmd
xfail_case "our help text" "$P" "$H" -- --help
xfail_case "our version string" "$P" "$H" -- --version
xfail_case "our version string, -v" "$P" "$H" -- -v
xfail_case "our version string, -V" "$P" "$H" -- -V
xfail_case "our help text, for no command at all" "$P" "$H" --
xfail_case "our help text: --help wins wherever it is" "$P" "$H" -- cmd --help

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
rm -rf "$c"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
