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

* **the points themselves** -- how many, and which are on the curve -- which
  catch a glyph read into different points: a composite assembled
  differently, a CFF contour closed differently, a line of no length kept.
* **y, exactly**, in 1/64 pixel. Light hinting moves nothing else, and a
  single-unit difference is a different rounding decision somewhere -- the
  thing a port gets wrong.
* **x, exactly**, in 1/64 pixel. Light hinting does not move it, but FreeType
  scales it from whole font units with `FT_MulFix`, and so does this crate: a
  difference is a coordinate read differently (a CFF fraction floored
  differently) or a glyph placed differently (a composite's metrics).

A glyph this crate leaves unhinted is counted apart rather than as a
mismatch: FreeType hints every glyph, including the ideographs and the
fallback style, whose CJK hinting is not ported (see `src/hint/mod.rs`).

Output
------

Per size, how many glyphs agree and what differs in the rest -- the points,
a y, or only an x; then, per style, the same totals, since most gaps are a
style at a time (a script whose hinting is not ported, a feature style); and
the first `--show` disagreements with their points side by side.
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
    """This crate's points for `gids` (every glyph when `None`, which
    `hint_dump` does itself: a large font's ids would overflow a Windows
    command line)."""
    cmd = [exe(target), path, ",".join(str(s) for s in sizes)] + [str(g) for g in gids or []]
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


def compare(pts, ft):
    """What differs between this crate's points and FreeType's: `None`, or
    `"points"`, `"y"` or `"x"`, the first that does."""
    if len(pts) != len(ft) or any(on != fon for (_, _, on), (_, _, fon) in zip(pts, ft)):
        return "points"
    if any(y != fy for (_, y, _), (_, fy, _) in zip(pts, ft)):
        return "y"
    if any(x != fx for (x, _, _), (fx, _, _) in zip(pts, ft)):
        return "x"
    return None


KINDS = ("same", "points", "y", "x", "unhinted", "empty")


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
    asked = [int(g) for g in args.gids.split(",")] if args.gids else None
    table = mine(args.font, sizes, asked, args.target)
    gids = asked if asked is not None else list(range(face.num_glyphs))

    shown = []
    grand = dict.fromkeys(KINDS, 0)
    styles = {}
    for px in sizes:
        tally = dict.fromkeys(KINDS, 0)
        for gid in gids:
            style, nonbase, pts = table[(px, gid)]
            ft = theirs(face, px, gid)
            if not ft:
                kind = "empty"
            elif not pts:
                kind = "unhinted"
            else:
                kind = compare(pts, ft) or "same"
            tally[kind] += 1
            styles.setdefault(style, dict.fromkeys(KINDS, 0))[kind] += 1
            if kind in ("points", "y", "x") and len(shown) < args.show:
                shown.append((px, gid, style, nonbase, pts, ft))
        for k in grand:
            grand[k] += tally[k]
        print(f"{px:5}px  {tally['same']:5} same  differ: {tally['points']:4} points"
              f" {tally['y']:4} y {tally['x']:4} x only"
              f"  {tally['unhinted']:5} unhinted here  {tally['empty']:5} empty")
    print("by style, all sizes:")
    for style, t in sorted(styles.items(), key=lambda kv: -sum(kv[1].values())):
        print(f"  {style:12} {t['same']:6} same  differ: {t['points']:4} points {t['y']:5} y"
              f" {t['x']:4} x only  {t['unhinted']:6} unhinted here  {t['empty']:5} empty")
    for px, gid, style, nonbase, pts, ft in shown:
        print(f"--- {px}px glyph {gid} ({style}{', non-base' if nonbase else ''}):"
              f" {len(pts)} points here, {len(ft)} in FreeType")
        for i, (a, b) in enumerate(zip(pts, ft)):
            mark = "" if a[:2] == (b[0], b[1]) and a[2] == b[2] else "   <--"
            print(f"    {i:3} here ({a[0]:8.2f},{a[1]:6}) {'on ' if a[2] else 'off'}"
                  f"  ft ({b[0]:6},{b[1]:6}) {'on ' if b[2] else 'off'}{mark}")
    hinted = sum(grand[k] for k in ("same", "points", "y", "x"))
    pct = 100.0 * grand["same"] / hinted if hinted else 0.0
    print(f"total: {grand['same']}/{hinted} hinted glyphs agree ({pct:.2f}%),"
          f" {grand['unhinted']} left unhinted here, {grand['empty']} empty")


if __name__ == "__main__":
    main()
