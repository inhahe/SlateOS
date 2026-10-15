#!/usr/bin/env bash
# shellcheck disable=SC2016  # the sh -c scripts expand in the inner shell
#
# Differential test: SlateOS's `top` (`userspace/coreutils/src/bin/top/`,
# procps-ng 4.0.4's `src/top/top.c`) against procps-ng 4.0.4's own `top`,
# built as Ubuntu builds it (`--disable-modern-top`) with no logind and no
# NUMA, as SlateOS has neither (`procps-ref.sh`).
#
# ## What is pinned
#
# Each case runs both programs in a user, mount and PID namespace of its own
# over the same fixture world:
#
#   /proc            a fake tree from `procps_fixture.py` -- the processes,
#                    `stat`, `meminfo`, `loadavg`, `uptime`, the kernel's
#                    release -- copied fresh for every run, so that a case
#                    that changes it between frames starts from the same state
#   /etc/passwd ...  the user and group databases, and an NSS reading only them
#   /run/utmp        empty: "0 users"
#   /dev/pts         a new instance, three terminals held open, so that the
#                    TTY column's names exist; the case's own is the fourth
#   the clock        frozen by a preloaded shim (`time`, `gettimeofday`,
#                    `CLOCK_REALTIME`), so the first line's time never moves
#   HOME             a fresh directory, which may be given an rcfile; what is
#                    in it afterwards -- a written rcfile -- is compared too
#
# The PID namespace keeps `k` and `r` from reaching anything real: the
# fixture's PIDs exist nowhere in it.
#
# The option cases run `top -b` (and a few that never get that far) with
# standard output to a file. The screen cases run `top` on a pseudo-terminal
# (`curses-ptyrun.py`), type keys at it, resize it, signal it and change its
# `/proc` between frames, and compare every byte sent to the terminal.
#
# ## Normalised
#
# Upstream's version string, `procps-ng 4.0.4`, is ours, `SlateOS coreutils
# 0.1.0`, in its output before comparing (deliberate difference 3): it
# appears on the help screen as well as in `-V`, and every help screen would
# otherwise differ for it alone.
#
# ## Cases that differ on purpose
#
# The help screen on a terminal too narrow for the version string: cut
# short there, upstream's `procps-ng 4.0.4` cannot be replaced by ours.
#
# Run `OURS=$HOME/.cache/slateos-procps-ref/procps-ng-4.0.4-top/src/top/top
# ./scripts/top-diff.sh` to check that the harness still discriminates.
# `DIFF_FAIL_FAST=1` stops the run at the first difference.
set -u

DIFF_PROG='top'
DIFF_NEED='python3 unshare timeout cc'
DIFF_TIMEOUT=${DIFF_TIMEOUT:-5400}
# shellcheck source=procps-ref.sh
. "$(dirname "$0")/procps-ref.sh"
DIFF_REF=$PROCPS_REF_TOP
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

runner=$root/scripts/curses-ptyrun.py
pass=0; fail=0; xfail=0; xpass=0; retried=0

if ! unshare -mUrpf --propagation private sh -c 'mount --bind /dev/null /proc/uptime' 2>/dev/null; then
  echo "top-diff: cannot mount over /proc in a namespace here; skipping"
  exit 0
fi

work=$DIFF_TMP/work
mkdir -p "$work"

# ---------------------------------------------------------------------------
# The clock, frozen.
shim=$work/fixtime.so
cat > "$work/fixtime.c" <<'EOF'
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include <sys/time.h>
#include <time.h>

static long long base (void)
{
  const char *s = getenv ("FIXTIME");
  return s ? atoll (s) : 1790000000;
}

time_t time (time_t *t)
{
  time_t v = base ();
  if (t)
    *t = v;
  return v;
}

int gettimeofday (struct timeval *tv, void *tz)
{
  (void) tz;
  if (tv)
    {
      tv->tv_sec = base ();
      tv->tv_usec = 0;
    }
  return 0;
}

int clock_gettime (clockid_t c, struct timespec *ts)
{
  static int (*real) (clockid_t, struct timespec *);
  if (c == CLOCK_REALTIME || c == CLOCK_REALTIME_COARSE)
    {
      ts->tv_sec = base ();
      ts->tv_nsec = 0;
      return 0;
    }
  if (!real)
    real = dlsym (RTLD_NEXT, "clock_gettime");
  return real (c, ts);
}
EOF
if ! cc -O2 -shared -fPIC -o "$shim" "$work/fixtime.c" -ldl; then
  echo "top-diff: could not build the clock shim" >&2
  exit 1
fi

# Three terminals held open, for the TTY column's names.
holdpy=$work/hold.py
cat > "$holdpy" <<'PY'
import os, sys, time
n, ready = int(sys.argv[1]), sys.argv[2]
fds = [os.openpty() for _ in range(n)]
with open(ready, "w") as f:
    f.write(" ".join(os.ttyname(s) for _, s in fds) + "\n")
time.sleep(600)
PY

# ---------------------------------------------------------------------------
# The worlds.
mkworld=$work/mkworld.py
cat > "$mkworld" <<'PY'
import os
import sys

dest = sys.argv[1]
sys.path.insert(0, sys.argv[2])
from procps_fixture import HZ, PTS, TTY, P  # noqa: E402
import procps_fixture  # noqa: E402

MEMINFO = (b"MemTotal:       16303232 kB\nMemFree:         9122340 kB\n"
           b"MemAvailable:   12934520 kB\nBuffers:          212344 kB\n"
           b"Cached:          3633516 kB\nSwapCached:            0 kB\n"
           b"Active:          2861960 kB\nInactive:        3593936 kB\n"
           b"Active(anon):    2552372 kB\nInactive(anon):        0 kB\n"
           b"Active(file):     309588 kB\nInactive(file):  3593936 kB\n"
           b"Unevictable:           0 kB\nMlocked:               0 kB\n"
           b"SwapTotal:       4194304 kB\nSwapFree:        4100000 kB\n"
           b"Dirty:               120 kB\nWriteback:             0 kB\n"
           b"AnonPages:       2610032 kB\nMapped:           711452 kB\n"
           b"Shmem:             21040 kB\nKReclaimable:     190212 kB\n"
           b"Slab:             301880 kB\nSReclaimable:     190212 kB\n"
           b"SUnreclaim:       111668 kB\nKernelStack:       11312 kB\n"
           b"PageTables:        28004 kB\nCommitLimit:    12345920 kB\n"
           b"Committed_AS:    6223388 kB\nVmallocTotal:   34359738367 kB\n"
           b"VmallocUsed:       53432 kB\nHugePages_Total:       0\n"
           b"Hugepagesize:       2048 kB\n")

# Terabytes of memory, for the scales.
BIGMEM = (b"MemTotal:       8589934592 kB\nMemFree:        1073741824 kB\n"
          b"MemAvailable:   4294967296 kB\nBuffers:        33554432 kB\n"
          b"Cached:         268435456 kB\nSwapTotal:      2147483648 kB\n"
          b"SwapFree:       2147480000 kB\nShmem:          1048576 kB\n"
          b"SReclaimable:   16777216 kB\n")

# No MemAvailable (before Linux 3.14) and no swap.
OLDMEM = (b"MemTotal:        2048000 kB\nMemFree:          100000 kB\n"
          b"Buffers:           50000 kB\nCached:           400000 kB\n"
          b"SwapTotal:             0 kB\nSwapFree:              0 kB\n"
          b"Shmem:              1000 kB\nSReclaimable:      20000 kB\n")


def stat(t):
    """`/proc/stat` T frames on: two CPUs, 300 ticks each a frame."""
    def cpu(name, base, step):
        v = [b + s * t for b, s in zip(base, step)]
        return name + b" " + b" ".join(b"%d" % x for x in v) + b"\n"
    return (cpu(b"cpu ", (10000, 500, 3000, 900000, 1000, 0, 200, 50, 0, 0),
                (200, 10, 60, 300, 20, 0, 5, 5, 0, 0))
            + cpu(b"cpu0", (5000, 250, 1500, 450000, 500, 0, 100, 25, 0, 0),
                  (100, 5, 30, 150, 10, 0, 3, 2, 0, 0))
            + cpu(b"cpu1", (5000, 250, 1500, 450000, 500, 0, 100, 25, 0, 0),
                  (100, 5, 30, 150, 10, 0, 2, 3, 0, 0))
            + b"intr 12345\nctxt %d\nbtime 1700000000\nprocesses 4000\n"
              b"procs_running 2\nprocs_blocked 1\n" % (67890 + 500 * t))


def procs(t):
    """The main world's processes, T frames on (a frame is three seconds)."""
    alice = dict(uid=1000, gid=1000)
    return [
        P(1, "systemd", ppid=0, start=10, utime=250 + 3 * t, stime=100 + t,
          cutime=9000, cstime=4000, vsize=170000000, rss=3000, vmdata=8000,
          cmdline=b"/sbin/init\0splash\0", wchan=b"ep_poll"),
        P(2, "kthreadd", ppid=0, pgrp=0, sid=0, flags=2129984, cmdline=b"",
          environ=b"", statm=b"0 0 0 0 0 0 0\n", wchan=b"kthreadd"),
        P(3, "rcu_gp", state="I", ppid=2, pgrp=0, sid=0, flags=69238880, prio=0,
          nice=-20, cmdline=b"", start=5, statm=b"0 0 0 0 0 0 0\n"),
        P(14, "ksoftirqd/0", ppid=2, pgrp=0, sid=0, flags=69238880, cmdline=b"",
          start=6, utime=40, stime=900 + 9 * t, statm=b"0 0 0 0 0 0 0\n",
          wchan=b"smpboot_thread_fn"),
        P(100, "sshd", ppid=1, start=2000, utime=12, stime=8, vsize=15000000,
          rss=1500, vmdata=600,
          cmdline=b"sshd: /usr/sbin/sshd -D [listener] 0 of 10-100 startups\0"),
        P(150, "sudo", ppid=200, pgrp=150, sid=200, tty=PTS(1), tpgid=300,
          start=99000 * HZ, cmdline=b"sudo\0-i\0", vsize=11000000, rss=800),
        P(200, "bash", ppid=100, tty=PTS(1), tpgid=300, start=90000 * HZ,
          utime=40, stime=20, vsize=9100000, rss=1300, cmdline=b"-bash\0",
          environ=b"USER=alice\0HOME=/home/alice\0LANG=C.UTF-8\0", **alice),
        P(300, "vim", ppid=200, pgrp=300, sid=200, tty=PTS(1), tpgid=300,
          start=95000 * HZ, utime=500 + 4 * t, stime=60, vsize=30000000,
          rss=5000, vmdata=2400, cmdline=b"vim\0notes.txt\0", **alice),
        P(301, "sleep", state="T", ppid=200, pgrp=301, sid=200, tty=PTS(1),
          tpgid=300, start=96000 * HZ, cmdline=b"sleep\0" + b"1000\0",
          wchan=b"hrtimer_nanosleep", **alice),
        P(400, "bash", ppid=100, tty=PTS(2), tpgid=400, uid=1001, gid=1001,
          start=80000 * HZ, utime=7, stime=3, cmdline=b"-bash\0",
          vsize=9000000, rss=1100),
        P(401, "make", state="Z", ppid=400, pgrp=400, sid=400, tty=PTS(2),
          tpgid=400, uid=1001, gid=1001, start=81000 * HZ, cmdline=b"",
          utime=900, stime=300, vsize=0, rss=0, wchan=b"0",
          statm=b"0 0 0 0 0 0 0\n"),
        P(500, "java", ppid=1, pgrp=500, sid=500, threads=3, start=3000,
          utime=60000 + 30 * t, stime=9000 + 10 * t, vsize=4000000000,
          rss=200000, vmdata=150000, tasks=[500, 501, 502], nice=5, prio=25,
          cmdline=b"/usr/bin/java\0-Xmx2g\0-jar\0app.jar\0", **alice),
        P(600, "my (odd) name", ppid=1, start=4000, uid=1002, gid=1002,
          cmdline=b"odd\tname\0caf\xc3\xa9\0\xe4\xb8\x80\xe4\xba\x8c\0bad\xff\0",
          environ=b"X=\x01\x02\0"),
        P(601, "caf\xc3\xa9 \xe4\xb8\x80", ppid=600, start=4100, uid=1003,
          gid=1003, cmdline=b"caf\xc3\xa9\0\xe4\xb8\x80\xe4\xba\x8c\0"),
        P(700, "agetty", ppid=1, tty=TTY(1), tpgid=700, start=5000, nice=-5,
          prio=15,
          cmdline=b"/sbin/agetty\0-o\0-p -- \\u\0--noclear\0tty1\0linux\0"),
        P(702, "agetty", ppid=1, tty=(4 << 8) | 64, tpgid=702, start=5002,
          policy=1, rtprio=50, prio=-51, cmdline=b"/sbin/agetty\0ttyS0\0"),
        P(703, "watchdogd", ppid=2, pgrp=0, sid=0, policy=1, rtprio=99,
          prio=-100, cmdline=b"", start=7, statm=b"0 0 0 0 0 0 0\n"),
        P(800, "spinner", state="R", ppid=1, start=99000 * HZ,
          utime=50000 + 150 * t, stime=40000 + 40 * t, uid=1004, cpu=1,
          policy=3, vsize=2000000, rss=600, cmdline=b"spinner\0--fast\0"),
        P(801, "worker", state="D", ppid=1, start=20000, uid=1003, gid=1003,
          cmdline=b"worker\0", policy=5, wchan=b"io_schedule"),
        P(900, "bigdb", ppid=1, start=9000, uid=0, vsize=3 << 40,
          rss=(10 << 30) // 4096, vmdata=(8 << 30) // 1024,
          cmdline=b"/opt/bigdb/bin/bigdb\0--config\0/etc/bigdb.conf\0"),
        P(32768, "tiny", ppid=1, start=6000, uid=65534, gid=65534,
          cmdline=b"tiny\0", vsize=1000000, rss=10, oom=(666, -1000)),
        P(4194303, "lastpid", ppid=1, start=99999 * HZ, uid=1000, gid=1000,
          cmdline=b"lastpid\0" + b"x" * 300 + b"\0"),
    ]


EXTRA = {"loadavg": b"0.52 0.58 0.59 2/345 4000\n",
         "sys/kernel/osrelease": b"6.6.87.2-microsoft-standard-WSL2\n"}


def world(name, ps, t=0, meminfo=MEMINFO, uptime=100000.0, extra=None):
    e = dict(EXTRA)
    e["stat"] = stat(t)
    e["meminfo"] = meminfo
    e.update(extra or {})
    procps_fixture.world(dest, name, ps, uptime=uptime + 3.0 * t, stat=False,
                         meminfo=False, extra=e)


world("main", procs(0))
world("later", procs(1), t=1)
world("bigmem", procs(0), meminfo=BIGMEM)
world("oldmem", procs(0), meminfo=OLDMEM)
world("one", [P(1, "init", ppid=0, cmdline=b"/sbin/init\0", vsize=1000000, rss=100)])

# A chain sixty deep and two hundred more, for the forest and for scrolling.
many = [P(1, "init", ppid=0, cmdline=b"/sbin/init\0", vsize=1000000, rss=100)]
for i in range(60):
    many.append(P(1000 + i, "level%d" % i, ppid=1 if i == 0 else 999 + i,
                  start=100 + i, cmdline=b"level\0%d\0" % i, vsize=1000000 + i,
                  rss=10 + i, utime=i))
for i in range(200):
    many.append(P(2000 + i, "proc%03d" % i, ppid=1 + (i % 3) * 999 if i % 3 else 1,
                  start=500 + i, uid=(0, 1000, 1001)[i % 3], cmdline=b"proc\0%d\0" % i,
                  vsize=(i + 1) * 1000000, rss=(i * 37) % 5000, utime=(i * 13) % 700))
world("many", many)

# Files the library reads that are missing or empty.
world("nostat", procs(0)[:5], extra={"stat": b""})
world("nomeminfo", procs(0)[:5], meminfo=b"")

# The files a frame changes: the main world's that differ from the later's.
changed = []
for base, _dirs, files in os.walk(f"{dest}/later/proc"):
    for f in files:
        rel = os.path.relpath(os.path.join(base, f), f"{dest}/later/proc")
        a = f"{dest}/main/proc/{rel}"
        with open(os.path.join(base, f), "rb") as x:
            later = x.read()
        try:
            with open(a, "rb") as x:
                now = x.read()
        except OSError:
            now = None
        if later != now:
            changed.append(rel)
with open(f"{dest}/changed", "w", encoding="utf-8") as out:
    out.write("".join(r + "\n" for r in sorted(changed)))
PY
worlds=$work/worlds
python3 "$mkworld" "$worlds" "$root/scripts" || { echo "top-diff: mkworld failed" >&2; exit 1; }

# One step that moves the main world a frame on.
later_step=
while IFS= read -r rel; do
  later_step=${later_step:+$later_step;}copy:$worlds/later/proc/$rel:/proc/$rel
done < "$worlds/changed"

etc=$work/etc
mkdir -p "$etc"
cat > "$etc/passwd" <<'EOF'
root:x:0:0:root:/root:/bin/bash
alice:x:1000:1000:Alice:/home/alice:/bin/bash
bob:x:1001:1001::/home/bob:/bin/sh
averyveryveryverylongusername123:x:1002:1002::/:/bin/sh
café:x:1003:1003::/:/bin/sh
twentycharacterslong:x:1004:1004::/:/bin/sh
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
EOF
cat > "$etc/group" <<'EOF'
root:x:0:
alice:x:1000:
bob:x:1001:
longgroup:x:1002:
café:x:1003:
nogroup:x:65534:
EOF
printf 'passwd: files\ngroup: files\nshadow: files\n' > "$etc/nsswitch.conf"
: > "$etc/empty"

# ---------------------------------------------------------------------------
# Running a side.
WORLD=main
TERMV=xterm-256color
LOC=C
TZV=UTC
RC=
ENVX=()
reset_knobs() {
  WORLD=main; TERMV=xterm-256color; LOC=C; TZV=UTC; RC=; ENVX=()
}

rcdir=$work/rc
mkdir -p "$rcdir"
home=$work/home
wcopy=$work/w

# prep: the world copied fresh, and a home with RC in it -- `NAME` at the
# usual place, `NAME:legacy` as ~/.toprc, `NAME:xdg` under XDG_CONFIG_HOME.
prep() {
  rm -rf "$wcopy" "$home"
  cp -a "$worlds/$WORLD" "$wcopy"
  mkdir -p "$home"
  case $RC in
    '') ;;
    *:legacy) cp "$rcdir/${RC%:*}" "$home/.toprc" ;;
    *:xdg) mkdir -p "$home/xdg/procps"; cp "$rcdir/${RC%:*}" "$home/xdg/procps/toprc" ;;
    *) mkdir -p "$home/.config/procps"; cp "$rcdir/$RC" "$home/.config/procps/toprc" ;;
  esac
}

# ns SIDE CMD...: CMD in a namespace of its own over the case's world.
ns() {
  local s=$1; shift
  local -a xdg=()
  case $RC in *:xdg) xdg=("XDG_CONFIG_HOME=$home/xdg") ;; esac
  diff_run timeout -k 2 60 unshare -mUrpf --propagation private sh -c '
      w=$1 e=$2 hold=$3; shift 3
      mount -t devpts -o newinstance,ptmxmode=0666 devpts /dev/pts || exit 125
      mount --bind /dev/pts/ptmx /dev/ptmx || exit 125
      python3 "$hold" 3 "$w/ready" &
      n=0
      while [ ! -s "$w/ready" ] && [ "$n" -lt 200 ]; do sleep 0.05; n=$((n + 1)); done
      [ -s "$w/ready" ] || exit 125
      mount --bind "$e/passwd" /etc/passwd || exit 125
      mount --bind "$e/group" /etc/group || exit 125
      mount --bind "$e/nsswitch.conf" /etc/nsswitch.conf || exit 125
      if [ -e /run/utmp ]; then mount --bind "$e/empty" /run/utmp || exit 125; fi
      mount --bind "$w/proc" /proc || exit 125
      cd "$w/cwd" || exit 125
      exec "$@"' _ "$wcopy" "$etc" "$holdpy" \
    env -i "PATH=$bindir/$s:/usr/bin:/bin" "TERM=$TERMV" "LC_ALL=$LOC" "TZ=$TZV" \
      "HOME=$home" "LD_PRELOAD=$shim" "${xdg[@]}" "${ENVX[@]}" "$@"
}

# What the run left in HOME: every entry with its mode, and each file.
collect() {
  local s=$1
  ( cd "$home" && find . -mindepth 1 -printf '%M %p\n' | LC_ALL=C sort
    find . -type f | LC_ALL=C sort | while IFS= read -r f; do
      printf '== %s\n' "$f"; cat "$f"
    done ) > "$DIFF_TMP/$s.home"
  if [ "$s" = gnu ]; then
    for f in out err; do
      LC_ALL=C sed -i 's/procps-ng 4\.0\.4/SlateOS coreutils 0.1.0/g' "$DIFF_TMP/$s.$f"
    done
  fi
}

# once_side SIDE ARGS...: top ARGS with no terminal.
once_side() {
  local s=$1; shift
  prep
  ns "$s" top "$@" < /dev/null > "$DIFF_TMP/$s.out" 2> "$DIFF_TMP/$s.err"
  printf '%s\n' "$?" > "$DIFF_TMP/$s.rc"
  collect "$s"
}

# screen_side SIDE SIZE STEP... -- ARGS...: top ARGS on a terminal of SIZE.
screen_side() {
  local s=$1 size=$2; shift 2
  local -a steps=()
  while [ "$1" != -- ]; do steps+=(--then "$1"); shift; done
  shift
  prep
  ns "$s" python3 "$runner" "${steps[@]}" "${size%x*}" "${size#*x}" \
    "$DIFF_TMP/$s.out" "$DIFF_TMP/$s.err" top "$@" < /dev/null
  printf '%s\n' "$?" > "$DIFF_TMP/$s.rc"
  collect "$s"
}

same() {
  local f
  for f in out err rc home; do
    cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" || return 1
  done
}

show() {
  local s f
  for s in ours gnu; do
    printf -- '--- %s: status %s\n' "$s" "$(cat "$DIFF_TMP/$s.rc")"
    printf 'err:\n'; head -5 "$DIFF_TMP/$s.err"
  done
  for f in out home; do
    if ! cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f"; then
      printf '%s: ' "$f"
      cmp "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" | head -1
      python3 - "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" <<'PY'
import sys
a, b = (open(p, "rb").read() for p in sys.argv[1:3])
i = next((k for k in range(min(len(a), len(b))) if a[k] != b[k]), min(len(a), len(b)))
lo = max(0, i - 120)
print("  ours:", repr(a[lo:i + 120]))
print("  gnu: ", repr(b[lo:i + 120]))
PY
    fi
  done
}

judge() {
  local label=$1 want=$2 why=${3:-}
  if same; then
    if [ "$want" = differ ]; then
      xpass=$((xpass+1))
      printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
    else
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
    fi
  elif [ "$want" = differ ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$label" "$why"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    show
    [ -n "${DIFF_FAIL_FAST:-}" ] && exit 1
  fi
  return 0
}

label_of() {
  local l="$1 [$WORLD"
  [ "$LOC" != C ] && l="$l, $LOC"
  [ "$TERMV" != xterm-256color ] && l="$l, $TERMV"
  [ "$TZV" != UTC ] && l="$l, TZ=$TZV"
  [ -n "$RC" ] && l="$l, rc=$RC"
  [ "${#ENVX[@]}" -gt 0 ] && l="$l, ${ENVX[*]}"
  printf '%s]' "$l"
}

# wanted LABEL: whether DIFF_ONLY (a substring of the label) lets the case
# run at all.
wanted() {
  case $1 in *"${DIFF_ONLY:-}"*) return 0 ;; esac
  return 1
}

# kept LABEL: both sides' results copied into DIFF_KEEP, when it is set.
kept() {
  [ -n "${DIFF_KEEP:-}" ] || return 0
  local n s f
  n=$(printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-80)
  mkdir -p "$DIFF_KEEP"
  for s in ours gnu; do
    for f in out err rc home; do cp "$DIFF_TMP/$s.$f" "$DIFF_KEEP/$n.$s.$f"; done
  done
}

once() {
  local label
  label=$(label_of "$1"); shift
  wanted "$label" || return 0
  once_side ours "$@"
  once_side gnu "$@"
  kept "$label"
  judge "$label" same
}

screen() {
  local label
  label=$(label_of "$1, $2"); shift
  wanted "$label" || return 0
  screen_side ours "$@"
  screen_side gnu "$@"
  # Both sides killed by the time limit, differing only in how many frames
  # they drew while waiting: a loaded machine let a key arrive between a
  # read and the `tcflush` after it, which both programs discard, and both
  # then wait for a `q` that never comes. Once more, before judging.
  if ! same && [ "$(cat "$DIFF_TMP/ours.rc")" = 137 ] && [ "$(cat "$DIFF_TMP/gnu.rc")" = 137 ]; then
    retried=$((retried+1))
    printf 'RETRY %s -- both sides hung\n' "$label"
    screen_side ours "$@"
    screen_side gnu "$@"
  fi
  kept "$label"
  judge "$label" same
}

# xscreen WHY LABEL SIZE STEP... -- ARGS...: a screen case that differs on
# purpose.
xscreen() {
  local why=$1 label
  label=$(label_of "$2, $3"); shift 2
  wanted "$label" || return 0
  screen_side ours "$@"
  screen_side gnu "$@"
  kept "$label"
  judge "$label" differ "$why"
}

# ---------------------------------------------------------------------------
# rcfiles, written by the reference: its defaults, and a window customised.
mkrc() {
  local name=$1; shift
  RC=
  prep
  ns gnu python3 "$runner" "$@" --then 'keys:W' --then 'keys:q' 24 80 \
    "$work/rcgen.out" "$work/rcgen.err" top < /dev/null
  if [ ! -s "$home/.config/procps/toprc" ]; then
    echo "top-diff: the reference wrote no rcfile for '$name'" >&2
    cat "$work/rcgen.err" >&2
    exit 1
  fi
  cp "$home/.config/procps/toprc" "$rcdir/$name"
}
mkrc default
mkrc custom --then 'keys:A' --then 'keys:z' --then 'keys:x' --then 'keys:b' \
  --then 'keys:c' --then 'keys:i' --then 'keys:S' --then 'keys:V' --then 'keys:E' \
  --then 'keys:e' --then 'keys:1' --then 'keys:t' --then 'keys:m' --then 'keys:R' \
  --then 'keys:<' --then 'keys:d' --then 'keys:1.5\n' --then 'keys:o' \
  --then 'keys:COMMAND=a\n' --then 'keys:a' --then 'keys:j' --then 'keys:H'
mkrc alt --then 'keys:A' --then 'keys:w' --then 'keys:G' --then 'keys:Xyz\n' \
  --then 'keys:f' --then 'keys:\e[B' --then 'keys: ' --then 'keys:\e[B' --then 'keys:s' \
  --then 'keys:q' --then 'keys:X' --then 'keys:2\n' --then 'keys:0' --then 'keys:I'
python3 - "$rcdir" <<'PY'
import sys
d = sys.argv[1]
with open(f"{d}/default", "rb") as f:
    text = f.read()
lines = text.split(b"\n")


def put(name, data):
    with open(f"{d}/{name}", "wb") as f:
        f.write(data)


put("trunc3", b"\n".join(lines[:3]) + b"\n")
put("trunc1", lines[0] + b"\n")
put("empty", b"")
put("garbage", b"\x00\xff\x01 not a toprc at all\n" * 3)
put("crlf", text.replace(b"\n", b"\r\n"))
for old in (b"a", b"f", b"i", b"j", b"z"):
    put("id" + old.decode(), text.replace(b"Id:k,", b"Id:" + old + b",", 1))
put("delayneg", text.replace(b"Delay_time=3.0", b"Delay_time=-2.0", 1))
put("delaybig", text.replace(b"Delay_time=3.0", b"Delay_time=1e9", 1))
put("curwin", text.replace(b"Curwin=0", b"Curwin=7", 1))
put("insp", text + b"pipe\tEcho\techo pid %d here\n"
     b"file\tStatus\t/proc/%d/status\nfile\tNothing\t/nonexistent/%d\n"
     b"pipe\tNo\x01Command\t\nnonsense line\n")
PY

# ---------------------------------------------------------------------------
# The command line, in batch mode.
once "-b -n 1" -b -n 1
once "-bn1" -bn1
once "--batch-mode --iterations=1" --batch-mode --iterations=1
once "-b -n 2 -d 0.1: two frames" -b -n 2 -d 0.1
for opt in -c -H -i -S -s -1 '-E k' '-E m' '-E g' '-E t' '-E p' '-E e' '-E x' \
           '-e k' '-e m' '-e g' '-e t' '-e p' '-e e' '-o %MEM' '-o PID' '-o -PID' \
           '-o +RES' '-o COMMAND' '-o USER' '-o TIME+' '-o bogus' '-o' \
           '-p 1' '-p 1,500,800' '-p 99999' '-p 1 -p 2' '-p abc' '-p 0' '-p 1,,2' \
           '-u root' '-u alice' '-u 1000' '-u !alice' '-u nosuch' '-u 99999' \
           '-U alice' '-U bob' '-u alice -U bob' '-w' '-w 200' '-w 40' '-w 0' \
           '-w 513' '-w abc' '-d 0' '-d -1' '-d abc' '-d 1.5' '-d 1,5' \
           '-c -H -i' '-H -o PID' '-S -o TIME+' '-ciS' '--cmdline-toggle' \
           '--threads-show' '--idle-toggle' '--accum-time-toggle' '--secure-mode' \
           '--single-cpu-toggle' '--scale-summary-mem=g' '--scale-task-mem=m' \
           '--sort-override=%CPU' '--pid=1,2' '--filter-only-euser=alice' \
           '--filter-any-user=bob' '--width' '--width=100' '--delay=0.5' '--wid'; do
  # shellcheck disable=SC2086  # each is a list of arguments
  once "-b -n 1 $opt" -b -n 1 $opt
done
for args in '-n 0 -b' '-n -1 -b' '-n abc -b' '-h' '--help' '-V' '--version' \
            '-v' '-O' '--list-fields' '-x' '--bogus' 'operand' '-b operand' \
            '-b -n 1 -- -x' '-n 1' '-p' '-d' '-u' '-b -n 1 -u alice -u bob' \
            '-b -n 1 -p 1 -u root' '-hV' '-bh' '-bV' '-b -n 1 -bb'; do
  # shellcheck disable=SC2086  # each is a list of arguments
  once "$args" $args
done
for WORLD in many bigmem oldmem one nostat nomeminfo later; do
  once "-b -n 1" -b -n 1
  once "-b -n 1 -H -c" -b -n 1 -H -c
done
WORLD=bigmem
for e in k m g t p e; do
  once "-b -n 1 -E $e -e $e" -b -n 1 -E "$e" -e "$e"
done
WORLD=main
for LOC in C.UTF-8 POSIX no_SUCH.locale; do
  once "-b -n 1 -c" -b -n 1 -c
  once "-b -n 1 -c -w 512" -b -n 1 -c -w 512
done
LOC=C
for TZV in America/New_York Asia/Kolkata Nowhere/Nothing; do
  once "-b -n 1" -b -n 1
done
TZV=UTC
ENVX=(COLUMNS=120 LINES=10); once "-b -n 1" -b -n 1
ENVX=(COLUMNS=30); once "-b -n 1" -b -n 1
ENVX=(COLUMNS=x); once "-b -n 1 -w" -b -n 1 -w
ENVX=(COLUMNS=600); once "-b -n 1 -w" -b -n 1 -w
ENVX=()
for RC in default custom alt trunc3 trunc1 empty garbage crlf ida idf idi idj idz \
          delayneg delaybig curwin insp default:legacy custom:xdg; do
  once "-b -n 1" -b -n 1
done
RC=custom
once "-b -n 1 -c -H (toggled back)" -b -n 1 -c -H
once "-b -n 1 -d 2" -b -n 1 -d 2
RC=

# ---------------------------------------------------------------------------
# The screen.
for TERMV in xterm-256color xterm vt100 linux screen dumb ansi no-such-terminal; do
  screen "one frame, q" 24x80 'keys:q' --
done
TERMV=xterm-256color
for size in 30x100 10x40 5x20 3x80 50x200 24x512 24x600 1x1; do
  screen "one frame, q" "$size" 'keys:q' --
done
LOC=C.UTF-8
screen "one frame, q" 24x80 'keys:q' --
screen "c, q" 24x120 'keys:c' 'keys:q' --
LOC=C

# Every key on its own, then q.
for key in B E e H I 0 '!' 1 2 3 4 5 C l m t '<' '>' b c i J j R S V x y z M N P T \
           A a w '-' _ '=' + '\n' ' ' K '\x05' '\x0f'; do
  screen "'$key', q" 24x80 "keys:$key" 'keys:q' --
done
# Keys that cycle, several times over.
screen "t t t t" 24x80 'keys:t' 'keys:t' 'keys:t' 'keys:t' 'keys:q' --
screen "m m m m" 24x80 'keys:m' 'keys:m' 'keys:m' 'keys:m' 'keys:q' --
screen "E x6" 24x80 'keys:E' 'keys:E' 'keys:E' 'keys:E' 'keys:E' 'keys:E' 'keys:q' --
screen "e x6" 24x80 'keys:e' 'keys:e' 'keys:e' 'keys:e' 'keys:e' 'keys:e' 'keys:q' --
screen "^E x4" 24x80 'keys:\x05' 'keys:\x05' 'keys:\x05' 'keys:\x05' 'keys:q' --
screen "< x14" 24x80 'keys:<<<<<<<<<<<<<<' 'keys:q' --
screen "> x14" 24x80 'keys:>>>>>>>>>>>>>>' 'keys:q' --
screen "1 4 ! 4" 24x120 'keys:1' 'keys:4' 'keys:!' 'keys:4' 'keys:q' --
screen "t 1 4" 40x160 'keys:t' 'keys:1' 'keys:4' 'keys:q' --
screen "V v (collapse) " 24x80 'keys:V' 'keys:v' 'keys:q' --
screen "c V H" 24x120 'keys:c' 'keys:V' 'keys:H' 'keys:q' --

# Help and the screens behind keys.
screen "h, any key" 24x80 'keys:h' 'keys:x' 'keys:q' --
screen "h, h: the windows' help" 24x80 'keys:h' 'keys:h' 'keys:q' 'keys:q' --
xscreen "the version, cut at the screen's edge, names SlateOS" "?, small" 10x40 \
  'keys:?' 'keys:q' 'keys:q' --
screen "f, q" 24x80 'keys:f' 'keys:q' 'keys:q' --
screen "f, small" 12x60 'keys:f' 'keys:q' 'keys:q' --
screen "f: move, toggle, sort" 24x80 'keys:f' 'keys:\e[B' 'keys:\e[B' 'keys:d' \
  'keys:\e[B' 'keys: ' 'keys:s' 'keys:\e[C' 'keys:\e[B' 'keys:\e[D' 'keys:\e[A' \
  'keys:\e[6~' 'keys:\e[5~' 'keys:\e[F' 'keys:\e[H' 'keys:q' 'keys:q' --
screen "f: other windows" 24x80 'keys:f' 'keys:w' 'keys:a' 'keys:a' 'keys:q' 'keys:q' --
screen "F, Esc" 24x80 'keys:F' 'keys:\e' 'keys:q' --
screen "Z, q" 24x80 'keys:Z' 'keys:q' 'keys:q' --
screen "Z: colours changed, Enter" 24x80 'keys:Z' 'keys:S' 'keys:2' 'keys:H' 'keys:4' \
  'keys:B' 'keys:\n' 'keys:q' --
screen "Z: other windows, Esc" 24x80 'keys:Z' 'keys:a' 'keys:T' 'keys:6' 'keys:\e' 'keys:q' --

# Prompts.
screen "d 1.5" 24x80 'keys:d' 'keys:1.5\n' 'keys:q' --
screen "s 2.5" 24x80 'keys:s' 'keys:2.5\n' 'keys:q' --
screen "d -1" 24x80 'keys:d' 'keys:-1\n' 'keys:q' --
screen "d abc" 24x80 'keys:d' 'keys:abc\n' 'keys:q' --
screen "d, Esc" 24x80 'keys:d' 'keys:\e' 'keys:q' --
screen "d, edited" 24x80 'keys:d' 'keys:12\x7f\x7f7\e[D\e[D9\e[H\e[3~\n' 'keys:q' --
screen "n 5" 24x80 'keys:n' 'keys:5\n' 'keys:q' --
screen "# 0" 24x80 'keys:#' 'keys:0\n' 'keys:q' --
screen "n -1" 24x80 'keys:n' 'keys:-1\n' 'keys:q' --
screen "u alice" 24x80 'keys:u' 'keys:alice\n' 'keys:q' --
screen "u !alice" 24x80 'keys:u' 'keys:!alice\n' 'keys:q' --
screen "U bob" 24x80 'keys:U' 'keys:bob\n' 'keys:q' --
screen "u nosuch" 24x80 'keys:u' 'keys:nosuch\n' 'keys:q' --
screen "u, blank" 24x80 'keys:u' 'keys:alice\n' 'keys:u' 'keys:\n' 'keys:q' --
screen "k 4000, default signal" 24x80 'keys:k' 'keys:4000\n' 'keys:\n' 'keys:q' --
screen "k 4000 KILL" 24x80 'keys:k' 'keys:4000\n' 'keys:KILL\n' 'keys:q' --
screen "k 4000 bad signal" 24x80 'keys:k' 'keys:4000\n' 'keys:NOSUCH\n' 'keys:q' --
screen "k, bad pid" 24x80 'keys:k' 'keys:abc\n' 'keys:q' --
screen "k, Esc" 24x80 'keys:k' 'keys:\e' 'keys:q' --
screen "r 4000 5" 24x80 'keys:r' 'keys:4000\n' 'keys:5\n' 'keys:q' --
screen "r 4000, bad nice" 24x80 'keys:r' 'keys:4000\n' 'keys:x\n' 'keys:q' --
screen "o COMMAND=bash" 24x80 'keys:o' 'keys:COMMAND=bash\n' 'keys:q' --
screen "O %CPU>1.0" 24x80 'keys:O' 'keys:%CPU>1.0\n' 'keys:q' --
screen "o !USER=root" 24x80 'keys:o' 'keys:!USER=root\n' 'keys:q' --
screen "o bad" 24x80 'keys:o' 'keys:xyz\n' 'keys:q' --
screen "o, o, ^O, =" 24x80 'keys:o' 'keys:COMMAND=a\n' 'keys:o' 'keys:PID<600\n' \
  'keys:\x0f' 'keys:=' 'keys:q' --
screen "L bash, &" 24x80 'keys:L' 'keys:bash\n' 'keys:&' 'keys:&' 'keys:q' --
screen "L nothing" 24x80 'keys:L' 'keys:nothing at all\n' 'keys:q' --
screen "g 2" 24x80 'keys:g' 'keys:2' 'keys:q' --
screen "g 9" 24x80 'keys:g' 'keys:9' 'keys:q' --
screen "G, not in A mode" 24x80 'keys:G' 'keys:q' --
screen "X 3" 24x80 'keys:X' 'keys:3\n' 'keys:q' --
screen "X -1" 24x80 'keys:X' 'keys:-1\n' 'keys:q' --
screen "W: written" 24x80 'keys:W' 'keys:q' --
screen "k in secure mode" 24x80 'keys:k' 'keys:q' -- -s
screen "d in secure mode" 24x80 'keys:d' 'keys:q' -- -s

# Inspect.
screen "Y 1" 24x80 'keys:Y' 'keys:1\n' 'keys:q' 'keys:q' --
screen "Y, Esc" 24x80 'keys:Y' 'keys:\e' 'keys:q' --
RC=insp
screen "Y 1, entries" 24x80 'keys:Y' 'keys:1\n' 'keys:\e[C' 'keys:\e[C' 'keys:\n' \
  'keys:\e[B' 'keys:/' 'keys:Pid\n' 'keys:n' 'keys:=' 'keys:q' 'keys:q' 'keys:q' --
screen "Y 4000" 24x80 'keys:Y' 'keys:4000\n' 'keys:q' --
RC=

# The bottom window.
for key in '\x07' '\x0b' '\x0e' '\x10' '\x15' '\x0c' '\x12'; do
  screen "bottom '$key'" 24x80 "keys:$key" "keys:$key" 'keys:q' --
done
screen "bottom ^K, tab, shift-tab" 24x80 'keys:\x0b' 'keys:\t' 'keys:\t' 'keys:\e[Z' \
  'keys:\x0b' 'keys:q' --
screen "bottom ^N, small" 12x50 'keys:\x0e' 'keys:\t' 'keys:q' --

# Scrolling.
WORLD=many
screen "scroll" 24x80 'keys:\e[B' 'keys:\e[B' 'keys:\e[6~' 'keys:\e[6~' 'keys:\e[5~' \
  'keys:\e[C' 'keys:\e[C' 'keys:\e[D' 'keys:\e[F' 'keys:\e[H' 'keys:q' --
screen "C, scroll" 24x80 'keys:C' 'keys:\e[6~' 'keys:\e[C' 'keys:q' --
screen "V, deep" 24x200 'keys:V' 'keys:q' --
screen "V, deep, narrow" 24x80 'keys:V' 'keys:\e[6~' 'keys:q' --
screen "V v on a parent" 24x120 'keys:V' 'keys:\e[B' 'keys:v' 'keys:q' --
screen "A" 40x120 'keys:A' 'keys:q' --
screen "n 5, A" 40x120 'keys:n' 'keys:5\n' 'keys:A' 'keys:q' --
screen "H, 4" 24x80 'keys:H' 'keys:4' 'keys:q' --
WORLD=main

# The alternate display and its windows.
screen "A, a, w, -, _, =, +" 40x120 'keys:A' 'keys:a' 'keys:w' 'keys:-' 'keys:_' \
  'keys:=' 'keys:+' 'keys:q' --
screen "A, small" 10x40 'keys:A' 'keys:q' --
screen "A, G" 40x120 'keys:A' 'keys:G' 'keys:new\n' 'keys:q' --

# The rcfiles on the screen.
for RC in default custom alt insp idj curwin; do
  screen "one frame, q" 24x80 'keys:q' --
done
RC=custom
screen "W after the custom one" 24x80 'keys:W' 'keys:q' --
RC=default:legacy
screen "W, legacy" 24x80 'keys:W' 'keys:q' --
RC=custom:xdg
screen "W, XDG" 24x80 'keys:W' 'keys:q' --
RC=
screen "A z x c then W" 24x80 'keys:A' 'keys:z' 'keys:x' 'keys:c' 'keys:W' 'keys:q' --

# Frames that move: the world a frame on, then a redraw.
screen "a frame later" 24x80 "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, 1" 24x80 'keys:1' "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, t, m" 24x80 'keys:t' 'keys:m' "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, H" 24x80 'keys:H' "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, S" 24x80 'keys:S' "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, I" 24x80 'keys:I' "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, i" 24x80 'keys:i' "$later_step" 'keys:\n' 'keys:q' --
screen "a frame later, 0" 24x80 'keys:0' "$later_step" 'keys:\n' 'keys:q' --

# Resizes and signals.
screen "grown" 24x80 'resize:30x100' 'keys:q' --
screen "shrunk" 30x100 'resize:20x70' 'keys:q' --
screen "made tiny" 24x80 'resize:3x10' 'keys:q' --
screen "resized twice" 24x80 'resize:40x120' 'resize:12x50' 'keys:q' --
screen "resized in help" 24x80 'keys:h' 'resize:30x100' 'keys:x' 'keys:q' --
screen "resized in a prompt" 24x80 'keys:d' 'resize:30x100' 'keys:1\n' 'keys:q' --
for sig in INT TERM HUP QUIT USR1 USR2 ALRM PIPE WINCH SYS URG; do
  screen "SIG$sig" 24x80 "signal:$sig" 'keys:q' --
done
screen "^Z, then SIGCONT" 24x80 'keys:\x1a' 'signal:CONT' 'keys:q' --
screen "SIGTSTP, then SIGCONT" 24x80 'signal:TSTP' 'signal:CONT' 'keys:q' --
screen "^C" 24x80 'keys:\x03' --
screen "^\\" 24x80 'keys:\x1c' --

# Batch mode on a terminal.
screen "-b -n 2 -d 0.2" 24x80 -- -b -n 2 -d 0.2
screen "-n 1" 24x80 -- -n 1
screen "-n 2 -d 0.2" 24x80 -- -n 2 -d 0.2

reset_knobs
echo "top-diff: $pass case(s) agree, $fail differ, $xfail differ on purpose, $xpass expected to differ and did not ($retried run again after both sides hung)"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
