#!/usr/bin/env python3
"""Compare the Rust port's decision trace with libvpx's (README.md beside
this file has the procedure).

Usage: python compare_traces.py libvpx.trace rust.trace [--context N] [--all N]

Walks both traces line by line and reports the first line that differs,
with the frame and superblock it falls in and the lines before it. Values
written as floats (`lca=`) are compared as numbers, since C's %.17g and
Rust's shortest round-trip form spell the same double differently.
With --all N, lists the first N differing lines (after the first, later
differences are usually consequences).
"""
import sys


def norm(line):
    out = []
    for tok in line.split():
        if tok.startswith("lca="):
            try:
                tok = "lca=" + repr(float(tok[4:]))
            except ValueError:
                pass
        out.append(tok)
    return " ".join(out)


def main():
    args = sys.argv[1:]
    ctx = 12
    show_all = 0
    if "--context" in args:
        i = args.index("--context")
        ctx = int(args[i + 1])
        del args[i:i + 2]
    if "--all" in args:
        i = args.index("--all")
        show_all = int(args[i + 1])
        del args[i:i + 2]
    ref = open(args[0], encoding="utf-8").read().splitlines()
    got = open(args[1], encoding="utf-8").read().splitlines()
    # libvpx logs its key frames' superblocks too (L and S lines); the port
    # logs no key frame decisions, which are checked byte for byte anyway.
    kept = []
    in_key = False
    for line in ref:
        if line.startswith("F "):
            in_key = " kf=1 " in line
        if in_key and not (line.startswith("F ") or line.startswith("E ")):
            continue
        kept.append(line)
    ref = kept
    frame = sb = None
    shown = 0
    for n, (a, b) in enumerate(zip(ref, got)):
        if a.startswith("F "):
            frame = a
        if a.startswith("S "):
            sb = a
        if norm(a) != norm(b):
            print(f"first difference at line {n + 1}")
            print(f"  frame: {frame}")
            print(f"  superblock: {sb}")
            print("  context (libvpx):")
            for line in ref[max(0, n - ctx):n]:
                print("    " + line)
            print(f"  libvpx: {a}")
            print(f"  rust:   {b}")
            shown += 1
            if shown >= max(1, show_all):
                return 1
    if len(ref) != len(got):
        print(f"one trace is longer: libvpx {len(ref)} lines, rust {len(got)}")
        return 1
    print(f"identical: {len(ref)} lines")
    return 0


if __name__ == "__main__":
    sys.exit(main())
