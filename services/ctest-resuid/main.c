/*
 * ctest-resuid -- ring-3 test that getresuid and getresgid report the
 * process's own ids, not root's.
 *
 * Until 2026-10-06 the C library answered 0 -- root -- for all three ids of
 * each, whatever the process was: a program that dropped to another user and
 * then checked its ids before something privileged (OpenSSH, sudo and
 * polkit check them) was told it was still root.  The kernel keeps one uid
 * and one gid a process, which are all three; getuid and getgid already
 * read them.  The decisive check is 21: after the drop to 1000, root is the
 * wrong answer.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.  No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  as started: getresuid's three are getuid's and geteuid's (11),
 *       getresgid's are getgid's and getegid's (12)
 *   2x  dropped to gid 1000, then uid 1000 (20: setgid or setuid refused --
 *       the fixture needs to start as root); getresuid answers 1000 three
 *       times (21: it said root), getresgid 1000 three times (22)
 *   3x  a NULL pointer is EFAULT, for each (31, 32)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <stddef.h>
#include <unistd.h>

int main(void)
{
    uid_t r, e, s;
    gid_t rg, eg, sg;

    /* 1x */
    if (getresuid(&r, &e, &s) != 0 || r != getuid() || e != geteuid() || s != getuid())
        return 11;
    if (getresgid(&rg, &eg, &sg) != 0 || rg != getgid() || eg != getegid() || sg != getgid())
        return 12;

    /* 2x: the group first, while the process may still change it. */
    if (setgid(1000) != 0 || setuid(1000) != 0)
        return 20;
    if (getresuid(&r, &e, &s) != 0 || r != 1000 || e != 1000 || s != 1000)
        return 21;
    if (getresgid(&rg, &eg, &sg) != 0 || rg != 1000 || eg != 1000 || sg != 1000)
        return 22;

    /* 3x */
    errno = 0;
    if (getresuid(NULL, &e, &s) != -1 || errno != EFAULT)
        return 31;
    errno = 0;
    if (getresgid(&rg, NULL, &sg) != -1 || errno != EFAULT)
        return 32;
    return 42;
}
