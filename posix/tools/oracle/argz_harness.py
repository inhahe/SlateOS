"""glibc 2.39's argz and envz vectors -- <argz.h> and <envz.h> -- as the
oracle for posix/src/argz.rs.

    python posix/tools/oracle/argz_harness.py   # writes posix/src/argz_oracle.txt

One line a probe, `<name> = <what it gave>`. A vector is written
`<length> <bytes>`, each NUL as `|` and other bytes below 0x20 or above 0x7e
as `\\xNN`; a NULL vector is `NULL`. An error_t result is its errno name, or
`0`.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "argz_oracle.txt"

PROGRAM = r'''
#define _GNU_SOURCE
#include <argz.h>
#include <envz.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void vec(const char *v, size_t len)
{
    if (!v) { printf(" NULL %zu", len); return; }
    printf(" %zu ", len);
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)v[i];
        if (c == 0) putchar('|');
        else if (c < 0x20 || c > 0x7e) printf("\\x%02x", c);
        else putchar(c);
    }
}

static void err(error_t e)
{
    printf(" %s", e == 0 ? "0" : e == ENOMEM ? "ENOMEM" : e == EINVAL ? "EINVAL" : "other");
}

static void make(char **v, size_t *len, const char *const *items)
{
    *v = NULL;
    *len = 0;
    for (; *items; items++) argz_add(v, len, *items);
}

int main(void)
{
    char *v; size_t len;

    /* argz_create */
    { char *argv[] = {"a", "bc", "", "def", NULL}; err(0); }
    printf("\n");
    {
        char *argv[] = {"a", "bc", "", "def", NULL};
        printf("create ="); err(argz_create(argv, &v, &len)); vec(v, len); printf("\n"); free(v);
        char *none[] = {NULL};
        printf("create-empty ="); err(argz_create(none, &v, &len)); vec(v, len); printf("\n"); free(v);
        char *one[] = {"", NULL};
        printf("create-one-empty ="); err(argz_create(one, &v, &len)); vec(v, len); printf("\n"); free(v);
    }
    /* argz_create_sep */
    {
        const char *cases[] = {"a:bc::def:", ":a", "", ":::", "abc", "a::", "::b"};
        for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
            printf("create_sep %zu =", i);
            err(argz_create_sep(cases[i], ':', &v, &len));
            vec(v, len);
            printf(" count=%zu\n", argz_count(v, len));
            free(v);
        }
    }
    /* argz_count, argz_extract, argz_stringify, argz_next */
    {
        const char *items[] = {"one", "", "three", NULL};
        make(&v, &len, items);
        printf("count = %zu\n", argz_count(v, len));
        char *argv[8];
        argz_extract(v, len, argv);
        printf("extract =");
        for (int i = 0; argv[i]; i++) printf(" [%s]", argv[i]);
        printf(" %d\n", argv[3] == NULL);
        printf("next =");
        for (char *e = argz_next(v, len, NULL); e; e = argz_next(v, len, e)) printf(" [%s]", e);
        printf(" %d\n", argz_next(NULL, 0, NULL) == NULL);
        argz_stringify(v, len, ',');
        printf("stringify ="); vec(v, len); printf("\n");
        free(v);
        printf("stringify-empty ="); argz_stringify(NULL, 0, ','); printf(" ok\n");
    }
    /* argz_append, argz_add, argz_add_sep */
    {
        v = NULL; len = 0;
        printf("add ="); err(argz_add(&v, &len, "x")); err(argz_add(&v, &len, "")); err(argz_add(&v, &len, "yz"));
        vec(v, len); printf("\n");
        printf("append ="); err(argz_append(&v, &len, "p\0q\0", 4)); err(argz_append(&v, &len, "", 0));
        vec(v, len); printf("\n");
        printf("add_sep ="); err(argz_add_sep(&v, &len, "m,,n,", ',')); err(argz_add_sep(&v, &len, "", ','));
        vec(v, len); printf("\n");
        free(v);
    }
    /* argz_delete */
    {
        const char *items[] = {"aa", "bb", "cc", NULL};
        const char *which[] = {"first", "middle", "last"};
        for (int k = 0; k < 3; k++) {
            make(&v, &len, items);
            char *e = v;
            for (int j = 0; j < k; j++) e = argz_next(v, len, e);
            argz_delete(&v, &len, e);
            printf("delete-%s =", which[k]); vec(v, len); printf("\n");
            free(v);
        }
        const char *only[] = {"solo", NULL};
        make(&v, &len, only);
        argz_delete(&v, &len, v);
        printf("delete-only ="); vec(v, len); printf("\n");
        free(v);
        make(&v, &len, items);
        argz_delete(&v, &len, NULL);
        printf("delete-null ="); vec(v, len); printf("\n");
        free(v);
    }
    /* argz_insert */
    {
        const char *items[] = {"aa", "bb", "cc", NULL};
        make(&v, &len, items);
        printf("insert-first ="); err(argz_insert(&v, &len, v, "new")); vec(v, len); printf("\n"); free(v);
        make(&v, &len, items);
        printf("insert-middle ="); err(argz_insert(&v, &len, argz_next(v, len, v), "new")); vec(v, len); printf("\n"); free(v);
        make(&v, &len, items);
        printf("insert-null ="); err(argz_insert(&v, &len, NULL, "new")); vec(v, len); printf("\n"); free(v);
        make(&v, &len, items);
        printf("insert-inside ="); err(argz_insert(&v, &len, v + 4, "new")); vec(v, len); printf("\n"); free(v);
        v = NULL; len = 0;
        printf("insert-empty ="); err(argz_insert(&v, &len, NULL, "new")); vec(v, len); printf("\n"); free(v);
        make(&v, &len, items);
        printf("insert-outside ="); err(argz_insert(&v, &len, v + len + 5, "new")); vec(v, len); printf("\n"); free(v);
    }
    /* argz_replace */
    {
        struct { const char *items[4]; const char *str, *with; } r[] = {
            {{"abcab", "xab", "zz", NULL}, "ab", "X"},
            {{"abcab", "xab", "zz", NULL}, "ab", "LONGER"},
            {{"abcab", "xab", "zz", NULL}, "ab", ""},
            {{"aaa", NULL}, "aa", "b"},
            {{"abc", NULL}, "", "X"},
            {{"abc", NULL}, "zz", "X"},
            {{"abc", NULL}, "abc", ""},
            {{"ababab", NULL}, "ab", "X"},
            {{"ab", "ab", "ab"}, "ab", "X"},
            {{"xabyabz", "q", NULL}, "ab", "--"},
        };
        for (size_t i = 0; i < sizeof r / sizeof r[0]; i++) {
            make(&v, &len, r[i].items);
            unsigned count = 7;
            printf("replace %zu =", i);
            err(argz_replace(&v, &len, r[i].str, r[i].with, &count));
            vec(v, len);
            printf(" count=%u\n", count);
            free(v);
        }
        make(&v, &len, r[0].items);
        printf("replace-null-count ="); err(argz_replace(&v, &len, "ab", "Y", NULL)); vec(v, len); printf("\n");
        free(v);
    }

    /* envz */
    {
        const char *items[] = {"A=1", "B", "C=", "PATH=/bin:/usr/bin", "AB=2", NULL};
        make(&v, &len, items);
        const char *names[] = {"A", "B", "C", "PATH", "AB", "D", "A=1", ""};
        for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
            char *e = envz_entry(v, len, names[i]);
            char *g = envz_get(v, len, names[i]);
            printf("envz %s = %s %s%s%s\n", names[i], e ? e : "NULL", g ? "[" : "", g ? g : "NULL", g ? "]" : "");
        }
        printf("envz_add ="); err(envz_add(&v, &len, "A", "9")); err(envz_add(&v, &len, "E", "5"));
        err(envz_add(&v, &len, "B", NULL)); err(envz_add(&v, &len, "C", NULL));
        vec(v, len); printf("\n");
        envz_remove(&v, &len, "PATH");
        envz_remove(&v, &len, "NOPE");
        printf("envz_remove ="); vec(v, len); printf("\n");
        envz_strip(&v, &len);
        printf("envz_strip ="); vec(v, len); printf("\n");
        free(v);
    }
    {
        const char *a[] = {"X=1", "Y=2", "Z", NULL};
        const char *b[] = {"Y=20", "W=3", "Z=30", "X", NULL};
        for (int override = 0; override < 2; override++) {
            char *v2; size_t len2;
            make(&v, &len, a);
            make(&v2, &len2, b);
            printf("envz_merge %d =", override);
            err(envz_merge(&v, &len, v2, len2, override));
            vec(v, len); printf("\n");
            free(v); free(v2);
        }
    }
    {
        /* Stripping everything leaves a NULL vector or an empty one? */
        const char *items[] = {"P", "Q", NULL};
        make(&v, &len, items);
        envz_strip(&v, &len);
        printf("envz_strip-all ="); vec(v, len); printf("\n");
        free(v);
        make(&v, &len, items);
        envz_remove(&v, &len, "P");
        envz_remove(&v, &len, "Q");
        printf("envz_remove-all ="); vec(v, len); printf("\n");
        free(v);
    }
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "az.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o az az.c && ./az")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's argz and envz vectors, for posix/src/argz.rs. Generated by\n"
            "# posix/tools/oracle/argz_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
