"""glibc 2.39's <gshadow.h> -- how sgetsgent, fgetsgent and their _r forms
read a line of /etc/gshadow, and what putsgent writes -- as the oracle for
posix/src/gshadow.rs.

    python posix/tools/oracle/gshadow_harness.py   # writes posix/src/gshadow_oracle.txt

One line a probe, `<probe> = <what it gave>`. An entry is written
`name|passwd|adm,...|mem,...`, with `(null)` for a NULL string or list and
`[]` around each list; a failure is `NULL` (or the `_r` form's number) and
errno's name. getsgnam and getsgent read /etc/gshadow, which is root's to
read and which this machine's is not the library's to choose: the tests
drive those through fgetsgent's parser, which glibc's share.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "gshadow_oracle.txt"

# Lines for sgetsgent and fgetsgent: C string literals.
LINES = [
    r"staff:!:ann,bob:cat,dan",
    r"staff:!::",
    r"staff::ann:",
    r"staff:x:ann",
    r"staff:x",
    r"staff",
    r"",
    r":x:a:b",
    r"staff:x: ann , bob :cat,  dan  ",
    r"staff:x:ann,,bob,:,cat,,",
    r"staff:x:ann bob:cat dan",
    r"staff:x:ann:cat:extra",
    r"staff:x:ann:cat\n",
    r"staff:x:\tann:\tcat",
    r"+staff",
    r"+staff:",
    r"-staff",
    r"+:x:a:b",
    r"+",
    r"#staff:x:a:b",
    r" staff:x:a:b",
    r"staff :x:a:b",
    r"st\001ff:x:a:b",
    r"staff:x:a,b:c,d:",
    r"staff:x:a:b:c:d",
    r"staff:pass word:a:b",
    r"a:b:c:d\ne:f:g:h",
]

# A file for fgetsgent: blank lines, comments, a bad line, entries.
FILE = r"\n# comment\nwheel:!:root:root,ann\n\n   \nbad\nstaff:x::bob\n  indented:x:a:b\n#last:x::\nlast:*::\n"

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <gshadow.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *en(int e)
{
    switch (e) {
    case 0: return "0";
    case 12345: return "kept";
    case EINVAL: return "EINVAL";
    case ERANGE: return "ERANGE";
    case ENOENT: return "ENOENT";
    case EBADF: return "EBADF";
    default: { static char b[16]; snprintf(b, sizeof b, "e%d", e); return b; }
    }
}

static void str(const char *s)
{
    if (!s) { printf("(null)"); return; }
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c == '\\') printf("\\\\");
        else if (c == '\n') printf("\\n");
        else if (c == '\t') printf("\\t");
        else if (c < 0x20 || c >= 0x7f) printf("\\x%02x", c);
        else putchar(c);
    }
}

static void list(char **l)
{
    if (!l) { printf("(null)"); return; }
    printf("[");
    for (int i = 0; l[i]; i++) { if (i) printf(","); str(l[i]); }
    printf("]");
}

static void entry(const struct sgrp *g)
{
    str(g->sg_namp); printf("|"); str(g->sg_passwd); printf("|");
    list(g->sg_adm); printf("|"); list(g->sg_mem);
}

static const char *LINES[] = {
@LINES@
};

int main(void)
{
    char buf[4096];
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    for (size_t i = 0; i < sizeof LINES / sizeof *LINES; i++) {
        printf("sgetsgent(\""); str(LINES[i]); printf("\") = ");
        errno = 12345;
        struct sgrp *g = sgetsgent(LINES[i]);
        if (g) entry(g); else printf("NULL");
        printf(" errno=%s\n", en(errno));
        /* the _r form at buffer sizes around the entry's need */
        for (size_t size = 0; size <= 64; size += 8) {
            struct sgrp r, *res = (struct sgrp *)1;
            errno = 12345;
            int rc = sgetsgent_r(LINES[i], &r, buf, size, &res);
            printf("sgetsgent_r(\""); str(LINES[i]); printf("\", %zu) = %s", size, en(rc));
            if (rc == 0 && res == &r) { printf(" "); entry(&r); }
            else printf(" %s", res == NULL ? "NULL" : res == (struct sgrp *)1 ? "untouched" : "other");
            printf(" errno=%s\n", en(errno));
        }
    }

    /* fgetsgent over a file */
    FILE *f = fmemopen((void *)FILE_TEXT, strlen(FILE_TEXT), "r");
    for (int n = 0; n < 8; n++) {
        errno = 12345;
        struct sgrp *g = fgetsgent(f);
        printf("fgetsgent #%d = ", n);
        if (g) entry(g); else printf("NULL");
        printf(" errno=%s\n", en(errno));
        if (!g) break;
    }
    fclose(f);
    f = fmemopen((void *)FILE_TEXT, strlen(FILE_TEXT), "r");
    for (int n = 0; n < 8; n++) {
        struct sgrp r, *res = (struct sgrp *)1;
        errno = 12345;
        int rc = fgetsgent_r(f, &r, buf, sizeof buf, &res);
        printf("fgetsgent_r #%d = %s", n, en(rc));
        if (rc == 0 && res == &r) { printf(" "); entry(&r); }
        else printf(" %s", res == NULL ? "NULL" : "other");
        printf(" errno=%s\n", en(errno));
        if (rc != 0) break;
    }
    fclose(f);

    /* putsgent */
    char *adm[] = { "root", "ann", NULL }, *mem[] = { "bob", NULL }, *none[] = { NULL };
    char *comma[] = { "a,b", NULL }, *colon[] = { "a:b", NULL }, *nl[] = { "a\nb", NULL };
    char *empty[] = { "", NULL };
    struct { const char *what; struct sgrp g; } PUTS[] = {
        { "full", { "wheel", "!", adm, mem } },
        { "null passwd", { "wheel", NULL, adm, mem } },
        { "null lists", { "wheel", "x", NULL, NULL } },
        { "empty lists", { "wheel", "x", none, none } },
        { "empty member", { "wheel", "x", empty, empty } },
        { "null name", { NULL, "x", adm, mem } },
        { "empty name", { "", "x", adm, mem } },
        { "colon in name", { "wh:eel", "x", adm, mem } },
        { "newline in passwd", { "wheel", "x\ny", adm, mem } },
        { "colon in passwd", { "wheel", "x:y", adm, mem } },
        { "comma in admin", { "wheel", "x", comma, mem } },
        { "colon in member", { "wheel", "x", adm, colon } },
        { "newline in member", { "wheel", "x", adm, nl } },
        { "comma in passwd", { "wheel", "x,y", adm, mem } },
        { "plus name", { "+wheel", "x", adm, mem } },
    };
    for (size_t i = 0; i < sizeof PUTS / sizeof *PUTS; i++) {
        char out[256] = "";
        FILE *o = fmemopen(out, sizeof out, "w");
        errno = 12345;
        int rc = putsgent(&PUTS[i].g, o);
        int e = errno;
        fclose(o);
        printf("putsgent %s = %d errno=%s \"", PUTS[i].what, rc, en(e));
        str(out);
        printf("\"\n");
    }
    return 0;
}
'''


def main() -> None:
    body = ",\n".join(f'    "{line}"' for line in LINES)
    program = PROGRAM.replace("@LINES@", body).replace(
        "int main(void)", f'static const char FILE_TEXT[] = "{FILE}";\n\nint main(void)')
    with workdir() as t:
        d = Path(t)
        (d / "gs.c").write_text(program, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -Wall -Werror -o gs gs.c && ./gs")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body_out = r.stdout
    head = ("# glibc 2.39's <gshadow.h>, for posix/src/gshadow.rs. Generated by\n"
            "# posix/tools/oracle/gshadow_harness.py; do not edit.\n")
    OUT.write_text(head + body_out, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body_out.count(chr(10))} lines")


if __name__ == "__main__":
    main()
