#!/usr/bin/env python3
"""Refuse to build when `open-questions.md` has stopped being a queue.

`open-questions.md` is the operator's decision queue.  Its value is entirely a
function of one property -- that reading the body tells you everything still
waiting on you -- and that property has no runtime symptom when it breaks.  A
question filed in the wrong half of the file is not an error, does not fail a
test, and does not look wrong when you are editing near it.  It simply never
gets answered, and the lane that filed it goes on believing it asked.

All three defects below were present simultaneously on 2026-09-03, in a file
whose own header states the rules they break:

* **Eight OPEN questions were filed below `# Resolved`** -- a third of the
  queue, sitting in the archive, under a heading whose following sentence reads
  "The body above holds OPEN questions only."  One of them (`A-Q2`) was wedged
  between that sentence and the first archive subsection, so even a reader who
  scrolled past the heading would have taken it for an archive entry.
* **Two different questions were both numbered `Q57`** -- filed ten hours
  apart, by the same lane, on the same day.  "Do B on Q57" would have been an
  ambiguous instruction and neither entry gave a reader any hint the other
  existed.
* **The same question was filed twice with opposite recommendations** -- the
  `grep` defaults, recommending C in one copy and A in the other.  That one is
  not mechanically detectable and is not checked here; it is named because it
  is what the other two cost when they compound.

The rules are written in the file's own header, and have been since the file
was split three ways.  Prose did not hold them -- every violation above was
committed by a lane that had read that header.  So the check is mechanical.

**A gate that discovers nothing reports no failures, which reads exactly like a
pass**, so this refuses to return a verdict at all if its parse comes back
implausibly empty (exit 2, distinct from the exit 1 that means the document is
wrong).  Every rule below is exercised by `--self-test` against a fixture that
makes it fire; a rule that has never been seen to fire is a rule you are
guessing about.

Exit codes: 0 clean (warnings do not fail), 1 the document is wrong, 2 the
checker could not reach a verdict.
"""

from __future__ import annotations

import hashlib
import pathlib
import subprocess
import re
import sys

import selftestflag

DOC = "open-questions.md"

#: The archive heading, matched exactly.  Not a prefix match: `# Resolved` and
#: `## Resolved — lane A` differ by one character at the start of the line, and
#: confusing them would put the boundary in the wrong place and report every
#: archive subsection as a stranded question.
RESOLVED_HEADING = "# Resolved"

#: `## Resolved — <lane>` subsections of the archive.  These are the only `## `
#: headings legitimately below the boundary.  The dash is an em dash in the
#: file; both dashes are accepted so a lane that types a hyphen gets a warning
#: about numbering rather than a spurious "stranded question" failure.
ARCHIVE_SUB_RE = re.compile(r"^## Resolved\s+[—-]\s+")

#: An identifier is the first token of the heading: `Q47`, `A-Q3`, `C-Q10`.
#: The unprefixed series is pre-split and closed, but it is still a valid
#: identifier for the entries that already carry one.
IDENT_RE = re.compile(r"^##\s+((?:[ABC]-)?Q\d+)\b")

#: The same identifier as it appears in the archive index, which is a list
#: rather than headings: `- Q31 SlateOS native-ABI ...`.  Anchored to the list
#: marker so that a `Q31` mentioned in running prose is not read as a filing.
INDEX_IDENT_RE = re.compile(r"^-\s+((?:[ABC]-)?Q\d+)\b")

#: `— Status: OPEN`, in the heading.  Everything else -- `RESOLVED`, `FIXED`,
#: `answered` -- means the entry belongs in the archive.  Optional: entries
#: predating the convention have no status in the heading at all, and that is a
#: warning (below), not a failure.
STATUS_RE = re.compile(r"[—-]\s+\*{0,2}Status\*{0,2}:\s*\*{0,2}([A-Za-z]+)")

#: A plausible parse.  These are floors, not targets: the file held 18 body
#: entries and 64 archive index lines when this was written, and the point is
#: only to notice that the format changed under the regexes rather than to
#: track the true count.  A file that legitimately falls below either floor has
#: emptied its own queue, which is worth a human look.
MIN_BODY_ENTRIES = 5
MIN_INDEX_ENTRIES = 10


class Findings:
    """Failures stop the build; warnings are counted and shown."""

    def __init__(self) -> None:
        self.failures: list[str] = []
        self.warnings: list[str] = []
        self.body: list[tuple[int, str]] = []
        self.index_idents: list[tuple[int, str]] = []

    def ok(self) -> bool:
        return not self.failures


def analyse(text: str) -> Findings:
    """Read a whole `open-questions.md` and report what is wrong with it.

    Raises `ValueError` for "I cannot read this", which the caller turns into
    exit 2.  A parse that cannot find the archive boundary must never fall
    through to "no failures found".
    """
    f = Findings()
    lines = text.split("\n")

    boundary = None
    for i, line in enumerate(lines):
        if line.rstrip() == RESOLVED_HEADING:
            boundary = i
            break
    if boundary is None:
        raise ValueError(
            f"no `{RESOLVED_HEADING}` heading: the archive boundary is what "
            "separates open questions from answered ones, so without it there "
            "is nothing to check against"
        )

    for i, line in enumerate(lines):
        if not line.startswith("## "):
            continue
        if i < boundary:
            f.body.append((i + 1, line))
            continue
        if ARCHIVE_SUB_RE.match(line):
            continue
        # A `## ` heading below the boundary that is not an archive subsection.
        f.failures.append(
            f"line {i + 1}: an OPEN question is filed below `{RESOLVED_HEADING}`, "
            f"where the body says only answered ones go:\n      {line[:110]}"
        )

    for i, line in enumerate(lines[boundary:], start=boundary + 1):
        m = INDEX_IDENT_RE.match(line)
        if m:
            f.index_idents.append((i, m.group(1)))

    if len(f.body) < MIN_BODY_ENTRIES:
        raise ValueError(
            f"only {len(f.body)} body entr(ies) found, below the floor of "
            f"{MIN_BODY_ENTRIES}. Either the queue is nearly empty or `## ` is "
            "no longer how an entry starts; both want a human, and reporting "
            "'no failures' over a parse this thin would be the failure this "
            "checker exists to prevent"
        )
    if len(f.index_idents) < MIN_INDEX_ENTRIES:
        raise ValueError(
            f"only {len(f.index_idents)} archive entr(ies) found, below the "
            f"floor of {MIN_INDEX_ENTRIES}; the index format has probably "
            "changed under `- Q<n> ...`"
        )

    # Identifiers, across the body and the archive index together: a new
    # question reusing the number of an answered one is the same ambiguity as
    # two open ones sharing a number.
    seen: dict[str, list[str]] = {}
    for line_no, heading in f.body:
        m = IDENT_RE.match(heading)
        if m:
            seen.setdefault(m.group(1), []).append(f"line {line_no} (body)")
        else:
            f.warnings.append(
                f"line {line_no}: no identifier, so the operator has no short "
                f"way to answer it:\n      {heading[:110]}"
            )
    for line_no, ident in f.index_idents:
        seen.setdefault(ident, []).append(f"line {line_no} (archive)")

    for ident, where in sorted(seen.items()):
        if len(where) <= 1:
            continue
        if any("(body)" in w for w in where):
            # At least one of the colliding entries is still open, so an answer
            # naming the number cannot be acted on -- which is the live harm.
            f.failures.append(
                f"`{ident}` is used {len(where)} times ({', '.join(where)}); "
                "an answer naming it would be ambiguous"
            )
        else:
            # Both are archived.  This is history -- `Q38` and `Q45` were each
            # issued twice in the single-agent append-only era, and the
            # surviving `Q45` entry says so in its own text.  Editing an
            # archive to satisfy a checker would be falsifying the record of
            # what the numbers meant when they were answered, and nothing can
            # be mis-answered now, so it is reported and not enforced.
            f.warnings.append(
                f"`{ident}` is used {len(where)} times in the archive "
                f"({', '.join(where)}); historic, from the append-only era"
            )

    # An answered entry left in the body sorts *first*, in front of the
    # questions that still need one -- which is the single thing the file
    # exists to show.
    for line_no, heading in f.body:
        m = STATUS_RE.search(heading)
        if m and m.group(1).upper() != "OPEN":
            f.failures.append(
                f"line {line_no}: body entry has `Status: {m.group(1)}`, not "
                f"OPEN; answered entries move to the archive index:\n"
                f"      {heading[:110]}"
            )

    return f


# ---------------------------------------------------------------------------
# Self-test.  Every rule gets a fixture that makes it fire, because a rule that
# has never been seen to fire is a rule you are guessing about -- and a clean
# fixture too, because a checker that fails on everything is as useless as one
# that fails on nothing.
# ---------------------------------------------------------------------------

def _doc(body: str, index: str = "") -> str:
    """A minimal but *plausible* document: above the floors, so a fixture
    tests the rule it is aimed at rather than tripping the floor first."""
    filler = "\n\n".join(
        f"## A-Q{n} — [A] Filler question {n}? — Status: OPEN\n\n**In short:** x."
        for n in range(80, 80 + MIN_BODY_ENTRIES)
    )
    idx = "\n".join(f"- Q{n} something — resolved 2026-01-01 (§{n})"
                    for n in range(1, MIN_INDEX_ENTRIES + 1))
    return (
        "# Open Questions\n\n"
        f"{filler}\n\n{body}\n\n"
        f"{RESOLVED_HEADING}\n\n"
        "## Resolved — lane A\n\n"
        f"{idx}\n{index}\n"
    )


def self_test() -> int:
    failures: list[str] = []
    count = 0

    def check(label: str, condition: bool) -> None:
        nonlocal count
        count += 1
        if not condition:
            failures.append(label)
            print(f"FAIL {label}")

    def raises(label: str, text: str, needle: str) -> None:
        nonlocal count
        count += 1
        try:
            analyse(text)
        except ValueError as exc:
            if needle not in str(exc):
                failures.append(f"{label} (message lacked {needle!r})")
                print(f"FAIL {label}: message lacked {needle!r}: {exc}")
            return
        failures.append(f"{label} (did not raise)")
        print(f"FAIL {label}: did not raise")

    clean = _doc("## A-Q9 — [A] A real question? — Status: OPEN\n\n**In short:** y.")
    f = analyse(clean)
    check("a clean document has no failures", f.ok())
    check("...and no warnings", not f.warnings)

    # 1. The defect that motivated the file: an open entry below the boundary.
    stranded = clean.replace(
        "## Resolved — lane A\n",
        "## Resolved — lane A\n\n## A-Q10 — [A] Stranded? — Status: OPEN\n",
    )
    f = analyse(stranded)
    check("an OPEN entry below `# Resolved` fails",
          any("filed below" in d for d in f.failures))
    check("...and the failure names its line",
          any("A-Q10" in d for d in f.failures))

    # The archive's own subsections must not be mistaken for stranded entries;
    # that false positive would fire on every correct file in the tree.
    f = analyse(clean.replace("## Resolved — lane A",
                              "## Resolved — lane A\n\n## Resolved — lane B"))
    check("an archive subsection is not a stranded question", f.ok())
    f = analyse(clean.replace("## Resolved — lane A", "## Resolved - lane A"))
    check("...spelled with a hyphen either", f.ok())

    # 2. The Q57 collision, in both of its shapes.
    f = analyse(_doc("## A-Q9 — [A] One? — Status: OPEN\n\n"
                     "## A-Q9 — [A] Two? — Status: OPEN"))
    check("two body entries sharing an identifier fail",
          any("`A-Q9` is used 2 times" in d for d in f.failures))
    f = analyse(_doc("## Q3 — [A] Reused? — Status: OPEN"))
    check("a body entry reusing an archived number fails",
          any("`Q3` is used 2 times" in d for d in f.failures))
    check("...and says where both are",
          any("(body)" in d and "(archive)" in d for d in f.failures))

    # Two archived entries sharing a number is history, not a live ambiguity:
    # nothing can be mis-answered, and the fix would be editing the record.
    f = analyse(_doc("## A-Q9 — [A] Fine? — Status: OPEN",
                     index="\n- Q3 a second thing also called Q3 — resolved"))
    check("an archive-only collision is a warning",
          any("historic" in w for w in f.warnings))
    check("...and does not fail the build", f.ok())

    # A number mentioned in the archive's prose is not a filing.
    f = analyse(_doc("## A-Q9 — [A] Fine? — Status: OPEN",
                     index="\nSee Q3 for the reasoning behind this."))
    check("a number in prose is not read as a second filing", f.ok())

    # 3. An answered entry left in the body.
    f = analyse(_doc("## A-Q9 — [A] Done? — Status: RESOLVED"))
    check("an answered entry in the body fails",
          any("not\nOPEN" in d or "not OPEN" in d.replace("\n", " ")
              for d in f.failures))

    # Warnings are counted, never fatal -- a hard failure here would let this
    # lane's gate red another lane's boot test over heading formatting.
    f = analyse(_doc("## An unnumbered question? (lane C)"))
    check("a missing identifier is a warning", len(f.warnings) == 1)
    check("...and does not fail the build", f.ok())

    # The floors.
    raises("an empty body refuses to return a verdict",
           f"# Open Questions\n\n{RESOLVED_HEADING}\n\n## Resolved — lane A\n"
           + "\n".join(f"- Q{n} x — resolved" for n in range(1, 20)),
           "below the floor")
    raises("a thin archive refuses to return a verdict",
           _doc("## A-Q9 — [A] x? — Status: OPEN").split(RESOLVED_HEADING)[0]
           + RESOLVED_HEADING + "\n\n## Resolved — lane A\n\n- Q1 x — resolved\n",
           "archive entr")
    raises("a missing boundary refuses to return a verdict",
           "# Open Questions\n\n## A-Q1 — [A] x? — Status: OPEN\n",
           "archive boundary")

    # The answers-file matcher. Its first version compared whole headings and
    # so reported nothing, which is why this is a function and not four lines.
    _body = [(1, "## A-Q10 \u2014 [A] x? \u2014 Status: OPEN"),
             (2, "## B-Q8 \u2014 [B] y?"),
             (3, "## C-Q11: colon form (raised by lane C)")]
    _ans = "A-Q10: do B" + "\n" + "Q46: c" + "\n" + "C-Q11: yes"
    _got = answered_still_open(_ans, _body)
    check("an answered id that is still open is reported", "A-Q10" in _got)
    check("the colon heading form is matched too", "C-Q11" in _got)
    check("an open question with no answer is not reported", "B-Q8" not in _got)
    check("an answer for nothing open here is not reported", "Q46" not in _got)
    check("a heading without ## does not yield an id",
          answered_still_open("A-Q10: x", [(1, "A-Q10 no hashes")]) == [])

    if failures:
        print(f"\n{len(failures)} of {count} self-test(s) FAILED")
        return 1
    print(f"check-open-questions: self-test passed ({count} checks)")
    return 0


NL_ = chr(10)
ANSWERS = "open-questions-answers.txt"


def answered_still_open(answers_text: str, open_idents: list) -> list:
    """IDs the operator has answered that are still open in the queue.

    Pure and separate from the I/O around it, because the first version did
    this inline against `f.body` -- (line, full heading) pairs, not bare
    identifiers -- so it matched nothing and said 'no answer matches a question
    still open'. A silent no-match reads exactly like a true negative, which is
    the defect this gate family exists to catch."""
    answered = set(re.findall(
        r"(?m)^([A-Za-z]*-?Q[0-9]+)\s*:", answers_text))
    head = re.compile(r"^##\s+([A-Za-z]*-?Q[0-9]+)")
    here = set()
    for entry in open_idents:
        text = entry[1] if isinstance(entry, tuple) else entry
        found = head.match(text)
        if found:
            here.add(found.group(1))
    return sorted(here & answered)

def answers_notice(open_idents: list) -> None:
    """Say so, once per lane, when the operator's answers file changes."""
    try:
        common = subprocess.run(
            ["git", "rev-parse", "--git-common-dir"],
            capture_output=True, text=True, check=True).stdout.strip()
        common = pathlib.Path(common).resolve()
        answers = common.parent / ANSWERS
        if not answers.is_file():
            return
        raw = answers.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()[:16]
        branch = subprocess.run(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"],
            capture_output=True, text=True, check=True).stdout.strip()
        mark = common / "coordination" / ("answers-seen-" + (branch or "detached"))
        prev = mark.read_text(encoding="utf-8").strip() if mark.is_file() else ""
        if prev == digest:
            return

        # Which answered IDs are still in the queue: the actionable half.
        still_open = answered_still_open(
            raw.decode("utf-8", "replace"), open_idents)

        print("")
        print("=== OPERATOR ANSWERS: %s has changed ===" % ANSWERS)
        print("    %s" % answers)
        if still_open:
            print("    %d answer(s) for questions still open here: %s"
                  % (len(still_open), ", ".join(still_open)))
            print("    Move each to design-decisions.md (Decided by: Operator) and")
            print("    remove it from open-questions.md.")
        else:
            print("    No answer in it matches a question still open on this branch.")
        print("    This notice fires once per branch; it is recorded now.")
        print("")
        mark.parent.mkdir(parents=True, exist_ok=True)
        # newline="": without it Windows writes CRLF here, so the digest read
        # back never matches the one written and the notice repeats forever.
        # check-text-mode-writes refuses the build over exactly this, and did.
        # digest read back never matches the one written. check-text-mode-writes
        # refuses the build over exactly this, and did.
        mark.write_text(digest + NL_, encoding="utf-8", newline="")
    except (OSError, subprocess.SubprocessError, UnicodeError):
        # Silence, never a refusal: see this function's rationale.
        return


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()

    root = pathlib.Path(__file__).resolve().parent.parent
    path = root / DOC
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"check-open-questions: cannot read {path}: {exc}", file=sys.stderr)
        return 2

    try:
        f = analyse(text)
    except ValueError as exc:
        print(f"check-open-questions: cannot check {DOC}: {exc}", file=sys.stderr)
        return 2

    for w in f.warnings:
        print(f"  warning: {w}")
    if f.failures:
        print(f"\n{DOC}: {len(f.failures)} problem(s):", file=sys.stderr)
        for d in f.failures:
            print(f"  - {d}", file=sys.stderr)
        return 1

    answers_notice(f.body)

    tail = (f", {len(f.warnings)} warning(s)" if f.warnings else ", no warnings")
    print(f"check-open-questions: OK ({len(f.body)} open, "
          f"{len(f.index_idents)} archived{tail})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
