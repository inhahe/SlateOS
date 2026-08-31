"""Lesson 91's mechanical check: whole-frame text assertions that name a band.

A windowed app's test suite grows a helper of this shape, copied forward from
app to app:

    fn says(frame: &Frame<Target>, needle: &str) -> bool {
        texts(frame).iter().any(|t| t.contains(needle))
    }

It answers "does the window tell the player X *somewhere*", which is a fine
thing to assert -- until the string it is given is one that more than one part
of the program paints.  Then a test called
`the_status_band_says_what_the_game_is_doing` is, in fact, only asserting
something about the frame, and a mutation that silences the status band goes
unnoticed because the header still says it.  That is exactly what happened in
`gomoku`, where "White is thinking" is painted by both `draw_header` and
`draw_status`; two tests named the band, and the mutant survived both.

This script runs the check that finds it without a mutation sweep.  For every
bare `says(&frame, "X")` call in a crate's test module, it reports which
production functions paint a string literal containing "X".  More than one
owner means the assertion cannot tell them apart; the fix is `says_in`, taking
the `Rect` the band occupies:

    fn says_in(frame: &Frame<Target>, needle: &str, r: Rect) -> bool {
        frame.commands().iter().any(|c| {
            matches!(c, RenderCommand::Text { text, x, y, .. }
                if text.contains(needle) && r.contains(*x, *y))
        })
    }

Usage:

    python scripts/check-frame-needles.py                # every app that has one
    python scripts/check-frame-needles.py gomoku chess   # named crates only

Exit status is 1 when any needle has more than one painter, so it can gate a
commit.  It is a heuristic, not a proof -- read the flagged lines rather than
trusting the count.

Two deliberate limits, both of which cost recall rather than precision:

  * Matching is against string *literals* inside each function, not against
    whole function bodies.  Body matching is useless -- the needle "A" is a
    substring of nearly every function in a file, and the real findings drown.
  * A needle assembled by `format!` ("Moves: 0") has no literal to match, so it
    is reported as `<no literal>` and never flagged.  Those are usually
    panel-only strings, which are unambiguous by construction, but the script
    cannot say so; it says it does not know.
  * Ownership is substring containment, so a one- or two-character needle is
    noisy in the other direction: `gomoku` asserts the coordinate margin says
    "A", and the script attributes it to `draw_status`, which paints "A draw.
    N for a new game".  One owner, so nothing is flagged -- but a short needle
    that collects two owners is as likely to be an accident of spelling as a
    real ambiguity.  Read the line before believing it.
"""

import io
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
APPS = REPO / "apps"

FN = re.compile(r"^    fn (\w+)", re.M)
LIT = re.compile(r'"((?:[^"\\]|\\.)*)"')
# Only the *bare* whole-frame form is the hazard.  `says_in` already names a
# band, so it is the fix, not the fault -- matching `says\w*` would flag every
# repaired call site as if it were still broken.  The `\b` before it is what
# keeps `says_in` and `frame_says` out.
CALL = re.compile(r"\bsays\(")
# Functions that return a string without painting it cannot be confused with a
# band: `App::title` hands "Gomoku" to the window manager, not to the frame.
NOT_PAINT = {"title", "app_id", "default", "new", "name", "label"}


def nth_argument(src, open_paren, want):
    """The text of argument `want` (0-based) of the call whose `(` is at `open_paren`.

    A regex cannot do this.  The first argument is routinely a call of its own
    (`&app.frame(W.0, W.1)`), so the argument separator is the comma at depth
    one and not the first comma; and the call is routinely wrapped in
    `assert!(..., "message")`, so a pattern that scans forward for a quoted
    string finds the *failure message* and reports it as a needle.  An earlier
    version of this script did exactly that and invented eleven needles for
    `wordsearch` that no test ever passed.

    Returns None if the call is malformed or has no argument `want`.
    """
    depth, arg, start, i = 0, 0, None, open_paren
    while i < len(src):
        c = src[i]
        if c == '"':  # skip a string whole, escapes and all
            i += 1
            while i < len(src) and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
        elif c in "([{":
            depth += 1
            if depth == 1:
                start = i + 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                return src[start:i].strip() if arg == want else None
        elif c == "," and depth == 1:
            if arg == want:
                return src[start:i].strip()
            arg += 1
            start = i + 1
        i += 1
    return None


def needle_position(tests):
    """Which argument of this crate's `says` helper is the needle.

    It is not always the second.  `gomoku` and `towers` declare
    `says(frame, needle)`, but `wordsearch` declares
    `says(a, size, needle)` -- it re-renders at a given window size rather
    than taking a frame -- so a script that assumed position 1 read the
    *size* as the needle and reported every call as unresolvable.  Read the
    position off the declaration instead of assuming it.
    """
    at = tests.index("fn says(")
    depth, args, start, i = 0, [], None, at + len("fn says")
    while i < len(tests):
        c = tests[i]
        if c in "([{":
            depth += 1
            if depth == 1:
                start = i + 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                args.append(tests[start:i])
                break
        elif c == "," and depth == 1:
            args.append(tests[start:i])
            start = i + 1
        i += 1
    for n, a in enumerate(args):
        if a.strip().startswith("needle"):
            return n
    # No parameter is called `needle`; fall back to the last `&str` it takes,
    # which is what such a helper's haystack argument has always been.
    for n, a in reversed(list(enumerate(args))):
        if "&str" in a:
            return n
    return 1


def needle_of(arg):
    """The string an argument denotes, or None when it is not a plain literal."""
    if arg is None:
        return None
    m = re.fullmatch(r'"((?:[^"\\]|\\.)*)"', arg)
    return m.group(1) if m else None


def fn_literals(prod):
    """(name, [string literals]) for every fn in the production half."""
    starts = [(m.group(1), m.start()) for m in FN.finditer(prod)]
    out = []
    for i, (name, at) in enumerate(starts):
        end = starts[i + 1][1] if i + 1 < len(starts) else len(prod)
        out.append((name, LIT.findall(prod[at:end])))
    return out


def check(path):
    """Report `path`'s ambiguous needles; return how many there were."""
    s = io.open(path, encoding="utf-8", newline="").read()
    if "fn says(" not in s or "mod tests {" not in s:
        return 0
    cut = s.index("mod tests {")
    prod, tests = s[:cut], s[cut:]
    fns = fn_literals(prod)

    want = needle_position(tests)
    args = [
        nth_argument(tests, m.end() - 1, want)
        for m in CALL.finditer(tests)
        # The helper's own declaration is a `says(` like any other, and reading
        # it as a call reports the parameter list as an unresolvable needle.
        if not tests[max(0, m.start() - 3) : m.start()].endswith("fn ")
    ]
    needles = sorted({n for a in args if (n := needle_of(a)) is not None})
    # A call whose needle is a variable -- `says(&frame, needle)` inside a loop
    # over a table of phases -- carries no literal to look up, and is precisely
    # the shape gomoku's surviving mutant hid behind.  The script cannot check
    # those, and saying so is the difference between a clean report and a
    # misleading one.
    opaque = sorted({a for a in args if needle_of(a) is None and a is not None})
    if not needles and not opaque:
        return 0
    print("=" * 72)
    print("%s -- %d whole-frame needle(s)" % (path.parent.parent.name, len(needles)))
    bad = 0
    for n in needles:
        owners = [
            f for f, lits in fns if f not in NOT_PAINT and any(n in lit for lit in lits)
        ]
        ambiguous = len(owners) > 1
        bad += ambiguous
        print(
            "  %s %-42r %s"
            % ("[!!]" if ambiguous else "    ", n, ", ".join(owners) or "<no literal>")
        )
    for a in opaque:
        print(
            "  [??] %-42s this script cannot resolve; read it by hand"
            % (a if len(a) < 42 else a[:39] + "...")
        )
    return bad


def main(argv):
    wanted = set(argv[1:])
    sources = sorted(APPS.glob("*/src/main.rs"))
    if wanted:
        sources = [p for p in sources if p.parent.parent.name in wanted]
        missing = wanted - {p.parent.parent.name for p in sources}
        if missing:
            print("no such app: %s" % ", ".join(sorted(missing)))
            return 2
    bad = sum(check(p) for p in sources)
    print("=" * 72)
    if bad:
        print(
            "%d needle(s) painted in more than one place: scope those "
            "assertions to the band's Rect (known-issues lesson 91)." % bad
        )
        return 1
    print("no whole-frame needle is painted in more than one place.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
