"""glibc 2.39's multibyte conversions in its C.UTF-8 locale -- <uchar.h>'s
mbrtoc8, c8rtomb, mbrtoc16, c16rtomb, mbrtoc32 and c32rtomb, and the string
forms mbsnrtowcs and wcsnrtombs -- as the oracle for posix/src/uchar.rs and
posix/src/wchar.rs. (This C library's multibyte encoding is UTF-8 in every
locale; open-questions D-Q7.)

    python posix/tools/oracle/multibyte_harness.py   # writes posix/src/multibyte_oracle.txt

Lines:

    <function> <input> <feed> = <step> <step> ...

The input is the bytes given (for the mbrto* functions) or the code units
given (for the c*rtomb ones), in hex; the feed is `all` (each call given
every byte left) or `one` (a byte a call). A step is `<result>:<what it
stored>` -- the result as a signed number, so (size_t)-1, -2 and -3 read as
such, the unit or the bytes stored in hex, `-` for none -- with `!EILSEQ`
after a result of -1. A conversion stops after a -1, or after the NUL.

And for the string forms, one line a call:

    mbsnrtowcs <input> <nms> <len> <dst> = <result> src+<offset> <characters> <mbsinit>
    wcsnrtombs <case> <nwc> <len> <dst> = <result> src+<offset> <bytes> <mbsinit>

where <dst> is `buf` or `NULL`, the source is `src=NULL` once the
terminator was converted, and a `then` line continues the conversion, state
and source, where the one before it stopped.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "multibyte_oracle.txt"

# Multibyte inputs: each ends with its NUL.
STRINGS = [
    "61626300",              # abc
    "c3a900",                # é
    "e282ac00",              # €
    "f09f988000",            # 😀
    "61c3a9e282acf09f98807a00",  # aé€😀z
    "00",
    "7f00",
    "c28000",                # U+0080
    "dfbf00",                # U+07FF
    "e0a08000",              # U+0800
    "efbfbf00",              # U+FFFF
    "f090808000",            # U+10000
    "f48fbfbf00",            # U+10FFFF
    "8000",                  # a lone continuation byte
    "c32800",                # a lead, then no continuation
    "eda08000",              # U+D800, a surrogate
    "edbfbf00",              # U+DFFF
    "f490808000",            # U+110000, past Unicode
    "c0af00",                # overlong /
    "e080af00",              # overlong /
    "f08082af00",            # overlong /
    "f800",                  # no lead byte means 5 bytes
    "ff00",
    "c3",                    # a lead with nothing after it (no NUL)
    "f09f98",                # three of four (no NUL)
]

# UTF-8 code units, fed to c8rtomb; each ends with a 0 unless it breaks first.
C8 = [
    "61", "c3a9", "e282ac", "f09f9880", "61c3a9e282acf09f98807a",
    "80", "c341", "c3c3a9", "eda080", "edbfbf", "f4908080", "c0af", "e080af", "f08082af",
    "f8", "ff", "c2", "e282", "f09f98",
]

# UTF-16 units, fed to c16rtomb.
C16 = [
    "0061", "00e9", "20ac", "d83dde00", "0061d83dde00007a",
    "de00", "d83d0041", "d83dd83dde00", "dbffdfff", "d800dc00", "fffe", "ffff", "d83d",
]

# Code points, fed to c32rtomb one at a time.
C32 = ["00000061", "000000e9", "000020ac", "0001f600", "0010ffff", "0000d800",
       "0000dfff", "00110000", "7fffffff", "80000000", "ffffffff", "0000fffe"]

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <uchar.h>
#include <wchar.h>
#include <unistd.h>
#include <sys/wait.h>

static size_t unhex(const char *h, unsigned char *out)
{
    size_t n = strlen(h) / 2;
    for (size_t i = 0; i < n; i++) { unsigned v; sscanf(h + 2 * i, "%2x", &v); out[i] = v; }
    return n;
}

static void result(size_t r)
{
    printf(" %ld", (long)r);
}

static void bad(size_t r)
{
    if (r == (size_t)-1) printf("!%s", errno == EILSEQ ? "EILSEQ" : "other");
}

/* mbrtoc8/16/32 over a multibyte string: all bytes a call, or one. */
#define MBRTO(NAME, T, FMT)                                                     \
static void NAME##_run(const char *hex, int one)                                \
{                                                                               \
    unsigned char in[64];                                                       \
    size_t len = unhex(hex, in), at = 0;                                        \
    mbstate_t st; memset(&st, 0, sizeof st);                                    \
    printf(#NAME " %s %s =", hex, one ? "one" : "all");                        \
    for (int guard = 0; guard < 64; guard++) {                                  \
        size_t left = len - at;                                                 \
        size_t n = one ? (left ? 1 : 0) : left;                                 \
        T unit = (T)0x5555;                                                     \
        errno = 0;                                                              \
        size_t r = NAME(&unit, (const char *)in + at, n, &st);                  \
        result(r);                                                              \
        if (r == (size_t)-1) { bad(r); break; }                                 \
        if (r == (size_t)-2) { printf(":-"); at += n; if (at >= len) break; continue; } \
        printf(":" FMT, (unsigned long)unit);                                   \
        if (r == 0) break;                                                      \
        if (r != (size_t)-3) at += r;                                           \
        if (at > len) break;                                                    \
    }                                                                           \
    printf("\n");                                                               \
}
MBRTO(mbrtoc8, char8_t, "%02lx")
MBRTO(mbrtoc16, char16_t, "%04lx")
MBRTO(mbrtoc32, char32_t, "%08lx")

static void stored(const char *buf, size_t r)
{
    printf(":");
    if (r == 0 || r == (size_t)-1) { printf("-"); return; }
    for (size_t i = 0; i < r; i++) printf("%02x", (unsigned char)buf[i]);
}

static void c8_run(const char *hex)
{
    unsigned char units[32];
    size_t n = unhex(hex, units);
    mbstate_t st; memset(&st, 0, sizeof st);
    printf("c8rtomb %s all =", hex);
    for (size_t i = 0; i <= n; i++) {
        char buf[16];
        errno = 0;
        size_t r = c8rtomb(buf, i < n ? units[i] : 0, &st);
        result(r); stored(buf, r); bad(r);
        if (r == (size_t)-1) break;
    }
    printf("\n");
}

static void c16_run(const char *hex)
{
    size_t n = strlen(hex) / 4;
    mbstate_t st; memset(&st, 0, sizeof st);
    printf("c16rtomb %s all =", hex);
    for (size_t i = 0; i <= n; i++) {
        unsigned v = 0;
        if (i < n) sscanf(hex + 4 * i, "%4x", &v);
        char buf[16];
        errno = 0;
        size_t r = c16rtomb(buf, (char16_t)v, &st);
        result(r); stored(buf, r); bad(r);
        if (r == (size_t)-1) break;
    }
    printf("\n");
}

static void c32_run(const char *hex)
{
    unsigned v; sscanf(hex, "%8x", &v);
    mbstate_t st; memset(&st, 0, sizeof st);
    char buf[16];
    errno = 0;
    size_t r = c32rtomb(buf, (char32_t)v, &st);
    printf("c32rtomb %s all =", hex);
    result(r); stored(buf, r); bad(r);
    printf("\n");
}

static const char *strings[] = {STRINGS};
static const char *c8s[] = {C8};
static const char *c16s[] = {C16};
static const char *c32s[] = {C32};
#define N(a) (sizeof(a) / sizeof(a)[0])

int main(void)
{
    if (!setlocale(LC_ALL, "C.UTF-8")) { fprintf(stderr, "no C.UTF-8\n"); return 1; }
    for (size_t i = 0; i < N(strings); i++)
        for (int one = 0; one < 2; one++) {
            mbrtoc8_run(strings[i], one);
            mbrtoc16_run(strings[i], one);
            mbrtoc32_run(strings[i], one);
        }
    for (size_t i = 0; i < N(c8s); i++) c8_run(c8s[i]);
    for (size_t i = 0; i < N(c16s); i++) c16_run(c16s[i]);
    for (size_t i = 0; i < N(c32s); i++) c32_run(c32s[i]);

    /* A NULL s -- mbrtoc8(NULL, NULL, 0, ps) is mbrtoc8(NULL, "", 1, ps) --
       in the initial state and with units still to come; each in a child,
       since glibc's can crash on some. */
    fflush(stdout);
    for (int k = 0; k < 6; k++) {
        pid_t pid = fork();
        if (pid == 0) {
            mbstate_t st; memset(&st, 0, sizeof st);
            char8_t u = 0x55; char16_t w = 0x5555; char buf[8];
            size_t r1 = 0, r2 = 0;
            const char *name = "";
            switch (k) {
            case 0: name = "mbrtoc8-null-initial"; r1 = mbrtoc8(NULL, NULL, 0, &st); r2 = mbrtoc8(&u, "a", 1, &st); break;
            case 1: name = "mbrtoc8-null-pending"; r1 = mbrtoc8(&u, "\xf0\x9f\x98\x80", 4, &st); r2 = mbrtoc8(NULL, NULL, 0, &st); break;
            case 2: name = "c8rtomb-null-initial"; r1 = c8rtomb(NULL, 0x41, &st); r2 = c8rtomb(buf, 0x41, &st); break;
            case 3: name = "c8rtomb-null-partial"; r1 = c8rtomb(buf, 0xc3, &st); r2 = c8rtomb(NULL, 0x41, &st); break;
            case 4: name = "mbrtoc16-null-pending"; r1 = mbrtoc16(&w, "\xf0\x9f\x98\x80", 4, &st); r2 = mbrtoc16(NULL, NULL, 0, &st); break;
            case 5: name = "c16rtomb-null-partial"; r1 = c16rtomb(buf, 0xd83d, &st); r2 = c16rtomb(NULL, 0, &st); break;
            }
            printf("%s = %ld %ld\n", name, (long)r1, (long)r2);
            fflush(stdout);
            _exit(0);
        }
        int status;
        waitpid(pid, &status, 0);
        if (!WIFEXITED(status)) printf("probe-%d = crash\n", k);
        fflush(stdout);
    }
    /* The string forms. */
    {
        struct { const char *in; size_t nms, len; int dst; size_t nms2, len2; } m[] = {
            {"61c3a92100", 2, 10, 1, 10, 10},   /* e-acute cut in two by nms */
            {"61c3a92100", 2, 10, 0, 10, 10},   /* the same, counting only */
            {"61c3a92100", 10, 2, 1, 10, 10},   /* len stops it after two */
            {"61c3a92100", 10, 10, 1, 0, 0},
            {"61c3a92100", 10, 10, 0, 0, 0},
            {"61ff2100", 10, 10, 1, 0, 0},      /* an invalid byte */
            {"61ff2100", 10, 10, 0, 0, 0},
            {"f09f988000", 3, 10, 1, 10, 10},   /* one character over two calls */
        };
        for (size_t i = 0; i < N(m); i++) {
            unsigned char in[32];
            unhex(m[i].in, in);
            const char *src = (const char *)in;
            mbstate_t st; memset(&st, 0, sizeof st);
            for (int pass = 0; pass < 2; pass++) {
                size_t nms = pass ? m[i].nms2 : m[i].nms, len = pass ? m[i].len2 : m[i].len;
                if (pass && !nms) break;
                wchar_t out[16];
                wmemset(out, 0x5555, 16);
                errno = 0;
                size_t r = mbsnrtowcs(m[i].dst ? out : NULL, &src, nms, len, &st);
                printf("mbsnrtowcs %s %zu %zu %s = %ld%s", pass ? "then" : m[i].in, nms, len,
                       m[i].dst ? "buf" : "NULL", (long)r,
                       r == (size_t)-1 ? (errno == EILSEQ ? "!EILSEQ" : "!other") : "");
                if (src) printf(" src+%ld", (long)(src - (const char *)in)); else printf(" src=NULL");
                printf(" ");
                if (m[i].dst && r != (size_t)-1)
                    for (size_t k = 0; k < r; k++) printf("%04x.", (unsigned)out[k]);
                printf("- %d\n", mbsinit(&st));
                if (!src) break;
            }
        }
        struct { const wchar_t *in; size_t nwc, len; int dst; } w[] = {
            {L"aé€", 10, 3, 1},    /* the euro sign does not fit */
            {L"aé€", 10, 3, 0},
            {L"aé€", 10, 10, 1},
            {L"aé€", 10, 10, 0},
            {L"aé€", 2, 10, 1},    /* nwc stops it */
        };
        for (size_t i = 0; i < N(w); i++) {
            const wchar_t *src = w[i].in;
            mbstate_t st; memset(&st, 0, sizeof st);
            char out[32];
            memset(out, 0x55, sizeof out);
            errno = 0;
            size_t r = wcsnrtombs(w[i].dst ? out : NULL, &src, w[i].nwc, w[i].len, &st);
            printf("wcsnrtombs %zu %zu %zu %s = %ld%s", i, w[i].nwc, w[i].len,
                   w[i].dst ? "buf" : "NULL", (long)r,
                   r == (size_t)-1 ? (errno == EILSEQ ? "!EILSEQ" : "!other") : "");
            if (src) printf(" src+%ld", (long)(src - w[i].in)); else printf(" src=NULL");
            printf(" ");
            if (w[i].dst && r != (size_t)-1)
                for (size_t k = 0; k < r; k++) printf("%02x", (unsigned char)out[k]);
            printf("- %d\n", mbsinit(&st));
        }
        /* A surrogate in a wide string. */
        wchar_t bad[] = {0x61, 0xd800, 0x62, 0};
        const wchar_t *src = bad;
        char out[16];
        errno = 0;
        size_t r = wcsnrtombs(out, &src, 10, 10, NULL);
        printf("wcsnrtombs surrogate = %ld%s src+%ld\n", (long)r, errno == EILSEQ ? "!EILSEQ" : "",
               src ? (long)(src - bad) : -1L);
    }
    printf("sizeof mbstate_t = %zu\n", sizeof(mbstate_t));
    return 0;
}
'''


def c_list(items):
    return "{" + ", ".join(f'"{s}"' for s in items) + "}"


def main() -> None:
    src = (PROGRAM.replace("{STRINGS}", c_list(STRINGS)).replace("{C8}", c_list(C8))
           .replace("{C16}", c_list(C16)).replace("{C32}", c_list(C32)))
    with workdir() as t:
        d = Path(t)
        (d / "uc.c").write_text(src, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -std=gnu2x -o uc uc.c && ./uc")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's <uchar.h> conversions in its C.UTF-8 locale, for posix/src/uchar.rs.\n"
            "# Generated by posix/tools/oracle/uchar_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
