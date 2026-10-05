"""Fake /proc trees for the procps differential harnesses.

`ps-diff.sh` and `pgrep-diff.sh` run both sides of every case over a fixture
directory bind-mounted on `/proc`, so that a process table -- which moves
between any two real runs -- is the same table for both programs. This module
is the model of a process those fixtures are written from, shared so that the
two harnesses describe a process the same way and a fix to one (a field
upstream's parser reads, a file it opens) reaches the other.

A world is a directory `DEST/NAME/proc` (the fixture) beside `DEST/NAME/cwd`
(the case's working directory). `P` is one process; `world()` writes a list
of them, the system files around them, and `self` as a link to `1/` -- in the
harness's PID namespace the program under test is PID 1, so `1/` describes it.

Not a script: a harness's own world definitions import it, with the
`scripts` directory on `sys.path`.
"""

import os

HZ = 100


def PTS(n):
    """The device number of /dev/pts/N."""
    return (136 << 8) | n


def TTY(n):
    """The device number of /dev/ttyN."""
    return (4 << 8) | n


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

# The namespaces `ps` reads by default, and all eight `procps_ns_read_pid`
# knows (`pgrep --ns` compares the first six of these by default).
NS_PS = ("ipc", "mnt", "net", "pid", "user", "uts")
NS_ALL = ("cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts")


def write(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def b(x):
    return x if isinstance(x, bytes) else str(x).encode()


class P:
    """One process. Every keyword has a default a real process would plausibly
    have; a name that is not one of them is a mistake, and raises."""

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
                 raw_status=None, files=None, name=None,
                 # Which `ns/` files exist, and which to share: a PID whose
                 # files to hard-link (so `stat` gives both one inode), or a
                 # dict of namespace name to PID for some of them.
                 ns=NS_PS, ns_from=None)
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

    def ns_source(self, ns):
        """The PID whose `ns/NS` this process shares, or None."""
        if isinstance(self.ns_from, dict):
            return self.ns_from.get(ns)
        return self.ns_from

    def files_for(self, d, tid=None, root=None):
        write(f"{d}/stat", self.stat(tid))
        if self.raw_status is not False:
            write(f"{d}/status", self.status(tid))
        write(f"{d}/cmdline", self.cmdline)
        if self.environ is not None:
            write(f"{d}/environ", self.environ)
        if self.wchan is not None:
            write(f"{d}/wchan", self.wchan)
        if self.cgroup is not None:
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
        for ns in self.ns:
            src = self.ns_source(ns)
            if src is not None and root is not None:
                os.link(f"{root}/{src}/ns/{ns}", f"{d}/ns/{ns}")
            else:
                write(f"{d}/ns/{ns}", b"")
        for name, data in (self.files or {}).items():
            write(f"{d}/{name}", data)


def world(dest, name, procs, btime=1700000000, uptime=100000.0, pid_max=b"4194304\n",
          tasks=True, stat=True, meminfo=True, extra=None):
    """Write world NAME under DEST: PROCS, in order (a process whose `ns/`
    files are shared from another must come after it), and the files around
    them. `uptime=None` leaves `/proc/uptime` out."""
    root = f"{dest}/{name}/proc"
    os.makedirs(root, exist_ok=True)
    os.makedirs(f"{dest}/{name}/cwd", exist_ok=True)
    if uptime is not None:
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
        p.files_for(d, root=root)
        if tasks:
            for tid in (p.tasks or [p.pid]):
                p.files_for(f"{d}/task/{tid}", tid, root=root)
    os.symlink("1", f"{root}/self")
    for path, data in (extra or {}).items():
        write(f"{root}/{path}", data)
