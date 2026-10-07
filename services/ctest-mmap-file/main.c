/*
 * ctest-mmap-file -- ring-3 test that mmap of a file gives the file's bytes.
 *
 * Until 2026-10-06 the C library handed a file mapping to the native
 * SYS_MMAP, which has no file to map from: the descriptor went where nothing
 * reads it, and the mapping was anonymous memory -- zeros, and no error. The
 * library now copies the file into fresh memory (posix/src/mman/file_map.rs)
 * until the kernel maps files for native programs
 * (requests/d-a-a-native-program-cannot-map-a-file.md). The decisive check is
 * 12: the bytes are the file's, which zeros are not.
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.  No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a 40000-byte file is written (10); mapped private and read-only
 *       (11), its bytes are the file's (12: they were zeros), and from its
 *       end to the end of the mapping's last page, zeros (13)
 *   2x  mapped from a page-aligned offset (20), the bytes are the file's from
 *       there (21); unmapped (22)
 *   3x  private and writable (30): a write to the mapping is in the mapping
 *       (31) and not in the file (32)
 *   5x  shared and read-only (50), the file's bytes too (51)
 *   6x  refused as Linux refuses, in its order: shared and writable on a
 *       read-only descriptor, EACCES (61); shared and writable on a
 *       read-write one, ENODEV -- this library cannot write it back (62); a
 *       write-only descriptor, EACCES (63); a pipe, ENODEV (64); a directory
 *       (65: it could not be opened), ENODEV (66)
 *   7x  a read-only private file mapping made writable by mprotect (71) and
 *       written; made inaccessible (72)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/mman.h>
#include <unistd.h>

#define PATH "/tmp/ctest-mmap-file.dat"
#define FILE_LEN 40000

/* The file's byte at `i`: not zero at most offsets, and different at each
 * offset of a page, so that bytes from the wrong place do not pass. */
static unsigned char pattern(size_t i)
{
    return (unsigned char)((i * 7 + 3) ^ (i >> 8));
}

/* Whether `n` bytes at `p` are the file's from `from`. */
static int is_file(const unsigned char *p, size_t from, size_t n)
{
    for (size_t i = 0; i < n; i++)
        if (p[i] != pattern(from + i))
            return 0;
    return 1;
}

/* Whether the mapping failed with `want`. */
static int refused(void *p, int want)
{
    return p == MAP_FAILED && errno == want;
}

int main(void)
{
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    size_t span = (FILE_LEN + page - 1) / page * page;
    static unsigned char buf[FILE_LEN];

    /* 1x */
    for (size_t i = 0; i < FILE_LEN; i++)
        buf[i] = pattern(i);
    int w = open(PATH, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (w < 0 || write(w, buf, FILE_LEN) != FILE_LEN || close(w) != 0)
        return 10;
    int rd = open(PATH, O_RDONLY);
    if (rd < 0)
        return 10;
    unsigned char *p = mmap(NULL, span, PROT_READ, MAP_PRIVATE, rd, 0);
    if (p == MAP_FAILED)
        return 11;
    if (!is_file(p, 0, FILE_LEN))
        return 12;
    for (size_t i = FILE_LEN; i < span; i++)
        if (p[i] != 0)
            return 13;
    munmap(p, span);

    /* 2x */
    unsigned char *q = mmap(NULL, page, PROT_READ, MAP_PRIVATE, rd, (off_t)page);
    if (q == MAP_FAILED)
        return 20;
    if (!is_file(q, page, page))
        return 21;
    if (munmap(q, page) != 0)
        return 22;

    /* 3x */
    unsigned char *m = mmap(NULL, page, PROT_READ | PROT_WRITE, MAP_PRIVATE, rd, 0);
    if (m == MAP_FAILED)
        return 30;
    m[0] = (unsigned char)~pattern(0);
    if (m[0] != (unsigned char)~pattern(0))
        return 31;
    unsigned char b = 0;
    if (pread(rd, &b, 1, 0) != 1 || b != pattern(0))
        return 32;
    munmap(m, page);

    /* 5x */
    unsigned char *s = mmap(NULL, page, PROT_READ, MAP_SHARED, rd, 0);
    if (s == MAP_FAILED)
        return 50;
    if (!is_file(s, 0, page))
        return 51;
    munmap(s, page);

    /* 6x */
    errno = 0;
    if (!refused(mmap(NULL, page, PROT_READ | PROT_WRITE, MAP_SHARED, rd, 0), EACCES))
        return 61;
    int rw = open(PATH, O_RDWR);
    errno = 0;
    if (rw < 0 || !refused(mmap(NULL, page, PROT_READ | PROT_WRITE, MAP_SHARED, rw, 0), ENODEV))
        return 62;
    int wo = open(PATH, O_WRONLY);
    errno = 0;
    if (wo < 0 || !refused(mmap(NULL, page, PROT_READ, MAP_PRIVATE, wo, 0), EACCES))
        return 63;
    int pp[2];
    errno = 0;
    if (pipe(pp) != 0 || !refused(mmap(NULL, page, PROT_READ, MAP_PRIVATE, pp[0], 0), ENODEV))
        return 64;
    int dir = open("/tmp", O_RDONLY | O_DIRECTORY);
    if (dir < 0)
        return 65;
    errno = 0;
    if (!refused(mmap(NULL, page, PROT_READ, MAP_PRIVATE, dir, 0), ENODEV))
        return 66;

    /* 7x */
    unsigned char *r = mmap(NULL, page, PROT_READ, MAP_PRIVATE, rd, 0);
    if (r == MAP_FAILED || mprotect(r, page, PROT_READ | PROT_WRITE) != 0)
        return 71;
    r[1] = 0x55;
    if (r[1] != 0x55)
        return 71;
    if (mprotect(r, page, PROT_NONE) != 0)
        return 72;
    munmap(r, page);

    close(dir);
    close(pp[0]);
    close(pp[1]);
    close(wo);
    close(rw);
    close(rd);
    unlink(PATH);
    return 42;
}
