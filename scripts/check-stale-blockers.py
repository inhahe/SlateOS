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
import gitenv
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


# A second shape of the same failure: prose that points at a file which is no
# longer there. Lane A hit five instances of stale prose in one day and only one
# of them was a cleared blocker; the rest included a design-decisions commitment
# to run a script that had been retired three weeks earlier. That one is
# mechanically checkable and nothing was checking it.
SCRIPT_REF = re.compile(r'`(scripts/[A-Za-z0-9_.\-]+\.(?:py|sh|ps1))`')

# Two things are NOT findings, and both were in the first run's output.
#
# A document that already says the file is gone has not misled anyone -- it has
# done the opposite. `design-decisions.md` carries a blockquote under its
# `stamp-ancestry.py` commitment saying exactly that, and reporting it would
# train a reader to skim past the ones that are wrong.
ALREADY_NOTED = re.compile(
    r"no longer exists|has been deleted|no longer present"
    # Tense and voice both vary, and the first version only matched the past
    # passive. `design-decisions.md` §382's own heading says "`diff-subject.sh`
    # is retired" -- a section whose whole subject is the retirement -- and was
    # reported six times because the regex wanted "was retired".
    r"|(?:is|was|been|are|were)\s+(?:retired|deleted|removed|gone)"
    r"|since removed|now gone|does not exist",
    re.I,
)
# And a name used as a stand-in for any script is not a claim that it exists.
# `known-issues.md` discusses "a literal `scripts/X.py` on a `run_checker`
# line", which is about pattern-matching, not about a file.
PLACEHOLDER_REF = re.compile(r'^scripts/(?:X|Y|N|FOO|NAME|SOMETHING)\.', re.I)
# The window is asymmetric on purpose. A note that a file is gone almost
# always follows the citation it corrects -- a blockquote under the claim, or
# a sentence after it -- and rarely precedes it.
NOTE_BEFORE = 3
NOTE_AFTER = 12


def dangling_references(doc_texts, exists):
    """Script paths cited in prose that no longer exist.

    `exists` is passed in rather than called directly so the selftest can
    describe a tree without creating one.
    """
    found = []
    for name, text in sorted(doc_texts.items()):
        lines = text.split(chr(10))
        for i, line in enumerate(lines):
            for m in SCRIPT_REF.finditer(line):
                path = m.group(1)
                if exists(path) or PLACEHOLDER_REF.match(path):
                    continue
                lo = max(0, i - NOTE_BEFORE)
                if ALREADY_NOTED.search(chr(10).join(lines[lo : i + NOTE_AFTER])):
                    continue
                found.append((name, i + 1, path))
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


DANGLING_SELFTEST = [
    (
        "a cited script that exists is not reported",
        "See `scripts/present.py` for the scan.",
        {"scripts/present.py"},
        0,
    ),
    (
        "a cited script that does not exist is reported",
        "Run `scripts/retired.py` after every merge.",
        {"scripts/present.py"},
        1,
    ),
    (
        "...unless the same passage already says it is gone",
        "Run `scripts/retired.py` after every merge." + chr(10)
        + "> Correction: it no longer exists.",
        {"scripts/present.py"},
        0,
    ),
    (
        "a stand-in name is not a claim that a file exists",
        "A literal `scripts/X.py` on a run_checker line under-counts.",
        set(),
        0,
    ),
    (
        "a path outside backticks is prose, not a reference",
        "We deleted scripts/retired.py last week.",
        set(),
        0,
    ),
    (
        "a shell script counts too, not just python",
        "The harness is `scripts/gone-diff.sh`.",
        set(),
        1,
    ),
]


def dangling_selftest():
    bad = 0
    for name, text, present, want in DANGLING_SELFTEST:
        got = len(dangling_references({"doc.md": text}, lambda rel: rel in present))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %d hit(s), got %d" % (want, got))
    return bad


def selftest():
    bad = 0
    for name, body, resolved, want in SELFTEST:
        got = len(stale("\n".join(body) + "\n", resolved))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %d hit(s), got %d" % (want, got))
    bad += dangling_selftest()
    total = len(SELFTEST) + len(DANGLING_SELFTEST)
    print()
    print("check-stale-blockers selftest: %d case(s), %d failed" % (total, bad))
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

    # A SCAN THAT READ ALMOST NOTHING IS REFUSED, which several gates in this
    # tree already do and this one did not. The floors are deliberately far
    # below the real figures -- 818 entries, 338 requests, 444 documents -- so
    # they fire on a scan that broke, not on a tree that shrank. Without them
    # the hostile-GIT_DIR failure above was a clean exit 0 over an empty set,
    # and the summary line was the only thing that gave it away. A summary a
    # human has to read is weaker than a status a hook can act on.
    FLOOR_ENTRIES = 100
    FLOOR_REQUESTS = 50
    FLOOR_DOCUMENTS = 50

    resolved = request_states(requests_dir)
    text = issues.read_text(encoding="utf-8", errors="surrogateescape")
    hits = stale(text, resolved)

    # Second pass: prose pointing at a script that is not there any more.
    import subprocess

    # `cwd=` DOES NOT ANCHOR THE REPOSITORY, and neither does `git -C`. Both
    # anchor the DIRECTORY; `GIT_DIR` still wins, and git sets `GIT_DIR` for
    # every hook it runs -- which is one of the two places this gate is wired.
    #
    # Measured rather than argued: with a foreign `GIT_DIR` this reported
    # "0 document(s) scanned for script references" and exited 0. It inspected
    # nothing and said so in a sentence that reads like a clean tree. Lane A hit
    # the same thing in check-shell-callables and passed the finding on; this is
    # that grep coming back positive.
    listed = subprocess.run(
        ["git", "ls-files", "*.md", "*.txt"],
        capture_output=True,
        text=True,
        cwd=ROOT,
        env=gitenv.clean_env(),
    ).stdout.split(chr(10))
    doc_texts = {}
    for name in listed:
        if not name.strip():
            continue
        f = ROOT / name
        try:
            doc_texts[name] = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
    dangling = dangling_references(doc_texts, lambda rel: (ROOT / rel).exists())

    for lineno, title, request in hits:
        print("known-issues.md:%d: %s" % (lineno, title.strip()))
        print("    cites requests/%s, which reports itself finished." % request)
        print("    Re-read it: the thing it waits for may already exist.")
        print()

    # Grouped by the missing file rather than by citation. One retired script is
    # one thing to fix, and `stamp-ancestry.py` alone is cited 19 times -- listing
    # each would bury the other six under it and make the report look like a
    # crisis instead of a list.
    by_path = {}
    for name, lineno, path in dangling:
        by_path.setdefault(path, []).append((name, lineno))
    # Git separates the two kinds without a hand-maintained list. A path that
    # once existed and was deleted is a RETIRED script, and a citation of it is
    # often legitimate history -- design-decisions.md narrates what the harnesses
    # did in the weeks before `diff-subject.sh` was retired, and that prose is
    # true. A path with no history at all was NEVER WRITTEN, so no reader has
    # ever been able to follow it: a typo, or a file someone meant to add.
    #
    # Both are reported, because a live instruction to run a retired script is
    # still wrong, but they are labelled differently so a reader can triage.
    # `scripts/check-textmode-writes.py` is the clearest case of the second
    # kind: the real gate is `check-text-mode-writes.py`, and the name is close
    # enough that anyone following it would doubt themselves first.
    import subprocess

    def ever_existed(rel):
        out = subprocess.run(
            ["git", "log", "--all", "--oneline", "--", rel],
            capture_output=True,
            text=True,
            cwd=ROOT,
            env=gitenv.clean_env(),
        ).stdout.strip()
        return bool(out)

    for path, sites in sorted(by_path.items()):
        where = ", ".join("%s:%d" % (n, l) for n, l in sites[:3])
        if len(sites) > 3:
            where += " and %d more" % (len(sites) - 3)
        if ever_existed(path):
            print("`%s` was deleted, still cited %d time(s): %s"
                  % (path, len(sites), where))
            print("    Historical narrative about it is fine. An instruction to")
            print("    RUN it is not -- check which of these are which.")
        else:
            print("`%s` NEVER EXISTED, cited %d time(s): %s"
                  % (path, len(sites), where))
            print("    No reader has ever been able to follow this. It is a typo")
            print("    or a file someone meant to add.")
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
    print(
        "check-stale-blockers: %d document(s) scanned for script references, "
        "%d citation(s) of %d file(s) that do not exist."
        % (len(doc_texts), len(dangling), len(by_path))
    )

    thin = []
    if total < FLOOR_ENTRIES:
        thin.append("known-issues entries: %d < %d" % (total, FLOOR_ENTRIES))
    if len(resolved) < FLOOR_REQUESTS:
        thin.append("request files: %d < %d" % (len(resolved), FLOOR_REQUESTS))
    if len(doc_texts) < FLOOR_DOCUMENTS:
        thin.append("documents: %d < %d" % (len(doc_texts), FLOOR_DOCUMENTS))
    if thin:
        print(
            "check-stale-blockers: REFUSING a verdict -- this scan read far less "
            "than the tree holds (%s)." % "; ".join(thin),
            file=sys.stderr,
        )
        print(
            "    Something stopped it reading rather than nothing being wrong. "
            "A foreign GIT_DIR does exactly this.",
            file=sys.stderr,
        )
        return 2
    if "--strict" in argv:
        return 1 if (hits or dangling) else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
