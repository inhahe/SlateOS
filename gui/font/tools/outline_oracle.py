r"""Check the outlines and boxes this crate draws against HarfBuzz's own, at
a variable font's instances.

    python gui/font/tools/outline_oracle.py FONT [--every N] [--instances SPEC ...]
                                            [--show N] [--no-build]

Needs `uharfbuzz`.

What it compares
----------------

For each glyph (every `--every`th) at each instance -- the default, every
named instance, and any `--instances` given as `tag=value,...` -- the path
`Face::outline_at` draws (through the `outline_dump` example, which needs the
`testing` feature) against the one `hb_font_draw_glyph` draws, and the box
`Face::glyph_extents_at` reports against `hb_font_get_glyph_extents`, both at
one unit per font unit.

The paths are compared coordinate for coordinate, exactly: HarfBuzz's CFF
interpreter works in `double` and hands the pen `float`s, and this crate's
does the same arithmetic in `f64` and keeps `f32`s. Two differences of
spelling, not of shape, are taken out first: HarfBuzz draws the line that
closes a contour back to its start where the contour does not end there
(this crate's `Close` implies it), and emits nothing for a contour with no
segments (this crate may keep its lone `MoveTo`).

Written for `CFF2` (design-decisions §1328), whose `blend`s are where the
arithmetic could part; a `glyf` face works too, though its quadratic contours
may be spelled differently where an implied point starts one.
"""

import argparse
import os
import struct
import subprocess
import sys

import uharfbuzz as hb

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.dirname(HERE)
TARGET = "x86_64-pc-windows-gnu"


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def build(target):
    r = subprocess.run(["cargo", "build", "--release", "--example", "outline_dump", "--features", "testing",
                        "--target", target], cwd=CRATE, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(r.stderr or "outline_dump did not build")


def exe(target):
    examples = os.path.join(CRATE, "..", "..", "target", target, "release", "examples")
    for name in ("outline_dump.exe", "outline_dump"):
        path = os.path.join(examples, name)
        if os.path.exists(path):
            return path
    sys.exit(f"outline_dump not found in {examples}")


class Recorder:
    """A fontTools-style pen that keeps HarfBuzz's calls as tuples."""

    def __init__(self):
        self.ops = []

    def moveTo(self, p):
        self.ops.append(("M", p))

    def lineTo(self, p):
        self.ops.append(("L", p))

    def qCurveTo(self, *points):
        self.ops.append(("Q",) + tuple(points))

    def curveTo(self, *points):
        self.ops.append(("C",) + tuple(points))

    def closePath(self):
        self.ops.append(("Z",))

    def endPath(self):
        self.ops.append(("Z",))


def contours(ops):
    """Split a flat op list into contours: (start, [segments])."""
    out = []
    start, segs = None, []
    for op in ops:
        if op[0] == "M":
            if start is not None:
                out.append((start, segs))
            start, segs = op[1], []
        elif op[0] == "Z":
            if start is not None:
                out.append((start, segs))
            start, segs = None, []
        else:
            segs.append(op)
    if start is not None:
        out.append((start, segs))
    return out


def normalize(cs):
    """Drop empty contours and a closing line back to the start."""
    out = []
    for start, segs in cs:
        if not segs:
            continue
        if segs[-1][0] == "L" and segs[-1][1] == start:
            segs = segs[:-1]
        if segs:
            out.append((start, segs))
    return out


def ours(text):
    toks = text.split()
    ops, i = [], 0
    arity = {"M": 2, "L": 2, "Q": 4, "C": 6, "Z": 0}
    while i < len(toks):
        kind = toks[i]
        n = arity[kind]
        vals = [f32(float(v)) for v in toks[i + 1:i + 1 + n]]
        pts = tuple((vals[k], vals[k + 1]) for k in range(0, n, 2))
        ops.append((kind,) + pts)
        i += 1 + n
    return ops


def theirs(font, gid):
    pen = Recorder()
    font.draw_glyph_with_pen(gid, pen)
    ops = []
    for op in pen.ops:
        ops.append((op[0],) + tuple((f32(x), f32(y)) for x, y in op[1:]))
    return ops


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("font")
    ap.add_argument("--every", type=int, default=1, help="check every Nth glyph")
    ap.add_argument("--instances", nargs="*", default=[], help="more instances, tag=value,...")
    ap.add_argument("--show", type=int, default=8)
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--target", default=TARGET)
    args = ap.parse_args()
    if not args.no_build:
        build(args.target)
    face = hb.Face(hb.Blob.from_file_path(args.font))
    font = hb.Font(face)
    font.scale = (face.upem, face.upem)
    axes = {a.tag: a for a in face.axis_infos} if hasattr(face, "axis_infos") else {}
    specs = [""]
    for inst in face.named_instances:
        tags = [a.tag for a in face.axis_infos]
        specs.append(",".join(f"{t}={f32(v)!r}" for t, v in zip(tags, inst.design_coords)))
    specs += args.instances
    count = face.glyph_count
    lines = []
    # Glyph ranges in chunks keep the dump's output bounded per run.
    for spec in specs:
        cmd = [exe(args.target), args.font, "0", str(count - 1)] + ([spec] if spec else [])
        r = subprocess.run(cmd, capture_output=True, text=True, check=True)
        lines.append((spec, r.stdout.splitlines()))
    paths_bad, boxes_bad, checked = [], [], 0
    for spec, out in lines:
        font.set_variations({})
        if spec:
            font.set_variations({t: float(v) for t, v in (p.split("=") for p in spec.split(","))})
        for line in out:
            _, gid, box, path = line.split("\t")
            gid = int(gid)
            if gid % args.every:
                continue
            checked += 1
            ext = font.get_glyph_extents(gid)
            theirs_box = "-" if ext is None else f"{ext.x_bearing} {ext.y_bearing} {ext.width} {ext.height}"
            if box != theirs_box:
                boxes_bad.append((spec, gid, box, theirs_box))
            if path.startswith("error"):
                paths_bad.append((spec, gid, path, ""))
                continue
            a = normalize(contours(ours(path)))
            b = normalize(contours(theirs(font, gid)))
            if a != b:
                paths_bad.append((spec, gid, a[:2], b[:2]))
    name = os.path.basename(args.font)
    print(f"{name}: {checked} glyph-instances; paths {checked - len(paths_bad)} agree, "
          f"boxes {checked - len(boxes_bad)} agree")
    for spec, gid, a, b in paths_bad[:args.show]:
        print(f"    path {gid} at [{spec or 'default'}]:\n      here     {a}\n      HarfBuzz {b}")
    for spec, gid, a, b in boxes_bad[:args.show]:
        print(f"    box {gid} at [{spec or 'default'}]: here {a}, HarfBuzz {b}")
    return 1 if paths_bad or boxes_bad else 0


if __name__ == "__main__":
    sys.exit(main())
