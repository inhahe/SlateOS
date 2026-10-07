/*
 * ctest-renice -- do getpriority and setpriority reach the process they
 * name on SlateOS, and refuse what Linux refuses?
 *
 * "Nice" is a process's scheduling politeness, -20 (greediest) to 19 (most
 * yielding). setpriority(PRIO_PROCESS, pid, n) is what renice and every
 * task manager call. A native SlateOS program could name no process but
 * itself until lane A's SYS_PROCESS_SET_PRIORITY: for a while the C library
 * changed the caller in its place and reported success, then refused every
 * other target (requests/e-ad-renicing-another-process-renices-the-caller.md).
 * The library asks the kernel's calls now, which take a process, a group or
 * a user, and decide who may: the target's parent may lower its priority,
 * nobody may raise it beyond its RLIMIT_NICE without the right, and a
 * stranger may not touch it.
 *
 * The checks, in order:
 *
 *    1x  a parent lowers its child's priority, and it is the child that moves
 *    2x  the refusals: a raise beyond RLIMIT_NICE (EACCES), a process that is
 *        not ours (EPERM), one that does not exist (ESRCH), a bad `which`
 *        (EINVAL); and reading anyone's needs no right
 *    3x  a process group: every member is set, and reading it answers its
 *        most favoured member (the lowest nice)
 *    4x  nice(): a lowering answers the new nice, a raise is EPERM
 *
 * It must run without the right to raise priority -- on Linux, not as root
 * (CAP_SYS_NICE); on SlateOS, with no Thread capability carrying
 * IO_REALTIME -- and with RLIMIT_NICE at its default of 0. The one process
 * it is refused, pid 1, it asks to set to the nice it already has, so that
 * if it were allowed nothing would change.
 *
 * Exit codes: 42 is every check passed.
 *    2  fork, pipe or waitpid failed     Otherwise <check><step>; no step
 *    is 42.
 *
 * The same source compiles and passes on Linux, which is how its
 * expectations were checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

/* A pid nothing has: above Linux's largest pid_max, 4194304. */
#define NO_SUCH_PID 999999999

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

static void emit_num(long n)
{
    char buf[24];
    int i = (int)sizeof buf;
    int neg = n < 0;
    unsigned long v = neg ? 0UL - (unsigned long)n : (unsigned long)n;
    buf[--i] = '\0';
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v != 0 && i > 1);
    if (neg) {
        buf[--i] = '-';
    }
    emit(buf + i);
}

static int fail(int code, const char *what)
{
    int e = errno;
    emit("ctest-renice: FAIL ");
    emit_num(code);
    emit(": ");
    emit(what);
    emit(" (errno ");
    emit_num(e);
    emit(")\n");
    return code;
}

static void ok(const char *what)
{
    emit("ctest-renice: ok ");
    emit(what);
    emit("\n");
}

/* getpriority, with its error told apart from a nice of -1: the nice, or
 * -100 with errno set. */
static int nice_of(int which, id_t who)
{
    errno = 0;
    int n = getpriority(which, who);
    if (n == -1 && errno != 0) {
        return -100;
    }
    return n;
}

/* A child that waits until `fd` reads end of file, then exits 0. */
static pid_t idle_child(int fd_read, int fd_write)
{
    pid_t pid = fork();
    if (pid == 0) {
        char c;
        close(fd_write);
        while (read(fd_read, &c, 1) > 0) {
        }
        _exit(0);
    }
    return pid;
}

/* Close the write end that keeps `child` waiting, and reap it. */
static int release(pid_t child, int fd_write)
{
    int status = 0;
    close(fd_write);
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status)) {
        return -1;
    }
    return 0;
}

/* Check 3, in a process that leads its own group: it and a child of its
 * are the group. 0, or the failing step. */
static int group_checks(void)
{
    if (setpgid(0, 0) != 0) {
        return fail(30, "setpgid(0, 0)");
    }
    int fds[2];
    if (pipe(fds) != 0) {
        return 2;
    }
    pid_t member = idle_child(fds[0], fds[1]);
    if (member < 0) {
        return 2;
    }
    close(fds[0]);
    int result = 0;
    if (setpriority(PRIO_PGRP, 0, 12) != 0) {
        result = fail(31, "setpriority(PRIO_PGRP, 0, 12)");
    } else if (nice_of(PRIO_PROCESS, (id_t)member) != 12 || nice_of(PRIO_PROCESS, 0) != 12) {
        result = fail(32, "every member of the group is at 12");
    } else if (setpriority(PRIO_PROCESS, 0, 15) != 0) {
        result = fail(33, "a lowering of its own, to 15");
    } else if (nice_of(PRIO_PGRP, 0) != 12) {
        emit("ctest-renice: the group read as ");
        emit_num(nice_of(PRIO_PGRP, 0));
        emit("\n");
        result = fail(34, "a group reads as its most favoured member, 12");
    }
    if (release(member, fds[1]) != 0 && result == 0) {
        result = 2;
    }
    return result;
}

int main(void)
{
    int own = nice_of(PRIO_PROCESS, 0);
    if (own == -100) {
        return fail(10, "getpriority(PRIO_PROCESS, 0)");
    }
    if (own > 9) {
        /* The checks lower a child to 10 and then raise it to 5. */
        return fail(10, "this must start at a nice of 9 or below");
    }

    /* 1. */
    int fds[2];
    if (pipe(fds) != 0) {
        return fail(2, "pipe");
    }
    pid_t child = idle_child(fds[0], fds[1]);
    if (child < 0) {
        return fail(2, "fork");
    }
    close(fds[0]);
    if (setpriority(PRIO_PROCESS, (id_t)child, 10) != 0) {
        return fail(11, "setpriority(PRIO_PROCESS, child, 10)");
    }
    if (nice_of(PRIO_PROCESS, (id_t)child) != 10) {
        return fail(12, "the child is at 10");
    }
    if (nice_of(PRIO_PROCESS, 0) != own) {
        return fail(13, "the caller's own nice did not move");
    }
    ok("a parent lowers its child's priority, and the child moves");

    /* 2. */
    errno = 0;
    if (setpriority(PRIO_PROCESS, (id_t)child, 5) != -1 || errno != EACCES) {
        return fail(21, "a raise beyond RLIMIT_NICE is EACCES");
    }
    if (nice_of(PRIO_PROCESS, (id_t)child) != 10) {
        return fail(22, "the refused raise left the child at 10");
    }
    if (release(child, fds[1]) != 0) {
        return fail(2, "waitpid");
    }
    int init_nice = nice_of(PRIO_PROCESS, 1);
    if (init_nice == -100) {
        return fail(23, "reading pid 1's nice needs no right");
    }
    errno = 0;
    if (setpriority(PRIO_PROCESS, 1, init_nice) != -1 || errno != EPERM) {
        return fail(24, "pid 1 is not ours to renice: EPERM");
    }
    errno = 0;
    if (getpriority(PRIO_PROCESS, NO_SUCH_PID) != -1 || errno != ESRCH) {
        return fail(25, "getpriority of no such process is ESRCH");
    }
    errno = 0;
    if (setpriority(PRIO_PROCESS, NO_SUCH_PID, 10) != -1 || errno != ESRCH) {
        return fail(26, "setpriority of no such process is ESRCH");
    }
    errno = 0;
    if (getpriority(3, 0) != -1 || errno != EINVAL) {
        return fail(27, "getpriority with which 3 is EINVAL");
    }
    errno = 0;
    if (setpriority(3, 0, 10) != -1 || errno != EINVAL) {
        return fail(28, "setpriority with which 3 is EINVAL");
    }
    ok("a raise, a stranger, no such process and a bad which are refused");

    /* 3. */
    pid_t leader = fork();
    if (leader < 0) {
        return fail(2, "fork");
    }
    if (leader == 0) {
        _exit(group_checks());
    }
    int status = 0;
    if (waitpid(leader, &status, 0) != leader || !WIFEXITED(status)) {
        return fail(2, "waitpid");
    }
    if (WEXITSTATUS(status) != 0) {
        return WEXITSTATUS(status);
    }
    ok("a group's members are all set, and it reads as its most favoured");

    /* 4. */
    int lowered = own + 1;
    errno = 0;
    int got = nice(1);
    if (got != lowered || errno != 0) {
        emit("ctest-renice: nice(1) answered ");
        emit_num(got);
        emit("\n");
        return fail(41, "nice(1) answers the new nice");
    }
    errno = 0;
    if (nice(-1) != -1 || errno != EPERM) {
        return fail(43, "nice(-1), a raise, is EPERM");
    }
    if (nice_of(PRIO_PROCESS, 0) != lowered) {
        return fail(44, "the refused raise left this process where it was");
    }
    ok("nice() lowers, and refuses a raise with EPERM");

    emit("ctest-renice: every check passed\n");
    return 42;
}
