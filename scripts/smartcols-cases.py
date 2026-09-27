#!/usr/bin/env python3
"""Scripts for scripts/smartcols-diff.sh: tables to build, each printed.

    python3 scripts/smartcols-cases.py OUTDIR COUNT [SEED]

writes COUNT scripts, OUTDIR/NNNNN.scols, in the command language that
scripts/scols-probe.c describes. Deterministic for a seed.

Four families, because each part of the port has its own way to go wrong:

  merge     lsblk --merge, simulated: a device graph where some devices have
            several parents (RAID, LVM), each added to the table the way
            lsblk's device_to_scols adds it -- the parents grouped, the
            device linked to the group. This is what the group chart is for.
  groups    the grouping API called at random on a random tree, bad calls
            included: every refusal, and every order of members and children
            the walk can meet -- including the ones upstream aborts on.
  sorttree  a tree sorted by a string or a number column (duplicates
            included, some cells without data), maybe then by tree.
  sortlist  the same on a list, and sorting by tree afterwards, as lsblk's
            --list --sort does.

Each table is printed once, in one of the output modes chosen at random:
human off a terminal, on terminals of several widths, ASCII, JSON, raw,
export, and a few switches.
"""

import random
import sys


def hx(s):
    return s.encode().hex() if s else "-"


class Script:
    def __init__(self):
        self.cmds = []
        self.nlines = 0

    def add(self, *words):
        self.cmds.append(" ".join(str(w) for w in words))

    def line(self, parent):
        self.add("line", -1 if parent is None else parent)
        self.nlines += 1
        return self.nlines - 1

    def text(self):
        return "\n".join(self.cmds) + "\n"


def output_mode(rng, s, tree=True):
    mode = rng.choice(["human", "human", "term", "term", "term", "ascii",
                       "json", "raw", "export", "noheadings", "maxout",
                       "minout", "nowrap"])
    if mode == "term":
        s.add("term", rng.choice([8, 12, 16, 20, 25, 30, 40, 50, 60, 80, 120]))
    elif mode == "ascii":
        s.add("ascii")
        if rng.random() < 0.5:
            s.add("term", rng.choice([20, 40, 80]))
    elif mode == "json":
        s.add("json")
        s.add("name", hx("blockdevices"))
    elif mode in ("maxout", "minout", "nowrap"):
        s.add(mode)
        s.add("term", rng.choice([20, 40, 80]))
    elif mode != "human":
        s.add(mode)
    if rng.random() < 0.2:
        s.add("ascii")


SIZES = ["0B", "1K", "512M", "10G", "10G", "1.8T", "100G", "4M", "931.5G"]


def columns(rng, s, tree, wrap):
    """NAME, MAJ:MIN, SIZE, TYPE and sometimes MOUNTPOINTS: lsblk's own."""
    names = []
    s.add("col", hx("NAME"), "t" if tree else "-", "0.25")
    names.append("NAME")
    s.add("col", hx("MAJ:MIN"), "-", "6")
    names.append("MAJ:MIN")
    s.add("col", hx("SIZE"), "r", "5")
    names.append("SIZE")
    s.add("col", hx("TYPE"), "-", "4")
    names.append("TYPE")
    if wrap:
        s.add("col", hx("MOUNTPOINTS"), "wN", "0.10")
        names.append("MOUNTPOINTS")
    if rng.random() < 0.3:
        s.add("jtype", 2, "number")
    return names


def fill(rng, s, ln, name, kind, ncols, sortcol=None, sortkind=None):
    size = rng.choice(SIZES)
    data = [name, "%d:%d" % (rng.randint(0, 259), rng.randint(0, 64)), size, kind]
    if ncols > 4:
        r = rng.random()
        if r < 0.5:
            data.append("")
        elif r < 0.8:
            data.append(rng.choice(["/", "/boot", "[SWAP]", "/home", "/var/lib/docker"]))
        else:
            data.append("/mnt/a\n/mnt/b" + ("\n/srv/c" if rng.random() < 0.5 else ""))
    for c, d in enumerate(data):
        if d or rng.random() < 0.5:
            s.add("data", ln, c, hx(d) if d else ".")
    if sortcol is not None and sortkind == "u64" and rng.random() < 0.9:
        s.add("udata", ln, sortcol, rng.choice([0, 1, 5, 5, 100, 2 ** 40, 2 ** 64 - 1]))


class Dev:
    def __init__(self, name, kind):
        self.name = name
        self.kind = kind
        self.parents = []
        self.children = []
        self.line = None


def link(p, c):
    p.children.append(c)
    c.parents.append(p)


def merge_case(rng, s):
    tree = True
    wrap = rng.random() < 0.3
    ncols = len(columns(rng, s, tree, wrap))
    sortcol = sortkind = None
    if rng.random() < 0.3:
        sortcol = rng.randint(0, 3)
        sortkind = "u64" if sortcol in (1, 2) else "str"
        s.add("cmp", sortcol, sortkind)
    devs, roots = [], []
    for i in range(rng.randint(1, 4)):
        d = Dev("sd" + "abcd"[i], "disk")
        roots.append(d)
        devs.append(d)
        for p in range(rng.randint(0, 3)):
            c = Dev("%s%d" % (d.name, p + 1), "part")
            link(d, c)
            devs.append(c)
    for m in range(rng.randint(0, 3)):
        k = rng.randint(1, min(4, len(devs)))
        md = Dev("md%d" % m, rng.choice(["raid1", "raid5", "lvm"]))
        for p in rng.sample(devs, k):
            link(p, md)
        devs.append(md)
        for q in range(rng.randint(0, 2)):
            c = Dev("%sp%d" % (md.name, q + 1), "part")
            link(md, c)
            devs.append(c)
    merge = rng.random() < 0.85

    def to_scols(dev, parent, parent_line):
        link_group = False
        if merge and len(dev.parents) > 1:
            if dev.parents[-1] is not parent:
                return
            link_group = True
        ln = s.line(None if link_group else parent_line)
        if link_group:
            for p in dev.parents:
                if p.line is None:
                    continue
                s.add("group", p.line, parent_line)
            s.add("link", ln, parent_line)
        fill(rng, s, ln, dev.name, dev.kind, ncols, sortcol, sortkind)
        dev.line = ln
        for child in dev.children:
            to_scols(child, dev, ln)

    order = roots[:]
    rng.shuffle(order)
    for r in order:
        to_scols(r, None, None)
    if sortcol is not None:
        s.add("sort", sortcol)
    output_mode(rng, s)
    s.add("print")


def groups_case(rng, s):
    wrap = rng.random() < 0.2
    ncols = len(columns(rng, s, True, wrap))
    n = rng.randint(1, 14)
    lines = []
    for i in range(n):
        parent = rng.choice(lines) if lines and rng.random() < 0.5 else None
        ln = s.line(parent)
        lines.append(ln)
        fill(rng, s, ln, "l%d" % i, "x", ncols)
    for _ in range(rng.randint(0, 8)):
        r = rng.random()
        if r < 0.55:
            a = rng.choice(lines + [-1])
            b = rng.choice(lines)
            s.add("group", a, b)
        else:
            s.add("link", rng.choice(lines), rng.choice(lines))
    if rng.random() < 0.3:
        c = rng.randint(0, 3)
        s.add("cmp", c, "str")
        s.add("sort", c)
    output_mode(rng, s)
    s.add("print")


def sort_case(rng, s, tree):
    wrap = rng.random() < 0.2
    ncols = len(columns(rng, s, tree, wrap))
    sortcol = rng.randint(0, 3)
    sortkind = rng.choice(["str", "u64"])
    set_cmp = rng.random() < 0.9
    if set_cmp:
        s.add("cmp", sortcol, sortkind)
    n = rng.randint(0, 16)
    lines = []
    for i in range(n):
        parent = rng.choice(lines) if tree and lines and rng.random() < 0.6 else None
        if not tree and lines and rng.random() < 0.4:
            # A list still has parents: lsblk --list builds the same tree
            # and prints it flat.
            parent = rng.choice(lines)
        ln = s.line(parent)
        lines.append(ln)
        name = rng.choice(["sda", "sdb", "sda1", "loop0", "nvme0n1", "md0", "dm-0", "sr0", ""])
        fill(rng, s, ln, name, rng.choice(["disk", "part", "loop", "rom"]), ncols,
             sortcol, sortkind)
    r = rng.random()
    if r < 0.6:
        s.add("sort", sortcol)
    elif r < 0.7:
        s.add("sort", -1)
    elif r < 0.8:
        s.add("sort", sortcol)
        s.add("sort", -1)
    if not tree and rng.random() < 0.6 or tree and rng.random() < 0.2:
        s.add("sorttree")
    output_mode(rng, s, tree)
    s.add("print")


def main():
    outdir, count = sys.argv[1], int(sys.argv[2])
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    for i in range(count):
        s = Script()
        family = ["merge", "merge", "groups", "sorttree", "sortlist"][i % 5]
        if family == "merge":
            merge_case(rng, s)
        elif family == "groups":
            groups_case(rng, s)
        elif family == "sorttree":
            sort_case(rng, s, True)
        else:
            sort_case(rng, s, False)
        with open("%s/%05d-%s.scols" % (outdir, i, family), "w") as f:
            f.write(s.text())


if __name__ == "__main__":
    main()
