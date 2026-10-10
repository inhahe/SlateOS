/* glibc's memory and string functions, timed as posix/benches/mem.rs times
 * ours: the same calls, sizes, offsets and buffers, and the same report
 * line, so the two outputs line up row for row.  It is the reference
 * performance-targets.md names for the C library ("within 2x of glibc").
 *
 * Run it on Linux, or under WSL on the machine the Rust bench runs on --
 * the comparison is only fair on one machine, and only quiet:
 *
 *     gcc -O2 -fno-builtin -o glibc-reference glibc-reference.c
 *     ./glibc-reference
 *
 * -fno-builtin keeps gcc from inlining or folding the calls, so each is
 * glibc's (through its IFUNC, which picks the CPU's best: AVX2 or EVEX on
 * a recent x86-64, which ours, SSE2 by the sysroot's target, cannot use).
 * Under WSL the smallest sizes' times include the VM's costs and read
 * high; from about 1 KiB they are glibc's. */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static volatile size_t sink;

static double now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec * 1e-9;
}

#define MEASURE(name, bytes, expr)                                          \
    do {                                                                    \
        for (int w = 0; w < 100; w++) { expr; }                             \
        unsigned long calls = 1;                                            \
        for (;;) {                                                          \
            double t0 = now();                                              \
            for (unsigned long i = 0; i < calls; i++) { expr; }             \
            double el = now() - t0;                                         \
            if (el >= 0.05 || calls >= (1ul << 30)) {                       \
                double ns = el * 1e9 / calls;                               \
                printf("%-22s %8zu B  %12.1f ns/call  %8.2f GB/s\n", name,  \
                       (size_t)(bytes), ns, ns > 0 ? (bytes) / ns : 0.0);   \
                break;                                                      \
            }                                                               \
            calls *= 4;                                                     \
        }                                                                   \
    } while (0)

int main(void) {
    size_t sizes[] = {8, 16, 32, 64, 256, 1024, 4096, 65536, 1 << 20};
    size_t nsizes = sizeof sizes / sizeof sizes[0];
    size_t max = sizes[nsizes - 1];
    unsigned char *a = malloc(max + 64), *b = malloc(max + 64);
    memset(a, 0x5a, max + 64);
    memset(b, 0xa5, max + 64);
    static const char needle[] = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxy";

    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        unsigned char *src = a + 3, *dst = b + 5;
        MEASURE("memcpy", n, (memcpy(dst, src, n), sink += dst[0]));
    }
    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        MEASURE("memmove forward", n, (memmove(a, a + 7, n), sink += a[0]));
        MEASURE("memmove backward", n, (memmove(a + 7, a, n), sink += a[7]));
    }
    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        MEASURE("memset", n, (memset(b + 1, 0x33, n), sink += b[1]));
    }
    memcpy(b, a, max + 64);
    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        MEASURE("memcmp (equal)", n, sink += memcmp(a + 3, b + 3, n));
    }
    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        MEASURE("memchr (absent)", n, sink += (size_t)memchr(a + 1, 0, n));
    }
    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        memset(a + 1, 'x', n);
        a[n + 1] = 0;
        MEASURE("strlen", n, sink += strlen((char *)a + 1));
        a[n + 1] = 0x5a;
    }
    for (size_t k = 0; k < nsizes; k++) {
        size_t n = sizes[k];
        memset(a + 1, 'x', n);
        a[n + 1] = 0;
        memset(b + 3, 'x', n);
        b[n + 3] = 0;
        char *s = (char *)a + 1, *t = (char *)b + 3;
        MEASURE("strnlen", n, sink += strnlen(s, (size_t)-1));
        MEASURE("strchr (absent)", n, sink += (size_t)strchr(s, 'q'));
        MEASURE("strrchr (absent)", n, sink += (size_t)strrchr(s, 'q'));
        MEASURE("strcmp (equal)", n, sink += strcmp(s, t));
        MEASURE("strspn", n, sink += strspn(s, "xyz"));
        MEASURE("strstr (x{31}y)", n, sink += (size_t)strstr(s, needle));
        b[n + 3] = 0x5a;
        char *d = (char *)b + 5;
        MEASURE("strcpy", n, (strcpy(d, s), sink += d[0]));
        a[n + 1] = 0x5a;
        memset(b, 0xa5, max + 64);
    }
    return 0;
}
