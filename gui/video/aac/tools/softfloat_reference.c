/*
 * libavutil's soft-float over a long run of pseudorandom inputs, each
 * function's results folded into an FNV-1a hash: the numbers
 * src/softfloat/tests.rs holds the translation to.
 *
 *     gcc -O1 -I<ffmpeg> gui/video/aac/tools/softfloat_reference.c -lm -o sfref && ./sfref
 *
 * <ffmpeg> is a configured FFmpeg 8.1.3 tree (tools/ffaac/build.sh makes
 * one); the inputs are the ones the test draws, from the same generator.
 */
#include <stdio.h>
#include <stdint.h>
#include "libavutil/softfloat.h"

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

static uint64_t fnv_sf(uint64_t h, SoftFloat v)
{
    return fnv(fnv(h, v.mant), v.exp);
}

/* A normalized soft-float from an int and a count of fraction bits. */
static SoftFloat draw(void)
{
    int32_t v = (int32_t)next();
    int frac = (int)(next() % 61);
    return av_int2sf(v, frac);
}

int main(void)
{
    uint64_t h_int2sf = 0xcbf29ce484222325ULL, h_mul = h_int2sf, h_add = h_int2sf,
             h_sub = h_int2sf, h_div = h_int2sf, h_gt = h_int2sf, h_cmp = h_int2sf,
             h_sqrt = h_int2sf, h_sincos = h_int2sf, h_norm = h_int2sf,
             h_divraw = h_int2sf;
    const int32_t edges[] = { 0, 1, -1, 2, -2, INT32_MAX, INT32_MIN, INT32_MIN + 1,
                              0x20000000, -0x20000000, 0x3fffffff, -0x40000000 };
    for (int i = 0; i < 12; i++)
        for (int f = 0; f < 64; f += 7)
            h_int2sf = fnv_sf(h_int2sf, av_int2sf(edges[i], f));
    for (int i = 0; i < 200000; i++) {
        SoftFloat a = draw(), b = draw();
        h_int2sf = fnv_sf(h_int2sf, a);
        h_mul = fnv_sf(h_mul, av_mul_sf(a, b));
        h_add = fnv_sf(h_add, av_add_sf(a, b));
        h_sub = fnv_sf(h_sub, av_sub_sf(a, b));
        if (b.mant)
            h_div = fnv_sf(h_div, av_div_sf(a, b));
        h_gt = fnv(h_gt, av_gt_sf(a, b));
        h_cmp = fnv(h_cmp, av_cmp_sf(a, b));
        if (a.mant >= 0)
            h_sqrt = fnv_sf(h_sqrt, av_sqrt_sf(a));
        {
            int s, c;
            av_sincos_sf((int32_t)next(), &s, &c);
            h_sincos = fnv(fnv(h_sincos, s), c);
        }
        {
            /* Raw mantissas and exponents, which the predictor's states start
             * as, through the normalizers. */
            /* One draw a statement: C leaves the order of calls within an
             * expression, or an initializer list, unspecified. */
            int32_t m = (int32_t)next();
            int shift = (int)(next() % 31);
            int e = (int)(next() % 300) - 150;
            SoftFloat r = { m >> shift, e };
            h_norm = fnv_sf(h_norm, av_normalize_sf(av_normalize1_sf(r)));
            /* And as divisors: one smaller than normalized gives a quotient
             * too big for 32 bits, which the division halves till it fits. */
            if (r.mant)
                h_divraw = fnv_sf(h_divraw, av_div_sf(a, r));
        }
    }
    printf("int2sf %016llx\nmul %016llx\nadd %016llx\nsub %016llx\ndiv %016llx\n"
           "gt %016llx\ncmp %016llx\nsqrt %016llx\nsincos %016llx\nnorm %016llx\n"
           "divraw %016llx\n",
           (unsigned long long)h_int2sf, (unsigned long long)h_mul,
           (unsigned long long)h_add, (unsigned long long)h_sub,
           (unsigned long long)h_div, (unsigned long long)h_gt,
           (unsigned long long)h_cmp, (unsigned long long)h_sqrt,
           (unsigned long long)h_sincos, (unsigned long long)h_norm,
           (unsigned long long)h_divraw);
    return 0;
}
