/*
 * gui/video/flac's reference: libFLAC 1.5.0's stream decoder reading a
 * native FLAC file, each thing it reports printed a line --
 *
 *   meta TYPE LENGTH          a metadata block, in order
 *   info MIN MAX MINF MAXF RATE CHANNELS BITS TOTAL   STREAMINFO's fields
 *   frame SAMPLE BLOCK CHANNELS BITS RATE HASH         a frame written: its
 *                             first sample, size, and the FNV-1a (64-bit) of
 *                             its samples, each a little-endian int32,
 *                             channel after channel
 *   error STATUS              the error callback
 *   seek SAMPLE               a seek (the frames from it follow)
 *   seek-failed               libFLAC refused it (a sample past the end)
 *   end STATE MD5             the decoder's state at the end, and its MD5
 *                             check: ok, bad, or none
 *
 * Build in WSL, libFLAC built static from its 1.5.0 release
 * (./configure --disable-ogg --disable-shared && make):
 *
 *   cc -O2 -o reference reference.c -I $FLAC/include \
 *      $FLAC/src/libFLAC/.libs/libFLAC.a -lm
 *
 * Usage: reference FILE [SAMPLE...] -- with samples, after reading the whole
 * file it seeks to each in turn and prints the frames from there to the end.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "FLAC/stream_decoder.h"

static const char *status_name(FLAC__StreamDecoderErrorStatus s)
{
    switch (s) {
    case FLAC__STREAM_DECODER_ERROR_STATUS_LOST_SYNC: return "lost-sync";
    case FLAC__STREAM_DECODER_ERROR_STATUS_BAD_HEADER: return "bad-header";
    case FLAC__STREAM_DECODER_ERROR_STATUS_FRAME_CRC_MISMATCH: return "crc-mismatch";
    case FLAC__STREAM_DECODER_ERROR_STATUS_UNPARSEABLE_STREAM: return "unparseable";
    case FLAC__STREAM_DECODER_ERROR_STATUS_BAD_METADATA: return "bad-metadata";
    case FLAC__STREAM_DECODER_ERROR_STATUS_OUT_OF_BOUNDS: return "out-of-bounds";
    case FLAC__STREAM_DECODER_ERROR_STATUS_MISSING_FRAME: return "missing-frame";
    default: return "other";
    }
}

static FLAC__StreamDecoderWriteStatus write_cb(const FLAC__StreamDecoder *d, const FLAC__Frame *frame,
                                               const FLAC__int32 *const buffer[], void *client)
{
    (void)d;
    (void)client;
    uint64_t h = 0xcbf29ce484222325ull;
    for (unsigned c = 0; c < frame->header.channels; c++) {
        for (unsigned i = 0; i < frame->header.blocksize; i++) {
            uint32_t v = (uint32_t)buffer[c][i];
            for (int b = 0; b < 4; b++) {
                h ^= (v >> (8 * b)) & 0xff;
                h *= 0x100000001b3ull;
            }
        }
    }
    printf("frame %llu %u %u %u %u %016llx\n", (unsigned long long)frame->header.number.sample_number,
           frame->header.blocksize, frame->header.channels, frame->header.bits_per_sample,
           frame->header.sample_rate, (unsigned long long)h);
    return FLAC__STREAM_DECODER_WRITE_STATUS_CONTINUE;
}

/* Whether STREAMINFO gave an MD5 to check against. */
static int has_md5;

static void metadata_cb(const FLAC__StreamDecoder *d, const FLAC__StreamMetadata *m, void *client)
{
    (void)d;
    (void)client;
    printf("meta %u %u\n", (unsigned)m->type, m->length);
    if (m->type == FLAC__METADATA_TYPE_STREAMINFO) {
        const FLAC__StreamMetadata_StreamInfo *s = &m->data.stream_info;
        has_md5 = 0;
        for (int i = 0; i < 16; i++)
            has_md5 |= s->md5sum[i] != 0;
        printf("info %u %u %u %u %u %u %u %llu\n", s->min_blocksize, s->max_blocksize, s->min_framesize,
               s->max_framesize, s->sample_rate, s->channels, s->bits_per_sample,
               (unsigned long long)s->total_samples);
    }
}

static void error_cb(const FLAC__StreamDecoder *d, FLAC__StreamDecoderErrorStatus s, void *client)
{
    (void)d;
    (void)client;
    printf("error %s\n", status_name(s));
}

/* Process to the end, carrying on past a call that fails where the state
 * still says there is more to read -- as a player that wants every frame
 * it can get does -- and stopping where the position no longer moves. */
static void run(FLAC__StreamDecoder *d)
{
    FLAC__uint64 last = (FLAC__uint64)-1;
    int stuck = 0;
    for (;;) {
        FLAC__bool ok = FLAC__stream_decoder_process_single(d);
        FLAC__StreamDecoderState st = FLAC__stream_decoder_get_state(d);
        if (st == FLAC__STREAM_DECODER_END_OF_STREAM || st > FLAC__STREAM_DECODER_READ_FRAME)
            break;
        if (!ok) {
            FLAC__uint64 pos = 0;
            if (!FLAC__stream_decoder_get_decode_position(d, &pos) || pos == last) {
                if (++stuck > 2)
                    break;
            } else
                stuck = 0;
            last = pos;
        }
    }
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: reference FILE [SAMPLE...]\n");
        return 2;
    }
    FLAC__StreamDecoder *d = FLAC__stream_decoder_new();
    FLAC__stream_decoder_set_md5_checking(d, true);
    FLAC__stream_decoder_set_metadata_respond_all(d);
    if (FLAC__stream_decoder_init_file(d, argv[1], write_cb, metadata_cb, error_cb, NULL) !=
        FLAC__STREAM_DECODER_INIT_STATUS_OK) {
        printf("init failed\n");
        return 1;
    }
    run(d);
    FLAC__StreamDecoderState st = FLAC__stream_decoder_get_state(d);
    for (int k = 2; k < argc; k++) {
        unsigned long long sample = strtoull(argv[k], NULL, 10);
        /* The seek itself writes the frame it lands in, trimmed: the line
         * comes first. */
        printf("seek %llu\n", sample);
        if (!FLAC__stream_decoder_seek_absolute(d, sample)) {
            printf("seek-failed\n");
            FLAC__stream_decoder_flush(d);
        } else
            run(d);
    }
    /* finish() reports the MD5 check; the state before it is the run's. */
    const char *md5 = "none";
    FLAC__bool md5_ok = FLAC__stream_decoder_finish(d);
    if (argc == 2 && has_md5)
        md5 = md5_ok ? "ok" : "bad";
    printf("end %s %s\n", FLAC__StreamDecoderStateString[st], md5);
    FLAC__stream_decoder_delete(d);
    return 0;
}
