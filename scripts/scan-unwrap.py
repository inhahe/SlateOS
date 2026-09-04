#!/usr/bin/env python3
"""Find `.unwrap()` / `.expect(...)` in *production* kernel code.

CLAUDE.md forbids these outside tests: every one is a potential DoS if an
attacker can shape the input, and the crate lints (`unwrap_used = "warn"`,
`expect_used = "warn"`) are set to say so. Tests are exempt -- panicking on bad
data is the point there -- so the only interesting number is the production one,
and a plain `grep -c` cannot produce it: at the time of writing about 4500 of
the ~4575 sites in `kernel/src` are in test bodies.

Two things make this harder than it sounds, and the first version of this script
got both wrong:

1.  **Not every `.expect(` is a panic.** `kernel/src/json.rs` has a parser whose
    own method is named `expect`: `self.expect(b'"')?` returns a `Result` and is
    exemplary error handling, not a panic. The discriminator used here is that
    `Result::expect` / `Option::expect` return `T`, so a `?` after the call
    means it cannot be either of them. Calls on `self` are likewise assumed to
    be inherent methods.

2.  **Attributing a site to its enclosing function -- all of them.** Three
    versions of this script have now existed, and the first two got this wrong
    in opposite directions. Falling back to the *nearest preceding* `fn` fails
    because these self-tests routinely define helpers inside themselves:

        pub fn self_test() {
            fn case(...) { ... }        // <- nearest preceding fn
            foo().expect("register");   // <- but this is still test code
        }

    Nearest-preceding blames `case`, which is not named `*test*`, so all 25
    sites across five files were reported as production. What matters is not
    the nearest enclosing `fn` but whether *any* enclosing `fn` is a test, so
    the second version kept a **stack** of open functions, using indentation as
    the nesting proxy.

    That worked, but only because the tree happens to be rustfmt-formatted --
    a precondition this script asserted and nothing checked. It is now shared
    with `scripts/rust_scopes.py`, which tracks brace depth through a lexer
    that understands strings, raw strings, char literals and comments, so it
    does not care how the file is laid out. Brace depth was rejected when the
    second version was written, on the grounds that "raw strings, char literals
    and macro bodies all perturb the count" -- true of a naive counter, which is
    why the shared one is not naive, and why it is checked: it closes every
    scope it opens across all 800 files of `kernel/src`, and a single
    desynchronisation anywhere would leave a dangling scope at some file's EOF.

    The two methods were run against each other over all 4407 panicking sites
    in the tree before the switch, and agreed on every one.

A site is TEST if **any** function on the enclosing stack is named `*test*`
(which covers the `self_test` / `*_self_test` convention this kernel uses for
its boot suites), or if it carries `#[test]` / sits under `#[cfg(test)]`.
Doc-comment examples are skipped -- they are prose, not code.

3.  **A name is a proxy for "is this a test", and a few production functions
    trip it.** `kshell::eval_test` is the shell's `test` / `[` builtin: it
    parses a user-supplied expression and is production code by any measure,
    but it ends in `_test` and so was silently exempt. Nothing hides behind it
    today -- it has no sites -- which is precisely why it is worth naming now
    rather than after one appears. `NOT_TESTS` below is the list of such
    functions, and adding to it is a deliberate, reviewable act.

    The name is only a proxy for the real question, which is whether the
    function is reachable from anything but the boot self-test suite. Answering
    *that* needs a call graph, and it is not obviously worth one: the convention
    is followed almost everywhere, and the exceptions fit on one screen. But the
    proxy cannot separate `eval_test` (a shell builtin) from `tx_datapath_test`
    (a driver self-test) on the strength of the name alone, so the list is not
    optional -- without it the gate has a hole shaped like anything an author
    happens to call `*_test`.

4.  **A count of zero is only meaningful next to a count of files read.**
    Until 2026-09-03 the last line read `0 production site(s) across 0
    file(s)`, where that second number is the count of files *with findings* --
    so on a clean tree it is zero however many files were read, including
    none. `roots` defaults to the *relative* path `kernel/src`, and
    `Path.rglob` on a directory that does not exist yields nothing and raises
    nothing: run from the wrong working directory, or after a tree rename,
    this script printed exactly the line above and exited 0. That is the
    failure this whole family of gates exists to prevent, sitting inside one
    of the gates.

    Discovery is therefore reported and floored. A named root that does not
    exist, or a default run finding fewer than `DISCOVERY_FLOOR` files, exits
    **2** -- this tree's code for "I did not reach a verdict" -- which
    `run-checker.sh` turns into an abort with the reason quoted. Exit 2 rather
    than 1 because nothing was *found*: a finding of zero and an inability to
    look must not share an exit code. See `run-checker.sh`, which spells the
    same rule out for floors specifically.

5.  **`--self-test` grades the gate against a mutated copy of its real
    subject**, not against a synthetic string. The distinction is the one
    recorded in `known-issues.md` ->
    `TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE-AND-LOOK-LIKE-IT-GRADES-A-SUBSYSTEM`:
    a fixture the author wrote proves the classifier works on input shaped the
    way the author imagined, and proves nothing about whether the gate is
    still attached to `kernel/src`. So the self-test picks a real file out of
    the tree, plants a panicking site in a real production function *in
    memory*, and requires the gate to report it -- then plants the same site
    in a test scope, behind a `?`, on `self`, and in a doc comment, and
    requires it to report none of those. Nothing is written to disk, so the
    gate can prove it refuses without any lane ever committing a broken file.

Usage:
    python scripts/scan-unwrap.py [path ...]        # default: kernel/src
    python scripts/scan-unwrap.py --summary         # per-file counts only
    python scripts/scan-unwrap.py --show-skipped    # explain what was filtered
    python scripts/scan-unwrap.py --self-test       # grade the gate, not the tree
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import rust_scopes

DEFAULT_ROOT = "kernel/src"
# Discovery floor for a default run. `kernel/src` held ~805 `.rs` files when
# this was set, so 100 is an order of magnitude of headroom: it cannot fire on
# a tree that merely shrank, and it fires instantly on the case it is for --
# a wrong working directory, which yields 0. A floor near the true count would
# have to be edited on every refactor and would eventually be raised past a
# real regression by someone in a hurry.
DISCOVERY_FLOOR = 100

SITE_RE = re.compile(r"\.(unwrap|expect)\s*\(")
# `.expect(...)?` or `.unwrap()?` -- returns a Result, so not the panicking one.
FALLIBLE_RE = re.compile(r"\.(?:unwrap|expect)\s*\([^()]*\)\s*\?")
SELF_CALL_RE = re.compile(r"\bself\.(?:unwrap|expect)\s*\(")

# The "is this test code?" policy -- the name rule and the list of production
# functions that collide with it -- lives in `rust_scopes`, shared with
# `clippy-sites.py`. Two copies of it would disagree the first time one was
# edited, and this one gates the build.
is_test_name = rust_scopes.is_test_name


def classify_line(line: str) -> tuple[bool, str]:
    """(is_a_panicking_site, reason_if_not)."""
    stripped = line.strip()
    if stripped.startswith("//!") or stripped.startswith("///"):
        return False, "doc comment"
    if stripped.startswith("//"):
        return False, "comment"
    if not SITE_RE.search(line):
        return False, "no site"
    if SELF_CALL_RE.search(line):
        return False, "inherent method on self"
    if FALLIBLE_RE.search(line):
        return False, "returns Result (followed by `?`)"
    return True, ""


def _top(stack: list[rust_scopes.Scope]) -> tuple[str, bool]:
    """The name to report for a site, and whether *any* enclosing scope is a test.

    The name is the innermost function -- that is the useful one to print when
    going to fix a site -- but the test verdict is taken over the whole stack,
    because a helper defined inside `self_test` is test code however it is
    named.
    """
    if not stack:
        return "<top level>", False
    is_test = any(is_test_name(s.name) or s.cfg_test for s in stack)
    return stack[-1].name, is_test


def scan_file(path: Path, show_skipped: bool = False):
    """Return (production_findings, skipped) lists for a file on disk."""
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:  # pragma: no cover
        print(f"warning: cannot read {path}: {exc}", file=sys.stderr)
        return [], []
    return scan_text(text.splitlines(), show_skipped)


def scan_text(lines: list[str], show_skipped: bool = False):
    """Return (production_findings, skipped) lists for source held in memory.

    Split out from `scan_file` so `--self-test` can hand this the real subject
    with a defect planted in it. A gate that can only be tested through the
    filesystem can only be tested by writing a broken file into the tree,
    which no lane should ever be asked to do to prove a checker works.
    """
    # Pass 1: for every line, the stack of items enclosing it. `rust_scopes`
    # tracks `mod` as well as `fn`, so a `#[cfg(test)] mod tests` is a scope
    # that opens and *closes* rather than a line number past which everything
    # is assumed to be test code -- which is what this script used to do, and
    # which was wrong for any file with code after such a module.
    scopes = rust_scopes.scope_stack_per_line(lines)

    def enclosing(lineno: int) -> tuple[str, bool]:
        idx = lineno - 1
        return _top(scopes[idx] if 0 <= idx < len(scopes) else [])

    findings = []
    skipped = []
    for i, raw in enumerate(lines, start=1):
        if not SITE_RE.search(raw):
            continue
        is_site, reason = classify_line(raw)
        if not is_site:
            if show_skipped:
                skipped.append((i, reason, raw.strip()))
            continue
        name, is_test = enclosing(i)
        if is_test:
            if show_skipped:
                skipped.append((i, f"in test scope `{name}`", raw.strip()))
            continue
        findings.append((i, name, raw.strip()))

    return findings, skipped


def _insertion_points(lines: list[str]):
    """Two line indices in a real file: one inside production, one inside a test.

    Returns `(prod_idx, prod_fn, test_idx, test_fn)` or `None`.

    An index is only usable if the line *and the one after it* sit in the same
    innermost `fn`, because the mutation is spliced between them: a line whose
    successor has left the scope is the function's closing brace, and a site
    planted after it would be attributed to whatever follows.

    For the test side a *nested helper* is preferred -- a `fn` inside a
    `self_test` whose own name contains no "test". That is the exact shape that
    broke two earlier versions of this script (see the header), so it is the
    shape worth grading, and falling back to a plainly-named test function
    would quietly stop covering it.
    """
    scopes = rust_scopes.scope_stack_per_line(lines)

    def usable(i):
        if i + 1 >= len(scopes):
            return None
        a, b = scopes[i], scopes[i + 1]
        if not a or not b or a[-1].kind != "fn":
            return None
        if b[-1] is not a[-1] and (b[-1].kind != "fn" or b[-1].name != a[-1].name):
            return None
        return a

    prod = test = nested_test = None
    for i in range(len(lines)):
        stack = usable(i)
        if stack is None:
            continue
        if any(is_test_name(s.name) or s.cfg_test for s in stack):
            if test is None:
                test = (i, stack[-1].name)
            if nested_test is None and len(stack) > 1 and not is_test_name(stack[-1].name):
                nested_test = (i, stack[-1].name)
        elif prod is None:
            prod = (i, stack[-1].name)
        if prod is not None and nested_test is not None:
            break

    chosen_test = nested_test or test
    if prod is None or chosen_test is None:
        return None
    return (prod[0], prod[1], chosen_test[0], chosen_test[1],
            nested_test is not None)


def self_test() -> int:
    """Grade the gate against a mutated copy of a real `kernel/src` file.

    Exits 2, not 1, when it cannot find a subject: being unable to run the
    grading is not the same as running it and failing, and the whole point of
    this file's fourth header note is that those two must not share a code.
    """
    files, why = collect([Path(DEFAULT_ROOT)])
    if why or len(files) < DISCOVERY_FLOOR:
        print(why or (f"cannot self-test: found only {len(files)} .rs file(s) "
                      f"under {DEFAULT_ROOT}"))
        return 2

    # Prefer a subject offering a *nested* test helper -- a `fn` inside a
    # `self_test` whose own name says nothing about testing. That is the shape
    # that defeated the nearest-preceding-`fn` version of this script, so a
    # self-test that never plants a site there would stop covering the very
    # regression the scope stack exists to prevent. A file with only a plainly
    # named test function is kept as a fallback rather than skipped, so the
    # self-test still runs on a tree that happens not to nest.
    subject = lines = points = None
    for f in files:
        try:
            candidate = f.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            continue
        found = _insertion_points(candidate)
        if not found:
            continue
        if points is None:
            subject, lines, points = f, candidate, found
        if found[4]:  # nested helper -- take it and stop looking
            subject, lines, points = f, candidate, found
            break
    if not points:
        print("cannot self-test: no file in "
              f"{DEFAULT_ROOT} offers both a production and a test scope")
        return 2

    prod_idx, prod_fn, test_idx, test_fn, nested = points
    base = len(scan_text(lines)[0])
    print(f"[scan-unwrap self-test] subject {subject} "
          f"(production `{prod_fn}` line {prod_idx + 1}, "
          f"{'nested ' if nested else ''}test `{test_fn}` line {test_idx + 1}, "
          f"{base} real production site(s))")

    def planted(idx: int, text: str) -> int:
        mutant = lines[:idx + 1] + [text] + lines[idx + 1:]
        return len(scan_text(mutant)[0])

    panic = "    let _zz_probe = zz_probe.unwrap();"
    cases = [
        # The one that matters: a real panicking site in a real production
        # function must be reported. If this fails the gate has come untethered
        # from kernel/src and its zero means nothing.
        ("a planted .unwrap() in production is reported",
         planted(prod_idx, panic), base + 1),
        ("a planted .expect() in production is reported",
         planted(prod_idx, '    let _zz_probe = zz_probe.expect("zz");'), base + 1),
        # Negative controls. Each is a way the classifier is allowed to say no,
        # and each has been got wrong at least once by some version of it.
        (f"the same site inside {'a nested helper in ' if nested else ''}"
         f"test scope `{test_fn}` is not reported",
         planted(test_idx, panic), base),
        ("a site followed by `?` is not reported",
         planted(prod_idx, "    let _zz_probe = zz_probe.unwrap()?;"), base),
        ("an inherent `self.expect(` is not reported",
         planted(prod_idx, "    let _zz_probe = self.expect(0);"), base),
        ("a site inside a doc comment is not reported",
         planted(prod_idx, "    /// let _zz_probe = zz_probe.unwrap();"), base),
        ("a site inside a line comment is not reported",
         planted(prod_idx, "    // let _zz_probe = zz_probe.unwrap();"), base),
    ]

    failed = 0
    for label, got, want in cases:
        if got == want:
            print(f"ok   {label}")
        else:
            failed += 1
            print(f"FAIL {label}: production count {got}, expected {want}")

    print(f"\n{len(cases)} self-test case(s), {failed} failed")
    return 1 if failed else 0


def _decline(reason: str, detail: str) -> int:
    """Print a no-verdict message whose FIRST line is `reason`, and return 2.

    Both halves go to stdout on purpose. `run-checker.sh` merges a checker's
    stdout and stderr into one log and quotes `head -n 1` of it as the reason
    the gate did not reach a verdict -- so the two streams must not race. They
    do: redirected stdout is block-buffered and stderr is not, so a reason on
    stdout and an explanation on stderr arrive in the merged log in the wrong
    order, and `head -n 1` then quotes a blank line. Measured, not theorised;
    see known-issues.md ->
    TD-A-A-SKIP-REASON-CAN-BE-A-BLANK-LINE-BECAUSE-TWO-STREAMS-RACE.

    One stream cannot race itself, which is why this is the fix here rather
    than a flush: a flush would have to be got right at every future exit
    point, and the one that is forgotten is the one that matters.
    """
    print(reason)
    print()
    print(detail)
    return 2


def collect(roots: list[Path]) -> tuple[list[Path], str]:
    """Every `.rs` under `roots`, plus a reason string if discovery is unsound.

    A missing root is reported rather than skipped. `Path.rglob` on a path that
    does not exist returns an empty iterator and raises nothing, so without
    this check a mistyped or moved root is indistinguishable from a clean one.
    """
    files: list[Path] = []
    missing = [str(r) for r in roots if not r.exists()]
    if missing:
        return [], ("cannot scan: no such path: " + ", ".join(missing))
    for root in roots:
        if root.is_file():
            files.append(root)
        else:
            files.extend(sorted(root.rglob("*.rs")))
    return files, ""


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("paths", nargs="*", default=None)
    ap.add_argument("--summary", action="store_true", help="per-file counts only")
    ap.add_argument("--show-skipped", action="store_true", help="explain filtering")
    ap.add_argument("--self-test", action="store_true",
                    help="grade this gate against a mutated copy of its real subject")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    default_root = not args.paths
    roots = [Path(p) for p in (args.paths or [DEFAULT_ROOT])]
    files, why = collect(roots)
    if why:
        return _decline(why,
                        "Nothing was scanned, so nothing was checked. This is not a "
                        "clean tree;\nit is a gate that could not find the tree at "
                        "all. Run it from the\nrepository root, or name the path "
                        "explicitly.")
    if default_root and len(files) < DISCOVERY_FLOOR:
        return _decline(
            f"cannot scan: found only {len(files)} .rs file(s) under "
            f"{DEFAULT_ROOT}, and this gate requires at least {DISCOVERY_FLOOR}",
            f"The floor exists because a scan that discovers nothing reports zero\n"
            f"findings in the same words a clean tree does. Either the working\n"
            f"directory is wrong, or {DEFAULT_ROOT} has genuinely shrunk below the\n"
            f"floor -- in which case lower DISCOVERY_FLOOR deliberately, as its own\n"
            f"commit.")

    total = 0
    per_file: list[tuple[int, Path]] = []
    for f in files:
        findings, skipped = scan_file(f, args.show_skipped)
        if args.show_skipped and skipped:
            for lineno, reason, line in skipped:
                print(f"  skip {f}:{lineno}: ({reason}) {line}")
        if not findings:
            continue
        total += len(findings)
        per_file.append((len(findings), f))
        if not args.summary:
            for lineno, where, line in findings:
                print(f"{f}:{lineno}: [fn {where}] {line}")

    print()
    for count, f in sorted(per_file, reverse=True):
        print(f"{count:5d}  {f}")
    # The denominator is the point: `0 site(s) in 0 file(s) scanned` used to be
    # the output of a gate that had lost its subject, and read identically to a
    # clean tree. Both numbers are printed so neither can be read alone.
    print(f"\n{total} production site(s) in {len(per_file)} file(s), "
          f"{len(files)} file(s) scanned")
    # Non-zero on findings, so `boot-test.sh` can gate on it. This was
    # documented from the start and returned 0 unconditionally anyway, which
    # made every caller's `if scan-unwrap.py; then` succeed regardless -- the
    # same shape of defect as the `validate()` in `mm/kvspace.rs` that was
    # documented "call once at boot" and called only from a self-test.
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
