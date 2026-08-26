#!/usr/bin/env python3
"""Guard the rule that answering a question is *not* reporting a failure.

The rule
--------
**If a kshell block can only be reached by the user asking -- no argument was
given -- and it answers by printing program state, it must not set a failure
status.**

This is the mirror of `check-usage-status.py`, and it exists because that
checker's rule, applied without this one, produces the opposite bug.  "Print a
usage line, set a failure status" is right for a complaint and wrong for a
hint, and the two are written identically:

    if parts.len() < 2 {
        shell_println!("Serial echo level: {} (and above)", level.as_str());
        shell_println!("Usage: elog echo <level>  to change");
        set_exit(1);            // <-- the answer above is correct
        return;
    }

`elog echo` and `fc algo` shipped exactly that for a month: they printed the
right answer and then told the caller they had failed, because a sweep that
was fixing genuine missing statuses could not tell a query from a complaint.
`$(elog echo)` in a script under `set -e` kills the script *after* producing
the value it wanted.

Why a second checker rather than a rule inside the first
--------------------------------------------------------
Because the two rules point in opposite directions and share no machinery
worth sharing.  A single script that decided both would have to hold "this
needs a status" and "this must not have one" in one classifier, and the first
time they disagreed the disagreement would be silent.  Kept apart, each one
states a property, and a site that both flag is a site whose author has to
say which it is -- which is the conversation that should happen.

What is checked
---------------
For each non-zero `set_exit`, find the block that encloses it and ask three
questions:

1. **Is the block guarded on "no argument was given"?**  `parts.is_empty()`,
   `parts.len() < 2`, a `None =>` arm of a `match` on an argument accessor.
   Negated forms (`!x.is_empty()`) mean an argument *was* given and do not
   count.
2. **Does it answer?**  At least one print *directly* in the block (not in a
   nested arm) whose argument list reads program state -- a path like
   `quota::is_enabled()` or a method call -- rather than only literal text.
3. **Does it then fail?**  The `set_exit` that started the walk.

All three, and the site is reported.

Testing it
----------
Pass a path to run against an older revision.  The positive control is the
revision before the fix:

    git show 9251e5a3d^:kernel/src/kshell.rs > /tmp/old.rs
    python scripts/check-query-status.py /tmp/old.rs

which must report `cmd_fcompress` ("Current: {}") and `cmd_elog` ("Serial echo
level: ..."), the two sites that shipped the bug.  A checker nobody has
watched fail is a checker nobody knows works.

Exit status: 0 clean, 1 sites found.
"""

import pathlib
import re
import sys

PATH = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "kshell.rs"

# Blocks that match the shape and are *right* to fail.  Keyed (function,
# fragment of the printed text), like ALLOWED in check-usage-status.py, and
# for the same reason: a line number drifts on every edit and would rot the
# list into a rubber stamp.  Each entry needs a reason.
ALLOWED = {
    # `match dpkg_find_member(...) { None => ... }`.  The `None` is a lookup
    # that found nothing, not an argument the user omitted -- the guard rule
    # below only requires the *scrutinee* to look argument-ish, and
    # `&members, "data.tar"` is close enough to fool it.
    ("cmd_dpkg_extract", "dpkg: no data.tar found in"):
        "the None is a failed lookup inside the .deb, not a missing argument",
    ("cmd_archive", "archive: {}: unknown archive format"):
        "the None is 'could not identify the file', which is the failure itself",
}

FN = re.compile(r"(?:pub )?(?:async )?fn ([a-z_0-9]+)")
FAIL = re.compile(r"set_exit\(\s*([1-9]\d*)\s*\)")
PRINT = re.compile(r"(?:console_print(?:ln)?!|shell_print(?:ln)?!)\s*\(\s*(.*)$")

# "No argument was given."  The negated forms are deliberately excluded: the
# lookbehind on the `is_empty` alternative rejects both `!x.is_empty()` and
# `!x.y.is_empty()`, which say an argument *was* given and whose blocks are
# ordinary error paths.
GUARD = re.compile(
    r"""(?x)
    (?:^|[^!\w])(?:parts|args|argv|words|toks|tokens|fields)
        \s*\.\s*len\s*\(\s*\)\s*(?:<\s*\d|==\s*[01]\b)
  | (?<![!.\w])[a-z_][a-z_0-9]*(?:\.[a-z_][a-z_0-9]*)*\.is_empty\(\)
  | \.\s*get\s*\(\s*\d+\s*\)[^;{]*\.\s*is_none\s*\(\s*\)
  | ^\s*None\s*=>
  | ^\s*""\s*=>
    """
)

# A `match` whose scrutinee is the user's words.  Only these make a `None =>`
# arm mean "the user gave no argument"; `match some_lookup() { None => ... }`
# means "it wasn't there", which is a genuine failure.
ARG_MATCH = re.compile(r"\bmatch\b[^{]*\b(?:parts|args|argv|words|sub|arg)\b")

# An argument list that reads program state: a module path or a method/field
# access, rather than only literal text or the user's own words echoed back.
STATE = re.compile(r"::|\.\s*[a-z_][a-z_0-9]*")


def strip_strings(s):
    """Drop string literals so braces inside them do not count as structure."""
    return "".join(s.split('"')[::2])


def block_start(lines, i):
    """The line holding the brace that opens the block containing line `i`.

    Braces are counted a character at a time, not a line at a time.  `} else {`
    closes and opens on one line, so a line-granular count nets to zero and the
    walk sails straight past the brace that actually delimits the block --
    which is how a sibling `else` branch gets mistaken for the guarded one.
    """
    depth = 0
    start = i
    for k in range(i - 1, max(-1, i - 400), -1):
        for ch in reversed(strip_strings(lines[k])):
            depth += -1 if ch == "{" else (1 if ch == "}" else 0)
            if depth < 0:
                break
        start = k
        if depth < 0:
            break
    return start


def block_end(lines, i):
    """The line holding the brace that closes the block containing line `i`."""
    depth = 0
    end = i
    for k in range(i + 1, min(len(lines), i + 400)):
        for ch in strip_strings(lines[k]):
            depth += 1 if ch == "{" else (-1 if ch == "}" else 0)
            if depth < 0:
                break
        end = k
        if depth < 0:
            break
    return end


def main(argv):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    path = pathlib.Path(argv[1]) if len(argv) > 1 else PATH
    lines = path.read_text(encoding="utf-8", errors="surrogateescape").split("\n")

    starts = [(i, m.group(1)) for i, ln in enumerate(lines) if (m := FN.match(ln))]

    def fn_of(i):
        name = "?"
        for s, n in starts:
            if s <= i:
                name = n
            else:
                break
        return name

    hits = []
    for i, ln in enumerate(lines):
        if not FAIL.search(ln) or ln.lstrip().startswith("//"):
            continue

        start = block_start(lines, i)
        guard = lines[start]
        if not GUARD.search(guard):
            continue
        # A `None =>` / `"" =>` arm only means "no argument" if the match is on
        # the user's words.
        if re.match(r"\s*(?:None|\"\")\s*=>", guard):
            if not ARG_MATCH.search(lines[block_start(lines, start)]):
                continue

        end = block_end(lines, i)
        answers = []
        rel = 0
        for k in range(start, end + 1):
            here = rel
            for ch in strip_strings(lines[k]):
                rel += 1 if ch == "{" else (-1 if ch == "}" else 0)
            m = PRINT.search(lines[k])
            # A print inside a nested arm belongs to that arm, not to the
            # query, so only lines sitting directly in this block count.
            if not m or (k != start and here != 1):
                continue
            payload = m.group(1)
            j = k
            while payload.count("(") - payload.count(")") >= 0 and j + 1 <= end:
                j += 1
                payload += " " + lines[j].strip()
            bits = payload.split('"')
            if len(bits) < 3:
                continue  # no argument list: literal text only
            if STATE.search('"'.join(bits[2:])):
                answers.append(payload.strip())
        if not answers:
            continue

        fn = fn_of(i)
        if any(fn == f and any(frag in a for a in answers) for (f, frag) in ALLOWED):
            continue
        hits.append((i + 1, fn, answers[0][:96]))

    if not hits:
        print(
            f"[query-status] kshell.rs: no query answers correctly and then reports "
            f"failure ({len(ALLOWED)} allowed)"
        )
        return 0

    print("", file=sys.stderr)
    print(
        f"{len(hits)} block(s) reachable only by asking, which answer and then "
        f"report FAILURE:", file=sys.stderr
    )
    for ln, fn, answer in hits:
        print(f"  {path}:{ln}  {fn}", file=sys.stderr)
        print(f"      {answer}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
