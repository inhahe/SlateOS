"""Check `mutate.py` tables against the source they claim to break.

A sweep costs half an hour.  Every row whose `old_string` no longer appears in
the file -- because the production code was rewritten under it -- is a `[skip]`
at best and a silent hole at worst, and the sweep is the most expensive place to
find that out.  This reads the tables without running them and reports:

* anchors that match zero times (dead) or more than once (ambiguous);
* expected test names that no longer exist in the source;
* duplicate row names within a table, which make `only=` filtering ambiguous.

A harness is imported, never run -- every one keeps its sweep behind
`if __name__ == "__main__"` -- and read the way it reads itself: every list of
`(name, old, new, [tests])` rows it defines (`MUTATIONS`, or one table per file:
`LIB_MUTATIONS`, `TABLES = {"shelf.rs": SHELF, ...}`), against every Rust source
it names (`SRC` as a file or as a `src` directory, `STORE_SRC`, `LIB`, ...).  A
row's anchor must occur exactly once in the file its table sweeps, where the
harness says which that is -- a `TABLES` dict of file name to table, or a table
`X_MUTATIONS` beside a path `X_SRC` (`MUTATIONS` beside `SRC`) -- and otherwise
exactly once in one of them.  Once in another table's file is not good enough:
that row finds no anchor when its own table is swept, and runs nothing
(credmanager's and email's keys rows, 2026-10-01).

With no paths, every `mutate.py` git knows of: seconds for the whole tree, so
it can run before every publish.  On 2026-09-28 it found 23 dead rows across
nine of lane E's crates, some dead since 2026-09-08, when the colours moved to
the palette and no one swept those crates again.

Usage:  python scripts/verify_mutations.py [apps/<app>/mutate.py | apps/<app> ...]
Exit 0 when every row is sound, 1 when any is not, 2 when a harness cannot be read.
"""

import importlib.util
import re
import subprocess
import sys
from pathlib import Path, PurePath


def harnesses(args):
    """The `mutate.py` files to check: those named, or every one git knows of."""
    if args:
        return [Path(a) / "mutate.py" if Path(a).is_dir() else Path(a) for a in args]
    listed = subprocess.run(
        ["git", "ls-files", "*mutate.py"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()
    return [Path(p) for p in listed]


def load(path):
    """Import a harness as a module without running its sweep."""
    spec = importlib.util.spec_from_file_location(f"harness_{abs(hash(str(path)))}", path)
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
        and all(isinstance(v, str) for v in value[:3])
        and isinstance(value[3], (list, tuple))
    )


def tables(module):
    """Every list of rows the harness defines, by the name it goes by."""
    return [
        (name, value)
        for name, value in vars(module).items()
        if isinstance(value, list) and value and all(is_row(v) for v in value)
    ]


def sources(module):
    """Every Rust source the harness names: a `.rs` path, or a directory's
    every `.rs` file beneath it."""
    found = []
    for value in vars(module).values():
        if not isinstance(value, PurePath):
            continue
        path = Path(value)
        if str(path).endswith(".rs"):
            found.append(path)
        elif path.is_dir():
            found.extend(sorted(path.rglob("*.rs")))
    return found


def table_files(module, texts):
    """The source each table sweeps, by the table's identity, where the
    harness says: a dict of `.rs` file names to tables (`TABLES =
    {"main.rs": MAIN, ...}`, the names taken under a directory the harness
    names), or -- in a harness of several tables -- a table `X_MUTATIONS`
    beside a path `X_SRC` (`MUTATIONS` beside `SRC`) that is a file.  A
    table neither names is absent.

    A harness of one table may send each row wherever its anchor is:
    calendar's runs a row against its own `main.rs` or the store's `lib.rs`
    by which holds the anchor, so its `SRC` is not every row's file."""
    found = {}
    names = vars(module)
    several = len(tables(module)) > 1
    dirs = [Path(v) for v in names.values() if isinstance(v, PurePath) and Path(v).is_dir()]
    for value in names.values():
        if not (isinstance(value, dict) and value and all(isinstance(k, str) for k in value)):
            continue
        for file_name, rows in value.items():
            if not isinstance(rows, list):
                continue
            under = [d / file_name for d in dirs if (d / file_name) in texts]
            if len(under) == 1:
                found[id(rows)] = under[0]
    for name, value in names.items():
        if not several or not isinstance(value, list) or id(value) in found:
            continue
        if name == "MUTATIONS":
            src = names.get("SRC")
        elif name.endswith("_MUTATIONS"):
            src = names.get(name[: -len("MUTATIONS")] + "SRC")
        else:
            continue
        if isinstance(src, PurePath) and Path(src) in texts:
            found[id(value)] = Path(src)
    return found


def check(path):
    """(rows, problems) for one harness; problems printed as found."""
    module = load(path)
    texts = {}
    for src in sources(module):
        texts[src] = src.read_text(encoding="utf-8", errors="replace", newline="")
    if not texts:
        raise ValueError("names no Rust source")
    own_file = table_files(module, texts)
    # A named test may live anywhere a sweep's `cargo test` runs it: in the
    # sources the harness breaks, or in its crate's other files and `tests/`.
    tests = set()
    crate_files = sorted(Path(path).resolve().parent.rglob("*.rs"))
    for text in [*texts.values(), *(f.read_text(encoding="utf-8", errors="replace") for f in crate_files)]:
        tests.update(re.findall(r"\bfn ([a-z_0-9]+)\(\)", text))

    rows = 0
    bad = 0
    for table, entries in tables(module):
        seen = set()
        own = own_file.get(id(entries))
        for name, old, _new, expect in entries:
            rows += 1
            if name in seen:
                print(f"{path} [{table}] DUPLICATE ROW NAME: {name}")
                bad += 1
            seen.add(name)
            # Where the sweep will look: the table's own file when the
            # harness says which, else every source it names.
            looked = {own: texts[own]} if own is not None else texts
            counts = [
                text.count(old.replace("\n", "\r\n") if "\r\n" in text else old)
                for text in looked.values()
            ]
            if 1 not in counts:
                where = ", ".join(f"{s.name} x{c}" for s, c in zip(looked, counts))
                print(f"{path} [{table}] ANCHOR {where}: {name}")
                bad += 1
            for t in expect:
                if t not in tests:
                    print(f"{path} [{table}] NO SUCH TEST {t}: {name}")
                    bad += 1
    return rows, bad


def main(argv):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    total_rows = 0
    total_bad = 0
    unreadable = 0
    for path in harnesses(argv[1:]):
        try:
            rows, bad = check(path)
        except Exception as e:  # noqa: BLE001 -- a harness that will not read is itself the finding
            print(f"{path}: cannot be read: {e}")
            unreadable += 1
            continue
        total_rows += rows
        total_bad += bad
    print(f"{total_rows} rows, {total_bad} problems, {unreadable} unreadable")
    if unreadable:
        return 2
    return 1 if total_bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
