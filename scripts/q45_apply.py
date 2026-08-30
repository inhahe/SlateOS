"""Q45: give every `RenderCommand::Text` construction an explicit `overflow`.

design-decisions.md §427. A site that already bounds its width
(`max_width: Some(..)`) gets `TextOverflow::Ellipsis`, because a silent cut is
the bug being fixed; an unbounded site (`max_width: None`) gets
`TextOverflow::Clip`, where the choice is vacuous.

Scripted rather than hand-edited because there are ~4200 of them across 208
files. Sites the classifier is not sure about are left alone and listed, so the
residue is handled by a human rather than by a guess.

Read-write. Run once, from the lane-c worktree root. Reports what it changed.
"""

import io
import os
import re
import sys

# `init` is lane B's tree, and is here because a required field cannot be added
# by one lane and filled in by another: the commit that adds it must also fill
# every construction of it, or the workspace does not build in between. See
# `requests/c-b-render-text-gained-a-required-field.md`.
ROOTS = ["gui", "apps", "net", "netscan", "pkg", "init"]
NEEDLE = "RenderCommand::Text {"
# The definition lives here and needs no import; its own sites are hand-written.
SKIP = {os.path.join("gui", "toolkit", "src", "render.rs")}

CLOSERS = {")": "(", "]": "[", "}": "{"}
OPENERS = set("([{")


def rs_files():
    for root in ROOTS:
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
            for name in sorted(filenames):
                if name.endswith(".rs"):
                    p = os.path.join(dirpath, name)
                    if p not in SKIP:
                        yield p


def scan(text, i, stop_depth0, end):
    """Walk from i to end, honouring strings/chars/brackets, and return the
    first index at which one of `stop_depth0` appears at bracket depth 0."""
    depth = 0
    while i < end:
        c = text[i]
        if c == '"':
            i += 1
            while i < end and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "'" and i + 2 < end and (text[i + 2] == "'" or text[i + 1] == "\\"):
            i += 1
            while i < end and text[i] != "'":
                i += 2 if text[i] == "\\" else 1
        elif c in OPENERS:
            depth += 1
        elif c in CLOSERS:
            if depth == 0 and c in stop_depth0:
                return i
            depth -= 1
        elif depth == 0 and c in stop_depth0:
            return i
        i += 1
    return -1


def block_end(text, open_idx):
    """Index of the `}` matching the `{` at open_idx."""
    j = scan(text, open_idx + 1, "}", len(text))
    return j


def main():
    changed_files = 0
    changed_sites = 0
    imports_added = 0
    skipped = []

    for path in rs_files():
        with io.open(path, encoding="utf-8", newline="") as f:
            src = f.read()
        if NEEDLE not in src:
            continue

        # Right to left, so earlier offsets stay valid as we splice.
        starts = [m.start() for m in re.finditer(re.escape(NEEDLE), src)]
        out = src
        local = 0
        for start in reversed(starts):
            open_idx = out.index("{", start)
            close_idx = block_end(out, open_idx)
            if close_idx < 0:
                skipped.append("%s @%d: unbalanced block" % (path, start))
                continue
            body_start, body_end = open_idx + 1, close_idx
            body = out[body_start:body_end]
            if "overflow" in body:
                continue

            # `Option::None` and `Some` are the same answer spelled two ways.
            mw = re.search(r"(?<![\w.])max_width\s*:\s*(?:\w+::)*(None|Some)\b", body)
            if not mw:
                # A pattern, or a construction that passes a binding through.
                # Both are left for a human; `..` patterns need nothing at all.
                if ".." not in body:
                    skipped.append("%s:%d" % (path, out.count("\n", 0, start) + 1))
                continue

            value = "Ellipsis" if mw.group(1) == "Some" else "Clip"
            # End of the `max_width` field: its comma, or the block's `}`.
            field_start = body_start + mw.start()
            term = scan(out, field_start, ",}", body_end + 1)
            if term < 0:
                skipped.append("%s:%d" % (path, out.count("\n", 0, start) + 1))
                continue

            line_start = out.rfind("\n", 0, field_start) + 1
            indent = out[line_start:field_start]
            if indent.strip():  # something shares the line; fall back to inline
                indent = ""
            multiline = "\n" in out[open_idx:close_idx]

            if out[term] == ",":
                ins_at = term + 1
                text = (
                    "\n%soverflow: TextOverflow::%s," % (indent, value)
                    if multiline
                    else " overflow: TextOverflow::%s," % value
                )
            else:  # no trailing comma; insert before the closing brace
                ins_at = term
                text = (
                    ",\n%soverflow: TextOverflow::%s,\n%s"
                    % (indent, value, indent[:-4])
                    if multiline
                    else ", overflow: TextOverflow::%s " % value
                )
                if not multiline:
                    # Swallow the space that preceded `}` so we don't double it.
                    while ins_at > 0 and out[ins_at - 1] == " ":
                        ins_at -= 1
            out = out[:ins_at] + text + out[ins_at:]
            local += 1

        if local == 0:
            continue

        # A handful of files sit in the working tree with CRLF endings (git is
        # `autocrlf=input`, so the committed blob is LF regardless and this is
        # cosmetic — but inserting an LF line into a CRLF file leaves the
        # working copy internally mixed, which is worse than either ending).
        if src.count("\r\n") > src.count("\n") - src.count("\r\n"):
            out = out.replace("\r\n", "\n").replace("\n", "\r\n")

        if "TextOverflow" not in src:
            out, n = add_import(out, path)
            if n == 0:
                skipped.append("%s: could not place the TextOverflow import" % path)
            imports_added += n

        with io.open(path, "w", encoding="utf-8", newline="") as f:
            f.write(out)
        changed_files += 1
        changed_sites += local

    print("sites changed: %d in %d files" % (changed_sites, changed_files))
    print("imports added: %d" % imports_added)
    print("left for a human: %d" % len(skipped))
    for s in skipped:
        print("  " + s)


def add_import(src, path):
    """Bring `TextOverflow` in by whatever route `FontWeightHint` already uses —
    every construction names it, so the route is guaranteed to exist."""
    m = re.search(r"^([ \t]*)((?:pub )?use ([\w:]*render)::)\{([^}]*)\};", src, re.M)
    if m and "FontWeightHint" in m.group(4):
        names = [n.strip() for n in m.group(4).split(",") if n.strip()]
        names.append("TextOverflow")
        names = sorted(set(names))
        line = "%s%s{%s};" % (m.group(1), m.group(2), ", ".join(names))
        return src[: m.start()] + line + src[m.end() :], 1
    m = re.search(r"^([ \t]*)((?:pub )?use ([\w:]*render)::)FontWeightHint;", src, re.M)
    if m:
        line = "%s%s{FontWeightHint, TextOverflow};" % (m.group(1), m.group(2))
        return src[: m.start()] + line + src[m.end() :], 1
    return src, 0


if __name__ == "__main__":
    sys.exit(main())
