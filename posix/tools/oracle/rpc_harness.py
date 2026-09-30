"""glibc 2.39's <rpc/netdb.h> -- getrpcent, getrpcbyname, getrpcbynumber,
their _r forms, setrpcent and endrpcent over /etc/rpc -- as the oracle for
posix/src/netdb.rs's RPC program database.

    python posix/tools/oracle/rpc_harness.py   # writes posix/src/rpc_oracle.txt

/etc/rpc is a fixed path, so the harness runs, statically linked, in a user
and mount namespace of its own (`unshare -rm`) with a tmpfs over /etc, three
times:

- `files`: /etc/rpc is the test file below -- comments, blank lines, the
  lines glibc's parser refuses, one with no newline;
- `builtin`: /etc/rpc is netdb.rs's built-in copy (its `RPC`), which that
  library answers from when there is no file: the tests replay this run with
  none, so glibc vouches for the copy's every answer;
- `none`: there is no /etc/rpc. glibc's lookups answer the open's ENOENT and
  its enumerations end at once; the tests replay it as a file that exists
  and cannot be read -- the open failing with ENOENT -- which glibc treats
  the same way.

One line a probe, `<run> <probe> = <what it gave>`: an entry as
`name|number|[alias,...]`; an _r form's number and entry; `NULL` and errno's
name (`kept`: as the probe left it). The inputs head the file, escaped:
`input rpc = ...`, `input builtin = ...`.
"""

import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "rpc_oracle.txt"

RPC = (
    "# /etc/rpc\n"
    "portmapper\t100000\tportmap sunrpc rpcbind\n"
    "rstatd\t\t100001\trstat rup perfmeter rstat_svc\n"
    "nfs\t\t100003\tnfsprog\n"
    "\n"
    "   ypserv 100004 ypprog # a comment\n"
    "mountd\t\t100005\tmount showmount\n"
    "bad100006\n"
    "nonum abc\n"
    "hex 0x1f\n"
    "neg -5 negative\n"
    "trailing 100007x\n"
    "big 4294967297 wrapped\n"
    "dup 100000 again\n"
    "nlockmgr 100021#tight\n"
    "tabs\t100024\t\tstatus\t\n"
    "last 100099"
)

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <netdb.h>
#include <rpc/netdb.h>
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

static void entry(const struct rpcent *r)
{
    printf("%s|%d|[", r->r_name, r->r_number);
    for (int i = 0; r->r_aliases[i]; i++) printf("%s%s", i ? "," : "", r->r_aliases[i]);
    printf("]");
}

static void show(const char *run, const char *what, struct rpcent *r)
{
    printf("%s %s = ", run, what);
    if (r) entry(r); else printf("NULL");
    printf(" errno=%s\n", en(errno));
}

int main(int argc, char **argv)
{
    const char *run = argc > 1 ? argv[1] : "files";
    char what[128], buf[1024];
    setvbuf(stdout, NULL, _IONBF, 0);
    for (int n = 0; n < 64; n++) {
        snprintf(what, sizeof what, "getrpcent #%d", n);
        errno = 12345;
        struct rpcent *r = getrpcent();
        show(run, what, r);
        if (!r) break;
    }
    setrpcent(0);
    errno = 12345;
    show(run, "getrpcent after setrpcent(0)", getrpcent());
    setrpcent(1);
    errno = 12345;
    show(run, "getrpcent after setrpcent(1)", getrpcent());
    errno = 12345;
    show(run, "getrpcent after that", getrpcent());
    endrpcent();
    errno = 12345;
    show(run, "getrpcent after endrpcent", getrpcent());
    endrpcent();
    static const char *NAMES[] = { "portmapper", "sunrpc", "rpcbind", "nfsprog", "ypserv",
        "ypprog", "mountd", "showmount", "bad100006", "nonum", "hex", "neg", "trailing",
        "big", "wrapped", "dup", "again", "nlockmgr", "tabs", "status", "last", "comment",
        "a", "missing", "", "rquota", "keyserver", "x25.inr", "sgi_fam", "amq", "NFS" };
    for (size_t i = 0; i < sizeof NAMES / sizeof *NAMES; i++) {
        snprintf(what, sizeof what, "getrpcbyname(%s)", NAMES[i]);
        errno = 12345;
        show(run, what, getrpcbyname(NAMES[i]));
    }
    static const int NUMBERS[] = { 100000, 100001, 100003, 100004, 100005, 100006, 100007, 0,
        31, -5, 1, 100021, 100024, 100099, 42, 100227, 150001, 391002, 545580417 };
    for (size_t i = 0; i < sizeof NUMBERS / sizeof *NUMBERS; i++) {
        snprintf(what, sizeof what, "getrpcbynumber(%d)", NUMBERS[i]);
        errno = 12345;
        show(run, what, getrpcbynumber(NUMBERS[i]));
    }
    /* the _r forms, at buffer sizes around an entry's need */
    for (size_t size = 0; size <= 96; size += 8) {
        struct rpcent r, *res = (struct rpcent *)1;
        errno = 12345;
        int rc = getrpcbyname_r("portmapper", &r, buf, size, &res);
        printf("%s getrpcbyname_r(portmapper, %zu) = %s ", run, size, en(rc));
        if (rc == 0 && res == &r) entry(&r);
        else printf("%s", res == NULL ? "NULL" : res == (struct rpcent *)1 ? "untouched" : "other");
        printf(" errno=%s\n", en(errno));
        res = (struct rpcent *)1;
        errno = 12345;
        rc = getrpcbynumber_r(100001, &r, buf, size, &res);
        printf("%s getrpcbynumber_r(100001, %zu) = %s ", run, size, en(rc));
        if (rc == 0 && res == &r) entry(&r);
        else printf("%s", res == NULL ? "NULL" : res == (struct rpcent *)1 ? "untouched" : "other");
        printf(" errno=%s\n", en(errno));
    }
    {
        struct rpcent r, *res = (struct rpcent *)1;
        errno = 12345;
        int rc = getrpcbyname_r("missing", &r, buf, sizeof buf, &res);
        printf("%s getrpcbyname_r(missing) = %s %s errno=%s\n", run, en(rc),
               res == NULL ? "NULL" : "other", en(errno));
    }
    setrpcent(0);
    for (int n = 0; n < 64; n++) {
        struct rpcent r, *res = (struct rpcent *)1;
        errno = 12345;
        int rc = getrpcent_r(&r, buf, n == 1 ? 8 : sizeof buf, &res);
        printf("%s getrpcent_r #%d = %s ", run, n, en(rc));
        if (rc == 0 && res == &r) entry(&r);
        else printf("%s", res == NULL ? "NULL" : "other");
        printf(" errno=%s\n", en(errno));
        if (rc != 0 && rc != ERANGE) break;
    }
    endrpcent();
    return 0;
}
'''


def builtin() -> str:
    """netdb.rs's `RPC`: the byte string's lines, which hold no escapes."""
    src = (POSIX_SRC / "netdb.rs").read_text(encoding="utf-8")
    m = re.search(r'pub\(crate\) const RPC: &\[u8\] = b"\\\n(.*?)";\n', src, re.S)
    if not m:
        sys.exit("netdb.rs has no `pub(crate) const RPC: &[u8] = b\"\\` ... `\";`")
    text = m.group(1)
    if "\\" in text or '"' in text:
        sys.exit("netdb.rs's RPC has an escape or a quote: this reader takes it as it is")
    return text


def escape(text: str) -> str:
    """`text` with \\, newline and tab escaped, for the tests to read back."""
    return text.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")


def main() -> None:
    table = builtin()
    with workdir() as t:
        d = Path(t)
        (d / "rp.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        (d / "rpc").write_text(RPC, encoding="utf-8", newline="\n")
        (d / "rpc.builtin").write_text(table, encoding="utf-8", newline="\n")
        w = wsl_path(d)

        def namespace(label: str, file: str) -> str:
            """The probe run as `label`, /etc/rpc being `file` (none: "")."""
            copy = f"cp /tmp/{file} /etc/rpc && " if file else ""
            return (f"unshare -rm sh -c 'mount -t tmpfs none /etc && "
                    f"cp /tmp/nsswitch.conf /etc/ && {copy}/tmp/rpcprobe {label}'; ")

        # The build's own warning (NSS in a static program) is not output; a
        # failure is, on stderr. /etc/nsswitch.conf says `rpc: files`, as a
        # system's does, so that only /etc/rpc differs between the runs.
        script = (
            f"set -e; cp {w}/rp.c {w}/rpc {w}/rpc.builtin /tmp/ && cd /tmp && "
            "{ gcc -static -O0 -Wall -Werror -o rpcprobe rp.c > gcc.log 2>&1 "
            "|| { cat gcc.log >&2; exit 1; }; }; "
            "printf 'rpc: files\\n' > /tmp/nsswitch.conf; "
            + namespace("files", "rpc")
            + namespace("builtin", "rpc.builtin")
            + namespace("none", "")
        )
        r = run(script)
        if r.returncode != 0 or "none getrpcent" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's <rpc/netdb.h>, for posix/src/netdb.rs. Generated by\n"
            "# posix/tools/oracle/rpc_harness.py; do not edit.\n")
    inputs = f"input rpc = {escape(RPC)}\ninput builtin = {escape(table)}\n"
    OUT.write_text(head + inputs + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
