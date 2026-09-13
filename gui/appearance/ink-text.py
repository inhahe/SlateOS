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
# The trailing negative lookahead is load-bearing: `cat.accent(&self.palette)`
# is a *method* that happens to be named like a role, and rewriting it gives
# `cat.ink(cat.accent)(&self.palette)`, which is not anything.
ROLE = re.compile(r"\b([A-Za-z_][\w.]*)\.(" + "|".join(DUAL) + r")\b(?!\s*\()")
COLOR = re.compile(r"^(\s*)color:\s*(.*)$")
# `readable_on(x)` picks an ink for text drawn *on* `x`. Its ground is `x`,
# not one of the theme's surfaces, so it must not be re-inked.
READABLE_ON = re.compile(r"readable_on\([^()]*\)")


def palette_receivers(lines):
    """Which expressions in this file are actually a `Palette`.

    `apps/ebook` has its own `ThemeColors` with an `accent` field, and
    `tc.accent` matches the role pattern exactly. Rewriting it asks a type
    that has no `ink` for one -- which at least fails to compile, but the
    same shape on a type that *did* have a similarly named method would not,
    so the receiver is checked rather than assumed.

    Deliberately conservative: an expression not recognised here is left
    alone. A missed site is a site someone looks at; a wrongly rewritten one
    is a colour nobody notices is wrong.
    """
    text = NL.join(lines)
    named = set(re.findall(r"\b([a-z_][\w]*)\s*:\s*&?\s*Palette\b", text))
    named |= set(
        re.findall(r"\blet\s+(?:mut\s+)?([a-z_][\w]*)\s*=\s*Palette::", text)
    )
    # A field declared `Palette`, reached as `self.<field>` or `x.<field>`.
    fields = set(re.findall(r"\b([a-z_][\w]*)\s*:\s*Palette\b", text))
    return named, fields


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


def ink_expression(value, allowed=None):
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
    def one(mo):
        recv = mo.group(1)
        if allowed is not None:
            names, fields = allowed
            if recv not in names and recv.rsplit(".", 1)[-1] not in fields:
                return mo.group(0)
        return recv + ".ink(" + recv + "." + mo.group(2) + ")"

    inked = ROLE.sub(one, guarded)
    for idx, original in enumerate(masked):
        inked = inked.replace(SENTINEL + str(idx) + SENTINEL, original)
    return inked


def convert(path, apply):
    lines = path.read_text(encoding="utf-8").splitlines()
    allowed = palette_receivers(lines)
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
        inked = ink_expression(value, allowed)
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


def stray_inks(path):
    """Lines calling `ink` from something that is not a `RenderCommand::Text`.

    The complement of the count `--check` reports, and it catches the opposite
    mistake. `--check` finds a text site that forgot to ask; this finds a site
    that asked when it should not have -- a fill, a stroke, a line. Both
    render, so neither is visible without looking.

    It exists because the converter's `enclosing_kind` is a twenty-line
    lookback rather than a parser, and a lookback can be wrong. Running this
    over the first 106 conversions found none, which is the only reason to
    believe the other 309.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    out = []
    seen = 0
    for i, line in enumerate(lines):
        if ".ink(" not in line or line.lstrip().startswith("//"):
            continue
        seen += 1
        kind = enclosing_kind(lines, i)
        # `None` means no `RenderCommand::` within the lookback at all, which
        # is an ordinary expression rather than a draw site -- a `let` binding,
        # a helper's argument. Those are the caller's business.
        if kind is not None and kind != "Text":
            out.append((i + 1, kind, line.strip()))
    return seen, out


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
    if "--verify" in sys.argv:
        examined = 0
        stray = []
        for path in paths:
            seen, found = stray_inks(path)
            examined += seen
            stray.extend((path, ln, kind, text) for ln, kind, text in found)
        for path, ln, kind, text in stray:
            print(str(path) + ":" + str(ln) + "  ink() inside a " + kind + ": " + text[:70])
        if stray:
            print(str(len(stray)) + " site(s) ask for a text ink and do not draw text.")
            sys.exit(1)
        if examined == 0:
            # Not a pass. Five separate times in this conversion a check has
            # reported success over an empty population: a collector matching
            # only `FillRect` after its subject became an outline, the
            # `.all()` over the empty vector it returned, two geometry helpers
            # in `notif_pane`, and a harness regex that dropped one character
            # and compared 0 > 0. Every one of them was green.
            print("no ink() call sites found at all; refusing to call that a pass")
            sys.exit(2)
        print("ok: all " + str(examined) + " ink() call sites are inside a Text command")
        sys.exit(0)

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


# ---------------------------------------------------------------------------
# The blind spots
# ---------------------------------------------------------------------------
#
# `--check` counts text sites that *name a dual-use role*. A colour can reach a
# `RenderCommand::Text` without naming one, and four ways were found during the
# shell conversion -- every one by a failing test, none by this script:
#
#     a method in the color: field    color: app.state.color(p)
#     a helper's return value         let (glyph, c) = icon_info(p, icon)
#     an argument to a draw helper    self.render_icon_text(.., p.red, ..)
#     a local used by field shorthand let color = ..; Text { .., color, .. }
#
# Two of those have a syntactic signature this script can look for: a `color:`
# whose value is a call, and a `color,` shorthand. The other two are properties
# of a function's body rather than of the draw site, and finding them needs
# something that understands Rust rather than lines.
#
# So this is a *report*, not a gate. It exists because "`--check` says zero"
# reads as "the conversion is complete" when it means "complete among the sites
# I can classify", and that gap is exactly the shape of every other near-miss
# in this conversion: a check answering confidently about a narrower population
# than its name implies.

CALL_VALUE = re.compile(r"^\s*color:\s*.*\w\s*\(")
SHORTHAND = re.compile(r"^\s*color,\s*$")
# Text drawn *on* a colour rather than on a surface: its ground is that colour,
# so it is already right and is not a blind spot. `on_accent` is literally
# `readable_on(self.accent)`; `on_wallpaper` answers for a photograph the shell
# did not choose.
GROUNDED = re.compile(r"readable_on|on_accent|on_wallpaper|contrast_text")


def blind_spots(path):
    """Text sites whose colour this script cannot classify."""
    lines = path.read_text(encoding="utf-8").splitlines()
    out = []
    in_test = False
    for i, line in enumerate(lines):
        if lines[i].strip().startswith("#[cfg(test)]"):
            in_test = True
        if in_test:
            continue
        if enclosing_kind(lines, i) != "Text":
            continue
        # A few lines of the value, since a conditional colour runs over
        # several and the exclusion may be on any of them.
        value = " ".join(l.strip() for l in lines[i : i + 5])
        if GROUNDED.search(value) or ".ink(" in value:
            continue
        if SHORTHAND.match(line):
            out.append((i + 1, "shorthand", line.strip()))
        elif CALL_VALUE.match(line) and not ROLE.search(value):
            out.append((i + 1, "a call", line.strip()))
    return out
