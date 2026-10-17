/*
 * ctest-swap -- ring-3 test that swapon and swapoff reach the kernel and
 * answer as Linux's do.
 *
 * Until 2026-10-16 the C library's swapon and swapoff checked their
 * arguments and answered ENOSYS. Lane A's kernel turns swap areas on and
 * off by path (SYS_SWAP_ON, SYS_SWAP_OFF; requests/a-d-swapon-and-swapoff-
 * are-kernel-calls-now.md), and the library now asks it. Turning a real
 * area on needs a swap file the test would have to make and the machine's
 * memory to page through, so this checks only what is safe: the answers for
 * paths that are not swap, and for a caller who is not root -- each of them
 * the kernel's, so each shows the call made and its answer translated.
 *
 * It runs as root, with a File capability (CAP_SYS_ADMIN's projection for
 * these calls) and the right to set credentials (for the child that gives
 * root up). It passes on Linux 6.6 with glibc, as root.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  swapon of a directory is EISDIR, as Linux's open for writing makes
 *       it (10); of a name that does not exist, ENOENT (11); of a character
 *       device, neither a block device nor a regular file, EINVAL (12);
 *       19: not root, so nothing can be checked
 *   2x  swapoff of the same three: EISDIR (20), ENOENT (21), and EINVAL for
 *       what is not swap (22)
 *   3x  a child that is not root is refused both: EPERM (30, 31)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <sys/swap.h>
#include <sys/wait.h>
#include <unistd.h>

static int fails_with(int ret, int err)
{
    return ret == -1 && errno == err;
}

int main(void)
{
    alarm(20);
    if (geteuid() != 0)
        return 19;

    errno = 0;
    if (!fails_with(swapon("/tmp", 0), EISDIR))
        return 10;
    errno = 0;
    if (!fails_with(swapon("/tmp/ctest-swap-no-such-file", 0), ENOENT))
        return 11;
    errno = 0;
    if (!fails_with(swapon("/dev/null", 0), EINVAL))
        return 12;

    errno = 0;
    if (!fails_with(swapoff("/tmp"), EISDIR))
        return 20;
    errno = 0;
    if (!fails_with(swapoff("/tmp/ctest-swap-no-such-file"), ENOENT))
        return 21;
    errno = 0;
    if (!fails_with(swapoff("/dev/null"), EINVAL))
        return 22;

    pid_t child = fork();
    if (child == 0) {
        if (setresuid(1000, 1000, 1000) != 0)
            _exit(9);
        errno = 0;
        if (!fails_with(swapon("/tmp", 0), EPERM))
            _exit(30);
        errno = 0;
        if (!fails_with(swapoff("/tmp"), EPERM))
            _exit(31);
        _exit(0);
    }
    int status = 0;
    if (child < 0 || waitpid(child, &status, 0) != child || !WIFEXITED(status))
        return 30;
    if (WEXITSTATUS(status) != 0)
        return WEXITSTATUS(status) == 31 ? 31 : 30;
    return 42;
}
