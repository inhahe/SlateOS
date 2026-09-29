#!/usr/bin/env python3
"""Generate posix/src/iconv_8bit.rs: glibc's table-driven 8-bit character
sets, from glibc's own charmaps and gconv-modules.

    python posix/tools/gen_iconv_8bit.py [path/to/glibc-2.39]

The default glibc tree is D:/refsrc/glibc-2.39.  The output goes next to this
script's crate, posix/src/iconv_8bit.rs, and says in its header which glibc it
came from.

Which sets: first, exactly the modules glibc 2.39's iconvdata/Makefile
generates from a charmap -- `gen-8bit-modules` (8bit-generic.c) and
`gen-8bit-gap-modules` (8bit-gap.c), 141 of them: the ISO-8859 family but
Latin-1, the Windows, IBM and DOS code pages, KOI8, the Mac sets, EBCDIC.
Then 25 single-byte modules glibc writes by hand whose converters are
nonetheless exactly their charmaps: iso646.c's 23 national variants of ISO
646 (one converter serves them all from a switch), ISO_11548-1 (8-dot
braille) and ARMSCII-8 (Armenian).  Then the 55 single-byte modules glibc
builds on 8bit-gap.c from a table header of its own rather than from a
charmap -- iconvdata/<module>.c defines `TABLES <module>.h` and the header is
checked in: most of the IBM EBCDIC and PC code pages past the generated ones
(IBM1025, IBM1140-IBM1149, IBM1160s, IBM803, IBM9448 ...), CP737, CP775 and
ISIRI-3342.  Most have no charmap at all, so their tables are read from the
headers, as 8bit-gap.c reads them.  All 80 are checked, not assumed:
posix/tools/oracle/iconv_hand_harness.py runs glibc's own converters over
every byte and every code point and writes what they do to
posix/src/iconv_hand_oracle.txt, which iconv.rs's tests hold these tables to.
Every one decodes and encodes by one rule, so one table apiece is the whole
of each:

- a byte is its table's code point; a byte the table leaves out is invalid
  (every generated module says HAS_HOLES 1, or has no holes; the hand-written
  ones refuse such a byte the same way) -- but for the bytes besides 0x00 a
  module's `NONNUL` says are U+0000 (ISIRI-3342's 0x80), `also_nul`;
- a code point is its byte, and one with no byte cannot be written -- but
  for the bytes that decode and are never written, `decode_only`: those a
  hand-written module's `<MODULE>.irreversible` lists (ARMSCII-8's five
  second copies of ASCII punctuation: 0xA4 decodes to U+0029, and U+0029 is
  written 0x29), and those a table header's encoder writes otherwise;
- and a table header's encoder writes some characters no byte decodes to,
  as the byte of another, `encode_only`: IBM1046 writes an Arabic letter's
  presentation forms as the letter, IBM9448 196 characters so.

How glibc builds a table, and so how this does: iconvdata/Makefile maps a
module to its charmap -- the name upper-cased, `ISO8859` spelled `ISO-8859`,
`ISO_2033` given its `-1983` -- and gen-8bit.sh / gen-8bit-gap.sh read the
charmap's `<Uxxxx> /xHH` lines up to `END`, skipping byte 0x00's (which stays
U+0000).  A hand-written module's charmap is its module name.  No charmap
here maps a byte twice, a code point twice (but where the second byte is
irreversible), or beyond the BMP; the generator checks, and refuses rather
than guess how glibc's sort-and-last-wins would have resolved one.

Names: every name gconv-modules and gconv-modules-extra.conf give the module
(`module NAME// INTERNAL FILE` and the `alias`es of NAME//), upper case, as
iconv.rs matches them after reading a name the way glibc does.
"""

import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "src" / "iconv_8bit.rs"
DEFAULT_GLIBC = Path("D:/refsrc/glibc-2.39")

LINE = re.compile(r"^<U([0-9A-Fa-f]{4})>\s*.x([0-9A-Fa-f]{2})")
WIDE = re.compile(r"^<U[0-9A-Fa-f]{5,}>")

# The hand-written converters whose modules are single-byte charmaps, by the
# file name gconv-modules gives them: every module line naming one of these
# files is a set here.
HAND_FILES = ("ISO646", "ISO_11548-1", "ARMSCII-8")


def module_lists(makefile):
    """The two generated-module lists, in the Makefile's order."""
    text = makefile.read_text(encoding="utf-8")
    lists = {}
    for var in ("gen-8bit-modules", "gen-8bit-gap-modules"):
        m = re.search(rf"^{var}\s*:=(.*?)\n\s*\n", text, re.S | re.M)
        if not m:
            raise ValueError(f"{var} not found in {makefile}")
        lists[var] = m.group(1).replace("\\", " ").split()
    return lists


def charmap_name(module):
    """iconvdata/Makefile's iconv-rules: the charmap a module is made from."""
    name = module.upper()
    name = re.sub(r"^ISO8859", "ISO-8859", name)
    if name == "ISO_2033":
        name += "-1983"
    return name


def read_irreversible(path):
    """The bytes a module's `.irreversible` file lists: decoded, never
    written. Each line is `0xBB 0xUUUU`."""
    if not path.exists():
        return {}
    out = {}
    for n, line in enumerate(path.read_text(encoding="latin-1").splitlines(), 1):
        fields = line.split()
        if not fields:
            continue
        if len(fields) != 2:
            raise ValueError(f"{path.name}:{n}: not `0xBB 0xUUUU`")
        out[int(fields[0], 16)] = int(fields[1], 16)
    return out


def read_charmap(path, irreversible=None, hand=False):
    """Each byte's code point, as gen-8bit(-gap).sh reads the charmap -- or,
    for a hand-written module (`hand`), as its charmap says for byte 0x00
    too, which gen-8bit.sh would have kept U+0000: ISO_11548-1's is U+2800,
    the blank braille cell."""
    irreversible = irreversible or {}
    table = [0] * 256
    seen_byte, seen_code = {}, {}
    for n, line in enumerate(path.read_text(encoding="latin-1").splitlines(), 1):
        if line.startswith("END"):
            break
        if WIDE.match(line):
            raise ValueError(f"{path.name}:{n}: a code point beyond the BMP")
        m = LINE.match(line)
        if not m:
            continue
        code, byte = int(m.group(1), 16), int(m.group(2), 16)
        if byte == 0:
            if hand:
                table[0] = code
                if code:
                    seen_code[code] = 0
                seen_byte[0] = code
                continue
            # gen-8bit.sh deletes byte 0x00's line: it stays U+0000.
            if code != 0:
                raise ValueError(f"{path.name}:{n}: byte 0x00 is U+{code:04X}")
            continue
        if code == 0:
            raise ValueError(f"{path.name}:{n}: byte 0x{byte:02x} is U+0000")
        if byte in seen_byte and seen_byte[byte] != code:
            raise ValueError(f"{path.name}:{n}: byte 0x{byte:02x} mapped twice")
        if byte in irreversible:
            if irreversible[byte] != code:
                raise ValueError(f"{path.name}:{n}: 0x{byte:02x} is U+{code:04X}, "
                                 f"its .irreversible line U+{irreversible[byte]:04X}")
        elif code in seen_code and seen_code[code] != byte:
            raise ValueError(f"{path.name}:{n}: U+{code:04X} mapped from two bytes")
        else:
            seen_code[code] = byte
        seen_byte[byte] = code
        table[byte] = code
    for byte in irreversible:
        if table[byte] == 0:
            raise ValueError(f"{path.name}: .irreversible byte 0x{byte:02x} is not in the charmap")
    return table


def table_header_modules(iconvdata, generated):
    """The single-byte modules glibc builds on 8bit-gap.c from a checked-in
    table header rather than a charmap: (module, header, has_holes,
    also_nul), by file name.  `also_nul` are the bytes besides 0x00 the
    module's `NONNUL` exempts from HAS_HOLES -- which read as U+0000."""
    out = []
    for c_file in sorted(iconvdata.glob("*.c")):
        text = c_file.read_text(encoding="latin-1")
        if not re.search(r'#include\s*[<"]8bit-(gap|generic)\.c[>"]', text):
            continue
        module = c_file.stem
        if module in generated:
            continue
        if "8bit-generic.c" in text:
            raise ValueError(f"{c_file.name}: 8bit-generic over a table header, not modelled")
        m = re.search(r"#define\s+TABLES\s+<([^>]+)>", text)
        if not m:
            raise ValueError(f"{c_file.name}: an 8bit-gap module with no TABLES header")
        header = iconvdata / m.group(1)
        if not header.exists():
            raise ValueError(f"{c_file.name}: no {header.name}")
        holes = re.search(r"#define\s+HAS_HOLES\s+([01])\b", text)
        if not holes:
            raise ValueError(f"{c_file.name}: no HAS_HOLES")
        also_nul = []
        nonnul = re.search(r"#define\s+NONNUL\(c\)\s*(.*)$", text, re.M)
        if nonnul:
            # `((c) != '\0' && (c) != 0x80)`: every term `(c) != X`, one of
            # them NUL's own.
            expr = nonnul.group(1).strip()
            if expr.startswith("(") and expr.endswith(")"):
                expr = expr[1:-1]
            terms = [t.strip() for t in expr.split("&&")]
            values = []
            for term in terms:
                t = re.fullmatch(r"\(?\(c\)\s*!=\s*('\\0'|0x[0-9a-fA-F]+)\)?", term)
                if not t:
                    raise ValueError(f"{c_file.name}: NONNUL term {term!r} not modelled")
                values.append(0 if t.group(1) == "'\\0'" else int(t.group(1), 16))
            if 0 not in values:
                raise ValueError(f"{c_file.name}: NONNUL without NUL's own term")
            also_nul = sorted(v for v in values if v != 0)
        out.append((module, header, holes.group(1) == "1", also_nul))
    return out


CHAR = re.compile(r"'(\\x[0-9a-fA-F]{1,2}|\\[0-7]{1,3}|\\.|[^\\'])'")


def char_value(lit):
    """A C character literal's value, as glibc's headers write them."""
    if lit.startswith("\\x"):
        return int(lit[2:], 16)
    if lit.startswith("\\") and lit[1:].isdigit():
        return int(lit[1:], 8)
    if lit.startswith("\\"):
        simple = {"\\\\": 0x5C, "\\'": 0x27, '\\"': 0x22, "\\0": 0}
        if lit not in simple:
            raise ValueError(f"character literal {lit!r} not modelled")
        return simple[lit]
    return ord(lit)


def read_gap_header(path, has_holes, also_nul):
    """A table header's two halves as 8bit-gap.c reads them: each byte's
    code point or None (`to_ucs4`), and each code point's byte (`from_idx`
    over `from_ucs4`)."""
    text = path.read_text(encoding="latin-1")
    m = re.search(r"to_ucs4\[256\]\s*=\s*\{(.*?)\};", text, re.S)
    if not m:
        raise ValueError(f"{path.name}: no to_ucs4[256]")
    body = re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S)
    pairs = re.findall(r"\[0x([0-9a-fA-F]+)\]\s*=\s*0x([0-9a-fA-F]+)", body)
    if len(re.findall(r"=", body)) != len(pairs):
        raise ValueError(f"{path.name}: to_ucs4 has an entry this does not read")
    to_ucs4 = [0] * 256
    for b, u in pairs:
        b, u = int(b, 16), int(u, 16)
        if b > 0xFF or u > 0xFFFF:
            raise ValueError(f"{path.name}: [0x{b:02x}] = U+{u:04X} out of range")
        to_ucs4[b] = u
    decode = {}
    for b, u in enumerate(to_ucs4):
        if u != 0 or b == 0 or b in also_nul or not has_holes:
            decode[b] = u
    m = re.search(r"from_idx\[\]\s*=\s*\{(.*?)\};", text, re.S)
    if not m:
        raise ValueError(f"{path.name}: no from_idx")
    # `.start = 0x0000` or, in the older headers, GNU's `start: 0x0000`.
    gaps = [(int(a, 16), int(e, 16), int(i)) for a, e, i in re.findall(
        r"(?:\.start\s*=|start:)\s*0x([0-9a-fA-F]+),\s*(?:\.end\s*=|end:)\s*0x([0-9a-fA-F]+),"
        r"\s*(?:\.idx\s*=|idx:)\s*(-?\d+)",
        m.group(1))]
    if len(gaps) != m.group(1).count("{"):
        raise ValueError(f"{path.name}: from_idx has an entry this does not read")
    if not gaps or gaps[-1][:2] != (0xFFFF, 0xFFFF):
        raise ValueError(f"{path.name}: from_idx does not end at 0xffff")
    for (a1, e1, _), (a2, _, _) in zip(gaps, gaps[1:]):
        if not a1 <= e1 < a2:
            raise ValueError(f"{path.name}: from_idx not sorted and disjoint")
    m = re.search(r"from_ucs4\[\]\s*=\s*\{(.*?)\};", text, re.S)
    if not m:
        raise ValueError(f"{path.name}: no from_ucs4")
    body = re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S)
    lits = CHAR.findall(body)
    if re.sub(r"[\s,]", "", CHAR.sub("", body)):
        raise ValueError(f"{path.name}: from_ucs4 has an entry this does not read")
    from_ucs4 = [char_value(lit) for lit in lits]
    encode = {}
    for start, end, idx in gaps[:-1]:
        for c in range(start, end + 1):
            if not 0 <= c + idx < len(from_ucs4):
                raise ValueError(f"{path.name}: U+{c:04X} indexes past from_ucs4")
            res = from_ucs4[c + idx]
            # 8bit-gap.c: a 0 is no byte, but for U+0000's.
            if res != 0 or c == 0:
                encode[c] = res
    return decode, encode


def read_names(conf_files):
    """Module file name -> (canonical name, [aliases]) from gconv-modules, and
    the module names each hand-written file serves, in order."""
    canon, aliases, by_file = {}, {}, {}
    for conf in conf_files:
        for line in conf.read_text(encoding="utf-8").splitlines():
            fields = line.split("#", 1)[0].split()
            if len(fields) >= 3 and fields[0] == "alias":
                aliases.setdefault(fields[2].upper(), []).append(fields[1].upper())
            elif len(fields) >= 4 and fields[0] == "module" and fields[2] == "INTERNAL":
                canon.setdefault(fields[3].upper(), fields[1].upper())
                served = by_file.setdefault(fields[3].upper(), [])
                if fields[1].upper() not in served:
                    served.append(fields[1].upper())
    return canon, aliases, by_file


def rust_table(module, source, names, table, decode_only, encode_only=(), also_nul=(),
               origin=None):
    """One `Table8` literal."""
    origin = origin or f"localedata/charmaps/{charmap_name(module)}"
    lines = [f"    // {module} ({source}, {origin})"]
    lines.append("    Table8 {")
    lines.append("        names: &[")
    for name in names:
        lines.append(f'            b"{name}",')
    lines.append("        ],")
    lines.append("        to_ucs: [")
    for row in range(0, 256, 16):
        cells = ", ".join(f"0x{table[b]:04x}" for b in range(row, row + 16))
        lines.append(f"            {cells},")
    lines.append("        ],")
    lines.append("        decode_only: &[" + ", ".join(f"0x{b:02x}" for b in decode_only) + "],")
    if encode_only:
        lines.append("        encode_only: &[")
        for k in range(0, len(encode_only), 6):
            row = ", ".join(f"(0x{c:04x}, 0x{b:02x})" for c, b in encode_only[k:k + 6])
            lines.append(f"            {row},")
        lines.append("        ],")
    else:
        lines.append("        encode_only: &[],")
    lines.append("        also_nul: &[" + ", ".join(f"0x{b:02x}" for b in also_nul) + "],")
    lines.append("    },")
    return lines


def main(argv):
    glibc = Path(argv[1]) if len(argv) > 1 else DEFAULT_GLIBC
    lists = module_lists(glibc / "iconvdata/Makefile")
    canon, aliases, by_file = read_names([
        glibc / "iconvdata/gconv-modules",
        glibc / "iconvdata/gconv-modules-extra.conf",
    ])
    out = [
        "// @generated by posix/tools/gen_iconv_8bit.py from glibc 2.39's",
        "// localedata/charmaps, iconvdata's table headers and",
        "// iconvdata/gconv-modules* -- do not edit; regenerate.",
        "",
        "//! glibc's table-driven 8-bit character sets, for [`crate::iconv`]: the",
        "//! modules glibc 2.39 generates from a charmap (iconvdata/Makefile's",
        "//! `gen-8bit-modules` and `gen-8bit-gap-modules`), the hand-written",
        "//! single-byte ones whose converters are their charmaps exactly --",
        "//! iso646.c's national variants, ISO_11548-1 and ARMSCII-8 -- and the",
        "//! ones glibc builds on 8bit-gap.c from table headers of their own (most",
        "//! of IBM's EBCDIC and PC code pages past the generated ones, CP737,",
        "//! CP775, ISIRI-3342).  Every one decodes and encodes by one rule,",
        "//! glibc's 8bit-generic.c and 8bit-gap.c: a byte is its table's code",
        "//! point and a byte the table leaves out is invalid; a code point is its",
        "//! byte, and one with no byte cannot be written -- but for a set's",
        "//! `decode_only` bytes, which decode and are never written, its",
        "//! `encode_only` characters, which are written as another's byte, and",
        "//! its `also_nul` bytes, which read as U+0000.",
        "//!",
        "//! Only the byte-to-code-point half is stored: `iconv_open` builds the",
        "//! other, sorted for a binary search, in the descriptor that needs it --",
        "//! 770 bytes there rather than twice these tables in every program that",
        "//! links `iconv` (design-decisions.md §1117).",
        "",
        "/// One 8-bit character set.",
        "pub(crate) struct Table8 {",
        "    /// Its names, as `iconv_open` matches them: upper case, two slashes.",
        "    pub(crate) names: &'static [&'static [u8]],",
        "    /// Each byte's code point; 0 for a byte the table leaves out (byte",
        "    /// 0 is U+0000, and so is each of `also_nul`).",
        "    pub(crate) to_ucs: [u16; 256],",
        "    /// The bytes that decode but are never written: the module's",
        "    /// `.irreversible` list, or those its table header's encoder writes",
        "    /// otherwise -- their code points have another byte.",
        "    pub(crate) decode_only: &'static [u8],",
        "    /// The characters no byte decodes to that the encoder still writes,",
        "    /// each as another character's byte (IBM1046 writes an Arabic",
        "    /// letter's presentation forms as the letter), sorted by character.",
        "    pub(crate) encode_only: &'static [(u16, u8)],",
        "    /// The bytes besides 0x00 whose 0 in `to_ucs` is U+0000 rather than",
        "    /// none: glibc's `NONNUL` (ISIRI-3342's 0x80).",
        "    pub(crate) also_nul: &'static [u8],",
        "}",
        "",
    ]
    body = []
    count = 0
    for var, source in (("gen-8bit-modules", "8bit-generic"), ("gen-8bit-gap-modules", "8bit-gap")):
        for module in lists[var]:
            path = glibc / "localedata/charmaps" / charmap_name(module)
            if not path.exists():
                raise ValueError(f"{module}: no charmap {path}")
            c_file = glibc / "iconvdata" / f"{module}.c"
            if not c_file.exists():
                raise ValueError(f"{module}: no {c_file}")
            if "NONNUL" in c_file.read_text(encoding="latin-1"):
                raise ValueError(f"{module}: defines NONNUL, which this generator does not model")
            if (glibc / "iconvdata" / f"{module.upper()}.irreversible").exists():
                raise ValueError(f"{module}: a generated module with an .irreversible file")
            table = read_charmap(path)
            file_name = module.upper()
            if file_name not in canon:
                raise ValueError(f"{module}: no `module ... INTERNAL {file_name}` in gconv-modules")
            name = canon[file_name]
            names = [name] + sorted(set(aliases.get(name, [])) - {name})
            body += rust_table(module, source, names, table, [])
            count += 1
    generated = count
    for file_name in HAND_FILES:
        c_file = glibc / "iconvdata" / f"{file_name.lower()}.c"
        if not c_file.exists():
            raise ValueError(f"{file_name}: no {c_file}")
        for name in by_file.get(file_name, []):
            module = name.rstrip("/")
            path = glibc / "localedata/charmaps" / module
            if not path.exists():
                raise ValueError(f"{module}: no charmap {path}")
            irreversible = read_irreversible(glibc / "iconvdata" / f"{module}.irreversible")
            table = read_charmap(path, irreversible, hand=True)
            names = [name] + sorted(set(aliases.get(name, [])) - {name})
            body += rust_table(module, f"{file_name.lower()}.c", names, table, sorted(irreversible))
            count += 1
    if count - generated != 25:
        raise ValueError(f"{count - generated} hand-written sets, not the 25 expected: "
                         "a glibc with more or fewer wants the docs above revisited")
    hand = count
    all_generated = set(lists["gen-8bit-modules"]) | set(lists["gen-8bit-gap-modules"])
    for module, header, has_holes, also_nul in table_header_modules(glibc / "iconvdata",
                                                                     all_generated):
        file_name = module.upper()
        served = by_file.get(file_name, [])
        if served != [f"{file_name}//"]:
            raise ValueError(f"{module}: gconv-modules serves {served}, not {file_name}// alone")
        decode, encode = read_gap_header(header, has_holes, also_nul)
        decode_only = sorted(b for b, u in decode.items() if encode.get(u) != b)
        encode_only = sorted((u, b) for u, b in encode.items() if decode.get(b) != u)
        reverse = {u: b for b, u in decode.items() if b not in decode_only}
        for u, b in encode_only:
            if u in reverse:
                raise ValueError(f"{module}: U+{u:04X} both read from 0x{reverse[u]:02x} "
                                 f"and written 0x{b:02x}")
        for b in also_nul:
            if b not in decode_only:
                raise ValueError(f"{module}: 0x{b:02x} reads as U+0000 and is written")
        table = [decode.get(b, 0) for b in range(256)]
        name = f"{file_name}//"
        names = [name] + sorted(set(aliases.get(name, [])) - {name})
        body += rust_table(module, f"{module}.c", names, table, decode_only, encode_only,
                           also_nul, origin=f"iconvdata/{header.name}")
        count += 1
    if count - hand != 55:
        raise ValueError(f"{count - hand} table-header sets, not the 55 expected: "
                         "a glibc with more or fewer wants the docs above revisited")
    out.append(f"/// The {count} sets: 8bit-generic's then 8bit-gap's, in the Makefile's order,")
    out.append("/// then the hand-written ones, in gconv-modules' order, then those with table")
    out.append("/// headers of their own, by file name.")
    out.append("#[rustfmt::skip]")
    out.append("pub(crate) static TABLES: &[Table8] = &[")
    out += body
    out.append("];")
    out.append("")
    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {count} character sets ({generated} generated, "
          f"{hand - generated} hand-written, {count - hand} from table headers)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
