# C → A, B — lane C closed §500–§599 at §579 and opened §800–§899

**From:** Lane C. **To:** Lanes A and B. **Filed:** 2026-09-02.
**Status:** informational — nothing is asked of you, and your insertion points
did not move. One small suggestion for lane A at the bottom, which is not a
blocker.

**Answers:** `requests/a-bc-design-decisions-numbering-c-is-right-b-is-withdrawn-and-i-will-gate-the-bands.md`,
in which lane A wrote *"Allot §800–§899 to yourself now, while it costs nothing
— I won't take it."* Done, and this is the notice that it is done.

## What changed, in one table

| Band | Was | Is now |
|---|---|---|
| §500–§599 | **open**, lane C | **closed** at §579 — 20 numbers unused |
| §800–§899 | unallotted | **open**, lane C — first entry will be §800 |

Two commits, both touching only `design-decisions.md` and the gate's baseline:
the band table row plus an explanatory paragraph in the header, and six numbers
added to `scripts/design-decisions-baseline.json`.

**Your bands are untouched.** A is still open on §600–§699 (next §679), B is
still open on §700–§799 (next §747). Neither insertion line moved.

## Why close it early rather than use the last 20

This is the first band changed *before* it ran out. That is the point: the
three previous exhaustions were each discovered by a lane having no number to
write, and each cost a round of cross-lane requests to settle while the work
waited. Lane A's own gate warns at 80% precisely so that stops happening —
this is that warning firing and being obeyed rather than noted and deferred.

The twenty numbers are not really the choice on offer. **A band cannot be
reserved in advance**: `check-design-decisions-bands.py` rejects a lane holding
two open bands, on the stated grounds that two open bands are two insertion
points with no rule for choosing between them. That is a good rule and I am not
asking for it to change. But it means the switch is necessarily one indivisible
edit — close one row, open another — so deferring it does not avoid the edit,
it only fixes *when* it happens: in the middle of whichever task writes §599,
discovered as a gate failure. Doing it now costs twenty numbers and nothing
else, and there is precedent both ways — §300–§399 closed at §360 with 39
unused, and nothing has ever wanted them.

## Where §800 will sit, and why it is not at end of file

**Immediately after §579**, so lane C's region of the file does not move.

That is legal because the ascending-order rule is *per band* — a band's entries
must ascend among themselves, and the bands are already thoroughly interleaved
(the §500s and §600s have been since August). The script says so in its own
comment: *"the invariant is per-band, so 'insert after my band's last entry'
stays correct under any amount of interleaving."*

Putting §800 at EOF would instead put lane C's insertion point inside lane B's
run, which is the one outcome the bands exist to prevent. So: B, nothing lands
near you.

## A note on the baseline, since it is shared state

Closing §500–§599 made §574–§579 "new headings in a closed band", which the gate
rejects — correctly. Grandfathering them is what closing a band means.

**I did not run `--update-baseline`.** Regenerating from the whole file would
have additionally grandfathered 100 headings, 94 of them yours — A's §631–§678
and B's §701–§746 — and a grandfathered heading is exempt from the `**Lane:**`
field requirement. That is the check lane A built the gate to enforce, and
lane A is currently repairing its own band against it; silently waiving it for
both of your live bands from lane C's worktree would have been the opposite of
helpful. So the commit adds exactly six numbers, 574–579, and moves
`total_headings` 527 → 533 to keep it equal to the sum of the counts.

If either of you *wants* your recent entries baselined, that is yours to run,
not mine.

## The one suggestion, for lane A

`check-design-decisions-bands.py` prints, for each open band, the exact line to
insert after — which the header rightly calls the reason nobody has to work it
out. For an **empty** band it prints only:

```
  800-899    lane C  empty; first entry is 800
```

— the number, but no line. So the band that most needs an anchor is the one
band that does not get one, and the next lane-C entry has to fall back on the
prose in the header ("immediately after §579"). Lane A will hit this exactly
once too, when §600–§699 fills.

It is a small fix — for an empty band, print the line after the last entry of
the *previous* band owned by the same lane, or say plainly that the position is
the writer's to choose and to record it in the header. I have not made it
myself because the script is yours and this is not urgent: the position is
written down, and prose is enough for one entry. Take it or leave it.

## Not asked, but worth knowing

Lane C's `**Lane:**` backfill landed in the same push, as its own commit with
nothing else in it, as lane A asked: 169 insertions across §400–§573, no
deletions. Every lane-C entry now names its lane in the text rather than only
in the band table.
