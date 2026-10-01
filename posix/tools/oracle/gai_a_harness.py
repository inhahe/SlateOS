"""glibc 2.39's asynchronous lookups (<netdb.h>, GNU) -- getaddrinfo_a,
gai_suspend, gai_error and gai_cancel -- as the oracle for
posix/src/gai_a.rs.

    python posix/tools/oracle/gai_a_harness.py   # writes posix/src/gai_a_oracle.txt

The lookups are answered from a hosts file alone (`hosts: files` in
/etc/nsswitch.conf, /etc/hosts below, both on a tmpfs over /etc in a user
namespace of the probe's own), so that none waits on a network. What
depends on scheduling -- whether a request is still running when it is
asked about -- is only asked where the answer cannot depend on it: after
the batch is waited for, or of a request queued behind more than glibc's
threads can take at once (glibc 2.39 runs at most 20). One line a probe.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "gai_a_oracle.txt"

HOSTS = "127.0.0.1 localhost\n10.1.2.3 alpha alpha.example\n::1 localhost ip6-localhost\n"

PROGRAM = r'''
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static const char *eai(int e)
{
    switch (e) {
    case 0: return "0";
    case EAI_NONAME: return "EAI_NONAME";
    case EAI_AGAIN: return "EAI_AGAIN";
    case EAI_SYSTEM: return "EAI_SYSTEM";
    case EAI_SERVICE: return "EAI_SERVICE";
    case EAI_INPROGRESS: return "EAI_INPROGRESS";
    case EAI_CANCELED: return "EAI_CANCELED";
    case EAI_NOTCANCELED: return "EAI_NOTCANCELED";
    case EAI_ALLDONE: return "EAI_ALLDONE";
    case EAI_INTR: return "EAI_INTR";
    default: { static char b[16]; snprintf(b, sizeof b, "%d", e); return b; }
    }
}

/* The first answer's address and port, or "-". */
static void answer(const struct gaicb *g)
{
    const struct addrinfo *ai = g->ar_result;
    if (!ai) { printf("-"); return; }
    char buf[64] = "?";
    int port = 0;
    if (ai->ai_family == AF_INET) {
        const struct sockaddr_in *s = (const void *)ai->ai_addr;
        inet_ntop(AF_INET, &s->sin_addr, buf, sizeof buf);
        port = ntohs(s->sin_port);
    } else if (ai->ai_family == AF_INET6) {
        const struct sockaddr_in6 *s = (const void *)ai->ai_addr;
        inet_ntop(AF_INET6, &s->sin6_addr, buf, sizeof buf);
        port = ntohs(s->sin6_port);
    }
    printf("%s:%d", buf, port);
}

static struct gaicb *make(const char *name, const char *service, int family)
{
    struct gaicb *g = calloc(1, sizeof *g);
    struct addrinfo *h = calloc(1, sizeof *h);
    h->ai_family = family;
    h->ai_socktype = SOCK_STREAM;
    g->ar_name = name;
    g->ar_service = service;
    g->ar_request = h;
    return g;
}

static sem_t notified;
static int notified_value;
static void notify(union sigval v) { notified_value = v.sival_int; sem_post(&notified); }

int main(void)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("sizeof(struct gaicb) = %zu\n", sizeof(struct gaicb));
    printf("GAI_WAIT = %d GAI_NOWAIT = %d\n", GAI_WAIT, GAI_NOWAIT);

    /* A never-submitted control block: gai_error reads what it holds. */
    struct gaicb zero;
    memset(&zero, 0, sizeof zero);
    printf("gai_error(never submitted) = %s\n", eai(gai_error(&zero)));
    printf("gai_cancel(never submitted) = %s\n", eai(gai_cancel(&zero)));

    /* Bad arguments. */
    errno = 12345;
    int rc = getaddrinfo_a(2, NULL, 0, NULL);
    printf("getaddrinfo_a(mode 2) = %s errno=%s\n", eai(rc), errno == EINVAL ? "EINVAL" : errno == 12345 ? "kept" : "other");
    errno = 12345;
    rc = getaddrinfo_a(GAI_WAIT, NULL, 0, NULL);
    printf("getaddrinfo_a(GAI_WAIT, 0 entries) = %s errno=%s\n", eai(rc), errno == 12345 ? "kept" : "other");
    struct gaicb *nulls[3] = { NULL, NULL, NULL };
    rc = getaddrinfo_a(GAI_WAIT, nulls, 3, NULL);
    printf("getaddrinfo_a(GAI_WAIT, only NULLs) = %s\n", eai(rc));
    rc = gai_suspend((const struct gaicb *const *)nulls, 3, NULL);
    printf("gai_suspend(only NULLs) = %s\n", eai(rc));

    /* A waited batch: numbers, hosts-file names, a name with none, a bad
     * service; NULLs between. */
    struct gaicb *list[] = {
        make("127.0.0.1", "80", AF_INET),
        NULL,
        make("alpha", "22", AF_INET),
        make("localhost", NULL, AF_INET6),
        make("no.such.host.invalid", "80", AF_UNSPEC),
        make("10.9.8.7", "nosuchservice", AF_INET),
        make(NULL, "443", AF_INET),
    };
    int n = sizeof list / sizeof *list;
    errno = 12345;
    rc = getaddrinfo_a(GAI_WAIT, list, n, NULL);
    printf("getaddrinfo_a(GAI_WAIT, batch) = %s errno=%s\n", eai(rc), errno == 12345 ? "kept" : "other");
    for (int i = 0; i < n; i++) {
        if (!list[i]) continue;
        printf("batch[%d] %s:%s gai_error=%s answer=", i, list[i]->ar_name ? list[i]->ar_name : "NULL",
               list[i]->ar_service ? list[i]->ar_service : "NULL", eai(gai_error(list[i])));
        answer(list[i]);
        printf("\n");
    }
    printf("gai_suspend(finished batch) = %s\n", eai(gai_suspend((const struct gaicb *const *)list, n, NULL)));
    struct timespec zero_time = { 0, 0 };
    printf("gai_suspend(finished batch, 0 s) = %s\n", eai(gai_suspend((const struct gaicb *const *)list, n, &zero_time)));
    printf("gai_cancel(finished) = %s\n", eai(gai_cancel(list[0])));
    printf("gai_error(finished, after cancel) = %s\n", eai(gai_error(list[0])));

    /* No wait, and a thread told when the whole batch is done. */
    sem_init(&notified, 0, 0);
    struct gaicb *two[] = { make("alpha.example", "80", AF_INET), make("localhost", "80", AF_INET) };
    struct sigevent sev;
    memset(&sev, 0, sizeof sev);
    sev.sigev_notify = SIGEV_THREAD;
    sev.sigev_notify_function = notify;
    sev.sigev_value.sival_int = 4242;
    rc = getaddrinfo_a(GAI_NOWAIT, two, 2, &sev);
    printf("getaddrinfo_a(GAI_NOWAIT, SIGEV_THREAD) = %s\n", eai(rc));
    sem_wait(&notified);
    printf("notified with %d; gai_error = %s %s\n", notified_value, eai(gai_error(two[0])), eai(gai_error(two[1])));

    /* No wait, no notice: gai_suspend waits for them. */
    struct gaicb *one[] = { make("alpha", "80", AF_INET) };
    rc = getaddrinfo_a(GAI_NOWAIT, one, 1, NULL);
    printf("getaddrinfo_a(GAI_NOWAIT, no notice) = %s\n", eai(rc));
    const struct gaicb *wait1[] = { one[0] };
    int s;
    while ((s = gai_suspend(wait1, 1, NULL)) == 0 && gai_error(one[0]) == EAI_INPROGRESS)
        ;
    printf("then gai_error = %s answer=", eai(gai_error(one[0])));
    answer(one[0]);
    printf("\n");

    /* Many more than glibc's threads take at once: the last is still queued,
     * and is cancelled; then the rest are waited for. */
    enum { MANY = 200 };
    struct gaicb *many[MANY];
    for (int i = 0; i < MANY; i++)
        many[i] = make("localhost", "80", AF_INET);
    rc = getaddrinfo_a(GAI_NOWAIT, many, MANY, NULL);
    printf("getaddrinfo_a(GAI_NOWAIT, %d) = %s\n", MANY, eai(rc));
    rc = gai_cancel(many[MANY - 1]);
    printf("gai_cancel(last of %d) = %s\n", MANY, eai(rc));
    printf("gai_error(cancelled) = %s\n", eai(gai_error(many[MANY - 1])));
    printf("gai_cancel(cancelled again) = %s\n", eai(gai_cancel(many[MANY - 1])));
    for (int i = 0; i < MANY - 1; i++) {
        const struct gaicb *w[] = { many[i] };
        while (gai_error(many[i]) == EAI_INPROGRESS)
            gai_suspend(w, 1, NULL);
    }
    int ok = 0;
    for (int i = 0; i < MANY - 1; i++)
        ok += gai_error(many[i]) == 0;
    printf("the other %d: %d answered\n", MANY - 1, ok);
    printf("gai_error(cancelled, later) = %s\n", eai(gai_error(many[MANY - 1])));
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "ga.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        (d / "hosts").write_text(HOSTS, encoding="utf-8", newline="\n")
        w = wsl_path(d)
        script = (
            f"set -e; cp {w}/ga.c {w}/hosts /tmp/ && cd /tmp && "
            "{ gcc -static -O0 -Wall -Werror -o gaprobe ga.c -lpthread > gcc.log 2>&1 "
            "|| { cat gcc.log >&2; exit 1; }; }; "
            "printf 'hosts: files\\nservices: files\\n' > /tmp/nsswitch.conf; "
            "cp /etc/services /tmp/services; "
            "unshare -rm sh -c 'mount -t tmpfs none /etc && cp /tmp/nsswitch.conf /tmp/hosts "
            "/tmp/services /etc/ && timeout 120 /tmp/gaprobe'"
        )
        r = run(script)
        if r.returncode != 0 or "answered" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's getaddrinfo_a, gai_suspend, gai_error and gai_cancel, for\n"
            "# posix/src/gai_a.rs. Generated by posix/tools/oracle/gai_a_harness.py; do not edit.\n")
    OUT.write_text(head + f"input hosts = {HOSTS!r}\n" + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
