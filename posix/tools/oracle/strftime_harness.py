"""glibc 2.39's `strftime` and `wcsftime`, as the oracle for posix/src/time.rs's.

    python posix/tools/oracle/strftime_harness.py   # writes posix/src/strftime_oracle.txt

Every conversion, known and unknown, under the flags (`_ - 0 ^ #`), field
widths and `E`/`O` modifiers glibc reads, alone and in combination; the
numbers past their ranges and below zero; the ISO 8601 week-based year
across the turn of 22 years, leap years and negative years among them;
centuries and years from INT_MIN to INT_MAX; `%z` from every kind of
`tm_gmtoff`, and `%Z` from `tm_zone`, from `tzname` when it is empty, and
from nothing; `%s` through `mktime`, in two zones; a format cut short, a
width too large for an int; and buffers too small, from none at all to one
more than enough -- what is written before the call gives up shows, since
the buffer is filled beforehand and what changed is recorded.

The same cases, those whose format is ASCII, again through `wcsftime`, and
the wide-only ones: characters that are not ASCII, and are not characters,
in the format; the case flags on them; `%Z`'s zone converted as `mbsrtowcs`
converts it.

In C.UTF-8, the locale this library's one locale is. The narrow answers are
checked to be the same in the C locale, and are.

One line a call:

    N <tz> <sec> <min> <hour> <mday> <mon> <year> <wday> <yday> <isdst> <gmtoff> <zone> <maxsize> <s> <format> | <ret> <errno> <written>
    W ... the same, for wcsftime

`<tz>` is `TZ`, set and `tzset` before the call. `<zone>` is `tm_zone`:
`-` for NULL, `~` for "", else its bytes in hex. `<s>` is 1 when the call
is given a NULL buffer, which glibc's counts into without writing. Narrow
`<format>` and `<written>` are bytes in hex; wide ones are the `wchar_t`
units as 8-digit hex, joined with `.`; `~` is an empty format, `-` nothing
written. `<written>` is the buffer up to the last unit the call changed:
it was filled with 0xaa bytes before the call, and has 16 units more than
`<maxsize>` says, so that a write past the end would show.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "strftime_oracle.txt"

UTC = "UTC0"
NY = "EST5EDT,M3.2.0,M11.1.0"
IN = "<+0530>-5:30"

INT_MAX = 2**31 - 1
INT_MIN = -(2**31)


def days_from_civil(y: int, m: int, d: int) -> int:
    """Days since 1970-01-01 of the proleptic Gregorian y-m-d, for any y."""
    y -= m <= 2
    era = y // 400
    yoe = y - era * 400
    doy = (153 * (m + (-3 if m > 2 else 9)) + 2) // 5 + d - 1
    doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    return era * 146097 + doe - 719468


def tm(y, mo, d, H=0, M=0, S=0, isdst=0, off=0, zone=b"UTC"):
    """A consistent `struct tm` for y-mo-d H:M:S: its weekday and year day
    computed, not given."""
    days = days_from_civil(y, mo, d)
    wday = (days + 4) % 7
    yday = days - days_from_civil(y, 1, 1)
    return (S, M, H, d, mo - 1, y - 1900, wday, yday, isdst, off, zone)


def with_fields(t, **kw):
    names = ["sec", "min", "hour", "mday", "mon", "year", "wday", "yday", "isdst", "off", "zone"]
    t = list(t)
    for k, v in kw.items():
        t[names.index(k)] = v
    return tuple(t)


TM_A = tm(2024, 1, 5, 9, 7, 3)
TM_B = tm(2026, 10, 5, 15, 42, 30, isdst=1, off=-14400, zone=b"EDT")
# Every field out of its range, and no zone.
TM_C = (-3, 99, 0, -5, 12, -2000, 7, -1, 2, 19800, None)

# (kind, tz, tm, maxsize, null_buffer, format): format is bytes for "N",
# a list of wchar_t values for "W".
CASES: list = []


def narrow(fmt: bytes, t=TM_A, tz=UTC, maxsize=256, null=False, wide=True) -> None:
    """A narrow case, and the same through wcsftime when the format is ASCII."""
    CASES.append(("N", tz, t, maxsize, null, fmt))
    if wide and all(b < 0x80 for b in fmt):
        CASES.append(("W", tz, t, maxsize, null, list(fmt)))


def wide(units: list, t=TM_A, tz=UTC, maxsize=256, null=False) -> None:
    CASES.append(("W", tz, t, maxsize, null, units))


def s(text: str) -> bytes:
    return text.encode("utf-8")


# -- every conversion under every flag, width and modifier ------------------

KNOWN = "aAbBcCdDeFgGhHIjklmMnpPrRsStTuUVwWxXyYzZ%"
V_FULL = ["", "E", "O", "_", "-", "0", "^", "#", "^#", "1", "3", "10", "_3", "-3",
          "03", "_10", "-10", "010", "^10", "#10", "10E", "3O", "_3O", "05E", "-O",
          "0O", "_E"]
V_SOME = ["", "E", "O", "_", "-", "0", "^", "#", "10", "_3", "03", "-10"]
V_UNKNOWN = ["", "^", "5", "E", "^5O"]

for code in range(0x20, 0x7F):
    conv = chr(code)
    if conv in KNOWN:
        for v in V_FULL:
            narrow(s(f"%{v}{conv}"), TM_A)
        for v in V_SOME:
            narrow(s(f"%{v}{conv}"), TM_B)
            narrow(s(f"%{v}{conv}"), TM_C)
    else:
        for v in V_UNKNOWN:
            narrow(s(f"%{v}{conv}"), TM_A)

# -- the ISO 8601 week-based year, and the week numbers, at the turn of years

WEEKS = s("%G %g %V %U %W %u %w %j %C %y %Y %a")
for y in [2004, 2005, 2008, 2009, 2010, 2015, 2016, 2020, 2021, 2026, 2027, 1900,
          2000, 2100, 1, 0, -1, -4, -100, -400, 9999, 10000]:
    for d in range(1, 8):
        narrow(WEEKS, tm(y, 1, d, 12))
    for d in range(25, 32):
        narrow(WEEKS, tm(y, 12, d, 12))

# -- centuries and years ------------------------------------------------------

YEAR_FORMATS = ["%C", "%y", "%Y", "%G", "%g", "%_C", "%-C", "%0C", "%4C", "%_4C",
                "%-4C", "%04C", "%6Y", "%_6Y", "%-6Y", "%06Y", "%EY", "%EC", "%Ey",
                "%OY", "%OC", "%Oy", "%3y", "%_3y", "%-y", "%_y", "%1Y", "%10G",
                "%_10G", "%-g"]
for y in [-12345, -1001, -1000, -999, -101, -100, -99, -10, -1, 0, 1, 9, 10, 99,
          100, 101, 999, 1000, 1999, 2000, 9999, 10000, 12345]:
    for f in YEAR_FORMATS:
        narrow(s(f), tm(y, 6, 15, 12))
# tm_year at an int's ends: the year glibc computes in int arithmetic is still
# an int at both.
for tm_year in [INT_MIN, INT_MAX - 1900]:
    t = tm(tm_year + 1900, 6, 15, 12)
    for f in YEAR_FORMATS:
        narrow(s(f), t)

# -- %z -----------------------------------------------------------------------

Z_FORMATS = ["%z", "%_z", "%-z", "%0z", "%^z", "%6z", "%_6z", "%-6z", "%06z", "%Ez",
             "%Oz", "%2z", "%1z", "%5z", "%#z"]
for off in [0, 1, -1, 59, -59, 60, -60, 3600, -3600, 19800, -12600, 50400, -43200,
            86399, -86399, 100000, -100000, INT_MAX, -INT_MAX, 2**32 + 3600,
            -(2**32) + 60, 2**40 - 7200]:
    for f in Z_FORMATS:
        narrow(s(f), with_fields(TM_A, off=off))
for isdst in [-1, 1, 2, -100]:
    for off in [0, 3600, -18000]:
        for f in ["%z", "%6z", "%-z", "[%z]"]:
            narrow(s(f), with_fields(TM_A, off=off, isdst=isdst))

# -- %Z -----------------------------------------------------------------------

ZONES = [None, b"", b"UTC", b"EST", b"LongZoneName", b"+0530", b"\xff", b"\xc3\xa9T"]
ZN_FORMATS = ["%Z", "%#Z", "%^Z", "%#^Z", "%10Z", "%-10Z", "%_10Z", "%010Z", "%EZ",
              "%OZ", "%1Z", "%^10Z", "%#010Z"]
for zone in ZONES:
    for isdst in [-1, 0, 1, 2]:
        for f in ZN_FORMATS:
            narrow(s(f), with_fields(TM_A, zone=zone, isdst=isdst), tz=UTC)
        for f in ["%Z", "%10Z", "%-10Z", "%#Z"]:
            narrow(s(f), with_fields(TM_A, zone=zone, isdst=isdst), tz=NY)
            narrow(s(f), with_fields(TM_A, zone=zone, isdst=isdst), tz=IN)

# -- %s, through mktime ------------------------------------------------------

S_FORMATS = ["%s", "%_20s", "%-20s", "%020s", "%20s", "%Es", "%Os", "%1s", "%^s"]
S_TMS = [
    tm(1970, 1, 1),
    tm(1970, 1, 1, 0, 0, 1),
    tm(1969, 12, 31, 23, 59, 59),
    TM_A,
    tm(2038, 1, 19, 3, 14, 8),
    tm(1900, 1, 1),
    tm(1, 1, 1),
    tm(-1000, 3, 1),
    tm(2026, 7, 1, 12, isdst=0),
    tm(2026, 7, 1, 12, isdst=1),
    tm(2026, 7, 1, 12, isdst=-1),
    tm(2026, 1, 15, 12, isdst=1),
    tm(2026, 3, 8, 2, 30, isdst=-1),
    tm(2026, 11, 1, 1, 30, isdst=-1),
    with_fields(TM_A, mday=100, mon=-13),
    with_fields(TM_A, sec=1_000_000_000, min=-1_000_000_000),
    TM_C,
    tm(300000000, 6, 1),
    tm(INT_MAX, 6, 15),
]
for t in S_TMS:
    for tz in [UTC, NY]:
        for f in S_FORMATS:
            narrow(s(f), t, tz=tz)

# -- hours past their range, for the 12-hour clock ----------------------------

for hour in [-25, -13, -12, -1, 0, 1, 11, 12, 13, 23, 24, 25, 36, 100]:
    narrow(s("%I %l %p %P %r %H %k %^P %#p %#P"), with_fields(TM_A, hour=hour))

# -- names past their range ---------------------------------------------------

NAMES = s("%a|%A|%b|%B|%h|%^a|%#A|%10a|%010b|%Ob|%OB|%-5B|%c|%^c|%#c|%_30c")
for wday in [-1, 0, 6, 7, 100, INT_MIN, INT_MAX]:
    narrow(NAMES, with_fields(TM_A, wday=wday))
for mon in [-1, 0, 11, 12, INT_MIN, INT_MAX]:
    narrow(NAMES, with_fields(TM_A, mon=mon))

# -- formats cut short, odd and long ------------------------------------------

for f in [b"", b"abc", b"%", b"abc%", b"%E", b"%O", b"%5", b"%-", b"%_", b"%^", b"%#",
          b"%10", b"%E%", b"%O%", b"%E%Y", b"%%Y", b"%5%", b"%-5%", b"%05%", b"%^%",
          b"100%%", b"%%%", b"%%%%", b"%Y%m%d%H%M%S", b"%a, %d %b %Y %H:%M:%S %z",
          b"%E5d", b"%5Ed", b"%_5q", b"%^5q", b"%#q", b"%-0_^#5a", b"%0_d", b"%_0d",
          b"%-_d", b"%_-d", b"%0002d", b"%00d", b"%^#Ea", b"%#Eb", b"%#EB", b"%^EP",
          b"%#Ep", b"%EEd", b"%EOd", b"%OEd", b"%E^d", b"%5E", b"%^5", b"%^5E",
          b"%99999999999d", b"%2147483647d", b"%2147483646d", b"%4294967296d",
          b"%2147483648Y", b"%99999999999q", b"%999d", b"%-999d", b"%_999d",
          b"%0999d", b"%999a", b"%999c", b"%999z", b"%999Z", b"%999%", b"%999n"]:
    narrow(f, maxsize=1100)
# Bytes that are not ASCII, as text and as conversions.
for f in [s("é%Y€"), b"\x80\xff%Y\xfe", s("%é"), s("%^é"), s("%5é"), s("%Eé"),
          b"%\xff", b"%^\xff", s("%#Z é")]:
    narrow(f)

# -- buffers too small, and none --------------------------------------------

SMALL = [b"%Y-%m-%d", b"%c", b"%10c", b"%^c", b"%010D", b"%_5d", b"%05d", b"%-5d",
         b"abc", b"%A", b"%10A", b"%^A", b"%Z", b"%10Z", b"%z", b"%6z", b"%s",
         b"%_10s", b"%010s", b"%%", b"%5%", b"%q", b"%10q", b"%_5C", b"%05C",
         b"x%e", b"%_5e", b"%-3k", b"%n%t", b"%3n", b"%Ec"]
for f in SMALL:
    # Every size up to past the longest answer (%10c's 24), on one tm; some,
    # on one whose numbers are negative.
    for maxsize in range(0, 34):
        narrow(f, maxsize=maxsize)
    for maxsize in [0, 1, 2, 3, 4, 5, 6, 8, 12, 16, 24, 32]:
        narrow(f, with_fields(TM_A, mday=-5, year=-2000), maxsize=maxsize)
for f in [b"%Y", b"%c", b"", b"abc", b"%10c", b"%Z", b"%10Z"]:
    for maxsize in [0, 1, 2, 4, 5, 100]:
        narrow(f, maxsize=maxsize, null=True)
# A zone that is no UTF-8 where there is no room at all: the wide copy's
# mbsrtowcs is told it may write nothing, and so never reads the zone.
for f in [b"%Z", b"x%Z", b"%3Z"]:
    for maxsize in [0, 1, 2, 3]:
        narrow(f, with_fields(TM_A, zone=b"\xff"), maxsize=maxsize)
        narrow(f, with_fields(TM_A, zone=b"\xff"), maxsize=maxsize, null=True)

# -- wide only ----------------------------------------------------------------

PCT, Y = ord("%"), ord("Y")
for units in [
    [0xE9, PCT, Y, 0x20AC, 0x1F600],
    [0xD800, PCT, Y, 0xDFFF],
    [0x110000, 0x7FFFFFFF, PCT, Y],
    [-1, INT_MIN, PCT, ord("d")],
    [PCT, ord("^"), 0xE9],
    [PCT, ord("#"), 0xE9],
    [PCT, ord("^"), 0xFF],
    [PCT, ord("^"), 0x131],
    [PCT, ord("^"), 0x17F],
    [PCT, ord("^"), 0x1C5],
    [PCT, ord("^"), 0x3C2],
    [PCT, ord("^"), 0x3C3],
    [PCT, ord("^"), 0xDF],
    [PCT, ord("^"), 0x1F600],
    [PCT, ord("^"), 0xD800],
    [PCT, ord("^"), -1],
    [PCT, ord("5"), 0x1F600],
    [PCT, 0xFF15, ord("d")],
    [PCT, ord("1"), 0xFF10, ord("d")],
    [PCT, ord("E"), 0xE9],
    [PCT, 0xE9],
    [PCT, -1],
    [PCT, 0x100 + ord("Y")],
    [PCT, 0x10000 + ord("d")],
    [PCT, ord("_"), 0x100 + ord("5"), ord("d")],
]:
    wide(units)
    wide(units, maxsize=4)

CASES.sort(key=lambda c: c[1])  # one tzset a zone, in the oracle and its replay


def c_bytes(b: bytes) -> str:
    return '"' + "".join(f"\\x{x:02x}" for x in b) + '"'


def c_int(v: int) -> str:
    return "(-2147483647-1)" if v == INT_MIN else str(v)


def c_wide(units: list) -> str:
    # wchar_t is int: a unit past INT_MAX is the int with its bits.
    vals = [u - 2**32 if u > INT_MAX else u for u in units] + [0]
    return "(const wchar_t[]){" + ",".join(c_int(v) for v in vals) + "}"


def c_long(v: int) -> str:
    return "(-9223372036854775807L-1)" if v == -(2**63) else f"{v}L"


def main() -> None:
    rows = []
    for kind, tz, t, maxsize, null, fmt in CASES:
        f9 = ",".join(c_int(v) for v in t[:9])
        zone = "NULL" if t[10] is None else c_bytes(t[10])
        fmt_c = c_bytes(fmt) if kind == "N" else c_wide(fmt)
        rows.append(f'{{{1 if kind == "W" else 0}, {c_bytes(tz.encode())}, {{{f9}}}, '
                    f'{c_long(t[9])}, {zone}, {maxsize}, {int(null)}, '
                    f'(const void *){fmt_c}}},')
    program = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wchar.h>

struct kase {
    int wide; const char *tz; int f[9]; long off; const char *zone;
    size_t maxsize; int null; const void *fmt;
};

static const struct kase K[] = {
''' + "\n".join(rows) + r'''
};

static void hexbytes(const unsigned char *b, size_t n, const char *empty) {
    if (!n) { fputs(empty, stdout); return; }
    for (size_t i = 0; i < n; i++) printf("%02x", b[i]);
}

static void hexunits(const wchar_t *w, size_t n, const char *empty) {
    if (!n) { fputs(empty, stdout); return; }
    for (size_t i = 0; i < n; i++) printf(i ? ".%08x" : "%08x", (unsigned) w[i]);
}

static unsigned char nbuf[2048];
static wchar_t wbuf[2048];

int main(int argc, char **argv) {
    if (!setlocale(LC_ALL, argc > 1 ? argv[1] : "C.UTF-8")) { puts("no locale"); return 1; }
    const char *last_tz = "";
    for (size_t k = 0; k < sizeof K / sizeof K[0]; k++) {
        const struct kase *c = &K[k];
        if (strcmp(c->tz, last_tz)) { setenv("TZ", c->tz, 1); tzset(); last_tz = c->tz; }
        struct tm t;
        memset(&t, 0, sizeof t);
        t.tm_sec = c->f[0]; t.tm_min = c->f[1]; t.tm_hour = c->f[2];
        t.tm_mday = c->f[3]; t.tm_mon = c->f[4]; t.tm_year = c->f[5];
        t.tm_wday = c->f[6]; t.tm_yday = c->f[7]; t.tm_isdst = c->f[8];
        t.tm_gmtoff = c->off; t.tm_zone = c->zone;
        size_t cap = c->maxsize + 16;
        memset(nbuf, 0xaa, sizeof nbuf);
        memset(wbuf, 0xaa, sizeof wbuf);
        size_t r;
        errno = 0;
        if (c->wide) {
            wchar_t *volatile out = c->null ? NULL : wbuf;
            r = wcsftime(out, c->maxsize, c->fmt, &t);
        } else {
            char *volatile out = c->null ? NULL : (char *) nbuf;
            r = strftime(out, c->maxsize, c->fmt, &t);
        }
        int e = errno;
        printf("%c %s %d %d %d %d %d %d %d %d %d %ld ", c->wide ? 'W' : 'N', c->tz,
               c->f[0], c->f[1], c->f[2], c->f[3], c->f[4], c->f[5], c->f[6], c->f[7],
               c->f[8], c->off);
        if (!c->zone) fputs("-", stdout);
        else hexbytes((const unsigned char *) c->zone, strlen(c->zone), "~");
        printf(" %zu %d ", c->maxsize, c->null);
        size_t touched = 0;
        if (c->wide) {
            hexunits(c->fmt, wcslen(c->fmt), "~");
            for (size_t i = 0; i < cap; i++)
                if ((unsigned) wbuf[i] != 0xaaaaaaaau) touched = i + 1;
            printf(" | %zu %d ", r, e);
            hexunits(wbuf, touched, "-");
        } else {
            hexbytes(c->fmt, strlen(c->fmt), "~");
            for (size_t i = 0; i < cap; i++)
                if (nbuf[i] != 0xaa) touched = i + 1;
            printf(" | %zu %d ", r, e);
            hexbytes(nbuf, touched, "-");
        }
        putchar('\n');
    }
    return 0;
}
'''
    with workdir() as d:
        d = Path(d)
        (d / "sf.c").write_text(program, encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O0 -w -o sf sf.c 2>&1")
        if built.returncode:
            sys.exit(f"sf.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./sf C.UTF-8")
        if r.returncode or r.stderr:
            sys.exit(f"sf: {r.stdout}{r.stderr}")
        c = run(f"cd {wsl_path(d)} && ./sf C")
        if c.returncode or c.stderr:
            sys.exit(f"sf C: {c.stdout}{c.stderr}")
    utf8_narrow = [line for line in r.stdout.splitlines() if line.startswith("N ")]
    c_narrow = [line for line in c.stdout.splitlines() if line.startswith("N ")]
    if utf8_narrow != c_narrow:
        diff = next(a for a, b in zip(utf8_narrow, c_narrow) if a != b)
        sys.exit(f"the C locale's narrow answers differ from C.UTF-8's, first: {diff}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(r.stdout.splitlines())} lines ({len(CASES)} cases)")


if __name__ == "__main__":
    main()
