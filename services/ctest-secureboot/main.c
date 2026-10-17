/*
 * ctest-secureboot -- ring-3 test of the Secure Boot lists' granted arm:
 * enrolling, verifying and removing entries through lane A's three calls
 * (1082-1084), as a process holding the right to change them sees them.
 *
 * The kernel's own boot tests show the calls refusing a caller without the
 * right; a kernel test has no process, so it cannot show them granting --
 * "the gate refuses everyone" and "the gate works" look the same from there
 * (requests/a-d-a-c-fixture-for-the-secure-boot-doors.md). The rung grants
 * this fixture `secureboot` (ENROLL_SECUREBOOT) and `file`, for
 * /proc/secureboot. It changes the system's live table, so it works on a
 * fingerprint of its own run and removes every entry it made, whatever
 * fails. The state is Disabled from userspace: nothing here enforces.
 *
 * SlateOS's alone: Linux keeps these lists in the firmware.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a fingerprint in no list: unlisted, decided by nothing, not
 *       enforced, may run (10)
 *   2x  enrolled in db: an id (20); again, uppercase and without "SHA256:",
 *       is EEXIST (21)
 *   3x  verified: allowed, by that entry (30)
 *   5x  enrolled in dbx too: a second id (50); verified: forbidden, by the
 *       dbx entry, which wins over db (51)
 *   6x  /proc/secureboot: the state Disabled (60), not enforcing (61), the
 *       fingerprint, lowercase with its prefix, listed twice (62)
 *   7x  the dbx entry removed: allowed again, by the db entry (70); removed
 *       again, ENOENT (71)
 *   8x  EINVAL: a list that is none (80), a fingerprint that is none (81),
 *       an empty subject (82)
 *   9x  a name that is not UTF-8 is taken (90)
 *  10x  (exit 100-109) the db entry removed (100): unlisted again (101)
 */

#define _GNU_SOURCE
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <slateos/secureboot.h>
#include <time.h>
#include <unistd.h>

static long db_id = -1, dbx_id = -1;

/* Leave the table as it was found, then fail with `code`. */
static int fail(int code)
{
    if (dbx_id >= 0)
        secureboot_remove((unsigned long)dbx_id);
    if (db_id >= 0)
        secureboot_remove((unsigned long)db_id);
    return code;
}

static int verdict_is(const struct secureboot_verdict *v, unsigned listing, long entry)
{
    return v->listing == listing && (long)v->entry == entry && v->enforced == 0 && v->may_run == 1;
}

int main(void)
{
    alarm(20);
    /* A fingerprint of this run's own: 64 hex digits from the pid and the
     * time, then a fixed tail. */
    char digits[65];
    snprintf(digits, sizeof digits, "%08x%08x%s", (unsigned)getpid(), (unsigned)time(NULL),
             "5ec0b0075ec0b0075ec0b0075ec0b0075ec0b0075ec0b007");
    char fp[80], upper[65];
    snprintf(fp, sizeof fp, "SHA256:%s", digits);
    for (int i = 0; i < 65; i++)
        upper[i] = (char)toupper((unsigned char)digits[i]);
    const char *subject = "ctest-secureboot";

    struct secureboot_verdict v;
    memset(&v, 0xff, sizeof v);
    if (secureboot_verify("probe.efi", 9, fp, strlen(fp), &v) != 0 || !verdict_is(&v, SECUREBOOT_UNLISTED, 0))
        return fail(10);

    db_id = secureboot_enroll(SECUREBOOT_DB, subject, strlen(subject), fp, strlen(fp));
    if (db_id <= 0)
        return fail(20);
    errno = 0;
    if (secureboot_enroll(SECUREBOOT_DB, subject, strlen(subject), upper, 64) != -1 || errno != EEXIST)
        return fail(21);

    if (secureboot_verify("probe.efi", 9, fp, strlen(fp), &v) != 0 || !verdict_is(&v, SECUREBOOT_ALLOWED, db_id))
        return fail(30);

    dbx_id = secureboot_enroll(SECUREBOOT_DBX, subject, strlen(subject), fp, strlen(fp));
    if (dbx_id <= 0 || dbx_id == db_id)
        return fail(50);
    if (secureboot_verify("probe.efi", 9, fp, strlen(fp), &v) != 0 || !verdict_is(&v, SECUREBOOT_FORBIDDEN, dbx_id))
        return fail(51);

    static char text[16384];
    int fd = open("/proc/secureboot", O_RDONLY | O_CLOEXEC);
    ssize_t n = fd < 0 ? -1 : read(fd, text, sizeof text - 1);
    if (fd >= 0)
        close(fd);
    if (n <= 0)
        return fail(60);
    text[n] = 0;
    if (!strstr(text, "state: Disabled\n"))
        return fail(60);
    if (!strstr(text, "enforcing: no\n"))
        return fail(61);
    const char *first = strstr(text, fp);
    if (!first || !strstr(first + 1, fp))
        return fail(62);

    if (secureboot_remove((unsigned long)dbx_id) != 0)
        return fail(70);
    long gone = dbx_id;
    dbx_id = -1;
    if (secureboot_verify("probe.efi", 9, fp, strlen(fp), &v) != 0 || !verdict_is(&v, SECUREBOOT_ALLOWED, db_id))
        return fail(70);
    errno = 0;
    if (secureboot_remove((unsigned long)gone) != -1 || errno != ENOENT)
        return fail(71);

    errno = 0;
    if (secureboot_enroll(5, subject, strlen(subject), fp, strlen(fp)) != -1 || errno != EINVAL)
        return fail(80);
    errno = 0;
    if (secureboot_enroll(SECUREBOOT_DBX, subject, strlen(subject), "SHA256:zz", 9) != -1 || errno != EINVAL)
        return fail(81);
    errno = 0;
    if (secureboot_enroll(SECUREBOOT_DBX, subject, 0, fp, strlen(fp)) != -1 || errno != EINVAL)
        return fail(82);

    if (secureboot_verify("probe-\xff.efi", 11, fp, strlen(fp), &v) != 0 || !verdict_is(&v, SECUREBOOT_ALLOWED, db_id))
        return fail(90);

    if (secureboot_remove((unsigned long)db_id) != 0)
        return fail(100);
    db_id = -1;
    if (secureboot_verify("probe.efi", 9, fp, strlen(fp), &v) != 0 || !verdict_is(&v, SECUREBOOT_UNLISTED, 0))
        return fail(101);
    return 42;
}
