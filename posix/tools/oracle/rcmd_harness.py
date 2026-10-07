"""glibc 2.39's BSD remote-execution calls (<netdb.h>) -- rcmd, rcmd_af,
rresvport, rresvport_af, ruserok, ruserok_af, iruserok, iruserok_af, rexec and
rexec_af, and the ruserpass that reads ~/.netrc for rexec -- as the oracle for
posix/src/rcmd.rs.

    python posix/tools/oracle/rcmd_harness.py   # writes posix/src/rcmd_oracle.txt

The calls read fixed paths (/etc/hosts.equiv, /etc/passwd, /etc/hosts,
/etc/netgroup) and bind ports below 1024, so the probe runs statically linked
in user, mount, network and UTS namespaces of its own (`unshare -rmnu`): root
there, with a tmpfs over /etc and the host name `probe.example.com`; each
protocol probe in a network namespace of its own besides, loopback up, so
that no port one leaves in TIME_WAIT moves the next one's. Every
user in its /etc/passwd is uid 0, the only uid the namespace maps.

Three parts, one line a probe:

- `ruserok` / `iruserok`: a remote host, user and local user against a
  /etc/hosts.equiv and a ~/.rhosts -- their text, mode, kind (file,
  directory, symbolic link) and link count, or absent -- giving the answer
  and `__rcmd_errstr`.
- `netrc`: `ruserpass` over a ~/.netrc's text and mode, for a host and a
  name and password given or not: the answer, the two strings, and what it
  printed (`warnx`, as `rcprobe: ...`).
- `rcmd` / `rexec` / `rresvport`: each call against a server the probe forks
  first -- listening or not, connecting back for the stderr channel from a
  given port or not, its reply -- giving the answer, `errno`, `*ahost`, what
  the call printed to stderr, what the caller then read on each channel,
  every byte the server received, and the port the client came from. A port
  marked busy is bound by the probe beforehand. `rresvport` lines give the
  port it settled on.

Fields are hex (`-` none; `~` an empty string; `NULL` a null pointer).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "rcmd_oracle.txt"

PASSWD = (
    "root:x:0:0:root:/tmp/rc/h/root:/bin/sh\n"
    "alice:x:0:0::/tmp/rc/h/alice:/bin/sh\n"
    "bob:x:0:0::/tmp/rc/h/bob:/bin/sh\n"
    "carol:x:0:0::/tmp/rc/h/nowhere:/bin/sh\n"
)
HOSTS = (
    "127.0.0.1 localhost\n"
    "127.0.0.2 alpha alpha.example.com\n"
    "127.0.0.3 beta\n"
    "127.0.0.4 gamma\n"
    "127.0.0.5 canon canon-alias\n"
    "::1 localhost6 dual\n"
    "127.0.0.1 dual\n"
    "fe80::2 sixhost\n"
)
NETGROUP = (
    "trusted (alpha,alice,) (beta,,)\n"
    "users (,bob,) (,carol,)\n"
)
NSSWITCH = "passwd: files\ngroup: files\nhosts: files\nnetgroup: files\n"
# Every line of /etc/hosts that names a host, not only the first: so `dual`
# has two addresses, and rcmd one to fall back on.
HOST_CONF = "multi on\n"

# -- ruserok and iruserok ------------------------------------------------------

# A file: None (absent), or (text, mode, kind, links) with kind "f", "d" or "l".
def f(text: str, mode: int = 0o600, kind: str = "f", links: int = 1):
    return (text, mode, kind, links)


EQUIVS = [
    None,
    f(""),
    f("alpha\n"),
    f("alpha bob\n"),
    f("alpha alice\n"),
    f("ALPHA bob\n"),
    f("alpha.example.com bob\n"),
    f("127.0.0.2 bob\n"),
    f("+\n"),
    f("+ bob\n"),
    f("+ -bob\n"),
    f("-alpha\nalpha bob\n"),
    f("alpha -bob\nalpha bob\n"),
    f("alpha bob\n-alpha\n"),
    f("+@trusted\n"),
    f("+@trusted +@users\n"),
    f("-@trusted\n+\n"),
    f("+ -@users\n+\n"),
    f("# comment\n\nalpha bob\n"),
    f("  alpha bob\n"),
    f("\talpha bob\n"),
    f("beta\nalpha  \t bob  trailing\n"),
    f("alpha bob"),
    f("gamma bob\nalpha bob\n"),
    f("nosuchhost bob\nalpha bob\n"),
    f("alpha +\n"),
    f("alpha bob\n", mode=0o644),
    f("alpha bob\n", mode=0o620),
    f("alpha bob\n", mode=0o602),
    f("alpha bob\n", links=2),
    f("", kind="d"),
    f("alpha bob\n", kind="l"),
]
RHOSTS = [
    None,
    f("alpha bob\n"),
    f("alpha\n"),
    f("+ +\n"),
    f("-alpha\n+ +\n"),
    f("alpha bob\n", mode=0o644),
    f("alpha bob\n", mode=0o620),
    f("alpha bob\n", mode=0o606),
    f("alpha bob\n", links=2),
    f("", kind="d"),
    f("alpha bob\n", kind="l"),
    f("beta root\n"),
]
QUERIES = [
    # (rhost, superuser, ruser, luser)
    ("alpha", 0, "bob", "alice"),
    ("alpha", 0, "alice", "alice"),
    ("ALPHA", 0, "bob", "alice"),
    ("127.0.0.2", 0, "bob", "alice"),
    ("beta", 0, "bob", "alice"),
    ("gamma", 0, "carol", "alice"),
    ("nosuchhost", 0, "bob", "alice"),
    ("alpha", 1, "bob", "alice"),
    ("beta", 1, "root", "root"),
    ("alpha", 0, "bob", "nosuchuser"),
    ("alpha", 0, "bob", "carol"),
]

PROBES: list[str] = []


def c_str(s) -> str:
    if s is None:
        return "NULL"
    return '"' + "".join(f"\\x{b:02x}" for b in s.encode()) + '"'


def c_file(spec) -> str:
    if spec is None:
        return "{0, NULL, 0, 0, 0}"
    text, mode, kind, links = spec
    return f"{{1, {c_str(text)}, 0{mode:o}, '{kind}', {links}}}"


for equiv in EQUIVS:
    for q in QUERIES:
        rhost, su, ruser, luser = q
        for af in (2, 0):
            PROBES.append(f"{{ static const struct file e = {c_file(equiv)}, r = {c_file(None)}; "
                          f"ruserok_case(&e, &r, {c_str(rhost)}, {su}, {c_str(ruser)}, {c_str(luser)}, {af}); }}")
for rh in RHOSTS:
    for q in QUERIES:
        rhost, su, ruser, luser = q
        PROBES.append(f"{{ static const struct file e = {c_file(None)}, r = {c_file(rh)}; "
                      f"ruserok_case(&e, &r, {c_str(rhost)}, {su}, {c_str(ruser)}, {c_str(luser)}, 2); }}")
    # Both files: hosts.equiv first, then .rhosts.
    PROBES.append(f"{{ static const struct file e = {c_file(f('beta bob'))}, r = {c_file(rh)}; "
                  f"ruserok_case(&e, &r, {c_str('alpha')}, 0, {c_str('bob')}, {c_str('alice')}, 2); }}")
for equiv in [None, f("alpha bob\n"), f("127.0.0.2 bob\n"), f("+@trusted\n"), f("-alpha\n+\n"),
              f("localhost6 bob\n"), f("::1 bob\n"), f("+ bob\n")]:
    for addr, af in [("127.0.0.2", 2), ("127.0.0.3", 2), ("::1", 10), ("127.0.0.2", 99)]:
        for su, ruser, luser in [(0, "bob", "alice"), (1, "bob", "alice"), (0, "carol", "alice")]:
            PROBES.append(f"{{ static const struct file e = {c_file(equiv)}, r = {c_file(f('alpha bob'))}; "
                          f"iruserok_case(&e, &r, {c_str(addr)}, {af}, {su}, {c_str(ruser)}, {c_str(luser)}); }}")

# -- ruserpass: ~/.netrc ---------------------------------------------------------

NETRCS = [
    None,
    f("machine localhost login carol password secret\n"),
    f("machine localhost login carol password secret\n", mode=0o644),
    f("machine localhost login anonymous password guest\n", mode=0o644),
    f("machine other login dave password d1\nmachine localhost login carol password c1\n"),
    f("default login guest password g1\n"),
    f("machine other login dave\ndefault login guest password g1\n"),
    f("machine LOCALHOST login carol password c1\n"),
    f('machine localhost login "car ol" password "p\\"w"\n'),
    f("machine localhost login c\\ arol password p\\,w\n"),
    f("machine localhost,login,carol,password,secret\n"),
    f("machine localhost login carol account acct password secret\n"),
    f("machine localhost login carol macdef init\nput x\n\npassword secret\n"),
    f("machine localhost login carol bogus password secret\n"),
    f("machine localhost password secret login carol\n"),
    f("machine localhost login carol\nmachine localhost login dave password d1\n"),
    f("machine alpha login carol password secret\n"),
    f("machine localhost\n"),
    f("login carol password secret\n"),
    f("machine localhost login carol password secret", mode=0o600),
    f("", kind="d"),
]
NETRC_QUERIES = [
    # (host, name, pass)
    ("localhost", None, None),
    ("localhost", "carol", None),
    ("localhost", "dave", None),
    ("localhost", "carol", "given"),
    ("other", None, None),
    ("alpha", None, None),
    ("unknown", None, None),
    ("alpha.example.com", None, None),
    ("alpha.other.com", None, None),
]
for netrc in NETRCS:
    for host, name, pw in NETRC_QUERIES:
        if netrc is not None and "password" in netrc[0] and name is None and "machine localhost password" in netrc[0]:
            # glibc's ruserpass reads `*aname` for a password line seen before
            # any login, a NULL here: it would crash.
            continue
        PROBES.append(f"{{ static const struct file n = {c_file(netrc)}; "
                      f"netrc_case(&n, {c_str(host)}, {c_str(name)}, {c_str(pw)}, 1); }}")
for host, name, pw in [("localhost", "carol", "given")]:
    PROBES.append(f"{{ static const struct file n = {c_file(None)}; "
                  f"netrc_case(&n, {c_str(host)}, {c_str(name)}, {c_str(pw)}, 0); }}")

# -- the protocol ---------------------------------------------------------------

# A server: (family, port, back, read_strings, reply, data, errdata). family 0
# is none listening; back is the port the stderr channel is connected from (0
# any), -1 to write a byte on the main connection instead, -2 to close it.

def srv(family=2, port=514, back=1000, strings=1, reply="\0", data="out\n", errdata="err\n"):
    return (family, port, back, strings, reply, data, errdata)


def c_srv(s) -> str:
    family, port, back, strings, reply, data, errdata = s
    return (f"{{{family}, {port}, {back}, {strings}, {c_str(reply)}, {len(reply.encode())}, "
            f"{c_str(data)}, {c_str(errdata)}}}")


def c_busy(ports) -> str:
    return "{" + ", ".join(str(p) for p in list(ports) + [0]) + "}"


RCMD = [
    # (name, af, host, port, loc, rem, cmd, fd2, server, busy)
    ("plain", 2, "localhost", 514, "alice", "bob", "ls -l", 0, srv(), []),
    ("stderr", 2, "localhost", 514, "alice", "bob", "ls -l", 1, srv(), []),
    ("stderr-busy", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=1000), [1023, 1022]),
    ("numeric", 2, "127.0.0.1", 514, "alice", "bob", "x", 0, srv(), []),
    ("canonical", 2, "canon-alias", 514, "alice", "bob", "x", 0, srv(), []),
    ("reject", 2, "localhost", 514, "alice", "bob", "x", 0,
     srv(reply="\1Permission denied.\nmore\n"), []),
    ("reject-no-newline", 2, "localhost", 514, "alice", "bob", "x", 1,
     srv(reply="\1denied"), []),
    ("short", 2, "localhost", 514, "alice", "bob", "x", 0, srv(reply=""), []),
    ("unknown-host", 2, "nosuchhost", 514, "alice", "bob", "x", 0, srv(family=0), []),
    ("bad-family", 1, "localhost", 514, "alice", "bob", "x", 0, srv(family=0), []),
    ("back-unreserved", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=2000), []),
    ("back-too-low", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=100), []),
    ("back-511", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=511), []),
    ("back-512", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=512), []),
    ("no-back-data", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=-1), []),
    ("no-back-close", 2, "localhost", 514, "alice", "bob", "x", 1, srv(back=-2), []),
    ("v6", 10, "::1", 514, "alice", "bob", "x", 1, srv(family=10), []),
    ("unspec-v4-only", 0, "dual", 514, "alice", "bob", "x", 0, srv(), []),
    ("other-port", 2, "localhost", 2222, "alice", "bob", "x", 0, srv(port=2222), []),
    ("empty-strings", 2, "localhost", 514, "", "", "", 0, srv(), []),
    ("refused", 2, "localhost", 514, "alice", "bob", "x", 0, srv(family=0), []),
]
REXEC = [
    # (name, af, host, port, user, pass, cmd, fd2, server, home-netrc)
    ("plain", 2, "localhost", 512, "carol", "secret", "ls", 0, srv(port=512, back=0), None),
    ("stderr", 2, "localhost", 512, "carol", "secret", "ls", 1, srv(port=512, back=0), None),
    ("netrc", 2, "localhost", 512, None, None, "ls", 0, srv(port=512, back=0),
     f("machine localhost login carol password secret\n")),
    ("netrc-name-given", 2, "localhost", 512, "carol", None, "ls", 0, srv(port=512, back=0),
     f("machine localhost login carol password secret\n")),
    ("netrc-open", 2, "localhost", 512, "carol", "given", "ls", 0, srv(port=512, back=0),
     f("machine localhost login carol password secret\n", mode=0o644)),
    ("reject", 2, "localhost", 512, "carol", "secret", "ls", 0,
     srv(port=512, back=0, reply="\1Login incorrect.\n"), None),
    ("short", 2, "localhost", 512, "carol", "secret", "ls", 0, srv(port=512, back=0, reply=""), None),
    ("unknown-host", 2, "nosuchhost", 512, "carol", "secret", "ls", 0, srv(family=0), None),
    ("canonical", 2, "canon-alias", 512, "carol", "secret", "ls", 0, srv(port=512, back=0), None),
    ("v6", 10, "::1", 512, "carol", "secret", "ls", 1, srv(family=10, port=512, back=0), None),
    ("refused", 2, "localhost", 512, "carol", "secret", "ls", 0, srv(family=0), None),
]
RRESVPORT = [
    # (af, start, busy)
    (2, 1023, []), (2, 1023, [1023]), (2, 1023, [1023, 1022, 1021]), (2, 600, []),
    (2, 512, []), (2, 511, []), (2, 0, []), (2, -5, []), (2, 1024, []), (2, 2000, []),
    (2, 512, [512]), (2, 512, [512, 1023]), (10, 1023, []), (1, 1023, []),
    (2, 700, list(range(512, 1024))),
]
for name, af, host, port, loc, rem, cmd, fd2, server, busy in RCMD:
    PROBES.append(f"ISOLATE({{ static const struct srv v = {c_srv(server)}; static const int busy[] = {c_busy(busy)}; "
                  f"rcmd_case({c_str(name)}, {af}, {c_str(host)}, {port}, {c_str(loc)}, {c_str(rem)}, "
                  f"{c_str(cmd)}, {fd2}, &v, busy); }});")
for name, af, host, port, user, pw, cmd, fd2, server, netrc in REXEC:
    PROBES.append(f"ISOLATE({{ static const struct srv v = {c_srv(server)}; static const struct file n = {c_file(netrc)}; "
                  f"rexec_case({c_str(name)}, {af}, {c_str(host)}, {port}, {c_str(user)}, {c_str(pw)}, "
                  f"{c_str(cmd)}, {fd2}, &v, &n); }});")
for af, start, busy in RRESVPORT:
    PROBES.append(f"ISOLATE({{ static const int busy[] = {c_busy(busy)}; rresvport_case({af}, {start}, busy); }});")

PROGRAM = r'''
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <net/if.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

extern char *__rcmd_errstr;
int ruserpass(const char *host, const char **aname, const char **apass);

struct file { int present; const char *text; int mode; char kind; int links; };
struct srv { int family, port, back, strings; const char *reply; int reply_len;
             const char *data, *errdata; };

static void hex(const void *p, size_t n) {
    const unsigned char *b = p;
    if (!n) { putchar('-'); return; }
    for (size_t i = 0; i < n; i++) printf("%02x", b[i]);
}
static void hexs(const char *s) {
    if (!s) fputs("NULL", stdout);
    else if (!*s) putchar('~');
    else hex(s, strlen(s));
}
static void spec(const struct file *f) {
    if (!f->present) { putchar('-'); return; }
    printf("%c:%o:%d:", f->kind, f->mode, f->links);
    hexs(f->text);
}

/* What the call prints goes to fd 2: through a pipe while it runs. */
static int cap_saved, cap_pipe[2];
static void cap_begin(void) {
    fflush(stderr);
    cap_saved = dup(2);
    if (pipe(cap_pipe)) abort();
    dup2(cap_pipe[1], 2);
    close(cap_pipe[1]);
}
static size_t cap_end(char *buf, size_t cap) {
    fflush(stderr);
    dup2(cap_saved, 2);
    close(cap_saved);
    size_t n = 0;
    ssize_t r;
    while (n < cap && (r = read(cap_pipe[0], buf + n, cap - n)) > 0) n += r;
    close(cap_pipe[0]);
    return n;
}

static void put_file(const char *path, const struct file *f) {
    char aside[512];
    snprintf(aside, sizeof aside, "%s.link", path);
    unlink(path); rmdir(path); unlink(aside);
    if (!f->present) return;
    if (f->kind == 'd') { mkdir(path, f->mode); return; }
    const char *target = path;
    if (f->kind == 'l') target = "/tmp/rc/target";
    unlink(target);
    int fd = open(target, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0 || write(fd, f->text, strlen(f->text)) != (ssize_t) strlen(f->text)) abort();
    close(fd);
    chmod(target, f->mode);
    if (f->kind == 'l') { if (symlink(target, path)) abort(); }
    if (f->links > 1) { if (link(path, aside)) abort(); }
}

static void ruserok_case(const struct file *equiv, const struct file *rhosts, const char *rhost,
                         int su, const char *ruser, const char *luser, int af) {
    put_file("/etc/hosts.equiv", equiv);
    put_file("/tmp/rc/h/alice/.rhosts", rhosts);
    put_file("/tmp/rc/h/root/.rhosts", rhosts);
    __rcmd_errstr = NULL;
    errno = 0;
    int r = ruserok_af(rhost, su, ruser, luser, af);
    printf("ruserok %d ", af); hexs(rhost); printf(" %d ", su); hexs(ruser); putchar(' ');
    hexs(luser); putchar(' '); spec(equiv); putchar(' '); spec(rhosts);
    printf(" | %d ", r); hexs(__rcmd_errstr); putchar('\n');
}

static void iruserok_case(const struct file *equiv, const struct file *rhosts, const char *addr,
                          int af, int su, const char *ruser, const char *luser) {
    put_file("/etc/hosts.equiv", equiv);
    put_file("/tmp/rc/h/alice/.rhosts", rhosts);
    unsigned char raw[16] = {0};
    int len = strchr(addr, ':') ? 16 : 4;
    inet_pton(len == 16 ? AF_INET6 : AF_INET, addr, raw);
    __rcmd_errstr = NULL;
    int r = iruserok_af(raw, su, ruser, luser, af);
    printf("iruserok %d ", af); hex(raw, len); printf(" %d ", su); hexs(ruser); putchar(' ');
    hexs(luser); putchar(' '); spec(equiv); putchar(' '); spec(rhosts);
    printf(" | %d ", r); hexs(__rcmd_errstr); putchar('\n');
}

static void netrc_case(const struct file *netrc, const char *host, const char *name,
                       const char *pass, int with_home) {
    put_file("/tmp/rc/h/carol/.netrc", netrc);
    if (with_home) setenv("HOME", "/tmp/rc/h/carol", 1); else unsetenv("HOME");
    const char *n = name, *p = pass;
    char err[4096];
    cap_begin();
    errno = 0;
    int r = ruserpass(host, &n, &p);
    size_t en = cap_end(err, sizeof err);
    printf("netrc %d ", with_home); hexs(host); putchar(' '); hexs(name); putchar(' ');
    hexs(pass); putchar(' '); spec(netrc);
    printf(" | %d ", r); hexs(n); putchar(' '); hexs(p); putchar(' '); hex(err, en);
    putchar('\n');
    setenv("HOME", "/tmp/rc/h/carol", 1);
}

static void srv_line(const struct srv *v) {
    printf("%d %d %d %d ", v->family, v->port, v->back, v->strings);
    hex(v->reply, v->reply_len); putchar(' '); hexs(v->data); putchar(' '); hexs(v->errdata);
}

static void loopback(int family, int port, struct sockaddr_storage *ss, socklen_t *len) {
    memset(ss, 0, sizeof *ss);
    if (family == AF_INET6) {
        struct sockaddr_in6 *s6 = (struct sockaddr_in6 *) ss;
        s6->sin6_family = AF_INET6; s6->sin6_port = htons(port); s6->sin6_addr = in6addr_loopback;
        *len = sizeof *s6;
    } else {
        struct sockaddr_in *s4 = (struct sockaddr_in *) ss;
        s4->sin_family = AF_INET; s4->sin_port = htons(port);
        s4->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        *len = sizeof *s4;
    }
}

/* Read from fd until `nuls` NULs have come, EOF, or 3 s of quiet. */
static size_t read_nuls(int fd, char *buf, size_t cap, int nuls) {
    size_t n = 0;
    while (n < cap && nuls > 0) {
        struct pollfd p = { fd, POLLIN, 0 };
        if (poll(&p, 1, 3000) <= 0) break;
        ssize_t r = read(fd, buf + n, 1);
        if (r <= 0) break;
        if (buf[n] == 0) nuls--;
        n++;
    }
    return n;
}

/* `family`'s wildcard address at `port`. */
static void wildcard(int family, int port, struct sockaddr_storage *ss, socklen_t *len) {
    loopback(family, port, ss, len);
    if (family == AF_INET6) ((struct sockaddr_in6 *) ss)->sin6_addr = in6addr_any;
    else ((struct sockaddr_in *) ss)->sin_addr.s_addr = htonl(INADDR_ANY);
}

/* The server: listens, on every address, before the fork, so the client
   cannot race it. Back 0 (rexec's) connects back from any port, and what it
   received has that port's digits as one `E`: the client's own port is any
   the kernel gave it, and so is not recorded. */
static pid_t serve(const struct srv *v, int report) {
    if (!v->family) return -1;
    int l = socket(v->family, SOCK_STREAM, 0);
    int one = 1;
    setsockopt(l, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_storage ss; socklen_t len;
    wildcard(v->family, v->port, &ss, &len);
    if (bind(l, (struct sockaddr *) &ss, len) || listen(l, 4)) abort();
    pid_t pid = fork();
    if (pid) { close(l); return pid; }
    /* A client that never comes -- every address refused -- is reported as
       none, after longer than its 31 s of retries. */
    struct pollfd lp = { l, POLLIN, 0 };
    if (poll(&lp, 1, 45000) <= 0) {
        static const char none[] = " recv=- cport=0";
        (void) !write(report, none, sizeof none - 1);
        _exit(0);
    }
    struct sockaddr_storage peer; socklen_t plen = sizeof peer;
    int c = accept(l, (struct sockaddr *) &peer, &plen);
    close(l);
    char got[4096]; size_t n = 0;
    int cport = ntohs(peer.ss_family == AF_INET6 ? ((struct sockaddr_in6 *) &peer)->sin6_port
                                                  : ((struct sockaddr_in *) &peer)->sin_port);
    n += read_nuls(c, got + n, sizeof got - n, 1);
    int port = atoi(got);
    if (v->back == 0 && port != 0) {
        size_t digits = strlen(got);
        memmove(got + 1, got + digits, n - digits);
        got[0] = 'E';
        n -= digits - 1;
        cport = -1;
    }
    int e = -1;
    if (port != 0) {
        if (v->back == -1) { (void) !write(c, "x", 1); }
        else if (v->back == -2) { close(c); c = -1; }
        else {
            e = socket(v->family, SOCK_STREAM, 0);
            setsockopt(e, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
            if (v->back > 0) {
                struct sockaddr_storage b; socklen_t bl;
                wildcard(v->family, v->back, &b, &bl);
                if (bind(e, (struct sockaddr *) &b, bl)) abort();
            }
            struct sockaddr_storage to; socklen_t tl;
            loopback(v->family, port, &to, &tl);
            if (connect(e, (struct sockaddr *) &to, tl)) abort();
        }
    } else if (v->back == 0) {
        cport = -1;
    }
    if (c >= 0 && v->strings) n += read_nuls(c, got + n, sizeof got - n, 3);
    /* The client may have gone already: these may fail, and that is fine. */
    if (c >= 0 && v->back != -1) {
        if (v->reply_len) (void) !write(c, v->reply, v->reply_len);
        if (v->reply_len && v->reply[0] == 0 && *v->data) (void) !write(c, v->data, strlen(v->data));
        if (e >= 0 && v->reply_len && v->reply[0] == 0 && *v->errdata)
            (void) !write(e, v->errdata, strlen(v->errdata));
    }
    if (v->back == -1) {
        /* Wait for the client to give up on the circuit. */
        char z; struct pollfd p = { c, POLLIN, 0 }; poll(&p, 1, 3000); (void) !read(c, &z, 1);
    }
    if (c >= 0) close(c);
    if (e >= 0) close(e);
    char line[9000]; int k = 0;
    k += snprintf(line + k, sizeof line - k, " recv=");
    if (!n) k += snprintf(line + k, sizeof line - k, "-");
    for (size_t i = 0; i < n; i++) k += snprintf(line + k, sizeof line - k, "%02x", (unsigned char) got[i]);
    k += snprintf(line + k, sizeof line - k, " cport=%d", cport);
    if (write(report, line, k) != k) abort();
    _exit(0);
}

static size_t drain(int fd, char *buf, size_t cap) {
    size_t n = 0;
    for (;;) {
        struct pollfd p = { fd, POLLIN, 0 };
        if (poll(&p, 1, 3000) <= 0) break;
        ssize_t r = read(fd, buf + n, cap - n);
        if (r <= 0) break;
        n += r;
    }
    return n;
}

static int busy_fds[600], nbusy;
static void hold_busy(const int *busy, int family) {
    nbusy = 0;
    for (; *busy; busy++) {
        int s = socket(family == AF_INET6 ? AF_INET6 : AF_INET, SOCK_STREAM, 0);
        struct sockaddr_storage b; socklen_t bl;
        wildcard(family == AF_INET6 ? AF_INET6 : AF_INET, *busy, &b, &bl);
        if (bind(s, (struct sockaddr *) &b, bl)) abort();
        busy_fds[nbusy++] = s;
    }
}
static void free_busy(void) { while (nbusy) close(busy_fds[--nbusy]); }
static void busy_line(const int *busy) {
    if (!*busy) { putchar('-'); return; }
    for (int first = 1; *busy; busy++, first = 0) printf(first ? "%d" : ",%d", *busy);
}

static void call_line(const char *what, int s, int e, char *ahost, const char *err, size_t en,
                      int fd2, int report, pid_t pid) {
    char got[4096], goterr[4096]; size_t gn = 0, gen = 0;
    if (s >= 0) {
        if (fd2 >= 0) { gen = drain(fd2, goterr, sizeof goterr); close(fd2); }
        gn = drain(s, got, sizeof got);
        close(s);
    }
    char rep[9000]; size_t rn = 0;
    if (pid > 0) {
        ssize_t r;
        while ((r = read(report, rep + rn, sizeof rep - rn - 1)) > 0) rn += r;
        waitpid(pid, NULL, 0);
    }
    close(report);
    rep[rn] = 0;
    printf(" | %s %d ", s >= 0 ? "fd" : "-1", s >= 0 ? 0 : e);
    hexs(ahost); putchar(' '); hex(err, en); putchar(' '); hex(got, gn); putchar(' ');
    hex(goterr, gen);
    printf("%s\n", pid > 0 ? rep : " recv=- cport=0");
    (void) what;
}

static void rcmd_case(const char *name, int af, const char *host, int port, const char *loc,
                      const char *rem, const char *cmd, int want_fd2, const struct srv *v,
                      const int *busy) {
    int rp[2]; if (pipe(rp)) abort();
    hold_busy(busy, af);
    pid_t pid = serve(v, rp[1]);
    close(rp[1]);
    char *ahost = strdup(host);
    int fd2 = -1;
    char err[4096];
    cap_begin();
    errno = 0;
    int s = rcmd_af(&ahost, htons(port), loc, rem, cmd, want_fd2 ? &fd2 : NULL, af);
    int e = errno;
    size_t en = cap_end(err, sizeof err);
    free_busy();
    printf("rcmd %s %d ", name, af); hexs(host); printf(" %d ", port); hexs(loc); putchar(' ');
    hexs(rem); putchar(' '); hexs(cmd); printf(" %d ", want_fd2); srv_line(v); putchar(' ');
    busy_line(busy);
    call_line("rcmd", s, e, ahost, err, en, s >= 0 && want_fd2 ? fd2 : -1, rp[0], pid);
}

static void rexec_case(const char *name, int af, const char *host, int port, const char *user,
                       const char *pass, const char *cmd, int want_fd2, const struct srv *v,
                       const struct file *netrc) {
    put_file("/tmp/rc/h/carol/.netrc", netrc);
    setenv("HOME", "/tmp/rc/h/carol", 1);
    int rp[2]; if (pipe(rp)) abort();
    pid_t pid = serve(v, rp[1]);
    close(rp[1]);
    char *ahost = strdup(host);
    int fd2 = -1;
    char err[4096];
    cap_begin();
    errno = 0;
    int s = rexec_af(&ahost, htons(port), user, pass, cmd, want_fd2 ? &fd2 : NULL, af);
    int e = errno;
    size_t en = cap_end(err, sizeof err);
    printf("rexec %s %d ", name, af); hexs(host); printf(" %d ", port); hexs(user); putchar(' ');
    hexs(pass); putchar(' '); hexs(cmd); printf(" %d ", want_fd2); srv_line(v); putchar(' ');
    spec(netrc);
    call_line("rexec", s, e, ahost, err, en, s >= 0 && want_fd2 ? fd2 : -1, rp[0], pid);
}

/* "rresvport", or "rresvport-unprivileged" for the call made outside the
   namespace, with no right to a reserved port. */
static const char *rresvport_kind = "rresvport";

static void rresvport_case(int af, int start, const int *busy) {
    hold_busy(busy, af);
    int port = start;
    errno = 0;
    int s = rresvport_af(&port, af);
    int e = errno;
    int bound = 0;
    if (s >= 0) {
        struct sockaddr_storage ss; socklen_t len = sizeof ss;
        getsockname(s, (struct sockaddr *) &ss, &len);
        bound = ntohs(ss.ss_family == AF_INET6 ? ((struct sockaddr_in6 *) &ss)->sin6_port
                                                : ((struct sockaddr_in *) &ss)->sin_port);
        close(s);
    }
    free_busy();
    printf("%s %d %d ", rresvport_kind, af, start); busy_line(busy);
    printf(" | %s %d %d %d\n", s >= 0 ? "fd" : "-1", s >= 0 ? 0 : e, port, bound);
}

static void lo_up(void) {
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct ifreq ifr;
    memset(&ifr, 0, sizeof ifr);
    strcpy(ifr.ifr_name, "lo");
    if (ioctl(s, SIOCGIFFLAGS, &ifr)) abort();
    ifr.ifr_flags |= IFF_UP | IFF_RUNNING;
    if (ioctl(s, SIOCSIFFLAGS, &ifr)) abort();
    close(s);
}

/* A protocol probe in a network namespace of its own, loopback up: a port
   one probe leaves in TIME_WAIT is not the next one's to walk past. */
#define ISOLATE(...) do {                                                    \
        fflush(stdout);                                                      \
        pid_t p_ = fork();                                                   \
        if (p_ == 0) {                                                       \
            if (unshare(CLONE_NEWNET)) abort();                              \
            lo_up();                                                         \
            __VA_ARGS__;                                                     \
            fflush(stdout);                                                  \
            _exit(0);                                                        \
        }                                                                    \
        int st_;                                                             \
        waitpid(p_, &st_, 0);                                                \
        if (!WIFEXITED(st_) || WEXITSTATUS(st_)) printf("crashed %#x\n", st_); \
    } while (0)

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    signal(SIGPIPE, SIG_IGN);
    if (argc > 1 && !strcmp(argv[1], "unprivileged")) {
        /* Outside the namespace: no right to a reserved port. */
        static const int none[] = {0};
        rresvport_kind = "rresvport-unprivileged";
        rresvport_case(AF_INET, 1023, none);
        return 0;
    }
    lo_up();
    if (sethostname("probe.example.com", 17)) abort();
    mkdir("/tmp/rc/h", 0755);
    mkdir("/tmp/rc/h/alice", 0755);
    mkdir("/tmp/rc/h/bob", 0755);
    mkdir("/tmp/rc/h/root", 0755);
    mkdir("/tmp/rc/h/carol", 0755);
    setenv("HOME", "/tmp/rc/h/carol", 1);
''' + "\n".join("    " + p for p in PROBES) + r'''
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "rc.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        for name, text in [("passwd", PASSWD), ("hosts", HOSTS), ("netgroup", NETGROUP),
                           ("nsswitch.conf", NSSWITCH), ("host.conf", HOST_CONF)]:
            (d / name).write_text(text, encoding="utf-8", newline="\n")
        w = wsl_path(d)
        # The build's own warnings (NSS in a static program) are not output;
        # a failure is, on stderr.
        script = (
            f"set -e; rm -rf /tmp/rc && mkdir -p /tmp/rc && cp {w}/rc.c {w}/passwd {w}/hosts "
            f"{w}/netgroup {w}/nsswitch.conf {w}/host.conf /tmp/rc/ && cd /tmp/rc && "
            "{ gcc -static -O0 -Wall -o rcprobe rc.c > gcc.log 2>&1 "
            "|| { cat gcc.log >&2; exit 1; }; }; "
            "unshare -rmnu sh -c 'mount -t tmpfs none /etc && "
            "cp /tmp/rc/passwd /tmp/rc/hosts /tmp/rc/netgroup /tmp/rc/nsswitch.conf "
            "/tmp/rc/host.conf /etc/ && timeout 900 /tmp/rc/rcprobe'; "
            "/tmp/rc/rcprobe unprivileged"
        )
        r = run(script)
        if r.returncode != 0 or "rresvport-unprivileged 2 1023 - | -1" not in r.stdout:
            sys.exit(f"the harness failed ({r.returncode}):\n{r.stderr}\n{r.stdout[-3000:]}")
        body = r.stdout
    head = ("# glibc 2.39's rcmd, rexec, rresvport, ruserok and ruserpass, for posix/src/rcmd.rs.\n"
            "# Generated by posix/tools/oracle/rcmd_harness.py; do not edit.\n")
    files = "".join(f"# {name}: {text.encode().hex()}\n"
                    for name, text in [("passwd", PASSWD), ("hosts", HOSTS), ("netgroup", NETGROUP),
                                       ("host.conf", HOST_CONF)])
    OUT.write_text(head + files + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
