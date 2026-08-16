"""How much would honouring LangSysRecords actually change on this host?

Run from anywhere:

    python gui/font/tools/langsys_survey.py [--fonts DIR] [--list-tags]

Needs `fontTools`. Unlike the other tools here it does not need an oracle:
the question is about what the *fonts* say, not about what a shaper does with
it, so it reads the ScriptList directly.

Why this exists
---------------

`known-issues.md` -> `TD-FONT-IGNORES-LANGSYS-OVERRIDES` says `otl::select`
reads each script's DefaultLangSys and ignores its LangSysRecords, so the
per-language overrides -- Turkish dotless `i` under `TRK `, Serbian Cyrillic
italics under `SRB ` -- are unreachable. Before writing the language-selection
machinery, ask the same question `script_survey.py` asks before writing a
shaper: **what on this host could measure it?**

That question has a sharp form here, because a language system that exists is
not the same as a language system that *matters*:

* A LangSysRecord whose feature-index list is identical to its script's
  DefaultLangSys changes nothing for anyone. Fonts ship these in quantity --
  a compiler emits one per language named in the source whether or not the
  rules differ.
* A LangSysRecord that differs only in features this crate never asks for
  changes nothing *for us*. We request a fixed set of tags (`FEATURES` in
  `gsub.rs`, the same set in `gpos.rs`); a face that adds `salt` or `ss01`
  under `TRK ` is invisible to us however different it is.

So the number to report is the third one: (face, table, script, language)
where the set of **requested** feature tags differs from the DefaultLangSys's.
That is exactly the set of cases where honouring the record changes a glyph on
this machine, and therefore the set the sweep could ever detect.

Required features
-----------------

A LangSys also names one *required* feature outside its index list
(`requiredFeatureIndex`, 0xFFFF for none). `otl.rs` ignores that field too --
listed under "What is not here" in its module doc -- so it is counted here as
part of the language's feature set. A language whose only difference from the
default is its required feature is still a difference, and would still be
missed.
"""

import argparse
import os
import sys
from collections import Counter, defaultdict

try:
    from fontTools.ttLib import TTFont, TTLibError
except ImportError:  # pragma: no cover - a missing dependency is a setup error
    sys.exit("fontTools is not installed: pip install fonttools")

# The feature tags this crate asks for, from `FEATURES` in `gsub.rs` (the
# `gpos.rs` list is deliberately the same set). Kept as a literal rather than
# parsed out of the source: a survey that silently measured a *different* set
# from the shaper would be worse than one that has to be updated by hand, and
# `the_survey_matches_the_shapers_feature_list` in `otl.rs` pins them equal.
WANTED = frozenset(
    """
    ccmp locl liga rlig clig calt rclt abvm blwm curs dist kern mark mkmk
    isol init medi fina
    nukt akhn rphf rkrf pref blwf abvf half pstf vatu cjct pres abvs blws
    psts haln
    ljmo vjmo tjmo
    """.split()
)

DEFAULT_FONT_DIRS = [
    r"C:\Windows\Fonts",
    os.path.expandvars(r"%LOCALAPPDATA%\Microsoft\Windows\Fonts"),
    "/usr/share/fonts",
]


def font_files(dirs):
    """Every font file under `dirs`, deduplicated by absolute path."""
    seen = {}
    for d in dirs:
        if not d or not os.path.isdir(d):
            continue
        for root, _, names in os.walk(d):
            for name in names:
                if not name.lower().endswith((".ttf", ".otf", ".ttc", ".otc")):
                    continue
                path = os.path.abspath(os.path.join(root, name))
                seen.setdefault(path.lower(), path)
    return sorted(seen.values())


def feature_tags(feature_list, lang_sys):
    """The feature tags a LangSys names, including its required feature.

    `None` for a missing LangSys, which is not the same as an empty set: a
    script with no DefaultLangSys says "this script gets no features", and a
    language that adds some to it is a difference worth counting.
    """
    if lang_sys is None:
        return frozenset()
    out = set()
    indices = list(lang_sys.FeatureIndex)
    req = getattr(lang_sys, "ReqFeatureIndex", 0xFFFF)
    if req != 0xFFFF:
        indices.append(req)
    for i in indices:
        try:
            out.add(feature_list.FeatureRecord[i].FeatureTag)
        except (IndexError, AttributeError):
            continue
    return frozenset(out)


def survey_table(table, rows, tag_counter, wanted_tag_counter):
    """Accumulate one face's GSUB or GPOS into the running totals."""
    script_list = getattr(table.table, "ScriptList", None)
    feature_list = getattr(table.table, "FeatureList", None)
    if script_list is None or feature_list is None:
        return
    for rec in script_list.ScriptRecord:
        script = rec.ScriptTag
        default = feature_tags(feature_list, getattr(rec.Script, "DefaultLangSys", None))
        for lang_rec in getattr(rec.Script, "LangSysRecord", []) or []:
            lang = lang_rec.LangSysTag
            tags = feature_tags(feature_list, lang_rec.LangSys)
            tag_counter[lang] += 1
            differs = tags != default
            differs_wanted = (tags & WANTED) != (default & WANTED)
            if differs_wanted:
                wanted_tag_counter[lang] += 1
            rows.append((script, lang, differs, differs_wanted,
                         tuple(sorted((tags ^ default) & WANTED))))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fonts", action="append", default=None,
                    help="a directory to scan (repeatable; defaults to the host's)")
    ap.add_argument("--list-tags", action="store_true",
                    help="also list every language tag seen, not just the ones that matter")
    args = ap.parse_args()

    files = font_files(args.fonts or DEFAULT_FONT_DIRS)
    if not files:
        sys.exit("no font files found")

    faces = 0
    faces_with_lang = 0
    faces_that_matter = set()
    rows = []
    tag_counter = Counter()
    wanted_tag_counter = Counter()
    # Which requested feature tags the overrides actually move.
    moved = Counter()
    per_face = defaultdict(list)

    for path in files:
        try:
            fonts = []
            if path.lower().endswith((".ttc", ".otc")):
                # A collection's faces share tables by offset; open each.
                from fontTools.ttLib import TTCollection
                fonts = list(TTCollection(path, lazy=True).fonts)
            else:
                fonts = [TTFont(path, lazy=True)]
        except (TTLibError, OSError, ValueError, KeyError, AssertionError):
            continue
        for font in fonts:
            faces += 1
            before = len(rows)
            for which in ("GSUB", "GPOS"):
                try:
                    table = font.get(which)
                except (TTLibError, ValueError, KeyError, AssertionError):
                    continue
                if table is None:
                    continue
                try:
                    survey_table(table, rows, tag_counter, wanted_tag_counter)
                except (AttributeError, TTLibError, ValueError, KeyError, AssertionError):
                    continue
            new = rows[before:]
            if new:
                faces_with_lang += 1
            for _script, _lang, _d, differs_wanted, delta in new:
                if differs_wanted:
                    faces_that_matter.add(os.path.basename(path))
                    per_face[os.path.basename(path)].append((_script, _lang, delta))
                    for tag in delta:
                        moved[tag] += 1
            try:
                font.close()
            except (AttributeError, OSError):
                pass

    total = len(rows)
    differ = sum(1 for r in rows if r[2])
    differ_wanted = sum(1 for r in rows if r[3])

    print(f"faces scanned                         {faces}")
    print(f"faces registering any LangSysRecord   {faces_with_lang}")
    print(f"(script, language) records total      {total}")
    print(f"  ... that differ from their default  {differ}")
    print(f"  ... in a feature tag we ask for     {differ_wanted}"
          f"   <- the only ones that can change a glyph here")
    print(f"faces where that happens              {len(faces_that_matter)}")

    if moved:
        print("\nrequested feature tags the overrides move:")
        for tag, n in moved.most_common():
            print(f"  {tag}  {n}")

    if wanted_tag_counter:
        print("\nlanguage tags that matter (records differing in a requested feature):")
        for tag, n in wanted_tag_counter.most_common():
            print(f"  {tag!r}  {n}")

    if per_face:
        print("\nfaces, with the (script, language, moved tags) that matter:")
        for name in sorted(per_face)[:40]:
            for script, lang, delta in per_face[name][:6]:
                print(f"  {name:<40} {script} {lang!r} {list(delta)}")

    if args.list_tags:
        print("\nevery language tag seen, by record count:")
        for tag, n in tag_counter.most_common():
            print(f"  {tag!r}  {n}")


if __name__ == "__main__":
    main()
