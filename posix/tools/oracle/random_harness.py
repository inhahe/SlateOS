"""glibc 2.39's pseudo-random sequences -- rand, random, rand_r, the five
state sizes initstate chooses among, setstate's switches, the reentrant
random_r family and the rand48 family, its reentrant forms included -- as the
oracle for posix/src/stdlib.rs.

    python posix/tools/oracle/random_harness.py   # writes posix/src/random_oracle.txt

Lines, all numbers decimal but drand48's and erand48's, which are C's %a:

    rand-default = <the first 64 rand() results, srand never called>
    drand48-default = <8 drand48> / <8 lrand48> / <8 mrand48>, srand48 never called
    rand <seed> = <first 64 results after srand(seed)>
    random <seed> = <first 64 after srandom(seed)>
    rand_r <seed> = <first 64 of rand_r, the state threaded through> : <state after>
    initstate <size> <seed> = <first 64 of random() after initstate(seed, buf, size)>
    initstate-small = <is initstate(1, buf, 7) NULL> <random() after it>
    switch = <random() around setstate's switches between two states, and
              whether each setstate returned the state it left>
    random_r <size> <seed> = <initstate_r's result> : <first 64 of random_r after it>
    random_r-small = <initstate_r(1, buf, 7, &d)'s result> <errno is EINVAL>
    drand48_r <seed> = <16 drand48_r> / <16 lrand48_r> / <16 mrand48_r>, after srand48_r(seed)
    drand48_r-zeroed = <8 lrand48_r> on a struct only zeroed
    seed48 = <the three values seed48 returned> / <8 lrand48 after it>
    lcong48 = <8 lrand48> / <4 nrand48 on its own state> / <4 lrand48 after srand48(5)>
    lcong48_r = <8 lrand48_r> / <4 erand48_r> <4 nrand48_r> <4 jrand48_r> on its own state
    seed48_r = <old state seed48_r kept> / <8 lrand48_r after it>
    layout <struct> <size> <field>=<offset>,...

`rand` with no `srand` first is `rand 1`, as C specifies.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "random_oracle.txt"

SEEDS = [0, 1, 2, 42, 12345, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]
SIZES = [8, 16, 31, 32, 63, 64, 100, 128, 255, 256, 1000]

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const unsigned seeds[] = {SEEDS};
static const size_t sizes[] = {SIZES};
#define N(a) (sizeof(a) / sizeof(a)[0])

int main(void)
{
    /* First, before anything seeds either family's global state. */
    printf("rand-default =");
    for (int i = 0; i < 64; i++) printf(" %d", rand());
    printf("\n");
    printf("drand48-default =");
    for (int i = 0; i < 8; i++) printf(" %a", drand48());
    printf(" /");
    for (int i = 0; i < 8; i++) printf(" %ld", lrand48());
    printf(" /");
    for (int i = 0; i < 8; i++) printf(" %ld", mrand48());
    printf("\n");

    for (size_t s = 0; s < N(seeds); s++) {
        srand(seeds[s]);
        printf("rand %u =", seeds[s]);
        for (int i = 0; i < 64; i++) printf(" %d", rand());
        printf("\n");
        srandom(seeds[s]);
        printf("random %u =", seeds[s]);
        for (int i = 0; i < 64; i++) printf(" %ld", random());
        printf("\n");
        unsigned st = seeds[s];
        printf("rand_r %u =", seeds[s]);
        for (int i = 0; i < 64; i++) printf(" %d", rand_r(&st));
        printf(" : %u\n", st);
    }
    static char bufs[N(sizes)][1024];
    for (size_t z = 0; z < N(sizes); z++) {
        for (size_t s = 0; s < N(seeds); s++) {
            memset(bufs[z], 0, sizeof bufs[z]);
            char *old = initstate(seeds[s], bufs[z], sizes[z]);
            printf("initstate %zu %u =", sizes[z], seeds[s]);
            for (int i = 0; i < 64; i++) printf(" %ld", random());
            printf("\n");
            setstate(old);
        }
    }
    {
        static char small[8];
        srandom(3);
        char *r = initstate(1, small, 7);
        printf("initstate-small = %d %ld\n", r == NULL, random());
    }
    /* setstate: two states, interleaved; the previous state returned */
    static char a[128], b[256];
    char *orig = initstate(7, a, sizeof a);
    (void)initstate(9, b, sizeof b);
    printf("switch =");
    for (int i = 0; i < 4; i++) printf(" %ld", random());
    char *pb = setstate(a);
    printf(" | %d |", pb == b);
    for (int i = 0; i < 4; i++) printf(" %ld", random());
    char *pa = setstate(b);
    printf(" | %d |", pa == a);
    for (int i = 0; i < 4; i++) printf(" %ld", random());
    printf("\n");
    setstate(orig);
    /* the reentrant family */
    for (size_t z = 0; z < N(sizes); z++) {
        for (size_t s = 0; s < N(seeds); s++) {
            struct random_data d;
            memset(&d, 0, sizeof d);
            memset(bufs[z], 0, sizeof bufs[z]);
            int rc = initstate_r(seeds[s], bufs[z], sizes[z], &d);
            printf("random_r %zu %u = %d :", sizes[z], seeds[s], rc);
            for (int i = 0; i < 64 && rc == 0; i++) {
                int32_t r;
                random_r(&d, &r);
                printf(" %d", r);
            }
            printf("\n");
        }
    }
    {
        struct random_data d;
        static char small[8];
        memset(&d, 0, sizeof d);
        errno = 0;
        int rc = initstate_r(1, small, 7, &d);
        printf("random_r-small = %d %d\n", rc, errno == EINVAL);
    }
    for (size_t s = 0; s < N(seeds); s++) {
        struct drand48_data d;
        memset(&d, 0, sizeof d);
        srand48_r(seeds[s], &d);
        printf("drand48_r %u =", seeds[s]);
        for (int i = 0; i < 16; i++) { double x; drand48_r(&d, &x); printf(" %a", x); }
        printf(" /");
        for (int i = 0; i < 16; i++) { long x; lrand48_r(&d, &x); printf(" %ld", x); }
        printf(" /");
        for (int i = 0; i < 16; i++) { long x; mrand48_r(&d, &x); printf(" %ld", x); }
        printf("\n");
    }
    {
        struct drand48_data d;
        memset(&d, 0, sizeof d);
        printf("drand48_r-zeroed =");
        for (int i = 0; i < 8; i++) { long x; lrand48_r(&d, &x); printf(" %ld", x); }
        printf("\n");
    }
    {
        unsigned short seed[3] = {0x1234, 0x5678, 0x9abc};
        srand48(77);
        unsigned short *old = seed48(seed);
        printf("seed48 = %u %u %u /", old[0], old[1], old[2]);
        for (int i = 0; i < 8; i++) printf(" %ld", lrand48());
        printf("\n");
    }
    {
        unsigned short param[7] = {0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x0066, 0x0777};
        unsigned short xsubi[3] = {1, 2, 3};
        lcong48(param);
        printf("lcong48 =");
        for (int i = 0; i < 8; i++) printf(" %ld", lrand48());
        printf(" /");
        for (int i = 0; i < 4; i++) printf(" %ld", nrand48(xsubi));
        srand48(5);
        printf(" /");
        for (int i = 0; i < 4; i++) printf(" %ld", lrand48());
        printf("\n");
    }
    {
        unsigned short param[7] = {0xaaaa, 0xbbbb, 0xcccc, 0x0101, 0x0202, 0x0003, 0x0005};
        unsigned short xsubi[3] = {7, 8, 9};
        struct drand48_data d;
        memset(&d, 0, sizeof d);
        lcong48_r(param, &d);
        printf("lcong48_r =");
        for (int i = 0; i < 8; i++) { long x; lrand48_r(&d, &x); printf(" %ld", x); }
        printf(" /");
        for (int i = 0; i < 4; i++) { double x; erand48_r(xsubi, &d, &x); printf(" %a", x); }
        for (int i = 0; i < 4; i++) { long x; nrand48_r(xsubi, &d, &x); printf(" %ld", x); }
        for (int i = 0; i < 4; i++) { long x; jrand48_r(xsubi, &d, &x); printf(" %ld", x); }
        printf("\n");
    }
    {
        unsigned short seed[3] = {0xfedc, 0xba98, 0x7654};
        struct drand48_data d;
        memset(&d, 0, sizeof d);
        srand48_r(1000, &d);
        seed48_r(seed, &d);
        printf("seed48_r = %u %u %u /", d.__old_x[0], d.__old_x[1], d.__old_x[2]);
        for (int i = 0; i < 8; i++) { long x; lrand48_r(&d, &x); printf(" %ld", x); }
        printf("\n");
    }
    printf("layout struct_random_data %zu fptr=%zu,rptr=%zu,state=%zu,rand_type=%zu,rand_deg=%zu,"
           "rand_sep=%zu,end_ptr=%zu\n", sizeof(struct random_data),
           offsetof(struct random_data, fptr), offsetof(struct random_data, rptr),
           offsetof(struct random_data, state), offsetof(struct random_data, rand_type),
           offsetof(struct random_data, rand_deg), offsetof(struct random_data, rand_sep),
           offsetof(struct random_data, end_ptr));
    printf("layout struct_drand48_data %zu __x=%zu,__old_x=%zu,__c=%zu,__init=%zu,__a=%zu\n",
           sizeof(struct drand48_data), offsetof(struct drand48_data, __x),
           offsetof(struct drand48_data, __old_x), offsetof(struct drand48_data, __c),
           offsetof(struct drand48_data, __init), offsetof(struct drand48_data, __a));
    return 0;
}
'''


def main() -> None:
    src = PROGRAM.replace("{SEEDS}", "{" + ", ".join(f"{s:#x}u" for s in SEEDS) + "}").replace(
        "{SIZES}", "{" + ", ".join(str(z) for z in SIZES) + "}")
    with workdir() as t:
        d = Path(t)
        (d / "rnd.c").write_text(src, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -o rnd rnd.c && ./rnd")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}")
        body = r.stdout
    head = ("# glibc 2.39's pseudo-random sequences, for posix/src/stdlib.rs: rand, random,\n"
            "# rand_r, initstate's five state sizes, setstate's switches, random_r and the rand48\n"
            "# family, its reentrant forms included. Generated by\n"
            "# posix/tools/oracle/random_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
