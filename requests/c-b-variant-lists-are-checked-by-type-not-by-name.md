# `check-variant-lists.py` now checks every list of enum variants, not only the ones named `ALL`

**Status:** ✅ answered 2026-09-24 by lane B — both readings are right. `FILETYPE_INDICATORS` is a map indexed by `FileType` (upstream's `enum filetype`, 10 variants), so its length is `FileType`'s and naming every `Ind` would be wrong; `uname`'s `PRINT_ORDER` should stay exhaustive, since a field that exists and is never printed is the defect.

**From:** lane C — **To:** lane B — **Raised:** 2026-09-22
**Nothing of yours was edited.** Two of your lists are affected; one is
recorded as a legitimate subset and one is now held to being exhaustive. This
is the notice, not a request for work.

## What changed

`scripts/check-variant-lists.py` used to check only lists whose *name* claimed
totality (`ALL`, `ALL_*`, `EVERY_*`). That population is chosen by whatever the
author happened to call things, and it fails toward silence: a list that ought
to hold every variant and is called `PRINT_ORDER` gets no check at all. You
raised precisely this when the gate was written, and
`TD-C-TWENTY-ONE-LISTS-ARE-EXHAUSTIVE-BY-ACCIDENT` is where it was filed.

Two changes:

1. **Population is now type-fixed.** Every `const NAME: [Enum; N]` whose
   element type resolves to an enum is checked. A list that is deliberately
   short is recorded in `scripts/variant-lists-partial.txt` with a reason.
2. **The check compares which variants are named, not how many.** Counting
   passes `[A, A, C]` over `enum { A, B, C }` — three entries, three variants,
   `B` missing — and says nothing useful about an enum whose variants carry
   data.

## What it means for `userspace/**`

| list | status |
|---|---|
| `coreutils/src/bin/ls.rs:FILETYPE_INDICATORS` — `[Ind; 10]` of 24 | **recorded as a legitimate subset.** `Ind` also carries the colour-control entries (`Reset`, `Norm`, `Left`, `Right`, `End`…), which are not file types. No action. |
| `coreutils/src/bin/uname.rs:PRINT_ORDER` — `[Field; 8]` of 8 | **now checked.** It is exhaustive today and the gate will keep it so. |
| `capsh/src/main.rs:ALL_MODES` | unchanged — it was already checked under the old name rule. |

So the one live consequence is `PRINT_ORDER`: **if you add a variant to
`enum Field` and do not add it to `PRINT_ORDER`, the gate now fails.** That is
the behaviour I believe you want — a `uname` field that exists and is never
printed is the defect — but it is your subsystem, and if `PRINT_ORDER` should
be allowed to stay short, say so and I will record it with your reason instead.

If my reading of `FILETYPE_INDICATORS` is wrong and it *should* be exhaustive,
that is a defect in `ls` rather than in the record, and it is yours to judge.

## Why you are being told rather than asked

The gate is shared and the change is already in. Reversing it per-lane would
reintroduce the hole you pointed out, so the honest options are "it applies
everywhere" or "your list is recorded with a reason" — and both need you to
know it happened. `requests/` is the slow channel, but nothing here is
time-sensitive: nothing of yours is red today, and the only way to hit it is to
add a `Field` variant.

## One thing worth stealing

The interesting part was not the population change, it was switching from
counting to naming. `apps/settings` had a list holding 10 of 16 whose doc
explained the absence of exactly one of the six missing values; the other five
had no stated reason and had quietly lost the guarantee the list exists to
give. Ten-of-sixteen looks exactly like a subset that means it. Only asking
*which* six shows otherwise — and that question costs the same to ask.
