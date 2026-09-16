#!/usr/bin/env python3
"""Refuse a test that drives an event loop without pinning the config directory.

WHAT THIS IS ABOUT
------------------

`settingsfile::testing::with_scratch_config` points `XDG_CONFIG_HOME` at a
temporary directory so a test can write a settings file and watch it being
noticed. That variable belongs to the *process*, and `cargo test` runs a
binary's tests as threads of one process.

Meanwhile every test that drives an application's event loop is a **reader** of
that directory: the loop re-reads the appearance file once per turn. When a
writer and a reader interleave, the reader sees the directory change under it,
concludes the user just edited their settings, and repaints -- so a test that
counted frames counts one too many.

`settingsfile`'s `ENV_LOCK` serialises writers against each other and not
against readers. The cure is for the reader to take `config_turn()` too, which
is the same lock.

WHY A GATE AND NOT A COMMENT
----------------------------

This has now been diagnosed twice from scratch, eight days apart:

* `known-issues.md`
  `TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`
  -- `gui/window`, `left: 2, right: 1`, marked FIXED 2026-09-08.
* `TD-C-THE-CONFIG-DIRECTORY-RACE-WAS-CURED-IN-ONE-BINARY-AND-RECURS-IN-EVERY-OTHER`
  -- `apps/settings`, `left: 3, right: 2`, 2026-09-16.

The first fix works and does not carry: it is `TestDesktop` holding a
`ConfigTurn` behind `gui/window`'s own `#[cfg(test)]`, because the guard's type
comes from a dev-dependency and cannot appear in the shipped library. Every
*dependent's* test binary is therefore unguarded, and three of them
(`apps/editor`, `apps/match3`, `apps/pinball`) use the harness today without a
writer -- latent, and live the moment someone adds one.

The failure rate is the point. It did not reproduce once in ten consecutive
runs of the affected binary; it appeared in a run of four crates together,
where the extra load changed the interleaving. At that rate the honest
reaction to a red run is "re-run it", and the honest reaction is wrong. A
human rule ("remember the guard") has already been forgotten once by the
person who wrote the cure. This is the mechanical version.

THE RULE
--------

Per **crate**, because a process is a test binary and a test binary is built
from a crate:

    if any file in the crate mentions `with_scratch_config`,
    then every function body that calls `testing::desktop()`
    must also call `config_turn()`.

Crate-level rather than file-level because the race is between *threads of one
process*, and the writer need not live in the same file as the reader.

Conservative in one direction on purpose: a crate whose writer is in an
integration test (a separate binary from the lib tests) is still required to
guard its lib tests. Proving the two never share a process would mean modelling
cargo's target layout, and the cost of the guard is a mutex nobody contends.

WHAT IT DOES NOT CATCH
----------------------

A reader that reaches the event loop by some route other than
`testing::desktop()`. The call is the only handle this has on "this test drives
a loop"; a future harness with a different name is invisible to it and would
need adding to `HARNESS_CALLS`.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

# The call that means "this function drives an application event loop".
HARNESS_CALLS = ("testing::desktop()",)

# The call that puts a process-global environment variable in play.
WRITER_CALL = "with_scratch_config"

# The guard that takes the same lock the writer holds.
GUARD_CALL = "config_turn()"

# A run that finds fewer than this many harness callers has not searched the
# tree it thinks it has -- a moved directory or a bad filter -- and should say
# so rather than report a clean sweep of nothing. Set below the six known
# today so that deleting one file is not a build break.
FLOOR_HARNESS_FILES = 4


def tracked_rust_files(root: Path) -> list[Path]:
    """Every tracked `.rs` file, asked of git rather than the filesystem.

    `git ls-files` rather than `Path.rglob`, so a stray build directory or an
    untracked scratch copy cannot contribute a finding nobody can fix.
    """
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", "*.rs"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [root / line for line in out.stdout.splitlines() if line]


def crate_of(path: Path, root: Path) -> Path | None:
    """The directory of the nearest `Cargo.toml` at or above `path`."""
    for parent in path.parents:
        if (parent / "Cargo.toml").is_file():
            return parent
        if parent == root:
            break
    return None


def function_bodies(text: str) -> list[tuple[int, str]]:
    """Every `fn` body in `text`, as `(line_number, body)`.

    Brace-matched rather than regex'd, because a body contains braces and the
    interesting functions are the long ones. String and comment contents are
    skipped so that a `{` inside either cannot unbalance the count -- an
    assertion message with a brace in it is ordinary in this tree.
    """
    bodies: list[tuple[int, str]] = []
    i = 0
    n = len(text)
    while True:
        at = text.find("fn ", i)
        if at == -1:
            return bodies
        # Only a real item, not `.fn` in an identifier or the word inside one.
        if at > 0 and (text[at - 1].isalnum() or text[at - 1] in "_."):
            i = at + 3
            continue
        open_at = text.find("{", at)
        if open_at == -1:
            return bodies
        # A `;` before the brace means a declaration with no body (a trait
        # method), and the brace found belongs to something after it.
        if ";" in text[at:open_at]:
            i = at + 3
            continue
        depth = 0
        j = open_at
        while j < n:
            ch = text[j]
            if ch == '"':
                j += 1
                while j < n and text[j] != '"':
                    j += 2 if text[j] == "\\" else 1
            elif ch == "/" and j + 1 < n and text[j + 1] == "/":
                j = text.find("\n", j)
                if j == -1:
                    break
            elif ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    bodies.append((text.count("\n", 0, at) + 1, text[open_at : j + 1]))
                    break
            j += 1
        i = j + 1 if j > at else at + 3


def scan(root: Path) -> tuple[list[tuple[Path, int]], int, int]:
    """Find unguarded harness callers.

    Returns `(offenders, harness_file_count, writer_crate_count)`. An offender
    is a `(file, line)` naming a function that calls the harness without the
    guard, in a crate that also writes a scratch configuration.
    """
    files = tracked_rust_files(root)
    texts: dict[Path, str] = {}
    for path in files:
        try:
            texts[path] = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue

    writer_crates: set[Path] = set()
    harness_files: set[Path] = set()
    for path, text in texts.items():
        crate = crate_of(path, root)
        if crate is None:
            continue
        if WRITER_CALL in text:
            writer_crates.add(crate)
        if any(call in text for call in HARNESS_CALLS):
            harness_files.add(path)

    offenders: list[tuple[Path, int]] = []
    for path in sorted(harness_files):
        crate = crate_of(path, root)
        if crate is None or crate not in writer_crates:
            # No writer in this binary: nothing to race against today. The
            # known-issues entry calls these latent, and they are not findings.
            continue
        for line, body in function_bodies(texts[path]):
            if not any(call in body for call in HARNESS_CALLS):
                continue
            if GUARD_CALL in body:
                continue
            offenders.append((path.relative_to(root), line))
    return offenders, len(harness_files), len(writer_crates)


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

GUARDED = """
#[test]
fn a_guarded_test() {
    let _turn = settingsfile::testing::config_turn();
    let (mut events, desktop) = testing::desktop();
    assert_eq!(1, 1, "a message with a { brace in it");
}
"""

UNGUARDED = """
#[test]
fn an_unguarded_test() {
    let (mut events, desktop) = testing::desktop();
}
"""


def selftest() -> int:
    """Cases the rule has to get right, including the ones that bit."""
    cases: list[tuple[str, bool]] = []

    def case(name: str, ok: bool) -> None:
        cases.append((name, ok))
        print(f"{'ok  ' if ok else 'FAIL'}  {name}")

    bodies = function_bodies(GUARDED)
    case("a function body is found", len(bodies) == 1)
    case("...and the guard in it is seen", bodies and GUARD_CALL in bodies[0][1])
    case(
        "a brace inside a string does not end the body early",
        bodies and bodies[0][1].rstrip().endswith("}") and "brace in it" in bodies[0][1],
    )

    bodies = function_bodies(UNGUARDED)
    case("an unguarded body is found", len(bodies) == 1)
    case("...and no guard is seen in it", bodies and GUARD_CALL not in bodies[0][1])

    case(
        "a declaration with no body is not read as one",
        function_bodies("fn no_body(&self) -> bool;\nfn real() { let x = 1; }")
        == [(2, "{ let x = 1; }")],
    )
    case(
        "a line comment containing a brace does not unbalance",
        len(function_bodies("fn f() {\n    // a } here\n    let x = 1;\n}")) == 1,
    )
    case(
        "an identifier ending in fn is not an item",
        function_bodies("let myfn = 1; fn real() { }") == [(1, "{ }")],
    )

    failed = sum(1 for _, ok in cases if not ok)
    print(f"\nselftest: {len(cases) - failed}/{len(cases)} cases pass")
    return 1 if failed else 0


# Both spellings, because the neighbouring gates disagree about which one it
# is: `check-scratch-config.py` takes `--self-test` and this one was written
# with `--selftest`. Accepting only one means the other spelling falls through
# to the default action -- a real scan, which exits 0 -- so somebody who typed
# the remembered form would be told the self-test passed when nothing had run
# it. Caught by `check-selftest-reachability` on the push that added this file.
SELFTEST_FLAGS = ("--selftest", "--self-test")


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    if any(flag in sys.argv for flag in SELFTEST_FLAGS):
        return selftest()

    if selftest():
        print("check-config-turn-guards: SELFTEST FAILED -- not scanning", file=sys.stderr)
        return 2
    print()

    offenders, harness_files, writer_crates = scan(root)

    if harness_files < FLOOR_HARNESS_FILES:
        print(
            f"check-config-turn-guards: DECLINING -- found only {harness_files} file(s) "
            f"calling {HARNESS_CALLS[0]}, below the floor of {FLOOR_HARNESS_FILES}. "
            "A run that cannot find the harness has not searched the tree it thinks "
            "it has, and a clean report from it would be a clean report about nothing.",
            file=sys.stderr,
        )
        return 2

    if offenders:
        print("check-config-turn-guards: REFUSING", file=sys.stderr)
        for path, line in offenders:
            print(f"  {path}:{line}: drives an event loop without {GUARD_CALL}", file=sys.stderr)
        print(
            f"\nThis crate also calls {WRITER_CALL}, which repoints XDG_CONFIG_HOME for\n"
            "the whole process. `cargo test` runs a binary's tests as threads of one\n"
            "process, so the loop above can watch the configuration directory change\n"
            "under it, decide the user edited their settings, and repaint -- one frame\n"
            "more than the test counted. It reproduces at well under one run in ten,\n"
            "which is why re-running a red build is the wrong reaction to it.\n\n"
            "The fix is one line at the top of the test:\n\n"
            "    let _config_turn = settingsfile::testing::config_turn();\n\n"
            "See known-issues.md\n"
            "TD-C-THE-CONFIG-DIRECTORY-RACE-WAS-CURED-IN-ONE-BINARY-AND-RECURS-IN-EVERY-OTHER.",
            file=sys.stderr,
        )
        return 1

    print(
        f"check-config-turn-guards: OK -- {harness_files} file(s) drive an event loop, "
        f"{writer_crates} crate(s) write a scratch configuration, 0 unguarded."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
