/* libopus's functions on inputs no stream makes often enough to test --
 * damaged and made-up packets, resonant signals, LSFs crowded together --
 * digested for the crate's unit tests to hold their own results to: a line
 * `NAME COUNT FNV1A64` for each set (each result added as 4 little-endian
 * bytes). The tests: `packet::tests::packet_functions_agree_with_libopus`,
 * `celt::reference::lpc_agrees_with_libopus` and `pitch_agrees_with_libopus`,
 * `silk::reference::nlsf_agrees_with_libopus` and
 * `silk_math_agrees_with_libopus`.
 *
 * The inputs come from the xorshift generator below, seeded afresh for each
 * set and one draw a statement, so that C and Rust draw in the same order;
 * and signals from an integer oscillator rather than libm, so that both
 * make the same ones.
 *
 *   L=<libopus 1.5.2, configured --enable-fixed-point and built>
 *   cc -O2 -I$L -I$L/include -I$L/celt -I$L/silk -I$L/src functions.c \
 *      $L/.libs/libopus.a -lm -o functions
 */
#include "config.h"
#include <stdio.h>
#include <string.h>
#include "opus.h"
#include "opus_custom.h"
#include "opus_multistream.h"
#include "opus_private.h"
#include "celt/celt_lpc.h"
#include "celt/modes.h"
#include "celt/pitch.h"
#include "silk/SigProc_FIX.h"
#include "silk/tables.h"

/* Each set seeds the generator afresh, so that each test can start where
 * its set does. */
static unsigned long long s;
static unsigned rnd(void) {
  s ^= s << 13;
  s ^= s >> 7;
  s ^= s << 17;
  return (unsigned)(s >> 16);
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

/* A tone from an integer oscillator: y[n] = 2c y[n-1] / 2^14 - y[n-2],
 * clipped to 16 bits, c in Q14 -- a sinusoid that integer rounding makes
 * wander a little, the same in any language. */
static void tone(int *out, int n, int c, int amp) {
  int y2 = 0, y1 = amp;
  for (int i = 0; i < n; i++) {
    int y = ((2 * c * y1) >> 14) - y2;
    if (y > 32767) y = 32767;
    if (y < -32768) y = -32768;
    out[i] = y;
    y2 = y1;
    y1 = y;
  }
}

/* A signal of one of eight kinds: noise, a tone, two tones and noise,
 * silence, resonant noise, impulses, a square wave, or the Nyquist tone. */
static void signal(int *out, int n) {
  unsigned kind = rnd() % 8;
  if (kind == 0) {
    unsigned shift = rnd() % 16;
    for (int i = 0; i < n; i++) out[i] = (short)(rnd() & 0xffff) >> shift;
  } else if (kind == 1) {
    int c = (int)(rnd() % 16384);
    int amp = (int)(rnd() % 32768);
    tone(out, n, c, amp);
  } else if (kind == 2) {
    static int other[4096];
    int c1 = (int)(rnd() % 16384);
    int c2 = (int)(rnd() % 16384);
    tone(out, n, c1, 12000);
    tone(other, n, c2, 8000);
    unsigned shift = rnd() % 16;
    for (int i = 0; i < n; i++) {
      int noise = (short)(rnd() & 0xffff) >> (shift + 4);
      int v = out[i] / 2 + other[i] / 2 + noise;
      out[i] = v > 32767 ? 32767 : v < -32768 ? -32768 : v;
    }
  } else if (kind == 3) {
    for (int i = 0; i < n; i++) out[i] = 0;
  } else if (kind == 4) {
    /* Quiet noise through three resonators at one frequency, poles just
     * inside the unit circle: a spectrum so peaky that an LPC fit's
     * coefficients pass what 16 bits hold. */
    long long cosw = rnd() % 16384;
    long long r = 16383 - (long long)(rnd() % 64);
    long long a1 = (2 * r * cosw) >> 14, a2 = (r * r) >> 14;
    long long y[3][2] = {{0}};
    for (int i = 0; i < n; i++) {
      long long v = (short)(rnd() & 0xffff) >> 8;
      for (int st = 0; st < 3; st++) {
        long long w = v + ((a1 * y[st][0]) >> 14) - ((a2 * y[st][1]) >> 14);
        if (w > (1 << 24)) w = 1 << 24;
        if (w < -(1 << 24)) w = -(1 << 24);
        y[st][1] = y[st][0];
        y[st][0] = w;
        v = w;
      }
      long long o = v >> 8;
      out[i] = o > 32767 ? 32767 : o < -32768 ? -32768 : (int)o;
    }
  } else if (kind == 5) {
    int period = 20 + (int)(rnd() % 300);
    int amp = (int)(rnd() % 32768);
    for (int i = 0; i < n; i++) out[i] = i % period == 0 ? amp : 0;
  } else if (kind == 6) {
    int half = 1 + (int)(rnd() % 100);
    int amp = (int)(rnd() % 32768);
    for (int i = 0; i < n; i++) out[i] = (i / half) & 1 ? amp : -amp;
  } else {
    int amp = (int)(rnd() % 32768);
    for (int i = 0; i < n; i++) out[i] = i & 1 ? amp : -amp;
  }
}

static void packets(void) {
  static unsigned char data[1700];
  s = 0x2545F4914F6CDD1Dull;
  start();
  for (int i = 0; i < 200000; i++) {
    unsigned r = rnd() % 4;
    int len;
    if (r == 0) len = (int)(rnd() % 4);
    else if (r == 1) len = (int)(rnd() % 16);
    else if (r == 2) len = (int)(rnd() % 300);
    else len = (int)(rnd() % 1700);
    for (int j = 0; j < len; j++) {
      unsigned b = rnd();
      data[j] = (b & 3) == 0 ? 0xff : (unsigned char)(b >> 2);
    }
    /* opus_packet_has_lbrr reads the first frame's first byte even when the
     * frame is empty: the next frame's, or, with nothing after it, the byte
     * past the packet's end. Zeroed, that byte gives what the port gives
     * where there is no byte to read (no LBRR) -- not stale data from the
     * packet before. */
    memset(data + len, 0, sizeof data - len);
    for (int sd = 0; sd < 2; sd++) {
      unsigned char toc;
      opus_int16 size[48];
      int payload_offset;
      opus_int32 packet_offset;
      int ret = opus_packet_parse_impl(data, len, sd, &toc, NULL, size, &payload_offset, &packet_offset, NULL, NULL);
      add(ret);
      if (ret > 0) {
        add(toc);
        for (int k = 0; k < ret; k++) add(size[k]);
        add(payload_offset);
        add(packet_offset);
      }
    }
    add(opus_packet_get_nb_frames(data, len));
    add(opus_packet_get_nb_samples(data, len, 48000));
    add(opus_packet_get_nb_samples(data, len, 8000));
    /* opus_packet_has_lbrr reads the TOC before it checks the length. */
    if (len > 0) add(opus_packet_has_lbrr(data, len));
  }
  end("packets");
}

static void celt_lpc_check(void) {
  const OpusCustomMode *mode = opus_custom_mode_create(48000, 960, NULL);
  static int x[1024];
  static opus_val16 x16[1024];
  opus_val32 ac[25];
  opus_val16 lpc[24];
  s = 0x9E3779B97F4A7C15ull;
  start();
  for (int i = 0; i < 20000; i++) {
    if (i % 2 == 0) {
      signal(x, 1024);
      for (int k = 0; k < 1024; k++) x16[k] = (opus_val16)x[k];
      int shift = _celt_autocorr(x16, ac, mode->window, mode->overlap, 24, 1024, 0);
      add(shift);
      for (int k = 0; k < 25; k++) add(ac[k]);
    } else {
      /* An autocorrelation that need not be one: what a damaged stream's
       * concealment could be handed. */
      unsigned bits0 = rnd();
      unsigned shift0 = rnd() % 31;
      ac[0] = (opus_val32)(bits0 & 0x7fffffff) >> shift0;
      for (int k = 1; k < 25; k++) {
        unsigned bits = rnd();
        unsigned shift = rnd() % 31;
        ac[k] = (opus_val32)(bits & 0x7fffffff) >> shift;
        if (rnd() & 1) ac[k] = -ac[k];
      }
    }
    /* The fallback leaves all but the first coefficient as they were. */
    for (int k = 0; k < 24; k++) lpc[k] = (opus_val16)(k * 1000 - 12000);
    _celt_lpc(lpc, ac, 24);
    for (int k = 0; k < 24; k++) add(lpc[k]);
  }
  end("celt_lpc");
}

static void celt_pitch_check(void) {
  static int a[2048], b[2048];
  static celt_sig ch0[2048], ch1[2048];
  static opus_val16 lp[1024];
  s = 0xD1B54A32D192ED03ull;
  start();
  for (int i = 0; i < 1500; i++) {
    int channels = 1 + (int)(rnd() % 2);
    unsigned shift = rnd() % 13;
    signal(a, 2048);
    signal(b, 2048);
    for (int k = 0; k < 2048; k++) {
      ch0[k] = (celt_sig)a[k] << shift;
      ch1[k] = (celt_sig)b[k] << shift;
    }
    celt_sig *chans[2] = {ch0, ch1};
    pitch_downsample(chans, lp, 2048, channels, 0);
    for (int k = 0; k < 1024; k++) add(lp[k]);
    int pitch;
    pitch_search(lp + 360, lp, 2048 - 720, 720 - 100, &pitch, 0);
    add(pitch);
  }
  end("celt_pitch");
}

static void silk_nlsf_check(void) {
  opus_int16 nlsf[16], stable[16], a[16];
  s = 0x8CB92BA72F3D8DD7ull;
  start();
  for (int i = 0; i < 100000; i++) {
    int d = (rnd() & 1) ? 16 : 10;
    unsigned kind = rnd() % 4;
    if (kind == 0 || kind == 3) {
      for (int k = 0; k < d; k++) nlsf[k] = (opus_int16)(rnd() % 32768);
    } else if (kind == 1) {
      /* Pairs crowded together: peaky spectra. */
      for (int k = 0; k < d; k += 2) {
        int base = (int)(rnd() % 32700);
        int gap = (int)(rnd() % 64);
        nlsf[k] = (opus_int16)base;
        nlsf[k + 1] = (opus_int16)(base + gap);
      }
    } else {
      /* Crowded at the ends. */
      for (int k = 0; k < d; k++) {
        unsigned v = rnd() % 600;
        nlsf[k] = (opus_int16)((rnd() & 1) ? v : 32767 - v);
      }
    }
    if (kind != 3) {
      /* Sorted (insertion sort), as a codebook's NLSFs are. */
      for (int k = 1; k < d; k++)
        for (int j = k; j > 0 && nlsf[j - 1] > nlsf[j]; j--) {
          opus_int16 t = nlsf[j];
          nlsf[j] = nlsf[j - 1];
          nlsf[j - 1] = t;
        }
      memset(a, 0, sizeof a);
      silk_NLSF2A(a, nlsf, d, 0);
      for (int k = 0; k < d; k++) add(a[k]);
    }
    memcpy(stable, nlsf, sizeof stable);
    silk_NLSF_stabilize(stable, d == 16 ? silk_NLSF_CB_WB.deltaMin_Q15 : silk_NLSF_CB_NB_MB.deltaMin_Q15, d);
    for (int k = 0; k < d; k++) add(stable[k]);
    memset(a, 0, sizeof a);
    silk_NLSF2A(a, stable, d, 0);
    for (int k = 0; k < d; k++) add(a[k]);
    /* And the stability test on filters of any shape. */
    unsigned shift = rnd() % 16;
    for (int k = 0; k < d; k++) a[k] = (opus_int16)((short)(rnd() & 0xffff) >> shift);
    add(silk_LPC_inverse_pred_gain_c(a, d));
  }
  end("silk_nlsf");
}

static void silk_math_check(void) {
  s = 0xF1357AEA2E62A9C5ull;
  start();
  for (int x = -10; x < 4000; x++) add(silk_log2lin(x));
  for (int i = 0; i < 100000; i++) {
    unsigned bits = rnd();
    unsigned shift = rnd() % 32;
    int x = (int)bits >> shift;
    add(silk_SQRT_APPROX(x));
  }
  for (int i = 0; i < 100000; i++) {
    unsigned abits = rnd();
    unsigned ashift = rnd() % 32;
    unsigned bbits = rnd();
    unsigned bshift = rnd() % 32;
    int q = (int)(rnd() % 31);
    int a32 = (int)(abits | 1) >> ashift;
    int b32 = (int)(bbits | 1) >> bshift;
    if (a32 == (int)0x80000000 || b32 == (int)0x80000000 || b32 == 0) continue;
    add(silk_DIV32_varQ(a32, b32, q));
    add(silk_INVERSE32_varQ(b32, 1 + q));
  }
  static opus_int16 v[401];
  for (int i = 0; i < 20000; i++) {
    int len = 1 + (int)(rnd() % 400);
    unsigned shift = rnd() % 16;
    for (int k = 0; k < len; k++) v[k] = (opus_int16)((short)(rnd() & 0xffff) >> shift);
    opus_int32 energy;
    int sh;
    silk_sum_sqr_shift(&energy, &sh, v, len);
    add(energy);
    add(sh);
  }
  end("silk_math");
}

/* Made-up packets through a multistream decoder (three channels: a coupled
 * stream and a mono one): its validation of the streams' framing, and what
 * it decodes. */
static void multistream_check(void) {
  static unsigned char data[1700];
  static short pcm[5760 * 3];
  const unsigned char mapping[3] = {0, 1, 2};
  int err;
  OpusMSDecoder *ms = opus_multistream_decoder_create(48000, 3, 2, 1, mapping, &err);
  s = 0xA0761D6478BD642Full;
  start();
  for (int i = 0; i < 20000; i++) {
    unsigned r = rnd() % 4;
    int len;
    if (r == 0) len = (int)(rnd() % 4);
    else if (r == 1) len = (int)(rnd() % 16);
    else if (r == 2) len = (int)(rnd() % 300);
    else len = (int)(rnd() % 1700);
    for (int j = 0; j < len; j++) {
      unsigned b = rnd();
      data[j] = (b & 3) == 0 ? 0xff : (unsigned char)(b >> 2);
    }
    int ret = opus_multistream_decode(ms, data, len, pcm, 5760, 0);
    add(ret);
    for (int k = 0; k < ret * 3; k += 7) add(pcm[k]);
  }
  opus_multistream_decoder_destroy(ms);
  end("multistream");
}

int main(void) {
  packets();
  celt_lpc_check();
  celt_pitch_check();
  silk_nlsf_check();
  silk_math_check();
  multistream_check();
  return 0;
}
