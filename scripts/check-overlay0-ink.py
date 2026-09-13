#!/usr/bin/env python3
"""Refuse `overlay0` as the ink of text that is not in a disabled state.

`overlay0` is the palette's deliberately-faint grey, and it fails the 4.5:1
contrast floor on all six surfaces by design -- 2.30:1 on `base`, 1.21:1 on
`surface2`. WCAG exempts *disabled* controls from the floor precisely so that
off can look off, which is what the role is for: `if enabled { pal.text } else
{ pal.overlay0 }` is correct and this gate leaves it alone.

What it refuses is the other shape -- a text draw inked `overlay0` where
nothing anywhere in the enclosing function is conditional on being enabled.
That is live text at 2.30:1, and on 2026-09-13 there were 208 of them: section
headings ("Hourly Forecast"), instructions ("Press F5 or click Run to start
benchmarking"), chart axis labels, a cursor position in a status bar.

**Why a source gate rather than a test.** The property is about *where a
colour is used*, which no amount of rendering can observe: a heading drawn in
the disabled grey composites perfectly and passes every pixel assertion. The
contrast guard in `gui/appearance/src/palette_check.rs` cannot see it either,
because the palette is not wrong -- `overlay0` is supposed to fail. Only the
draw site knows whether the thing being drawn is switched off.

**The three verdicts**, and why the middle one is allowed:

  live       no disabled/enabled word anywhere in the enclosing function.
             Refused.
  ambiguous  such a word is in the function but not beside the draw. Allowed,
             because "not beside a condition" is not the same as "not
             conditional", and a disabled label that stops looking disabled is
             this same bug pointing the other way. Counted, so the number is
             visible; see TD-C-OVERLAY0-IS-A-DISABLED-INK.
  exempt     the draw sits with its own enabled/disabled test. Allowed.

Usage:  python scripts/check-overlay0-ink.py [--list]
"""
import collections
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "gui" / "appearance"))
from rustslice import production_end  # noqa: E402

NL = chr(10)

# Lane C's trees. The roles are `appearance`'s, and nothing outside these
# directories draws with them.
ROOTS = ("gui", "apps")

EXEMPT = re.compile(
    r"\b(enabled|disabled|is_empty|is_none|available|unavailable|active|inactive|"
    r"selectable|editable|locked|greyed|grayed|readonly|read_only)\b", re.I)
FIELD = re.compile(r"^\s*color:\s*(.*)$")
FN = re.compile(r"^\s{0,8}(pub\s+)?(const\s+)?(async\s+)?fn\s")
OV = re.compile(r"\boverlay0\b")
CALL = re.compile(r"\.(text|text_in|text_in_weighted)\s*\(")
# Which argument carries the ink. `text(x, y, s, ink, size)` and
# `text_in[_weighted](x, y, w, s, ink, size, ..)`.
COLOR_ARG = {"text": 3, "text_in": 4, "text_in_weighted": 4}


def arg_spans(text, start):
    """(begin, end) of each depth-zero argument, as offsets into `text`."""
    depth, spans, begin = 0, [], start
    for pos in range(start, len(text)):
        ch = text[pos]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth < 0:
                spans.append((begin, pos))
                return spans
        if ch == "," and depth == 0:
            spans.append((begin, pos))
            begin = pos + 1
    return None


def verdict(lines, i):
    """`exempt`, `ambiguous` or `live` for a draw whose ink is on line `i`."""
    if EXEMPT.search(NL.join(lines[max(0, i - 9):i + 2])):
        return "exempt"
    start = 0
    for k in range(i, max(0, i - 400), -1):
        if FN.match(lines[k]):
            start = k
            break
    return "ambiguous" if EXEMPT.search(NL.join(lines[start:i + 2])) else "live"


def sites(path):
    """Every text draw in this file whose ink names `overlay0`, with a verdict.

    Production code only: a test that asserts the disabled grey *is* the
    disabled grey is the guard working, not a violation of it.
    """
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    end = production_end(lines)
    found = []

    # The struct form, where the ink is a field several lines below the call.
    for i in range(end):
        fm = FIELD.match(lines[i])
        if not fm or not OV.search(fm.group(1)):
            continue
        kind = None
        for k in range(i, max(0, i - 24), -1):
            km = re.search(r"RenderCommand::(\w+)", lines[k])
            if km:
                kind = km.group(1)
                break
        # A `Rect`'s `color` is a fill, not an ink, and a faint separator is
        # exactly what `overlay0` is for.
        if kind == "Text":
            found.append((i + 1, verdict(lines, i), lines[i].strip()[:72]))

    # The positional form.
    blob = NL.join(lines[:end])
    for m in CALL.finditer(blob):
        spans = arg_spans(blob, m.end())
        if spans is None:
            continue
        idx = COLOR_ARG[m.group(1)]
        if idx >= len(spans) or not OV.search(blob[spans[idx][0]:spans[idx][1]]):
            continue
        i = blob.count(NL, 0, m.start())
        found.append((i + 1, verdict(lines, i), lines[i].strip()[:72]))

    return sorted(found)


# Fixtures. Each is a whole file, because the verdicts depend on where the
# `fn` starts and where the test module begins -- the two things a fixture of
# snippets would quietly lose.
SELF_TESTS = [
    (
        "a heading in the disabled grey is live text",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: 8.0,
        text: "Hourly Forecast".to_string(),
        color: self.palette.overlay0,
        font_size: 13.0,
    });
}
""",
        {"live": 1},
    ),
    (
        "the same draw beside its own enabled test is exempt",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    let ink = if self.enabled { p.text } else { p.overlay0 };
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: 8.0,
        text: "Hourly Forecast".to_string(),
        color: self.palette.overlay0,
        font_size: 13.0,
    });
}
""",
        {"exempt": 1},
    ),
    (
        "a conditional elsewhere in the function is ambiguous, not clean",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    if !self.enabled {
        return;
    }
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    let f = 6;
    let g = 7;
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: 8.0,
        text: "Hourly Forecast".to_string(),
        color: self.palette.overlay0,
        font_size: 13.0,
    });
}
""",
        {"ambiguous": 1},
    ),
    (
        "the positional call form is seen too",
        """fn render(&self, tree: &mut RenderTree) {
    tree.text(8.0, 8.0, &label, self.palette.overlay0, 11.0);
}
""",
        {"live": 1},
    ),
    (
        "a rectangle filled with overlay0 is a separator, not an ink",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    cmds.push(RenderCommand::Rect {
        x: 8.0,
        y: 8.0,
        color: self.palette.overlay0,
    });
}
""",
        {},
    ),
    (
        "a draw inside the test module is not production code",
        """fn render(&self) {}

#[cfg(test)]
mod tests {
    fn a_disabled_label_is_drawn_in_overlay0() {
        tree.text(8.0, 8.0, &label, p.overlay0, 11.0);
    }
}
""",
        {},
    ),
    (
        "a string that merely contains the word is not a draw",
        """fn render(&self, tree: &mut RenderTree) {
    tree.text(8.0, 8.0, "overlay0 is the disabled grey", p.subtext0, 11.0);
}
""",
        {},
    ),
]


def self_test() -> int:
    """Grade the detector against files whose verdicts are known.

    The failure this exists to catch is the one every scanner has: a rule that
    stops matching reports an empty finding set, and an empty finding set is
    printed in exactly the words a clean tree is. Three of the seven cases
    below expect *no* finding, so a detector that has stopped looking fails
    the other four rather than passing everything.
    """
    import tempfile

    failed = 0
    for name, source, expected in SELF_TESTS:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fixture.rs"
            path.write_text(source, encoding="utf-8", newline=NL)
            got = collections.Counter(v for _, v, _ in sites(path))
        ok = dict(got) == expected
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            print(f"       expected {expected}, got {dict(got)}")
            failed += 1
    print(f"{NL}{len(SELF_TESTS)} self-test case(s), {failed} failed")
    return 1 if failed else 0


def main():
    if "--self-test" in sys.argv or "--selftest" in sys.argv:
        return self_test()
    show_all = "--list" in sys.argv
    counts = collections.Counter()
    bad = []
    files = 0
    for top in ROOTS:
        for path in sorted((ROOT / top).rglob("*.rs")):
            files += 1
            for line, v, src in sites(path):
                counts[v] += 1
                if v == "live" or show_all:
                    bad.append((v, path.relative_to(ROOT).as_posix(), line, src))

    for v, rel, line, src in bad:
        if v == "live":
            print(f"{rel}:{line}: live text inked overlay0 (2.30:1 on base)", file=sys.stderr)
            print(f"    {src}", file=sys.stderr)
        elif show_all:
            print(f"{rel}:{line}: {v}")
            print(f"    {src}")

    total = sum(counts.values())
    if counts["live"]:
        print(
            f"{NL}{counts['live']} text draw(s) use overlay0 with nothing disabled. "
            f"Live text belongs in `subtext0` (9.58:1 on base); overlay0 is for "
            f"the off state only.",
            file=sys.stderr,
        )
        return 1
    # The corpus travels with the verdict, so that a reader can tell a thorough
    # scan from one that found nothing because it looked nowhere.
    print(
        f"ok -- no live text inked overlay0 ({files} file(s), {total} overlay0 "
        f"text draw(s): {counts['exempt']} beside their own enabled/disabled "
        f"test, {counts['ambiguous']} conditional somewhere in the function)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
