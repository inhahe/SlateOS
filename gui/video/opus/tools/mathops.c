/* libopus's fixed-point math, over the inputs src/celt/mathops.rs's
 * `agree_with_libopus` test makes, digested: a line `NAME COUNT FNV1A64`
 * for each function -- the digest of its results, each as 4 little-endian
 * bytes -- which the test holds its own results to.
 *
 * Every input set is every value of a 16-bit domain, or values from the
 * xorshift generator below, one draw a statement so that the order of draws
 * is C's as well as Rust's.
 *
 *   L=<libopus 1.5.2, configured --enable-fixed-point and built>
 *   cc -O2 -I$L -I$L/include -I$L/celt mathops.c $L/.libs/libopus.a -lm -o mathops
 */
#include "config.h"
#include <stdio.h>
#include "celt/mathops.h"

short bitexact_cos(short x);
int bitexact_log2tan(int isin, int icos);

static unsigned long long s = 0x9E3779B97F4A7C15ull;
static unsigned rnd(void) {
  s ^= s << 13;
  s ^= s >> 7;
  s ^= s << 17;
  return (unsigned)(s >> 16);
}

/* A positive value of any magnitude: 31 random bits shifted right 0 to 30. */
static int any_magnitude(void) {
  unsigned bits = rnd();
  unsigned shift = rnd();
  return (int)(bits & 0x7fffffff) >> (shift % 31);
}

static unsigned long long digest;
static long count;
static void start(void) { digest = 0xCBF29CE484222325ull; count = 0; }
static void add(int v) {
  unsigned u = (unsigned)v;
  for (int i = 0; i < 4; i++) {
    digest ^= (u >> (8 * i)) & 0xff;
    digest *= 0x100000001B3ull;
  }
  count++;
}
static void end(const char *name) { printf("%s %ld %016llx\n", name, count, digest); }

int main(void) {
  int i;
  start();
  for (i = -32768; i < 32768; i++) add(celt_exp2((opus_val16)i));
  end("exp2");
  start();
  for (i = -32768; i < 32768; i++) add(celt_exp2_frac((opus_val16)i));
  end("exp2_frac");
  start();
  for (i = -32768; i < 32768; i++) add(bitexact_cos((short)i));
  end("bitexact_cos");
  start();
  for (i = 0; i < 131072; i++) add(celt_cos_norm(i));
  for (i = 0; i < 20000; i++) add(celt_cos_norm((int)rnd()));
  end("cos_norm");
  start();
  for (i = 16384; i < 65536; i++) add(celt_rsqrt_norm(i));
  end("rsqrt_norm");
  start();
  for (i = 0; i < 65536; i++) add(celt_sqrt(i));
  for (i = 0; i < 200000; i++) add(celt_sqrt(any_magnitude()));
  end("sqrt");
  start();
  for (i = 1; i < 65536; i++) add(celt_rcp(i));
  for (i = 0; i < 200000; i++) {
    int x = any_magnitude();
    if (x > 0) add(celt_rcp(x));
  }
  end("rcp");
  start();
  for (i = 0; i < 200000; i++) {
    int a = any_magnitude();
    int b = any_magnitude();
    if (rnd() & 1) a = -a;
    if (b > 0 && a < b && a > -b) add(frac_div32(a, b));
  }
  end("frac_div32");
  start();
  for (i = 0; i < 200000; i++) {
    unsigned v = rnd();
    unsigned shift = rnd();
    v >>= shift % 32;
    if (v) add((int)isqrt32(v));
  }
  end("isqrt32");
  start();
  for (i = 0; i < 200000; i++) {
    int a = 1 + (int)(rnd() % 32767);
    int b = 1 + (int)(rnd() % 32767);
    add(bitexact_log2tan(a, b));
  }
  end("bitexact_log2tan");
  return 0;
}
