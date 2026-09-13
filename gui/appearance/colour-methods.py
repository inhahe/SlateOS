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


def arg_index(line, callee, needle):
    """Which argument of `callee(...)` on `line` contains `needle`."""
    m = re.search(r"[^\w.]" + re.escape(callee) + r"\s*\(", " " + line)
    if not m:
        return None
    depth, idx, cur = 0, 0, ""
    for ch in (" " + line)[m.end():]:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        if ch == "," and depth == 0:
            if re.search(r"[^\w.]" + re.escape(needle) + r"[^\w]", " " + cur + " "):
                return idx
            idx += 1
            cur = ""
        else:
            cur += ch
    if re.search(r"[^\w.]" + re.escape(needle) + r"[^\w]", " " + cur + " "):
        return idx
    return None


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
                    self.fns.setdefault(m.group(1), []).append(
                        (rel, lo, hi, params_of(lines, i))
                    )
        self.param_kind = {}
        self._solve_params()

    def _uses(self, rel, lo, hi, name):
        """Kinds and onward dependencies for `name` inside one function body."""
        kinds, deps = set(), set()
        lines, field = self.lines[rel], self.field[rel]
        for j in range(lo, min(hi, len(lines))):
            line = lines[j]
            if not re.search(r"[^\w.]" + re.escape(name) + r"[^\w]", " " + line + " "):
                continue
            if j in field:
                kinds.add(field[j])
                continue
            if SHORTHAND.match(line) and name == "color":
                kinds.add(field.get(j) or "unresolved")
                continue
            for callee in set(CALL.findall(line)):
                if callee == name or callee not in self.fns:
                    continue
                k = arg_index(line, callee, name)
                if k is not None:
                    deps.add((callee, k))
        return kinds, deps

    def _solve_params(self):
        """`param_kind[(fn, index)]` -> the commands that parameter colours.

        A fixpoint rather than one pass: a colour is routinely handed down two
        levels -- `render_row` takes it and gives it to `render_badge`.
        """
        direct, deps = {}, {}
        for name, defs in self.fns.items():
            for rel, lo, hi, params in defs:
                for k, p in enumerate(params):
                    if not p:
                        continue
                    kinds, dep = self._uses(rel, lo, hi, p)
                    direct.setdefault((name, k), set()).update(kinds)
                    deps.setdefault((name, k), set()).update(dep)
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

    def classify(self, rel, i, produced_by):
        """What the colour produced on line `i` of `rel` ends up colouring."""
        lines, field = self.lines[rel], self.field[rel]
        if i in field:
            return {field[i]}
        kinds = set()
        for callee in set(CALL.findall(lines[i])):
            if callee == produced_by or callee not in self.fns:
                continue
            k = arg_index(lines[i], callee, produced_by)
            if k is not None:
                kinds |= self.param_kind.get((callee, k), set())
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
    for name, places in world.fns.items():
        for rel, lo, hi, _params in places:
            head = NL.join(world.lines[rel][lo : lo + 6])
            head = head[: head.find("{")] if "{" in head else head
            if not RETURNS_COLOR.search(head):
                continue
            body = NL.join(world.lines[rel][lo:hi])
            if not DUAL_USE.search(body) or GROUNDED.search(body):
                continue
            defs[(crate_of(rel), name)] = {
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
                    defs[(crate, name)]["sites"][kind].append(rel + ":" + str(i + 1))

    buckets = collections.defaultdict(list)
    for (_crate, name), d in sorted(defs.items()):
        drawn = {k: v for k, v in d["kinds"].items() if k != "unresolved"}
        text = drawn.pop("Text", 0)
        if not text and not drawn:
            buckets["UNRESOLVED"].append((name, d))
        elif not text:
            buckets["NO TEXT CALLER"].append((name, d))
        elif drawn:
            buckets["SPLIT"].append((name, d))
        elif d["exempt"]:
            buckets["PER-ARM"].append((name, d))
        else:
            buckets["INK"].append((name, d))

    print(str(len(defs)) + " colour-returning functions name a dual-use role in their body")
    for key in ("INK", "PER-ARM", "SPLIT", "NO TEXT CALLER", "UNRESOLVED"):
        rows = buckets.get(key, [])
        if not rows:
            continue
        todo = [r for r in rows if not r[1]["inked"]]
        print(NL + "=== " + key + ": " + str(len(rows)) + "  (" + str(len(todo)) + " not yet inked)")
        for name, d in rows:
            if d["inked"] and not show_all:
                continue
            counts = ", ".join(k + " x" + str(v) for k, v in d["kinds"].most_common())
            pal = "" if d["takes_palette"] else "  [takes no palette]"
            print("  " + name.ljust(24) + " " + d["where"].ljust(44) + " " + counts + pal)
            for site in d["sites"].get("Text", [])[:2]:
                print("        text at " + site)
    return 0


if __name__ == "__main__":
    sys.exit(main("--all" in sys.argv))
