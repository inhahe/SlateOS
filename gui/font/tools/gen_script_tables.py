"""Generate `gui/font/src/script_tables.rs` from the Unicode Script property.

Run from anywhere:

    python gui/font/tools/gen_script_tables.py

Needs `fonttools`, which carries both the Unicode `Script` property and the
OpenType script-tag registry, so the two halves of the mapping come from one
source and cannot drift apart.

What the tables are for
-----------------------

`SCRIPT_RANGES`
    The Unicode `Script` property as sorted, disjoint ranges, each naming a
    row of `SCRIPT_TAGS`. Only characters with a *real* script are listed:
    `Common` (digits, punctuation, spaces), `Inherited` (combining marks) and
    `Unknown` are left out, because none of them selects a script — they take
    the script of the text around them, and the range table's job is to answer
    "what script is this, if it is one".

`SCRIPT_TAGS`
    For each script, the OpenType script tag(s) a font may register its
    features under, most-preferred first. Two tags rather than one because
    OpenType revised the Indic script tags: a font may register Devanagari
    features under `dev2` (the 2004 rules) or `deva` (the original), and a
    shaper must try the newer first and fall back. Scripts with only one tag
    repeat it, so the lookup never has to branch on how many there are.

`SCRIPT_EXT_RANGES` and `SCRIPT_EXT_POOL`
    The `Script_Extensions` property, but only where it differs from `Script`
    — which is the only place it carries information. `Script_Extensions`
    answers "which scripts is this character *used by*", as opposed to "which
    script does it belong to": U+0964 DEVANAGARI DANDA is `Common` and is used
    by twenty-one Brahmic scripts, and U+0660 ARABIC-INDIC DIGIT ZERO is
    `Arabic` and is also used by Thaana and Yezidi. UAX #24 resolves a run by
    intersecting these sets, which is what lets an Arabic-Indic digit sit
    inside a Thaana word without splitting it in three.

    A range names a slice of `SCRIPT_EXT_POOL`, which holds rows of
    `SCRIPT_TAGS`. **The character's own script comes first in its slice**
    when it has one, and the rest follow in table order — that is, earliest
    script encoded in Unicode first. The order is the tie-break for a run that
    stays ambiguous from end to end, which is the only case where the set has
    more than one member left to choose from; putting the own script first
    guarantees such a character resolves alone exactly as `Script` would.

`Common` and `Inherited` deliberately have no row. A shaper resolves them to
the surrounding script, and a table that mapped them to `DFLT` would make it
too easy to write a run splitter that starts a new run at every space.
"""

import os

from fontTools import unicodedata as fu

# Scripts that never select a script of their own. `Zyyy` is Common, `Zinh`
# is Inherited, `Zzzz` is Unknown.
NEUTRAL = ("Zyyy", "Zinh", "Zzzz")


def script_ranges():
    """The Script property as (first, last, script) runs, neutrals dropped."""
    runs = []
    for cp in range(0x110000):
        script = fu.script(chr(cp))
        if script in NEUTRAL:
            continue
        if runs and runs[-1][2] == script and runs[-1][1] + 1 == cp:
            runs[-1] = (runs[-1][0], cp, script)
        else:
            runs.append((cp, cp, script))
    return runs


def ot_pair(script):
    """The preferred and fallback OpenType tags for `script`."""
    ot = fu.ot_tags_from_script(script)
    if not ot:
        raise SystemExit(f"{script} has no OpenType tag")
    # Most-preferred first, padded to two so the lookup is branchless.
    first = ot[0]
    second = ot[1] if len(ot) > 1 else ot[0]
    for tag in (first, second):
        if len(tag) != 4 or not all(0x20 <= ord(c) <= 0x7E for c in tag):
            raise SystemExit(f"{script}: {tag!r} is not a four-byte tag")
    return first, second


def extension_ranges(tags):
    """`Script_Extensions` where it differs from `Script`, as ranges of rows.

    Yields `(first, last, rows)` with `rows` a tuple of `SCRIPT_TAGS` indices,
    the character's own script first. Characters whose extension set is just
    their own script carry no information and are left out, as are the ones
    whose set is purely neutral — both are answered by `SCRIPT_RANGES`.
    """
    out = []
    for cp in range(0x110000):
        ch = chr(cp)
        script = fu.script(ch)
        ext = set(fu.script_extension(ch))
        if ext & set(NEUTRAL):
            continue
        # Own script first (so a lone character resolves as `Script` does),
        # then the rest in table order, which is earliest-encoded first. Rows
        # rather than scripts, so Hiragana and Katakana collapse into the one
        # `kana` they share instead of appearing twice.
        own = [] if script in NEUTRAL else [tags[script][0]]
        rest = sorted({tags[s][0] for s in ext} - set(own))
        rows = tuple(own + rest)
        # Nothing `SCRIPT_RANGES` does not already say.
        if rows == tuple(own):
            continue
        if out and out[-1][2] == rows and out[-1][1] + 1 == cp:
            out[-1] = (out[-1][0], cp, rows)
        else:
            out.append((cp, cp, rows))
    return out


def pool(ranges):
    """A flat row pool and a `rows -> offset` index, sharing common tails."""
    flat = []
    at = {}
    # Longest first, so a shorter set that happens to be a slice of a longer
    # one costs nothing. Deterministic: ties break on the rows themselves.
    for rows in sorted({r for _, _, r in ranges}, key=lambda r: (-len(r), r)):
        found = -1
        for i in range(len(flat) - len(rows) + 1):
            if flat[i : i + len(rows)] == list(rows):
                found = i
                break
        if found < 0:
            found = len(flat)
            flat.extend(rows)
        at[rows] = found
    return flat, at


def main():
    runs = script_ranges()

    # One row per *tag pair*, in the order the ranges first mention it, so
    # that the common scripts cluster at the front of the table. Per tag pair
    # rather than per script because two Unicode scripts that OpenType files
    # under one tag are the same script as far as a font is concerned:
    # Hiragana and Katakana are both `kana`, and giving them separate rows
    # would make a run splitter cut Japanese at every change between the two
    # for a difference no font can act on.
    rows = {}
    tags = {}

    def row_for(script):
        if script in tags:
            return
        pair = ot_pair(script)
        rows.setdefault(pair, (len(rows), []))[1].append(script)
        tags[script] = (rows[pair][0], pair[0], pair[1])

    for _, _, script in runs:
        row_for(script)

    # A script may in principle appear in an extension set without owning any
    # character of its own; give it a row too rather than failing the lookup.
    for cp in range(0x110000):
        for script in fu.script_extension(chr(cp)):
            if script not in NEUTRAL:
                row_for(script)

    exts = extension_ranges(tags)
    flat, at = pool(exts)
    widest = max((len(r) for _, _, r in exts), default=0)

    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "src", "script_tables.rs")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w("//! The Unicode `Script` property, and the OpenType tags it maps to.\n")
        w("//!\n")
        w("//! Generated by `gui/font/tools/gen_script_tables.py` from Unicode\n")
        w(f"//! {fu.unidata_version} and fontTools' OpenType tag registry. Do not\n")
        w("//! edit: run the script instead.\n")
        w("//!\n")
        w("//! See that script for what each table is for, and for why `Common`\n")
        w("//! and `Inherited` are deliberately absent.\n")
        w("\n")

        w("/// The `Script` property as sorted, disjoint ranges of code points:\n")
        w("/// `(first, last, tag_index)`. A character in no range has no script\n")
        w("/// of its own and takes the script of the text around it.\n")
        w(f"pub(crate) static SCRIPT_RANGES: [(u32, u32, u16); {len(runs)}] = [\n")
        for lo, hi, script in runs:
            i = tags[script][0]
            w(f"    (0x{lo:04X}, 0x{hi:04X}, {i}), // {script}\n")
        w("];\n\n")

        w("/// The OpenType script tags for each script, preferred first.\n")
        w("///\n")
        w("/// A font registers its features under one of these; the pair exists\n")
        w("/// because OpenType revised the Indic tags and a font may use either\n")
        w("/// spelling. Scripts with a single tag repeat it, so a caller can\n")
        w("/// always try both without asking how many there are.\n")
        w("///\n")
        w("/// One row per tag pair, not per Unicode script: Hiragana and\n")
        w("/// Katakana are both `kana`, and a font cannot tell them apart.\n")
        w(f"pub(crate) static SCRIPT_TAGS: [([u8; 4], [u8; 4]); {len(rows)}] = [\n")
        for (first, second), (_, scripts) in sorted(rows.items(), key=lambda kv: kv[1][0]):
            # `*b"latn"` rather than `[b'l', b'a', b't', b'n']`: the same array,
            # but the byte-string spelling is the one clippy's `byte_char_slices`
            # asks for, and this crate denies `clippy::all`.
            w(f'    (*b"{first}", *b"{second}"), // {" ".join(scripts)}\n')
        w("];\n\n")

        w("/// The largest `Script_Extensions` set in this Unicode version.\n")
        w("///\n")
        w("/// A run's script set only ever shrinks, so this bounds the inline\n")
        w("/// array a run splitter needs to carry one in.\n")
        w(f"pub(crate) const WIDEST_EXTENSION: usize = {widest};\n\n")

        w("/// `Script_Extensions` where it differs from `Script`, as sorted,\n")
        w("/// disjoint ranges: `(first, last, pool_offset, count)`. The rows\n")
        w("/// live at `SCRIPT_EXT_POOL[pool_offset..][..count]`, the\n")
        w("/// character's own script first.\n")
        w("///\n")
        w("/// A character in no range is used by exactly the one script\n")
        w("/// `SCRIPT_RANGES` gives it, or by none at all.\n")
        name = "SCRIPT_EXT_RANGES"
        w(f"pub(crate) static {name}: [(u32, u32, u16, u16); {len(exts)}] = [\n")
        by_row = {i: "/".join(s) for i, s in rows.values()}
        for lo, hi, set_rows in exts:
            names = " ".join(by_row[r] for r in set_rows)
            n, count = at[set_rows], len(set_rows)
            w(f"    (0x{lo:04X}, 0x{hi:04X}, {n}, {count}), // {names}\n")
        w("];\n\n")

        w("/// Rows of `SCRIPT_TAGS`, sliced by `SCRIPT_EXT_RANGES`.\n")
        w("///\n")
        w("/// One set that appears verbatim inside another is stored once, so\n")
        w("/// the pool is smaller than the sum of the sets in it.\n")
        w(f"pub(crate) static SCRIPT_EXT_POOL: [u16; {len(flat)}] = [\n")
        for i in range(0, len(flat), 16):
            w("    " + " ".join(f"{v}," for v in flat[i : i + 16]) + "\n")
        w("];\n")

    print(f"wrote {out}")
    print(f"  Unicode {fu.unidata_version}")
    print(f"  {len(runs)} script ranges over {len(tags)} scripts, {len(rows)} tag rows")
    dual = sum(1 for first, second in rows if first != second)
    print(f"  {dual} tag rows carry a second, older OpenType spelling")
    chars = sum(hi - lo + 1 for lo, hi, _ in exts)
    print(f"  {len(exts)} extension ranges ({chars} characters), widest {widest}")
    print(f"  {len(flat)} pooled rows for {len({r for _, _, r in exts})} sets")


if __name__ == "__main__":
    main()
