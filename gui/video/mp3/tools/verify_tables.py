"""Check the properties of minimp3's tables that its decoder relies on, and
that the Rust translation indexes by: every scale-factor band table covers
576 lines before its terminator; every Huffman lookup a bit pattern can reach
stays inside its table; no subtable is reached with a width of 0; and the
longest codeword is short enough for the bit cache.

    python tools/verify_tables.py <minimp3>/minimp3.h

The indexing in `src/l3.rs` is bounded by these properties (its allow of
`clippy::indexing_slicing` cites this), so run it if a table ever changes."""
import re
import sys

src = open(sys.argv[1], encoding="utf-8").read()


def braces(text, i):
    depth = 0
    for j in range(i, len(text)):
        if text[j] == "{":
            depth += 1
        elif text[j] == "}":
            depth -= 1
            if depth == 0:
                return text[i + 1:j], j
    sys.exit("unbalanced")


def parse(body, dims):
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    if len(dims) == 1:
        values = [v.strip() for v in body.split(",") if v.strip()]
        if dims[0] is not None:
            values += ["0"] * (dims[0] - len(values))
        return values
    out = []
    i = 0
    rows = 0
    while True:
        k = body.find("{", i)
        if k < 0:
            break
        inner, end = braces(body, k)
        out += parse(inner, dims[1:])
        rows += 1
        i = end + 1
    if dims[0] is not None:
        size = 1
        for d in dims[1:]:
            size *= d
        out += ["0"] * ((dims[0] - rows) * size)
    return out


def table(name):
    m = re.search(r"static const (\w+) " + re.escape(name) + r"((?:\[[^\]]*\])+)\s*=\s*\{", src)
    dims = [eval(d) if d.strip() else None for d in re.findall(r"\[([^\]]*)\]", m.group(2))]
    body, _ = braces(src, m.end() - 1)
    return dims, [int(v) for v in parse(body, dims)]


ok = True
for name, width in (("g_scf_long", 23), ("g_scf_short", 40), ("g_scf_mixed", 40)):
    _, v = table(name)
    for r in range(8):
        row = v[r * width:(r + 1) * width]
        n = row.index(0)
        s = sum(row[:n])
        if s != 576:
            print(f"{name}[{r}] sums to {s} over {n} bands")
            ok = False
        else:
            print(f"{name}[{r}]: {n} bands, 576 lines")

_, tabs = table("tabs")
_, tabindex = table("tabindex")
_, linbits = table("g_linbits")
starts = sorted(set(tabindex))
longest = 0
for t in range(32):
    base = tabindex[t]
    end = min([s for s in starts if s > base] + [len(tabs)])
    if base == 0:
        # Tables 0, 4 and 14 (the last two do not exist in the standard):
        # 32 zeros, every pair (0, 0) in no bits.
        assert all(x == 0 for x in tabs[0:32]), "table 0 is not all zeros"
        print(f"table {t}: zeros, no bits")
        continue
    # Walk every reachable lookup: (offset into the table, width, bits so far).
    stack = [(0, 5, 0)]
    seen = set()
    while stack:
        off, w, depth = stack.pop()
        if (off, w) in seen:
            continue
        seen.add((off, w))
        if w == 0:
            print(f"table {t}: a subtable of width 0 at {off}")
            ok = False
            continue
        for bits in range(1 << w):
            i = off + bits
            if not (0 <= base + i < end):
                print(f"table {t}: lookup {base + i} outside [{base}, {end})")
                ok = False
                continue
            leaf = tabs[base + i]
            if leaf < 0:
                # The next lookup is at -(leaf >> 3) from the table's base,
                # `leaf & 7` bits wide, after the `w` bits of this one.
                stack.append((-(leaf >> 3), leaf & 7, depth + w))
            else:
                length = depth + (leaf >> 8)
                longest = max(longest, length)
                if (leaf >> 8) > w or (leaf >> 8) == 0:
                    print(f"table {t}: leaf {leaf} uses {leaf >> 8} of {w} bits")
                    ok = False
    print(f"table {t} (linbits {linbits[t]}): lookups {sorted(seen)[:1]}... all inside [{base}, {end})")
print("longest codeword:", longest)

for name in ("tab32", "tab33"):
    _, v = table(name)
    for p in range(16):
        leaf = v[p]
        if not leaf & 8:
            n = leaf & 3
            if n == 0:
                print(f"{name}[{p}] = {leaf}: second lookup of 0 bits")
                ok = False
            for extra in range(1 << n):
                j = (leaf >> 3) + extra
                if j >= len(v):
                    print(f"{name}: index {j} outside {len(v)}")
                    ok = False
                elif not v[j] & 8:
                    print(f"{name}[{j}] is not a leaf")
                    ok = False
print("OK" if ok else "FAILED")
