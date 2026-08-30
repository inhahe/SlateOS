#!/usr/bin/env python3
"""Find GUI programs whose `main` never opens a window.

A SlateOS application becomes a window in exactly one place:

    oswindow::app::launch("name", &mut app)

`launch` connects to the compositor, creates the surface, and runs the event
loop until the window closes. A program that does not reach it has no window,
receives no key, no click and no tick, and exits as soon as `main` returns --
no matter how complete the app behind it is.

This is `known-issues.md` lesson 45 -- a feature with no production caller is a
feature that does not exist -- raised to whole-program scale, and it is the
same shape as the dead Write button and the dead Refresh button in
`apps/diskimager`: the code is *there*, the code is *tested*, and nothing in
the toolchain will ever mention it. `dead_code` cannot help, because from the
compiler's point of view every one of these types is constructed and every one
of these methods is called -- by `main`, or by the tests, or by both.

On 2026-08-26 this was measured across the tree: **six** programs opened a
window and **132** did not. Twenty had an empty `fn main()`. The other hundred
and twelve built the app, rendered one throwaway frame nobody would ever see,
printed a line, and exited -- several saying so out loud:

    // In a real Slate OS environment this would enter the compositor event loop
                                        -- apps/calculator/src/main.rs

    fn main() { let _app = SnakeApp::new(); }
                                        -- apps/snake/src/main.rs, in full

The point of a gate rather than a habit is the ratchet. `BASELINE` below is the
count as found; the check fails if the number goes *up*, and its number is
meant to be edited downwards as apps are wired. That is deliberately weaker
than "fail on any finding", because failing on all of them today would mean a
gate that is red from the moment it is written, and a gate that is always red
is a gate that gets commented out.

## What it flags

A file is reported when all three hold:

1.  It defines `fn main` in production code -- it is a program, not a library.
2.  It names `RenderTree` in production code -- it draws. This is what
    separates a GUI application from the command-line tools that also live
    under `apps/`; a program that never builds a render tree has no window to
    be missing.
3.  It never names `app::launch` (or `launch_with`) in production code.

The words "in production code" are what make this a gate rather than a
decoration. Comments and `#[cfg(test)]` items are blanked before the search
(see `rustscan`), so neither the comment promising that a real system "would
enter the event loop" nor a test that calls `App::render` by hand can vouch for
wiring that is not there.

Each finding also says *which kind* it is, because they need different work:

* `empty main` -- `fn main() {}` with nothing in it. The app is unreachable and
  nothing was even attempted.
* `simulation main` -- a `main` with a body that does not launch. Usually
  constructs the app, renders one frame and exits. Wiring it means writing an
  `impl oswindow::app::App` and replacing the body.
* `no main` -- a `RenderTree`-drawing `src/main.rs` with no `fn main` at all,
  which will not even link as a binary.

## What it cannot see

A program that calls `launch` from a branch that never runs, one that names its
entry point something other than `main` behind a macro, and any app whose
drawing goes through a type this check does not know by name. It never proves a
program *is* wired; it only names ones that provably are not.
"""

from __future__ import annotations

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

# Comment/literal/`#[cfg(test)]` blanking, and the brace matching that finds
# the end of an item. Shared with `check-tick-wiring.py`; the rationale for
# each defence is in that module's docstring.
from rustscan import INDENT, fn_body, production_only  # noqa: E402

ROOTS = ["gui", "apps", "net", "pkg"]

MAIN_RE = re.compile(INDENT + r"(?:pub\s+)?(?:async\s+)?fn\s+main\s*\(", re.M)

# The one door to the compositor. Both spellings, and both the fully-qualified
# and the imported form -- `use oswindow::app; ... app::launch(...)` is as wired
# as the long way round.
LAUNCH_RE = re.compile(r"\bapp::launch(?:_with)?\s*[(:]|\blaunch(?:_with)?::<")

# What a program that expects to be drawn builds. Not `Event`, and not
# `handle_event`: a headless harness can have both, but nothing assembles
# either of these unless it means for a compositor to paint the result.
#
# Both spellings are needed because the tree holds two generations of app. The
# newer ones build a `RenderTree`, which is what `oswindow::app::App::render`
# returns; the older ones return a flat `Vec<RenderCommand>` and were written
# before the tree existed. The flat ones are *more* unwired, not less -- they
# need the conversion as well as the launch -- so a check that only knew
# `RenderTree` would miss the harder half of its own population. Measured:
# 39 findings with `RenderTree` alone, 132 with both.
DRAWS_RE = re.compile(r"\bRenderTree\b|\bRenderCommand\b")


def classify(text: str) -> str | None:
    """How `text` fails to open a window, or None if it does not.

    None covers three separate "not applicable" answers on purpose -- the file
    is not a program, it is not a GUI program, or it is a GUI program that is
    correctly wired. A caller that wanted to tell those apart would be asking a
    different question than this gate asks.
    """
    prod = production_only(text)
    if not DRAWS_RE.search(prod):
        return None
    m = MAIN_RE.search(prod)
    if m is None:
        # A `main.rs` that draws but declares no `main` will not link. Anywhere
        # else -- a module, a library -- there is simply no entry point here to
        # be unwired, which is not this check's business.
        return "no main"
    if LAUNCH_RE.search(prod):
        return None
    body = fn_body(prod, m.start())
    if body is None or not body.strip():
        return "empty main"
    return "simulation main"


def main_line(text: str) -> int:
    """The 1-based line of `fn main`, for a finding to point at."""
    prod = production_only(text)
    m = MAIN_RE.search(prod)
    return 1 if m is None else prod.count("\n", 0, m.start()) + 1


# The count of findings as of 2026-08-30, so the gate can ratchet. Lower this
# whenever apps are wired; the check fails if it is ever exceeded. It is a
# ceiling, not a target -- see the module docstring on why it is not zero.
BASELINE = 64


# --------------------------------------------------------------------------
# Self-test
# --------------------------------------------------------------------------

WIRED = """
impl oswindow::app::App for Calc {
    fn render(&mut self, w: f32, h: f32) -> RenderTree { RenderTree::new() }
}

fn main() -> ExitCode {
    oswindow::app::launch("calc", &mut Calc::new())
}
"""

WIRED_VIA_IMPORT = """
use oswindow::app;

impl app::App for Calc {
    fn render(&mut self, w: f32, h: f32) -> RenderTree { RenderTree::new() }
}

fn main() -> ExitCode {
    app::launch_with("calc", None, &mut Calc::new())
}
"""

EMPTY_MAIN = """
impl Calc {
    fn render(&self, rt: &mut RenderTree) {}
}

fn main() {}
"""

SIMULATION_MAIN = """
impl Calc {
    fn render(&self, rt: &mut RenderTree) {}
}

fn main() {
    let mut app = Calc::new();
    let mut rt = RenderTree::new();
    app.render(&mut rt);
    println!("rendered {} nodes", rt.len());
}
"""

# A command-line tool under `apps/` is not a window that failed to open.
NOT_A_GUI = """
fn main() -> ExitCode {
    let args = parse();
    print_report(&args);
    ExitCode::SUCCESS
}
"""

# A library module draws, but its owner is what opens the window.
NO_MAIN_IN_A_MODULE = """
impl Sidebar {
    pub fn render(&self, rt: &mut RenderTree) {}
}
"""

# The fixture that makes this a gate rather than a decoration. Nearly every
# unwired app carries a comment promising the event loop it does not enter;
# if that counted as wiring, the check would be blind to the exact population
# it exists to find.
LAUNCH_ONLY_IN_A_COMMENT = """
impl Calc {
    fn render(&self, rt: &mut RenderTree) {}
}

fn main() {
    // In a real Slate OS environment this would call app::launch here and
    // enter the compositor event loop.
    let _app = Calc::new();
}
"""

# The other half: a test may drive `render` and even construct the app the way
# `launch` would, and a fix's regression test names `launch` in its own doc.
LAUNCH_ONLY_IN_A_TEST = """
impl Calc {
    fn render(&self, rt: &mut RenderTree) {}
}

fn main() {
    let _app = Calc::new();
}

#[cfg(test)]
mod tests {
    use oswindow::app::launch;

    #[test]
    fn it_launches() {
        let _ = launch;
    }
}
"""

# `-> ExitCode` must not be read as an unclosed generic; if it were, the body
# would never be found and a simulation would be misreported as empty.
RETURN_TYPE_BEFORE_BODY = """
impl Calc {
    fn render(&self, rt: &mut RenderTree) {}
}

fn main() -> Result<(), Box<dyn Error>> {
    let _app = Calc::new();
    Ok(())
}
"""

# Whitespace and a lone comment are not a body: after blanking, `{ }` is what
# a `fn main() { /* nothing yet */ }` leaves behind, and it is empty.
COMMENT_ONLY_MAIN = """
impl Calc {
    fn render(&self, rt: &mut RenderTree) {}
}

fn main() {
    // TODO: wire this up.
}
"""

SELF_TESTS = [
    ("a launched app is not reported", WIRED, None),
    ("`use oswindow::app` then `app::launch` counts", WIRED_VIA_IMPORT, None),
    ("an empty main is reported as empty", EMPTY_MAIN, "empty main"),
    ("a render-and-exit main is reported as a simulation", SIMULATION_MAIN, "simulation main"),
    ("a command-line tool is not reported", NOT_A_GUI, None),
    ("a drawing module with no main is reported", NO_MAIN_IN_A_MODULE, "no main"),
    ("a promise of an event loop is not an event loop", LAUNCH_ONLY_IN_A_COMMENT, "simulation main"),
    ("`launch` in a test does not count as wiring", LAUNCH_ONLY_IN_A_TEST, "simulation main"),
    ("a return type does not hide the body", RETURN_TYPE_BEFORE_BODY, "simulation main"),
    ("a main holding only a comment is empty", COMMENT_ONLY_MAIN, "empty main"),
]


def self_test() -> int:
    failed = 0
    for name, source, expected in SELF_TESTS:
        got = classify(source)
        ok = got == expected
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            print(f"       expected {expected!r}, got {got!r}")
            failed += 1
    print(f"\n{len(SELF_TESTS)} self-test case(s), {failed} failed")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    verbose = "-v" in argv or "--verbose" in argv

    root = pathlib.Path(__file__).resolve().parent.parent
    files: list[pathlib.Path] = []
    for name in ROOTS:
        for d in sorted(root.glob(f"{name}*")):
            if d.is_dir():
                files.extend(sorted(d.rglob("*.rs")))
    files = [f for f in files if "target" not in f.parts]

    problems: list[tuple[str, str]] = []
    wired: list[str] = []
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        prod = production_only(text)
        if not DRAWS_RE.search(prod):
            continue
        rel = path.relative_to(root).as_posix()
        kind = classify(text)
        if kind is None:
            if MAIN_RE.search(prod):
                wired.append(rel)
            continue
        if kind == "no main" and path.name != "main.rs":
            # A module that draws and has no `main` is the normal case.
            continue
        problems.append((f"{rel}:{main_line(text)}", kind))

    by_kind: dict[str, int] = {}
    for where, kind in problems:
        by_kind[kind] = by_kind.get(kind, 0) + 1
        print(f"{where}: {kind} -- this program draws, but never reaches `app::launch`")

    if verbose:
        print("\nwired:")
        for rel in wired:
            print(f"  {rel}")

    summary = ", ".join(f"{n} {k}" for k, n in sorted(by_kind.items()))
    print(
        f"\n{len(wired)} program(s) open a window, "
        f"{len(problems)} do not ({summary}); baseline {BASELINE}"
    )
    if len(problems) > BASELINE:
        print(
            f"FAIL: {len(problems) - BASELINE} more unwired program(s) than the "
            f"baseline of {BASELINE}. A new GUI app must reach `app::launch`."
        )
        return 1
    if len(problems) < BASELINE:
        print(
            f"note: {BASELINE - len(problems)} fewer than the baseline -- "
            f"lower BASELINE to {len(problems)} to hold the ground gained."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
