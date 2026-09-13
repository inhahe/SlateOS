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

**Exempt means one thing: the draw is a switched-off state.** Three positions
say so, and nothing else does -- the ink expression itself, the enclosing
function's name (`fn render_disabled_button`), and any block that structurally
encloses the draw (`if !self.enabled { .. }`).

**What is deliberately not exempt**, because the first version of this gate got
all of these wrong and let live text through:

  * **Emptiness.** `if list.is_empty() { draw("No devices found") }` is an
    empty-state message -- often the only thing in the pane, and the one
    sentence the user has to read. WCAG exempts *disabled controls*; it says
    nothing about empty lists. Sixty-nine draws hid behind `is_empty` alone.
  * **The word appearing in the drawn string or in a comment.** `text: "No
    updates available."`, `text: "Disabled".to_string()`, and a comment reading
    "the placeholder is not editable text" all satisfied a proximity rule, and
    none of them is a condition. Strings and comments are blanked before any
    match.
  * **Proximity at all.** A nine-line window missed `fn render_disabled_button`
    by one line.

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

# Being switched off, and nothing else. `is_empty`, `is_none`, `available`
# and `active` were here and are not: an empty list is not a disabled
# control, and "available" mostly turned up inside the sentence being drawn.
# Boundaries are non-alphanumeric rather than `\b`, because an underscore is a
# word character: `\bdisabled\b` does not match inside
# `render_disabled_button` or `wifi_enabled`, which is most of how these
# words actually appear in code.
EXEMPT = re.compile(
    r"(?<![A-Za-z0-9])(enabled|disabled|unavailable|inactive|insensitive"
    r"|selectable|editable|locked|greyed|grayed|readonly)(?![A-Za-z0-9])", re.I)
STRING = re.compile(r'"(?:[^"\\]|\\.)*"')
COMMENT = re.compile(r"//.*$")


def code_only(line):
    """The line with string literals and comments blanked out.

    A condition is code. `text: "No updates available."` is not a condition,
    and neither is a comment mentioning a disabled state -- both matched the
    first version of this gate and exempted live text.
    """
    return COMMENT.sub("", STRING.sub('""', line))


def enclosing_conditions(lines, i, fn_start):
    """Every block opener that encloses line `i`, innermost first.

    Walks out one brace at a time rather than reading the lines above, which
    is the difference between "the draw is inside `if !enabled {`" and "the
    word `enabled` occurs somewhere nearby".
    """
    out, depth = [], 0
    for k in range(i - 1, fn_start - 1, -1):
        line = lines[k]
        depth += line.count("}") - line.count("{")
        if depth < 0:
            out.append(line)
            depth = 0
    return out
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
    """`exempt` or `live` for a draw whose ink is on line `i`.

    There is no third answer any more. The old `ambiguous` verdict meant "a
    disabled-word occurs somewhere in this function", which described 169 live
    draws and 10 real ones -- and its 9-line proximity half missed
    `fn render_disabled_button` by a single line.
    """
    # The ink expression itself: `if self.enabled { .. } else { p.overlay0 }`,
    # which may run over a few lines.
    if EXEMPT.search(code_only(NL.join(lines[max(0, i - 3):i + 1]))):
        return "exempt"
    start = 0
    # `-1` as the stop, not `0`: a `range(i, 0, -1)` never yields index 0, and
    # a free function at the top of a file has its signature exactly there.
    # That off-by-one hid `fn render_disabled_button` from its own name.
    for k in range(i, max(-1, i - 400), -1):
        if FN.match(lines[k]):
            start = k
            break
    # The function's own name. `fn render_disabled_button` says everything it
    # draws is the disabled rendering, and it is the only place that says so:
    # there is no condition inside, because the caller already decided.
    if EXEMPT.search(code_only(lines[start])):
        return "exempt"
    # A block that encloses the draw -- `if !self.enabled { .. }`. Found by
    # walking out one brace at a time, which is the difference between "the
    # draw is inside the condition" and "the words are near each other".
    for opener in enclosing_conditions(lines, i, start):
        if EXEMPT.search(code_only(opener)):
            return "exempt"
    return "live"


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
        if not fm:
            continue
        # The value, which is not always on this line. A conditional ink --
        #
        #     color: if self.query.is_empty() {
        #         p.overlay0
        #     } else {
        #         p.text
        #     },
        #
        # puts the role three lines below its field, and a rule that reads only
        # the `color:` line cannot see it. Five draws hid there, four of them
        # placeholders, and they were found by a failing test rather than by
        # this gate -- which is the failure this whole file exists to prevent.
        value, depth, j = fm.group(1), 0, i
        while j < end:
            for ch in (fm.group(1) if j == i else lines[j]):
                depth += (ch in "([{") - (ch in ")]}")
            if depth <= 0 and (j > i or fm.group(1).rstrip().endswith(",")):
                break
            j += 1
            if j < end:
                value += NL + lines[j]
        if not OV.search(value):
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
        "the ink expression's own enabled test is exempt",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: 8.0,
        text: "Wi-Fi".to_string(),
        color: if self.enabled { p.text } else { p.overlay0 },
        font_size: 13.0,
    });
}
""",
        {"exempt": 1},
    ),
    (
        "a block that encloses the draw is exempt",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    if !self.enabled {
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: 8.0,
            text: "Wi-Fi".to_string(),
            color: self.palette.overlay0,
            font_size: 13.0,
        });
    }
}
""",
        {"exempt": 1},
    ),
    (
        "the function's own name is exempt, however far above it is",
        """fn render_disabled_button(tree: &mut RenderTree, pal: &Palette, label: &str) {
    fill_rounded(
        tree,
        x,
        y,
        button_width(label),
        BUTTON_HEIGHT,
        pal.surface0,
        6.0,
    );
    tree.text(x + 12.0, y + 8.0, label, pal.overlay0, 13.0);
}
""",
        {"exempt": 1},
    ),
    (
        "an early return on !enabled leaves the rest of the function LIVE",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    if !self.enabled {
        return;
    }
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
        "an empty-state message is live text, not a disabled control",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    if self.devices.is_empty() {
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: 8.0,
            text: "No devices found".to_string(),
            color: self.palette.overlay0,
            font_size: 13.0,
        });
    }
}
""",
        {"live": 1},
    ),
    (
        "a let-binding above an unrelated draw does not exempt it",
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
        {"live": 1},
    ),
    (
        "a conditional ink puts the role lines below its field",
        """fn render(&self, cmds: &mut Vec<RenderCommand>) {
    cmds.push(RenderCommand::Text {
        x: 8.0,
        y: 8.0,
        text: search_text,
        color: if self.query.is_empty() {
            p.overlay0
        } else {
            p.text
        },
        font_size: 13.0,
    });
}
""",
        {"live": 1},
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
        f"text draw(s), all {counts['exempt']} of them a switched-off state)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
