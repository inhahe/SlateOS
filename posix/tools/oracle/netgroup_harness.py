"""glibc 2.39's netgroups (<netdb.h>) -- setnetgrent, getnetgrent,
getnetgrent_r, endnetgrent and innetgr over /etc/netgroup -- as the oracle
for posix/src/netgroup.rs.

    python posix/tools/oracle/netgroup_harness.py   # writes posix/src/netgroup_oracle.txt

/etc/netgroup is a fixed path, so the harness runs, statically linked, in a
user and mount namespace of its own (`unshare -rm`) with a tmpfs over /etc,
twice: over the test file below, and with no file -- each under a timeout,
in case a group glibc cannot finish expanding makes it loop. One line a
probe, `<run> <probe> = <what it gave>`: a triple as `(host,user,domain)`,
each field quoted, or `NULL`; a function's number; errno's name (`kept`: as
the probe left it). The input heads the file, escaped: `input netgroup = ...`.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "netgroup_oracle.txt"

NETGROUP = (
    "# /etc/netgroup\n"
    "trusted (alpha,root,example.com) (beta,,) (,guest,)\n"
    "servers (web1,-,prod) (web2,-,prod) \\\n"
    "\t(db1,-,prod)\n"
    "all trusted servers (gamma,admin,)\n"
    "loopA loopB (a1,,)\n"
    "loopB loopA (b1,,)\n"
    "self self (s1,,)\n"
    "spaced ( host , user , dom )\n"
    "empty\n"
    "dupes (x,y,z) (x,y,z)\n"
    "nested all (delta,,)\n"
    "missingref nosuchgroup (m,,)\n"
    "Upper (UP,Us,Dom)\n"
    "tabs\t(t1,u1,d1)\t(t2,,)\n"
    "commented (c1,,) # (c2,,)\n"
    "   indented (i1,,)\n"
    "trusted (dup-line,,)\n"
    "bad1 (unclosed,a,b\n"
    "bad2 (a,b,c,d)\n"
    "bad3 (a,b)\n"
    "bad4 (a,b,c)(d,e,f)\n"
    "cont (k1,,) \\\n"
    "\n"
    "after-empty-cont (k2,,)\n"
    "last (z1,,)"
)

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <netdb.h>
#include <stdio.h>
#include <string.h>

static const char *en(int e)
{
    switch (e) {
    case 0: return "0";
    case 12345: return "kept";
    case ENOENT: return "ENOENT";
    case ERANGE: return "ERANGE";
    case EINVAL: return "EINVAL";
    default: { static char b[16]; snprintf(b, sizeof b, "e%d", e); return b; }
    }
}

static void field(const char *f)
{
    if (f) printf("\"%s\"", f); else printf("NULL");
}

static void triple(const char *h, const char *u, const char *d)
{
    printf("(");
    field(h); printf(","); field(u); printf(","); field(d);
    printf(")");
}

static void walk(const char *run, const char *group)
{
    errno = 12345;
    int rc = setnetgrent(group);
    printf("%s setnetgrent(%s) = %d errno=%s\n", run, group, rc, en(errno));
    for (int n = 0; n < 64; n++) {
        char *h = (char *)1, *u = (char *)1, *d = (char *)1;
        errno = 12345;
        rc = getnetgrent(&h, &u, &d);
        printf("%s getnetgrent(%s) #%d = %d ", run, group, n, rc);
        if (rc == 1) triple(h, u, d);
        printf(" errno=%s\n", en(errno));
        if (rc != 1) break;
    }
    endnetgrent();
}

static void in(const char *run, const char *g, const char *h, const char *u, const char *d)
{
    errno = 12345;
    int rc = innetgr(g, h, u, d);
    printf("%s innetgr(%s,", run, g ? g : "NULL");
    field(h); printf(","); field(u); printf(","); field(d);
    printf(") = %d errno=%s\n", rc, en(errno));
}

int main(int argc, char **argv)
{
    const char *run = argc > 1 ? argv[1] : "files";
    setvbuf(stdout, NULL, _IONBF, 0);
    {
        char *h = (char *)1, *u = (char *)1, *d = (char *)1;
        errno = 12345;
        int rc = getnetgrent(&h, &u, &d);
        printf("%s getnetgrent before setnetgrent = %d errno=%s\n", run, rc, en(errno));
    }
    static const char *GROUPS[] = { "trusted", "servers", "all", "loopA", "self", "spaced",
        "empty", "dupes", "nested", "missingref", "Upper", "upper", "tabs", "commented",
        "indented", "bad1", "bad2", "bad3", "bad4", "cont", "after-empty-cont", "last",
        "nosuchgroup", "", "#" };
    for (size_t i = 0; i < sizeof GROUPS / sizeof *GROUPS; i++)
        walk(run, GROUPS[i]);
    /* innetgr: fields that match, that do not, NULL, empty, "-", and case */
    in(run, "trusted", "alpha", "root", "example.com");
    in(run, "trusted", "alpha", NULL, NULL);
    in(run, "trusted", "ALPHA", "root", "EXAMPLE.COM");
    in(run, "trusted", "alpha", "ROOT", NULL);
    in(run, "trusted", "alpha", "other", NULL);
    in(run, "trusted", "beta", "anyone", "anywhere");
    in(run, "trusted", "anyhost", "guest", NULL);
    in(run, "trusted", "gamma", NULL, NULL);
    in(run, "trusted", NULL, NULL, NULL);
    in(run, "trusted", "", NULL, NULL);
    in(run, "trusted", "dup-line", NULL, NULL);
    in(run, "servers", "web1", NULL, "prod");
    in(run, "servers", "web1", "someone", "prod");
    in(run, "servers", "web1", "-", "prod");
    in(run, "servers", "db1", NULL, NULL);
    in(run, "all", "alpha", "root", NULL);
    in(run, "all", "web2", NULL, "prod");
    in(run, "all", "gamma", "admin", "anything");
    in(run, "nested", "db1", NULL, NULL);
    in(run, "nested", "delta", NULL, NULL);
    in(run, "loopA", "b1", NULL, NULL);
    in(run, "loopA", "zz", NULL, NULL);
    in(run, "self", "s1", NULL, NULL);
    in(run, "missingref", "m", NULL, NULL);
    in(run, "spaced", "host", "user", "dom");
    in(run, "spaced", " host ", " user ", " dom ");
    in(run, "Upper", "up", "Us", "dom");
    in(run, "Upper", "UP", "us", "Dom");
    in(run, "upper", "UP", "Us", "Dom");
    in(run, "empty", NULL, NULL, NULL);
    in(run, "nosuchgroup", NULL, NULL, NULL);
    in(run, "bad4", "a", "b", "c");
    in(run, "bad4", "d", "e", "f");
    in(run, "cont", "k1", NULL, NULL);
    in(run, "after-empty-cont", "k2", NULL, NULL);
    in(run, "last", "z1", NULL, NULL);
    /* getnetgrent_r at buffer sizes around a triple's need */
    for (size_t size = 0; size <= 48; size += 4) {
        char buf[64], *h = (char *)1, *u = (char *)1, *d = (char *)1;
        setnetgrent("trusted");
        errno = 12345;
        int rc = getnetgrent_r(&h, &u, &d, buf, size);
        printf("%s getnetgrent_r(trusted, %zu) = %d ", run, size, rc);
        if (rc == 1) triple(h, u, d);
        printf(" errno=%s", en(errno));
        errno = 12345;
        rc = getnetgrent_r(&h, &u, &d, buf, sizeof buf);
        printf(" then %d ", rc);
        if (rc == 1) triple(h, u, d);
        printf(" errno=%s\n", en(errno));
        endnetgrent();
    }
    /* setnetgrent twice, and enumeration after endnetgrent */
    setnetgrent("trusted");
    setnetgrent("servers");
    {
        char *h, *u, *d;
        int rc = getnetgrent(&h, &u, &d);
        printf("%s after setnetgrent(trusted), setnetgrent(servers) = %d ", run, rc);
        if (rc == 1) triple(h, u, d);
        printf("\n");
        endnetgrent();
        errno = 12345;
        rc = getnetgrent(&h, &u, &d);
        printf("%s getnetgrent after endnetgrent = %d errno=%s\n", run, rc, en(errno));
    }
    return 0;
}
'''


def escape(text: str) -> str:
    """`text` with \\, newline, tab and carriage return escaped, for the tests
    to read back. Any other control character is refused rather than written
    raw: the tests have no escape for it, and a raw one is a byte of the
    oracle file that an editor or a line-ending conversion is free to change."""
    out = (text.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")
           .replace("\r", "\\r"))
    raw = sorted({f"{ord(c):#04x}" for c in out if ord(c) < 0x20 or ord(c) == 0x7F})
    if raw:
        sys.exit(f"escape: no escape for {', '.join(raw)}")
    return out


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "ng.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        (d / "netgroup").write_text(NETGROUP, encoding="utf-8", newline="\n")
        w = wsl_path(d)

        def namespace(label: str, copy: str) -> str:
            """The probe run as `label`, after `copy` fills /etc."""
            return (f"unshare -rm sh -c 'mount -t tmpfs none /etc && "
                    f"cp /tmp/nsswitch.conf /etc/ && {copy}timeout 60 /tmp/ngprobe {label}'"
                    f" || echo '{label} = loops or fails ('$?')'; ")

        # The build's own warning (NSS in a static program) is not output; a
        # failure is, on stderr. /etc/nsswitch.conf says `netgroup: files`,
        # as a system's does, so that only /etc/netgroup differs between runs.
        script = (
            f"set -e; cp {w}/ng.c {w}/netgroup /tmp/ && cd /tmp && "
            "{ gcc -static -O0 -Wall -Werror -o ngprobe ng.c > gcc.log 2>&1 "
            "|| { cat gcc.log >&2; exit 1; }; }; "
            "printf 'netgroup: files\\n' > /tmp/nsswitch.conf; "
            + namespace("files", "cp /tmp/netgroup /etc/ && ")
            + namespace("none", "")
        )
        r = run(script)
        if r.returncode != 0 or "none getnetgrent" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's netgroups, for posix/src/netgroup.rs. Generated by\n"
            "# posix/tools/oracle/netgroup_harness.py; do not edit.\n")
    OUT.write_text(head + f"input netgroup = {escape(NETGROUP)}\n" + body, encoding="utf-8",
                   newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
