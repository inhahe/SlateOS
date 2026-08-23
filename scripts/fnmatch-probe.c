/* Emit glibc fnmatch(3)'s answer for a cross product of patterns, names and
 * flag sets, as the fixture read by userspace/coreutils/tests/fnmatch_glibc.rs.
 *
 * The cross product is large, so it is written as one line per
 * (flags, pattern) with a bitmap over the shared name list rather than one
 * line per case — same information, a twentieth of the bytes.
 *
 * Format (tab-separated, `#` comments ignored):
 *
 *   N<TAB><name-hex>                     the name list, in order, bit 0 first
 *   M<TAB><flags><TAB><pattern-hex><TAB><bitmap-hex>
 *
 * `flags` is glibc's numeric FNM_* value, which `Flags` reproduces exactly.
 * The bitmap is little-endian by nibble: character k of the bitmap holds names
 * 4k..4k+3, bit 0 of the nibble being the lowest-numbered of the four. Hex
 * throughout because a pattern or a name may hold any byte but NUL.
 *
 *   gcc -O2 -o fnmatch_probe fnmatch_probe.c && ./fnmatch_probe > fixture.txt
 */
#define _GNU_SOURCE
#include <fnmatch.h>
#include <stdio.h>
#include <string.h>

static const char *patterns[] = {
    /* literals */
    "", "a", "abc", "a/b", "/", "//", ".", "..", "a.b",
    /* star */
    "*", "**", "*a", "a*", "*a*", "a*b", "*/*", "a*/*b", "*.*", "**/*",
    "*x*x*x*y", ".*", "*.", "/*", "*/", "a**b", "*/keep", "*a*b*c*",
    /* question */
    "?", "??", "a?", "?a", "a?b", "?*", "*?", "/?", "?/", "?.?",
    /* brackets: sets */
    "[a]", "[ab]", "[abc]", "[a][b]", "[a]*", "*[a]", "[/]", "[.]", "[*]",
    "[?]", "[[]", "[]]", "[]a]", "[a]]", "[ab]c",
    /* brackets: negation */
    "[!a]", "[^a]", "[!ab]", "[!/]", "[^/]", "[!.]", "[a!]", "[a^]", "[!]]",
    "[!]a]", "[!!]", "*[!a]",
    /* brackets: ranges */
    "[a-c]", "[a-]", "[-a]", "[-]", "[a-c-e]", "[!a-c]", "[A-Za-z]", "[0-9]",
    "[c-a]", "[a-a]", "[.-/]", "[--0]", "[]-a]", "[a-]]", "[!a-]",
    /* brackets: classes */
    "[[:alpha:]]", "[[:digit:]]", "[[:upper:]]", "[[:lower:]]", "[[:space:]]",
    "[[:punct:]]", "[[:xdigit:]]", "[[:alnum:]]", "[[:blank:]]", "[[:cntrl:]]",
    "[[:graph:]]", "[[:print:]]", "[![:digit:]]", "[[:digit:]abc]",
    "[abc[:digit:]]", "[[:bogus:]]", "[![:bogus:]]", "[[:digit:]-a]",
    "[[:alpha:][:digit:]]", "[![:alpha:]]", "[[:upper:]][[:lower:]]",
    /* brackets: collating / equivalence */
    "[[.a.]]", "[.a.]", "[=a=]", "[[=a=]]", "[[.hyphen.]]", "[[.a.]b]",
    /* brackets: malformed */
    "[", "[a", "[abc", "a[b", "[!", "[]", "[]a", "[[:alpha:", "[a-", "a[",
    /* escapes */
    "\\*", "\\?", "\\[", "\\\\", "a\\*b", "\\a", "\\", "*\\*", "[\\]]",
    "[a\\-c]", "[\\a]", "\\.", "\\/", "\\*\\?", "a\\", "[a\\]",
};

static const char *names[] = {
    "", "a", "b", "c", "z", "A", "Z", "abc", "aXc", "a.b", "ab", "aa",
    ".", "..", ".a", "a.", "a/b", "/", "//", "a/", "/a", "a/b/c", "a/.b",
    "/.a", "*", "?", "[", "]", "-", "!", "^", "\\", "/a/b", "3", "7", "0",
    "f", "g", " ", "\t", "\x01", "\xe9", "\xff", "\x80", "[a]", "[abc",
    "a[b", "[.a.]", "[=a=]", "xxxy", "xxxxxxxxxxxxxxxxxxxxy", "a*b", "a?b",
    ".hidden", "a/.hidden", "abcd", "aXbXc", "AbC", "a-c", "]a", "a]",
    "[]", "\\a", "e", "d", "ac", "keep", "aa/keep", "a\\b", "ABC", "aBc",
};

static const int flagsets[] = {
    0,
    FNM_PATHNAME,
    FNM_PERIOD,
    FNM_PATHNAME | FNM_PERIOD,
    FNM_NOESCAPE,
    FNM_CASEFOLD,
    FNM_LEADING_DIR,
    FNM_PATHNAME | FNM_LEADING_DIR,
    FNM_PATHNAME | FNM_PERIOD | FNM_LEADING_DIR,
    FNM_NOESCAPE | FNM_CASEFOLD,
    FNM_PATHNAME | FNM_NOESCAPE | FNM_PERIOD | FNM_CASEFOLD | FNM_LEADING_DIR,
};

static void put_hex(const char *s) {
    for (const unsigned char *p = (const unsigned char *)s; *p; p++)
        printf("%02x", *p);
}

int main(void) {
    size_t np = sizeof patterns / sizeof *patterns;
    size_t nn = sizeof names / sizeof *names;
    size_t nf = sizeof flagsets / sizeof *flagsets;

    printf("# glibc fnmatch(3) reference answers; see fnmatch_probe.c\n");
    for (size_t j = 0; j < nn; j++) {
        printf("N\t");
        put_hex(names[j]);
        printf("\n");
    }
    for (size_t f = 0; f < nf; f++)
        for (size_t i = 0; i < np; i++) {
            printf("M\t%d\t", flagsets[f]);
            put_hex(patterns[i]);
            printf("\t");
            for (size_t j = 0; j < nn; j += 4) {
                unsigned nib = 0;
                for (size_t k = 0; k < 4 && j + k < nn; k++)
                    if (fnmatch(patterns[i], names[j + k], flagsets[f]) == 0)
                        nib |= 1u << k;
                printf("%x", nib);
            }
            printf("\n");
        }
    return 0;
}
