#!/usr/bin/env python3
"""Find `known-issues.md` entries whose stated blocker has already been cleared.

WHY THIS EXISTS. On 2026-09-12 lane B hit the same failure three times in one
day: an entry said the work was waiting on a native syscall number, the request
file it pointed at had said `LANDED` in its own status line for days, and
nothing had re-read either. The interval timers waited three days that way;
`setgroups` and `chroot` waited five. Lane A named the shape in an unrelated
exchange the same week -- *a statement that was true when written, in a document
read as present tense* -- and reported four instances of their own.

The lesson is not "write more carefully". Every one of those entries was written
carefully and was correct on the day. The lesson is that a document naming a
blocker somebody else can clear needs re-reading when they clear it, and that
nobody is going to remember to. So this is a check rather than a habit.

WHAT IT LOOKS AT. Every `requests/*.md` carries a `**Status:**` line; a request
whose status says LANDED, DONE, RESOLVED, CLOSED, ANSWERED or carries a tick is
finished. Every `known-issues.md` entry that is not itself marked closed, and
that cites such a request *in blocking language*, is reported.

WHY THE BLOCKING-LANGUAGE FILTER. Without it the answer is 36 entries, most of
which merely cite a request for context -- "asked in X, which landed" -- and a
reader learns to skim. With it the answer is 8, every one of which claims to be
waiting for something that is no longer missing. A check nobody acts on is worse
than no check, because it looks like coverage.

WHAT IT DOES NOT CLAIM. A hit is "re-read this", not "this is wrong". A request
can land while the entry stays legitimately open, because the request was only
part of what the entry needed. That is why this prints a report and exits 0
unless `--strict` is given: the judgement is a person's, and the cost of the
failure it catches is days of waiting, not a broken build.
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag

ROOT = Path(__file__).resolve().parent.parent

# A request is finished when its own status line says so. Matched at the start
# of a line so that a sentence *about* a status in the body does not count.
RESOLVED_REQUEST = re.compile(
    r"^\*\*Status:\*\*.*(LANDED|DONE|RESOLVED|CLOSED|ANSWERED|✅)", re.M
)

# An entry announces its own closure in the first few lines, not the heading --
# which is why a heading-only filter reports entries that were fixed months ago.
CLOSED_ENTRY = re.compile(
    r"FIXED|RESOLVED|CLOSED|WITHDRAWN|SUPERSEDED|IMPLEMENTED|DONE"
    r"|no impact|crate deleted",
    re.I,
)
CLOSED_WINDOW = 8

# `## ` followed by a letter or a backtick. The document also uses `## ` for
# four STRUCTURAL headings that organise it rather than describing a defect, and
# those are named below rather than guessed at. Every rule that tried to infer
# the difference was wrong in one direction or the other: entry titles are
# sometimes plain prose ("Two process bugs this round, both about trusting a
# green result"), and sometimes a bare dashed identifier with no lane or date
# ("TD-C-THE-ORPHAN-LEDGER-IS-NOT-A-QUEUE-OF-READY-WORK"), so neither shape nor
# length separates them. Four names, checked by the selftest, is honest; a
# clever regex here would be wrong the first time someone writes a fifth.
ENTRY_HEAD = re.compile(r"^## [A-Z`]")
STRUCTURAL = {
    "Active Bugs",
    "Fixed Bugs",
    "Technical Debt",
    "Reference Material",
}

REFERENCE = re.compile(r"requests/([a-z0-9][a-z0-9.\-]*\.md)")

BLOCKING = re.compile(
    r"blocked|waiting on|waits on|asked of|requested in|asked for in"
    r"|until .{0,40}lands|cannot .{0,40}until|needs lane"
    r"|no native (number|syscall)",
    re.I,
)
# How many lines either side of the citation count as its context.
CONTEXT_BEFORE = 4
CONTEXT_AFTER = 3


def request_states(requests_dir):
    """Map each request file name to whether its own status says it is done."""
    out = {}
    for f in sorted(requests_dir.glob("*.md")):
        text = f.read_text(encoding="utf-8", errors="replace")
        out[f.name] = bool(RESOLVED_REQUEST.search(text))
    return out


def entries(text):
    """Yield (line_number, title, body_lines) for each entry in known-issues."""
    lines = text.split("\n")
    heads = [i for i, l in enumerate(lines) if ENTRY_HEAD.match(l)]
    for k, i in enumerate(heads):
        end = heads[k + 1] if k + 1 < len(heads) else len(lines)
        title = lines[i][3:]
        if title.strip() in STRUCTURAL:
            continue
        yield i + 1, title, lines[i:end]


def stale(text, resolved):
    """Entries that are open and cite a finished request in blocking language."""
    found = []
    for lineno, title, body in entries(text):
        if CLOSED_ENTRY.search("\n".join(body[:CLOSED_WINDOW])):
            continue
        for j, line in enumerate(body):
            names = [m.group(1) for m in REFERENCE.finditer(line)]
            done = [n for n in names if resolved.get(n)]
            if not done:
                continue
            lo = max(0, j - CONTEXT_BEFORE)
            context = "\n".join(body[lo : j + CONTEXT_AFTER])
            if BLOCKING.search(context):
                found.append((lineno, title, done[0]))
                break
    return found


SELFTEST = [
    (
        "an open entry citing a landed request in blocking language is reported",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This is blocked on a kernel change.",
         "Asked of lane A in `requests/x.md`."],
        {"x.md": True},
        1,
    ),
    (
        "...but not when the request is still open",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This is blocked on a kernel change.",
         "Asked of lane A in `requests/x.md`."],
        {"x.md": False},
        0,
    ),
    (
        "...and not when the entry itself is already marked fixed",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "**Status: FIXED 2026-02-02.**",
         "This was blocked on a kernel change.",
         "Asked of lane A in `requests/x.md`."],
        {"x.md": True},
        0,
    ),
    (
        "a citation with no blocking language is not a blocker",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "For background see `requests/x.md`, which covers the same ground.",
         "The fix here is local."],
        {"x.md": True},
        0,
    ),
    (
        "a section heading is not an entry",
        ["## Technical Debt",
         "",
         "This is blocked on something.",
         "Asked of lane A in `requests/x.md`."],
        {"x.md": True},
        0,
    ),
    (
        "an unknown request name is not assumed landed",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This is blocked on a kernel change.",
         "Asked of lane A in `requests/never-filed.md`."],
        {"x.md": True},
        0,
    ),
    (
        "blocking language four lines above the citation still counts",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This is blocked on a kernel change.",
         "a",
         "b",
         "c",
         "The request is `requests/x.md`."],
        {"x.md": True},
        1,
    ),
    (
        "...but not eight lines above, which is a different subject",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This is blocked on a kernel change.",
         "a", "b", "c", "d", "e", "f", "g",
         "Separately, see `requests/x.md`."],
        {"x.md": True},
        0,
    ),
]


def selftest():
    bad = 0
    for name, body, resolved, want in SELFTEST:
        got = len(stale("\n".join(body) + "\n", resolved))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %d hit(s), got %d" % (want, got))
    print()
    print("check-stale-blockers selftest: %d case(s), %d failed" % (len(SELFTEST), bad))
    return 1 if bad else 0


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if selftestflag.wants_selftest(argv):
        return selftest()

    requests_dir = ROOT / "requests"
    issues = ROOT / "known-issues.md"
    if not requests_dir.is_dir() or not issues.is_file():
        print("check-stale-blockers: no requests/ or known-issues.md here", file=sys.stderr)
        return 2

    resolved = request_states(requests_dir)
    text = issues.read_text(encoding="utf-8", errors="surrogateescape")
    hits = stale(text, resolved)

    for lineno, title, request in hits:
        print("known-issues.md:%d: %s" % (lineno, title.strip()))
        print("    cites requests/%s, which reports itself finished." % request)
        print("    Re-read it: the thing it waits for may already exist.")
        print()

    # Name the population. A checker that prints only a verdict reads the same
    # whether it inspected 849 entries or none, which is how a regex that stops
    # matching after a refactor goes unnoticed.
    total = sum(1 for _ in entries(text))
    print(
        "check-stale-blockers: %d entr(ies) and %d request(s) inspected, "
        "%d request(s) report themselves finished, %d entr(ies) may have been "
        "unblocked without noticing."
        % (total, len(resolved), sum(resolved.values()), len(hits))
    )
    if "--strict" in argv:
        return 1 if hits else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
