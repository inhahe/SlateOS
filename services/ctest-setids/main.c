/*
 * ctest-setids -- ring-3 test of the setuid family over the kernel's four
 * ids of each kind: real, effective, saved and filesystem.
 *
 * Until 2026-10-16 the C library knew one uid and one gid a process, set by
 * SYS_PROCESS_SET_CREDENTIALS: seteuid(1000) was a permanent drop and
 * getresuid reported one id three times. Lane A's kernel keeps four ids of
 * each kind (its design-decisions 1552) and moves between them by Linux's
 * rules (SYS_PROCESS_SET_IDS, SYS_PROCESS_GET_IDS), and the library now asks
 * it. The host has no kernel to keep them, so only here are they seen.
 *
 * It runs as root with the right to set credentials (the "creds" grant),
 * and gives root up only at the end. It passes on Linux 6.6 with glibc, as
 * root.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  it starts as root, all three ids 0 (10; 19: not root, so nothing
 *       else can be checked); seteuid(1000) sets the effective id alone
 *       (11) and seteuid(0) takes root back (12); -1 everywhere in
 *       setresuid changes nothing (13); setuid(-1) and seteuid(-1) are
 *       EINVAL (15)
 *   2x  setfsuid answers the previous filesystem id (20), which follows the
 *       effective id (21); -1 reads it without change (22)
 *   3x  the group ids: setegid(100) sets the effective gid alone (30) and
 *       setegid(0) takes it back (31); setresgid(10, 20, 30) sets each (32)
 *   5x  setreuid(1000, 1000) as root keeps no 0 but the saved id, which
 *       setreuid makes the new effective id: seteuid(0) is then EPERM (50);
 *       a fresh start: setresuid(1000, 1000, 0) keeps root in the saved id,
 *       so seteuid(0) works (51)
 *   6x  setresuid(1000, 1000, 1000) gives root up: seteuid(0) and setuid(0)
 *       are EPERM (60, 61); moving among the ids it has still works (62)
 *   7x  (run before 6x, while root) the supplementary groups setgroups
 *       installs are what getgroups reports -- how many (70), and which, in
 *       ascending order (71) -- though this process holds no File
 *       capability, which getgroups once needed to read /proc/self/status
 */

#define _GNU_SOURCE
#include <errno.h>
#include <grp.h>
#include <sys/fsuid.h>
#include <sys/wait.h>
#include <unistd.h>

static int uids_are(uid_t r, uid_t e, uid_t s)
{
    uid_t a, b, c;
    return getresuid(&a, &b, &c) == 0 && a == r && b == e && c == s;
}

static int gids_are(gid_t r, gid_t e, gid_t s)
{
    gid_t a, b, c;
    return getresgid(&a, &b, &c) == 0 && a == r && b == e && c == s;
}

static int fails_with(int ret, int err)
{
    return ret == -1 && errno == err;
}

/* Check 5x's first half in a child, which may lose root for good. */
static int setreuid_child(void)
{
    pid_t child = fork();
    if (child == 0) {
        if (setreuid(1000, 1000) != 0 || !uids_are(1000, 1000, 1000))
            _exit(1);
        errno = 0;
        _exit(fails_with(seteuid(0), EPERM) ? 0 : 2);
    }
    int status = 0;
    if (child < 0 || waitpid(child, &status, 0) != child)
        return 0;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0;
}

int main(void)
{
    alarm(20);
    if (getuid() != 0)
        return 19;
    if (!uids_are(0, 0, 0))
        return 10;

    if (seteuid(1000) != 0 || !uids_are(0, 1000, 0) || geteuid() != 1000)
        return 11;
    if (seteuid(0) != 0 || !uids_are(0, 0, 0))
        return 12;
    if (setresuid((uid_t)-1, (uid_t)-1, (uid_t)-1) != 0 || !uids_are(0, 0, 0))
        return 13;
    errno = 0;
    if (!fails_with(setuid((uid_t)-1), EINVAL))
        return 15;
    errno = 0;
    if (!fails_with(seteuid((uid_t)-1), EINVAL))
        return 15;

    /* 2x */
    if (setfsuid(1234) != 0)
        return 20;
    if (setfsuid((uid_t)-1) != 1234)
        return 22;
    setfsuid(0);
    if (seteuid(1000) != 0 || setfsuid((uid_t)-1) != 1000)
        return 21;
    if (seteuid(0) != 0)
        return 21;

    /* 3x */
    if (setegid(100) != 0 || !gids_are(0, 100, 0) || getegid() != 100)
        return 30;
    if (setegid(0) != 0 || !gids_are(0, 0, 0))
        return 31;
    if (setresgid(10, 20, 30) != 0 || !gids_are(10, 20, 30) || getgid() != 10)
        return 32;
    if (setresgid(0, 0, 0) != 0)
        return 32;

    /* 7x */
    gid_t want[] = {27, 4};
    if (setgroups(2, want) != 0 || getgroups(0, 0) != 2)
        return 70;
    gid_t got[8];
    if (getgroups(8, got) != 2 || got[0] != 4 || got[1] != 27)
        return 71;
    if (setgroups(0, 0) != 0 || getgroups(0, 0) != 0)
        return 71;

    /* 5x */
    if (!setreuid_child())
        return 50;
    if (setresuid(1000, 1000, 0) != 0 || !uids_are(1000, 1000, 0))
        return 51;
    if (seteuid(0) != 0 || !uids_are(1000, 0, 0))
        return 51;
    if (setresuid(0, 0, 0) != 0 || !uids_are(0, 0, 0))
        return 51;

    /* 6x */
    if (setresuid(1000, 1000, 1000) != 0 || !uids_are(1000, 1000, 1000))
        return 60;
    errno = 0;
    if (!fails_with(seteuid(0), EPERM))
        return 60;
    errno = 0;
    if (!fails_with(setuid(0), EPERM))
        return 61;
    if (setreuid((uid_t)-1, 1000) != 0 || setuid(1000) != 0 || !uids_are(1000, 1000, 1000))
        return 62;
    return 42;
}
