#!/usr/bin/env python3
"""Guard the rule that a self-test leaves its table EMPTY, never DEAD.

The rule
--------
**A `self_test` that clears a `Mutex<Option<_>>` state table must re-open it
before returning.** Clearing is right; stopping there is not.

Why the distinction is not pedantic
-----------------------------------
These modules keep their data in `static STATE: Mutex<Option<State>>`.
`init_defaults()` sets it to `Some(..)`; every writer goes through a
`with_state` helper that returns `KernelError::NotSupported` while it is
`None`. So the two end states are not "empty" and "slightly emptier" -- they
are *empty* and *switched off*:

    *STATE.lock() = None;                  <- module is off for the rest of boot
    *STATE.lock() = None; init_defaults();  <- module is empty and live

`init_defaults()` is called once per boot, and for most of these modules that
once is inside `self_test` itself. So the teardown was not restoring a
pre-test state, it was destroying the only state there would ever be.

What that cost, concretely
--------------------------
146 modules did this. 18 of them are opened by *nothing* except their own
self-test, so `/proc/cpustat`, `/proc/inodestat`, `/proc/schedlat` and fifteen
others returned zeros for the entire life of every boot -- on a busy machine
and an idle one alike. The remaining 124 were opened lazily by their kshell
command, which is worse than it sounds: a `/proc` file whose contents depend on
whether an operator has ever typed the matching shell command is not a `/proc`
file, it is a shell cache.

Why a gate and not a habit
--------------------------
Because the defect is invisible from inside the function that has it. The
teardown reads as obviously correct -- it *is* obviously correct, as an
intention -- and nothing downstream complains, because the callers of these
recorders must discard accounting errors (statistics may never make a real
operation fail). So a dead table and an unfed table produce byte-identical
output, which is exactly how this hid: it was found only when `futexstat` was
given a real writer and the writes still did not appear.

A rule whose violation produces no signal is a rule that needs a gate, not a
convention. See known-issues.md
`A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT`.

What is checked
---------------
Inside a function whose name contains `self_test`, a statement of the form

    *<UPPERCASE_IDENT>.lock() = None;

must be followed -- ignoring blank lines and comments -- by `init_defaults();`.

Scoped to `self_test` deliberately. The same statement in production code is
usually correct and unrelated: `net/dhcp.rs` clears `*PENDING_OFFER.lock()` to
return the state machine to Idle, which is a legitimate `Option` clear and no
business of this checker's. A gate that fired there would be trained away
within a week.

The baseline is empty, and that is the point
--------------------------------------------
`scripts/selftest-reinit-baseline.txt` would pin known violations one per line,
and a violation NOT in it fails the check. **The file does not exist, because
the count is zero** -- all 117 sites were fixed in one pass, since the fix is
the same two lines everywhere and there was no reason to carry a backlog for a
change that mechanical.

The mechanism is kept anyway, for the case this checker is wrong rather than
the code: a module where `None` is a *meaningful* state and re-opening would be
incorrect. That case is real -- `viewstate.rs` clears `*GLOBAL_DEFAULTS.lock()`
in `clear_global_defaults()`, where `None` means "revert to built-in" -- which
is why the `self_test` scoping exists. If such a thing ever appears *inside* a
self-test, pin it with a comment saying why, rather than loosening the rule.

The list may shrink and may never grow. Adding a line to buy silence for new
code is the exact failure this gate exists to prevent.

Why a clean run reports what it *found*, not just what was wrong
----------------------------------------------------------------
This gate's healthy state is zero violations, which means its success message
is the same sentence whether the rule holds everywhere or the scan collapsed
and read nothing. So the message leads with the discovery count -- "273 clears
inside a self_test across 805 files" -- and two floors (`MIN_FILES`,
`MIN_CLEARS`) turn an implausibly thin scan into a refusal to answer rather
than a pass. A `--self-test` cannot cover this: fixtures are handed to
`scan_lines` directly, so they exercise the analyser and say nothing about the
tree walk that decides what the analyser sees.

Exit codes: 0 clean; 1 an unpinned violation (or a pinned entry that no longer
violates, which means the baseline needs a deletion); 2 no verdict -- not a
worktree, an unreadable file, or a scan below a discovery floor. `--self-test`
runs the fixtures instead and returns 0 or 1.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts" / "selftest-reinit-baseline.txt"

RESET = re.compile(r"^(\s*)\*([A-Z][A-Z0-9_]*)\.lock\(\) = None;\s*$")
FN = re.compile(r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)")


def load_baseline() -> set:
    """Pinned violations, as `<path>` strings relative to the repo root."""
    if not BASELINE.exists():
        return set()
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


def enclosing_fn(lines, i):
    """Name of the nearest function whose `fn` line is above `i` and less indented.

    A brace-counting scope tracker would be more precise, but these files are
    plain module code with no closures around the statements in question, and
    an indentation walk cannot be fooled by a brace inside a string literal --
    which a counter can, and which has bitten a sibling checker before.
    """
    indent = len(lines[i]) - len(lines[i].lstrip())
    for k in range(i - 1, -1, -1):
        m = FN.match(lines[k])
        if m and len(m.group(1)) < indent:
            return m.group(2)
    return None


def scan_lines(lines):
    """Every `*X.lock() = None;` inside a `self_test` fn, fixed or not.

    Returns `(line_no, var, fn, reopened)`.  Both halves are returned on
    purpose: the *fixed* ones are what tells a caller the scan actually
    reached the code it was written against.  A run that reports zero
    violations because it found zero clears is indistinguishable, in its own
    output, from a run that found 117 and every one was correct -- and those
    are the two states this gate has to tell apart.  See `floor()`.
    """
    out = []
    for i, line in enumerate(lines):
        m = RESET.match(line)
        if not m:
            continue
        fn = enclosing_fn(lines, i)
        if not fn or "self_test" not in fn:
            continue
        j = i + 1
        while j < len(lines) and (
            not lines[j].strip() or lines[j].strip().startswith("//")
        ):
            j += 1
        nxt = lines[j].strip() if j < len(lines) else ""
        out.append((i + 1, m.group(2), fn, nxt.startswith("init_defaults(")))
    return out


def scan_tree():
    """`(relpath, line_no, var, fn, reopened)` for the whole kernel tree.

    An unreadable file raises rather than being skipped.  A `continue` here
    would silently shrink the scan, which is the one thing this gate must not
    do quietly: fewer files scanned means fewer findings, and fewer findings
    is spelled the same as a clean tree.
    """
    out = []
    files = 0
    for path in sorted((ROOT / "kernel" / "src").rglob("*.rs")):
        files += 1
        lines = path.read_text(encoding="utf-8").splitlines()
        rel = path.relative_to(ROOT).as_posix()
        for line_no, var, fn, reopened in scan_lines(lines):
            out.append((rel, line_no, var, fn, reopened))
    return files, out


def violations():
    """Every unfixed site, as (relpath, line_no, var, fn)."""
    return [(rel, ln, var, fn) for rel, ln, var, fn, ok in scan_tree()[1] if not ok]


#: Discovery floors.  Measured 2026-09-03: 805 `.rs` files under `kernel/src`,
#: 277 `*X.lock() = None;` statements, 273 of them inside a `self_test`, and 0
#: of those unfixed.  The floors sit far below both counts so that ordinary
#: churn never trips them and a *collapsed scan* always does.  What they assert
#: is not "the code is right" -- the rest of the file does that -- but "I am
#: still reading the code at all".
MIN_FILES = 200
MIN_CLEARS = 40


def floor(files, sites):
    """Refuse a verdict if the scan came back implausibly empty.

    Returns a complaint string, or None.  This is the check a `--self-test`
    structurally cannot make for us: a fixture is handed to `scan_lines`
    directly, so it exercises the analyser and says nothing at all about the
    step that decides what the analyser is handed.
    """
    if files < MIN_FILES:
        return (f"found only {files} .rs file(s) under kernel/src, below the "
                f"floor of {MIN_FILES}; the tree walk is not reaching the "
                "kernel, so 'no violations' would mean 'no code read'")
    if len(sites) < MIN_CLEARS:
        return (f"found only {len(sites)} `*X.lock() = None;` statement(s) "
                f"inside a self_test, below the floor of {MIN_CLEARS}. Either "
                "the modules stopped using `Mutex<Option<_>>` -- in which case "
                "this gate needs rewriting, not passing -- or RESET/FN stopped "
                "matching the code they were written against")
    return None


def self_test() -> int:
    """Fixtures for both directions of every rule the analyser applies.

    A suite of true positives passes for a checker that reports everything; a
    suite of true negatives passes for one that reports nothing.  Each rule
    below therefore gets one of each, and each case states what it would mean
    if it were the one to fail.
    """
    failures = []
    n = 0

    def check(label, condition):
        nonlocal n
        n += 1
        if not condition:
            failures.append(label)
            print(f"FAIL {label}")

    def sites(src):
        return scan_lines(src.strip("\n").split("\n"))

    bad = sites("""
fn self_test() -> bool {
    *CPUSTAT.lock() = None;
    true
}
""")
    check("a bare clear in a self_test is found", len(bad) == 1)
    check("...and is reported as not re-opened", bad and bad[0][3] is False)
    check("...and names the static", bad and bad[0][1] == "CPUSTAT")

    good = sites("""
fn self_test() -> bool {
    *CPUSTAT.lock() = None;
    init_defaults();
    true
}
""")
    check("a clear followed by init_defaults is not a violation",
          len(good) == 1 and good[0][3] is True)

    # The skip-blanks-and-comments walk: if it stopped working, every fixed
    # site in the tree would come back as a fresh violation at once.
    spaced = sites("""
fn self_test() -> bool {
    *CPUSTAT.lock() = None;

    // Re-open: `None` is a switched-off module, not an empty table.
    init_defaults();
    true
}
""")
    check("blank lines and comments do not hide the re-open",
          len(spaced) == 1 and spaced[0][3] is True)

    # The `self_test` scoping.  This is the clause that keeps the gate off
    # `net/dhcp.rs`, where clearing to `None` is the correct state machine
    # transition; lose it and the gate fires on correct production code.
    check("the same clear in production code is not scanned",
          sites("""
fn clear_global_defaults() {
    *GLOBAL_DEFAULTS.lock() = None;
}
""") == [])
    check("...and a helper named for it still is",
          len(sites("""
fn run_self_test_inner() -> bool {
    *CPUSTAT.lock() = None;
    true
}
""")) == 1)

    # `enclosing_fn` walks up to a *less indented* `fn`.  A nested item would
    # otherwise attribute the statement to the wrong function.
    check("an unindented statement has no enclosing fn",
          sites("*CPUSTAT.lock() = None;") == [])

    # The floors, in both directions.
    check("a plausible scan passes the floor",
          floor(MIN_FILES, [None] * MIN_CLEARS) is None)
    check("an empty tree walk refuses a verdict",
          "no code read" in (floor(0, [None] * MIN_CLEARS) or ""))
    check("a collapsed pattern match refuses a verdict",
          "stopped matching" in (floor(MIN_FILES, []) or ""))

    if failures:
        print(f"\n{len(failures)} of {n} self-test(s) FAILED")
        return 1
    print(f"[selftest-reinit] self-test passed ({n} checks)")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()

    src = ROOT / "kernel" / "src"
    if not src.is_dir():
        print(f"[selftest-reinit] not a SlateOS worktree: {ROOT}", file=sys.stderr)
        return 2

    if "--pin" in sys.argv:
        # Writes the baseline. Read the diff before committing: a --pin that
        # ADDS a line is the regression this gate exists to catch, committed by
        # hand. Kept because regenerating 100+ entries by hand is its own
        # source of error, not because adding entries is expected.
        seen = sorted({rel for rel, _, _, _ in violations()})
        header = BASELINE.read_text(encoding="utf-8").split("\n# ---8<---\n")[0] if BASELINE.exists() else ""
        BASELINE.write_text(
            (header + "\n# ---8<---\n" if header else "") + "\n".join(seen) + "\n",
            encoding="utf-8",
        )
        print(f"[selftest-reinit] pinned {len(seen)} file(s) to {BASELINE.name}")
        return 0

    pinned = load_baseline()
    try:
        files, sites = scan_tree()
    except OSError as exc:
        print(f"[selftest-reinit] cannot read the kernel tree: {exc}", file=sys.stderr)
        return 2
    complaint = floor(files, sites)
    if complaint is not None:
        print(f"[selftest-reinit] refusing a verdict: {complaint}", file=sys.stderr)
        return 2

    found = [(rel, ln, var, fn) for rel, ln, var, fn, ok in sites if not ok]
    by_file = {}
    for rel, ln, var, fn in found:
        by_file.setdefault(rel, []).append((ln, var, fn))

    unpinned = sorted(set(by_file) - pinned)
    stale = sorted(pinned - set(by_file))

    if unpinned:
        print("", file=sys.stderr)
        print("ERROR: a self_test clears its state table and never re-opens it.", file=sys.stderr)
        print("", file=sys.stderr)
        for rel in unpinned:
            for ln, var, fn in by_file[rel]:
                print(f"  {rel}:{ln}  in {fn}(): *{var}.lock() = None;", file=sys.stderr)
        print("", file=sys.stderr)
        print("`None` is not an empty table, it is a switched-off module: every", file=sys.stderr)
        print("later write takes the NotSupported arm and is discarded by a caller", file=sys.stderr)
        print("that must not let statistics fail a real operation.  /proc then", file=sys.stderr)
        print("reports zeros for the rest of the boot, which reads as a measured", file=sys.stderr)
        print("zero rather than as a missing measurement.", file=sys.stderr)
        print("", file=sys.stderr)
        print("Fix: follow the clear with `init_defaults();` (it is idempotent --", file=sys.stderr)
        print("it returns early when the table is already open).  Do NOT fix it by", file=sys.stderr)
        print("deleting the clear: the fixtures genuinely must not survive into the", file=sys.stderr)
        print("live table, and rungs like pagecache's 'empty after init' depend on", file=sys.stderr)
        print("starting clean.  Worked examples: fs/futexstat.rs, fs/pagecache.rs.", file=sys.stderr)
        print("", file=sys.stderr)
        print("known-issues.md A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT", file=sys.stderr)
        return 1

    if stale:
        print("", file=sys.stderr)
        print("ERROR: the baseline pins files that no longer violate the rule.", file=sys.stderr)
        print("This is good news with a required edit: delete these lines from", file=sys.stderr)
        print(f"{BASELINE.name} so the count cannot drift back up unnoticed.", file=sys.stderr)
        print("", file=sys.stderr)
        for rel in stale:
            print(f"  {rel}", file=sys.stderr)
        return 1

    # The count that is reported first is the one that was *inspected*, not the
    # one that was wrong.  "0 violations" is the same sentence whether the rule
    # holds across 273 sites or the scan found none at all, and only the second
    # number tells them apart.
    print(
        f"Self-test tables OK ({len(sites)} clear(s) inside a self_test across "
        f"{files} file(s); {len(sites) - len(found)} re-open, {len(found)} do "
        f"not, {len(pinned)} pinned as known debt; none new)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
