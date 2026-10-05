/*
 * libvpx's inverse transforms on seeded random blocks, hashed: the
 * reference `src/idct.rs`'s `matches_libvpx_*` tests compare against.
 *
 * Each case fills a block's coefficients from a linear congruential
 * generator (the same one the Rust test runs), either densely or only along
 * the first `eob` positions of the default scan, fills the prediction with
 * random pixels, runs libvpx's dispatcher -- which takes its eob shortcuts --
 * and hashes the pixels with 64-bit FNV-1a. Coefficients are bounded so that
 * no intermediate value leaves the range the VP9 specification allows,
 * where libvpx's 8-bit and high-bit-depth transforms (and this port) agree.
 *
 * Build against a libvpx v1.17.0 tree configured with
 * --enable-vp9-highbitdepth (the build directory holds vpx_config.h and the
 * rtcd headers):
 *
 *   cc -O1 -I$BUILD -I$LIBVPX idct_reference.c $BUILD/libvpx.a -lm -lpthread
 *   ./a.out
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "./vpx_config.h"
#include "vp9/common/vp9_idct.h"
#include "vp9/common/vp9_scan.h"

static uint32_t seed = 0x5eed1234u;
static uint32_t next(void) {
  seed = seed * 1664525u + 1013904223u;
  return seed >> 8;
}
/* A value in [-bound, bound]. */
static int32_t coef(int32_t bound) {
  return (int32_t)(next() % (uint32_t)(2 * bound + 1)) - bound;
}

static uint64_t fnv(const uint8_t *p, size_t n, uint64_t h) {
  for (size_t i = 0; i < n; ++i) {
    h ^= p[i];
    h *= 0x100000001b3ull;
  }
  return h;
}

/* Bounds for 8-bit streams: per size, small enough that every intermediate
 * value of the 2-D transform fits 16 bits even with every coefficient
 * aligned. */
static const int32_t bound8[4] = { 2048, 512, 128, 32 };

static const int16_t *scan_of(int tx) {
  return vp9_default_scan_orders[tx].scan;
}

int main(void) {
  /* 8-bit, then 10- and 12-bit; dense blocks, then sparse ones. */
  for (int bd = 8; bd <= 12; bd += 2) {
    for (int tx = 0; tx < 4; ++tx) {
      const int n = 4 << tx;
      const int max_type = tx == 3 ? 1 : 4;
      for (int tx_type = 0; tx_type < max_type; ++tx_type) {
        for (int sparse = 0; sparse < 2; ++sparse) {
          uint64_t h = 0xcbf29ce484222325ull;
          for (int k = 0; k < 64; ++k) {
            tran_low_t in[32 * 32] __attribute__((aligned(32)));
            uint8_t d8[32 * 32];
            uint16_t d16[32 * 32];
            const int32_t b =
                bd == 8 ? bound8[tx] : bound8[tx] << (bd - 8);
            int eob = n * n;
            memset(in, 0, sizeof(in));
            if (sparse) {
              /* DC only, a few, or the 12/10/34/38/135 boundaries. */
              static const int eobs[] = { 1, 2, 10, 12, 34, 38, 135 };
              eob = eobs[next() % 7];
              if (eob > n * n) eob = n * n;
              for (int c = 0; c < eob; ++c) in[scan_of(tx)[c]] = coef(b);
            } else {
              for (int c = 0; c < n * n; ++c) in[c] = coef(b);
            }
            for (int c = 0; c < n * n; ++c) {
              const uint32_t r = next();
              d8[c] = (uint8_t)r;
              d16[c] = (uint16_t)(r & ((1u << bd) - 1));
            }
            if (bd == 8) {
              switch (tx) {
                case 0: vp9_iht4x4_add(tx_type, in, d8, n, eob); break;
                case 1: vp9_iht8x8_add(tx_type, in, d8, n, eob); break;
                case 2: vp9_iht16x16_add(tx_type, in, d8, n, eob); break;
                default: vp9_idct32x32_add(in, d8, n, eob); break;
              }
              h = fnv(d8, (size_t)(n * n), h);
            } else {
              switch (tx) {
                case 0:
                  vp9_highbd_iht4x4_add(tx_type, in, d16, n, eob, bd);
                  break;
                case 1:
                  vp9_highbd_iht8x8_add(tx_type, in, d16, n, eob, bd);
                  break;
                case 2:
                  vp9_highbd_iht16x16_add(tx_type, in, d16, n, eob, bd);
                  break;
                default: vp9_highbd_idct32x32_add(in, d16, n, eob, bd); break;
              }
              h = fnv((const uint8_t *)d16, (size_t)(n * n) * 2, h);
            }
          }
          printf("%d %d %d %d 0x%016llxu64\n", bd, tx, tx_type, sparse,
                 (unsigned long long)h);
        }
      }
    }
    /* The lossless Walsh-Hadamard transform. */
    {
      uint64_t h = 0xcbf29ce484222325ull;
      for (int k = 0; k < 64; ++k) {
        tran_low_t in[16] __attribute__((aligned(32)));
        uint8_t d8[16];
        uint16_t d16[16];
        const int eob = (k & 1) ? 1 : 16;
        memset(in, 0, sizeof(in));
        for (int c = 0; c < eob; ++c)
          in[scan_of(0)[c]] = coef(bd == 8 ? 1020 : 1020 << (bd - 8));
        for (int c = 0; c < 16; ++c) {
          const uint32_t r = next();
          d8[c] = (uint8_t)r;
          d16[c] = (uint16_t)(r & ((1u << bd) - 1));
        }
        if (bd == 8) {
          vp9_iwht4x4_add(in, d8, 4, eob);
          h = fnv(d8, 16, h);
        } else {
          vp9_highbd_iwht4x4_add(in, d16, 4, eob, bd);
          h = fnv((const uint8_t *)d16, 32, h);
        }
      }
      printf("%d wht 0x%016llxu64\n", bd, (unsigned long long)h);
    }
  }
  return 0;
}
