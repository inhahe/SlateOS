r"""Check this crate's normalization of variable-font instances against
HarfBuzz's and FreeType's own, over a grid of instances of real faces.

    python gui/font/tools/var_oracle.py FONT|DIR ... [--steps N] [--show N] [--no-build]

Needs `uharfbuzz` and `freetype-py`.

What it compares
----------------

A variable font's instance -- weight 700, width 87.5 -- reaches every
variation table as *normalized* coordinates, and the two libraries this
crate follows compute them differently: HarfBuzz in `F2Dot14` by way of
16.16 floats, FreeType in 16.16 integers (see `src/var.rs`). This crate
computes both. The script builds the `var_dump` example (which needs the
`testing` feature), runs it over a grid of instances of each face -- every
axis from end to end, alone, with points between the round ones and a hair
from each end and from the default, then combinations of a few values of
each axis together -- and compares every coordinate with what the libraries
report (`hb_font_get_var_coords_normalized`,
`FT_Get_Var_Blend_Coordinates`). An instance FreeType refuses (a coordinate
past +/-1) is counted apart, since FreeType gives no answer to compare with.

`tests` in `src/var.rs` hold the crate to both libraries on faces built to
reach each rule (`tools/gen_var_fixture.py`). This is the other half: real
faces, at far more instances than a fixture carries. A clean run is the
expected result; a disagreement is a real one.
"""

import argparse
import itertools
import os
import struct
import subprocess
import sys

import freetype
import uharfbuzz as hb

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.dirname(HERE)
TARGET = "x86_64-pc-windows-gnu"


def f32(x):
    """`x` rounded to the nearest `f32`: every value is handed to all three
    readers as the same number."""
    return struct.unpack("<f", struct.pack("<f", x))[0]


def build(target):
    r = subprocess.run(["cargo", "build", "--release", "--example", "var_dump", "--features", "testing",
                        "--target", target], cwd=CRATE, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(r.stderr or "var_dump did not build")


def exe(target):
    examples = os.path.join(CRATE, "..", "..", "target", target, "release", "examples")
    for name in ("var_dump.exe", "var_dump"):
        path = os.path.join(examples, name)
        if os.path.exists(path):
            return path
    sys.exit(f"var_dump not found in {examples}")


def axis_values(lo, default, hi, steps):
    vs = {lo, default, hi}
    for k in range(1, steps + 1):
        vs.add(lo + (default - lo) * k / (steps + 1))
        vs.add(default + (hi - default) * k / (steps + 1))
    # Off any round number, and a hair from each end and from the default.
    for frac in (0.1234567, 0.3141593, 0.4999, 0.5001, 0.6180339, 0.7071068, 0.9999):
        vs.add(lo + (hi - lo) * frac)
    for v in (default + 0.001, default - 0.001, hi - 0.001, lo + 0.001):
        vs.add(v)
    return sorted({f32(v) for v in vs if lo <= v <= hi})


def grid(axes, steps):
    """Instances as {tag: value}: each axis's values alone, then every
    combination of a few of each."""
    values = {a.tag: axis_values(a.minimum, a.default, a.maximum, steps) for a in axes}
    out = [{tag: v} for tag, vs in values.items() for v in vs]
    few = {tag: vs[:: max(1, len(vs) // 5)] for tag, vs in values.items()}
    if len(few) > 1:
        out += [dict(zip(few, combo)) for combo in itertools.product(*few.values())][:4000]
    return out


def check(path, runner, steps, show):
    try:
        ft = freetype.Face(path)
        info = ft.get_variation_info()
    except Exception:  # noqa: BLE001 -- not a variable face FreeType reads
        return None
    axes = info.axes
    if not axes:
        return None
    instances = grid(axes, steps)
    specs = [",".join(f"{tag}={v!r}" for tag, v in inst.items()) for inst in instances]
    lines = []
    for i in range(0, len(specs), 400):
        r = subprocess.run([runner, path] + specs[i:i + 400], capture_output=True, text=True)
        if r.returncode != 0:
            print(f"{os.path.basename(path)}: var_dump failed: {r.stderr.strip()[:200]}")
            return None
        lines += r.stdout.splitlines()
    font = hb.Font(hb.Face(hb.Blob.from_file_path(path)))
    bad = {"HarfBuzz": [], "FreeType": []}
    refused = 0
    for inst, line in zip(instances, lines):
        norm, fixed = line.split("|")
        mine_hb = [int(v) for v in norm.split()]
        mine_ft = [int(v) for v in fixed.split()]
        font.set_variations(dict(inst))
        theirs_hb = [round(c * 16384) for c in font.get_var_coords_normalized()]
        if mine_hb != theirs_hb:
            bad["HarfBuzz"].append((inst, mine_hb, theirs_hb))
        try:
            ft.set_var_design_coords([inst.get(a.tag, a.default) for a in axes])
        except freetype.FT_Exception:
            refused += 1
            continue
        theirs_ft = [round(c * 65536) for c in ft.get_var_blend_coords()]
        if mine_ft != theirs_ft:
            bad["FreeType"].append((inst, mine_ft, theirs_ft))
    name = os.path.basename(path)
    tags = "/".join(a.tag for a in axes)
    note = f", {refused} refused by FreeType" if refused else ""
    print(f"{name} ({tags}): {len(instances)} instances; HarfBuzz {len(instances) - len(bad['HarfBuzz'])} agree, "
          f"FreeType {len(instances) - refused - len(bad['FreeType'])} agree{note}")
    for lib, rows in bad.items():
        for inst, mine, theirs in rows[:show]:
            print(f"    {lib} {inst}: here {mine}, {lib} {theirs}")
    return sum(len(rows) for rows in bad.values())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="+", help="font files, or directories of them")
    ap.add_argument("--steps", type=int, default=7, help="points between each axis end and its default")
    ap.add_argument("--show", type=int, default=6, help="disagreements to print per face and library")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--target", default=TARGET)
    args = ap.parse_args()
    if not args.no_build:
        build(args.target)
    runner = exe(args.target)
    files = []
    for p in args.paths:
        if os.path.isdir(p):
            files += sorted(os.path.join(p, f) for f in os.listdir(p)
                            if f.lower().endswith((".ttf", ".otf")))
        else:
            files.append(p)
    faces = disagreements = 0
    for path in files:
        got = check(path, runner, args.steps, args.show)
        if got is not None:
            faces += 1
            disagreements += got
    print(f"{faces} variable faces, {disagreements} disagreements")
    return 1 if disagreements else 0


if __name__ == "__main__":
    sys.exit(main())
