// libyuv's ScalePlane and ScalePlane_12 with kFilterBox on seeded planes,
// hashed: the reference `src/avif/scale.rs`'s tests compare against.
//
// Each case fills a source plane of 8-, 10- or 12-bit samples -- from
// xorshift32 seeded by the case, all at the depth's maximum, or a ramp (the
// same fills the Rust tests make) -- scales it to the destination size as
// libavif's avifImageScaleWithLimit does (ScalePlane for 8 bits, ScalePlane_12
// for deeper), and hashes the result with 64-bit FNV-1a, a byte a sample at
// 8 bits and two (little-endian) deeper.
//
// Build against libyuv 644251f252a84bf8ce91ff0aca86a9b16b069ab8 (version
// 1924, what libavif 1.4.2 pins) with its x86 code switched off, which is
// libyuv's C as MSVC's x86-64 build runs it:
//
//   cmake -S $LIBYUV -B build -DCMAKE_BUILD_TYPE=Release \
//     -DCMAKE_C_FLAGS="-O2 -DLIBYUV_DISABLE_X86" \
//     -DCMAKE_CXX_FLAGS="-O2 -DLIBYUV_DISABLE_X86" -DUNIT_TEST=OFF
//   cmake --build build --target yuv
//   c++ -std=c++17 -O2 -DLIBYUV_DISABLE_X86 -I$LIBYUV/include \
//     libyuv_scale_reference.cc build/libyuv.a -o libyuv_scale_reference
//
// (Without the two -DLIBYUV_DISABLE_X86 it builds against libyuv's SIMD,
// which is how the differences design-decisions §1344 lists were found.)
//
// Usage:
//   libyuv_scale_reference sweep N     one hash over every case with all four
//                                      sizes in 1..=N, random fill, per depth
//   libyuv_scale_reference sweeplist   the same over 25 sizes from 1 to 129
//   libyuv_scale_reference listcases8  each 8-bit sweeplist case's hash
//   libyuv_scale_reference cases       for each "D SW SH DW DH FILL" line on
//                                      stdin, the case as a Rust tuple with
//                                      its hash (scale.rs's CASES)
//   libyuv_scale_reference dump D SW SH DW DH FILL   one case's samples
//
// FILL is one of
//   rand  xorshift32 seeded from the case (seed_of), each sample the top D
//         bits of the next draw
//   max   every sample (1 << D) - 1
//   ramp  sample (x, y) = (x * 3 + y * 5) mod (1 << D)

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <string_view>
#include <vector>

#include "libyuv/scale.h"

namespace {

uint32_t seed_of(uint32_t depth, uint32_t sw, uint32_t sh, uint32_t dw,
                 uint32_t dh) {
  uint32_t s = sw;
  s = s * 31u + sh;
  s = s * 31u + dw;
  s = s * 31u + dh;
  s = s * 31u + depth;
  return s | 1u;
}

struct Rng {
  uint32_t x;
  uint32_t next() {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    return x;
  }
};

struct Fnv {
  uint64_t h = 0xcbf29ce484222325ull;
  void byte(uint8_t b) {
    h ^= b;
    h *= 0x100000001b3ull;
  }
};

template <typename T>
void fill(std::vector<T>& src, uint32_t depth, int sw, int sh, int dw, int dh,
          std::string_view pattern) {
  Rng rng{seed_of(depth, sw, sh, dw, dh)};
  uint32_t top = (1u << depth) - 1;
  for (int y = 0; y < sh; ++y)
    for (int x = 0; x < sw; ++x) {
      uint32_t v;
      if (pattern == "max")
        v = top;
      else if (pattern == "ramp")
        v = (uint32_t)(x * 3 + y * 5) & top;
      else
        v = rng.next() >> (32 - depth);
      src[(size_t)y * sw + x] = (T)v;
    }
}

// Scale one case; feed its output samples (little-endian for 16-bit) to fnv,
// or print them if dump.
int run(uint32_t depth, int sw, int sh, int dw, int dh,
        std::string_view pattern, Fnv* fnv, bool dump) {
  if (depth == 8) {
    std::vector<uint8_t> src((size_t)sw * sh);
    fill(src, depth, sw, sh, dw, dh, pattern);
    std::vector<uint8_t> dst((size_t)dw * dh);
    int r = libyuv::ScalePlane(src.data(), sw, sw, sh, dst.data(), dw, dw, dh,
                               libyuv::kFilterBox);
    if (r) return r;
    for (size_t i = 0; i < dst.size(); ++i) {
      if (dump) printf("%u%c", dst[i], (i + 1) % dw ? ' ' : '\n');
      if (fnv) fnv->byte(dst[i]);
    }
  } else {
    std::vector<uint16_t> src((size_t)sw * sh);
    fill(src, depth, sw, sh, dw, dh, pattern);
    std::vector<uint16_t> dst((size_t)dw * dh);
    int r = libyuv::ScalePlane_12(src.data(), sw, sw, sh, dst.data(), dw, dw,
                                  dh, libyuv::kFilterBox);
    if (r) return r;
    for (size_t i = 0; i < dst.size(); ++i) {
      if (dump) printf("%u%c", dst[i], (i + 1) % dw ? ' ' : '\n');
      if (fnv) {
        fnv->byte((uint8_t)dst[i]);
        fnv->byte((uint8_t)(dst[i] >> 8));
      }
    }
  }
  return 0;
}

const int kListSizes[] = {1,  2,  3,  4,  5,  6,  7,  8,  9,  12, 15, 16, 17,
                          24, 31, 32, 33, 48, 63, 64, 65, 96, 127, 128, 129};

}  // namespace

int main(int argc, char** argv) {
  if (argc == 3 && std::string_view(argv[1]) == "sweep") {
    int n = atoi(argv[2]);
    for (uint32_t depth : {8u, 10u, 12u}) {
      Fnv fnv;
      for (int sw = 1; sw <= n; ++sw)
        for (int sh = 1; sh <= n; ++sh)
          for (int dw = 1; dw <= n; ++dw)
            for (int dh = 1; dh <= n; ++dh)
              if (run(depth, sw, sh, dw, dh, "rand", &fnv, false)) return 1;
      printf("sweep %d depth %u %016llx\n", n, depth,
             (unsigned long long)fnv.h);
    }
    return 0;
  }
  if (argc == 2 && std::string_view(argv[1]) == "sweeplist") {
    for (uint32_t depth : {8u, 10u, 12u}) {
      Fnv fnv;
      for (int sw : kListSizes)
        for (int sh : kListSizes)
          for (int dw : kListSizes)
            for (int dh : kListSizes)
              if (run(depth, sw, sh, dw, dh, "rand", &fnv, false)) return 1;
      printf("sweeplist depth %u %016llx\n", depth, (unsigned long long)fnv.h);
    }
    return 0;
  }
  if (argc == 2 && std::string_view(argv[1]) == "listcases8") {
    for (int sw : kListSizes)
      for (int sh : kListSizes)
        for (int dw : kListSizes)
          for (int dh : kListSizes) {
            Fnv fnv;
            if (run(8, sw, sh, dw, dh, "rand", &fnv, false)) return 1;
            printf("%d %d %d %d %016llx\n", sw, sh, dw, dh,
                   (unsigned long long)fnv.h);
          }
    return 0;
  }
  if (argc == 2 && std::string_view(argv[1]) == "cases") {
    unsigned depth;
    int sw, sh, dw, dh;
    char pattern[16];
    while (scanf("%u %d %d %d %d %15s", &depth, &sw, &sh, &dw, &dh,
                 pattern) == 6) {
      Fnv fnv;
      if (run(depth, sw, sh, dw, dh, pattern, &fnv, false)) return 1;
      printf("    (%u, %d, %d, %d, %d, \"%s\", 0x%016llx),\n", depth, sw, sh,
             dw, dh, pattern, (unsigned long long)fnv.h);
    }
    return 0;
  }
  if (argc == 8 && std::string_view(argv[1]) == "dump") {
    uint32_t depth = (uint32_t)atoi(argv[2]);
    int sw = atoi(argv[3]), sh = atoi(argv[4]), dw = atoi(argv[5]),
        dh = atoi(argv[6]);
    return run(depth, sw, sh, dw, dh, argv[7], nullptr, true);
  }
  fprintf(stderr,
          "usage: harness sweep N | sweeplist | listcases8 | cases | "
          "dump D SW SH DW DH PATTERN\n");
  return 2;
}
