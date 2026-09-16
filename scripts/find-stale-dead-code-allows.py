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

One build per crate rather than one per allow -- the compiler reports every
dead item at once, so stripping them all together is both faster and more
accurate than stripping one at a time (removing a single allow can leave a
caller that is itself dead, hiding the warning).

## What it cannot see, said plainly

  * **An allow on a whole `impl` block or module.** The warning names the
    item; matching it to a block-level allow is guesswork, so those are
    reported as `unmatched` rather than guessed at either way.
  * **A name guarded twice** -- two types with an allowed `fn all`, say. An
    absent warning could mean either is live, so those are reported and not
    judged.
  * **Items dead only under a different `--cfg`.** This builds one
    configuration. An allow covering a `cfg(unix)`-only item will look stale
    on a Windows host and is not.
  * **An item a test uses and nothing else** is correctly reported as still
    needing its allow, because the build here excludes tests. Whether it
    *should* exist at all is a different question, and one
    `find-stranded-serialisers.py` asks.

Every file is restored from the bytes read at the start and the restore is
verified by SHA-256.

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
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}")
        else:
            print(f"  ok    {label}")

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


def check_crate(crate: Path) -> int:
    name = crate.name
    files = sorted(crate.rglob("*.rs"))
    originals = {f: f.read_bytes() for f in files}
    digests = {f: hashlib.sha256(b).hexdigest() for f, b in originals.items()}

    guarded: dict[str, list[str]] = {}
    unmatched = 0
    try:
        for f in files:
            text = f.read_text(encoding="utf-8", errors="replace")
            lines = text.splitlines(keepends=True)
            for i, line in enumerate(lines):
                if ALLOW.match(line):
                    item = guarded_item(lines, i)
                    if item is None:
                        unmatched += 1
                    else:
                        guarded.setdefault(item, []).append(str(f.relative_to(ROOT)))
            stripped = ALLOW.sub("", text)
            if stripped != text:
                f.write_text(stripped, encoding="utf-8", newline="\n")

        if not guarded and not unmatched:
            return 0

        r = subprocess.run(
            ["cargo", "check", "-p", name, "--target", TARGET],
            capture_output=True, text=True, errors="replace", cwd=ROOT,
        )
        out = r.stdout + r.stderr
        if "error[E" in out or "error: could not compile" in out:
            print(f"--   {name}: did not compile without its allows; not evidence")
            return 0
        dead = set(NEVER_USED.findall(out))
    finally:
        for f, b in originals.items():
            f.write_bytes(b)
            assert hashlib.sha256(f.read_bytes()).hexdigest() == digests[f], (
                f"RESTORE FAILED for {f}"
            )

    # **A name guarded twice cannot be judged from the warning.** The
    # compiler names the item, not which of two `fn all` it meant, so an
    # absent warning could mean either one is live. Rare -- one crate in
    # `apps/` -- and reported rather than guessed, because a wrong "delete
    # this" is exactly the damage this checker exists to prevent elsewhere.
    ambiguous = sorted(k for k, v in guarded.items() if len(v) > 1)
    stale = sorted(k for k in guarded if k not in dead and k not in ambiguous)
    if ambiguous:
        print(f"??   {name}: {len(ambiguous)} name(s) guarded more than once, "
              f"not judged: {', '.join(ambiguous)}")
    if stale:
        print(f"!!   {name}: {len(stale)} allow(s) suppressing nothing")
        for item in stale:
            print(f"       {item}  ({guarded[item][0]})")
    else:
        print(f"ok   {name}: {len(guarded)} allow(s), all still needed")
    if unmatched:
        print(f"       ({unmatched} block-level allow(s) not matched to an item)")
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

    print(f"{len(crates)} crate(s) with at least one dead_code allow\n")
    total = sum(check_crate(c) for c in crates)
    print(f"\n{total} allow(s) suppressing nothing")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
