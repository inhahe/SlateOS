#!/usr/bin/env bash
# Differential test: our `w` against procps-ng 4.0.4's, built as SlateOS's
# port is configured (`--enable-w-from`, no logind) -- see procps-ref.sh for
# why that is a build of the release and not Ubuntu's `/usr/bin/w`.
#
# ## Real sessions, a fixture utmp
#
# `w` prints a row only for a session whose login process exists, and fills
# it from the process table: the processes whose controlling terminal is the
# session's, which of them is in the terminal's foreground group, whose they
# are. So the sessions here are real. `session.py` starts each one as the
# leader of a new session on a new pseudo-terminal -- one with a foreground
# job newer than its leader, one with only a background job, one alone, a
# zombie, one whose command line is `-` -- and the utmp each case reads is a
# fixture naming those terminals and leaders, in glibc's 384-byte layout, with
# hosts, addresses, names and login times chosen to reach every branch.
#
# Every process those sessions run is asleep, so its CPU time is frozen and
# both sides read the same JCPU and PCPU.
#
# ## Pinned machine, one moving clock
#
# Each side runs in its own `unshare -mUr`, with `/run/utmp` (which
# `/var/run/utmp` is), `/proc/uptime` and `/proc/loadavg` bind-mounted to
# fixtures. Inside that user namespace the harness's own user is root, so a
# session whose utmp name is `root` owns its processes and one named for the
# harness's user does not -- which is how the ownership test in choosing WHAT
# is reached without a second account.
#
# The idle times come from the terminals' access times, re-set before every
# case to values whose rendering cannot change in the seconds a case takes:
# days, hours-and-minutes, the future. The header's clock is the one field
# that moves; it is compared as a number, within the time the case took, as
# `uptime-diff.sh` compares it.
#
# ## Cases that differ on purpose
#
# `--version`, and a `/var/run/utmp` that cannot be read: procps reads it
# through glibc's `getutxent`, which cannot say so, and prints a header over
# an empty table; ours says `w: /var/run/utmp: <reason>` and exits 1, as
# `users`, `who` and `pinky` do.
set -u

DIFF_PROG='w'
DIFF_NEED="python3 unshare timeout"
# shellcheck source=procps-ref.sh
. "$(dirname "$0")/procps-ref.sh"
DIFF_REF=$PROCPS_REF_W
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# ---------------------------------------------------------------------------
# mkutmp.py: utmp records from a spec, one per line:
#   type|pid|line|id|user|host|term|exit|session|sec|usec|addr
# Empty fields are zero or empty; \xHH writes a byte; addr is an IPv4 or
# IPv6 address in text, stored as ut_addr_v6 stores it.
# ---------------------------------------------------------------------------
mkutmp=$DIFF_TMP/mkutmp.py
cat >"$mkutmp" <<'PY'
import socket, struct, sys

def field(text):
    return text.encode("utf-8").decode("unicode_escape").encode("latin-1")

def num(s):
    return int(s) if s.strip() else 0

def addr(text):
    if not text.strip():
        return b"\0" * 16
    if ":" in text:
        return socket.inet_pton(socket.AF_INET6, text)
    return socket.inet_pton(socket.AF_INET, text) + b"\0" * 12

data = b""
for raw in sys.stdin.read().splitlines():
    if not raw.strip() or raw.startswith("#"):
        continue
    p = (raw.split("|") + [""] * 12)[:12]
    rec = struct.pack("<hxxi32s4s32s256shhiii16s20s",
                      num(p[0]), num(p[1]), field(p[2])[:32], field(p[3])[:4],
                      field(p[4])[:32], field(p[5])[:256], num(p[6]), num(p[7]),
                      num(p[8]), num(p[9]), num(p[10]), addr(p[11]), b"\0" * 20)
    assert len(rec) == 384
    data += rec
sys.stdout.buffer.write(data)
PY

# ---------------------------------------------------------------------------
# session.py: COMMAND as the leader of a new session on a new pty.
#   session.py [--argv0=NAME] [--type=TEXT] PROGRAM [ARG...]
# Prints "<line> <pid>", then holds the master open -- typing TEXT into it
# first, if given -- until it is killed.
# ---------------------------------------------------------------------------
sesspy=$DIFF_TMP/session.py
cat >"$sesspy" <<'PY'
import os, sys, time

args = sys.argv[1:]
argv0 = typed = None
while args and args[0].startswith("--"):
    opt = args.pop(0)
    if opt.startswith("--argv0="):
        argv0 = opt[len("--argv0="):]
    elif opt.startswith("--type="):
        typed = opt[len("--type="):]
master, slave = os.openpty()
name = os.ttyname(slave)
argv = [args[0] if argv0 is None else argv0] + args[1:]
# glibc's posix_spawn calls setsid before the file actions, so the child opens
# the slave as a session leader with no terminal, which makes it the
# controlling terminal and the leader's group its foreground.
pid = os.posix_spawnp(args[0], argv, os.environ, setsid=True, file_actions=[
    (os.POSIX_SPAWN_OPEN, 0, name, os.O_RDWR, 0),
    (os.POSIX_SPAWN_DUP2, 0, 1),
    (os.POSIX_SPAWN_DUP2, 0, 2),
])
os.close(slave)
line = name[len("/dev/"):] if name.startswith("/dev/") else name
sys.stdout.write(f"{line} {pid}\n")
sys.stdout.flush()
if typed is not None:
    time.sleep(0.3)
    os.write(master, os.fsencode(typed))
while True:
    try:
        if not os.read(master, 4096):
            break
    except OSError:
        break
while True:
    time.sleep(3600)
PY

# ---------------------------------------------------------------------------
# ptyrun.py: COMMAND with standard output on a pty COLS columns wide; what it
# wrote there comes out on our standard output.
#   ptyrun.py COLS COMMAND...
# ---------------------------------------------------------------------------
ptyrun=$DIFF_TMP/ptyrun.py
cat >"$ptyrun" <<'PY'
import fcntl, os, struct, subprocess, sys, termios

cols = int(sys.argv[1])
master, slave = os.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, cols, 0, 0))
p = subprocess.Popen(sys.argv[2:], stdin=subprocess.DEVNULL, stdout=slave)
os.close(slave)
out = b""
while True:
    try:
        chunk = os.read(master, 65536)
    except OSError:
        break
    if not chunk:
        break
    out += chunk
sys.stdout.buffer.write(out)
sys.exit(p.wait())
PY

# ---------------------------------------------------------------------------
# The sessions
# ---------------------------------------------------------------------------
sessions=""
# $1 = name; the rest is session.py's command line.
start_session() {
  local name=$1; shift
  python3 "$sesspy" "$@" >"$work/$name.info" 2>/dev/null &
  echo $! >"$work/$name.holder"
  sessions="$sessions $name"
}

stop_sessions() {
  local s line sid p
  for s in $sessions; do
    if [ -s "$work/$s.info" ] && read -r line sid <"$work/$s.info"; then
      # Every process of the session, each by its own PID: these are the
      # processes this harness started and nothing else.
      for p in $(ps -o pid= -s "$sid" 2>/dev/null); do
        kill "$p" 2>/dev/null
      done
    fi
    [ -s "$work/$s.holder" ] && kill "$(cat "$work/$s.holder")" 2>/dev/null
  done
  return 0
}
trap 'stop_sessions; diff_cleanup' EXIT

# S1: a foreground job newer than its leader, and a background one.
start_session s1 sh -mc 'sleep 0.05; sleep 601 & sleep 0.05; sleep 602'
# S2: the leader alone.
start_session s2 sleep 603
# S3: a background job, and the leader -- still the foreground -- after it.
start_session s3 sh -mc 'sleep 0.05; sleep 604 & sleep 0.05; exec sleep 605'
# S4: a command line that has to be escaped: a tab, and a byte that begins a
# UTF-8 sequence it is not followed by.
start_session s4 "--argv0=$(printf 'sleep\t\351x')" sleep 606
# S5: a leader that has exited and not been reaped: a zombie, named from its
# `stat` as `[sh] <defunct>`.
start_session s5 sh -c 'exit 0'
# S6: a leader whose command line is `-`, and a background job: the one case
# in which the newest process on the terminal stands in for WHAT.
start_session s6 bash -mc 'sleep 0.05; sleep 607 & sleep 0.05; exec -a - cat'

# Every session has said where it is, and every process in it has settled.
for s in $sessions; do
  n=0
  while ! [ -s "$work/$s.info" ] && [ "$n" -lt 100 ]; do
    sleep 0.1; n=$((n + 1))
  done
done
sleep 1
for s in $sessions; do
  if ! read -r line pid <"$work/$s.info"; then
    echo "w-diff: session $s did not start" >&2
    exit 1
  fi
  eval "${s}_line=\$line ${s}_pid=\$pid"
done
# shellcheck disable=SC2154 # set by the eval above
{
  L1=$s1_line P1=$s1_pid L2=$s2_line P2=$s2_pid L3=$s3_line P3=$s3_pid
  L4=$s4_line P4=$s4_pid L5=$s5_line P5=$s5_pid L6=$s6_line P6=$s6_pid
}

# The terminals' access times, re-set before every case: the idle times are
# then three days, two and a half hours, and the future (`?`).
set_atimes() {
  local now
  now=$(date +%s)
  touch -a -d "@$((now - 3 * 86400 - 600))" "/dev/$L1" "/dev/$L4" "/dev/$L5"
  touch -a -d "@$((now - 9015))" "/dev/$L2" "/dev/$L6"
  touch -a -d "@$((now + 3600))" "/dev/$L3"
}

# ---------------------------------------------------------------------------
# The fixtures
# ---------------------------------------------------------------------------
me=$(id -un)
now=$(date +%s)
mk() { python3 "$mkutmp" >"$work/$1"; }

# Every session, every kind of record w passes over, and every host shape.
mk main <<SPEC
2|0|~|~~|reboot|6.6.87||||$((now - 86400))|0
1|$((78 * 256 + 53))|~|~~|runlevel|6.6.87||||$((now - 86300))|0
6|$P2|tty2|2|LOGIN|||||$((now - 86000))|0
7|$P1|$L1|s1|root|10.0.0.2:0||||$((now - 300))|0|10.0.0.2
7|$P2|$L2|s2|root|host.example:1||||$((now - 12 * 3600 - 600))|0
7|$P3|$L3|s3|root|||||$((now - 3 * 86400))|0
7|$P1|$L1|s1u|$me|fe80::1%eth0||||$((now - 40 * 86400))|0|fe80::1
7|$P4|$L4|s4|root|bad host||||0|0
7|$P5|$L5|s5|root|zombie||||-86400|0
7|$P6|$L6|s6|root|::ffff:192.168.1.5||||2147483647|0|::ffff:192.168.1.5
7|$P2|:0|x0|root|:0||||$((now - 600))|0
7|$P2|nosuch|n0|root|tab\\there||||$((now - 600))|0
7|$P1|$L1-x|cl|root|h\\xe9st||||$((now - 600))|0
7|$P1|/dev/$L1|ab|root|a-host-name-longer-than-sixteen||||$((now - 600))|0
7|2147483647|$L1|st|root|stale||||$((now - 600))|0
7|$P1|$L1|nu|nosuchuser|nobody.example||||$((now - 600))|0
7|$P1|$L1|em||nameless||||$((now - 600))|0
8|$P1|$L1|dd|root|dead||||$((now - 600))|0
SPEC

# Addresses for -i: IPv4 that fits the column only at its default width, a
# long IPv6 address, IPv4-compatible, and none at all.
mk addrs <<SPEC
7|$P1|$L1|a1|root|wide.example:2||||$((now - 300))|0|192.168.100.200
7|$P2|$L2|a2|root|v6.example||||$((now - 300))|0|2001:db8:1234:5678:9abc:def0:1234:5678
7|$P3|$L3|a3|root|compat:0.0||||$((now - 300))|0|::1.2.3.4
7|$P4|$L4|a4|root|noaddr:3||||$((now - 300))|0|
7|$P6|$L6|a5|root|fe80::2%wlan0||||$((now - 300))|0|fe80::2
SPEC

# Names at the edge of the field: 32 bytes, and longer than the default
# column, of users that do not exist -- so only `-u` shows them.
mk names <<SPEC
7|$P1|$L1|n1|ABCDEFGHIJKLMNOPQRSTUVWXYZ012345|h1||||$((now - 300))|0
7|$P2|$L2|n2|nine-char|h2||||$((now - 300))|0
7|$P3|$L3|n3|caf\\xc3\\xa9|h3||||$((now - 300))|0
SPEC

: >"$work/empty"
printf '16000.00 100000.00\n' >"$work/uptime"
printf '0.07 1.05 12.34 2/297 51893\n' >"$work/loadavg"

# --- knobs ------------------------------------------------------------------
# Each is reset after every case.
UTMP=main        # a fixture's name, or `missing` (no /run/utmp) or `dir`
UPTIME=          # /proc/uptime's text, if not the default
LOADAVG=         # /proc/loadavg's text, if not the default
COLS=-           # COLUMNS: `-` for unset
USERLEN=-        # PROCPS_USERLEN: `-` for unset
FROMLEN=-        # PROCPS_FROMLEN: `-` for unset
LOCALE=C.UTF-8
ZONE=UTC
PTYCOLS=         # run with standard output on a pty this many columns wide
reset_knobs() {
  UTMP=main; UPTIME=; LOADAVG=; COLS=-; USERLEN=-; FROMLEN=-
  LOCALE=C.UTF-8; ZONE=UTC; PTYCOLS=
}

# $1 = side, $2 = output prefix; the rest is w's argv.
run_side() {
  local side=$1 p=$2; shift 2
  local up=$work/uptime la=$work/loadavg ut
  if [ -n "$UPTIME" ]; then up=$p.$side.uptime; printf '%s' "$UPTIME" >"$up"; fi
  if [ -n "$LOADAVG" ]; then la=$p.$side.loadavg; printf '%s' "$LOADAVG" >"$la"; fi
  case $UTMP in
    missing|dir) ut=$UTMP ;;
    *) ut=$work/$UTMP ;;
  esac
  local -a envs=(env -u COLUMNS -u PROCPS_USERLEN -u PROCPS_FROMLEN
                 "LC_ALL=$LOCALE" "TZ=$ZONE" "PATH=$bindir/$side:$PATH")
  [ "$COLS" != - ] && envs+=("COLUMNS=$COLS")
  [ "$USERLEN" != - ] && envs+=("PROCPS_USERLEN=$USERLEN")
  [ "$FROMLEN" != - ] && envs+=("PROCPS_FROMLEN=$FROMLEN")
  local -a ns=(timeout -k 5 60 unshare -mUr sh -c '
      ut=$1 up=$2 la=$3; shift 3
      case $ut in
        missing) mount -t tmpfs tmpfs /run || exit 125 ;;
        dir) mount -t tmpfs tmpfs /run && mkdir /run/utmp || exit 125 ;;
        *) mount --bind "$ut" /run/utmp || exit 125 ;;
      esac
      mount --bind "$up" /proc/uptime || exit 125
      mount --bind "$la" /proc/loadavg || exit 125
      exec "$@"' _ "$ut" "$up" "$la" "${envs[@]}" w "$@")
  set_atimes
  if [ -n "$PTYCOLS" ]; then
    diff_run python3 "$ptyrun" "$PTYCOLS" "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err"
  else
    diff_run "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err" </dev/null
  fi
  echo $? >"$p.$side.rc"
  return 0
}

# The header's clock, as seconds since midnight, or nothing.
clock_secs() {
  sed -n -e '1s/^ \([0-9][0-9]\):\([0-9][0-9]\):\([0-9][0-9]\) up .*/\1 \2 \3/p' "$1" \
  | { read -r h m s || exit 0
      printf '%s' "$(( 10#$h * 3600 + 10#$m * 60 + 10#$s ))"; }
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no t0 t1 allowed o_clock g_clock skew=0
  t0=$(date +%s)
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  t1=$(date +%s)
  allowed=$(( t1 - t0 + 1 ))
  [ "$allowed" -lt 2 ] && allowed=2
  LABEL="w $*"
  [ "$UTMP" != main ] && LABEL="$LABEL [utmp=$UTMP]"
  [ -n "$UPTIME" ] && LABEL="$LABEL [uptime=$(printf '%s' "$UPTIME" | tr '\n\t' '|>')]"
  [ -n "$LOADAVG" ] && LABEL="$LABEL [loadavg=$(printf '%s' "$LOADAVG" | tr '\n\t' '|>')]"
  [ "$COLS" != - ] && LABEL="$LABEL [COLUMNS=$COLS]"
  [ "$USERLEN" != - ] && LABEL="$LABEL [PROCPS_USERLEN=$USERLEN]"
  [ "$FROMLEN" != - ] && LABEL="$LABEL [PROCPS_FROMLEN=$FROMLEN]"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL [LC_ALL=$LOCALE]"
  [ "$ZONE" != UTC ] && LABEL="$LABEL [TZ=$ZONE]"
  [ -n "$PTYCOLS" ] && LABEL="$LABEL [pty $PTYCOLS columns]"
  reset_knobs

  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  # 125 is a mount that failed and 124 a timeout: either way the case never
  # reached `w`, which is not agreement however alike the two sides look.
  case "$o_rc $g_rc" in
    *124*|*125*|*127*)
      AGREED=broken
      REPORT="  ours rc=$o_rc  gnu rc=$g_rc"
      return 0 ;;
  esac
  o_clock=$(clock_secs "$p.ours.out"); g_clock=$(clock_secs "$p.gnu.out")
  if [ -n "$o_clock" ] && [ -n "$g_clock" ]; then
    skew=$(( o_clock - g_clock ))
    [ "$skew" -lt 0 ] && skew=$(( -skew ))
    [ "$skew" -gt 43200 ] && skew=$(( 86400 - skew ))
  elif [ -n "$o_clock" ] || [ -n "$g_clock" ]; then
    skew=99999
  fi
  local s
  for s in ours gnu; do
    sed -e '1s/^ [0-9][0-9]:[0-9][0-9]:[0-9][0-9] up / <CLOCK> up /' "$p.$s.out" >"$p.$s.body"
  done
  if cmp -s "$p.ours.body" "$p.gnu.body" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && [ "$o_rc" = "$g_rc" ] && [ "$skew" -le "$allowed" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  clock skew %ss (allowed %ss)\n  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$skew" "$allowed" "$o_rc" "$(cat -A "$p.ours.out" | head -40)" "$(cat -A "$p.ours.err" | head -20)" \
    "$g_rc" "$(cat -A "$p.gnu.out" | head -40)" "$(cat -A "$p.gnu.err" | head -20)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached w on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached w on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# The cases
# ---------------------------------------------------------------------------
# --- every session, every switch --------------------------------------------
run_case
# Agreement about an empty table is not agreement: the fixture has to reach
# the rows, or every case below compares two headers. Each live session's
# terminal must appear in the reference's first table.
for l in "$L1" "$L2" "$L3" "$L4" "$L5" "$L6"; do
  if ! grep -qF " $l " "$work/c1.gnu.out"; then
    echo "w-diff: the reference printed no row for $l; the sessions never reached w" >&2
    cat -A "$work/c1.gnu.out" >&2
    exit 1
  fi
done
[ -n "${VERBOSE:-}" ] && cat "$work/c1.gnu.out"
for opt in -h -s -f -i -o -u -p -hs -sf -fi -if -ff -iu -ip -up -sp -so -ou -sou -hsfoiup \
           --no-header --short --from --old-style --ip-addr --pids --no-current --no-c --pid; do
  run_case "$opt"
done
run_case -p -s -o

# --- whose sessions ----------------------------------------------------------
run_case root
run_case "$me"
run_case "$me" -u
run_case nosuchuser
run_case nosuchuser -u
run_case ''
run_case root extra operands
run_case -- -h
run_case "$(printf 'r%.0s' $(seq 40))"

# --- the WHAT column's width: the terminal, then COLUMNS, then 512 ----------
for c in 80 40 0 1 7 100 200 1000 abc '' -5 ' 90' 4294967376; do
  COLS=$c; run_case
done
COLS=60; run_case -s
COLS=60; run_case -p
COLS=30; run_case -sp
COLS=60; run_case -f
PTYCOLS=100; run_case
PTYCOLS=100; COLS=40; run_case
PTYCOLS=0; COLS=70; run_case
PTYCOLS=30; run_case -p

# --- PROCPS_USERLEN and PROCPS_FROMLEN, and their refusals ------------------
for v in 7 8 9 20 32 33 '' abc 4294967304 -1; do
  USERLEN=$v; run_case
done
for v in 7 8 12 30 256 257 '' abc; do
  FROMLEN=$v; run_case
done
USERLEN=7; FROMLEN=7; run_case -s
FROMLEN=8; run_case -i

# --- names, hosts and addresses ---------------------------------------------
UTMP=names; run_case
UTMP=names; run_case -u
UTMP=names; USERLEN=32; run_case -u
UTMP=names; USERLEN=10; run_case -u
UTMP=addrs; run_case
UTMP=addrs; run_case -i
UTMP=addrs; FROMLEN=8; run_case -i
UTMP=addrs; FROMLEN=40; run_case -i
UTMP=addrs; run_case -i -s
UTMP=addrs; run_case -if

# --- escaping, in UTF-8 and not ---------------------------------------------
LOCALE=C; run_case
LOCALE=POSIX; run_case -u
UTMP=names; LOCALE=C; run_case -u

# --- login times in another zone --------------------------------------------
ZONE=America/New_York; run_case
ZONE=Asia/Kolkata; run_case -o
ZONE=Etc/GMT+12; run_case

# --- the status line ---------------------------------------------------------
for u in '0 0' '59.9 0' '60 0' '3599 0' '3600 0' '86399 0' '86400 0' '90000 0' \
         '172800 0' '604800 0' '16000.00' '' 'garbage' '1e 5' '1e3 0' '0x10 0' \
         ' 16000.00	100000.00' '-3600 0' '-59 0' '3000000000.00 0' 'nan 0' 'inf 0' \
         '-inf 0' '2147483647.9 0'; do
  UPTIME="$u
"; run_case
done
for l in '1.5 x' '' 'a line of prose' 'nan -nan inf' '0.125 0.375 2.675' \
         '1e2 2e-1 0x10' '-1 -2 -3' '12345678.999 0.004999 0.005'; do
  LOADAVG="$l
"; run_case
done
UPTIME='16000.00
'; run_case -s

# --- utmp itself -------------------------------------------------------------
UTMP=empty; run_case
UTMP=empty; run_case -s
UTMP=missing; run_case
UTMP=missing; run_case root
UTMP=dir
xfail_case "a utmp that cannot be read is an error here; procps sees nobody"

# --- the command line --------------------------------------------------------
for a in -x --nosuch --no --pids=1 -hx --from=x; do
  run_case "$a"
done
run_case -h --help
run_case --help -x
run_case -x --help
run_case --help
xfail_case "our version string, not procps-ng's" -V
xfail_case "our version string, not procps-ng's" --version
xfail_case "our version string, not procps-ng's" -Vx
xfail_case "our version string, not procps-ng's" -sV extra -x

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
[ "$broken" -gt 0 ] && printf ', %d BROKEN (never reached the subject)' "$broken"
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$broken" = 0 ]
