#!/usr/bin/env python3
"""Generate posix/src/iconv_combining.rs: the tables of glibc's three 8-bit
character sets with combining characters, CP1255 (Hebrew), CP1258 and
TCVN5712-1 (Vietnamese), from glibc's own converters.

    python posix/tools/gen_iconv_combining.py [path/to/glibc-2.39]

The default glibc tree is D:/refsrc/glibc-2.39.  The output goes next to this
script's crate, posix/src/iconv_combining.rs, and says in its header which
glibc it came from.

Why generated, and from the .c files: these three are not charmap-driven
like iconv_8bit.rs's 141.  glibc writes them by hand (iconvdata/cp1255.c,
cp1258.c, tcvn5712-1.c) because a character can be a base followed by
combining marks, which the decoder composes -- keeping the last character
back to see whether a mark follows -- and the encoder decomposes.  The logic is ported by hand in
iconv.rs; the tables are data, so they are read out of the C, never typed:

- `to_ucs4[128]`: bytes 0x80-0xFF, 0 for none -- TCVN5712-1's in two,
  `map_from_tcvn_low[0x18]` for bytes 0x00-0x17 (its letters sit among the
  C0 controls) and `map_from_tcvn_high[0x80]`;
- `comp_table_data[]`, grouped by `COMP_TABLE_IDX_xxxx`: for each combining
  character xxxx, the (base, composed) pairs, sorted by base;
- `from_ucs4[]` and its `FROM_IDX_nn` offsets, which the encoder indexes by
  the ranges glibc's code names -- carried as they are, offsets and all,
  because the encoder is not the decoder's inverse (CP1258 writes U+0340 and
  U+0341 as U+0300's and U+0301's bytes);
- `decomp_table[]`: a precomposed character, and what it decomposes into --
  CP1255's as a base character and one or two indices into `comb_table`,
  CP1258's and TCVN5712-1's as the base's and the mark's bytes.

Names: every name gconv-modules gives the module, as for iconv_8bit.rs.
"""

import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "src" / "iconv_combining.rs"
DEFAULT_GLIBC = Path("D:/refsrc/glibc-2.39")

NUM = r"(0x[0-9A-Fa-f]+|-?\d+)"


def array_body(src: str, decl: str) -> str:
    """The text between the `{` after `decl` and the `};` that closes it."""
    i = src.index(decl)
    j = src.index("{", i)
    k = src.index("};", j)
    return src[j + 1:k]


def numbers(text: str) -> list[int]:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    return [int(n, 0) for n in re.findall(NUM + r"\s*[,}\n]", text + "\n")]


def decode_table(src: str, decl: str, size: int) -> list[int]:
    vals = numbers(array_body(src, decl))
    assert len(vals) == size, (decl, len(vals))
    return vals


def comp_groups(src: str) -> list[tuple[int, list[tuple[int, int]]]]:
    """[(combining char, [(base, composed), ...]), ...] in glibc's order."""
    body = array_body(src, "comp_table_data[]")
    groups, cur = [], None
    for line in body.splitlines():
        m = re.match(r"\s*#define COMP_TABLE_IDX_([0-9A-Fa-f]{4})\b", line)
        if m:
            cur = (int(m.group(1), 16), [])
            groups.append(cur)
            continue
        m = re.match(r"\s*\{\s*" + NUM + r"\s*,\s*" + NUM + r"\s*\}", line)
        if m:
            assert cur is not None, line
            cur[1].append((int(m.group(1), 0), int(m.group(2), 0)))
    for comb, pairs in groups:
        bases = [b for b, _ in pairs]
        assert bases == sorted(bases) and len(set(bases)) == len(bases), hex(comb)
        # The LEN define must agree with what was read.
        m = re.search(r"#define COMP_TABLE_LEN_%04X (\d+)" % comb, src, re.I)
        assert m and int(m.group(1)) == len(pairs), (hex(comb), m and m.group(1), len(pairs))
    return groups


def from_ucs4(src: str) -> tuple[list[int], dict[str, int]]:
    """glibc's flat `from_ucs4` bytes, and each `FROM_IDX_nn` as the offset
    at which its #define sits."""
    body = array_body(src, "from_ucs4[] =")
    vals, idx = [], {}
    for line in body.splitlines():
        m = re.match(r"\s*#define (FROM_IDX_[0-9A-Za-z]+)\b", line)
        if m:
            idx[m.group(1)] = len(vals)
            continue
        vals.extend(numbers(line))
    # The last marker closes the table, after its last byte: `FROM_IDX_FF`,
    # or TCVN5712-1's `FROM_IDX_END`.
    tail = re.search(r"#define (FROM_IDX_(?:FF|END))\s", src)
    if tail and tail.group(1) not in idx:
        idx[tail.group(1)] = len(vals)
    # Each marker's offset, counted here, must be the one glibc's define
    # states -- `#define FROM_IDX_02 (FROM_IDX_00 + 88)` -- or the table read
    # is not the table glibc indexes.
    for name, prev, n in re.findall(r"#define (FROM_IDX_\w+) \((FROM_IDX_\w+) \+ (\d+)\)", src):
        assert idx[name] == idx[prev] + int(n), (name, idx[name], prev, idx[prev], n)
    assert idx.get("FROM_IDX_FF", idx.get("FROM_IDX_END")) == len(vals), (idx, len(vals))
    return vals, idx


def decomp(src: str, fields: int) -> list[tuple[int, ...]]:
    body = array_body(src, "decomp_table[] =")
    rows = []
    for line in body.splitlines():
        m = re.match(r"\s*\{\s*" + r"\s*,\s*".join([NUM] * fields) + r"\s*\}", line)
        if m:
            rows.append(tuple(int(g, 0) for g in m.groups()))
    composed = [r[0] for r in rows]
    assert composed == sorted(composed) and len(set(composed)) == len(composed)
    return rows


def names(glibc: Path, module: str) -> list[str]:
    out = [module]
    for conf in ("iconvdata/gconv-modules", "iconvdata/gconv-modules-extra.conf"):
        p = glibc / conf
        if not p.exists():
            continue
        for line in p.read_text(encoding="utf-8").splitlines():
            m = re.match(r"alias\s+(\S+)//\s+(\S+)//", line)
            if m and m.group(2).upper() == module and m.group(1).upper() not in out:
                out.append(m.group(1).upper())
    return out


def rs_u16s(vals: list[int], per: int = 8) -> str:
    lines = []
    for i in range(0, len(vals), per):
        lines.append("    " + ", ".join(f"0x{v:04X}" for v in vals[i:i + per]) + ",")
    return "\n".join(lines)


def rs_u8s(vals: list[int], per: int = 12) -> str:
    lines = []
    for i in range(0, len(vals), per):
        lines.append("    " + ", ".join(f"0x{v:02X}" for v in vals[i:i + per]) + ",")
    return "\n".join(lines)


def emit_charset(prefix: str, src: str, glibc: Path, module: str,
                 tables: list[tuple[str, str, int, str]]):
    """`tables`: each decode table as (C declaration, Rust suffix, size, doc)."""
    groups = comp_groups(src)
    fb, fidx = from_ucs4(src)
    out = [f"// ---- {module} ({'iconvdata/' + module.lower() + '.c'}) ----", ""]
    out.append(f"/// {module}'s names, as gconv-modules gives them.")
    nm = names(glibc, module)
    out.append(f"pub(crate) const {prefix}_NAMES: [&[u8]; {len(nm)}] = [" + ", ".join(f'b"{n}//"' for n in nm) + "];")
    out.append("")
    t = []
    for decl, suffix, size, doc in tables:
        vals = decode_table(src, decl, size)
        t.append(vals)
        out.append(f"/// {doc}")
        out.append(f"pub(crate) const {prefix}_{suffix}: [u16; {size}] = [")
        out.append(rs_u16s(vals))
        out.append("];")
        out.append("")
    out.append(f"/// glibc's `comp_table_data`, by combining character: the (base,")
    out.append(f"/// composed) pairs it composes, sorted by base.")
    out.append(f"pub(crate) const {prefix}_COMPOSE: [(u16, &[(u16, u16)]); {len(groups)}] = [")
    for comb, pairs in groups:
        body = ", ".join(f"(0x{b:04X}, 0x{c:04X})" for b, c in pairs)
        out.append(f"    (0x{comb:04X}, &[{body}]),")
    out.append("];")
    out.append("")
    out.append(f"/// glibc's `from_ucs4`, flat, indexed from the `{prefix}_FROM_IDX_*` offsets")
    out.append(f"/// by the ranges the encoder names; 0 for no byte.")
    out.append(f"pub(crate) const {prefix}_FROM_UCS4: [u8; {len(fb)}] = [")
    out.append(rs_u8s(fb))
    out.append("];")
    for k, v in fidx.items():
        if k in ("FROM_IDX_FF", "FROM_IDX_END"):
            continue  # the table's end: its length, checked in from_ucs4()
        out.append(f"/// glibc's `{k}`.")
        out.append(f"pub(crate) const {prefix}_{k}: usize = {v};")
    out.append("")
    return "\n".join(out), t, groups, fb, fidx


def main() -> None:
    glibc = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_GLIBC
    c55 = (glibc / "iconvdata" / "cp1255.c").read_text(encoding="utf-8")
    c58 = (glibc / "iconvdata" / "cp1258.c").read_text(encoding="utf-8")
    ctc = (glibc / "iconvdata" / "tcvn5712-1.c").read_text(encoding="utf-8")
    to128 = [("to_ucs4[128]", "TO_UCS4", 128,
              "glibc's `to_ucs4`: bytes 0x80-0xFF, 0 for a byte that is no character.")]

    parts = [
        "// Generated by posix/tools/gen_iconv_combining.py from glibc 2.39's",
        f"// iconvdata/cp1255.c, cp1258.c and tcvn5712-1.c ({glibc.as_posix()}).  Do not",
        "// edit: rerun the generator.  The converters that read these tables are in",
        "// iconv.rs.",
        "",
        "//! glibc's CP1255, CP1258 and TCVN5712-1 tables (see `crate::iconv`).",
        "",
    ]
    text55, t55, g55, f55, i55 = emit_charset("CP1255", c55, glibc, "CP1255", to128)
    parts.append(text55)
    comb55 = numbers(array_body(c55, "comb_table[8]"))
    assert len(comb55) == 8
    parts.append("/// glibc's `comb_table`: the byte of each combining character a")
    parts.append("/// `CP1255_DECOMPOSE` entry names by index.")
    parts.append("pub(crate) const CP1255_COMB_TABLE: [u8; 8] = [" + ", ".join(f"0x{b:02X}" for b in comb55) + "];")
    parts.append("")
    d55 = decomp(c55, 4)
    parts.append("/// glibc's `decomp_table`: a precomposed character, its base character,")
    parts.append("/// and one or two `CP1255_COMB_TABLE` indices (-1: none).")
    parts.append(f"pub(crate) const CP1255_DECOMPOSE: [(u16, u16, i8, i8); {len(d55)}] = [")
    for c, b, m1, m2 in d55:
        parts.append(f"    (0x{c:04X}, 0x{b:04X}, {m1}, {m2}),")
    parts.append("];")
    parts.append("")

    text58, t58, g58, f58, i58 = emit_charset("CP1258", c58, glibc, "CP1258", to128)
    parts.append(text58)
    d58 = decomp(c58, 3)
    parts.append("/// glibc's `decomp_table`: a precomposed character, and the bytes of its")
    parts.append("/// base and its mark.")
    parts.append(f"pub(crate) const CP1258_DECOMPOSE: [(u16, u8, u8); {len(d58)}] = [")
    for c, b, m in d58:
        parts.append(f"    (0x{c:04X}, 0x{b:02X}, 0x{m:02X}),")
    parts.append("];")
    parts.append("")

    texttc, ttc, gtc, ftc, itc = emit_charset("TCVN", ctc, glibc, "TCVN5712-1", [
        ("map_from_tcvn_low[0x18]", "FROM_LOW", 0x18,
         "glibc's `map_from_tcvn_low`: bytes 0x00-0x17, where TCVN5712-1 puts "
         "letters among the controls."),
        ("map_from_tcvn_high[0x80]", "FROM_HIGH", 0x80,
         "glibc's `map_from_tcvn_high`: bytes 0x80-0xFF."),
    ])
    parts.append(texttc)
    dtc = decomp(ctc, 3)
    parts.append("/// glibc's `decomp_table`: a precomposed character, and the bytes of its")
    parts.append("/// base and its mark.")
    parts.append(f"pub(crate) const TCVN_DECOMPOSE: [(u16, u8, u8); {len(dtc)}] = [")
    for c, b, m in dtc:
        parts.append(f"    (0x{c:04X}, 0x{b:02X}, 0x{m:02X}),")
    parts.append("];")
    parts.append("")

    # Every table keeps the layout written here: rustfmt would rewrap them,
    # and running this again must reproduce the committed file exactly.
    text = "\n".join(parts)
    text = re.sub(r"^(pub\(crate\) const \w+: [\[(])", "#[rustfmt::skip]\n\\1", text, flags=re.M)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: CP1255 {sum(len(p) for _, p in g55)} compositions, "
          f"{len(d55)} decompositions; CP1258 {sum(len(p) for _, p in g58)}, {len(d58)}; "
          f"from_ucs4 {len(f55)}/{len(f58)} bytes, idx {i55} {i58}; TCVN "
          f"{sum(len(p) for _, p in gtc)} compositions, {len(dtc)} decompositions, idx {itc}")


if __name__ == "__main__":
    main()
