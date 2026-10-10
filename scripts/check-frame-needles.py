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
    python scripts/check-frame-needles.py --self-test    # check the checker

Exit status is 1 when any needle has more than one painter, so it can gate a
commit, and 2 for an app name or an option it does not know.  It is a
heuristic, not a proof -- read the flagged lines rather than trusting the
count.

`--self-test` builds small fixture crates in a temporary folder and checks
the verdict on each: two painters reported, one painter passed, `says_in`
passed, a needle that appears only in a comment or only in the test module
passed, and a crate named on the command line scanned while one not named is
not (requests/a-e-check-frame-needles-needs-a-self-test.md).  It runs at push
time, before the real scan is trusted (design-decisions §974).

Comments are blanked out before literals are collected: a literal in a
comment -- `// the header used to say "White is thinking" too` -- paints
nothing, and counted, it made one painter look like two.

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

import contextlib
import io
import pathlib
import re
import sys
import tempfile

import selftestflag  # scripts/selftestflag.py

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


def strip_comments(src):
    """`src` with its comments and char literals blanked out and its string
    literals kept.

    `//` runs to the end of the line (the line break stays, so `FN`'s `^`
    still finds the next line); `/* */` nests, as Rust's does, and becomes a
    space.  String literals are copied whole, so a `//` inside
    `"http://..."` is not taken for a comment.  A char literal becomes `'_'`:
    it paints nothing, and the quote in `'"'` would otherwise open a string
    for `LIT` that swallows the real literal after it.  A `'` that does not
    open a char literal is a lifetime and is copied as it is.
    """
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            out.append(src[i : j + 1])
            i = j + 1
        elif c == "'":
            if i + 1 < n and src[i + 1] == "\\":
                # '\n', '\'', '\u{1F4B0}': up to the quote after the escape.
                close = src.find("'", i + 3)
                close = n - 1 if close < 0 else close
                out.append("'_'")
                i = close + 1
            elif i + 2 < n and src[i + 2] == "'":
                out.append("'_'")
                i += 3
            else:
                out.append(c)
                i += 1
        elif src.startswith("//", i):
            end = src.find("\n", i)
            i = n if end < 0 else end
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(" ")
            i = j
        else:
            out.append(c)
            i += 1
    return "".join(out)


def fn_literals(prod):
    """(name, [string literals]) for every fn in the production half,
    comments left out (`strip_comments`)."""
    prod = strip_comments(prod)
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


def main(argv, apps=APPS):
    unknown = selftestflag.unknown_options(argv[1:])
    if unknown:
        print("unrecognised option(s): %s" % ", ".join(unknown))
        return 2
    if selftestflag.wants_selftest(argv[1:]):
        return self_test()
    wanted = set(argv[1:])
    sources = sorted(apps.glob("*/src/main.rs"))
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


# ── The self-test's fixtures ─────────────────────────────────────────────────
#
# Each is the production half of a crate, or its test module, as an app's
# `main.rs` has them: methods four spaces in (what `FN` finds), and a test
# module holding the `says` helper.

PROD_TWO_PAINTERS = """
impl Game {
    fn draw_header(&self, f: &mut Frame) {
        f.text("White is thinking");
    }
    fn draw_status(&self, f: &mut Frame) {
        f.text("White is thinking about its move");
    }
}
"""

PROD_ONE_PAINTER = """
impl Game {
    fn draw_header(&self, f: &mut Frame) {
        f.text("Gomoku");
    }
    fn draw_status(&self, f: &mut Frame) {
        f.text("White is thinking");
    }
}
"""

# The needle in comments of the header -- a line comment, and a block comment
# beside a real literal. One painter, as far as anything is painted.
PROD_IN_COMMENTS = """
impl Game {
    fn draw_header(&self, f: &mut Frame) {
        // The header used to say "White is thinking" too.
        f.text("Gomoku"); /* not "White is thinking" any more */
    }
    fn draw_status(&self, f: &mut Frame) {
        f.text("White is thinking");
    }
}
"""

# Two painters, one with a char literal that is a quote before its literal:
# taken for the start of a string, it hides the second painter.
PROD_QUOTE_CHAR = """
impl Game {
    fn draw_header(&self, f: &mut Frame) {
        f.text("White is thinking");
    }
    fn draw_status(&self, f: &mut Frame) {
        let quote = '"';
        f.text("White is thinking about its move");
    }
}
"""

TESTS_SAYS = """
mod tests {
    use super::*;

    fn says(frame: &Frame, needle: &str) -> bool {
        frame.texts().iter().any(|t| t.contains(needle))
    }

    #[test]
    fn the_status_band_says_white_is_thinking() {
        let frame = Frame::new();
        assert!(says(&frame, "White is thinking"), "the status band said nothing");
    }
}
"""

TESTS_SAYS_IN = """
mod tests {
    use super::*;

    fn says(frame: &Frame, needle: &str) -> bool {
        frame.texts().iter().any(|t| t.contains(needle))
    }

    fn says_in(frame: &Frame, needle: &str, r: Rect) -> bool {
        frame.texts_in(r).iter().any(|t| t.contains(needle))
    }

    #[test]
    fn the_status_band_says_white_is_thinking() {
        let frame = Frame::new();
        assert!(says_in(&frame, "White is thinking", status_band()));
    }
}
"""

# The needle in the test module's own functions as well as its call: what the
# tests say is not what the program paints.
TESTS_SAYS_WITH_A_TEST_PAINTER = """
mod tests {
    use super::*;

    fn says(frame: &Frame, needle: &str) -> bool {
        frame.texts().iter().any(|t| t.contains(needle))
    }

    fn expected_status() -> &'static str {
        "White is thinking"
    }

    #[test]
    fn the_status_band_says_white_is_thinking() {
        let frame = Frame::new();
        assert!(says(&frame, "White is thinking"), "{}", expected_status());
    }
}
"""


def write_crate(apps, name, prod, tests):
    """`apps/<name>/src/main.rs` holding `prod` then `tests`; its path."""
    src = apps / name / "src"
    src.mkdir(parents=True, exist_ok=True)
    path = src / "main.rs"
    path.write_bytes((prod + "\n#[cfg(test)]\n" + tests).encode("utf-8"))
    return path


def self_test():
    """Check the checker's verdicts on fixture crates. 0 when every one is
    right; 1, naming the wrong ones, otherwise."""
    cases = [
        ("two painters of a bare says() needle are reported", PROD_TWO_PAINTERS, TESTS_SAYS, 1),
        ("one painter is passed", PROD_ONE_PAINTER, TESTS_SAYS, 0),
        ("says_in names the band, and is passed", PROD_TWO_PAINTERS, TESTS_SAYS_IN, 0),
        ("a needle that is only in comments paints nothing", PROD_IN_COMMENTS, TESTS_SAYS, 0),
        ("a quote in a char literal hides no painter", PROD_QUOTE_CHAR, TESTS_SAYS, 1),
        (
            "a needle the test module itself spells is not a painter",
            PROD_ONE_PAINTER,
            TESTS_SAYS_WITH_A_TEST_PAINTER,
            0,
        ),
    ]
    failures = []
    ran = 0
    with tempfile.TemporaryDirectory(prefix="check-frame-needles-") as tmp:
        apps = pathlib.Path(tmp) / "apps"
        for n, (what, prod, tests, want) in enumerate(cases):
            path = write_crate(apps, "case%d" % n, prod, tests)
            with contextlib.redirect_stdout(io.StringIO()):
                got = check(path)
            ran += 1
            if got != want:
                failures.append("%s: %d ambiguous needle(s), want %d" % (what, got, want))

        # Which crates a run scans: only those named, every one when none is.
        named = pathlib.Path(tmp) / "named"
        write_crate(named, "alpha", PROD_ONE_PAINTER, TESTS_SAYS)
        write_crate(named, "beta", PROD_TWO_PAINTERS, TESTS_SAYS)
        for argv, want, what in [
            (["alpha"], 0, "a crate not named is not scanned"),
            (["beta"], 1, "a crate named is scanned"),
            ([], 1, "with none named, every crate is scanned"),
            (["gamma"], 2, "a name no crate has is refused"),
            (["--slef-test"], 2, "an option it does not know is refused"),
        ]:
            with contextlib.redirect_stdout(io.StringIO()):
                got = main(["check-frame-needles.py", *argv], apps=named)
            ran += 1
            if got != want:
                failures.append("%s: exit %d, want %d" % (what, got, want))

    for failure in failures:
        print("FAIL %s" % failure)
    print(
        "check-frame-needles self-test: %d case(s) ran, %d wrong"
        % (ran, len(failures))
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
