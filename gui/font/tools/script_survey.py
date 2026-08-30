"""Which complex scripts can this host actually measure a shaper against?

Run from anywhere:

    python gui/font/tools/script_survey.py [--fonts DIR]

Needs `uharfbuzz`, the same oracle `harfbuzz_sweep.py` uses.

Why this exists
---------------

`known-issues.md` → `TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE` says the four
missing shapers "want their own measurement first", and that the host has to
be surveyed for faces covering those scripts before any of the work can be
checked. That is not a formality. The sweep is this crate's only external
oracle, and a shaper written against zero measuring faces is a shaper whose
correctness rests entirely on having transcribed HarfBuzz accurately — which
is exactly the assumption the sweep exists to stop us making.

So: before writing a shaper, ask what the host can weigh it against.

What "can measure" means here
-----------------------------

Two questions, and the second is the one that matters.

* **Coverage** — does the face have glyphs for the script's characters? A
  face with no glyphs shapes to boxes in both implementations, so it agrees
  with HarfBuzz trivially and measures nothing.
* **Declares** — does the face register the script's OpenType tag in `GSUB`?
  This is the sharper test. Shaping is mostly the *face's* lookups; a face
  that covers Khmer but files no `khmr`/`khmer` features gives both
  implementations nothing to do and again agrees trivially. Only a face that
  both covers the script and declares it can distinguish a correct shaper
  from an absent one.

The count reported per script is therefore `declares`, and `covers` is shown
beside it to make the gap visible — a large gap means the host has the fonts
but not the shaping data, which is the normal situation on Windows for
everything outside its own UI scripts.
"""

import argparse
import os
import sys
from collections import defaultdict

try:
    import uharfbuzz as hb
except ImportError:  # pragma: no cover - a missing oracle is a setup error
    sys.exit("uharfbuzz is not installed: pip install uharfbuzz")

HERE = os.path.dirname(os.path.abspath(__file__))

# One row per shaping family we might write, with the OpenType script tags
# that route to it and a probe string of characters the script cannot be
# written without.
#
# Two tags per Indic-model script because OpenType has two generations of
# them: the `deva`-era tags and the `dev2`-era ones that fixed the reordering
# rules. A face may declare either, and HarfBuzz accepts both.
SCRIPTS = [
    # --- the four missing shapers, in the order the known-issues entry
    # --- proposes writing them.
    ("Khmer", "khmer", ["khmr"], "\u1780\u17d2\u1798\u17c1\u179a"),
    ("Myanmar", "myanmar", ["mymr", "mym2"], "\u1019\u103c\u1014\u103a\u1019\u102c"),
    ("Thai", "thai", ["thai"], "\u0e17\u0e35\u0e48\u0e19\u0e35\u0e48"),
    ("Lao", "thai", ["lao "], "\u0ea5\u0eb2\u0ea7"),
    # --- USE, which is one shaper covering many scripts. Each is listed
    # --- separately because "can we measure USE" is really "is there any one
    # --- of these the host can measure".
    ("Tibetan", "use", ["tibt"], "\u0f56\u0f7c\u0f51\u0f0b"),
    ("Javanese", "use", ["java"], "\ua9a0\ua9b4\ua9a4"),
    ("Balinese", "use", ["bali"], "\u1b13\u1b35\u1b19"),
    ("Sinhala", "use", ["sinh"], "\u0dc3\u0dd2\u0d82\u0dc4\u0dbd"),
    ("Tifinagh", "use", ["tfng"], "\u2d5c\u2d49\u2d3c"),
    ("Cham", "use", ["cham"], "\uaa00\uaa2f\uaa06"),
    ("Buginese", "use", ["bugi"], "\u1a00\u1a17\u1a01"),
    ("Tai Tham", "use", ["lana"], "\u1a20\u1a6b\u1a45"),
    # --- the two we already have, as a control. If Devanagari and Arabic do
    # --- not come back with healthy counts the survey itself is wrong, since
    # --- both were measured against this host at double figures.
    ("Devanagari (have)", "indic", ["deva", "dev2"], "\u0939\u093f\u0928\u094d\u0926\u0940"),
    ("Arabic (have)", "arabic", ["arab"], "\u0627\u0644\u0639\u0631\u0628\u064a\u0629"),
]


def font_files(root):
    """Every font file under `root`, sorted so runs are comparable."""
    out = []
    for dirpath, _dirnames, filenames in os.walk(root):
        for name in sorted(filenames):
            if name.lower().endswith((".ttf", ".otf", ".ttc", ".otc")):
                out.append(os.path.join(dirpath, name))
    return sorted(out)


def faces(path):
    """Every face in a font file, since a `.ttc` holds several."""
    try:
        with open(path, "rb") as handle:
            blob = hb.Blob(handle.read())
    except OSError:
        return
    try:
        first = hb.Face(blob)
    except (RuntimeError, ValueError):
        return
    yield first
    # `count` is 1 for a plain font and the face count for a collection. Only
    # a collection needs the reopen loop, and only from index 1.
    try:
        count = first.count
    except (AttributeError, RuntimeError):
        return
    for i in range(1, count):
        try:
            yield hb.Face(blob, i)
        except (RuntimeError, ValueError, TypeError):
            return


def gsub_tags(face):
    """The `GSUB` script tags a face registers, lowercased and padded to four.

    Read off the raw ScriptList, which is the same source `otl::chosen_from`
    reads — a face may register a script and file no lookup this crate can
    apply, and for "does the face declare it" that still counts.
    """
    try:
        tags = face.get_table_script_tags("GSUB")
    except (AttributeError, RuntimeError):
        try:
            tags = hb.ot_layout_table_get_script_tags(face, "GSUB")
        except (AttributeError, RuntimeError):
            return frozenset()
    return frozenset(str(t).lower().ljust(4)[:4] for t in tags)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--fonts",
        default=os.path.join(os.environ.get("WINDIR", "/usr/share"), "Fonts"),
        help="directory to survey (default: the host's font directory)",
    )
    args = ap.parse_args()
    if not os.path.isdir(args.fonts):
        sys.exit(f"{args.fonts} is not a directory")
    files = font_files(args.fonts)
    if not files:
        sys.exit(f"no fonts under {args.fonts}")

    covers = defaultdict(list)
    declares = defaultdict(list)
    total = 0
    for path in files:
        for face in faces(path):
            total += 1
            try:
                unicodes = face.unicodes
            except (AttributeError, RuntimeError):
                continue
            have = frozenset(unicodes)
            tags = gsub_tags(face)
            name = os.path.basename(path)
            for label, _family, script_tags, probe in SCRIPTS:
                if not all(ord(ch) in have for ch in probe):
                    continue
                covers[label].append(name)
                if tags & frozenset(script_tags):
                    declares[label].append(name)

    print(f"{len(files)} files, {total} faces under {args.fonts}\n")
    width = max(len(label) for label, *_ in SCRIPTS)
    print(f"{'script':<{width}}  {'shaper':<8}  {'covers':>6}  {'declares':>8}  examples")
    print("-" * (width + 40))
    for label, family, _tags, _probe in SCRIPTS:
        names = sorted(set(declares[label]))
        example = ", ".join(names[:3]) if names else "-"
        print(
            f"{label:<{width}}  {family:<8}  {len(set(covers[label])):>6}  "
            f"{len(names):>8}  {example}"
        )


if __name__ == "__main__":
    main()
