/*
 * ctest-dumpable -- ring-3 test of the dumpable flag, prctl's
 * PR_SET_DUMPABLE and PR_GET_DUMPABLE.
 *
 * Until 2026-10-16 the C library kept the flag itself, where nothing in the
 * kernel could see it. It is the kernel's now (SYS_PROCESS_DUMPABLE, 1142,
 * lane A's answer in
 * requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md): the
 * flag /proc consults, which fork copies and exec resets. The host tests
 * check the library's half against a stand-in; only here is the kernel's
 * flag seen to cross fork and to come back at exec.
 *
 * Nothing in it waits, but on a child, which it waits for with an alarm
 * behind it.
 *
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a process starts dumpable (10); it can be made not dumpable (11)
 *       and dumpable again (13); 2 is refused, EINVAL (12)
 *   2x  a forked child has its parent's flag, 0 (20); 29: fork or wait
 *       failed
 *   3x  an exec makes the process dumpable again (30); 39: the program
 *       could not find itself, or the exec failed
 */

#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

/* The child's code for "the flag was what it should be". */
#define CHILD_OK 7

static int run_child(int (*body)(void))
{
    pid_t child = fork();
    if (child < 0)
        return -1;
    if (child == 0)
        _exit(body());
    int status;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status))
        return -1;
    return WEXITSTATUS(status);
}

static int child_sees_zero(void)
{
    return prctl(PR_GET_DUMPABLE, 0, 0, 0, 0) == 0 ? CHILD_OK : 1;
}

static char self[4096];

static int child_execs_itself(void)
{
    char *args[] = {self, "after-exec", 0};
    char *env[] = {0};
    execve(self, args, env);
    return 2;
}

int main(int argc, char **argv)
{
    if (argc > 1 && strcmp(argv[1], "after-exec") == 0)
        return prctl(PR_GET_DUMPABLE, 0, 0, 0, 0) == 1 ? CHILD_OK : 1;

    alarm(20);
    if (prctl(PR_GET_DUMPABLE, 0, 0, 0, 0) != 1)
        return 10;
    if (prctl(PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 || prctl(PR_GET_DUMPABLE, 0, 0, 0, 0) != 0)
        return 11;
    errno = 0;
    if (prctl(PR_SET_DUMPABLE, 2, 0, 0, 0) != -1 || errno != EINVAL)
        return 12;

    int rc = run_child(child_sees_zero);
    if (rc < 0)
        return 29;
    if (rc != CHILD_OK)
        return 20;

    ssize_t len = readlink("/proc/self/exe", self, sizeof self - 1);
    if (len <= 0)
        return 39;
    self[len] = 0;
    rc = run_child(child_execs_itself);
    if (rc < 0 || rc == 2)
        return 39;
    if (rc != CHILD_OK)
        return 30;

    if (prctl(PR_SET_DUMPABLE, 1, 0, 0, 0) != 0 || prctl(PR_GET_DUMPABLE, 0, 0, 0, 0) != 1)
        return 13;
    return 42;
}
