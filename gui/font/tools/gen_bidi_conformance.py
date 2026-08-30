"""Fetch Unicode's bidi conformance suite and cut a checked-in sample from it.

Run from anywhere:

    python gui/font/tools/gen_bidi_conformance.py          # sample + full copy
    python gui/font/tools/gen_bidi_conformance.py --stats  # just report

Why a sample rather than the whole file
---------------------------------------

`BidiCharacterTest.txt` is the real oracle: ninety-odd thousand strings, each
with the paragraph level, the per-character embedding levels and the visual
order that a conforming implementation must produce. It is also 6.8 MB, which
is not a thing to put in a source tree that is cloned on every machine.

So two files come out of this script:

  `gui/font/tests/data/bidi_conformance.txt`
      A sample, checked in, run by `tests/bidi.rs` on every `cargo test`. It is
      chosen to cover every *shape* of case in the suite rather than to be a
      uniform slice — see `sample()` — so a rule that breaks shows up in the
      ordinary test run rather than only in a nightly sweep.

  `gui/font/tests/data/BidiCharacterTest.txt`
      The whole suite, git-ignored. `tests/bidi.rs` runs it too when it is
      present, under `--ignored`, which is how the full ninety thousand get
      checked before a commit that touches `bidi.rs`.

The file format, which both tests parse:

    codepoints ; direction ; paragraph level ; levels ; visual order

`direction` is 0 for left-to-right, 1 for right-to-left, 2 for auto (rule P2).
`levels` has one entry per input character, or `x` where the character was
removed by rule X9 and has no level to check. `visual order` lists the indices
of the *remaining* characters in the order they are drawn.
"""

import argparse
import os
import unicodedata
import urllib.request

URL = "https://www.unicode.org/Public/{version}/ucd/BidiCharacterTest.txt"

# How many cases the checked-in sample aims for. Big enough that every rule is
# exercised many times over, small enough that the file stays a few hundred
# kilobytes and `cargo test` stays fast.
TARGET = 4000


def fetch(version):
    with urllib.request.urlopen(URL.format(version=version), timeout=300) as r:
        return r.read().decode("utf-8", errors="replace")


def cases(text):
    """`(line, fields)` for every test case, comments and blanks dropped."""
    out = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        fields = stripped.split(";")
        if len(fields) != 5:
            raise SystemExit(f"unexpected field count in {stripped!r}")
        out.append((stripped, fields))
    return out


def shape(fields):
    """A key describing what a case *exercises*, for stratified sampling.

    Two cases with the same key test the same combination of features, so
    keeping a bounded number of each covers the suite's variety far better than
    taking every twentieth line — which would take twenty thousand near-copies
    of the commonest shape and none of the rare ones.
    """
    chars = [chr(int(c, 16)) for c in fields[0].split()]
    classes = frozenset(unicodedata.bidirectional(c) or "L" for c in chars)
    return (
        classes,
        fields[1],                      # the requested paragraph direction
        min(len(chars), 8),             # length, saturated
        any(unicodedata.bidirectional(c) in ("ON",) and _is_bracket(c)
            for c in chars),            # does rule N0 have anything to do
    )


_BRACKETS = None


def _is_bracket(ch):
    global _BRACKETS
    if _BRACKETS is None:
        version = unicodedata.unidata_version
        url = f"https://www.unicode.org/Public/{version}/ucd/BidiBrackets.txt"
        with urllib.request.urlopen(url, timeout=120) as r:
            text = r.read().decode("utf-8", errors="replace")
        _BRACKETS = set()
        for line in text.splitlines():
            line = line.split("#", 1)[0].strip()
            if line:
                _BRACKETS.add(chr(int(line.split(";")[0].strip(), 16)))
    return ch in _BRACKETS


def sample(all_cases):
    """A stratified sample: up to `per` cases of each distinct shape.

    The buckets are very uneven — a few shapes have tens of thousands of cases
    and most have a handful — so `TARGET // len(buckets)` undershoots badly:
    the small buckets cannot fill their quota and nothing takes up the slack.
    Instead search for the per-bucket cap that lands the total nearest TARGET,
    which lets the big buckets absorb what the small ones cannot supply.
    """
    by_shape = {}
    for line, fields in all_cases:
        by_shape.setdefault(shape(fields), []).append(line)

    def total(per):
        return sum(min(per, len(v)) for v in by_shape.values())

    per, lo, hi = 1, 1, max(len(v) for v in by_shape.values())
    while lo <= hi:
        mid = (lo + hi) // 2
        if total(mid) <= TARGET:
            per, lo = mid, mid + 1
        else:
            hi = mid - 1

    out = []
    for key in sorted(by_shape, key=lambda k: (sorted(k[0]), k[1], k[2], k[3])):
        lines = by_shape[key]
        # Spread the picks across the bucket rather than taking its first few,
        # since the suite is generated in a systematic order.
        step = max(1, len(lines) // per)
        out.extend(lines[::step][:per])
    return out, len(by_shape)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stats", action="store_true", help="report, write nothing")
    args = ap.parse_args()

    version = unicodedata.unidata_version
    text = fetch(version)
    all_cases = cases(text)
    picked, shapes = sample(all_cases)

    print(f"Unicode {version}")
    print(f"  {len(all_cases)} cases in {shapes} distinct shapes")
    print(f"  sampled {len(picked)}")
    if args.stats:
        return

    here = os.path.dirname(os.path.abspath(__file__))
    data = os.path.join(here, "..", "tests", "data")
    os.makedirs(data, exist_ok=True)

    full = os.path.join(data, "BidiCharacterTest.txt")
    with open(full, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {full} ({len(text)} bytes, git-ignored)")

    out = os.path.join(data, "bidi_conformance.txt")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write(
            f"# A stratified sample of Unicode {version}'s "
            "BidiCharacterTest.txt.\n"
            "#\n"
            "# Generated by gui/font/tools/gen_bidi_conformance.py. Do not\n"
            "# edit: run the script instead. See it for why this is a sample\n"
            "# and where the whole suite lives.\n"
            "#\n"
            "# codepoints ; direction ; paragraph level ; levels ; visual order\n"
        )
        for line in picked:
            f.write(line + "\n")
    print(f"wrote {out} ({len(picked)} cases)")


if __name__ == "__main__":
    main()
