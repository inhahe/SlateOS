"""glibc 2.39's reading of `TZ`, as the oracle for posix/src/tz.rs.

    python posix/tools/oracle/tz_harness.py   # writes posix/src/tz_oracle.txt

What a C program sees of its zone: which zone a `TZ` value names -- a file
before a rule, `TZ=` the file `Universal`, a `:` that means nothing, the
`posixrules` file standing in for a rule with no dates -- and then `tzname`,
`timezone` and `daylight` after `tzset`; `localtime_r` at instants around
the zone's changes; `mktime` of wall-clock times in the gaps and overlaps;
and `strftime`'s `%Z` and `%z`. requests/b-d-the-libc-reads-tz-unlike-glibc-
and-now-unlike-date.md listed where the libc and glibc differed; every row of
its table is a scenario here.

Each scenario is one process, its steps in order, because glibc's state
outlives a `tzset`: `__tzfile_default` leaves `rule_dstoff` behind for the
next zone, and a zone it built is read again at every `tzset`.

The zone files a scenario can read are recorded first, byte for byte, so the
replay can offer posix exactly the files glibc read:

    file <path> <hex>
    === <scenario>
    tz <value-hex | unset> dir <value-hex | unset> | <globals>
    local <secs> | <ret> <sec> <min> <hour> <mday> <mon> <year> <wday> <yday> <isdst> <gmtoff> <zone-hex> | <globals>
    mk <year> <mon> <mday> <hour> <min> <sec> <isdst> | <ret> <errno> <sec> <min> <hour> <mday> <mon> <year> <wday> <yday> <isdst> <gmtoff> <zone-hex> | <globals>
    strf <secs> | <hex of "%Z %z">

`<globals>` is `<tzname0-hex> <tzname1-hex> <timezone> <daylight>` as they
stand after the call: glibc's `localtime_r` and `mktime` move `tzname` to the
names in force around the instant converted.

A NULL name is `-`. `<ret>` is 1 for a pointer and 0 for NULL.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "tz_oracle.txt"
ZONEINFO = "/usr/share/zoneinfo"

# The files a scenario may read: every name below that exists in WSL's tzdata,
# under the default TZDIR, and `/etc/localtime` for `TZ` unset.
FILES = [
    "posixrules", "America/New_York", "Europe/Paris", "Australia/Sydney",
    "Asia/Kolkata", "Pacific/Chatham", "Europe/Dublin", "America/Sao_Paulo",
    "Pacific/Apia", "Antarctica/Troll", "America/Nuuk", "Asia/Tehran",
    "Pacific/Kiritimati", "EST5EDT", "UTC", "Etc/GMT+5", "Europe/London",
]

# Instants every zone is read at: the epochs of interest, then each side of
# the 2021 changes in the zones that have them.
COMMON = [
    -2208988800, -1000000000, -1, 0, 1, 1000000000, 1500000000, 1700000000,
    2000000000, 2147483647, 4000000000,
]
NY_2021 = [1615705199, 1615705200, 1636264799, 1636264800]
PARIS_2021 = [1616893199, 1616893200, 1635641999, 1635642000]
SYDNEY_2021 = [1617465599, 1617465600, 1633190399, 1633190400]
APIA_2011 = [1325239199, 1325239200]
TROLL_2021 = [1616893199, 1616893200, 1635641999, 1635642000]
NUUK_2023 = [1679792399, 1679792400, 1698541199, 1698541200]
YEAR_ENDS = [1609459199, 1609459200, 1609502400, 1640995199, 1640995200]
PRE_1970 = [-31536000, -15000000, -86400]

# Wall-clock times for mktime: (year, mon, mday, hour, min, sec, isdst).
GAP_OVERLAP = [
    (2021, 2, 14, 1, 59, 59, -1), (2021, 2, 14, 2, 30, 0, -1), (2021, 2, 14, 2, 30, 0, 0),
    (2021, 2, 14, 2, 30, 0, 1), (2021, 2, 14, 3, 0, 0, -1),
    (2021, 10, 7, 0, 59, 59, -1), (2021, 10, 7, 1, 30, 0, -1), (2021, 10, 7, 1, 30, 0, 0),
    (2021, 10, 7, 1, 30, 0, 1), (2021, 10, 7, 2, 0, 0, -1),
    (2021, 0, 1, 0, 0, 0, -1), (2021, 6, 1, 12, 0, 0, -1), (2021, 6, 1, 12, 0, 0, 0),
    (1969, 6, 1, 12, 0, 0, -1), (2100, 6, 1, 12, 0, 0, -1),
    (2021, 13, 40, 25, 61, 61, -1), (2021, -1, 0, -1, -1, -1, -1),
]
EU_GAP_OVERLAP = [
    (2021, 2, 28, 2, 30, 0, -1), (2021, 2, 28, 2, 30, 0, 1), (2021, 9, 31, 2, 30, 0, -1),
    (2021, 9, 31, 2, 30, 0, 0), (2021, 9, 31, 2, 30, 0, 1),
]

SCENARIOS = []


def sc(name, *steps):
    SCENARIOS.append((name, list(steps)))


def tz(value, tzdir=None):
    return ("tz", value, tzdir)


def local(*ts):
    return [("local", t) for t in ts]


def mk(*cases):
    return [("mk", c) for c in cases]


def strf(*ts):
    return [("strf", t) for t in ts]


def zone(name, value, instants, mks=(), tzdir=None):
    sc(name, tz(value, tzdir), *local(*instants), *mk(*mks), *strf(*instants[:4]))


# --- which zone a value names (tzset_internal) --------------------------------
zone("unset", None, COMMON)
zone("empty", "", COMMON)
zone("utc file", "UTC", COMMON)
zone("utc0 rule", "UTC0", COMMON)
zone("colon utc", ":UTC", COMMON)
zone("bare colon", ":", COMMON)
zone("est5edt file first", "EST5EDT", COMMON + NY_2021 + [-631152000, 631152000])
zone("est5edt rule without the file", "EST5EDT", COMMON + NY_2021 + [-631152000, 631152000],
     tzdir="/nonexistent")
zone("colon est5edt", ":EST5EDT", COMMON + NY_2021)
zone("colon est5edt rule without the file", ":EST5EDT", COMMON + NY_2021, tzdir="/nonexistent")
zone("new york", "America/New_York", COMMON + NY_2021, GAP_OVERLAP)
zone("colon new york", ":America/New_York", COMMON + NY_2021)
zone("absolute new york", ZONEINFO + "/America/New_York", COMMON + NY_2021)
zone("tzdir given", "America/New_York", COMMON + NY_2021, tzdir=ZONEINFO)
zone("tzdir trailing slash", "America/New_York", NY_2021, tzdir=ZONEINFO + "/")
zone("no such file", "Foo/Bar", COMMON)
zone("no such file in a real dir", "America/X", COMMON)
zone("dotdot", "../zoneinfo/America/New_York", COMMON)
zone("localtime path itself", "/etc/localtime", COMMON)

# --- the POSIX rule (__tzset_parse_tz) ----------------------------------------
zone("rule us", "EST5EDT,M3.2.0,M11.1.0", COMMON + NY_2021 + PRE_1970, GAP_OVERLAP)
zone("rule junk after offset", "EST5x", COMMON + NY_2021)
zone("rule dst name no dates", "AAA3BBB", COMMON + NY_2021 + [-631152000, 631152000])
zone("rule dst name lone comma", "AAA3BBB,", COMMON + NY_2021)
zone("rule cet before 1970", "CET-1CEST,M3.5.0,M10.5.0/3", COMMON + PRE_1970 + PARIS_2021,
     EU_GAP_OVERLAP)
zone("rule southern", "NZST-12NZDT,M9.5.0,M4.1.0/3", COMMON + YEAR_ENDS)
zone("rule year boundary", "XXX-14YYY,M12.5.0/23,M1.1.0/1", COMMON + YEAR_ENDS)
zone("rule half hour", "IST-5:30", COMMON)
zone("rule bracketed", "<+0330>-3:30", COMMON)
zone("rule bracketed dst", "<-02>2<-01>,M3.5.0/-1,M10.5.0/0", COMMON + NUUK_2023)
zone("rule space before offset", "EST+ 5", COMMON)
zone("rule plus sign", "EST+5EDT", COMMON + NY_2021)
zone("rule julian", "XXX3YYY,J60,J300", COMMON + YEAR_ENDS)
zone("rule zero based", "XXX3YYY,59,300", COMMON + YEAR_ENDS)
zone("rule j365 to j1", "XXX3YYY,J365,J1", COMMON + YEAR_ENDS)
zone("rule odd times", "EST5EDT,M3.2.0/-1:30,M11.1.0/26", COMMON + NY_2021)
zone("rule long names", "<ABCDEFGHIJKLMNOPQRSTUVWXYZ>5<abcdefghijklmnopqrstuvwxyz>,M3.2.0,M11.1.0",
     COMMON + NY_2021)
zone("rule two letter name", "ES5", COMMON)
zone("rule no offset", "EST", COMMON)
zone("rule offset 25 hours", "XXX25", COMMON)
zone("rule dst offset given", "AAA3BBB2,M3.2.0,M11.1.0", COMMON + NY_2021)

# --- zone files ---------------------------------------------------------------
zone("paris", "Europe/Paris", COMMON + PARIS_2021 + PRE_1970, EU_GAP_OVERLAP)
zone("sydney", "Australia/Sydney", COMMON + SYDNEY_2021 + YEAR_ENDS)
zone("kolkata", "Asia/Kolkata", COMMON)
zone("chatham", "Pacific/Chatham", COMMON + YEAR_ENDS)
zone("dublin negative dst", "Europe/Dublin", COMMON + PARIS_2021)
zone("sao paulo dst abolished", "America/Sao_Paulo", COMMON + [1550000000, 1572000000])
zone("apia date line", "Pacific/Apia", COMMON + APIA_2011)
zone("troll", "Antarctica/Troll", COMMON + TROLL_2021)
zone("nuuk negative footer times", "America/Nuuk", COMMON + NUUK_2023)
zone("tehran", "Asia/Tehran", COMMON + [1600000000])
zone("kiritimati", "Pacific/Kiritimati", COMMON + [788918400, 788918399])
zone("gmt plus five", "Etc/GMT+5", COMMON)
zone("london", "Europe/London", COMMON + PARIS_2021 + PRE_1970)

# --- what one tzset leaves for the next ---------------------------------------
sc("posixrules after a file",
   tz("America/New_York"), *local(*NY_2021),
   tz("AAA3BBB"), *local(*NY_2021, -631152000),
   tz("AAA3BBB"), *local(*NY_2021))
sc("posixrules twice",
   tz("AAA3BBB"), *local(*NY_2021),
   tz("CCC4DDD"), *local(*NY_2021),
   tz("AAA3BBB"), *local(*NY_2021))
sc("zone then rule then zone",
   tz("Europe/Paris"), *local(*PARIS_2021),
   tz("EST5EDT,M3.2.0,M11.1.0"), *local(*NY_2021),
   tz("Europe/Paris"), *local(*PARIS_2021))
sc("unset then set",
   tz(None), *local(0, 1700000000),
   tz("Asia/Kolkata"), *local(0, 1700000000),
   tz(None), *local(0, 1700000000))


C_MAIN = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/wait.h>
#include <unistd.h>

static void hex(const char *s) {
    if (!s) { fputs("-", stdout); return; }
    if (!*s) { fputs("\"\"", stdout); return; }
    for (; *s; s++) printf("%02x", (unsigned char)*s);
}

static void tmline(const struct tm *tm) {
    printf("%d %d %d %d %d %d %d %d %d %ld ", tm->tm_sec, tm->tm_min, tm->tm_hour,
           tm->tm_mday, tm->tm_mon, tm->tm_year, tm->tm_wday, tm->tm_yday,
           tm->tm_isdst, tm->tm_gmtoff);
    hex(tm->tm_zone);
}

static struct tm marked(void) {
    struct tm tm;
    memset(&tm, 0, sizeof tm);
    tm.tm_sec = 77; tm.tm_min = 77; tm.tm_hour = 77; tm.tm_mday = 77;
    tm.tm_mon = 77; tm.tm_year = 77; tm.tm_wday = 77; tm.tm_yday = 777;
    tm.tm_isdst = 77; tm.tm_gmtoff = 77; tm.tm_zone = "marked";
    return tm;
}

static void set(const char *name, const char *value) {
    if (value) setenv(name, value, 1); else unsetenv(name);
}

/* The globals as they stand: `localtime_r` and `mktime` can move tzname. */
static void globals(void) {
    hex(tzname[0]); putchar(' '); hex(tzname[1]);
    printf(" %ld %d", timezone, daylight);
}

static void do_tz(const char *v, const char *tv, const char *dir, const char *dv) {
    set("TZDIR", dir);
    set("TZ", v);
    tzset();
    printf("tz %s dir %s | ", tv, dv);
    globals();
    putchar('\n');
}

static void do_local(long long secs) {
    time_t t = (time_t)secs;
    struct tm tm = marked();
    struct tm *r = localtime_r(&t, &tm);
    printf("local %lld | %d ", secs, r != NULL);
    tmline(&tm);
    fputs(" | ", stdout);
    globals();
    putchar('\n');
}

static void do_mk(int y, int mo, int d, int h, int mi, int s, int isdst) {
    struct tm tm = marked();
    tm.tm_year = y - 1900; tm.tm_mon = mo; tm.tm_mday = d;
    tm.tm_hour = h; tm.tm_min = mi; tm.tm_sec = s; tm.tm_isdst = isdst;
    errno = 0;
    time_t r = mktime(&tm);
    int e = errno;
    printf("mk %d %d %d %d %d %d %d | %lld %d ", y, mo, d, h, mi, s, isdst, (long long)r, e);
    tmline(&tm);
    fputs(" | ", stdout);
    globals();
    putchar('\n');
}

static void run(const char *name, void (*scenario)(void)) {
    printf("=== %s\n", name);
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        scenario();
        fflush(stdout);
        _exit(0);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        printf("ended %d\n", status);
    }
    fflush(stdout);
}

static void do_strf(long long secs) {
    time_t t = (time_t)secs;
    struct tm tm = marked();
    char buf[256];
    localtime_r(&t, &tm);
    size_t n = strftime(buf, sizeof buf, "%Z %z", &tm);
    buf[n] = 0;
    printf("strf %lld | ", secs);
    hex(buf);
    putchar('\n');
}

@SCENARIOS@

int main(void) {
    alarm(120);
@MAIN@
    return 0;
}
'''


def c_str(s):
    """A C string literal for `s` (bytes as octal escapes where needed)."""
    out = []
    for b in s.encode("utf-8"):
        if 32 <= b < 127 and chr(b) not in '"\\?':
            out.append(chr(b))
        else:
            out.append("\\%03o" % b)
    return '"' + "".join(out) + '"'


def text_hex(v):
    return "unset" if v is None else (v.encode("utf-8").hex() or '""')


def body(steps):
    lines = []
    for step in steps:
        if step[0] == "tz":
            _, v, d = step
            lines.append("    do_tz(%s, %s, %s, %s);" % (
                "NULL" if v is None else c_str(v), c_str(text_hex(v)),
                "NULL" if d is None else c_str(d), c_str(text_hex(d))))
        elif step[0] == "local":
            lines.append("    do_local(%dLL);" % step[1])
        elif step[0] == "mk":
            lines.append("    do_mk(%d, %d, %d, %d, %d, %d, %d);" % step[1])
        elif step[0] == "strf":
            lines.append("    do_strf(%dLL);" % step[1])
    return "\n".join(lines)


def candidate_files():
    """Every file a scenario's `TZ` could name, where glibc looks for it: the
    listed zones, then each value itself -- `EST` and `UTC` are files as well
    as rules -- absolute, or under the default TZDIR. A `..` is left out: posix
    refuses such a name before looking (the DEVIATIONS of the replay)."""
    paths = [f"{ZONEINFO}/{n}" for n in FILES] + ["/etc/localtime"]
    for _name, steps in SCENARIOS:
        for step in steps:
            if step[0] != "tz" or not step[1]:
                continue
            value = step[1][1:] if step[1].startswith(":") else step[1]
            if not value or ".." in value.split("/"):
                continue
            paths.append(value if value.startswith("/") else f"{ZONEINFO}/{value}")
    return sorted(set(paths))


def main() -> None:
    out = []
    with workdir() as d:
        d = Path(d)
        # A file that does not exist is left out; one that does is read
        # whole, through any symbolic link, as glibc reads it.
        listing = run("for f in " + " ".join(f"'{p}'" for p in candidate_files())
                      + "; do [ -f \"$f\" ] && printf '%s ' \"$f\" && "
                        "od -An -v -tx1 \"$f\" | tr -d ' \\n' && echo; done; true")
        if listing.returncode:
            sys.exit(listing.stderr)
        for line in listing.stdout.splitlines():
            if line.strip():
                path, _, data = line.partition(" ")
                out.append(f"file {path} {data}")
        # One program, a child process per scenario: each starts from a fresh
        # process's state, as it would as a program of its own.
        funcs, calls = [], []
        for i, (name, steps) in enumerate(SCENARIOS):
            funcs.append(f"static void scenario_{i}(void) {{\n{body(steps)}\n}}")
            calls.append(f"    run({c_str(name)}, scenario_{i});")
        src = d / "tz.c"
        src.write_text(C_MAIN.replace("@SCENARIOS@", "\n\n".join(funcs))
                       .replace("@MAIN@", "\n".join(calls)), encoding="utf-8", newline="\n")
        # Compiled apart from the run, and warnings refused: the run's output
        # is the oracle, and a compiler's must never reach it.
        built = run(f"cd {wsl_path(d)} && gcc -O1 -Wall -Werror -o tz tz.c")
        if built.returncode or built.stdout or built.stderr:
            sys.exit(f"tz.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./tz")
        if r.returncode or r.stderr:
            sys.exit(f"tz: {r.stdout}{r.stderr}")
        out += r.stdout.splitlines()
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(SCENARIOS)} scenarios, {len(out)} lines")


if __name__ == "__main__":
    main()
