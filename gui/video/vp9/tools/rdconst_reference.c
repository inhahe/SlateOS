/*
 * libvpx's floating-point rate-distortion constants at every quantiser
 * index, as glibc computes them, hashed: the reference `src/enc/rd.rs`'s
 * `the_floating_point_constants_are_glibcs` test compares against.
 *
 * Three tables, each the same expression libvpx evaluates, against its own
 * quantiser tables (8-bit):
 *
 * - `compute_rd_thresh_factor` (`vp9_rd.c`): the mode threshold factor,
 *   `max(8, (int)(pow(dc_quant / 4.0, 1.25) * 5.12))`;
 * - `init_me_luts_bd` (`vp9_rd.c`): `sad_per_bit16`,
 *   `(int)(0.0418 * ac_quant / 4.0 + 2.4107)`;
 * - `vp9_compute_rd_mult_based_on_qindex` (`vp9_rd.c`) for an inter frame:
 *   `(int)(dc_quant^2 * (4.15 + 0.001 * qindex))`, at least 1.
 *
 * Each hash is 64-bit FNV-1a over the 256 values' little-endian bytes.
 *
 * Build against a libvpx v1.17.0 tree (only its quantiser tables are used):
 *
 *   cc -O1 -I$BUILD -I$LIBVPX rdconst_reference.c \
 *     $LIBVPX/vp9/common/vp9_quant_common.c -lm && ./a.out
 *
 * With glibc 2.39 it prints thresh=10639358763122460441,
 * spb=11837657131020772153 and rdm=4301386624769563932.
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "vp9/common/vp9_quant_common.h"

static uint64_t fnv(uint64_t h, int v) {
  unsigned char b[4];
  int k;
  memcpy(b, &v, 4);
  for (k = 0; k < 4; ++k) {
    h ^= b[k];
    h *= 0x100000001b3ULL;
  }
  return h;
}

int main(void) {
  uint64_t h_thresh = 0xcbf29ce484222325ULL;
  uint64_t h_spb = 0xcbf29ce484222325ULL;
  uint64_t h_rdm = 0xcbf29ce484222325ULL;
  int i;
  for (i = 0; i < 256; ++i) {
    const double q = vp9_dc_quant(i, 0, VPX_BITS_8) / 4.0;
    const int t = (int)(pow(q, 1.25) * 5.12);
    const int thresh = t > 8 ? t : 8;
    const double aq = vp9_ac_quant(i, 0, VPX_BITS_8) / 4.0;
    const int spb = (int)(0.0418 * aq + 2.4107);
    const int dq = vp9_dc_quant(i, 0, VPX_BITS_8);
    int rdm = (int)((double)(dq * dq) * (4.15 + 0.001 * (double)i) * 1.0);
    if (rdm < 1) rdm = 1;
    h_thresh = fnv(h_thresh, thresh);
    h_spb = fnv(h_spb, spb);
    h_rdm = fnv(h_rdm, rdm);
    if (i == 0 || i == 100 || i == 200 || i == 255)
      printf("q=%d thresh=%d spb=%d rdm=%d\n", i, thresh, spb, rdm);
  }
  printf("thresh=%llu\nspb=%llu\nrdm=%llu\n", (unsigned long long)h_thresh,
         (unsigned long long)h_spb, (unsigned long long)h_rdm);
  return 0;
}
