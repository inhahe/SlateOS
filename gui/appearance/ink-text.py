"""Route text drawn in a dual-use colour through `Palette::ink`.

An accent is also a switch that is on; `red` is also an error bar. So the
palette keeps those roles exactly as the theme and the user chose them, and a
site that draws *text* in one asks for the legible version. See design
decision 837 and `Palette::ink`.

Only `RenderCommand::Text` literals, only outside `#[cfg(test)]`, and only
where the expression actually names a dual-use role -- `p.text` and
`p.subtext0` are already floored in the palette and are left alone.

Usage:  python ink-text.py <file.rs> [--apply]
"""
import re
import sys
import pathlib

NL = chr(10)
SENTINEL = chr(0)

DUAL = ("accent", "red", "green", "yellow", "peach", "blue", "lavender", "mauve",
        "sapphire", "teal", "sky")
ROLE = re.compile(r"\b([A-Za-z_][\w.]*)\.(" + "|".join(DUAL) + r")\b")
COLOR = re.compile(r"^(\s*)color:\s*(.*)$")
# `readable_on(x)` picks an ink for text drawn *on* `x`. Its ground is `x`,
# not one of the theme's surfaces, so it must not be re-inked.
READABLE_ON = re.compile(r"readable_on\([^()]*\)")


def enclosing_kind(lines, i):
    """Which RenderCommand this field belongs to.

    A fixed twenty-line lookback rather than real brace tracking, because no
    literal in the tree spans anything like that before its `color:` field.
    """
    for k in range(i, max(0, i - 20), -1):
        m = re.search(r"RenderCommand::(\w+)", lines[k])
        if m:
            return m.group(1)
    return None


def value_span(lines, i, first_rest):
    """The last line of the `color:` value, and the value text.

    The value ends at the comma that closes it at depth zero, which is not
    necessarily on the same line: a conditional colour runs over four or five.
    """
    depth = 0
    parts = []
    j = i
    rest = first_rest
    while j < len(lines):
        buf = ""
        for ch in rest:
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            if ch == "," and depth == 0:
                parts.append(buf)
                return j, NL.join(parts)
            buf += ch
        parts.append(buf)
        j += 1
        if j < len(lines):
            rest = lines[j]
    return None, None


def ink_expression(value):
    """Wrap each dual-use role *occurrence*, not the expression around them.

    A conditional colour usually has one branch that is a dual-use role and
    one that is not:

        color: if on { p.accent } else { p.overlay0 }

    Wrapping the whole thing is wrong in two different ways. `overlay0` is the
    muted ink -- a disabled control's label -- which WCAG 1.4.3 exempts, and
    raising it to 4.5 makes a disabled control look enabled: the worse
    failure. And `p.text` needs no adjustment at all, so wrapping it is noise
    that hides the sites that do.
    """
    flat = " ".join(part.strip() for part in value.split(NL)).strip()
    masked = []

    def mask(mo):
        masked.append(mo.group(0))
        return SENTINEL + str(len(masked) - 1) + SENTINEL

    guarded = READABLE_ON.sub(mask, flat)
    inked = ROLE.sub(lambda mo: mo.group(1) + ".ink(" + mo.group(1) + "." + mo.group(2) + ")",
                     guarded)
    for idx, original in enumerate(masked):
        inked = inked.replace(SENTINEL + str(idx) + SENTINEL, original)
    return inked


def convert(path, apply):
    lines = path.read_text(encoding="utf-8").splitlines()
    out = []
    i = 0
    n = 0
    in_test = False
    while i < len(lines):
        if lines[i].strip().startswith("#[cfg(test)]"):
            in_test = True
        m = COLOR.match(lines[i])
        if in_test or not m or enclosing_kind(lines, i) != "Text":
            out.append(lines[i])
            i += 1
            continue
        end, value = value_span(lines, i, m.group(2))
        if end is None or ".ink(" in value or ROLE.search(value) is None:
            out.extend(lines[i : (end if end is not None else i) + 1])
            i = (end if end is not None else i) + 1
            continue
        # Every role in it was inside a `readable_on`, so there is nothing here.
        inked = ink_expression(value)
        if ".ink(" not in inked:
            out.extend(lines[i : end + 1])
            i = end + 1
            continue
        out.append(m.group(1) + "color: " + inked + ",")
        n += 1
        i = end + 1
    if apply:
        path.write_text(NL.join(out) + NL, encoding="utf-8", newline=NL)
    return n


def default_paths():
    """Every production Rust file in the two trees this lane owns.

    So that `--check` cannot pass by being pointed at nothing -- which is the
    same failure as a test collecting an empty vector, and has happened four
    separate ways in this conversion. The directories are asserted to exist.
    """
    root = pathlib.Path(__file__).resolve().parents[2]
    found = []
    for sub in ("gui", "apps"):
        here = root / sub
        if not here.is_dir():
            raise SystemExit("no " + sub + "/ under " + str(root) + "; this script has moved")
        found.extend(sorted(here.glob("**/*.rs")))
    if not found:
        raise SystemExit("no .rs files found; refusing to report success over nothing")
    return found


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    check = "--check" in sys.argv
    paths = [pathlib.Path(a) for a in args] if args else default_paths()
    total = 0
    for path in paths:
        c = convert(path, "--apply" in sys.argv)
        if c:
            print(str(path) + ": " + str(c))
        total += c
    if check:
        # A gate, not a report. A site that draws text in a dual-use role
        # without asking `ink` renders perfectly and cannot be read on a card,
        # which is exactly the kind of defect nobody notices until someone
        # switches themes. See design decision 837.
        if total:
            print(str(total) + " text site(s) still name a dual-use role directly.")
            print("Run:  python gui/appearance/ink-text.py --apply  (then check the diff)")
            sys.exit(1)
        print("ok: every text site in " + str(len(paths)) + " files goes through ink()")
        sys.exit(0)
    print(str(total) + " text sites routed through ink()")
