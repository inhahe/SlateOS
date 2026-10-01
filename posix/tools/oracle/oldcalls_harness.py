"""glibc 2.39's old BSD and System V calls -- sigblock, sigsetmask, siggetmask,
sigstack, sigreturn, gsignal, ssignal; getwd, group_member, revoke, setlogin,
ttyslot, profil; getpw; gtty, stty; isctype, isfdtype, dysize; vlimit,
rpmatch; execveat's refusals -- as the oracle for the modules that define
them.

    python posix/tools/oracle/oldcalls_harness.py   # writes posix/src/oldcalls_oracle.txt

One line a probe, `<name> = <what it gave>`: a result, and `errno`'s name
after a failure. Each probe runs in a child of its own, so that one that
changes the process (a signal mask, a handler) or crashes it changes no
other.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "oldcalls_oracle.txt"

PROGRAM = r'''
#define _GNU_SOURCE
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <pwd.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sgtty.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/vlimit.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PROBE(NAME, ...)                                                      \
    do {                                                                       \
        fflush(stdout);                                                        \
        pid_t pid_ = fork();                                                   \
        if (pid_ == 0) {                                                       \
            printf("%s =", NAME);                                              \
            __VA_ARGS__;                                                       \
            printf("\n");                                                      \
            fflush(stdout);                                                    \
            _exit(0);                                                          \
        }                                                                      \
        int st_;                                                               \
        waitpid(pid_, &st_, 0);                                                \
        if (!WIFEXITED(st_)) printf("%s = crash %d\n", NAME, WTERMSIG(st_));  \
    } while (0)

static const char *en(int e)
{
    switch (e) {
    case 0: return "0";
    case EINVAL: return "EINVAL";
    case ENOSYS: return "ENOSYS";
    case EPERM: return "EPERM";
    case ENOENT: return "ENOENT";
    case EBADF: return "EBADF";
    case ENOTDIR: return "ENOTDIR";
    case ELOOP: return "ELOOP";
    case EACCES: return "EACCES";
    case ENOTTY: return "ENOTTY";
    case ERANGE: return "ERANGE";
    case EFAULT: return "EFAULT";
    default: { static char b[16]; snprintf(b, sizeof b, "e%d", e); return b; }
    }
}

#define RC(expr) do { errno = 0; long r_ = (long)(expr); int e_ = errno; \
    printf(" %ld", r_); if (r_ == -1) printf("!%s", en(e_)); } while (0)

static void on_usr1(int s) { (void)s; }

int main(void)
{
    /* The BSD masks: an int of signals 1 to 32, bit n-1 for signal n. */
    PROBE("siggetmask-initial", RC(siggetmask()));
    PROBE("sigblock", {
        int a = sigblock(sigmask(SIGUSR1) | sigmask(SIGINT));
        int b = siggetmask();
        int c = sigblock(sigmask(SIGUSR2));
        int d = siggetmask();
        printf(" %d %#x %#x %#x", a, b, c, d);
    });
    PROBE("sigsetmask", {
        int a = sigsetmask(sigmask(SIGHUP) | sigmask(SIGTERM));
        int b = sigsetmask(0);
        int c = siggetmask();
        printf(" %#x %#x %#x", a, b, c);
    });
    PROBE("sigblock-kill-stop", {
        sigblock(sigmask(SIGKILL) | sigmask(SIGSTOP) | sigmask(SIGUSR1));
        printf(" %#x", siggetmask());
    });
    PROBE("sigblock-all", {
        sigblock(~0);
        printf(" %#x", siggetmask());
        sigset_t set;
        sigprocmask(SIG_BLOCK, NULL, &set);
        printf(" rt%d", sigismember(&set, SIGRTMIN));
    });
    PROBE("sigsetmask-keeps-rt", {
        sigset_t set;
        sigemptyset(&set);
        sigaddset(&set, SIGRTMIN + 1);
        sigprocmask(SIG_BLOCK, &set, NULL);
        sigsetmask(sigmask(SIGUSR1));
        sigprocmask(SIG_BLOCK, NULL, &set);
        printf(" rt%d usr1%d", sigismember(&set, SIGRTMIN + 1), sigismember(&set, SIGUSR1));
    });
    PROBE("sigstack", {
        static char stack[65536];
        struct sigstack ss = {stack + sizeof stack, 0}, old;
        RC(sigstack(&ss, &old));
        RC(sigstack(NULL, &old));
        printf(" onstack=%d", old.ss_onstack);
    });
    PROBE("sigreturn", RC(sigreturn(NULL)));
    PROBE("gsignal", {
        RC(gsignal(0));
        signal(SIGUSR1, SIG_IGN);
        RC(gsignal(SIGUSR1));
        RC(gsignal(-1));
        RC(gsignal(65));
    });
    PROBE("ssignal", {
        __sighandler_t a = ssignal(SIGUSR1, on_usr1);
        __sighandler_t b = ssignal(SIGUSR1, SIG_DFL);
        printf(" %d %d", a == SIG_DFL, b == on_usr1);
        errno = 0;
        __sighandler_t c = ssignal(SIGKILL, on_usr1);
        printf(" %d!%s", c == SIG_ERR, en(errno));
        errno = 0;
        c = ssignal(0, on_usr1);
        printf(" %d!%s", c == SIG_ERR, en(errno));
    });

    PROBE("getwd", {
        char buf[4096], cwd[4096];
        chdir("/tmp");
        char *r = getwd(buf);
        getcwd(cwd, sizeof cwd);
        printf(" %d %d", r == buf, strcmp(buf, cwd) == 0);
    });
    PROBE("getwd-null", {
        errno = 0;
        char *r = getwd(NULL);
        printf(" %d!%s", r == NULL, en(errno));
    });
    PROBE("group_member", {
        printf(" %d %d", group_member(getegid()), group_member((gid_t)987654));
        gid_t groups[64];
        int n = getgroups(64, groups);
        int all = 1;
        for (int i = 0; i < n; i++) all &= group_member(groups[i]);
        printf(" %d", all);
    });
    PROBE("revoke", RC(revoke("/dev/null")));
    PROBE("revoke-missing", RC(revoke("/nonexistent/x")));
    PROBE("setlogin", RC(setlogin("nobody")));
    PROBE("ttyslot", RC(ttyslot()));
    PROBE("profil-off", RC(profil(NULL, 0, 0, 0)));

    PROBE("getpw-0", {
        char buf[1024];
        errno = 0;
        int r = getpw(0, buf);
        struct passwd *pw = getpwuid(0);
        char want[1024];
        snprintf(want, sizeof want, "%s:%s:%d:%d:%s:%s:%s", pw->pw_name, pw->pw_passwd,
                 (int)pw->pw_uid, (int)pw->pw_gid, pw->pw_gecos, pw->pw_dir, pw->pw_shell);
        printf(" %d %d", r, strcmp(buf, want) == 0);
    });
    PROBE("getpw-missing", {
        char buf[1024];
        RC(getpw(987654, buf));
    });
    PROBE("getpw-null", RC(getpw(0, NULL)));

    /* glibc's <sgtty.h> declares struct sgttyb and never defines it. */
    PROBE("gtty", { static char b[64]; RC(gtty(0, (struct sgttyb *)b)); });
    PROBE("stty", { static char b[64]; RC(stty(0, (struct sgttyb *)b)); });
    PROBE("gtty-badf", { static char b[64]; RC(gtty(-1, (struct sgttyb *)b)); });

    PROBE("isctype", {
        const int masks[] = {_ISupper, _ISlower, _ISalpha, _ISdigit, _ISxdigit, _ISspace,
                             _ISprint, _ISgraph, _ISblank, _IScntrl, _ISpunct, _ISalnum};
        const int cs[] = {'A', 'z', '5', 'f', ' ', '\t', '\n', '!', 0x7f, 0, 0xe9, -1, 200};
        for (size_t c = 0; c < sizeof cs / sizeof cs[0]; c++) {
            printf(" %d:", cs[c]);
            for (size_t m = 0; m < sizeof masks / sizeof masks[0]; m++)
                printf("%d", isctype(cs[c], masks[m]) != 0);
        }
    });
    PROBE("isctype-masks", {
        printf(" upper=%#x lower=%#x alpha=%#x digit=%#x xdigit=%#x space=%#x print=%#x graph=%#x "
               "blank=%#x cntrl=%#x punct=%#x alnum=%#x", _ISupper, _ISlower, _ISalpha, _ISdigit,
               _ISxdigit, _ISspace, _ISprint, _ISgraph, _ISblank, _IScntrl, _ISpunct, _ISalnum);
    });
    PROBE("isfdtype", {
        int f = open("/tmp", O_RDONLY | O_DIRECTORY);
        int g = open("/dev/null", O_RDONLY);
        int s = socket(AF_UNIX, SOCK_STREAM, 0);
        printf(" %d %d %d %d %d", isfdtype(f, S_IFDIR), isfdtype(f, S_IFREG), isfdtype(g, S_IFCHR),
               isfdtype(s, S_IFSOCK), isfdtype(s, S_IFREG));
        RC(isfdtype(-1, S_IFREG));
    });
    PROBE("dysize", {
        const int ys[] = {1900, 1996, 1999, 2000, 2023, 2024, 2100, 2400, 0, -4, -100};
        for (size_t i = 0; i < sizeof ys / sizeof ys[0]; i++) printf(" %d", dysize(ys[i]));
    });

    /* 4.2BSD's vlimit: a soft limit, the resource one past setrlimit's. */
    PROBE("vlimit", {
        struct rlimit r;
        RC(vlimit(LIM_CPU, 100)); getrlimit(RLIMIT_CPU, &r); printf(" %ld", (long)r.rlim_cur);
        RC(vlimit(LIM_FSIZE, 4096)); getrlimit(RLIMIT_FSIZE, &r); printf(" %ld", (long)r.rlim_cur);
        RC(vlimit(LIM_CPU, -1)); getrlimit(RLIMIT_CPU, &r); printf(" %ld", (long)r.rlim_cur);
        RC(vlimit(LIM_NORAISE, 0));
        RC(vlimit(LIM_MAXRSS + 1, 0));
        RC(vlimit(-1, 0));
    });

    /* rpmatch in the C locale: ^[yY] is 1, ^[nN] 0, anything else -1. */
    PROBE("rpmatch", {
        const char *rs[] = {"y", "Y", "yes", "n", "N", "no", "x", "", " y", "yn", "ny", "\377"};
        for (size_t i = 0; i < sizeof rs / sizeof rs[0]; i++) printf(" %d", rpmatch(rs[i]));
    });

    /* execveat's refusals, where it returns. */
    PROBE("execveat-badf", { char *argv[] = {"x", NULL}; RC(execveat(-5, "x", argv, NULL, 0)); });
    PROBE("execveat-flags", { char *argv[] = {"x", NULL}; RC(execveat(AT_FDCWD, "/bin/true", argv, NULL, 0x40000)); });
    PROBE("execveat-missing", { char *argv[] = {"x", NULL}; RC(execveat(AT_FDCWD, "/nonexistent/x", argv, NULL, 0)); });
    PROBE("execveat-empty-no-flag", { char *argv[] = {"x", NULL}; RC(execveat(AT_FDCWD, "", argv, NULL, 0)); });
    PROBE("execveat-nofollow", {
        symlink("/bin/true", "/tmp/oldcalls-link");
        char *argv[] = {"x", NULL};
        RC(execveat(AT_FDCWD, "/tmp/oldcalls-link", argv, NULL, AT_SYMLINK_NOFOLLOW));
        unlink("/tmp/oldcalls-link");
    });
    PROBE("execveat-notdir", {
        int f = open("/etc/passwd", O_RDONLY);
        char *argv[] = {"x", NULL};
        RC(execveat(f, "x", argv, NULL, 0));
    });
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "oc.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o oc oc.c && ./oc")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's old BSD and System V calls, for posix/src. Generated by\n"
            "# posix/tools/oracle/oldcalls_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
