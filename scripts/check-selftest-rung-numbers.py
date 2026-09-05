#!/usr/bin/env python3
"""Guard the numbering that the whole self-test log is read by.

Every rung of `kshell::self_test` announces itself on the serial log as

    kshell::self_test <N>: <what this rung pins>

and those numbers are the only handle anyone has on a 103-rung log.  A boot
report says "rung 67 failed"; `known-issues.md` says "rung 79 cleared the 21
`matches!` sites"; a batch entry says "pinned by rung 103".  All of that is
indexing into the log *by number*, so the numbering has to be a real index:
unique, contiguous, and in the order the rungs run.

Three ways it stops being one, all of which have a cheap mechanical signature:

  **duplicate**  Two rungs numbered the same.  This is the likely one, because
  it is what near-simultaneous batches produce: two rungs are drafted against
  the same tip, both take the next free number, and the merge keeps both.  The
  log then has two "rung 103" lines and a failure report naming 103 points at
  either -- so the reference that was supposed to locate the failure is what
  makes it ambiguous.  Nothing else catches this: both rungs compile, both run,
  both pass, and the duplication is visible only by reading 103 banners.

  **gap**  A rung deleted or renumbered leaves a hole.  A hole is not harmful
  on its own, but it silently breaks the one invariant that makes the count
  meaningful -- with no gaps, "the log ended at 103" and "103 rungs ran" are
  the same statement, and a truncated log is obvious.  With a gap they diverge,
  and a boot that died halfway through the rungs looks like a boot that ran
  them all.

  **out of order**  A rung whose number is lower than one above it in the file.
  The log is written in file order, so this makes the printed sequence
  non-monotonic, and "the log reached rung N" stops implying "rungs 1..N ran".

  **dangling citation**  A prose reference to a rung that the log never
  announces.  This is the same defect as the unbannered rung below, caught from
  the direction it actually hurts: nobody is harmed by rung 119 not printing in
  the abstract, they are harmed by `known-issues.md` offering rung 119 as the
  evidence for a closed defect and the log containing no such thing.  It also
  fires on a rung deleted while still cited, a rung renumbered by a merge, and
  a typo'd number -- none of which any other rule here notices.

What this gate deliberately does NOT check, because it cannot
--------------------------------------------------------------

It does not check that every block of assertions *has* a banner.  That was the
bug that prompted this script -- rung 103's assertions were committed with no
banner, so they ran and passed while being invisible in the log, and deleting
the entire block would have left the serial output byte-identical.  It is not
statically detectable.  A sibling rung missing its banner and a legitimate
scoping sub-block are the same syntax: a brace-block at the function's top
level containing assertions.  `self_test` has two real ones (a block that saves
and restores `KSHELL_SELFTEST_VAR`, and one that shadows a `PrintfSpec`), and
no rule separates them from an unbannered rung without reading intent.

So the mitigation for that failure mode is not a gate but the log itself: after
adding a rung, read the boot's serial output and confirm the new banner appears
where it should.  A gate that guessed at this would either miss the real cases
or condemn the two legitimate blocks, and DD 635 is explicit that a gate is
narrowed until it can start at zero rather than broadened and given a backlog.

**The citation rule above is not an exception to that paragraph, and the
distinction is worth stating so the next reader does not take this section as
having already ruled it out.**  "Does every block have a banner" has to *guess*
which brace-blocks are rungs.  The citation rule guesses nothing: it reads
numbers a human wrote down deliberately -- in a `// Rung N` comment or in prose
-- and asks only whether they resolve.  There is no intent to infer.

That distinction was earned, not assumed.  On 2026-09-05 rungs 116-119 were
found to run with no banner at all, declaring themselves with a `// --- N:`
comment that prints nothing; five citations across `kshell.rs` and
`known-issues.md` pointed at rungs the log did not contain, two of them offered
as the evidence for closed defects.  The first rule drafted for this was
"a `// --- N:` marker implies a banner", and it was rejected on measurement:
it would have protected only the four rungs that already carried a marker,
left a second naming convention in the file permanently, and done nothing for
rung 120 onward.

One consequence, deliberate and worth knowing before it surprises you: **you
cannot cite a rung by number before it exists.**  A design note that says "rung
120 will pin this" fails the gate until rung 120 is announced.  That is the
rule working rather than misfiring -- a reader who greps for rung 120 today
finds nothing, which is precisely the failure being gated -- but the remedy is
not obvious in the moment, so: write "the next rung", and put the number in
once the banner is in.  This was found the honest way, by the gate rejecting
the very `design-decisions.md` entry that introduced it (§912), which had
forward-referenced rung 120 in an argument about why marker-based gating would
not protect it.

Scoping the markdown corpus was measured too, not guessed.  Qualifying a
citation by the literal `kshell::self_test` on the *same line* is the tighter
rule and the wrong one: it qualified 45 of 160 citations and found 1 of the 4
dangling rungs, because the citations that dangle are ordinary prose ("Rung 116
still passes unchanged") and prose does not repeat the fully-qualified name of
the function it is discussing.  Qualifying by the enclosing `###` section
mentioning kshell qualified 150 of 160 and found 4 of 4, at no measurable cost
in precision: 146 of its 150 resolve, and `design-decisions.md` qualifies 18
with zero dangling.  Section scope is what is implemented below.

Exit status: 0 clean, 1 findings, 2 could not read the file.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

BANNER = re.compile(r'kshell::self_test (\d+):')

# A citation inside kshell.rs itself: any `// Rung N`, declaration or
# cross-reference.  The two cannot be told apart -- 13 of the 20 uses declare a
# rung (`// Rung N -- ...`) and 7 cite another one (`// Rung N's ...`,
# `// Rung N established ...`), separated only by punctuation -- but the
# distinction is irrelevant here.  Either way the number must name a rung that
# exists, so this rule needs no rule for telling them apart.  (A gate that
# treated them as declarations *would* need one, and would report "rung 33"
# cited far below rung 33 as a bogus out-of-order finding.)
SRC_CITE = re.compile(r'//\s*[Rr]ung (\d+)')

# A citation in shared markdown.  Deliberately bare: see the module docstring
# for why the qualifier is the enclosing section rather than the same line.
MD_CITE = re.compile(r'[Rr]ung (\d+)')

# The shared documents are worked by three lanes, so a bare `rung 12` may be
# another lane's ladder.  Only these two were measured; per DD 635 the corpus
# starts at what is known to land clean rather than at everything that might.
MD_DOCS = ("known-issues.md", "design-decisions.md")

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "kernel" / "src" / "kshell.rs"


def banners(text: str) -> list[tuple[int, int]]:
    """Return [(rung_number, line_number)] in file order."""
    out = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        for m in BANNER.finditer(line):
            out.append((int(m.group(1)), lineno))
    return out


def check(text: str) -> list[str]:
    found = banners(text)
    if not found:
        return ["no `kshell::self_test <N>:` banners found at all -- either the "
                "convention changed or this gate is looking at the wrong file"]

    findings: list[str] = []
    nums = [n for n, _ in found]

    seen: dict[int, int] = {}
    for n, line in found:
        if n in seen:
            findings.append(
                f"rung {n} is announced twice: line {seen[n]} and line {line}. "
                f"A failure report naming rung {n} points at either one."
            )
        else:
            seen[n] = line

    top = max(nums)
    missing = [n for n in range(1, top + 1) if n not in seen]
    if missing:
        shown = ", ".join(str(n) for n in missing[:20])
        more = f" (and {len(missing) - 20} more)" if len(missing) > 20 else ""
        findings.append(
            f"the numbering runs to {top} but {len(missing)} number(s) are "
            f"missing: {shown}{more}. With a gap, 'the log reached rung {top}' "
            f"no longer means 'all {top} rungs ran'."
        )

    for (a, la), (b, lb) in zip(found, found[1:]):
        if b < a:
            findings.append(
                f"rung {b} (line {lb}) is announced after rung {a} (line {la}), "
                f"so the log prints them out of order and reaching rung N stops "
                f"implying rungs 1..N ran."
            )

    return findings


def kshell_sections(lines: list[str]) -> list[bool]:
    """Per line, whether its enclosing `###` section mentions kshell.

    Boundaries are lines starting with `###`, which is what the calibration in
    the docstring measured.  `##` deliberately does not split: in
    `design-decisions.md` every numbered entry is a `##`, and splitting there
    would scope each citation to its own entry and lose the surrounding
    context that identifies the lane.

    The match is case-insensitive because the issue headings are upper-case
    (`### A-KSHELL-TAB-COMPLETION-...`) while the prose is lower-case.  A
    case-sensitive test silently drops any kshell section whose body never
    happens to spell the name in lower case.  Measured on the real corpus
    rather than assumed: case-insensitive qualifies 169 of 180 citations
    against case-sensitive's 168, and both find zero dangling -- so the
    broader test costs nothing and closes that hole.
    """
    bounds: list[tuple[int, int]] = []
    start = 0
    for i, line in enumerate(lines):
        if line.startswith("###"):
            bounds.append((start, i))
            start = i
    bounds.append((start, len(lines)))

    flags = [False] * len(lines)
    for a, b in bounds:
        if "kshell" in "\n".join(lines[a:b]).lower():
            for i in range(a, b):
                flags[i] = True
    return flags


def check_citations(
    announced: set[int],
    src_text: str,
    md_docs: list[tuple[str, str]],
) -> list[str]:
    """Every rung *citation* must name a rung that is actually announced."""
    findings: list[str] = []

    for lineno, line in enumerate(src_text.splitlines(), start=1):
        for m in SRC_CITE.finditer(line):
            n = int(m.group(1))
            if n not in announced:
                findings.append(
                    f"kernel/src/kshell.rs:{lineno} cites rung {n}, which no "
                    f"banner announces. Either the comment's number is wrong or "
                    f"the rung it names prints nothing, in which case grepping "
                    f"a boot log for it finds nothing."
                )

    for name, text in md_docs:
        lines = text.splitlines()
        in_kshell = kshell_sections(lines)
        for lineno, line in enumerate(lines, start=1):
            if not in_kshell[lineno - 1]:
                continue
            for m in MD_CITE.finditer(line):
                n = int(m.group(1))
                if n not in announced:
                    findings.append(
                        f"{name}:{lineno} cites rung {n} in a section about "
                        f"kshell, but no banner announces rung {n}. The evidence "
                        f"this line offers cannot be checked by the route it "
                        f"tells the reader to take."
                    )

    return findings


# A fixture, for the same reason every other gate here has one: this checker
# reports a clean tree and a checker that has stopped finding anything in
# identical words.  Each case is one of the three failure modes, plus a control
# that must stay silent.
_FIXTURE_OK = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 2: second");
    serial_println!(
        "  kshell::self_test 3: a banner wrapped across lines by rustfmt, which \\
         is how most of the real ones are written"
    );
'''

_FIXTURE_DUP = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 2: second");
    serial_println!("  kshell::self_test 2: second again, from a parallel batch");
'''

_FIXTURE_GAP = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 3: third, with 2 deleted");
'''

_FIXTURE_ORDER = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 3: third");
    serial_println!("  kshell::self_test 2: second, below third");
'''

# Citation fixtures.  Both directions are covered on purpose: a checker that
# reports nothing because its regex never matches looks exactly like a clean
# tree, so "must report" cases alone would not distinguish the two.
_CITE_ANNOUNCED = {1, 2, 3}

_CITE_SRC_OK = '''
    // Rung 2 -- declares itself, and 2 is announced.
    // Rung 1 covers where the word begins; this covers what goes there.
'''

_CITE_SRC_DANGLING = '''
    // Rung 2 -- fine.
    // Rung 9 covers the other half.
'''

# The `###` heading mentions kshell, so citations below it are in scope.
_CITE_MD_OK = '''
### A-KSHELL-SOMETHING-WAS-WRONG

Pinned by rung 3, which still passes unchanged.
'''

_CITE_MD_DANGLING = '''
### A-KSHELL-SOMETHING-WAS-WRONG

Pinned by rung 7, which is offered here as the evidence.
'''

# The control that makes section-scope meaningful rather than a bare grep: a
# rung number in a section with nothing to do with kshell is another lane's
# ladder and must stay silent even though 7 is not announced.
_CITE_MD_OTHER_LANE = '''
### B-SOMETHING-IN-THE-POSIX-LAYER

Pinned by rung 7 of the libc conformance ladder.
'''


def self_test() -> int:
    cases = [
        ("clean numbering", _FIXTURE_OK, False),
        ("a duplicated rung number", _FIXTURE_DUP, True),
        ("a hole in the numbering", _FIXTURE_GAP, True),
        ("a rung announced out of order", _FIXTURE_ORDER, True),
    ]
    bad = 0
    for name, text, should_report in cases:
        got = bool(check(text))
        if got != should_report:
            verb = "reported nothing for" if should_report else "reported"
            print(f"SELF-TEST FAIL: {verb} {name}", file=sys.stderr)
            bad += 1

    cite_cases = [
        ("a source comment citing an announced rung",
         _CITE_SRC_OK, [], False),
        ("a source comment citing a rung that is never announced",
         _CITE_SRC_DANGLING, [], True),
        ("prose citing an announced rung",
         "", [("known-issues.md", _CITE_MD_OK)], False),
        ("prose citing a rung that is never announced",
         "", [("known-issues.md", _CITE_MD_DANGLING)], True),
        ("a rung number in a section that is not about kshell",
         "", [("known-issues.md", _CITE_MD_OTHER_LANE)], False),
    ]
    for name, src, docs, should_report in cite_cases:
        got = bool(check_citations(_CITE_ANNOUNCED, src, docs))
        if got != should_report:
            verb = "reported nothing for" if should_report else "reported"
            print(f"SELF-TEST FAIL: {verb} {name}", file=sys.stderr)
            bad += 1

    if bad:
        print(
            f"\n{bad} fixture case(s) disagree with the checker. Its verdict on "
            f"kshell.rs means nothing until they agree.",
            file=sys.stderr,
        )
        return 1
    print("self-test OK: the rung-numbering gate reports all 3 broken fixtures "
          "and not the clean one; the citation rule reports both dangling cases, "
          "and stays silent on both resolving ones and on another lane's ladder")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()

    try:
        text = TARGET.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"cannot read {TARGET}: {exc}", file=sys.stderr)
        return 2

    findings = check(text)
    for f in findings:
        print(f"kshell.rs: {f}", file=sys.stderr)

    # Citations are only meaningful against a numbering that is itself sound,
    # but they are still worth reporting when it is not: a duplicate does not
    # make a dangling citation less dangling, and reporting both at once saves
    # a second run.
    announced = {n for n, _ in banners(text)}
    md_docs: list[tuple[str, str]] = []
    for name in MD_DOCS:
        p = ROOT / name
        if not p.exists():
            # Not fatal: the shared documents live at the repo root and a
            # scratch checkout may not have them.  Say so rather than silently
            # checking a smaller corpus and reporting the same clean verdict.
            print(f"note: {name} not found, so its citations were not checked",
                  file=sys.stderr)
            continue
        md_docs.append((name, p.read_text(encoding="utf-8", errors="replace")))

    cite_findings = check_citations(announced, text, md_docs)
    for f in cite_findings:
        print(f, file=sys.stderr)

    if findings or cite_findings:
        return 1

    total = len(banners(text))
    print(f"self-test rung numbering OK: {total} rung(s), numbered 1..{total} "
          f"with no duplicate, no gap and none out of order; every rung "
          f"citation in kshell.rs and in {len(md_docs)} shared document(s) "
          f"names a rung that is announced")
    return 0


if __name__ == "__main__":
    sys.exit(main())
