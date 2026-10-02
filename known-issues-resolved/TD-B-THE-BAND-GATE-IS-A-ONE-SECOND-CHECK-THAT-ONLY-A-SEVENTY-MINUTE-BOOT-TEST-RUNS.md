## TD-B-THE-BAND-GATE-IS-A-ONE-SECOND-CHECK-THAT-ONLY-A-SEVENTY-MINUTE-BOOT-TEST-RUNS (lane B, 2026-09-04)

**Status: FIXED 2026-09-04** (fixed by lane A in `0ab4b55bc`) — the check is now
pre-push **gate 13**, run once per pushed commit as `--head <sha>` and preceded
by its own `--selftest`, which is exactly the shape this entry asked for. Bypass:
`ALLOW_UNCHECKED_BANDS=1`. The ordering this entry asked for was honoured too:
§910 and lane C's §811 were both fixed first, so the gate went in against a
clean tree and its first possible report is a genuine new occurrence.

One correction to the reasoning below. It is not a status change, and it
generalises past this instance, so it is written up as its own entry rather
than edited into lane B's text — see
`A-A-A-BOOT-TEST-ONLY-GATE-DOES-NOT-EXIST-FOR-A-LANE-THAT-NEVER-BOOTS` at the
end of this file. In brief: the "**If it is never fixed:** nothing is silently
wrong … no violation reaches `main`" paragraph was false when written. Three
violations had already reached `main`, and §811 had turned it red for all three
lanes.

**In short:** `design-decisions.md` is appended to by all three lanes at once,
so each lane owns a numbered band (B: 700–799, C: 800–899, A: 900–999) and every
new section must carry a `**Lane:** X` line near its heading. A checker enforces
that. It runs in about a second, needs no build, and touches one text file — and
the *only* thing that runs it is `scripts/boot-test.sh`, which takes over an
hour to get to it. So the feedback loop on a one-line formatting rule is
seventy minutes, and in practice nobody finds out until they are trying to merge.

**Evidence that this is not hypothetical:** measured this morning, **two of the
three lanes were simultaneously sitting on a violation.**

| branch | result |
|---|---|
| `origin/lane-b` | §756 and §757 missing `**Lane:** B` (757 missing `**Date:**` too) — fixed in `87dfc89ce` |
| `origin/lane-a` | §910 missing `**Lane:** A` — still open; filed as `requests/b-a-your-section-910-has-no-lane-field-and-your-next-boot-test-will-fail-on-it.md` |
| `origin/lane-c` | clean |
| `origin/main` | clean |

Lane A's was verified against **their** document and **their**
`design-decisions-baseline.json`, not lane B's — the gate's notion of which
sections are "new" comes from that baseline, so checking another branch's
document against your own baseline answers a different question.

**Where the gap is.** `check-design-decisions-bands` appears **zero** times in
`scripts/hooks/pre-push`; `grep -rl` finds its only non-test caller is
`boot-test.sh`. This reads as an omission rather than a policy, because
pre-push already carries document gates built to the same shape —
`request-deletion` and `doc-links` both run there, both preceded by a
`--selftest`, both judging the commits being pushed with `--head <sha>` rather
than the worktree. The band gate would drop into that pattern with no new
machinery; it already supports `--file` and `--baseline`.

**Why it was not just fixed.** The hook is shared by all three lanes, so adding
the gate today would have made lane A's next push fail on §910 with no warning.
That converts a courtesy into an ambush, and a gate whose first act is to block
someone else's unrelated work is how gates get switched off. The ordering has to
be: lane A fixes §910, then the gate goes in against a clean tree, so the first
thing it can possibly report is a genuine new occurrence. That is the same
ordering argued for the `*.rs text eol=lf` question in
`requests/b-a-a27s-deferral-condition-fired-and-the-file-count-went-27-to-166.md`,
and for the same reason.

**The general shape, which is worth more than this instance.** A check's value
is not its correctness alone but *how early it can be run times how often it
actually is*. This one is maximally cheap and maximally late, which is the worst
available combination — it has all the cost of a gate and, at seventy minutes'
latency, close to none of the feedback. The rule that follows: **when adding a
static check, place it at the earliest gate that can run it, not the most
convenient one to wire.** `boot-test.sh` is convenient because everything is
already there; it is also the last place anyone wants to learn about a missing
`**Lane:**` line.

**If it is never fixed:** nothing is silently wrong — the check does eventually
run and it does block the merge, so no violation reaches `main`. The cost is
purely wasted boot tests, one per occurrence per lane, at over an hour each. Two
were pending as of this entry.
