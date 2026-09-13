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

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from rustslice import production_end  # noqa: E402

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


# ---------------------------------------------------------------------------
# The second shape: a colour that arrives from a method
# ---------------------------------------------------------------------------
#
# `color: tx.category.color(&self.palette)` names no role, so the sweep above
# is silent about it. That is the blind spot recorded as TD-C-FORTY-NINE, and
# 37 of the methods behind it cannot be fixed in their own body because the
# same method also fills a badge -- inking inside would darken the fill.
#
# The ink goes at the draw site instead, which is where design decision 837
# says it belongs: legibility is a property of the ink-and-ground pair, not of
# the palette field. `p.ink(tx.category.color(&p))` leaves every fill caller
# of that method exactly as it was.
#
# The palette to ask is not guessed: it is the argument the method was just
# handed. A call with no palette argument is left alone, because there is
# nothing to ask -- those crates are on the conversion list instead.
METHOD_CALL = re.compile(
    r"^(?P<recv>[A-Za-z_][\w.]*)\.(?P<name>[a-z_][\w]*)"
    r"\(&?(?P<pal>[A-Za-z_][\w.]*)\)$"
)


def ink_a_call(value, allowed, inked_already):
    """Wrap a colour-method call in `ink`, or return None to leave it.

    Conservative on every axis. One argument only, and that argument must be
    something this file actually knows to be a `Palette`; no rewriting of a
    method that already inks its own body, which would be a no-op but a
    confusing one; and nothing that draws on a ground of its own.
    """
    flat = " ".join(part.strip() for part in value.split(NL)).strip()
    if ".ink(" in flat or GROUNDED.search(flat):
        return None
    m = METHOD_CALL.match(flat)
    if not m:
        return None
    if m.group("name") in inked_already:
        return None
    pal = m.group("pal")
    names, fields = allowed
    if pal not in names and pal.rsplit(".", 1)[-1] not in fields:
        return None
    return pal + ".ink(" + flat + ")"


def crate_of(path):
    """`apps/weather/src/main.rs` -> `apps/weather`."""
    parts = path.as_posix().split("/")
    for i, part in enumerate(parts):
        if part in ("apps", "gui") and i + 1 < len(parts):
            return part + "/" + parts[i + 1]
    return path.parent.as_posix()


def already_inking(paths):
    """Per crate, the method names whose own body calls `ink`.

    Per crate, because `color` is defined in thirty of them. A tree-wide set
    of bare names put `color` in it -- `jsonviewer` and `netscan` ink theirs --
    and every `x.color(&p)` in the tree was then skipped as already handled.
    One site survived that, which is how it was noticed: a sweep that finds
    one site where it found many yesterday is reporting a bug in itself.
    """
    out = {}
    for path in paths:
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except OSError:
            continue
        end = production_end(lines)
        name = None
        seen = out.setdefault(crate_of(path), set())
        for i, line in enumerate(lines[:end]):
            m = re.match(r"^\s*(?:pub[\w(): ]*)?fn\s+(\w+)", line)
            if m:
                name = m.group(1)
            elif name and ".ink(" in line:
                seen.add(name)
    return out


def convert(path, apply, inked_already=frozenset()):
    lines = path.read_text(encoding="utf-8").splitlines()
    allowed = palette_receivers(lines)
    out = []
    i = 0
    n = 0
    # Not `end`: the loop below rebinds that to the *site's* last line, and a
    # bound named twice is a bound that silently shrinks -- after the first
    # site, `i >= end` was true for the whole rest of the file.
    prod_end = production_end(lines)
    while i < len(lines):
        m = COLOR.match(lines[i])
        if i >= prod_end or not m or enclosing_kind(lines, i) != "Text":
            out.append(lines[i])
            i += 1
            continue
        end, value = value_span(lines, i, m.group(2))
        if end is None:
            out.append(lines[i])
            i += 1
            continue
        if ROLE.search(value) is None:
            call = ink_a_call(value, allowed, inked_already)
            if call is None:
                out.extend(lines[i : end + 1])
                i = end + 1
                continue
            out.append(m.group(1) + "color: " + call + ",")
            n += 1
            i = end + 1
            continue
        if ".ink(" in value:
            out.extend(lines[i : end + 1])
            i = end + 1
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
    # Production code only. A test may legitimately ask for an ink while
    # asserting about something that is not a `Text` -- `launcher`'s badge
    # tests compare a wash against the inked hue it is derived from -- and
    # reporting those made the gate cry wolf on its own fixtures.
    for i, line in enumerate(lines[: production_end(lines)]):
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


# Roles that are floored in the palette itself, so a text site naming one is
# already legible and needs no `ink()`. The split is the design -- see
# `known-issues.md` TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS.
# The inks that are floored in the palette itself, so a text site naming one
# is already legible and needs no `ink()`. Deliberately NOT the surface roles:
# `surface1` used as a text colour is not a floored ink, it is a background
# being used as a foreground, and excluding those would hide a real question
# behind a rule written for a different one.
FLOORED = re.compile(r"\b[A-Za-z_][\w.]*\.(subtext0|subtext1|link|text)\b(?!\s*\()")


def value_of(lines, i, first_rest):
    """The whole `color:` value, however many lines it spans.

    A fixed window was the previous rule -- five lines, joined -- and it is the
    wrong shape twice over: a conditional ink running to six lines was judged
    on its first five, and a *neighbouring* field within five lines of a short
    value was read as part of it. The balanced span is the value and nothing
    else.
    """
    depth, parts, j, rest = 0, [], i, first_rest
    while j < len(lines):
        buf = ""
        for ch in rest:
            depth += (ch in "([{") - (ch in ")]}")
            # A semicolon ends a `let` as a comma ends a struct field, and
            # stopping only at the comma was a real bug: the value of
            # `let color = shape.stroke.effective_color();` ran on past the
            # statement until it met a comma several lines later, swept up a
            # palette role from unrelated code, and reported the site as
            # naming a dual-use role. One false positive, in the one file
            # where the colour is the user's drawing rather than the theme's.
            if ch in ",;" and depth == 0:
                parts.append(buf)
                return NL.join(parts)
            buf += ch
        parts.append(buf)
        j += 1
        if j < len(lines):
            rest = lines[j]
    return NL.join(parts)


FN = re.compile(r"^\s{0,8}(?:pub(?:\([\w:]+\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s")
# `let color = ..`, and also a destructuring that binds `color` alongside
# something else: `let (label, color) = match profile { .. }` is how a colour
# and the word it labels get chosen in one expression, and it is common enough
# that matching only the simple form left a real contrast failure unseen in
# `gui/desktop/src/power.rs`.
LET_COLOR = re.compile(r"^\s*let\s+(?:mut\s+)?(?:color\s*(?::[^=]*)?|\([^)]*\bcolor\b[^)]*\))\s*=\s*(.*)$")


# A function that takes its ink as an argument. This is the correct shape for
# a toolkit primitive -- `RenderTree::text(x, y, text, color, size)` must not
# ink, because it does not know what ground the caller is drawing on -- so a
# site inside one is not an unresolved colour. It is a colour resolved one
# frame up, at a call site this script does convert.
COLOR_PARAM = re.compile(r"\bcolor\s*:\s*&?\s*Color\b")


def binding_of(lines, i, fn_start):
    """The value of the nearest `let color = ...` above line `i`.

    The shorthand form -- `Text { .., color, .. }` -- names a local, so the
    ink is wherever that local was bound. Walking back to it is the whole of
    what the docstring above calls "a property of a function's body rather
    than of the draw site"; it does not need something that understands Rust,
    only the nearest binding of that one name.

    `None` when there is no such binding in the function, which is the honest
    answer: the local came from a parameter, a destructuring, or a loop, and
    this script genuinely cannot see it.
    """
    for k in range(i - 1, fn_start - 1, -1):
        m = LET_COLOR.match(lines[k])
        if m:
            return value_of(lines, k, m.group(1))
    return None


# A `RenderCommand::Text { .. }` that is a *pattern* rather than a value: a
# match arm or an `if let` destructuring one, as the compositor's
# `execute_command` and the wire encoder both do. Its `color,` is a binding
# being introduced, not a colour being chosen, and reporting it as an
# unclassifiable ink sends the reader to a file that draws nothing.
#
# The discriminator is what precedes the brace on its own line. A value is
# built inside something -- `push(`, `= `, `vec![` -- and a pattern stands at
# the start of its line as a match arm.
TEXT_VALUE = re.compile(r"[=(!\[,]\s*RenderCommand::Text\s*\{")


def is_text_pattern(lines, i):
    """Whether the `Text` block enclosing line `i` is a pattern, not a value."""
    for k in range(i, max(-1, i - 24), -1):
        if "RenderCommand::Text" in lines[k]:
            return not TEXT_VALUE.search(lines[k])
    return False


def blind_spots(path):
    """Text sites whose colour this script cannot classify.

    Returns `(line, kind, text)`, where `kind` says *why* it could not be
    classified -- which is the difference between a site that needs looking at
    and one the classifier simply cannot see through.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    out = []
    end = production_end(lines)
    for i, line in enumerate(lines[:end]):
        if enclosing_kind(lines, i) != "Text":
            continue
        m = COLOR.match(line)
        value = value_of(lines, i, m.group(2)) if m else line
        if GROUNDED.search(value) or ".ink(" in value:
            continue
        if SHORTHAND.match(line):
            if is_text_pattern(lines, i):
                continue
            start = 0
            for k in range(i, -1, -1):
                if FN.match(lines[k]):
                    start = k
                    break
            bound = binding_of(lines, i, start)
            if bound is None:
                sig = NL.join(lines[start:start + 12])
                kind = ("shorthand, inked by the caller"
                        if COLOR_PARAM.search(sig)
                        else "shorthand, no local binding")
                out.append((i + 1, kind, line.strip()))
            elif GROUNDED.search(bound) or ".ink(" in bound:
                continue
            elif FLOORED.search(bound) and not ROLE.search(bound):
                continue
            elif ROLE.search(bound):
                out.append((i + 1, "shorthand naming a dual-use role", line.strip()))
            else:
                out.append((i + 1, "shorthand, colour from elsewhere", line.strip()))
        elif CALL_VALUE.match(line) and not ROLE.search(value):
            # A conditional whose every branch names a floored role is not a
            # blind spot: it is legible by construction, and the only reason
            # it looked like one is that the value opens with `if` and so
            # contains a `(`. Forty-seven of `gui/desktop`'s were this.
            if FLOORED.search(value) and not ROLE.search(value):
                continue
            out.append((i + 1, "a call", line.strip()))
    return out


# Fixtures, each a whole file, because every verdict here depends on where the
# test module starts and where a value ends -- the two things a fixture of
# snippets would quietly lose. `(source, converted, blind)`: how many sites the
# conversion rewrites, and how many the blind report cannot classify.
SELF_TESTS = [
    (
        "a dual-use role at a text site is converted",
        """fn render(&self, cmds: &mut Vec<RenderCommand>, p: &Palette) {
    cmds.push(RenderCommand::Text {
        x: 1.0,
        text: "hi".to_string(),
        color: p.green,
        font_size: 12.0,
    });
}
""",
        1,
        0,
    ),
    (
        "a role already inked is left alone",
        """fn render(&self, cmds: &mut Vec<RenderCommand>, p: &Palette) {
    cmds.push(RenderCommand::Text {
        x: 1.0,
        text: "hi".to_string(),
        color: p.ink(p.green),
        font_size: 12.0,
    });
}
""",
        0,
        0,
    ),
    (
        "a floored ink needs no ink() and is not a blind spot",
        """fn render(&self, cmds: &mut Vec<RenderCommand>, p: &Palette) {
    cmds.push(RenderCommand::Text {
        x: 1.0,
        text: "hi".to_string(),
        color: p.subtext0,
        font_size: 12.0,
    });
}
""",
        0,
        0,
    ),
    (
        "a fill is not a text site",
        """fn render(&self, cmds: &mut Vec<RenderCommand>, p: &Palette) {
    cmds.push(RenderCommand::Rect {
        x: 1.0,
        color: p.green,
    });
}
""",
        0,
        0,
    ),
    (
        "a Text command matched as a pattern is not a draw site",
        """fn encode(cmd: &RenderCommand) {
    match cmd {
        RenderCommand::Text {
            x,
            text,
            color,
            font_size,
            ..
        } => write(x, text, color, font_size),
        _ => {}
    }
}
""",
        0,
        0,
    ),
    (
        "a shorthand whose let ends at a semicolon does not read past it",
        """fn render(&self, cmds: &mut Vec<RenderCommand>, p: &Palette) {
    let color = shape.stroke.effective_color();
    cmds.push(RenderCommand::Text {
        x: 1.0,
        text: "hi".to_string(),
        color,
        font_size: 12.0,
    });
    let other = vec![p.green, p.red];
}
""",
        0,
        1,
    ),
    (
        "a shorthand bound to a dual-use role is a blind-spot finding",
        """fn render(&self, cmds: &mut Vec<RenderCommand>, p: &Palette) {
    let color = p.green;
    cmds.push(RenderCommand::Text {
        x: 1.0,
        text: "hi".to_string(),
        color,
        font_size: 12.0,
    });
}
""",
        0,
        1,
    ),
    (
        "a colour the caller supplies is not unresolved",
        """fn draw(&self, cmds: &mut Vec<RenderCommand>, color: Color) {
    cmds.push(RenderCommand::Text {
        x: 1.0,
        text: "hi".to_string(),
        color,
        font_size: 12.0,
    });
}
""",
        0,
        1,
    ),
    (
        "a site inside the test module is not production code",
        """fn render(&self) {}

#[cfg(test)]
mod tests {
    fn a_label_is_green(p: &Palette) {
        cmds.push(RenderCommand::Text {
            x: 1.0,
            text: "hi".to_string(),
            color: p.green,
            font_size: 12.0,
        });
    }
}
""",
        0,
        0,
    ),
]


def self_test():
    """Grade the classifier against files whose verdicts are known.

    Two of the nine cases are bugs this script actually had on 2026-09-13, and
    they are the reason it has a self-test at all: a `let`'s value was read to
    the next depth-zero *comma* rather than its semicolon, so it ran past the
    statement and accused an innocent file; and a `RenderCommand::Text` matched
    as a *pattern* was counted as a draw site, which put six files that draw
    nothing into the report. Both produced confident, specific, wrong answers.
    """
    import tempfile

    failed = 0
    for name, source, want_converted, want_blind in SELF_TESTS:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fixture.rs"
            path.write_text(source, encoding="utf-8", newline=NL)
            got_converted = convert(path, False, frozenset())
            got_blind = len(blind_spots(path))
        ok = (got_converted, got_blind) == (want_converted, want_blind)
        print(("ok   " if ok else "FAIL ") + name)
        if not ok:
            print("       expected " + str((want_converted, want_blind))
                  + ", got " + str((got_converted, got_blind)))
            failed += 1
    print(NL + str(len(SELF_TESTS)) + " self-test case(s), " + str(failed) + " failed")
    return 1 if failed else 0


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--self-test" in sys.argv or "--selftest" in sys.argv:
        sys.exit(self_test())
    check = "--check" in sys.argv
    paths = [pathlib.Path(a) for a in args] if args else default_paths()
    inked_already = already_inking(paths)
    total = 0
    for path in paths:
        c = convert(path, "--apply" in sys.argv, inked_already.get(crate_of(path), frozenset()))
        if c:
            print(str(path) + ": " + str(c))
        total += c
    if "--blind" in sys.argv:
        # Was accepted and did nothing for a day: every `--` argument is
        # filtered out of `args` above, so `--blind` parsed fine, ran the
        # ordinary conversion and printed "0 text sites routed through ink()".
        # A flag that silently means nothing is the same defect as a check
        # over an empty population, in a smaller package.
        found = 0
        for path in paths:
            for ln, kind, text in blind_spots(path):
                print(str(path) + ":" + str(ln) + "  " + kind + ": " + text[:70])
                found += 1
        print(str(found) + " text site(s) this script cannot classify.")
        sys.exit(0)

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
