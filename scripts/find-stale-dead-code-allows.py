#!/usr/bin/env python3
r"""Which `#[allow(dead_code)]` attributes are suppressing nothing?

`check-dead-code-allows.py` asks whether an allow is **new** and whether it
carries a reason. Both are worth asking. Neither catches the other failure: an
allow that was right when written and stopped being right when somebody wired
the thing up.

`apps/kanban` had six. Four said `"import needs a file chooser"` and two said
`"export has nowhere to write yet"`, and by the time they were found the file
picker existed, `import_board` had a caller and `export_board` was reached
through `write_board`. The gate reported `0 new` throughout, correctly: **a
stale allow is not a new one, and a checker built to catch additions cannot
see survivals.**

That matters more than tidiness. A `reason` string is what somebody greps to
size a piece of work, and the reasons in that file described a feature as
half-built for as long as they sat there. One of them nearly cost an afternoon
rebuilding a JSON parser that already existed.

## How it decides

For one crate: strip every `#[allow(dead_code, …)]` and `#[expect(dead_code,
…)]`, build, and read which items the compiler then reports as never used. An
allow whose item is **not** in that list was suppressing nothing.

**Without `--all-targets`, deliberately**, and this was wrong in the first
version. `dead_code` is about the shipped binary. `--all-targets` compiles the
tests too, so an item used only by tests is not dead *in that build* -- and its
allow, which is genuinely needed for the ordinary build, would have been
reported as suppressing nothing. In a tree with as many test-only helpers as
this one that is not an edge case; it is most of the output. The plain build is
the configuration the lint is about.

**The rule is: does removing this allow change what the compiler says?** Take
the crate's warnings with every allow in place, then remove one allow and take
them again. If the two sets are identical, that allow suppressed nothing.

**This is the third rule tried, and the first two were wrong in ways worth
recording**, because both sounded right.

*Strip them all and take one build.* The reasoning was that the compiler
reports every dead item at once. It does not: `dead_code` names the **roots**
of a dead subgraph, so stripping everything lets an item become reachable
*from another dead item* and go unmentioned. On `apps/credmanager` this called
twelve allows stale; removing `toggle_star`'s alone produced exactly `method
toggle_star is never used`. **Wrong about three-quarters of its own output, in
the direction of telling someone to delete working code.**

*Strip one and look for that item's name.* Closer, and still wrong. Removing
`remove_folder`'s allow produced a warning about **`set_folder`** -- a
different item. An allow's removal has effects past the item it sits on,
because reachability is a graph and an allowed item is still dead for the
purpose of what it keeps alive. Asking "was my item named?" answers a
question about one node; asking "did anything change?" answers the question
that was meant.

The third rule needs no mapping from an allow to an item at all, which is
also why it has no trouble with block-level allows or with two types sharing a
method name. It costs one build per allow plus one baseline.

Each result was found by checking a single reported row by hand before
believing the total. That cost one build each time and would have cost an
afternoon to act on.

## What it cannot see, said plainly

  * **Nothing about block-level allows or repeated names**, which the first
    two rules both stumbled on. Comparing warning sets asks about the allow
    rather than about an item, so an allow over an `impl` block and two types
    sharing a `fn all` are both ordinary cases.
  * **Items dead only under a different `--cfg`.** This builds one
    configuration. An allow covering a `cfg(unix)`-only item will look stale
    on a Windows host and is not.
  * **An item a test uses and nothing else** is correctly reported as still
    needing its allow, because the build here excludes tests. Whether it
    *should* exist at all is a different question, and one
    `find-stranded-serialisers.py` asks.

Every file is restored from the bytes read at the start and the restore is
verified by SHA-256.

**It edits the working tree, so do not run it beside another build.** A file
is modified for the length of each `cargo check`, one per allow, and a workspace
gate running at the same time would compile them -- reporting dead-code
warnings that belong to this tool's scratch state, or worse, a green result
about a tree that existed for four seconds. The same race cost a misleading
PASS earlier in this tree's history when a gate started before two commits and
appeared to cover them.

Usage:  python scripts/find-stale-dead-code-allows.py --crate=apps/kanban
        python scripts/find-stale-dead-code-allows.py --roots=apps,gui
        python scripts/find-stale-dead-code-allows.py --self-test
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import selftestflag  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

ALLOW = re.compile(
    r"^[ \t]*#\[(?:allow|expect)\(\s*dead_code(?:[^)]*)\)\]\s*\n", re.M
)
# `fn foo`, `struct Foo`, `const FOO`, and so on -- the item an attribute is
# attached to, which is what the compiler names in its warning.
ITEM = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|unsafe\s+|const\s+|extern\s+\"[^\"]*\"\s+)*"
    r"(fn|struct|enum|const|static|type|trait|union|mod)\s+([A-Za-z_][A-Za-z0-9_]*)"
)
NEVER_USED = re.compile(r"(?:field|method|function|struct|constant|enum|variant|associated function|type alias)\s+`([^`]+)`\s+is never (?:used|read|constructed)")


def guarded_item(lines: list[str], idx: int) -> str | None:
    """The name of the item the allow at `lines[idx]` guards, if it names one.

    Attributes and doc comments may sit between the allow and the item, so this
    walks forward past them. A block-level allow -- one whose next item is an
    `impl` -- returns None and is reported as unmatched rather than guessed.
    """
    for line in lines[idx + 1:]:
        stripped = line.strip()
        if not stripped or stripped.startswith(("#[", "///", "//", "#!")):
            continue
        m = ITEM.match(line)
        return m.group(2) if m else None
    return None


def _self_test() -> int:
    failures = 0

    def expect(label, got, want):
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}", flush=True)
        else:
            print(f"  ok    {label}", flush=True)

    src = [
        '#[allow(dead_code, reason = "x")]\n',
        "struct JsonImporter;\n",
    ]
    expect("a struct right after the allow", guarded_item(src, 0), "JsonImporter")

    src = [
        '    #[allow(dead_code, reason = "x")]\n',
        "    /// Doc between the two.\n",
        "    // And a plain comment.\n",
        "    fn parse_string(data: &str) -> u8 {\n",
    ]
    expect("doc comments between allow and item", guarded_item(src, 0), "parse_string")

    src = ['#[allow(dead_code)]\n', "impl Thing {\n"]
    expect("a block-level allow names no item", guarded_item(src, 0), None)

    src = ['#[allow(dead_code)]\n', "pub const FOO: u8 = 1;\n"]
    expect("pub const", guarded_item(src, 0), "FOO")

    src = ['#[allow(dead_code)]\n', '    pub unsafe extern "C" fn go() {}\n']
    expect("an extern fn with modifiers", guarded_item(src, 0), "go")

    expect("the attribute matches with a reason",
           bool(ALLOW.search('  #[allow(dead_code, reason = "x")]\n')), True)
    expect("...and without one", bool(ALLOW.search("#[allow(dead_code)]\n")), True)
    expect("...and as expect()", bool(ALLOW.search("#[expect(dead_code)]\n")), True)
    expect("an unrelated allow is left alone",
           bool(ALLOW.search("#[allow(unused_variables)]\n")), False)

    out = "warning: method `export_json` is never used\nwarning: field `raw` is never read\n"
    expect("both warning shapes are read",
           set(NEVER_USED.findall(out)), {"export_json", "raw"})

    print(f"find-stale-dead-code-allows: self-test "
          f"{'passed' if not failures else 'FAILED'} ({failures} failure(s))")
    return 1 if failures else 0


def warnings_of(name: str) -> tuple[set[str], bool]:
    """The crate's dead-code warnings, and whether the build failed.

    A set of whole warning lines rather than item names: the rule compares two
    of these, so what matters is that the same warning text means the same
    thing to both sides.
    """
    r = subprocess.run(
        ["cargo", "check", "-p", name, "--target", TARGET],
        capture_output=True, text=True, errors="replace", cwd=ROOT,
    )
    out = r.stdout + r.stderr
    if "error[E" in out or "error: could not compile" in out:
        return set(), True
    return {ln.strip() for ln in out.splitlines() if NEVER_USED.search(ln)}, False


def check_crate(crate: Path) -> int:
    name = crate.name
    files = sorted(crate.rglob("*.rs"))
    originals = {f: f.read_bytes() for f in files}
    digests = {f: hashlib.sha256(b).hexdigest() for f, b in originals.items()}

    sites: list[tuple[Path, str, int]] = []
    for f in files:
        lines = originals[f].decode("utf-8", errors="replace").splitlines(keepends=True)
        for i, line in enumerate(lines):
            if ALLOW.match(line):
                sites.append((f, line, i + 1))
    if not sites:
        return 0

    stale: list[str] = []
    skipped = 0
    try:
        base, broke = warnings_of(name)
        if broke:
            print(f"--   {name}: does not compile as it stands; not evidence", flush=True)
            return 0

        for path, line_text, lineno in sites:
            text = originals[path].decode("utf-8", errors="replace")
            # By line number, not by text. One reason string is often shared by
            # several items -- `credmanager` reuses one across nine -- and
            # cutting by text would remove the first every time: that one
            # measured nine times, the other eight never.
            lines = text.splitlines(keepends=True)
            cut = "".join(lines[: lineno - 1] + lines[lineno:])
            path.write_text(cut, encoding="utf-8", newline="\n")
            after, broke_one = warnings_of(name)
            path.write_bytes(originals[path])
            if broke_one:
                skipped += 1
                continue
            if after == base:
                rel = path.relative_to(ROOT)
                stale.append(f"{rel}:{lineno}  {line_text.strip()}")
    finally:
        for f, b in originals.items():
            f.write_bytes(b)
            assert hashlib.sha256(f.read_bytes()).hexdigest() == digests[f], (
                f"RESTORE FAILED for {f}"
            )

    if stale:
        print(f"!!   {name}: {len(stale)} of {len(sites)} allow(s) suppressing nothing", flush=True)
        for row in stale:
            print(f"       {row}", flush=True)
    else:
        print(f"ok   {name}: {len(sites)} allow(s), all still needed", flush=True)
    if skipped:
        print(f"       ({skipped} not measured: a duplicate line, or the strip "
              f"did not compile)")
    return len(stale)


def main() -> int:
    if selftestflag.wants_selftest(sys.argv[1:]):
        return _self_test()

    ap = argparse.ArgumentParser()
    ap.add_argument("--crate", help="one crate directory, e.g. apps/kanban")
    ap.add_argument("--roots", default="apps,gui")
    ap.add_argument("--self-test", "--selftest", "--self_test",
                    dest="self_test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return _self_test()

    if args.crate:
        crates = [ROOT / args.crate]
    else:
        crates = []
        for r in args.roots.split(","):
            root = ROOT / r
            if not root.is_dir():
                continue
            crates += [c for c in sorted(root.iterdir())
                       if (c / "Cargo.toml").is_file()
                       and any("allow(dead_code" in f.read_text(encoding="utf-8",
                                                               errors="replace")
                               for f in c.rglob("*.rs"))]
    if not crates:
        print("no crate with a dead_code allow -- refusing to call that a pass",
              file=sys.stderr)
        return 2

    print(f"{len(crates)} crate(s) with at least one dead_code allow\n", flush=True)
    total = sum(check_crate(c) for c in crates)
    print(f"\n{total} allow(s) suppressing nothing", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
