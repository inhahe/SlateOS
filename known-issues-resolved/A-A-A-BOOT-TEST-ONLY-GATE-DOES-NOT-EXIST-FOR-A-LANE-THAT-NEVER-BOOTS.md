## A-A-A-BOOT-TEST-ONLY-GATE-DOES-NOT-EXIST-FOR-A-LANE-THAT-NEVER-BOOTS (lane A, 2026-09-04)

**Status: FIXED 2026-09-04** — the gate is now pre-push gate 13 (`0ab4b55bc`).
The measurement below is what stays useful; it applies to every future gate,
not to this one.

**In short:** a check that only runs inside `scripts/boot-test.sh` is not a
check the whole project has. It is a check the lanes that run boot tests have.
This project has three lanes, and the boot-test log says one of them has never
run one — so for that lane the check did not exist at all. That is not a
latency problem, which is how it was filed; it is a coverage hole, and it let a
violation onto `main`.

**The measurement.** `bench/boot-history.jsonl`, 648 records:

| `branch` | boot-test records |
|---|---|
| `lane-a` | 620 |
| `lane-b` | 28 |
| `lane-c` | **0** |
| `main` | **0** |

Lane C owns band 800–899 of `design-decisions.md` and appends to it as often as
anyone. It is also the lane that has never once run the only thing that
executed the band checker. So the gate's coverage was inverted relative to its
risk: heaviest on the lane least likely to trip it, absent on the lane whose
band was most active.

`main` recording zero is the same finding restated. Nothing runs a boot test
*as* `main`; `main` is only ever the result of a merge that someone else
tested. So "the boot test will catch it before it reaches `main`" is a claim
about the merger's habits, not about a gate.

**What it cost, concretely.** §811 (lane C) reached `origin/main` in the merge
`67768ee0` at 19:28 on 2026-09-04 without its `**Lane:**` field, and was fixed
in `d86ae72d7` at 20:53. For those ~85 minutes `origin/main` was red for **all
three lanes** — every lane that merged `main` down inherited a document that
failed the gate, on a defect none of them wrote. Two other lanes were carrying
the same defect on their own branches at the same time (§756/§757 on `lane-b`,
§910 on `lane-a`), found by lane B's audit rather than by any gate.

**The correction to `TD-B-THE-BAND-GATE-IS-A-ONE-SECOND-CHECK…`.** That entry
closes with "**If it is never fixed:** nothing is silently wrong — the check
does eventually run and it does block the merge, so no violation reaches
`main`." That was already untrue when it was written, for the reason above. The
mistake is worth more than the instance: the entry reasoned about the gate's
*wiring* (`boot-test.sh` runs it, `boot-test.sh` precedes a merge) and not
about its *execution record*, and the two disagreed by a whole lane. The wiring
was read correctly; it just does not answer the question.

**The general rule this yields.** *A gate's coverage is a measurement, not a
reading of where it is wired.* Before claiming a check protects the tree, count
how many times it has actually run, **per lane**, and treat a lane with zero
runs as a lane the check does not cover. For anything driven by `boot-test.sh`
the count is already in `bench/boot-history.jsonl` and takes one command; for
anything else, if you cannot produce the count, you cannot make the claim.

**Where to look next.** Every other check whose only caller is `boot-test.sh`
inherits this hole exactly. `grep -rl <checker> scripts/` finding only
`boot-test.sh` is the tell, and the fix is the shape gate 13 now has: a
`--head <sha>` mode so it can judge the pushed commit, a `--selftest` ahead of
it so a broken checker cannot pass silently, and a `touches` guard so it costs
nothing on pushes that cannot trip it.
