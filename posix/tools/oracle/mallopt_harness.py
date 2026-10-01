"""glibc 2.39's mallopt, its MALLOC_*_ environment variables and malloc_info
(<malloc.h>) as the oracle for posix/src/malloc.rs.

    python posix/tools/oracle/mallopt_harness.py   # writes posix/src/mallopt_oracle.txt

What depends on the allocator's insides -- which chunks a heap has, how big
its default thresholds are -- differs between glibc's and this library's by
design, so the probes record what the interface promises:

- `mallopt P V = R`: what mallopt answers, for every parameter glibc names and
  a few it does not, over values around each one's limits -- each in a fresh
  process, since some change the heap;
- `perturb V = ...`: the bytes M_PERTURB fills a block with -- malloc's,
  calloc's, the part realloc adds -- by the value mallopt was given;
- `env NAME=TEXT = ...`: what an environment variable does -- MALLOC_PERTURB_
  by the byte malloc fills with, MALLOC_MMAP_THRESHOLD_ and MALLOC_MMAP_MAX_
  by whether a block of 512 KiB and one of 2 MiB are mapped on their own
  (mallinfo2's hblks) -- by the text it is set to;
- `mmap ...`: the same for mallopt's M_MMAP_THRESHOLD and M_MMAP_MAX;
- `malloc_info`: its answer to options it does not know, and its XML's
  outline -- each element's name and attribute names, one to a line, the
  heap's <size> and <unsorted> lines (one per bin with free chunks, which
  the heap's state decides) left out.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "mallopt_oracle.txt"

PARAMS = [("M_MXFAST", 1), ("M_TRIM_THRESHOLD", -1), ("M_TOP_PAD", -2),
          ("M_MMAP_THRESHOLD", -3), ("M_MMAP_MAX", -4), ("M_CHECK_ACTION", -5),
          ("M_PERTURB", -6), ("M_ARENA_TEST", -7), ("M_ARENA_MAX", -8),
          ("0", 0), ("2", 2), ("99", 99), ("-9", -9), ("-99", -99)]
VALUES = [-2147483648, -1, 0, 1, 64, 160, 161, 255, 256, 4096, 1048576, 33554432,
          33554433, 67108864, 2147483647]
PERTURBS = [0x42, 0x142, 0x100, 0x1ff, 0xff, 1, -1, -256, 0x7fffffff]
ENV_TEXTS = ["66", "0x42", "0X42", "0102", "0", "255", "256", "300", "-1", "abc", "66abc",
             " 66", "66 ", "+66", "", "0x", "99999999999999999999"]
THRESHOLD_TEXTS = ["1048576", "0x100000", "4194304", "0", "abc", "-1", ""]
MAX_TEXTS = ["0", "1", "-1", "abc", ""]

PROGRAM = r'''
#include <errno.h>
#include <malloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Whether a block of `size` is mapped on its own: whether it adds to hblks. */
static int alone(size_t size)
{
    struct mallinfo2 before = mallinfo2();
    void *p = malloc(size);
    struct mallinfo2 after = mallinfo2();
    free(p);
    return after.hblks > before.hblks;
}

static void fills(const char *what)
{
    unsigned char *p = malloc(64), *c = calloc(1, 64);
    unsigned char *r = realloc(malloc(16), 4096);
    printf("%s = malloc %02x %02x calloc %02x realloc-added %02x\n", what, p[0], p[63], c[0],
           r[1000]);
    free(p);
    free(c);
    free(r);
}

int main(int argc, char **argv)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc == 4 && strcmp(argv[1], "mallopt") == 0) {
        int p = atoi(argv[2]), v = atoi(argv[3]);
        errno = 12345;
        int rc = mallopt(p, v);
        printf("= %d errno=%s\n", rc, errno == 12345 ? "kept" : "changed");
        return 0;
    }
    if (argc == 3 && strcmp(argv[1], "perturb") == 0) {
        mallopt(M_PERTURB, atoi(argv[2]));
        fills("");
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "fills") == 0) {
        fills("");
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "mapped") == 0) {
        printf("= 512K %d 2M %d\n", alone(512 << 10), alone(2 << 20));
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "mmap") == 0) {
        mallopt(M_MMAP_THRESHOLD, 1 << 20);
        printf("mmap M_MMAP_THRESHOLD 1048576 = 512K %d 2M %d\n", alone(512 << 10), alone(2 << 20));
        mallopt(M_MMAP_THRESHOLD, 4 << 20);
        printf("mmap M_MMAP_THRESHOLD 4194304 = 512K %d 2M %d\n", alone(512 << 10), alone(2 << 20));
        mallopt(M_MMAP_THRESHOLD, 1 << 20);
        mallopt(M_MMAP_MAX, 0);
        printf("mmap M_MMAP_MAX 0 = 512K %d 2M %d\n", alone(512 << 10), alone(2 << 20));
        mallopt(M_MMAP_MAX, 1);
        void *keep = malloc(2 << 20);
        printf("mmap M_MMAP_MAX 1 with one mapped = 2M %d\n", alone(2 << 20));
        free(keep);
        printf("mmap M_MMAP_MAX 1 after = 2M %d\n", alone(2 << 20));
        return 0;
    }
    /* malloc_info */
    errno = 12345;
    int rc = malloc_info(1, stdout);
    printf("malloc_info(1) = %d errno=%s\n", rc, errno == 12345 ? "kept" : "changed");
    errno = 12345;
    rc = malloc_info(-1, stdout);
    printf("malloc_info(-1) = %d errno=%s\n", rc, errno == 12345 ? "kept" : "changed");
    void *a = malloc(100), *b = malloc(4 << 20), *c = malloc(40);
    free(a);
    char *text = NULL;
    size_t len = 0;
    FILE *f = open_memstream(&text, &len);
    errno = 12345;
    rc = malloc_info(0, f);
    fclose(f);
    printf("malloc_info(0) = %d errno=%s\n", rc, errno == 12345 ? "kept" : "changed");
    /* The outline: each line's element and attribute names, <size> lines out. */
    for (char *line = strtok(text, "\n"); line; line = strtok(NULL, "\n")) {
        while (*line == ' ') line++;
        if (strncmp(line, "<size ", 6) == 0 || strncmp(line, "<unsorted ", 10) == 0) continue;
        printf("malloc_info outline ");
        for (char *p = line; *p; p++) {
            if (*p == '"') {             /* a value: shown as V */
                p = strchr(p + 1, '"');
                if (!p) break;
                printf("V");
                continue;
            }
            putchar(*p);
        }
        printf("\n");
    }
    free(text);
    free(b);
    free(c);
    return 0;
}
'''


def main() -> None:
    lines = []
    with workdir() as t:
        d = Path(t)
        (d / "mo.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        w = wsl_path(d)
        build = (f"set -e; cp {w}/mo.c /tmp/ && cd /tmp && "
                 "gcc -O0 -Wall -Werror -o moprobe mo.c")
        r = run(build)
        if r.returncode != 0:
            sys.exit(f"the build failed:\n{r.stderr}")
        script = []
        for name, p in PARAMS:
            for v in VALUES:
                script.append(f"printf 'mallopt {name} {v} '; /tmp/moprobe mallopt {p} {v}")
        for v in PERTURBS:
            script.append(f"printf 'perturb {v} '; /tmp/moprobe perturb {v}")
        for text in ENV_TEXTS:
            script.append(f"printf 'env MALLOC_PERTURB_=[%s] ' '{text}'; "
                          f"MALLOC_PERTURB_='{text}' /tmp/moprobe fills")
        for text in THRESHOLD_TEXTS:
            script.append(f"printf 'env MALLOC_MMAP_THRESHOLD_=[%s] ' '{text}'; "
                          f"MALLOC_MMAP_THRESHOLD_='{text}' /tmp/moprobe mapped")
        for text in MAX_TEXTS:
            script.append(f"printf 'env MALLOC_MMAP_THRESHOLD_=[1048576] MALLOC_MMAP_MAX_=[%s] ' "
                          f"'{text}'; MALLOC_MMAP_THRESHOLD_=1048576 MALLOC_MMAP_MAX_='{text}' "
                          f"/tmp/moprobe mapped")
        script.append("/tmp/moprobe mmap")
        script.append("/tmp/moprobe info")
        r = run("unset MALLOC_PERTURB_ MALLOC_MMAP_THRESHOLD_ MALLOC_MMAP_MAX_ GLIBC_TUNABLES; "
                + "; ".join(script))
        if r.returncode != 0 or "malloc_info outline" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        lines = r.stdout
    head = ("# glibc 2.39's mallopt, MALLOC_*_ and malloc_info, for posix/src/malloc.rs.\n"
            "# Generated by posix/tools/oracle/mallopt_harness.py; do not edit.\n")
    OUT.write_text(head + lines, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {lines.count(chr(10))} lines")


if __name__ == "__main__":
    main()
