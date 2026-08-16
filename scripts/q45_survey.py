"""Survey every `RenderCommand::Text {` site before Q45 edits a single one.

Classifies each occurrence as a construction (needs the new `overflow` field)
or a pattern (does not), and reports how `FontWeightHint` — a name every
construction already mentions — is imported, since `TextOverflow` has to arrive
by the same route.

Read-only. Run from the lane-c worktree root.
"""

import collections
import io
import os
import re
import sys

ROOTS = ["gui", "apps", "net", "netscan", "pkg"]
NEEDLE = "RenderCommand::Text {"


def rs_files():
    for root in ROOTS:
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
            for name in filenames:
                if name.endswith(".rs"):
                    yield os.path.join(dirpath, name)


def block_end(text, open_idx):
    """Index just past the `}` matching the `{` at open_idx. Brace counting is
    enough here: these blocks hold field values, and the only braces inside are
    balanced (closures, nested struct literals, format strings are quoted)."""
    depth = 0
    i = open_idx
    in_str = False
    in_char = False
    while i < len(text):
        c = text[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
        elif in_char:
            if c == "\\":
                i += 2
                continue
            if c == "'":
                in_char = False
        elif c == '"':
            in_str = True
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return -1


def main():
    kinds = collections.Counter()
    imports = collections.Counter()
    odd = []
    total_files = 0

    for path in rs_files():
        with io.open(path, encoding="utf-8", newline="") as f:
            src = f.read()
        if NEEDLE not in src:
            continue
        total_files += 1

        for m in re.finditer(re.escape(NEEDLE), src):
            open_idx = src.index("{", m.start())
            end = block_end(src, open_idx)
            body = src[open_idx + 1 : end - 1]
            line = src.count("\n", 0, m.start()) + 1
            mw = re.search(r"(?<![\w.])max_width\s*:\s*(None|Some)", body)
            if mw:
                kinds["construct_" + mw.group(1)] += 1
            elif ".." in body:
                kinds["pattern"] += 1
            elif "max_width" in body:
                kinds["pattern_exhaustive"] += 1
                odd.append("%s:%d exhaustive pattern" % (path, line))
            else:
                kinds["unknown"] += 1
                odd.append("%s:%d %r" % (path, line, body[:120]))

        for im in re.finditer(r"^\s*(pub )?use ([^;]*FontWeightHint[^;]*);", src, re.M):
            imports[re.sub(r"\s+", " ", im.group(0).strip())] += 1
        if "FontWeightHint" not in src:
            odd.append("%s: constructs Text but never names FontWeightHint" % path)

    print("files touched: %d" % total_files)
    for k, v in sorted(kinds.items()):
        print("  %-22s %d" % (k, v))
    print("\ndistinct FontWeightHint import forms: %d" % len(imports))
    for k, v in imports.most_common(20):
        print("  %5d  %s" % (v, k))
    print("\nneeds a human: %d" % len(odd))
    for o in odd[:40]:
        print("  " + o)


if __name__ == "__main__":
    sys.exit(main())
