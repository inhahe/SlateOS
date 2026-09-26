r"""Check the auto-hinter in `gui/font/src/hint/` against FreeType's own.

    python gui/font/tools/hint_oracle.py FONT [--sizes 9,10,...] [--gids 1,2,...]
                                              [--show N] [--no-build]

Needs `freetype-py` (`pip install freetype-py`), whose wheels bundle FreeType
2.13.2 built with HarfBuzz -- the release and configuration the port follows.

What it compares
----------------

The hinter is a port of FreeType's auto-hinter in its light mode, so for every
glyph at every size the two must put every stored point in the same place.
This builds and runs the `hint_dump` example, loads the same glyphs through
FreeType with `FT_LOAD_FORCE_AUTOHINT | FT_LOAD_TARGET_LIGHT`, and compares:

* **y, exactly**, in 1/64 pixel. Light hinting moves nothing else, and a
  single-unit difference is a different rounding decision somewhere -- the
  thing a port gets wrong.
* **x, within 1/64 pixel**: FreeType rounds each scaled x to 1/64, this crate
  keeps it exact, and neither is hinted.
* **the point count and on-curve flags**, which catch a glyph read into
  different points (a composite assembled differently, a CFF contour closed
  differently).

A glyph this crate leaves unhinted is counted apart rather than as a
mismatch: FreeType hints every glyph, including the ideographs and the
fallback style, whose CJK hinting is not ported (see `src/hint/mod.rs`).

Output
------

Per size, how many glyphs agree; then the first `--show` disagreements with
their points side by side.
"""

import argparse
import os
import subprocess
import sys

import freetype

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.dirname(HERE)


# The workspace's host target on the development machine; see build-env.md.
TARGET = "x86_64-pc-windows-gnu"


def build(target):
    r = subprocess.run(
        ["cargo", "build", "--release", "--example", "hint_dump", "--target", target],
        cwd=CRATE,
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        sys.exit(r.stderr or "hint_dump did not build")


def exe(target):
    examples = os.path.join(CRATE, "..", "..", "target", target, "release", "examples")
    for name in ("hint_dump.exe", "hint_dump"):
        path = os.path.join(examples, name)
        if os.path.exists(path):
            return path
    sys.exit(f"hint_dump not found in {examples}")


def mine(path, sizes, gids, target):
    cmd = [exe(target), path, ",".join(str(s) for s in sizes)] + [str(g) for g in gids]
    out = subprocess.run(cmd, capture_output=True, text=True, check=True).stdout
    table = {}
    for line in out.splitlines():
        parts = line.split()
        px, gid, style, nonbase, n = float(parts[0]), int(parts[1]), parts[2], parts[3], int(parts[4])
        pts = []
        for p in parts[5:5 + n]:
            x, y, on = p.split(",")
            pts.append((float(x), int(float(y)), on == "1"))
        table[(px, gid)] = (style, nonbase == "1", pts)
    return table


def theirs(face, px, gid):
    face.set_char_size(0, int(round(px * 64)), 72, 72)
    flags = freetype.FT_LOAD_NO_BITMAP | freetype.FT_LOAD_FORCE_AUTOHINT | freetype.FT_LOAD_TARGET_LIGHT
    face.load_glyph(gid, flags)
    o = face.glyph.outline
    return [(x, y, (t & 1) == 1) for (x, y), t in zip(o.points, o.tags)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("font")
    ap.add_argument("--sizes", default="9,10,11,12,13,14,15,16,18,20,24")
    ap.add_argument("--gids", default="")
    ap.add_argument("--show", type=int, default=5)
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--target", default=TARGET)
    args = ap.parse_args()
    if not args.no_build:
        build(args.target)
    sizes = [float(s) for s in args.sizes.split(",")]
    face = freetype.Face(args.font)
    gids = [int(g) for g in args.gids.split(",")] if args.gids else list(range(face.num_glyphs))
    table = mine(args.font, sizes, gids, args.target)

    shown = 0
    grand = {"same": 0, "differ": 0, "unhinted": 0, "empty": 0}
    for px in sizes:
        tally = dict.fromkeys(grand, 0)
        for gid in gids:
            style, nonbase, pts = table[(px, gid)]
            ft = theirs(face, px, gid)
            if not ft:
                tally["empty"] += 1
                continue
            if not pts:
                tally["unhinted"] += 1
                continue
            same = len(pts) == len(ft) and all(
                y == fy and on == fon and abs(x - fx) <= 1.0
                for (x, y, on), (fx, fy, fon) in zip(pts, ft)
            )
            tally["same" if same else "differ"] += 1
            if not same and shown < args.show:
                shown += 1
                print(f"--- {px}px glyph {gid} ({style}{', non-base' if nonbase else ''}):"
                      f" {len(pts)} points here, {len(ft)} in FreeType")
                for i, (a, b) in enumerate(zip(pts, ft)):
                    mark = "" if a[1] == b[1] and abs(a[0] - b[0]) <= 1.0 else "   <--"
                    print(f"    {i:3} here ({a[0]:8.2f},{a[1]:6}) {'on ' if a[2] else 'off'}"
                          f"  ft ({b[0]:6},{b[1]:6}) {'on ' if b[2] else 'off'}{mark}")
        for k in grand:
            grand[k] += tally[k]
        print(f"{px:5}px  {tally['same']:5} same  {tally['differ']:5} differ"
              f"  {tally['unhinted']:5} unhinted here  {tally['empty']:5} empty")
    hinted = grand["same"] + grand["differ"]
    pct = 100.0 * grand["same"] / hinted if hinted else 0.0
    print(f"total: {grand['same']}/{hinted} hinted glyphs agree ({pct:.2f}%),"
          f" {grand['unhinted']} left unhinted here, {grand['empty']} empty")


if __name__ == "__main__":
    main()
