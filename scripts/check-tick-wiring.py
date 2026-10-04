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

## The other half: the clock has to be asked for

Routing the tick is half of the wiring. `oswindow` delivers `Event::Tick` only
to a window whose `App::tick_interval` names an interval, and the trait's
default is `None` -- so an app that acts on the tick but never says how often
it wants one receives none, and is exactly as frozen as one with no Tick arm.
Its tests pass all the same, because a test hands the tick in by hand.

On 2026-10-03 lane E found three this way, all in a sweep prompted by the
first:

  * `apps/credmanager` -- the vault never auto-locked, however long it was
    left. A password manager.
  * `apps/defrag`      -- a started defrag stood at nought per cent for good.
  * `apps/benchmark`   -- the timer beside a running suite; the suite does not
    yet run between events, so the next change to it would have frozen it.

So a crate is also reported when it implements `App`, names `Event::Tick` in
production code, and never writes `fn tick_interval` there. Per crate, not per
file: the `impl App`, the Tick arm and `tick_interval` can each live in a
different module. An app that names the tick and genuinely wants none says so
by writing `tick_interval` out, returning `None`, with the reason beside it --
which is also the only way a reader can tell "wants no clock" from "forgot".

## What it cannot see

An app that *does* match `Event::Tick` but routes it somewhere useless, an app
whose time-advancing parameter is named something this list does not know, and
an app whose `tick_interval` asks for a clock at the wrong moments. It never
proves an app is correctly wired; it only names ones that provably are not.
Exit status is 1 if any are found, so it can be run as a gate.
"""

from __future__ import annotations

import pathlib
import re
import sys

import selftestflag

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


# The second rule's two patterns -- see "The other half" in the docstring.
# `impl App for`, optionally generic and optionally path-qualified
# (`impl oswindow::app::App for`).
APP_IMPL_RE = re.compile(r"\bimpl(?:\s*<[^>]*>)?\s+(?:[\w:]+::)?App\s+for\b")
TICK_INTERVAL_RE = re.compile(INDENT + r"fn\s+tick_interval\s*\(", re.M)


def asks_for_no_clock(sources: list[str]) -> bool:
    """Whether the crate made of `sources` acts on the tick but never asks
    for one: it implements `App` and names `Event::Tick` in production code,
    and writes no `fn tick_interval` there."""
    prod = "\n".join(production_only(s) for s in sources)
    return (
        bool(APP_IMPL_RE.search(prod))
        and bool(TICK_RE.search(prod))
        and not TICK_INTERVAL_RE.search(prod)
    )


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


# The second rule, over whole crates: each case is a crate's sources and
# whether it asks for no clock.
ACTS_ON_THE_TICK = """
impl App for Defrag {
    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::Tick { .. } => self.step(),
            _ => Response::Idle,
        }
    }
}
"""

ASKS_FOR_IT = """
impl App for Defrag {
    fn tick_interval(&self) -> Option<Duration> {
        self.running.then_some(TICK)
    }
    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::Tick { .. } => self.step(),
            _ => Response::Idle,
        }
    }
}
"""

# The arm in one module, `impl App` and `tick_interval` in another.
ARM_IN_ONE_FILE = """
fn handle_event(state: &mut State, event: &Event) -> EventResult {
    match event {
        Event::Tick { elapsed_ms } => state.advance(*elapsed_ms),
        _ => EventResult::Ignored,
    }
}
"""

APP_IN_ANOTHER = """
impl oswindow::app::App for State {
    fn tick_interval(&self) -> Option<Duration> { Some(TICK) }
}
"""

APP_IN_ANOTHER_ASKING_NOTHING = """
impl<'a> oswindow::app::App for State<'a> {
    fn title(&self) -> String { String::new() }
}
"""

# A library type: its owner ticks it, and asks for the owner's clock.
NO_APP = """
impl Toast {
    pub fn handle_event(&mut self, event: &Event) {
        if let Event::Tick { elapsed_ms } = event { self.age += *elapsed_ms; }
    }
}
"""

TICK_INTERVAL_ONLY_IN_A_COMMENT = """
impl App for Defrag {
    // fn tick_interval(&self) -> Option<Duration> -- to do
    fn on_event(&mut self, event: &Event) -> Response {
        if let Event::Tick { .. } = event { self.step(); }
        Response::Idle
    }
}
"""

TICK_ONLY_IN_A_TEST_OF_AN_APP = """
impl App for Calculator {
    fn on_event(&mut self, event: &Event) -> Response { Response::Idle }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tick_changes_nothing() {
        assert_eq!(app.on_event(&Event::Tick { elapsed_ms: 16 }), Response::Idle);
    }
}
"""

CRATE_SELF_TESTS = [
    ("an app that acts on the tick and asks for none is reported", [ACTS_ON_THE_TICK], True),
    ("an app that asks for the tick it acts on is not", [ASKS_FOR_IT], False),
    ("the arm and the ask may be in different modules", [ARM_IN_ONE_FILE, APP_IN_ANOTHER], False),
    (
        "an arm in one module with no ask in the other is reported",
        [ARM_IN_ONE_FILE, APP_IN_ANOTHER_ASKING_NOTHING],
        True,
    ),
    ("a library type its owner ticks is not reported", [NO_APP], False),
    (
        "`tick_interval` in a comment does not count as asking",
        [TICK_INTERVAL_ONLY_IN_A_COMMENT],
        True,
    ),
    ("a tick in an app's tests is not a production arm", [TICK_ONLY_IN_A_TEST_OF_AN_APP], False),
]


def self_test() -> int:
    failed = 0
    for name, sources, expected in CRATE_SELF_TESTS:
        got = asks_for_no_clock(sources)
        ok = got == expected
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            print(f"       expected {expected}, got {got}")
            failed += 1
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
    cases = len(SELF_TESTS) + len(CRATE_SELF_TESTS)
    print(f"\n{cases} self-test case(s), {failed} failed")
    return 1 if failed else 0


def crates_asking_for_no_clock(root: pathlib.Path) -> list[str]:
    """Every crate under the scanned roots that acts on the tick and never
    asks for one, as a path relative to `root`."""
    found = []
    for name in ROOTS:
        for d in sorted(root.glob(f"{name}*")):
            if not d.is_dir():
                continue
            for manifest in sorted(d.rglob("Cargo.toml")):
                crate = manifest.parent
                if "target" in crate.relative_to(root).parts:
                    continue
                sources = []
                for path in sorted((crate / "src").rglob("*.rs")):
                    try:
                        sources.append(path.read_text(encoding="utf-8"))
                    except (OSError, UnicodeDecodeError):
                        continue
                if asks_for_no_clock(sources):
                    found.append(crate.relative_to(root).as_posix())
    return found


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
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

    unasked = crates_asking_for_no_clock(root)
    for crate in unasked:
        problems.append(
            f"{crate}: implements `App` and acts on `Event::Tick`, but never "
            f"asks for one -- `App::tick_interval` is the trait's default, "
            f"`None`, so no tick is delivered and what the arm advances is "
            f"frozen. Write `tick_interval` out: the interval while there is "
            f"something to advance, `None` otherwise -- and `None` with the "
            f"reason beside it if the app truly wants no clock."
        )

    for p in problems:
        print(p)
    print(
        f"{considered} file(s) with a `handle_event` checked, "
        f"{wired} already route `Event::Tick`, "
        f"{len(problems) - len(unasked)} timekeeping function(s) left unwired; "
        f"{len(unasked)} crate(s) act on the tick and never ask for one"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
