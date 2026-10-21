/*
 * What FFmpeg 8.1.3's own film grain synthesis (ff_h274_apply_film_grain,
 * libavcodec/h274.c) makes of synthetic pictures, for src/h274/tests.rs to
 * hold the translation to: for each case, the Adler-32 of each output plane
 * (rows unpadded), or the error it returns. Built against the static
 * libraries tools/ffdbg/build.sh leaves, in WSL:
 *
 *   gcc -O1 -I ~/ffdbg/src -o /tmp/h274_expect gui/video/h264/tools/h274_expect.c \
 *       ~/ffdbg/src/libavcodec/libavcodec.a ~/ffdbg/src/libavutil/libavutil.a -lm -lpthread
 *   /tmp/h274_expect
 *
 * The cases are mirrored, value for value, by the tests' `CASES`.
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "libavcodec/h274.h"
#include "libavutil/adler32.h"
#include "libavutil/film_grain_params.h"
#include "libavutil/frame.h"
#include "libavutil/pixdesc.h"

/* Each input sample: a gradient, so that the 8x8 blocks' means spread over
 * 0-255, with some noise on it. */
static int sample(int c, int x, int y)
{
    return ((x + y) * 3 + c * 59 + ((x * 37 ^ y * 101) & 31)) & 0xFF;
}

struct band { int lo, hi, v0, v1, v2; };
struct comp { int present, n; struct band b[4]; };
struct cas {
    const char *name;
    int w, h;
    enum AVPixelFormat fmt;
    int model_id, log2_scale_factor;
    int seed; /* FFmpeg's int seed, as ff_h2645_sei_to_frame is handed it */
    struct comp c[3];
};

static const struct cas cases[] = {
    { "bands", 64, 48, AV_PIX_FMT_YUV420P, 0, 3, 0, {
        { 1, 3, { { 0, 80, 40, 4, 6 }, { 81, 160, -60, 8, 3 }, { 161, 255, 90, 14, 14 } } },
        { 1, 2, { { 0, 127, 30, 2, 2 }, { 128, 255, -25, 5, 7 } } },
        { 1, 1, { { 0, 255, 50, 6, 4 } } } } },
    { "gaps", 48, 48, AV_PIX_FMT_YUV420P, 0, 2, 7, {
        { 1, 2, { { 20, 60, 70, 3, 9 }, { 140, 200, -45, 12, 2 } } },
        { 1, 1, { { 100, 180, -7, 4, 4 } } },
        { 0, 0, { { 0 } } } } },
    { "strong", 32, 32, AV_PIX_FMT_YUV420P, 0, 0, -5, {
        { 1, 1, { { 0, 255, 2000, 9, 11 } } },
        { 1, 1, { { 0, 255, -3001, 1, 99 } } },
        { 1, 1, { { 0, 255, 32767, 20000, -20000 } } } } },
    { "faint", 80, 16, AV_PIX_FMT_YUV420P, 0, 15, 123456789, {
        { 1, 2, { { 0, 127, -32768, 2, 14 }, { 128, 255, 32767, 14, 2 } } },
        { 0, 0, { { 0 } } },
        { 1, 1, { { 0, 255, 300, 7, 7 } } } } },
    { "model1", 32, 32, AV_PIX_FMT_YUV420P, 1, 3, 0, {
        { 1, 1, { { 0, 255, 50, 6, 6 } } }, { 0 }, { 0 } } },
    { "yuv422", 32, 32, AV_PIX_FMT_YUV422P, 0, 3, 0, {
        { 1, 1, { { 0, 255, 50, 6, 6 } } }, { 0 }, { 0 } } },
};

int main(void)
{
    for (size_t k = 0; k < sizeof(cases) / sizeof(cases[0]); k++) {
        const struct cas *cs = &cases[k];
        AVFrame *in = av_frame_alloc(), *out = av_frame_alloc();
        const AVPixFmtDescriptor *d = av_pix_fmt_desc_get(cs->fmt);
        AVFilmGrainParams p;
        AVFilmGrainH274Params *h = &p.codec.h274;
        int ret;

        in->format = out->format = cs->fmt;
        in->width = out->width = cs->w;
        in->height = out->height = cs->h;
        if (!in || !out || av_frame_get_buffer(in, 0) < 0 || av_frame_get_buffer(out, 0) < 0)
            return 1;
        for (int c = 0; c < 3; c++) {
            int cw = c ? AV_CEIL_RSHIFT(cs->w, d->log2_chroma_w) : cs->w;
            int ch = c ? AV_CEIL_RSHIFT(cs->h, d->log2_chroma_h) : cs->h;
            for (int y = 0; y < ch; y++)
                for (int x = 0; x < cw; x++)
                    in->data[c][y * in->linesize[c] + x] = sample(c, x, y);
        }

        memset(&p, 0, sizeof(p));
        p.type = AV_FILM_GRAIN_PARAMS_H274;
        p.seed = cs->seed;
        h->model_id = cs->model_id;
        h->log2_scale_factor = cs->log2_scale_factor;
        for (int c = 0; c < 3; c++) {
            h->component_model_present[c] = cs->c[c].present;
            h->num_intensity_intervals[c] = cs->c[c].n;
            h->num_model_values[c] = 3;
            for (int i = 0; i < cs->c[c].n; i++) {
                const struct band *b = &cs->c[c].b[i];
                h->intensity_interval_lower_bound[c][i] = b->lo;
                h->intensity_interval_upper_bound[c][i] = b->hi;
                h->comp_model_value[c][i][0] = b->v0;
                h->comp_model_value[c][i][1] = b->v1;
                h->comp_model_value[c][i][2] = b->v2;
            }
        }

        ret = ff_h274_apply_film_grain(out, in, &p);
        printf("%s", cs->name);
        if (ret < 0) {
            printf(" error %d\n", ret);
        } else {
            for (int c = 0; c < 3; c++) {
                int cw = c ? AV_CEIL_RSHIFT(cs->w, d->log2_chroma_w) : cs->w;
                int ch = c ? AV_CEIL_RSHIFT(cs->h, d->log2_chroma_h) : cs->h;
                unsigned long sum = 1;
                for (int y = 0; y < ch; y++)
                    sum = av_adler32_update(sum, out->data[c] + y * out->linesize[c], cw);
                printf(" 0x%08lx", sum);
            }
            printf("\n");
        }
        av_frame_free(&in);
        av_frame_free(&out);
    }
    return 0;
}
