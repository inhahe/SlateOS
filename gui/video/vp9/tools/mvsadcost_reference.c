/*
 * libvpx's motion search SAD cost table, as glibc computes it, hashed: the
 * reference `src/enc/mcomp.rs`'s `the_sad_cost_table_is_libvpxs` test
 * compares against.
 *
 * libvpx's `cal_nmvsadcosts` (`vp9_encoder.c`) fills `nmvsadcost[0][i]` for
 * i = 1 to MV_MAX with `(int)(256 * (2 * (log2f(8 * i) + .6)))` -- a float
 * logarithm promoted to double -- so the table is whatever the C library's
 * `log2f` gives. The hash is 64-bit FNV-1a over each value's four
 * little-endian bytes; a few values are printed too.
 *
 * Needs no libvpx (the expression is copied whole):
 *
 *   cc -O1 mvsadcost_reference.c -lm && ./a.out
 *
 * With glibc 2.39 it prints fnv(le bytes)=10080772854876135856.
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  uint64_t h_bytes = 0xcbf29ce484222325ULL;
  int i;
  for (i = 1; i <= 16383; ++i) {
    double z = 256 * (2 * (log2f(8 * i) + .6));
    int v = (int)z;
    unsigned char b[4];
    size_t k;
    memcpy(b, &v, 4);
    for (k = 0; k < 4; ++k) {
      h_bytes ^= b[k];
      h_bytes *= 0x100000001b3ULL;
    }
    if (i == 1 || i == 2 || i == 3 || i == 100 || i == 16383)
      printf("i=%d v=%d\n", i, v);
  }
  printf("fnv(le bytes)=%llu\n", (unsigned long long)h_bytes);
  return 0;
}
