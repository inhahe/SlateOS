#!/usr/bin/env python3
"""Check that every mutation-harness row still matches the code it breaks.

A row in a crate's `mutate.py` names a piece of source (its *needle*) to
replace with a broken version. When the code moves -- a constant becomes a
palette field, a function gains an argument -- the needle matches nothing,
and the row tests nothing: the sweep reports it only when somebody runs
that row, which for a stable crate may be months. On 2026-09-28 lane E found
23 such rows across nine crates, some stale since 2026-09-08.

This reads every harness without running it: it imports each `mutate.py`
(every one keeps its sweep behind `if __name__ == "__main__"`), finds its
row tables -- any list of `(name, needle, replacement, [tests])` tuples --
and the Rust sources it names (module attributes that are paths to `.rs`
files: `SRC`, `STORE_SRC`, `LIB`, ...), and reports each row whose needle
does not occur exactly once in any of those sources. Seconds, not the hours
a sweep of everything takes, so it can be run before every publish.

usage: python scripts/check-mutation-needles.py [path ...]

With no paths, every `mutate.py` git knows of. Exit 0 when every row
matches, 1 when any does not, 2 when a harness could not be read.
"""

import importlib.util
import pathlib
import subprocess
import sys


def harnesses(args):
    """The `mutate.py` files to check."""
    if args:
        out = []
        for a in args:
            p = pathlib.Path(a)
            out.append(p / "mutate.py" if p.is_dir() else p)
        return out
    listed = subprocess.run(
        ["git", "ls-files", "*mutate.py"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()
    return [pathlib.Path(p) for p in listed]


def load(path):
    """Import a harness as a module, without running its sweep."""
    spec = importlib.util.spec_from_file_location(f"harness_{abs(hash(path))}", path)
    module = importlib.util.module_from_spec(spec)
    saved = sys.argv
    sys.argv = [str(path)]
    try:
        spec.loader.exec_module(module)
    finally:
        sys.argv = saved
    return module


def is_row(value):
    return (
        isinstance(value, tuple)
        and len(value) == 4
        and isinstance(value[0], str)
        and isinstance(value[1], str)
        and isinstance(value[2], str)
        and isinstance(value[3], (list, tuple))
    )


def tables(module):
    """Every list of rows the module defines."""
    return [
        (name, value)
        for name, value in vars(module).items()
        if isinstance(value, list) and value and all(is_row(v) for v in value)
    ]


def sources(module):
    """Every Rust source the module names: a path to a `.rs` file, or to a
    directory -- `SRC = .../src`, with the rows filed by name in a dict --
    for every `.rs` file beneath it."""
    found = []
    for value in vars(module).values():
        if not isinstance(value, pathlib.PurePath):
            continue
        path = pathlib.Path(value)
        if str(path).endswith(".rs"):
            found.append(path)
        elif path.is_dir():
            found.extend(sorted(path.rglob("*.rs")))
    return found


def main():
    bad = 0
    unreadable = 0
    checked = 0
    for path in harnesses(sys.argv[1:]):
        try:
            module = load(path)
        except Exception as e:  # noqa: BLE001 -- a harness that will not load is itself the finding
            print(f"{path}: could not be read: {e}")
            unreadable += 1
            continue
        srcs = sources(module)
        texts = {}
        for s in srcs:
            try:
                texts[s] = s.read_text(encoding="utf-8")
            except OSError as e:
                print(f"{path}: names {s}, which cannot be read: {e}")
                unreadable += 1
        if not srcs:
            print(f"{path}: names no Rust source")
            unreadable += 1
            continue
        for table, rows in tables(module):
            for name, needle, _new, _tests in rows:
                checked += 1
                counts = {s.name: t.count(needle) for s, t in texts.items()}
                if 1 in counts.values():
                    continue
                bad += 1
                where = ", ".join(f"{n} {c}x" for n, c in counts.items())
                print(f"{path} [{table}] {name!r}: {where}")
    print(f"{checked} row(s) checked; {bad} match nothing or too much; {unreadable} unreadable")
    if unreadable:
        return 2
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
