#!/usr/bin/env python3
"""Find windowed programs that treat a key coming back up as a second press.

A `guitk::event::KeyEvent` carries a `pressed` flag, and the compositor sends
*two* of them per keystroke -- one with `pressed: true` when the key goes down
and one with `pressed: false` when it comes back up (`gui/compositor/src/lib.rs`
builds both). An application that matches on `key` and never reads `pressed`
therefore runs every action twice, and the second run happens at the moment the
user lets go, which is the moment they believe they have finished.

The damage is not cosmetic and it is not uniform. In `apps/connect4` -- where
this gate came from -- a keypress dropped the player's piece and the release
dropped a second one into the next free cell, played on the AI's behalf, so the
board advanced two moves for one keystroke and the game the player was shown was
not the game they were playing. In `apps/life` the same fault stepped the
generation twice per press; in `apps/maze` it moved the walker two cells.

## Why a gate and not a habit

Because the habit was already there and it was still lost. At the time this was
written, ninety-two applications in the tree read `pressed` and one did not --
and the one that did not was the *most recently rewritten* of them, whose author
had just written the guard into six other apps in a row. A rule kept only by
copying is a rule that gets dropped the first time somebody writes the handler
from memory rather than from the file next door, and nothing in the toolchain
says a word: the code compiles, every test that presses a key passes, and the
fault is visible only to someone who thinks to release one.

## What it flags

A file is reported when all three hold in **production** code:

1.  It reaches `app::launch` -- it really opens a window, so a real compositor
    really will send it releases. An unwired simulation cannot exhibit the
    fault yet, and reporting one would put this gate's findings in competition
    with `check-window-wiring.py`'s, which are the same files for a different
    and more basic reason.
2.  It matches `Event::Key` -- it routes key events at all.
3.  It never reads `.pressed` -- neither as a guard, nor as a match arm
    (`pressed: true`), nor passed on to a handler that takes it.

Test code and comments are blanked first (see `rustscan`), for the reason that
module's docstring gives: the regression test written to catch a relapse is
exactly what would otherwise hide it, since that test is the one place a
`pressed: false` event is constructed.

## Baseline

Zero, and it should stay zero. Unlike `check-window-wiring.py` -- whose
population was 132 files on the day it was written, so a zero baseline would
have meant a permanently red gate -- this one starts clean: the single finding
that motivated it was fixed in the same change. There is no reason to admit a
second.

## What it cannot see

An app that reads `pressed` for one key and forgets it for another; an app whose
guard is written in a helper this check cannot follow (it only asks whether the
field is read *somewhere* in production code, not whether it gates the right
thing); and any app whose key routing goes through a name other than
`Event::Key`. Like its neighbour, it never proves an app is right -- it names
ones that are provably wrong.
"""

from __future__ import annotations

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from rustscan import production_only  # noqa: E402

ROOTS = ["gui", "apps", "net", "pkg"]

# The one door to the compositor -- the same shape `check-window-wiring.py`
# uses, and deliberately the same, because the two gates partition the
# population between them: that check owns the programs that never launch, this
# one owns the programs that do.
LAUNCH_RE = re.compile(r"\bapp::launch(?:_with)?\s*[(:]|\blaunch(?:_with)?::<")

# Routing a key. Both the enum arm and the bare payload type: a program can
# destructure `Event::Key(ev)` in one place and take a `&KeyEvent` in the
# handler, and either is enough to make the question apply.
KEY_RE = re.compile(r"\bEvent::Key\b|\bKeyEvent\b")

# Reading the flag, in any of the three shapes production code uses it in: a
# field access (`ev.pressed`), a pattern (`pressed: true`), and a call that
# passes it on (`handle_key(ke.key, ke.pressed)`). The first covers the last,
# which is why there are two patterns and not three.
PRESSED_RE = re.compile(r"\.pressed\b|\bpressed\s*:")


def classify(text: str) -> str | None:
    """Whether `text` is a windowed key handler that ignores releases."""
    prod = production_only(text)
    if not LAUNCH_RE.search(prod):
        return None
    if not KEY_RE.search(prod):
        return None
    if PRESSED_RE.search(prod):
        return None
    return "ignores key releases"


def key_line(text: str) -> int:
    """The 1-based line of the first key match, for a finding to point at."""
    prod = production_only(text)
    m = KEY_RE.search(prod)
    return 1 if m is None else prod.count("\n", 0, m.start()) + 1


# The count of findings as of 2026-08-28. See the module docstring: this one is
# meant to stay at zero, not to be ratcheted down.
BASELINE = 0


# --------------------------------------------------------------------------
# Self-test
# --------------------------------------------------------------------------

GUARDED = """
use oswindow::app;

pub fn key_intent(ev: &KeyEvent) -> Option<Intent> {
    if !ev.pressed {
        return None;
    }
    match ev.key { Key::N => Some(Intent::NewGame), _ => None }
}

fn main() -> ExitCode { app::launch("game", &mut Game::new()) }
"""

# The shape the fault actually shipped in: a complete, correct-looking handler
# that reads `key` and nothing else.
UNGUARDED = """
use oswindow::app;

fn on_event(&mut self, event: &Event) -> Response {
    match event {
        Event::Key(ev) => self.handle_key(ev),
        _ => Response::Idle,
    }
}

fn main() -> ExitCode { app::launch("game", &mut Game::new()) }
"""

# An app that opens a window and takes no keys at all -- a clock, a monitor --
# has no question to answer here.
LAUNCHES_BUT_TAKES_NO_KEYS = """
use oswindow::app;

fn on_event(&mut self, event: &Event) -> Response {
    match event {
        Event::Tick { .. } => Response::Redraw,
        _ => Response::Idle,
    }
}

fn main() -> ExitCode { app::launch("clock", &mut Clock::new()) }
"""

# An unwired simulation with the same defect is `check-window-wiring.py`'s
# finding, not this one. Reporting it here would double-count the same file for
# a fault it cannot yet exhibit.
UNWIRED_SIMULATION = """
fn on_event(&mut self, event: &Event) -> Response {
    match event {
        Event::Key(ev) => self.handle_key(ev),
        _ => Response::Idle,
    }
}

fn main() { let _app = Game::new(); }
"""

# A pattern match on the flag is a reading of it. Menus and accelerator tables
# are written this way rather than with an early return.
PATTERN_MATCH_COUNTS = """
use oswindow::app;

fn on_event(&mut self, event: &Event) -> Response {
    match event {
        Event::Key(KeyEvent { key, pressed: true, .. }) => self.act(*key),
        _ => Response::Idle,
    }
}

fn main() -> ExitCode { app::launch("game", &mut Game::new()) }
"""

# Passing the flag on to a handler counts: the handler is where the guard is,
# and it is reached because the caller read the field.
PASSING_IT_ON_COUNTS = """
use oswindow::app;

fn on_event(&mut self, event: &Event) -> Response {
    match event {
        Event::Key(ke) => self.handle_key(ke.key, ke.pressed),
        _ => Response::Idle,
    }
}

fn main() -> ExitCode { app::launch("game", &mut Game::new()) }
"""

# The fixture this gate exists for. A file that has *just* been fixed carries a
# comment naming the flag, and a test that builds the release event -- and both
# would vouch for a guard that a later edit removed. This source has the guard
# only in a comment and only in a test, and must still be reported.
COMMENT_AND_TEST_DO_NOT_COUNT = """
use oswindow::app;

fn on_event(&mut self, event: &Event) -> Response {
    // Presses only: a handler that ignores `pressed` runs every action twice.
    match event {
        Event::Key(ev) => self.handle_key(ev),
        _ => Response::Idle,
    }
}

fn main() -> ExitCode { app::launch("game", &mut Game::new()) }

#[cfg(test)]
mod tests {
    #[test]
    fn a_release_is_not_a_press() {
        let mut up = probe::press(Key::N);
        up.pressed = false;
        assert_eq!(handle_event(&mut app, &Event::Key(up)), EventResult::Ignored);
    }
}
"""

SELF_TESTS = [
    ("a guarded handler is not reported", GUARDED, None),
    ("an unguarded handler is reported", UNGUARDED, "ignores key releases"),
    ("a windowed app with no keys is not reported", LAUNCHES_BUT_TAKES_NO_KEYS, None),
    ("an unwired simulation is the other gate's finding", UNWIRED_SIMULATION, None),
    ("`pressed: true` in a pattern counts as reading it", PATTERN_MATCH_COUNTS, None),
    ("passing the flag to a handler counts", PASSING_IT_ON_COUNTS, None),
    (
        "a comment and a test do not vouch for a guard",
        COMMENT_AND_TEST_DO_NOT_COUNT,
        "ignores key releases",
    ),
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

    problems: list[str] = []
    guarded: list[str] = []
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        prod = production_only(text)
        if not LAUNCH_RE.search(prod) or not KEY_RE.search(prod):
            continue
        rel = path.relative_to(root).as_posix()
        if classify(text) is None:
            guarded.append(rel)
        else:
            problems.append(f"{rel}:{key_line(text)}")

    for where in problems:
        print(
            f"{where}: ignores key releases -- this program opens a window and "
            f"routes keys, but never reads `KeyEvent::pressed`, so every action "
            f"runs again when the key comes back up"
        )

    if verbose:
        print("\nguarded:")
        for rel in guarded:
            print(f"  {rel}")

    print(
        f"\n{len(guarded)} windowed key handler(s) read `pressed`, "
        f"{len(problems)} do not; baseline {BASELINE}"
    )
    if len(problems) > BASELINE:
        print(
            f"FAIL: {len(problems) - BASELINE} windowed program(s) treat a key "
            f"release as a second press. Guard on `KeyEvent::pressed`."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
