r"""Which of our own documents credit a program with an act it cannot perform?

The fifth of the set, and the first that reads the documents rather than the
code:

  find-reachable-fixtures.py    programs that say too much
  find-silent-incapacity.py     programs that say too little
  find-stranded-serialisers.py  work that is finished and cannot be used
  find-stale-admissions.py      a denial that used to be true
  this one                      a record that was never true

`apps/renamer` is the case that prompted it. Its status line said

    Renamed {count} files

while `apply_plan` edited a `Vec<FileEntry>` and the crate contained no
`std::fs` at all. That much the silence scanner could in principle have led
someone to. What it could not reach is that `known-issues.md` -- the file whose
whole job is to track claims that outrun the code -- opened the renamer's own
entry with

    The bulk renamer can now be given files and rules, and can actually rename.

The author meant *the rename action is reachable from a key*. What they wrote
credits the program with renaming files. **A tracking document that repeats the
program's false claim is worse than the claim**, because it is the artefact a
later reader consults specifically to find out whether the claim is true, and
it is the one place a sweep looks to decide an app is done. `sysinfo` sat on a
FIXED list while three of its controls were still lying; this is how that
happens twice.

HOW IT DECIDES

Two halves, and both must fire:

1.  **The entry credits an external act.** Not any mention of a verb -- the
    phrasing has to award the capability: "can actually rename", "can now
    save", "now writes". A sentence that merely *describes* the act ("the
    rename applies rules in order") is not a credit and is not reported.

2.  **The crate it names has no external capability whatsoever** -- no
    `std::fs`, no `safeio::`, no socket, no dialog, in production code. This is
    deliberately the blunt version of the question. Checking that a *specific*
    verb works would need to know which call implements "rename", which is the
    kind of guess that makes a checker wrong quietly. A program that reaches
    nothing at all cannot have done the thing any credit describes, whatever
    the verb.

The capability predicate is imported from `find-silent-incapacity.py` rather
than restated. Two derivations of "can this program reach the outside world" is
how two scanners come to disagree, and the disagreement would surface as this
one going quiet.

Report-only, and no --check mode, like the rest of the set: an entry may be
worded loosely and still be understood by everyone who reads it, and a gate
would train the next reader to reword rather than to look.

Usage:  python scripts/find-overstated-records.py [--docs=known-issues.md,roadmap.md]
"""

import importlib.util
import pathlib
import re
import sys

from rustlex import live_code

ROOT = pathlib.Path(__file__).resolve().parent.parent


def _capability_predicate():
    """The CAPABILITY regex from the silence scanner, not a copy of it."""
    path = ROOT / "scripts" / "find-silent-incapacity.py"
    spec = importlib.util.spec_from_file_location("_silent_incapacity", path)
    if spec is None or spec.loader is None:
        print(f"cannot load {path}", file=sys.stderr)
        raise SystemExit(2)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.CAPABILITY


CAPABILITY = _capability_predicate()

# Verbs for acts a process cannot perform on its own.
#
# The first draft also held `open`, `record`, `print`, `copy`, `load` and
# `connect`, and reported twelve entries of which ONE was real. Six were "now
# opens a real window", one was "the comment now records the decision", one was
# "those sites now print CANNOT TELL". **Opening a window, recording a decision
# in a comment and printing into a render tree are all things a process does
# entirely to itself.** A verb belongs here only if it has no in-process
# reading, because the whole check is "this program reaches nothing, so it
# cannot have done this".
#
# That is the silence scanner's lesson arriving from the other side. Its
# vocabulary had to be *widened* until it stopped accusing honest code; this one
# had to be *narrowed* for the same reason. Measure what a change clears and
# read the difference before keeping it.
# Split by whether the verb needs an object to be an external act at all.
#
# `renam`/`download`/`upload` have no in-process reading worth the name: a
# program that renames has renamed something outside itself. `writ`/`sav`/
# `export`/`import`/`delet`/`scan` are ordinary words for things done to memory
# -- "the sweep now writes a different test name", "deletes the entry from the
# list", "scans the string" -- and taking them bare produced exactly those
# false accusations. They count only with an external object nearby.
ACT_ALONE = r"renam\w*|download\w*|upload\w*"
ACT_WITH_OBJECT = r"sav\w*|writ\w*|export\w*|import\w*|delet\w*|scan\w*"
OBJECT = (
    r"(?:\W+\w+){0,3}?\W+(?:files?|disks?|drives?|paths?|director(?:y|ies)|folders?"
    r"|filesystems?|discs?|to disk|to file|on disk|\.[a-z]{2,4}\b)"
)
ACT = ACT_ALONE + r"|" + ACT_WITH_OBJECT

# Words that turn a credit into a plan or a denial. "`fs::rename` WHEN this is
# wired to a real filesystem" describes what will be true later; "WHILE neither
# is wired to the other" describes what is not true now. A fixed-width
# lookbehind cannot reach them -- `re` has no variable-length lookbehind and
# there are words in between -- so the run-up to each match is checked here.
CONDITIONAL = re.compile(
    r"\b(when|until|once|if|unless|while|neither|before|after|should|would|could"
    r"|plan|planned|intend|intended|plans to|plan is|plan was)\b",
    re.I,
)

def _credit(act, needs_object):
    tail = OBJECT if needs_object else r"\b"
    return (
        r"can (?:now|actually|finally) (?:\w+\s+){0,3}?(?:" + act + r")" + tail
        + r"|(?:now|actually) (?:" + act + r")" + tail
        + r"|does (?:now )?(?:" + act + r")" + tail
    )


CREDIT = re.compile(
    _credit(ACT_ALONE, False) + r"|" + _credit(ACT_WITH_OBJECT, True) + r"|is (?:now )?wired to",
    re.I,
)


def crates(root="apps"):
    """Crate name -> whether its production code reaches outside the process."""
    out = {}
    base = ROOT / root
    if not base.is_dir():
        return out
    for crate in sorted(p for p in base.iterdir() if (p / "src").is_dir()):
        prod = "".join(
            live_code(f.read_text(encoding="utf-8", errors="replace"))[0]
            for f in sorted((crate / "src").rglob("*.rs"))
        )
        out[crate.name] = bool(CAPABILITY.search(prod))
    return out


def entries(text):
    """Split a markdown document into (heading, body) at any heading level."""
    out, head, body = [], None, []
    for line in text.splitlines():
        if line.startswith("#"):
            if head is not None:
                out.append((head, "\n".join(body)))
            head, body = line.lstrip("#").strip(), []
        elif head is not None:
            body.append(line)
    if head is not None:
        out.append((head, "\n".join(body)))
    return out


# An entry slug: two or more ALL-CAPS words joined by dashes, as every
# `TD-C-...`/`C-...` heading in `known-issues.md` is.
SLUG = re.compile(r"[A-Z][A-Z0-9]*-[A-Z][A-Z0-9]*-[A-Z]")


def named_crate(heading, line, known):
    """The crate a crediting LINE is about, if exactly one is identifiable.

    Attribution is per-line, not per-section. Attributing from the whole body
    works for `known-issues.md`, where an entry is a few paragraphs about one
    program, and fails badly for `roadmap.md`, where one heading -- "Lane B --
    POSIX & Userland" -- covers hundreds of lines about dozens of crates. Any
    crate named anywhere under it became the subject of every credit in it,
    which filed lane B's `UserDb::save now writes` under `apps/terminal`.

    Ambiguity is dropped rather than guessed: a line naming three apps reports
    none, because reporting the first would attach the finding to whichever
    happened to sort earliest.
    """
    found = {c for c in known if re.search(rf"\bapps/{re.escape(c)}\b", line)}
    if not found and SLUG.search(heading):
        # The heading slug: TD-C-RENAMER-CAN-ONLY-... names its subject. Only
        # slug-shaped headings qualify -- a prose heading ("The fix") can
        # contain a crate name incidentally.
        words = set(re.split(r"[^A-Za-z0-9]+", heading.lower()))
        found = {c for c in known if c in words}
    return next(iter(found)) if len(found) == 1 else None


# Every case below was measured against the real documents while this checker
# was being narrowed, not invented afterwards. The first draft reported twelve
# entries and one was real; these are the other eleven, in the shape they
# actually appeared. A later widening of the vocabulary that is worth keeping
# will leave them all silent.
SELF_TEST = [
    # (line, heading, should_fire, why)
    (
        "The bulk renamer can now be given files and rules, and can actually rename.",
        "TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING",
        True,
        "the real one: credits the act, program reaches nothing",
    ),
    (
        "**In short.** The finance app now opens a window and responds to the keyboard,",
        "TD-C-FINANCE-IS-A-VIEWER-OVER-SAMPLE-DATA",
        False,
        "opening a window is done entirely inside the process",
    ),
    (
        "**In short:** The PDF viewer now opens as a real window and everything in it",
        "C-PDFVIEWER-HAS-NO-PDF-PARSER-AND-NO-PRINT-SPOOLER",
        False,
        "same shape, five more of these were reported",
    ),
    (
        "The comment now records the decision, why accepting stays (it is the fallback",
        "TD-C-DESIGN-DECISION-818-HAS-NOWHERE-TO-BE-IMPLEMENTED",
        False,
        "a comment recording something is not an external act",
    ),
    (
        "So those sites now print `CANNOT TELL WHICH METHOD` rather than `NOT INKED`.",
        "TD-C-FORTY-NINE-COLOUR-METHODS-ARE-INVISIBLE-TO-THE-INK-SWEEP",
        False,
        "printing into a render tree is not printing",
    ),
    (
        "`fs::rename` when this is wired to a real filesystem, which is the point:",
        "B-RENAMER-A-BULK-RENAME-COULD-DESTROY-A-FILE",
        False,
        "a plan, not a claim -- the conditional is four words back",
    ),
    (
        "crate while neither is wired to the other costs nothing; doing it afterwards",
        "The fix",
        False,
        "a denial, and the heading is prose so it names no subject either",
    ),
    (
        "sweeps this campaign now writes: not `nothing_is_painted_entirely_outside`,",
        "C-ALARMCLOCK-SCROLLS-BY-CLIP-ALONE",
        False,
        "what a test now writes is a test name",
    ),
    (
        "- `[x]` **2. Generate on change**. `UserDb::save` now writes the file",
        "Lane B - POSIX & Userland",
        False,
        "a prose section heading names no crate; this was filed under apps/terminal",
    ),
    (
        "The renamer can now save files to disk.",
        "TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING",
        True,
        "an object-taking verb WITH its object still fires",
    ),
]


def self_test():
    """Check the predicate against the shapes it was narrowed on.

    Against a **synthetic** capability map, not the tree's. The first version
    of this used the real one and went red the same afternoon -- because the
    finding it was built from got fixed, so `renamer` acquired `std::fs` and
    the two positive cases stopped firing. A self-test that passes or fails
    depending on the code it happens to be sitting next to is testing the tree,
    not the predicate, and the tree is the part that is supposed to change.
    """
    # Only the crate names the cases actually name. Deriving this from the
    # headings' words instead made every word a candidate crate -- "td", "c",
    # "only" -- so attribution was ambiguous for every case and none fired.
    capable = {
        c: False
        for c in (
            "renamer", "finance", "pdfviewer", "lockscreen",
            "alarmclock", "weather", "terminal",
        )
    }
    bad = 0
    for line, heading, should, why in SELF_TEST:
        m = CREDIT.search(line)
        fired = bool(m) and not (m and CONDITIONAL.search(line[: m.start()]))
        if fired:
            crate = named_crate(heading, line, capable)
            fired = crate is not None and not capable.get(crate, True)
        if fired != should:
            bad += 1
            verb = "fired" if fired else "stayed silent"
            print(f"  FAIL  {verb}, expected the opposite -- {why}")
            print(f"        {line[:88]}")
    print(f"{len(SELF_TEST) - bad}/{len(SELF_TEST)} self-test case(s) as expected")
    return 1 if bad else 0


def main():
    docs = ["known-issues.md", "roadmap.md", "todo.txt"]
    for arg in sys.argv[1:]:
        if arg.startswith("--docs="):
            docs = [d for d in arg.split("=", 1)[1].split(",") if d]
        elif arg == "--self-test":
            return self_test()
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    capable = crates()
    if not capable:
        print("no apps/ crates found -- refusing to call that a pass", file=sys.stderr)
        return 2

    findings, scanned = [], 0
    for doc in docs:
        path = ROOT / doc
        if not path.is_file():
            continue
        for heading, body in entries(path.read_text(encoding="utf-8", errors="replace")):
            scanned += 1
            for line in body.splitlines():
                m = CREDIT.search(line)
                if not m:
                    continue
                # A conditional anywhere before the phrase, on the same line,
                # makes it a plan rather than a claim.
                if CONDITIONAL.search(line[: m.start()]):
                    continue
                crate = named_crate(heading, line, capable)
                if crate is None or capable[crate]:
                    continue
                findings.append((doc, crate, heading, line.strip(), m.group(0)))
                break

    incapable = sum(1 for v in capable.values() if not v)
    print(
        f"{len(findings)} record(s) credit an act to a program that reaches nothing\n"
        f"({scanned} entries across {len(docs)} document(s); "
        f"{incapable} of {len(capable)} apps/ crates reach nothing outside the process)\n"
    )
    if findings:
        print("  READ THE ENTRY. Loose wording about an in-memory action trips")
        print("  this too, and rewording it is not the same as fixing the program.\n")
    for doc, crate, heading, line, hit in findings:
        print(f"    {doc}  [{crate}]  {heading[:60]}")
        print(f"      credits: {hit!r}")
        print(f"      in: {line[:100]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
