"""glibc 2.39's `strptime`, as the oracle for posix/src/time.rs.

    python posix/tools/oracle/strptime_harness.py   # writes posix/src/strptime_oracle.txt

Every conversion glibc knows, alone and in the combinations programs use,
over inputs chosen to find the edges: each field's range and digit count,
leading white space, names in every case and abbreviation, the `E` and `O`
modifiers, strftime's flags and widths, and the date arithmetic glibc does
after the parse (the weekday and day of the year from a date, a date from a
week number). One line per call:

    <format-hex> <input-hex> <init> = <consumed> <sec> <min> <hour> <mday> <mon> <year> <wday> <yday> <isdst> <gmtoff>

`<init>` is the `struct tm` the call starts from -- `s`, recognisable
values in every field, or `z`, all zero -- so the line shows which fields a
parse writes and which it leaves. `<consumed>` is how many bytes the call
took, `-1` for NULL. The format and input are hex, so that any byte can be
in one. `TZ` is `UTC`, for `%s`. Built with -fno-builtin, so gcc can
neither fold a call nor substitute its own answer.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "strptime_oracle.txt"

# Every conversion alone, over every input below.
SINGLE = [f"%{c}" for c in "aAbBhcCdDeFgGHIjklmMnpPrRsStTuUVwWxXyYzZ%"]
INPUTS = [
    "", " ", "0", "00", "000", "1", "01", "001", "7", "9", "12", "13", "23", "24", "31", "32",
    "53", "54", "59", "60", "61", "62", "69", "99", "100", "123", "366", "367", "1969", "2026",
    "9999", "10000", "12345", " 5", "  17x", "\t8", "\n9", "\v3", "-1", "+1", "Mon", "monday",
    "TUESDAY", "wed", "Thurs", "Sep", "september", "Sept", "MAY", "AM", "pm", "a.m.", "P",
    "Z", "+05", "-0800", "+05:30", "+0560", "+5", "+123", "-12:3", "UTC", "EST5EDT x", "%",
    "%x", "x", "1727519700", "-5", "99999999999999999999", "12:34:56", "2026-09-28",
    "09/28/26", "Mon Sep 28 10:35:00 2026", "10:35:00 PM", " Mon", "\tSep", " pm", " Z",
    " +0530", " UTC", "Sundays", "Mayday", "Sat.", "DEC", "Decembe",
]

# Combinations, each over its own inputs.
COMBOS = {
    "%Y-%m-%d": ["2026-09-28", "2024-02-29", "2023-02-29", "1999-12-31x", "0-1-1",
                 "2026-9-8", "2026 - 09 - 28", "2026-13-01", "2026-00-10", "2026-01-00"],
    "%F %T": ["2026-09-28 10:35:00", "2026-09-28 23:59:60", "2026-09-28 24:00:00",
              "2026-09-28T10:35:00"],
    "%c": ["Mon Sep 28 10:35:00 2026", "mon sep 28 10:35:00 2026", "Monday September 28 10:35:00 2026",
           "Mon Sep  8 01:02:03 1999", "Mon Sep 28 10:35 2026"],
    "%D": ["09/28/26", "9/8/69", "12/31/68", "13/01/00"],
    "%x %X": ["09/28/26 10:35:00", "01/01/70 00:00:00"],
    "%r": ["10:35:00 PM", "12:00:00 AM", "12:00:00 PM", "00:00:00 AM", "13:00:00 PM", "01:02:03 am"],
    "%R": ["10:35", "23:59", "24:00", "9:5"],
    "%I:%M %p": ["12:30 AM", "12:30 PM", "1:05 pm", "11:59 PM", "0:30 AM", "13:00 PM"],
    "%H:%M %p": ["13:30 PM", "01:30 PM", "12:00 AM"],
    "%p %I": ["PM 3", "AM 12"],
    "%a, %d %b %Y %H:%M:%S %z": ["Mon, 28 Sep 2026 10:35:00 +0000",
                                 "Tue, 1 Jan 2030 00:00:00 -0500",
                                 "Wed, 02 Oct 2002 08:00:00 EST"],
    "%Y-%m-%dT%H:%M:%S%z": ["2026-09-28T10:35:00Z", "2026-09-28T10:35:00+05:30",
                            "2026-09-28T10:35:00-08"],
    "%Y %U %w": ["2026 38 1", "2026 0 0", "2026 0 6", "2024 53 2", "2026 52 6", "2023 1 0"],
    "%Y %W %a": ["2026 39 Mon", "2026 0 Sun", "2026 1 Mon", "2027 52 Fri"],
    "%U %w": ["10 3"],
    "%Y %U %w %m": ["2026 38 1 05"],
    "%Y %U %w %d": ["2026 38 1 17"],
    "%Y %j": ["2026 271", "2024 366", "2023 366", "2026 001", "2026 0", "2024 60", "2023 365"],
    "%C%Oy": ["2026", "1969"],
    "%Oy %C": ["26 20"],
    "%y %C": ["26 20", "69 19"],
    "%H %p": ["01 PM", "13 AM"],
    "%I %H": ["03 15"],
    "%H %I %p": ["15 03 PM"],
    "%I %p %H": ["03 PM 04"],
    "%m/%d/%Y %I:%M:%S %p": ["12/31/1999 11:59:59 PM", "02/29/2000 12:00:00 AM"],
    "%d %B %Y": ["1 January 1970", "31 december 9999", "29 February 2100"],
    "%Y-%m": ["2026-02", "2024-12"],
    "%m": ["2", "12"],
    "%Y %W %u": ["2026 1 1", "2026 0 7", "2021 53 5", "2026 54 1"],
    "%Y %U %a %H": ["2026 38 Mon 10"],
    "%U %Y %w": ["38 2026 1"],
    "%y %U %w": ["26 38 1"],
    "%C %y %U %w": ["20 26 38 1"],
    "%j": ["271", "60"],
    "%Y %j %m": ["2024 100 01"],
    "%Y %j %d": ["2024 100 05"],
    "%C%y": ["2026", "1969", "1868", "0000"],
    "%y%C": ["2620"],
    "%C": ["20", "19", "0"],
    "%C %Y": ["19 2026"],
    "%y": ["68", "69", "00"],
    "%Y%m%d%H%M%S": ["20260928103500", "2026092810350"],
    "%s": ["0", "1727519700", "-1", "86399", "9223372036854775807", "9223372036854775808",
           "253402300799", "67767976233316800"],
    "%s %Y": ["0 2026"],
    "%e/%m": [" 8/09", "18/09"],
    "%k|%l": [" 7| 7", "23|12", "7|0"],
    "%d%m%Y": ["28092026", "2892026"],
    "%m%d": ["0928", "928", "1231"],
    "%H%M": ["1035", "235", "0959"],
    "%Ey %EY %EC": ["26 2026 20"],
    "%Ec": ["Mon Sep 28 10:35:00 2026"],
    "%Ex": ["09/28/26"],
    "%EX": ["10:35:00"],
    "%Ea": ["Mon"],
    "%Od %Oe %OH %OI %Om %OM %OS": ["28  8 10 11 09 35 00"],
    "%OU %OV %OW %Ow %Oy": ["38 39 39 1 26"],
    "%Ob %OB %Oh": ["Sep September sep"],
    "%Oa": ["Mon"],
    "%OY": ["2026"],
    **{f"%O{c}": ["12", "Sep", "Mon", "PM", "2026", "+0100"] for c in "bBhdeHIkljmMSUVWwyYCgGuaApzZsnt"},
    **{f"%E{c}": ["12", "Mon Sep 28 10:35:00 2026", "09/28/26", "10:35:00", "2026", "Mon", "Sep"]
       for c in "cCxXyYabdHmMSjeUVW"},
    "%Q": ["x"],
    "%": ["", "%"],
    "%-d %_m %0H %^a %#b": ["5 7 9 MON sep"],
    "%010Y": ["2026", "0000002026"],
    "%4d": ["28"],
    "%G %V %u": ["2026 39 1"],
    "%g": ["26"],
    "%n%t%%": ["  \t %", "%"],
    " %Y": ["   2026"],
    "%Y ": ["2026", "2026   x"],
    "%Y\t%m": ["2026 09", "2026\n09", "202609"],
    "x%Yy": ["x2026y", "X2026Y", "x 2026y"],
    "%a %b": ["Thu Jan", "thursday january", "Thur Janu", "Tu Ja", "Mon xyz", "Mon  Sep",
              "Mon\tSep"],
    "%A%B": ["SundayMay", "SunMay", "SundayMaybe"],
    "%b%d": ["May05", "Mar15", "Jun1", "June1"],
    "%Z%Y": ["UTC2026", "UTC 2026"],
    "%Z %Y": ["UTC 2026", "  UTC 2026", "2026"],
    "%z %z": ["+01 -02", "Z Z"],
    "%S": ["61", "62"],
    "%M:%S": ["5:5", "05:05", " 5: 5"],
    "%H%%%M": ["10%35", "10 % 35"],
    "%Y-%m-%d %H:%M:%S.%s": ["2026-09-28 10:35:00.123"],
}

INITS = ["s", "z"]


def c_str(s: str) -> str:
    """A C string literal holding exactly `s`'s bytes."""
    out = []
    for b in s.encode("utf-8"):
        if b in (0x22, 0x5C):
            out.append("\\" + chr(b))
        elif 0x20 <= b < 0x7F and b != 0x3F:  # '?' escaped: no trigraphs
            out.append(chr(b))
        else:
            out.append(f"\\{b:03o}")
    return '"' + "".join(out) + '"'


def unmodified(fmt: str) -> str:
    """`fmt` with every `E` and `O` modifier taken out: `%Ey` -> `%y`."""
    out, i = [], 0
    while i < len(fmt):
        out.append(fmt[i])
        if fmt[i] == "%":
            i += 1
            while i < len(fmt) and fmt[i] in "-_0^#123456789":
                out.append(fmt[i])
                i += 1
            if i < len(fmt) and fmt[i] in "EO":
                i += 1
            if i < len(fmt):
                out.append(fmt[i])
        i += 1
    return "".join(out)


def cases():
    out = []
    for fmt in SINGLE:
        for inp in INPUTS:
            out.append((fmt, inp))
    for fmt, inps in COMBOS.items():
        for inp in inps:
            out.append((fmt, inp))
    # Every format with a modifier, again without it: in the C locale the
    # two mean the same, and the test holds this library to the second
    # where glibc's modifiers go wrong.
    seen = set(out)
    for fmt, inp in list(out):
        plain = unmodified(fmt)
        if plain != fmt and (plain, inp) not in seen:
            seen.add((plain, inp))
            out.append((plain, inp))
    return out


def gen_c(cs) -> str:
    c = [
        "#define _GNU_SOURCE",
        "#include <stdio.h>", "#include <stdlib.h>", "#include <string.h>", "#include <time.h>",
        "static void hexs(const char *s) { if (!*s) { printf(\"-\"); return; }"
        " for (; *s; s++) printf(\"%02x\", (unsigned char)*s); }",
        "static const char *const fmts[] = {" + ", ".join(c_str(f) for f, _ in cs) + "};",
        "static const char *const ins[] = {" + ", ".join(c_str(i) for _, i in cs) + "};",
        "static void init(struct tm *t, char kind) {",
        "  memset(t, 0, sizeof *t);",
        "  if (kind == 's') {",
        "    t->tm_sec = 47; t->tm_min = 37; t->tm_hour = 17; t->tm_mday = 27; t->tm_mon = 6;",
        "    t->tm_year = 77; t->tm_wday = 3; t->tm_yday = 222; t->tm_isdst = -7;",
        "    t->tm_gmtoff = -12345;",
        "  }",
        "}",
        "int main(void) {",
        "  setenv(\"TZ\", \"UTC\", 1); tzset();",
        "  for (unsigned i = 0; i < sizeof fmts / sizeof fmts[0]; i++) {",
        "    for (const char *k = \"" + "".join(INITS) + "\"; *k; k++) {",
        "      struct tm t; init(&t, *k);",
        "      char *r = strptime(ins[i], fmts[i], &t);",
        "      hexs(fmts[i]); printf(\" \"); hexs(ins[i]);",
        "      printf(\" %c = %ld %d %d %d %d %d %d %d %d %d %ld\\n\", *k,",
        "             r ? (long)(r - ins[i]) : -1L, t.tm_sec, t.tm_min, t.tm_hour, t.tm_mday,",
        "             t.tm_mon, t.tm_year, t.tm_wday, t.tm_yday, t.tm_isdst, (long)t.tm_gmtoff);",
        "    }",
        "  }",
        "  return 0;",
        "}",
    ]
    return "\n".join(c) + "\n"


def main():
    cs = cases()
    with workdir() as tmp:
        (Path(tmp) / "strptime_oracle.c").write_text(gen_c(cs), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -w -o strptime_oracle strptime_oracle.c "
                f"&& ./strptime_oracle > '{wsl_path(OUT)}'")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines from {len(cs)} cases, {OUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
