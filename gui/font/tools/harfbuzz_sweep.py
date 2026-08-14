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
    """`{(path, index): [gid, ...]}` from this crate's shaper."""
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
        if len(parts) != 3:
            continue
        path, index, gids = parts
        out[(path, int(index))] = [int(g) for g in gids.split(",") if g]
    return out


def theirs(path, strings):
    """`[[gid, ...], ...]` from HarfBuzz, or `None` if it will not open."""
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
        try:
            hb.shape(font, buf)
        except Exception:  # noqa: BLE001
            out.append(None)
            continue
        out.append([info.codepoint for info in buf.glyph_infos])
    return out


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
    reversed_only = Counter()
    differ = Counter()
    examples = {}
    skipped = 0
    for path in fonts:
        hb_out = theirs(path, strings)
        if hb_out is None:
            skipped += 1
            continue
        for i, expected in enumerate(hb_out):
            got = mine.get((path, i))
            if got is None or expected is None:
                continue
            if got == expected:
                agree += 1
            elif got == expected[::-1]:
                # Same glyphs, opposite order: HarfBuzz reverses a
                # right-to-left buffer so the caller can draw it left to
                # right, and this crate does not reorder at all yet. The
                # *shaping* agreed exactly, which for Arabic is the whole
                # question — so this is worth separating from a real
                # disagreement rather than burying in the total.
                reversed_only[CORPUS[i]] += 1
            else:
                differ[CORPUS[i]] += 1
                examples.setdefault(
                    CORPUS[i], (os.path.basename(path), got, expected)
                )

    print(f"agree    {agree}")
    print(f"reversed {sum(reversed_only.values())}  (same glyphs, RTL order)")
    print(f"differ   {sum(differ.values())}")
    print(f"faces HarfBuzz would not open: {skipped}")
    if reversed_only:
        print("\nreversed only, by string:")
        for text, n in reversed_only.most_common():
            print(f"  {n:5}  {text!r}")
    if differ:
        print("\nby string, most disagreed first:")
        for text, n in differ.most_common():
            face, got, expected = examples[text]
            print(f"  {n:5}  {text!r}")
            print(f"         e.g. {face}: ours {got} vs harfbuzz {expected}")


if __name__ == "__main__":
    main()
