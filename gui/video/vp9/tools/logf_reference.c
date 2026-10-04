/*
 * glibc's fma and logf, hashed: the references `src/enc/glibcmath.rs`'s
 * tests compare against.
 *
 * libvpx's learned partitioning takes logarithms with the C library's
 * `logf`. glibc 2.39's x86-64 `logf` is an ifunc that runs a build compiled
 * with FMA on any processor with FMA and AVX2, and that build rounds
 * differently from the plain one in the last bit now and then -- so run
 * this on such a processor, as the reference encodes were made. (Which
 * build runs can be read from the library: `objdump -d libm.so.6` at
 * `logf`'s resolver shows the two candidates, and the FMA one is the
 * sequence of `vfmadd` instructions `glibcmath.rs`'s `logf` follows.)
 *
 * Prints three 64-bit FNV-1a hashes over little-endian result bits:
 *   fma     -- fma(a, b, c) on a million triples of normal doubles from
 *              xorshift64* (seed 0x2545f4914f6cdd1d), exponents within
 *              2^-40..2^40 and either sign;
 *   strided -- logf on every 4099th float from 1 to 2^29;
 *   all     -- logf on every float from 1 to 2^29.
 *
 *   cc -O2 -fno-builtin logf_reference.c -lm && ./a.out
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static uint64_t fnv(uint64_t h, const void *p, size_t n) {
  const unsigned char *b = p;
  size_t i;
  for (i = 0; i < n; ++i) {
    h ^= b[i];
    h *= 0x100000001b3ULL;
  }
  return h;
}

static uint64_t state = 0x2545f4914f6cdd1dULL;

static uint64_t next(void) {
  state ^= state >> 12;
  state ^= state << 25;
  state ^= state >> 27;
  return state * 0x2545f4914f6cdd1dULL;
}

static double draw(void) {
  const uint64_t r = next();
  const uint64_t exp = 1023 - 40 + (r >> 52) % 80;
  const uint64_t bits = (r & 0x800fffffffffffffULL) | (exp << 52);
  double d;
  memcpy(&d, &bits, sizeof d);
  return d;
}

static uint32_t bits_of(float f) {
  uint32_t u;
  memcpy(&u, &f, sizeof u);
  return u;
}

static float float_of(uint32_t u) {
  float f;
  memcpy(&f, &u, sizeof f);
  return f;
}

int main(void) {
  uint64_t h_fma = 0xcbf29ce484222325ULL;
  uint64_t h_strided = 0xcbf29ce484222325ULL;
  uint64_t h_all = 0xcbf29ce484222325ULL;
  const uint32_t start = bits_of(1.0f), end = bits_of(536870912.0f);
  uint32_t b;
  int i;
  for (i = 0; i < 1000000; ++i) {
    const double a = draw();
    const double x = draw();
    const double c = draw();
    const double r = fma(a, x, c);
    uint64_t rb;
    memcpy(&rb, &r, sizeof rb);
    h_fma = fnv(h_fma, &rb, sizeof rb);
  }
  for (b = start; b <= end; b += 4099) {
    const uint32_t r = bits_of(logf(float_of(b)));
    h_strided = fnv(h_strided, &r, sizeof r);
  }
  for (b = start; b <= end; ++b) {
    const uint32_t r = bits_of(logf(float_of(b)));
    h_all = fnv(h_all, &r, sizeof r);
  }
  printf("fma=%llu\nstrided=%llu\nall=%llu\n", (unsigned long long)h_fma,
         (unsigned long long)h_strided, (unsigned long long)h_all);
  printf("logf(2)=%08x\n", bits_of(logf(2.0f)));
  return 0;
}
