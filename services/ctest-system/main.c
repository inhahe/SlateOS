/*
 * ctest-system -- ring-3 test of system(3): the shell gets the caller's
 * environment, and while it runs the caller ignores SIGINT and SIGQUIT and
 * blocks SIGCHLD, getting them back afterwards (POSIX; glibc's do_system).
 *
 * Until 2026-10-06 the C library's system() passed the shell no environment
 * at all -- no PATH, no HOME, nothing the caller had set -- and left the
 * caller's signals as they were while the shell ran
 * (known-issues-resolved/D-SYSTEM-RAN-ITS-SHELL-WITH-NO-ENVIRONMENT.md).
 * The host test can only see what the caller gets back; the environment and
 * the signals need a shell that runs.  /bin/sh on the image is dash, a Linux
 * program, so this is also a native program starting a Linux one.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.  No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   9   there is no /bin/sh in this system's root -- not system()'s doing:
 *       the kernel mounts the image at /mnt, and only a boot-test rung that
 *       copies the shell in puts one there
 *       (requests/d-ab-the-booted-system-has-no-bin-sh.md)
 *   1x  a variable the caller set reaches the shell (10: it did not)
 *   2x  the shell's exit status comes back (20), and a command that begins
 *       with '-' is a command, not an option to the shell (21)
 *   3x  a SIGINT the shell sends its caller while system() waits is not
 *       caught (30: the caller's handler ran); afterwards the handler is the
 *       caller's again (31)
 *   5x  a SIGCHLD handler that reaps cannot take the shell from system():
 *       system() returns the shell's status (50); the SIGCHLD held back
 *       meanwhile arrives once system() has returned (51)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t interrupted;
static volatile sig_atomic_t chld_seen;
static volatile sig_atomic_t chld_reaped;

static void on_int(int sig)
{
    (void)sig;
    interrupted = 1;
}

static void on_chld(int sig)
{
    (void)sig;
    chld_seen = 1;
    int saved = errno;
    while (waitpid(-1, NULL, WNOHANG) > 0)
        chld_reaped = 1;
    errno = saved;
}

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
}

int main(void)
{
    int st;

    if (access("/bin/sh", X_OK) != 0)
        return 9;

    /* 1x */
    if (setenv("SLATE_SYSTEM_PROBE", "it-arrived", 1) != 0)
        return 10;
    st = system("test \"$SLATE_SYSTEM_PROBE\" = it-arrived");
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0)
        return 10;

    /* 2x */
    st = system("exit 3");
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 3)
        return 20;
    /* "-c": run as a command named -c, which does not exist -- 127 -- rather
     * than taken as the shell's own -c, which would want another word. */
    st = system("-c");
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 127)
        return 21;

    /* 3x: the shell interrupts its caller; system() is ignoring SIGINT. */
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_int;
    sigemptyset(&sa.sa_mask);
    if (sigaction(SIGINT, &sa, NULL) != 0)
        return 30;
    st = system("kill -INT $PPID");
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0)
        return 30;
    sleep_ms(50); /* time for a delivery that should not come */
    if (interrupted)
        return 30;
    struct sigaction now;
    if (sigaction(SIGINT, NULL, &now) != 0 || now.sa_handler != on_int)
        return 31;

    /* 5x */
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_chld;
    sigemptyset(&sa.sa_mask);
    if (sigaction(SIGCHLD, &sa, NULL) != 0)
        return 50;
    st = system("exit 5");
    if (st == -1 || !WIFEXITED(st) || WEXITSTATUS(st) != 5)
        return 50;
    for (int i = 0; i < 100 && !chld_seen; i++)
        sleep_ms(10);
    if (!chld_seen)
        return 51;
    return 42;
}
