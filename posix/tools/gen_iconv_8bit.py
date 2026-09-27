#!/usr/bin/env python3
"""Generate posix/src/iconv_8bit.rs: glibc's table-driven 8-bit character
sets, from glibc's own charmaps and gconv-modules.

    python posix/tools/gen_iconv_8bit.py [path/to/glibc-2.39]

The default glibc tree is D:/refsrc/glibc-2.39.  The output goes next to this
script's crate, posix/src/iconv_8bit.rs, and says in its header which glibc it
came from.

Which sets: exactly the modules glibc 2.39's iconvdata/Makefile generates from
a charmap -- `gen-8bit-modules` (8bit-generic.c) and `gen-8bit-gap-modules`
(8bit-gap.c), 141 of them: the ISO-8859 family but Latin-1, the Windows, IBM
and DOS code pages, KOI8, the Mac sets, EBCDIC.  Every one decodes and encodes
by one rule, so one table apiece is the whole of each:

- a byte is its table's code point; a byte the charmap leaves out is invalid
  (every module here says HAS_HOLES 1, or has no holes);
- a code point is its byte; one with no byte cannot be written.

How glibc builds a table, and so how this does: iconvdata/Makefile maps a
module to its charmap -- the name upper-cased, `ISO8859` spelled `ISO-8859`,
`ISO_2033` given its `-1983` -- and gen-8bit.sh / gen-8bit-gap.sh read the
charmap's `<Uxxxx> /xHH` lines up to `END`, skipping byte 0x00's (which stays
U+0000).  No charmap here maps a byte twice or a code point twice, or maps
beyond the BMP; the generator checks, and refuses rather than guess how
glibc's sort-and-last-wins would have resolved one.

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


def read_charmap(path):
    """Each byte's code point, as gen-8bit(-gap).sh reads the charmap."""
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
            # gen-8bit.sh deletes byte 0x00's line: it stays U+0000.
            if code != 0:
                raise ValueError(f"{path.name}:{n}: byte 0x00 is U+{code:04X}")
            continue
        if code == 0:
            raise ValueError(f"{path.name}:{n}: byte 0x{byte:02x} is U+0000")
        if byte in seen_byte and seen_byte[byte] != code:
            raise ValueError(f"{path.name}:{n}: byte 0x{byte:02x} mapped twice")
        if code in seen_code and seen_code[code] != byte:
            raise ValueError(f"{path.name}:{n}: U+{code:04X} mapped from two bytes")
        seen_byte[byte], seen_code[code] = code, byte
        table[byte] = code
    return table


def read_names(conf_files):
    """Module file name -> (canonical name, [aliases]) from gconv-modules."""
    canon, aliases = {}, {}
    for conf in conf_files:
        for line in conf.read_text(encoding="utf-8").splitlines():
            fields = line.split("#", 1)[0].split()
            if len(fields) >= 3 and fields[0] == "alias":
                aliases.setdefault(fields[2].upper(), []).append(fields[1].upper())
            elif len(fields) >= 4 and fields[0] == "module" and fields[2] == "INTERNAL":
                canon.setdefault(fields[3].upper(), fields[1].upper())
    return canon, aliases


def rust_table(module, source, names, table):
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
    lines.append("    },")
    return lines


def main(argv):
    glibc = Path(argv[1]) if len(argv) > 1 else DEFAULT_GLIBC
    lists = module_lists(glibc / "iconvdata/Makefile")
    canon, aliases = read_names([
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
        "//! `gen-8bit-modules` and `gen-8bit-gap-modules`).  Every one decodes and",
        "//! encodes by one rule, glibc's 8bit-generic.c and 8bit-gap.c: a byte is",
        "//! its table's code point and a byte the charmap leaves out is invalid; a",
        "//! code point is its byte, and one with no byte cannot be written.",
        "//!",
        "//! Only the byte-to-code-point half is stored: `iconv_open` builds the",
        "//! other, sorted for a binary search, in the descriptor that needs it --",
        "//! 770 bytes there rather than 141 KiB in every program that links",
        "//! `iconv`.",
        "",
        "/// One 8-bit character set.",
        "pub(crate) struct Table8 {",
        "    /// Its names, as `iconv_open` matches them: upper case, two slashes.",
        "    pub(crate) names: &'static [&'static [u8]],",
        "    /// Each byte's code point; 0 for a byte the charmap leaves out (byte",
        "    /// 0 is U+0000).",
        "    pub(crate) to_ucs: [u16; 256],",
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
            table = read_charmap(path)
            file_name = module.upper()
            if file_name not in canon:
                raise ValueError(f"{module}: no `module ... INTERNAL {file_name}` in gconv-modules")
            name = canon[file_name]
            names = [name] + sorted(set(aliases.get(name, [])) - {name})
            body += rust_table(module, source, names, table)
            count += 1
    out.append(f"/// The {count} sets, 8bit-generic's then 8bit-gap's, in the Makefile's order.")
    out.append("#[rustfmt::skip]")
    out.append("pub(crate) static TABLES: &[Table8] = &[")
    out += body
    out.append("];")
    out.append("")
    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {count} character sets")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
