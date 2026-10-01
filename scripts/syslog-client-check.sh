#!/usr/bin/env bash
# What `libcsyslog`'s callers actually send to the system log, end to end.
#
# ## Why a check and not a diff
#
# `libcsyslog` is a wrapper over the C library's syslog(3), and `ntpdate -s`
# is SlateOS's own program, not a port -- there is no second implementation to
# compare against. What can be checked is the datagram that arrives at
# `/dev/log`: its priority (facility and severity), the identity and PID the
# libc writes, and the message, byte for byte.
#
# ## How
#
# Inside WSL the script re-executes itself under `unshare -r -m -n`: a user
# namespace (whose root may mount), a mount namespace and an empty network
# namespace. There `/dev/log` -- in WSL a symlink to journald's socket -- has a
# private listening socket bind-mounted over it, so nothing reaches the host's
# journal, and a fake NTP server answers on 127.0.0.1:123, so `ntpdate` has a
# server that is really there and one (127.0.0.2) that is not. The build runs
# inside the namespace too; its dependencies are all local paths.
#
# ## The cases
#
# * The crate's `log_args` example: identity, `[PID]`, facility and severity,
#   a `%` that must reach the log as itself, UTF-8 and a byte that is not.
# * `ntpdate -s` / `sntp -s`: results at daemon.notice, failures at
#   daemon.err, nothing on stdout or stderr, and the exit status unchanged.
# * `ntpdate` without `-s`: the same results on stdout, and nothing logged.
# * `ntpd`, the daemon: its start, its sync, a correction it cannot make, a
#   server it cannot reach -- each at its severity -- and, under a `logfile`
#   directive, the same lines in that file in ntpd's format and none in the
#   log. The server runs 10 s ahead, so there is always a correction to make,
#   and on this host (no SlateOS kernel) making one always fails the same way.
set -u

# --- into private namespaces ------------------------------------------------------
#
# Before `diff-wsl.sh` is sourced, as `df-diff.sh` does and for its reason: the
# preamble makes a scratch directory with an `EXIT` trap, which `exec` would
# skip. After we are inside WSL, because `unshare` is a Linux command.
if command -v wslpath >/dev/null 2>&1 && [ -z "${SYSLOG_CHECK_NS:-}" ]; then
  if command -v unshare >/dev/null 2>&1 && unshare -r -m -n true 2>/dev/null; then
    export SYSLOG_CHECK_NS=1
    exec unshare -r -m -n bash "$0" "$@"
  fi
  echo "syslog-client-check: no user, mount and network namespaces here; SKIPPED"
  exit 0
fi

DIFF_PROG='syslog-client'
DIFF_PKG='libcsyslog ntpd'
DIFF_BINS='ntpd'
DIFF_EXAMPLES='log_args'
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED='timeout python3 ip mount'
DIFF_FORWARD='SYSLOG_CHECK_NS'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0
ok() { pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'ok   %s\n' "$1"; return 0; }
bad() { fail=$((fail + 1)); printf 'FAIL %s\n%s\n' "$1" "$2"; return 0; }

if [ -z "${SYSLOG_CHECK_NS:-}" ]; then
  echo "syslog-client-check: not inside the private namespaces; refusing to touch /dev/log"
  exit 1
fi
if [ ! -e /dev/log ]; then
  echo "syslog-client-check: no /dev/log here to stand in for; SKIPPED"
  exit 0
fi
ip link set lo up || { echo "syslog-client-check: cannot bring up lo"; exit 1; }

# --- the log --------------------------------------------------------------------
# Each datagram on a line, escaped by Python's `unicode_escape` over Latin-1 --
# so a byte that is not ASCII reads `\xff`, a backslash `\\` -- after
# `$1.ready` appears.
cat > "$DIFF_TMP/listen.py" <<'PY'
import socket, sys

s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
s.bind(sys.argv[1])
open(sys.argv[1] + ".ready", "w").close()
out = sys.stdout
while True:
    data = s.recv(65536)
    out.write(data.decode("latin-1").encode("unicode_escape").decode("ascii") + "\n")
    out.flush()
PY
log=$DIFF_TMP/log.sock
python3 "$DIFF_TMP/listen.py" "$log" > "$DIFF_TMP/log.out" & listener=$!
for _ in {1..100}; do [ -e "$log.ready" ] && break; sleep 0.05; done
[ -e "$log.ready" ] || { echo "syslog-client-check: the log listener never started"; exit 1; }
# Onto whatever `/dev/log` resolves to -- journald's socket, in WSL.
mount --bind "$log" /dev/log || { echo "syslog-client-check: cannot bind over /dev/log"; exit 1; }

# --- an NTP server --------------------------------------------------------------
# Answers every client packet as a stratum-2 server whose clock is this one's
# plus `$2` seconds: mode 4, the client's transmit time as the origin, that
# time as receive and transmit.
cat > "$DIFF_TMP/ntpserver.py" <<'PY'
import socket, struct, sys, time

EPOCH = 2208988800


def stamp(t):
    frac = int((t - int(t)) * 2**32) & 0xFFFFFFFF
    return struct.pack("!II", (int(t) + EPOCH) & 0xFFFFFFFF, frac)


s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind(("127.0.0.1", 123))
open(sys.argv[1], "w").close()
while True:
    data, peer = s.recvfrom(1024)
    if len(data) < 48:
        continue
    version = (data[0] >> 3) & 7
    now = time.time() + float(sys.argv[2])
    reply = struct.pack("!BBbb", (version << 3) | 4, 2, 6, -20)
    reply += struct.pack("!II", 0, 0) + b"LOCL" + stamp(now)
    reply += data[40:48] + stamp(now) + stamp(now)
    s.sendto(reply, peer)
PY
python3 "$DIFF_TMP/ntpserver.py" "$DIFF_TMP/ntp.ready" 10 2>"$DIFF_TMP/ntp.err" & server=$!
for _ in {1..100}; do [ -e "$DIFF_TMP/ntp.ready" ] && break; sleep 0.05; done
[ -e "$DIFF_TMP/ntp.ready" ] || { echo "syslog-client-check: the NTP server never started"; exit 1; }
trap 'kill "$listener" "$server" 2>/dev/null' EXIT

# `ntpdate` and `sntp` are personalities of `ntpd`, chosen by argv[0].
mkdir -p "$DIFF_TMP/bin"
ln -s "$OURS" "$DIFF_TMP/bin/ntpdate"
ln -s "$OURS" "$DIFF_TMP/bin/sntp"
log_args=$(diff_ours_example log_args)

# --- running one case -------------------------------------------------------------
# start CMD...: run CMD in the background; `pid` is its PID.
start() {
  before=$(wc -l < "$DIFF_TMP/log.out")
  "$@" > "$DIFF_TMP/out" 2> "$DIFF_TMP/err" & pid=$!
}
# settle WANT_LINES [SECS]: wait (up to SECS, default 5) for WANT_LINES new
# lines in the log, and a moment more for any that should not be there. Sets
# out, err, and logged -- the new lines, with each clock replaced by DATE and
# the process's PID by PID, so a wrong PID still shows.
settle() {
  local want=$1 tries=$(( ${2:-5} * 20 )) after
  while [ "$tries" -gt 0 ]; do
    after=$(wc -l < "$DIFF_TMP/log.out")
    [ $((after - before)) -ge "$want" ] && break
    sleep 0.05
    tries=$((tries - 1))
  done
  sleep 0.2
  out=$(cat "$DIFF_TMP/out"); err=$(cat "$DIFF_TMP/err")
  logged=$(tail -n +"$((before + 1))" "$DIFF_TMP/log.out" | sed -E \
    -e 's/^(<[0-9]+>)[A-Z][a-z]{2} [ 0-9][0-9] [0-9]{2}:[0-9]{2}:[0-9]{2} /\1DATE /' \
    -e "s/\\[$pid\\]: /[PID]: /")
}
# run WANT_LINES CMD...: CMD to its end; `rc` is its status.
run() {
  local want=$1; shift
  start "$@"
  wait "$pid"; rc=$?
  settle "$want"
}

# expect LABEL WANT-RC WANT-OUT WANT-ERR WANT-LOGGED: after `run`.
expect() {
  if [ "$rc" = "$2" ] && [ "$out" = "$3" ] && [ "$err" = "$4" ] && [ "$logged" = "$5" ]; then
    ok "$1"
  else
    bad "$1" "$(printf '  rc:     %s (want %s)\n  stdout: %q\n  want:   %q\n  stderr: %q\n  want:   %q\n  logged:\n%s\n  want:\n%s' \
      "$rc" "$2" "$out" "$3" "$err" "$4" "$logged" "$5")"
  fi
}

# An NTP result's numbers change run to run; its shape does not.
ntp_shape() {
  sed -E -e 's/offset -?[0-9]+\.[0-9]+ ms, delay -?[0-9]+\.[0-9]+ ms/offset N ms, delay N ms/' \
         -e 's/  [0-9]{4}-[0-9]{2}-[0-9]{2}[T ][0-9:.]+Z? \(UTC\)/  ISO (UTC)/'
}

# --- libcsyslog, directly -------------------------------------------------------
run 5 "$log_args" libcsyslog-test 3 5 plain '100% %s %n' $'caf\xc3\xa9' $'raw\xff' ''
expect 'identity, PID, daemon.notice; % stays literal; bytes kept' 0 '' '' \
'<29>DATE libcsyslog-test[PID]: plain
<29>DATE libcsyslog-test[PID]: 100% %s %n
<29>DATE libcsyslog-test[PID]: caf\xc3\xa9
<29>DATE libcsyslog-test[PID]: raw\xff
<29>DATE libcsyslog-test[PID]: '
run 1 "$log_args" 'another name' 16 7 'local0.debug'
expect 'facility and severity come through as given' 0 '' '' \
'<135>DATE another name[PID]: local0.debug'
run 1 "$log_args" 'back\slash' 9 3 'a \ b'
expect 'a backslash is a byte like any other' 0 '' '' \
'<75>DATE back\\slash[PID]: a \\ b'

# --- ntpdate -s ------------------------------------------------------------------
run 2 "$DIFF_TMP/bin/ntpdate" -s -q -u -p 1 -t 2 127.0.0.1
logged=$(printf '%s\n' "$logged" | ntp_shape)
expect 'ntpdate -s: the result goes to the log at daemon.notice' 0 '' '' \
'<29>DATE ntpdate[PID]: server 127.0.0.1, stratum 2, offset N ms, delay N ms
<29>DATE ntpdate[PID]:   ISO (UTC)'
run 2 "$DIFF_TMP/bin/sntp" -s -q -u -p 1 -t 2 127.0.0.1
logged=$(printf '%s\n' "$logged" | ntp_shape)
expect 'sntp -s: logged under its own name' 0 '' '' \
'<29>DATE sntp[PID]: server 127.0.0.1, stratum 2, offset N ms, delay N ms
<29>DATE sntp[PID]:   ISO (UTC)'
run 2 "$DIFF_TMP/bin/ntpdate" -s -q -u -p 1 -t 1 127.0.0.2
expect 'ntpdate -s: failures go to the log at daemon.err, and the status says so' 1 '' '' \
'<27>DATE ntpdate[PID]: no response from 127.0.0.2
<27>DATE ntpdate[PID]: no usable responses from any server'

# --- ntpdate without -s ------------------------------------------------------------
run 0 "$DIFF_TMP/bin/ntpdate" -q -u -p 1 -t 2 127.0.0.1
out=$(printf '%s\n' "$out" | ntp_shape)
expect 'ntpdate: without -s the result is on stdout and nothing is logged' 0 \
'server 127.0.0.1, stratum 2, offset N ms, delay N ms
  ISO (UTC)' '' ''
run 0 "$DIFF_TMP/bin/ntpdate" -q -u -p 1 -t 1 127.0.0.2
expect 'ntpdate: without -s failures are on stderr and nothing is logged' 1 '' \
'ntpdate: no response from 127.0.0.2
ntpdate: no usable responses from any server' ''

# --- ntpd, the daemon ------------------------------------------------------------
conf=$DIFF_TMP/ntp.conf
printf 'server 127.0.0.1\ndriftfile %s/ntp1.drift\n' "$DIFF_TMP" > "$conf"
run 3 "$OURS" -q -g -c "$conf"
expect 'ntpd: its start and sync are notices, a step it cannot make an error' 0 '' '' \
'<29>DATE ntpd[PID]: ntpd starting; servers: 127.0.0.1
<29>DATE ntpd[PID]: synchronized to 127.0.0.1, stratum 2
<27>DATE ntpd[PID]: cannot adjust the clock: clock_settime: not supported on this host (no SlateOS kernel)'

printf 'server 127.0.0.1\ndriftfile %s/ntp2.drift\nlogfile %s/ntpd.log\n' "$DIFF_TMP" "$DIFF_TMP" > "$conf"
run 0 "$OURS" -q -c "$conf"
expect 'ntpd: under a logfile directive, nothing goes to syslog' 0 '' '' ''
got=$(sed -E -e 's/^[ 0-9][0-9] [A-Z][a-z]{2} [0-9]{2}:[0-9]{2}:[0-9]{2} /DATE /' \
  -e "s/ ntpd\\[$pid\\]: / ntpd[PID]: /" "$DIFF_TMP/ntpd.log" 2>&1)
want='DATE ntpd[PID]: ntpd starting; servers: 127.0.0.1
DATE ntpd[PID]: synchronized to 127.0.0.1, stratum 2
DATE ntpd[PID]: cannot adjust the clock: clock_adjtime: not supported on this host (no SlateOS kernel)'
if [ "$got" = "$want" ]; then ok 'ntpd: ... it goes to the file, in ntpd'"'"'s own format'
else bad 'ntpd: ... it goes to the file, in ntpd'"'"'s own format' "$(printf '  got:\n%s\n  want:\n%s' "$got" "$want")"; fi
# 10 s ahead is 10 000 PPM of frequency error unbounded; ntpd steers 500 at most.
got=$(cat "$DIFF_TMP/ntp2.drift" 2>&1)
if [ "$got" = '500.000000' ]; then ok 'ntpd: the saved drift is within ntpd'"'"'s bound'
else bad 'ntpd: the saved drift is within ntpd'"'"'s bound' "  got: $got  want: 500.000000"; fi

# Nothing answers at 127.0.0.2, and the daemon's socket is not connected, so
# each of its four queries waits out its 5 s: the report comes after ~21 s.
# The daemon then sleeps until its next poll, and is stopped.
printf 'server 127.0.0.2\ndriftfile %s/ntp3.drift\n' "$DIFF_TMP" > "$conf"
start "$OURS" -q -c "$conf"
settle 2 30
kill "$pid" 2>/dev/null; wait "$pid"; rc=$?
expect 'ntpd: an unreachable server is a warning, said once' 143 '' '' \
'<29>DATE ntpd[PID]: ntpd starting; servers: 127.0.0.2
<28>DATE ntpd[PID]: no server reachable (127.0.0.2)'

echo "syslog-client-check: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
