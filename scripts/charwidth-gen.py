#!/usr/bin/env python3
"""Generate the terminal-column width tables of `userspace/charwidth`.

They are built from a pinned Unicode Character Database, by the policy
design-decisions §1042 set out. The tables used to come from whichever Python ran the old generator
(`unicodedata`, so Unicode 16.0 on one machine and 15.0 on another). They now
come from the UCD's own files, for the version named below, downloaded once
into a cache and checked against the SHA-256 recorded here.

The policy, which is gnulib's (coreutils 9.5's `wcwidth`) where gnulib renders
better and the UCD's where it is newer (§1042):

* **None** -- the C0 and C1 controls and DEL. Not in the tables: `char_width`
  tests for them first.
* **0** -- general categories Mn (non-spacing marks) and Me (enclosing marks);
  Cf (format characters) *except* the prepended concatenation marks, which
  are visible signs and take a cell -- so the soft hyphen U+00AD is 0, shown
  by Unicode only at a line break; and the Hangul Jamo medial vowels and final
  consonants (U+1160-11FF, and the assigned ones of U+D7B0-D7FF), which
  combine with the syllable before them.
* **2** -- East Asian Width W or F (UAX #11), including the defaults
  EastAsianWidth.txt declares for unassigned code points (its `@missing`
  lines), so a cell in an East Asian block is wide before it is assigned.
* **1** -- everything else.

The tables are a modified copy of the UCD's files, so the image carries the
Unicode licence's notice for them: `userspace/charwidth/licenses/notices.yaml`,
whose version `--emit` keeps equal to `UCD_VERSION`.

    python scripts/charwidth-gen.py --emit      # rewrite the tables in lib.rs
    python scripts/charwidth-gen.py --check     # exit 1 if lib.rs or the notice differs
    python scripts/charwidth-gen.py --compare DUMP   # every code point where
        # the result differs from a "LO HI WIDTH" dump (gnulib's, glibc's)
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import sys
import urllib.request

UCD_VERSION = "18.0.0"
# SHA-256 of each file this reads, as unicode.org publishes it for
# UCD_VERSION. A mismatch removes the download: nothing is generated from
# bytes that are not the ones recorded.
UCD_FILES = {
    "UnicodeData.txt": "0736451de439ae7baf1425136617da495e09ee5afbe6e394374db7009ea08950",
    "EastAsianWidth.txt": "a0cf29eacd00cfcaec4381c6b7c281685f18dbb4e7ff82b4076ccb342ca839aa",
    "PropList.txt": "f438f532e8737bb8a2702126cdf9c4af5e357c58c7acf9d9eb2fc7c1a1d955d6",
}
UCD_URL = "https://www.unicode.org/Public/{v}/ucd/{f}"

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
LIB = os.path.join(ROOT, "userspace", "charwidth", "src", "lib.rs")
# The tables are a modified copy of the UCD's files, so the image carries the
# Unicode licence's notice for them (design-decisions 1433), under the version
# they were generated from. `--emit` keeps that version in step and `--check`
# refuses a manifest that names another.
NOTICES = os.path.join(ROOT, "userspace", "charwidth", "licenses", "notices.yaml")
NOTICE_VERSION = re.compile(r"(?m)^(Unicode Character Database:\n  version: )(\S+)$")
CACHE = os.path.join(os.path.expanduser("~"), ".cache", "slateos-ucd", UCD_VERSION)

MAX = 0x110000


def fetch(name: str) -> str:
    os.makedirs(CACHE, exist_ok=True)
    path = os.path.join(CACHE, name)
    if not os.path.exists(path):
        url = UCD_URL.format(v=UCD_VERSION, f=name)
        with urllib.request.urlopen(url) as r:  # noqa: S310 (a fixed https URL)
            data = r.read()
        with open(path + ".part", "wb") as f:
            f.write(data)
        os.replace(path + ".part", path)
    data = open(path, "rb").read()
    want = UCD_FILES[name]
    got = hashlib.sha256(data).hexdigest()
    if want is not None and got != want:
        os.remove(path)
        sys.exit(f"charwidth-gen: {name} does not match its recorded SHA-256; removed")
    if want is None:
        print(f"charwidth-gen: {name} sha256 {got} (not yet pinned)", file=sys.stderr)
    return data.decode("utf-8")


def ranges_of(field: str) -> range:
    lo, _, hi = field.partition("..")
    return range(int(lo, 16), int(hi or lo, 16) + 1)


def general_categories() -> list[str]:
    cat = ["Cn"] * MAX
    first = None
    for line in fetch("UnicodeData.txt").splitlines():
        f = line.split(";")
        cp, name, gc = int(f[0], 16), f[1], f[2]
        if name.endswith(", First>"):
            first = cp
            continue
        if name.endswith(", Last>") and first is not None:
            for c in range(first, cp + 1):
                cat[c] = gc
            first = None
            continue
        cat[cp] = gc
    return cat


def east_asian_widths() -> list[str]:
    eaw = ["N"] * MAX
    text = fetch("EastAsianWidth.txt")
    # The defaults first, in file order (a later @missing narrows an earlier one).
    for m in re.finditer(r"^# @missing: ([0-9A-F.]+); (\w+)", text, re.M):
        for c in ranges_of(m.group(1)):
            eaw[c] = m.group(2)
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        field, value = (x.strip() for x in line.split(";"))
        for c in ranges_of(field):
            eaw[c] = value
    return eaw


def prop(name: str) -> set[int]:
    out: set[int] = set()
    for line in fetch("PropList.txt").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        field, value = (x.strip() for x in line.split(";"))
        if value == name:
            out.update(ranges_of(field))
    return out


def widths() -> list[int]:
    gc = general_categories()
    eaw = east_asian_widths()
    prepended = prop("Prepended_Concatenation_Mark")
    w = [1] * MAX
    for c in range(MAX):
        if c < 0x20 or 0x7F <= c < 0xA0:
            w[c] = -1
        elif gc[c] in ("Mn", "Me") or (gc[c] == "Cf" and c not in prepended):
            w[c] = 0
        elif 0x1160 <= c <= 0x11FF or (0xD7B0 <= c <= 0xD7FF and gc[c] != "Cn"):
            w[c] = 0
        elif eaw[c] in ("W", "F"):
            w[c] = 2
    return w


def runs(w: list[int], value: int) -> list[tuple[int, int]]:
    out: list[list[int]] = []
    for c in range(MAX):
        if 0xD800 <= c <= 0xDFFF or w[c] != value:
            continue
        if out and out[-1][1] + 1 == c:
            out[-1][1] = c
        else:
            out.append([c, c])
    return [(a, b) for a, b in out]


def table(name: str, rs: list[tuple[int, int]], doc: str) -> str:
    rows = "\n".join(f"    (0x{a:04X}, 0x{b:04X})," for a, b in rs)
    head = "\n".join(f"/// {line}".rstrip() for line in doc.splitlines())
    return f"{head}\nstatic {name}: [(u32, u32); {len(rs)}] = [\n{rows}\n];\n"


def emit_tables(w: list[int]) -> str:
    return (
        table("ZERO_WIDTH", runs(w, 0),
              f"Code points that occupy no terminal column (Unicode {UCD_VERSION}): the\n"
              "non-spacing and enclosing marks (`Mn`, `Me`), the format characters (`Cf`)\n"
              "other than the prepended concatenation marks, and the Hangul Jamo medial\n"
              "vowels and final consonants. Generated by `scripts/charwidth-gen.py`.")
        + "\n"
        + table("WIDE", runs(w, 2),
                f"Code points that occupy two terminal columns (Unicode {UCD_VERSION}): East\n"
                "Asian Wide and Fullwidth (UAX #11), including the unassigned code points\n"
                "EastAsianWidth.txt defaults to Wide. Generated by `scripts/charwidth-gen.py`.")
    )


TABLES = re.compile(
    r"/// Code points that occupy no terminal column.*?static WIDE: \[\(u32, u32\); \d+\] = \[.*?\n\];\n",
    re.S,
)


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or check charwidth's tables.")
    ap.add_argument("--emit", action="store_true")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--compare", metavar="DUMP")
    a = ap.parse_args()
    w = widths()
    new = emit_tables(w)
    src = open(LIB, encoding="utf-8").read()
    m = TABLES.search(src)
    if not m:
        sys.exit("charwidth-gen: the tables in lib.rs were not found")
    notices = open(NOTICES, encoding="utf-8").read()
    nm = NOTICE_VERSION.search(notices)
    if not nm:
        sys.exit("charwidth-gen: notices.yaml names no Unicode Character Database version")
    if a.emit:
        open(LIB, "w", encoding="utf-8", newline="\n").write(src[: m.start()] + new + src[m.end():])
        print("charwidth-gen: tables rewritten")
        if nm.group(2) != UCD_VERSION:
            fixed = notices[: nm.start(2)] + UCD_VERSION + notices[nm.end(2):]
            open(NOTICES, "w", encoding="utf-8", newline="\n").write(fixed)
            print(f"charwidth-gen: notices.yaml now names Unicode {UCD_VERSION}")
    if a.check and m.group(0) != new:
        print("charwidth-gen: lib.rs's tables are not what the UCD gives; run --emit", file=sys.stderr)
        return 1
    if a.check and nm.group(2) != UCD_VERSION:
        print(f"charwidth-gen: notices.yaml names Unicode {nm.group(2)}, the tables are "
              f"{UCD_VERSION}'s; run --emit", file=sys.stderr)
        return 1
    if a.compare:
        other = {}
        for line in open(a.compare, encoding="utf-8"):
            lo, hi, v = line.split()
            for c in range(int(lo, 16), int(hi, 16) + 1):
                other[c] = int(v)
        diff = [c for c in sorted(other) if other[c] != w[c]]
        print(f"{len(diff)} code points differ from {a.compare}", file=sys.stderr)
        run = None
        for c in diff:
            key = (w[c], other[c])
            if run and run[1] + 1 == c and run[2] == key:
                run[1] = c
            else:
                if run:
                    print(f"{run[0]:04X}-{run[1]:04X}\tours {run[2][0]}\ttheirs {run[2][1]}")
                run = [c, c, key]
        if run:
            print(f"{run[0]:04X}-{run[1]:04X}\tours {run[2][0]}\ttheirs {run[2][1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
