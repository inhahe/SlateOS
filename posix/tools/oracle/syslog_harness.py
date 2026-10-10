"""glibc 2.39's syslog(3), as the oracle for posix/src/syslog.rs.

    python posix/tools/oracle/syslog_harness.py   # writes posix/src/syslog_oracle.txt

What a program's `openlog`, `syslog`, `setlogmask` and `closelog` do: each
datagram or stream record that reaches `/dev/log`, byte for byte; what
`LOG_PERROR` copies to standard error; what `LOG_CONS` writes to the console
when the log cannot be reached; and what `setlogmask` returns.

Run in WSL inside private user, mount and network namespaces (`unshare -r -m
-n`, as `scripts/syslog-client-check.sh` does). Each scenario is a process of
its own -- glibc's logger state is per process -- in a mount namespace of its
own, where a socket of the harness's stands in for the daemon at `/dev/log`
and a FIFO of the harness's for `/dev/console`. Nothing reaches the host's
journal or console.

A scenario starts with a datagram daemon listening unless it says otherwise:
`stream` starts a stream daemon instead, and `none` leaves `/dev/log` a
socket nobody listens on. Its statements may stop the daemon
(`daemon_stop()`) or start one of either type at a new socket
(`daemon_start(...)`), which is what a daemon's restart looks like to a
client: the socket it was connected to is gone, and the path names another.

    === <scenario>
    daemon <dgram | stream | none: what listens at /dev/log when it starts>
    decl <a buffer the statements after it use: `char NAME[N]` of N-1 bytes C>
    call <statement>
    returned <setlogmask's result>
    sent <hex>       one datagram, or what a stream delivered since the last look
    stderr <hex>
    console <hex>

The timestamp -- `Mmm dd hh:mm:ss `, sixteen bytes after `<pri>` -- is the
clock's, so it is replaced by sixteen `@`s in each record; the replay checks
its own against `strftime ("%h %e %T ")` separately. The scenario's PID is
`[PID]`. The identity is fixed by `openlog`, or is the program name
`syslog-oracle` when it is not.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "syslog_oracle.txt"

# (name, daemon, [C statements]); each statement that logs is followed by a
# look at everything the logger could have written to.
SCENARIOS = []


def sc(name, *calls, daemon="dgram"):
    SCENARIOS.append((name, daemon, list(calls)))


sc("default tag and facility", 'syslog(LOG_INFO, "hello")')
sc("pid", 'openlog("tag", LOG_PID, 0)', 'syslog(LOG_NOTICE, "with pid")')
sc("tag only", 'openlog("tag", 0, 0)', 'syslog(LOG_ERR, "no pid")')
sc("facility from openlog", 'openlog("tag", 0, LOG_DAEMON)', 'syslog(LOG_WARNING, "daemon")')
sc("facility in the priority wins", 'openlog("tag", 0, LOG_DAEMON)',
   'syslog(LOG_LOCAL3 | LOG_DEBUG, "local3")')
sc("kern is the default facility's zero", 'syslog(LOG_KERN | LOG_EMERG, "kern is 0, so user")')
sc("an invalid facility in openlog is ignored", 'openlog("tag", 0, 5)',
   'syslog(LOG_INFO, "still user")')
sc("invalid bits", 'syslog(0x10000 | LOG_INFO, "after the complaint")')
sc("negative priority", 'syslog(-1, "minus one")')
sc("the complaint is copied to stderr too", 'openlog("tag", LOG_PERROR, LOG_USER)',
   'syslog(0x10000 | LOG_INFO, "x")')
sc("the complaint is masked like any error", 'setlogmask(LOG_MASK(LOG_INFO))',
   'syslog(0x10000 | LOG_INFO, "only this")')
sc("the complaint does not disturb percent m", 'errno = ENOENT',
   'syslog(0x10000 | LOG_ERR, "%m")')
sc("mask", 'setlogmask(LOG_UPTO(LOG_WARNING))', 'syslog(LOG_INFO, "masked")',
   'syslog(LOG_ERR, "kept")', 'setlogmask(0)', 'syslog(LOG_DEBUG, "still masked")',
   'setlogmask(LOG_UPTO(LOG_DEBUG))', 'syslog(LOG_DEBUG, "unmasked")')
sc("percent m", 'errno = ENOENT', 'syslog(LOG_ERR, "open: %m")')
sc("percent m names", 'errno = EACCES', 'syslog(LOG_ERR, "%#m [%m]")')
sc("format", 'syslog(LOG_INFO, "%d %s %5.2f %x %c", 42, "str", 3.14159, 255, 65)')
sc("a percent sign", 'syslog(LOG_INFO, "100%% sure")')
sc("trailing newline", 'openlog("tag", LOG_PERROR, 0)', 'syslog(LOG_INFO, "line\\n")')
sc("perror", 'openlog("tag", LOG_PERROR | LOG_PID, LOG_USER)', 'syslog(LOG_INFO, "both")')
sc("perror empty message", 'openlog("tag", LOG_PERROR, 0)', 'syslog(LOG_INFO, "")')
sc("an embedded nul", 'openlog("tag", LOG_PERROR, LOG_USER)', 'syslog(LOG_INFO, "a%cb", 0)')
sc("long message", 'char big[3000]; memset(big, \'x\', sizeof big - 1); big[sizeof big - 1] = 0',
   'syslog(LOG_INFO, "%s", big)')
sc("just over the buffer", 'char big[1001]; memset(big, \'y\', sizeof big - 1); big[sizeof big - 1] = 0',
   'syslog(LOG_INFO, "%s", big)')
sc("a tag longer than the buffer",
   'char tag[2000]; memset(tag, \'t\', sizeof tag - 1); tag[sizeof tag - 1] = 0',
   'openlog(tag, LOG_PID, LOG_USER)', 'syslog(LOG_INFO, "after a long tag")')
sc("non utf8", 'syslog(LOG_INFO, "byte \\xff here")')
sc("closelog forgets the tag", 'openlog("first", LOG_PID, 0)', 'syslog(LOG_INFO, "one")',
   'closelog()', 'syslog(LOG_INFO, "two")', 'closelog()', 'closelog()')
sc("openlog null keeps the tag", 'openlog("kept", 0, 0)', 'openlog(NULL, LOG_PID, 0)',
   'syslog(LOG_INFO, "kept, now with pid")')
sc("ndelay", 'openlog("tag", LOG_NDELAY, 0)', 'syslog(LOG_INFO, "connected early")')
sc("vsyslog", 'call_vsyslog(LOG_INFO, "v %d", 7)')
sc("chk", '__syslog_chk(LOG_INFO, 1, "chk %d", 8)')
sc("empty tag", 'openlog("", LOG_PID, 0)', 'syslog(LOG_INFO, "empty")')
sc("every level",
   *[f'syslog({lvl}, "{lvl}")' for lvl in
     ("LOG_EMERG", "LOG_ALERT", "LOG_CRIT", "LOG_ERR", "LOG_WARNING", "LOG_NOTICE", "LOG_INFO",
      "LOG_DEBUG")])
sc("every facility",
   *[f'syslog({fac} | LOG_INFO, "{fac}")' for fac in
     ("LOG_KERN", "LOG_USER", "LOG_MAIL", "LOG_DAEMON", "LOG_AUTH", "LOG_SYSLOG", "LOG_LPR",
      "LOG_NEWS", "LOG_UUCP", "LOG_CRON", "LOG_AUTHPRIV", "LOG_FTP", "LOG_LOCAL0", "LOG_LOCAL7")])

# What happens when the log is not a datagram socket, or not there at all.
sc("a stream daemon", 'syslog(LOG_INFO, "over a stream")', 'syslog(LOG_INFO, "and again")',
   'closelog()', 'syslog(LOG_INFO, "after closelog")', daemon="stream")
sc("a stream daemon and the complaint", 'syslog(0x10000 | LOG_INFO, "two records")',
   daemon="stream")
sc("no daemon", 'syslog(LOG_INFO, "lost")', daemon="none")
sc("no daemon, the console", 'openlog("tag", LOG_CONS | LOG_PID, 0)',
   'syslog(LOG_ERR, "to the console")', 'syslog(LOG_INFO, "line\\n")',
   'syslog(LOG_INFO, "a%cb", 0)', daemon="none")
sc("no daemon, the console and stderr", 'openlog("tag", LOG_CONS | LOG_PERROR, LOG_USER)',
   'syslog(LOG_ERR, "twice")', daemon="none")
sc("no daemon, the complaint goes to the console",
   'openlog("tag", LOG_CONS, LOG_USER)', 'syslog(0x10000 | LOG_INFO, "x")', daemon="none")
sc("ndelay with no daemon, then one", 'openlog("tag", LOG_NDELAY, LOG_USER)',
   'syslog(LOG_INFO, "lost")', 'daemon_start(SOCK_DGRAM)', 'syslog(LOG_INFO, "found")',
   daemon="none")
sc("the daemon restarts", 'syslog(LOG_INFO, "before")', 'daemon_stop()',
   'daemon_start(SOCK_DGRAM)', 'syslog(LOG_INFO, "after the restart")')
sc("the daemon goes away", 'openlog("tag", LOG_CONS, LOG_USER)', 'syslog(LOG_INFO, "before")',
   'daemon_stop()', 'syslog(LOG_ERR, "to the console")', 'daemon_start(SOCK_DGRAM)',
   'syslog(LOG_INFO, "back")')
sc("the daemon comes back as a stream", 'syslog(LOG_INFO, "datagram")', 'daemon_stop()',
   'daemon_start(SOCK_STREAM)', 'syslog(LOG_INFO, "stream")')
sc("a stream daemon goes away", 'openlog("tag", LOG_CONS, LOG_USER)',
   'syslog(LOG_INFO, "before")', 'daemon_stop()', 'syslog(LOG_ERR, "to the console")',
   'daemon_start(SOCK_DGRAM)', 'syslog(LOG_INFO, "a stream logger finds a datagram daemon")',
   daemon="stream")

C_MAIN = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <syslog.h>
#include <unistd.h>

extern void __syslog_chk(int, int, const char *, ...);

static const char *sockdir;
static int scenario_no;
static int daemons;             /* sockets made so far, for unique names */
static int listener = -1;       /* the daemon's socket; -1 when none runs */
static int listener_type;
static int conns[16];           /* a stream daemon's accepted connections */
static int nconns;
static int errpipe[2];
static int console = -1;        /* read end of the FIFO over /dev/console */

static void fail(const char *what) {
    printf("harness: %s: %s\n", what, strerror(errno));
    fflush(stdout);
    _exit(3);
}

static void hex(const unsigned char *s, size_t n) {
    if (n == 0) { fputs("\"\"", stdout); return; }
    for (size_t i = 0; i < n; i++) printf("%02x", s[i]);
}

/* The timestamp of each record in s[0..n) masked: a stream's records end in
   a NUL each, a datagram is one record. */
static void mask(unsigned char *s, size_t n) {
    size_t i = 0;
    while (i < n) {
        size_t end = i;
        while (end < n && s[end] != 0) end++;
        unsigned char *rec = s + i;
        size_t len = end - i;
        unsigned char *gt = memchr(rec, '>', len);
        if (gt && (size_t)(gt - rec) + 17 <= len && gt[16] == ' ' && gt[4] == ' ')
            memset(gt + 1, '@', 16);
        i = end + 1;
    }
}

/* A daemon of `type` listening at a new socket, mounted over /dev/log. */
static void daemon_start(int type) {
    char path[512];
    snprintf(path, sizeof path, "%s/log-%d-%d.sock", sockdir, scenario_no, ++daemons);
    /* Non-blocking, so that a look finds what is there and no more. */
    int s = socket(AF_UNIX, type | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (s < 0) fail("socket");
    struct sockaddr_un a = { .sun_family = AF_UNIX };
    strncpy(a.sun_path, path, sizeof a.sun_path - 1);
    if (bind(s, (struct sockaddr *)&a, sizeof a) != 0) fail("bind");
    if (type == SOCK_STREAM && listen(s, 16) != 0) fail("listen");
    /* The last daemon's socket, if one is mounted; nothing to undo if not. */
    umount2("/dev/log", MNT_DETACH);
    if (mount(path, "/dev/log", NULL, MS_BIND, NULL) != 0) fail("mount /dev/log");
    listener = s;
    listener_type = type;
}

/* The daemon gone: its socket closed, the path left naming it. */
static void daemon_stop(void) {
    for (int i = 0; i < nconns; i++) close(conns[i]);
    nconns = 0;
    if (listener >= 0) close(listener);
    listener = -1;
}

/* Print everything written to the log, stderr and the console since the last
   look. */
static void drain(void) {
    static unsigned char buf[65536];
    if (listener >= 0 && listener_type == SOCK_DGRAM) {
        for (;;) {
            ssize_t n = recv(listener, buf, sizeof buf, MSG_DONTWAIT);
            if (n < 0) break;
            mask(buf, (size_t)n);
            fputs("sent ", stdout);
            hex(buf, (size_t)n);
            putchar('\n');
        }
    }
    if (listener >= 0 && listener_type == SOCK_STREAM) {
        for (;;) {
            int c = accept4(listener, NULL, NULL, SOCK_NONBLOCK | SOCK_CLOEXEC);
            if (c < 0) break;
            if (nconns == 16) fail("too many connections");
            conns[nconns++] = c;
        }
    }
    for (int i = 0; i < nconns; i++) {
        size_t got = 0;
        for (;;) {
            ssize_t n = read(conns[i], buf + got, sizeof buf - got);
            if (n <= 0) break;
            got += (size_t)n;
        }
        if (got) {
            mask(buf, got);
            fputs("sent ", stdout);
            hex(buf, got);
            putchar('\n');
        }
    }
    ssize_t m = read(errpipe[0], buf, sizeof buf);
    if (m > 0) { fputs("stderr ", stdout); hex(buf, (size_t)m); putchar('\n'); }
    m = read(console, buf, sizeof buf);
    if (m > 0) { fputs("console ", stdout); hex(buf, (size_t)m); putchar('\n'); }
}

static void call_vsyslog(int pri, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vsyslog(pri, fmt, ap);
    va_end(ap);
}

@SCENARIOS@

/* What each scenario has around it: a mount namespace of its own, so its
   daemons are its own; stderr into a pipe; a FIFO over /dev/console. */
static void setup(const char *daemon) {
    if (unshare(CLONE_NEWNS) != 0) fail("unshare");
    if (mount("none", "/", NULL, MS_REC | MS_PRIVATE, NULL) != 0) fail("make private");
    char fifo[512];
    snprintf(fifo, sizeof fifo, "%s/console-%d", sockdir, scenario_no);
    if (mkfifo(fifo, 0600) != 0) fail("mkfifo");
    console = open(fifo, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (console < 0) fail("open console fifo");
    if (mount(fifo, "/dev/console", NULL, MS_BIND, NULL) != 0) fail("mount /dev/console");
    if (strcmp(daemon, "stream") == 0) {
        daemon_start(SOCK_STREAM);
    } else {
        daemon_start(SOCK_DGRAM);
        if (strcmp(daemon, "none") == 0) daemon_stop();
    }
    if (pipe(errpipe) != 0) fail("pipe");
    fcntl(errpipe[0], F_SETFL, O_NONBLOCK);
    fflush(stdout);
    if (dup2(errpipe[1], 2) < 0) fail("dup2");
}

static void run(const char *name, const char *daemon, void (*scenario)(void)) {
    printf("=== %s\ndaemon %s\n", name, daemon);
    fflush(stdout);
    scenario_no++;
    pid_t pid = fork();
    if (pid == 0) {
        setup(daemon);
        scenario();
        fflush(stdout);
        _exit(0);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) printf("ended %d\n", status);
    fflush(stdout);
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    sockdir = argv[1];
@MAIN@
    return 0;
}
'''

RUNNER = r'''
set -e
cd "$1"
if [ -z "${IN_NS:-}" ]; then
  export IN_NS=1
  exec unshare -r -m -n bash "$0" "$@"
fi
[ -e /dev/log ] || { echo "no /dev/log to stand in for" >&2; exit 1; }
[ -e /dev/console ] || { echo "no /dev/console to stand in for" >&2; exit 1; }
# A Linux directory for the sockets and FIFOs: the work directory is on a
# Windows drive, which cannot hold either.
sockdir="$(mktemp -d)"
trap 'rm -rf "$sockdir"' EXIT
./syslog-oracle "$sockdir" > out.txt
'''

LOGGING = ("syslog(", "call_vsyslog(", "__syslog_chk(")


def body(calls):
    """The scenario's statements, each announced, with everything the logger
    writes to looked at after each that logs, and `setlogmask`'s result
    printed. The harness's own output goes around `errno`, so a statement
    that sets it is seen by the next as it was set."""
    out = ['    { int e = errno; printf("pid %d\\n", (int)getpid()); fflush(stdout); errno = e; }']
    for c in calls:
        if c.startswith("char "):
            # A buffer the statements after it use, announced so that the
            # replay can make the same one.
            out.append(f'    printf("decl %s\\n", {c_str(c)});')
            out.append(f"    {c};")
            continue
        out.append(f'    {{ int e = errno; printf("call %s\\n", {c_str(c)}); fflush(stdout); errno = e; }}')
        if c.startswith("setlogmask("):
            out.append(f'    {{ int r = {c}; int e = errno; printf("returned %d\\n", r); '
                       f'fflush(stdout); errno = e; }}')
            continue
        out.append(f"    {c};")
        if c.startswith(LOGGING):
            out.append("    { int e = errno; drain(); fflush(stdout); errno = e; }")
    return "\n".join(out)


def c_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main() -> None:
    funcs, calls = [], []
    for i, (name, daemon, steps) in enumerate(SCENARIOS):
        funcs.append(f"static void scenario_{i}(void) {{\n{body(steps)}\n}}")
        calls.append(f"    run({c_str(name)}, {c_str(daemon)}, scenario_{i});")
    src = C_MAIN.replace("@SCENARIOS@", "\n\n".join(funcs)).replace("@MAIN@", "\n".join(calls))
    with workdir() as d:
        d = Path(d)
        (d / "oracle.c").write_text(src, encoding="utf-8", newline="\n")
        (d / "run.sh").write_text(RUNNER, encoding="utf-8", newline="\n")
        # -Wno-format: the formats are the point, and gcc 13 knows neither
        # `%#m` (glibc 2.35's) nor a reason to log an empty message.
        built = run(f"cd {wsl_path(d)} && gcc -O1 -Wall -Werror -Wno-format "
                    f"-Wno-unused-function -o syslog-oracle oracle.c")
        if built.returncode or built.stdout or built.stderr:
            sys.exit(f"oracle.c: {built.stdout}{built.stderr}")
        r = run(f"bash {wsl_path(d)}/run.sh {wsl_path(d)}")
        if r.returncode or r.stderr:
            sys.exit(f"run: {r.stdout}{r.stderr}")
        text = (d / "out.txt").read_text(encoding="utf-8")
    bad = [line for line in text.splitlines() if line.startswith(("ended ", "harness: "))]
    if bad:
        sys.exit("a scenario failed:\n" + "\n".join(bad))
    OUT.write_text(mask_pids(text), encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(SCENARIOS)} scenarios, {len(text.splitlines())} lines")


def mask_pids(text):
    """Each scenario's own PID, as `[PID]`, in what it sent and printed: the
    number is the run's, and the replay masks its own the same way."""
    out, pid = [], None
    for line in text.splitlines():
        if line.startswith("pid "):
            pid = line.split()[1]
            continue
        kind, _, rest = line.partition(" ")
        if pid and kind in ("sent", "stderr", "console") and rest != '""':
            data = bytes.fromhex(rest).replace(f"[{pid}]".encode(), b"[PID]")
            line = f"{kind} {data.hex()}"
        out.append(line)
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    main()
