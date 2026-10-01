"""glibc 2.39's pidfd_getpid, process_madvise, process_mrelease and
pidfd_spawn on Linux, as the oracle for posix/src/process.rs and
posix/src/spawn.rs.

    python posix/tools/oracle/pidfd_harness.py   # writes posix/src/pidfd_oracle.txt

One line a call, `<call> = <-1 or ok> <errno>` (`pidfd_spawn*` answer an
error number instead). A native SlateOS program has no pidfds -- its
syscall table has no number for `pidfd_open` (known-issues
B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS) -- so what
posix replays is the refusals and their order: the flags and the vector
before the descriptor, and `EBADF` for a descriptor that is not a pidfd.
The calls on a real pidfd, and the spawns, are recorded for what the native
side cannot do.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "pidfd_oracle.txt"

PROGRAM = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/pidfd.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *name(int e)
{
    switch (e) {
    case 0: return "0";
    case EBADF: return "EBADF";
    case EINVAL: return "EINVAL";
    case ESRCH: return "ESRCH";
    case EFAULT: return "EFAULT";
    case ENOENT: return "ENOENT";
    case ENOSYS: return "ENOSYS";
    default: return "other";
    }
}

#define SHOW(what, call)                                                  \
    do {                                                                  \
        errno = 0;                                                        \
        long r_ = (long)(call);                                           \
        printf("%s = %s %s\n", what, r_ < 0 ? "-1" : "ok", r_ < 0 ? name(errno) : "0"); \
    } while (0)

int main(void)
{
    char buf[64];
    struct iovec iov = { buf, sizeof buf };

    SHOW("pidfd_getpid(-1)", pidfd_getpid(-1));
    SHOW("pidfd_getpid(0)", pidfd_getpid(0));
    SHOW("pidfd_getpid(900)", pidfd_getpid(900));

    SHOW("process_madvise(-1, &iov, 1, MADV_COLD, 0)", process_madvise(-1, &iov, 1, MADV_COLD, 0));
    SHOW("process_madvise(-1, &iov, 1, MADV_COLD, 1)", process_madvise(-1, &iov, 1, MADV_COLD, 1));
    SHOW("process_madvise(0, &iov, 1, MADV_COLD, 0)", process_madvise(0, &iov, 1, MADV_COLD, 0));
    SHOW("process_madvise(-1, &iov, 1025, MADV_COLD, 0)", process_madvise(-1, &iov, 1025, MADV_COLD, 0));
    SHOW("process_madvise(-1, &iov, 1024, MADV_COLD, 1)", process_madvise(-1, &iov, 1024, MADV_COLD, 1));
    SHOW("process_madvise(-1, NULL, 1, MADV_COLD, 0)", process_madvise(-1, NULL, 1, MADV_COLD, 0));
    SHOW("process_madvise(-1, NULL, 0, MADV_COLD, 0)", process_madvise(-1, NULL, 0, MADV_COLD, 0));
    SHOW("process_madvise(-1, NULL, 0, 12345, 0)", process_madvise(-1, NULL, 0, 12345, 0));

    SHOW("process_mrelease(-1, 0)", process_mrelease(-1, 0));
    SHOW("process_mrelease(-1, 1)", process_mrelease(-1, 1));
    SHOW("process_mrelease(0, 0)", process_mrelease(0, 0));

    pid_t child = fork();
    if (child == 0) {
        pause();
        _exit(0);
    }
    int pfd = pidfd_open(child, 0);
    printf("pidfd_getpid(a live child's pidfd) = %s\n",
           pidfd_getpid(pfd) == child ? "its pid" : "something else");
    SHOW("process_madvise(a live child's pidfd, &iov, 1, MADV_DONTNEED, 0)",
         process_madvise(pfd, &iov, 1, MADV_DONTNEED, 0));
    SHOW("process_mrelease(a live child's pidfd, 0)", process_mrelease(pfd, 0));
    kill(child, SIGKILL);
    waitpid(child, NULL, 0);
    SHOW("pidfd_getpid(a reaped child's pidfd)", pidfd_getpid(pfd));
    close(pfd);

    int spfd = -1;
    char *argv[] = { "true", NULL };
    char *envp[] = { NULL };
    int rc = pidfd_spawnp(&spfd, "true", NULL, NULL, argv, envp);
    printf("pidfd_spawnp(true) = %d %s, %s\n", rc, name(rc), spfd >= 0 ? "a pidfd" : "no pidfd");
    if (spfd >= 0) {
        siginfo_t si;
        waitid(P_PIDFD, (id_t)spfd, &si, WEXITED);
        close(spfd);
    }
    rc = pidfd_spawn(&spfd, "/nonexistent", NULL, NULL, argv, envp);
    printf("pidfd_spawn(/nonexistent) = %d %s\n", rc, name(rc));
    return 0;
}
"""


def main() -> int:
    with workdir() as d:
        c = Path(d) / "pidfd.c"
        c.write_text(PROGRAM, encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "pidfd")
        r = run(f"gcc -O1 -Wall -Werror -o {exe} {wsl_path(c)} && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's pidfd_getpid, process_madvise, process_mrelease and pidfd_spawn\n"
              "# on Linux (posix/tools/oracle/pidfd_harness.py): <call> = <-1 or ok> <errno>\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
