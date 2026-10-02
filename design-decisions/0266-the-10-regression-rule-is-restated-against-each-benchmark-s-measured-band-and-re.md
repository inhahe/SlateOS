## §266 — The "10% regression" rule is restated against each benchmark's measured band, and real hardware is booked as the fix that makes it mean something again

**Date:** 2026-08-21
**Decided by:** Operator (Claude recommended this option; operator: "i'll go
with your recommendation")
**Lane:** A

**In short:** `CLAUDE.md` says to investigate any benchmark that gets more than
10% slower. We measured what a *no-op rebuild* does — same code, same machine,
just recompiled — and 71% of benchmarks move by more than 10% from nothing at
all. The median is 26%; the worst is 182%; and the hot paths the document
singles out as critical are among the *worst*. So the rule as written points at
phantoms, and — much worse — teaches a reader to shrug off a real 15% regression
as "probably layout". The decision is to restate the rule as "larger than that
benchmark's measured band, or 10% if no band has been measured", **and** to book
running the suite on real hardware as the change that actually restores the
guarantee.

### Why the restatement alone is not enough

Because it is an admission, not a solution. It concedes in writing that we
cannot detect a **100% regression in `pick_next`**, which is not an acceptable
end state for a scheduler hot path. `page_alloc_free` 92%, `page_fault` 85%,
`ipc_channel` 83%, `syscall_dispatch` 42% — these are tight hot loops, which is
precisely what the emulator's page-straddle penalty acts on. That is not bad
luck; it is the mechanism.

### Why not simply raise the flat number

A single higher threshold (say 30%, just above the median band) is wrong in both
directions at once: far too loose for the 25 quiet benchmarks, still far too
tight for the 26 that move ≥50%.

### What makes the hardware half reachable now

When this question was filed, "run the benchmarks on real hardware" was
infrastructure we did not have and had no plan for. **§263 changes that** —
bare-metal boot is now a decided, sequenced piece of work. Two caveats come with
it and are recorded so they are not discovered afterwards: every assumption that
breaks under a hardware accelerator breaks on bare metal too, and a bare-metal
run currently records its accelerator as *absent*
(`TD-A-A-BARE-METAL-RUN-RECORDS-ITS-ACCELERATOR-AS-ABSENT`).

Scope note: the sweep behind these numbers is **release-profile only** on one
host. The `debug` profile — which is what the majority of historical benchmark
records were taken under — has no band measured at all.
