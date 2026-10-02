## TD-B-LESSON-NUMBERS-ARE-ALLOCATED-FIRST-COME-AND-HAVE-STARTED-COLLIDING (lane B, 2026-09-04)

**In short:** The numbered "Lesson N" entries in this file are allocated
first-come by whichever lane writes one, with no per-lane split. Lanes B and C
both took 110, 111 and 112 — on consecutive days, without either noticing. Since
lessons are cited by bare number (`lesson 51` appears 75 times, `lesson 47` 43
times), a citation like "the same defect as Lesson 111's fixture" now has two
possible referents.

**This is the same defect `design-decisions.md`'s `§` numbers had**, and it was
cured there on 2026-08-29 after eleven duplicates: per-lane bands, a required
`**Lane:**` field, and `scripts/check-design-decisions-bands.py` wired into
`boot-test.sh`. The Lesson numbers were never migrated. See
`requests/a-bc-design-decisions-numbering-c-is-right-b-is-withdrawn-and-i-will-gate-the-bands.md`
for why the tempting fix — a lane letter on the number, `Lesson B-110` — was
proposed, measured against a real `git merge`, and **withdrawn**: two lanes
appending `heading / blank / text` at EOF merge the common suffix, handing the
resolver two titles above one body, which files one lane's lesson under the
other's title. The number is not what conflicts; the position in the file is.

**Status: blocked on cross-lane agreement, proposal filed** at
`requests/b-ac-lesson-numbers-have-the-disease-the-section-numbers-were-cured-of.md`
— bands (A 200–299, B 300–399, C 400–499, 1–114 closed), C to move its
110/111/112 to 400/401/402 as the second writer, and lane B to write and wire
the gate once A and C agree. Not landed unilaterally on purpose: a gate
encoding a convention two lanes have not accepted refuses their builds over one
lane's opinion, which is how a gate gets bypassed instead of fixed.

**Done in the meantime, and independent of the outcome:** lane B's four
citations of its own 110/111/112 now read "Lesson 111 (lane B)". That removes
the ambiguity a reader actually hits today without presuming the scheme.

**Not proposed:** backfilling the **15 of 90** lesson headings that carry no
`(lane X, date)` marker at all. They predate the convention, their numbers are
not in dispute, and — as with the `§` gate — a checker that only judges *new*
entries does not need them.

**If it is never fixed:** all three lanes will pick 115 next, and the next
collision is a coin-flip away. Each one costs more to disentangle later than
allocating from a band costs now, and the cost lands on whoever is reading a
citation months from now rather than on whoever wrote it.
