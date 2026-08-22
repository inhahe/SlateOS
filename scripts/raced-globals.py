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
    # Reached only from `current()`'s `try_with(...).unwrap_or(&raw mut HOST_FALLBACK)`
    # arm, which is taken only once a thread's TLS has already been destroyed. A
    # `#[test]` body always runs with live TLS, so no test can reach it -- and a
    # thread that does is, as the comment there notes, the last one alive.
    "posix/src/perthread.rs:HOST_FALLBACK": "only reachable after thread-local teardown, which no #[test] body is",
    # Write-once under a three-state latch. `ensure_at_random_initialized()`
    # admits exactly one writer via compare_exchange(UNINIT -> FILLING); every
    # other caller spins until READY rather than writing, and the Release/Acquire
    # pair publishes all sixteen bytes. So the `addr_of_mut!` the detector sees
    # is reachable from ten tests but executes in only one of them, and no test
    # can observe a partially-written or re-rolled buffer. It genuinely raced
    # before 2026-08-22 -- the old bool let two threads both fill it, which
    # would re-roll a stack canary another thread had already cached -- so this
    # entry records a fixed bug, not an excused one.
    "posix/src/crt.rs:AT_RANDOM_BYTES": "write-once behind a compare_exchange latch; losers wait rather than write",
}


def _relpath(p: Path) -> str:
    """Repo-relative POSIX path, or the bare path if it is outside the repo.

    Everything the checker scans in anger lives under `ROOT`, so the fallback
    exists for one caller: `--selftest`, which analyses synthetic files in a
    temp directory. A `relative_to` that raises there would mean the self-test
    could not exercise `analyse()` at all -- which is precisely the function
    the self-test exists to pin down.
    """
    try:
        return p.relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


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


_CFG_ATTR = re.compile(r"^\s*#\[cfg\((.*)\)\]\s*$")
_ATTR_OR_DOC = re.compile(r"^\s*(?:#\[|#!\[|///|//!|//|$)")


def _cfg_true_on_host(pred: str) -> bool:
    """Could an item under `#[cfg(<pred>)]` be compiled by a host `cargo test`?

    Only two facts are known: `target_os = "none"` is false (tests run on the
    host, not on the bare-metal target) and `test` is true. *Everything else is
    treated as true*, which is the safe direction: an unknown predicate keeps
    the item in the report, so the tool can only ever fail toward noise, never
    toward silence.

    This has to actually parse, not pattern-match. The tree contains
    `#[cfg(any(target_os = "none", test))]` twenty times, and that item IS
    compiled on host -- via the `test` arm. A rule that merely looked for the
    string `target_os = "none"` would excuse all twenty.
    """

    def parse(s: str, i: int) -> tuple[bool, int]:
        while i < len(s) and s[i].isspace():
            i += 1
        start = i
        while i < len(s) and (s[i].isalnum() or s[i] in "_"):
            i += 1
        word = s[start:i]
        while i < len(s) and s[i].isspace():
            i += 1

        if i < len(s) and s[i] == "(":  # all(..) / any(..) / not(..)
            i += 1
            args: list[bool] = []
            while True:
                val, i = parse(s, i)
                args.append(val)
                while i < len(s) and s[i].isspace():
                    i += 1
                if i < len(s) and s[i] == ",":
                    i += 1
                    # Trailing comma before ')'.
                    j = i
                    while j < len(s) and s[j].isspace():
                        j += 1
                    if j < len(s) and s[j] == ")":
                        i = j
                    else:
                        continue
                if i < len(s) and s[i] == ")":
                    i += 1
                break
            if word == "not":
                return (not args[0] if args else True), i
            if word == "all":
                return all(args), i
            if word == "any":
                return any(args), i
            return True, i  # unrecognised combinator: assume compiled

        if i < len(s) and s[i] == "=":  # key = "value"
            i += 1
            while i < len(s) and s[i].isspace():
                i += 1
            if i < len(s) and s[i] == '"':
                i += 1
                vs = i
                while i < len(s) and s[i] != '"':
                    i += 1
                value = s[vs:i]
                i += 1 if i < len(s) else 0
                if word == "target_os":
                    return value != "none", i
            return True, i

        return True, i  # bare ident (`test`, `unix`, a feature): assume true

    try:
        return parse(pred, 0)[0]
    except (IndexError, RecursionError):
        return True


def _host_excluded_lines(lines: list[str]) -> set[int]:
    """Line indices belonging to items a host `cargo test` never compiles.

    Without this the tool reports the code that did it *right*. `posix`'s
    `no_new_privs` bit is a `thread_local!` on host and a `static AtomicBool`
    only under `#[cfg(target_os = "none")]` -- the exact fix this tool exists to
    ask for -- and the bare-metal half was being flagged by twenty host tests
    that cannot reach it. Eight of the first 48 baselined entries were this.
    """
    excluded: set[int] = set()
    for i, line in enumerate(lines):
        m = _CFG_ATTR.match(line)
        if not m or _cfg_true_on_host(m.group(1)):
            continue
        # The attribute binds to the next real item; skip further attributes,
        # doc comments and blank lines to find it.
        j = i + 1
        while j < len(lines) and _ATTR_OR_DOC.match(lines[j]):
            j += 1
        if j >= len(lines):
            continue
        end = _block_end(lines, j) if "{" in "\n".join(lines[j : j + 3]) else j
        excluded.update(range(i, end + 1))
    return excluded


def strip_comments_and_strings(src: str) -> str:
    """Blank out Rust comments and literals, keeping every byte offset intact.

    This is not cosmetic. Reachability here is "does the word appear in the
    body", and `time.rs`'s `settimeofday` contains the line

        // as a (deprecated) timezone-only update.  We accept it as a no-op.

    which made it a "toucher" of the `timezone` global and dragged all
    thirty-one settimeofday tests into the report -- none of which read the
    zone at all. Prose about a global is the single most likely place for its
    name to appear without being an access, so matching in comments does not
    merely add noise, it adds noise concentrated exactly where the tool is
    supposed to be precise.

    Replacement is space-for-character (newlines kept) so line numbers, block
    matching and `_host_excluded_lines` all keep working against the result.
    """
    out = list(src)
    i, n = 0, len(src)

    def blank(start: int, stop: int) -> None:
        for k in range(start, min(stop, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        # Raw string: r"..." / r#"..."# / br##"..."##
        m = re.compile(r"(?:b|c)?r(#*)\"").match(src, i)
        if m and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            close = '"' + m.group(1)
            end = src.find(close, m.end())
            end = n if end < 0 else end + len(close)
            blank(i, end)
            i = end
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            end = src.find("\n", i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            # Rust block comments nest.
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            blank(i, j)
            i = j
            continue
        if c == "'":
            # A char literal, or a lifetime (`'a`) -- only the former closes.
            m = re.compile(r"'(?:\\.|[^\\'])'").match(src, i)
            if m:
                blank(i, m.end())
                i = m.end()
                continue
        i += 1
    return "".join(out)


def analyse(path: Path) -> list[tuple[str, str, list[str], list[str]]]:
    """Return (name, decl_site, unserialised_tests, serialised_tests) per global."""
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    lines = raw.splitlines()
    # Structural matching (`#[cfg]`, `///`, `#[test]`, `fn`) needs the real
    # text; word matching inside bodies must not see prose. Same line count, so
    # every index below is valid against either.
    clean_lines = strip_comments_and_strings(raw).splitlines()

    # A global the host build never compiles cannot be raced by a host test.
    excluded = _host_excluded_lines(lines)

    # A *list*, not a dict keyed by name: a name identifies a global only at
    # file scope. Function-local statics make it ambiguous, and the ambiguity
    # is not theoretical -- `userspace/oils/src/interp.rs` declares eleven
    # separate statics called `COUNTER`, `kernel/src/sched/mod.rs` four called
    # `WARNED`. Keyed by name, ten of those eleven simply vanished from the
    # analysis, which is the failure direction that matters: a checker that
    # drops declarations reports a clean tree, not a broken one.
    globals_: list[tuple[str, str, int]] = []
    for i, line in enumerate(lines):
        if i in excluded:
            continue
        m = _STATIC_MUT.match(line) or _STATIC_ATOMIC.match(line)
        if m and not _IS_LOCK_NAME.search(m.group(1)):
            globals_.append((m.group(1), f"{_relpath(path)}:{i + 1}", i))
    if not globals_:
        return []

    # Drop the globals nothing ever resets -- see `_reset_write`. Checked over
    # the whole file rather than per function, because the reset and the read
    # are routinely in different places (a helper resets, the test reads).
    text = "\n".join(clean_lines)
    globals_ = [g for g in globals_ if _reset_write(g[0]).search(text)]
    if not globals_:
        return []

    # Function spans, and whether each is a #[test].
    fns: list[tuple[str, int, int, bool]] = []
    for i, line in enumerate(lines):
        m = _FN.match(line)
        if not m:
            continue
        if i in excluded:
            continue
        is_test = any(_TEST_ATTR.match(lines[k]) for k in range(max(0, i - 6), i))
        fns.append((m.group(1), i, _block_end(lines, i), is_test))

    def body(span: tuple[int, int]) -> str:
        return "\n".join(clean_lines[span[0] : span[1] + 1])

    def innermost_fn(line_idx: int) -> tuple[str, int, int, bool] | None:
        """The smallest function span containing `line_idx`, if any.

        A `static` declared inside a function body is a process-global *value*
        but a function-local *name*: Rust does not put it in scope anywhere
        else, so no code outside that function -- including any other test --
        can touch it. That is a guarantee from the language, not a heuristic,
        and it is worth encoding because the pattern is common in tests:
        `signal.rs` declares `static RECEIVED: AtomicI32` inside one `#[test]`
        purely so a nested `extern "C"` handler can record what it was passed.
        Three such statics were being reported, each against two tests that
        could not name them, purely because a generic nested-handler name
        (`handler`, `h`) collided with an unrelated call elsewhere in the file.
        """
        best = None
        for f in fns:
            if f[1] <= line_idx <= f[2] and (best is None or f[1] > best[1]):
                best = f
        return best

    # Helpers that take a lock on the caller's behalf. A test calling one of
    # these is serialised even though the word never appears in it.
    lock_helpers = {n for (n, s, e, _t) in fns if _LOCK_HINT.search(body((s, e)))}

    results = []
    for name, site, decl_line in globals_:
        if IGNORE.get(f"*:{name}") or IGNORE.get(f"{site.rsplit(':', 1)[0]}:{name}"):
            continue
        word = re.compile(rf"\b{re.escape(name)}\b")

        # Restrict the search to where the name is actually in scope.
        owner = innermost_fn(decl_line)
        if owner is not None and owner[3]:
            # Declared inside a `#[test]`: exactly one test can name it, so
            # there is no second test to race against, whatever it does.
            continue
        scope = [f for f in fns if owner is None or (owner[1] <= f[1] <= owner[2])]

        # Non-test functions that name the global: the one hop a test may take.
        touchers = {n for (n, s, e, t) in scope if not t and word.search(body((s, e)))}

        unser, ser = [], []
        for fn_name, s, e, is_test in fns:
            if not is_test:
                continue
            b = body((s, e))
            # Two different ways a test reaches the global, and only one of
            # them is scope-free. Naming it directly requires the name to be
            # in scope, so a match outside the declaring function is a
            # different `COUNTER` and not this one. *Calling* a toucher works
            # from anywhere, which is why that arm searches all of `fns`.
            in_scope = owner is None or (owner[1] <= s <= owner[2])
            reaches = (in_scope and bool(word.search(b))) or any(
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


def selftest() -> int:
    """Check the rules that decide what this tool reports.

    Every one of them was added because it had already produced a wrong
    answer against the real tree, and every one is easy to break silently
    while editing the regexes -- a checker that fails toward silence looks
    exactly like a clean tree. So they get real cases rather than a hand
    verification that is true once.

    Rules are counted from `rule()` calls rather than from a literal, so
    adding a case cannot leave the summary line claiming a total that no
    longer matches what ran.
    """
    import tempfile

    def classify_all(src: str) -> list[tuple[str, list[str], list[str]]]:
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "x.rs"
            p.write_text(src, encoding="utf-8")
            return [(n, u, s) for n, _site, u, s in analyse(p)]

    def classify(src: str) -> dict[str, tuple[list[str], list[str]]]:
        # Convenience view for the cases that declare each name once. Rules
        # about *duplicate* names must use `classify_all`, since collapsing
        # by name here is the very bug rule 8 exists to catch.
        return {n: (u, s) for n, u, s in classify_all(src)}

    failures: list[str] = []
    rules: list[str] = []
    current = ""

    def rule(name: str) -> None:
        nonlocal current
        current = name
        rules.append(name)

    def expect(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{current}: {label}: want {want!r}, got {got!r}")

    # 1. The base case: two tests drive one global, nothing serialises them.
    rule("base")
    got = classify(
        """
static mut COUNT: u32 = 0;
fn bump() { unsafe { COUNT = 1; } }
#[test]
fn a() { bump(); }
#[test]
fn b() { bump(); }
"""
    )
    expect("base/reported", sorted(got.get("COUNT", ([], []))[0]), ["a", "b"])

    # 2. A lock in either test moves it to the serialised column.
    rule("lock")
    got = classify(
        """
static mut COUNT: u32 = 0;
static COUNT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn bump() { unsafe { COUNT = 1; } }
#[test]
fn a() { let _g = COUNT_TEST_LOCK.lock().unwrap(); bump(); }
#[test]
fn b() { let _g = COUNT_TEST_LOCK.lock().unwrap(); bump(); }
"""
    )
    expect("lock/unserialised", got.get("COUNT", ([], []))[0], [])
    expect("lock/serialised", sorted(got.get("COUNT", ([], []))[1]), ["a", "b"])

    # 3. Comments must not make a function a toucher. Without the stripper
    #    `unrelated` names COUNT and both tests get dragged in.
    rule("comment")
    got = classify(
        """
static mut COUNT: u32 = 0;
fn bump() { unsafe { COUNT = 1; } }
fn unrelated() -> u32 { /* COUNT is not touched here */ 7 }
#[test]
fn a() { unrelated(); }
#[test]
fn b() { unrelated(); }
"""
    )
    expect("comment/not-reported", "COUNT" in got, False)

    # 4. A `static` declared inside a `#[test]` is in scope in exactly one
    #    test, so it cannot race however generic the nested helper's name is.
    rule("fn-local")
    got = classify(
        """
#[test]
fn a() {
    static SEEN: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
    extern "C" fn h(v: i32) { SEEN.store(v, Ordering::Relaxed); }
    SEEN.store(0, Ordering::Relaxed);
    h(1);
}
#[test]
fn b() { h(2); }
"""
    )
    expect("fn-local/not-reported", "SEEN" in got, False)

    # 5. A global the host never compiles cannot be raced by a host test --
    #    but `test` in the predicate means it *is* compiled on host.
    rule("cfg")
    got = classify(
        """
#[cfg(target_os = "none")]
static mut GONE: u32 = 0;
#[cfg(any(target_os = "none", test))]
static mut KEPT: u32 = 0;
fn t1() { unsafe { GONE = 1; } }
fn t2() { unsafe { KEPT = 1; } }
#[test]
fn a() { t1(); t2(); }
#[test]
fn b() { t1(); t2(); }
"""
    )
    expect("cfg/none-dropped", "GONE" in got, False)
    expect("cfg/test-arm-kept", sorted(got.get("KEPT", ([], []))[0]), ["a", "b"])

    # 6. A global nothing ever resets cannot lie to a test.
    rule("fetch_add-only")
    got = classify(
        """
static TALLY: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
fn hit() { TALLY.fetch_add(1, Ordering::Relaxed); }
#[test]
fn a() { hit(); }
#[test]
fn b() { hit(); }
"""
    )
    expect("fetch_add-only/not-reported", "TALLY" in got, False)

    # 7. A static owned by a *non-test* function is still reachable -- through
    #    a call to its owner -- but only tests that can actually name it may
    #    be counted for naming it. `c` below writes its own local `S`.
    rule("owner-scope")
    got = classify(
        """
fn owner_fn() {
    static mut S: u32 = 0;
    fn inner() { unsafe { S = 1; } }
    inner();
}
#[test]
fn a() { owner_fn(); }
#[test]
fn b() { owner_fn(); }
#[test]
fn c() { let S = 3; assert_eq!(S, 3); }
"""
    )
    expect("owner-scope/reached-via-owner", sorted(got.get("S", ([], []))[0]), ["a", "b"])

    # 8. Two function-local statics may share a name. Keyed by name, one of
    #    them silently replaces the other and its tests are never analysed --
    #    the real tree has eleven `COUNTER`s in one file and four `WARNED`s.
    rule("duplicate-names")
    all_ = classify_all(
        """
fn helper1() {
    static mut S: u32 = 0;
    fn inner1() { unsafe { S = 1; } }
    inner1();
}
fn helper2() {
    static mut S: u32 = 0;
    fn inner2() { unsafe { S = 2; } }
    inner2();
}
#[test]
fn a() { helper1(); }
#[test]
fn b() { helper1(); }
#[test]
fn c() { helper2(); }
#[test]
fn d() { helper2(); }
"""
    )
    expect(
        "duplicate-names/both-analysed",
        sorted(sorted(u) for n, u, _s in all_ if n == "S"),
        [["a", "b"], ["c", "d"]],
    )

    for f in failures:
        print(f"FAIL {f}", file=sys.stderr)
    bad = {f.split(":", 1)[0] for f in failures}
    print(f"selftest: {len(rules) - len(bad)}/{len(rules)} rules ok")
    return 1 if failures else 0


def main() -> int:
    if "--selftest" in sys.argv[1:]:
        return selftest()
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
