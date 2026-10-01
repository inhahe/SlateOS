"""glibc 2.39's reentrant resolver (`<resolv.h>`: `res_ninit`, `res_nclose`,
`res_nquery`, `res_nsearch`, `res_nquerydomain`, `res_nmkquery`,
`res_nsend`), as the oracle for posix/src/resolv.rs's:

    python posix/tools/oracle/resolvn_harness.py   # writes posix/src/resolvn_oracle.txt

The sandbox is `unshare -r -n -m`, as gai_harness.py's: a user, network and
mount namespace with loopback up and RESOLV_CONF below over /etc/resolv.conf.
The program forks a DNS responder onto 127.0.0.1:53/udp first:
`a.example.test` has the A record 10.0.0.1, `srv.example.test` answers
SERVFAIL, every other name NXDOMAIN.

One line a case, `<case> = <answer>`:

    I zeroed = <rc> <retrans> <retry> <options> <nscount> <ns0> <ndots> <search...> ;
          res_ninit on a zeroed state -- the state it asks for; one of
          garbage is undefined, and glibc's follows its pointers and
          crashes -- and the fields it sets. `options` in hex, `ns0` as
          address:port.
    Q/S/D <names> = <rc> <h_errno> <res_h_errno> <rcode> <ancount> <A>
          res_nquery, res_nsearch, res_nquerydomain on an initialised
          state; `<A>` the first A record's address, or `-`.
    M <name> = <rc> <bytes>      res_nmkquery; the id's two bytes zeroed
    N = <rc> <rcode> <ancount>   res_nsend of that query
    U <call> = <rc> <h_errno> <res_h_errno>   on a zeroed state res_ninit has
                                 not seen: used as it is, with no nameserver
    C init-bit = <0 or 1>        whether RES_INIT is still set after
                                 res_nclose. Using the state after that
                                 without res_ninit again is undefined -- a
                                 query crashes glibc's -- and is not asked.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "resolvn_oracle.txt"

RESOLV_CONF = (b"nameserver 127.0.0.1\n"
               b"search example.test other.test\n"
               b"options ndots:2 timeout:1 attempts:1\n")

PROGRAM = r"""
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <arpa/nameser.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <resolv.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

/* The responder: one datagram in, one out, for as long as it lives. */
static void responder(void)
{
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in a = { .sin_family = AF_INET, .sin_port = htons(53) };
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (s < 0 || bind(s, (struct sockaddr *)&a, sizeof a) != 0)
        _exit(1);
    for (;;) {
        unsigned char q[512], r[512];
        struct sockaddr_in from;
        socklen_t fl = sizeof from;
        ssize_t n = recvfrom(s, q, sizeof q, 0, (struct sockaddr *)&from, &fl);
        if (n < 12)
            continue;
        char name[256];
        int at = 12, k = 0;
        while (at < n && q[at] != 0 && k < 250) {
            int l = q[at++];
            if (k)
                name[k++] = '.';
            for (int j = 0; j < l && at < n; j++)
                name[k++] = (char)q[at++];
        }
        name[k] = 0;
        int qend = at + 1 + 4;
        if (qend > n)
            continue;
        memcpy(r, q, (size_t)qend);
        r[2] = (unsigned char)(0x80 | (q[2] & 0x01));
        r[3] = 0x80;
        r[4] = 0; r[5] = 1;
        r[6] = 0; r[7] = 0; r[8] = 0; r[9] = 0; r[10] = 0; r[11] = 0;
        int len = qend;
        int qtype = (q[at + 1] << 8) | q[at + 2];
        if (strcasecmp(name, "a.example.test") == 0 && qtype == 1) {
            static const unsigned char ans[] = { 0xc0, 12, 0, 1, 0, 1, 0, 0, 1, 0x2c, 0, 4, 10, 0, 0, 1 };
            memcpy(r + len, ans, sizeof ans);
            len += (int)sizeof ans;
            r[7] = 1;
        } else if (strcasecmp(name, "srv.example.test") == 0) {
            r[3] |= 2;
        } else {
            r[3] |= 3;
        }
        sendto(s, r, (size_t)len, 0, (struct sockaddr *)&from, fl);
    }
}

static void fields(const char *how, int rc, const struct __res_state *st)
{
    char ns[64] = "-";
    if (st->nscount > 0)
        snprintf(ns, sizeof ns, "%s:%u", inet_ntoa(st->nsaddr_list[0].sin_addr),
                 ntohs(st->nsaddr_list[0].sin_port));
    printf("I %s = %d %d %d %lx %d %s %u", how, rc, st->retrans, st->retry,
           (unsigned long)st->options, st->nscount, ns, (unsigned)st->ndots);
    for (int i = 0; i < MAXDNSRCH && st->dnsrch[i]; i++)
        printf(" %s", st->dnsrch[i]);
    printf(" ;\n");
}

static void answer(const unsigned char *a, int rc, char *out, size_t n)
{
    if (rc < 12) { snprintf(out, n, "- - -"); return; }
    int rcode = a[3] & 0x0f, an = (a[6] << 8) | a[7];
    char addr[32] = "-";
    ns_msg m;
    if (an > 0 && ns_initparse(a, rc, &m) == 0) {
        ns_rr rr;
        for (int i = 0; i < an; i++)
            if (ns_parserr(&m, ns_s_an, i, &rr) == 0 && ns_rr_type(rr) == ns_t_a && ns_rr_rdlen(rr) == 4) {
                inet_ntop(AF_INET, ns_rr_rdata(rr), addr, sizeof addr);
                break;
            }
    }
    snprintf(out, n, "%d %d %s", rcode, an, addr);
}

#define ASK(kind, what, call)                                                          \
    do {                                                                               \
        unsigned char buf[1024];                                                       \
        h_errno = 12345; st.res_h_errno = 12345;                                        \
        int rc_ = (call);                                                              \
        char sum[64]; answer(buf, rc_, sum, sizeof sum);                               \
        printf("%s %s = %d %d %d %s\n", kind, what, rc_ < 0 ? -1 : 1, h_errno, st.res_h_errno, rc_ < 0 ? "- - -" : sum); \
    } while (0)

int main(void)
{
    /* Unbuffered: a line is out before the next call can take the process. */
    setvbuf(stdout, NULL, _IONBF, 0);
    pid_t r = fork();
    if (r == 0)
        responder();
    usleep(200000);

    struct __res_state st;
    memset(&st, 0, sizeof st);
    int rc = res_ninit(&st);
    fields("zeroed", rc, &st);
    rc = res_ninit(&st);
    printf("R again = %d\n", rc);

    ASK("Q", "a.example.test", res_nquery(&st, "a.example.test", C_IN, T_A, buf, sizeof buf));
    ASK("Q", "nx.example.test", res_nquery(&st, "nx.example.test", C_IN, T_A, buf, sizeof buf));
    ASK("Q", "srv.example.test", res_nquery(&st, "srv.example.test", C_IN, T_A, buf, sizeof buf));
    ASK("Q", "a.example.test/AAAA", res_nquery(&st, "a.example.test", C_IN, T_AAAA, buf, sizeof buf));
    ASK("S", "a", res_nsearch(&st, "a", C_IN, T_A, buf, sizeof buf));
    ASK("S", "a.example.test.", res_nsearch(&st, "a.example.test.", C_IN, T_A, buf, sizeof buf));
    ASK("S", "nothere", res_nsearch(&st, "nothere", C_IN, T_A, buf, sizeof buf));
    ASK("D", "a example.test", res_nquerydomain(&st, "a", "example.test", C_IN, T_A, buf, sizeof buf));
    ASK("D", "a other.test", res_nquerydomain(&st, "a", "other.test", C_IN, T_A, buf, sizeof buf));

    unsigned char qb[512], ab[1024];
    rc = res_nmkquery(&st, QUERY, "a.example.test", C_IN, T_A, NULL, 0, NULL, qb, sizeof qb);
    printf("M a.example.test = %d", rc);
    putchar(' ');
    for (int i = 0; i < rc; i++)
        printf("%02x", i < 2 ? 0 : qb[i]);
    putchar('\n');
    int n = res_nsend(&st, qb, rc, ab, sizeof ab);
    char sum[64];
    answer(ab, n, sum, sizeof sum);
    printf("N = %d %s\n", n < 0 ? -1 : 1, n < 0 ? "- - -" : sum);

    struct __res_state z;
    memset(&z, 0, sizeof z);
    {
        unsigned char buf[1024];
        h_errno = 12345; z.res_h_errno = 12345;
        rc = res_nquery(&z, "a.example.test", C_IN, T_A, buf, sizeof buf);
        printf("U query = %d %d %d\n", rc, h_errno, z.res_h_errno);
        h_errno = 12345; z.res_h_errno = 12345;
        rc = res_nsearch(&z, "a", C_IN, T_A, buf, sizeof buf);
        printf("U search = %d %d %d\n", rc, h_errno, z.res_h_errno);
        h_errno = 12345; z.res_h_errno = 12345;
        rc = res_nquerydomain(&z, "a", "example.test", C_IN, T_A, buf, sizeof buf);
        printf("U querydomain = %d %d %d\n", rc, h_errno, z.res_h_errno);
        h_errno = 12345; z.res_h_errno = 12345;
        rc = res_nmkquery(&z, QUERY, "a.example.test", C_IN, T_A, NULL, 0, NULL, buf, sizeof buf);
        printf("U mkquery = %d %d %d\n", rc, h_errno, z.res_h_errno);
        h_errno = 12345; z.res_h_errno = 12345;
        rc = res_nsend(&z, qb, 32, buf, sizeof buf);
        printf("U send = %d %d %d\n", rc, h_errno, z.res_h_errno);
    }

    /* A state used after res_nclose, without res_ninit again, is undefined:
     * glibc's res_nquery then crashes. Only what the close leaves is asked. */
    res_nclose(&st);
    printf("C init-bit = %d\n", (st.options & RES_INIT) ? 1 : 0);

    kill(r, SIGKILL);
    waitpid(r, NULL, 0);
    return 0;
}
"""


def main() -> int:
    with workdir() as d:
        work = Path(d)
        (work / "resolv.conf").write_bytes(RESOLV_CONF)
        c = work / "resolvn.c"
        c.write_text(PROGRAM, encoding="utf-8", newline="\n")
        w = wsl_path(work)
        script = (
            f"cd {w} && gcc -O1 -Wall -Werror -o resolvn resolvn.c -lresolv && "
            "unshare -r -n -m sh -c '"
            "ip link set lo up && "
            "e=/tmp/etcr.$$; mkdir -p $e && mount -t tmpfs none $e && "
            "(cp -a /etc/. $e/ 2>/dev/null; true) && rm -f $e/resolv.conf && "
            f"cp {w}/resolv.conf $e/resolv.conf && mount --bind $e /etc && "
            "./resolvn'"
        )
        r = run(script)
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's reentrant resolver in a sandbox, a responder on 127.0.0.1:53\n"
              "# (posix/tools/oracle/resolvn_harness.py). resolv.conf:\n"
              + "".join(f"#   {ln}\n" for ln in RESOLV_CONF.decode().splitlines()))
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
