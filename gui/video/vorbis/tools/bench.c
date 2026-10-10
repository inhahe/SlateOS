/* tests/bench.rs's measurement made of Tremor: every encoded stream in a
 * directory (*.ogg, not synthetic_*), each VORBIS_BENCH_PASSES times
 * (default 3) with a fresh decoder, the fastest kept, only decoding timed
 * (vorbis_synthesis, blockin, and ov_read's conversion to 16 bits), in
 * times real time.
 *
 *   gcc -O2 -I<libogg>/include -I<tremor> bench.c <tremor and libogg objects> -o bench
 *   ./bench tests/data
 *
 * tools/build_reference.sh's objects serve; see it for the build.
 */
#include <dirent.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <ogg/ogg.h>
#include "ivorbiscodec.h"

typedef struct {
    unsigned char *data;
    long len;
} packet;

static packet *read_packets(const char *path, long *count) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    ogg_sync_state oy;
    ogg_stream_state os;
    ogg_page og;
    ogg_packet op;
    int started = 0;
    long n = 0, cap = 0;
    packet *list = NULL;
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
                if (r < 0) continue;
                if (n == cap) {
                    cap = cap ? cap * 2 : 1024;
                    list = realloc(list, cap * sizeof *list);
                }
                list[n].data = malloc(op.bytes ? op.bytes : 1);
                memcpy(list[n].data, op.packet, op.bytes);
                list[n].len = op.bytes;
                n++;
            }
        }
        if (got == 0) break;
    }
    if (started) ogg_stream_clear(&os);
    ogg_sync_clear(&oy);
    fclose(f);
    *count = n;
    return list;
}

static int by_name(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec * 1e-9;
}

int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : "tests/data";
    const char *p = getenv("VORBIS_BENCH_PASSES");
    int passes = p ? atoi(p) : 3;
    DIR *d = opendir(dir);
    if (!d) {
        perror(dir);
        return 1;
    }
    char *names[256];
    int count = 0;
    struct dirent *e;
    while ((e = readdir(d)) && count < 256) {
        size_t l = strlen(e->d_name);
        if (l > 4 && !strcmp(e->d_name + l - 4, ".ogg") && strncmp(e->d_name, "synthetic_", 10))
            names[count++] = strdup(e->d_name);
    }
    closedir(d);
    qsort(names, count, sizeof *names, by_name);
    double seconds = 0, sound = 0;
    int16_t *out = malloc(8192 * 256 * sizeof *out);
    for (int i = 0; i < count; i++) {
        char path[4096];
        snprintf(path, sizeof path, "%s/%s", dir, names[i]);
        long n;
        packet *list = read_packets(path, &n);
        double best = 1e30;
        long samples = 0, rate = 0;
        for (int pass = 0; pass < passes; pass++) {
            vorbis_info vi;
            vorbis_comment vc;
            vorbis_dsp_state vd;
            vorbis_block vb;
            vorbis_info_init(&vi);
            vorbis_comment_init(&vc);
            for (int h = 0; h < 3; h++) {
                ogg_packet op = {0};
                op.packet = list[h].data;
                op.bytes = list[h].len;
                op.b_o_s = h == 0;
                op.packetno = h;
                op.granulepos = -1;
                if (vorbis_synthesis_headerin(&vi, &vc, &op)) return 1;
            }
            vorbis_synthesis_init(&vd, &vi);
            vorbis_block_init(&vd, &vb);
            rate = vi.rate;
            samples = 0;
            double start = now();
            for (long k = 3; k < n; k++) {
                ogg_packet op = {0};
                op.packet = list[k].data;
                op.bytes = list[k].len;
                op.packetno = k;
                op.granulepos = -1;
                if (vorbis_synthesis(&vb, &op)) continue;
                vorbis_synthesis_blockin(&vd, &vb);
                ogg_int32_t **pcm;
                int got = vorbis_synthesis_pcmout(&vd, &pcm);
                /* ov_read's conversion: interleaved, 16 bits, clipped. */
                for (int c = 0; c < vi.channels; c++) {
                    for (int s = 0; s < got; s++) {
                        int32_t x = pcm[c][s] >> 9;
                        out[s * vi.channels + c] = x > 32767 ? 32767 : x < -32768 ? -32768 : x;
                    }
                }
                vorbis_synthesis_read(&vd, got);
                samples += got;
            }
            double t = now() - start;
            if (t < best) best = t;
            vorbis_block_clear(&vb);
            vorbis_dsp_clear(&vd);
            vorbis_comment_clear(&vc);
            vorbis_info_clear(&vi);
        }
        double length = (double)samples / rate;
        printf("%-24s %6.2f s of sound, %7.0f times real time\n", names[i], length, length / best);
        seconds += best;
        sound += length;
    }
    printf("all %.2f s of sound: %.0f times real time\n", sound, sound / seconds);
    return 0;
}
