#!/usr/bin/env python3
"""Find lock guards held across a call that re-acquires the same lock.

This is the cross-function form of the bug that froze a boot in
`fs::encrypt::encrypt`:

    STATE.lock().files_encrypted = STATE.lock().files_encrypted + 1

That one is visible to a grep, because both acquires are in one statement.  The
form this script exists for is not::

    pub fn set_policy(..) {
        let mut state = STATE.lock();
        state.rules.push(..);
        refresh_cache();          // <-- and refresh_cache() takes STATE.lock()
    }

Both deadlock identically on a non-reentrant spinlock, and the second is
invisible to every text search, because no single line mentions the lock twice.

Scope and honesty about it
--------------------------
This is a *heuristic within one file*.  It resolves calls only to functions
defined in the same file, and locks only via ALL-CAPS static receivers.  That
covers the module-private `static STATE: Mutex<..>` pattern this kernel uses
everywhere, which is where the risk actually lives, and it keeps the analysis
free of any need to resolve imports or trait dispatch.

It therefore has false negatives by construction (a call into another module
that reaches back is not seen).  It aims to have few false positives, and every
report is meant to be read by a human before anything is changed:

  * a guard explicitly `drop()`ed before the call is not reported;
  * a guard whose enclosing block ends before the call is not reported;
  * `try_lock()` is never reported -- it cannot deadlock, it returns None.

Exit codes: 0 clean, 1 findings, 2 could not run.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# A lock acquisition on a static receiver: STATE.lock(), TABLE.lock_irqsave().
# try_lock() is deliberately excluded -- it returns None rather than spinning,
# so re-entering it is not a deadlock.
ACQUIRE = re.compile(r"\b([A-Z][A-Z0-9_]{2,})\s*\.\s*(lock|lock_irqsave)\s*\(\s*\)")

# `let g = STATE.lock();` / `let mut g = STATE.lock();` -- a *named* guard, which
# is the only kind that can still be alive when a later call runs.
BINDING = re.compile(
    r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*=\s*([A-Z][A-Z0-9_]{2,})\s*\.\s*"
    r"(?:lock|lock_irqsave)\s*\(\s*\)\s*;"
)

FN_DEF = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\s*[(<]")
CALL = re.compile(r"(?<![\w:.])([a-z_][a-z0-9_]*)\s*\(")
DROP = re.compile(r"\bdrop\s*\(\s*([a-z_][a-z0-9_]*)\s*\)")


def strip_noise(src: str) -> str:
    """Blank out comments and string/char literals, preserving byte offsets.

    Offsets must be preserved because every later step reports and slices by
    index; replacing rather than deleting keeps line numbers exact.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                out[i] = " "
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth = 1
            out[i] = out[i + 1] = " "
            i += 2
            while i < n and depth:
                if src[i] == "/" and i + 1 < n and src[i + 1] == "*":
                    depth += 1
                    out[i] = out[i + 1] = " "
                    i += 2
                    continue
                if src[i] == "*" and i + 1 < n and src[i + 1] == "/":
                    depth -= 1
                    out[i] = out[i + 1] = " "
                    i += 2
                    continue
                if src[i] != "\n":
                    out[i] = " "
                i += 1
        elif c == '"':
            out[i] = " "
            i += 1
            while i < n:
                if src[i] == "\\":
                    out[i] = " "
                    if i + 1 < n and src[i + 1] != "\n":
                        out[i + 1] = " "
                    i += 2
                    continue
                if src[i] == '"':
                    out[i] = " "
                    i += 1
                    break
                if src[i] != "\n":
                    out[i] = " "
                i += 1
        else:
            i += 1
    return "".join(out)


def find_bodies(src: str) -> dict[str, tuple[int, int]]:
    """Map each function name in the file to its body's [start, end) offsets.

    Nested functions and same-named methods in different impl blocks both occur
    here; a later definition simply wins, which is acceptable for a heuristic
    whose output is hand-checked.
    """
    bodies: dict[str, tuple[int, int]] = {}
    for m in FN_DEF.finditer(src):
        name = m.group(1)
        # Walk forward to the body's opening brace, skipping the parameter list,
        # return type and any where-clause. A `{` at paren/bracket depth 0 is it.
        i = m.end() - 1
        depth_paren = 0
        depth_angle = 0
        start = None
        while i < len(src):
            ch = src[i]
            if ch in "([":
                depth_paren += 1
            elif ch in ")]":
                depth_paren -= 1
            elif ch == "<":
                depth_angle += 1
            elif ch == ">":
                depth_angle = max(0, depth_angle - 1)
            elif ch == ";" and depth_paren <= 0:
                break  # a trait method declaration with no body
            elif ch == "{" and depth_paren <= 0:
                start = i
                break
            i += 1
        if start is None:
            continue
        depth = 0
        j = start
        while j < len(src):
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    bodies[name] = (start + 1, j)
                    break
            j += 1
    return bodies


def direct_locks(body: str) -> set[str]:
    return {m.group(1) for m in ACQUIRE.finditer(body)}


def called_names(body: str, known: set[str]) -> set[str]:
    return {m.group(1) for m in CALL.finditer(body) if m.group(1) in known}


def transitive_locks(
    fn: str, bodies: dict[str, tuple[int, int]], src: str, memo: dict[str, set[str]],
    stack: tuple[str, ...] = (),
) -> set[str]:
    """Locks `fn` may acquire, directly or through same-file callees."""
    if fn in memo:
        return memo[fn]
    if fn in stack:
        return set()  # recursion in the call graph; the fixpoint handles it
    span = bodies.get(fn)
    if span is None:
        return set()
    body = src[span[0] : span[1]]
    acc = direct_locks(body)
    for callee in called_names(body, set(bodies)):
        if callee == fn:
            continue
        acc |= transitive_locks(callee, bodies, src, memo, stack + (fn,))
    memo[fn] = acc
    return acc


def block_end(src: str, pos: int, limit: int) -> int:
    """Offset at which the block containing `pos` closes (or `limit`)."""
    depth = 0
    i = pos
    while i < limit:
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            if depth == 0:
                return i
            depth -= 1
        i += 1
    return limit


def analyse(path: Path) -> list[str]:
    raw = path.read_text(encoding="utf-8", errors="replace")
    src = strip_noise(raw)
    bodies = find_bodies(src)
    if not bodies:
        return []
    memo: dict[str, set[str]] = {}
    known = set(bodies)
    findings: list[str] = []

    for fn, (bstart, bend) in sorted(bodies.items()):
        body = src[bstart:bend]
        for bind in BINDING.finditer(body):
            guard, lock = bind.group(1), bind.group(2)
            # The guard is live from just after the binding until either an
            # explicit drop(guard) or the end of its enclosing block.
            live_from = bstart + bind.end()
            live_to = block_end(src, live_from, bend)
            region = src[live_from:live_to]
            d = DROP.search(region)
            if d and d.group(1) == guard:
                region = region[: d.start()]
            for callee in sorted(called_names(region, known)):
                if callee == fn:
                    continue
                if lock in transitive_locks(callee, bodies, src, memo):
                    line = raw.count("\n", 0, bstart + bind.start()) + 1
                    findings.append(
                        f"{path}:{line}: `{fn}` holds `{lock}` in `{guard}` and then "
                        f"calls `{callee}`, which acquires `{lock}` again"
                    )
    return findings


def main() -> int:
    root = Path(__file__).resolve().parent.parent / "kernel" / "src"
    if not root.is_dir():
        print(f"error: no such directory: {root}", file=sys.stderr)
        return 2
    findings: list[str] = []
    files = 0
    for path in sorted(root.rglob("*.rs")):
        files += 1
        try:
            findings.extend(analyse(path))
        except (OSError, ValueError) as exc:  # keep going; report at the end
            print(f"warning: {path}: {exc}", file=sys.stderr)
    for f in findings:
        print(f)
    print(
        f"\nscanned {files} file(s); "
        f"{len(findings)} guard(s) held across a re-acquiring call",
        file=sys.stderr,
    )
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
