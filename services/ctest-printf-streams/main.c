/*
 * ctest-printf-streams -- ring-3 test of the printf paths that write to a real
 * stream, which the host tests in posix/ cannot reach (posix/src/printf.rs):
 *
 *   1. fprintf and dprintf write ALL of a long output. Until 2026-09-26 the
 *      printf family formatted into a 4096-byte stack buffer and wrote at
 *      most that much, while returning the full count.
 *   2. fwprintf and wprintf, which this libc lacked until 2026-09-26: the
 *      bytes on the stream are the UTF-8 of the wide output, and the return
 *      value counts wide characters, not bytes.
 *   3. swprintf into a buffer that is not zeroed. Until 2026-09-26 vswprintf
 *      read its own formatted output as a string without terminating it, so a
 *      buffer holding anything but zeros made it miscount or fail.
 *
 * Every stream check writes into a pipe whose other end a forked child reads
 * to EOF and compares; the child's exit status is its verdict, so the pipe's
 * capacity never matters.
 *
 * Exit codes -- 42 is every check passing; otherwise the first failure:
 *   10  a pipe, fork or fdopen failed
 *   11  fprintf of 10000 bytes did not return 10000
 *   12  the 10000 bytes did not all arrive, in order, at the far end
 *   13  dprintf of 10000 bytes did not return 10000
 *   14  the dprintf bytes did not all arrive, in order
 *   15  fwprintf(L"%ls-%d", L"é€", 7) did not return 4 wide characters
 *   16  its bytes on the stream were not the 7 of UTF-8 "é€-7"
 *   17  wprintf, to a standard output that is a pipe, did not return 2
 *   18  its bytes were not the UTF-8 of "éx"
 *   19  swprintf into a 0x5a5a-filled buffer: wrong count or wrong text
 */

#include <stdio.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>

#define BIG 10000

static char big[BIG + 1];

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

/* In a child: read `fd` to EOF and compare it with `want` (`len` bytes).
 * Exits 0 on a match, 1 otherwise; never returns. */
static void read_and_compare(int fd, const char *want, size_t len)
{
    static char got[BIG + 64];
    size_t n = 0;
    for (;;) {
        ssize_t r = read(fd, got + n, sizeof got - n);
        if (r <= 0) {
            break;
        }
        n += (size_t)r;
        if (n == sizeof got) {
            break;
        }
    }
    _exit(n == len && memcmp(got, want, len) == 0 ? 0 : 1);
}

/* Start a reader child on a new pipe. Returns its pid and leaves the write
 * end in *wfd, or returns -1. */
static pid_t start_reader(int *wfd, const char *want, size_t len)
{
    int fds[2];
    if (pipe(fds) != 0) {
        return -1;
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(fds[0]);
        close(fds[1]);
        return -1;
    }
    if (pid == 0) {
        close(fds[1]);
        read_and_compare(fds[0], want, len);
    }
    close(fds[0]);
    *wfd = fds[1];
    return pid;
}

/* 0 if the reader child matched, 1 otherwise. */
static int reader_verdict(pid_t pid)
{
    int status = 0;
    if (waitpid(pid, &status, 0) != pid) {
        return 1;
    }
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 1;
}

static int long_fprintf(void)
{
    int wfd = -1;
    pid_t pid = start_reader(&wfd, big, BIG);
    if (pid < 0) {
        return 10;
    }
    FILE *fp = fdopen(wfd, "w");
    if (fp == NULL) {
        close(wfd);
        (void)reader_verdict(pid);
        return 10;
    }
    int n = fprintf(fp, "%s", big);
    fclose(fp);
    int bad = reader_verdict(pid);
    if (n != BIG) {
        return 11;
    }
    return bad ? 12 : 0;
}

static int long_dprintf(void)
{
    int wfd = -1;
    pid_t pid = start_reader(&wfd, big, BIG);
    if (pid < 0) {
        return 10;
    }
    int n = dprintf(wfd, "%s", big);
    close(wfd);
    int bad = reader_verdict(pid);
    if (n != BIG) {
        return 13;
    }
    return bad ? 14 : 0;
}

static int wide_fwprintf(void)
{
    static const char want[] = "\xc3\xa9\xe2\x82\xac-7";
    int wfd = -1;
    pid_t pid = start_reader(&wfd, want, sizeof want - 1);
    if (pid < 0) {
        return 10;
    }
    FILE *fp = fdopen(wfd, "w");
    if (fp == NULL) {
        close(wfd);
        (void)reader_verdict(pid);
        return 10;
    }
    int n = fwprintf(fp, L"%ls-%d", L"é€", 7);
    fclose(fp);
    int bad = reader_verdict(pid);
    if (n != 4) {
        return 15;
    }
    return bad ? 16 : 0;
}

/* wprintf goes to standard output, so it runs in a child whose standard
 * output is the pipe; that child reports wprintf's count through its exit
 * status, and a reader child checks the bytes. */
static int wide_wprintf(void)
{
    static const char want[] = "\xc3\xa9x";
    int wfd = -1;
    pid_t reader = start_reader(&wfd, want, sizeof want - 1);
    if (reader < 0) {
        return 10;
    }
    pid_t writer = fork();
    if (writer < 0) {
        close(wfd);
        (void)reader_verdict(reader);
        return 10;
    }
    if (writer == 0) {
        dup2(wfd, 1);
        close(wfd);
        int n = wprintf(L"%lcx", (wint_t)0xe9);
        fflush(stdout);
        _exit(n == 2 ? 0 : 1);
    }
    close(wfd);
    int status = 0;
    int count_ok = waitpid(writer, &status, 0) == writer && WIFEXITED(status)
                   && WEXITSTATUS(status) == 0;
    int bad = reader_verdict(reader);
    if (!count_ok) {
        return 17;
    }
    return bad ? 18 : 0;
}

static int swprintf_unzeroed(void)
{
    wchar_t buf[16];
    for (int k = 0; k < 16; k++) {
        buf[k] = 0x5a5a;
    }
    if (swprintf(buf, 16, L"n=%d", 42) != 4 || wcscmp(buf, L"n=42") != 0) {
        return 19;
    }
    return 0;
}

int main(void)
{
    for (int i = 0; i < BIG; i++) {
        big[i] = (char)('a' + i % 26);
    }
    big[BIG] = '\0';

    emit("[ps] fprintf and dprintf of 10000 bytes\n");
    int rc = long_fprintf();
    if (rc != 0) {
        return rc;
    }
    rc = long_dprintf();
    if (rc != 0) {
        return rc;
    }
    emit("[ps] fwprintf and wprintf\n");
    rc = wide_fwprintf();
    if (rc != 0) {
        return rc;
    }
    rc = wide_wprintf();
    if (rc != 0) {
        return rc;
    }
    emit("[ps] swprintf into a buffer that is not zeroed\n");
    rc = swprintf_unzeroed();
    if (rc != 0) {
        return rc;
    }
    emit("[ps] ok (long fprintf and dprintf whole; fwprintf, wprintf and swprintf right)\n");
    return 42;
}
