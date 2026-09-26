"""Generate `gui/font/src/hint/tables.rs` from FreeType's auto-hinter tables.

Run from anywhere:

    python gui/font/tools/gen_autofit_tables.py [--freetype DIR]

`DIR` holds FreeType 2.13.2's `src/autofit/afscript.h`, `afstyles.h`,
`afranges.c` and `afblue.dat`. Without it, the four files are fetched from the
`VER-2-13-2` tag of FreeType's GitHub mirror. The version is pinned because the
hinter in `gui/font/src/hint/` is a port of that release's `autofit` module and
is checked against it (see `hint_oracle.py`): the tables and the algorithm that
reads them have to be the same vintage.

What the tables are for
-----------------------

The auto-hinter aligns each glyph to the heights its *script* shares -- Latin's
baseline, x-height and cap height, Hebrew's top line, Devanagari's headline --
so it first has to know which script a glyph belongs to, and then which of that
script's letters to measure the heights from.

`SCRIPTS`
    One row per FreeType script: its OpenType tags (whose `GSUB` lookups reach
    the glyphs no character maps to directly, such as ligatures), whether its
    stems are hinted top down, and the letters its standard stem width is
    measured from.

`STYLES`
    The scripts' default styles, in FreeType's order -- which is a priority
    order: a glyph two scripts could claim goes to the earlier one. Each names
    its writing system (the algorithm that hints it) and its alignment zones,
    every zone a string of reference letters and a set of properties.
    FreeType's feature styles (small capitals, superscripts and the rest) are
    left out: they need per-feature coverage the port does not do.

`RANGES`
    Code points to styles, as sorted, disjoint ranges -- FreeType's per-script
    range lists resolved first-come-first-served in style order, which is how
    its coverage pass resolves them.

Each style also lists its script's *non-base* ranges (combining marks and the
like), which the hinter never snaps to a zone. They stay per style rather than
folded into `RANGES` because FreeType marks a glyph non-base when any
character reaching it is in its own style's list, whichever style that
character would have picked alone.

FreeType's copyright
--------------------

The tables are FreeType's work, so the generated file carries its credit line
and names the licence (`gui/font/licenses/FTL.TXT`) it is used under.
"""

import argparse
import os
import re
import sys
import urllib.request

from rustfmt_out import rustfmt

TAG = "VER-2-13-2"
FILES = ("afscript.h", "afstyles.h", "afranges.c", "afblue.dat")
URL = "https://raw.githubusercontent.com/freetype/freetype/{tag}/src/autofit/{name}"

# FreeType's default `ftoption.h` defines these two and not the third.
DEFINED = {"AF_CONFIG_OPTION_CJK", "AF_CONFIG_OPTION_INDIC"}

# ISO 15924 codes of the HarfBuzz scripts FreeType names, from which HarfBuzz
# derives the OpenType script tags (`hb_ot_all_tags_from_script`).
ISO = {
    "HB_SCRIPT_ADLAM": "Adlm", "HB_SCRIPT_ARABIC": "Arab",
    "HB_SCRIPT_ARMENIAN": "Armn", "HB_SCRIPT_AVESTAN": "Avst",
    "HB_SCRIPT_BAMUM": "Bamu", "HB_SCRIPT_BENGALI": "Beng",
    "HB_SCRIPT_BUHID": "Buhd", "HB_SCRIPT_CANADIAN_SYLLABICS": "Cans",
    "HB_SCRIPT_CARIAN": "Cari", "HB_SCRIPT_CHAKMA": "Cakm",
    "HB_SCRIPT_CHEROKEE": "Cher", "HB_SCRIPT_COPTIC": "Copt",
    "HB_SCRIPT_CYPRIOT": "Cprt", "HB_SCRIPT_CYRILLIC": "Cyrl",
    "HB_SCRIPT_DESERET": "Dsrt", "HB_SCRIPT_DEVANAGARI": "Deva",
    "HB_SCRIPT_ETHIOPIC": "Ethi", "HB_SCRIPT_GEORGIAN": "Geor",
    "HB_SCRIPT_GLAGOLITIC": "Glag", "HB_SCRIPT_GOTHIC": "Goth",
    "HB_SCRIPT_GREEK": "Grek", "HB_SCRIPT_GUJARATI": "Gujr",
    "HB_SCRIPT_GURMUKHI": "Guru", "HB_SCRIPT_HAN": "Hani",
    "HB_SCRIPT_HANIFI_ROHINGYA": "Rohg", "HB_SCRIPT_HEBREW": "Hebr",
    "HB_SCRIPT_KANNADA": "Knda", "HB_SCRIPT_KAYAH_LI": "Kali",
    "HB_SCRIPT_KHMER": "Khmr", "HB_SCRIPT_LAO": "Laoo",
    "HB_SCRIPT_LATIN": "Latn", "HB_SCRIPT_LIMBU": "Limb",
    "HB_SCRIPT_LISU": "Lisu", "HB_SCRIPT_MALAYALAM": "Mlym",
    "HB_SCRIPT_MEDEFAIDRIN": "Medf", "HB_SCRIPT_MONGOLIAN": "Mong",
    "HB_SCRIPT_MYANMAR": "Mymr", "HB_SCRIPT_NKO": "Nkoo",
    "HB_SCRIPT_OLD_TURKIC": "Orkh", "HB_SCRIPT_OL_CHIKI": "Olck",
    "HB_SCRIPT_ORIYA": "Orya", "HB_SCRIPT_OSAGE": "Osge",
    "HB_SCRIPT_OSMANYA": "Osma", "HB_SCRIPT_SAURASHTRA": "Saur",
    "HB_SCRIPT_SHAVIAN": "Shaw", "HB_SCRIPT_SINHALA": "Sinh",
    "HB_SCRIPT_SUNDANESE": "Sund", "HB_SCRIPT_SYLOTI_NAGRI": "Sylo",
    "HB_SCRIPT_TAI_VIET": "Tavt", "HB_SCRIPT_TAMIL": "Taml",
    "HB_SCRIPT_TELUGU": "Telu", "HB_SCRIPT_THAI": "Thai",
    "HB_SCRIPT_TIBETAN": "Tibt", "HB_SCRIPT_TIFINAGH": "Tfng",
    "HB_SCRIPT_VAI": "Vaii",
}

# HarfBuzz's `hb_ot_new_tag_from_script`: the revised Indic tags.
NEW_TAG = {
    "Beng": "bng2", "Deva": "dev2", "Gujr": "gjr2", "Guru": "gur2",
    "Knda": "knd2", "Mlym": "mlm2", "Orya": "ory2", "Taml": "tml2",
    "Telu": "tel2", "Mymr": "mym2",
}

# HarfBuzz's `hb_ot_old_tag_from_script` exceptions to "lowercase the code".
OLD_TAG = {"Laoo": "lao ", "Nkoo": "nko ", "Vaii": "vai ", "Hira": "kana"}

BLUE_PROPS = {
    "AF_BLUE_PROPERTY_LATIN_TOP": 1,
    "AF_BLUE_PROPERTY_LATIN_SUB_TOP": 2,
    "AF_BLUE_PROPERTY_LATIN_NEUTRAL": 4,
    "AF_BLUE_PROPERTY_LATIN_X_HEIGHT": 8,
    "AF_BLUE_PROPERTY_LATIN_LONG": 16,
    "AF_BLUE_PROPERTY_CJK_TOP": 1,
    "AF_BLUE_PROPERTY_CJK_HORIZ": 2,
    "AF_BLUE_PROPERTY_CJK_RIGHT": 1,
}

SYSTEMS = {
    "AF_WRITING_SYSTEM_DUMMY": "Dummy",
    "AF_WRITING_SYSTEM_LATIN": "Latin",
    "AF_WRITING_SYSTEM_CJK": "Cjk",
    "AF_WRITING_SYSTEM_INDIC": "Indic",
}


def ot_tags(hb):
    """HarfBuzz's `hb_ot_all_tags_from_script`, at most three tags."""
    iso = ISO.get(hb)
    if iso is None:
        return []
    tags = []
    new = NEW_TAG.get(iso)
    if new is not None:
        if new != "mym2":
            tags.append(new[:3] + "3")
        tags.append(new)
    tags.append(OLD_TAG.get(iso, iso.lower()))
    return tags[:3]


def fetch(src):
    out = {}
    for name in FILES:
        if src:
            with open(os.path.join(src, name), encoding="utf-8") as f:
                out[name] = f.read()
        else:
            url = URL.format(tag=TAG, name=name)
            with urllib.request.urlopen(url, timeout=60) as r:
                out[name] = r.read().decode("utf-8")
    return out


def preprocess(text):
    """Drop the lines a C preprocessor would, for `DEFINED`."""
    out, stack = [], []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("#if"):
            if s.startswith("#ifdef"):
                on = s.split()[1] in DEFINED
            elif s.startswith("#ifndef"):
                on = s.split()[1] not in DEFINED
            else:
                on = s.split()[1] not in ("0",)
            stack.append(on)
            continue
        if s.startswith("#else"):
            stack[-1] = not stack[-1]
            continue
        if s.startswith("#endif"):
            stack.pop()
            continue
        if all(stack):
            out.append(line)
    return "\n".join(out)


def c_string(body):
    """A C string literal's contents (escapes included) as text."""
    raw = bytearray()
    i = 0
    while i < len(body):
        c = body[i]
        if c == "\\":
            n = body[i + 1]
            if n == "x":
                j = i + 2
                while j < len(body) and j < i + 4 and body[j] in "0123456789abcdefABCDEF":
                    j += 1
                raw.append(int(body[i + 2:j], 16))
                i = j
                continue
            raw.append({"n": 10, "t": 9, "\\": 92, '"': 34}[n])
            i += 2
            continue
        raw.extend(c.encode("utf-8"))
        i += 1
    return raw.decode("utf-8")


def scripts(text):
    text = preprocess(text)
    pat = re.compile(
        r'SCRIPT\(\s*(\w+),\s*(\w+),\s*"[^"]*",\s*(\w+),\s*(\w+),\s*((?:"(?:[^"\\]|\\.)*"\s*)+)\)'
    )
    out = []
    for m in pat.finditer(text):
        if m.group(1) == "s":
            continue  # the macro's own definition
        chars = "".join(c_string(s) for s in re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(5)))
        out.append({
            "name": m.group(1),
            "upper": m.group(2),
            "ot": ot_tags(m.group(3)),
            "top_to_bottom": m.group(4) == "HINTING_TOP_TO_BOTTOM",
            "standard": chars,
        })
    return out


def styles(text):
    text = preprocess(text)
    out = []
    style = re.compile(
        r'^\s*STYLE\(\s*(\w+),\s*\w+,\s*"[^"]*",\s*(\w+),\s*(\w+),\s*([\w()]+),\s*(\w+)\s*\)', re.M
    )
    meta = re.compile(r'^\s*META_STYLE_LATIN\(\s*(\w+),\s*(\w+),\s*"[^"]*"\s*\)', re.M)
    indic = re.compile(r'^\s*STYLE_DEFAULT_INDIC\(\s*(\w+),\s*(\w+),\s*"[^"]*"\s*\)', re.M)
    events = []
    for m in style.finditer(text):
        if m.group(5) != "AF_COVERAGE_DEFAULT":
            continue
        events.append((m.start(), m.group(1), SYSTEMS[m.group(2)],
                       m.group(3).replace("AF_SCRIPT_", "").lower(),
                       m.group(4)))
    for m in meta.finditer(text):
        # Only the `dflt` member of the ten a meta style expands to has the
        # default coverage.
        events.append((m.start(), m.group(1) + "_dflt", "Latin", m.group(1),
                       "AF_BLUE_STRINGSET_" + m.group(2)))
    for m in indic.finditer(text):
        if m.group(1) == "s":
            continue
        events.append((m.start(), m.group(1) + "_dflt", "Indic", m.group(1), None))
    events.sort()
    for _, name, system, script, stringset in events:
        out.append({"name": name, "system": system, "script": script, "stringset": stringset})
    return out


def ranges(text):
    text = preprocess(text)
    arr = re.compile(r"af_(\w+?)_(nonbase_)?uniranges\[\]\s*=\s*\{(.*?)\};", re.S)
    rec = re.compile(r"AF_UNIRANGE_REC\(\s*(0x[0-9A-Fa-f]+|0)\s*,\s*(0x[0-9A-Fa-f]+|0)\s*\)")
    base, nonbase = {}, {}
    for m in arr.finditer(text):
        pairs = [(int(a, 0), int(b, 0)) for a, b in rec.findall(m.group(3))]
        pairs = [p for p in pairs if p != (0, 0)]
        (nonbase if m.group(2) else base)[m.group(1)] = pairs
    return base, nonbase


def blues(text):
    text = preprocess(text)
    strings_at = text.index("AF_BLUE_STRING_ENUM")
    sets_at = text.index("AF_BLUE_STRINGSET_ENUM")
    strings, name = {}, None
    for line in text[strings_at:sets_at].splitlines()[1:]:
        s = line.strip()
        if not s or s.startswith("//"):
            continue
        if s.startswith("AF_BLUE_STRING_"):
            name = s
            strings[name] = ""
        elif s.startswith('"'):
            strings[name] += c_string(s[1:-1])
    sets, name, pending = {}, None, ""
    for line in text[sets_at:].splitlines()[1:]:
        s = line.strip()
        if not s or s.startswith("//"):
            continue
        if s.startswith("AF_BLUE_STRINGSET_"):
            name = s
            sets[name] = []
            continue
        pending += " " + s
        if "}" not in s:
            continue
        m = re.match(r"\s*\{\s*(\w+)\s*,(.*)\}\s*$", pending)
        pending = ""
        if m is None or m.group(1) == "AF_BLUE_STRING_MAX":
            continue
        props = 0
        for p in m.group(2).split("|"):
            p = p.strip()
            props |= BLUE_PROPS[p] if p in BLUE_PROPS else int(p, 0)
        sets[name].append((strings[m.group(1)], props, m.group(1)))
    return sets


def resolve(style_list, base):
    """Paint code points first-come-first-served in style order."""
    painted = []  # (lo, hi, style index)

    def free(lo, hi):
        # The parts of [lo, hi] no earlier style has claimed.
        spans = [(lo, hi)]
        for a, b, _ in painted:
            nxt = []
            for x, y in spans:
                if b < x or a > y:
                    nxt.append((x, y))
                    continue
                if x < a:
                    nxt.append((x, a - 1))
                if y > b:
                    nxt.append((b + 1, y))
            spans = nxt
        return spans

    for i, st in enumerate(style_list):
        for lo, hi in base.get(st["script"], []):
            for x, y in free(lo, hi):
                painted.append((x, y, i))
    painted.sort()
    out = []
    for lo, hi, i in painted:
        if out and out[-1][2] == i and out[-1][1] + 1 == lo:
            out[-1] = (out[-1][0], hi, i)
        else:
            out.append((lo, hi, i))
    return out


def rust_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--freetype", help="directory holding the four FreeType files")
    args = ap.parse_args()
    src = fetch(args.freetype)

    script_list = scripts(src["afscript.h"])
    by_name = {s["name"]: i for i, s in enumerate(script_list)}
    style_list = styles(src["afstyles.h"])
    base, nonbase = ranges(src["afranges.c"])
    sets = blues(src["afblue.dat"])
    table = resolve(style_list, base)

    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "hint", "tables.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! FreeType's auto-hinter tables: its scripts, their styles' alignment\n")
        w("//! zones, and which code points belong to which.\n")
        w("//!\n")
        w(f"//! Generated by `gui/font/tools/gen_autofit_tables.py` from FreeType\n")
        w(f"//! {TAG[4:].replace('-', '.')}'s `src/autofit/afscript.h`, `afstyles.h`, `afranges.c` and\n")
        w("//! `afblue.dat`. Do not edit: run the script instead. See that script for\n")
        w("//! what each table is for.\n")
        w("//!\n")
        w("//! Portions of this file are copyright (C) 2013-2023 The FreeType Project\n")
        w("//! (www.freetype.org). All rights reserved. Used under the FreeType\n")
        w("//! License: see `gui/font/licenses/FTL.TXT`.\n")
        w("\n")
        w("use super::{Blue, Script, Style, System};\n\n")

        w("/// FreeType's scripts, in its order.\n")
        w(f"pub(super) static SCRIPTS: [Script; {len(script_list)}] = [\n")
        for s in script_list:
            ot = ", ".join(f"*b{rust_str(t)}" for t in s["ot"])
            w("    Script {\n")
            w(f"        name: {rust_str(s['name'])},\n")
            w(f"        ot: &[{ot}],\n")
            w(f"        top_to_bottom: {'true' if s['top_to_bottom'] else 'false'},\n")
            w(f"        standard: {rust_str(s['standard'])},\n")
            w("    },\n")
        w("];\n\n")

        w("/// The scripts' default styles, in FreeType's order, which is the\n")
        w("/// priority order when two could claim one glyph.\n")
        w(f"pub(super) static STYLES: [Style; {len(style_list)}] = [\n")
        for st in style_list:
            zones = sets.get(st["stringset"], []) if st["stringset"] else []
            if st["system"] == "Indic":
                zones = []  # FreeType's Indic system has no zones yet.
            w("    Style {\n")
            w(f"        name: {rust_str(st['name'])},\n")
            w(f"        script: {by_name[st['script']]},\n")
            w(f"        system: System::{st['system']},\n")
            w("        blues: &[\n")
            for chars, props, blue_name in zones:
                w(f"            // {blue_name}\n")
                w(f"            Blue {{ chars: {rust_str(chars)}, props: {props} }},\n")
            w("        ],\n")
            nb = sorted(nonbase.get(st["script"], []))
            w("        nonbase: &[" + ", ".join(f"(0x{a:04X}, 0x{b:04X})" for a, b in nb) + "],\n")
            w("    },\n")
        w("];\n\n")

        w("/// Code points to styles: sorted, disjoint `(first, last, style)`\n")
        w("/// ranges, resolved first-come-first-served in style order.\n")
        w(f"pub(super) static RANGES: [(u32, u32, u8); {len(table)}] = [\n")
        for lo, hi, i in table:
            w(f"    (0x{lo:04X}, 0x{hi:04X}, {i}), // {style_list[i]['name']}\n")
        w("];\n")
    rustfmt(out)
    print(f"{len(script_list)} scripts, {len(style_list)} styles, {len(table)} ranges -> {out}")


if __name__ == "__main__":
    sys.exit(main())
