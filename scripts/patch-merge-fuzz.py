#!/usr/bin/env python3
"""Random `patch --merge` cases: ours against GNU patch, case by case.

`scripts/patch-diff.sh` runs this as its last case. Merging is the one part
of `patch` whose answer depends on an edit-distance search (gnulib's
`compareseq`, patch's `bestmatch`), and a handful of hand-made fixtures
samples that search too thinly: a slip in a diagonal's bookkeeping shows only
on the inputs that walk through it. So this makes a few hundred: a random
file, two random edits of it -- one becomes the patch, written by GNU `diff`,
the other the file the patch is applied to -- applied by both binaries under
`--merge`, `--merge=diff3` and `-m`, with `-x 2` so that each side also dumps
the search's result (the old lines it marked deleted and the file lines it
marked inserted). Output, standard error, exit status and the merged file must
all agree.

The cases are seeded, so a failure reproduces: the seed and the case number
are printed with the first few disagreements.

Usage: patch-merge-fuzz.py OURS GNU [COUNT [SEED [MAXLINES [MAXEDITS]]]]
Exits 0 when every case agrees, 1 when any differ, 2 on bad usage.
"""

import os
import random
import shutil
import subprocess
import sys
import tempfile

# A small alphabet, so that lines repeat and the search has choices to make.
ALPHA = ["a", "b", "c", "d", "e", "f", "g"]


def mutate(rng, lines, k):
    """`lines` with `k` random insertions, deletions and changes."""
    lines = list(lines)
    for _ in range(k):
        op = rng.choice("idc")
        i = rng.randrange(len(lines) + 1)
        if op == "i" or not lines:
            lines.insert(i, rng.choice(ALPHA) + rng.choice(["", "x", "y"]))
        elif op == "d" and i < len(lines):
            del lines[i]
        elif i < len(lines):
            lines[i] = rng.choice(ALPHA) + "z"
    return lines


def write(path, lines):
    with open(path, "w", encoding="ascii", newline="") as fh:
        fh.write("".join(line + "\n" for line in lines))


def run(binary, d, args, patch):
    p = subprocess.run([binary] + args, cwd=d, input=patch, capture_output=True, timeout=60, check=False)
    with open(os.path.join(d, "f"), "rb") as fh:
        content = fh.read()
    return p.returncode, p.stdout, p.stderr, content


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip().splitlines()[-2], file=sys.stderr)
        return 2
    ours, gnu = sys.argv[1], sys.argv[2]
    count = int(sys.argv[3]) if len(sys.argv) > 3 else 300
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    maxlines = int(sys.argv[5]) if len(sys.argv) > 5 else 60
    maxedits = int(sys.argv[6]) if len(sys.argv) > 6 else 12
    rng = random.Random(seed)
    differ = 0
    ran = 0
    for case in range(count):
        base = [rng.choice(ALPHA) for _ in range(rng.randint(3, maxlines))]
        new = mutate(rng, base, rng.randint(1, maxedits))
        target = mutate(rng, base, rng.randint(1, maxedits))
        context = rng.choice(["-U0", "-U1", "-U2", "-U3"])
        style = rng.choice([["--merge"], ["--merge=diff3"], ["-m"]])
        tmp = tempfile.mkdtemp()
        try:
            write(os.path.join(tmp, "f"), base)
            write(os.path.join(tmp, "f.new"), new)
            patch = subprocess.run(
                ["diff", context, "--label", "f", "--label", "f", "f", "f.new"],
                cwd=tmp, capture_output=True, check=False,
            ).stdout
            if not patch:
                continue
            ran += 1
            args = ["-p0", "-x", "2", "--no-backup-if-mismatch"] + style
            results = []
            for name, binary in (("ours", ours), ("gnu", gnu)):
                d = os.path.join(tmp, name)
                os.mkdir(d)
                write(os.path.join(d, "f"), target)
                results.append(run(binary, d, args, patch))
            if results[0] != results[1]:
                differ += 1
                if differ <= 3:
                    print(f"DIFF seed {seed} case {case}: patch {' '.join(args)}")
                    print(patch.decode(errors="replace"), end="")
                    print("target:", " ".join(target))
                    for name, r in zip(("ours", "gnu"), results):
                        print(f"-- {name}: rc={r[0]}")
                        print("   stdout:", r[1].decode(errors="replace").replace("\n", "|"))
                        print("   stderr:", r[2].decode(errors="replace").replace("\n", "|"))
                        print("   file:  ", r[3].decode(errors="replace").replace("\n", "|"))
        finally:
            shutil.rmtree(tmp)
    print(f"{ran} merges, {differ} differ")
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
