/*
 * libvpx's intra predictors on seeded random edges, hashed: the reference
 * `src/intra.rs`'s `kernels_match_libvpx` test compares against.
 *
 * For each bit depth, block size and mode (the four DC variants counted
 * separately), 32 edges are drawn from a linear congruential generator --
 * the above-left pixel, two block widths of the row above, one of the column
 * to the left -- the predictor runs, and its block is hashed with 64-bit
 * FNV-1a. The Rust test draws the same edges in the same order.
 *
 * Build against a libvpx v1.17.0 tree configured with
 * --enable-vp9-highbitdepth (the build directory holds vpx_config.h and the
 * rtcd headers):
 *
 *   cc -O1 -I$BUILD -I$LIBVPX intra_reference.c $BUILD/libvpx.a -lm -lpthread
 *   ./a.out
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "./vpx_config.h"
#include "./vpx_dsp_rtcd.h"

static uint32_t seed = 0x1a7a5eedu;
static uint32_t next(void) {
  seed = seed * 1664525u + 1013904223u;
  return seed >> 8;
}

static uint64_t fnv(const uint8_t *p, size_t n, uint64_t h) {
  for (size_t i = 0; i < n; ++i) {
    h ^= p[i];
    h *= 0x100000001b3ull;
  }
  return h;
}

typedef void (*pred8)(uint8_t *, ptrdiff_t, const uint8_t *, const uint8_t *);
typedef void (*pred16)(uint16_t *, ptrdiff_t, const uint16_t *,
                       const uint16_t *, int);

/* Modes in this order: libvpx's mode numbers 0..9 with DC replaced by its
 * four variants at the end. */
#define ROW8(sz)                                                    \
  { vpx_v_predictor_##sz##_c,    vpx_h_predictor_##sz##_c,          \
    vpx_d45_predictor_##sz##_c,  vpx_d135_predictor_##sz##_c,       \
    vpx_d117_predictor_##sz##_c, vpx_d153_predictor_##sz##_c,       \
    vpx_d207_predictor_##sz##_c, vpx_d63_predictor_##sz##_c,        \
    vpx_tm_predictor_##sz##_c,   vpx_dc_predictor_##sz##_c,         \
    vpx_dc_top_predictor_##sz##_c, vpx_dc_left_predictor_##sz##_c,  \
    vpx_dc_128_predictor_##sz##_c }
#define ROW16(sz)                                                          \
  { vpx_highbd_v_predictor_##sz##_c,    vpx_highbd_h_predictor_##sz##_c,   \
    vpx_highbd_d45_predictor_##sz##_c,  vpx_highbd_d135_predictor_##sz##_c, \
    vpx_highbd_d117_predictor_##sz##_c, vpx_highbd_d153_predictor_##sz##_c, \
    vpx_highbd_d207_predictor_##sz##_c, vpx_highbd_d63_predictor_##sz##_c,  \
    vpx_highbd_tm_predictor_##sz##_c,   vpx_highbd_dc_predictor_##sz##_c,   \
    vpx_highbd_dc_top_predictor_##sz##_c,                                  \
    vpx_highbd_dc_left_predictor_##sz##_c,                                 \
    vpx_highbd_dc_128_predictor_##sz##_c }

int main(void) {
  static const pred8 p8[4][13] = { ROW8(4x4), ROW8(8x8), ROW8(16x16),
                                   ROW8(32x32) };
  static const pred16 p16[4][13] = { ROW16(4x4), ROW16(8x8), ROW16(16x16),
                                     ROW16(32x32) };
  for (int bd = 8; bd <= 12; bd += 2) {
    for (int tx = 0; tx < 4; ++tx) {
      const int bs = 4 << tx;
      for (int m = 0; m < 13; ++m) {
        uint64_t h = 0xcbf29ce484222325ull;
        for (int k = 0; k < 32; ++k) {
          uint8_t a8[1 + 64], l8[32], d8[32 * 32];
          uint16_t a16[1 + 64], l16[32], d16[32 * 32];
          /* above[-1], then 2 * bs above, then bs left. */
          for (int i = 0; i < 1 + 2 * bs; ++i) {
            const uint32_t r = next();
            a8[i] = (uint8_t)r;
            a16[i] = (uint16_t)(r & ((1u << bd) - 1));
          }
          for (int i = 0; i < bs; ++i) {
            const uint32_t r = next();
            l8[i] = (uint8_t)r;
            l16[i] = (uint16_t)(r & ((1u << bd) - 1));
          }
          if (bd == 8) {
            memset(d8, 0, sizeof(d8));
            p8[tx][m](d8, bs, a8 + 1, l8);
            h = fnv(d8, (size_t)(bs * bs), h);
          } else {
            memset(d16, 0, sizeof(d16));
            p16[tx][m](d16, bs, a16 + 1, l16, bd);
            h = fnv((const uint8_t *)d16, (size_t)(bs * bs) * 2, h);
          }
        }
        printf("        (%d, %d, %d, 0x%016llx),\n", bd, tx, m,
               (unsigned long long)h);
      }
    }
  }
  return 0;
}
