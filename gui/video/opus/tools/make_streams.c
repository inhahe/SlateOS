/* The Opus streams the decoder's tests decode: libopus's encoder run over
 * signals made up here -- voice-like (a pitched pulse train through moving
 * formants, with breaths of noise and pauses of digital silence) and
 * music-like (chords of harmonics, with drum-like bursts) and two steady
 * tones (the peakiest spectrum, for the concealment's LPC) -- in every mode,
 * bandwidth and frame size, mono and stereo, loud and quiet, with DTX, in-band
 * FEC, CBR padding, mode switches and several frames a packet; and
 * multistream ones in each channel mapping family (1 surround, 2 and 3
 * ambisonics, 255 discrete with a silent channel). Nothing here is anyone's
 * recording, so the streams are this project's own.
 *
 * Writes NAME.bit (opus_demo's format: 4-byte length, 4-byte final range,
 * the packet) for each stream, and for the multistream ones NAME.head (the
 * OpusHead) too. The signals use libm, so another libm may make other
 * streams; the streams committed are the ones the tests' digests are of, so
 * regenerating them means regenerating the digests (tools/references.py).
 *
 *   L=<libopus 1.5.2, configured --enable-fixed-point and built>
 *   cc -O2 -I$L -I$L/include -I$L/src make_streams.c $L/.libs/libopus.a -lm \
 *      -o make_streams
 *   ./make_streams OUTDIR
 */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "opus.h"
#include "opus_multistream.h"
#include "opus_projection.h"
#include "opus_private.h"

#define FS 48000
#define MAX_PACKET 8000
#define PI 3.14159265358979323846

static unsigned long long rng_state = 0x2545F4914F6CDD1DULL;
static double noise(void) {
  rng_state ^= rng_state << 13;
  rng_state ^= rng_state >> 7;
  rng_state ^= rng_state << 17;
  return (double)(rng_state >> 11) / (double)(1ULL << 53) * 2.0 - 1.0;
}

static short clip(double s) {
  if (s > 32767) return 32767;
  if (s < -32768) return -32768;
  return (short)s;
}

/* A two-pole resonator. */
typedef struct { double b0, a1, a2, y1, y2; } Reson;
static void reson_set(Reson *r, double freq, double bw) {
  double rad = exp(-PI * bw / FS);
  r->a1 = 2 * rad * cos(2 * PI * freq / FS);
  r->a2 = -rad * rad;
  r->b0 = 1 - rad;
}
static double reson_run(Reson *r, double x) {
  double y = r->b0 * x + r->a1 * r->y1 + r->a2 * r->y2;
  r->y2 = r->y1;
  r->y1 = y;
  return y;
}

/* Voice-like: n samples into out (stride `stride`), pitch around `f0`,
 * scaled by `level`. Its pauses are digital silence. */
static void voice(short *out, int stride, long n, double f0, double seed_phase, double level) {
  Reson f1 = {0}, f2 = {0}, f3 = {0};
  double phase = seed_phase;
  for (long i = 0; i < n; i++) {
    double t = (double)i / FS;
    /* Syllables: 4 a second, with a pause every 1.7 s. */
    double syl = sin(2 * PI * 2.0 * t + seed_phase);
    double env = syl > 0 ? syl : 0;
    if (fmod(t + seed_phase, 1.7) > 1.45) env = 0;
    double pitch = f0 * (1 + 0.15 * sin(2 * PI * 0.7 * t + seed_phase));
    phase += pitch / FS;
    double pulse = 0;
    if (phase >= 1) { phase -= 1; pulse = 1; }
    /* Voiced most of the time, a breath of noise at each syllable's start. */
    double breath = fmod(t * 4 + seed_phase, 1.0) < 0.12 ? 0.3 * noise() : 0;
    double x = pulse * 3.0 + breath + 0.01 * noise();
    reson_set(&f1, 500 + 300 * sin(2 * PI * 1.3 * t), 80);
    reson_set(&f2, 1500 + 600 * sin(2 * PI * 0.9 * t + 1), 120);
    reson_set(&f3, 2600 + 400 * sin(2 * PI * 1.7 * t + 2), 160);
    double y = reson_run(&f1, x) * 1.0 + reson_run(&f2, x) * 0.6 + reson_run(&f3, x) * 0.3;
    out[i * stride] = clip(y * env * 6000 * level);
  }
}

/* Music-like: chords of harmonics changing twice a second, a drum burst
 * every 0.375 s, scaled by `level`. */
static void music(short *out, int stride, long n, double detune, double level) {
  static const double roots[] = {130.81, 174.61, 196.00, 146.83, 164.81, 220.00};
  double ph[4][6] = {{0}};
  double drum = 0;
  for (long i = 0; i < n; i++) {
    double t = (double)i / FS;
    int chord = (int)(t * 2) % 6;
    double s = 0;
    for (int v = 0; v < 4; v++) {
      double f = roots[(chord + v) % 6] * (v + 1) * 0.5 * (1 + detune);
      for (int h = 0; h < 6; h++) {
        ph[v][h] += f * (h + 1) / FS;
        if (ph[v][h] >= 1) ph[v][h] -= 1;
        s += sin(2 * PI * ph[v][h]) / (h + 1) / (h + 1);
      }
    }
    if (fmod(t, 0.375) < 1.0 / FS) drum = 1;
    s = s * 2500 + drum * noise() * 9000;
    drum *= 0.9992;
    out[i * stride] = clip(s * level);
  }
}

/* Tones: two steady sinusoids, 440 and 1250 Hz -- the peakiest spectrum
 * there is, which drives the concealment's LPC analysis to its limits. */
static void tones(short *out, int stride, long n, double level) {
  for (long i = 0; i < n; i++) {
    double t = (double)i / FS;
    double s = 12000 * sin(2 * PI * 440 * t) + 6000 * sin(2 * PI * 1250 * t);
    out[i * stride] = clip(s * level);
  }
}

static void put32(FILE *f, unsigned v) {
  unsigned char b[4] = {v >> 24, v >> 16, v >> 8, v};
  fwrite(b, 1, 4, f);
}

static FILE *open_out(const char *dir, const char *name, const char *ext) {
  char path[1024];
  snprintf(path, sizeof path, "%s/%s.%s", dir, name, ext);
  FILE *f = fopen(path, "wb");
  if (!f) { perror(path); exit(1); }
  return f;
}

static void put_packet(FILE *bit, const unsigned char *packet, int len, opus_uint32 rng) {
  put32(bit, len);
  put32(bit, rng);
  fwrite(packet, 1, len, bit);
}

/* A stream's settings: what the encoder is told, and how it changes. */
typedef struct {
  const char *name;
  int channels;
  int application;
  int force_mode;        /* MODE_SILK_ONLY etc, or OPUS_AUTO */
  int bandwidth;         /* OPUS_BANDWIDTH_*, or OPUS_AUTO */
  int bitrate;
  int frame_ms10;        /* frame size in tenths of a ms: 25 .. 1200 */
  int fec, loss, dtx, vbr, cvbr;
  int signal;            /* 0 voice, 1 music, 2 tones */
  int seconds;
  int sweep_to;          /* if not 0, the rate ramps 6 kb/s to this and back every 8 s */
  int frame_switch;      /* the frame size cycles through every one */
  double level;
  int force_stereo;      /* coded in stereo however few the bits */
} Stream;

static void encode_stream(const char *dir, const Stream *s) {
  int err;
  long n = (long)s->seconds * FS;
  short *pcm = calloc((size_t)n * s->channels, sizeof(short));
  for (int c = 0; c < s->channels; c++) {
    if (s->signal == 2) tones(pcm + c, s->channels, n, s->level);
    else if (s->signal == 1) music(pcm + c, s->channels, n, 0.003 * c, s->level);
    else voice(pcm + c, s->channels, n, c ? 180.0 : 120.0, 0.37 * c, s->level);
  }
  OpusEncoder *enc = opus_encoder_create(FS, s->channels, s->application, &err);
  if (err) { fprintf(stderr, "%s: %s\n", s->name, opus_strerror(err)); exit(1); }
  opus_encoder_ctl(enc, OPUS_SET_BITRATE(s->bitrate));
  opus_encoder_ctl(enc, OPUS_SET_COMPLEXITY(10));
  opus_encoder_ctl(enc, OPUS_SET_INBAND_FEC(s->fec));
  opus_encoder_ctl(enc, OPUS_SET_PACKET_LOSS_PERC(s->loss));
  opus_encoder_ctl(enc, OPUS_SET_DTX(s->dtx));
  opus_encoder_ctl(enc, OPUS_SET_VBR(s->vbr));
  opus_encoder_ctl(enc, OPUS_SET_VBR_CONSTRAINT(s->cvbr));
  if (s->bandwidth != OPUS_AUTO) opus_encoder_ctl(enc, OPUS_SET_BANDWIDTH(s->bandwidth));
  if (s->force_mode != OPUS_AUTO) opus_encoder_ctl(enc, OPUS_SET_FORCE_MODE(s->force_mode));
  if (s->force_stereo) opus_encoder_ctl(enc, OPUS_SET_FORCE_CHANNELS(2));
  FILE *bit = open_out(dir, s->name, "bit");
  /* Every frame size opus_encode takes; 80 ms and over are several frames
   * repacketized into one packet. */
  static const int cycle[] = {25, 50, 100, 200, 400, 600, 800, 1000, 1200, 200};
  const int cycle_len = (int)(sizeof cycle / sizeof cycle[0]);
  unsigned char packet[MAX_PACKET];
  long at = 0;
  int k = 0;
  while (1) {
    int ms10 = s->frame_switch ? cycle[k % cycle_len] : s->frame_ms10;
    int frame = FS * ms10 / 10000;
    if (at + frame > n) break;
    if (s->sweep_to) {
      double phase = fmod((double)at / FS, 8.0) / 8.0;
      double tri = phase < 0.5 ? phase * 2 : 2 - phase * 2;
      opus_encoder_ctl(enc, OPUS_SET_BITRATE((int)(6000 + tri * (s->sweep_to - 6000))));
    }
    int len = opus_encode(enc, pcm + at * s->channels, frame, packet, MAX_PACKET);
    if (len < 0) { fprintf(stderr, "%s: encode: %s\n", s->name, opus_strerror(len)); exit(1); }
    opus_uint32 rng;
    opus_encoder_ctl(enc, OPUS_GET_FINAL_RANGE(&rng));
    put_packet(bit, packet, len, rng);
    at += frame;
    k++;
  }
  fclose(bit);
  opus_encoder_destroy(enc);
  free(pcm);
  fprintf(stderr, "%s: %d packets\n", s->name, k);
}

static void write_head(const char *dir, const char *name, int channels, int family,
                       int streams, int coupled, const unsigned char *table, int table_len) {
  FILE *f = open_out(dir, name, "head");
  unsigned char h[19] = {'O','p','u','s','H','e','a','d', 1, channels, 0x38, 0x01,
                         0x80, 0xbb, 0, 0, 0, 0, family};
  fwrite(h, 1, 19, f);
  unsigned char sc[2] = {streams, coupled};
  fwrite(sc, 1, 2, f);
  fwrite(table, 1, table_len, f);
  fclose(f);
}

/* A multistream stream's settings. */
typedef struct {
  const char *name;
  int family;            /* 1, 2, 3 or 255 */
  int channels;
  int application;
  int bitrate;
  int fec, loss;
  int seconds;
} Multi;

/* The signal of channel c of a multistream stream: voice and music in turn,
 * each a little different, the last of six (5.1's LFE) quieter. */
static void multi_signal(short *pcm, int channels, long n) {
  for (int c = 0; c < channels; c++) {
    if (c % 2 == 0) voice(pcm + c, channels, n, 110 + 20 * c, 0.3 * c, 1.0);
    else music(pcm + c, channels, n, 0.002 * c, channels == 6 && c == 5 ? 0.25 : 1.0);
  }
}

static void encode_multi(const char *dir, const Multi *m) {
  int err, streams = 0, coupled = 0;
  long n = (long)m->seconds * FS;
  short *pcm = calloc((size_t)n * m->channels, sizeof(short));
  multi_signal(pcm, m->channels, n);
  OpusMSEncoder *ms = NULL;
  OpusProjectionEncoder *proj = NULL;
  unsigned char mapping[255];
  if (m->family == 3) {
    proj = opus_projection_ambisonics_encoder_create(FS, m->channels, 3, &streams, &coupled,
                                                     m->application, &err);
    if (err) { fprintf(stderr, "%s: %s\n", m->name, opus_strerror(err)); exit(1); }
    opus_projection_encoder_ctl(proj, OPUS_SET_BITRATE(m->bitrate));
    opus_projection_encoder_ctl(proj, OPUS_SET_INBAND_FEC(m->fec));
    opus_projection_encoder_ctl(proj, OPUS_SET_PACKET_LOSS_PERC(m->loss));
    opus_int32 size;
    opus_projection_encoder_ctl(proj, OPUS_PROJECTION_GET_DEMIXING_MATRIX_SIZE(&size));
    unsigned char *matrix = malloc(size);
    opus_projection_encoder_ctl(proj, OPUS_PROJECTION_GET_DEMIXING_MATRIX(matrix, size));
    write_head(dir, m->name, m->channels, 3, streams, coupled, matrix, size);
    free(matrix);
  } else {
    if (m->family == 255) {
      /* Three channels in two streams, one coupled, and a fourth silent
       * (mapped to 255): the layout only family 255 can say. */
      static const unsigned char discrete[4] = {0, 1, 255, 2};
      streams = 2;
      coupled = 1;
      memcpy(mapping, discrete, 4);
      ms = opus_multistream_encoder_create(FS, m->channels, streams, coupled, mapping,
                                           m->application, &err);
    } else {
      ms = opus_multistream_surround_encoder_create(FS, m->channels, m->family, &streams, &coupled,
                                                    mapping, m->application, &err);
    }
    if (err) { fprintf(stderr, "%s: %s\n", m->name, opus_strerror(err)); exit(1); }
    opus_multistream_encoder_ctl(ms, OPUS_SET_BITRATE(m->bitrate));
    opus_multistream_encoder_ctl(ms, OPUS_SET_INBAND_FEC(m->fec));
    opus_multistream_encoder_ctl(ms, OPUS_SET_PACKET_LOSS_PERC(m->loss));
    write_head(dir, m->name, m->channels, m->family, streams, coupled, mapping, m->channels);
  }
  FILE *bit = open_out(dir, m->name, "bit");
  unsigned char packet[MAX_PACKET];
  int k = 0;
  for (long at = 0; at + 960 <= n; at += 960, k++) {
    int len;
    opus_uint32 rng;
    if (proj) {
      len = opus_projection_encode(proj, pcm + at * m->channels, 960, packet, MAX_PACKET);
      opus_projection_encoder_ctl(proj, OPUS_GET_FINAL_RANGE(&rng));
    } else {
      len = opus_multistream_encode(ms, pcm + at * m->channels, 960, packet, MAX_PACKET);
      opus_multistream_encoder_ctl(ms, OPUS_GET_FINAL_RANGE(&rng));
    }
    if (len < 0) { fprintf(stderr, "%s: encode: %s\n", m->name, opus_strerror(len)); exit(1); }
    put_packet(bit, packet, len, rng);
  }
  fclose(bit);
  if (proj) opus_projection_encoder_destroy(proj);
  if (ms) opus_multistream_encoder_destroy(ms);
  free(pcm);
  fprintf(stderr, "%s: %d packets, %d streams, %d coupled\n", m->name, k, streams, coupled);
}

int main(int argc, char **argv) {
  if (argc != 2) { fprintf(stderr, "usage: make_streams OUTDIR\n"); return 1; }
  const char *dir = argv[1];
  const int V = OPUS_APPLICATION_VOIP, A = OPUS_APPLICATION_AUDIO, AUTO = OPUS_AUTO;
  const int SILK = MODE_SILK_ONLY, HYB = MODE_HYBRID, CELT = MODE_CELT_ONLY;
  const int NB = OPUS_BANDWIDTH_NARROWBAND, MB = OPUS_BANDWIDTH_MEDIUMBAND, WB = OPUS_BANDWIDTH_WIDEBAND;
  const int SWB = OPUS_BANDWIDTH_SUPERWIDEBAND, FB = OPUS_BANDWIDTH_FULLBAND;
  const Stream streams[] = {
    /* name              ch app force band   rate  ms10 fec loss dtx vbr cvbr sig sec sweep_to fsw level */
    {"silk_nb",           1, V, SILK, NB,     9000, 200, 1, 15, 1, 1, 0, 0,  5,     0, 0, 1.0},
    {"silk_mb_stereo",    2, V, SILK, MB,    24000, 400, 1, 20, 0, 1, 0, 0,  5,     0, 0, 1.0},
    {"silk_wb_60ms",      1, V, SILK, WB,    32000, 600, 1, 10, 0, 1, 1, 0,  5,     0, 0, 1.0},
    {"silk_wb_10ms_cbr",  2, V, SILK, WB,    24000, 100, 0,  0, 1, 0, 0, 0,  4,     0, 0, 1.0},
    {"hybrid_swb",        2, V, HYB,  SWB,   48000, 100, 1, 10, 0, 1, 0, 0,  5,     0, 0, 1.0},
    {"hybrid_fb",         1, A, HYB,  FB,    32000, 200, 1, 15, 0, 1, 0, 1,  5,     0, 0, 1.0},
    {"celt_fb_stereo",    2, A, CELT, FB,   128000, 200, 0,  0, 0, 1, 0, 1,  5,     0, 0, 1.0},
    {"celt_2_5ms",        1, A, CELT, FB,    64000,  25, 0,  0, 0, 0, 0, 1,  3,     0, 0, 1.0},
    {"celt_nb_5ms",       2, A, CELT, NB,    24000,  50, 0,  0, 0, 1, 1, 1,  4,     0, 0, 1.0},
    {"celt_wb_40ms",      1, A, CELT, WB,    32000, 400, 0,  0, 0, 1, 0, 1,  4,     0, 0, 1.0},
    {"celt_voice",        1, A, CELT, FB,    48000, 200, 0,  0, 1, 1, 0, 0,  4,     0, 0, 1.0},
    {"celt_voice_stereo", 2, A, CELT, FB,    40000, 100, 0,  0, 0, 1, 0, 0,  4,     0, 0, 1.0},
    {"celt_loud",         2, A, CELT, FB,    96000, 200, 0,  0, 0, 1, 0, 1,  3,     0, 0, 4.0},
    {"celt_quiet",        1, A, CELT, FB,    32000, 200, 0,  0, 0, 1, 0, 1,  3,     0, 0, 0.002},
    {"celt_tone",         1, A, CELT, FB,    64000, 200, 0,  0, 0, 1, 0, 2,  3,     0, 0, 1.0},
    {"celt_tone_stereo",  2, A, CELT, WB,    48000, 100, 0,  0, 0, 1, 0, 2,  3,     0, 0, 1.0},
    {"switching_voice",   2, V, AUTO, AUTO,  16000, 200, 1, 10, 1, 1, 0, 0, 16, 64000, 0, 1.0},
    {"switching_music",   2, A, AUTO, AUTO,  64000, 200, 1,  5, 1, 1, 0, 1, 16, 64000, 0, 1.0},
    {"switching_speech",  2, A, AUTO, AUTO,  32000, 200, 0,  0, 0, 1, 0, 0, 16, 96000, 0, 1.0},
    {"frame_sizes",       1, A, AUTO, AUTO,  48000, 200, 1,  5, 0, 1, 0, 0,  8,     0, 1, 1.0},
    {"frame_sizes_cbr",   2, V, AUTO, AUTO,  24000, 200, 1, 10, 0, 0, 0, 0,  8,     0, 1, 1.0},
  };
  for (size_t i = 0; i < sizeof streams / sizeof streams[0]; i++) encode_stream(dir, &streams[i]);
  const Multi multis[] = {
    /* name               family ch app rate   fec loss sec */
    {"surround",            1,  6, A, 192000, 0,  0, 4},
    {"surround_voice",      1,  6, V,  96000, 1, 10, 4},
    {"ambisonics_family2",  2,  4, A,  96000, 0,  0, 3},
    {"ambisonics",          3,  4, A, 128000, 0,  0, 4},
    {"ambisonics_2nd",      3, 11, A, 256000, 0,  0, 3},
    {"discrete",          255,  4, V,  40000, 1, 10, 4},
  };
  for (size_t i = 0; i < sizeof multis / sizeof multis[0]; i++) encode_multi(dir, &multis[i]);
  /* Streams added since go last: the noise generator is shared, so a stream
   * put among the others would change every one after it. */
  const Stream later[] = {
    /* Stereo CELT starved of bits: bands too poor for more than one step
     * of their split, where a stream may still ask for the side inverted. */
    {"celt_stereo_low",   2, A, CELT, FB,    12000, 200, 0,  0, 0, 1, 0, 1,  3,     0, 0, 1.0, 1},
  };
  for (size_t i = 0; i < sizeof later / sizeof later[0]; i++) encode_stream(dir, &later[i]);
  return 0;
}
