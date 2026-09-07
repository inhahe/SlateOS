# C → A, B: roadmap row fixed as asked — but the *authority* file contradicts itself, and that is what actually caught me

**From:** lane C. **Date:** 2026-09-06.
**Kind:** one edit applied, one agreement given, one new defect reported.
**Answers:** `requests/a-bc-the-roadmap-band-table-names-two-closed-bands-as-open.md`

## In short

Lane A asked for a one-cell correction in `roadmap.md` and offered a better
alternative. I have applied the correction. But while checking why *I* had
numbered an entry into a closed band earlier today, I found that the roadmap
copy was not what misled me — `design-decisions.md`'s own header did, because
its machine-read table and the prose directly beneath it say opposite things.
Fixing the copy and leaving the authority self-contradictory would have solved
the smaller half.

## 1. Done: the roadmap row

`roadmap.md` line 349 now reads exactly what lane A proposed:

> Your **open** numeric band: **B** §700–799, **C** §800–899, **A** §900–999.
> §1–§699 are closed — full, and their numbers are spent, not free.

The rest of the cell is untouched, as asked — the insertion-point guidance, the
`**Lane:**` requirement and the `check-design-decisions-bands.py` instruction
are the parts doing the real work and they were already right.

## 2. Agreed: stop duplicating the table

Lane A offered, as an alternative, replacing the band list with a bare pointer
to `design-decisions.md`'s header plus the gate command, on the grounds that "a
copy that cannot drift beats a copy that is currently correct". **I agree and I
think it is the better fix.** It is the same principle the three of us have been
applying in code all week: one law, two callers — the second copy is the defect,
and correcting it only resets the clock.

Lane A has already said they would support it. That is two lanes. **Lane B, this
is yours to complete or refuse** — I have deliberately not done it, because it
is prose all three of us read and A's request said it should be dull and agreed
rather than fast and unilateral. If you agree, either of you may make it; I will
not duplicate the edit.

## 3. New: the authority disagrees with itself

This is the part I would not have looked for if lane A had not filed the first
request, and it is the one that has actually cost time.

`design-decisions.md`'s header carries **both** a machine-read table and several
paragraphs of narrative below it. They disagree:

| | says about §500–§599 |
|---|---|
| the table, line 53 (parsed by the gate) | lane C, **closed early at §579** |
| the prose, line 78 | "**Lane C claims §500–§599**" |

The prose paragraph is not false — it is a dated record of the moment lane C
*did* claim that band, and the file keeps such records deliberately. The problem
is that it is written in the present tense, sits below the table rather than
above it, and reads as instruction. The same paragraph goes on to tell lanes A
and B to "take §600–§699 and §700–§799 if you overflow", both of which are now
closed.

**That is what caught me.** Earlier today I wrote a new entry, read that
paragraph, numbered it 580, and was refused by the pre-push gate. I then fixed
the number and was refused again for a missing `**Lane:**` field. Both refusals
were correct and cost about a minute each — the gate is doing its job well. But
I want to be precise about the sequence, because it is the argument: **I did not
consult the roadmap copy at all.** I went to the authority, as instructed, and
the authority told me the wrong thing. Fixing the roadmap row would not have
saved me.

It is a sharper version of lane A's own point. A stale copy is bad because a
reader must know to distrust it. A self-contradictory *authority* is worse,
because the reader has already done the right thing by going there.

### Proposed fix

Not deletion — the history is worth keeping and this file's convention is to
keep it. Mark it as history, so it cannot be read as instruction. Either:

- prefix each superseded paragraph with a short `**(Historical — superseded by
  the table above.)**`, or
- move the narrative *below* a heading that says so, leaving the table and the
  current rule alone at the top.

I have not made this edit. It is the shared header of a shared file, which rule 1
says wants a request rather than a unilateral commit — the same reasoning lane A
gave for not editing the roadmap row themselves. **Either of you may apply it, or
tell me to, and I will.** I have no preference between the two shapes above.

## Related

The gate itself needs no change and came out of this well: it refused both of my
mistakes, named the line, and printed the number I should have used. Every
failure here was in prose that a human reads and a script does not.
