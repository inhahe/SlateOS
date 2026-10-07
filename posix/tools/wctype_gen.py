"""Generate the C library's wide-character class and case tables.

    python posix/tools/wctype_gen.py --oracle   # the rules on Unicode 15.1.0 vs glibc's C.UTF-8
    python posix/tools/wctype_gen.py --emit     # write posix/src/wctype_tables.rs (Unicode 18.0.0)
    python posix/tools/wctype_gen.py --check    # exit 1 if the tables are not what --emit writes

`iswalpha` and its kin, and `towlower`/`towupper`, answer as glibc's
`C.UTF-8` does (design-decisions §1167). glibc builds that locale from the
Unicode Character Database by rules of its own; this script applies the same
rules. `--oracle` proves they are the same rules: run on the version of the
UCD glibc 2.39's built-in `C.UTF-8` was made from (15.1.0), they must give
glibc's answer for every code point, class and mapping in
`posix/src/wctype_oracle.txt` (`posix/tools/oracle/wctype_harness.py`).
They do: the rules were found by running exactly that comparison until no
code point differed.
`--emit` then applies them to the system's version, 18.0.0 -- `charwidth`'s
(design-decisions §1042) -- so that a character the terminal measures is one
the library can classify.

The UCD files are downloaded once into ~/.cache/slateos-ucd/<version>/ and
checked against the SHA-256 recorded here; nothing is generated from bytes
that are not the ones recorded.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
POSIX = HERE.parent
TABLES = POSIX / "src" / "wctype_tables.rs"
ORACLE = POSIX / "src" / "wctype_oracle.txt"

SYSTEM_VERSION = "18.0.0"
GLIBC_VERSION = "15.1.0"
UCD = {
    "15.1.0": {
        "UnicodeData.txt": "2fc713e6a31a87c4850a37fe2caffa4218180fadb5de86b43a143ddb4581fb86",
        "DerivedCoreProperties.txt": "f55d0db69123431a7317868725b1fcbf1eab6b265d756d1bd7f0f6d9f9ee108b",
        "PropList.txt": "05672956317b6296bc2ec3d6cef1f6452b57ff4f2efc6dc55b0a19277d5fcfd1",
    },
    "18.0.0": {
        "UnicodeData.txt": "0736451de439ae7baf1425136617da495e09ee5afbe6e394374db7009ea08950",
        "DerivedCoreProperties.txt": "09c928886a178fcafd93c29e4bd59073a058e5a100b716d425cb563ab50f68c9",
        "PropList.txt": "f438f532e8737bb8a2702126cdf9c4af5e357c58c7acf9d9eb2fc7c1a1d955d6",
    },
}
URL = "https://www.unicode.org/Public/{v}/ucd/{f}"

CLASSES = ("alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
           "space", "upper", "xdigit")
BIT = {name: 1 << i for i, name in enumerate(CLASSES)}
MAX = 0x10FFFF


# ---------------------------------------------------------------------------
# The UCD
# ---------------------------------------------------------------------------

def ucd_file(version: str, name: str) -> str:
    """The text of `name` for `version`, from the cache, fetched if absent."""
    want = UCD[version][name]
    cache = Path(os.path.expanduser("~")) / ".cache" / "slateos-ucd" / version
    path = cache / name
    if not path.exists():
        cache.mkdir(parents=True, exist_ok=True)
        data = fetch(URL.format(v=version, f=name))
        path.write_bytes(data)
    data = path.read_bytes()
    got = hashlib.sha256(data).hexdigest()
    if got != want:
        path.unlink()
        sys.exit(f"{path}: SHA-256 {got}, not the {want} recorded; removed")
    return data.decode("utf-8")


def fetch(url: str) -> bytes:
    """`url`'s bytes. unicode.org resets connections from Python's default
    client, so a browser's agent string is sent, and curl tried after."""
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.read()
    except OSError:
        r = subprocess.run(["curl", "-sS", "-f", "-L", "--retry", "3", "-A", "Mozilla/5.0", url],
                           capture_output=True)
        if r.returncode:
            sys.exit(f"cannot fetch {url}: {r.stderr.decode(errors='replace')}")
        return r.stdout


class Ucd:
    """What the rules read, for every code point: the general category (None
    for an unassigned one), the decomposition, the simple case mappings, and
    the properties."""

    def __init__(self, version: str):
        self.category: dict[int, str] = {}
        self.decomposition: dict[int, str] = {}
        self.name: dict[int, str] = {}
        self.upper: dict[int, int] = {}
        self.lower: dict[int, int] = {}
        first = None
        for line in ucd_file(version, "UnicodeData.txt").splitlines():
            f = line.split(";")
            cp = int(f[0], 16)
            if f[1].endswith(", First>"):
                first = cp
                continue
            span = range(first, cp + 1) if f[1].endswith(", Last>") else range(cp, cp + 1)
            first = None
            for c in span:
                self.category[c] = f[2]
                self.decomposition[c] = f[5]
                self.name[c] = f[1]
                if f[12]:
                    self.upper[c] = int(f[12], 16)
                if f[13]:
                    self.lower[c] = int(f[13], 16)
        self.props: dict[str, set[int]] = {}
        for name in ("DerivedCoreProperties.txt", "PropList.txt"):
            for line in ucd_file(version, name).splitlines():
                line = line.split("#", 1)[0].strip()
                if not line:
                    continue
                cps, prop = (part.strip() for part in line.split(";")[:2])
                lo, _, hi = cps.partition("..")
                self.props.setdefault(prop, set()).update(range(int(lo, 16), int(hi or lo, 16) + 1))

    def prop(self, name: str, cp: int) -> bool:
        return cp in self.props.get(name, ())


# ---------------------------------------------------------------------------
# The rules
# ---------------------------------------------------------------------------

def to_upper(u: Ucd, cp: int) -> int:
    return u.upper.get(cp, cp)


def to_lower(u: Ucd, cp: int) -> int:
    return u.lower.get(cp, cp)


def is_digit(cp: int) -> bool:
    # C: the ten decimal digits, and only they.
    return 0x30 <= cp <= 0x39


def is_xdigit(cp: int) -> bool:
    # C: [0-9A-Fa-f], and only they.
    return is_digit(cp) or 0x41 <= cp <= 0x46 or 0x61 <= cp <= 0x66


def is_space(u: Ucd, cp: int) -> bool:
    cat = u.category.get(cp)
    return (cp in (0x20, 0x0C, 0x0A, 0x0D, 0x09, 0x0B)
            or cat in ("Zl", "Zp")
            or (cat == "Zs" and "<noBreak>" not in u.decomposition.get(cp, "")))


def is_blank(u: Ucd, cp: int) -> bool:
    return cp == 0x09 or (u.category.get(cp) == "Zs"
                          and "<noBreak>" not in u.decomposition.get(cp, ""))


def is_cntrl(u: Ucd, cp: int) -> bool:
    return u.category.get(cp) in ("Cc", "Zl", "Zp")


def is_alpha(u: Ucd, cp: int) -> bool:
    return u.prop("Alphabetic", cp) or (u.category.get(cp) == "Nd" and not is_digit(cp))


def is_upper(u: Ucd, cp: int) -> bool:
    return to_lower(u, cp) != cp or u.prop("Uppercase", cp)


def is_lower(u: Ucd, cp: int) -> bool:
    return to_upper(u, cp) != cp or u.prop("Lowercase", cp)


def is_graph(u: Ucd, cp: int) -> bool:
    cat = u.category.get(cp)
    return cat not in (None, "Cc", "Cs") and not is_space(u, cp)


def is_print(u: Ucd, cp: int) -> bool:
    cat = u.category.get(cp)
    return cat not in (None, "Cc", "Cs", "Zl", "Zp")


def is_punct(u: Ucd, cp: int) -> bool:
    return is_graph(u, cp) and not is_alpha(u, cp) and not is_digit(cp)


def classes(u: Ucd, cp: int) -> int:
    """The class mask of `cp`."""
    mask = 0
    alpha = is_alpha(u, cp)
    if alpha:
        mask |= BIT["alpha"]
    if alpha or is_digit(cp):
        mask |= BIT["alnum"]
    for name, test in (("blank", is_blank), ("cntrl", is_cntrl), ("graph", is_graph),
                       ("lower", is_lower), ("print", is_print), ("punct", is_punct),
                       ("space", is_space), ("upper", is_upper)):
        if test(u, cp):
            mask |= BIT[name]
    if is_digit(cp):
        mask |= BIT["digit"]
    if is_xdigit(cp):
        mask |= BIT["xdigit"]
    return mask


def compute(u: Ucd) -> tuple[list[int], dict[int, int], dict[int, int]]:
    """Every code point's mask, and the lower and upper mappings that are not
    the identity."""
    masks = [classes(u, cp) for cp in range(MAX + 1)]
    lower = {cp: to_lower(u, cp) for cp in u.lower if to_lower(u, cp) != cp}
    upper = {cp: to_upper(u, cp) for cp in u.upper if to_upper(u, cp) != cp}
    return masks, lower, upper


# ---------------------------------------------------------------------------
# --oracle
# ---------------------------------------------------------------------------

def read_oracle() -> tuple[list[int], dict[int, int], dict[int, int]]:
    masks = [0] * (MAX + 1)
    lower: dict[int, int] = {}
    upper: dict[int, int] = {}
    for line in ORACLE.read_text(encoding="utf-8").splitlines()[1:]:
        kind, *rest = line.split()
        if kind == "class":
            lo, hi, mask = (int(x, 16) for x in rest)
            masks[lo:hi + 1] = [mask] * (hi - lo + 1)
        elif kind == "lower":
            lower[int(rest[0], 16)] = int(rest[1], 16)
        elif kind == "upper":
            upper[int(rest[0], 16)] = int(rest[1], 16)
    return masks, lower, upper


def names(mask: int) -> str:
    return ",".join(n for n in CLASSES if mask & BIT[n]) or "-"


def oracle(limit: int) -> int:
    u = Ucd(GLIBC_VERSION)
    ours = compute(u)
    theirs = read_oracle()
    bad = 0
    diffs: dict[tuple[int, int], list[int]] = {}
    for cp in range(MAX + 1):
        if ours[0][cp] != theirs[0][cp]:
            diffs.setdefault((ours[0][cp], theirs[0][cp]), []).append(cp)
            bad += 1
    for (a, b), cps in sorted(diffs.items(), key=lambda kv: -len(kv[1]))[:limit]:
        sample = " ".join(f"{c:04X}({u.category.get(c, '--')})" for c in cps[:8])
        print(f"class: {len(cps)} code points ours [{names(a)}] glibc [{names(b)}]: {sample}")
    for which, mine, glibc in (("lower", ours[1], theirs[1]), ("upper", ours[2], theirs[2])):
        keys = sorted(set(mine) | set(glibc))
        wrong = [k for k in keys if mine.get(k, k) != glibc.get(k, k)]
        bad += len(wrong)
        for k in wrong[:limit]:
            print(f"{which}: {k:04X} ours {mine.get(k, k):04X} glibc {glibc.get(k, k):04X} "
                  f"({u.name.get(k, '?')})")
        if wrong:
            print(f"{which}: {len(wrong)} differ")
    print(f"--oracle: {bad} differences" if bad else "--oracle: every answer is glibc's")
    return 1 if bad else 0


# ---------------------------------------------------------------------------
# --emit / --check
# ---------------------------------------------------------------------------

# The classes the tables store, one bit each beside a run's first code point.
# The other four follow from these and from ASCII, exactly as the rules above
# make them: digit and xdigit are ASCII's, alnum is alpha or digit, and punct
# is graph and neither alpha nor digit.
STORED = ("alpha", "blank", "cntrl", "graph", "lower", "print", "space", "upper")


def stored_bits(mask: int) -> int:
    return sum(1 << i for i, name in enumerate(STORED) if mask & BIT[name])


def derived_mask(bits: int, cp: int) -> int:
    """The full mask from the stored bits, as `wctype.rs` derives it."""
    mask = sum(BIT[name] for i, name in enumerate(STORED) if bits & (1 << i))
    if is_digit(cp):
        mask |= BIT["digit"]
    if is_xdigit(cp):
        mask |= BIT["xdigit"]
    if mask & (BIT["alpha"] | BIT["digit"]):
        mask |= BIT["alnum"]
    if mask & BIT["graph"] and not mask & (BIT["alpha"] | BIT["digit"]):
        mask |= BIT["punct"]
    return mask


def case_runs(mapping: dict[int, int]) -> list[tuple[int, int, int, bool]]:
    """`mapping` as runs (first, last, delta, every_other): consecutive code
    points -- or every other one -- that move by the same amount."""
    items = sorted(mapping.items())
    runs = []
    i = 0
    while i < len(items):
        first, to = items[i]
        delta = to - first
        j, stride = i + 1, None
        while j < len(items):
            cp, t = items[j]
            step = cp - items[j - 1][0]
            if t - cp != delta or step not in (1, 2) or (stride is not None and step != stride):
                break
            stride = step
            j += 1
        runs.append((first, items[j - 1][0], delta, stride == 2))
        i = j
    return runs


def ranges(cps: list[int]) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    for cp in cps:
        if out and out[-1][1] == cp - 1:
            out[-1] = (out[-1][0], cp)
        else:
            out.append((cp, cp))
    return out


def emit_text() -> str:
    new_u, old_u = Ucd(SYSTEM_VERSION), Ucd(GLIBC_VERSION)
    new = compute(new_u)
    old = compute(old_u)
    masks, lower, upper = new
    for cp in range(MAX + 1):
        if derived_mask(stored_bits(masks[cp]), cp) != masks[cp]:
            sys.exit(f"U+{cp:04X}: its classes do not follow from the stored ones")
    runs = []
    for cp in range(MAX + 1):
        bits = stored_bits(masks[cp])
        if not runs or runs[-1][1] != bits:
            runs.append((cp, bits))
    differ = [cp for cp in range(MAX + 1)
              if (masks[cp], lower.get(cp, cp), upper.get(cp, cp))
              != (old[0][cp], old[1].get(cp, cp), old[2].get(cp, cp))]
    assigned = ranges(sorted(cp for cp in new_u.category if cp not in old_u.category))
    changed = [cp for cp in differ if cp in old_u.category]
    if len(differ) != len(changed) + sum(hi - lo + 1 for lo, hi in assigned):
        sys.exit("a code point differs from glibc's data without being new or changed")
    lo_runs, up_runs = case_runs(lower), case_runs(upper)

    out = [
        "//! The classes and case mappings of the wide characters: glibc's `C.UTF-8`",
        f"//! rules, applied to the Unicode Character Database {SYSTEM_VERSION}",
        "//! (design-decisions §1167).",
        "//!",
        "//! Generated by `posix/tools/wctype_gen.py --emit`; do not edit -- run the",
        "//! script. `--check` fails when this file is not what it writes.",
        "",
        "// Each bit is one of the classes the runs store; the other four follow",
        "// from these (`wctype.rs`).",
    ]
    for i, name in enumerate(STORED):
        out.append(f"pub(crate) const {name.upper()}: u32 = 1 << {i};")
    out += [
        "",
        "/// Runs of code points with the same classes, sorted: the run's first code",
        "/// point shifted left by 8, its class bits in the low 8. A run lasts until",
        "/// the next one's first code point.",
        f"pub(crate) static CLASS_RUNS: [u32; {len(runs)}] = [",
    ]
    out += [f"    0x{(cp << 8) | bits:08x}," for cp, bits in runs]
    out.append("];")
    for name, doc, rs in (("TO_LOWER", "`towlower`", lo_runs), ("TO_UPPER", "`towupper`", up_runs)):
        out += [
            "",
            f"/// {doc}'s mappings, as runs: (first, last, delta, every other code point",
            "/// only). A code point a run covers maps to itself plus the delta.",
            f"pub(crate) static {name}: [(u32, u32, i32, bool); {len(rs)}] = [",
        ]
        out += [f"    (0x{a:04x}, 0x{b:04x}, {d}, {str(e).lower()})," for a, b, d, e in rs]
        out.append("];")
    out += [
        "",
        f"/// The code points Unicode assigned between {GLIBC_VERSION}, glibc 2.39's data,",
        f"/// and {SYSTEM_VERSION}: characters here, nothing in glibc. The tests check that",
        "/// glibc gives each no class and no case.",
        "#[cfg(test)]",
        f"pub(crate) static ASSIGNED_SINCE_GLIBC: [(u32, u32); {len(assigned)}] = [",
    ]
    out += [f"    (0x{a:04x}, 0x{b:04x})," for a, b in assigned]
    out += [
        "];",
        "",
        f"/// The characters glibc's data already had whose classes or case Unicode",
        f"/// changed by {SYSTEM_VERSION}: here they are the newer version's. The tests",
        "/// hold every code point but these and the new ones to glibc's answer.",
        "#[cfg(test)]",
        f"pub(crate) static CHANGED_SINCE_GLIBC: [u32; {len(changed)}] = [",
    ]
    out += [f"    0x{cp:04x}, // {new_u.name[cp]}" for cp in changed]
    out.append("];")
    return rustfmt("\n".join(out) + "\n")


def rustfmt(text: str) -> str:
    """`text` as rustfmt lays it out, so that the file the tree checks with
    `cargo fmt` is byte for byte the one `--check` compares."""
    r = subprocess.run(["rustfmt", "--edition", "2024"], input=text, capture_output=True,
                       text=True, encoding="utf-8")
    if r.returncode:
        sys.exit(f"rustfmt: {r.stderr}")
    return r.stdout.replace("\r\n", "\n")


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or check the libc's wctype tables.")
    ap.add_argument("--oracle", action="store_true")
    ap.add_argument("--emit", action="store_true")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--limit", type=int, default=40)
    a = ap.parse_args()
    if a.oracle:
        return oracle(a.limit)
    if a.emit:
        text = emit_text()
        TABLES.write_bytes(text.encode("utf-8"))
        print(f"wrote {TABLES}")
        return 0
    if a.check:
        want = emit_text()
        have = TABLES.read_text(encoding="utf-8") if TABLES.exists() else ""
        if have != want:
            print(f"{TABLES}: not what --emit writes; run it")
            return 1
        print(f"{TABLES}: current")
        return 0
    ap.error("one of --oracle, --emit, --check")
    return 2


if __name__ == "__main__":
    sys.exit(main())
