"""Print what HarfBuzz does to a run, one shaping stage at a time.

The companion to `harfbuzz_sweep.py`, and the instrument to reach for when the
sweep says a run is misplaced and the question is *why*. The sweep compares
finished output; this shows the buffer after every stage HarfBuzz announces,
so the stage that moved a glyph names itself instead of being guessed at.

It was written after five hypotheses in a row were spent guessing which
predicate HarfBuzz used to zero a mark, when the answer was that the routine
under suspicion was not the one doing it. See known-issues.md,
`TD-FONT-A-ZEROED-MARK-IS-NOT-PARKED-ON-ITS-BASE`.

    python gui/font/tools/hb_trace.py 0F40 0F72 0F74
    python gui/font/tools/hb_trace.py --font C:/Windows/Fonts/Hack-Bold.ttf A98F A9C0

Characters are given as hex codepoints rather than literal text on purpose:
a Windows console is cp1252 and cannot pass most of the scripts worth tracing.

Needs `uharfbuzz`: pip install uharfbuzz
"""

import argparse
import os
import sys

try:
    import uharfbuzz as hb
except ImportError:
    sys.exit("uharfbuzz is not installed: pip install uharfbuzz")


def state(buf):
    """The buffer as a list, or None before it holds glyphs."""
    try:
        return [
            (info.codepoint, pos.x_advance, pos.y_advance, pos.x_offset, pos.y_offset)
            for info, pos in zip(buf.glyph_infos, buf.glyph_positions)
        ]
    except TypeError:
        # Before the first stage the buffer holds characters, not glyphs, and
        # `glyph_positions` is null. That is a stage worth showing, not an error.
        return None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("codepoints", nargs="+", help="hex codepoints, e.g. 0F40 0F72")
    ap.add_argument(
        "--font",
        default=os.path.join(os.environ.get("WINDIR", "/usr/share"), "Fonts", "Hack-Bold.ttf"),
        help="the face to shape with (default: Hack-Bold)",
    )
    args = ap.parse_args()

    if not os.path.exists(args.font):
        sys.exit("no such face: %s" % args.font)
    with open(args.font, "rb") as handle:
        face = hb.Face(hb.Blob(handle.read()))
    font = hb.Font(face)

    text = "".join(chr(int(c, 16)) for c in args.codepoints)
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()

    print("face   %s" % args.font)
    print("run    %s" % " ".join("U+%04X" % ord(c) for c in text))
    print("script %s" % buf.script)
    print("glyph classes in GDEF: %s"
          % ("yes" if hb.ot_layout_has_glyph_classes(face) else "no -- HarfBuzz will synthesize them"))
    print()

    stages = []
    buf.set_message_func(lambda msg: stages.append((msg, state(buf))) or True)
    hb.shape(font, buf)

    previous = None
    for message, seen in stages:
        if seen is None:
            print("%-38s (no glyphs yet)" % message)
        elif seen == previous:
            print("%-38s (unchanged)" % message)
        else:
            print("%-38s %s" % (message, seen))
            previous = seen
    print("\n%-38s %s" % ("FINAL (gid, xadv, yadv, xoff, yoff)", state(buf)))


if __name__ == "__main__":
    main()
