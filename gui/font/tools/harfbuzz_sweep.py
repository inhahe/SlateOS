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
    "hello \\u05e9\\u05dc\\u05d5\\u05dd world",
    "abc \\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629 xyz",
    # Arabic on its own, where a face with Arabic features should use them.
    "\\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629",
    # Fully vowelled Arabic — `bismi`, a letter and a mark alternating. Every
    # lookup here has to step over the marks to see the letters, so this is
    # the string that exercises `IgnoreMarks`; the unvowelled entry above
    # cannot, because it has no marks to ignore.
    "\\u0628\\u0650\\u0633\\u0652\\u0645\\u0650",
    # Devanagari, the reason script tags have two spellings.
    "\\u0939\\u093f\\u0928\\u094d\\u0926\\u0940",
    # Scriptless text, which selects the font's default features.
    "123 456",
]


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
        for line in corpus:
            f.write(line + "\n")
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


def theirs(path, strings):
    """`[([gid, ...], [(adv, dx, dy), ...], rtl), ...]`, or `None` if it will
    not open.

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
    for text in strings:
        buf = hb.Buffer()
        buf.add_str(text)
        # The same thing this crate does not do: guess one script for the
        # whole string. It is the divergence the mixed-script entries in the
        # corpus are there to expose.
        buf.guess_segment_properties()
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

    strings = [unescape(line) for line in CORPUS]
    print(f"{len(fonts)} faces x {len(strings)} strings")
    mine = ours(CORPUS, fonts)

    agree = 0
    order_only = Counter()
    placed = Counter()
    differ = Counter()
    examples = {}
    placed_examples = {}
    skipped = 0
    for path in fonts:
        hb_out = theirs(path, strings)
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
            if ours_here == expected:
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
    if differ:
        print("\nby string, most disagreed first:")
        for text, n in differ.most_common():
            face, got, expected = examples[text]
            print(f"  {n:5}  {text!r}")
            print(f"         e.g. {face}: ours {got} vs harfbuzz {expected}")


if __name__ == "__main__":
    main()
