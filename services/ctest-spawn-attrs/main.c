/*
 * ctest-spawn-attrs -- ring-3 test of posix_spawn's attributes, which the
 * kernel applies to the child before its first instruction.
 *
 * Until 2026-10-16 every posix_spawn attribute was recorded and ignored.
 * Since then lane A's spawn applies the process group, the session, the
 * signal mask and the signals to reset to their default action
 * (SpawnEx2Args, requests/d-a-ignored-signals-and-spawn-attributes-need-a-
 * kernel-record.md), and the library refuses POSIX_SPAWN_RESETIDS only
 * while the effective ids differ from the real ones, which the spawn cannot
 * yet reset. The host has no kernel to spawn with, so only here is the
 * child seen.
 *
 * The test spawns itself: "<self> child <what>" checks one thing about the
 * child it is and exits 0 when it is as asked, 1 when not.
 *
 * Every wait is for a child that exits at once, and an alarm ends the test
 * after twenty seconds, so it cannot hang. It passes on Linux 6.6 with
 * glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  with no attribute the child is in its parent's group and session
 *       (10), with its parent's signal mask (11) and an ignored signal
 *       still ignored (12); 19: it could not be spawned
 *   2x  SETPGROUP with 0 puts the child in a group of its own (20)
 *   3x  SETSID makes it a session of its own (30)
 *   5x  SETSIGMASK gives it the mask asked (50); SETSIGDEF gives a signal
 *       the parent ignores its default action (51)
 *   6x  RESETIDS with nothing to reset starts it as before (60)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static char self[256];

static int child(const char *what)
{
    sigset_t now;
    sigprocmask(SIG_BLOCK, 0, &now);
    struct sigaction usr2;
    sigaction(SIGUSR2, 0, &usr2);
    pid_t me = getpid();
    pid_t parent = getppid();
    if (strcmp(what, "plain") == 0)
        return getpgid(0) == getpgid(parent) && getsid(0) == getsid(parent) ? 0 : 1;
    if (strcmp(what, "plain-mask") == 0)
        return sigismember(&now, SIGUSR1) && !sigismember(&now, SIGTERM) ? 0 : 1;
    if (strcmp(what, "plain-ignored") == 0)
        return usr2.sa_handler == SIG_IGN ? 0 : 1;
    if (strcmp(what, "own-group") == 0)
        return getpgid(0) == me && getsid(0) == getsid(parent) ? 0 : 1;
    if (strcmp(what, "own-session") == 0)
        return getsid(0) == me && getpgid(0) == me ? 0 : 1;
    if (strcmp(what, "mask") == 0)
        return sigismember(&now, SIGTERM) && !sigismember(&now, SIGUSR1) ? 0 : 1;
    if (strcmp(what, "default") == 0)
        return usr2.sa_handler == SIG_DFL ? 0 : 1;
    if (strcmp(what, "ids") == 0)
        return geteuid() == getuid() && getegid() == getgid() ? 0 : 1;
    return 2;
}

/* Spawn "<self> child <what>" with attr and answer its exit status: 0 when
 * the child found itself as asked, -1 when it could not be spawned. */
static int spawned(const char *what, posix_spawnattr_t *attr)
{
    char *argv[] = {self, (char *)"child", (char *)what, 0};
    pid_t pid;
    if (posix_spawn(&pid, self, 0, attr, argv, environ) != 0)
        return -1;
    int status = 0;
    if (waitpid(pid, &status, 0) != pid || !WIFEXITED(status))
        return -1;
    return WEXITSTATUS(status);
}

int main(int argc, char **argv)
{
    if (argc == 3 && strcmp(argv[1], "child") == 0)
        return child(argv[2]);
    alarm(20);
    if (strchr(argv[0], '/'))
        snprintf(self, sizeof self, "%s", argv[0]);
    else
        snprintf(self, sizeof self, "/mnt/tests/%s.elf", argv[0]);

    /* The parent blocks SIGUSR1 and ignores SIGUSR2. */
    sigset_t usr1;
    sigemptyset(&usr1);
    sigaddset(&usr1, SIGUSR1);
    sigprocmask(SIG_BLOCK, &usr1, 0);
    signal(SIGUSR2, SIG_IGN);

    int r = spawned("plain", 0);
    if (r < 0)
        return 19;
    if (r != 0)
        return 10;
    if (spawned("plain-mask", 0) != 0)
        return 11;
    if (spawned("plain-ignored", 0) != 0)
        return 12;

    posix_spawnattr_t attr;
    posix_spawnattr_init(&attr);
    posix_spawnattr_setpgroup(&attr, 0);
    posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETPGROUP);
    if (spawned("own-group", &attr) != 0)
        return 20;

    posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSID);
    if (spawned("own-session", &attr) != 0)
        return 30;

    sigset_t term;
    sigemptyset(&term);
    sigaddset(&term, SIGTERM);
    posix_spawnattr_setsigmask(&attr, &term);
    posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGMASK);
    if (spawned("mask", &attr) != 0)
        return 50;

    sigset_t usr2;
    sigemptyset(&usr2);
    sigaddset(&usr2, SIGUSR2);
    posix_spawnattr_setsigdefault(&attr, &usr2);
    posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGDEF);
    if (spawned("default", &attr) != 0)
        return 51;

    if (getuid() == geteuid() && getgid() == getegid()) {
        posix_spawnattr_setflags(&attr, POSIX_SPAWN_RESETIDS);
        if (spawned("ids", &attr) != 0)
            return 60;
    }
    posix_spawnattr_destroy(&attr);
    return 42;
}
