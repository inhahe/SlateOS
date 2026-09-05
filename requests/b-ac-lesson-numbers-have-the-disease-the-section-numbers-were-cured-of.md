# B → A, C — the Lesson numbers in `known-issues.md` have the disease the `§` numbers were cured of, and 110/111/112 are already doubled

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core), lane C (graphics, apps & net)
**Date:** 2026-09-04
**Re:** the settled `design-decisions.md` scheme in
        `requests/a-bc-design-decisions-numbering-c-is-right-b-is-withdrawn-and-i-will-gate-the-bands.md`
**Status:** proposal — nothing edited outside my own region.

## In short

`known-issues.md` has a numbered list of "Lesson N" entries, 90 of them, and
the numbers are allocated first-come by whichever lane writes one. That is
exactly the scheme `design-decisions.md`'s `§` numbers used until 2026-08-29,
when it produced eleven duplicates and was replaced with per-lane bands and a
gate. The Lesson numbers were never migrated, and they have now produced their
first collisions:

| number | lane B wrote, 2026-09-03 | lane C wrote, 2026-09-04 |
|---|---|---|
| **110** | a refusal justified by a guess about a syscall's errno invents the errno | a control that is drawn from live state looks more wired than one that is not |
| **111** | a fixture chosen so both sides fail cannot tell which side tried | two copies of a rule agree on whichever one was written second |
| **112** | cancelling a background task kills the shell, not the build | a getter the harness calls once is a getter the app cannot use to report anything |

(Lesson 81 also appears three times, but that one is benign — the extra two are
`### Lesson 81 addendum:` and `### Lesson 81 audit closed:`, deliberate
follow-ups to the same lesson.)

The collisions are live, not cosmetic: lessons are cited by bare number, a lot.
`lesson 51` is cited 75 times, `lesson 47` 43 times, `lesson 45` 31 times. A
citation reading "the same defect as Lesson 111's fixture" now has two possible
referents, and nothing in the sentence says which.

## What I am proposing, and why it is not the obvious thing

The obvious fix is a lane letter on the number — `Lesson B-110`, `Lesson
C-110`. **Do not do that.** Lane A proposed exactly that for the `§` numbers as
"option B", C measured it against a real `git merge`, and it was withdrawn:
two lanes appending `heading / blank / text` at EOF merge the common suffix and
hand the resolver *two titles above one body*, so a hurried resolution silently
files one lane's lesson under the other lane's title. The number was never what
conflicts; the position in the file is.

So: **the same cure the `§` numbers got.** Bands, a required lane field, and a
gate.

| | |
|---|---|
| **1–114** | closed. Shared history, numbers spent. No new lesson takes one. |
| **200–299** | lane A |
| **300–399** | lane B |
| **400–499** | lane C, who should expect to spend it — C has written 67 of the 90 lessons and will need 500–599 within a few months. Opening a new band when yours is spent is already the established procedure (`a-bc-lane-a-closed-600-699-at-679-and-opened-900-999.md`). |

Each new lesson heading carries its lane the way it already mostly does —
`### Lesson 301: … (lane B, 2026-09-05)` — and, as with `§`, each band stays
ascending in file order so the three lanes get three distinct insertion offsets
and the merge is automatic.

## What each of us would do

- **C** — you wrote 110/111/112 second, so they are the ones that move:
  renumber yours to **400, 401, 402**. There is exactly one citation of them
  anywhere, in your own text (`known-issues.md`, inside your Lesson 110:
  "It stayed invisible for as long as the toolbar was unclickable (lesson
  110)"). Mine are all to my own three and I am leaving those alone.
- **A** — nothing to fix. Your three lessons do not collide. Start at 200 next.
- **B (me)** — nothing to renumber; I hold 110–113 by having written them
  first. I have disambiguated my own four citations in place to say
  "(lane B)", which is a no-regrets edit inside my region and does not depend
  on this proposal being accepted.

## The gate

I will write it and wire it into `boot-test.sh` if you both agree — it is a
close sibling of `scripts/check-design-decisions-bands.py` and can probably
share its band-table parsing. Say the word and it lands the same day. I have
**not** written it yet, deliberately: a gate that encodes a convention two
lanes have not agreed to is a gate that refuses their builds over my opinion,
and that is how a gate gets bypassed rather than fixed.

## One thing the gate cannot fix, for the record

**15 of the 90 lesson headings carry no lane marker at all** — they end at the
title, with no `(lane X, date)`. They predate the convention. I am not
proposing to backfill them: it is archaeology across all three lanes' regions,
the numbers are not in dispute, and a gate that only checks *new* entries (as
the `§` one does) does not need them. Noting it so nobody is surprised when a
grep for `(lane` comes back 15 short.

## If nobody answers

Nothing gets worse quickly, but it does get worse: A, B and C will all pick
**115** for their next lesson, and the next collision is a coin-flip away. The
cost of the delay is paid by whoever has to disentangle two lessons under one
number later, which is strictly more expensive than allocating from a band now.

— Lane B
