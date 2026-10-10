/*
 * ctest-llvm-tools -- do LLVM's opt, llc and ld.lld run on SlateOS, and make
 * a program between them?
 *
 * scripts/llvm-spike/ cross-builds LLVM 20.1.8's three tools against
 * SlateOS's own libc.a, and the rootfs recipe stages them at /bin/opt,
 * /bin/llc and /bin/ld.lld, with that libc.a at /usr/lib/x86_64-slateos/.
 * They are the back half of the compiler fastpy needs on SlateOS
 * (requests/b-d-fastpy-on-slateos-needs-llvm-tools.md): fastpy writes LLVM IR
 * and runs these to make a program of it. A clean link says nothing is
 * missing; only here do they start, read their arguments, write files and
 * make something that runs.
 *
 * The checks, in order:
 *
 *   1x  opt --version prints "LLVM version 20.1.8", and exits 0
 *   2x  llc --version prints it too, with x86-64 among its targets
 *   3x  ld.lld --version prints "LLD 20.1.8"
 *   5x  a program from IR: a function `main` that returns 42, written as
 *       LLVM IR to /tmp, through opt -passes='default<O2>', llc -O2
 *       -filetype=obj and ld.lld -static -e _start ... -lc against the
 *       image's libc.a -- and the program it makes, run, exits 42
 *
 * There is no check 4, so no failure is 42.
 *
 * /mnt/bin, not /bin: the kernel mounts the image at /mnt
 * (ctest-coreutils-runs/main.c has the story), so the tools are run from
 * /mnt/bin and the libc.a is /mnt/usr/lib/x86_64-slateos/libc.a.
 *
 * ## It cannot hang
 *
 * Each tool is a child this fixture forks, and is waited for by polling
 * waitpid for at most 60 s, then killed: a tool that never exits is a wrong
 * exit code, not a held-up boot. Its output comes through a pipe read until
 * end-of-file, which the child's exit gives.
 *
 * Exit codes: 42 is every check passed.
 *    2  pipe or fork failed     3  a file in /tmp could not be written
 * Otherwise <check><step>:
 *    x1  the tool did not run: it could not be exec'd, a signal ended it,
 *        or it was still running at 60 s
 *    x2  it exited other than 0
 *    x3  its output lacked what it should say (the output is printed)
 *    x4  it said so, but the file it was to write is missing or empty
 *    59  the program made from the IR exited other than 42
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Each can be given at compile time, which is how the same source is run
 * under Linux against the host's LLVM to check the fixture itself. */
#ifndef OPT
#define OPT "/mnt/bin/opt"
#endif
#ifndef LLC
#define LLC "/mnt/bin/llc"
#endif
#ifndef LLD
#define LLD "/mnt/bin/ld.lld"
#endif
#ifndef LIBDIR
#define LIBDIR "/mnt/usr/lib/x86_64-slateos"
#endif
#ifndef LLVM_SAYS
#define LLVM_SAYS "LLVM version 20.1.8"
#endif
#ifndef LLD_SAYS
#define LLD_SAYS "LLD 20.1.8"
#endif

#define IR_PATH "/tmp/ctest-llvm-tools.ll"
#define OPT_PATH "/tmp/ctest-llvm-tools.opt.ll"
#define OBJ_PATH "/tmp/ctest-llvm-tools.o"
#define EXE_PATH "/tmp/ctest-llvm-tools.exe"

/* How long one tool may run. */
#define TOOL_MS 60000

static char out[16384];

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

/* `n` in decimal, for the report: no printf, so a broken stdio cannot be what
 * the fixture ends up measuring. */
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

static long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long)ts.tv_sec * 1000L + (long)(ts.tv_nsec / 1000000L);
}

static void sleep_ms(long ms)
{
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    while (nanosleep(&t, &t) == -1 && errno == EINTR) {
    }
}

static int fail(int code, const char *why)
{
    emit("ctest-llvm-tools: FAIL ");
    emit_num(code);
    emit(": ");
    emit(why);
    emit("\n");
    return code;
}

/* Run `argv`, with its standard output and error into `out` (NUL-ended, cut
 * at the buffer's end). Its exit status, 256 + the signal that ended it, or
 * -1 when it could not be run or did not finish within TOOL_MS (it is then
 * killed). */
static int run(char *const argv[])
{
    int p[2];
    if (pipe(p) != 0) {
        return -2;
    }
    pid_t pid = fork();
    if (pid < 0) {
        close(p[0]);
        close(p[1]);
        return -2;
    }
    if (pid == 0) {
        close(p[0]);
        dup2(p[1], 1);
        dup2(p[1], 2);
        close(p[1]);
        execv(argv[0], argv);
        _exit(127);
    }
    close(p[1]);
    size_t len = 0;
    long deadline = now_ms() + TOOL_MS;
    int status = 0;
    int done = 0;
    int flags = fcntl(p[0], F_GETFL, 0);
    fcntl(p[0], F_SETFL, flags | O_NONBLOCK);
    for (;;) {
        char chunk[4096];
        ssize_t n = read(p[0], chunk, sizeof chunk);
        if (n > 0) {
            /* Kept while there is room; the rest is read and let go, as it
             * is not needed to judge the tool. */
            size_t room = sizeof out - 1 - len;
            size_t keep = (size_t)n < room ? (size_t)n : room;
            memcpy(out + len, chunk, keep);
            len += keep;
            continue;
        }
        if (n < 0 && errno == EINTR) {
            continue;
        }
        /* End-of-file, or nothing to read yet. */
        if (done) {
            break; /* the tool is gone, and what it wrote has been read */
        }
        if (waitpid(pid, &status, WNOHANG) == pid) {
            done = 1;
            continue; /* read what it left in the pipe */
        }
        if (now_ms() > deadline) {
            kill(pid, SIGKILL);
            waitpid(pid, &status, 0);
            close(p[0]);
            out[len] = '\0';
            return -1;
        }
        sleep_ms(20);
    }
    close(p[0]);
    out[len] = '\0';
    if (WIFSIGNALED(status)) {
        return 256 + WTERMSIG(status);
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

/* What became of a tool, as a check's code: 0 when it exited 0 and said
 * `want`. */
static int judge(int check, const char *what, int status, const char *want)
{
    emit("ctest-llvm-tools: ");
    emit(what);
    emit(" -> ");
    emit_num(status);
    emit("\n");
    if (status < 0 || status == 127 || status >= 256) {
        emit(out);
        return fail(check * 10 + 1, "the tool did not run (exec failed, a signal, or 60 s)");
    }
    if (status != 0) {
        emit(out);
        return fail(check * 10 + 2, "the tool exited other than 0");
    }
    if (want != NULL && strstr(out, want) == NULL) {
        emit(out);
        emit("ctest-llvm-tools: wanted: ");
        emit(want);
        emit("\n");
        return fail(check * 10 + 3, "the tool's output lacked what it should say");
    }
    return 0;
}

/* A file a step was to write: there, and not empty. */
static int made(int check, const char *path)
{
    struct stat st;
    if (stat(path, &st) != 0 || st.st_size == 0) {
        emit("ctest-llvm-tools: no output file ");
        emit(path);
        emit("\n");
        return fail(check * 10 + 4, "the tool said it worked, but its file is missing or empty");
    }
    return 0;
}

static int write_file(const char *path, const char *text)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) {
        return -1;
    }
    size_t n = strlen(text);
    ssize_t w = write(fd, text, n);
    close(fd);
    return w == (ssize_t)n ? 0 : -1;
}

int main(void)
{
    int code;

    char *opt_v[] = { OPT, "--version", NULL };
    if ((code = judge(1, "opt --version", run(opt_v), LLVM_SAYS)) != 0) {
        return code;
    }
    char *llc_v[] = { LLC, "--version", NULL };
    if ((code = judge(2, "llc --version", run(llc_v), LLVM_SAYS)) != 0) {
        return code;
    }
    if (strstr(out, "x86-64") == NULL) {
        emit(out);
        return fail(23, "llc --version does not list the x86-64 target");
    }
    char *lld_v[] = { LLD, "--version", NULL };
    if ((code = judge(3, "ld.lld --version", run(lld_v), LLD_SAYS)) != 0) {
        return code;
    }

    /* main returns 42; the C library's _start calls it and exits with its
     * value. */
    const char *ir = "define i32 @main() {\n"
                     "entry:\n"
                     "  ret i32 42\n"
                     "}\n";
    if (write_file(IR_PATH, ir) != 0) {
        return fail(3, "could not write the IR to /tmp");
    }
    char *opt_run[] = { OPT, "-passes=default<O2>", "-S", "-o", OPT_PATH, IR_PATH, NULL };
    if ((code = judge(5, "opt -passes=default<O2>", run(opt_run), NULL)) != 0
        || (code = made(5, OPT_PATH)) != 0) {
        return code;
    }
    char *llc_run[] = { LLC, "-O2", "-filetype=obj", "-mtriple=x86_64-unknown-linux-musl",
                        "-o", OBJ_PATH, OPT_PATH, NULL };
    if ((code = judge(5, "llc -filetype=obj", run(llc_run), NULL)) != 0
        || (code = made(5, OBJ_PATH)) != 0) {
        return code;
    }
    char *lld_run[] = { LLD, "-static", "--no-dynamic-linker", "-e", "_start", "-o", EXE_PATH,
                        OBJ_PATH, "-L" LIBDIR, "-lc", NULL };
    if ((code = judge(5, "ld.lld -static ... -lc", run(lld_run), NULL)) != 0
        || (code = made(5, EXE_PATH)) != 0) {
        return code;
    }
    if (chmod(EXE_PATH, 0755) != 0) {
        return fail(3, "could not make the program executable");
    }
    char *exe[] = { EXE_PATH, NULL };
    int got = run(exe);
    emit("ctest-llvm-tools: the program made from the IR -> ");
    emit_num(got);
    emit("\n");
    if (got != 42) {
        emit(out);
        return fail(59, "the program made from the IR exited other than 42");
    }
    unlink(IR_PATH);
    unlink(OPT_PATH);
    unlink(OBJ_PATH);
    unlink(EXE_PATH);
    emit("ctest-llvm-tools: every check passed\n");
    return 42;
}
