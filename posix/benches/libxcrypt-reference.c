/* libxcrypt's crypt, timed as posix/benches/crypt.rs times ours: the same
 * settings, password and repetitions, and the same report line, so the two
 * outputs line up row for row.  It is the reference performance-targets.md
 * names for password hashing.
 *
 * Run it under WSL on the machine the Rust bench runs on -- Ubuntu's
 * libcrypt.so.1 is libxcrypt 4.4.36, built with its SSE2 code -- and, like
 * the Rust bench, on a quiet machine:
 *
 *     gcc -O2 -o libxcrypt-reference libxcrypt-reference.c -lcrypt
 *     ./libxcrypt-reference
 */

#include <crypt.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

static double now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec / 1e9;
}

int main(void)
{
    static const struct {
        const char *setting;
        int reps;
    } cases[] = {
        { "$y$j9T$PKXc3hCOSyMqdaEQArI62/", 20 },
        { "$y$j75$LdJMENpBABJJ3hIHjB1Bi.", 100 },
        { "$7$CU..../....SodiumChloride", 10 },
        { "$7$66..../....SodiumChloride", 200 },
        { "$6$saltstring", 200 },
    };
    static struct crypt_data data;
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        /* Warm: the first hash pays for the first mapping. */
        if (!crypt_r("pleaseletmein", cases[i].setting, &data) || data.output[0] == '*') {
            fprintf(stderr, "%s does not hash\n", cases[i].setting);
            return 1;
        }
        double best = 1e9;
        for (int round = 0; round < 3; round++) {
            double started = now();
            for (int k = 0; k < cases[i].reps; k++)
                crypt_r("pleaseletmein", cases[i].setting, &data);
            double each = (now() - started) / cases[i].reps;
            if (each < best)
                best = each;
        }
        printf("%-34s %9.3f ms\n", cases[i].setting, best * 1e3);
    }
    return 0;
}
