#!/bin/bash
# pidof-diff.sh -- run our `pidof` and `killall5` and sysvinit's side by side
# and report every case where they disagree.
#
# ## The reference
#
# Ubuntu's sysvinit-utils 3.08-6ubuntu3: /usr/sbin/killall5, and
# /usr/bin/pidof, a link to it. None of Ubuntu's or Debian's patches touches
# `src/killall5.c`.
#
# ## Where it runs
#
# Every case runs in a fresh user and PID namespace with a /proc of its own
# (`unshare -Urpf --mount-proc`): `scripts/pidof-fixture.sh` is its init,
# starts the same processes in the same order on both sides -- so each has
# the same pid -- and runs the program among them. The program sees those
# processes and nothing else, which makes what pidof finds the same on both
# sides; and killall5, whose first act is to stop every process it may
# signal, can reach nothing outside the namespace. Inside it the program is
# root, so these are root's answers.
#
# The fixtures, under names nothing else has: `alpha`, a program that ignores
# its arguments and waits for a signal, run plainly and under other
# `argv[0]`s -- `beta`, a title with blanks, a login shell's `-delta` with and
# without arguments, a path elsewhere, an `argv[0]` longer than the 4096
# bytes pidof reads of one, and as `python3` running a script named like it;
# some in a session of their own, which are what killall5 signals;
# `myscript.sh`, a script started through its `#!`; `doomed`, whose file is
# removed once it runs; and a zombie, `zed`. `{N}` in a case's arguments is
# the pid of the Nth.
#
# What pidof and killall5 have to say goes to syslog unless standard input is
# a terminal. Each case is run both ways: with standard input /dev/null and a
# socket bound over /dev/log, whose datagrams are compared (the time in each
# masked), and with standard input a pseudo-terminal (`scripts/
# pidof-ttyrun.py`), when it goes to standard error.
#
# ## Cases that differ on purpose
#
# None.
set -u

DIFF_PROG='pidof'
DIFF_BINS='pidof killall5'
DIFF_NO_BINDIR=1
DIFF_NO_REF=1
DIFF_NEED='cc python3 timeout unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

bindir=$DIFF_TMP/bin
mkdir -p "$bindir/ours" "$bindir/gnu"
for b in pidof killall5; do
  if [ -n "$OURS" ] && [ -d "$OURS" ]; then
    ln -s "$OURS/$b" "$bindir/ours/$b" || exit 1
  else
    ln -s "$(diff_ours "$b")" "$bindir/ours/$b" || exit 1
  fi
done
ln -s /usr/bin/pidof "$bindir/gnu/pidof" || exit 1
ln -s /usr/sbin/killall5 "$bindir/gnu/killall5" || exit 1
for b in pidof killall5; do
  [ -x "$bindir/gnu/$b" ] || { echo "pidof-diff: no $b to compare with; skipping"; exit 0; }
done

if ! unshare -Urpf --mount-proc true 2>/dev/null; then
  echo "pidof-diff: cannot make a PID namespace here; skipping"
  exit 0
fi

fix=$DIFF_TMP/fixtures
mkdir -p "$fix"
cat >"$DIFF_TMP/alpha.c" <<'EOF'
#include <unistd.h>
int main (void) { for (;;) pause (); }
EOF
cc -O2 -o "$fix/alpha" "$DIFF_TMP/alpha.c" || exit 1
cp "$(type -P true)" "$fix/zed" || exit 1
printf '#!/bin/sh\nread -r x < "$0.fifo"\n' >"$fix/myscript.sh"
chmod +x "$fix/myscript.sh"
mkfifo "$fix/myscript.sh.fifo" "$fix/nap.fifo" || exit 1
printf '%s' "$(printf 'x%.0s' $(seq 5000))" >"$fix/longarg"
cp "$root/scripts/pidof-ttyrun.py" "$fix/ttyrun.py"
fixture=$root/scripts/pidof-fixture.sh

pass=0; fail=0

# The listener: every datagram sent to SOCKET, appended to FILE.
cat >"$DIFF_TMP/listen.py" <<'EOF'
import os, socket, sys
path, out = sys.argv[1], sys.argv[2]
try:
    os.unlink(path)
except FileNotFoundError:
    pass
s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
s.bind(path)
with open(out, "ab", buffering=0) as f:
    print("ready", flush=True)
    while True:
        f.write(s.recv(65536) + b"\n")
EOF

# run_side SIDE KIND PROGRAM ARGS...: one case on one side, its report in
# $DIFF_TMP/SIDE.report.
run_side() {
  local side=$1 kind=$2 prog=$3; shift 3
  local w=$DIFF_TMP/$side.work
  rm -rf "$w"
  mkdir -p "$w"
  cp "$fix/alpha" "$fix/doomed"
  : >"$w/log"
  python3 -u "$DIFF_TMP/listen.py" "$w/log.sock" "$w/log" >"$w/listener.ready" &
  local listener=$!
  local n=0
  until [ -s "$w/listener.ready" ]; do
    n=$((n + 1)); [ "$n" -gt 500 ] && { echo "pidof-diff: no listener" >&2; exit 1; }
    sleep 0.01
  done
  diff_run timeout -k 2 60 unshare -Urpf --mount-proc bash -c '
    mount --bind "$1" /dev/log || exit 125
    shift
    exec bash "$@"' _ "$w/log.sock" "$fixture" "$fix" "$w" "$kind" \
    "$bindir/$side/$prog" "$@" >"$DIFF_TMP/$side.report" 2>&1
  printf -- '--fixture status %s\n--syslog\n' "$?" >>"$DIFF_TMP/$side.report"
  # Let the last datagram land before the listener goes.
  sleep 0.1
  kill "$listener" 2>/dev/null
  wait "$listener" 2>/dev/null
  # The time in each line is when it was sent, which the sides do not share.
  sed -E 's/^(<[0-9]+>)[A-Z][a-z]{2} [ 0-9][0-9] [0-9:]{8} /\1TIME /' "$w/log" \
    >>"$DIFF_TMP/$side.report"
}

# check LABEL PROGRAM ARGS...: both sides, with standard input /dev/null and
# with a terminal. A case whose fixtures did not come up on either side is a
# failure, not a pass: the two reports would agree, about nothing.
check() {
  local label=$1 prog=$2; shift 2
  local kind side broken
  for kind in null tty; do
    run_side ours "$kind" "$prog" "$@"
    run_side gnu "$kind" "$prog" "$@"
    broken=
    for side in ours gnu; do
      grep -q -- '^--fixture status 0$' "$DIFF_TMP/$side.report" || broken=$side
    done
    if [ -n "$broken" ]; then
      fail=$((fail+1))
      printf 'BROKEN %s (stdin %s): the %s side did not run\n' "$label" "$kind" "$broken"
      head -30 "$DIFF_TMP/$broken.report"
    elif cmp -s "$DIFF_TMP/ours.report" "$DIFF_TMP/gnu.report"; then
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   %s (%s)\n' "$label" "$kind"
    else
      fail=$((fail+1))
      printf 'DIFF %s (stdin %s)\n' "$label" "$kind"
      diff <(cat -A "$DIFF_TMP/ours.report") <(cat -A "$DIFF_TMP/gnu.report") | head -30
    fi
  done
  return 0
}

# Every case below judges reports; a report that is the same on both sides
# because nothing ran in it would pass. So first: the reference's complaint
# reaches the listener, and the reference finds a fixture.
run_side gnu null pidof -y
if ! grep -q '^<27>TIME pidof\[[0-9]*\]: invalid options on command line!' "$DIFF_TMP/gnu.report"; then
  echo "pidof-diff: the reference's complaint never reached the socket over /dev/log:"
  cat "$DIFF_TMP/gnu.report"
  exit 1
fi
run_side gnu null pidof beta
if ! grep -q -- '^--status 0$' "$DIFF_TMP/gnu.report"; then
  echo "pidof-diff: the reference found no beta among the fixtures:"
  cat "$DIFF_TMP/gnu.report"
  exit 1
fi

pf() { local label=$1; shift; check "pidof: $label" pidof "$@"; }
ka() { local label=$1; shift; check "killall5: $label" killall5 "$@"; }

long_arg=$(cat "$fix/longarg")
# --- finding processes ---------------------------------------------------------
pf "by name" alpha
pf "by argv[0]" beta
pf "a title with blanks, whole" "gamma with spaces"
pf "a login shell's -name, without the -" delta
pf "a login shell's -name, after --" -- -delta
pf "the full path, which exists" "$fix/alpha"
pf "a path that is not where it runs" /elsewhere/alpha
pf "-x and a path that is not where it runs" -x /elsewhere/alpha
pf "a path to nothing" /no/such/alpha
pf "a deleted program by path" "$fix/doomed"
pf "a deleted program by name" doomed
pf "a script, without -x" myscript.sh
pf "a script, with -x" -x myscript.sh
pf "a script's interpreter" sh
pf "a script by its path, -x" -x "$fix/myscript.sh"
pf "an interpreter's argument by path" /opt/tool/alpha
pf "an interpreter's argument by path, -x" -x /opt/tool/alpha
pf "an interpreter's argument by name, -x" -x alpha
pf "argv[0] longer than pidof reads" "$long_arg"
pf "argv[0]'s first 4096 bytes" "${long_arg:0:4096}"
pf "the rest of it, with -x" -x "${long_arg:4096}"
pf "a long path's last part" longname
pf "a zombie" zed
pf "a zombie, -z" -z zed
pf "python" python3
pf "init" bash
pf "itself" pidof
pf "two names" alpha beta
pf "a name twice" beta beta
pf "nothing" nosuchprogram
pf "no names at all"
pf "an empty name" ''
pf "a name that ends in /" alpha/
pf "/" /

# --- options -------------------------------------------------------------------
pf "-s" -s alpha
pf "-s with two names" -s alpha beta
pf "-o one" -o '{1}' alpha
pf "-o several, , ; and :" -o '{1},{3};{4}:{8}' alpha
pf "-o twice" -o '{1}' -o '{3}' alpha
pf "-o %PPID" -o %PPID alpha
pf "-o something that is not a number" -o 'x,0,-3,,{1}' alpha
pf "-o with nothing" -o '' alpha
pf "-o of no process" -o 99999 alpha
pf "-d ," -d , alpha
pf "-d with more than one byte" -d ab alpha
pf "-d with nothing: a NUL" -d '' alpha
pf "-q" -q alpha
pf "-q of nothing" -q nosuchprogram
pf "-c" -c alpha
pf "-n" -n alpha
pf "-z" -z alpha
pf "-x twice" -x -x myscript.sh
pf "-h" -h
pf "-h after a name" alpha -h
pf "-h after an option that is none" -y -h
pf "an option that is none" -y alpha
pf "-o without its argument" alpha -o
pf "--" -- alpha
pf "a long option" --help
pf "-sq" -sq alpha

# --- killall5 -------------------------------------------------------------------
# Fixtures 8, 9 and 12 have sessions of their own; every other one shares the
# harness's, whose leader is outside the namespace, and is left alone as
# killall5's own. No case sends a signal whose default is a core dump: WSL
# hands each one to its crash collector.
ka "TERM" -15
ka "the default, KILL"
ka "USR1" -10
ka "HUP without a dash" 1
ka "CONT" -18
ka "STOP, then the CONT every case ends with" -19
ka "a number with more after it" -15x
ka "a signal twice: the first counts" -15 -9
ka "signal 0 is the usage" -0
ka "signal 32 is the usage" -32
ka "a signal that is no number" -x
ka "--o is a signal, and no number" --o
ka "31, the highest, every target left out" -31 -o '{8},{9},{12}'
ka "TERM, one left out" -15 -o '{9}'
ka "TERM, several left out" -15 -o '{8},{9}:{12};{1}'
ka "TERM, -o twice" -15 -o '{8}' -o '{9}'
ka "anything that begins with o is -o" -15 -omit '{9}'
ka "-o before the signal is the usage" -o '{8}' -15
ka "-o alone: KILL to the rest, nothing stopped" -o '{8}'
ka "-o without its list" -o
ka "-o of no pid" -15 -o x
ka "-o of an empty list" -15 -o ''
ka "-o of a pid that is not there" -15 -o 99999

printf '\n%d passed, %d differed\n' "$pass" "$fail"
[ "$fail" = 0 ]
