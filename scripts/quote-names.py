#!/usr/bin/env python3
"""Diagnostics that name a file must route the name through `quote`.

`userspace/coreutils/tests/diagnostics_quote_names.rs` already says this, and
enforces it — over exactly one directory, `coreutils/src/bin`. Every other
utility crate in the tree is outside it, including all 41 that duplicate a
coreutils name. So `userspace/coreutils/src/bin/du.rs` is checked and
`userspace/du/src/main.rs` is not, and the two are the same program.

The defect being guarded is that a diagnostic written the natural way,

    eprintln!("cut: {path}: {e}");

compiles, reads correctly, and hands the *name* control of the error stream:
a file called `x\\ncut: /etc/shadow: Permission denied` makes `cut` appear to
have written a second line it never wrote. `quotef_os`/`quoteaf_os` render the
name unambiguously; nothing else does.

This script is that test's two detectors, run over the whole of lane B's tree
rather than one directory, as a **ratchet**: the 1796 sites that exist today
are recorded in `quote-names-baseline.txt`, and `--check` fails only on a
*new* one. A backlog that cannot grow is a different thing from a backlog.

    python scripts/quote-names.py                  # per-crate report
    python scripts/quote-names.py --list           # ... with every line
    python scripts/quote-names.py --check          # ratchet: new violations only
    python scripts/quote-names.py --selftest       # check the detectors
    python scripts/quote-names.py --write-baseline # re-record after a burn-down

## Why the baseline counts violations per file rather than naming them

Three keys were possible, and the choice is a real tradeoff:

* `path:line` — exact, and useless: every edit above a site renumbers it, so
  the baseline would go stale on commits that touch nothing relevant.
* `path:<source text>` — exact and stable under line movement, but *not*
  under `rustfmt`, which rewraps argument lists routinely. A gate that fires
  on reformatting is a gate that gets bypassed, and a bypassed gate protects
  nothing.
* `path:<count>` — what is used here. Immune to both, and precise enough:
  adding a site to an already-listed file raises its count and fails the
  check, which is the case that actually happens.

The residual gap is a 1-for-1 swap inside one file — remove one violation and
add another in the same commit, and the count is unchanged. That shape is rare
enough to be worth the two failure modes it avoids, and the *Rust* test still
covers `coreutils/src/bin` exactly, where most of the traffic is.

Per-file (not per-crate) because the unit of repair is a call site and the
unit of review is a file; a crate-level count would hide a new violation in
`btrfs`'s 46 behind any one of them being fixed.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "quote-names-baseline.txt"

# The trees lane B owns. `apps/` and `gui/` are lane C's and are deliberately
# outside: a gate that fails another lane's commit for another lane's code is
# a gate that lane turns off.
ROOTS = ("userspace", "services", "init", "posix")

# Kept byte-for-byte in step with NOT_A_NAME in diagnostics_quote_names.rs.
NOT_A_NAME = {"msg", "e", "err", "error", "message", "reason"}

# Files whose hits are not defects, with the reason. This table records *why*
# a file is exempt; the baseline records only *that* a site exists, which is
# the wrong place for a judgement. Keep it short -- every entry is a hole.
IGNORE = {
    # The bad form appears here as the detector's own fixtures: the test feeds
    # `eprintln!("cp: {path}: {e}")` to `bare_interpolated_name` and asserts it
    # is caught. Baselining them would be worse than a false positive -- they
    # can never be "fixed", so the count could never reach zero, and a ratchet
    # with an unreachable floor stops being read.
    "userspace/coreutils/tests/diagnostics_quote_names.rs": "the detector's own fixtures",
}


def bare_interpolated_name(line: str) -> str | None:
    """Port of `bare_interpolated_name` in diagnostics_quote_names.rs.

    Matches `eprintln!("prog: {ident}: ...` — the shape where `ident` reaches
    the message as a bare name.
    """
    head, sep, after = line.partition('eprintln!("')
    if not sep:
        return None
    prog, sep, tail = after.partition(": {")
    if not sep:
        return None
    if not prog or not all(c.islower() or c.isdigit() or c == "_" for c in prog):
        return None
    ident, sep, rest = tail.partition("}")
    if not sep:
        return None
    if not ident or not all(c.isalnum() or c == "_" for c in ident):
        return None
    if not rest.startswith(": "):
        return None
    if ident in NOT_A_NAME:
        return None
    return ident


def hand_written_quotes(line: str) -> bool:
    """Port of the `no_diagnostic_hand_writes_quotes_around_a_name` detector.

    `'{path}'` is worse than `{path}`, not better: it *looks* quoted, so it
    survives review, while a name containing a `'` still breaks out.
    """
    if "eprintln!(" not in line and "println!(" not in line:
        return False
    return "'{" in line and "}'" in line


def is_prose(line: str) -> bool:
    """A comment, not code.

    The Rust test needs no such filter because it reads only
    `coreutils/src/bin`, where no comment happens to contain the pattern. A
    tree-wide scan does — this script's own docstring would otherwise flag
    itself, and so do several doc-comments that quote the bad form in order to
    warn about it.
    """
    return line.lstrip().startswith("//")


def violations(text: str) -> list[tuple[int, str, str]]:
    """`(line number, what, source line)` for every flagged line in `text`."""
    out: list[tuple[int, str, str]] = []
    for i, line in enumerate(text.splitlines(), 1):
        if is_prose(line):
            continue
        ident = bare_interpolated_name(line)
        if ident is not None:
            out.append((i, f"{{{ident}}} unquoted", line.strip()))
        elif hand_written_quotes(line):
            out.append((i, "hand-written quotes", line.strip()))
    return out


def survey(root: Path = ROOT) -> dict[str, list[tuple[int, str, str]]]:
    """Every flagged line in lane B's tree, keyed by repo-relative path."""
    found: dict[str, list[tuple[int, str, str]]] = {}
    for top in ROOTS:
        base = root / top
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if "target" in path.parts:
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError):
                continue
            rel = path.relative_to(root).as_posix()
            if rel in IGNORE:
                continue
            hits = violations(text)
            if hits:
                found[rel] = hits
    return found


def read_baseline() -> dict[str, int]:
    """`path -> count` from the baseline file, `#` comments stripped."""
    if not BASELINE.is_file():
        return {}
    out: dict[str, int] = {}
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        path, _, count = line.rpartition(":")
        if not path or not count.isdigit():
            print(f"malformed baseline line: {line!r}", file=sys.stderr)
            continue
        out[path] = int(count)
    return out


def write_baseline(found: dict[str, list[tuple[int, str, str]]]) -> None:
    body = [
        "# Diagnostics that put a file name into the message without routing it",
        "# through `quote` -- one line per source file, with the number of sites",
        "# in it. Generated by `scripts/quote-names.py --write-baseline`; see that",
        "# script's docstring for why the key is a count and not a line number.",
        "#",
        "# THIS FILE IS A RATCHET AND ONLY EVER SHRINKS. Each site is a place",
        "# where a hostile file name can forge a line of this program's stderr.",
        "# The fix is one call: `quotef_os(path)` (or `quoteaf_os` where the name",
        "# is always quoted) from the crate's `quoting` module.",
        "#",
        "# Do NOT raise a number, and do NOT add a file, to turn a red `--check`",
        "# green: that is the defect being recorded, not an exception to it.",
        "#",
        f"# {sum(len(v) for v in found.values())} sites across {len(found)} files.",
        "",
    ]
    body += [f"{path}:{len(hits)}" for path, hits in sorted(found.items())]
    # newline="" stops Python translating "\n" to "\r\n" on Windows. Git
    # normalises it on commit either way, so without this the file on disk
    # differs from the file in the index and every checkout shows it dirty.
    BASELINE.write_text("\n".join(body) + "\n", encoding="utf-8", newline="")
    total = sum(len(v) for v in found.values())
    print(f"wrote {BASELINE.name} with {len(found)} files, {total} sites")


def selftest() -> int:
    """Check the rule that decides what this tool reports.

    A detector that fails toward silence looks exactly like a clean tree, and
    this one is unusually exposed to that: its signal lives *inside* a string
    literal, so the obvious lexer to reach for blanks out precisely the text
    being searched. The cases below are the ones that a rewrite would break
    first -- and each of them is a shape that exists in the tree today.
    """
    failures: list[str] = []
    checked = 0

    def expect(label: str, src: str, want: int) -> None:
        nonlocal checked
        checked += 1
        got = len(violations(src))
        if got != want:
            failures.append(f"{label}: want {want}, got {got}\n    {src}")

    # 1. The base case, in the exact shape the recorded sites are written in.
    expect("bare", 'eprintln!("cut: {path}: {e}");', 1)
    expect("bare-nested-prog", 'eprintln!("tar_x: {name}: {e}");', 1)
    expect("digit-in-prog", 'eprintln!("b2sum: {f}: {e}");', 1)

    # 2. The fix must not match, or the tool flags its own remedy.
    expect("quoted", 'eprintln!("cut: {}: {e}", quotef_os(path));', 0)
    expect("quoted-always", 'eprintln!("cut: {}: {e}", quoteaf_os(path));', 0)

    # 3. A message is not a name: re-quoting rendered text would be wrong, and
    #    flagging it would bury the real hits in noise.
    for ident in sorted(NOT_A_NAME):
        expect(f"not-a-name-{ident}", f'eprintln!("cut: {{{ident}}}: rest");', 0)

    # 4. Shapes that look like the pattern but are not it. Each of these
    #    over-matching would put a false positive in a 1796-line baseline,
    #    where nobody would ever find it again.
    expect("no-trailing-colon", 'eprintln!("cut: {path} is a directory");', 0)
    expect("uppercase-prog", 'eprintln!("Cut: {path}: {e}");', 0)
    expect("empty-prog", 'eprintln!(": {path}: {e}");', 0)
    expect("not-an-ident", 'eprintln!("cut: {path.display()}: {e}");', 0)
    expect("stdout-not-stderr", 'println!("cut: {path}: {e}");', 0)

    # 5. Hand-written quotes: the shape that looks fixed and is not.
    expect("hand-quotes", "eprintln!(\"cut: '{path}': {e}\");", 1)
    expect("hand-quotes-stdout", "println!(\"cut: '{path}'\");", 1)
    expect("hand-quotes-no-macro", "let s = format!(\"'{path}'\");", 0)

    # 6. Prose is not code. Seven doc-comments in this tree quote the bad form
    #    in order to warn about it; counting them would overstate the backlog
    #    and, worse, make the backlog unfixable -- you cannot repair a comment.
    expect("line-comment", '// eprintln!("cut: {path}: {e}");', 0)
    expect("doc-comment", '/// eprintln!("cut: {path}: {e}");', 0)
    expect("module-doc", '//! eprintln!("cut: {path}: {e}");', 0)

    # 7. Multi-line input, since that is what a file is.
    expect(
        "two-in-one-file",
        'eprintln!("cut: {path}: {e}");\nlet x = 1;\neprintln!("cut: {name}: {e}");',
        2,
    )

    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {checked - len(failures)}/{checked} cases pass")
    return 1 if failures else 0


def report(found: dict[str, list[tuple[int, str, str]]], show_lines: bool) -> int:
    per_crate: dict[str, int] = {}
    for path, hits in found.items():
        parts = path.split("/")
        crate = "/".join(parts[:2]) if len(parts) > 1 else path
        per_crate[crate] = per_crate.get(crate, 0) + len(hits)
    total = sum(len(v) for v in found.values())
    print(f"{total} violations in {len(found)} files, {len(per_crate)} crates\n")
    for crate, n in sorted(per_crate.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"{n:5}  {crate}")
    if show_lines:
        print()
        for path, hits in sorted(found.items()):
            for line_no, what, src in hits:
                print(f"  {path}:{line_no}: {what}\n      {src}")
    return 0


def check(found: dict[str, list[tuple[int, str, str]]]) -> int:
    baseline = read_baseline()
    now = {path: len(hits) for path, hits in found.items()}

    grew = sorted(p for p, n in now.items() if n > baseline.get(p, 0))
    shrank = sorted(p for p, n in baseline.items() if now.get(p, 0) < n)

    for path in shrank:
        was, is_now = baseline[path], now.get(path, 0)
        print(f"fixed: {path} {was} -> {is_now} -- run --write-baseline to record it")

    if not grew:
        total = sum(now.values())
        print(f"ok -- {total} known sites in {len(now)} files ({len(shrank)} improved)")
        return 0

    new_sites = sum(now[p] - baseline.get(p, 0) for p in grew)
    print(
        f"\n{new_sites} NEW diagnostic(s) put a file name into a message without\n"
        "routing it through `quote`. A name containing a newline can then forge a\n"
        "line of this program's stderr:\n\n"
        "    $ touch $'x\\ncut: /etc/shadow: Permission denied'\n\n"
        "and the second line is indistinguishable from one `cut` really wrote.\n",
        file=sys.stderr,
    )
    for path in grew:
        was, is_now = baseline.get(path, 0), now[path]
        where = f"{path}  ({was} known -> {is_now} now)" if was else f"{path}  (new file)"
        print(f"\n  {where}", file=sys.stderr)
        for line_no, what, src in found[path]:
            print(f"    :{line_no}: {what}", file=sys.stderr)
            print(f"        {src}", file=sys.stderr)
    print(
        "\nThe fix is one call, not a baseline entry:\n"
        '    eprintln!("cut: {}: {e}", quotef_os(path));\n'
        "`quotef_os` quotes only when the name needs it; `quoteaf_os` always does.\n"
        "Raising a number in scripts/quote-names-baseline.txt records the defect\n"
        "instead of fixing it, which is the one thing that file must never hold.",
        file=sys.stderr,
    )
    return 1


def main() -> int:
    args = sys.argv[1:]
    if "--selftest" in args:
        return selftest()
    found = survey()
    if "--write-baseline" in args or "--update-baseline" in args:
        write_baseline(found)
        return 0
    if "--check" in args:
        return check(found)
    return report(found, "--list" in args)


if __name__ == "__main__":
    sys.exit(main())
