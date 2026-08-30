#!/usr/bin/env python3
"""Find apps that keep time but never receive the clock.

A GUI program's clock arrives as one event:

    Event::Tick { elapsed_ms }

`oswindow`'s event loop computes `now - this window's previous tick` and sends
that interval to the window. An app that wants to age something -- a stopwatch,
a metronome, a toast that expires, a WPM figure -- has to route that event to
whatever advances its state. If `handle_event` does not name `Event::Tick`, the
event lands in the `_ => {}` arm and the state is frozen for the life of the
process.

That failure is silent in a specific and nasty way. The advancing function is
usually *correct* and usually has its own tests, because a test can pass the
timestamp in by hand; the frozen program still lays out, still repaints, still
responds to the keyboard. What it shows is a plausible zero. This is
`known-issues.md` lesson 45 -- a feature with no production caller is a feature
that does not exist -- but the version of it that the `dead_code` lint cannot
reach, because the function *is* called: by the tests.

On 2026-08-25 this rule was applied to lane C by hand and found four:

  * `apps/stopwatch`   -- the stopwatch never counted.
  * `apps/metronome`   -- the metronome never beat, and `T` never tapped.
  * `apps/typingtutor` -- every WPM figure and every duration read zero.
  * `gui/notifications`-- toasts never aged out, so they never left the screen.

All four had passing tests over the frozen code. That is why this is a script
and not a review habit: four for four is not a coincidence, it is the default
outcome of an event enum with a `_ =>` arm.

## What it flags

A file is reported when all three hold:

1.  It defines `fn handle_event` -- so it is something the compositor drives,
    not a library type whose owner drives it.
2.  It defines a function taking a named *time* parameter -- `delta_ms`,
    `elapsed_ms`, `current_ms`, `time_ms`, `now_ms`, `delta_secs`, and the
    like. This is the tight half of the rule. A formatting helper
    (`format_time(total_ms)`) does not match, because it is not asking to be
    driven; a parameter called `delta_ms` is.
3.  It never mentions `Event::Tick` *in production code*.

Held to three conditions on purpose. Flagging every file with a `_ms` constant
would report dozens of non-problems, and a gate that cries wolf is a gate that
gets commented out.

The words "in production code" in condition 3 are the whole difference between
a gate and a decoration, and they were not there in the first draft. Comments
and `#[cfg(test)]` items are blanked out before the search, so neither the
explanatory comment nor the regression test that each of the four fixes leaves
behind can vouch for the wiring it is there to describe. Without that, every
file this check ever caused to be fixed would go permanently blind: delete the
match arm again and the test that was written to catch exactly that still
holds the file green. Verified by deleting `apps/stopwatch`'s arm on the live
tree -- the first draft said nothing, this one names it.

## What it cannot see

An app that *does* match `Event::Tick` but routes it somewhere useless, and an
app whose time-advancing parameter is named something this list does not know.
It never proves an app is correctly wired; it only names ones that provably are
not. Exit status is 1 if any are found, so it can be run as a gate.
"""

from __future__ import annotations

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

# The three defences that make this a gate rather than a decoration --
# comments, `#[cfg(test)]` items and brace-bearing literals all blanked before
# the search -- live in `rustscan`, because `check-window-wiring.py` needs
# exactly the same ones and two copies of a scanner are two copies that drift.
# The rationale for each, and the measurements behind their precise shapes
# (including why `INDENT` is `[ \t]*` and never `\s*`), are in that module.
from rustscan import INDENT, production_only, signature_of  # noqa: E402

ROOTS = ["gui", "apps", "net", "pkg"]

# The entry point the compositor calls. A type without one is driven by its
# owner, which may well tick it directly -- not this check's business.
HANDLE_EVENT_RE = re.compile(
    INDENT + r"(?:pub(?:\([^)]*\))?\s+)?fn\s+handle_event\s*[(<]", re.M
)

# The event that carries the clock. Matching the bare name is deliberate: an
# app that names it anywhere -- a `use`, a match arm, a test -- has at least
# been told the event exists, and a false negative is cheaper here than the
# alternative of parsing match arms.
TICK_RE = re.compile(r"\bEvent::Tick\b")

# A parameter that is asking to be driven by a clock. Not every `_ms`: see the
# module docstring on why the rule is this tight.
TIME_PARAM_RE = re.compile(
    r"\b(?:delta|elapsed|current|now|tick)_(?:ms|secs|seconds|time_ms)\b"
    r"|\bcurrent_time_ms\b"
    r"|\btime_ms\s*:\s*u\d+"
)

FN_RE = re.compile(
    INDENT + r"(?:pub(?:\([^)]*\))?\s+)?fn\s+([a-z_]\w*)\s*(?:<[^>]*>)?\s*\(", re.M
)


def timekeeping_functions(text: str) -> list[tuple[int, str]]:
    """Every `fn` in `text` whose parameters name a clock, as (line, name)."""
    found = []
    for m in FN_RE.finditer(text):
        sig = signature_of(text, m.start())
        if TIME_PARAM_RE.search(sig):
            found.append((text.count("\n", 0, m.start()) + 1, m.group(1)))
    return found


def inspect(text: str) -> list[tuple[int, str]] | None:
    """The unwired timekeeping functions in `text`, or None if not applicable.

    None means "this file is not the kind of thing the check is about" --
    it has no `handle_event`, or it already routes the tick.
    """
    prod = production_only(text)
    if not HANDLE_EVENT_RE.search(prod):
        return None
    if TICK_RE.search(prod):
        return None
    return timekeeping_functions(prod)


# --------------------------------------------------------------------------
# Self-test
# --------------------------------------------------------------------------

WIRED = """
impl App {
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(k) => self.key(k),
            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),
            _ => {}
        }
    }
    fn tick(&mut self, delta_ms: u64) { self.now += delta_ms; }
}
"""

UNWIRED = """
impl App {
    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(k) = event { self.key(k); }
    }
    fn tick(&mut self, delta_ms: u64) { self.now += delta_ms; }
}
"""

NO_HANDLE_EVENT = """
impl Animation {
    pub fn tick(&mut self, delta_ms: u64) { self.age += delta_ms; }
}
"""

NO_CLOCK = """
impl App {
    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(k) = event { self.key(k); }
    }
    fn format_time(&self, total_ms: u64) -> String { String::new() }
}
"""

TICK_ONLY_IN_A_COMMENT = """
impl App {
    // In production this would route Event::Tick to `tick`.
    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(k) = event { self.key(k); }
    }
    fn tick(&mut self, delta_ms: u64) { self.now += delta_ms; }
}
"""

NESTED_PAREN_SIGNATURE = """
impl App {
    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(k) = event { self.key(k); }
    }
    fn advance(&mut self, on_beat: impl Fn(u32) -> bool, delta_ms: u64) {}
}
"""

SECONDS_FLAVOUR = """
impl App {
    fn handle_event(&mut self, event: &Event) {}
    pub fn tick(&mut self, delta_secs: f32, mbps: f64) {}
}
"""

TIMESTAMP_FLAVOUR = """
impl App {
    fn handle_event(&mut self, event: &Event) {}
    fn set_time(&mut self, time_ms: u64) { self.current_time_ms = time_ms; }
}
"""

# The fixture that made this a gate rather than a decoration. Every file the
# check causes to be fixed acquires a test that constructs an `Event::Tick`;
# if that counted, the file would be permanently exempt from the check that
# found it, and deleting the arm again would go unnoticed by the very test
# written to notice it.
TICK_ONLY_IN_A_TEST = """
impl App {
    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(k) = event { self.key(k); }
    }
    fn tick(&mut self, delta_ms: u64) { self.now += delta_ms; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_event_advances_the_clock() {
        let mut app = App::new();
        app.handle_event(&Event::Tick { elapsed_ms: 100 });
        assert_eq!(app.now, 100);
    }
}
"""

# `#[cfg(test)]` on a bare item rather than a module, and a `;`-terminated one
# at that -- `item_end` has to know that an item can end without a brace.
CFG_TEST_ON_A_USE = """
#[cfg(test)]
use crate::testing::Event::Tick;

impl App {
    fn handle_event(&mut self, event: &Event) {}
    fn tick(&mut self, delta_ms: u64) {}
}
"""

# `not(test)` is production code: stripping it would hide a real finding.
CFG_NOT_TEST_IS_PRODUCTION = """
impl App {
    fn handle_event(&mut self, event: &Event) {}

    #[cfg(not(test))]
    fn tick(&mut self, delta_ms: u64) {}
}
"""

# A brace inside a literal must not throw the `#[cfg(test)]` brace match off
# and swallow the production code that follows it.
BRACE_IN_A_LITERAL = """
#[cfg(test)]
mod tests {
    const OPEN: char = '{';
    const ALSO: &str = "unbalanced { { {";
}

impl App {
    fn handle_event(&mut self, event: &Event) {}
    fn tick(&mut self, delta_ms: u64) {}
}
"""

# The signature is on the line the report points at, so blanking must keep
# every newline it removes.
LINE_NUMBERS_SURVIVE_BLANKING = """
/* a block comment
   spanning
   several lines */
impl App {
    fn handle_event(&mut self, event: &Event) {}
    fn tick(&mut self, delta_ms: u64) {}
}
"""

# `None` expects "not applicable"; a bare name expects that function to be
# reported; a `(line, name)` pair expects it on that line as well.
SELF_TESTS = [
    ("a wired app is not reported", WIRED, []),
    ("an unwired app is reported", UNWIRED, ["tick"]),
    ("a library type with no handle_event is not reported", NO_HANDLE_EVENT, None),
    ("an app with no clock parameter is not reported", NO_CLOCK, []),
    ("`Event::Tick` in a comment does not count as wiring", TICK_ONLY_IN_A_COMMENT, ["tick"]),
    ("`Event::Tick` in a test does not count as wiring", TICK_ONLY_IN_A_TEST, ["tick"]),
    ("a `#[cfg(test)] use` is stripped without eating the file", CFG_TEST_ON_A_USE, ["tick"]),
    ("`#[cfg(not(test))]` code is production code", CFG_NOT_TEST_IS_PRODUCTION, ["tick"]),
    ("a brace in a literal does not derail the test-module strip", BRACE_IN_A_LITERAL, ["tick"]),
    ("a nested paren in the signature does not hide the parameter", NESTED_PAREN_SIGNATURE, ["advance"]),
    ("seconds are a clock too", SECONDS_FLAVOUR, ["tick"]),
    ("an absolute timestamp counts as a clock", TIMESTAMP_FLAVOUR, ["set_time"]),
    ("blanking a block comment keeps the line numbers", LINE_NUMBERS_SURVIVE_BLANKING, [(7, "tick")]),
]


def self_test() -> int:
    failed = 0
    for name, source, expected in SELF_TESTS:
        got = inspect(source)
        if expected is None:
            ok = got is None
            got_desc = "not applicable" if got is None else [n for _, n in got]
        else:
            found = [] if got is None else got
            # Compare on whichever of (line, name) the expectation names, so a
            # case that does not care about lines does not have to count them.
            actual = [
                (line, fn) if isinstance(want, tuple) else fn
                for want, (line, fn) in zip(expected, found)
            ]
            ok = len(found) == len(expected) and actual == expected
            got_desc = "not applicable" if got is None else found
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            print(f"       expected {expected}, got {got_desc}")
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

    problems: list[str] = []
    wired = 0
    considered = 0
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        prod = production_only(text)
        if not HANDLE_EVENT_RE.search(prod):
            continue
        considered += 1
        if TICK_RE.search(prod):
            wired += 1
            continue
        rel = path.relative_to(root).as_posix()
        found = timekeeping_functions(prod)
        for line, fn in found:
            problems.append(
                f"{rel}:{line}: fn {fn} takes a clock, but this file's "
                f"`handle_event` never matches `Event::Tick`"
            )
        if verbose and not found:
            print(f"{rel}: handle_event, no Event::Tick, no clock parameter")

    for p in problems:
        print(p)
    print(
        f"{considered} file(s) with a `handle_event` checked, "
        f"{wired} already route `Event::Tick`, "
        f"{len(problems)} timekeeping function(s) left unwired"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
