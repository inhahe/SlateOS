/* The reference the Rust port is held to: Tremor's low-level API over an
 * Ogg Vorbis file's packets, exactly as the tests drive the port.
 *
 *   reference decode <file.ogg> [-v]
 *   reference damage <file.ogg> <seed> [-v]
 *   reference headers <file.ogg>
 *   reference setupdamage <file.ogg> <seed>
 *   reference pcm <file.ogg> <out.pcm>
 *
 * decode: every packet in order (granule positions withheld, so Tremor
 * trims nothing), printing a summary line -- the packets, the errors, the
 * samples a channel, and FNV-1a 64 digests of everything returned at
 * Tremor's precision and as ov_read's 16-bit samples. With -v, a line a
 * packet: its number, vorbis_synthesis's result, the samples, and an
 * FNV-1a 32 digest of them.
 *
 * damage: the same, the audio packets first damaged as damage() below
 * says, from a seed; tests/damage.rs repeats the damage draw for draw.
 *
 * headers: what the three headers say, or which one Tremor refused.
 *
 * setupdamage: the setup header damaged (bits flipped, or cut short) as
 * damage_setup() says, then the first 24 audio packets decoded as by
 * decode -- or which header Tremor refused, or that it would not start.
 *
 * pcm: every packet's samples as ov_read gives them -- 16-bit, clipped,
 * interleaved, little-endian -- into out.pcm, nothing trimmed (the first
 * packet gives none); on stdout, each audio packet's samples a channel, a
 * line each, in order. What gui/video/codec's sound fixtures are held to.
 *
 * Built by tools/build_reference.sh against Tremor and libogg.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ogg/ogg.h>
#include "ivorbiscodec.h"

typedef struct {
    unsigned char *data;
    long len;
} packet;

static packet *packets;
static long npackets;

/* Every packet of the file's first logical stream, in order. */
static int read_packets(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        perror(path);
        return -1;
    }
    ogg_sync_state oy;
    ogg_stream_state os;
    ogg_page og;
    ogg_packet op;
    int started = 0;
    long cap = 0;
    ogg_sync_init(&oy);
    for (;;) {
        char *buf = ogg_sync_buffer(&oy, 65536);
        long got = (long)fread(buf, 1, 65536, f);
        ogg_sync_wrote(&oy, got);
        while (ogg_sync_pageout(&oy, &og) == 1) {
            if (!started) {
                ogg_stream_init(&os, ogg_page_serialno(&og));
                started = 1;
            }
            if (ogg_page_serialno(&og) != os.serialno) continue;
            ogg_stream_pagein(&os, &og);
            int r;
            while ((r = ogg_stream_packetout(&os, &op)) != 0) {
                if (r < 0) continue; /* a hole: the fixtures have none */
                if (npackets == cap) {
                    cap = cap ? cap * 2 : 1024;
                    packets = realloc(packets, cap * sizeof *packets);
                }
                packets[npackets].data = malloc(op.bytes ? op.bytes : 1);
                memcpy(packets[npackets].data, op.packet, op.bytes);
                packets[npackets].len = op.bytes;
                npackets++;
            }
        }
        if (got == 0) break;
    }
    if (started) ogg_stream_clear(&os);
    ogg_sync_clear(&oy);
    fclose(f);
    return 0;
}

static uint64_t fnv64(uint64_t h, const void *p, size_t n) {
    const unsigned char *b = p;
    for (size_t i = 0; i < n; i++) {
        h ^= b[i];
        h *= 0x100000001b3ULL;
    }
    return h;
}

static uint32_t fnv32(uint32_t h, const void *p, size_t n) {
    const unsigned char *b = p;
    for (size_t i = 0; i < n; i++) {
        h ^= b[i];
        h *= 0x01000193u;
    }
    return h;
}

static void put32(unsigned char *b, int32_t v) {
    uint32_t u = (uint32_t)v;
    b[0] = u;
    b[1] = u >> 8;
    b[2] = u >> 16;
    b[3] = u >> 24;
}

/* The damage generator: an LCG, each draw its top 15 bits. */
static uint32_t lcg;
static uint32_t draw(void) {
    lcg = lcg * 1103515245u + 12345u;
    return lcg >> 16;
}

/* Damages packet p (a copy) in place, by the next draws; returns 0 to drop
 * it, 2 to restart the decoder before it, 1 otherwise. */
static int damage(packet *p, long cap) {
    uint32_t action = draw() % 16;
    long i, n;
    switch (action) {
    case 0: /* lost */
        return 0;
    case 1: /* cut short */
        p->len = draw() % (uint32_t)(p->len + 1);
        return 1;
    case 2: /* one bit flipped */
        if (p->len) {
            i = draw() % (uint32_t)p->len;
            p->data[i] ^= 1 << (draw() % 8);
        }
        return 1;
    case 3: /* up to eight bits flipped */
        n = draw() % 8 + 1;
        while (n-- && p->len) {
            i = draw() % (uint32_t)p->len;
            p->data[i] ^= 1 << (draw() % 8);
        }
        return 1;
    case 4: /* noise */
        for (i = 0; i < p->len; i++) p->data[i] = draw() & 0xff;
        return 1;
    case 5: /* zeros from a point on */
        i = p->len ? draw() % (uint32_t)p->len : 0;
        for (; i < p->len; i++) p->data[i] = 0;
        return 1;
    case 6: /* noise appended */
        n = draw() % 16 + 1;
        while (n-- && p->len < cap) p->data[p->len++] = draw() & 0xff;
        return 1;
    case 7: /* empty */
        p->len = 0;
        return 1;
    case 8: /* a seek: the decoder restarted */
        return 2;
    default:
        return 1;
    }
}

/* Damages the setup header in place, by the next draws. */
static void damage_setup(packet *p) {
    uint32_t action = draw() % 10;
    long i, n;
    if (action < 5) {
        n = 1;
    } else if (action < 8) {
        n = draw() % 3 + 2;
    } else {
        /* cut short */
        p->len = p->len ? draw() % (uint32_t)p->len : 0;
        return;
    }
    while (n-- && p->len) {
        i = draw() % (uint32_t)p->len;
        p->data[i] ^= 1 << (draw() % 8);
    }
}

static int headers(vorbis_info *vi, vorbis_comment *vc) {
    if (npackets < 3) return -1;
    vorbis_info_init(vi);
    vorbis_comment_init(vc);
    for (int i = 0; i < 3; i++) {
        ogg_packet op = {0};
        op.packet = packets[i].data;
        op.bytes = packets[i].len;
        op.b_o_s = i == 0;
        op.packetno = i;
        op.granulepos = -1;
        int r = vorbis_synthesis_headerin(vi, vc, &op);
        if (r) return 100 * (i + 1) - r;
    }
    return 0;
}

static int run(int damaged, uint32_t seed, int verbose, long limit) {
    vorbis_info vi;
    vorbis_comment vc;
    vorbis_dsp_state vd;
    vorbis_block vb;
    int r = headers(&vi, &vc);
    if (r) {
        printf("headers refused %d\n", r);
        return 1;
    }
    if (vorbis_synthesis_init(&vd, &vi)) {
        printf("init refused\n");
        return 1;
    }
    vorbis_block_init(&vd, &vb);
    lcg = seed;
    uint64_t h32 = 0xcbf29ce484222325ULL, h16 = 0xcbf29ce484222325ULL;
    long errors = 0, samples = 0, decoded = 0, seq = 3;
    unsigned char *copy = malloc(1 << 20);
    long last = limit && limit + 3 < npackets ? limit + 3 : npackets;
    for (long k = 3; k < last; k++) {
        packet p = packets[k];
        long cap = 1 << 20;
        if (damaged) {
            if (p.len > cap - 64) p.len = cap - 64;
            memcpy(copy, p.data, p.len);
            p.data = copy;
            int what = damage(&p, cap);
            if (what == 0) continue;
            if (what == 2) vorbis_synthesis_restart(&vd);
        }
        ogg_packet op = {0};
        op.packet = p.data;
        op.bytes = p.len;
        op.packetno = seq++;
        op.granulepos = -1;
        r = vorbis_synthesis(&vb, &op);
        int n = 0;
        uint32_t ph = 0x811c9dc5u;
        if (r == 0) {
            vorbis_synthesis_blockin(&vd, &vb);
            ogg_int32_t **pcm;
            n = vorbis_synthesis_pcmout(&vd, &pcm);
            for (int s = 0; s < n; s++) {
                for (int c = 0; c < vi.channels; c++) {
                    unsigned char b[4];
                    int32_t v = pcm[c][s];
                    put32(b, v);
                    h32 = fnv64(h32, b, 4);
                    ph = fnv32(ph, b, 4);
                    int32_t x = v >> 9;
                    int16_t w = x > 32767 ? 32767 : x < -32768 ? -32768 : x;
                    h16 = fnv64(h16, &w, 2);
                }
            }
            vorbis_synthesis_read(&vd, n);
            decoded++;
        } else {
            errors++;
        }
        unsigned char rb[8];
        put32(rb, r);
        put32(rb + 4, n);
        h32 = fnv64(h32, rb, 8);
        h16 = fnv64(h16, rb, 8);
        samples += n;
        if (verbose) printf("%ld %d %d %08x\n", k, r, n, ph);
    }
    printf("packets %ld decoded %ld errors %ld samples %ld i32 %016llx i16 %016llx\n", last - 3, decoded,
           errors, samples, (unsigned long long)h32, (unsigned long long)h16);
    vorbis_block_clear(&vb);
    vorbis_dsp_clear(&vd);
    vorbis_comment_clear(&vc);
    vorbis_info_clear(&vi);
    free(copy);
    return 0;
}

static int write_pcm(const char *path) {
    vorbis_info vi;
    vorbis_comment vc;
    vorbis_dsp_state vd;
    vorbis_block vb;
    int r = headers(&vi, &vc);
    if (r) {
        printf("headers refused %d\n", r);
        return 1;
    }
    if (vorbis_synthesis_init(&vd, &vi)) {
        printf("init refused\n");
        return 1;
    }
    vorbis_block_init(&vd, &vb);
    FILE *out = fopen(path, "wb");
    if (!out) {
        perror(path);
        return 1;
    }
    for (long k = 3; k < npackets; k++) {
        ogg_packet op = {0};
        op.packet = packets[k].data;
        op.bytes = packets[k].len;
        op.packetno = k;
        op.granulepos = -1;
        if (vorbis_synthesis(&vb, &op)) {
            printf("0\n");
            continue;
        }
        vorbis_synthesis_blockin(&vd, &vb);
        ogg_int32_t **pcm;
        int n = vorbis_synthesis_pcmout(&vd, &pcm);
        for (int s = 0; s < n; s++) {
            for (int c = 0; c < vi.channels; c++) {
                int32_t x = pcm[c][s] >> 9;
                int16_t w = x > 32767 ? 32767 : x < -32768 ? -32768 : x;
                unsigned char b[2] = {(unsigned char)w, (unsigned char)((uint16_t)w >> 8)};
                fwrite(b, 1, 2, out);
            }
        }
        vorbis_synthesis_read(&vd, n);
        printf("%d\n", n);
    }
    fclose(out);
    vorbis_block_clear(&vb);
    vorbis_dsp_clear(&vd);
    vorbis_comment_clear(&vc);
    vorbis_info_clear(&vi);
    return 0;
}

static int show_headers(void) {
    vorbis_info vi;
    vorbis_comment vc;
    int r = headers(&vi, &vc);
    if (r) {
        printf("headers refused %d\n", r);
        return 0;
    }
    printf("channels %d rate %ld bitrates %ld %ld %ld blocksizes %d %d\n", vi.channels, vi.rate, vi.bitrate_upper,
           vi.bitrate_nominal, vi.bitrate_lower, vorbis_info_blocksize(&vi, 0), vorbis_info_blocksize(&vi, 1));
    uint64_t h = fnv64(0xcbf29ce484222325ULL, vc.vendor, strlen(vc.vendor));
    for (int i = 0; i < vc.comments; i++) h = fnv64(h, vc.user_comments[i], vc.comment_lengths[i]);
    printf("vendor %s comments %d digest %016llx\n", vc.vendor, vc.comments, (unsigned long long)h);
    vorbis_dsp_state vd;
    printf("init %s\n", vorbis_synthesis_init(&vd, &vi) ? "refused" : "ok");
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: reference decode|damage|headers|setupdamage|pcm <file.ogg> [seed] [-v]\n");
        return 2;
    }
    if (read_packets(argv[2])) return 2;
    int verbose = argc > 3 && !strcmp(argv[argc - 1], "-v");
    if (!strcmp(argv[1], "decode")) return run(0, 0, verbose, 0);
    if (!strcmp(argv[1], "damage") && argc > 3) return run(1, (uint32_t)strtoul(argv[3], 0, 0), verbose, 0);
    if (!strcmp(argv[1], "setupdamage") && argc > 3 && npackets >= 3) {
        lcg = (uint32_t)strtoul(argv[3], 0, 0);
        damage_setup(&packets[2]);
        return run(0, 0, verbose, 24);
    }
    if (!strcmp(argv[1], "headers")) return show_headers();
    if (!strcmp(argv[1], "pcm") && argc > 3) return write_pcm(argv[3]);
    fprintf(stderr, "unknown mode %s\n", argv[1]);
    return 2;
}
