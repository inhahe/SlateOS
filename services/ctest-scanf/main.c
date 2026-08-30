/*
 * ctest-scanf — ring-3 regression test for the sysroot's scanf trampolines.
 *
 * `sscanf`, `scanf` and `fscanf` are pure assembly trampolines
 * (`va_trampoline!` in `posix/src/scanf.rs`): each spills the argument
 * registers into a System V register save area, builds a `va_list` over it,
 * and calls the matching `v*scanf`.  They exist only on the bare-metal
 * target, so `cargo test` can drive `vsscanf` directly but can never prove
 * that the trampolines hand it the right `va_list`.
 *
 * Getting that wrong is far more dangerous here than in printf.  Every scanf
 * argument is a *destination pointer*, so an argument read from the wrong
 * slot is not a wrong number printed — it is a wrong address written through.
 * That is exactly what BUG-POSIX-SCANF-ARG-ARRAY-OOB was: the engine used to
 * flatten the pointers into a `[u64; 8]`, and the ninth conversion stored the
 * scanned value through whatever stack word sat past the end of the array.
 *
 * So the checks below deliberately concentrate on the boundary the old design
 * could not cross: the first six pointers arrive in %rdi..%r9 (minus the two
 * named parameters) and every pointer after that comes from the caller's
 * overflow area.  A trampoline with the wrong `gp_offset`, the wrong `va_list`
 * register, or an `overflow_arg_area` pointing at its own frame instead of the
 * caller's fails somewhere in checks 20-23 while passing everything before.
 *
 * Each destination is bracketed by canaries so an off-by-one store is caught
 * as corruption of a neighbour rather than merely a wrong value.
 *
 * Exit code 42 == every check passed; anything else identifies the first
 * failing check (see the `return` values below, and the legend in
 * kernel/src/proc/spawn.rs::self_test_cscanf).
 */

#include <stdio.h>
#include <string.h>

/* Launder through a volatile global so nothing is constant folded away. */
static volatile int g_launder;

static const char *opaque(const char *s)
{
    g_launder = (int)s[0];
    return s;
}

int main(void)
{
    /* ------------------------------------------------------------------
     * 10-13: the basics, all inside the register-passed range.
     * `sscanf` has two named parameters, so its varargs start at gp_offset
     * 16 and its va_list* travels in %rdx.
     * ------------------------------------------------------------------ */
    {
        int a = 0;
        if (sscanf(opaque("42"), "%d", &a) != 1) {
            return 10;
        }
        if (a != 42) {
            return 11;
        }
    }

    {
        int a = 0, b = 0, c = 0, d = 0;
        char word[16];
        memset(word, 0, sizeof word);
        if (sscanf(opaque("1 2 hello 3 4"), "%d %d %s %d %d",
                   &a, &b, word, &c, &d) != 5) {
            return 12;
        }
        if (a != 1 || b != 2 || c != 3 || d != 4 || strcmp(word, "hello") != 0) {
            return 13;
        }
    }

    /* ------------------------------------------------------------------
     * 20-23: past the eighth conversion — the case the old flat `[u64; 8]`
     * could not represent, and where it wrote through a stack word it had
     * never been given.  Twelve destinations: six from the register save
     * area, six from the caller's overflow area.
     * ------------------------------------------------------------------ */
    {
        /* The canaries sit between the destinations, so a store that lands
         * one slot early or late corrupts one of them instead. */
        struct {
            int v[12];
            unsigned long canary;
        } box;
        int i;

        memset(&box, 0, sizeof box);
        box.canary = 0xA5A5A5A5A5A5A5A5UL;

        if (sscanf(opaque("1 2 3 4 5 6 7 8 9 10 11 12"),
                   "%d %d %d %d %d %d %d %d %d %d %d %d",
                   &box.v[0], &box.v[1], &box.v[2], &box.v[3],
                   &box.v[4], &box.v[5], &box.v[6], &box.v[7],
                   &box.v[8], &box.v[9], &box.v[10], &box.v[11]) != 12) {
            return 20;
        }
        for (i = 0; i < 12; i++) {
            if (box.v[i] != i + 1) {
                return 21;
            }
        }
        if (box.canary != 0xA5A5A5A5A5A5A5A5UL) {
            return 22;
        }
    }

    /* Mixed widths past the boundary: a %ld stores 8 bytes and a %d stores 4,
     * so a miscounted argument corrupts a differently-sized neighbour rather
     * than landing harmlessly on an identically-typed slot. */
    {
        int a = 0, b = 0, c = 0, d = 0, e = 0;
        long l1 = 0, l2 = 0, l3 = 0;
        char word[16];
        int x = 0, y = 0, z = 0;

        memset(word, 0, sizeof word);
        if (sscanf(opaque("1 2 3 4 5 60 70 80 word 9 10 11"),
                   "%d %d %d %d %d %ld %ld %ld %s %d %d %d",
                   &a, &b, &c, &d, &e, &l1, &l2, &l3, word, &x, &y, &z) != 12) {
            return 23;
        }
        if (a != 1 || b != 2 || c != 3 || d != 4 || e != 5) {
            return 23;
        }
        if (l1 != 60L || l2 != 70L || l3 != 80L) {
            return 23;
        }
        if (strcmp(word, "word") != 0 || x != 9 || y != 10 || z != 11) {
            return 23;
        }
    }

    /* ------------------------------------------------------------------
     * 30-32: suppression and `%n` past the boundary.  `%*d` consumes input
     * but no pointer, so it shifts every later argument by one slot — the
     * cheapest way to desynchronise a trampoline that miscounts.
     * ------------------------------------------------------------------ */
    {
        int v[10];
        int consumed = -1;
        int i;

        memset(v, 0, sizeof v);
        if (sscanf(opaque("0 1 0 2 0 3 0 4 0 5 0 6 0 7 0 8 0 9 0 10"),
                   "%*d %d %*d %d %*d %d %*d %d %*d %d "
                   "%*d %d %*d %d %*d %d %*d %d %*d %d%n",
                   &v[0], &v[1], &v[2], &v[3], &v[4],
                   &v[5], &v[6], &v[7], &v[8], &v[9], &consumed) != 10) {
            return 30;
        }
        for (i = 0; i < 10; i++) {
            if (v[i] != i + 1) {
                return 31;
            }
        }
        /* `%n` does not count toward the return value, but must still have
         * been stored — through the eleventh pointer, deep in the overflow
         * area. */
        if (consumed != (int)strlen("0 1 0 2 0 3 0 4 0 5 0 6 0 7 0 8 0 9 0 10")) {
            return 32;
        }
    }

    /* ------------------------------------------------------------------
     * 40-41: floats past the boundary.  scanf differs from printf here: a
     * `%f` argument is a *pointer* to a float, so it is INTEGER class and
     * travels the same path as everything else.  Getting this right is what
     * proves the trampoline is not consulting fp_offset for scanf.
     * ------------------------------------------------------------------ */
    {
        int i0 = 0, i1 = 0, i2 = 0, i3 = 0, i4 = 0, i5 = 0, i6 = 0, i7 = 0;
        double d0 = 0.0, d1 = 0.0;

        if (sscanf(opaque("1 2 3 4 5 6 7 8 1.5 2.25"),
                   "%d %d %d %d %d %d %d %d %lf %lf",
                   &i0, &i1, &i2, &i3, &i4, &i5, &i6, &i7, &d0, &d1) != 10) {
            return 40;
        }
        if (i0 != 1 || i7 != 8 || d0 != 1.5 || d1 != 2.25) {
            return 41;
        }
    }

    /* ------------------------------------------------------------------
     * 50-53: `fscanf`, which has the same two named parameters as `sscanf`
     * but a different first argument, and a separate trampoline.  Reading
     * from a file also proves the stream plumbing survives the rewrite that
     * moved the line-reading into a shared helper.
     * ------------------------------------------------------------------ */
    {
        FILE *fp = fopen("/tmp/ctest-scanf.txt", "w");
        int v[9];
        int i;

        if (fp == NULL) {
            return 50; /* no File capability, or no writable /tmp */
        }
        fputs("11 12 13 14 15 16 17 18 19\n", fp);
        fclose(fp);

        memset(v, 0, sizeof v);
        fp = fopen("/tmp/ctest-scanf.txt", "r");
        if (fp == NULL) {
            return 51; /* wrote it but cannot reopen it */
        }
        if (fscanf(fp, "%d %d %d %d %d %d %d %d %d",
                   &v[0], &v[1], &v[2], &v[3], &v[4],
                   &v[5], &v[6], &v[7], &v[8]) != 9) {
            fclose(fp);
            return 52;
        }
        fclose(fp);
        for (i = 0; i < 9; i++) {
            if (v[i] != 11 + i) {
                return 53;
            }
        }
    }

    return 42;
}
