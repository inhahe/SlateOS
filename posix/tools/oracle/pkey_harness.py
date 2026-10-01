"""glibc 2.39's memory protection keys on Linux, on a processor without
them, as the oracle for posix/src/mman.rs's `pkey_*`.

    python posix/tools/oracle/pkey_harness.py   # writes posix/src/pkey_oracle.txt

SlateOS has no protection keys, so the answers that matter are Linux's where
the processor has none -- which is what this machine is (its cpuinfo has no
`pku`); the harness refuses to run on one that has them.

One line a call, `<call> = <return> <errno>`; `p` is two pages mapped, the
second then unmapped, so that `p, 2*page` spans a hole. `pkey_get` and
`pkey_set` with a key in range run in a child each, since glibc's execute
the `rdpkru`/`wrpkru` instructions, which such a processor does not have:
those lines say what ended the child.

Linux 6.6 answers the *first* `pkey_alloc` of a process `EINVAL` whatever
it asks, by glibc or by the raw system call, and every later one `ENOSPC`
(measured). pkey_alloc(2) documents `ENOSPC` for a processor without
protection keys, and that is the steady answer, so the harness makes one
call first and records from the second.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "pkey_oracle.txt"

PROGRAM = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *name(int e)
{
    switch (e) {
    case EINVAL: return "EINVAL";
    case ENOSPC: return "ENOSPC";
    case ENOMEM: return "ENOMEM";
    case ENOSYS: return "ENOSYS";
    default: return "other";
    }
}

#define SHOW(what, call)                                                  \
    do {                                                                  \
        errno = 0;                                                        \
        long r_ = (long)(call);                                           \
        printf("%s = %ld %s\n", what, r_, r_ < 0 ? name(errno) : "0");  \
    } while (0)

/* pkey_get (set 0) or pkey_set (set 1) in a child: its answer, or what
 * ended it. */
static void in_child(const char *what, int set, int key, unsigned rights)
{
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        errno = 0;
        int r = set ? pkey_set(key, rights) : pkey_get(key);
        printf("%s = %d %s\n", what, r, r < 0 ? name(errno) : "0");
        fflush(stdout);
        _exit(0);
    }
    int st = 0;
    waitpid(c, &st, 0);
    if (WIFSIGNALED(st))
        printf("%s = killed by %s\n", what, WTERMSIG(st) == SIGILL ? "SIGILL" : "another signal");
}

int main(void)
{
    long page = sysconf(_SC_PAGESIZE);
    char *p = mmap(NULL, 2 * page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED || munmap(p + page, page) != 0)
        return 1;
    (void)pkey_alloc(0, 0); /* the first call: see the harness's docstring */

    SHOW("pkey_alloc(0, 0)", pkey_alloc(0, 0));
    SHOW("pkey_alloc(0, PKEY_DISABLE_ACCESS)", pkey_alloc(0, PKEY_DISABLE_ACCESS));
    SHOW("pkey_alloc(0, PKEY_DISABLE_WRITE)", pkey_alloc(0, PKEY_DISABLE_WRITE));
    SHOW("pkey_alloc(0, 3)", pkey_alloc(0, 3));
    SHOW("pkey_alloc(0, 4)", pkey_alloc(0, 4));
    SHOW("pkey_alloc(0, 8)", pkey_alloc(0, 8));
    SHOW("pkey_alloc(1, 0)", pkey_alloc(1, 0));
    SHOW("pkey_alloc(0x80000000, 0)", pkey_alloc(0x80000000u, 0));

    SHOW("pkey_free(0)", pkey_free(0));
    SHOW("pkey_free(1)", pkey_free(1));
    SHOW("pkey_free(15)", pkey_free(15));
    SHOW("pkey_free(16)", pkey_free(16));
    SHOW("pkey_free(-1)", pkey_free(-1));

    SHOW("pkey_mprotect(p, page, RW, -1)", pkey_mprotect(p, page, PROT_READ | PROT_WRITE, -1));
    SHOW("pkey_mprotect(p, page, RW, 0)", pkey_mprotect(p, page, PROT_READ | PROT_WRITE, 0));
    SHOW("pkey_mprotect(p, page, RW, 1)", pkey_mprotect(p, page, PROT_READ | PROT_WRITE, 1));
    SHOW("pkey_mprotect(p, page, RW, 15)", pkey_mprotect(p, page, PROT_READ | PROT_WRITE, 15));
    SHOW("pkey_mprotect(p, page, RW, 16)", pkey_mprotect(p, page, PROT_READ | PROT_WRITE, 16));
    SHOW("pkey_mprotect(p, page, RW, -2)", pkey_mprotect(p, page, PROT_READ | PROT_WRITE, -2));
    SHOW("pkey_mprotect(p + 1, page, RW, -1)", pkey_mprotect(p + 1, page, PROT_READ | PROT_WRITE, -1));
    SHOW("pkey_mprotect(p + 1, page, RW, 5)", pkey_mprotect(p + 1, page, PROT_READ | PROT_WRITE, 5));
    SHOW("pkey_mprotect(p, 0, RW, 5)", pkey_mprotect(p, 0, PROT_READ | PROT_WRITE, 5));
    SHOW("pkey_mprotect(p, page, RW|GROWSDOWN|GROWSUP, 5)",
         pkey_mprotect(p, page, PROT_READ | PROT_WRITE | PROT_GROWSDOWN | PROT_GROWSUP, 5));
    SHOW("pkey_mprotect(p, page, 0x1000, 5)", pkey_mprotect(p, page, 0x1000, 5));
    SHOW("pkey_mprotect(p, 2*page, RW, -1)", pkey_mprotect(p, 2 * page, PROT_READ | PROT_WRITE, -1));
    SHOW("pkey_mprotect(p, 2*page, RW, 5)", pkey_mprotect(p, 2 * page, PROT_READ | PROT_WRITE, 5));
    SHOW("pkey_mprotect(NULL, page, RW, -1)", pkey_mprotect(NULL, page, PROT_READ | PROT_WRITE, -1));
    SHOW("pkey_mprotect(NULL, page, RW, 5)", pkey_mprotect(NULL, page, PROT_READ | PROT_WRITE, 5));

    SHOW("pkey_get(-1)", pkey_get(-1));
    SHOW("pkey_get(16)", pkey_get(16));
    in_child("pkey_get(0)", 0, 0, 0);
    in_child("pkey_get(1)", 0, 1, 0);
    in_child("pkey_get(15)", 0, 15, 0);
    SHOW("pkey_set(-1, 0)", pkey_set(-1, 0));
    SHOW("pkey_set(16, 0)", pkey_set(16, 0));
    SHOW("pkey_set(1, 4)", pkey_set(1, 4));
    in_child("pkey_set(0, 0)", 1, 0, 0);
    in_child("pkey_set(1, 0)", 1, 1, 0);
    in_child("pkey_set(1, 3)", 1, 1, 3);
    return 0;
}
"""


def main() -> int:
    has = run("grep -qw pku /proc/cpuinfo && echo yes || echo no")
    if has.stdout.strip() != "no":
        sys.stderr.write("this processor has protection keys; the oracle is one without\n")
        return 1
    with workdir() as d:
        c = Path(d) / "pkey.c"
        c.write_text(PROGRAM, encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "pkey")
        r = run(f"gcc -O1 -Wall -Werror -o {exe} {wsl_path(c)} && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's pkey_* on Linux 6.6, a processor without protection keys\n"
              "# (posix/tools/oracle/pkey_harness.py): <call> = <return> <errno>\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
