/* What libopus's decoders make of a stream, driven the way the tests drive
 * this crate's: the outputs the tests' digests (tests/data/references.txt)
 * are of.
 *
 * A single stream is decoded as `opus_demo -d` decodes it -- this is its
 * loop, which tools/references.py checks against opus_demo itself -- with,
 * optionally, what opus_demo has no option for:
 *
 *   -gain Q8DB   OPUS_SET_GAIN
 *   -noinv       OPUS_SET_PHASE_INVERSION_DISABLED(1)
 *   -float       opus_decode_float, the samples written as 32-bit floats
 *   -reset N     OPUS_RESET_STATE before every Nth packet
 *   -odd SEED    before each packet that arrives, a call a player would not
 *                make, drawn from SEED (see odd_call): concealment or FEC of
 *                any length, on the 2.5 ms grid or off it, or a buffer of
 *                any size
 *
 * A multistream one (-head NAME.head, its OpusHead) is decoded by libopus's
 * multistream decoder (mapping families 1, 2 and 255) or its projection
 * decoder (family 3), at the head's gain unless -gain says otherwise, in the
 * same loop less opus_demo's opus_packet_has_lbrr test (which reads only a
 * single stream's packets): a lost packet's audio is concealed when the next
 * one comes, the last of a run of them from that one's in-band FEC.
 *
 * Output is little-endian 16-bit samples, or 32-bit floats with -float; and
 * on stderr, for each decode call that failed or made no samples a line
 * `E PACKET CODE`, and after each packet a line `S STATE`: the decoder's
 * bandwidth, pitch (a single stream's), last packet's duration and final
 * range, colon-separated.
 *
 *   L=<libopus 1.5.2, configured --enable-fixed-point and built>
 *   cc -O2 -I$L/include reference.c $L/.libs/libopus.a -lm -o reference
 *   reference [options] [-head FILE] RATE CHANNELS IN.bit OUT
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "opus.h"
#include "opus_multistream.h"
#include "opus_projection.h"

#define MAX_PACKET 1500
#define MAX_FRAME (48000 * 2)

/* Whichever decoder the stream needs. */
typedef struct {
  OpusDecoder *single;
  OpusMSDecoder *ms;
  OpusProjectionDecoder *proj;
  int channels;
  int use_float;
} Dec;

static int dec_ctl_get(Dec *d, int request, opus_int32 *value) {
  if (d->single) return opus_decoder_ctl(d->single, request, value);
  if (d->ms) return opus_multistream_decoder_ctl(d->ms, request, value);
  return opus_projection_decoder_ctl(d->proj, request, value);
}

static int dec_ctl_set(Dec *d, int request, opus_int32 value) {
  if (d->single) return opus_decoder_ctl(d->single, request, value);
  if (d->ms) return opus_multistream_decoder_ctl(d->ms, request, value);
  return opus_projection_decoder_ctl(d->proj, request, value);
}

static int dec_reset(Dec *d) {
  if (d->single) return opus_decoder_ctl(d->single, OPUS_RESET_STATE);
  if (d->ms) return opus_multistream_decoder_ctl(d->ms, OPUS_RESET_STATE);
  return opus_projection_decoder_ctl(d->proj, OPUS_RESET_STATE);
}

/* Decodes into `out` (shorts or floats, as d->use_float says). */
static int dec_decode(Dec *d, const unsigned char *data, int len, void *out, int frame_size, int fec) {
  if (d->use_float) {
    float *f = out;
    if (d->single) return opus_decode_float(d->single, data, len, f, frame_size, fec);
    if (d->ms) return opus_multistream_decode_float(d->ms, data, len, f, frame_size, fec);
    return opus_projection_decode_float(d->proj, data, len, f, frame_size, fec);
  }
  short *s = out;
  if (d->single) return opus_decode(d->single, data, len, s, frame_size, fec);
  if (d->ms) return opus_multistream_decode(d->ms, data, len, s, frame_size, fec);
  return opus_projection_decode(d->proj, data, len, s, frame_size, fec);
}

static unsigned read32(const unsigned char *b) {
  return (unsigned)b[0] << 24 | (unsigned)b[1] << 16 | (unsigned)b[2] << 8 | b[3];
}

static unsigned char *read_file(const char *path, long *size) {
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); exit(1); }
  fseek(f, 0, SEEK_END);
  *size = ftell(f);
  fseek(f, 0, SEEK_SET);
  unsigned char *p = malloc(*size > 0 ? *size : 1);
  if (fread(p, 1, *size, f) != (size_t)*size) { perror(path); exit(1); }
  fclose(f);
  return p;
}

static void fail(const char *what, int err) {
  fprintf(stderr, "%s: %s\n", what, opus_strerror(err));
  exit(1);
}

static FILE *fout;
static void *out;
static size_t sample_size;
static long count;

/* A decode call's samples written, or its failure noted. */
static void emit(Dec *d, int got) {
  if (got > 0) {
    if (fwrite(out, sample_size * d->channels, got, fout) != (size_t)got) {
      fprintf(stderr, "Error writing.\n");
      exit(1);
    }
  } else {
    fprintf(stderr, "E %ld %d\n", count, got);
  }
}

/* -odd's generator: the damage pattern's (tools/references.py's Lcg). */
static unsigned long long odd_seed;
static unsigned odd_next(void) {
  odd_seed = (odd_seed * 1103515245 + 12345) & 0x7FFFFFFF;
  return (unsigned)(odd_seed >> 16);
}

/* A length of concealment or FEC: up to 49 steps of 2.5 ms, a quarter of
 * the time a little off the grid. */
static int odd_length(int rate) {
  int step = rate / 400;
  int n = step * (int)(odd_next() % 50);
  if (odd_next() % 4 == 0) n += (int)(odd_next() % (unsigned)step);
  return n;
}

/* -odd's call before a packet: one time in six concealment of an odd
 * length, one in six FEC of an odd length from this packet, one in six
 * this packet into a buffer of any size; otherwise none. */
static void odd_call(Dec *d, int rate, const unsigned char *data, int len) {
  unsigned what = odd_next() % 6;
  if (what == 0) {
    int n = odd_length(rate);
    emit(d, dec_decode(d, NULL, 0, out, n, 0));
  } else if (what == 1) {
    int n = odd_length(rate);
    emit(d, dec_decode(d, data, len, out, n, 1));
  } else if (what == 2) {
    int n = (int)(odd_next() % (MAX_FRAME + 1));
    emit(d, dec_decode(d, data, len, out, n, 0));
  }
}

int main(int argc, char **argv) {
  int args = 1, gain = 0, have_gain = 0, noinv = 0, use_float = 0, reset_every = 0, odd = 0;
  const char *head_path = NULL;
  while (args < argc && argv[args][0] == '-') {
    if (!strcmp(argv[args], "-gain") && args + 1 < argc) { gain = atoi(argv[args + 1]); have_gain = 1; args += 2; }
    else if (!strcmp(argv[args], "-noinv")) { noinv = 1; args++; }
    else if (!strcmp(argv[args], "-float")) { use_float = 1; args++; }
    else if (!strcmp(argv[args], "-reset") && args + 1 < argc) { reset_every = atoi(argv[args + 1]); args += 2; }
    else if (!strcmp(argv[args], "-odd") && args + 1 < argc) { odd = 1; odd_seed = strtoull(argv[args + 1], NULL, 10); args += 2; }
    else if (!strcmp(argv[args], "-head") && args + 1 < argc) { head_path = argv[args + 1]; args += 2; }
    else break;
  }
  if (argc - args != 4) {
    fprintf(stderr, "usage: reference [-gain N] [-noinv] [-float] [-reset N] [-odd SEED] [-head FILE] RATE CHANNELS IN.bit OUT\n");
    return 1;
  }
  opus_int32 rate = atoi(argv[args]);
  int channels = atoi(argv[args + 1]);
  int err;
  Dec d = {0};
  d.use_float = use_float;
  if (head_path) {
    long hsize;
    unsigned char *h = read_file(head_path, &hsize);
    if (hsize < 21 || memcmp(h, "OpusHead", 8)) { fprintf(stderr, "%s: not an OpusHead\n", head_path); return 1; }
    channels = h[9];
    int head_gain = (short)(h[16] | h[17] << 8);
    int family = h[18], streams = h[19], coupled = h[20];
    if (family == 3) {
      int size = 2 * channels * (streams + coupled);
      if (hsize != 21 + size) { fprintf(stderr, "%s: matrix of %ld bytes, not %d\n", head_path, hsize - 21, size); return 1; }
      d.proj = opus_projection_decoder_create(rate, channels, streams, coupled, h + 21, size, &err);
      if (err) fail("opus_projection_decoder_create", err);
    } else {
      if (hsize != 21 + channels) { fprintf(stderr, "%s: mapping of %ld bytes, not %d\n", head_path, hsize - 21, channels); return 1; }
      d.ms = opus_multistream_decoder_create(rate, channels, streams, coupled, h + 21, &err);
      if (err) fail("opus_multistream_decoder_create", err);
    }
    if ((err = dec_ctl_set(&d, OPUS_SET_GAIN_REQUEST, head_gain))) fail("OPUS_SET_GAIN", err);
    free(h);
  } else {
    d.single = opus_decoder_create(rate, channels, &err);
    if (err) fail("opus_decoder_create", err);
  }
  d.channels = channels;
  if (have_gain && (err = dec_ctl_set(&d, OPUS_SET_GAIN_REQUEST, gain))) fail("OPUS_SET_GAIN", err);
  if (noinv && (err = dec_ctl_set(&d, OPUS_SET_PHASE_INVERSION_DISABLED_REQUEST, 1))) fail("OPUS_SET_PHASE_INVERSION_DISABLED", err);

  long bit_size;
  unsigned char *bit = read_file(argv[args + 2], &bit_size);
  fout = fopen(argv[args + 3], "wb");
  if (!fout) { perror(argv[args + 3]); return 1; }
  sample_size = use_float ? sizeof(float) : sizeof(short);
  out = malloc((size_t)MAX_FRAME * channels * sample_size);
  int lost_count = 0, lost_prev = 1;
  long at = 0;
  while (at + 8 <= bit_size) {
    int len = (int)read32(bit + at);
    opus_uint32 enc_final_range = read32(bit + at + 4);
    at += 8;
    if (len > MAX_PACKET || len < 0) { fprintf(stderr, "Invalid payload length: %d\n", len); break; }
    if (at + len > bit_size) { fprintf(stderr, "Ran out of input\n"); break; }
    const unsigned char *data = bit + at;
    at += len;
    if (reset_every && count > 0 && count % reset_every == 0) dec_reset(&d);
    int lost = len == 0;
    if (odd && !lost) odd_call(&d, rate, data, len);
    int run_decoder = lost ? 0 : 1 + lost_count;
    if (lost) lost_count++;
    for (int fr = 0; fr < run_decoder; fr++) {
      opus_int32 output_samples = 0;
      int fec_here = fr == lost_count - 1 && (head_path || opus_packet_has_lbrr(data, len));
      if (fec_here) {
        dec_ctl_get(&d, OPUS_GET_LAST_PACKET_DURATION_REQUEST, &output_samples);
        output_samples = dec_decode(&d, data, len, out, output_samples, 1);
      } else if (fr < lost_count) {
        dec_ctl_get(&d, OPUS_GET_LAST_PACKET_DURATION_REQUEST, &output_samples);
        output_samples = dec_decode(&d, NULL, 0, out, output_samples, 0);
      } else {
        output_samples = dec_decode(&d, data, len, out, MAX_FRAME, 0);
      }
      emit(&d, output_samples);
    }
    opus_uint32 dec_final_range;
    dec_ctl_get(&d, OPUS_GET_FINAL_RANGE_REQUEST, (opus_int32 *)&dec_final_range);
    if (enc_final_range != 0 && !lost && !lost_prev && dec_final_range != enc_final_range) {
      fprintf(stderr, "Error: Range coder state mismatch between encoder and decoder in frame %ld: 0x%8lx vs 0x%8lx\n",
              count, (unsigned long)enc_final_range, (unsigned long)dec_final_range);
      return 1;
    }
    opus_int32 bandwidth = 0, pitch = 0, duration = 0;
    dec_ctl_get(&d, OPUS_GET_BANDWIDTH_REQUEST, &bandwidth);
    if (d.single) opus_decoder_ctl(d.single, OPUS_GET_PITCH(&pitch));
    dec_ctl_get(&d, OPUS_GET_LAST_PACKET_DURATION_REQUEST, &duration);
    fprintf(stderr, "S %d:%d:%d:%u\n", bandwidth, pitch, duration, dec_final_range);
    lost_prev = lost;
    if (!lost) lost_count = 0;
    count++;
  }
  fclose(fout);
  free(out);
  free(bit);
  return 0;
}
