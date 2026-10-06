#!/usr/bin/env python3
"""Generate the character tables `charnames` is built from.

    python gui/charnames/gen.py <emoji-test.txt> <UnicodeData.txt> \
        <annotations/en.xml> <annotationsDerived/en.xml>

All four inputs are the Unicode Consortium's, free to redistribute under the
Unicode License:

    https://www.unicode.org/Public/emoji/latest/emoji-test.txt
    https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt
    https://github.com/unicode-org/cldr/blob/main/common/annotations/en.xml
    https://github.com/unicode-org/cldr/blob/main/common/annotationsDerived/en.xml

Writes `gui/charnames/src/emoji_table.rs` and `gui/charnames/src/names_table.rs`:

* every emoji once, in CLDR's order -- its sequence, its name, its subgroup,
  its keywords, and, for one that comes in skin tones, where its five toned
  variants are; and the subgroups and groups;
* each character's name and keywords in the blocks a character picker
  offers, and the categories those blocks make up.

Run it again on newer files to take a new Unicode version; the version each
table was made from is written at its top.

Why generated rather than read at run time: a picker opens instantly and on
an image with no Unicode data on it, and the tables are a few hundred
kilobytes in the one crate that needs them, which only programs offering a
picker link.

# Skin tones

An emoji that can be drawn in a skin tone is listed once, untoned, with its
five single-tone variants (light to dark) beside it: a picker offers each
emoji once and draws it in the tone the user chose, rather than listing
every hand six times. A sequence of two people in two different tones -- a
handshake between a light hand and a dark one -- is not offered: a single
tone choice cannot name it. The generator checks the file's shape it relies
on -- every toned emoji is a variant of an untoned one, and each such emoji
has exactly one variant per tone -- and refuses a file that breaks it.
"""
import io
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

HERE = pathlib.Path(__file__).resolve().parent

# The skin-tone modifiers, light to dark (the Fitzpatrick scale).
TONES = [0x1F3FB, 0x1F3FC, 0x1F3FD, 0x1F3FE, 0x1F3FF]
VS16 = 0xFE0F

# The categories a picker offers besides emoji, each as the blocks (inclusive
# ranges of code points) it is made of. A character with no name in
# UnicodeData.txt -- an unassigned one -- is left out.
CATEGORIES = [
    ("Symbols", [
        (0x00A1, 0x00BF),  # Latin-1 punctuation and symbols
        (0x2010, 0x205E),  # General Punctuation (the printing ones)
        (0x2100, 0x214F),  # Letterlike Symbols
        (0x2300, 0x23FF),  # Miscellaneous Technical
        (0x2460, 0x24FF),  # Enclosed Alphanumerics
        (0x2500, 0x257F),  # Box Drawing
        (0x2580, 0x259F),  # Block Elements
        (0x25A0, 0x25FF),  # Geometric Shapes
        (0x2600, 0x26FF),  # Miscellaneous Symbols
        (0x2700, 0x27BF),  # Dingbats
    ]),
    ("Math", [
        (0x00B1, 0x00B1),
        (0x00D7, 0x00D7),
        (0x00F7, 0x00F7),
        (0x2070, 0x209F),  # Superscripts and Subscripts
        (0x2150, 0x218F),  # Number Forms
        (0x2200, 0x22FF),  # Mathematical Operators
        (0x27C0, 0x27EF),  # Miscellaneous Mathematical Symbols-A
        (0x2980, 0x29FF),  # Miscellaneous Mathematical Symbols-B
        (0x2A00, 0x2AFF),  # Supplemental Mathematical Operators
    ]),
    ("Arrows", [
        (0x2190, 0x21FF),  # Arrows
        (0x27F0, 0x27FF),  # Supplemental Arrows-A
        (0x2900, 0x297F),  # Supplemental Arrows-B
        (0x2B00, 0x2BFF),  # Miscellaneous Symbols and Arrows
    ]),
    ("Currency", [
        (0x0024, 0x0024),
        (0x00A2, 0x00A5),
        (0x20A0, 0x20CF),  # Currency Symbols
    ]),
    ("Latin", [
        (0x00C0, 0x024F),  # Latin-1 letters, Extended-A, Extended-B
        (0x1E00, 0x1EFF),  # Latin Extended Additional
    ]),
    ("Greek", [
        (0x0370, 0x03FF),  # Greek and Coptic
        (0x1F00, 0x1FFF),  # Greek Extended
    ]),
    ("Cyrillic", [
        (0x0400, 0x04FF),
        (0x0500, 0x052F),  # Cyrillic Supplement
    ]),
]


def rust_str(s):
    """`s` as a Rust string literal: printable characters as they are,
    everything else escaped."""
    out = []
    for c in s:
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif c.isprintable() and c != "­":
            out.append(c)
        else:
            out.append("\\u{%x}" % ord(c))
    return '"' + "".join(out) + '"'


def version_of(lines, pattern):
    for line in lines[:40]:
        m = re.search(pattern, line)
        if m:
            return m.group(1)
    return "unknown"


def keywords(*paths):
    """CLDR's keywords for each character or sequence, keyed by its text
    with U+FE0F removed (as CLDR writes the keys), each list joined by `|`."""
    out = {}
    for path in paths:
        for a in ET.parse(path).getroot().iter("annotation"):
            if a.get("type") == "tts" or not a.text:
                continue
            words = [w.strip() for w in a.text.split("|") if w.strip()]
            # The base file wins: a derived entry only fills a gap.
            out.setdefault(a.get("cp"), "|".join(words))
    return out


def tone_of(cps):
    """0 for an untoned sequence, 1 to 5 for one in a single tone, and
    None for one mixing two tones."""
    tones = {c for c in cps if c in TONES}
    if not tones:
        return 0
    if len(tones) == 1:
        return 1 + TONES.index(tones.pop())
    return None


def untoned_key(cps):
    """The sequence with its tones and presentation selectors taken out:
    what a toned variant and its untoned emoji share."""
    return tuple(c for c in cps if c not in TONES and c != VS16)


def emoji(path, kw):
    lines = io.open(path, encoding="utf-8").read().splitlines()
    version = version_of(lines, r"Version:\s*([\d.]+)")
    groups, subgroups, rows = [], [], []
    for line in lines:
        if line.startswith("# group:"):
            groups.append(line.split(":", 1)[1].strip())
        elif line.startswith("# subgroup:"):
            subgroups.append((len(groups) - 1, line.split(":", 1)[1].strip()))
        else:
            m = re.match(r"^([0-9A-F ]+?)\s*;\s*fully-qualified\s*#\s*\S+\s+E[\d.]+\s+(.*)$", line)
            if m:
                cps = [int(cp, 16) for cp in m.group(1).split()]
                rows.append((cps, m.group(2).strip(), len(subgroups) - 1))

    # The untoned emoji, each with its variants in each single tone.
    untoned = []
    at = {}
    for cps, name, sg in rows:
        if tone_of(cps) == 0:
            at[untoned_key(cps)] = len(untoned)
            untoned.append((cps, name, sg, [None] * 5))
    for cps, name, sg in rows:
        t = tone_of(cps)
        if t is None or t == 0:
            continue
        i = at.get(untoned_key(cps))
        if i is None:
            sys.exit("a toned emoji with no untoned one: %s" % name)
        variants = untoned[i][3]
        if variants[t - 1] is not None:
            sys.exit("two variants of %s in one tone" % untoned[i][1])
        variants[t - 1] = (cps, name)
    toned = []
    for cps, name, sg, variants in untoned:
        if any(v is not None for v in variants) and not all(v is not None for v in variants):
            sys.exit("%s comes in some tones and not others" % name)
    if sum(1 for *_, variants in untoned if variants[0] is not None) >= 0xFFFF:
        sys.exit("more toned emoji than a u16 row index holds")
    if len(subgroups) > 0xFF or len(groups) > 0xFF:
        sys.exit("more subgroups or groups than a u8 holds")

    out = io.StringIO()
    out.write("// Generated by gui/charnames/gen.py from emoji-test.txt %s and CLDR's\n" % version)
    out.write("// English annotations. Do not edit.\n\n")
    out.write("/// The emoji version these were made from.\n")
    out.write("pub const VERSION: &str = %s;\n\n" % rust_str(version))
    out.write("/// The groups, in order.\n")
    out.write("pub static GROUPS: [&str; %d] = [\n" % len(groups))
    for g in groups:
        out.write("    %s,\n" % rust_str(g))
    out.write("];\n\n")
    out.write("/// The subgroups, in order: each its group's index and its name.\n")
    out.write("pub static SUBGROUPS: [(u8, &str); %d] = [\n" % len(subgroups))
    for g, s in subgroups:
        out.write("    (%d, %s),\n" % (g, rust_str(s)))
    out.write("];\n\n")
    out.write("/// What an emoji with no skin tones has where its tones' row would be.\n")
    out.write("pub const NO_TONES: u16 = u16::MAX;\n\n")
    rows_out = []
    for cps, name, sg, variants in untoned:
        text = "".join(chr(c) for c in cps)
        kws = kw.get("".join(chr(c) for c in cps if c != VS16), "")
        if variants[0] is not None:
            tones_at = len(toned)
            toned.append(variants)
        else:
            tones_at = None
        rows_out.append((text, name, sg, kws, tones_at))
    out.write("/// Every emoji once, untoned, in CLDR's order: its text, its name, its\n")
    out.write("/// subgroup's index, its keywords joined by `|`, and its row in\n")
    out.write("/// [`TONED`] -- [`NO_TONES`] for one that has none.\n")
    out.write("pub static EMOJI: [(&str, &str, u8, &str, u16); %d] = [\n" % len(rows_out))
    for text, name, sg, kws, tones_at in rows_out:
        tones = "NO_TONES" if tones_at is None else str(tones_at)
        out.write("    (%s, %s, %d, %s, %s),\n" % (rust_str(text), rust_str(name), sg, rust_str(kws), tones))
    out.write("];\n\n")
    out.write("/// The skin-toned variants of the emoji that have them, light to dark:\n")
    out.write("/// each its text and its name.\n")
    out.write("pub static TONED: [[(&str, &str); 5]; %d] = [\n" % len(toned))
    for variants in toned:
        out.write("    [\n")
        for cps, name in variants:
            out.write("        (%s, %s),\n" % (rust_str("".join(chr(c) for c in cps)), rust_str(name)))
        out.write("    ],\n")
    out.write("];\n")
    return out.getvalue(), len(rows_out), len(toned)


def names(path, kw):
    lines = io.open(path, encoding="utf-8").read().splitlines()
    by_cp = {}
    for line in lines:
        fields = line.split(";")
        if len(fields) < 3:
            continue
        cp, name, category = int(fields[0], 16), fields[1], fields[2]
        if name.startswith("<"):
            continue
        # What can be picked and seen on its own: letters, numbers,
        # punctuation and symbols -- not a space, a line separator, a
        # format control or a combining mark with nothing to sit on.
        if category[:1] not in ("L", "N", "P", "S"):
            continue
        by_cp[cp] = name
    wanted = sorted({cp for _, ranges in CATEGORIES for lo, hi in ranges
                     for cp in range(lo, hi + 1) if cp in by_cp})
    out = io.StringIO()
    out.write("// Generated by gui/charnames/gen.py from UnicodeData.txt and CLDR's English\n")
    out.write("// annotations. Do not edit.\n\n")
    out.write("/// The categories a picker offers besides emoji: each its name and\n")
    out.write("/// the ranges of code points it is made of.\n")
    out.write("pub static CATEGORIES: [(&str, &[(u32, u32)]); %d] = [\n" % len(CATEGORIES))
    for name, ranges in CATEGORIES:
        rs = ", ".join("(0x%04X, 0x%04X)" % r for r in ranges)
        out.write("    (%s, &[%s]),\n" % (rust_str(name), rs))
    out.write("];\n\n")
    out.write("/// Each named character in those ranges, by code point: the code\n")
    out.write("/// point, the character as text, its name, and its keywords joined\n")
    out.write("/// by `|`.\n")
    out.write("pub static NAMES: [(u32, &str, &str, &str); %d] = [\n" % len(wanted))
    for cp in wanted:
        out.write("    (0x%04X, %s, %s, %s),\n" % (
            cp, rust_str(chr(cp)), rust_str(by_cp[cp]), rust_str(kw.get(chr(cp), ""))))
    out.write("];\n")
    return out.getvalue(), len(wanted)


def main(argv):
    if len(argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    kw = keywords(argv[3], argv[4])
    e, n_emoji, n_toned = emoji(argv[1], kw)
    t, n_names = names(argv[2], kw)
    src = HERE / "src"
    (src / "emoji_table.rs").write_bytes(e.encode("utf-8"))
    (src / "names_table.rs").write_bytes(t.encode("utf-8"))
    print(f"{n_emoji} emoji ({n_toned} in skin tones), {n_names} named characters")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
