/*
 * ctest-stdio -- ring-3 test of the C library's streams over real file
 * descriptors, which the host tests in posix/ cannot reach: they drive the
 * buffering through in-memory streams (fopencookie), and every descriptor
 * call on the host is a stub.  posix/src/stdio.rs was rewritten on
 * 2026-09-27 (design-decisions.md §1121); this checks, on the kernel, the
 * behaviour the rewrite changed:
 *
 *   11  fopen(path, "wx") on a file that exists must fail with EEXIST, not
 *       truncate it (the old parser ignored 'x')
 *   12  fopen "re" sets FD_CLOEXEC on the descriptor; "r" does not
 *   13  a hundred streams open at once (the old pool had sixteen)
 *   14  fread of 8000 bytes from a pipe that delivers them in pieces returns
 *       8000 (the old fread returned after the first short read)
 *   15  fdopen refuses a descriptor it cannot use: "w" on a read-only one is
 *       EINVAL, a closed one EBADF
 *   16  a stream opened "a" starts at the end: ftell is the file's size
 *   17  writing a read-only stream fails at once: EOF, EBADF, ferror
 *   18  on an "r+" stream a write after a read lands at the stream's
 *       position, not after the read-ahead
 *   19  two threads writing lines to one stream never tear a line
 *   20  getline reads a line longer than the buffer, whole
 *   21  freopen(NULL, "r", f) gives back a working stream
 *   22  popen("echo ...", "r"): the line comes through, pclose is exit 0
 *   23  popen("exit 3", "r"): pclose's status says 3
 *
 * 22 and 23 need /bin/sh.  The boot test mounts the image at /mnt, so a
 * running process may have no /bin/sh at all (requests/
 * d-ab-the-booted-system-has-no-bin-sh.md); they are then reported as not
 * checked, and do not fail the run.
 *
 * Exit codes -- 42 is every check passing; 10 is a failed setup (a file in
 * /tmp could not be made); otherwise the number of the first check to fail.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define FILE_A "/tmp/ctest-stdio-a"
#define FILE_B "/tmp/ctest-stdio-b"
#define FILE_T "/tmp/ctest-stdio-threads"
#define FILE_L "/tmp/ctest-stdio-long"

static void say(const char *s)
{
    size_t n = strlen(s);
    while (n != 0) {
        ssize_t w = write(2, s, n);
        if (w <= 0)
            return;
        s += w;
        n -= (size_t)w;
    }
}

static int fail(int code, const char *why)
{
    say("[ctest-stdio] FAIL: ");
    say(why);
    say("\n");
    return code;
}

/* Write `data` to `path` with plain descriptor calls: 0, or -1. */
static int put_file(const char *path, const char *data, size_t len)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0)
        return -1;
    ssize_t w = write(fd, data, len);
    close(fd);
    return w == (ssize_t)len ? 0 : -1;
}

/* Read up to `cap` bytes of `path` into `buf`: the count, or -1. */
static ssize_t get_file(const char *path, char *buf, size_t cap)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0)
        return -1;
    size_t n = 0;
    for (;;) {
        ssize_t r = read(fd, buf + n, cap - n);
        if (r <= 0)
            break;
        n += (size_t)r;
        if (n == cap)
            break;
    }
    close(fd);
    return (ssize_t)n;
}

static int check_wx(void)
{
    if (put_file(FILE_A, "keep", 4) != 0)
        return 10;
    errno = 0;
    FILE *f = fopen(FILE_A, "wx");
    if (f != NULL) {
        fclose(f);
        return fail(11, "fopen \"wx\" opened a file that exists");
    }
    if (errno != EEXIST)
        return fail(11, "fopen \"wx\" failed, but not with EEXIST");
    char buf[8];
    if (get_file(FILE_A, buf, sizeof buf) != 4 || memcmp(buf, "keep", 4) != 0)
        return fail(11, "fopen \"wx\" changed the file it refused");
    return 0;
}

static int check_cloexec(void)
{
    FILE *e = fopen(FILE_A, "re");
    FILE *r = fopen(FILE_A, "r");
    if (e == NULL || r == NULL)
        return fail(12, "fopen \"re\" or \"r\" failed");
    int fe = fcntl(fileno(e), F_GETFD);
    int fr = fcntl(fileno(r), F_GETFD);
    fclose(e);
    fclose(r);
    if (fe < 0 || !(fe & FD_CLOEXEC))
        return fail(12, "fopen \"re\" did not set FD_CLOEXEC");
    if (fr < 0 || (fr & FD_CLOEXEC))
        return fail(12, "fopen \"r\" set FD_CLOEXEC");
    return 0;
}

static int check_hundred(void)
{
    FILE *fs[100];
    int opened = 0;
    int code = 0;
    for (; opened < 100; opened++) {
        fs[opened] = fopen(FILE_A, "r");
        if (fs[opened] == NULL) {
            code = fail(13, "the hundred streams: an fopen failed");
            break;
        }
    }
    for (int i = 0; i < opened; i++)
        fclose(fs[i]);
    return code;
}

static int check_pipe_fread(void)
{
    int p[2];
    if (pipe(p) != 0)
        return 10;
    pid_t pid = fork();
    if (pid < 0)
        return 10;
    if (pid == 0) {
        close(p[0]);
        char chunk[1000];
        for (int i = 0; i < 8; i++) {
            memset(chunk, 'a' + i, sizeof chunk);
            if (write(p[1], chunk, sizeof chunk) != (ssize_t)sizeof chunk)
                _exit(1);
            usleep(2000);
        }
        _exit(0);
    }
    close(p[1]);
    FILE *f = fdopen(p[0], "r");
    if (f == NULL)
        return 10;
    static char buf[8000];
    size_t got = fread(buf, 1, sizeof buf, f);
    int status = 0;
    waitpid(pid, &status, 0);
    fclose(f);
    if (got != sizeof buf)
        return fail(14, "fread from a pipe returned short");
    for (int i = 0; i < 8; i++)
        if (buf[i * 1000] != 'a' + i || buf[i * 1000 + 999] != 'a' + i)
            return fail(14, "fread from a pipe: the bytes are not in order");
    return 0;
}

static int check_fdopen(void)
{
    int fd = open(FILE_A, O_RDONLY);
    if (fd < 0)
        return 10;
    errno = 0;
    FILE *f = fdopen(fd, "w");
    int e = errno;
    if (f != NULL) {
        fclose(f);
        return fail(15, "fdopen \"w\" took a read-only descriptor");
    }
    close(fd);
    if (e != EINVAL)
        return fail(15, "fdopen \"w\" on a read-only descriptor: not EINVAL");
    errno = 0;
    f = fdopen(fd, "r"); /* closed now */
    if (f != NULL || errno != EBADF)
        return fail(15, "fdopen of a closed descriptor: not EBADF");
    return 0;
}

static int check_append_tell(void)
{
    if (put_file(FILE_B, "12345", 5) != 0)
        return 10;
    FILE *f = fopen(FILE_B, "a");
    if (f == NULL)
        return 10;
    long at = ftell(f);
    fclose(f);
    if (at != 5)
        return fail(16, "a stream opened \"a\" does not start at the end");
    return 0;
}

static int check_readonly_write(void)
{
    FILE *f = fopen(FILE_A, "r");
    if (f == NULL)
        return 10;
    errno = 0;
    int r = fputc('x', f);
    int e = errno;
    int err = ferror(f);
    fclose(f);
    if (r != EOF || e != EBADF || !err)
        return fail(17, "fputc on a read-only stream did not fail at once with EBADF");
    return 0;
}

static int check_read_then_write(void)
{
    if (put_file(FILE_B, "abcdef", 6) != 0)
        return 10;
    FILE *f = fopen(FILE_B, "r+");
    if (f == NULL)
        return 10;
    int c = fgetc(f);
    int w = fputc('X', f);
    fclose(f);
    char buf[8];
    if (c != 'a' || w != 'X' || get_file(FILE_B, buf, sizeof buf) != 6)
        return fail(18, "read then write on \"r+\": a call failed");
    if (memcmp(buf, "aXcdef", 6) != 0)
        return fail(18, "read then write on \"r+\": the write landed in the wrong place");
    return 0;
}

#define LINES 400
#define LINE_LEN 40

static FILE *shared;

static void *writer(void *arg)
{
    char line[LINE_LEN + 1];
    memset(line, *(const char *)arg, LINE_LEN - 1);
    line[LINE_LEN - 1] = '\n';
    line[LINE_LEN] = 0;
    for (int i = 0; i < LINES; i++)
        if (fputs(line, shared) == EOF)
            return (void *)1;
    return NULL;
}

static int check_threads(void)
{
    shared = fopen(FILE_T, "w");
    if (shared == NULL)
        return 10;
    pthread_t a, b;
    static char ca = 'p', cb = 'q';
    if (pthread_create(&a, NULL, writer, &ca) != 0)
        return 10;
    if (pthread_create(&b, NULL, writer, &cb) != 0)
        return 10;
    void *ra, *rb;
    pthread_join(a, &ra);
    pthread_join(b, &rb);
    fclose(shared);
    if (ra != NULL || rb != NULL)
        return fail(19, "two threads: an fputs failed");
    static char buf[2 * LINES * LINE_LEN + 16];
    ssize_t n = get_file(FILE_T, buf, sizeof buf);
    if (n != 2 * LINES * LINE_LEN)
        return fail(19, "two threads: the file is not every line");
    for (ssize_t at = 0; at < n; at += LINE_LEN) {
        char first = buf[at];
        if (first != 'p' && first != 'q')
            return fail(19, "two threads: a line starts wrong");
        for (int i = 1; i < LINE_LEN - 1; i++)
            if (buf[at + i] != first)
                return fail(19, "two threads: a torn line");
        if (buf[at + LINE_LEN - 1] != '\n')
            return fail(19, "two threads: a line without its newline");
    }
    return 0;
}

static int check_long_getline(void)
{
    static char data[10001];
    memset(data, 'L', 10000);
    data[10000] = '\n';
    if (put_file(FILE_L, data, sizeof data) != 0)
        return 10;
    FILE *f = fopen(FILE_L, "r");
    if (f == NULL)
        return 10;
    char *line = NULL;
    size_t cap = 0;
    ssize_t n = getline(&line, &cap, f);
    int ok = n == 10001 && line != NULL && line[0] == 'L' && line[9999] == 'L'
             && line[10000] == '\n' && line[10001] == 0;
    ssize_t more = getline(&line, &cap, f);
    free(line);
    fclose(f);
    if (!ok || more != -1)
        return fail(20, "getline of a 10000-byte line");
    return 0;
}

static int check_freopen_null(void)
{
    if (put_file(FILE_B, "xyz", 3) != 0)
        return 10;
    FILE *f = fopen(FILE_B, "r");
    if (f == NULL)
        return 10;
    int c1 = fgetc(f);
    FILE *g = freopen(NULL, "r", f);
    if (g == NULL)
        return fail(21, "freopen(NULL, \"r\", f) failed on a file");
    int c2 = fgetc(g);
    fclose(g);
    if (c1 != 'x' || (c2 != 'x' && c2 != 'y'))
        return fail(21, "freopen(NULL, \"r\", f): the stream does not read the file");
    say(c2 == 'x' ? "[ctest-stdio] freopen(NULL) reopened the file (position reset, glibc's way)\n"
                  : "[ctest-stdio] freopen(NULL) kept the descriptor (position kept: the in-place fallback)\n");
    return 0;
}

static int check_popen(void)
{
    if (access("/bin/sh", X_OK) != 0) {
        say("[ctest-stdio] popen NOT CHECKED: this process has no /bin/sh\n");
        return 0;
    }
    FILE *p = popen("echo hello", "r");
    if (p == NULL)
        return fail(22, "popen(\"echo hello\", \"r\") failed");
    char buf[32] = {0};
    char *got = fgets(buf, sizeof buf, p);
    int status = pclose(p);
    if (got == NULL || strcmp(buf, "hello\n") != 0)
        return fail(22, "popen: the line did not come through");
    if (status == -1 || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return fail(22, "popen: pclose did not report exit 0");
    p = popen("exit 3", "r");
    if (p == NULL)
        return fail(23, "popen(\"exit 3\", \"r\") failed");
    status = pclose(p);
    if (status == -1 || !WIFEXITED(status) || WEXITSTATUS(status) != 3)
        return fail(23, "popen: pclose did not report exit 3");
    return 0;
}

int main(void)
{
    int (*checks[])(void) = {
        check_wx, check_cloexec, check_hundred, check_pipe_fread, check_fdopen,
        check_append_tell, check_readonly_write, check_read_then_write,
        check_threads, check_long_getline, check_freopen_null, check_popen,
    };
    for (size_t i = 0; i < sizeof checks / sizeof checks[0]; i++) {
        int code = checks[i]();
        if (code != 0)
            return code;
    }
    unlink(FILE_A);
    unlink(FILE_B);
    unlink(FILE_T);
    unlink(FILE_L);
    say("[ctest-stdio] all checks passed\n");
    return 42;
}
