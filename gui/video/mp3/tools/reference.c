/*
 * The reference this crate is held to: minimp3 built without SIMD, decoding
 * a file frame by frame as `mp3::Decoder` is driven by the tests -- each call
 * given the rest of the file -- and printing one line a call:
 *
 *   frame <offset> <frame_bytes> <samples> <channels> <hz> <layer> <kbps> <hash>
 *
 * where <offset> is the call's start in the file, <frame_bytes> and the
 * rest are minimp3's answer, and <hash> is FNV-1a (64-bit) over the samples'
 * bytes, little-endian, interleaved (0xcbf29ce484222325 when there are
 * none). It stops at the first call that takes no bytes, printing
 *
 *   end <offset>
 *
 * With a second argument, also writes the samples there, raw.
 *
 * Build (WSL): minimp3.h with the crate's two deliberate differences
 * applied (tools/patch_minimp3.py), then
 *   python3 patch_minimp3.py <minimp3>/minimp3.h patched/minimp3.h
 *   cc -O2 -ffp-contract=off -Wall -o reference tools/reference.c -I patched
 *
 * -ffp-contract=off: no fused multiply-adds, which would round differently
 * from the Rust (x86-64's baseline has none to fuse into anyway).
 */
#define MINIMP3_IMPLEMENTATION
#define MINIMP3_NO_SIMD
#include "minimp3.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: reference <file> [<pcm out>]\n");
        return 2;
    }
    FILE *f = fopen(argv[1], "rb");
    if (!f) {
        perror(argv[1]);
        return 1;
    }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *buf = malloc(size > 0 ? size : 1);
    if (!buf || fread(buf, 1, size, f) != (size_t)size) {
        fprintf(stderr, "cannot read %s\n", argv[1]);
        return 1;
    }
    fclose(f);
    FILE *out = argc > 2 ? fopen(argv[2], "wb") : NULL;

    static mp3dec_t dec;
    mp3dec_init(&dec);
    int16_t pcm[MINIMP3_MAX_SAMPLES_PER_FRAME];
    long pos = 0;
    for (;;) {
        mp3dec_frame_info_t info = { 0 };
        int samples = mp3dec_decode_frame(&dec, buf + pos, (int)(size - pos), pcm, &info);
        if (info.frame_bytes == 0) {
            printf("end %ld\n", pos);
            break;
        }
        uint64_t hash = 0xcbf29ce484222325ull;
        int n = samples * (samples ? info.channels : 0);
        for (int i = 0; i < n; i++) {
            uint16_t v = (uint16_t)pcm[i];
            hash = (hash ^ (v & 0xff)) * 0x100000001b3ull;
            hash = (hash ^ (v >> 8)) * 0x100000001b3ull;
        }
        printf("frame %ld %d %d %d %d %d %d %016" PRIx64 "\n", pos, info.frame_bytes, samples,
               info.channels, info.hz, info.layer, info.bitrate_kbps, hash);
        if (out && n)
            fwrite(pcm, 2, n, out);
        pos += info.frame_bytes;
    }
    if (out)
        fclose(out);
    free(buf);
    return 0;
}
