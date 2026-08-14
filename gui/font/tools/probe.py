"""Ad-hoc: shape one string on one face both ways and print the two answers.

    python gui/font/tools/probe.py <font path> <string with \\uXXXX escapes>

The sweep says *how many* faces disagree; this says *what* the disagreement is
on one of them. Not a test — a magnifying glass, kept because reaching for it
is the first step of every font bug and rewriting it each time is how a
transcription error gets into a diagnosis.
"""

import sys

import harfbuzz_sweep as sweep


def main():
    path, raw = sys.argv[1], sys.argv[2]
    text = raw.encode("utf-8").decode("unicode_escape")
    got = sweep.ours([raw], [path])
    (lgids, lpos), (vgids, vpos) = got[(path, 0)]
    want = sweep.theirs(path, [text])[0]
    hgids, hpos, rtl = want

    print(f"{path}  rtl={rtl}  {[hex(ord(c)) for c in text]}")
    print(f"  ours logical {lgids}")
    print(f"       {lpos}")
    print(f"  ours visual  {vgids}")
    print(f"       {vpos}")
    print(f"  harfbuzz     {hgids}")
    print(f"       {hpos}")
    mine = vpos if rtl else lpos
    print(f"  places ours     {sweep.places(mine)}")
    print(f"  places harfbuzz {sweep.places(hpos)}")
    print(f"  first difference: {sweep.first_difference(mine, hpos)}")


if __name__ == "__main__":
    main()
