"""Count the unbounded vertical-centring sites the C-CENTRING-IS-NOT-A-BOUND
campaign is working through.

The shape is `band.y + (band.h - size) / 2.0`: an *offset*, not a bound.  When
the band is shorter than the thing being centred in it the slack goes negative
and the run is placed above the band's top, spilling symmetrically out of both
ends -- so nothing in the picture says which band the fault is in.  The fix is
a `centre_line(band, size) -> Option<f32>` that answers `None` instead.

The campaign's per-app tally was being taken with ad-hoc greps, which do not
agree with each other: a pattern loose enough to catch the wrapped forms also
catches horizontal centrings and the already-fixed helper, and a pattern tight
enough to avoid those misses every site rustfmt happened to wrap.  A number
quoted in `known-issues.md` that cannot be reproduced is worse than no number,
so this is the one definition.

What counts:

* a `.y +` (or `.top() +`, or a bare `y +` on a local) followed by a halved
  difference in which the subtrahend is a `.h`/`height` term -- across line
  breaks, since rustfmt wraps these freely;
* in `apps/*/src/main.rs`, outside `#[cfg(test)]`, outside comments.

What does not:

* the body of `centre_line` itself, and of the `centred_in`/`label_centred`
  helpers that call it -- an app that has been through the campaign has
  exactly one such site left and it is the fix, not the fault;
* horizontal centrings (`.x + (w - ...) / 2.0`), which are a different and
  much less severe shape: a run centred in a box too narrow is clipped or
  ellipsised, not moved outside the box it belongs to.

Usage:  python scripts/count_centrings.py [app ...]
        python scripts/count_centrings.py --sites <app>
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Apps that have been through the campaign: they carry `centre_line` and a
# `no_pass_paints_outside_the_region_it_owns` test.  Detected, not listed, so
# this cannot go stale.
DONE_MARKER = "fn centre_line("

# `.y +` / `y +` ... `( ... .h|height ... ) / 2.0`, across line breaks.
SITE = re.compile(
    r"(?:\.y|\.top\(\)|\by)\s*\+\s*\(\s*[^();]{0,120}?"
    r"(?:\.h\b|height|_h\b)[^();]{0,120}?\)\s*/\s*2\.0",
    re.S,
)

# The helpers whose whole job is to be the one place this shape lives.
HELPERS = ("fn centre_line(", "fn centred_in(", "fn label_centred(")


def strip_comments(text):
    """Blank out `//` comments, keeping offsets so line numbers stay right."""
    out = []
    for line in text.split("\n"):
        i = line.find("//")
        out.append(line[:i] + " " * (len(line) - i) if i >= 0 else line)
    return "\n".join(out)


def helper_spans(text):
    """Byte ranges of the helper bodies, which are the fix and not the fault."""
    spans = []
    for h in HELPERS:
        start = text.find(h)
        while start >= 0:
            depth, i, seen = 0, start, False
            while i < len(text):
                if text[i] == "{":
                    depth += 1
                    seen = True
                elif text[i] == "}":
                    depth -= 1
                    if seen and depth == 0:
                        break
                i += 1
            spans.append((start, i))
            start = text.find(h, i)
    return spans


def count(path):
    text = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
    tests = text.find("\nmod tests {")
    if tests < 0:
        tests = len(text)
    spans = helper_spans(text)
    hits = []
    for m in SITE.finditer(text, 0, tests):
        if any(a <= m.start() < b for a, b in spans):
            continue
        hits.append(text.count("\n", 0, m.start()) + 1)
    return hits


def main(argv):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    show_sites = "--sites" in argv
    names = [a for a in argv[1:] if not a.startswith("--")]
    paths = (
        [ROOT / "apps" / n / "src" / "main.rs" for n in names]
        if names
        else sorted((ROOT / "apps").glob("*/src/main.rs"))
    )

    rows, done = [], []
    for p in paths:
        if not p.exists():
            print(f"no such app: {p}")
            return 2
        hits = count(p)
        app = p.parent.parent.name
        if DONE_MARKER in p.read_text(encoding="utf-8", errors="replace"):
            done.append((app, hits))
        elif hits:
            rows.append((app, hits))

    rows.sort(key=lambda r: (-len(r[1]), r[0]))
    for app, hits in rows:
        line = f"{len(hits):3}  {app}"
        if show_sites:
            line += "  " + ",".join(str(h) for h in hits)
        print(line)

    total = sum(len(h) for _, h in rows)
    print(f"\n{len(rows)} apps, {total} sites remain")
    if done:
        print(f"{len(done)} done: " + ", ".join(a for a, _ in done))
        # A done app with sites left is the interesting case, not the boring
        # one: either the site is genuinely bounded by construction (in which
        # case the proof is in a comment beside it, and this is a false
        # positive worth knowing the shape of) or the campaign missed it.
        for app, hits in done:
            if hits:
                line = f"     {app}: {len(hits)} still matching"
                if show_sites:
                    line += " at " + ",".join(str(h) for h in hits)
                print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
