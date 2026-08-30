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

Exit codes: 0 clean; 1 an unpinned violation (or a pinned entry that no longer
violates, which means the baseline needs a deletion); 2 could not run.
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


def violations():
    """Every unfixed site, as (relpath, line_no, var, fn)."""
    out = []
    for path in sorted((ROOT / "kernel" / "src").rglob("*.rs")):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except OSError:
            continue
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
            if nxt.startswith("init_defaults("):
                continue
            rel = path.relative_to(ROOT).as_posix()
            out.append((rel, i + 1, m.group(2), fn))
    return out


def main() -> int:
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
    found = violations()
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

    carried = len(pinned)
    print(
        f"Self-test tables OK ({len(found)} clear-without-reopen site(s) carried "
        f"as known debt across {carried} file(s); none new)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
