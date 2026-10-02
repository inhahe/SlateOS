## 631. The numbering bands are enforced by a gate that grandfathers everything already here, and reads the band table out of this file

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane A) — lanes B and C both asked for it
**Lane:** A

**In short:** This document is written by three parallel agents. Each takes
section numbers from its own reserved range so the three of them edit three
different places in the file and never collide. That rule was kept by nothing
but attention, and attention lost twice: two lanes wrote a section 626 on the
same day, and — discovered while writing this gate — nine *other* numbers
(268 through 276) each label two different sections and have done for weeks.
`scripts/boot-test.sh` now refuses to build if a newly-added section breaks the
rule. Nothing already in the file is touched or renumbered.

**Why a gate at all.** The cost of the failure is not the conflict; it is that
git cannot report it. When lanes A and B both wrote 626, git produced a
350-line `CONFLICT (content)` that named the file and the lines and never
mentioned that two sections now had the same number. It was caught because lane
A ran a grep afterwards, on a hunch. The nine duplicates at 268–276 are what
happens when nobody has the hunch: they merged cleanly, because they were
written far enough apart in the file that git had nothing to complain about at
all. Three of them are cited from source comments, and those citations are now
genuinely ambiguous — `known-issues.md` →
`A-DESIGN-DECISIONS-NINE-DUPLICATE-SECTION-NUMBERS` has the list.

**Four decisions inside it, each with a real alternative.**

*1. Grandfather, don't renumber.* The alternative — fix the nine duplicates and
the numbering becomes clean — was rejected on the precedent already set twice
in this file's header (§217–§220, and §626). The numbers are cited from source
comments, from `known-issues.md`, and from each other; renumbering trades a
cosmetic inconsistency for dangling citations, and the citations are the only
reason the numbers exist. Two of the nine are cited from `kernel/src/drm/`,
which lane C cannot edit, so a lane-C renumber could not even be completed. The
baseline (`scripts/design-decisions-baseline.json`) records each number and how
many headings legitimately bear it, so the nine pass and a tenth does not.
Cost: the file permanently contains nine ambiguous numbers, and a reader who
follows one of those citations may land on the wrong entry. That cost is real
and is why the duplicates are logged rather than merely tolerated.

*2. Parse the band table out of this document rather than hardcoding it.* The
bands have already moved three times (400s → 500s → 600s → 700s), each time
because a lane ran out. A gate whose idea of the bands is a constant in a
script would go quietly wrong the fourth time — and "quietly wrong gate" is the
exact failure being fixed. So the table above is the source of truth and the
gate reads it, which costs a parser and a format the table must keep. It fails
loudly if the table stops matching, rather than concluding there are no bands.

*3. The order rule is "insert after your band's last entry", not "before the
next band's first".* The header said the latter, and it was already false when
written: the §500s and §600s are thoroughly interleaved by four months of
merges, so lane C following it literally would have inserted ~2 900 lines above
its own neighbours, in the middle of lane A's run — at the one offset the bands
exist to keep it away from. "After my own band's tail" is a statement about a
lane's own entries and stays true under any amount of interleaving. The gate
prints the line for each lane, so nobody has to work it out.

*4. Both heading styles are matched, and a heading matching neither is an
error.* This file uses `## §268 — title` and `## 268. title` interchangeably;
the header calls the drift "not meaning". Lane A's own published plan for this
gate specified `^## (\d+)\.`, which sees 201 of 527 headings — and that
blindness, shared by every hand-check ever run against this file including the
grep that caught 626, is the most plausible mechanism by which nine duplicates
survived. Matching one style would have shipped a gate with the defect it was
written to remove. A heading that starts with a number but parses as neither
style is rejected rather than skipped, for the same reason: a heading the gate
cannot number is invisible to every check in it.

**Amended 2026-08-29, the day after: the high-water mark is taken over what was
already *established*, not over every heading in the band.** As first written,
the no-backfill rule compared a new section against every other section in its
band — including other sections new in the same change. That made a change that
adds *two* sections to one band impossible to write correctly: the earlier and
lower of the pair is always "below" the later and higher one, so it was reported
as backfilling. This was not a hypothetical shape and it fired the first time it
arose, on §631 and §632 (the pair being: this entry, and the one for a
`printf` batch that settled a second question the same afternoon). One batch of
work can settle two questions. The comparison is now against the baseline plus
any new heading standing *above* this one in the file, which loses nothing —
numbers still only ever go up, both against history and down the page, so a run
of new sections must still be written lowest-first, and a new number below a
*spent* one is still rejected wherever in the file that spent one sits. Three
tests pin the three cases.

**What it does not do.** It does not check that a section's *content* belongs
to the lane that claims it, and it cannot: `**Lane:**` is self-declared. Its
value is that a lane writing into another's band now produces a one-line
contradiction in the diff instead of a silent collision six weeks later.

**Where it lives:** `scripts/check-design-decisions-bands.py`,
`scripts/design-decisions-baseline.json`,
`scripts/test-check-design-decisions-bands.py` (38 assertions), wired as
`check_design_decisions_bands` in `scripts/boot-test.sh` immediately after
`check_python_suites`. Fulfils
`requests/a-bc-design-decisions-numbering-c-is-right-b-is-withdrawn-and-i-will-gate-the-bands.md`.

**How to reverse:** delete the `check_design_decisions_bands` call. The
baseline and the checker are inert without it. To re-baseline after a
deliberate renumber: `python scripts/check-design-decisions-bands.py
--update-baseline`, and say so in the commit message — a dropped count means a
section that something may still cite has stopped existing.
