#!/usr/bin/env bash
# Differential test: our `flock` against util-linux 2.39.3's.
#
# What is compared: stdout, stderr and the exit status, for the options and
# their refusals, running a command (its status, a signal's 128+N, `-c` and
# `$SHELL`, a command that cannot be run, `-F`), which descriptors the command
# inherits (`-o`), a directory, a file that cannot be opened, a descriptor
# number, and -- with a lock held by a neutral third party, a Python
# `fcntl.flock` -- `-n`, `-w`, `-E`, shared against exclusive, and `--verbose`.
#
# The one timing figure, `--verbose`'s "getting lock took S.UUUUUU seconds",
# is compared as a shape. Our `-w` waits by polling `LOCK_NB` where upstream's
# blocks under a timer (main.rs, "What is not upstream's"); the cases here
# are what that must not change: the lock as soon as it is free, the conflict
# status at the deadline, `-w 0` as `-n`, and upstream's refusal of a timeout
# its timer cannot take.
set -u

DIFF_PROG='flock'
DIFF_PKG='flock'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

F=$DIFF_TMP/lockme
D=$DIFF_TMP/lockdir
mkdir -p "$D"
: > "$F"

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL=C.UTF-8 "${ENVS[@]}" PATH="$bindir/$side:$PATH" \
    timeout -k 2 20 flock "$@"
}
# $1 = side, $2 = a bash script that runs `flock` from that side's PATH.
run_side_sh() {
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$1:$PATH" timeout -k 2 20 bash -c "$2"
}

# The one figure that differs run to run.
normalize() {
  sed -E 's/getting lock took [0-9]+\.[0-9]{6} seconds/getting lock took N seconds/'
}

# Compare the outputs a side left in $DIFF_TMP/{o,g}.{out,err}.
judge() {
  local o_rc=$1 g_rc=$2
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif [ "$(normalize < "$DIFF_TMP/o.out")" = "$(normalize < "$DIFF_TMP/g.out")" ] \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(cat "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" \
    "$g_rc" "$(cat "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

ENVS=()
# case_ ARGV...
case_() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "${ENVS[*]:+${ENVS[*]} }flock $(printf '%q ' "$@")"
}

# sh_case SCRIPT: a bash script calling `flock`, run for each side.
sh_case() {
  local o_rc g_rc
  run_side_sh ours "$1" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_sh gnu "$1" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "bash -c $(printf '%q' "$1")"
}

# A third party holding flock(MODE) on a file, from before the case runs
# until SECS seconds later. `hold` starts it and waits until it holds.
hold() {
  local mode=$1 secs=$2 file=$3
  rm -f "$file.held"
  python3 -c '
import fcntl, sys, time
f = open(sys.argv[2], "a")
fcntl.flock(f, fcntl.LOCK_EX if sys.argv[1] == "ex" else fcntl.LOCK_SH)
open(sys.argv[2] + ".held", "w").close()
time.sleep(float(sys.argv[3]))
' "$mode" "$file" "$secs" & holder=$!
  for _ in {1..200}; do [ -e "$file.held" ] && return 0; sleep 0.02; done
  return 1
}
# held_case MODE SECS ARGV...: each side runs against a fresh holder.
held_case() {
  local mode=$1 secs=$2 o_rc g_rc; shift 2
  hold "$mode" "$secs" "$F" || { AGREED=broken; REPORT='  the holder never held'; report "held: $*"; return 0; }
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  kill "$holder" 2>/dev/null; wait "$holder" 2>/dev/null
  hold "$mode" "$secs" "$F" || { AGREED=broken; REPORT='  the holder never held'; report "held: $*"; return 0; }
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  kill "$holder" 2>/dev/null; wait "$holder" 2>/dev/null
  judge "$o_rc" "$g_rc"
  report "held $mode ${secs}s: flock $(printf '%q ' "$@")"
}

# --- options and their refusals ------------------------------------------------------
case_
case_ -h
case_ --help
case_ -V
case_ --version
case_ '-?'
case_ -z "$F" true
case_ --bogus "$F" true
case_ --n "$F" true
case_ --no "$F" true
case_ --nonb "$F" true
case_ --nb "$F" true
case_ --wai 1 "$F" true
case_ -w
case_ -w abc "$F" true
case_ -w 1x "$F" true
case_ -w '' "$F" true
case_ -w 1e5000 "$F" true
case_ -E abc "$F" true
case_ -E 256 "$F" true
case_ -E -1 "$F" true
case_ -E 99999999999 "$F" true
case_ -E "it's" "$F" true
case_ -o -F "$F" true
case_ -s
case_ -- "$F" true
case_ -x -s -u "$F" true
case_ "$F" -c
case_ "$F" -c a b
case_ "$F" --command
case_ abc
case_ 99999999999
case_ ''

# --- running a command ---------------------------------------------------------------
case_ "$F" true
case_ "$F" false
case_ "$F" sh -c 'exit 7'
case_ "$F" sh -c 'kill -TERM $$'
case_ "$F" -c 'echo hi; exit 3'
case_ "$F" --command 'echo "$0"'
ENVS=(SHELL=/bin/sh)
case_ "$F" -c 'echo "$0"'
ENVS=(SHELL=)
case_ "$F" -c 'echo "$0"'
ENVS=()
case_ "$F" nosuchcommand
case_ "$F" /
case_ -F "$F" true
case_ -F "$F" sh -c 'exit 4'
case_ -F "$F" nosuchcommand
case_ "$D" echo directory
# Started with SIGCHLD ignored, flock resets it before forking, or it could
# not collect the command's status.
sh_case "python3 -c 'import os, signal, sys; signal.signal(signal.SIGCHLD, signal.SIG_IGN); os.execvp(\"flock\", [\"flock\", sys.argv[1], \"sh\", \"-c\", \"exit 5\"])' $(printf '%q' "$F"); echo \$?"
case_ /nonexistent/dir/lock true
case_ "$F" echo "-c" x

# --- what the command inherits ---------------------------------------------------------
PROBE='ls -l /proc/$$/fd | grep -c lockme'
case_ "$F" sh -c "$PROBE"
case_ -o "$F" sh -c "$PROBE"
case_ -F "$F" sh -c "$PROBE"

# --- a descriptor --------------------------------------------------------------------------
sh_case "exec 9>>$(printf '%q' "$F"); flock -n 9; echo \$?; flock -u 9; echo \$?"
sh_case "exec 9<$(printf '%q' "$F"); flock -s 9; echo \$?"
sh_case 'flock 9; echo $?'
sh_case 'flock -n 0; echo $?'

# --- against a lock someone else holds ------------------------------------------------------
held_case ex 3 -n "$F" true
held_case ex 3 -n -E 42 "$F" true
held_case ex 3 --verbose -n "$F" true
held_case ex 3 -w 0.3 "$F" true
held_case ex 3 --verbose -w 0.2 "$F" true
held_case ex 3 -w 0 "$F" true
held_case ex 3 -w 0.0000001 "$F" true
held_case ex 1 -w 5 "$F" echo got
held_case ex 1 "$F" echo got
held_case sh 3 -s -n "$F" echo shared
held_case sh 3 -x -n "$F" echo exclusive
held_case ex 3 -u "$F" echo unlocked
held_case ex 3 -w -1 "$F" true
held_case ex 3 -w inf "$F" true
held_case ex 3 -w 1e30 "$F" true
held_case ex 3 -w -0.5 "$F" true

# --- --verbose, and stdout ------------------------------------------------------------------
case_ --verbose "$F" echo hi
case_ --verbose -F "$F" echo hi
case_ --verbose "$F"
if [ -w /dev/full ]; then
  full_case() {
    local o_rc g_rc
    run_side ours "$@" >/dev/full 2>"$DIFF_TMP/o.err"; o_rc=$?
    run_side gnu "$@" >/dev/full 2>"$DIFF_TMP/g.err"; g_rc=$?
    : >"$DIFF_TMP/o.out"; : >"$DIFF_TMP/g.out"
    judge "$o_rc" "$g_rc"
    report "to /dev/full: flock $(printf '%q ' "$@")"
  }
  full_case --verbose "$F" true
  full_case -V
fi

echo "flock-diff: $pass passed, $fail failed, $broken broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
