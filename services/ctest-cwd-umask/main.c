/*
 * ctest-cwd-umask -- ring-3 test that a program's working directory and
 * file-creation mask reach the programs it starts (design-decisions.md §960;
 * known-issues.md TD-D-CWD-AND-UMASK-DO-NOT-SURVIVE-EXEC-OR-SPAWN).
 *
 * Until 2026-09-25 both lived only in libc's memory, which a new program image
 * does not inherit: every program a native program exec'd or spawned started
 * in `/` with umask 022, so a shell's `cd` and `umask` were forgotten by every
 * command it ran. The kernel now keeps both as the process's record; libc's
 * `chdir` and `umask` keep it current, and start-up reads it back. This
 * fixture is the parent and the children at once, so the test needs nothing on
 * the image but itself.
 *
 * As the PARENT (no arguments) it moves to `/mnt/tests` with umask 027 and
 * starts itself as a child four ways, telling each child what it should find:
 *
 *   1. fork + execv                       -> /mnt/tests, 027
 *   2. posix_spawn, no file actions       -> /mnt/tests, 027
 *   3. posix_spawn + addchdir_np(/mnt/bin)-> /mnt/bin,   027
 *   4. chdir(".."), then posix_spawn      -> /mnt,       027
 *
 * As a CHILD (`child <dir> <octal-mask>`) it exits 0 if getcwd() is <dir> and
 * the umask is <mask>; otherwise 1 (wrong directory), 2 (wrong mask), 3
 * (getcwd failed) or 4 (bad arguments).
 *
 * Parent exit codes -- 42 is every check passing; anything else names the
 * first that failed:
 *
 *   10  chdir("/mnt/tests") failed
 *   11  getcwd() after it did not say /mnt/tests
 *   12  umask(027) did not return 022, the default a kernel-spawned process
 *       starts with
 *   2x  case 1 (fork + execv), 3x case 2, 4x case 3, 5x case 4, where x is
 *         0  the fork or posix_spawn failed
 *         1  waitpid failed or returned the wrong pid
 *         2  the child did not exit normally
 *         3  the child found the wrong DIRECTORY -- it started in `/`: the
 *            record did not reach it (a kernel without §960, or libc not
 *            reading it at start-up)
 *         4  the child found the wrong MASK -- 022 where 027 was set
 *         5  the child could not run getcwd
 *         6  the child was given bad arguments
 *         7  the child could not be exec'd at all (127): the path below, or a
 *            missing (File, EXECUTE/READ) grant
 *         8  any other child exit code
 *   56  chdir("..") failed (case 4 only)
 *
 * On a kernel without the record, cases 1, 2 and 4 fail with x = 3 -- that is
 * the defect, reproduced -- and case 3 with x = 3 as well: libc then hands the
 * kernel no directory, because an older kernel would refuse the spawn over the
 * field (`posix/src/unistd.rs` `kernel_keeps_cwd`).
 */

#define _GNU_SOURCE
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

/* Where this fixture is at run time: the image stages it at
 * /tests/ctest-cwd-umask.elf and the kernel mounts the image at /mnt. */
#define SELF "/mnt/tests/ctest-cwd-umask.elf"

#define DIR_TESTS "/mnt/tests"
#define DIR_BIN "/mnt/bin"
#define DIR_MNT "/mnt"
#define MASK_SET 027
#define MASK_TEXT "27"

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

/* ---- child ------------------------------------------------------------- */

static int parse_octal(const char *s, mode_t *out)
{
    mode_t v = 0;
    if (*s == '\0') {
        return -1;
    }
    for (; *s != '\0'; s++) {
        if (*s < '0' || *s > '7') {
            return -1;
        }
        v = (mode_t)(v * 8 + (mode_t)(*s - '0'));
        if (v > 0777) {
            return -1;
        }
    }
    *out = v;
    return 0;
}

static int run_child(int argc, char **argv)
{
    mode_t want_mask = 0;
    if (argc != 4 || parse_octal(argv[3], &want_mask) != 0) {
        return 4;
    }
    char cwd[4096];
    if (getcwd(cwd, sizeof cwd) == NULL) {
        return 3;
    }
    if (strcmp(cwd, argv[2]) != 0) {
        return 1;
    }
    mode_t mask = umask(0);
    umask(mask);
    if (mask != want_mask) {
        return 2;
    }
    return 0;
}

/* ---- parent ------------------------------------------------------------ */

/* Turn a reaped child's status into this case's code (see the legend). */
static int verdict(int band, pid_t got, pid_t want, int status)
{
    if (got != want) {
        return band + 1;
    }
    if (!WIFEXITED(status)) {
        return band + 2;
    }
    switch (WEXITSTATUS(status)) {
    case 0:
        return 0;
    case 1:
        return band + 3;
    case 2:
        return band + 4;
    case 3:
        return band + 5;
    case 4:
        return band + 6;
    case 127:
        return band + 7;
    default:
        return band + 8;
    }
}

static int case_fork_exec(const char *want_dir)
{
    char *args[] = {"ctest-cwd-umask", "child", (char *)want_dir, MASK_TEXT, NULL};
    pid_t pid = fork();
    if (pid < 0) {
        return 20;
    }
    if (pid == 0) {
        execv(SELF, args);
        _exit(127);
    }
    int status = 0;
    pid_t got = waitpid(pid, &status, 0);
    return verdict(20, got, pid, status);
}

static int case_spawn(int band, const char *chdir_to, const char *want_dir)
{
    char *args[] = {"ctest-cwd-umask", "child", (char *)want_dir, MASK_TEXT, NULL};
    posix_spawn_file_actions_t acts;
    posix_spawn_file_actions_t *actsp = NULL;
    if (chdir_to != NULL) {
        if (posix_spawn_file_actions_init(&acts) != 0 ||
            posix_spawn_file_actions_addchdir_np(&acts, chdir_to) != 0) {
            return band;
        }
        actsp = &acts;
    }
    pid_t pid = -1;
    int rc = posix_spawn(&pid, SELF, actsp, NULL, args, environ);
    if (actsp != NULL) {
        posix_spawn_file_actions_destroy(actsp);
    }
    if (rc != 0) {
        return band;
    }
    int status = 0;
    pid_t got = waitpid(pid, &status, 0);
    return verdict(band, got, pid, status);
}

static int run_parent(void)
{
    emit("[cw] chdir /mnt/tests, umask 027\n");
    if (chdir(DIR_TESTS) != 0) {
        return 10;
    }
    char cwd[4096];
    if (getcwd(cwd, sizeof cwd) == NULL || strcmp(cwd, DIR_TESTS) != 0) {
        return 11;
    }
    if (umask(MASK_SET) != 022) {
        return 12;
    }

    int rc;
    emit("[cw] 1 fork + execv (expect /mnt/tests, 027)\n");
    if ((rc = case_fork_exec(DIR_TESTS)) != 0) {
        return rc;
    }
    emit("[cw] 2 posix_spawn (expect /mnt/tests, 027)\n");
    if ((rc = case_spawn(30, NULL, DIR_TESTS)) != 0) {
        return rc;
    }
    emit("[cw] 3 posix_spawn + addchdir_np /mnt/bin (expect /mnt/bin, 027)\n");
    if ((rc = case_spawn(40, DIR_BIN, DIR_BIN)) != 0) {
        return rc;
    }
    emit("[cw] 4 chdir .., posix_spawn (expect /mnt, 027)\n");
    if (chdir("..") != 0) {
        return 56;
    }
    if ((rc = case_spawn(50, NULL, DIR_MNT)) != 0) {
        return rc;
    }
    emit("[cw] ok (all four children started where their parent said)\n");
    return 42;
}

int main(int argc, char **argv)
{
    if (argc > 1 && strcmp(argv[1], "child") == 0) {
        return run_child(argc, argv);
    }
    return run_parent();
}
