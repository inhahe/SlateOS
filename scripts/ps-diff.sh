#!/usr/bin/env bash
# Differential test: our `ps` against procps-ng 4.0.4's, built as SlateOS's
# port is configured (no logind, no libnuma) -- see procps-ref.sh.
#
# ## A made-up machine
#
# `ps` prints the process table, and a real one moves: PIDs, CPU times and
# start times all differ between two runs a second apart. So neither side
# reads the real `/proc`. Each case runs both programs in their own
# `unshare -mUrpf` -- new user, mount and PID namespaces -- with a fixture
# directory bind-mounted over `/proc`. Inside, the program is PID 1, and the
# fixture's `1/` describes it (its terminal is the "own terminal" `ps` and
# `ps T` select by); `self` is a link to it. Everything `ps` reads from
# `/proc` -- `stat`, `status`, `cmdline`, `environ`, `wchan`, `cgroup`,
# `io`, `smaps_rollup`, `tty/drivers`, `uptime`, `stat`'s `btime`, `meminfo`,
# `sys/kernel/pid_max` -- is a file this script wrote, and the same file for
# both sides.
#
# Several worlds are built (`mkworld.py`): `main`, a small desktop's worth of
# processes whose boot was in 2023, so every start time prints as a date and
# nothing depends on today's clock; `recent`, booted two hours ago, for the
# time-of-day formats; `notask`, with no `task/` directories (as an old
# kernel); `broken`, whose files are malformed in the ways upstream's parsers
# are sensitive to; and a few with one system file missing.
#
# ## Names
#
# `/etc/passwd`, `/etc/group` and `/etc/nsswitch.conf` are bind-mounted to
# fixtures too, so user names are this script's (with glibc reading the
# files directly, as SlateOS's `pwdb` does), and `/dev/pts` is a fresh
# devpts instance in which this script opens three terminals, so `pts/0` to
# `pts/2` exist with known numbers. `/dev/tty1`, `/dev/console` and the
# serial ports are WSL's own and the same for both sides.
#
# ## What this replaced
#
# Until 2026-10-02 this harness pinned `ps` with `unshare -pf --mount-proc`:
# a real `/proc`, of a PID namespace holding nothing but `ps` itself. That
# pins the table, but at one row -- the only process it could ever show was
# the subject -- and it compared against Ubuntu's installed `ps`, not the
# release. It measured the `ps` written here, and its cases that still apply
# (a non-UTF-8 argument, `-h`, empty headers) are among the ones below.
#
# ## Cases that differ on purpose
#
# `--version`, `-V`, `V` and `--info`: this port names its own build.
set -u

DIFF_PROG='ps'
DIFF_NEED="python3 unshare timeout setsid"
# shellcheck source=procps-ref.sh
. "$(dirname "$0")/procps-ref.sh"
DIFF_REF=$PROCPS_REF_PS
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# ---------------------------------------------------------------------------
# hold.py: opens N pseudo-terminals in the current devpts and keeps them
# open, writing "ready" to FILE once they exist.
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
# ptyrun.py: COMMAND with standard output on a pty COLS columns wide (and
# ROWS rows); what it wrote there comes out on our standard output.
# ---------------------------------------------------------------------------
ptyrun=$DIFF_TMP/ptyrun.py
cat >"$ptyrun" <<'PY'
import fcntl, os, struct, subprocess, sys, termios

cols, rows = int(sys.argv[1]), int(sys.argv[2])
master, slave = os.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
p = subprocess.Popen(sys.argv[3:], stdin=subprocess.DEVNULL, stdout=slave)
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
sys.stdout.buffer.write(out.replace(b"\r\n", b"\n"))
sys.exit(p.wait())
PY

# ---------------------------------------------------------------------------
# mkworld.py: the fake /proc trees.   mkworld.py DEST NOW
# ---------------------------------------------------------------------------
mkworld=$DIFF_TMP/mkworld.py
cat >"$mkworld" <<'PY'
import os, sys

dest, now = sys.argv[1], int(sys.argv[2])
HZ = 100
PTS = lambda n: (136 << 8) | n
TTY = lambda n: (4 << 8) | n
CONSOLE = (5 << 8) | 1
TTYS0 = (4 << 8) | 64

DRIVERS = (b"/dev/tty             /dev/tty        5       0 system:/dev/tty\n"
           b"/dev/console         /dev/console    5       1 system:console\n"
           b"/dev/ptmx            /dev/ptmx       5       2 system\n"
           b"/dev/vc/0            /dev/vc/0       4       0 system:vtmaster\n"
           b"serial               /dev/ttyS       4 64-95 serial\n"
           b"pty_slave            /dev/pts      136 0-1048575 pty:slave\n"
           b"pty_master           /dev/ptm      128 0-1048575 pty:master\n"
           b"console              /dev/tty        4 1-63 console\n")


def write(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def b(x):
    return x if isinstance(x, bytes) else str(x).encode()


class P:
    def __init__(self, pid, comm, **kw):
        self.pid = pid
        self.comm = b(comm)
        d = dict(state="S", ppid=1, pgrp=None, sid=None, tty=0, tpgid=-1,
                 flags=4194560, minflt=100, cminflt=5, majflt=2, cmajflt=1,
                 utime=0, stime=0, cutime=0, cstime=0, prio=20, nice=0,
                 threads=1, start=0, vsize=0, rss=0, rlim=18446744073709551615,
                 code=(4194304, 4202496), stack=140737488347136, esp=0, eip=0,
                 exit_sig=17, cpu=0, rtprio=0, policy=0,
                 uid=0, gid=0, groups=None, vmsize=None, vmrss=None, vmlck=0,
                 vmdata=0, vmstk=132, sigpnd="0" * 16, shdpnd="0" * 16,
                 sigblk="0" * 16, sigign="0" * 16, sigcgt="0" * 16,
                 cmdline=None, environ=b"HOME=/root\0TERM=xterm\0",
                 wchan=b"do_select", cgroup=b"0::/user.slice\n",
                 io=None, smaps=None, oom=(0, 0), loginuid=b"4294967295",
                 autogroup=b"/autogroup-12 nice 0\n", attr=None, exe=None,
                 fd2=None, tasks=None, status_extra=b"", raw_stat=None,
                 raw_status=None, files=None, name=None)
        for k, v in kw.items():
            if k not in d:
                raise KeyError(k)
            d[k] = v
        self.__dict__.update(d)
        if self.pgrp is None:
            self.pgrp = pid
        if self.sid is None:
            self.sid = pid
        if self.cmdline is None:
            self.cmdline = self.comm + b"\0"
        if self.vmsize is None:
            self.vmsize = self.vsize // 1024
        if self.vmrss is None:
            self.vmrss = self.rss * 4

    def uids(self):
        u = self.uid if isinstance(self.uid, tuple) else (self.uid,) * 4
        return "\t".join(str(x) for x in u)

    def gids(self):
        g = self.gid if isinstance(self.gid, tuple) else (self.gid,) * 4
        return "\t".join(str(x) for x in g)

    def stat(self, tid=None):
        if self.raw_stat is not None:
            return self.raw_stat
        tid = self.pid if tid is None else tid
        f = [self.state, self.ppid, self.pgrp, self.sid, self.tty, self.tpgid,
             self.flags, self.minflt, self.cminflt, self.majflt, self.cmajflt,
             self.utime, self.stime, self.cutime, self.cstime, self.prio,
             self.nice, self.threads, 0, self.start, self.vsize, self.rss,
             self.rlim, self.code[0], self.code[1], self.stack, self.esp,
             self.eip, 0, 0, 0, 0, 0, 0, 0, self.exit_sig, self.cpu,
             self.rtprio, self.policy, 3, 4, 5, 0, 0, 0, 0, 0, 0, 0]
        return b"%d (%s) " % (tid, self.comm) + b" ".join(b(x) for x in f) + b"\n"

    def status(self, tid=None):
        if self.raw_status is not None:
            return self.raw_status
        tid = self.pid if tid is None else tid
        name = self.name if self.name is not None else self.comm
        groups = self.groups if self.groups is not None else ""
        s = (b"Name:\t%s\nUmask:\t0022\nState:\t%s (x)\nTgid:\t%d\nNgid:\t0\nPid:\t%d\n"
             b"PPid:\t%d\nTracerPid:\t0\nUid:\t%s\nGid:\t%s\nFDSize:\t64\nGroups:\t%s\n"
             b"VmPeak:\t%8d kB\nVmSize:\t%8d kB\nVmLck:\t%8d kB\nVmHWM:\t%8d kB\n"
             b"VmRSS:\t%8d kB\nVmData:\t%8d kB\nVmStk:\t%8d kB\nVmExe:\t     100 kB\n"
             b"VmLib:\t    2000 kB\nVmPTE:\t      60 kB\nVmSwap:\t       0 kB\n"
             b"Threads:\t%d\nSigQ:\t0/63\nSigPnd:\t%s\nShdPnd:\t%s\nSigBlk:\t%s\n"
             b"SigIgn:\t%s\nSigCgt:\t%s\nCapInh:\t0000000000000000\n") % (
            name, b(self.state), self.pid, tid, self.ppid, b(self.uids()),
            b(self.gids()), b(groups), self.vmsize, self.vmsize, self.vmlck,
            self.vmrss, self.vmrss, self.vmdata, self.vmstk, self.threads,
            b(self.sigpnd), b(self.shdpnd), b(self.sigblk), b(self.sigign),
            b(self.sigcgt))
        return s + self.status_extra

    def files_for(self, d, tid=None):
        write(f"{d}/stat", self.stat(tid))
        if self.raw_status is not False:
            write(f"{d}/status", self.status(tid))
        write(f"{d}/cmdline", self.cmdline)
        if self.environ is not None:
            write(f"{d}/environ", self.environ)
        if self.wchan is not None:
            write(f"{d}/wchan", self.wchan)
        write(f"{d}/cgroup", self.cgroup)
        io = self.io or (1000 + self.pid, 200 + self.pid, 30, 4, 4096, 8192, 0)
        write(f"{d}/io", b"rchar: %d\nwchar: %d\nsyscr: %d\nsyscw: %d\nread_bytes: %d\n"
              b"write_bytes: %d\ncancelled_write_bytes: %d\n" % io)
        sm = self.smaps or (self.vmrss, self.vmrss // 2, 10, 20, 0, 4, 8, 12, 16)
        write(f"{d}/smaps_rollup",
              b"00400000-7ffd0000 ---p 00000000 00:00 0  [rollup]\n"
              b"Rss:             %d kB\nPss:             %d kB\nPss_Anon:        %d kB\n"
              b"Pss_File:        %d kB\nPss_Shmem:       %d kB\nShared_Clean:    %d kB\n"
              b"Shared_Dirty:    %d kB\nPrivate_Clean:   %d kB\nPrivate_Dirty:   %d kB\n" % sm)
        write(f"{d}/oom_score", b"%d\n" % self.oom[0])
        write(f"{d}/oom_score_adj", b"%d\n" % self.oom[1])
        write(f"{d}/loginuid", self.loginuid)
        write(f"{d}/autogroup", self.autogroup)
        if self.attr is not None:
            write(f"{d}/attr/current", self.attr)
        if self.exe is not None:
            os.symlink(self.exe, f"{d}/exe")
        os.makedirs(f"{d}/fd", exist_ok=True)
        if self.fd2 is not None:
            os.symlink(self.fd2, f"{d}/fd/2")
        os.makedirs(f"{d}/ns", exist_ok=True)
        for ns in ("ipc", "mnt", "net", "pid", "user", "uts"):
            write(f"{d}/ns/{ns}", b"")
        for name, data in (self.files or {}).items():
            write(f"{d}/{name}", data)


def world(name, procs, btime=1700000000, uptime=100000.0, pid_max=b"4194304\n",
          tasks=True, stat=True, meminfo=True, extra=None):
    root = f"{dest}/{name}/proc"
    os.makedirs(root, exist_ok=True)
    os.makedirs(f"{dest}/{name}/cwd", exist_ok=True)
    write(f"{root}/uptime", b"%.2f 123456.78\n" % uptime)
    if stat:
        write(f"{root}/stat", b"cpu  100 0 50 9000 10 0 5 0 0 0\n"
              b"cpu0 50 0 25 4500 5 0 2 0 0 0\ncpu1 50 0 25 4500 5 0 3 0 0 0\n"
              b"intr 12345\nctxt 67890\nbtime %d\nprocesses 4000\nprocs_running 2\n"
              b"procs_blocked 0\n" % btime)
    if meminfo:
        write(f"{root}/meminfo", b"MemTotal:       16000000 kB\nMemFree:         8000000 kB\n"
              b"MemAvailable:   12000000 kB\n")
    if pid_max is not None:
        write(f"{root}/sys/kernel/pid_max", pid_max)
    write(f"{root}/tty/drivers", DRIVERS)
    for p in procs:
        d = f"{root}/{p.pid}"
        p.files_for(d)
        if tasks:
            for tid in (p.tasks or [p.pid]):
                p.files_for(f"{d}/task/{tid}", tid)
    os.symlink("1", f"{root}/self")
    for path, data in (extra or {}).items():
        write(f"{root}/{path}", data)


S = 1  # start ticks, relative

main = [
    P(1, "ps", state="R", ppid=150, pgrp=1, sid=151, tty=PTS(1), tpgid=1,
      start=99990 * HZ, utime=1, stime=2, vsize=12000000, rss=900,
      cmdline=b"ps\0-ef\0", wchan=b"0", sigblk="0000000000000000"),
    P(2, "kthreadd", ppid=0, pgrp=0, sid=0, flags=2129984, start=0,
      cmdline=b"", wchan=b"kthreadd", environ=b""),
    P(3, "rcu_gp", state="I", ppid=2, pgrp=0, sid=0, flags=69238880, prio=0,
      nice=-20, cmdline=b"", start=5, wchan=b"rescuer_thread"),
    P(5, "init", ppid=0, start=10, utime=250, stime=100, cutime=9000,
      cstime=4000, vsize=170000000, rss=3000, cmdline=b"/sbin/init\0splash\0",
      sigign="0000000000001000", sigcgt="00000001000004ec",
      sigblk="7be3c0fe28014a03", exe=b"/usr/lib/systemd/systemd"),
    P(100, "sshd", ppid=5, start=2000, utime=12, stime=8, vsize=15000000,
      rss=1500, cmdline=b"sshd: /usr/sbin/sshd -D [listener] 0 of 10-100 startups\0",
      sigign="0000000000001000", sigcgt="0000000180014a03"),
    P(150, "sudo", ppid=200, pgrp=150, sid=200, tty=PTS(1), tpgid=1,
      start=99000 * HZ, cmdline=b"sudo\0-i\0", vsize=11000000, rss=800),
    P(151, "bash", ppid=150, pgrp=151, sid=151, tty=PTS(1), tpgid=1,
      start=99001 * HZ, utime=3, stime=1, cmdline=b"-bash\0", vsize=9000000,
      rss=1200, sigign="0000000000384004", sigcgt="000000004b813efb",
      fd2=b"/dev/pts/1"),
    P(200, "bash", ppid=100, pgrp=200, sid=200, tty=PTS(1), tpgid=300,
      uid=1000, gid=1000, groups="4 24 27 1000 ", start=90000 * HZ, utime=40,
      stime=20, vsize=9100000, rss=1300, cmdline=b"-bash\0",
      environ=b"USER=alice\0HOME=/home/alice\0LANG=C.UTF-8\0", fd2=b"/dev/pts/1"),
    P(300, "vim", ppid=200, pgrp=300, sid=200, tty=PTS(1), tpgid=300,
      uid=1000, gid=1000, start=95000 * HZ, utime=500, stime=60, vsize=30000000,
      rss=5000, cmdline=b"vim\0notes.txt\0", wchan=b"do_select"),
    P(301, "sleep", ppid=200, pgrp=301, sid=200, tty=PTS(1), tpgid=300,
      uid=1000, gid=1000, start=96000 * HZ, cmdline=b"sleep\0" + b"1000\0",
      wchan=b"hrtimer_nanosleep", state="T"),
    P(400, "bash", ppid=100, pgrp=400, sid=400, tty=PTS(2), tpgid=400,
      uid=1001, gid=1001, start=80000 * HZ, utime=7, stime=3, cmdline=b"-bash\0",
      vsize=9000000, rss=1100),
    P(401, "make", state="Z", ppid=400, pgrp=400, sid=400, tty=PTS(2),
      tpgid=400, uid=1001, gid=1001, start=81000 * HZ, cmdline=b"", utime=900,
      stime=300, vsize=0, rss=0, wchan=b"0"),
    P(500, "java", ppid=5, pgrp=500, sid=500, uid=1000, gid=1000, threads=3,
      start=3000, utime=60000, stime=9000, vsize=4000000000, rss=200000,
      cmdline=b"/usr/bin/java\0-Xmx2g\0-jar\0app.jar\0", tasks=[500, 501, 502],
      nice=5, prio=25),
    P(600, "my (odd) name", ppid=5, start=4000, uid=1002,
      cmdline=b"odd\tname\0caf\xc3\xa9\0\xe4\xb8\x80\xe4\xba\x8c\0bad\xff\0",
      environ=b"X=\x01\x02\0"),
    P(700, "agetty", ppid=5, tty=TTY(1), tpgid=700, start=5000,
      cmdline=b"/sbin/agetty\0-o\0-p -- \\u\0--noclear\0tty1\0linux\0", nice=-5,
      prio=15),
    P(701, "agetty", ppid=5, tty=CONSOLE, tpgid=701, start=5001,
      cmdline=b"/sbin/agetty\0--keep-baud\0console\0", vmlck=64),
    P(702, "agetty", ppid=5, tty=TTYS0, tpgid=702, start=5002,
      cmdline=b"/sbin/agetty\0ttyS0\0", policy=1, rtprio=50, prio=-51),
    P(800, "spinner", state="R", ppid=5, start=99000 * HZ, utime=50000,
      stime=40000, uid=1004, cpu=1, cmdline=b"spinner\0--fast\0", policy=3,
      vsize=2000000, rss=600, io=(5000, 6000, 7000, 8000, 9000, 10000, 11000)),
    P(801, "worker", state="D", ppid=5, start=20000, uid=1003, gid=1003,
      cmdline=b"worker\0", policy=5, wchan=b"_.__io_schedule"),
    P(32768, "tiny", ppid=5, start=6000, uid=65534, gid=65534,
      cmdline=b"tiny\0", vsize=1000000, rss=10, oom=(666, -1000)),
]
world("main", main)

# Booted two hours ago: start times print as times of day.
recent_boot = now - 7200
recent = [
    P(1, "ps", state="R", ppid=0, tty=PTS(1), tpgid=1, start=7190 * HZ,
      cmdline=b"ps\0"),
    P(10, "daemon", ppid=1, start=60 * HZ, utime=100),
    P(11, "shell", ppid=1, tty=PTS(1), tpgid=11, start=3600 * HZ),
    P(12, "fresh", ppid=1, tty=PTS(1), tpgid=11, start=7000 * HZ),
]
world("recent", recent, btime=recent_boot, uptime=7200.0)

# No task directories: thread options fall back to processes.
notask = [
    P(1, "ps", state="R", ppid=0, tty=PTS(1), tpgid=1, start=99990 * HZ),
    P(500, "java", ppid=1, threads=3, start=3000, utime=600, cmdline=b"java\0"),
]
world("notask", notask, tasks=False)

# Malformed files.
broken = [
    P(1, "ps", state="R", ppid=0, tty=PTS(1), tpgid=1, start=99990 * HZ),
    # stat with no parentheses: only the defaults; the name from status.
    P(20, "noparen", raw_stat=b"20 noparen S 1 20 20 0 -1\n"),
    # stat that stops early: later fields keep their defaults.
    P(21, "short", raw_stat=b"21 (short) R 1 21 21 0 -1 4194304 1 2\n"),
    # status without Pid: the PID column is 0.
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
    # Command line of NULs only; an empty one; a zombie with no command line.
    P(27, "nuls", cmdline=b"\0\0\0"),
    P(28, "empty", cmdline=b""),
    P(29, "zomb", state="Z", cmdline=b""),
    # Status Name with escapes, and stat name ending at the last parenthesis.
    P(30, "a) b (c", name=b"esc\\\\aped\\nname"),
    # Supplementary groups, some unknown.
    P(31, "groupy", groups="0 4 999 1000", uid=(1000, 1001, 1002, 1003),
      gid=(1000, 50, 4, 999)),
    # wchan spellings.
    P(32, "wchanzero", wchan=b"0"),
    P(33, "wchanempty", wchan=b""),
    P(34, "nowchan", wchan=None),
    # A name that is fifteen bytes, for -C's special case.
    P(35, "systemd-journal", cmdline=b"/lib/systemd/systemd-journald\0"),
    # Signals: a hex mask with letters, for --signames' decimal reading.
    P(36, "sigs", sigcgt="000000004b813efb", sigblk="0000000000010000",
      sigign="0000000000000000", sigpnd="0000000000000100"),
    # A SELinux label.
    P(37, "labelled", attr=b"unconfined_u:unconfined_r:unconfined_t:s0\n\x01junk"),
    # lxc and cgroups.
    P(38, "container", cgroup=b"12:pids:/lxc/web1/x\n0::/lxc.payload.db2/sub\n"
      b"1:name=systemd:/user.slice/u.scope\n3:cpu:/\n"),
]
world("broken", broken)

# One system file missing each.
mini = [P(1, "ps", state="R", ppid=0, tty=PTS(1), tpgid=1, start=99990 * HZ,
          cmdline=b"ps\0")]
world("nostat", mini, stat=False)
world("nomem", mini, meminfo=False)
world("nopidmax", mini, pid_max=None)
world("shortpidmax", mini, pid_max=b"32768\n")
world("emptypidmax", mini, pid_max=b"")
PY

python3 "$mkworld" "$work/worlds" "$(date +%s)" || { echo "ps-diff: mkworld failed" >&2; exit 1; }

# The password and group databases, and an NSS that reads only them.
mkdir -p "$work/etc"
cat >"$work/etc/passwd" <<'EOF'
root:x:0:0:root:/root:/bin/bash
alice:x:1000:1000:Alice:/home/alice:/bin/bash
bob:x:1001:1001::/home/bob:/bin/sh
averyveryveryverylongusername123:x:1002:1002::/:/bin/sh
café:x:1003:1003::/:/bin/sh
twentycharacterslong:x:1004:1004::/:/bin/sh
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
EOF
cat >"$work/etc/group" <<'EOF'
root:x:0:
adm:x:4:alice
cdrom:x:24:alice
sudo:x:27:alice
staff:x:50:
alice:x:1000:
bob:x:1001:
longgroup:x:1002:
café:x:1003:
nogroup:x:65534:
EOF
printf 'passwd: files\ngroup: files\nshadow: files\n' >"$work/etc/nsswitch.conf"

# --- knobs ------------------------------------------------------------------
WORLD=main       # which fake /proc
LOCALE=C.UTF-8
ZONE=UTC
PTY=             # "COLS ROWS": run with standard output on a pty that size
ENVS=()          # extra NAME=VALUE for ps's environment
CWDFILES=()      # files to create in the case's working directory
reset_knobs() {
  WORLD=main; LOCALE=C.UTF-8; ZONE=UTC; PTY=; ENVS=(); CWDFILES=()
}

# $1 = side, $2 = output prefix; the rest is ps's argv.
run_side() {
  local side=$1 p=$2; shift 2
  local wdir=$work/worlds/$WORLD
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=$LOCALE" "TZ=$ZONE" "${ENVS[@]}")
  local -a ns=(timeout -k 5 60 unshare -mUrpf --propagation private setsid sh -c '
      w=$1 e=$2 hold=$3 ready=$4; shift 4
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
      exec "$@"' _ "$wdir" "$work/etc" "$holdpy" "$p.$side.ready" "${envs[@]}" ps "$@")
  if [ -n "$PTY" ]; then
    # shellcheck disable=SC2086 # COLS ROWS are two words on purpose
    diff_run python3 "$ptyrun" $PTY "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err"
  else
    diff_run "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err" </dev/null
  fi
  echo $? >"$p.$side.rc"
  return 0
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  local f
  for f in "${CWDFILES[@]}"; do : >"$work/worlds/$WORLD/cwd/$f"; done
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  for f in "${CWDFILES[@]}"; do rm -f "$work/worlds/$WORLD/cwd/$f"; done
  LABEL="ps $*"
  [ "$WORLD" != main ] && LABEL="$LABEL [world=$WORLD]"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL [LC_ALL=$LOCALE]"
  [ "$ZONE" != UTC ] && LABEL="$LABEL [TZ=$ZONE]"
  [ -n "$PTY" ] && LABEL="$LABEL [pty $PTY]"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  reset_knobs

  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  # 125 is a mount that failed and 124 a timeout: the case never reached
  # ps, which is not agreement however alike the two sides look.
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
    printf 'BROKEN %s -- never reached ps on one or both sides\n%s\n' "$LABEL" "$REPORT"
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

# `xfail_case REASON -- ARGS...`: a difference on purpose.
xfail_case() {
  local why=$1; shift
  [ "${1:-}" = -- ] && shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    report
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# A guard against vacuous agreement: the first case must print the fake
# world's own process, with its own terminal and command, on both sides.
# ---------------------------------------------------------------------------
run_case -p 1 -o pid,tty,args
if ! grep -q '^ *1 pts/1 *ps -ef$' "$work/c1.gnu.out"; then
  echo "ps-diff: the reference did not see the fixture /proc:" >&2
  cat "$work/c1.gnu.out" "$work/c1.gnu.err" >&2
  exit 1
fi

# --- selection --------------------------------------------------------------
run_case
run_case -e
run_case -A
run_case -a
run_case -d
run_case -N
run_case -e -N
run_case a
run_case x
run_case ax
run_case aux
run_case g
run_case r
run_case -er
run_case T
run_case t
run_case -t pts/2
run_case -t tty1
run_case -t /dev/tty1
run_case -t console
run_case -t co
run_case t pts/1,pts/2
run_case -t '?'
run_case -t -
run_case -t ttyS0
run_case -t nosuch
run_case -t /dev/null
run_case -p 200,300
run_case -p '200 300'
run_case -p 0x12c
run_case -p 0454
run_case 200 300
run_case 200 -300
run_case -200
run_case +200
run_case -s 200
run_case --sid 200,400
run_case --ppid 200
run_case --ppid 5 --forest
run_case -u alice
run_case -u 1000,1001
run_case -U root
run_case -u nosuchuser
run_case -N -u nosuchuser
run_case -g 200
run_case -g alice
run_case -G alice
run_case --group 1001
run_case --Group bob
run_case --User alice
run_case --user root -o pid,user
run_case -C bash
run_case -C vim,sleep
run_case -C systemd-journal
run_case -q 300
run_case -q 300,200,1
run_case q 300
run_case --quick-pid=300
run_case -q 1 -q 2
run_case -q 1 -e
run_case -q 1 --forest
run_case -q 1 --sort pid
run_case -q 1 -N
run_case -p 0
run_case -p 2147483648
run_case -p abc
run_case -p
run_case -p ''
run_case -p 1,
run_case -p ,1
run_case -p 1,,2
run_case U alice
run_case p 1
run_case 99999
run_case -p 99999 -o pid=
run_case -u root,nobody
run_case -u 0
run_case -u 99999
run_case -p notanumber
run_case -e -t '?'
run_case -fe
run_case -Ae
run_case -Af
run_case -h
run_case -eh
# Arguments that are not UTF-8: an option letter, a command name, a header.
run_case "-$(printf '\351')"
run_case -C "$(printf 'caf\351')"
run_case -o "pid=$(printf '\351'),comm"
run_case -o comm=
run_case -o pid,comm=
run_case -o pid=MYPID,comm

# --- formats ----------------------------------------------------------------
run_case -ef
run_case -eF
run_case -el
run_case -ely
run_case -elc
run_case -ej
run_case -efj
run_case -elf
run_case -efL
run_case -ec
run_case -eP
run_case -eM
run_case -fc
run_case -y
run_case u
run_case aux
run_case v
run_case j
run_case l
run_case s
run_case X
run_case -o pid,comm
run_case -o pid=X,comm=Y
run_case -o pid:10,comm:3
run_case -o comm,pid
run_case -o 'pid,comm=MY COMMAND'
run_case -o pid,args
run_case -eo pid,cmd
run_case -eo pid,command
run_case -eo pid,ucmd,ucomm,fname
run_case -eo pid,%cpu,%mem,pcpu,pmem
run_case -eo pid,c,cp,cuc,cuu
run_case -eo pid,lstart
run_case -eo pid,etime,etimes,time,times,cputime,cputimes,atime,bsdtime
run_case -eo pid,stime,start,bsdstart,start_time
run_case -eo pid,user,ruser,euser,uname,uid_hack
run_case -eo pid,group,rgroup,egroup,fgroup,sgroup
run_case -eo pid,uid,ruid,euid,suid,fuid,svuid,fsuid
run_case -eo pid,gid,rgid,egid,sgid,fgid,svgid,fsgid
run_case -eo pid,suser,fuser,svuser,fsuser,svgroup,fsgroup
run_case -eo pid,tty,tt,tname,tty4,tty8,longtname
run_case -eo pid,stat,s,state
run_case -eo pid,f,flag,flags
run_case -eo pid,ni,nice,pri,opri,priority,intpri,pri_api,pri_bar,pri_baz,pri_foo
run_case -eo pid,rtprio,cls,class,policy,sched
run_case -eo pid,vsz,vsize,rss,rssize,rsz,sz,size,sgi_rss
run_case -eo pid,drs,trs,dsiz,tsiz,trss,m_size,m_drs,m_trs
run_case -eo pid,maj_flt,min_flt,majflt,minflt,pagein
run_case -eo pid,lim
run_case -eo pid,wchan,nwchan,wname
run_case -eo pid,sig,sigmask,sigignore,sigcatch
run_case -eo pid,pending,blocked,caught,ignored,tsig
run_case -eo pid,sig_pend,sig_block,sig_catch,sig_ignore
run_case -eo pid,ppid,pgid,pgrp,sid,sess,session,tpgid,tgid
run_case -eo pid,lwp,spid,tid,nlwp,thcount
run_case -eo pid,rbytes,rchars,rops,wbytes,wcbytes,wchars,wops
run_case -eo pid,pss,uss
run_case -eo pid,oom,oomadj
run_case -eo pid,cgroup
run_case -eo pid,cgname
run_case -eo pid,exe
run_case -eo pid,lxc
run_case -eo pid,luid
run_case -eo pid,supgid,supgrp
run_case -eo pid,label
run_case -eo pid,context
run_case -eo pid,zone
run_case -eo pid,ag_id,ag_nice
run_case -eo pid,numa,psr,cpuid,lastcpu,sgi_p
run_case -eo pid,unit,seat,machine,ouid,slice,uunit,lsession
run_case -eo pid,eip,esp,stackp,start_stack
run_case -eo pid,addr,addr_1,acflag,alarm,cpu,~
run_case -eo pid,environ,cwd,login,logname,luser
run_case -eo pid,_left,_right,_unlimited
run_case -o _left2,_right2,_unlimited2
run_case -o pid,ipcns,mntns,netns,pidns,userns,utsns,cgroupns,timens
run_case -o
run_case -o ''
run_case -o nosuch
run_case -o pid,nosuch
run_case -o pid:0
run_case -o pid:x
run_case -o pid:
run_case -o pid:4294967297,comm
run_case -o DefSysV:5
run_case -o DefSysV
run_case -o DefBSD=X
run_case -o pid, -o comm
run_case -o pid -o comm
run_case -o ',pid'
run_case -o 'pid,,comm'
run_case -o 'pid comm'
run_case -o $'pid\tcomm'
run_case -o $'pid\ncomm'
run_case -O comm
run_case -O comm -o pid
run_case -o pid -O comm
run_case O comm
run_case O Pp
run_case O -p
run_case O +P-p
run_case O zz
run_case O PP
run_case O Kp
run_case O J
run_case -o '%p %a'
run_case -o '%p %x %z %y %u %U %G %g %c %C %r %n %t %P'
run_case -o '%p%a'
run_case -o 'pid %a'
run_case -o '%%'
run_case -o '%q'
run_case -o '% '
run_case -o 'x%p'
run_case --format pid,comm
run_case --format=pid,comm
run_case --format
run_case --context
run_case -F -o pid
run_case -f -l -j
run_case u -f
run_case -D '%s' -o pid,lstart
run_case -D %H -o pid,lstart
run_case --date-format '%F %T' -o pid,lstart
run_case --date-format='[%a]' -o pid,lstart
run_case -o pid,lstart --date-format ''

# --- BSD modifiers ------------------------------------------------------------
run_case axe
run_case axc
run_case axn
run_case axnl
run_case axS
run_case axcS -o pid,comm,time,bsdtime,c,%cpu
run_case -e -o pid,args e
run_case axf
run_case axjf
run_case -ef --forest
run_case -eH
run_case -eH -o pid,args
run_case axf -o pid,args,comm
run_case --forest -o pid,fname
run_case -ejH
run_case f -o pid,args --sort=pid
run_case axf --sort=-pid

# --- threads ----------------------------------------------------------------
run_case -eL
run_case -eLf
run_case -eT
run_case -em
run_case m
run_case axm
run_case axH
run_case -L -p 500
run_case -T -p 500 -o pid,spid,tid,args
run_case -m -p 500
run_case m -p 500
run_case H -p 500
run_case -L -q 500
run_case -L -T
run_case -m m
run_case H -m
run_case -L --forest
run_case -L -o pid,lwp
run_case -m -o pid,lwp,nlwp,stat
run_case -mL
run_case -eL --sort=-lwp
run_case -m --sort=pid
WORLD=notask; run_case -eL
WORLD=notask; run_case -em
WORLD=notask; run_case H

# --- sorting ------------------------------------------------------------------
run_case -e --sort pid
run_case -e --sort -pid
run_case -e --sort=user,-pid
run_case -e --sort=tty
run_case -e --sort=-tty,pid
run_case -e --sort=comm
run_case -e --sort=-args
run_case -e --sort=start_time
run_case -e --sort=%cpu
run_case -e --sort=-%mem
run_case -e --sort=-rss,pid
run_case -e --sort=utime
run_case -e --sort=fgid
run_case -e --sort=stat
run_case -e --sort=-nice
run_case -e --sort=wchan
run_case -e --sort=nosuch
run_case -e --sort=
run_case -e --sort pid,
run_case -e --sort ',pid'
run_case -e --sort pid --sort ppid
run_case ax k -pid
run_case ax k
run_case axO Pp
run_case axO -p
run_case -e --sort=+ppid,-pid
run_case -e --sort=lstart

# --- widths and headers ---------------------------------------------------------
run_case -ef --cols 60
run_case -ef --columns=40
run_case -ef --width 100
run_case -ef --cols 0
run_case -ef --cols x
run_case -ef --cols
run_case -ef --rows 5 --headers
run_case -e --headers
run_case -e --lines 3 --headers
run_case -e --no-headers
run_case -e --no-heading
run_case -e --noheaders
run_case -e --headers --no-headers
run_case -e --no-header=x
run_case aux h
run_case -e -o pid=
run_case -e -o pid=,comm=
run_case -e -o pid=,comm
run_case ef -w
run_case -ef -w
run_case -ef -ww
run_case axuww
ENVS=(COLUMNS=60); run_case -ef
ENVS=(COLUMNS=8); run_case -ef
ENVS=(COLUMNS=0x50); run_case -ef
ENVS=(COLUMNS=junk); run_case -ef
ENVS=(LINES=1); run_case -e
ENVS=(LINES=4); run_case -e --headers
PTY="80 24"; run_case -ef
PTY="40 24"; run_case -ef
PTY="200 50"; run_case aux
PTY="80 3"; run_case -e --headers
PTY="80 24"; run_case -eo pid,args
PTY="30 24"; run_case -eo user,pid,args
PTY="80 24"; run_case -e -o pid,sigcatch,sigmask
PTY="200 24"; run_case -e -o pid,sigcatch,sigmask
PTY="80 24"; run_case -e -o pid,wchan,args
PTY="21 24"; run_case -e -o pid,user:5,args
ENVS=(COLUMNS=12); run_case -e -o pid,user,comm
ENVS=(COLUMNS=13); run_case -e -o pid,args

# --- signals by name ------------------------------------------------------------
run_case --signames -eo pid,sigcatch,sigmask,sigignore
run_case --signames -eo pid,pending,comm
PTY="60 24"; run_case --signames -eo pid,sigcatch
run_case --signames s
WORLD=broken; run_case --signames -eo pid,sigcatch,sigmask,sig

# --- personalities --------------------------------------------------------------
for per in bsd old debian gnu linux default unknown aix tru64 compaq digital \
           sunos4 irix sgi os390 s390 390 hp hpux svr4 sysv sco posix solaris2 \
           unix unix95 unix98 BSD nosuch averyveryverylongpersonality; do
  ENVS=("PS_PERSONALITY=$per"); run_case
  ENVS=("PS_PERSONALITY=$per"); run_case -ef
  ENVS=("PS_PERSONALITY=$per"); run_case -el
  ENVS=("PS_PERSONALITY=$per"); run_case aux
done
ENVS=(CMD_ENV=bsd); run_case -ef
ENVS=(I_WANT_A_BROKEN_PS=1); run_case ef
ENVS=(PS_PERSONALITY=sgi _XPG=1); run_case -el
ENVS=(PS_PERSONALITY=hp); run_case -ex
ENVS=(PS_PERSONALITY=svr4); run_case -elx
ENVS=(PS_PERSONALITY=gnu); run_case -ej
ENVS=(PS_PERSONALITY=gnu); run_case -ejl
ENVS=(PS_PERSONALITY=gnu); run_case -ejf
ENVS=(PS_PERSONALITY=digital); run_case -efl
ENVS=(PS_PERSONALITY=s390); run_case -j
run_case -ex
ENVS=("PS_FORMAT=pid,comm"); run_case
ENVS=("PS_FORMAT=pid,comm"); run_case -f
ENVS=(PS_FORMAT=nosuch); run_case
ENVS=("PS_FORMAT=pid,comm"); run_case -L
ENVS=(PS_FORMAT='%p %c'); run_case
ENVS=(LIBPROC_HIDE_KERNEL=1); run_case -ef
ENVS=(LIBPROC_HIDE_KERNEL=); run_case -e -o pid,ppid,comm

# --- options that end ps ----------------------------------------------------------
run_case --help
run_case --help s
run_case --help=list
run_case --help output
run_case --help t
run_case --help misc
run_case --help a
run_case --help nosuch
run_case --help -e
run_case L
run_case L x
xfail_case "our version string names this port" -- -V
xfail_case "our version string names this port" -- V
run_case -eV
run_case aV
xfail_case "our version string names this port" -- --version
run_case --version x
xfail_case "our --info names our build" -- --info
run_case --info x

# --- the second, BSD reading ------------------------------------------------------
run_case -aux
run_case -ax
run_case -auxf
run_case -aux --sort=-pid
run_case -Q
run_case Q
run_case -eQ
run_case --nosuch
run_case --
run_case -
run_case -e --
run_case -e -- x
run_case '-e' 'x' '-'
run_case _
run_case -e-
run_case a-
run_case -e e
run_case --cumulative -o pid,time,bsdtime
run_case --cumulative=x
run_case --deselect -p 1
run_case --deselect=1
run_case --forest=x
run_case --signames=x -o pid,sig
run_case --pid
run_case --pid=
run_case --tty
run_case --tty=pts/1
run_case --sort
run_case --abcdefghijklmnopq
run_case -W
run_case W
run_case k
run_case O
run_case o
run_case -o
run_case -O
run_case -C
run_case -D
run_case -G
run_case -U
run_case -g
run_case -s
run_case -t
run_case -u
run_case -q
run_case q
run_case p
run_case U
run_case ah
run_case ahh
run_case -e h --headers
CWDFILES=(x); run_case -t x
run_case -t y

# --- broken files -------------------------------------------------------------------
WORLD=broken; run_case -e
WORLD=broken; run_case -ef
WORLD=broken; run_case -el
WORLD=broken; run_case -eF
WORLD=broken; run_case aux
WORLD=broken; run_case -eo pid,ppid,stat,ni,pri,nlwp,flags,args
WORLD=broken; run_case -eo pid,tid,tgid,user,ruser,euser,suser,fuser
WORLD=broken; run_case -eo pid,supgid,supgrp,group,rgroup,sgroup,fgroup
WORLD=broken; run_case -eo pid,sig,sigmask,sigcatch,sigignore,tsig
WORLD=broken; run_case -eo pid,wchan,comm
WORLD=broken; run_case -eo pid,comm,cmd
WORLD=broken; run_case -eo pid,label,context
WORLD=broken; run_case -eo pid,lxc,cgname,cgroup
WORLD=broken; run_case -eo pid,time,etime,vsz,rss,maj_flt
WORLD=broken; run_case -C systemd-journald
WORLD=broken; run_case -C systemd-journal -o pid,comm
WORLD=broken; run_case -C 'a) b (c'
WORLD=broken; run_case -e --sort=comm
WORLD=broken; run_case -e --sort=-ppid
WORLD=broken; run_case -e --forest
WORLD=broken; run_case -e -m
WORLD=broken; run_case -eo pid,stat --sort=stat
WORLD=broken; LOCALE=C; run_case -eo pid,comm,args
WORLD=main; LOCALE=C; run_case -eo pid,comm,args
WORLD=main; LOCALE=C; run_case -eo pid,user,args e
WORLD=main; LOCALE=C.UTF-8; run_case -eo pid,user:4,args
run_case -eo pid,user:3,args
run_case -eo pid,user:1,args
run_case -eo pid,user:16
run_case -eo pid,user=WHO
run_case -eo pid,group:4

# --- time ---------------------------------------------------------------------------
WORLD=recent; run_case -ef
WORLD=recent; run_case -eo pid,stime,start,bsdstart,lstart,etime
WORLD=recent; run_case aux
WORLD=recent; ZONE=America/New_York; run_case -eo pid,stime,start,lstart
WORLD=recent; ZONE=Asia/Tokyo; run_case -ef
ZONE=Europe/Berlin; run_case -eo pid,stime,start,bsdstart,lstart
ZONE=Australia/Lord_Howe; run_case -eo pid,lstart
run_case -eo pid,lstart -D '%Y-%m-%d %H:%M:%S %Z %z'
run_case -eo pid,lstart -D "$(printf '%0300d' 0)"

# --- missing system files -------------------------------------------------------------
WORLD=nostat; run_case -ef
WORLD=nostat; run_case -eo pid,comm
WORLD=nostat; run_case -eo pid,lstart
WORLD=nostat; run_case -eo pid,bsdstart
WORLD=nomem; run_case aux
WORLD=nomem; run_case -eo pid,rss
WORLD=nopidmax; run_case -ef
WORLD=shortpidmax; run_case -ef
WORLD=emptypidmax; run_case -ef

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
