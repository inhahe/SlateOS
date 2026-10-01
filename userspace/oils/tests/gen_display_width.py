#!/usr/bin/env python3
"""Measure osh's display widths against the reference bash.

`osh`'s `display_width` has to reproduce bash's `displen` (execute_cmd.c),
which is `wcswidth` of the decoded string -- except for the width of each
character, which comes from `charwidth`'s table. That table is SlateOS's, not
bash's: `scripts/charwidth-gen.py` builds it from the Unicode 18.0 data with
the policy of design-decisions §1042, and bash measures with glibc's. So this
no longer derives or emits anything; it measures where the two differ.

    python gen_display_width.py --check        # bash's width at every range edge
    python gen_display_width.py --diff-osh PATH  # osh's menu bytes against bash's

`--check` reports every boundary code point where bash's width is not the
table's -- the deliberate differences §1042 lists (the soft hyphen, the
prepended concatenation marks, Hangul Jamo Extended-B, characters newer than
the host's glibc) and anything else, which would be news.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import sys
import unicodedata as ud

HERE = os.path.dirname(os.path.abspath(__file__))
GEN = os.path.join(HERE, "..", "..", "..", "scripts", "charwidth-gen.py")


def table_widths() -> list[int]:
    """Every code point's width as `charwidth` gives it (-1 for none)."""
    spec = importlib.util.spec_from_file_location("charwidth_gen", GEN)
    if spec is None or spec.loader is None:
        sys.exit(f"cannot load {GEN}")
    gen = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gen)
    return gen.widths()


def boundaries(widths: list[int]) -> list[int]:
    """The code points on either side of every change of width."""
    out = []
    for cp in range(1, len(widths)):
        if widths[cp] != widths[cp - 1]:
            out += [cp - 1, cp]
    return sorted(set(out))


def probeable(cp: int) -> bool:
    """A code point the `select` probe can carry through the shell."""
    if 0xD800 <= cp <= 0xDFFF:  # surrogates: not encodable
        return False
    if ud.category(chr(cp)) == "Cc":  # control: measures the strlen fallback
        return False
    return cp not in (0x00, 0x0A, 0x27)  # NUL, newline, the quote we wrap with


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
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
    if not (a.check or a.diff_osh):
        ap.error("give --check or --diff-osh PATH")

    import probe_displen as probe

    widths = table_widths()
    cps = [cp for cp in boundaries(widths) if probeable(cp)]
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
                print(f"U+{cp:04X}\tbash {ref!r}\tosh {mine!r}")
        else:
            want = widths[cp]
            got = probe.measure(a.shell, a.locale, chr(cp))
            if got != [want]:
                bad += 1
                print(f"U+{cp:04X}\ttable {want}\tbash {got}")
        if i % 200 == 199:
            print(f"  … {i + 1}/{len(cps)}, {bad} differences", file=sys.stderr)
    print(f"{len(cps)} boundary code points, {bad} differences", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
