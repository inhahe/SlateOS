#!/usr/bin/env python3
"""Measure the reference bash's `displen` (execute_cmd.c) for arbitrary strings.

bash never prints a display width, so this recovers it from the one place the
width is observable: the column arithmetic of a `select` menu. The menu is laid
out by `print_select_list`, which is simulated here exactly; the probe runs a
four-item menu whose second item is the string under test, then reports the
`displen` values that would reproduce bash's bytes. When exactly one value
does, that is the measurement.

Usage:  python probe_displen.py [--shell PATH] [--locale LOC] STRING...
        python probe_displen.py --codepoints 0x4e00 0x300 ...
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys

TABSIZE = 8
RP_SPACE_LEN = 2


def number_len(n: int) -> int:
    """bash's NUMBER_LEN macro."""
    for i, bound in enumerate((10, 100, 1000, 10000, 100000), start=1):
        if n < bound:
            return i
    return 6


def indent(frm: int, to: int) -> str:
    """bash's `indent` (execute_cmd.c)."""
    out = []
    while frm < to:
        if to // TABSIZE > frm // TABSIZE:
            out.append("\t")
            frm += TABSIZE - frm % TABSIZE
        else:
            out.append(" ")
            frm += 1
    return "".join(out)


def render(items: list[str], widths: list[int], cols_env: int) -> str:
    """bash's `print_select_list`, given each item's `displen`."""
    list_len = len(items)
    indices_len = number_len(list_len)
    max_elem_len = max(widths) + indices_len + RP_SPACE_LEN + 2

    cols = cols_env // max_elem_len if max_elem_len else 1
    cols = cols or 1
    rows = (list_len // cols + (list_len % cols != 0)) if list_len else 1
    cols = (list_len // rows + (list_len % rows != 0)) if list_len else 1
    if rows == 1:
        rows, cols = cols, 1

    first_col_indices_len = number_len(rows)
    out = []
    for row in range(rows):
        ind = row
        pos = 0
        while True:
            il = first_col_indices_len if pos == 0 else indices_len
            out.append(f"{ind + 1:>{il}}) {items[ind]}")
            elem_len = widths[ind] + il + RP_SPACE_LEN
            ind += rows
            if ind >= list_len:
                break
            out.append(indent(pos + elem_len, pos + max_elem_len))
            pos += max_elem_len
        out.append("\n")
    return "".join(out)


# The padding between two menu columns is emitted with tabs, so a single menu
# cannot tell apart two widths that land in the same tab block. Each filler
# width shifts where the blocks fall, and the intersection over several of them
# is unique for every width the probe has been asked for so far.
FILLERS = (2, 20, 21, 22, 23, 24)


def measure_one(
    shell: str, locale: str, subject: str, filler: int
) -> tuple[list[int], str]:
    """The `displen` values consistent with one menu, and that menu's bytes."""
    items = ["x" * filler, subject, "b", "c"]
    script = "select o in " + " ".join(f"'{i}'" for i in items) + "; do break; done"
    cols_env = 2 * (filler + 5) + 1
    env = dict(os.environ, LC_ALL=locale, COLUMNS=str(cols_env))
    got = subprocess.run(
        [shell, "--norc", "-c", script],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        env=env,
    ).stderr.decode("utf-8", "surrogateescape")
    got = got.split("#? ")[0]

    # 0 is bash's answer for a string that will not decode at all; the byte
    # length is its answer when the decode succeeds but `wcswidth` refuses.
    hi = len(subject.encode("utf-8", "surrogateescape"))
    hits = [
        d for d in range(0, hi + 1) if render(items, [filler, d, 1, 1], cols_env) == got
    ]
    return hits, got


def measure(shell: str, locale: str, subject: str) -> list[int]:
    """Return every `displen` for `subject` that reproduces bash's menus."""
    hits: set[int] | None = None
    for filler in FILLERS:
        one, _ = measure_one(shell, locale, subject, filler)
        hits = set(one) if hits is None else hits & set(one)
        if len(hits) == 1:
            break
    return sorted(hits or ())


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--shell", default="C:/Program Files/Git/usr/bin/bash.exe")
    ap.add_argument("--locale", default="C.UTF-8")
    ap.add_argument("--codepoints", action="store_true")
    ap.add_argument("subject", nargs="+")
    a = ap.parse_args()

    for s in a.subject:
        subject = chr(int(s, 0)) if a.codepoints else s
        hits = measure(a.shell, a.locale, subject)
        label = f"U+{ord(subject):04X}" if len(subject) == 1 else repr(subject)
        print(f"{label}\t{hits if len(hits) != 1 else hits[0]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
