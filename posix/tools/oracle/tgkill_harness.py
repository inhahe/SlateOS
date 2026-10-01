"""glibc 2.39's `tgkill` on Linux, as the oracle for posix/src/signal.rs's.

    python posix/tools/oracle/tgkill_harness.py   # writes posix/src/tgkill_oracle.txt

One line a case, `<case> = <return> <errno>`: the refusals and their order
(a `tgid` or `tid` not above zero; no such thread, before the signal is
looked at; a bad signal to a real thread), signal 0, a thread of another
process and one not its, and the calling thread -- whose handler runs
before `tgkill` returns, told `SI_TKILL`.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "tgkill_oracle.txt"

PROGRAM = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static volatile sig_atomic_t got;
static volatile int code;

static void on_usr1(int sig, siginfo_t *info, void *uc)
{
    (void)uc;
    got = sig;
    code = info->si_code;
}

static const char *name(int e)
{
    switch (e) {
    case EINVAL: return "EINVAL";
    case ESRCH: return "ESRCH";
    case EPERM: return "EPERM";
    default: return "other";
    }
}

static void show(const char *what, int tgid, int tid, int sig)
{
    errno = 0;
    int r = tgkill(tgid, tid, sig);
    printf("%s = %d %s\n", what, r, r == 0 ? "0" : name(errno));
}

int main(void)
{
    int me = getpid(), t = gettid();
    show("tgid 0", 0, t, SIGUSR1);
    show("tid 0", me, 0, SIGUSR1);
    show("tgid -1", -1, t, SIGUSR1);
    show("tid -5", me, -5, SIGUSR1);
    show("no such thread here, bad signal", me, 0x3fffffff, 999);
    show("no such process, bad signal", 0x3ffffff0, 1, 999);
    show("this thread, signal 65", me, t, 65);
    show("this thread, signal -1", me, t, -1);
    show("this thread, signal 0", me, t, 0);

    pid_t child = fork();
    if (child == 0) {
        pause();
        _exit(0);
    }
    show("another process's main thread, signal 0", child, child, 0);
    show("another process, a thread not its, signal 0", child, t, 0);
    kill(child, SIGKILL);
    waitpid(child, NULL, 0);

    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_usr1;
    sa.sa_flags = SA_SIGINFO;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGUSR1, &sa, NULL);
    got = 0;
    int r = tgkill(me, t, SIGUSR1);
    printf("this thread, SIGUSR1 = %d %s, the handler %s, told %s\n", r,
           r == 0 ? "0" : name(errno), got == SIGUSR1 ? "before the return" : "not yet",
           code == SI_TKILL ? "SI_TKILL" : "something else");
    return 0;
}
"""


def main() -> int:
    with workdir() as d:
        c = Path(d) / "tgkill.c"
        c.write_text(PROGRAM, encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "tgkill")
        r = run(f"gcc -O1 -Wall -Werror -o {exe} {wsl_path(c)} && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's tgkill on Linux (posix/tools/oracle/tgkill_harness.py):\n"
              "# <case> = <return> <errno>\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
