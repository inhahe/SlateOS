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
braille) and ARMSCII-8 (Armenian).  That "exactly" is checked, not assumed:
posix/tools/oracle/iconv_hand_harness.py runs glibc's own converters over
every byte and every code point and writes what they do to
posix/src/iconv_hand_oracle.txt, which iconv.rs's tests hold these tables to.
Every one decodes and encodes by one rule, so one table apiece is the whole
of each:

- a byte is its table's code point; a byte the charmap leaves out is invalid
  (every generated module says HAS_HOLES 1, or has no holes; the hand-written
  ones refuse such a byte the same way);
- a code point is its byte, and one with no byte cannot be written -- but
  for the bytes a hand-written module's `<MODULE>.irreversible` lists, which
  decode and are never written: ARMSCII-8's five second copies of ASCII
  punctuation (0xA4 decodes to U+0029, and U+0029 is written 0x29).

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


def rust_table(module, source, names, table, decode_only):
    """One `Table8` literal."""
    lines = [f"    // {module} ({source}, localedata/charmaps/{charmap_name(module)})"]
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
    if decode_only:
        lines.append("        decode_only: &[" + ", ".join(f"0x{b:02x}" for b in decode_only) + "],")
    else:
        lines.append("        decode_only: &[],")
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
        "// localedata/charmaps and iconvdata/gconv-modules* -- do not edit;",
        "// regenerate.",
        "",
        "//! glibc's table-driven 8-bit character sets, for [`crate::iconv`]: the",
        "//! modules glibc 2.39 generates from a charmap (iconvdata/Makefile's",
        "//! `gen-8bit-modules` and `gen-8bit-gap-modules`), and the hand-written",
        "//! single-byte ones whose converters are their charmaps exactly --",
        "//! iso646.c's national variants, ISO_11548-1 and ARMSCII-8.  Every one",
        "//! decodes and encodes by one rule, glibc's 8bit-generic.c and 8bit-gap.c:",
        "//! a byte is its table's code point and a byte the charmap leaves out is",
        "//! invalid; a code point is its byte, and one with no byte cannot be",
        "//! written -- but for a set's `decode_only` bytes, which decode and are",
        "//! never written.",
        "//!",
        "//! Only the byte-to-code-point half is stored: `iconv_open` builds the",
        "//! other, sorted for a binary search, in the descriptor that needs it --",
        "//! 770 bytes there rather than 166 KiB in every program that links",
        "//! `iconv`.",
        "",
        "/// One 8-bit character set.",
        "pub(crate) struct Table8 {",
        "    /// Its names, as `iconv_open` matches them: upper case, two slashes.",
        "    pub(crate) names: &'static [&'static [u8]],",
        "    /// Each byte's code point; 0 for a byte the charmap leaves out (byte",
        "    /// 0 is U+0000).",
        "    pub(crate) to_ucs: [u16; 256],",
        "    /// The bytes that decode but are never written: the module's",
        "    /// `.irreversible` list, whose code points have another byte.",
        "    pub(crate) decode_only: &'static [u8],",
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
    out.append(f"/// The {count} sets: 8bit-generic's then 8bit-gap's, in the Makefile's order,")
    out.append("/// then the hand-written ones, in gconv-modules' order.")
    out.append("#[rustfmt::skip]")
    out.append("pub(crate) static TABLES: &[Table8] = &[")
    out += body
    out.append("];")
    out.append("")
    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {count} character sets ({generated} generated, {count - generated} hand-written)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
