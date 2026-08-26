#!/usr/bin/env python3
"""Stop `apps/diskcleanup`'s own tests from pointing the deleter at the host.

`apps/diskcleanup` deletes files. Not "will one day delete files" -- since the
back end was made real it calls `fs::remove_file` and `fs::remove_dir_all` on
every path in the plan, and `CleanupUI` will do that from a test the moment a
confirmation dialog test presses Enter.

The production default scan root is:

    const DEFAULT_SCAN_ROOTS: [&str; 1] = ["/"];

which is correct on SlateOS and catastrophic here. `cargo test` runs on the
Windows dev host, where `"/"` resolves to `C:\\` and `"/tmp"` to `C:\\tmp` --
a directory that on the machine this gate was written on held about 4.5 GB of
the operator's files. That is not a hypothetical: a scan rooted there was
committed, ran for **152 seconds** measuring the operator's scratch tree, and
the only reason nothing was erased is that the paths those tests injected did
not happen to exist. The next such test would not have been so lucky, because
a scanned directory is also an *allowed* one -- `CleanupPlan::permits` grants
deletion inside exactly the directories the scan enumerated, so pointing a
test scan at `C:\\tmp` hands the executor a licence to empty it.

The fix in the code is `ScratchDir`: every test that touches a disk builds its
own tree under `std::env::temp_dir()` and scans that. This gate exists because
that is a *convention*, and a convention is a comment with better posture. The
comment above `scanned_over` says the rule; nothing in the toolchain says it
back. `cargo test` on a green tree that has just erased `C:\\tmp` exits 0.

## What it flags

Inside `#[cfg(test)]` code under `apps/diskcleanup` only:

1.  Any mention of `DEFAULT_SCAN_ROOTS`. It *is* `["/"]`; there is no safe way
    for a test to use it, and no reason to -- the constant's own value is what
    production is for.
2.  A **root-anchored path literal** -- `"/"`, `"/tmp/..."`, `"C:\\..."`,
    `"\\\\server\\..."` -- appearing in the argument list of a call that walks
    or deletes: `run_scan`, `scan`, `allow_root`, `execute`, `dry_run`,
    `measure_recursive`, `enumerate_entries`, `remove`, `set_scan_roots`.

Note what is deliberately *not* flagged: a root-anchored literal on its own.
`CleanupItem::new("/tmp/foo", Temp)` is inert -- it builds a struct, and the
confinement list means the executor refuses that path unless some scan
enumerated its parent. Flagging it would make the gate argue with tests that
are provably harmless, and a gate that cries wolf is a gate that gets a
`# noqa` culture. The dangerous act is not naming a path; it is handing one to
something that reads or unlinks it.

That it is a gate and not a decoration is measured, not assumed. Run against
`apps/diskcleanup/src/main.rs` as it stood one commit before the `ScratchDir`
rewrite, it names **eight** sites -- the two scans rooted at `"/"` and the six
uses of `DEFAULT_SCAN_ROOTS` -- and against the fixed file, none. Those eight
are the near-miss, in full and by line number.

Unlike `check-window-wiring.py` there is no `BASELINE` ratchet here. That
idiom exists for a population of pre-existing findings too large to fix in one
commit, where a gate that failed on day one would be commented out by day two.
This gate is at zero already -- the tests were rewritten onto `ScratchDir`
first -- and the thing it prevents is not tech debt, it is the operator's
files. Zero is the only defensible ceiling.

## What it cannot see

A path assembled at runtime (`format!("/{}", x)`, `PathBuf::from("/").join(y)`)
and a literal that reaches a scan through a variable rather than directly.
Both are reachable in principle; neither is what the near-miss looked like,
and a check that needed name resolution to say anything would want `syn` and
not a regex. It also only reads `apps/diskcleanup`: no other program in the
tree unlinks what it scans, and widening it before one does would be a gate
looking for a population that does not exist.

Usage:

    python scripts/check-diskcleanup-test-roots.py [-v] [--self-test]
"""

from __future__ import annotations

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from rustscan import CFG_TEST_RE, blank_ranges, item_end, signature_of, strip_comments

# The subtree this gate reads. See the module docstring for why it is one crate
# and not the tree: `diskcleanup` is the only program that unlinks what it
# scanned.
CRATE = "apps/diskcleanup"

# The constant that makes a scan start at the filesystem root.
DEFAULT_ROOTS_RE = re.compile(r"\bDEFAULT_SCAN_ROOTS\b")

# Calls that walk a directory or unlink something. A root-anchored literal in
# any of their argument lists is the bug this gate exists for.
DANGEROUS_RE = re.compile(
    r"\b(?P<name>run_scan|scan|allow_root|set_scan_roots|execute|dry_run"
    r"|measure_recursive|enumerate_entries|remove)\s*(?=\()"
)

# A string literal naming an absolute location on *some* filesystem:
#
#   "/"  "/tmp/x"      -- POSIX absolute, and what `"/"` means on the dev host
#   "C:\\x"  "c:/x"    -- a Windows drive
#   "\\\\host\\share"  -- a UNC path
#
# Anything relative is fine: a relative path in a test resolves under the
# crate's own directory, which is not the operator's data.
ROOT_LITERAL_RE = re.compile(
    r"\"(?P<value>/[^\"]*|[A-Za-z]:[\\/][^\"]*|\\\\\\\\[^\"]*)\""
)


def test_only(text: str) -> tuple[str, str]:
    """`text` reduced to the parts that run *under* `cargo test`, twice over.

    The mirror image of `rustscan.production_only`, and needed for the same
    reason in reverse: this gate's entire subject is test code, so reading the
    whole file would flag `DEFAULT_SCAN_ROOTS` at its own definition and at
    every production use -- which are the correct uses -- and say nothing at
    all about the tests.

    Two versions come back, at identical offsets, because this gate needs both
    halves of a tension the rest of `rustscan`'s callers never feel:

    * `safe` has comments *and literals* blanked. Structure is read from this
      one -- which items are `#[cfg(test)]`, where a call's argument list ends
      -- because a `"{"` or a `"("` inside a string is precisely what throws
      a brace matcher off the end of the file.
    * `kept` has only the comments blanked. The *findings* are read from this
      one, because the finding **is** a string literal. Blanking it would
      leave the gate searching for evidence it had just destroyed.

    Slicing one by an offset computed from the other is sound only because
    both blanking passes replace characters in place and preserve every
    newline, so the two strings are the same length as each other and as the
    original, and a reported line number is a line the reader can go and look
    at.
    """
    safe = strip_comments(text)
    kept = strip_comments(text, keep_literals=True)
    keep: list[tuple[int, int]] = []
    pos = 0
    while (m := CFG_TEST_RE.search(safe, pos)) is not None:
        args = m.group("args")
        if not re.search(r"\btest\b", args) or re.search(r"\bnot\s*\(\s*test\b", args):
            pos = m.end()
            continue
        end = item_end(safe, m.end())
        keep.append((m.start(), end))
        pos = end
    # Blank the complement of `keep`: the gaps between the kept ranges.
    drop: list[tuple[int, int]] = []
    cursor = 0
    for start, end in keep:
        drop.append((cursor, start))
        cursor = end
    drop.append((cursor, len(safe)))
    return blank_ranges(safe, drop), blank_ranges(kept, drop)


def line_of(text: str, offset: int) -> int:
    """1-based line number of `offset`."""
    return text.count("\n", 0, offset) + 1


def findings(text: str) -> list[tuple[int, str]]:
    """`(line, message)` for every root-anchored reach in `text`'s test code."""
    safe, kept = test_only(text)
    out: list[tuple[int, str]] = []

    for m in DEFAULT_ROOTS_RE.finditer(safe):
        out.append(
            (
                line_of(safe, m.start()),
                "test code names DEFAULT_SCAN_ROOTS, which is [\"/\"] -- "
                "scan a ScratchDir instead",
            )
        )

    for m in DANGEROUS_RE.finditer(safe):
        # The span of the argument list is measured on `safe`, where a paren
        # inside a string cannot close the call early, and then read off
        # `kept`, where the literal still exists to be found.
        open_paren = safe.find("(", m.start())
        if open_paren < 0:
            continue
        span = len(signature_of(safe, m.start()))
        args = kept[open_paren : open_paren + span]
        lit = ROOT_LITERAL_RE.search(args)
        if lit is None:
            continue
        out.append(
            (
                line_of(safe, m.start()),
                f"test code passes {lit.group('value')!r} to "
                f"`{m.group('name')}` -- that walks or unlinks the host's "
                f"filesystem; scan a ScratchDir instead",
            )
        )

    return sorted(out)


# --------------------------------------------------------------------------
# Self-test fixtures.

# The shape the real tests use: a scratch tree, and a relative name inside it.
SCRATCH = """
#[cfg(test)]
mod tests {
    #[test]
    fn it_scans() {
        let scratch = ScratchDir::new("a");
        let mut ui = CleanupUI::new();
        ui.run_scan(&[scratch.as_str()]);
    }
}
"""

# The near-miss this gate is made of.
SCANS_THE_HOST = """
#[cfg(test)]
mod tests {
    #[test]
    fn it_scans() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&["/"]);
    }
}
"""

# Same, wearing the constant's name instead of its value.
USES_THE_CONSTANT = """
#[cfg(test)]
mod tests {
    #[test]
    fn it_scans() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&DEFAULT_SCAN_ROOTS);
    }
}
"""

# Production is where `DEFAULT_SCAN_ROOTS` belongs; flagging it there would
# make the gate demand its own deletion.
PRODUCTION_USE = """
const DEFAULT_SCAN_ROOTS: [&str; 1] = ["/"];

fn main() {
    let mut ui = CleanupUI::new();
    ui.run_scan(&DEFAULT_SCAN_ROOTS);
}
"""

# A comment in test code that quotes the rule must not trip the rule. Every
# finding this gate causes to be fixed leaves behind exactly such a comment.
RULE_IN_A_COMMENT = """
#[cfg(test)]
mod tests {
    // Never `ui.run_scan(&["/"])` -- see DEFAULT_SCAN_ROOTS.
    #[test]
    fn it_scans() {
        let scratch = ScratchDir::new("a");
        let mut ui = CleanupUI::new();
        ui.run_scan(&[scratch.as_str()]);
    }
}
"""

# Naming a path is not using one: this builds a struct and the confinement
# list keeps the executor away from it.
INERT_LITERAL = """
#[cfg(test)]
mod tests {
    #[test]
    fn it_defaults() {
        let item = CleanupItem::new("/tmp/foo", CleanupCategory::TempFiles);
        assert_eq!(item.path, PathBuf::from("/tmp/foo"));
    }
}
"""

# A Windows drive is a filesystem root too, and is what `"/"` becomes here.
WINDOWS_ROOT = """
#[cfg(test)]
mod tests {
    #[test]
    fn it_scans() {
        let mut scanner = CleanupScanner::new();
        scanner.allow_root("C:\\\\tmp");
    }
}
"""

# A nested paren in the argument list must not end the scan early.
NESTED_PARENS = """
#[cfg(test)]
mod tests {
    #[test]
    fn it_scans() {
        let mut ui = CleanupUI::new();
        ui.run_scan(&[String::from("/tmp").as_str()]);
    }
}
"""

SELF_TESTS = [
    ("a scratch-dir scan is not reported", SCRATCH, 0),
    ("scanning \"/\" is reported", SCANS_THE_HOST, 1),
    # One finding, not two: the constant rule fires, and the argument-list rule
    # does not, because `&DEFAULT_SCAN_ROOTS` holds no literal. That is the
    # whole reason the constant needs a rule of its own -- it is a root-anchored
    # path wearing a name instead of a value, and the literal scan is blind to
    # it by construction.
    ("DEFAULT_SCAN_ROOTS in a test is reported", USES_THE_CONSTANT, 1),
    ("DEFAULT_SCAN_ROOTS in production is not reported", PRODUCTION_USE, 0),
    ("the rule quoted in a comment is not reported", RULE_IN_A_COMMENT, 0),
    ("an inert path literal is not reported", INERT_LITERAL, 0),
    ("a Windows drive root is reported", WINDOWS_ROOT, 1),
    ("a nested paren does not end the argument list", NESTED_PARENS, 1),
]


def self_test() -> int:
    failed = 0
    for name, source, expected in SELF_TESTS:
        got = findings(source)
        ok = len(got) == expected
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            print(f"       expected {expected} finding(s), got {len(got)}: {got}")
            failed += 1
    print(f"\n{len(SELF_TESTS)} self-test case(s), {failed} failed")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    verbose = "-v" in argv or "--verbose" in argv

    root = pathlib.Path(__file__).resolve().parent.parent
    crate = root / CRATE
    if not crate.is_dir():
        print(f"FAIL: {CRATE} is missing -- this gate has lost its subject.")
        return 1

    files = [f for f in sorted(crate.rglob("*.rs")) if "target" not in f.parts]
    problems = 0
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        rel = path.relative_to(root).as_posix()
        found = findings(text)
        for line, message in found:
            print(f"{rel}:{line}: {message}")
        problems += len(found)
        if verbose and not found:
            print(f"  ok {rel}")

    print(f"\n{len(files)} file(s) checked in {CRATE}, {problems} finding(s)")
    if problems:
        print(
            "FAIL: a diskcleanup test would scan or delete under a real "
            "filesystem root. Build a ScratchDir and scan that -- see "
            "`scanned_over` in apps/diskcleanup/src/main.rs."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
