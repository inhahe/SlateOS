/*
 * FFmpeg's fixed-point Kaiser-Bessel-derived windows, as aacdec_fixed.c
 * makes them when the decoder opens (ff_kbd_window_init_fixed), printed as
 * C arrays for tools/gen_tables.py to read: they come of `double`
 * arithmetic and libm's bessel terms, which another maths library could
 * round otherwise, so they are taken as FFmpeg's build computes them.
 *
 * Compiled and run by tables.sh.
 */
#include <stdio.h>
#include "libavcodec/kbdwin.c"

static void print(const char *name, const int *v, int n)
{
    printf("static const int %s[%d] = {\n", name, n);
    for (int i = 0; i < n; i++)
        printf("    %d,\n", v[i]);
    printf("};\n\n");
}

int main(void)
{
    static int long_1024[1024], short_128[128], long_960[960], short_120[120];
    ff_kbd_window_init_fixed(long_1024, 4.0, 1024);
    ff_kbd_window_init_fixed(short_128, 6.0, 128);
    ff_kbd_window_init_fixed(long_960, 4.0, 960);
    ff_kbd_window_init_fixed(short_120, 6.0, 120);
    print("aac_kbd_long_1024_fixed", long_1024, 1024);
    print("aac_kbd_short_128_fixed", short_128, 128);
    print("aac_kbd_long_960_fixed", long_960, 960);
    print("aac_kbd_short_120_fixed", short_120, 120);
    return 0;
}
