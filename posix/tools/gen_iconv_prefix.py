#!/usr/bin/env python3
"""Generate posix/src/iconv_prefix.rs: glibc's four character sets of ISO
6937's shape -- T.61, ISO_6937, ISO_6937-2 and ANSI_X3.110 -- from glibc
2.39's own converters, run under WSL.

    python posix/tools/gen_iconv_prefix.py [path/to/glibc-2.39]

The glibc tree (default D:/refsrc/glibc-2.39) gives the names, from
iconvdata/gconv-modules*; WSL's glibc 2.39 gives the tables, by running the
converters. The output goes next to this script's crate, and says which
glibc it came from.

The shape: one byte a character, but for a letter with a diacritic, which is
two -- the diacritic first (0xC1-0xCF: grave, acute, circumflex, tilde,
macron, breve, dot, diaeresis ...), then the letter (0x20-0x7F). glibc writes
each converter by hand (iconvdata/t.61.c, iso_6937.c, iso_6937-2.c,
ansi_x3.110.c): a byte table, a table of the pairs, and an encoder written as
a table plus a switch of special cases. Here the tables are read out of
glibc's running converters rather than out of the C, because the encoders'
special cases are code, not data:

- every byte alone is decoded (the fifteen diacritic bytes answer "input
  incomplete"), and every diacritic byte followed by every byte;
- every code point from U+0000 to U+10FFFF (surrogates aside) is encoded.

The generator then checks what iconv.rs's decoder and encoder assume, and
refuses to write anything that does not hold: the diacritic bytes are 0xC1 to
0xCF in all four; every code point is in the BMP; a code point is written as
the one byte or pair that decodes to it, and nothing else is written -- but
for ISO_6937-2's three decode-only sequences (0x23 reads as U+0023 and 0x24
as U+00A4, which are written 0xA6 and 0xA8; 0xC4 0x20, a tilde on a space,
reads as U+007E, which is written 0x7E) and the Unicode tag characters,
which every glibc converter drops without a word.

What the tables cannot say, iconv.rs's decoder takes from the C, and
posix/tools/oracle/prefix_harness.py's cases check against glibc: a pair
whose second byte is outside 0x20-0x7F is refused one byte at a time (glibc
skips the diacritic alone and reads on from the second byte), a pair the
table leaves out both bytes at once.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "oracle"))
from _wsl import run, workdir, wsl_path  # noqa: E402

OUT = HERE.parent / "src" / "iconv_prefix.rs"
DEFAULT_GLIBC = Path("D:/refsrc/glibc-2.39")

# The module file names (gconv-modules' fourth column), in the order the
# sets are written.
FILES = ["T.61", "ISO_6937", "ISO_6937-2", "ANSI_X3.110"]
PREFIX_LO, PREFIX_HI = 0xC1, 0xCF

PROBE = r"""
#include <errno.h>
#include <iconv.h>
#include <stdint.h>
#include <stdio.h>

static int decode(iconv_t cd, const unsigned char *in, size_t n, uint32_t *out) {
    char *ip = (char *) in, *op = (char *) out;
    size_t il = n, ol = 4 * 4;
    iconv(cd, NULL, NULL, NULL, NULL);
    size_t r = iconv(cd, &ip, &il, &op, &ol);
    if (r == (size_t) -1) return errno == EINVAL ? -2 : -1;
    if (il != 0 || 16 - ol != 4) return -3;
    return 0;
}

int main(int argc, char **argv) {
    for (int k = 1; k < argc; k++) {
        iconv_t dec = iconv_open("UCS-4LE", argv[k]);
        iconv_t enc = iconv_open(argv[k], "UCS-4LE");
        if (dec == (iconv_t) -1 || enc == (iconv_t) -1) {
            printf("open %s failed\n", argv[k]);
            return 1;
        }
        printf("set %s\n", argv[k]);
        for (int b = 0; b < 256; b++) {
            unsigned char in[1] = {(unsigned char) b};
            uint32_t out[4];
            int r = decode(dec, in, 1, out);
            if (r == 0) printf("s %02x %x\n", b, out[0]);
            else if (r == -2) printf("s %02x incomplete\n", b);
            else printf("s %02x -\n", b);
            if (r == -2) {
                for (int c = 0; c < 256; c++) {
                    unsigned char in2[2] = {(unsigned char) b, (unsigned char) c};
                    if (decode(dec, in2, 2, out) == 0) printf("p %02x %02x %x\n", b, c, out[0]);
                }
            }
        }
        for (uint32_t cp = 0; cp <= 0x10FFFF; cp++) {
            if (cp >= 0xD800 && cp <= 0xDFFF) continue;
            uint32_t in = cp;
            unsigned char out[8];
            char *ip = (char *) &in, *op = (char *) out;
            size_t il = 4, ol = sizeof out;
            iconv(enc, NULL, NULL, NULL, NULL);
            size_t r = iconv(enc, &ip, &il, &op, &ol);
            if (r == (size_t) -1 || il != 0) continue;
            printf("e %x", cp);
            for (size_t i = 0; i < sizeof out - ol; i++) printf(" %02x", out[i]);
            printf("\n");
        }
        iconv_close(dec);
        iconv_close(enc);
    }
    return 0;
}
"""


def read_names(glibc):
    """For each file in FILES: its module's canonical name and aliases."""
    canon, aliases = {}, {}
    for conf in ("iconvdata/gconv-modules", "iconvdata/gconv-modules-extra.conf"):
        for line in (glibc / conf).read_text(encoding="utf-8").splitlines():
            f = line.split("#", 1)[0].split()
            if len(f) >= 3 and f[0] == "alias":
                aliases.setdefault(f[2].upper(), []).append(f[1].upper())
            elif len(f) >= 4 and f[0] == "module" and f[2] == "INTERNAL":
                canon.setdefault(f[3].upper(), f[1].upper())
    out = {}
    for file_name in FILES:
        name = canon[file_name.upper()]
        out[file_name] = [name] + sorted(set(aliases.get(name, [])) - {name})
    return out


def probe(module_names):
    with workdir() as tmp:
        (Path(tmp) / "prefix.c").write_text(PROBE, encoding="utf-8", newline="\n")
        args = " ".join(n.rstrip("/") for n in module_names)
        r = run(f"cd {wsl_path(tmp)} && gcc -O1 -w -o prefix prefix.c && ./prefix {args}")
    if r.returncode != 0:
        sys.exit(f"probe failed:\n{r.stdout[-2000:]}\n{r.stderr[-2000:]}")
    sets, cur = {}, None
    for line in r.stdout.splitlines():
        f = line.split()
        if f[0] == "set":
            cur = sets.setdefault(f[1], {"single": {}, "incomplete": [], "pairs": {}, "encode": {}})
        elif f[0] == "s":
            if f[2] == "incomplete":
                cur["incomplete"].append(int(f[1], 16))
            elif f[2] != "-":
                cur["single"][int(f[1], 16)] = int(f[2], 16)
        elif f[0] == "p":
            cur["pairs"][(int(f[1], 16), int(f[2], 16))] = int(f[3], 16)
        elif f[0] == "e":
            cur["encode"][int(f[1], 16)] = tuple(int(b, 16) for b in f[2:])
    return sets


def check(name, s):
    """The assumptions iconv.rs makes, checked; the decode-only sequences."""
    assert s["incomplete"] == list(range(PREFIX_LO, PREFIX_HI + 1)), (name, s["incomplete"])
    for (p, c), cp in s["pairs"].items():
        assert 0x20 <= c <= 0x7F, (name, hex(p), hex(c), "a pair outside 0x20-0x7F decodes")
    decoded = {(b,): cp for b, cp in s["single"].items()}
    decoded.update({(p, c): cp for (p, c), cp in s["pairs"].items()})
    for seq, cp in decoded.items():
        assert cp <= 0xFFFF, (name, seq, hex(cp))
    tags = set(range(0xE0000, 0xE0080))
    enc = {cp: bs for cp, bs in s["encode"].items() if not (cp in tags and bs == ())}
    for cp, bs in enc.items():
        assert bs in decoded and decoded[bs] == cp, (name, hex(cp), bs, "written as bytes that read otherwise")
    decode_only = sorted(seq for seq, cp in decoded.items() if enc.get(cp) != seq)
    for seq in decode_only:
        assert decoded[seq] in enc, (name, seq, "a decoded code point that is never written")
    return enc, decode_only


def rust(names, s, enc, decode_only):
    lines = [f"    // {names[0].rstrip('/')}"]
    lines.append("    Prefixed {")
    lines.append("        names: &[")
    for n in names:
        lines.append(f'            b"{n}",')
    lines.append("        ],")
    lines.append("        single: [")
    single = [s["single"].get(b, 0) for b in range(256)]
    for row in range(0, 256, 16):
        lines.append("            " + ", ".join(f"0x{v:04x}" for v in single[row:row + 16]) + ",")
    lines.append("        ],")
    lines.append("        pairs: [")
    for p in range(PREFIX_LO, PREFIX_HI + 1):
        lines.append(f"            // 0x{p:02x}")
        lines.append("            [")
        row = [s["pairs"].get((p, c), 0) for c in range(0x20, 0x80)]
        for k in range(0, 96, 16):
            lines.append("                " + ", ".join(f"0x{v:04x}" for v in row[k:k + 16]) + ",")
        lines.append("            ],")
    lines.append("        ],")
    lines.append("        encode: &[")
    for cp in sorted(enc):
        bs = enc[cp]
        second = bs[1] if len(bs) == 2 else 0
        lines.append(f"            (0x{cp:04x}, 0x{bs[0]:02x}, 0x{second:02x}),")
    lines.append("        ],")
    if decode_only:
        shown = ", ".join(" ".join(f"0x{b:02x}" for b in seq) for seq in decode_only)
        lines.append(f"        // Decode-only: {shown}.")
    lines.append("    },")
    return lines


def main(argv):
    glibc = Path(argv[1]) if len(argv) > 1 else DEFAULT_GLIBC
    names = read_names(glibc)
    sets = probe([names[f][0] for f in FILES])
    out = [
        "// @generated by posix/tools/gen_iconv_prefix.py from glibc 2.39's own",
        "// converters under WSL (and its iconvdata/gconv-modules* for the names)",
        "// -- do not edit; regenerate.",
        "",
        "//! glibc's four character sets of ISO 6937's shape, for [`crate::iconv`]:",
        "//! T.61, ISO_6937, ISO_6937-2 and ANSI_X3.110. A character is one byte, or",
        "//! a diacritic byte (0xC1-0xCF) and a letter (0x20-0x7F). Each set is",
        "//! glibc's, read out of its running converter byte by byte and code point",
        "//! by code point (the generator says why, and what it checks).",
        "",
        "/// One of the four sets.",
        "pub(crate) struct Prefixed {",
        "    /// Its names, as `iconv_open` matches them: upper case, two slashes.",
        "    pub(crate) names: &'static [&'static [u8]],",
        "    /// Each byte alone: its code point, or 0 where the set has none (byte 0",
        "    /// is U+0000). The diacritic bytes are 0 here: each starts a pair.",
        "    pub(crate) single: [u16; 256],",
        "    /// Diacritic byte 0xC1 + i before byte 0x20 + j: the pair's code point,",
        "    /// or 0 where the set has none.",
        "    pub(crate) pairs: [[u16; 96]; 15],",
        "    /// Every code point the set writes, sorted: its byte, and the pair's",
        "    /// second byte (0 for a single byte).",
        "    pub(crate) encode: &'static [(u16, u8, u8)],",
        "}",
        "",
        "/// The four, in the order above.",
        "#[rustfmt::skip]",
        "pub(crate) static SETS: [Prefixed; 4] = [",
    ]
    for file_name in FILES:
        module = names[file_name][0]
        s = sets[module.rstrip("/")]
        enc, decode_only = check(module, s)
        out += rust(names[file_name], s, enc, decode_only)
        print(f"{module}: {len(s['single'])} bytes, {len(s['pairs'])} pairs, {len(enc)} written, "
              f"decode-only {decode_only}")
    out.append("];")
    out.append("")
    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
