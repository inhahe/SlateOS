#!/usr/bin/env python3
"""Derive osh's display-width table, and check it against the reference bash.

`osh`'s `display_width` has to reproduce bash's `displen` (execute_cmd.c),
which is `wcswidth` of the decoded string. The rule below is the usual
`wcwidth` one — zero for combining marks, format characters and the Hangul
Jamo medial/final blocks, two for East Asian Wide and Fullwidth, one for
everything else — and `--check` measures the reference shell at every range
boundary the rule produces, so the table is verified against the shell rather
than assumed from the standard.

    python gen_display_width.py --check          # measure, report disagreements
    python gen_display_width.py --emit > width.rs
"""

from __future__ import annotations

import argparse
import sys
import unicodedata as ud

MAX = 0x110000
# Hangul Jamo medial vowels and final consonants combine with the leading
# consonant before them, so they occupy no column of their own. They are `Lo`,
# not marks, so no general-category rule catches them; every wcwidth carries
# this range as a special case.
JAMO_ZERO = range(0x1160, 0x1200)
# The soft hyphen is `Cf`, but is the one format character that is printed.
SOFT_HYPHEN = 0x00AD


def width_of(cp: int) -> int:
    c = chr(cp)
    cat = ud.category(c)
    if cat in ("Mn", "Me") or (cat == "Cf" and cp != SOFT_HYPHEN) or cp in JAMO_ZERO:
        return 0
    if ud.east_asian_width(c) in ("W", "F"):
        return 2
    return 1


def ranges(width: int) -> list[tuple[int, int]]:
    out: list[list[int]] = []
    for cp in range(MAX):
        if width_of(cp) != width:
            continue
        if out and out[-1][1] + 1 == cp:
            out[-1][1] = cp
        else:
            out.append([cp, cp])
    return [(a, b) for a, b in out]


def boundaries(rs: list[tuple[int, int]]) -> list[int]:
    """The code points a wrong edge would show up at."""
    seen = []
    for lo, hi in rs:
        for cp in (lo - 1, lo, hi, hi + 1):
            if 0 <= cp < MAX:
                seen.append(cp)
    return seen


def probeable(cp: int) -> bool:
    """A code point the `select` probe can carry through the shell."""
    if 0xD800 <= cp <= 0xDFFF:  # surrogates: not encodable
        return False
    if ud.category(chr(cp)) == "Cc":  # control: measures the strlen fallback
        return False
    return cp not in (0x00, 0x0A, 0x27)  # NUL, newline, the quote we wrap with


def emit(zero: list[tuple[int, int]], wide: list[tuple[int, int]]) -> str:
    def table(name: str, rs: list[tuple[int, int]], doc: str) -> str:
        rows = ",\n".join(f"    (0x{a:04X}, 0x{b:04X})" for a, b in rs)
        head = "\n".join(f"/// {line}".rstrip() for line in doc.splitlines())
        return f"{head}\nstatic {name}: [(u32, u32); {len(rs)}] = [\n{rows},\n];\n"

    return (
        table(
            "ZERO_WIDTH",
            zero,
            "Code points that occupy no terminal column: the combining marks\n"
            "(`Mn`/`Me`), the format characters (`Cf`) other than the soft\n"
            "hyphen, and the Hangul Jamo medial vowels and final consonants.",
        )
        + "\n"
        + table(
            "WIDE",
            wide,
            "Code points that occupy two terminal columns: East Asian Wide and\n"
            "Fullwidth (UAX #11).",
        )
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--emit", action="store_true")
    ap.add_argument("--shell", default="C:/Program Files/Git/usr/bin/bash.exe")
    ap.add_argument("--locale", default="C.UTF-8")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument(
        "--diff-osh",
        metavar="PATH",
        help="compare osh's menu bytes with bash's at every boundary, "
        "instead of inferring the width bash used",
    )
    a = ap.parse_args()

    zero = ranges(0)
    wide = ranges(2)
    print(
        f"zero-width: {len(zero)} ranges; wide: {len(wide)} ranges",
        file=sys.stderr,
    )

    if a.emit:
        sys.stdout.write(emit(zero, wide))

    if a.check:
        import probe_displen as probe

        cps = sorted({cp for cp in boundaries(zero) + boundaries(wide) if probeable(cp)})
        if a.limit:
            cps = cps[:: max(1, len(cps) // a.limit)]
        bad = 0
        for i, cp in enumerate(cps):
            if a.diff_osh:
                # The end-to-end test: not "what width did bash use" but "does
                # osh lay the menu out byte for byte as bash does".
                ref, mine = (
                    probe.measure_one(sh, a.locale, chr(cp), probe.FILLERS[0])[1]
                    for sh in (a.shell, a.diff_osh)
                )
                if ref != mine:
                    bad += 1
                    print(f"U+{cp:04X}\tbash {ref!r}\tosh {mine!r}\t{ud.category(chr(cp))}")
            else:
                want = width_of(cp)
                got = probe.measure(a.shell, a.locale, chr(cp))
                if got != [want]:
                    bad += 1
                    print(
                        f"U+{cp:04X}\tpredicted {want}\tbash {got}\t{ud.category(chr(cp))}"
                    )
            if i % 200 == 199:
                print(f"  … {i + 1}/{len(cps)}, {bad} disagreements", file=sys.stderr)
        print(f"{len(cps)} boundary code points, {bad} disagreements", file=sys.stderr)
        return 1 if bad else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
