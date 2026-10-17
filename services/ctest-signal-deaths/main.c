/*
 * ctest-signal-deaths -- does a process that dies of a signal reach its
 * parent as a death by that signal, and one that exits as an exit?
 *
 * The C library carries out a signal's default action itself, and ends the
 * process through the kernel's SYS_SIGNAL_EXIT_SELF, so that its parent's
 * wait reports WIFSIGNALED with WTERMSIG -- what a shell's "Killed" and
 * "Aborted" and GNU timeout go by, and what an exit status of 128 + n
 * cannot say. abort() raises SIGABRT, as POSIX asks, so a handler for it
 * runs first, and the process still dies of it when the handler returns.
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  raise(SIGTERM) at its default: dies of SIGTERM
 *   2x  raise(SIGKILL): dies of SIGKILL
 *   3x  abort(): dies of SIGABRT
 *   5x  abort() with a SIGABRT handler that returns: the handler ran (the
 *       child says so in its pipe) (50), and the child dies of SIGABRT (51)
 *   6x  exit(130): an exit with status 130, not a death by SIGINT
 *   7x  kill(getpid(), SIGUSR1) at its default: dies of SIGUSR1
 */

#define _GNU_SOURCE
#include <signal.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

static int pipe_w = -1;

static void says_so(int sig)
{
    (void)sig;
    if (write(pipe_w, "h", 1) != 1)
        _exit(97);
}

/* Fork a child that does `what`; its wait status, or -1. */
static int child_status(int what)
{
    pid_t pid = fork();
    if (pid < 0)
        return -1;
    if (pid == 0) {
        switch (what) {
        case 1: raise(SIGTERM); break;
        case 2: raise(SIGKILL); break;
        case 3: abort();
        case 5: {
            struct sigaction sa = {0};
            sa.sa_handler = says_so;
            sigaction(SIGABRT, &sa, NULL);
            abort();
        }
        case 6: exit(130);
        case 7: kill(getpid(), SIGUSR1); break;
        }
        _exit(99);
    }
    int status = 0;
    if (waitpid(pid, &status, 0) != pid)
        return -1;
    return status;
}

static int died_of(int status, int sig)
{
    return status != -1 && WIFSIGNALED(status) && WTERMSIG(status) == sig;
}

int main(void)
{
    alarm(20);
    if (!died_of(child_status(1), SIGTERM))
        return 10;
    if (!died_of(child_status(2), SIGKILL))
        return 20;
    if (!died_of(child_status(3), SIGABRT))
        return 30;

    int p[2];
    if (pipe(p) != 0)
        return 50;
    pipe_w = p[1];
    int status = child_status(5);
    close(p[1]);
    char c = 0;
    if (read(p[0], &c, 1) != 1 || c != 'h')
        return 50;
    if (!died_of(status, SIGABRT))
        return 51;

    status = child_status(6);
    if (status == -1 || !WIFEXITED(status) || WEXITSTATUS(status) != 130)
        return 60;
    if (!died_of(child_status(7), SIGUSR1))
        return 70;
    return 42;
}
