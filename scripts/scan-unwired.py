#!/usr/bin/env python3
"""Find work that no path from `main` reaches, but a test does.

The shape this looks for is lesson 45 in known-issues.md: a function that does
real work, has a test proving it works, and is never called by the program.
`apps/indexer`'s `index_file_content` was exactly this -- content search had a
config option, a search path and a green test, and could not be reached.

**Reachability from `main`, not "has a non-test caller".**  The first version
of this script asked only whether some line outside `#[cfg(test)]` mentioned
the function, and that is too weak: a helper called only by another unwired
function has a production caller and is still unreachable.  It is also the
question that produces the noise, because a stub `main` makes every function
in the file callerless in exactly the same way, which is a *crate*-level fact
reported 335 times.  Walking the call graph forward from `main` answers both
at once.

A hit is not automatically a bug.  Three benign explanations are common:

  * the function is `pub` in a library crate and the caller is another crate;
  * it is a trait method or an interface implemented for future use;
  * it is genuinely dead code that should be deleted.

Only the fourth kind matters: a function that some *option or command* claims
to invoke and does not.  Triage by asking what user-visible promise depends on
it.
"""

import pathlib
import re
import sys

# Lane C's tree.  Root-agnostic otherwise.
ROOTS = ["gui", "apps", "pkg"]

FN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+"
    r"([a-z_][a-z0-9_]*)"
)

# Names whose callers are the language, a derive, or another crate, so
# "not reached from main" says nothing.
EXEMPT = {
    "main", "new", "default", "fmt", "drop", "from", "clone", "eq", "ne",
    "hash", "cmp", "partial_cmp", "next", "deref", "deref_mut", "as_ref",
    "borrow", "borrow_mut", "into_iter", "try_from", "from_str", "add", "sub",
    "mul", "div", "neg", "not", "index", "index_mut", "poll", "serialize",
    "deserialize",
}

# A `main` that reaches less than this share of its own binary is not an entry
# point, it is a placeholder -- `fn main() { let _app = TetrisApp::new(); }`.
# Sixty of lane C's 140 app binaries are in that state, which is one fact
# (`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`) and not one finding per function;
# reported per function it was 335 of the 338 lines the first run printed.
#
# A count threshold does not work here and was tried: `Yahtzee::new()` alone
# reaches three functions, so a stub clears any small fixed bar.  The share is
# the property that actually distinguishes the two -- a real `main` reaches
# most of the program, a placeholder reaches its constructor and stops.
STUB_MAIN_SHARE = 0.25


def test_spans(lines):
    """Line ranges (0-based, inclusive) covered by a `#[cfg(test)]` item."""
    spans = []
    for i, line in enumerate(lines):
        if "cfg(test)" not in line:
            continue
        depth = 0
        started = False
        j = i
        while j < len(lines):
            depth += lines[j].count("{") - lines[j].count("}")
            if "{" in lines[j]:
                started = True
            if started and depth <= 0:
                break
            j += 1
        spans.append((i, j))
    return spans


def in_spans(idx, spans):
    return any(a <= idx <= b for a, b in spans)


def block_end(lines, idx):
    """Last line (0-based, inclusive) of the block opened at or after `idx`."""
    depth = 0
    started = False
    j = idx
    while j < len(lines):
        depth += lines[j].count("{") - lines[j].count("}")
        if "{" in lines[j]:
            started = True
        if started and depth <= 0:
            return j
        j += 1
    return len(lines) - 1


def enclosing_block(lines, idx):
    """The line that opens the block `idx` sits directly inside, or None.

    Walks backwards accumulating braces.  The first line that leaves more `{`
    open than it closed is the one that opened this block; `impl`/`fn`/`mod`
    headers are all found this way, without needing to parse Rust.
    """
    depth = 0
    for j in range(idx - 1, -1, -1):
        depth += lines[j].count("}") - lines[j].count("{")
        if depth < 0:
            return j
    return None


def in_trait_impl(lines, idx):
    """True if `idx` is a method of an `impl Trait for Type` block.

    Such a method is called *through the trait*, by generic code in some other
    file that this file-local scan cannot see, so "not reached from main" says
    nothing.  An inherent `impl Type` block is deliberately *not* excluded: a
    private method there is reachable only from this file, which is the
    property the whole scan rests on.
    """
    j = enclosing_block(lines, idx)
    if j is None:
        return False
    # The header may be split over lines (`impl<T>\n    Trait for Type`), so
    # look at a small window rather than the single line.
    header = " ".join(lines[max(0, j - 2) : j + 1])
    if not re.search(r"\bimpl\b", header):
        return False
    return re.search(r"\bimpl\b[^;{]*\bfor\b", header) is not None


def call_graph(lines, spans):
    """`{name: (defline, callee_names)}` for every fn defined outside tests.

    Callee edges are name-based, so two methods with one name on different
    types merge into a single node.  That errs toward calling a function
    *reachable*, which is the safe direction: this script's output is read by
    hand and a missed finding costs less than a list nobody reads.
    """
    bodies = {}
    for i, line in enumerate(lines):
        m = FN.match(line)
        if not m or in_spans(i, spans):
            continue
        # *Every* definition of the name, not the first.  `apps/indexer` has
        # two `fn serialize` -- `Config`'s at line 287 and `FileIndex`'s at 605
        # -- and keeping only the first made the second's whole body invisible,
        # so `FileType::as_byte`, called from inside it, was reported as
        # unreachable while sitting in a function that runs on every save.
        bodies.setdefault(m.group(1), []).append((i, block_end(lines, i)))

    names = set(bodies)
    graph = {}
    for name, occurrences in bodies.items():
        callees = set()
        for start, end in occurrences:
            for j in range(start + 1, min(end + 1, len(lines))):
                for other in mentions(lines[j]):
                    if other in names and other != name:
                        callees.add(other)
        graph[name] = (occurrences[0][0], callees)
    return graph


# A name used as a *value* rather than called: `or_else(card_from_env)`,
# `map(ics_unescape)`, `unwrap_or_else(quick_scan_ports)`, a function pointer
# in a dispatch table.  Rust reaches the body just as surely as `f()` does, and
# looking only for `name(` cannot see any of it.
#
# This is not a hypothetical gap.  `gui/compositor`'s `card_from_value` was
# reported unreachable because its one production caller, `card_from_env`, is
# named at `options.card.or_else(card_from_env)` -- so the *caller* looked dead
# and took its callee down with it.  `apps/calendar`'s `ics_unescape`
# (`map(ics_unescape)`) and `apps/netscan`'s `quick_scan_ports` were the same
# defect wearing different idioms.  Three of the report's highest-reach
# findings, all false.
#
# `.` excluded on the left so `entry.name` does not credit a `fn name`; `(`,
# `:` and `!` on the right so a call, a path segment (`mod::f`) and a macro are
# left to the call rule or ignored.  A local variable that shares a function's
# name will make a spurious edge and mark a dead function live -- that is the
# direction this script prefers to be wrong in, per the note above.
IDENT = re.compile(r"(?<![.\w])([a-z_][a-z0-9_]*)\b(?![\s]*[(:!])")
CALL = re.compile(r"\b([a-z_][a-z0-9_]*)\s*\(")


def mentions(line):
    """Every identifier on `line` that could name a function being reached."""
    return CALL.findall(line) + IDENT.findall(line)


def reachable_from(graph, root):
    if root not in graph:
        return set()
    seen = {root}
    stack = [root]
    while stack:
        for callee in graph[stack.pop()][1]:
            if callee not in seen:
                seen.add(callee)
                stack.append(callee)
    return seen


def scan_file(path):
    """Scan one binary crate's `main.rs`.

    Returns `(reached, total, findings)`, where `findings` is a list of
    `(line, name, test_calls)` for private non-trait functions that tests
    exercise and no path from `main` reaches.  `None` if there is nothing to
    say about the file at all.
    """
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    lines = text.split("\n")
    spans = test_spans(lines)
    if not spans:
        return None

    graph = call_graph(lines, spans)
    if "main" not in graph or not graph:
        return None
    live = reachable_from(graph, "main")

    out = []
    for name, (defline, _) in graph.items():
        if name in live or name in EXEMPT:
            continue
        # `pub`, `pub(crate)`, `pub(super)` -- all reachable from elsewhere.
        if re.match(r"^\s*pub\b", lines[defline]):
            continue
        if in_trait_impl(lines, defline):
            continue
        call = re.compile(r"\b" + re.escape(name) + r"\s*\(")
        tests = sum(
            1
            for i, line in enumerate(lines)
            if i != defline and in_spans(i, spans) and call.search(line)
        )
        if tests:
            out.append((defline + 1, name, tests))
    return (len(live), len(graph), sorted(out))


def main():
    roots = list(ROOTS)
    base = pathlib.Path(".")
    roots += [p.name for p in base.iterdir() if p.is_dir() and p.name.startswith("net")]

    binaries = []
    for root in sorted(set(roots)):
        rp = base / root
        if not rp.is_dir():
            continue
        for f in sorted(rp.rglob("*.rs")):
            if "target" in f.parts:
                continue
            # Binary crates only: a private fn in a `main.rs` has nowhere else
            # its callers could be hiding.  A private fn in a lib's module can
            # be called by a sibling module, which this file-local scan would
            # not see and would report as a false positive.
            if f.name != "main.rs":
                continue
            result = scan_file(f)
            if result is None:
                continue
            reached, total, findings = result
            if findings:
                binaries.append((reached / total, reached, total, f.as_posix(), findings))

    # Highest reach share last, because that end of the list is the one worth
    # reading and a terminal shows the end of it.  A binary whose `main` reaches
    # nearly everything and strands three tested functions is the `apps/indexer`
    # shape; one that reaches a tenth of itself has no entry point at all, which
    # is a single known debt and not a list of findings.
    binaries.sort()
    stranded = 0
    for share, reached, total, path, findings in binaries:
        print(f"\n{path}  --  main reaches {reached}/{total} fns ({share:.0%})")
        for line_no, name, count in findings:
            print(f"  :{line_no}: fn {name} -- {count} test call(s), no path from main")
            stranded += 1

    print(f"\n{stranded} stranded function(s) across {len(binaries)} binary/binaries.")
    print(
        "Read from the bottom: a low reach share means the binary has no entry"
        " point at all\n(one known debt, not one finding per function); a high"
        " one means the program runs and\nthese particular functions are what it"
        " never calls."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
