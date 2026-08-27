#!/usr/bin/env python3
"""Guard the rule that a self-test may not compare two readings of one counter.

The rule
--------
**Within a `self_test`, an equality assertion may not compare two separate
readings of the same live counter.** If it needs two views, it takes one
snapshot and derives both from it.

A *live counter* here means specifically one that moves **without the test
doing anything**: the timer interrupt, device interrupts, the clock, an AP
coming online. A counter the test itself drives -- `active_count()` in a
container test that creates and destroys containers -- is not one of these,
and is not what this file is about.

Why this exists
---------------
Because it happened four times in one day, and the fourth one panicked the
kernel on a green tree:

    [5/6] tick cross-check (39171 >= 39171): OK
    !!! KERNEL PANIC !!!
    panicked at kernel/src/fs/irqstat.rs:354:5:
    assertion `left == right` failed
      left: 39171
     right: 39170

One timer interrupt. `irqstat::self_test` rung 3 did `let t = totals();` and
rung 6, three rungs later, did `let lines = irq_lines();`. Each takes its own
`idt::vector_counts()` snapshot, so the assertion held only if no interrupt
arrived in the microseconds between them. It held for a few hundred boots and
then did not. That is the worst frequency a flake can have: too rare to
reproduce on demand, too common never to be seen.

The same shape was in three other places the same day -- `numastat`
(`adopt_topology` enumerated CPUs, the self-test re-read `smp::cpu_count()`),
`fs/sysfs.rs` (reads `cpu/online`, then rebuilds the expected string from a
second `smp::cpu_count()`), and `irqbalance.rs` (`stats()` reads
`smp::cpu_count()`, and the assertion compares it to another one). Nobody
introduced these together; each was locally reasonable.

That is the signature of a rule with no gate. And the most uncomfortable
evidence that review alone will not do it: `irqstat`'s rung 5 reasons this
exact race out correctly and at length --

    Read the ticks FIRST: a tick landing between the two reads can only then
    make the count look larger, never smaller, so this cannot flake.

-- and the same function then compares two live samples with `==` one rung
later. Knowing a counter moves is not the same as noticing every place you
assumed it did not.

Why the trigger is a comparison and not a count
-----------------------------------------------
The first version of this file triggered on the cheap thing: a self-test that
*reads* one live counter twice, whether or not it compares the readings. The
reasoning was that proving a comparison needs dataflow, that dataflow is
beyond a regex, and that the weaker trigger would cost little because the
kernel only had about a dozen such sites.

That estimate was wrong by a factor of twenty-five. The weak trigger reported
**297** sites, and the reason is obvious in hindsight: 173 of them were
`elapsed_ns`, which every duration measurement in the tree reads exactly twice
*on purpose* --

    let t0 = hpet::elapsed_ns();
    ...
    let dt = hpet::elapsed_ns() - t0;

-- and 116 were `cpu_count`, which any test that loops over CPUs more than
once reads more than once, harmlessly. A ledger of 297 grandfathered entries
is a rubber stamp by construction: nobody reads it, and the one real entry
that matters is lost among the 296 that do not. `design-decisions.md` 299 says
a gate's trigger is part of its rule, and a trigger with a 98% false-positive
rate is not a rule, it is noise.

So this file does the dataflow. It is not much: bind live-counter reads to the
locals they flow into, then ask whether an equality assertion has two
*different* reads of one counter on opposite sides. That is the actual bug
shape, it is what all four instances have, and `assert!(t1 >= t0)` on a clock
is not it.

That took the same tree from **297 findings to 2**, in four steps, each of
which is written up at the function that implements it:

    297  every self-test that reads one live counter twice
     20  only equality assertions across two *different* reads  (`walk`)
     10  wrappers propagate a counter only if it reaches their return value,
         which is what tells a returned reading from a filed timestamp
         (`resolve_returns`, `tail_span`)
      7  `if let` binds from a scrutinee, not from the whole `if` body (`walk`)
      2  `online_count` is driven by explicit calls, not by an AP; it was
         never a live counter and should not have been in `LIVE`

The five real bugs those steps preserved are numastat, irqstat, sysfs (two
assertions), irqbalance and cpu_hotplug -- the last found by this file rather
than by hand, and it turned out to sit on a genuine defect in AP bringup
(`A-CPU-HOTPLUG-INIT-SNAPSHOTS-A-CPU-COUNT-THAT-CAN-STILL-GROW`). The 2 that
remain are one over-approximation in one function, recorded in the ledger with
the reason.

Both numbers are worth keeping. A gate is only as good as its false-positive
rate, and the first version's rate -- 98% -- is what a plausible-sounding
trigger looks like before anyone runs it.

What is checked, and what it cannot see
---------------------------------------
* The **counters** are a named list (`LIVE`), not a heuristic. Adding one is a
  deliberate act. The list is the point of judgement in this file.
* A **wrapper** counts as a read: a function in the same file whose body
  reaches a live counter, directly or through another same-file function.
  `irqbalance::stats` is a read of `smp::cpu_count` because it calls it.
* **Same file only.** Wrapper resolution does not cross modules, because bare
  function names collide across the ~600 files in `kernel/src` (`stats()` is
  defined in dozens of them) and a call graph keyed on the bare name would
  taint nearly every self-test in the tree. A self-test that reaches a live
  counter through *another* module's helper is therefore NOT caught. That is a
  real hole and it is left open on purpose: a gate that cries wolf is turned
  off, and all four instances found so far are intra-file.
* Only **equality** is a trigger: `assert_eq!`, `assert_ne!`, their
  `debug_` forms, and `assert!(a == b)` / `assert!(a != b)`. An ordering
  assertion across two reads is usually the correct way to write a race-free
  check -- irqstat rung 5 compares two genuinely different counters with `>=`
  precisely so that drift cannot fail it -- so flagging those would punish the
  fix.
* Taint follows `let` bindings only. A read stored into a struct field, sent
  through a closure, or written to a `static` is not followed.

The ledger
----------
`live-counter-ledger.txt`, keyed by `file::function`, each with a one-line
reason. Following 296, a legitimate site is recorded with its reason rather
than suppressed, so the file doubles as the list of every place in the kernel
that compares two readings of a live counter and why each is safe.

An entry naming a function that no longer trips is itself reported: that means
it was fixed (delete the line) or renamed (the entry now exempts something it
was never meant to).

Exit status: 0 clean, 1 unaccounted sites found, 2 bad usage.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
KERNEL = ROOT / "kernel" / "src"
LEDGER = pathlib.Path(__file__).resolve().parent / "live-counter-ledger.txt"

# --------------------------------------------------------------------------
# The counters that move without the test doing anything.
# --------------------------------------------------------------------------
# Keyed by the accessor's bare name; the module path is matched loosely so
# `crate::apic::tick_count()`, `apic::tick_count()` and a local `use` all hit.
#
# What belongs here: a counter advanced by an interrupt, by the clock, or by
# another CPU. What does not: anything only the calling test mutates -- those
# are the overwhelming majority of `*_count()` accessors in this tree, and
# including them would bury the handful that matter.
LIVE = {
    "tick_count": "APIC timer ticks -- advances on every timer interrupt",
    "vector_count": "per-vector interrupt count -- advances on every interrupt",
    "vector_counts": "the whole per-vector table -- advances on every interrupt",
    "elapsed_ns": "HPET wall clock",
    "elapsed_ms": "HPET wall clock",
    "cpu_count": "CPUs online -- an AP can bump this after smp::init returns",
    "processor_count": "ACPI MADT processors seen",
}

# Deliberately NOT in the list, recorded here because it was and was wrong:
#
# `online_count` -- both definitions of it (`cpu_hotplug`, `fs/cputopo`) are
#   backed by state that only an explicit `online`/`offline` call moves. No AP
#   bumps it; that is exactly the defect logged as
#   A-CPU-HOTPLUG-INIT-SNAPSHOTS-A-CPU-COUNT-THAT-CAN-STILL-GROW. So a
#   `cpu_hotplug::self_test` that offlines a CPU and then checks the count
#   *is* reading a counter twice, and is supposed to -- it is the counter the
#   test itself drives, which is the one distinction this whole file rests on.
#   Listing it turned the framework's own offline/online rung into a finding.
#
# The bare-name matching has the mirror-image cost: `cputopo::cpu_count()`
# reads a parsed topology table and is not live at all, but shares a name with
# `smp::cpu_count()`, which is. Nothing in the tree trips on that today. If
# something does, the answer is to qualify the key by module, not to drop the
# entry -- `smp::cpu_count` is the counter that caused four bugs in a day.

_FN = re.compile(r"\n[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?:unsafe[ \t]+)?fn[ \t]+(\w+)")
_CALL = re.compile(r"\b(\w+)\s*\(")
_IDENT = re.compile(r"\b([a-z_]\w*)\b")
_LET = re.compile(r"\blet\b")
_ASSERT = re.compile(
    r"\b(debug_assert_eq|debug_assert_ne|assert_eq|assert_ne|debug_assert|assert)\s*!\s*\("
)
_EQ_MACROS = {"assert_eq", "assert_ne", "debug_assert_eq", "debug_assert_ne"}
_RETURN = re.compile(r"\breturn\b")
_CLOSURE = re.compile(r"\|[^|(){}]*\|\s*")
_PAT_LET = re.compile(r"\b(?:if|while)\s+$")

# Call-graph levels the return-taint fixpoint will chase. See `resolve_returns`.
ROUNDS = 6


def strip_noise(src: str) -> str:
    """Blank out line comments, block comments and string literals.

    Comments in this tree quote code constantly -- this file's own docstring
    quotes `let t = totals();` -- so a scan that reads them finds calls that
    are not there. Newlines are preserved so line numbers stay true.
    """
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                out.append(" ")
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth = 1
            out.append("  ")
            i += 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth += 1
                    out.append("  ")
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    out.append("  ")
                    i += 2
                else:
                    out.append("\n" if src[i] == "\n" else " ")
                    i += 1
        elif c == '"':
            out.append(" ")
            i += 1
            while i < n:
                if src[i] == "\\":
                    out.append("  ")
                    i += 2
                    continue
                if src[i] == '"':
                    out.append(" ")
                    i += 1
                    break
                out.append("\n" if src[i] == "\n" else " ")
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def functions(src: str):
    """`(name, body, body_start, start_line)` for every `fn` with a braced body."""
    for m in _FN.finditer(src):
        i = src.find("{", m.end())
        if i < 0:
            continue
        # Refuse to walk past the next `fn`: a declaration with no body (a
        # trait method) would otherwise swallow the function after it.
        nxt = _FN.search(src, m.end())
        if nxt and nxt.start() < i:
            continue
        depth, j = 0, i
        while j < len(src):
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        yield m.group(1), src[i : j + 1], i, src[: m.start()].count("\n") + 2


def calls_in(body: str) -> set:
    """Every `name(` in `body`.

    One pass, rather than one regex per candidate: `kshell.rs` alone defines
    about 1500 functions, and `for callee in fns: re.findall(callee, body)`
    inside a fixpoint is cubic in that -- the first version of this file did
    exactly that and never finished.
    """
    return {m.group(1) for m in _CALL.finditer(body)}


def resolve_returns(fns: dict) -> dict:
    """`{function: live counters that reach its return value}`.

    # Why returning, and not merely reading

    The obvious rule -- a wrapper that touches a live counter anywhere in its
    body is a read of that counter -- is what this file did first, and it is
    far too coarse, because *timestamping* is everywhere. `certmgr::import_cert`
    reads the HPET only to stamp `created_ns` on the record it files; it then
    returns a `CertId`. Under the coarse rule every value that ever came out of
    `import_cert` was a clock reading, so

        assert_ne!(get_cert(c3)?.cert_path, get_cert(c4)?.cert_path)

    -- a comparison of two file paths -- was reported as comparing two readings
    of the wall clock. `dnssettings::resolve` was the same: it stamps a cache
    entry, so comparing the IP it returned against the IP it returned again
    looked like a clock comparison. Thirteen of the twenty first-pass findings
    were this, and every one was noise.

    Asking whether the read reaches the *return value* separates them cleanly,
    and it keeps all the real ones: `irqbalance::stats` copies `cpu_count` into
    the struct it returns, `sysfs::read_file` formats it into the string it
    returns, `irqstat::totals` sums the vector table into the struct it returns.
    Those are readings handed to the caller. A timestamp filed in a static is
    not.

    Fixpoint, because a wrapper may return another wrapper's result. `ROUNDS`
    bounds it: each pass propagates one call-graph level, and a chain of live
    readers deeper than that does not occur in this tree. Stopping early can
    only under-report, which is the direction a gate should fail in.
    """
    ret = {name: set() for name in fns}
    for _ in range(ROUNDS):
        changed = False
        for name, (body, _, _) in fns.items():
            grown = walk(body, ret)[1]
            if grown != ret[name]:
                ret[name] = grown
                changed = True
        if not changed:
            break
    return ret


def match_paren(src: str, i: int) -> int:
    """Index just past the `)` matching the `(` at `i` (or len(src))."""
    depth = 0
    while i < len(src):
        if src[i] in "([{":
            depth += 1
        elif src[i] in ")]}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return len(src)


def split_top(src: str, seps) -> list:
    """Split `src` on any of `seps` that occur at bracket depth zero."""
    parts, depth, last, i = [], 0, 0, 0
    while i < len(src):
        c = src[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif depth == 0:
            for s in seps:
                if src.startswith(s, i):
                    parts.append(src[last:i])
                    i += len(s)
                    last = i
                    break
            else:
                i += 1
                continue
            continue
        i += 1
    parts.append(src[last:])
    return parts


def split_eq(arg: str):
    """`(lhs, rhs)` around a top-level `==` or `!=`, else `None`.

    Ordering comparisons are deliberately not matched -- see the module
    docstring -- so `>=` and `<=` must not be mistaken for one, which is why
    the character before the operator is checked.
    """
    depth = 0
    for i in range(len(arg) - 1):
        c = arg[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif depth == 0 and arg[i : i + 2] in ("==", "!="):
            prev = arg[i - 1] if i else " "
            if prev in "=!<>":
                continue
            if arg[i + 2 : i + 3] == "=":
                continue
            return arg[:i], arg[i + 2 :]
    return None


def read_sites(body: str, reads: dict) -> list:
    """`(start, end, counters)` for every call that reads a live counter."""
    sites = []
    for m in _CALL.finditer(body):
        name = m.group(1)
        if name in LIVE:
            counters = {name}
        elif name in reads and reads[name]:
            counters = reads[name]
        else:
            continue
        sites.append((m.start(1), match_paren(body, m.end() - 1), counters))
    return sites


def block_tail(body: str, lo: int, hi: int):
    """Everything after the last top-level `;` of the block `body[lo:hi]`.

    `lo` indexes the `{`, `hi` is just past the matching `}`.
    """
    inner_hi = hi - 1
    depth, last, i = 0, -1, lo + 1
    while i < inner_hi:
        c = body[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif depth == 0 and c == ";":
            last = i
        i += 1
    start = last + 1 if last >= 0 else lo + 1
    return (start, inner_hi) if body[start:inner_hi].strip() else None


def tail_span(body: str):
    """The sub-expression that supplies the body's return value, if any.

    Two narrowings are applied, both because the un-narrowed answer is wrong in
    a way that produced false reports:

    * A tail that is itself a **block** yields the block's tail.
    * A tail that is a **call taking a closure** yields the closure's tail.
      `dnssettings::resolve` is `with_state(|state| { ...; Ok(ip) })` -- one
      expression at depth zero, so without this the whole closure counts as
      returned and the `let now = hpet::elapsed_ns();` inside it makes every IP
      the function ever returned a clock reading. The `with_*` idiom is
      pervasive in this tree, and a rule that cannot see through it cannot see
      most kernel state accessors at all.

    Anything else -- notably a tail `match` or `if` -- is left whole, so a read
    in any arm counts. That over-approximation is deliberate and is what keeps
    `sysfs::read_file`, whose live read sits in one arm of a large match.
    """
    t = block_tail(body, 0, len(body))
    if t is None:
        return None
    lo, hi = t
    for _ in range(4):  # bounded: `with_a(|x| with_b(|y| { .. }))` and no more
        while lo < hi and body[lo].isspace():
            lo += 1
        while hi > lo and body[hi - 1].isspace():
            hi -= 1
        if lo >= hi:
            return None
        if body[lo] == "{" and match_paren(body, lo) == hi:
            inner = block_tail(body, lo, hi)
            if inner is None:
                return None
            lo, hi = inner
            continue
        m = _CLOSURE.search(body, lo, hi)
        if m and body[hi - 1] == ")":
            b = body.find("{", m.end())
            if 0 <= b < hi and match_paren(body, b) <= hi:
                lo, hi = b, match_paren(body, b)
                continue
        break
    return (lo, hi)


def walk(body: str, returns_live: dict):
    """`(equality hits, counters reaching a return)` for one function body.

    Hits are `[(offset, counter)]`. Walks `let` bindings, `return`s and
    assertions in source order, carrying a taint map from local name to the set
    of `(counter, read id)` pairs that flowed into it, so that two views derived
    from *one* snapshot -- which is the fix, not the bug -- do not trip.
    """
    sites = read_sites(body, returns_live)
    taint = {}
    returned = set()

    def taint_of(lo, hi):
        """Every `(counter, read id)` reaching the expression at `[lo, hi)`."""
        out = set()
        for sid, (s, _e, counters) in enumerate(sites):
            if lo <= s < hi:
                out |= {(c, sid) for c in counters}
        for m in _IDENT.finditer(body[lo:hi]):
            out |= taint.get(m.group(1), set())
        return out

    events = [(m.start(), "let", m) for m in _LET.finditer(body)]
    events += [(m.start(), "assert", m) for m in _ASSERT.finditer(body)]
    events += [(m.start(), "return", m) for m in _RETURN.finditer(body)]
    events.sort(key=lambda e: (e[0], e[1]))

    hits = []
    for pos, kind, m in events:
        if kind == "return":
            end = _find_top(body, m.end(), ";")
            if end < 0:
                end = len(body) - 1
            returned |= {c for c, _ in taint_of(m.end(), end)}
            continue
        if kind == "let":
            eq = _find_top(body, m.end(), "=")
            if eq < 0:
                continue
            # `if let` / `while let` bind from a scrutinee that ends at the `{`,
            # not at a `;` -- there is no `;` until after the whole block. Left
            # to the statement rule below, the binding in
            # `if let Some(d) = ...find(..) { d.x = elapsed_ns(); return Ok(d.id); }`
            # absorbed the entire body of the `if`, which made `d` -- and so the
            # id every caller got back -- a reading of the clock.
            head = body[max(0, m.start() - 16) : m.start()]
            if _PAT_LET.search(head):
                end = _find_scrutinee_end(body, eq + 1)
                if end < 0:
                    continue
                init_end = end
            else:
                end = _find_top(body, eq + 1, ";")
                if end < 0:
                    continue
                els = body.find(" else ", eq, end)
                init_end = els if els > 0 else end
            flow = taint_of(eq + 1, init_end)
            for im in _IDENT.finditer(body[m.end() : eq]):
                if im.group(1) not in ("mut", "ref"):
                    taint[im.group(1)] = set(flow)
            continue

        open_paren = m.end() - 1
        close = match_paren(body, open_paren)
        inner = body[open_paren + 1 : close - 1]
        base = open_paren + 1
        args = split_top(inner, [","])
        if m.group(1) in _EQ_MACROS:
            if len(args) < 2:
                continue
            spans, off = [], base
            for a in args[:2]:
                spans.append((off, off + len(a)))
                off += len(a) + 1
        else:
            halves = split_eq(args[0])
            if halves is None:
                continue
            spans = [
                (base, base + len(halves[0])),
                (base + len(halves[0]) + 2, base + len(args[0])),
            ]

        sides = [taint_of(lo, hi) for lo, hi in spans]
        by_counter = [{}, {}]
        for k, side in enumerate(sides):
            for c, sid in side:
                by_counter[k].setdefault(c, set()).add(sid)
        # Sorted, not set order: a gate whose output changes between runs on
        # the same tree cannot be diffed, and Python's string hashing is seeded
        # per process, so an assertion touching two counters would name a
        # different one each run.
        for c in sorted(by_counter[0]):
            left = by_counter[0][c]
            right = by_counter[1].get(c)
            if right and (left | right) - (left & right):
                hits.append((pos, c))
                break

    tail = tail_span(body)
    if tail:
        returned |= {c for c, _ in taint_of(*tail)}
    return hits, returned


def _find_scrutinee_end(src: str, i: int) -> int:
    """Index of the `{` opening an `if let` / `while let` body, else -1.

    Cannot use `_find_top`, which counts `{` as an opening bracket and so can
    never report one: here the brace is the terminator, and only `(` and `[`
    nest inside the scrutinee.
    """
    depth = 0
    while i < len(src):
        c = src[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            if depth == 0:
                return -1
            depth -= 1
        elif depth == 0 and c == "{":
            return i
        elif depth == 0 and c == ";":
            return -1
        i += 1
    return -1


def _find_top(src: str, i: int, ch: str) -> int:
    """Index of the next `ch` at bracket depth zero from `i`, else -1."""
    depth = 0
    while i < len(src):
        c = src[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return -1
            depth -= 1
        elif depth == 0 and c == ch:
            return i
        i += 1
    return -1


def load_ledger():
    if not LEDGER.exists():
        return {}
    out = {}
    for line in LEDGER.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        key, _, reason = s.partition("  ")
        out[key.strip()] = reason.strip()
    return out


def main() -> int:
    if not KERNEL.is_dir():
        print(f"error: {KERNEL} is not a directory", file=sys.stderr)
        return 2

    ledger = load_ledger()
    findings = []  # (key, rel, line, {counter})
    for path in sorted(KERNEL.rglob("*.rs")):
        src = strip_noise(path.read_text(encoding="utf-8", errors="replace"))
        fns = {}
        for name, body, start, line in functions(src):
            fns[name] = (body, start, line)
        if not any(n.startswith("self_test") for n in fns):
            continue
        returns_live = resolve_returns(fns)
        rel = str(path.relative_to(ROOT)).replace("\\", "/")
        for name, (body, start, _line) in fns.items():
            if not name.startswith("self_test"):
                continue
            for off, counter in walk(body, returns_live)[0]:
                line = src[: start + off].count("\n") + 1
                findings.append((f"{rel}::{name}", rel, line, counter))

    seen = {f[0] for f in findings}
    unaccounted = [f for f in findings if f[0] not in ledger]
    stale = [k for k in ledger if k not in seen]

    for key, rel, line, counter in unaccounted:
        fn = key.split("::")[1]
        print(
            f"{rel}:{line}: {fn} compares two readings of `{counter}' "
            f"({LIVE[counter]})"
        )
        print("    A live counter moves between the two reads, so this")
        print("    assertion holds only if nothing happened in between. Take")
        print("    ONE snapshot and derive both sides from it -- see")
        print("    kernel/src/fs/irqstat.rs, fixed this way after it panicked")
        print("    the kernel over a single timer tick. If the two readings")
        print("    are genuinely meant to be independent, add a line to")
        print(f"    {LEDGER.name} with the reason.")

    for key in sorted(stale):
        print(f"{LEDGER.name}: `{key}' no longer compares two live readings.")
        print("    Delete the line if it was fixed; correct it if the function")
        print("    was renamed, since the entry now exempts something else.")

    if unaccounted or stale:
        return 1

    print(
        f"[live-counter] no self-test compares two readings of one live counter "
        f"({len(ledger)} reviewed comparison(s) carried with reasons)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
