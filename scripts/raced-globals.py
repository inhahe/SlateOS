#!/usr/bin/env python3
"""Find mutable process-globals that two or more `#[test]`s drive unserialised.

## Why this exists

`cargo test` runs the tests in a binary on many threads at once. A test that
sets a *process-global* to a known value, provokes something, and reads it back
is only meaningful if nothing else touches that global in between -- and when
five sibling tests do exactly the same thing on five threads, nothing is what
they get.

On 2026-08-22 this defect was found five times in one crate (`posix`):

    HTAB          `search.rs`   7 tests   -> SIGSEGV, killing all 20515 results
    SAVED         `string.rs`   3 tests   -> `assertion failed: !tok2.is_null()`,
                                             and, available but unobserved, one
                                             thread writing a NUL into another
                                             thread's live stack frame
    DL_ERROR      `dlfcn.rs`   15 tests   -> a stolen error message
    UMASK_VALUE   `file.rs`     3 tests   -> a stale previous-mask
    WALK_COUNT    `search.rs`    3 tests   -> a counter zeroed mid-walk

Two were found because they *failed*, under load, intermittently. Three were
found by then reading every global in the crate by hand. Nothing was looking,
which is why the count reached five. This looks.

See `known-issues.md` -> `B-POSIX-FOUR-MORE-PROCESS-GLOBALS-ARE-RACED-BY-THEIR-
OWN-TESTS`.

## What counts as a defect here

A **mutable global**: `static mut NAME`, or `static NAME: Atomic*` -- the two
shapes whose value can change through a shared reference. `Mutex`, `RwLock`,
`OnceLock`, `LazyLock` and `thread_local!` are excluded: they are the *fixes*,
not the defect.

...that is also **reset somewhere**: assigned, `store`d, `swap`ped, or reached
through `addr_of_mut!` / `&raw mut`. This condition is what separates the defect
from the pattern that *prevents* it. A `static COUNTER: AtomicU64` incremented
with `fetch_add` to name a scratch directory is read by every test in the file
and cannot go wrong for any of them -- whoever wins the race, both get a number
nobody else got, which is the entire point of it. It is only when a test can put
the global *back to a known value* that a sibling doing the same thing in the
middle destroys the answer. Without this condition the tool reported 84
globals, 7 of them unique-id counters accounting for the three largest reports
(106, 88 and 35 tests); with it, it reports the ones that can actually lie.

**Reached by a test** if the test's body names the global directly, or calls a
non-test function in the same file whose body names it. One level of indirection
is deliberate -- it is what connects `test_strtok_basic` to `SAVED` through
`strtok` -- and going deeper would need a real call graph for very little: the
tests in this tree call the function under test directly, because that is what
a unit test is.

**Unserialised** if neither the test's body nor any same-file helper it calls
mentions a lock. Any of `.lock()`, a `lock_*` helper, or a `*_LOCK` static
counts, wherever it appears. The one-hop indirection matters as much here as it
does for reachability: `posix::getopt`'s tests serialise by calling
`reset_getopt_state()`, which takes `GETOPT_TEST_LOCK` and hands back the guard,
so the word "lock" never appears in the test itself. Without following the call,
the tool reports the crate that got this *right* as its second-largest offender.

Two or more unserialised tests reaching one mutable global is a report. *One*
test is not: a global only raced by a single test is raced by nothing.

## What it is worth

This is a heuristic over syntax, so it has both kinds of error and neither is
silent.

**False positives** are expected and fine. A counter that only ever increments,
a one-shot init flag, an `AtomicUsize` handing out unique ids -- all are read by
several tests and none can produce a wrong answer. They go in `IGNORE` below
*with a reason*, which is auditable, rather than in the baseline, which is not.

**False negatives** are likelier, and worth knowing about:

  * A global reached through two or more hops is missed.
  * A global reached from an *integration* test (`tests/*.rs`) is missed: it is
    a different file, and often a different process, so the analysis does not
    apply.
  * The `perprocess!` / `perthread!` macro users in `posix` are missed, because
    the global is created by a macro and never appears as a `static` in the
    caller. That family was not audited by hand either -- it is the largest
    known gap in coverage, and the reason this script's clean run is not a
    proof of absence.

## The baseline, and why this is a ratchet

A check that goes red the day it lands is a check somebody deletes. The known
set is recorded in `raced-globals-baseline.txt`; `--check` fails only on a pair
that is *not* in it. The file only ever shrinks, and every line removed is a
race that can no longer cost a whole test binary.

**Do not add a line to make a red run green.** A new entry means a new test was
written against shared state without serialising it, and the fix is one of two
things, chosen by *why* the state is shared:

  * Shared **by specification** -- POSIX mandates one `hsearch` table, one
    `strtok` save pointer, one umask -- so it cannot stop being shared. Add a
    `static FOO_TEST_LOCK: Mutex<()>` and take it as the **first statement** of
    every test that touches it. The indivisible unit is the whole
    set-provoke-read body, not any single call. Recover poison
    (`unwrap_or_else(PoisonError::into_inner)`) so one real failure reports once
    instead of poisoning every sibling and burying the cause.
  * Shared only **incidentally** -- a test counter that happens to live in a
    `static` -- so it should stop being shared. Make it a `thread_local!`
    `Cell<T>`: libtest gives every test its own thread, so that is exactly
    per-test isolation, with no lock and no loss of concurrency. See
    `posix::malloc::live_regions` and `posix::search::WALK_COUNT`.

Usage:

    python scripts/raced-globals.py              # report everything found
    python scripts/raced-globals.py --all        # also list serialised globals,
                                                 #   to check the detector sees them
    python scripts/raced-globals.py --write-baseline
    python scripts/raced-globals.py --check      # exit 1 on a NEW raced global
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "raced-globals-baseline.txt"

# Directories that are not our source.
SKIP_DIRS = {"target", ".git", "node_modules", "vendor", "third_party"}

# `static mut NAME: ...` or `static NAME: AtomicUsize = ...`.
#
# `mut` and the atomics are the two ways a `static`'s value can change through a
# shared reference. Everything else -- Mutex, RwLock, OnceLock, LazyLock, a
# plain `static FOO: u32` -- either cannot change or serialises its own change,
# and is not what this is looking for.
_STATIC_MUT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+mut\s+([A-Za-z_]\w*)\s*:")
_STATIC_ATOMIC = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+([A-Za-z_]\w*)\s*:\s*(?:.*::)?Atomic\w+"
)

def _reset_write(name: str) -> re.Pattern[str]:
    """Does anything put `name` back to a chosen value?

    `fetch_add` and friends are deliberately *not* here: a counter that only
    ever moves forward hands every caller a distinct value, which is what makes
    it safe to share. The shapes below are the ones that overwrite.
    """
    n = re.escape(name)
    return re.compile(
        rf"\b{n}\s*\.\s*(?:store|swap|compare_exchange\w*)\s*\("
        rf"|addr_of_mut!\s*\(\s*{n}\b"
        rf"|&\s*raw\s+mut\s+{n}\b"
        # `HTAB = ...`, but also `HTAB.buckets = ...` and `TABLE[i] = ...`:
        # overwriting a *field* of a global is overwriting the global, and the
        # `hsearch` segfault this tool exists for was exactly that shape
        # (`HTAB.buckets = null_mut()` in `hdestroy`, read by `hsearch`).
        rf"|\b{n}(?:\s*\.\s*\w+|\s*\[[^\]]*\])*\s*=[^=]"
    )


# A global that *is* the lock is not a global being raced.
#
# `posix::sys_timex::TIMEX_LOCK` is an `AtomicBool` spin-lock, so every test
# that calls `adjtimex` reaches it -- 58 of them -- and every one of those is
# reaching it precisely *in order to* serialise. Reporting it inverts the
# meaning of the report. Matched on the name because that is what the tree
# spells consistently, and because the alternative (recognising a spin-acquire
# by its `compare_exchange` shape) would also match a legitimate CAS on ordinary
# state.
_IS_LOCK_NAME = re.compile(r"_(?:LOCK|MUTEX|GUARD|SEMAPHORE)$")

_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+([A-Za-z_]\w*)")
_TEST_ATTR = re.compile(r"^\s*#\[(?:\w+::)*test\]")

# Any of these in a test body means the author thought about serialisation. We
# do not try to check they took the *right* lock -- that is a judgement call, and
# a checker that second-guesses it would be wrong more often than the author.
_LOCK_HINT = re.compile(r"\.lock\(\)|\block_\w+|\b\w*_LOCK\b|#\[serial\]")

# False positives, each with the reason it is one. Keyed by "<relpath>:<NAME>",
# or "*:<NAME>" to excuse a name everywhere.
#
# Add to this table, not to the baseline: a line here says *why* the tool is
# wrong and can be argued with; a line in the baseline only says "known".
IGNORE: dict[str, str] = {
    # A global that only ever moves one way cannot give a test a wrong answer:
    # whichever sibling won the race, the value is still "some test has run".
    "*:TESTS_RAN": "monotonic flag; no test can observe a wrong value",
    # Memoisation, not state: every writer stores the *same* address of the
    # *same* `static` table, so a racing write stores the value that was already
    # there and a racing read gets either the null it started with (and then
    # recomputes the same answer) or that one address. Audited by hand
    # 2026-08-22 alongside the four real races in this crate.
    "posix/src/ctype.rs:CACHED": "memoised pointer to a static table; every write stores the same value",
}


def _relpath(p: Path) -> str:
    return p.relative_to(ROOT).as_posix()


def rust_files() -> list[Path]:
    """Every `.rs` we wrote.

    Prunes `SKIP_DIRS` *while walking* rather than filtering afterwards:
    `target/` holds tens of gigabytes of build output, and descending into it
    just to discard the results takes minutes.
    """
    out: list[Path] = []
    stack = [ROOT]
    while stack:
        d = stack.pop()
        try:
            entries = list(d.iterdir())
        except OSError:
            continue
        for e in entries:
            if e.is_dir():
                if e.name not in SKIP_DIRS:
                    stack.append(e)
            elif e.suffix == ".rs":
                out.append(e)
    return sorted(out)


def _block_end(lines: list[str], start: int) -> int:
    """Index of the line closing the block opened on or after `start`.

    Counts braces, ignoring those inside a line comment or a string literal.
    Approximate on purpose: a miscount costs a slightly wrong function boundary
    and so a slightly wrong report, never a wrong *file*.
    """
    depth = 0
    seen_open = False
    for i in range(start, len(lines)):
        line = lines[i]
        j = 0
        in_str = False
        while j < len(line):
            c = line[j]
            if in_str:
                if c == "\\":
                    j += 2
                    continue
                if c == '"':
                    in_str = False
            elif c == '"':
                in_str = True
            elif c == "/" and j + 1 < len(line) and line[j + 1] == "/":
                break
            elif c == "{":
                depth += 1
                seen_open = True
            elif c == "}":
                depth -= 1
                if seen_open and depth <= 0:
                    return i
            j += 1
    return len(lines) - 1


def analyse(path: Path) -> list[tuple[str, str, list[str], list[str]]]:
    """Return (name, decl_site, unserialised_tests, serialised_tests) per global."""
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return []

    globals_: dict[str, str] = {}
    for i, line in enumerate(lines):
        m = _STATIC_MUT.match(line) or _STATIC_ATOMIC.match(line)
        if m and not _IS_LOCK_NAME.search(m.group(1)):
            globals_[m.group(1)] = f"{_relpath(path)}:{i + 1}"
    if not globals_:
        return []

    # Drop the globals nothing ever resets -- see `_reset_write`. Checked over
    # the whole file rather than per function, because the reset and the read
    # are routinely in different places (a helper resets, the test reads).
    text = "\n".join(lines)
    globals_ = {n: s for n, s in globals_.items() if _reset_write(n).search(text)}
    if not globals_:
        return []

    # Function spans, and whether each is a #[test].
    fns: list[tuple[str, int, int, bool]] = []
    for i, line in enumerate(lines):
        m = _FN.match(line)
        if not m:
            continue
        is_test = any(_TEST_ATTR.match(lines[k]) for k in range(max(0, i - 6), i))
        fns.append((m.group(1), i, _block_end(lines, i), is_test))

    def body(span: tuple[int, int]) -> str:
        return "\n".join(lines[span[0] : span[1] + 1])

    # Helpers that take a lock on the caller's behalf. A test calling one of
    # these is serialised even though the word never appears in it.
    lock_helpers = {n for (n, s, e, _t) in fns if _LOCK_HINT.search(body((s, e)))}

    results = []
    for name, site in globals_.items():
        if IGNORE.get(f"*:{name}") or IGNORE.get(f"{site.rsplit(':', 1)[0]}:{name}"):
            continue
        word = re.compile(rf"\b{re.escape(name)}\b")

        # Non-test functions that name the global: the one hop a test may take.
        touchers = {n for (n, s, e, t) in fns if not t and word.search(body((s, e)))}

        unser, ser = [], []
        for fn_name, s, e, is_test in fns:
            if not is_test:
                continue
            b = body((s, e))
            reaches = bool(word.search(b)) or any(
                re.search(rf"\b{re.escape(t)}\s*\(", b) for t in touchers
            )
            if not reaches:
                continue
            serialised = _LOCK_HINT.search(b) or any(
                re.search(rf"\b{re.escape(h)}\s*\(", b)
                for h in lock_helpers
                if h != fn_name
            )
            (ser if serialised else unser).append(fn_name)
        if unser or ser:
            results.append((name, site, sorted(unser), sorted(ser)))
    return results


def load_baseline() -> set[str]:
    if not BASELINE.is_file():
        return set()
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def main() -> int:
    check = "--check" in sys.argv[1:]
    write = "--write-baseline" in sys.argv[1:]
    show_all = "--all" in sys.argv[1:]

    raced: list[tuple[str, str, list[str], list[str]]] = []
    guarded: list[tuple[str, str, list[str], list[str]]] = []
    for path in rust_files():
        for name, site, unser, ser in analyse(path):
            if len(unser) >= 2:
                raced.append((name, site, unser, ser))
            elif ser:
                guarded.append((name, site, unser, ser))

    if show_all:
        print(f"--- {len(guarded)} global(s) reached by tests and serialised ---")
        for name, site, unser, ser in guarded:
            note = f"  (+{len(unser)} unserialised)" if unser else ""
            print(f"  {site}  {name}  {len(ser)} serialised test(s){note}")
        print()

    keys = {f"{site.rsplit(':', 1)[0]}:{name}" for name, site, _, _ in raced}

    if write:
        body = [
            "# Mutable process-globals raced by two or more unserialised tests.",
            "# Generated by scripts/raced-globals.py --write-baseline.",
            "#",
            "# This file is a ratchet and only ever shrinks. Do NOT add a line to turn a",
            "# red --check green: a new entry is a new test written against shared state",
            "# without serialising it. Fix it -- a *_TEST_LOCK taken as the first",
            "# statement if the state is shared by specification, a thread_local! Cell if",
            "# it is shared only incidentally. See the module docstring.",
            "#",
            "# A genuine false positive belongs in the IGNORE table in the script, which",
            "# records *why*, not here, which records only *that*.",
            "",
        ]
        body += sorted(keys)
        BASELINE.write_text("\n".join(body) + "\n", encoding="utf-8", newline="")
        print(f"wrote {_relpath(BASELINE)} with {len(keys)} entries")
        return 0

    known = load_baseline()
    new = sorted(keys - known)

    # Under --check this runs in a push hook, where the baselined backlog is not
    # news: printing all 48 of them on every successful push is how a gate
    # teaches its readers to scroll past it. Report only what is actually new,
    # and only the summary line when there is nothing. Without --check a human
    # is asking to see the backlog, so print it.
    for name, site, unser, ser in sorted(raced, key=lambda r: r[1]):
        key = f"{site.rsplit(':', 1)[0]}:{name}"
        is_new = key in new
        if check and not is_new:
            continue
        flag = "NEW " if is_new else "    "
        extra = f", {len(ser)} serialised" if ser else ""
        print(f"{flag}{site}  {name}  {len(unser)} unserialised test(s){extra}")
        print(f"        {', '.join(unser)}")

    print(f"\n{len(raced)} raced global(s); {len(new)} not in the baseline.")

    if check and new:
        print("\nA test drives a mutable process-global that other tests also drive,")
        print("with no lock between them. Under load that is a wrong value at best")
        print("and a dead test binary at worst. See scripts/raced-globals.py.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
