"""Which colour-returning functions may be inked in their own body.

The gap this fills is recorded as TD-C-FORTY-NINE-COLOUR-METHODS-ARE-
INVISIBLE-TO-THE-INK-SWEEP. `ink-text.py` finds a text site by the role named
at the draw site; a site whose colour arrives from a method names no role, so
the sweep is silent about it, and the silence reads as "done".

Design decision 837 says legibility belongs to the ink-and-ground pair, and a
method may be inked *in its body* only when every path through it is text. The
precondition found afterwards, the hard way, is that this is a statement about
the method's callers as much as its body: a method also used for a fill -- a
heat-map cell, a legend swatch -- must not be inked inside, because that would
darken the fill for no reason.

Answering that means following the colour from where it is produced to where
it is drawn, and the three hops it takes are exactly the three blind spots
recorded in that entry:

    color: f(p)                     the call is the field           (direct)
    let c = f(p); ... color: c,     a local carries it              (binding)
    draw_row(.., f(p), ..)          an argument carries it          (parameter)

The third needs the callee's body to say what its parameter colours, so the
parameter kinds are solved to a fixpoint before the call sites are classified.

Reports, never rewrites: the rewrite is per-arm surgery inside a body and is
not mechanical. What was missing was knowing which bodies.

Usage:  python colour-methods.py [--all]
"""
import collections
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from rustslice import production_end  # noqa: E402

NL = chr(10)
BS = chr(92)
WORD_END = BS + "b"
ROOT = pathlib.Path(__file__).resolve().parents[2]
DUAL = ("accent", "red", "green", "yellow", "peach", "blue", "lavender",
        "mauve", "sapphire", "teal", "sky")
EXEMPT = ("overlay0", "subtext0", "subtext1", "text", "link")
DUAL_USE = re.compile(r"[.](" + "|".join(DUAL) + r")" + WORD_END)
EXEMPT_USE = re.compile(r"[.](" + "|".join(EXEMPT) + r")" + WORD_END)
DEF = re.compile(r"^\s*(?:pub(?:\([\w:]+\))?\s+)?(?:const\s+)?fn\s+(\w+)\s*[(<]")
FN_LINE = re.compile(r"^\s*(?:pub(?:\([\w:]+\))?\s+)?(?:const\s+)?fn" + WORD_END)
RETURNS_COLOR = re.compile(r"->[^{;]*[^\w]Color[^\w]")
GROUNDED = re.compile(r"readable_on|on_accent|on_wallpaper|contrast_text")
def block_end(lines, i, hi):
    """The line where the block containing the binding on line `i` closes.

    A binding is scoped to its block, not to its function. Scanning to the end
    of the function let a *later* `for (si, (text, color))` loop, a hundred
    lines below and in a different scope, count as a use of a local called
    `color` -- and that loop's text is coloured by something else entirely.
    """
    depth = 0
    for j in range(i, hi):
        depth += lines[j].count("{") - lines[j].count("}")
        if depth < 0:
            return j
    return hi


def mentions(line, name):
    """Does `line` use the value `name`, as opposed to naming a field?

    `color: self.palette.text,` contains the word `color` and has nothing to
    do with a local called `color`. Reading it as one made speedtest's gauge
    -- whose local only ever colours a `Line` -- report an uninked text site
    three commands further down that belongs to a different colour entirely.
    The field shorthand `color,` is a real use and survives, because the
    character after it is a comma rather than a colon.
    """
    return bool(re.search(r"[^\w.]" + re.escape(name) + r"[^\w:]", " " + line + " "))


LET = re.compile(r"^\s*let\s+(?:mut\s+)?(?:\(\s*[\w, ]*?(\w+)\s*\)|(\w+))\s*(?::[^=]+)?=")
SHORTHAND = re.compile(r"^\s*color,\s*$")
CALL = re.compile(r"(\w+)\s*\(")


def crate_of(rel):
    """`apps/weather/src/main.rs` -> `apps/weather`.

    Every question here is asked within one crate. Without this the scan
    matched by bare name across the whole tree, and `color` -- which eleven
    crates define -- collected 270 call sites, attributing `gui/desktop`'s
    Bluetooth list to `apps/weather`. A survey that confident and that wrong
    is worse than no survey.
    """
    parts = rel.split("/")
    return "/".join(parts[:2])


def rust_files():
    for pat in ("gui/**/*.rs", "apps/**/*.rs"):
        yield from sorted(ROOT.glob(pat))


def item_span(lines, i, end):
    """The half-open line range of the item that starts at `i`."""
    depth, j, started = 0, i, False
    while j < end:
        depth += lines[j].count("{") - lines[j].count("}")
        if "{" in lines[j]:
            started = True
        if started and depth <= 0:
            return i, j + 1
        j += 1
    return i, end


def enclosing_fn(lines, i):
    while i > 0 and not FN_LINE.match(lines[i]):
        i -= 1
    return i


def colour_field_lines(lines, end):
    """Line index -> which `RenderCommand` that line helps colour.

    A call that supplies a colour is often not on the `color:` line: a
    conditional runs over five lines and the call is on the third. Matching
    only the `color:` line is how the first version of this reported 27
    functions as never drawn at all -- every one of them was drawn, a line or
    two further down than it looked.
    """
    out, kind, i = {}, None, 0
    while i < end:
        m = re.search(r"RenderCommand::(\w+)", lines[i])
        if m:
            kind = m.group(1)
        # `color,` -- field shorthand -- is a colour field too, and missing it
        # is not a silent omission: the shorthand line then falls through to a
        # default of "Text", which is the *wrong* answer in the one case that
        # matters. `touchpad_status` feeds a 12px status dot and a label, both
        # by shorthand; read as two text sites it looked safe to ink, and
        # inking it would have darkened the dot.
        if SHORTHAND.match(lines[i]) and kind:
            out[i] = kind
            i += 1
            continue
        cm = re.match(r"^\s*color:\s*(.*)$", lines[i])
        if cm and kind:
            j, rest, depth = i, cm.group(1), 0
            while j < end:
                depth += sum(rest.count(c) for c in "({[") - sum(rest.count(c) for c in ")}]")
                out[j] = kind
                if depth <= 0 and rest.rstrip().endswith(","):
                    break
                j += 1
                if j < end:
                    rest = lines[j]
            i = j + 1
            continue
        i += 1
    return out


def params_of(lines, i):
    """The parameter names of the function defined at line `i`, in order."""
    head, j = "", i
    while j < len(lines) and ")" not in head:
        head += lines[j]
        j += 1
    inner = head[head.find("(") + 1 : head.rfind(")")] if "(" in head else ""
    names, depth, cur = [], 0, ""
    for ch in inner:
        if ch in "(<[":
            depth += 1
        elif ch in ")>]":
            depth -= 1
        if ch == "," and depth == 0:
            names.append(cur)
            cur = ""
        else:
            cur += ch
    names.append(cur)
    out = []
    for raw in names:
        raw = raw.strip()
        if not raw or raw.startswith("&") or raw in ("self", "mut self"):
            out.append(None)
            continue
        out.append(raw.split(":")[0].strip().removeprefix("mut ").strip())
    return out


def method_offset(text, callee):
    """1 if `callee` is called as a method here, 0 otherwise.

    `params_of` counts `self` as a parameter so that the slots line up with
    the declaration; a method *call* does not pass it. Without this shift
    every method's parameters were read one to the left -- `render_stat_card`
    reported its title parameter's use for its colour parameter -- which is
    the quietest possible way to be wrong, since both are plausible.
    """
    return 1 if re.search(r"[.]" + re.escape(callee) + r"\s*\(", text) else 0


def arg_index(line, callee, needle, needle_may_be_a_method=False):
    """Which argument of `callee(...)` on `line` contains `needle`.

    The callee pattern allows a leading dot: the enclosing call is very often
    a method -- `self.render_stat_card(..)` -- and refusing one found none of
    those, leaving eight of `rate_color`'s ten callers unresolved. The
    *needle* refuses one by default, because there a leading dot means a field
    read (`self.color`) rather than the local being tracked.
    """
    m = re.search(r"[^\w]" + re.escape(callee) + r"\s*\(", " " + line)
    if not m:
        return None
    edge = r"[^\w]" if needle_may_be_a_method else r"[^\w.]"
    depth, idx, cur = 0, 0, ""
    for ch in (" " + line)[m.end():]:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        if ch == "," and depth == 0:
            if re.search(edge + re.escape(needle) + r"[^\w]", " " + cur + " "):
                return idx
            idx += 1
            cur = ""
        else:
            cur += ch
    if re.search(edge + re.escape(needle) + r"[^\w]", " " + cur + " "):
        return idx
    return None


def statement_at(lines, i):
    """Line `i` joined with the lines it is a continuation of.

    A call is routinely split over several lines by rustfmt::

        self.render_stat_card(
            cmds, x, y, w, h, "7-Day Average", &text,
            rate_color(avg_7, &self.palette),
        );

    Reading only the line the inner call sits on finds `rate_color` and no
    enclosing call at all, so the site is unresolved -- and an unresolved
    site used to fall through to a verdict of INK. `rate_color` feeds a
    card's fill through that parameter; inking it would have darkened the
    card.
    """
    bal = 0
    for j in range(i, max(-1, i - 16), -1):
        bal += lines[j].count("(") - lines[j].count(")")
        # An unmatched `(` in lines[j..i] is the enclosing call's own paren.
        # Stopping the moment the *first* line balances -- which it usually
        # does, since `rate_color(x, &p),` opens and closes -- finds no
        # enclosing call at all, and that was the original bug.
        if bal > 0:
            return " ".join(line.strip() for line in lines[j : i + 1])
    return lines[i]


def split_args(line, callee):
    """The argument expressions of `callee(...)` on `line`, in order."""
    m = re.search(r"[^\w]" + re.escape(callee) + r"\s*\(", " " + line)
    if not m:
        return []
    out, depth, cur = [], 0, ""
    for ch in (" " + line)[m.end():]:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    out.append(cur)
    return out


class World:
    """Every function in gui/ and apps/, and what its parameters colour."""

    def __init__(self):
        self.fns = {}
        self.lines = {}
        self.field = {}
        for path in rust_files():
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
            end = production_end(lines)
            rel = path.relative_to(ROOT).as_posix()
            self.lines[rel] = lines[:end]
            self.field[rel] = colour_field_lines(lines, end)
            for i, line in enumerate(lines[:end]):
                m = DEF.match(line)
                if m:
                    lo, hi = item_span(lines, i, end)
                    # Keyed by crate as well as name. `draw_button` exists
                    # in a dozen crates; a global key unioned all their
                    # parameter uses, so fileassoc's `bg` argument inherited
                    # some other crate's text parameter and the site was
                    # reported as text. It fills a button.
                    self.fns.setdefault((crate_of(rel), m.group(1)), []).append(
                        (rel, lo, hi, params_of(lines, i))
                    )
        self.param_kind = {}
        self.param_text_inked = {}
        self._solve_params()

    def known(self, rel, callee):
        """Is `callee` a function of this file's own crate?"""
        return (crate_of(rel), callee) in self.fns

    def _uses(self, rel, lo, hi, name):
        """Kinds and onward dependencies for `name` inside one function body."""
        kinds, deps = set(), set()
        lines, field = self.lines[rel], self.field[rel]
        for j in range(lo, min(hi, len(lines))):
            line = lines[j]
            if not mentions(line, name):
                continue
            if j in field:
                kinds.add(field[j])
                continue
            if SHORTHAND.match(line) and name == "color":
                kinds.add(field.get(j) or "unresolved")
                continue
            stmt = statement_at(lines, j)
            for callee in set(CALL.findall(stmt)):
                if callee == name or not self.known(rel, callee):
                    continue
                k = arg_index(stmt, callee, name)
                if k is not None:
                    deps.add((crate_of(rel), callee, k + method_offset(stmt, callee)))
        return kinds, deps

    def _param_inked(self, rel, lo, hi, name):
        """Is every text use of this parameter inked or grounded?"""
        lines, field = self.lines[rel], self.field[rel]
        saw = False
        for j in range(lo, min(hi, len(lines))):
            if field.get(j) != "Text" or not mentions(lines[j], name):
                continue
            saw = True
            stmt = statement_at(lines, j)
            if ".ink(" not in stmt and not GROUNDED.search(stmt):
                return False
        return saw

    def _solve_params(self):
        """`param_kind[(fn, index)]` -> the commands that parameter colours.

        A fixpoint rather than one pass: a colour is routinely handed down two
        levels -- `render_row` takes it and gives it to `render_badge`.
        """
        direct, deps = {}, {}
        for (crate, name), defs in self.fns.items():
            for rel, lo, hi, params in defs:
                for k, p in enumerate(params):
                    if not p:
                        continue
                    kinds, dep = self._uses(rel, lo, hi, p)
                    direct.setdefault((crate, name, k), set()).update(kinds)
                    deps.setdefault((crate, name, k), set()).update(dep)
                    self.param_text_inked[(crate, name, k)] = self._param_inked(
                        rel, lo, hi, p
                    )
        self.param_kind = {key: set(v) for key, v in direct.items()}
        for _ in range(8):
            changed = False
            for key, dep in deps.items():
                for d in dep:
                    add = self.param_kind.get(d, set())
                    if not add <= self.param_kind.get(key, set()):
                        self.param_kind.setdefault(key, set()).update(add)
                        changed = True
            if not changed:
                break

    def text_is_handled(self, rel, i, produced_by=""):
        """Is the colour produced on line `i` already legible where it is drawn?

        Two ways it can be, and neither is visible on the producing line:

          * the *use* is inked, not the binding -- `color: p.ink(c)` fifty
            lines below `let c = gauge_color_at(..)`;
          * the use is grounded -- `readable_on(badge_color)`, text drawn on
            that very colour, which must not be re-inked.

        Without this, `notif_pane` and `user_accounts` were reported as
        needing work they had already done correctly, and speedtest's gauge --
        whose local only ever colours a `Line` -- was reported as text.
        """
        lines, field = self.lines[rel], self.field[rel]
        if ".ink(" in statement_at(lines, i) or GROUNDED.search(lines[i]):
            return True
        # The colour may be handed straight to a helper that inks it inside.
        # `startupmanager` does exactly that: `draw_button` takes one colour,
        # washes the button with it, outlines with it, and writes the label in
        # `pal.ink(color)` -- so the call site is right and looks wrong.
        stmt = statement_at(lines, i)
        for callee in set(CALL.findall(stmt)):
            if not self.known(rel, callee):
                continue
            for idx, arg in enumerate(split_args(stmt, callee)):
                if produced_by not in arg:
                    continue
                key = (crate_of(rel), callee, idx + method_offset(stmt, callee))
                if self.param_text_inked.get(key):
                    return True
        m = LET.match(lines[i])
        if not m:
            return False
        name = m.group(1) or m.group(2)
        lo = enclosing_fn(lines, i)
        _, fn_hi = item_span(lines, lo, len(lines))
        hi = block_end(lines, i, fn_hi)
        found_text = False
        for j in range(i + 1, hi):
            if field.get(j) != "Text":
                continue
            if not mentions(lines[j], name):
                continue
            found_text = True
            stmt = statement_at(lines, j)
            if ".ink(" not in stmt and not GROUNDED.search(stmt):
                return False
        return found_text or True

    def classify(self, rel, i, produced_by):
        """What the colour produced on line `i` of `rel` ends up colouring."""
        lines, field = self.lines[rel], self.field[rel]
        if i in field:
            return {field[i]}
        kinds = set()
        stmt = statement_at(lines, i)
        for callee in set(CALL.findall(stmt)):
            if callee == produced_by or not self.known(rel, callee):
                continue
            k = arg_index(stmt, callee, produced_by, needle_may_be_a_method=True)
            if k is not None:
                kinds |= self.param_kind.get(
                    (crate_of(rel), callee, k + method_offset(stmt, callee)), set()
                )
        m = LET.match(lines[i])
        if m:
            name = m.group(1) or m.group(2)
            lo = enclosing_fn(lines, i)
            _, hi = item_span(lines, lo, len(lines))
            more, deps = self._uses(rel, i + 1, hi, name)
            kinds |= more
            for d in deps:
                kinds |= self.param_kind.get(d, set())
        return kinds or {"unresolved"}


def main(show_all):
    world = World()
    defs = {}
    for (_c, name), places in world.fns.items():
        for rel, lo, hi, _params in places:
            head = NL.join(world.lines[rel][lo : lo + 6])
            head = head[: head.find("{")] if "{" in head else head
            if not RETURNS_COLOR.search(head):
                continue
            body = NL.join(world.lines[rel][lo:hi])
            if not DUAL_USE.search(body) or GROUNDED.search(body):
                continue
            key = (crate_of(rel), name)
            if key in defs:
                # Two colour methods of the same name in one crate --
                # `DeviceCategory::color` and `DeviceStatus::color`. Nothing
                # here can tell their call sites apart without knowing the
                # receiver's type, so merging them invents a SPLIT: one's text
                # callers and the other's fills, reported as a single method
                # that does both. Marked ambiguous instead, which is the true
                # statement.
                defs[key]["ambiguous"] = True
                defs[key]["where"] += " and " + rel + ":" + str(lo + 1)
                continue
            defs[key] = {
                "ambiguous": False,
                "where": rel + ":" + str(lo + 1),
                "exempt": bool(EXEMPT_USE.search(body)),
                "takes_palette": "Palette" in head,
                "inked": ".ink(" in body,
                "kinds": collections.Counter(),
                "sites": collections.defaultdict(list),
            }

    for rel, lines in world.lines.items():
        for i, line in enumerate(lines):
            if DEF.match(line):
                continue
            for crate, name in defs:
                if crate != crate_of(rel):
                    continue
                # `[^\w]`, not `[^\w.]`: most of these are *methods*, so the
                # call reads `self.condition.color(p)` and a pattern that
                # refused a leading dot found none of them. Nine of the
                # forty-five reported no call site at all for that reason,
                # and a function with no callers is reported as needing no
                # decision -- the quietest way to be wrong here.
                if not re.search(r"[^\w]" + re.escape(name) + r"\s*\(", " " + line):
                    continue
                for kind in world.classify(rel, i, name):
                    defs[(crate, name)]["kinds"][kind] += 1
                    handled = defs[(crate, name)]["inked"] or world.text_is_handled(
                        rel, i, name
                    )
                    # "NOT INKED" is a *finding*; inside an ambiguous name it
                    # would be a guess. Two methods called `color` in one crate
                    # mean `defs[(crate, name)]` holds one of them, so `inked`
                    # answers about whichever was seen last -- and on
                    # 2026-09-13 that sent a reader to three sites, two of
                    # which were already inked per arm by the *other* method of
                    # the same name. Say which it is.
                    if defs[(crate, name)]["ambiguous"]:
                        mark = "  CANNOT TELL WHICH METHOD"
                    else:
                        mark = "" if handled else "  NOT INKED"
                    defs[(crate, name)]["sites"][kind].append(
                        rel + ":" + str(i + 1) + mark
                    )

    buckets = collections.defaultdict(list)
    for (_crate, name), d in sorted(defs.items()):
        drawn = dict(d["kinds"])
        unknown = drawn.pop("unresolved", 0)
        text = drawn.pop("Text", 0)
        if d["ambiguous"]:
            buckets["AMBIGUOUS -- TWO METHODS SHARE A NAME"].append((name, d))
        elif not text and not drawn:
            buckets["UNRESOLVED"].append((name, d))
        elif not text:
            buckets["NO TEXT CALLER"].append((name, d))
        elif drawn:
            buckets["SPLIT"].append((name, d))
        elif unknown:
            # Not INK. A caller this tool could not follow is not a caller
            # that draws text -- it is a caller nobody has looked at, and
            # `rate_color` was declared safe to ink on exactly that basis
            # while eight of its callers were filling the stat cards.
            buckets["INK IF THE REST CHECK OUT"].append((name, d))
        elif d["exempt"]:
            buckets["PER-ARM"].append((name, d))
        else:
            buckets["INK"].append((name, d))

    print(str(len(defs)) + " colour-returning functions name a dual-use role in their body")
    for key in ("INK", "PER-ARM", "INK IF THE REST CHECK OUT", "SPLIT",
                "NO TEXT CALLER", "UNRESOLVED",
                "AMBIGUOUS -- TWO METHODS SHARE A NAME"):
        rows = buckets.get(key, [])
        if not rows:
            continue
        # One rule for every bucket: a method is done when its text is
        # legible, whether that was arranged in its body or at its call
        # sites. Counting bodies alone reported 35 SPLIT methods outstanding
        # after all 24 of their call sites had been fixed -- and a SPLIT
        # method must *not* ink its body, so that count could never reach
        # zero. Counting sites alone would miss a method whose body inks and
        # whose callers this tool cannot follow.
        # A method with no text caller needs nothing: it is a fill's colour
        # and inking it would darken the fill. So the only outstanding work
        # is a *text site* that is neither inked at the site nor inked by the
        # method that produced it.
        todo = [
            r for r in rows
            if any("NOT INKED" in x for x in r[1]["sites"].get("Text", []))
        ]
        label = " with text that is not legible yet)"
        print(NL + "=== " + key + ": " + str(len(rows)) + "  (" + str(len(todo)) + label)
        for name, d in rows:
            if (name, d) not in todo and not show_all:
                continue
            counts = ", ".join(k + " x" + str(v) for k, v in d["kinds"].most_common())
            pal = "" if d["takes_palette"] else "  [takes no palette]"
            print("  " + name.ljust(24) + " " + d["where"].ljust(44) + " " + counts + pal)
            # Outstanding sites first. Printing the first two in file order
            # hid the only one that needed work behind two that did not, so
            # the summary said "1 outstanding" and the listing showed nothing.
            sites = sorted(
                d["sites"].get("Text", []), key=lambda x: "NOT INKED" not in x
            )
            for site in sites[:3]:
                print("        text at " + site)
    return 0


if __name__ == "__main__":
    sys.exit(main("--all" in sys.argv))
