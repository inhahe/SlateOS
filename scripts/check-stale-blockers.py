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


# The SECOND source of a stale blocker, and the one that cost five days here.
#
# An entry can wait on a `requests/` file -- covered above -- or on an
# `open-questions.md` question. The known-issues entry for the duplicate-pair
# conversions read "Everything remaining is blocked on B-Q7, so there is no
# unblocked work left in this entry", and B-Q7 had been answered by the
# operator on 2026-09-07 (design-decisions 1005, "coreutils is the one home").
# The sentence told every reader to go and do something else, and it was wrong
# for five days.
#
# It was invisible here because this gate only ever cross-referenced
# `requests/`. Nothing misbehaved: the check ran, inspected 832 entries and
# reported 4, and all 4 were real. It answered the question it was built to
# answer, in full, about a population that excluded the failure -- the same
# shape as a `grep -c` on a path that does not exist. See known-issues ->
# "never pipe a command whose exit status you intend to believe".
QUESTION_REF = re.compile(r"\b([ABC]-Q\d+)\b")

# `## B-Q7 - ...` in the live half; `- B-Q7 ...` in the archive below
# `# Resolved`.
QUESTION_OPEN = re.compile(r"^## ([ABC]-Q\d+)\b", re.M)
QUESTION_DONE = re.compile(r"^- ([ABC]-Q\d+)\b", re.M)
RESOLVED_HEAD = re.compile(r"^# Resolved\s*$", re.M)


def question_states(open_questions_text):
    """Map each question id to whether `open-questions.md` says it is answered.

    A question is answered when it sits in the archive below `# Resolved`, and
    open when it has a `##` heading above that line. An id in neither is absent
    from the map rather than guessed at: an unknown id must not read as
    "resolved", because that is the direction that invents a finding.
    """
    split = RESOLVED_HEAD.search(open_questions_text)
    cut = split.start() if split else len(open_questions_text)
    live, archive = open_questions_text[:cut], open_questions_text[cut:]
    out = {}
    for m in QUESTION_OPEN.finditer(live):
        out[m.group(1)] = False
    for m in QUESTION_DONE.finditer(archive):
        out.setdefault(m.group(1), True)
    return out


# A passage that already says the question is ANSWERED is doing the reader's
# work, not misleading them. Without this, CORRECTING one of these entries
# makes it fire twice as loudly: the correction has to name the question, and
# it lands next to the "blocked" sentence it is correcting. All three entries
# fixed on 2026-09-12 kept reporting for exactly that reason.
#
# Same argument as `ALREADY_NOTED` above, and the same failure mode without
# it -- a gate that flags its own fix teaches people to stop fixing.
QUESTION_SETTLED = re.compile(
    # `unblocked` IS NOT HERE, and the self-test is why. The sentence this
    # whole check was written for reads "there is no unblocked work left in
    # this entry" -- which means the opposite, and matching the word alone
    # suppressed the one finding the gate exists to make. A recogniser for
    # "already corrected" that also matches the uncorrected original is
    # worse than not having one.
    r"answered|resolved|decided|landed|settled"
    r"|no longer (?:blocked|waiting)"
    # The decision record itself. An entry that cites the section number has
    # looked the answer up, which is the opposite of being stuck.
    r"|\u00a71005|Decided by",
    re.I,
)


def stale_questions(text, answered):
    """Entries that are open and cite an ANSWERED question in blocking language.

    Deliberately the same context test as `stale`: a citation alone is not a
    finding, because an entry may mention a question it is not waiting on.
    """
    found = []
    for lineno, title, body in entries(text):
        if CLOSED_ENTRY.search("\n".join(body[:CLOSED_WINDOW])):
            continue
        for j, line in enumerate(body):
            names = [m.group(1) for m in QUESTION_REF.finditer(line)]
            done = [n for n in names if answered.get(n)]
            if not done:
                continue
            lo = max(0, j - CONTEXT_BEFORE)
            context = "\n".join(body[lo : j + CONTEXT_AFTER])
            if BLOCKING.search(context) and not QUESTION_SETTLED.search(context):
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
# A scan that read almost nothing is refused rather than reported clean. The
# floors sit far below the real figures so they fire on a scan that BROKE, not
# on a tree that shrank.
#
# They are module-level so the selftest can assert the real tree stays above
# them -- lane A's idea, and the half I had missed. A floor that drifts up to
# meet a shrinking population is a ratchet running backwards: it would go on
# passing while measuring less and less, which is the exact failure the floors
# were added to prevent, one level up.
FLOOR_ENTRIES = 100
FLOOR_REQUESTS = 50
FLOOR_DOCUMENTS = 50

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


def floors_selftest():
    """The real tree must stay comfortably above every floor.

    Lane A's case, and the one I had missed. Without it a floor can be raised
    past what it measures and the gate goes on passing while inspecting less and
    less -- a ratchet running backwards. This is the only self-test case here
    that reads the tree rather than a string literal, and it reads it for
    exactly that reason: the claim being checked is about the real populations.

    A tree that has genuinely shrunk below a floor fails this too, which is
    correct -- it means the floor and the tree disagree and a person should look,
    not that the floor should quietly follow the tree down.
    """
    requests_dir = ROOT / "requests"
    issues = ROOT / "known-issues.md"
    if not requests_dir.is_dir() or not issues.is_file():
        print("ok   (floors not checked: no tree here)")
        return 0
    text = issues.read_text(encoding="utf-8", errors="surrogateescape")
    entries_n = sum(1 for _ in entries(text))
    requests_n = len(list(requests_dir.glob("*.md")))
    docs_n = len(tracked_documents())
    bad = 0
    for name, actual, floor in (
        ("known-issues entries", entries_n, FLOOR_ENTRIES),
        ("request files", requests_n, FLOOR_REQUESTS),
        ("documents", docs_n, FLOOR_DOCUMENTS),
    ):
        ok = actual > floor
        bad += 0 if ok else 1
        print(
            "%-4s the real tree is above the %s floor (%d > %d)"
            % ("ok" if ok else "FAIL", name, actual, floor)
        )
    return bad


QUESTION_SELFTEST = [
    (
        "an open entry blocked on an ANSWERED question is reported",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "Everything remaining is blocked on B-Q7, so there is no unblocked",
         "work left in this entry."],
        {"B-Q7": True},
        1,
    ),
    (
        "...but not when the question is still open",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "Everything remaining is blocked on B-Q7, so there is no unblocked",
         "work left in this entry."],
        {"B-Q7": False},
        0,
    ),
    (
        "a question MENTIONED without blocking language is not a finding",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "The reasoning behind this is recorded in B-Q7.",
         "It has no bearing on the work below."],
        {"B-Q7": True},
        0,
    ),
    (
        "an id nobody has heard of does not read as answered",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This is blocked on B-Q99."],
        {"B-Q7": True},
        0,
    ),
    (
        "...and not once the passage says the question was answered",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "This used to say it was blocked on B-Q7.",
         "B-Q7 was answered on 2026-09-07, so it is not waiting on anything."],
        {"B-Q7": True},
        0,
    ),
    (
        "a correction that cites the decision record is not a finding either",
        ["## B-SOMETHING-IS-BROKEN (lane B, 2026-01-01)",
         "",
         "Blocked on B-Q7 until it lands.",
         "Decided by: Operator, so this is unblocked."],
        {"B-Q7": True},
        0,
    ),
]


def question_selftest():
    """`question_states` against a miniature open-questions.md.

    The archive/live split is the whole mechanism, so it is tested on a
    document that has both halves rather than on the real file, where a change
    to an unrelated question would move the answer.
    """
    doc = "\n".join([
        "# Open Questions",
        "",
        "## B-Q14 - something undecided - Status: OPEN",
        "",
        "body",
        "",
        "# Resolved",
        "",
        "- B-Q7 which copy is canonical - resolved 2026-09-07",
        "- A-Q3 something else - resolved 2026-08-01",
    ])
    got = question_states(doc)
    want = {"B-Q14": False, "B-Q7": True, "A-Q3": True}
    ok = got == want
    print("%-4s %s" % ("ok" if ok else "FAIL", "question_states splits live from archived"))
    if not ok:
        print("       wanted %r" % (want,))
        print("       got    %r" % (got,))
    return 0 if ok else 1


def selftest():
    bad = 0
    for name, body, resolved, want in SELFTEST:
        got = len(stale("\n".join(body) + "\n", resolved))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %d hit(s), got %d" % (want, got))
    for name, body, answered, want in QUESTION_SELFTEST:
        got = len(stale_questions("\n".join(body) + "\n", answered))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %d hit(s), got %d" % (want, got))
    bad += question_selftest()
    bad += dangling_selftest()
    bad += floors_selftest()
    total = (
        len(SELFTEST) + len(QUESTION_SELFTEST) + 1 + len(DANGLING_SELFTEST) + 3
    )
    print()
    print("check-stale-blockers selftest: %d case(s), %d failed" % (total, bad))
    return 1 if bad else 0


def tracked_documents():
    """Every tracked `.md`/`.txt` path, as git sees it.

    One implementation, used by the scan and by the floors self-test. Two would
    be two chances to get the environment wrong, and the environment is the
    thing that went wrong here.
    """
    import subprocess

    out = subprocess.run(
        ["git", "ls-files", "*.md", "*.txt"],
        capture_output=True,
        text=True,
        cwd=ROOT,
        env=gitenv.clean_env(),
    ).stdout
    return [n for n in out.split(chr(10)) if n.strip()]


def document_texts():
    """Map tracked document path -> contents, skipping what cannot be read."""
    texts = {}
    for name in tracked_documents():
        try:
            texts[name] = (ROOT / name).read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
    return texts


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

    resolved = request_states(requests_dir)
    text = issues.read_text(encoding="utf-8", errors="surrogateescape")
    hits = stale(text, resolved)

    # The same question asked of `open-questions.md`. An entry waiting on an
    # ANSWERED question is as stuck as one waiting on a landed request, and
    # reads more convincingly because a question sounds like it is still being
    # thought about.
    questions = ROOT / "open-questions.md"
    answered = (
        question_states(questions.read_text(encoding="utf-8", errors="surrogateescape"))
        if questions.is_file()
        else {}
    )
    qhits = stale_questions(text, answered)

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
    doc_texts = document_texts()
    dangling = dangling_references(doc_texts, lambda rel: (ROOT / rel).exists())

    for lineno, title, request in hits:
        print("known-issues.md:%d: %s" % (lineno, title.strip()))
        print("    cites requests/%s, which reports itself finished." % request)
        print("    Re-read it: the thing it waits for may already exist.")
        print()

    for lineno, title, question in qhits:
        print("known-issues.md:%d: %s" % (lineno, title.strip()))
        print("    says it is blocked on %s, which open-questions.md records"
              " as answered." % question)
        print("    Re-read it: the decision it waits for has been made.")
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
    # BOTH counts, and the questions one is named separately rather than added
    # in. When the question pass was first wired the summary still printed only
    # `len(hits)`: four new findings were printed above it and the line under
    # them said "4", which is the exact defect the comment above warns about.
    # A reader who trusts the summary would have seen half the report.
    print(
        "check-stale-blockers: %d entr(ies) and %d request(s) inspected, "
        "%d request(s) report themselves finished, %d entr(ies) cite a "
        "finished request, %d cite an answered question."
        % (total, len(resolved), sum(resolved.values()), len(hits), len(qhits))
    )
    print(
        "check-stale-blockers: %d question(s) known, %d answered."
        % (len(answered), sum(1 for v in answered.values() if v))
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
