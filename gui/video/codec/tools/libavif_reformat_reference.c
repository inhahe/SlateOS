// Frames of raw planar YUV converted to pixels by libavif: the reference
// `tests/frames.rs` holds a video's frames to. Each frame is handed to
// avifImageYUVToRGB exactly as libavif hands a decoded AVIF to it -- BGRA,
// 8 bits a channel, straight alpha, the default (automatic) chroma
// upsampling -- with the matrix, primaries and range given on the command
// line, which are the ones `src/colour.rs` resolves for the stream.
//
// The frames come from ffmpeg (`-f rawvideo`): planes Y, U, V, then A for a
// picture with alpha, each row packed, samples deeper than 8 bits as 16-bit
// little-endian -- ffmpeg's yuv*p, yuv*p10le, yuva*p and gbrp layouts (gbrp's
// planes are G, B and R, which is how VP9 stores its RGB: as Y, U and V).
//
// Build against libavif 1.3.0 with libyuv
// 644251f252a84bf8ce91ff0aca86a9b16b069ab8 (version 1924) whose x86 code is
// switched off -- libyuv's C, which design-decisions §1344 chose -- as
// gui/video/yuv/tools/libyuv_scale_reference.cc builds it; then:
/*
   cmake -S $LIBAVIF -B build-libavif -DCMAKE_BUILD_TYPE=Release \
     -DBUILD_SHARED_LIBS=OFF -DCMAKE_C_FLAGS="-O2 -DLIBYUV_DISABLE_X86" \
     -DAVIF_LIBYUV=SYSTEM -DLIBYUV_INCLUDE_DIR=$LIBYUV/include \
     -DLIBYUV_LIBRARY=$LIBYUV_C_BUILD/libyuv.a
   cmake --build build-libavif
   cc -O2 -I$LIBAVIF/include libavif_reformat_reference.c \
     build-libavif/libavif.a $LIBYUV_C_BUILD/libyuv.a -lstdc++ -lm -lpthread \
     -o libavif_reformat_reference
*/
// Usage:
//   libavif_reformat_reference W H FORMAT DEPTH MATRIX PRIMARIES RANGE ALPHA
//       < frames.raw > frames.bgra
// FORMAT is 444, 422, 420 or 400; DEPTH 8, 10 or 12; MATRIX and PRIMARIES
// are ITU-T H.273's numbers; RANGE is full or limited; ALPHA is 1 when each
// frame carries an alpha plane, else 0. Every whole frame on stdin is
// converted; a partial one at the end is an error.

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "avif/avif.h"

static int fail(const char *what) {
  fprintf(stderr, "libavif_reformat_reference: %s\n", what);
  return 1;
}

// Reads `rows` rows of `width` samples into a plane whose rows are
// `row_bytes` apart. 0 at a clean end of input before the first byte of the
// plane, 1 when the plane was read whole, -1 for a short read.
static int read_plane(uint8_t *plane, uint32_t row_bytes, uint32_t width,
                      uint32_t rows, uint32_t sample_bytes, int first) {
  const size_t want = (size_t)width * sample_bytes;
  for (uint32_t y = 0; y < rows; ++y) {
    const size_t got = fread(plane + (size_t)y * row_bytes, 1, want, stdin);
    if (got != want) {
      return (first && y == 0 && got == 0) ? 0 : -1;
    }
  }
  return 1;
}

int main(int argc, char **argv) {
  if (argc != 9) {
    return fail("usage: W H FORMAT DEPTH MATRIX PRIMARIES RANGE ALPHA");
  }
  const uint32_t width = (uint32_t)strtoul(argv[1], NULL, 10);
  const uint32_t height = (uint32_t)strtoul(argv[2], NULL, 10);
  const char *format_name = argv[3];
  const uint32_t depth = (uint32_t)strtoul(argv[4], NULL, 10);
  const int matrix = atoi(argv[5]);
  const int primaries = atoi(argv[6]);
  const int full = strcmp(argv[7], "full") == 0;
  const int alpha = atoi(argv[8]) != 0;
  if (!full && strcmp(argv[7], "limited") != 0) {
    return fail("RANGE is full or limited");
  }
  avifPixelFormat format;
  if (strcmp(format_name, "444") == 0) {
    format = AVIF_PIXEL_FORMAT_YUV444;
  } else if (strcmp(format_name, "422") == 0) {
    format = AVIF_PIXEL_FORMAT_YUV422;
  } else if (strcmp(format_name, "420") == 0) {
    format = AVIF_PIXEL_FORMAT_YUV420;
  } else if (strcmp(format_name, "400") == 0) {
    format = AVIF_PIXEL_FORMAT_YUV400;
  } else {
    return fail("FORMAT is 444, 422, 420 or 400");
  }
  if (width == 0 || height == 0 || (depth != 8 && depth != 10 && depth != 12)) {
    return fail("a size of nothing, or a depth that is not 8, 10 or 12");
  }
  const uint32_t sample_bytes = depth > 8 ? 2 : 1;

  for (uint64_t frame = 0;; ++frame) {
    avifImage *image = avifImageCreate(width, height, depth, format);
    if (!image) {
      return fail("avifImageCreate");
    }
    image->yuvRange = full ? AVIF_RANGE_FULL : AVIF_RANGE_LIMITED;
    image->matrixCoefficients = (avifMatrixCoefficients)matrix;
    image->colorPrimaries = (avifColorPrimaries)primaries;
    image->transferCharacteristics = AVIF_TRANSFER_CHARACTERISTICS_UNSPECIFIED;
    if (avifImageAllocatePlanes(image, alpha ? AVIF_PLANES_ALL : AVIF_PLANES_YUV) !=
        AVIF_RESULT_OK) {
      return fail("avifImageAllocatePlanes");
    }
    int planes = format == AVIF_PIXEL_FORMAT_YUV400 ? 1 : 3;
    int status = 1;
    for (int p = 0; p < planes && status == 1; ++p) {
      const uint32_t w = avifImagePlaneWidth(image, p);
      const uint32_t h = avifImagePlaneHeight(image, p);
      status = read_plane(avifImagePlane(image, p), avifImagePlaneRowBytes(image, p),
                          w, h, sample_bytes, p == 0);
    }
    if (status == 0) {
      avifImageDestroy(image);
      return 0;
    }
    if (status == 1 && alpha) {
      status = read_plane(avifImagePlane(image, AVIF_CHAN_A),
                          avifImagePlaneRowBytes(image, AVIF_CHAN_A), width, height,
                          sample_bytes, 0);
    }
    if (status != 1) {
      fprintf(stderr, "frame %llu: ", (unsigned long long)frame);
      return fail("the input ends inside a frame");
    }

    avifRGBImage rgb;
    avifRGBImageSetDefaults(&rgb, image);
    rgb.format = AVIF_RGB_FORMAT_BGRA;
    rgb.depth = 8;
    if (avifRGBImageAllocatePixels(&rgb) != AVIF_RESULT_OK) {
      return fail("avifRGBImageAllocatePixels");
    }
    const avifResult result = avifImageYUVToRGB(image, &rgb);
    if (result != AVIF_RESULT_OK) {
      fprintf(stderr, "frame %llu: %s\n", (unsigned long long)frame,
              avifResultToString(result));
      return 1;
    }
    for (uint32_t y = 0; y < height; ++y) {
      if (fwrite(rgb.pixels + (size_t)y * rgb.rowBytes, 1, (size_t)width * 4, stdout) !=
          (size_t)width * 4) {
        return fail("writing the pixels");
      }
    }
    avifRGBImageFreePixels(&rgb);
    avifImageDestroy(image);
  }
}
