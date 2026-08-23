"""Resolve, for every line of a Rust file, the item scopes enclosing it.

Why this exists
---------------
Sorting a warning backlog into "production" and "test" is only useful if the
sort is right, and the obvious way to do it is wrong.  The obvious way is a
line-anchored regex: remember the last ``fn NAME`` seen above the line, call
that the context.  That attributes every site to the *innermost* function,
which is exactly backwards for this codebase.

The kernel is a ``no_std`` binary, so ``cargo test`` never runs and its tests
are ordinary ``pub fn self_test()`` functions.  Those routinely wrap each case
in a nested helper::

    pub fn self_test() -> KernelResult<()> {
        {
            #[inline(never)]
            fn case() -> KernelResult<()> {
                serial_println!("progmgr::self_test 4: notifications");
                let x = thing.unwrap();     // <- test code
            }
        }
    }

An innermost-match classifier reports ``fn case`` for that ``unwrap`` and files
it under production, and there are thousands of such lines.  The number it
produces is not slightly off; it is wrong in the direction that makes the
backlog look scarier than it is, which is the direction that wastes the most
effort.  What the question "is this test code?" actually asks about is the
*outermost* enclosing function -- the one that has a name a human chose to
describe the whole thing.

So this module tracks a scope *stack* rather than a last-seen name.

Doing that means counting braces, and counting braces means lexing, because
kernel source is full of braces that are not delimiters:

    serial_println!("progmgr: {} of {}", i, n);   // format placeholders
    let open = '{';                               // char literal
    // } in a comment
    r#"raw string with " and { inside"#           // raw string

A brace counter that does not know about strings, char literals, raw strings
and comments desynchronises on the first ``println!`` and never recovers.  The
scanner below handles all four.  It does not attempt to be a Rust parser -- it
does not need to be, because every construct that can *nest* a scope also opens
a brace, so brace depth is sufficient to know where a scope ends.

Known limits (all benign for classification):

  * A trait method declared without a body (``fn f(&self);``) is discarded when
    the ``;`` is reached, but a ``;`` inside the parameter list of a
    *multi-line* signature would discard it early.  Rust has no such syntax.
  * Nested block comments are counted, matching rustc.
  * A lifetime (``'a``) is distinguished from a char literal by lookahead, the
    same way rustc's lexer does it.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

# `fn NAME`, `mod NAME`, `impl ...`. Only the first two carry a useful name;
# `impl` blocks are tracked purely so their braces balance and so a
# `#[cfg(test)]` on one is seen.
ITEM_RE = re.compile(
    r"\b(?:(?P<fn>fn)|(?P<mod>mod))\s+(?P<name>\w+)"
)
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\(\s*(?:.*\b)?test\b")


@dataclass
class Scope:
    """One brace-delimited item, while it is open."""

    kind: str  # "fn" | "mod"
    name: str
    depth: int  # brace depth *outside* this scope's braces
    cfg_test: bool


def _strip(line: str, state: dict) -> str:
    """The delimiter-significant characters of `line`, comments and literals gone.

    `state` carries the lexer's position across lines: whether we are inside a
    block comment, a string, or a raw string, and at what hash count.  Every
    character that survives is either a brace, a semicolon, or part of an
    identifier -- which is all the caller looks at.
    """
    out = []
    i = 0
    n = len(line)
    while i < n:
        # --- continuing a multi-line construct from a previous line ---
        if state["block_comment"]:
            end = line.find("*/", i)
            # Nested block comments: rustc counts them, so we do too.
            start = line.find("/*", i)
            if start != -1 and (end == -1 or start < end):
                state["block_comment"] += 1
                i = start + 2
                continue
            if end == -1:
                break
            state["block_comment"] -= 1
            i = end + 2
            continue

        if state["raw_hashes"] is not None:
            close = '"' + "#" * state["raw_hashes"]
            end = line.find(close, i)
            if end == -1:
                break
            state["raw_hashes"] = None
            i = end + len(close)
            continue

        if state["in_string"]:
            while i < n:
                c = line[i]
                if c == "\\":
                    i += 2
                    continue
                if c == '"':
                    state["in_string"] = False
                    i += 1
                    break
                i += 1
            else:
                # Unterminated: a string continued by a trailing backslash.
                break
            continue

        c = line[i]

        # --- starting something ---
        if c == "/" and i + 1 < n:
            if line[i + 1] == "/":
                break  # line comment: nothing after it matters
            if line[i + 1] == "*":
                state["block_comment"] = 1
                i += 2
                continue

        if c == '"':
            state["in_string"] = True
            i += 1
            continue

        # Raw string: r"..." or r#"..."# (any number of hashes).
        if c in "rb" and i + 1 < n:
            j = i + 1
            if line[j] == '"' or line[j] == "#":
                k = j
                while k < n and line[k] == "#":
                    k += 1
                if k < n and line[k] == '"':
                    state["raw_hashes"] = k - j
                    i = k + 1
                    continue

        if c == "'":
            # Char literal or lifetime? A char literal is `'x'` or `'\x'`,
            # so the quote three-or-four characters along decides it.
            if i + 2 < n and line[i + 1] == "\\":
                end = line.find("'", i + 2)
                if end != -1:
                    i = end + 1
                    continue
            elif i + 2 < n and line[i + 2] == "'":
                i += 3
                continue
            # Otherwise a lifetime; fall through and emit it harmlessly.

        out.append(c)
        i += 1
    return "".join(out)


def scope_stack_per_line(lines: list[str]) -> list[list[Scope]]:
    """The stack of open item scopes at the *start* of each line.

    Index `i` of the result corresponds to `lines[i]`, and `[0]` of each stack
    is the outermost item -- the one to classify by.
    """
    state = {"block_comment": 0, "in_string": False, "raw_hashes": None}
    stack: list[Scope] = []
    depth = 0
    pending: Scope | None = None
    cfg_test_pending = False
    per_line: list[list[Scope]] = []

    for line in lines:
        per_line.append(list(stack))

        # `#[cfg(test)]` applies to the next item, and attributes may sit
        # several lines above it (`#[cfg(test)]` then `#[allow(..)]` then
        # `mod tests {`), so the flag survives until an item consumes it.
        if CFG_TEST_RE.search(line):
            cfg_test_pending = True

        code = _strip(line, state)

        pos = 0
        while pos < len(code):
            c = code[pos]
            if c == "{":
                if pending is not None:
                    stack.append(pending)
                    pending = None
                    cfg_test_pending = False
                depth += 1
                pos += 1
                continue
            if c == "}":
                depth = max(0, depth - 1)
                while stack and stack[-1].depth >= depth:
                    stack.pop()
                pos += 1
                continue
            if c == ";":
                # A bodiless declaration (trait method, `mod foo;`).
                pending = None
                pos += 1
                continue
            # Look for an item keyword starting here.
            m = ITEM_RE.match(code, pos)
            if m and (pos == 0 or not (code[pos - 1].isalnum() or code[pos - 1] == "_")):
                kind = "fn" if m.group("fn") else "mod"
                pending = Scope(kind, m.group("name"), depth, cfg_test_pending)
                pos = m.end()
                continue
            pos += 1

    return per_line


# Production functions whose names contain "test" and would otherwise be exempt
# from every check keyed on the naming convention. A panic in any of these takes
# down a running machine, so they are held to the production standard despite
# the name. Keep the reason with the entry.
#
# This lives here, not in the scripts that consume it, because there is exactly
# one question being asked -- "is this test code?" -- and two answers to it
# drift apart the moment one is edited. `scan-unwrap.py` gates the build on this
# and `clippy-sites.py` reports on it; they must not disagree about whether
# `kshell::eval_test` is production.
NOT_TESTS = {
    # The shell's `test` / `[` builtin. Parses a user-supplied expression and
    # runs on every `[ -f "$x" ]` a script executes.
    "eval_test",
    # kshell `memtest` command: user-supplied size and pattern arguments.
    "cmd_memtest",
    # kshell command that plays a tone; user-supplied frequency and duration.
    "play_test_tone",
}


def is_test_name(name: str) -> bool:
    """Does this function name mark test code?

    Substring rather than prefix/suffix, because the conventions in this tree
    are `self_test`, `test_foo`, `foo_test`, `selftest` and `build_foo_test_elf`
    all at once, and enumerating them invites the next one to be missed.
    `NOT_TESTS` carries the handful of production names that collide.

    The name is only a proxy for the real question -- whether the function is
    reachable from anything but the boot self-test suite -- and it cannot
    separate `eval_test` (a shell builtin) from `tx_datapath_test` (a driver
    self-test) on the strength of the name alone. Answering properly needs a
    call graph. The list is what stands in for one, and it is deliberately a
    list: an exception with a comment stays visible to the next reader, which
    a cleverer regex does not.
    """
    return "test" in name.lower() and name not in NOT_TESTS


def classify(stack: list[Scope], path: str) -> str:
    """Bucket a site: `test`, `self-test`, or `production`.

    * `test` -- inside a `#[cfg(test)]` item, or a file that only exists to be
      compiled as a test or benchmark.
    * `self-test` -- inside a kernel self-test or one of its fixture builders.
      These are this codebase's real tests: `cargo test` cannot run in a
      `no_std` binary, so they are ordinary functions the kernel calls at boot,
      and the fixture builders (`build_*_test_elf` and friends) are test code
      by every measure except where they happen to be defined.
    * `production` -- everything else.

    The verdict is taken over the *whole* stack, not the outermost frame alone,
    for the reason in this module's header: a helper defined inside a
    `self_test` is test code however it is named.
    """
    posix = path.replace("\\", "/")
    if (
        "/tests/" in posix
        or posix.startswith("tests/")
        or "/benches/" in posix
        or posix.startswith("bench/")
        or posix.endswith("build.rs")
    ):
        return "test"
    if any(s.cfg_test for s in stack):
        return "test"
    if any(s.kind == "mod" and s.name in ("tests", "test") for s in stack):
        return "test"
    if any(is_test_name(s.name) for s in stack):
        return "self-test"
    return "production"
