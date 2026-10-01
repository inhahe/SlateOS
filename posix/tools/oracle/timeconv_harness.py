"""glibc 2.39's time conversions at every range, as the oracle for
posix/src/time.rs.

    python posix/tools/oracle/timeconv_harness.py   # writes posix/src/timeconv_oracle.txt

`gmtime_r`, `localtime_r`, `mktime` and `timegm` over the whole of `time_t`
and `int`: the ordinary instants, the calendar's corners, and the values
whose year does not fit in `tm_year`, where glibc answers `EOVERFLOW` -- and,
for `mktime`/`timegm`, fields out of range in every direction, which they
must normalise. `asctime_r` and `ctime_r` over the years whose text does not
fit. Two zones: `UTC0`, and `EST5EDT,M3.2.0,M11.1.0` for the local-time
rules (a POSIX TZ string, so no zone file is involved).

    g <tz> <secs> = <ret> <errno> <tm>
    l <tz> <secs> = <ret> <errno> <tm>
    m <tz> <tm-in> = <result> <errno> <tm-out>
    t <tz> <tm-in> = <result> <errno> <tm-out>
    a <sec> <min> <hour> <mday> <mon> <year> <wday> = <ret> <errno> <text-hex>
    c <tz> <secs> = <ret> <errno> <text-hex>

`<tm>` is `sec min hour mday mon year wday yday isdst gmtoff zone`, the zone
its name or `-` for NULL; `<ret>` is 1 for a pointer, 0 for NULL. Every call
starts from the same recognisable `struct tm`, so a line shows what a failing
call leaves behind as well as what a successful one writes.
"""

import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "timeconv_oracle.txt"
ZONES = ["UTC0", "EST5EDT,M3.2.0,M11.1.0"]
I64_MAX = 2 ** 63 - 1
I32_MAX = 2 ** 31 - 1


def secs_values(rng):
    v = [0, 1, -1, 59, 60, 3599, 3600, 86399, 86400, -86399, -86400, -86401,
         951782400, 951868799, 951868800, 1727519700, 1741503600, 1741507199, 1741507200,
         1762063199, 1762063200, 1762066800, -2208988800, -2208988801, -62135596800,
         -62135596801, -62167219200, -62167219201, 253402300799, 253402300800,
         2 ** 31 - 1, 2 ** 31, -2 ** 31, -2 ** 31 - 1, 2 ** 32, 2 ** 40, -2 ** 40,
         # tm_year's ends: INT_MAX is year 2147485547, INT_MIN year -2147481748.
         67767976233532799, 67767976233532800, 67767976233529200, 67767976233516000,
         -67768040609740800, -67768040609740801, -67768040609722801,
         I64_MAX, -I64_MAX - 1, -I64_MAX, I64_MAX - 86400, 2 ** 62, -2 ** 62, 2 ** 56, -2 ** 56,
         10 ** 15, -10 ** 15, 10 ** 17, -10 ** 17, 10 ** 18, -10 ** 18]
    for _ in range(30):
        v.append(rng.randint(-2 ** 40, 2 ** 40))
    for _ in range(10):
        v.append(rng.randint(-2 ** 63, 2 ** 63 - 1))
    return v


def tm_values(rng):
    """`(sec, min, hour, mday, mon, year, isdst)` for mktime and timegm."""
    v = [
        (0, 0, 0, 1, 0, 70, 0), (59, 59, 23, 31, 11, 99, 0), (0, 0, 0, 29, 1, 100, 0),
        (0, 0, 0, 29, 1, 101, 0), (60, 59, 23, 31, 11, 116, 0), (0, 0, 0, 0, 0, 70, 0),
        (0, 0, 0, 1, 12, 70, 0), (0, 0, 0, 1, -1, 70, 0), (0, 0, 0, 1, -13, 70, 0),
        (-1, 0, 0, 1, 0, 70, 0), (0, -1, 0, 1, 0, 70, 0), (0, 0, -1, 1, 0, 70, 0),
        (0, 0, 0, -1, 0, 70, 0), (0, 0, 0, 366, 0, 70, 0), (0, 0, 0, 1000000, 0, 70, 0),
        (3600 * 24 * 400, 0, 0, 1, 0, 70, 0), (0, 0, 0, 1, 0, -1900, 0), (0, 0, 0, 1, 0, -1901, 0),
        (0, 0, 0, 1, 0, 8099, 0), (0, 0, 0, 1, 0, 8100, 0),
        (0, 0, 0, 1, 0, I32_MAX, 0), (0, 0, 0, 31, 11, I32_MAX, 0), (59, 59, 23, 31, 11, I32_MAX, 0),
        (0, 0, 0, 1, 0, -I32_MAX - 1, 0), (0, 0, 0, 1, 12, I32_MAX, 0), (0, 0, 0, 1, -1, -I32_MAX - 1, 0),
        (I32_MAX, I32_MAX, I32_MAX, I32_MAX, I32_MAX, I32_MAX, 0),
        (-I32_MAX - 1, -I32_MAX - 1, -I32_MAX - 1, -I32_MAX - 1, -I32_MAX - 1, -I32_MAX - 1, 0),
        (I32_MAX, 0, 0, 1, 0, 70, 0), (-I32_MAX - 1, 0, 0, 1, 0, 70, 0),
        (0, 0, 0, I32_MAX, 0, 70, 0), (0, 0, 0, -I32_MAX - 1, 0, 70, 0),
        (0, 0, 0, 1, I32_MAX, 70, 0), (0, 0, 0, 1, -I32_MAX - 1, 70, 0),
        (0, 0, 0, 1, 0, 2147481747, 0), (0, 0, 0, 1, 0, 2147481748, 0),
        # the local-time rules: the spring-forward gap and the fall-back overlap
        (0, 30, 2, 9, 2, 125, -1), (0, 30, 2, 9, 2, 125, 0), (0, 30, 2, 9, 2, 125, 1),
        (0, 30, 1, 2, 10, 125, -1), (0, 30, 1, 2, 10, 125, 0), (0, 30, 1, 2, 10, 125, 1),
        (0, 0, 12, 1, 6, 125, 0), (0, 0, 12, 1, 0, 125, 1),
    ]
    for _ in range(30):
        v.append((rng.randint(-100, 200), rng.randint(-100, 200), rng.randint(-50, 80),
                  rng.randint(-400, 800), rng.randint(-30, 40), rng.randint(-300, 400),
                  rng.choice([-1, 0, 1])))
    for _ in range(10):
        v.append(tuple(rng.randint(-I32_MAX - 1, I32_MAX) for _ in range(6)) + (0,))
    return v


def asctime_cases():
    """`(sec, min, hour, mday, mon, year, wday)` for asctime_r."""
    base = (47, 37, 17, 27, 6, 77, 3)
    out = [base[:5] + (y,) + base[6:] for y in (
        70, 8099, 8100, -1900, -1901, -2899, -2900, I32_MAX - 1900, I32_MAX - 1899, I32_MAX,
        -I32_MAX - 1, 999999 - 1900, 1000000 - 1900, 99999999, -99999999)]
    for i, vals in enumerate([(0, 59, 60, 99, 100, -1, -10, 1000), (0, 59, 60, 99, 100, -1),
                              (0, 23, 24, 99, 100, -1), (0, 1, 31, 32, 99, 100, 999, -1, -99, -100),
                              (-1, 0, 11, 12, 13), (), (-1, 0, 6, 7, 8)]):
        for x in vals:
            t = list(base)
            t[i] = x
            out.append(tuple(t))
    return out


def gen_c(rng) -> str:
    sv = secs_values(rng)
    tv = tm_values(rng)
    ac = asctime_cases()
    c = [
        "#define _GNU_SOURCE",
        "#include <errno.h>", "#include <stdio.h>", "#include <stdlib.h>", "#include <string.h>",
        "#include <time.h>",
        "static const char *const zones[] = {" + ", ".join(f'"{z}"' for z in ZONES) + "};",
        "static const long long sv[] = {" + ", ".join(
            (f"{x}LL" if x != -2 ** 63 else "(-9223372036854775807LL - 1)") for x in sv) + "};",
        "static const int tv[][7] = {" + ", ".join(
            "{" + ", ".join((str(x) if x != -2 ** 31 else "(-2147483647 - 1)") for x in t) + "}"
            for t in tv) + "};",
        "static const int ac[][7] = {" + ", ".join(
            "{" + ", ".join((str(x) if x != -2 ** 31 else "(-2147483647 - 1)") for x in t) + "}"
            for t in ac) + "};",
        "static void init(struct tm *t) {",
        "  memset(t, 0, sizeof *t);",
        "  t->tm_sec = 47; t->tm_min = 37; t->tm_hour = 17; t->tm_mday = 27; t->tm_mon = 6;",
        "  t->tm_year = 77; t->tm_wday = 3; t->tm_yday = 222; t->tm_isdst = -7;",
        "  t->tm_gmtoff = -12345; t->tm_zone = \"sentinel\";",
        "}",
        "static void put_tm(const struct tm *t) {",
        "  printf(\"%d %d %d %d %d %d %d %d %d %ld %s\", t->tm_sec, t->tm_min, t->tm_hour,",
        "         t->tm_mday, t->tm_mon, t->tm_year, t->tm_wday, t->tm_yday, t->tm_isdst,",
        "         (long)t->tm_gmtoff, t->tm_zone ? t->tm_zone : \"-\");",
        "}",
        "static void hexs(const char *s) { if (!s || !*s) { printf(\"-\"); return; }",
        "  for (; *s; s++) printf(\"%02x\", (unsigned char)*s); }",
        "int main(void) {",
        "  for (unsigned z = 0; z < sizeof zones / sizeof zones[0]; z++) {",
        "    setenv(\"TZ\", zones[z], 1); tzset();",
        "    for (unsigned i = 0; i < sizeof sv / sizeof sv[0]; i++) {",
        "      time_t s = (time_t)sv[i]; struct tm t; struct tm *r; int e;",
        "      init(&t); errno = 0; r = gmtime_r(&s, &t); e = errno;",
        "      printf(\"g %u %lld = %d %d \", z, sv[i], r != NULL, e); put_tm(&t); printf(\"\\n\");",
        "      init(&t); errno = 0; r = localtime_r(&s, &t); e = errno;",
        "      printf(\"l %u %lld = %d %d \", z, sv[i], r != NULL, e); put_tm(&t); printf(\"\\n\");",
        "      { char buf[64]; char *cr; memset(buf, 0, sizeof buf); errno = 0;",
        "        cr = ctime_r(&s, buf); e = errno;",
        "        printf(\"c %u %lld = %d %d \", z, sv[i], cr != NULL, e); hexs(cr); printf(\"\\n\");",
        "        errno = 0; cr = ctime(&s); e = errno;",
        "        printf(\"C %u %lld = %d %d \", z, sv[i], cr != NULL, e); hexs(cr); printf(\"\\n\"); }",
        "    }",
        "    for (unsigned i = 0; i < sizeof tv / sizeof tv[0]; i++) {",
        "      for (int which = 0; which < 2; which++) {",
        "        struct tm t; time_t r; int e;",
        "        init(&t); t.tm_sec = tv[i][0]; t.tm_min = tv[i][1]; t.tm_hour = tv[i][2];",
        "        t.tm_mday = tv[i][3]; t.tm_mon = tv[i][4]; t.tm_year = tv[i][5]; t.tm_isdst = tv[i][6];",
        "        if (!which) {",
        "          char sbuf[64]; size_t n; errno = 0;",
        "          n = strftime(sbuf, sizeof sbuf, \"%s\", &t); e = errno;",
        "          printf(\"f %u %d %d %d %d %d %d %d = %zu %d \", z, tv[i][0], tv[i][1], tv[i][2],",
        "                 tv[i][3], tv[i][4], tv[i][5], tv[i][6], n, e);",
        "          hexs(n ? sbuf : \"\"); printf(\"\\n\");",
        "        }",
        "        errno = 0; r = which ? timegm(&t) : mktime(&t); e = errno;",
        "        printf(\"%c %u %d %d %d %d %d %d %d = %lld %d \", which ? 't' : 'm', z, tv[i][0],",
        "               tv[i][1], tv[i][2], tv[i][3], tv[i][4], tv[i][5], tv[i][6], (long long)r, e);",
        "        put_tm(&t); printf(\"\\n\");",
        "      }",
        "    }",
        "  }",
        "  for (unsigned i = 0; i < sizeof ac / sizeof ac[0]; i++) {",
        "    struct tm t; char buf[64]; char *r; int e;",
        "    init(&t); t.tm_sec = ac[i][0]; t.tm_min = ac[i][1]; t.tm_hour = ac[i][2];",
        "    t.tm_mday = ac[i][3]; t.tm_mon = ac[i][4]; t.tm_year = ac[i][5]; t.tm_wday = ac[i][6];",
        "    memset(buf, 0, sizeof buf);",
        "    errno = 0; r = asctime_r(&t, buf); e = errno;",
        "    printf(\"a %d %d %d %d %d %d %d = %d %d \", ac[i][0], ac[i][1], ac[i][2], ac[i][3],",
        "           ac[i][4], ac[i][5], ac[i][6], r != NULL, e); hexs(r); printf(\"\\n\");",
        "    errno = 0; r = asctime(&t); e = errno;",
        "    printf(\"A %d %d %d %d %d %d %d = %d %d \", ac[i][0], ac[i][1], ac[i][2], ac[i][3],",
        "           ac[i][4], ac[i][5], ac[i][6], r != NULL, e); hexs(r); printf(\"\\n\");",
        "  }",
        "  { char *r; int e; errno = 0; r = asctime(NULL); e = errno;",
        "    printf(\"A null = %d %d \", r != NULL, e); hexs(r); printf(\"\\n\"); }",
        "  return 0;",
        "}",
    ]
    return "\n".join(c) + "\n"


def main():
    rng = random.Random(20260928)
    with workdir() as tmp:
        (Path(tmp) / "timeconv_oracle.c").write_text(gen_c(rng), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -w -o timeconv_oracle timeconv_oracle.c "
                f"&& ./timeconv_oracle > '{wsl_path(OUT)}'")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines, {OUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
