/*
 * ctest-groups -- ring-3 test that getgroups reports the supplementary
 * groups the kernel keeps, and that a program's name cannot forge them.
 *
 * Until 2026-10-06 the C library's getgroups answered "no groups" for every
 * process, while setgroups and initgroups installed real ones in the kernel,
 * whose file access gate consults them.  It now reads the kernel's list from
 * /proc/self/status -- the last Groups: line, because the first line is the
 * task's name, written raw, and a name holding a newline makes a Groups:
 * line of its own (requests/d-a-proc-status-writes-the-name-raw-so-a-
 * newline-in-it-forges-lines.md).  Check 6x runs a copy of this program
 * under such a name.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check: the tens digit names the check, the units the step.  No check is
 * numbered 4, so that no failure reads as 42.
 *
 *   1x  setgroups({30, 10, 20}) refused (11) -- the fixture needs
 *       (Process, SET_CREDENTIALS), the generic rung's `creds` grant
 *   2x  read back: getgroups(0, NULL) is 3 (21); getgroups(3, list) is 3
 *       (22), in ascending order as Linux keeps them (23); a list of 2 is
 *       EINVAL (24); a size of -1 is EINVAL (25); a NULL list is EFAULT
 *       (26); a list of 8 gets 3 and the rest stays as it was (27)
 *   3x  group_member: 20 is a member (31), 40 is not (32)
 *   5x  setgroups(0, NULL) drops them: refused (51), getgroups still
 *       counts some (52), group_member(20) still 1 (53); reinstalling
 *       {30, 10, 20} refused (54)
 *   6x  the forged name: this program copied to /tmp under the name
 *       "x\nGroups:\t99 98" -- the copy could not be made (61), execve of it
 *       failed (62); in the copy, run with argv[1] "forged": getgroups is
 *       not {10, 20, 30} (63), group_member(99) is 1 (64), and
 *       /proc/self/status holds neither the forged line nor the name escaped
 *       as Linux escapes it, so the check proved nothing (65)
 *
 * Nothing in it waits.  It needs the `file` grant too: /proc/self/status is
 * a file, and so is the copy.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <stddef.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* GNU's; musl's headers, which the sysroot compiles against, lack it. */
int group_member(gid_t gid);

/* 15 bytes, a whole task name: "Groups:\t99 98" is a line of its own. */
static const char FORGED_NAME[] = "x\nGroups:\t99 98";
static const char COPY_DIR[] = "/tmp/";

static int sorted_is_10_20_30(const gid_t *g, int n)
{
    return n == 3 && g[0] == 10 && g[1] == 20 && g[2] == 30;
}

/* How many lines of /proc/self/status start with "Groups:", and whether its
 * first line holds the name escaped as Linux escapes it ("x\\nGroups:"). */
static int status_shape(int *escaped)
{
    char buf[4096];
    size_t len = 0;
    int fd = open("/proc/self/status", O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return -1;
    for (;;) {
        ssize_t n = read(fd, buf + len, sizeof buf - 1 - len);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0)
            break;
        len += (size_t)n;
        if (len == sizeof buf - 1)
            break;
    }
    close(fd);
    buf[len] = '\0';
    int lines = 0;
    for (size_t i = 0; i < len; i++)
        if ((i == 0 || buf[i - 1] == '\n') && strncmp(buf + i, "Groups:", 7) == 0)
            lines++;
    const char *nl = memchr(buf, '\n', len);
    size_t first = nl ? (size_t)(nl - buf) : len;
    *escaped = 0;
    for (size_t i = 0; i + 10 <= first; i++)
        if (memcmp(buf + i, "x\\nGroups:", 10) == 0)
            *escaped = 1;
    return lines;
}

/* Run as the forged copy: the checks of 6x that need the name. */
static int forged(const char *self)
{
    /* The copy has done its part once it is running. */
    unlink(self);

    gid_t g[8];
    int n = getgroups(8, g);
    if (!sorted_is_10_20_30(g, n))
        return 63;
    if (group_member(99) != 0)
        return 64;
    int escaped = 0;
    int lines = status_shape(&escaped);
    if (lines != 2 && !(lines == 1 && escaped))
        return 65;
    return 42;
}

/* Copy the file at `from` to `to`, executable.  0, or -1. */
static int copy_file(const char *from, const char *to)
{
    int in = open(from, O_RDONLY | O_CLOEXEC);
    if (in < 0)
        return -1;
    int out = open(to, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0755);
    if (out < 0) {
        close(in);
        return -1;
    }
    char buf[16384];
    int ok = 0;
    for (;;) {
        ssize_t n = read(in, buf, sizeof buf);
        if (n < 0 && errno == EINTR)
            continue;
        if (n < 0) {
            ok = -1;
            break;
        }
        if (n == 0)
            break;
        for (ssize_t done = 0; done < n;) {
            ssize_t w = write(out, buf + done, (size_t)(n - done));
            if (w < 0 && errno == EINTR)
                continue;
            if (w <= 0) {
                ok = -1;
                break;
            }
            done += w;
        }
        if (ok != 0)
            break;
    }
    close(in);
    if (close(out) != 0)
        ok = -1;
    if (ok == 0 && chmod(to, 0755) != 0)
        ok = -1;
    return ok;
}

int main(int argc, char **argv)
{
    if (argc >= 2 && strcmp(argv[1], "forged") == 0)
        return forged(argv[0][0] == '/' ? argv[0] : "/tmp/x\nGroups:\t99 98");

    const gid_t install[3] = {30, 10, 20};
    gid_t g[8];

    /* 1x */
    if (setgroups(3, install) != 0)
        return 11;

    /* 2x */
    if (getgroups(0, NULL) != 3)
        return 21;
    if (getgroups(3, g) != 3)
        return 22;
    if (!sorted_is_10_20_30(g, 3))
        return 23;
    errno = 0;
    if (getgroups(2, g) != -1 || errno != EINVAL)
        return 24;
    errno = 0;
    if (getgroups(-1, g) != -1 || errno != EINVAL)
        return 25;
    errno = 0;
    if (getgroups(3, NULL) != -1 || errno != EFAULT)
        return 26;
    for (int i = 0; i < 8; i++)
        g[i] = 77;
    if (getgroups(8, g) != 3 || !sorted_is_10_20_30(g, 3))
        return 27;
    for (int i = 3; i < 8; i++)
        if (g[i] != 77)
            return 27;

    /* 3x */
    if (group_member(20) != 1)
        return 31;
    if (group_member(40) != 0)
        return 32;

    /* 5x */
    if (setgroups(0, NULL) != 0)
        return 51;
    if (getgroups(0, NULL) != 0)
        return 52;
    if (group_member(20) != 0)
        return 53;
    if (setgroups(3, install) != 0)
        return 54;

    /* 6x: this program, copied under a name that forges a Groups: line. */
    char self[512];
    ssize_t len = readlink("/proc/self/exe", self, sizeof self - 1);
    if (len > 0) {
        self[len] = '\0';
    } else {
        strcpy(self, "/mnt/tests/ctest-groups.elf");
    }
    char copy[64];
    strcpy(copy, COPY_DIR);
    strcat(copy, FORGED_NAME);
    if (copy_file(self, copy) != 0) {
        unlink(copy);
        return 61;
    }
    /* argv[0] is the path, so the copy can remove itself, and its last
     * component is the forged name, whichever of the two the kernel names
     * the task from. */
    char *const args[] = {copy, (char *)"forged", NULL};
    char *const env[] = {NULL};
    execve(copy, args, env);
    unlink(copy);
    return 62;
}
