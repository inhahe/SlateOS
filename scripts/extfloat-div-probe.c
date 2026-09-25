/* Print x87 long double division vectors: "A B Q" as 20 hex digits each,
   the ten bytes of the value in memory order (little-endian). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <float.h>
#include <math.h>

static void hex(long double v) {
    unsigned char b[16];
    memset(b, 0, sizeof b);
    memcpy(b, &v, 10);
    for (int i = 0; i < 10; i++) printf("%02x", b[i]);
}

static uint64_t state = 0x9e3779b97f4a7c15ull;
static uint64_t next(void) {
    state ^= state << 13; state ^= state >> 7; state ^= state << 17;
    return state;
}

static long double random_ld(void) {
    unsigned char b[16] = {0};
    uint64_t sig = next() | (1ull << 63);
    int exp = (int)(next() % 200) - 100 + 16383;
    if (next() % 7 == 0) exp = (int)(next() % 32766) + 1;
    if (next() % 13 == 0) { exp = 0; sig >>= (next() % 64); }
    uint16_t se = (uint16_t)exp | ((next() & 1) ? 0x8000 : 0);
    memcpy(b, &sig, 8);
    memcpy(b + 8, &se, 2);
    long double v;
    memcpy(&v, b, 10);
    return v;
}

static void pair(long double a, long double b) {
    volatile long double q = a / b;
    hex(a); putchar(' '); hex(b); putchar(' '); hex(q); putchar('\n');
}

int main(void) {
    long double specials[] = {
        1.0L, 3.0L, 10.0L, 1000.0L, 1024.0L, 7.0L, 0.1L, 1e18L, 123456789.0L,
        LDBL_MAX, LDBL_MIN, LDBL_TRUE_MIN, -2.5L, 9223372036854775807.0L,
        18446744073709551615.0L, 1e-4000L, 1e4000L, 0.0L, -0.0L,
    };
    int n = sizeof specials / sizeof specials[0];
    for (int i = 0; i < n; i++)
        for (int j = 0; j < n; j++)
            if (!(specials[j] == 0 && specials[i] == 0))
                pair(specials[i], specials[j]);
    for (int i = 0; i < 400; i++)
        pair(random_ld(), random_ld());
    return 0;
}
