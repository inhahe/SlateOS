/* glibc's answers for the long double classification functions,
 * significandl and nanl, over every class of 80-bit encoding: the table
 * `CLASS_ORACLE` in posix/src/mathl.rs's tests is this program's output,
 * from glibc 2.39 under WSL.
 *
 *   gcc -O0 -fno-builtin -o ldclass ldclass.c -lm && ./ldclass
 *
 * Lines: "class SSSS:MMMMMMMMMMMMMMMM = fpclassify signbit finite isinf isnan"
 *        "significandl SSSS:MMMM = SSSS:MMMM errno"
 *        "nanl <tag> = SSSS:MMMM"
 */
#define _GNU_SOURCE
#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern int __fpclassifyl(long double);
extern int __signbitl(long double);
extern int finitel(long double);
extern int isinfl(long double);
extern int isnanl(long double);
extern long double significandl(long double);

union ld { long double v; struct { uint64_t m; uint16_t se; } b; };

static long double mk(uint16_t se, uint64_t m) { union ld u; memset(&u, 0, sizeof u); u.b.m = m; u.b.se = se; return u.v; }
static void pr(long double v) { union ld u; u.v = v; printf("%04X:%016llX", u.b.se, (unsigned long long)u.b.m); }

int main(void) {
    static const struct { uint16_t se; uint64_t m; } in[] = {
        {0x0000, 0}, {0x8000, 0},                                   /* zeros */
        {0x0000, 1}, {0x8000, 0x7FFFFFFFFFFFFFFFull},              /* subnormals */
        {0x0000, 0x8000000000000000ull}, {0x8000, 0xC000000000000001ull}, /* pseudo-denormals */
        {0x0001, 0x8000000000000000ull}, {0x3FFF, 0x8000000000000000ull}, /* normals */
        {0xBFFF, 0xC90FDAA22168C235ull}, {0x7FFE, 0xFFFFFFFFFFFFFFFFull},
        {0x3FFF, 0x4000000000000000ull}, {0x3FFF, 0},                   /* unnormal, pseudo-zero */
        {0x7FFF, 0x8000000000000000ull}, {0xFFFF, 0x8000000000000000ull}, /* infinities */
        {0x7FFF, 0}, {0xFFFF, 0x4000000000000000ull},                  /* pseudo-inf, pseudo-NaN */
        {0x7FFF, 0xC000000000000000ull}, {0xFFFF, 0xC000000000000001ull}, /* quiet NaNs */
        {0x7FFF, 0x8000000000000001ull}, {0x7FFF, 0xBFFFFFFFFFFFFFFFull}, /* signalling NaNs */
    };
    for (size_t i = 0; i < sizeof in / sizeof in[0]; i++) {
        long double x = mk(in[i].se, in[i].m);
        printf("class %04X:%016llX = %d %d %d %d %d\n", in[i].se, (unsigned long long)in[i].m,
               __fpclassifyl(x), __signbitl(x), finitel(x), isinfl(x), isnanl(x));
    }
    for (size_t i = 0; i < sizeof in / sizeof in[0]; i++) {
        long double x = mk(in[i].se, in[i].m);
        errno = 0;
        long double r = significandl(x);
        int e = errno;
        printf("significandl %04X:%016llX = ", in[i].se, (unsigned long long)in[i].m);
        pr(r);
        printf(" %d\n", e);
    }
    static const char *tags[] = {
        "", "0", "1", "123", "0x1", "0X7f", "017", "0x3FFFFFFFFFFFFFFF", "0x4000000000000000",
        "0xFFFFFFFFFFFFFFFF", "18446744073709551615", "18446744073709551616", "abc", "_",
        "12a", "0x", "-1", " 1", "1 ", "0x1p3", "9999999999999999999999",
    };
    for (size_t i = 0; i < sizeof tags / sizeof tags[0]; i++) {
        printf("nanl \"%s\" = ", tags[i]);
        pr(nanl(tags[i]));
        printf("\n");
    }
    return 0;
}
