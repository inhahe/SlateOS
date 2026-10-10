/*
 * ctest-binutils-runs -- do GNU binutils' programs run on SlateOS, and do as
 * and ld make a program that runs?
 *
 * scripts/binutils-spike/ cross-builds binutils 2.47 against SlateOS's own
 * libc.a -- all fourteen programs with nothing missing -- and the rootfs
 * recipe stages them: /bin/as, /bin/ld.bfd, /bin/nm, /bin/objcopy, /bin/size,
 * /bin/addr2line, /bin/c++filt and /bin/elfedit under their own names, and ar,
 * ranlib, strip, strings, objdump and readelf as /bin/gnu-* (the plain names
 * are lane B's programs), with the image's libc.a at
 * /usr/lib/x86_64-slateos/. A clean link says nothing is missing; only here do
 * they start, read their arguments and files, and write files another program
 * can use.
 *
 * The checks, in order -- the assembler and the linker first, since a C
 * compiler on SlateOS needs both:
 *
 *    1x  as --version names binutils 2.47
 *    2x  ld.bfd --version names it too
 *    3x  as -g assembles a `main` that returns 42, written to /tmp
 *    5x  ld.bfd -static links it with the image's libc.a -- and the program
 *        it makes, run, exits 42
 *    6x  nm lists `main` in the object: "0000000000000000 T main"
 *    7x  size gives its sections: six bytes of text and nothing else
 *    8x  objcopy -O binary -j .text writes those six bytes, b8 2a 00 00 00 c3
 *    9x  addr2line puts address 0 at line 5 of the source, through the DWARF
 *        line table as -g wrote
 *   10x  c++filt demangles _ZN3foo3barEv to "foo::bar()"
 *   11x  elfedit --version names binutils 2.47
 *   12x  gnu-objdump -d disassembles the object: "mov    $0x2a,%eax"
 *   13x  gnu-readelf -h calls the program an executable
 *   14x  gnu-ar rc makes a library of the object, and gnu-ar t lists it
 *   15x  gnu-ranlib indexes it, and nm -s shows "main in ctest-binutils.o"
 *   16x  gnu-strip copies the program without its symbols, and the copy, run,
 *        still exits 42
 *   17x  gnu-strings finds the assembler's name in the object's DWARF
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
 * exit code, not a held-up boot. Its output -- standard output and error
 * together -- comes through a pipe read until end-of-file, which the child's
 * exit gives. (ctest-llvm-tools's arrangement, whose run() this is.)
 *
 * Exit codes: 42 is every check passed.
 *    2  pipe or fork failed     3  a file in /tmp could not be written
 * Otherwise <check><step>:
 *    x1  the tool did not run: it could not be exec'd, a signal ended it, or
 *        it was still running at 60 s
 *    x2  it exited other than 0
 *    x3  its output lacked what it should say (the output is printed)
 *    x4  it said so, but the file it was to write is missing, empty or wrong
 *    59  the program as and ld.bfd made exited other than 42
 *   169  the copy gnu-strip made exited other than 42
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
 * under Linux against the host's binutils to check the fixture itself (with
 * NO_RUN, since the programs it links are SlateOS's and run only there). */
#ifndef BIN
#define BIN "/mnt/bin/"
#endif
#ifndef GNU
#define GNU "gnu-"
#endif
#ifndef LD_BFD
#define LD_BFD "ld.bfd"
#endif
#ifndef LIBDIR
#define LIBDIR "/mnt/usr/lib/x86_64-slateos"
#endif
#ifndef BINUTILS_SAYS
#define BINUTILS_SAYS " (GNU Binutils) 2.47"
#endif
#ifndef AS_PRODUCER
#define AS_PRODUCER "GNU AS 2.47"
#endif

#define SRC_PATH "/tmp/ctest-binutils.s"
#define OBJ_PATH "/tmp/ctest-binutils.o"
#define EXE_PATH "/tmp/ctest-binutils.exe"
#define BIN_PATH "/tmp/ctest-binutils.bin"
#define LIB_PATH "/tmp/libctest-binutils.a"
#define STRIPPED_PATH "/tmp/ctest-binutils.stripped"

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
    emit("ctest-binutils-runs: FAIL ");
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

/* What became of a tool, as a check's code: 0 when it exited 0 and its
 * output holds `want` (any output, for a NULL `want`). */
static int judge(int check, const char *what, int status, const char *want)
{
    emit("ctest-binutils-runs: ");
    emit(what);
    emit(" -> ");
    emit_num(status);
    emit("\n");
    if (status == -2) {
        return fail(2, "pipe or fork failed -- this fixture's plumbing");
    }
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
        emit("ctest-binutils-runs: wanted: ");
        emit(want);
        emit("\n");
        return fail(check * 10 + 3, "the tool's output lacked what it should say");
    }
    return 0;
}

/* Whether some line of `out` is exactly `line`: a substring would pass on a
 * usage message that happens to quote the words. */
static int has_line(const char *line)
{
    size_t n = strlen(line);
    const char *at = out;
    while (*at != '\0') {
        const char *end = strchr(at, '\n');
        size_t len = end != NULL ? (size_t)(end - at) : strlen(at);
        if (len == n && strncmp(at, line, n) == 0) {
            return 1;
        }
        if (end == NULL) {
            break;
        }
        at = end + 1;
    }
    return 0;
}

/* judge(), then `line` required as a whole line of the output. */
static int judge_line(int check, const char *what, int status, const char *line)
{
    int code = judge(check, what, status, NULL);
    if (code != 0) {
        return code;
    }
    if (!has_line(line)) {
        emit(out);
        emit("ctest-binutils-runs: wanted the line: ");
        emit(line);
        emit("\n");
        return fail(check * 10 + 3, "the tool's output lacked the line it should print");
    }
    return 0;
}

/* A file a step was to write: there, and not empty. */
static int made(int check, const char *path)
{
    struct stat st;
    if (stat(path, &st) != 0 || st.st_size == 0) {
        emit("ctest-binutils-runs: no output file ");
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

/* Whether the file at `path` holds exactly `want`'s `n` bytes. */
static int file_holds(const char *path, const unsigned char *want, size_t n)
{
    unsigned char got[64];
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return 0;
    }
    ssize_t r = read(fd, got, sizeof got);
    close(fd);
    return r == (ssize_t)n && memcmp(got, want, n) == 0;
}

/* Run a program the tools made: it should exit 42. */
static int run_made(int code, const char *what, const char *path)
{
    if (chmod(path, 0755) != 0) {
        return fail(3, "could not make a program in /tmp executable");
    }
#ifdef NO_RUN
    (void)code;
    (void)what;
    return 0;
#else
    char *argv[] = { (char *)path, NULL };
    int got = run(argv);
    emit("ctest-binutils-runs: ");
    emit(what);
    emit(" -> ");
    emit_num(got);
    emit("\n");
    if (got != 42) {
        emit(out);
        return fail(code, what);
    }
    return 0;
#endif
}

int main(void)
{
    int code;

    /* 1, 2: the assembler and the linker start, and say what they are. */
    char *as_v[] = { BIN "as", "--version", NULL };
    if ((code = judge(1, "as --version", run(as_v), "GNU assembler" BINUTILS_SAYS)) != 0) {
        return code;
    }
    char *ld_v[] = { BIN LD_BFD, "--version", NULL };
    if ((code = judge(2, "ld.bfd --version", run(ld_v), "GNU ld" BINUTILS_SAYS)) != 0) {
        return code;
    }

    /* 3: `main` returns 42, and the C library's _start exits with its value.
     * Line 5 is the movl, which check 9 asks addr2line for. */
    const char *src = "\t.text\n"
                      "\t.globl\tmain\n"
                      "\t.type\tmain, @function\n"
                      "main:\n"
                      "\tmovl\t$42, %eax\n"
                      "\tret\n"
                      "\t.size\tmain, .-main\n"
                      "\t.section\t.note.GNU-stack,\"\",@progbits\n";
    if (write_file(SRC_PATH, src) != 0) {
        return fail(3, "could not write the assembly source to /tmp");
    }
    char *as_run[] = { BIN "as", "-g", "-o", OBJ_PATH, SRC_PATH, NULL };
    if ((code = judge(3, "as -g", run(as_run), NULL)) != 0 || (code = made(3, OBJ_PATH)) != 0) {
        return code;
    }

    /* 5: linked with the image's libc.a, whose _start calls main; GNU ld
     * takes _start from the archive as its default entry, as lld does. */
    char *ld_run[] = { BIN LD_BFD, "-static", "-o", EXE_PATH, OBJ_PATH, "-L" LIBDIR, "-lc", NULL };
    if ((code = judge(5, "ld.bfd -static ... -lc", run(ld_run), NULL)) != 0
        || (code = made(5, EXE_PATH)) != 0) {
        return code;
    }
    if ((code = run_made(59, "the program as and ld.bfd made", EXE_PATH)) != 0) {
        return code;
    }

    /* 6: the object's symbol table. */
    char *nm_run[] = { BIN "nm", OBJ_PATH, NULL };
    if ((code = judge_line(6, "nm", run(nm_run), "0000000000000000 T main")) != 0) {
        return code;
    }

    /* 7: its sections' sizes -- movl's five bytes and ret's one. */
    char *size_run[] = { BIN "size", OBJ_PATH, NULL };
    if ((code = judge_line(7, "size", run(size_run),
                           "      6\t      0\t      0\t      6\t      6\t" OBJ_PATH)) != 0) {
        return code;
    }

    /* 8: those six bytes, written out raw. */
    char *objcopy_run[] = { BIN "objcopy", "-O", "binary", "-j", ".text", OBJ_PATH, BIN_PATH, NULL };
    if ((code = judge(8, "objcopy -O binary", run(objcopy_run), NULL)) != 0) {
        return code;
    }
    static const unsigned char text[] = { 0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3 };
    if (!file_holds(BIN_PATH, text, sizeof text)) {
        return fail(84, "objcopy's file is not the six bytes b8 2a 00 00 00 c3");
    }

    /* 9: an address back to its source line, through the line table. */
    char *addr2line_run[] = { BIN "addr2line", "-e", OBJ_PATH, "0", NULL };
    if ((code = judge_line(9, "addr2line", run(addr2line_run), SRC_PATH ":5")) != 0) {
        return code;
    }

    /* 10: a mangled C++ name. */
    char *cxxfilt_run[] = { BIN "c++filt", "_ZN3foo3barEv", NULL };
    if ((code = judge_line(10, "c++filt", run(cxxfilt_run), "foo::bar()")) != 0) {
        return code;
    }

    /* 11: elfedit starts. */
    char *elfedit_v[] = { BIN "elfedit", "--version", NULL };
    if ((code = judge(11, "elfedit --version", run(elfedit_v), "GNU elfedit" BINUTILS_SAYS)) != 0) {
        return code;
    }

    /* 12: the disassembler. */
    char *objdump_run[] = { BIN GNU "objdump", "-d", OBJ_PATH, NULL };
    if ((code = judge(12, "objdump -d", run(objdump_run), "mov    $0x2a,%eax")) != 0) {
        return code;
    }

    /* 13: the program's ELF header. */
    char *readelf_run[] = { BIN GNU "readelf", "-h", EXE_PATH, NULL };
    if ((code = judge(13, "readelf -h", run(readelf_run), "EXEC (Executable file)")) != 0) {
        return code;
    }

    /* 14: a static library of the object. */
    (void)unlink(LIB_PATH);
    char *ar_rc[] = { BIN GNU "ar", "rc", LIB_PATH, OBJ_PATH, NULL };
    if ((code = judge(14, "ar rc", run(ar_rc), NULL)) != 0 || (code = made(14, LIB_PATH)) != 0) {
        return code;
    }
    char *ar_t[] = { BIN GNU "ar", "t", LIB_PATH, NULL };
    if ((code = judge_line(14, "ar t", run(ar_t), "ctest-binutils.o")) != 0) {
        return code;
    }

    /* 15: its symbol index, which a linker reads to find `main`. */
    char *ranlib_run[] = { BIN GNU "ranlib", LIB_PATH, NULL };
    if ((code = judge(15, "ranlib", run(ranlib_run), NULL)) != 0) {
        return code;
    }
    char *nm_s[] = { BIN "nm", "-s", LIB_PATH, NULL };
    if ((code = judge_line(15, "nm -s", run(nm_s), "main in ctest-binutils.o")) != 0) {
        return code;
    }

    /* 16: a copy without symbols, which must still run. */
    char *strip_run[] = { BIN GNU "strip", "-o", STRIPPED_PATH, EXE_PATH, NULL };
    if ((code = judge(16, "strip -o", run(strip_run), NULL)) != 0
        || (code = made(16, STRIPPED_PATH)) != 0) {
        return code;
    }
    if ((code = run_made(169, "the copy gnu-strip made", STRIPPED_PATH)) != 0) {
        return code;
    }

    /* 17: the text in the object -- the assembler's name, in its DWARF. */
    char *strings_run[] = { BIN GNU "strings", "-a", OBJ_PATH, NULL };
    if ((code = judge_line(17, "strings -a", run(strings_run), AS_PRODUCER)) != 0) {
        return code;
    }

    unlink(SRC_PATH);
    unlink(OBJ_PATH);
    unlink(EXE_PATH);
    unlink(BIN_PATH);
    unlink(LIB_PATH);
    unlink(STRIPPED_PATH);
    emit("ctest-binutils-runs: every check passed\n");
    return 42;
}
