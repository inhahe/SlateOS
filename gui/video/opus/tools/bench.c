/* tests/bench.rs's measurement made of libopus: every stream in a directory
 * (NAME.bit, and NAME.head for a multistream one) decoded at 48 kHz, the
 * single streams in stereo, each OPUS_BENCH_PASSES times (default 3) with a
 * fresh decoder, the fastest kept, only decoding timed; then the same groups
 * as the Rust bench's, in times real time.
 *
 *   L=<libopus 1.5.2, configured --enable-fixed-point and built>
 *   cc -O2 -I$L/include bench.c $L/.libs/libopus.a -lm -o bench
 *   ./bench tests/data
 */
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "opus.h"
#include "opus_multistream.h"
#include "opus_projection.h"

static unsigned char *read_file(const char *path, long *size) {
  FILE *f = fopen(path, "rb");
  if (!f) return NULL;
  fseek(f, 0, SEEK_END);
  *size = ftell(f);
  fseek(f, 0, SEEK_SET);
  unsigned char *p = malloc(*size > 0 ? *size : 1);
  if (fread(p, 1, *size, f) != (size_t)*size) { perror(path); exit(1); }
  fclose(f);
  return p;
}

static int by_name(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec + t.tv_nsec * 1e-9;
}

static const char *group(const char *name, int multistream) {
  if (multistream) return "multistream";
  if (!strncmp(name, "silk", 4) || !strncmp(name, "hybrid", 6)) return "silk+hybrid";
  if (!strncmp(name, "celt", 4)) return "celt";
  return "mixed";
}

int main(int argc, char **argv) {
  if (argc != 2) { fprintf(stderr, "usage: bench DIR\n"); return 1; }
  const char *env = getenv("OPUS_BENCH_PASSES");
  int passes = env ? atoi(env) : 3;
  DIR *d = opendir(argv[1]);
  if (!d) { perror(argv[1]); return 1; }
  char *names[256];
  int n = 0;
  struct dirent *e;
  while ((e = readdir(d)) && n < 256) {
    size_t len = strlen(e->d_name);
    if (len > 4 && !strcmp(e->d_name + len - 4, ".bit")) names[n++] = strndup(e->d_name, len - 4);
  }
  closedir(d);
  qsort(names, n, sizeof names[0], by_name);
  const char *groups[] = {"silk+hybrid", "celt", "mixed", "multistream"};
  double sound[4] = {0}, spent[4] = {0};
  short *pcm = malloc(5760 * 255 * sizeof(short));
  for (int s = 0; s < n; s++) {
    char path[4096];
    long bit_size, head_size = 0;
    snprintf(path, sizeof path, "%s/%s.bit", argv[1], names[s]);
    unsigned char *bit = read_file(path, &bit_size);
    snprintf(path, sizeof path, "%s/%s.head", argv[1], names[s]);
    unsigned char *head = read_file(path, &head_size);
    double best = 1e30;
    long samples = 0;
    for (int pass = 0; pass < passes; pass++) {
      int err;
      OpusDecoder *single = NULL;
      OpusMSDecoder *ms = NULL;
      OpusProjectionDecoder *proj = NULL;
      if (head) {
        int channels = head[9], family = head[18], streams = head[19], coupled = head[20];
        if (family == 3)
          proj = opus_projection_decoder_create(48000, channels, streams, coupled, head + 21,
                                                2 * channels * (streams + coupled), &err);
        else
          ms = opus_multistream_decoder_create(48000, channels, streams, coupled, head + 21, &err);
      } else {
        single = opus_decoder_create(48000, 2, &err);
      }
      if (err) { fprintf(stderr, "%s: %s\n", names[s], opus_strerror(err)); return 1; }
      samples = 0;
      double start = now();
      for (long at = 0; at + 8 <= bit_size;) {
        int len = bit[at] << 24 | bit[at + 1] << 16 | bit[at + 2] << 8 | bit[at + 3];
        at += 8;
        int got;
        if (single) got = opus_decode(single, bit + at, len, pcm, 5760, 0);
        else if (ms) got = opus_multistream_decode(ms, bit + at, len, pcm, 5760, 0);
        else got = opus_projection_decode(proj, bit + at, len, pcm, 5760, 0);
        if (got < 0) { fprintf(stderr, "%s: %s\n", names[s], opus_strerror(got)); return 1; }
        samples += got;
        at += len;
      }
      double t = now() - start;
      if (t < best) best = t;
      if (single) opus_decoder_destroy(single);
      if (ms) opus_multistream_decoder_destroy(ms);
      if (proj) opus_projection_decoder_destroy(proj);
    }
    double seconds = samples / 48000.0;
    printf("%-20s %6.1f s of sound: %7.0fx real time\n", names[s], seconds, seconds / best);
    const char *g = group(names[s], head != NULL);
    for (int i = 0; i < 4; i++)
      if (!strcmp(g, groups[i])) { sound[i] += seconds; spent[i] += best; }
    free(bit);
    free(head);
  }
  double all_sound = 0, all_spent = 0;
  for (int i = 0; i < 4; i++) {
    printf("%-12s %6.1f s of sound in %.3f s: %7.0fx real time\n", groups[i], sound[i], spent[i], sound[i] / spent[i]);
    all_sound += sound[i];
    all_spent += spent[i];
  }
  printf("%-12s %6.1f s of sound in %.3f s: %7.0fx real time\n", "all", all_sound, all_spent, all_sound / all_spent);
  return 0;
}
