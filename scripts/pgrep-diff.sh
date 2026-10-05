#!/usr/bin/env bash
# Differential test: our `pgrep`, `pkill` and `pidwait` against procps-ng
# 4.0.4's, built as SlateOS's port is configured -- see procps-ref.sh.
#
# ## Three programs, one source
#
# Upstream builds `src/pgrep.c` three times and each executable decides what
# it is from its name; ours is one module (`coreutils::pgrep`) behind two
# binaries, and `pidwait` is the `pgrep` binary under that name (SlateOS
# builds no `pidwait` until its native ABI has `pidfd_open`; see the module's
# docs). So `bin/ours/pidwait` is a link to our `pgrep`, and `ARGV0=` cases
# run either side's binary under a chosen `argv[0]` to show the name -- and
# nothing else -- picks the program.
#
# ## A made-up machine
#
# As in `ps-diff.sh`: each case runs both programs in their own `unshare
# -mUrpf` -- new user, mount and PID namespaces -- with a fixture directory
# bind-mounted over `/proc` (the model of a process is `procps_fixture.py`,
# shared with that harness). The program is PID 1, which `pgrep` skips as
# itself, and the fixture's `1/` describes it, with parents to walk for `-A`.
# `/etc/passwd`, `/etc/group` and `/etc/nsswitch.conf` are fixtures too, and
# a fresh devpts instance holds `pts/0`-`pts/2`, so `-t pts/1` names a real
# terminal number.
#
# Worlds: `main`, a desktop's worth of processes, every selection option's
# subject among them, namespaces shared by hard links so `--ns` has something
# to compare; `live`, where PID 2 is a real process (`LIVE=`, started first
# in the namespace so its PID is known) that `pkill` can signal and `pidwait`
# can wait for, beside fixture processes of the same name that do not exist;
# `broken`, whose files are malformed the ways upstream's parsers notice; and
# `notask`, with no `task/` directories.
#
# ## Cases that differ on purpose
#
# `-V` and `--version` name this build; `pkill` sends nothing to a PID below
# 1, which only a malformed `/proc` produces and upstream passes to `kill` as
# "my process group" (0) or "everyone" (-1); and getopt's message escapes an
# option byte that is not text. See the module's docs.
set -u

DIFF_PROG='pgrep'
DIFF_BINS='pgrep pkill'
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED="python3 unshare timeout setsid mkfifo"
# shellcheck source=procps-ref.sh
. "$(dirname "$0")/procps-ref.sh"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if ! [ -x "$PROCPS_REF_PGREP" ] || ! [ -x "$PROCPS_REF_PKILL" ] \
   || ! [ -x "$PROCPS_REF_PIDWAIT" ]; then
  echo "pgrep-diff: procps-ng $PROCPS_REF_VERSION's pgrep did not build; SKIPPED"
  exit 0
fi

# The programs, each behind the bare name it runs as. `OURS` may name a
# directory holding our `pgrep` and `pkill` instead of the build.
mkdir -p "$bindir/ours" "$bindir/gnu"
if [ -n "${OURS:-}" ] && [ -d "$OURS" ]; then
  ours_pgrep=$OURS/pgrep ours_pkill=$OURS/pkill
else
  ours_pgrep=$(diff_ours pgrep) ours_pkill=$(diff_ours pkill)
fi
ln -s "$ours_pgrep" "$bindir/ours/pgrep" || exit 1
ln -s "$ours_pkill" "$bindir/ours/pkill" || exit 1
ln -s "$ours_pgrep" "$bindir/ours/pidwait" || exit 1
ln -s "$PROCPS_REF_PGREP" "$bindir/gnu/pgrep" || exit 1
ln -s "$PROCPS_REF_PKILL" "$bindir/gnu/pkill" || exit 1
ln -s "$PROCPS_REF_PIDWAIT" "$bindir/gnu/pidwait" || exit 1

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# ---------------------------------------------------------------------------
# hold.py: opens N pseudo-terminals in the current devpts and keeps them
# open, writing their names to FILE once they exist.
# ---------------------------------------------------------------------------
holdpy=$DIFF_TMP/hold.py
cat >"$holdpy" <<'PY'
import os, sys, time
n, ready = int(sys.argv[1]), sys.argv[2]
fds = [os.openpty() for _ in range(n)]
with open(ready, "w") as f:
    f.write(" ".join(os.ttyname(s) for _, s in fds) + "\n")
time.sleep(120)
PY

# ---------------------------------------------------------------------------
# execas.py: run PROGRAM with argv[0] ARGV0.   execas.py PROGRAM ARGV0 ARGS...
# ---------------------------------------------------------------------------
execpy=$DIFF_TMP/execas.py
cat >"$execpy" <<'PY'
import os, sys
os.execv(sys.argv[1], [sys.argv[2]] + sys.argv[3:])
PY

# ---------------------------------------------------------------------------
# lockhold.py: hold a lock on FILE -- `flock` or a POSIX record lock -- and
# say so by creating READY.   lockhold.py flock|fcntl FILE READY
# ---------------------------------------------------------------------------
LOCKPY=$DIFF_TMP/lockhold.py
export LOCKPY
cat >"$LOCKPY" <<'PY'
import fcntl, sys, time
mode, path, ready = sys.argv[1:4]
f = open(path, "r+")
if mode == "flock":
    fcntl.flock(f, fcntl.LOCK_EX)
else:
    fcntl.lockf(f, fcntl.LOCK_EX)
open(ready, "w").close()
time.sleep(30)
PY

# ---------------------------------------------------------------------------
# mkworld.py: the fake /proc trees.   mkworld.py DEST SCRIPTS
# ---------------------------------------------------------------------------
mkworld=$DIFF_TMP/mkworld.py
cat >"$mkworld" <<'PY'
import os
import sys

dest = sys.argv[1]
sys.path.insert(0, sys.argv[2])
from procps_fixture import HZ, PTS, TTY, CONSOLE, TTYS0, NS_ALL, P  # noqa: E402
import procps_fixture  # noqa: E402


def world(name, procs, **kw):
    procps_fixture.world(dest, name, procs, **kw)


def Q(pid, comm, **kw):
    """A process in the host's namespaces -- 5's -- unless told otherwise."""
    kw.setdefault("ns", NS_ALL)
    kw.setdefault("ns_from", 5)
    return P(pid, comm, **kw)


U = b"0::/user.slice/user-1000.slice/session-3.scope\n"

main = [
    # First: every other process's namespaces are hard links to these.
    Q(5, "systemd", ns_from=None, ppid=0, start=10, utime=250,
      cmdline=b"/sbin/init\0splash\0", sigcgt="00000001000004ec",
      cgroup=b"0::/init.scope\n"),
    Q(1, "pgrep", state="R", ppid=151, pgrp=1, sid=1, tty=PTS(1), tpgid=1,
      start=99990 * HZ, cmdline=b"pgrep\0"),
    Q(2, "kthreadd", ppid=0, pgrp=0, sid=0, start=0, cmdline=b"",
      cgroup=b"0::/\n"),
    Q(3, "rcu_gp", state="I", ppid=2, pgrp=0, sid=0, start=5, cmdline=b"",
      cgroup=b"0::/\n"),
    Q(100, "sshd", ppid=5, start=2000,
      cmdline=b"sshd: /usr/sbin/sshd -D [listener] 0 of 10-100 startups\0",
      sigcgt="0000000180014a03", cgroup=b"0::/system.slice/ssh.service\n"),
    Q(150, "sudo", ppid=200, pgrp=150, sid=200, tty=PTS(1), tpgid=1,
      start=99000 * HZ, cmdline=b"sudo\0-i\0", cgroup=U),
    Q(151, "bash", ppid=150, pgrp=151, sid=151, tty=PTS(1), tpgid=1,
      start=99001 * HZ, cmdline=b"-bash\0", sigcgt="000000004b813efb",
      cgroup=U),
    Q(200, "bash", ppid=100, pgrp=200, sid=200, tty=PTS(1), tpgid=300,
      uid=1000, gid=1000, start=90000 * HZ, cmdline=b"-bash\0", cgroup=U),
    Q(300, "vim", ppid=200, pgrp=300, sid=200, tty=PTS(1), tpgid=300,
      uid=1000, gid=1000, start=95000 * HZ, cmdline=b"vim\0notes.txt\0",
      cgroup=U),
    Q(301, "sleep", state="T", ppid=200, pgrp=301, sid=200, tty=PTS(1),
      tpgid=300, uid=1000, gid=1000, start=96000 * HZ,
      cmdline=b"sleep\x001000\0", cgroup=U),
    Q(400, "bash", ppid=100, pgrp=400, sid=400, tty=PTS(2), tpgid=400,
      uid=1001, gid=1001, start=80000 * HZ, cmdline=b"-bash\0"),
    Q(401, "make", state="Z", ppid=400, pgrp=400, sid=400, tty=PTS(2),
      tpgid=400, uid=1001, gid=1001, start=81000 * HZ, cmdline=b""),
    # A container: its own namespaces except the network's.
    Q(500, "java", ns_from={"net": 5}, ppid=5, uid=1000, gid=1000, threads=3,
      start=3000, cmdline=b"/usr/bin/java\0-Xmx2g\0-jar\0app.jar\0",
      tasks=[500, 501, 502], cgroup=b"12:pids:/docker/abc\n0::/docker/abc\n"),
    Q(600, "my (odd) name", ns_from=500, ppid=500, start=4000, uid=1002,
      cmdline=b"odd\tname\0caf\xc3\xa9\0\xe4\xb8\x80\xe4\xba\x8c\0bad\xff\0",
      cgroup=b"0::/docker/abc\n"),
    Q(700, "agetty", ppid=5, tty=TTY(1), tpgid=700, start=5000,
      cmdline=b"/sbin/agetty\0-o\0-p -- \\u\0--noclear\0tty1\0linux\0"),
    Q(701, "agetty", ppid=5, tty=CONSOLE, tpgid=701, start=5001,
      cmdline=b"/sbin/agetty\0--keep-baud\0console\0"),
    Q(702, "agetty", ppid=5, tty=TTYS0, tpgid=702, start=5002,
      cmdline=b"/sbin/agetty\0ttyS0\0"),
    # Started in the same tick as 150: -n and -o break the tie by PID.
    Q(800, "spinner", state="R", ppid=5, start=99000 * HZ, uid=1004,
      cmdline=b"spinner\0--fast\0", sigcgt="0000000000004000"),
    Q(801, "worker", state="D", ppid=5, start=20000, uid=1003, gid=1003,
      cmdline=b"worker\0"),
    Q(802, "systemd-journal", ppid=5, start=900,
      cmdline=b"/lib/systemd/systemd-journald\0", sigcgt="0000000000000001",
      cgroup=b"0::/system.slice/systemd-journald.service\n"),
    # Started after the uptime says now: the elapsed time wraps.
    Q(803, "future", ppid=5, start=200000 * HZ, cmdline=b"future\0"),
    # Started at exactly now: the elapsed time keeps the previous process's.
    Q(804, "now", ppid=5, start=100000 * HZ, cmdline=b"now\0"),
    # Catches signal 64 -- bit 63, which `pkill -0 -H` tests.
    Q(805, "nobody", ppid=5, start=6000, uid=65534, gid=65534,
      cmdline=b"tiny\0", sigcgt="8000000000000000"),
    # In the program's own process group and session (both 1 in the
    # namespace), for -g 0 and -s 0.
    Q(806, "inpg1", ppid=1, pgrp=1, sid=1, start=99995 * HZ,
      cmdline=b"inpg1\0"),
    Q(807, "cgnone", ppid=5, start=7000, cgroup=None),
    Q(808, "cgv1", ppid=5, start=7001, cgroup=b"5:cpu:/a\n4:mem:/b\n"),
    Q(809, "cgnonl", ppid=5, start=7002, cgroup=b"0::/nonl"),
    Q(810, "cggap", ppid=5, start=7003, cgroup=b"1:x:/\n\n0::/gap\n"),
    Q(811, "cgnul", ppid=5, start=7004, cgroup=b"0::/one\x000::/two\n"),
    Q(812, "cgempty", ppid=5, start=7005, cgroup=b""),
    Q(32768, "bigpid", ppid=5, start=7006),
    Q(4194304, "maxpid", ppid=5, start=7007),
]
world("main", main)

# PID files, in the cases' working directory.
c = f"{dest}/main/cwd"
for name, data in {
    "pid.ok": b"300\n", "pid.space": b"  300 extra", "pid.nl": b"\n300",
    "pid.junk": b"300x", "pid.zero": b"0\n", "pid.neg": b"-300\n",
    "pid.wrap": b"4294967596\n", "pid.long": b"123456789012\n",
    "pid.empty": b"", "pid.nul": b"30\x000", "pid.java": b"500\n",
    "pid.thread": b"501\n", "pid.tab": b"300\t",
}.items():
    with open(f"{c}/{name}", "wb") as f:
        f.write(data)
os.makedirs(f"{c}/pid.dir", exist_ok=True)
os.mkfifo(f"{c}/pid.fifo")

# PID 2 is real: the harness starts it first in the namespace.
live = [
    P(1, "pkill", state="R", ppid=0, start=99990 * HZ, cmdline=b"pkill\0"),
    P(2, "victim", ppid=1, start=99991 * HZ, cmdline=b"sleep\x0030\0",
      sigcgt="0000000000004000"),
    # These do not exist in the namespace: a signal finds no such process.
    P(4001, "victim", ppid=1, start=50 * HZ, cmdline=b"sleep\x0030\0"),
    P(4002, "ghost", ppid=1, start=60 * HZ, cmdline=b"ghost\0"),
]
world("live", live)

broken = [
    # A parent that does not exist, so that -A stops at once: a PID-0 process
    # below would otherwise be found as 1's parent "0", and lead back to 1 --
    # a loop upstream walks for ever (and this port stops; see its docs).
    P(1, "pgrep", state="R", ppid=999, start=99990 * HZ),
    # stat with no parentheses: the name from status.
    P(20, "noparen", raw_stat=b"20 noparen S 1 20 20 0 -1\n"),
    # stat that stops early: later fields keep their defaults.
    P(21, "short", raw_stat=b"21 (short) R 1 21 21 0 -1 4194304 1 2\n"),
    # status without Pid: the PID is 0.
    P(22, "nopid", raw_status=b"Name:\tnopid\nState:\tS (sleeping)\nUid:\t0\t0\t0\t0\n"),
    # A short signal mask takes the next line with it.
    P(23, "shortsig", raw_status=b"Name:\tshortsig\nPid:\t23\nPPid:\t1\nUid:\t0\t0\t0\t0\n"
      b"SigCgt:\t12\nSigBlk:\t0000000000000003\n"),
    # A colon not followed by a tab ends the status file.
    P(24, "badcolon", raw_status=b"Name:\tbadcolon\nPid:\t24\nOops: 1\nPPid:\t7\n"
      b"Uid:\t1000\t1000\t1000\t1000\n"),
    # No status file at all.
    P(25, "nostatus", raw_status=False),
    # Numbers that overflow their C types.
    P(26, "big", raw_stat=b"26 (big) S 4294967297 -1 26 0 -1 18446744073709551615 "
      b"99999999999999999999 0 0 0 99999999999 0 0 0 20 0 1 0 5 0 0 0 0 0 0 0 0\n"),
    # A command line of NULs only; a zombie with no command line.
    P(27, "nuls", cmdline=b"\0\0\0"),
    P(29, "zomb", state="Z", cmdline=b""),
    # A name with parentheses in it, and an escaped status name.
    P(30, "a) b (c", name=b"esc\\\\aped\\nname"),
    # No state anywhere: the state is NUL, which every -r list contains.
    P(31, "nostate", raw_stat=b"31 (nostate) ",
      raw_status=b"Name:\tnostate\nPid:\t31\nPPid:\t1\nUid:\t0\t0\t0\t0\n"),
    # The state `0`, for `--ns 0`'s run states.
    P(32, "zerostate", state="0"),
    # Signal masks that are not all hexadecimal.
    P(33, "hexbad", sigcgt="zzzz000000000000"),
    P(34, "hexzerox", sigcgt="0x00000000004000"),
    P(35, "hexspace", sigcgt=" 000000000004000"),
    P(36, "hexxg", sigcgt="0xg0000000000000"),
    # Directory names past `int`. Upstream reads `/proc/%d` of the PID cut to
    # `int` -- `/proc/-1`, `/proc/0` -- not the directory it found, so both
    # are skipped as unreadable.
    P(4294967295, "wrapneg"),
    P(4294967296, "wrapzero"),
    # A status that says the PID is -1, which `pkill` would hand to `kill`.
    P(37, "negpid", raw_status=b"Name:\tnegpid\nPid:\t-1\nPPid:\t1\nUid:\t0\t0\t0\t0\n"),
    # Past `unsigned long`: skipped entirely.
    P(99999999999999999999, "overflow"),
]
world("broken", broken)

# No task directories: threads fall back to processes.
notask = [
    P(1, "pgrep", state="R", ppid=0, start=99990 * HZ),
    P(500, "java", ppid=1, threads=3, start=3000, cmdline=b"java\0"),
]
world("notask", notask, tasks=False)
PY

python3 "$mkworld" "$work/worlds" "$root/scripts" \
  || { echo "pgrep-diff: mkworld failed" >&2; exit 1; }

# The password and group databases, and an NSS that reads only them.
mkdir -p "$work/etc"
cat >"$work/etc/passwd" <<'EOF'
root:x:0:0:root:/root:/bin/bash
alice:x:1000:1000:Alice:/home/alice:/bin/bash
bob:x:1001:1001::/home/bob:/bin/sh
carol:x:1002:1002::/:/bin/sh
café:x:1003:1003::/:/bin/sh
dave:x:1004:1004::/:/bin/sh
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
EOF
cat >"$work/etc/group" <<'EOF'
root:x:0:
adm:x:4:alice
alice:x:1000:
bob:x:1001:
carol:x:1002:
café:x:1003:
nogroup:x:65534:
EOF
printf 'passwd: files\ngroup: files\nshadow: files\n' >"$work/etc/nsswitch.conf"

# --- knobs ------------------------------------------------------------------
WORLD=main       # which fake /proc
LOCALE=C.UTF-8
ENVS=()          # extra NAME=VALUE for the program's environment
LIVE=            # a command started first in the namespace, as PID 2
PRE=             # shell run in the namespace, in the working directory, last
ARGV0=           # run the program under this argv[0] instead of its name
reset_knobs() {
  WORLD=main; LOCALE=C.UTF-8; ENVS=(); LIVE=; PRE=; ARGV0=
}

# $1 = side, $2 = output prefix, $3 = program; the rest is its argv.
run_side() {
  local side=$1 p=$2 prog=$3; shift 3
  local wdir=$work/worlds/$WORLD
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=$LOCALE" "${ENVS[@]}")
  local -a cmd
  if [ -n "$ARGV0" ]; then
    cmd=(python3 "$execpy" "$bindir/$side/$prog" "$ARGV0" "$@")
  else
    cmd=("$prog" "$@")
  fi
  local -a ns=(timeout -k 5 60 unshare -mUrpf --propagation private setsid sh -c '
      w=$1 e=$2 hold=$3 ready=$4 live=$5 pre=$6; shift 6
      # First, so that it is PID 2.
      if [ -n "$live" ]; then $live & fi
      mount -t devpts -o newinstance,ptmxmode=0666 devpts /dev/pts || exit 125
      mount --bind /dev/pts/ptmx /dev/ptmx || exit 125
      rm -f "$ready"
      python3 "$hold" 3 "$ready" &
      n=0
      while [ ! -s "$ready" ] && [ "$n" -lt 100 ]; do sleep 0.05; n=$((n + 1)); done
      [ -s "$ready" ] || exit 125
      mount --bind "$e/passwd" /etc/passwd || exit 125
      mount --bind "$e/group" /etc/group || exit 125
      mount --bind "$e/nsswitch.conf" /etc/nsswitch.conf || exit 125
      mount --bind "$w/proc" /proc || exit 125
      cd "$w/cwd" || exit 125
      if [ -n "$pre" ]; then eval "$pre" || exit 125; fi
      exec "$@"' _ "$wdir" "$work/etc" "$holdpy" "$p.$side.ready" "$LIVE" "$PRE" \
      "${envs[@]}" "${cmd[@]}")
  diff_run "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err" </dev/null
  echo $? >"$p.$side.rc"
  return 0
}

# compare PROGRAM ARGS...
compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  [ "$WORLD" = live ] && [ -z "$LIVE" ] && LIVE='sleep 30'
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="$*"
  [ "$WORLD" != main ] && LABEL="$LABEL [world=$WORLD]"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL [LC_ALL=$LOCALE]"
  [ -n "$ARGV0" ] && LABEL="$LABEL [argv0=$ARGV0]"
  [ -n "$PRE" ] && LABEL="$LABEL [pre: $PRE]"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  reset_knobs

  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  # 125 is a setup step that failed and 124 a timeout: the case never ran,
  # which is not agreement however alike the two sides look.
  case "$o_rc $g_rc" in
    *124*|*125*|*127*)
      AGREED=broken
      REPORT="  ours rc=$o_rc  gnu rc=$g_rc
$(head -5 "$p.ours.err")
$(head -5 "$p.gnu.err")"
      return 0 ;;
  esac
  if cmp -s "$p.ours.out" "$p.gnu.out" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$p.ours.out" | head -40)" "$(cat -A "$p.ours.err" | head -20)" \
    "$g_rc" "$(cat -A "$p.gnu.out" | head -40)" "$(cat -A "$p.gnu.err" | head -20)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached the program on one or both sides\n%s\n' "$LABEL" "$REPORT"
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

# `xfail_case REASON -- PROGRAM ARGS...`: a difference on purpose.
xfail_case() {
  local why=$1; shift
  [ "${1:-}" = -- ] && shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s)\n%s\n' "$LABEL" "$why" "$REPORT"
  elif [ "$AGREED" = broken ]; then
    report
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# Hold a lock on pid.ok while the program runs: `lockpre flock` or `lockpre fcntl`.
lockpre() {
  printf '%s' "rm -f lock.ready; python3 \"\$LOCKPY\" $1 pid.ok lock.ready & n=0; \
while [ ! -e lock.ready ] && [ \$n -lt 100 ]; do sleep 0.05; n=\$((n + 1)); done; [ -e lock.ready ]"
}

# ---------------------------------------------------------------------------
# A guard against vacuous agreement: the first case must find the fake
# world's processes, by name, on both sides.
# ---------------------------------------------------------------------------
run_case pgrep -l bash
if ! grep -q '^200 bash$' "$work/c1.gnu.out"; then
  echo "pgrep-diff: the reference did not see the fixture /proc:" >&2
  cat "$work/c1.gnu.out" "$work/c1.gnu.err" >&2
  exit 1
fi

# --- the pattern ------------------------------------------------------------
run_case pgrep bash
run_case pgrep -a bash
run_case pgrep -c bash
run_case pgrep -l -a bash
run_case pgrep -d , bash
run_case pgrep -d '' bash
run_case pgrep -d ' -- ' -l bash
run_case pgrep --delimiter=: agetty
run_case pgrep --delimiter= bash
run_case pgrep -d "$(printf '\351')" bash
run_case pgrep -d , -c bash
run_case pgrep 'ba.h'
run_case pgrep '^s'
run_case pgrep 'sh$'
run_case pgrep 'a|v'
run_case pgrep '(b|j)a'
run_case pgrep '[[:digit:]]'
run_case pgrep '[[:upper:]]'
run_case pgrep 'g{2}'
run_case pgrep -x bash
run_case pgrep -x bas
run_case pgrep -x 'a|bash'
run_case pgrep -x 'my (odd) name'
run_case pgrep -l 'odd'
run_case pgrep -a 'odd'
run_case pgrep ''
run_case pgrep -c ''
run_case pgrep -v ''
run_case pgrep -v bash
run_case pgrep -v -c bash
run_case pgrep -l -v bash
run_case pgrep -i BASH
run_case pgrep -i -x VIM
run_case pgrep -i 'S[A-Z]'
run_case pgrep -f notes
run_case pgrep -f -x 'vim notes.txt'
run_case pgrep -f -x vim
run_case pgrep -x -f 'sshd: .*'
LOCALE=C; run_case pgrep -i "$(printf 'CAF\303\211')"
LOCALE=C; run_case pgrep -l "$(printf '\351')"
run_case pgrep -f 'sshd: /usr'
run_case pgrep -f '\[kthreadd\]'
run_case pgrep -a -f '\[.*\]'
run_case pgrep -f defunct
run_case pgrep -a -f '<defunct>'
run_case pgrep -l -f 'agetty.*tty1'
run_case pgrep -a -f '\\u'
run_case pgrep -f 'caf.'
run_case pgrep -f 'caf..$'
run_case pgrep -f "$(printf '\344\270\200')"
run_case pgrep -f 'bad.'
LOCALE=C; run_case pgrep -f 'caf.'
LOCALE=C; run_case pgrep -a odd
LOCALE=C; run_case pgrep -f 'caf..'
LOCALE=C; run_case pgrep -i 'SYSTEMD'
run_case pgrep -i "$(printf 'CAF\303\211')"
run_case pgrep -f -i "$(printf 'CAF\303\211')"
# Process names are fifteen bytes: a longer plain pattern earns a warning.
run_case pgrep systemd-journal
run_case pgrep systemd-journald
run_case pgrep -c systemd-journald
run_case pgrep -f systemd-journald
run_case pgrep 'systemd-journald|x'
run_case pgrep 'systemd-journal[d]'
run_case pgrep -v systemd-journald
run_case pgrep -x systemd-journald
# Patterns that do not compile.
run_case pgrep '('
run_case pgrep '['
run_case pgrep 'a{1,0}'
run_case pgrep '*a'
run_case pgrep 'a**'
run_case pgrep '\'
run_case pgrep -x '('
run_case pgrep -x 'a)|(b'
run_case pgrep '[[:nope:]]'
run_case pgrep 'z-a]'
run_case pgrep '[z-a]'
run_case pgrep '(a)\2'
run_case pgrep '(a)\1'
run_case pgrep -c ')'

# --- newest and oldest ------------------------------------------------------
run_case pgrep -n
run_case pgrep -o
run_case pgrep -n bash
run_case pgrep -o bash
run_case pgrep -n -l agetty
run_case pgrep -o -a agetty
run_case pgrep -n -u alice
run_case pgrep -o -t pts/1
run_case pgrep -n -c ''
run_case pgrep -o -c nosuch
run_case pgrep -n -U 0
run_case pgrep -o -P 5
run_case pgrep -n -w java
run_case pgrep -n -o
run_case pgrep -n -v x
run_case pgrep -v -n x
run_case pgrep -o -o x
run_case pgrep -v -v x
run_case pgrep --newest --oldest x
run_case pgrep --inverse --oldest x

# --- IDs ---------------------------------------------------------------------
run_case pgrep -P 200
run_case pgrep -P 200,400
run_case pgrep -P 0
run_case pgrep -P 5 -l
run_case pgrep -P ''
run_case pgrep -P x
run_case pgrep -P 1,x
run_case pgrep -P ,
run_case pgrep -P 1,,2
run_case pgrep -P +200
run_case pgrep -P -1
run_case pgrep -P 99999999999999999999
run_case pgrep --parent 100 bash
run_case pgrep -g 200
run_case pgrep -g 0
run_case pgrep -g 0,400
run_case pgrep -g x
run_case pgrep --pgroup 1
run_case pgrep -s 200
run_case pgrep -s 0
run_case pgrep -s 151,400
run_case pgrep -s ''
run_case pgrep -s y
run_case pgrep --session 0 -l
run_case pgrep -u alice
run_case pgrep -u 1000
run_case pgrep -u alice,bob
run_case pgrep -u alice -l
run_case pgrep -u root
run_case pgrep -u nosuch
run_case pgrep -u ''
run_case pgrep -u alice,nosuch
run_case pgrep -u -1
run_case pgrep -u 4294967295
run_case pgrep -u +1000
run_case pgrep -u 01000
run_case pgrep -u "$(printf 'caf\303\251')"
run_case pgrep -u nobody -l
run_case pgrep --euid=bob
run_case pgrep -U root
run_case pgrep -U 1001
run_case pgrep -U bob,alice -c
run_case pgrep -U nosuch
run_case pgrep --uid 1000 vim
run_case pgrep -G alice
run_case pgrep -G 1000
run_case pgrep -G nosuch
run_case pgrep -G ''
run_case pgrep -G 0,1001
run_case pgrep --group=root
run_case pgrep -u root -U root
run_case pgrep -u alice -P 200
run_case pgrep -u alice -U bob
run_case pgrep -u alice -U alice -G alice -l
run_case pgrep -l -w -v java
run_case pgrep -u alice -v
run_case pgrep -u alice -v -c

# --- terminals ----------------------------------------------------------------
run_case pgrep -t pts/1
run_case pgrep -t pts/2,tty1
run_case pgrep -t tty1
run_case pgrep -t console
run_case pgrep -t ttyS0
run_case pgrep -t '?'
run_case pgrep -t /dev/pts/1
run_case pgrep -t pts/9
run_case pgrep -t ''
run_case pgrep -t pts/1 -l -v
run_case pgrep --terminal pts/2 make

# --- run states -----------------------------------------------------------------
run_case pgrep -r Z
run_case pgrep -r S,R
run_case pgrep -r DZ
run_case pgrep -r T -l
run_case pgrep -r I
run_case pgrep -r ''
run_case pgrep -r Z -v
run_case pgrep --runstates=R
WORLD=broken; run_case pgrep -r X -l
WORLD=broken; run_case pgrep -r 0 -l

# --- namespaces -----------------------------------------------------------------
run_case pgrep --ns 5
run_case pgrep --ns 500
run_case pgrep --ns 600 -l
run_case pgrep --ns 5 --nslist net
run_case pgrep --ns 500 --nslist net
run_case pgrep --ns 500 --nslist uts
run_case pgrep --ns 500 --nslist net,uts
run_case pgrep --ns 500 --nslist uts,net
run_case pgrep --ns 5 --nslist user
run_case pgrep --ns 5 --nslist time
run_case pgrep --ns 5 --nslist cgroup bash
run_case pgrep --ns 5 --nslist ipc,nope
run_case pgrep --ns 5 --nslist ''
run_case pgrep --nslist net x
run_case pgrep --ns 0
run_case pgrep --ns 0 -l
run_case pgrep --ns abc
run_case pgrep --ns -5
run_case pgrep --ns 99999
run_case pgrep --ns 1
run_case pgrep --ns 5 -w java
WORLD=broken; run_case pgrep --ns 1
WORLD=broken; run_case pgrep --ns 1 -l nopid
WORLD=broken; run_case pgrep --ns 0 -l

# --- cgroups --------------------------------------------------------------------
run_case pgrep --cgroup /user.slice/user-1000.slice/session-3.scope
run_case pgrep --cgroup /system.slice/ssh.service -l
run_case pgrep --cgroup x,/system.slice/ssh.service
run_case pgrep --cgroup /docker/abc
run_case pgrep --cgroup /
run_case pgrep --cgroup ''
run_case pgrep --cgroup /nonl -l
run_case pgrep --cgroup /gap -l
run_case pgrep --cgroup /one -l
run_case pgrep --cgroup /two -l
run_case pgrep --cgroup - -l
run_case pgrep --cgroup /a -l
run_case pgrep --cgroup 0::/init.scope
run_case pgrep --cgroup /init.scope -v -c

# --- age ------------------------------------------------------------------------
run_case pgrep -O 3000
run_case pgrep -O 3000 -l
run_case pgrep -O 0
run_case pgrep -O abc
run_case pgrep -O 99990
run_case pgrep -O 999999999
run_case pgrep -O -5 -c
run_case pgrep -O 2147483648
run_case pgrep -O 4294967296000 -c
run_case pgrep -O 5000 -n
run_case pgrep --older 9000 -l
# `now` started at exactly the uptime's tick, so its elapsed time is the
# previous process's; `future` started after it, so its elapsed time wraps.
run_case pgrep -O 1 -l now
run_case pgrep -O 1 -l future
run_case pgrep -O 1 -v -l

# --- signal handlers --------------------------------------------------------------
run_case pgrep -H
run_case pgrep -H -l
run_case pgrep -H --signal HUP
run_case pgrep -H --signal 1 -l
run_case pgrep -H --signal 0 -l
run_case pgrep -H --signal 64 -l
run_case pgrep -H --signal 65
run_case pgrep -H --signal QUIT -l
run_case pgrep --require-handler --signal USR1 -l
WORLD=broken; run_case pgrep -H -l
WORLD=broken; run_case pgrep -H -l hex
WORLD=broken; run_case pgrep -H -l shortsig

# --- ancestors and threads --------------------------------------------------------
run_case pgrep -A
run_case pgrep -A bash
run_case pgrep -A -l -v bash
run_case pgrep -A -c ''
run_case pgrep --ignore-ancestors sudo
run_case pgrep -w java
run_case pgrep -w -l java
run_case pgrep -w -c ''
run_case pgrep -w -P 5
run_case pgrep --lightweight -a java
WORLD=notask; run_case pgrep -w java
WORLD=notask; run_case pgrep -w -l ''

# --- PID files --------------------------------------------------------------------
run_case pgrep -F pid.ok
run_case pgrep -F pid.ok -l
run_case pgrep -F pid.space
run_case pgrep -F pid.nl
run_case pgrep -F pid.tab
run_case pgrep -F pid.junk
run_case pgrep -F pid.zero
run_case pgrep -F pid.neg
run_case pgrep -F pid.wrap
run_case pgrep -F pid.long
run_case pgrep -F pid.empty
run_case pgrep -F pid.nul
run_case pgrep -F pid.dir
run_case pgrep -F pid.fifo
run_case pgrep -F nosuch
run_case pgrep -F pid.java
run_case pgrep -F pid.java -w
run_case pgrep -F pid.thread -w
run_case pgrep -F pid.ok -F pid.junk
run_case pgrep -F pid.junk -F pid.ok
run_case pgrep -F pid.ok bash
run_case pgrep -F pid.ok vim
run_case pgrep -F pid.ok -v
run_case pgrep --pidfile=pid.ok -c
run_case pgrep -L -F pid.ok
run_case pgrep -L x
run_case pgrep --logpidfile -F pid.ok
PRE=$(lockpre flock); run_case pgrep -L -F pid.ok
PRE=$(lockpre fcntl); run_case pgrep -L -F pid.ok
PRE=$(lockpre flock); run_case pgrep -F pid.ok

# --- the command line ---------------------------------------------------------------
run_case pgrep
run_case pgrep a b
run_case pgrep -c
run_case pgrep -l
run_case pgrep -z
run_case pgrep -zx bash
run_case pgrep --nosuch
run_case pgrep --nosuch=1
run_case pgrep --s 1
run_case pgrep --sig KILL x
run_case pgrep --l
run_case pgrep --list
run_case pgrep --count=1 x
run_case pgrep -d
run_case pgrep --delimiter
run_case pgrep -h
run_case pgrep --help
run_case pgrep -h -z
run_case pgrep -z -h
run_case pgrep -?
run_case pgrep -i -i x
run_case pgrep -ii x
run_case pgrep -lc bash
run_case pgrep -c -- -bash
run_case pgrep -- -x
run_case pgrep -l -- bash
run_case pgrep bash -l
ENVS=(POSIXLY_CORRECT=1); run_case pgrep bash -l
ENVS=(POSIXLY_CORRECT=1); run_case pgrep -l bash
run_case pgrep --signal NOPE x
run_case pgrep --signal 9x x
run_case pgrep --signal '' x
run_case pgrep --signal 200 -H
run_case pgrep -q 1 x
run_case pgrep --queue 1 x
run_case pgrep --echo bash
run_case pgrep -e bash
# getopt's message escapes a byte that is not text, as every diagnostic here
# does (design-decisions §370); glibc writes it raw.
xfail_case "a byte that is not text is escaped" -- pgrep "-$(printf '\351')"
run_case pgrep "$(printf 'caf\351')"
ENVS=(LIBPROC_HIDE_KERNEL=1); run_case pgrep -l ''
ENVS=(LIBPROC_HIDE_KERNEL=1); run_case pgrep -c ''
ENVS=(LIBPROC_HIDE_KERNEL=); run_case pgrep -P 2
xfail_case "names this build" -- pgrep -V
xfail_case "names this build" -- pgrep --version
xfail_case "names this build" -- pkill -V
xfail_case "names this build" -- pidwait --version

# --- the broken world ---------------------------------------------------------------
WORLD=broken; run_case pgrep -l ''
WORLD=broken; run_case pgrep -a ''
WORLD=broken; run_case pgrep noparen
WORLD=broken; run_case pgrep -l short
WORLD=broken; run_case pgrep -l nopid
WORLD=broken; run_case pgrep -l -u 0
WORLD=broken; run_case pgrep -P 7
WORLD=broken; run_case pgrep -l -u 1000
WORLD=broken; run_case pgrep -l nostatus
WORLD=broken; run_case pgrep -l big
WORLD=broken; run_case pgrep -P -1
WORLD=broken; run_case pgrep -a nuls
WORLD=broken; run_case pgrep -a zomb
WORLD=broken; run_case pgrep -l 'a\) b'
WORLD=broken; run_case pgrep -l esc
WORLD=broken; run_case pgrep -l wrap
WORLD=broken; run_case pgrep -l overflow
WORLD=broken; run_case pgrep -n
WORLD=broken; run_case pgrep -o -l
WORLD=broken; run_case pgrep -A -l ''
WORLD=broken; run_case pgrep -w -l ''

# --- under other names ---------------------------------------------------------------
ARGV0=pkill; run_case pgrep -h
ARGV0=/usr/local/bin/pkill; run_case pgrep -z
ARGV0=lt-pkill; run_case pgrep -l
ARGV0=pidwait; run_case pgrep -h
ARGV0=lt-pidwait; run_case pgrep -v x
ARGV0=pidwait.exe; run_case pgrep -l bash
ARGV0=./pgrep; run_case pgrep --nosuch
ARGV0=/x/y/pgrep; run_case pgrep a b
ARGV0=pgrep; run_case pkill -h
ARGV0=PKILL; run_case pkill -9 x

# --- pkill ------------------------------------------------------------------------
run_case pkill -h
run_case pkill --help
run_case pkill
run_case pkill -l bash
run_case pkill -v bash
run_case pkill -w bash
run_case pkill -d , bash
run_case pkill -c ghostly
run_case pkill -9 -9 x
run_case pkill -NOPE x
run_case pkill --signal NOPE x
run_case pkill -e
run_case pkill -c -e nosuchprocess
WORLD=live; run_case pkill victim
WORLD=live; run_case pkill -e victim
WORLD=live; run_case pkill -c victim
WORLD=live; run_case pkill -e -c victim
WORLD=live; run_case pkill -9 -e victim
WORLD=live; run_case pkill -e victim -HUP
WORLD=live; run_case pkill -e -SIGUSR1 victim
WORLD=live; run_case pkill -e -usr2 victim
WORLD=live; run_case pkill -e -RTMIN+1 victim
WORLD=live; run_case pkill -e -0 victim
WORLD=live; run_case pkill -e -STOP victim
WORLD=live; run_case pkill -e --signal KILL victim
WORLD=live; run_case pkill -e --signal 9 victim
WORLD=live; run_case pkill -e --signal 200 victim
WORLD=live; run_case pkill -e -93 victim
WORLD=live; run_case pkill -e -94 victim
WORLD=live; run_case pkill -e ghost
WORLD=live; run_case pkill ghost
WORLD=live; run_case pkill -c ghost
WORLD=live; run_case pkill -e -n victim
WORLD=live; run_case pkill -e -o victim
WORLD=live; run_case pkill -e -f 'sleep 30'
WORLD=live; run_case pkill -e -x victim
WORLD=live; run_case pkill -e -P 1
WORLD=live; run_case pkill -e --inverse victim
WORLD=live; run_case pkill -e --inverse ghost
WORLD=live; run_case pkill -e -q 5 victim
WORLD=live; run_case pkill -e --queue=-1 victim
WORLD=live; run_case pkill -e -H victim
WORLD=live; run_case pkill -e -H -HUP victim
WORLD=live; run_case pkill -e -H -0 victim
WORLD=live; run_case pkill -e -u root
WORLD=live; run_case pkill -e -u root ghost
WORLD=live; run_case pkill -e --list-name victim
WORLD=live; run_case pkill -e --list-full victim
WORLD=live; run_case pkill -e -t '?' victim
WORLD=live; run_case pkill --echo --count victim
WORLD=live; run_case pkill -e -c --signal 0 victim
WORLD=broken; run_case pkill -e shortsig
WORLD=broken; run_case pkill -H -e hex
WORLD=broken; xfail_case "no signal to a PID below 1" -- pkill -e nopid
WORLD=broken; xfail_case "no signal to a PID below 1" -- pkill -e negpid
WORLD=broken; run_case pkill -e wrapneg

# --- pidwait ------------------------------------------------------------------------
run_case pidwait -h
run_case pidwait
run_case pidwait -l x
run_case pidwait -9 x
run_case pidwait --list-name x
run_case pidwait -e nosuchprocess
run_case pidwait -c -e nosuchprocess
LIVE='sleep 2'; WORLD=live; run_case pidwait victim
LIVE='sleep 2'; WORLD=live; run_case pidwait -e victim
LIVE='sleep 2'; WORLD=live; run_case pidwait -c -e victim
LIVE='sleep 2'; WORLD=live; run_case pidwait -e -n victim
WORLD=live; run_case pidwait -e ghost
WORLD=live; run_case pidwait -e -c ghost
WORLD=live; run_case pidwait -e -o victim
LIVE='sleep 2'; WORLD=live; run_case pidwait --echo --list-full victim
WORLD=broken; run_case pidwait -e nopid
WORLD=broken; run_case pidwait -e wrapneg
WORLD=broken; run_case pidwait -e negpid
WORLD=broken; run_case pgrep -l negpid
WORLD=broken; run_case pgrep -a -d , neg

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
