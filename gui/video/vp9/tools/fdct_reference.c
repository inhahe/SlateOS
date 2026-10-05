/*
 * libvpx's forward transforms on seeded random residual blocks, hashed: the
 * reference `src/enc/fdct.rs`'s `matches_libvpx_*` test compares against.
 *
 * Each case fills a block with residuals from a linear congruential
 * generator (the same one the Rust test runs): most blocks random in
 * [-255, 255], the residuals an 8-bit picture can have, and some at the
 * extremes, every sample +255 or -255, where the transforms' intermediate
 * values come nearest their bounds. It runs libvpx's C transform and hashes
 * the coefficients, each as a little-endian 32-bit integer, with 64-bit
 * FNV-1a.
 *
 * Build against a libvpx v1.17.0 tree configured with the VP9 encoder and
 * without high bit depth (vpxenc's default build; the build directory holds
 * vpx_config.h and the rtcd headers):
 *
 *   cc -O1 -I$BUILD -I$LIBVPX fdct_reference.c $BUILD/libvpx.a -lm -lpthread
 *   ./a.out
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "./vpx_config.h"
#include "./vp9_rtcd.h"
#include "./vpx_dsp_rtcd.h"

static uint32_t seed = 0xfdc7a5edu;
static uint32_t next(void) {
  seed = seed * 1664525u + 1013904223u;
  return seed >> 8;
}

static uint64_t fnv_coefs(const tran_low_t *c, int n, uint64_t h) {
  for (int i = 0; i < n; ++i) {
    const uint32_t v = (uint32_t)(int32_t)c[i];
    for (int b = 0; b < 4; ++b) {
      h ^= (v >> (8 * b)) & 0xff;
      h *= 0x100000001b3ull;
    }
  }
  return h;
}

/* A residual block of n x n at stride 64: random, or (one block in eight)
 * every sample at +255 or -255. */
static void fill(int16_t *block, int n) {
  const int extreme = next() % 8 == 0;
  for (int r = 0; r < n; ++r)
    for (int c = 0; c < n; ++c)
      block[r * 64 + c] = extreme ? ((next() & 1) ? 255 : -255)
                                  : (int16_t)((int)(next() % 511) - 255);
}

enum { BLOCKS = 200 };

int main(void) {
  static int16_t block[64 * 64];
  static tran_low_t out[32 * 32];
  /* (transform, tx_type, hash): 0 fdct4x4, 1 fdct8x8, 2 fdct16x16,
   * 3 fdct32x32, 4 fdct32x32_rd, 5 fht4x4, 6 fht8x8, 7 fht16x16,
   * 8 fwht4x4. */
  for (int t = 0; t <= 8; ++t) {
    const int types = (t >= 5 && t <= 7) ? 4 : 1;
    for (int type = 0; type < types; ++type) {
      uint64_t h = 0xcbf29ce484222325ull;
      for (int i = 0; i < BLOCKS; ++i) {
        int n = 4;
        memset(out, 0, sizeof(out));
        switch (t) {
          case 0: fill(block, 4); vpx_fdct4x4_c(block, out, 64); n = 4; break;
          case 1: fill(block, 8); vpx_fdct8x8_c(block, out, 64); n = 8; break;
          case 2: fill(block, 16); vpx_fdct16x16_c(block, out, 64); n = 16; break;
          case 3: fill(block, 32); vpx_fdct32x32_c(block, out, 64); n = 32; break;
          case 4: fill(block, 32); vpx_fdct32x32_rd_c(block, out, 64); n = 32; break;
          case 5: fill(block, 4); vp9_fht4x4_c(block, out, 64, type); n = 4; break;
          case 6: fill(block, 8); vp9_fht8x8_c(block, out, 64, type); n = 8; break;
          case 7: fill(block, 16); vp9_fht16x16_c(block, out, 64, type); n = 16; break;
          default: fill(block, 4); vp9_fwht4x4_c(block, out, 64); n = 4; break;
        }
        h = fnv_coefs(out, n * n, h);
      }
      printf("        (%d, %d, 0x%016llxu64),\n", t, type, (unsigned long long)h);
    }
  }
  return 0;
}
