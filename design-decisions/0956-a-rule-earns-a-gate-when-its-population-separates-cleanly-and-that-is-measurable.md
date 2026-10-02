## 956. A rule earns a gate when its population separates cleanly, and that is measurable before writing it

**Date:** 2026-09-21 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** four times in one day I found a defect and asked whether the
rule behind it should become an automatic check. Once the answer was yes and
twice it was no, and in every case the deciding evidence was a count taken
before any code was written. The rule that came out of it: measure the
population the gate would judge, and build the gate only if the good and bad
cases fall into two piles with nothing in the middle.

| candidate rule | population | verdict |
|---|---|---|
| every ring-3 entry must define the six syscall-argument registers | six correct sites defined **6 of 6**; the defect defined **0 of 6**; nothing in between | **built** — `scripts/check-ring3-entry-regs.py`, and it found the bug it was written for and nothing else |
| a table answering "may this be modified?" must key on the file, not the name | **80** path-comparison sites across **25** files, and most are correct: `devfs`, `cgroupfs`, `index`, `fontmgr` are namespaces where the name *is* the thing | **not built** — a gate here is ~70 false positives, and the distinguishing feature is *what question the table answers*, which no regex sees |
| a self-test must be able to fail | **823** self-tests; a no-`Err` scan flags 53, tightened to 31 — and the survivors are `assert!`-based (panic is a failure path) or thin wrappers delegating to a shared runner (`run_hosted_cc_case`) | **not built** — ~90% false positives, and separating them means following calls, not matching text |
| a `/proc` generator must serve the rows its module holds | **126** generators call `stats()` while their module also offers a `list_*()` they never call | **not built**, and not even swept: `gen_locale`'s module offers `list_timezones`, and publishing every timezone into `/proc` would be absurd. Whether rows belong is a judgement about what a reader needs |

**Why this is worth a number rather than three notes.** The tempting move
in rows two and three is to build the gate anyway and add a baseline for the
70 exceptions. That produces a check whose output is mostly noise, which
trains every reader to skim it — the exact failure lane C described about
stale notices, and the reason `check-gates-can-refuse` exists at all. A gate
with a large baseline is not a strict rule with exceptions; it is a list of
known cases wearing a rule's clothes.

**The operational form:** before writing a checker, count the population it
would judge and look at the distribution. Two piles with a gap means the
rule is real and the threshold is *discovered*. One smear means the rule is
a judgement, and judgements belong in review and documentation, not in a
gate. A threshold chosen after seeing a smear is fitted to the data and
means nothing — lane C put that better than I did, and it is their sentence.

**The fourth row is the most instructive, because the class is REAL.**
Lane C filed two instances (`/proc/memlayout`, `/proc/servicemgr`); both
were genuine and are fixed, and I found a third in `/proc/secureboot`,
where the enrolled keys are exactly what a reader wants and only
`key_count` was published. So the rule catches real defects and still
must not be mechanised: 126 candidates with no gap means each one needs a
human to ask *would anybody want this list?*. A gate here would carry a
123-entry baseline, and a 123-entry baseline is a list of known cases
wearing a rule's clothes.

**The cheapest version of this discipline** is that all three measurements
above took under a minute each, and two of them saved a day of building
something that would then have had to be maintained or deleted.
