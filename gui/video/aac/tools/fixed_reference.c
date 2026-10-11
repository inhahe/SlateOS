/*
 * libavutil's fixed-point DSP (its C versions, as the oracle's build has no
 * assembly) and libavcodec's integer square roots, over pseudorandom inputs,
 * each function's results folded into an FNV-1a hash: the numbers
 * src/fixed/tests.rs holds the translation to.
 *
 *     gcc -O1 -I<ffmpeg> gui/video/aac/tools/fixed_reference.c \
 *         <ffmpeg>/libavutil/libavutil.a -lm -o fxref && ./fxref
 *
 * <ffmpeg> is a configured FFmpeg 8.1.3 tree (tools/ffaac/build.sh makes
 * one); the inputs are the ones the test draws, from the same generator.
 */
#include <stdio.h>
#include <stdint.h>
#include "libavutil/fixed_dsp.c"
#include "libavcodec/mathtables.c"
#include "libavcodec/mathops.h"

static uint64_t state = 0x2545f4914f6cdd1dULL;

static uint32_t next(void)
{
    state = state * 6364136223846793005ULL + 1442695040888963407ULL;
    return (uint32_t)(state >> 32);
}

static uint64_t fnv(uint64_t h, int32_t v)
{
    for (int i = 0; i < 4; i++) {
        h ^= (uint8_t)((uint32_t)v >> (8 * i));
        h *= 0x100000001b3ULL;
    }
    return h;
}

/* A sample of `bits` bits: a random word shifted down, so that every
 * magnitude turns up. */
static int32_t sample(void)
{
    int32_t v = (int32_t)next();
    int shift = (int)(next() % 32);
    return v >> shift;
}

/* A power of two of either sign, now and then one off it: products of
 * these fall on Q31's rounding boundaries. */
static int32_t power(void)
{
    int exp = (int)(next() % 31);
    int32_t v = (int32_t)(1u << exp);
    uint32_t r = next();
    if (r & 1)
        v = -v;
    if (r & 2)
        v += (r & 4) ? 1 : -1;
    return v;
}

#define N 64

int main(void)
{
    uint64_t h_sqrt = 0xcbf29ce484222325ULL, h_fsqrt = h_sqrt, h_fmul = h_sqrt,
             h_rev = h_sqrt, h_add = h_sqrt, h_win = h_sqrt, h_scalar = h_sqrt,
             h_bfly = h_sqrt;
    for (uint32_t a = 0; a < 70000; a++)
        h_sqrt = fnv(h_sqrt, (int32_t)ff_sqrt(a));
    for (int i = 0; i < 200000; i++)
        h_sqrt = fnv(h_sqrt, (int32_t)ff_sqrt(next()));
    for (int i = 0; i < 200000; i++) {
        /* What the decoder asks: band energies, not negative, at 31 bits.
         * One draw a statement: C leaves the order of calls within an
         * expression unspecified. */
        uint32_t r = next();
        int shift = (int)(next() % 32);
        int32_t x = (int32_t)(r >> shift) & 0x7fffffff;
        int bits = 16 + (int)(next() % 16);
        h_fsqrt = fnv(h_fsqrt, fixed_sqrt(x, bits));
    }
    for (int round = 0; round < 2000; round++) {
        int32_t a[2 * N], b[2 * N], c[2 * N], w[2 * N], d[2 * N];
        int len = 1 + (int)(next() % N);
        for (int i = 0; i < 2 * N; i++) {
            a[i] = sample();
            b[i] = sample();
            c[i] = sample();
            w[i] = sample();
        }
        vector_fmul_c(d, a, b, len);
        for (int i = 0; i < len; i++)
            h_fmul = fnv(h_fmul, d[i]);
        vector_fmul_reverse_c(d, a, b, len);
        for (int i = 0; i < len; i++)
            h_rev = fnv(h_rev, d[i]);
        vector_fmul_add_c(d, a, b, c, len);
        for (int i = 0; i < len; i++)
            h_add = fnv(h_add, d[i]);
        vector_fmul_window_c(d, a, b, w, len);
        for (int i = 0; i < 2 * len; i++)
            h_win = fnv(h_win, d[i]);
        /* Products small enough not to overflow the sum, as the noise's are. */
        for (int i = 0; i < len; i++) {
            a[i] >>= 4;
            b[i] >>= 4;
        }
        h_scalar = fnv(h_scalar, scalarproduct_fixed_c(a, b, len));
        butterflies_fixed_c(a, b, len);
        for (int i = 0; i < len; i++)
            h_bfly = fnv(fnv(h_bfly, a[i]), b[i]);
    }
    /* Random words seldom land on a rounding boundary; powers of two, and
     * squares, land on nothing else. */
    uint64_t e_fsqrt = 0xcbf29ce484222325ULL, e_fmul = e_fsqrt, e_win = e_fsqrt;
    for (int i = 0; i < 200000; i++) {
        uint32_t k = next() % 46341;
        int shift = (int)(next() % 4);
        int32_t x = (int32_t)((k * k) >> shift);
        int bits = 16 + (int)(next() % 16);
        e_fsqrt = fnv(e_fsqrt, fixed_sqrt(x, bits));
    }
    for (int round = 0; round < 2000; round++) {
        int32_t a[2 * N], b[2 * N], w[2 * N], d[2 * N];
        int len = 1 + (int)(next() % N);
        for (int i = 0; i < 2 * N; i++) {
            a[i] = power();
            b[i] = power();
            w[i] = power();
        }
        vector_fmul_c(d, a, b, len);
        for (int i = 0; i < len; i++)
            e_fmul = fnv(e_fmul, d[i]);
        vector_fmul_window_c(d, a, b, w, len);
        for (int i = 0; i < 2 * len; i++)
            e_win = fnv(e_win, d[i]);
    }
    printf("sqrt %016llx\nfixed_sqrt %016llx\nfmul %016llx\nfmul_reverse %016llx\n"
           "fmul_add %016llx\nfmul_window %016llx\nscalarproduct %016llx\n"
           "butterflies %016llx\nsquares_fixed_sqrt %016llx\npowers_fmul %016llx\n"
           "powers_fmul_window %016llx\n",
           (unsigned long long)h_sqrt, (unsigned long long)h_fsqrt,
           (unsigned long long)h_fmul, (unsigned long long)h_rev,
           (unsigned long long)h_add, (unsigned long long)h_win,
           (unsigned long long)h_scalar, (unsigned long long)h_bfly,
           (unsigned long long)e_fsqrt, (unsigned long long)e_fmul,
           (unsigned long long)e_win);
    return 0;
}
