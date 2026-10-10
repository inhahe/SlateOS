/* ctest-string: the C library's memory and string functions on SlateOS
 * itself -- its 16 KiB pages, its kernel, its code model -- where the host
 * tests (posix/src/string.rs, wchar.rs) cannot reach.
 *
 * Since 2026-10-06 these are SSE2 and `rep movsb` (string.rs, "The engines"
 * and "The scanners"), and the scanners read whole aligned 16-byte blocks,
 * past a string's terminator but never past its page.  So:
 *
 *   1. every length to 80 at every alignment, against byte loops;
 *   2. strings and buffers set against a page that faults (mmap, then the
 *      neighbouring pages taken away with mprotect, or munmap if that is
 *      refused) -- a read outside the page kills the fixture;
 *   3. copies large enough for `rep movsb`, and moves overlapping both ways;
 *   4. the searches against trying every place, and the wide functions.
 *
 * Exit 42 when every check passes; 1 with a line per failure otherwise.
 * Nothing in it waits, and it needs no capability.
 *
 * Compiled with -fno-builtin, so the reference loops below stay loops and
 * every call reaches libc.a. */

#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wchar.h>

static int failures;
static int checks;

#define CHECK(cond, ...)                                                    \
    do {                                                                    \
        checks++;                                                           \
        if (!(cond)) {                                                      \
            failures++;                                                     \
            if (failures <= 40) {                                           \
                printf("FAIL line %d: ", __LINE__);                         \
                printf(__VA_ARGS__);                                        \
                printf("\n");                                               \
            }                                                               \
        }                                                                   \
    } while (0)

/* ---- the reference: a byte at a time ---------------------------------- */

static size_t ref_strlen(const char *s) {
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

static const char *ref_strchr(const char *s, int c) {
    for (;; s++) {
        if (*s == (char)c) return s;
        if (!*s) return NULL;
    }
}

static const char *ref_strrchr(const char *s, int c) {
    const char *last = NULL;
    for (;; s++) {
        if (*s == (char)c) last = s;
        if (!*s) return last;
    }
}

static int sign(int x) { return (x > 0) - (x < 0); }

static int ref_strcmp(const char *a, const char *b) {
    const unsigned char *x = (const unsigned char *)a, *y = (const unsigned char *)b;
    while (*x && *x == *y) { x++; y++; }
    return (int)*x - (int)*y;
}

static int ref_memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (size_t i = 0; i < n; i++)
        if (x[i] != y[i]) return (int)x[i] - (int)y[i];
    return 0;
}

static const char *ref_strstr(const char *h, const char *n) {
    size_t hl = ref_strlen(h), nl = ref_strlen(n);
    if (nl == 0) return h;
    for (size_t i = 0; i + nl <= hl; i++)
        if (ref_memcmp(h + i, n, nl) == 0) return h + i;
    return NULL;
}

/* A byte that is never 0 and never 'q', varying with i and seed. */
static unsigned char filler(size_t i, size_t seed) {
    unsigned char b = (unsigned char)((i * 37 + seed * 11) % 250 + 1);
    return b == 'q' ? 'r' : b;
}

/* ---- 1. every length and alignment against the byte loops ------------- */

static void lengths_and_alignments(void) {
    static unsigned char a[512], b[512];
    for (size_t len = 0; len <= 80; len++) {
        for (size_t align = 0; align < 16; align++) {
            char *s = (char *)a + 16 + align;
            for (size_t i = 0; i < len; i++) s[i] = (char)filler(i, len);
            s[len] = 0;
            s[len + 1] = 'Z';
            CHECK(strlen(s) == len, "strlen len %zu align %zu", len, align);
            CHECK(strnlen(s, len / 2) == len / 2, "strnlen len %zu", len);
            CHECK(strchr(s, 'q') == NULL, "strchr absent len %zu align %zu", len, align);
            CHECK(strchr(s, 0) == s + len, "strchr NUL len %zu", len);
            CHECK(rawmemchr(s, 0) == s + len, "rawmemchr len %zu", len);
            CHECK(memchr(s, 'q', len) == NULL, "memchr absent len %zu", len);
            for (size_t at = 0; at < len; at += 3) {
                char saved = s[at];
                s[at] = 'q';
                CHECK(strchr(s, 'q') == ref_strchr(s, 'q'), "strchr len %zu at %zu", len, at);
                CHECK(strrchr(s, 'q') == ref_strrchr(s, 'q'), "strrchr len %zu at %zu", len, at);
                CHECK(memchr(s, 'q', len) == s + at, "memchr len %zu at %zu", len, at);
                CHECK(memrchr(s, 'q', len) == s + at, "memrchr len %zu at %zu", len, at);
                CHECK(strchrnul(s, 'q') == s + at, "strchrnul len %zu at %zu", len, at);
                s[at] = saved;
            }
            /* Comparisons: equal, then differing at each place by a high byte. */
            char *t = (char *)b + 16 + (15 - align);
            memcpy(t, s, len + 1);
            CHECK(strcmp(s, t) == 0, "strcmp equal len %zu", len);
            CHECK(memcmp(s, t, len) == 0, "memcmp equal len %zu", len);
            CHECK(strcasecmp(s, t) == 0, "strcasecmp equal len %zu", len);
            for (size_t at = 0; at < len; at += 5) {
                char saved = t[at];
                t[at] = (char)0xF7;
                CHECK(sign(strcmp(s, t)) == sign(ref_strcmp(s, t)), "strcmp len %zu at %zu", len, at);
                CHECK(sign(memcmp(s, t, len)) == sign(ref_memcmp(s, t, len)), "memcmp len %zu at %zu", len, at);
                CHECK(strncmp(s, t, at) == 0, "strncmp before len %zu at %zu", len, at);
                t[at] = saved;
            }
        }
    }
}

/* ---- 2. against a page that faults ------------------------------------ */

static unsigned char *guarded_page(long page) {
    unsigned char *region = mmap(NULL, 3 * (size_t)page, PROT_READ | PROT_WRITE,
                                 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (region == MAP_FAILED) return NULL;
    if (mprotect(region, (size_t)page, PROT_NONE) == 0 &&
        mprotect(region + 2 * page, (size_t)page, PROT_NONE) == 0)
        return region + page;
    if (munmap(region, (size_t)page) == 0 && munmap(region + 2 * page, (size_t)page) == 0)
        return region + page;
    printf("note: no page could be taken away; the edge checks run unguarded\n");
    return region + page;
}

static void page_edges(void) {
    long page = sysconf(_SC_PAGESIZE);
    CHECK(page >= 4096, "page size %ld", page);
    if (page < 4096) return;
    unsigned char *g = guarded_page(page), *h = guarded_page(page);
    CHECK(g != NULL && h != NULL, "mmap of the guarded pages");
    if (!g || !h) return;
    unsigned char *gend = g + page, *hend = h + page;
    for (size_t len = 0; len <= 48; len++) {
        /* Strings whose terminator is the page's last byte. */
        char *p = (char *)gend - len - 1, *q = (char *)hend - len - 1;
        for (size_t i = 0; i < len; i++) p[i] = q[i] = (char)filler(i, len);
        p[len] = q[len] = 0;
        CHECK(strlen(p) == len, "edge strlen %zu", len);
        CHECK(strnlen(p, (size_t)-1) == len, "edge strnlen %zu", len);
        CHECK(strchr(p, 'q') == NULL, "edge strchr %zu", len);
        CHECK(strrchr(p, 'q') == NULL, "edge strrchr %zu", len);
        CHECK(strcmp(p, q) == 0, "edge strcmp %zu", len);
        CHECK(strncmp(p, q, (size_t)-1) == 0, "edge strncmp %zu", len);
        CHECK(strcasecmp(p, q) == 0, "edge strcasecmp %zu", len);
        CHECK(strstr(p, "qq") == NULL, "edge strstr %zu", len);
        CHECK(strspn(p, "q") == 0 || p[0] == 'q', "edge strspn %zu", len);
        CHECK(strcspn(p, "q") == len, "edge strcspn %zu", len);
        /* Runs with no terminator, ending at the page's end. */
        unsigned char *r = gend - len, *u = hend - len;
        for (size_t i = 0; i < len; i++) r[i] = u[i] = filler(i, len + 7);
        CHECK(strnlen((char *)r, len) == len, "edge strnlen run %zu", len);
        CHECK(memchr(r, 'q', len) == NULL, "edge memchr %zu", len);
        CHECK(memrchr(r, 'q', len) == NULL, "edge memrchr %zu", len);
        CHECK(memcmp(r, u, len) == 0, "edge memcmp %zu", len);
        CHECK(strncmp((char *)r, (char *)u, len) == 0, "edge strncmp run %zu", len);
        CHECK(memmem(r, len, "qq", 2) == NULL, "edge memmem %zu", len);
        /* The page's first bytes, for the backward search: nothing below
         * them may be read. */
        CHECK(memrchr(g, 'q', len) == NULL, "start memrchr %zu", len);
        CHECK(memchr(g, 'q', len) == NULL, "start memchr %zu", len);
    }
}

/* ---- 3. large copies and overlapping moves ---------------------------- */

static void copies(void) {
    size_t big = 1 << 20;
    unsigned char *src = malloc(big + 64), *dst = malloc(big + 64);
    CHECK(src && dst, "malloc for the copies");
    if (!src || !dst) return;
    for (size_t i = 0; i < big + 64; i++) src[i] = filler(i, i >> 8);
    for (size_t n = 0; n < big; n = n < 300 ? n + 1 : n * 2 + 17) {
        memset(dst, 0xEE, big + 64);
        CHECK(memcpy(dst + 5, src + 3, n) == dst + 5, "memcpy return %zu", n);
        CHECK(ref_memcmp(dst + 5, src + 3, n) == 0, "memcpy bytes %zu", n);
        CHECK(dst[4] == 0xEE && dst[5 + n] == 0xEE, "memcpy edges %zu", n);
        memset(dst, 0xEE, big + 64);
        CHECK(memset(dst + 1, 0x33, n) == dst + 1, "memset return %zu", n);
        int ok = dst[0] == 0xEE && dst[1 + n] == 0xEE;
        for (size_t i = 0; i < n && ok; i++) ok = dst[1 + i] == 0x33;
        CHECK(ok, "memset %zu", n);
    }
    /* memmove, overlapping either way, against a copy made a byte at a time. */
    for (size_t n = 0; n < 70000; n = n < 300 ? n + 1 : n * 3 + 1) {
        for (int shift = -9; shift <= 9; shift += 3) {
            for (size_t i = 0; i < n + 40; i++) dst[i] = filler(i, n);
            unsigned char *from = dst + 20, *to = dst + 20 + shift;
            static unsigned char want[1 << 17];
            if (n + 40 > sizeof want) continue;
            for (size_t i = 0; i < n + 40; i++) want[i] = dst[i];
            for (size_t i = 0; i < n; i++) want[20 + shift + i] = dst[20 + i];
            memmove(to, from, n);
            CHECK(ref_memcmp(dst, want, n + 40) == 0, "memmove %zu shift %d", n, shift);
        }
    }
    free(src);
    free(dst);
}

/* ---- 4. the searches and the wide functions --------------------------- */

static void searches(void) {
    /* Every haystack of a and b to length 10, every needle to 5. */
    char hay[16], needle[8];
    for (int hl = 0; hl <= 10; hl++) {
        for (unsigned hb = 0; hb < (1u << hl); hb++) {
            for (int i = 0; i < hl; i++) hay[i] = (hb >> i) & 1 ? 'b' : 'a';
            hay[hl] = 0;
            for (int nl = 1; nl <= 5; nl++) {
                for (unsigned nb = 0; nb < (1u << nl); nb++) {
                    for (int i = 0; i < nl; i++) needle[i] = (nb >> i) & 1 ? 'b' : 'a';
                    needle[nl] = 0;
                    const char *want = ref_strstr(hay, needle);
                    CHECK(strstr(hay, needle) == want, "strstr %s %s", hay, needle);
                    CHECK(memmem(hay, (size_t)hl, needle, (size_t)nl) == want, "memmem %s %s", hay, needle);
                }
            }
        }
    }
    /* The long periodic needle: a^2000 b in a^100000 then b. */
    size_t n = 100000;
    char *h = malloc(n + 2), *nd = malloc(2002);
    CHECK(h && nd, "malloc for the long search");
    if (h && nd) {
        memset(h, 'a', n);
        h[n] = 0;
        memset(nd, 'a', 2000);
        nd[2000] = 'b';
        nd[2001] = 0;
        CHECK(strstr(h, nd) == NULL, "long strstr absent");
        h[n] = 'b';
        h[n + 1] = 0;
        CHECK(strstr(h, nd) == h + n - 2000, "long strstr found");
    }
    free(h);
    free(nd);
    /* The wide functions, at every alignment a wchar_t may have. */
    static wchar_t w[200], v[200];
    for (size_t len = 0; len <= 40; len++) {
        for (size_t off = 0; off < 4; off++) {
            wchar_t *s = w + 4 + off, *t = v + 4 + (3 - off);
            for (size_t i = 0; i < len; i++) s[i] = t[i] = (wchar_t)(0x41 + (i * 7) % 40) | (i % 5 == 0 ? 0x1F000 : 0);
            s[len] = t[len] = 0;
            CHECK(wcslen(s) == len, "wcslen %zu", len);
            CHECK(wcscmp(s, t) == 0, "wcscmp %zu", len);
            CHECK(wcschr(s, L'q') == NULL, "wcschr %zu", len);
            CHECK(wmemchr(s, L'q', len) == NULL, "wmemchr %zu", len);
            CHECK(wmemcmp(s, t, len) == 0, "wmemcmp %zu", len);
            if (len > 0) {
                t[len - 1] = -1;
                CHECK(wcscmp(s, t) > 0, "wcscmp signed %zu", len);
                t[len - 1] = s[len - 1];
            }
        }
    }
}

int main(void) {
    lengths_and_alignments();
    page_edges();
    copies();
    searches();
    printf("ctest-string: %d checks, %d failed\n", checks, failures);
    return failures == 0 ? 42 : 1;
}
