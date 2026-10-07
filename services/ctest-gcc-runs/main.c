/*
 * ctest-gcc-runs -- does GCC 16.2 compile, link and make programs that run,
 * on SlateOS?
 *
 * scripts/gcc-spike/ builds GCC to run on SlateOS: native.sh compiles it, and
 * libgcc and libstdc++ with it, with the cross GCC cross.sh builds, and the
 * rootfs recipe stages the result under /usr with what a compiler needs from
 * the system -- the C library's headers in /usr/include, our overlay of them
 * in /usr/x86_64-linux-musl/include, libc.a and the crt objects in /usr/lib,
 * and binutils' as and ld in /usr/x86_64-linux-musl/bin
 * (design-decisions.md 1185). Each piece links; only here do they work
 * together.
 *
 * The checks, in order -- each adds to the one before, so the boundary
 * between the last pass and the first failure is the finding:
 *
 *    1x  gcc --version names GCC 16.2.0
 *    2x  g++ --version names it too
 *    3x  cpp -P expands a macro: the preprocessor, and GCC's search for its
 *        own programs
 *    5x  gcc compiles and links a C program -- stdio, string.h, stdlib.h,
 *        through the overlay and musl's headers -- and the program it makes
 *        runs, prints what it should and exits 42
 *    6x  g++ compiles and links a C++ program that throws and catches an
 *        exception, writes to std::cout, builds strings in a vector and joins
 *        a thread -- libstdc++, libgcc's unwinder, and our pthreads -- and the
 *        program it makes runs and exits 42
 *
 * There is no check 4, so no failure is 42.
 *
 * The image is mounted at /mnt (ctest-coreutils-runs/main.c has the story).
 * GCC finds its own programs and libraries relative to where it is run from,
 * so /mnt/usr/bin/gcc finds /mnt/usr/libexec/gcc/... and /mnt/usr/lib by
 * itself; the C library's headers are the system's, /usr/include, so
 * --sysroot=/mnt tells it where the system is. On an image mounted at /,
 * neither the path nor the flag is needed.
 *
 * ## It cannot hang
 *
 * Each tool is a child this fixture forks, and is waited for by polling
 * waitpid for at most TOOL_MS, then killed (ctest-llvm-tools's run()). cc1plus
 * parsing <iostream> is the slowest step here, and under emulation it is the
 * one most likely to need the time.
 *
 * Exit codes: 42 is every check passed.
 *    2  pipe or fork failed     3  a file in /tmp could not be written
 * Otherwise <check><step>:
 *    x1  the tool did not run: it could not be exec'd, a signal ended it, or
 *        it was still running at the time limit
 *    x2  it exited other than 0 (its output, the compiler's errors, is
 *        printed)
 *    x3  its output lacked what it should say
 *    x4  it said so, but the program it was to write is missing or empty
 *    59  the C program GCC made did not exit 42, or printed the wrong line
 *    69  the C++ program g++ made did not exit 42, or printed the wrong line
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

#ifndef GCC
#define GCC "/mnt/usr/bin/gcc"
#endif
#ifndef GXX
#define GXX "/mnt/usr/bin/g++"
#endif
#ifndef CPP
#define CPP "/mnt/usr/bin/cpp"
#endif
#ifndef SYSROOT_FLAG
#define SYSROOT_FLAG "--sysroot=/mnt"
#endif
#ifndef GCC_SAYS
#define GCC_SAYS "(GCC) 16.2.0"
#endif
#ifndef GCC_MAJOR_MINOR
#define GCC_MAJOR_MINOR "16.2"
#endif

#define C_SRC "/tmp/ctest-gcc.c"
#define C_EXE "/tmp/ctest-gcc.exe"
#define CXX_SRC "/tmp/ctest-gcc.cpp"
#define CXX_EXE "/tmp/ctest-gcc-cxx.exe"
#define CPP_SRC "/tmp/ctest-gcc.h"

/* How long one tool may run: cc1plus on <iostream> under emulation. */
#define TOOL_MS 300000

static char out[32768];

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

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
    emit("ctest-gcc-runs: FAIL ");
    emit_num(code);
    emit(": ");
    emit(why);
    emit("\n");
    return code;
}

/* Run `argv`, with its standard output and error into `out` (NUL-ended, cut
 * at the buffer's end). Its exit status, 256 + the signal that ended it, -1
 * when it could not be run or did not finish within TOOL_MS (it is then
 * killed), or -2 when the pipe or the fork failed. */
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
            size_t room = sizeof out - 1 - len;
            size_t keep = (size_t)n < room ? (size_t)n : room;
            memcpy(out + len, chunk, keep);
            len += keep;
            continue;
        }
        if (n < 0 && errno == EINTR) {
            continue;
        }
        if (done) {
            break;
        }
        if (waitpid(pid, &status, WNOHANG) == pid) {
            done = 1;
            continue;
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

/* What became of a tool, as a check's code: 0 when it exited 0 and its
 * output holds `want` (any output, for a NULL `want`). */
static int judge(int check, const char *what, int status, const char *want)
{
    emit("ctest-gcc-runs: ");
    emit(what);
    emit(" -> ");
    emit_num(status);
    emit("\n");
    if (status == -2) {
        return fail(2, "pipe or fork failed -- this fixture's plumbing");
    }
    if (status < 0 || status == 127 || status >= 256) {
        emit(out);
        return fail(check * 10 + 1, "the tool did not run (exec failed, a signal, or the time limit)");
    }
    if (status != 0) {
        emit(out);
        return fail(check * 10 + 2, "the tool exited other than 0");
    }
    if (want != NULL && strstr(out, want) == NULL) {
        emit(out);
        emit("ctest-gcc-runs: wanted: ");
        emit(want);
        emit("\n");
        return fail(check * 10 + 3, "the tool's output lacked what it should say");
    }
    return 0;
}

static int made(int check, const char *path)
{
    struct stat st;
    if (stat(path, &st) != 0 || st.st_size == 0) {
        emit("ctest-gcc-runs: no output file ");
        emit(path);
        emit("\n");
        return fail(check * 10 + 4, "the compiler said it worked, but its program is missing or empty");
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

/* Run a program the compiler made: it should exit 42 and print `want`. */
static int run_made(int code, const char *what, const char *path, const char *want)
{
#ifdef NO_RUN
    (void)code;
    (void)what;
    (void)path;
    (void)want;
    return 0;
#else
    char *argv[] = { (char *)path, NULL };
    int got = run(argv);
    emit("ctest-gcc-runs: ");
    emit(what);
    emit(" -> ");
    emit_num(got);
    emit("\n");
    if (got != 42 || strstr(out, want) == NULL) {
        emit(out);
        emit("ctest-gcc-runs: wanted exit 42 and: ");
        emit(want);
        emit("\n");
        return fail(code, what);
    }
    return 0;
#endif
}

int main(void)
{
    int code;

    /* 1, 2: the drivers start, and say what they are. */
    char *gcc_v[] = { GCC, "--version", NULL };
    if ((code = judge(1, "gcc --version", run(gcc_v), "gcc " GCC_SAYS)) != 0) {
        return code;
    }
    char *gxx_v[] = { GXX, "--version", NULL };
    if ((code = judge(2, "g++ --version", run(gxx_v), "g++ " GCC_SAYS)) != 0) {
        return code;
    }

    /* 3: the preprocessor -- the driver finding cc1, which preprocesses. */
    if (write_file(CPP_SRC, "#define ANSWER 6 * 7\nthe answer is ANSWER\n") != 0) {
        return fail(3, "could not write the preprocessor's input to /tmp");
    }
    char *cpp_run[] = { CPP, SYSROOT_FLAG, "-P", CPP_SRC, NULL };
    if ((code = judge(3, "cpp -P", run(cpp_run), "the answer is 6 * 7")) != 0) {
        return code;
    }

    /* 5: a C program, through the C library's headers -- the overlay's
     * strdup and musl's printf -- linked static against libc.a. */
    const char *c_src = "#include <stdio.h>\n"
                        "#include <stdlib.h>\n"
                        "#include <string.h>\n"
                        "\n"
                        "static int cmp(const void *a, const void *b)\n"
                        "{\n"
                        "    return *(const int *)a - *(const int *)b;\n"
                        "}\n"
                        "\n"
                        "int main(void)\n"
                        "{\n"
                        "    int v[] = { 3, 1, 2 };\n"
                        "    qsort(v, 3, sizeof v[0], cmp);\n"
                        "    char *s = strdup(\"GCC\");\n"
                        "    if (s == NULL) {\n"
                        "        return 1;\n"
                        "    }\n"
                        "    printf(\"ctest-gcc: C, built by %s %d.%d, sorted %d %d %d\\n\", s,\n"
                        "           __GNUC__, __GNUC_MINOR__, v[0], v[1], v[2]);\n"
                        "    free(s);\n"
                        "    return 42;\n"
                        "}\n";
    if (write_file(C_SRC, c_src) != 0) {
        return fail(3, "could not write the C source to /tmp");
    }
    char *gcc_run[] = { GCC, SYSROOT_FLAG, "-O2", "-Wall", "-o", C_EXE, C_SRC, NULL };
    if ((code = judge(5, "gcc -O2 -o", run(gcc_run), NULL)) != 0 || (code = made(5, C_EXE)) != 0) {
        return code;
    }
    if ((code = run_made(59, "the C program gcc made", C_EXE,
                         "ctest-gcc: C, built by GCC " GCC_MAJOR_MINOR ", sorted 1 2 3")) != 0) {
        return code;
    }

    /* 6: a C++ program -- libstdc++, an exception through libgcc's
     * unwinder, a thread through our pthreads. */
    const char *cxx_src = "#include <iostream>\n"
                          "#include <stdexcept>\n"
                          "#include <string>\n"
                          "#include <thread>\n"
                          "#include <vector>\n"
                          "\n"
                          "int main()\n"
                          "{\n"
                          "    int caught = 0;\n"
                          "    try {\n"
                          "        throw std::runtime_error(\"thrown\");\n"
                          "    } catch (const std::exception &e) {\n"
                          "        caught = std::string(e.what()) == \"thrown\";\n"
                          "    }\n"
                          "    std::vector<std::string> words{\"built\", \"by\", \"g++\"};\n"
                          "    std::string joined;\n"
                          "    for (const auto &w : words) {\n"
                          "        joined += (joined.empty() ? \"\" : \" \") + w;\n"
                          "    }\n"
                          "    int from_thread = 0;\n"
                          "    std::thread t([&from_thread] { from_thread = 7; });\n"
                          "    t.join();\n"
                          "    std::cout << \"ctest-gcc: C++, \" << joined << \", caught \" << caught\n"
                          "              << \", thread \" << from_thread << std::endl;\n"
                          "    return caught == 1 && from_thread == 7 ? 42 : 1;\n"
                          "}\n";
    if (write_file(CXX_SRC, cxx_src) != 0) {
        return fail(3, "could not write the C++ source to /tmp");
    }
    char *gxx_run[] = { GXX, SYSROOT_FLAG, "-O2", "-Wall", "-o", CXX_EXE, CXX_SRC, NULL };
    if ((code = judge(6, "g++ -O2 -o", run(gxx_run), NULL)) != 0 || (code = made(6, CXX_EXE)) != 0) {
        return code;
    }
    if ((code = run_made(69, "the C++ program g++ made", CXX_EXE,
                         "ctest-gcc: C++, built by g++, caught 1, thread 7")) != 0) {
        return code;
    }

    unlink(C_SRC);
    unlink(C_EXE);
    unlink(CXX_SRC);
    unlink(CXX_EXE);
    unlink(CPP_SRC);
    emit("ctest-gcc-runs: every check passed\n");
    return 42;
}
