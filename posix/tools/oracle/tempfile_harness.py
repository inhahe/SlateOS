"""glibc 2.39's temporary files -- mkstemp, mkostemp, mkstemps, mkostemps,
mkdtemp, mktemp and their *64 names; tmpfile; tmpnam, tmpnam_r, tempnam --
as the oracle for posix/src/tempname.rs and the functions over it.

    python posix/tools/oracle/tempfile_harness.py   # writes posix/src/tempfile_oracle.txt

One line a probe, `<probe> = <what it gave>`. A name is random, so what is
recorded of a template is its shape afterwards: each byte as it was, `*`
where a letter or digit replaced it, `?` where anything else did, `@` for a
NUL. Each probe runs in a child of its own, in a directory of its own
under /tmp (not the Windows mount, whose files have no Unix modes), with
errno set to 12345 first so that a success that disturbs it shows.

What the library decides -- which templates are refused, which bytes a
name takes, the flags and modes a file is created with, where tmpnam and
tempnam look -- the tests replay. What the kernel decides (whether an
open with those flags succeeds, a file's link count) is recorded for the
reader, and for the boot-time test that runs on this kernel.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "tempfile_oracle.txt"

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *en(int e)
{
    switch (e) {
    case 0: return "0";
    case 12345: return "kept";
    case EINVAL: return "EINVAL";
    case ENOENT: return "ENOENT";
    case EEXIST: return "EEXIST";
    case ENOTDIR: return "ENOTDIR";
    case EACCES: return "EACCES";
    case EISDIR: return "EISDIR";
    case ELOOP: return "ELOOP";
    case ENAMETOOLONG: return "ENAMETOOLONG";
    case EOPNOTSUPP: return "EOPNOTSUPP";
    case EFAULT: return "EFAULT";
    default: { static char b[16]; snprintf(b, sizeof b, "e%d", e); return b; }
    }
}

/* A probe in a child of its own, in a fresh directory under /tmp. */
#define PROBE(NAME, ...)                                                      \
    do {                                                                       \
        fflush(stdout);                                                        \
        pid_t pid_ = fork();                                                   \
        if (pid_ == 0) {                                                       \
            char dir_[] = "/tmp/tfh-XXXXXX";                                   \
            if (!mkdtemp(dir_) || chdir(dir_) != 0) _exit(3);                  \
            printf("%s =", NAME);                                              \
            __VA_ARGS__;                                                       \
            printf("\n");                                                      \
            fflush(stdout);                                                    \
            char rm_[64];                                                      \
            snprintf(rm_, sizeof rm_, "rm -rf %s", dir_);                      \
            if (system(rm_) != 0) _exit(4);                                    \
            _exit(0);                                                          \
        }                                                                      \
        int st_;                                                               \
        waitpid(pid_, &st_, 0);                                                \
        if (!WIFEXITED(st_) || WEXITSTATUS(st_) != 0)                          \
            printf("%s = crash %d\n", NAME, st_);                              \
    } while (0)

/* What became of each byte of a template: 0 kept so far, 1 replaced by a
 * letter or digit, 2 by something else, 3 by a NUL. A replacement can pick
 * the byte that was there, so a call is made RUNS times and the most any
 * run did is kept: a position left alone every time was never chosen. */
#define RUNS 8
static void merge(unsigned char *state, const char *before, const char *after, size_t len)
{
    for (size_t i = 0; i < len; i++) {
        unsigned char a = (unsigned char)after[i], s = 0;
        if (a == 0) s = 3;
        else if (a == (unsigned char)before[i]) s = 0;
        else if ((a >= '0' && a <= '9') || (a >= 'a' && a <= 'z') || (a >= 'A' && a <= 'Z'))
            s = 1;
        else s = 2;
        if (s > state[i]) state[i] = s;
    }
}

static void shape(const char *before, const unsigned char *state, size_t len)
{
    printf(" [");
    for (size_t i = 0; i < len; i++)
        putchar("\0*?@"[state[i]] ? "\0*?@"[state[i]] : before[i]);
    printf("]");
}

enum fn { MKSTEMP, MKSTEMP64, MKOSTEMP, MKOSTEMP64, MKSTEMPS, MKSTEMPS64, MKOSTEMPS,
          MKOSTEMPS64, MKDTEMP, MKTEMP };
static const char *fn_name[] = { "mkstemp", "mkstemp64", "mkostemp", "mkostemp64",
    "mkstemps", "mkstemps64", "mkostemps", "mkostemps64", "mkdtemp", "mktemp" };

/* `f` on a copy of `tmpl`, RUNS times: its answer, errno, the shape, and
 * what the name is afterwards -- the same every run, or `unstable`. */
static void call(enum fn f, const char *tmpl, int suffixlen, int flags)
{
    size_t len = strlen(tmpl);
    unsigned char state[256] = { 0 };
    char first[256] = "";
    for (int run = 0; run < RUNS; run++) {
        char buf[256], out[256];
        memcpy(buf, tmpl, len + 1);
        errno = 12345;
        long r = 0;
        char *p = NULL;
        int is_ptr = 0;
        switch (f) {
        case MKSTEMP: r = mkstemp(buf); break;
        case MKSTEMP64: r = mkstemp64(buf); break;
        case MKOSTEMP: r = mkostemp(buf, flags); break;
        case MKOSTEMP64: r = mkostemp64(buf, flags); break;
        case MKSTEMPS: r = mkstemps(buf, suffixlen); break;
        case MKSTEMPS64: r = mkstemps64(buf, suffixlen); break;
        case MKOSTEMPS: r = mkostemps(buf, suffixlen, flags); break;
        case MKOSTEMPS64: r = mkostemps64(buf, suffixlen, flags); break;
        case MKDTEMP: p = mkdtemp(buf); is_ptr = 1; break;
        case MKTEMP: p = mktemp(buf); is_ptr = 1; break;
        }
        int e = errno;
        int n = snprintf(out, sizeof out, " %s errno=%s",
                         is_ptr ? (p == NULL ? "NULL" : p == buf ? "tmpl" : "other")
                                : (r < 0 ? "-1" : "fd"),
                         en(e));
        struct stat st;
        if (!is_ptr && r >= 0) {
            /* the file is the template's name, and a regular file */
            if (stat(buf, &st) == 0 && S_ISREG(st.st_mode))
                snprintf(out + n, sizeof out - n, " file");
            close((int)r);
        } else if (f == MKDTEMP && p != NULL) {
            if (stat(buf, &st) == 0 && S_ISDIR(st.st_mode))
                snprintf(out + n, sizeof out - n, " dir");
        } else if (f == MKTEMP && p != NULL && buf[0] != 0) {
            snprintf(out + n, sizeof out - n, lstat(buf, &st) == 0 ? " exists" : " absent");
        }
        merge(state, tmpl, buf, len);
        if (run == 0) strcpy(first, out);
        else if (strcmp(first, out) != 0) strcpy(first, " unstable");
    }
    printf("%s", first);
    shape(tmpl, state, len);
}

static const struct { const char *t; int s; } TEMPLATES[] = {
    { "", 0 }, { "X", 0 }, { "XXXXX", 0 }, { "XXXXXX", 0 }, { "aXXXXXX", 0 },
    { "XXXXXXX", 0 }, { "XXXXXXXXXX", 0 }, { "xxxxxx", 0 }, { "XXXXXx", 0 },
    { "xXXXXX", 0 }, { "XXXXXX/", 0 }, { "a.XXXXXX", 0 }, { "XXXXXXa", 0 },
    { "XXXXXXa", 1 }, { "XXXXXXab", 1 }, { "XXXXXXab", 2 }, { "abcXXXXXXdef", 3 },
    { "abcXXXXXdef", 3 }, { "abcXXXXXXXdef", 3 }, { "XXXXXX", 1 }, { "XXXXXXX", 1 },
    { "XXXXXX.c", 2 }, { "a", 1 }, { "XXXXXX", 6 }, { "XXXXXX", 7 },
    { "XXXXXX", -1 }, { "XXXXXXa", -1 }, { "XXXXXX", INT_MAX }, { "XXXXXX", INT_MIN },
    { "nodir/XXXXXX", 0 }, { "nodir/XXXXXXa", 1 }, { "file/XXXXXX", 0 },
    { "sub/XXXXXX", 0 }, { "sub/XXXXXX.txt", 4 },
};

/* Flags for mkostemp, by name. */
static const struct { const char *n; int f; } FLAGS[] = {
    { "0", 0 }, { "O_WRONLY", O_WRONLY }, { "O_RDWR", O_RDWR }, { "O_ACCMODE", O_ACCMODE },
    { "O_APPEND", O_APPEND }, { "O_CLOEXEC", O_CLOEXEC }, { "O_SYNC", O_SYNC },
    { "O_DSYNC", O_DSYNC }, { "O_NONBLOCK", O_NONBLOCK }, { "O_NOCTTY", O_NOCTTY },
    { "O_TRUNC", O_TRUNC }, { "O_CREAT", O_CREAT }, { "O_EXCL", O_EXCL },
    { "O_NOFOLLOW", O_NOFOLLOW }, { "O_DIRECTORY", O_DIRECTORY }, { "O_PATH", O_PATH },
    { "O_TMPFILE", O_TMPFILE }, { "O_NOATIME", O_NOATIME }, { "O_ASYNC", O_ASYNC },
    { "O_APPEND|O_CLOEXEC", O_APPEND | O_CLOEXEC }, { "O_WRONLY|O_APPEND", O_WRONLY | O_APPEND },
    { "-1", -1 }, { "0x40000000", 0x40000000 },
};

/* The descriptor's status flags and FD_CLOEXEC, as names. */
static void fd_flags(int fd)
{
    int fl = fcntl(fd, F_GETFL), fdf = fcntl(fd, F_GETFD);
    int acc = fl & O_ACCMODE;
    printf(" %s", acc == O_RDONLY ? "rdonly" : acc == O_WRONLY ? "wronly" :
           acc == O_RDWR ? "rdwr" : "acc3");
    if (fl & O_APPEND) printf("|append");
    if ((fl & O_SYNC) == O_SYNC) printf("|sync");
    else if (fl & O_DSYNC) printf("|dsync");
    if (fl & O_NONBLOCK) printf("|nonblock");
    if (fl & O_NOATIME) printf("|noatime");
    if (fl & O_ASYNC) printf("|async");
    if (fdf & FD_CLOEXEC) printf(" cloexec");
}

int main(void)
{
    char name[128];
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);

    /* Every template through every function; the s forms with each
     * suffix length, the others only where it is 0. */
    for (size_t i = 0; i < sizeof TEMPLATES / sizeof *TEMPLATES; i++) {
        for (int f = MKSTEMP; f <= MKTEMP; f++) {
            int takes_suffix = f >= MKSTEMPS && f <= MKOSTEMPS64;
            if (!takes_suffix && TEMPLATES[i].s != 0) continue;
            snprintf(name, sizeof name, "%s(\"%s\", %d)", fn_name[f], TEMPLATES[i].t,
                     TEMPLATES[i].s);
            PROBE(name, {
                mkdir("sub", 0700);
                close(open("file", O_CREAT | O_WRONLY, 0600));
                call((enum fn)f, TEMPLATES[i].t, TEMPLATES[i].s, 0);
            });
        }
    }

    /* Every name taken but one: the call finds the one left. */
    PROBE("mkstemp(\"XXXXXX\") with names taken", {
        /* (not all 62^6; enough to see that EEXIST is retried) */
        char t[] = "XXXXXX";
        int fd = mkstemp(t);
        char again[] = "XXXXXX";
        int fd2 = mkstemp(again);
        printf(" %s %s", fd >= 0 ? "fd" : "-1", fd2 >= 0 && strcmp(t, again) != 0 ? "distinct" : "same");
    });

    /* The flags the o forms take: what the descriptor has. */
    static const enum fn WITH_FLAGS[] = { MKOSTEMP, MKOSTEMP64, MKOSTEMPS, MKOSTEMPS64 };
    for (size_t i = 0; i < sizeof FLAGS / sizeof *FLAGS; i++) {
        for (size_t k = 0; k < 4; k++) {
            enum fn f = WITH_FLAGS[k];
            snprintf(name, sizeof name, "%s flags %s", fn_name[f], FLAGS[i].n);
            PROBE(name, {
                char t[] = "XXXXXX";
                int fl = FLAGS[i].f, fd = -1;
                errno = 12345;
                switch (f) {
                case MKOSTEMP: fd = mkostemp(t, fl); break;
                case MKOSTEMP64: fd = mkostemp64(t, fl); break;
                case MKOSTEMPS: fd = mkostemps(t, 0, fl); break;
                default: fd = mkostemps64(t, 0, fl); break;
                }
                printf(" %s errno=%s", fd >= 0 ? "fd" : "-1", en(errno));
                if (fd >= 0) fd_flags(fd);
            });
        }
    }

    /* The modes files and directories are made with, under three umasks. */
    static const int UMASKS[] = { 0, 022, 0277 };
    for (size_t i = 0; i < 3; i++) {
        snprintf(name, sizeof name, "modes under umask %03o", UMASKS[i]);
        PROBE(name, {
            umask(UMASKS[i]);
            struct stat st;
            char a[] = "XXXXXX", b[] = "XXXXXX", c[] = "XXXXXX.c", d[] = "XXXXXX";
            int fa = mkstemp(a), fb = mkostemp(b, O_APPEND);
            int fc = mkostemps(c, 2, 0);
            char *dd = mkdtemp(d);
            if (fa >= 0 && fstat(fa, &st) == 0) printf(" mkstemp %04o", st.st_mode & 07777);
            if (fb >= 0 && fstat(fb, &st) == 0) printf(" mkostemp %04o", st.st_mode & 07777);
            if (fc >= 0 && fstat(fc, &st) == 0) printf(" mkostemps %04o", st.st_mode & 07777);
            if (dd && stat(d, &st) == 0) printf(" mkdtemp %04o", st.st_mode & 07777);
        });
    }

    /* tmpfile: the stream, the descriptor, and where the file is. */
    static const char *TMPDIRS[] = { NULL, "/nonexistent", "." };
    for (size_t i = 0; i < 3; i++) {
        snprintf(name, sizeof name, "tmpfile TMPDIR=%s", TMPDIRS[i] ? TMPDIRS[i] : "(unset)");
        PROBE(name, {
            if (TMPDIRS[i]) setenv("TMPDIR", TMPDIRS[i], 1); else unsetenv("TMPDIR");
            umask(022);
            errno = 12345;
            FILE *f = tmpfile();
            printf(" %s errno=%s", f ? "stream" : "NULL", en(errno));
            if (f) {
                struct stat st;
                int fd = fileno(f);
                fd_flags(fd);
                if (fstat(fd, &st) == 0)
                    printf(" mode %04o nlink %lu", st.st_mode & 07777, (unsigned long)st.st_nlink);
                char back[4] = { 0 };
                int ok = fputs("abc", f) >= 0 && fseek(f, 0, SEEK_SET) == 0 &&
                         fread(back, 1, 3, f) == 3 && strcmp(back, "abc") == 0;
                printf(" %s", ok ? "rw" : "not-rw");
                fclose(f);
            }
        });
    }
    PROBE("tmpfile64", {
        FILE *f = tmpfile64();
        printf(" %s", f ? "stream" : "NULL");
        if (f) fclose(f);
    });

    /* tmpnam, tmpnam_r, tempnam: the names, their random part masked. */
    static const char *DIRS[] = { NULL, "", "sub", "sub/", "sub//", "nodir", "file", "/tmp", "/" };
    static const char *PFXS[] = { NULL, "", "ab", "abcde", "abcdefgh", "a/b" };
    static const char *ENVS[] = { NULL, "sub", "nodir", "" };
    for (size_t e = 0; e < 4; e++)
    for (size_t d = 0; d < sizeof DIRS / sizeof *DIRS; d++)
    for (size_t p = 0; p < sizeof PFXS / sizeof *PFXS; p++) {
        snprintf(name, sizeof name, "tempnam(%s, %s) TMPDIR=%s", DIRS[d] ? DIRS[d] : "NULL",
                 PFXS[p] ? PFXS[p] : "NULL", ENVS[e] ? ENVS[e] : "(unset)");
        PROBE(name, {
            mkdir("sub", 0700);
            close(open("file", O_CREAT | O_WRONLY, 0600));
            if (ENVS[e]) setenv("TMPDIR", ENVS[e], 1); else unsetenv("TMPDIR");
            errno = 12345;
            char *s = tempnam(DIRS[d], PFXS[p]);
            printf(" %s errno=%s", s ? "name" : "NULL", en(errno));
            if (s) {
                size_t n = strlen(s);
                char masked[512];
                memcpy(masked, s, n + 1);
                for (size_t k = n >= 6 ? n - 6 : 0; k < n; k++) masked[k] = '*';
                printf(" %s", masked);
                free(s);
            }
        });
    }
    PROBE("tmpnam(NULL)", {
        errno = 12345;
        char *s = tmpnam(NULL);
        printf(" %s errno=%s", s ? "name" : "NULL", en(errno));
        if (s) printf(" %.*s****** len %zu", (int)strlen(s) - 6, s, strlen(s));
    });
    PROBE("tmpnam(buf)", {
        char buf[L_tmpnam];
        char *s = tmpnam(buf);
        printf(" %s L_tmpnam %d", s == buf ? "buf" : s ? "other" : "NULL", L_tmpnam);
    });
    PROBE("tmpnam_r(NULL)", {
        errno = 12345;
        char *s = tmpnam_r(NULL);
        printf(" %s errno=%s", s ? "name" : "NULL", en(errno));
    });
    PROBE("tmpnam_r(buf)", {
        char buf[L_tmpnam];
        errno = 12345;
        char *s = tmpnam_r(buf);
        printf(" %s errno=%s", s == buf ? "buf" : s ? "other" : "NULL", en(errno));
        if (s) printf(" %.*s******", (int)strlen(s) - 6, s);
    });
    printf("TMP_MAX = %d\nP_tmpdir = %s\n", TMP_MAX, P_tmpdir);
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "tf.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -Wall -Werror -o tf tf.c && ./tf && uname -r")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        *body, kernel = r.stdout.rstrip("\n").split("\n")
    head = ("# glibc 2.39's temporary files, for posix/src/tempname.rs. Generated by\n"
            "# posix/tools/oracle/tempfile_harness.py; do not edit.\n"
            f"# kernel: Linux {kernel}\n")
    OUT.write_text(head + "\n".join(body) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(body)} lines")


if __name__ == "__main__":
    main()
