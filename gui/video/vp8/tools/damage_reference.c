/*
 * What libvpx's VP8 decoder does with a damaged stream, frame by frame: the
 * reference `tests/damage.rs` holds the crate to.
 *
 *     damage_reference VECTOR.ivf [MUTATION...]
 *
 * Each MUTATION damages one frame before it is decoded:
 *
 *     flip:F:O:X   XOR byte O of frame F with X (all decimal)
 *     cut:F:N      keep only the first N bytes of frame F
 *     drop:F       leave frame F out
 *     limit:N      stop after the first N frames
 *
 * and for each frame decoded, one line:
 *
 *     ok MD5 C     the frame showed a picture, hashing to MD5 as libvpx's
 *                  test/md5_helper.h hashes it; C is 1 if libvpx marks it
 *                  corrupt (VP8D_GET_FRAME_CORRUPTED)
 *     none         it decoded and showed nothing
 *     badcopy      it decoded, was meant to be shown, and showed nothing:
 *                  libvpx's answer to a reference copied from buffer "3"
 *     err CODE     vpx_codec_decode failed with CODE
 *
 * The decoder is libvpx's single-threaded one, error concealment off, as
 * the crate's is. Build against a libvpx v1.17.0 configured with
 *
 *     ../libvpx/configure --target=generic-gnu --disable-vp9 \
 *         --disable-vp8-encoder --disable-unit-tests --disable-docs
 *     make
 *
 * from that build directory:
 *
 *     cc -O1 -I. -I../libvpx damage_reference.c md5_utils.c.o libvpx.a \
 *         -lm -lpthread -o damage_reference
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "md5_utils.h"
#include "vpx/vp8dx.h"
#include "vpx/vpx_decoder.h"

static unsigned char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  unsigned char *data;
  long n;
  if (!f) return NULL;
  fseek(f, 0, SEEK_END);
  n = ftell(f);
  fseek(f, 0, SEEK_SET);
  data = malloc((size_t)n);
  if (!data || fread(data, 1, (size_t)n, f) != (size_t)n) {
    fclose(f);
    return NULL;
  }
  fclose(f);
  *len = (size_t)n;
  return data;
}

static unsigned le32(const unsigned char *p) {
  return p[0] | p[1] << 8 | p[2] << 16 | (unsigned)p[3] << 24;
}

static void picture_md5(const vpx_image_t *img, char out[33]) {
  MD5Context md5;
  unsigned char digest[16];
  int plane, y, i;
  MD5Init(&md5);
  for (plane = 0; plane < 3; ++plane) {
    const unsigned char *buf = img->planes[plane];
    const int h = plane ? (img->d_h + img->y_chroma_shift) >> img->y_chroma_shift
                        : (int)img->d_h;
    const int w = plane ? (img->d_w + img->x_chroma_shift) >> img->x_chroma_shift
                        : (int)img->d_w;
    for (y = 0; y < h; ++y) {
      MD5Update(&md5, buf, w);
      buf += img->stride[plane];
    }
  }
  MD5Final(digest, &md5);
  for (i = 0; i < 16; ++i) sprintf(out + 2 * i, "%02x", digest[i]);
}

int main(int argc, char **argv) {
  size_t len, pos, header;
  unsigned char *data;
  vpx_codec_ctx_t codec;
  vpx_codec_dec_cfg_t cfg = { 1, 0, 0 };
  int frame = 0;
  int limit = 1 << 30;
  if (argc < 2 || !(data = read_file(argv[1], &len)) || len < 32) {
    fprintf(stderr, "usage: damage_reference VECTOR.ivf [MUTATION...]\n");
    return 2;
  }
  if (vpx_codec_dec_init(&codec, vpx_codec_vp8_dx(), &cfg, 0)) {
    fprintf(stderr, "cannot initialise the decoder\n");
    return 2;
  }
  header = data[6] | data[7] << 8;
  pos = header < 32 ? 32 : header;
  for (int a = 2; a < argc; ++a) sscanf(argv[a], "limit:%d", &limit);
  while (pos + 12 <= len && frame < limit) {
    size_t size = le32(data + pos);
    unsigned char *buf;
    int a, skip = 0;
    pos += 12;
    if (size > len - pos) break;
    buf = malloc(size ? size : 1);
    memcpy(buf, data + pos, size);
    pos += size;
    for (a = 2; a < argc; ++a) {
      int f, o, x, n;
      if (sscanf(argv[a], "flip:%d:%d:%d", &f, &o, &x) == 3 && f == frame) {
        if ((size_t)o < size) buf[o] ^= (unsigned char)x;
      } else if (sscanf(argv[a], "cut:%d:%d", &f, &n) == 2 && f == frame) {
        if ((size_t)n < size) size = (size_t)n;
      } else if (sscanf(argv[a], "drop:%d", &f) == 1 && f == frame) {
        skip = 1;
      }
    }
    if (!skip) {
      vpx_codec_err_t err = vpx_codec_decode(&codec, buf, (unsigned)size, NULL, 0);
      if (err) {
        printf("err %d\n", (int)err);
      } else {
        vpx_codec_iter_t iter = NULL;
        vpx_image_t *img = vpx_codec_get_frame(&codec, &iter);
        if (img) {
          char md5[33];
          int corrupted = 0;
          vpx_codec_control(&codec, VP8D_GET_FRAME_CORRUPTED, &corrupted);
          picture_md5(img, md5);
          printf("ok %s %d\n", md5, corrupted ? 1 : 0);
        } else if (size > 0 && (buf[0] >> 4) & 1) {
          printf("badcopy\n");
        } else {
          printf("none\n");
        }
      }
    }
    free(buf);
    ++frame;
  }
  vpx_codec_destroy(&codec);
  free(data);
  return 0;
}
