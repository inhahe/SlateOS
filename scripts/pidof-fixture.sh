#!/bin/bash
# pidof-fixture.sh -- the init of a PID namespace running one pidof-diff case.
#
# One case of scripts/pidof-diff.sh, run as a fresh PID namespace's init
# (pid 1), with a /proc of its own.
#
# Usage: pidof-fixture.sh FIXDIR WORKDIR STDIN PROGRAM ARGS...
#
# Starts the fixture processes -- always the same ones, in the same order,
# so each has the same pid on both sides -- runs PROGRAM ARGS..., and
# reports. Nothing forks while waiting for a process to be ready, nor in the
# processes themselves: a fork takes a pid, and anything that forked a
# different number of times, or at a different moment, on the two sides
# would number everything after it differently.
#
# `{N}` in an argument is the pid of the Nth fixture started, so a case can
# name one without knowing how the namespace numbered it.
#
# STDIN is `null` (complaints go to syslog, which the harness collects
# through a socket bound over /dev/log) or `tty` (a pseudo-terminal, so they
# go to standard error).
#
# Output, in order: the program's standard output, `--err` and its standard
# error, `--status N`, and for killall5 `--fates` and how each fixture ended
# (`pid name: status`, or `running`).
set -u

# killall5 stops and signals every process it can see. Seeing only these is
# what makes running it safe, and this script is pid 1 only as a new PID
# namespace's init.
if [ "$$" != 1 ]; then
  echo "pidof-fixture: not a PID namespace's init (pid $$); refusing to run" >&2
  exit 125
fi

f=$1 w=$2 stdin_kind=$3
shift 3

# Spin until /proc/PID/comm reads NAME: builtins only, so no pid is taken.
await() {
  local pid=$1 name=$2 comm n=0
  while :; do
    comm=
    { read -r comm < "/proc/$pid/comm"; } 2>/dev/null
    [ "$comm" = "$name" ] && return 0
    n=$((n + 1))
    if [ "$n" -gt 5000000 ]; then
      echo "fixture: $pid never became $name" >&2
      exit 125
    fi
  done
}

# Spin until process PID is a zombie.
await_zombie() {
  local pid=$1 line n=0
  while :; do
    line=
    { read -r line < "/proc/$pid/stat"; } 2>/dev/null
    case $line in *") Z "*) return 0 ;; esac
    n=$((n + 1))
    if [ "$n" -gt 5000000 ]; then
      echo "fixture: $pid never became a zombie" >&2
      exit 125
    fi
  done
}

fixtures=()
names=()
start() {
  local name=$1; shift
  "$@" &
  fixtures+=("$!")
  names+=("$name")
  await "$!" "$name"
}

# `alpha` ignores its arguments and waits for a signal, so each fixture's
# arguments are whatever the matching under test needs. `bash -c 'exec -a
# ...'` sets an argv[0] other than the file's name; `setsid` puts a fixture in
# a session of its own -- every other one shares the harness's session, whose
# leader is outside the namespace, so killall5 leaves them alone as its own.
long_arg=$(<"$f/longarg")
start alpha "$f/alpha"                                                   # 1
start alpha bash -c 'exec -a beta "$0"' "$f/alpha"                       # 2
start alpha bash -c 'exec -a "gamma with spaces" "$0"' "$f/alpha"        # 3
start alpha bash -c 'exec -a -delta "$0"' "$f/alpha"                     # 4
start alpha bash -c 'exec -a -delta "$0" 600' "$f/alpha"                 # 5
start myscript.sh "$f/myscript.sh"                                       # 6
start doomed "$f/doomed"                                                 # 7
start alpha setsid "$f/alpha"                                            # 8
start alpha setsid bash -c 'exec -a "$1" "$0"' "$f/alpha" "$f/longname"  # 9
start alpha bash -c 'exec -a "$1" "$0" 600' "$f/alpha" "$long_arg"       # 10
start alpha bash -c 'exec -a /elsewhere/alpha "$0" -flag -x 600' "$f/alpha"  # 11
start alpha setsid bash -c 'exec -a python3 "$0" -u /opt/tool/alpha' "$f/alpha"  # 12
# 13: a zombie. zed exits at once, and its parent never waits for it. The
# child is the next pid after its parent's.
start python3 python3 -c '
import os, sys, time
if os.fork() == 0:
    os.execv(sys.argv[1], ["zed"])
time.sleep(600)
' "$f/zed"
await_zombie $((${fixtures[-1]} + 1))
rm -f "$f/doomed"

prog=$1; shift
args=()
for a in "$@"; do
  for i in "${!fixtures[@]}"; do
    a=${a//"{$((i + 1))}"/${fixtures[$i]}}
  done
  args+=("$a")
done

out=$w/run.out err=$w/run.err
case $stdin_kind in
  tty)
    python3 "$f/ttyrun.py" "$out" "$err" "$prog" "${args[@]}"
    status=$?
    ;;
  *)
    "$prog" "${args[@]}" </dev/null >"$out" 2>"$err"
    status=$?
    ;;
esac
cat "$out"
printf '\n--err\n'
cat "$err"
printf -- '--status %s\n' "$status"

case $prog in
  *killall5)
    # A signal is delivered when its target next runs, so give any that is
    # dying the time to: `read` timing out on a FIFO nothing writes, which
    # takes no pid as `sleep` would.
    read -rt 0.3 <>"$f/nap.fifo"
    printf -- '--fates\n'
    i=0
    for pid in "${fixtures[@]}"; do
      state=
      { read -r state < "/proc/$pid/stat"; } 2>/dev/null
      case $state in
        *") Z "*|"")
          # Dead: its status, which bash keeps for `wait` even when its
          # handler has already reaped it.
          wait "$pid" 2>/dev/null
          printf '%s %s: %s\n' "$pid" "${names[$i]}" "$?"
          ;;
        *) printf '%s %s: running\n' "$pid" "${names[$i]}" ;;
      esac
      i=$((i + 1))
    done
    ;;
esac
