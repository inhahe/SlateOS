/*
 * ctest-sigpipe -- ring-3 test that a write to a pipe or stream socket with
 * no reader raises SIGPIPE, as Linux's kernel does, and fails with EPIPE.
 *
 * Guards design-decisions.md §1176 (posix's broken_pipe).  The answers below
 * are Linux 6.6's, measured with the same calls under WSL.
 *
 * ## Why a fixture and not only the unit tests
 *
 * The host tests run the dispatch with a handler installed: they cannot let
 * SIGPIPE's default action happen, since it ends the process.  Only here does
 * a C program that does nothing about SIGPIPE die of it -- the case that
 * matters, since it is how `producer | head -1` stops the producer.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.  No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   x0  its setup failed
 *   x1  the (first) call answered, or set errno, otherwise than Linux does
 *   x2  the handler ran a number of times other than expected after it
 *   x3  what the handler was told (si_code, si_pid) is not Linux's
 *
 *   1x  a child writes to a pipe with no reader, SIGPIPE at its default:
 *       it dies of SIGPIPE (11: fork failed; 12: it lived, or died of
 *       something else)
 *   2x  SIGPIPE ignored: the write is -1, EPIPE
 *   3x  a handler (SA_SIGINFO) that sets errno: it runs once, told SI_USER
 *       from this process, and the write is still -1, EPIPE
 *   5x  a socketpair whose peer has closed: send with MSG_NOSIGNAL is EPIPE
 *       and raises nothing (51, 52); write then raises it once (55, 56)
 *   6x  SIGPIPE blocked: EPIPE (61), no handler (62); pending (65);
 *       delivered once unblocked (66)
 *   7x  tee into a pipe with no reader: EPIPE, raised once
 *   8x  write after shutdown(SHUT_WR) on a socketpair end: EPIPE, raised once
 *   9x  write to a TCP socket never connected: EPIPE, raised once
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <signal.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static volatile sig_atomic_t pipes;
static volatile sig_atomic_t code;
static volatile sig_atomic_t sender;

static void on_pipe(int sig, siginfo_t *info, void *uc)
{
    (void)sig;
    (void)uc;
    pipes++;
    code = info->si_code;
    sender = info->si_pid;
    /* A handler that changes errno: the write's answer must not change. */
    errno = EINTR;
}

static int handle_pipe(void)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_pipe;
    sa.sa_flags = SA_SIGINFO;
    sigemptyset(&sa.sa_mask);
    return sigaction(SIGPIPE, &sa, NULL);
}

/* A pipe whose reading end is closed; its writing end, or -1. */
static int broken_pipe_fd(void)
{
    int p[2];
    if (pipe(p) != 0)
        return -1;
    close(p[0]);
    return p[1];
}

/* Whether a call answered -1 with errno EPIPE. */
static int is_epipe(long r)
{
    return r == -1 && errno == EPIPE;
}

/* 1x: SIGPIPE at its default ends a process that writes to a broken pipe. */
static int default_kills(void)
{
    char b = 'x';
    int fd = broken_pipe_fd();
    if (fd < 0 || signal(SIGPIPE, SIG_DFL) == SIG_ERR)
        return 10;
    pid_t child = fork();
    if (child < 0)
        return 11;
    if (child == 0) {
        (void)write(fd, &b, 1);
        _exit(7); /* lived: SIGPIPE was not raised, or did not kill */
    }
    close(fd);
    int status = 0;
    if (waitpid(child, &status, 0) != child)
        return 12;
    /* A signal's death, however this system reports it to the parent: as
     * one, or as the 128 + n status the C library exits with. */
    int died_of_it = (WIFSIGNALED(status) && WTERMSIG(status) == SIGPIPE)
        || (WIFEXITED(status) && WEXITSTATUS(status) == 128 + SIGPIPE);
    return died_of_it ? 0 : 12;
}

/* 2x */
static int ignored(void)
{
    char b = 'x';
    int fd = broken_pipe_fd();
    if (fd < 0 || signal(SIGPIPE, SIG_IGN) == SIG_ERR)
        return 20;
    errno = 0;
    int ok = is_epipe(write(fd, &b, 1));
    close(fd);
    return ok ? 0 : 21;
}

/* 3x */
static int handled(void)
{
    char b = 'x';
    int fd = broken_pipe_fd();
    if (fd < 0 || handle_pipe() != 0)
        return 30;
    pipes = 0;
    errno = 0;
    int ok = is_epipe(write(fd, &b, 1));
    close(fd);
    if (!ok)
        return 31;
    if (pipes != 1)
        return 32;
    if (code != SI_USER || sender != getpid())
        return 33;
    return 0;
}

/* 5x */
static int nosignal(void)
{
    char b = 'x';
    int s[2];
    if (handle_pipe() != 0 || socketpair(AF_UNIX, SOCK_STREAM, 0, s) != 0)
        return 50;
    close(s[1]);
    pipes = 0;
    errno = 0;
    if (!is_epipe(send(s[0], &b, 1, MSG_NOSIGNAL)))
        return 51;
    if (pipes != 0)
        return 52;
    errno = 0;
    if (!is_epipe(write(s[0], &b, 1)))
        return 55;
    if (pipes != 1)
        return 56;
    close(s[0]);
    return 0;
}

/* 6x */
static int blocked(void)
{
    char b = 'x';
    sigset_t set, old, pending;
    int fd = broken_pipe_fd();
    if (fd < 0 || handle_pipe() != 0)
        return 60;
    sigemptyset(&set);
    sigaddset(&set, SIGPIPE);
    if (sigprocmask(SIG_BLOCK, &set, &old) != 0)
        return 60;
    pipes = 0;
    errno = 0;
    int ok = is_epipe(write(fd, &b, 1));
    close(fd);
    if (!ok)
        return 61;
    if (pipes != 0)
        return 62;
    sigemptyset(&pending);
    if (sigpending(&pending) != 0 || !sigismember(&pending, SIGPIPE))
        return 65;
    if (sigprocmask(SIG_SETMASK, &old, NULL) != 0)
        return 60;
    return pipes == 1 ? 0 : 66;
}

/* 7x */
static int tee_to_nobody(void)
{
    int src[2], dst[2];
    if (handle_pipe() != 0 || pipe(src) != 0 || pipe(dst) != 0)
        return 70;
    if (write(src[1], "hello", 5) != 5)
        return 70;
    close(dst[0]);
    pipes = 0;
    errno = 0;
    if (!is_epipe(tee(src[0], dst[1], 5, 0)))
        return 71;
    return pipes == 1 ? 0 : 72;
}

/* 8x */
static int after_shut_wr(void)
{
    char b = 'x';
    int s[2];
    if (handle_pipe() != 0 || socketpair(AF_UNIX, SOCK_STREAM, 0, s) != 0)
        return 80;
    if (shutdown(s[0], SHUT_WR) != 0)
        return 80;
    pipes = 0;
    errno = 0;
    if (!is_epipe(write(s[0], &b, 1)))
        return 81;
    return pipes == 1 ? 0 : 82;
}

/* 9x */
static int never_connected(void)
{
    char b = 'x';
    int t = socket(AF_INET, SOCK_STREAM, 0);
    if (t < 0 || handle_pipe() != 0)
        return 90;
    pipes = 0;
    errno = 0;
    if (!is_epipe(write(t, &b, 1)))
        return 91;
    close(t);
    return pipes == 1 ? 0 : 92;
}

int main(void)
{
    int (*checks[])(void) = {
        default_kills, ignored, handled, nosignal, blocked, tee_to_nobody, after_shut_wr,
        never_connected,
    };
    for (unsigned i = 0; i < sizeof checks / sizeof checks[0]; i++) {
        int r = checks[i]();
        if (r != 0)
            return r;
    }
    return 42;
}
