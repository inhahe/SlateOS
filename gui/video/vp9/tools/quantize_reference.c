/*
 * libvpx's quantisers on seeded random blocks, hashed: the reference
 * `src/enc/quantize.rs`'s `matches_libvpx_*` test compares against.
 *
 * Each case fills a residual block from a linear congruential generator (the
 * same one the Rust test runs) -- random in [-255, 255], or one block in
 * eight at the extremes -- transforms it with libvpx's C DCT, and quantises
 * the coefficients with libvpx's tables (vp9_init_quantizer, 8-bit, no
 * deltas, sharpness 0) at one of a spread of quantiser indices. It hashes
 * the levels, the reconstructions (each as a little-endian 32-bit integer)
 * and the end of block with 64-bit FNV-1a.
 *
 * Build against a libvpx v1.17.0 tree configured with the VP9 encoder and
 * without high bit depth (vpxenc's default build):
 *
 *   cc -O1 -I$BUILD -I$LIBVPX quantize_reference.c $BUILD/libvpx.a -lm -lpthread
 *   ./a.out
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "./vpx_config.h"
#include "./vp9_rtcd.h"
#include "./vpx_dsp_rtcd.h"
#include "vp9/common/vp9_scan.h"
#include "vp9/encoder/vp9_encoder.h"
#include "vp9/encoder/vp9_quantize.h"

static uint32_t seed = 0x9a4a7123u;
static uint32_t next(void) {
  seed = seed * 1664525u + 1013904223u;
  return seed >> 8;
}

static uint64_t fnv_bytes(const uint8_t *p, size_t n, uint64_t h) {
  for (size_t i = 0; i < n; ++i) {
    h ^= p[i];
    h *= 0x100000001b3ull;
  }
  return h;
}

static uint64_t fnv_coefs(const tran_low_t *c, int n, uint64_t h) {
  for (int i = 0; i < n; ++i) {
    const uint32_t v = (uint32_t)(int32_t)c[i];
    uint8_t b[4] = { v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff, v >> 24 };
    h = fnv_bytes(b, 4, h);
  }
  return h;
}

static void fill(int16_t *block, int n) {
  const int extreme = next() % 8 == 0;
  for (int r = 0; r < n; ++r)
    for (int c = 0; c < n; ++c)
      block[r * 64 + c] = extreme ? ((next() & 1) ? 255 : -255)
                                  : (int16_t)((int)(next() % 511) - 255);
}

static const int qindices[] = { 0, 1, 4, 20, 60, 100, 140, 180, 220, 255 };

int main(void) {
  VP9_COMP *cpi = calloc(1, sizeof(*cpi));
  cpi->common.bit_depth = VPX_BITS_8;
  vp9_init_quantizer(cpi);
  static int16_t block[64 * 64];
  static tran_low_t coeff[1024], qcoeff[1024], dqcoeff[1024];
  /* (quantiser, tx size, hash): quantiser 0 the fast one, 1 the regular. */
  for (int quantiser = 0; quantiser < 2; ++quantiser) {
    for (int tx = 0; tx < 4; ++tx) {
      const int n = 4 << tx;
      uint64_t h = 0xcbf29ce484222325ull;
      for (int qi = 0; qi < (int)(sizeof(qindices) / sizeof(qindices[0])); ++qi) {
        const int q = qindices[qi];
        struct macroblock_plane p;
        memset(&p, 0, sizeof(p));
        p.quant = cpi->quants.y_quant[q];
        p.quant_fp = cpi->quants.y_quant_fp[q];
        p.round_fp = cpi->quants.y_round_fp[q];
        p.quant_shift = cpi->quants.y_quant_shift[q];
        p.zbin = cpi->quants.y_zbin[q];
        p.round = cpi->quants.y_round[q];
        const int16_t *dequant = cpi->y_dequant[q];
        const ScanOrder *so = &vp9_default_scan_orders[tx];
        for (int b = 0; b < 40; ++b) {
          uint16_t eob = 0;
          fill(block, n);
          switch (tx) {
            case 0: vpx_fdct4x4_c(block, coeff, 64); break;
            case 1: vpx_fdct8x8_c(block, coeff, 64); break;
            case 2: vpx_fdct16x16_c(block, coeff, 64); break;
            default: vpx_fdct32x32_c(block, coeff, 64); break;
          }
          if (quantiser == 0) {
            if (tx == 3)
              vp9_quantize_fp_32x32_c(coeff, n * n, &p, qcoeff, dqcoeff, dequant, &eob, so);
            else
              vp9_quantize_fp_c(coeff, n * n, &p, qcoeff, dqcoeff, dequant, &eob, so);
          } else {
            if (tx == 3)
              vpx_quantize_b_32x32_c(coeff, &p, qcoeff, dqcoeff, dequant, &eob, so);
            else
              vpx_quantize_b_c(coeff, n * n, &p, qcoeff, dqcoeff, dequant, &eob, so);
          }
          h = fnv_coefs(qcoeff, n * n, h);
          h = fnv_coefs(dqcoeff, n * n, h);
          uint8_t e[2] = { eob & 0xff, eob >> 8 };
          h = fnv_bytes(e, 2, h);
        }
      }
      printf("        (%d, %d, 0x%016llxu64),\n", quantiser, tx, (unsigned long long)h);
    }
  }
  free(cpi);
  return 0;
}
