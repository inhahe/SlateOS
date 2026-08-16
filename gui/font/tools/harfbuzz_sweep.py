"""Check this crate's shaper against HarfBuzz over every font on the host.

Run from anywhere:

    python gui/font/tools/harfbuzz_sweep.py [--corpus FILE] [--fonts DIR]

Needs `uharfbuzz`. Builds and runs the `shape_dump` example, shapes the same
corpus with HarfBuzz, and reports every (face, string) pair the two disagree
about, grouped by string so a systematic difference is one line rather than
five hundred.

Why an external oracle
----------------------

This crate's own tests can only check it against itself. That is enough for
"does the parser read what the spec says" and useless for "does this face
actually have a glyph for that", because *no glyph* is a legal answer and a
self-consistency check cannot tell a correct absence from a bug. Two real
bugs were found exactly this way and by nothing else:

* symbol-encoded faces (Wingdings and friends) drew every character as a box,
  because their `cmap` keys on `U+F0xx` rather than on Unicode;
* text was not normalized at all, so `e` + combining acute rendered as two
  glyphs where `\u00e9` rendered as one.

Reading the output
------------------

Disagreement is not automatically failure. Some of it is deliberate — the
divergences are recorded in `design-decisions.md` §410 — and the value of the
sweep is that the *set* of disagreements stays the one you expect. A new
string appearing in the report is the signal.

The `mixed` line is not a verdict at all: HarfBuzz guesses one script for the
whole buffer where this crate itemizes into script runs, so for the two
mixed-script strings the halves are answering different questions. See
`MIXED`.
"""

import argparse
import os
import subprocess
import sys
import tempfile
from collections import Counter

try:
    import uharfbuzz as hb
except ImportError:  # pragma: no cover - a missing oracle is a setup error
    sys.exit("uharfbuzz is not installed: pip install uharfbuzz")

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.normpath(os.path.join(HERE, ".."))
REPO = os.path.normpath(os.path.join(CRATE, "..", ".."))

TARGET = "x86_64-pc-windows-gnu"

# The corpus. Each entry is a question about one stage of shaping, and the
# escapes are expanded by both halves of the sweep identically.
CORPUS = [
    # Plain Latin: nothing should happen, and something usually does.
    "The quick brown fox jumps over the lazy dog",
    "Hamburgefonstiv",
    # Ligatures, which is what `liga` is for.
    "office fluffy waffle",
    "fi fl ffi ffl",
    # Normalization: the same text spelled two ways must shape the same.
    "\\u00e9t\\u00e9",
    "e\\u0301te\\u0301",
    "c\\u0327\\u0301",
    "\\u1e09",
    # A singleton decomposition, where we are spec-NFC and HarfBuzz is not.
    "\\u212b",
    # Hangul jamo, which NFC composes into syllables.
    "\\u1100\\u1161\\u11a8",
    "\\uac00",
    # Contextual alternates and fractions.
    "1/2 3/4",
    "0O1lI",
    # Mixed scripts in one string, which is the case script runs exist for.
    # Listed in `MIXED` below: the two halves are not answering the same
    # question here, so their disagreements are reported apart from the rest.
    "hello \\u05e9\\u05dc\\u05d5\\u05dd world",
    "abc \\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629 xyz",
    # Arabic on its own, where a face with Arabic features should use them.
    "\\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629",
    # Fully vowelled Arabic — `bismi`, a letter and a mark alternating. Every
    # lookup here has to step over the marks to see the letters, so this is
    # the string that exercises `IgnoreMarks`; the unvowelled entry above
    # cannot, because it has no marks to ignore.
    "\\u0628\\u0650\\u0633\\u0652\\u0645\\u0650",
    # LAM, FATHA, ALEF: the lam and the alef ligate across the fatha, which
    # then has to be placed over the *first* component of the joined glyph.
    # This is the string mark-to-ligature positioning exists for, and the only
    # one in the corpus where getting the component wrong is visible.
    "\\u0644\\u064e\\u0627",
    # Pointed Hebrew: SHIN QAMATS SHIN-DOT, LAMED, VAV HOLAM, FINAL MEM. The
    # points sit at a height that depends on the letter under them and on what
    # else is already there.
    "\\u05e9\\u05b8\\u05c1\\u05dc\\u05d5\\u05b9\\u05dd",
    # LAMED, LAMED QAMATS METEG. The meteg is a vertical stroke that shares the
    # space under the letter with whatever vowel is already there, so a Hebrew
    # face shifts the vowel aside — and writes that shift as a *chained
    # contextual* rule keyed on "letter, vowel, meteg". This is the string GPOS
    # types 7 and 8 exist for; the pointed-Hebrew entry above has no meteg and
    # so reaches no such rule.
    "\\u05dc\\u05dc\\u05b8\\u05bd",
    # Thai with a tone mark over a vowel over a consonant. A tone mark that has
    # a vowel under it must be lifted clear of it, and Thai faces write that
    # lift as a contextual rule too.
    "\\u0e17\\u0e35\\u0e48\\u0e19\\u0e35\\u0e48",
    # Devanagari, the reason script tags have two spellings.
    "\\u0939\\u093f\\u0928\\u094d\\u0926\\u0940",
    # Scriptless text, which selects the font's default features.
    "123 456",
    # --- language ---
    #
    # A language selects a LangSysRecord in place of the script's default
    # language system, and that record *replaces* the default's feature list
    # rather than adding to it -- so naming a language can take a feature away
    # as readily as turn one on. Most entries below are a string already in the
    # corpus, or paired with a language-less twin, so that a difference between
    # the two halves is the language and nothing else.
    #
    # Turkish: `TRK ` suppresses `fi`, which a Turkish reader sees as a
    # misspelling of the dotless/dotted pair rather than as a ligature.
    ("tr", "office fluffy waffle"),
    ("tr", "fi fl ffi ffl"),
    # Serbian and Macedonian Cyrillic: `SRB `/`MKD ` swap six italic letters
    # for their locally-correct shapes. 41 host faces register `SRB `.
    ("sr", "\\u0431\\u0433\\u0434\\u043f\\u0442"),
    ("mk", "\\u0431\\u0433\\u0434\\u043f\\u0442"),
    "\\u0431\\u0433\\u0434\\u043f\\u0442",
    # Romanian and Moldovan: `ROM `/`MOL ` put a comma below s and t where the
    # default draws a cedilla. The two most common LangSysRecords on this host
    # (140 and 76 faces), and `ro-MD` is the one tag here whose *region* subtag
    # changes the answer.
    ("ro", "\\u0219\\u021b \\u015f\\u0163"),
    ("ro-MD", "\\u0219\\u021b \\u015f\\u0163"),
    "\\u0219\\u021b \\u015f\\u0163",
    # Catalan: `CAT ` keeps the middle dot of l-l apart from the `ll` ligature.
    ("ca", "col\\u00b7legi"),
    "col\\u00b7legi",
    # Urdu, which selects a different set of Arabic forms from Arabic itself.
    ("ur", "\\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629"),
    # A language essentially no face registers. It must shape exactly like no
    # language at all: selection falls back to the script's own default, never
    # to another script's and never to another language's.
    ("chr", "office fluffy waffle"),
]


def lang_of(entry):
    """The BCP 47 tag an entry names, or `""` for text that names none."""
    return entry[0] if isinstance(entry, tuple) else ""


def string_of(entry):
    """The (still escaped) string an entry shapes."""
    return entry[1] if isinstance(entry, tuple) else entry


# Corpus entries the two halves cannot be asked the same question about.
#
# `hb_buffer_guess_segment_properties` picks *one* script and one direction for
# the whole buffer, from the first strong character. This crate itemizes into
# script runs and shapes each on its own. For a string that is entirely one
# script the two agree by construction; for a mixed one they do not, and the
# difference is the itemizer, not the shaper:
#
# * `hello <hebrew> world` guesses Latin, so HarfBuzz never selects `hebr` and
#   never applies the lamed/vav kern that our Hebrew run does. Shaping the
#   Hebrew word on its own agrees exactly, -10 and all.
# * `abc <arabic> xyz` guesses Latin too, so the Arabic is shaped with no
#   joining at all — HarfBuzz returns the isolated forms where we return the
#   joined ones.
#
# Both are counted and reported, but apart: a difference here says nothing
# about whether the shaper is right, and burying it with the ones that do is
# how a real regression hides behind forty-nine lines of noise.
MIXED = frozenset(
    [
        "hello \\u05e9\\u05dc\\u05d5\\u05dd world",
        "abc \\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629 xyz",
    ]
)
assert MIXED <= set(CORPUS), "MIXED names a string that is not in the corpus"


def unescape(line):
    """Expand `\\uXXXX`, matching `shape_dump.rs`'s reader exactly."""
    out = []
    i = 0
    while i < len(line):
        if line[i] != "\\":
            out.append(line[i])
            i += 1
        elif line[i : i + 2] == "\\u":
            out.append(chr(int(line[i + 2 : i + 6], 16)))
            i += 6
        elif line[i : i + 2] == "\\\\":
            out.append("\\")
            i += 2
        else:
            out.append(line[i])
            i += 1
    return "".join(out)


def font_files(root):
    """Every font file under `root`, at any depth."""
    out = []
    for base, _, names in os.walk(root):
        for name in names:
            if name.lower().endswith((".ttf", ".otf", ".ttc", ".otc")):
                out.append(os.path.join(base, name))
    return sorted(out)


def ours(corpus, fonts):
    """`{(path, index): ((lgids, lpos), (vgids, vpos))}` from this crate.

    `l` is logical order, `v` is the order rule L2 draws in, and a position is
    `(advance, dx, dy)` in font units.
    """
    with tempfile.NamedTemporaryFile(
        "w", suffix=".txt", delete=False, encoding="utf-8", newline="\n"
    ) as f:
        f.write(f"{len(corpus)}\n")
        for entry in corpus:
            f.write(f"{lang_of(entry)}\t{string_of(entry)}\n")
        for path in fonts:
            f.write(path + "\n")
        input_path = f.name

    try:
        build = subprocess.run(
            [
                "cargo",
                "build",
                "--release",
                "-p",
                "osfont",
                "--target",
                TARGET,
                "--example",
                "shape_dump",
            ],
            cwd=REPO,
            capture_output=True,
            text=True,
        )
        if build.returncode != 0:
            sys.exit(build.stderr or "shape_dump did not build")
        exe = os.path.join(
            REPO, "target", TARGET, "release", "examples", "shape_dump.exe"
        )
        if not os.path.exists(exe):
            exe = exe[: -len(".exe")]
        run = subprocess.run(
            [exe, input_path], capture_output=True, text=True, encoding="utf-8"
        )
        if run.returncode != 0:
            sys.exit(run.stderr or "shape_dump failed")
    finally:
        os.unlink(input_path)

    out = {}
    for line in run.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) != 6:
            continue
        path, index, logical, visual, logical_pos, visual_pos = parts
        out[(path, int(index))] = (
            (gids(logical), positions(logical_pos)),
            (gids(visual), positions(visual_pos)),
        )
    return out


def gids(field):
    return [int(g) for g in field.split(",") if g]


def positions(field):
    return [tuple(int(n) for n in p.split(";")) for p in field.split(",") if p]


def theirs(path, questions):
    """`[([gid, ...], [(adv, dx, dy), ...], rtl), ...]`, or `None` if it will
    not open.

    `questions` is `[(bcp47 or "", text), ...]`. A named language is set on the
    buffer, which is how HarfBuzz reaches a LangSysRecord; it maps the tag with
    `hb_ot_tags_from_language`, the same function `lang.rs` is generated from,
    so the two halves select the same language system or the comparison is
    meaningless.

    `rtl` is the direction HarfBuzz guessed, and it decides which of our two
    orders its answer is comparable to. HarfBuzz does no bidi of its own: it
    picks one direction for the whole buffer, so its output is our *visual*
    order when it guessed right-to-left and our *logical* order when it did
    not. Comparing against the wrong one of those writes off every
    right-to-left string as a difference, which is what this sweep used to do.

    Positions are in font units: an `hb.Font` with no scale set reports the
    face's own design units, which is why `shape_dump` builds its face at the
    em size rather than at some pixel size that would round.
    """
    try:
        with open(path, "rb") as f:
            blob = hb.Blob(f.read())
        face = hb.Face(blob)
        font = hb.Font(face)
    except Exception:  # noqa: BLE001 - a face HarfBuzz rejects is not a result
        return None

    out = []
    for tag, string in questions:
        buf = hb.Buffer()
        buf.add_str(string)
        # The same thing this crate does not do: guess one script for the
        # whole string. For a single-script string that is the same answer an
        # itemizer gives; for a mixed one it is not, which is why `MIXED`
        # holds those two entries out of the verdict.
        buf.guess_segment_properties()
        # After the guess, which sets a language of its own from the locale.
        # An empty tag has to mean *no* language rather than "whatever this
        # machine is set to", or the sweep would compare our "no language"
        # against HarfBuzz's "en", and pass or fail by where it was run.
        buf.language = tag
        rtl = str(buf.direction).lower().endswith("rtl")
        try:
            hb.shape(font, buf)
        except Exception:  # noqa: BLE001
            out.append(None)
            continue
        out.append(
            (
                [info.codepoint for info in buf.glyph_infos],
                [
                    (round(p.x_advance), round(p.x_offset), round(p.y_offset))
                    for p in buf.glyph_positions
                ],
                rtl,
            )
        )
    return out


# One font unit. Both sides print integers, but they round independently — a
# position that is 249.5 in the design is allowed to be 249 on one side and 250
# on the other. Two units apart is a real difference at any em size.
TOLERANCE = 1


def places(pos):
    """`([(ink x, ink y), ...], total advance)` from `[(advance, dx, dy)]`.

    Compare *this*, not the raw advances, because the two engines charge a
    kern to different glyphs and both are right. HarfBuzz splits a legacy
    `kern` value in half: the left glyph's advance takes `k >> 1` and the
    right glyph takes the remainder in *both* its advance and its offset. We
    put the whole correction on the left glyph's advance, which is what makes
    a run's width the sum of its advances and what
    [`ShapedGlyph::kern_next`] documents. Arial Rounded Bold shaping `Th`
    (kern -27) is the worked example: HarfBuzz says advances 1266, 1224 with
    the `h` offset -13; we say 1253, 1237 with no offset. Every glyph lands on
    the same pixel and the run is the same width. Only a comparison of raw
    advances would call that a disagreement.
    """
    x = 0
    out = []
    for adv, dx, dy in pos:
        out.append((x + dx, dy))
        x += adv
    return out, x


def same_positions(ours_pos, expected_pos):
    ours, ours_width = places(ours_pos)
    expected, expected_width = places(expected_pos)
    if len(ours) != len(expected) or abs(ours_width - expected_width) > TOLERANCE:
        return False
    return all(
        all(abs(a - b) <= TOLERANCE for a, b in zip(one, other))
        for one, other in zip(ours, expected)
    )


def first_difference(ours_pos, expected_pos):
    """The first glyph the two disagree about, rather than the whole run."""
    ours, ours_width = places(ours_pos)
    expected, expected_width = places(expected_pos)
    if len(ours) != len(expected):
        return f"{len(ours)} glyphs vs {len(expected)}"
    for n, (one, other) in enumerate(zip(ours, expected)):
        if any(abs(a - b) > TOLERANCE for a, b in zip(one, other)):
            return f"glyph {n} at {one} vs {other}"
    if abs(ours_width - expected_width) > TOLERANCE:
        return f"width {ours_width} vs {expected_width}"
    return "no difference"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--fonts",
        default=os.path.join(os.environ.get("WINDIR", "/usr/share"), "Fonts"),
        help="directory to sweep (default: the host's font directory)",
    )
    ap.add_argument(
        "--limit", type=int, default=0, help="stop after this many faces (0 = all)"
    )
    args = ap.parse_args()

    if not os.path.isdir(args.fonts):
        sys.exit(f"{args.fonts} is not a directory")
    fonts = font_files(args.fonts)
    if args.limit:
        fonts = fonts[: args.limit]
    if not fonts:
        sys.exit(f"no fonts under {args.fonts}")

    questions = [(lang_of(entry), unescape(string_of(entry))) for entry in CORPUS]
    print(f"{len(fonts)} faces x {len(questions)} strings")
    mine = ours(CORPUS, fonts)

    agree = 0
    order_only = Counter()
    placed = Counter()
    differ = Counter()
    mixed = Counter()
    examples = {}
    placed_examples = {}
    skipped = 0
    for path in fonts:
        hb_out = theirs(path, questions)
        if hb_out is None:
            skipped += 1
            continue
        for i, answer in enumerate(hb_out):
            got = mine.get((path, i))
            if got is None or answer is None:
                continue
            expected, expected_pos, rtl = answer
            # Ours in the order HarfBuzz was asked for: visual when it decided
            # the buffer was right-to-left, logical when it did not.
            logical, visual = got
            ours_here, ours_pos = visual if rtl else logical
            if CORPUS[i] in MIXED:
                # Not a verdict on the shaper — see `MIXED`. Counted as
                # agreement only when the itemizer happened not to matter for
                # this face, which is the interesting half of the answer.
                if ours_here == expected and same_positions(ours_pos, expected_pos):
                    agree += 1
                else:
                    mixed[CORPUS[i]] += 1
            elif ours_here == expected:
                if same_positions(ours_pos, expected_pos):
                    agree += 1
                else:
                    # The glyphs are right and they are in the right places
                    # relative to each other only if this is empty. Kerning
                    # and mark attachment live here and nowhere else in this
                    # sweep: a mark stacked on the wrong base picks the same
                    # glyph id as one stacked on the right base.
                    placed[CORPUS[i]] += 1
                    placed_examples.setdefault(
                        CORPUS[i],
                        (os.path.basename(path), ours_pos, expected_pos),
                    )
            elif sorted(ours_here) == sorted(expected):
                # Same glyphs, different order. Now that this crate resolves
                # bidi and HarfBuzz still does not, this is where the mixed
                # strings land: HarfBuzz guesses one direction for the whole
                # buffer and leaves the Arabic half of a Latin sentence
                # backwards, where we reorder each run on its own. The
                # *shaping* agreed exactly, so it is worth separating from a
                # real disagreement rather than burying in the total.
                order_only[CORPUS[i]] += 1
            else:
                differ[CORPUS[i]] += 1
                examples.setdefault(
                    CORPUS[i], (os.path.basename(path), ours_here, expected)
                )

    print(f"agree    {agree}  (same glyphs, same positions)")
    print(f"reordered {sum(order_only.values())}  (same glyphs, different order)")
    print(f"misplaced {sum(placed.values())}  (same glyphs, different positions)")
    print(f"differ   {sum(differ.values())}")
    print(f"mixed    {sum(mixed.values())}  (itemizer, not shaper — see MIXED)")
    print(f"faces HarfBuzz would not open: {skipped}")
    if placed:
        print("\nsame glyphs in different places, by string:")
        for text, n in placed.most_common():
            face, got, expected = placed_examples[text]
            print(f"  {n:5}  {text!r}")
            print(f"         e.g. {face}: {first_difference(got, expected)}")
    if order_only:
        print("\nsame glyphs in a different order, by string:")
        for text, n in order_only.most_common():
            print(f"  {n:5}  {text!r}")
    if mixed:
        print("\nmixed-script strings, where HarfBuzz guessed one script:")
        for text, n in mixed.most_common():
            print(f"  {n:5}  {text!r}")
    if differ:
        print("\nby string, most disagreed first:")
        for text, n in differ.most_common():
            face, got, expected = examples[text]
            print(f"  {n:5}  {text!r}")
            print(f"         e.g. {face}: ours {got} vs harfbuzz {expected}")


if __name__ == "__main__":
    main()
