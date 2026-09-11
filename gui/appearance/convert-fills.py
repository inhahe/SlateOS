"""Rewrite surface-role fills to go through the theme's one decision point.

Conservative by construction: anything it cannot parse cleanly it leaves alone
and reports. A site left behind is a site someone looks at; a site rewritten
wrongly is a bug nobody sees until the theme is switched.

Usage:  python convert_fills.py <file.rs> [--apply]
Without --apply it prints what it would do and changes nothing.
"""
import re
import sys
import pathlib
import collections

ROLES = ("surface0", "surface1", "surface2", "mantle", "crust")

CHROME = re.compile(
    r"track|trough|groove|scrollbar|scroll_bar|thumb|slider|progress|gauge|"
    r"meter|switch|toggle|knob|handle|tick|grip",
    re.I,
)
PANEL = re.compile(r"menu|popup|dropdown|dialog|modal|tooltip|notification|toast|flyout", re.I)
SELECTED = re.compile(r"select|active|current|highlight|hover|focus|chosen|pressed", re.I)
SIDEBAR = re.compile(r"sidebar|side_bar|nav_|navigation|rail|gutter", re.I)

# `<receiver>.push(RenderCommand::FillRect {`
OPEN = re.compile(r"^(\s*)([A-Za-z_][A-Za-z0-9_.]*)\.push\(RenderCommand::FillRect \{\s*$")
# `color: <palette expr>.<role>,`
COLOR = re.compile(r"^\s*color:\s*([A-Za-z_][A-Za-z0-9_.]*?)\.(" + "|".join(ROLES) + r"),\s*$")
RADII_ALL = re.compile(r"^\s*corner_radii:\s*CornerRadii::all\(([^)]+)\),\s*$")
RADII_ZERO = re.compile(r"^\s*corner_radii:\s*CornerRadii::ZERO,\s*$")
FIELD = re.compile(r"^\s*([a-z_]+)(?::\s*(.+?))?,\s*$")
CLOSE = re.compile(r"^\s*\}\);\s*$")


# A fill is "selected" only when it sits inside a conditional that tests for
# selection -- `if is_active {`. Merely having a selection word nearby is not
# enough and produced a real bug on the first file tried: clipmanager's search
# bar has `let focused = ...` above it, feeding an unrelated border colour, and
# was classified Selected. It would have rendered permanently accent-outlined,
# looking focused at all times.
GUARD = re.compile(
    # `hover` stays, because a hovered menu row IS the selection -- context_ext
    # has two of those and both are right. `current` does not: it matched
    # `self.current_utc` and `settings.current_language()`, which name what a
    # card is *showing*, not a selection, and mislabelled three display cards.
    r"^\s*(\}\s*)?(else\s+)?if\s+.*(select|active|highlight|focus|chosen|pressed|hover"
    # `current` is back to a broad match, and the reason is worth keeping: it
    # was narrowed to compensate for the `if let` guard three lines above,
    # which contained a literal backspace byte instead of `\b` and therefore
    # never fired (lane B found it). With that repaired, the two cases the
    # narrowing existed for -- `if let Some(lang) = settings.current_language()`
    # and `if let .. = local_time(self.current_utc)` -- are excluded as the
    # lookups they are, and a genuine `if row.path == state.current_dir` is
    # classified as the selection it is.
    r"|[\w.]*current)",
    re.I,
)


# Sites this tool refuses to touch, because converting them is a design
# decision rather than a mechanical one. Left as fills, so they look exactly
# as they do today and someone can decide later.
#
#  * A data bar -- a statistics or percentage bar -- conveys magnitude by the
#    area it fills. Outlined, it says nothing. jsonviewer per-type statistics
#    bars were converted and two of its tests caught it immediately, which is
#    the only reason this category exists.
#  * A structural strip -- toolbar, tab bar, status bar -- spans the window.
#    Whether a border theme draws those as an outlined box or as a plain
#    region with a separator underneath is a question about what the theme
#    looks like, and nobody has answered it.
SKIP = re.compile(
    r"bar_w|bar_x|bar_h|bar_max|percent|pct_|histogram|chart|graph|sparkline|"
    r"toolbar|tool_bar|tab_bar|status_bar|statusbar|mode_bar|menu_bar|menubar",
    re.I,
)


# How raised each rung is, for reading a conditional colour. A site that says
# `if active { surface1 } else { surface0 }` is choosing between selected and
# not, and says so more clearly as two Surface kinds than as two shades.
RAISED = {"crust": 0, "mantle": 1, "base": 2, "surface0": 3, "surface1": 4, "surface2": 5}
COND_COLOR = re.compile(r"^\s*color:\s*if\s+(.+?)\s*\{\s*$")
BRANCH = re.compile(r"^\s*([A-Za-z_][A-Za-z0-9_.]*)\.(" + "|".join(ROLES) + r")\s*$")


def classify(context: str, preceding: list) -> str:
    if CHROME.search(context):
        return "ControlTrack"
    # Look only at the few lines directly above: an enclosing `if` is adjacent,
    # while an unrelated `let` binding five lines up is not.
    for line in [l for l in preceding[-4:] if l.strip()][-3:]:
        # `if let Some(node) = nodes.get(doc.selected_node)` is a *lookup* of
        # the selected item, usually to draw a details panel about it -- not a
        # test that the box being drawn is itself selected. jsonviewer had
        # exactly that, and its JSON-path panel was classified Selected.
        if re.match(r"^\s*(\}\s*)?(else\s+)?if\s+let\b", line):
            continue
        if GUARD.match(line):
            return "Selected"
    if SIDEBAR.search(context):
        return "Sidebar"
    if PANEL.search(context):
        return "Panel"
    return "Card"


def convert(path: pathlib.Path, apply: bool):
    lines = path.read_text(encoding="utf-8").splitlines()
    out = []
    i = 0
    done = collections.Counter()
    skipped = []

    while i < len(lines):
        m = OPEN.match(lines[i])
        if not m:
            out.append(lines[i])
            i += 1
            continue

        indent, receiver = m.group(1), m.group(2)
        # Gather the literal's body up to its closing `});`.
        body, j, depth = [], i + 1, 1
        while j < len(lines) and depth > 0:
            if CLOSE.match(lines[j]) and depth == 1:
                break
            depth += lines[j].count("{") - lines[j].count("}")
            body.append(lines[j])
            j += 1
        if j >= len(lines) or not CLOSE.match(lines[j]):
            out.append(lines[i])
            i += 1
            continue

        fields, pal, role, radius, ok = {}, None, None, None, True
        cond = None          # (condition, kind_if_true, kind_if_false)
        skip_to = -1
        for bi, b in enumerate(body):
            if bi <= skip_to:
                continue
            cc = COND_COLOR.match(b)
            if cc and bi + 4 < len(body):
                m_hi = BRANCH.match(body[bi + 1])
                m_lo = BRANCH.match(body[bi + 3]) if "} else {" in body[bi + 2] else None
                if m_hi and m_lo and body[bi + 4].strip() == "},":
                    a, bb = m_hi.group(2), m_lo.group(2)
                    if RAISED[a] != RAISED[bb]:
                        hi_is_true = RAISED[a] > RAISED[bb]
                        cond = (
                            cc.group(1),
                            "Selected" if hi_is_true else "Card",
                            "Card" if hi_is_true else "Selected",
                        )
                        pal, role = m_hi.group(1), a
                        skip_to = bi + 4
                        continue
            cm = COLOR.match(b)
            if cm:
                pal, role = cm.group(1), cm.group(2)
                continue
            rm = RADII_ALL.match(b)
            if rm:
                radius = rm.group(1).strip()
                continue
            if RADII_ZERO.match(b):
                radius = "0.0"
                continue
            fm = FIELD.match(b)
            if fm:
                fields[fm.group(1)] = (fm.group(2) or fm.group(1)).strip()
            else:
                ok = False

        if not (ok and pal and role and radius is not None):
            if pal and role:
                why = "unparsed corner_radii" if radius is None else "unparsed field"
                skipped.append(f"{path.name}:{i+1}  {why}")
            out.extend(lines[i : j + 1])
            i = j + 1
            continue
        if not all(k in fields for k in ("x", "y", "width", "height")):
            skipped.append(f"{path.name}:{i+1}  missing a geometry field")
            out.extend(lines[i : j + 1])
            i = j + 1
            continue

        context = "\n".join(lines[max(0, i - 5) : i] + body)
        # A hairline is not a box. A 1px-tall fill is a separator rule --
        # the line under a calendar grid, the divider between two panes --
        # and outlining it produces a degenerate rectangle of zero height.
        # systray's calendar rule was converted and its "does the month fit"
        # test lost the boundary it measures against.
        if fields.get("height") in ("1.0", "1.0_f32") or fields.get("width") in ("1.0", "1.0_f32"):
            skipped.append(f"{path.name}:{i+1}  a hairline rule, not a box")
            out.extend(lines[i : j + 1])
            i = j + 1
            continue
        if SKIP.search(context):
            skipped.append(f"{path.name}:{i+1}  needs a design decision, left as a fill")
            out.extend(lines[i : j + 1])
            i = j + 1
            continue
        kind = classify(context, lines[max(0, i - 6) : i])
        done[(kind, role)] += 1
        what = (
            f"if {cond[0]} {{ Surface::{cond[1]} }} else {{ Surface::{cond[2]} }}"
            if cond
            else f"Surface::{kind}"
        )
        call = (
            f"{indent}{pal}.push_surface("
            f"{receiver}, {fields['x']}, {fields['y']}, "
            f"{fields['width']}, {fields['height']}, {radius}, {what});"
        )
        out.append(call)
        i = j + 1

    if apply:
        path.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")

    total = sum(done.values())
    print(f"{path}: {total} converted, {len(skipped)} left for review")
    for (kind, role), n in sorted(done.items(), key=lambda kv: -kv[1]):
        print(f"    {kind:<13} was {role:<9} x{n}")
    for s in skipped:
        print(f"    SKIPPED {s}")
    return total


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    convert(pathlib.Path(args[0]), "--apply" in sys.argv)
