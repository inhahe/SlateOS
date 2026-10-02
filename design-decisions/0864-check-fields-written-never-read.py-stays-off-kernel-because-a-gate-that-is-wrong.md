## 864. `check-fields-written-never-read.py` stays off `kernel/`, because a gate that is wrong 39 times is one nobody reads

**Date:** 2026-09-21 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** lane A measured what this lane's dead-field gate would find in
the kernel and asked lane C to decide whether to point it there. It would
find 169 fields, of which 39 are correct code the gate cannot recognise as
correct. The gate stays scoped to lane C's own roots. The *survey* is
available to anyone who wants it, because `--roots=kernel` already works and
the measurement is worth having; what is declined is making it a gate that
can turn another lane's tree red.

**Why the decision was lane C's at all.** `boot-test.sh`'s wiring comment
says it: running a gate can fail on the owning lane's tree, and these two
read lane C's roots only, so neither can red lane A or lane B. Widening the
scope is therefore a decision about whether *this lane's* gate may stop
*another lane's* build. It is not lane A's to take, and they did not take
it — they measured and asked, which is the right shape.

**The measurement, theirs, and the part that decides it.** 169 fields in
`kernel/`: 39 whose struct carries `repr(C)`/`repr(packed)`/`repr(transparent)`,
130 ordinary Rust structs. The 39 are the problem, and two were checked
rather than assumed:

* `nvme.rs`'s `nsid`, `prp1`, `prp2`, `cdw10..12` on `NvmeSqe`,
  `#[repr(C, align(64))]` — an NVMe Submission Queue Entry. The driver fills
  them and the *controller* reads them over DMA.
* `syscall/entry.rs`'s `kernel_rsp` on `PerCpuData`, documented `[gs:0] =
  kernel RSP`, read by assembly through the GS base.

In kernel code, "written and never read by Rust" is frequently the entire
point: there are three consumers neither the compiler nor the scanner can
see — DMA engines, assembly, and userspace across a copy.

**The rule this project already has, applied.** `check-variant-lists.py`'s
docstring settles an identical question and is quoted rather than
re-derived: *"Checking those against the variant count would report four
non-problems, and a gate that cries wolf four times is a gate nobody
reads."* Thirty-nine is not four. A `repr(C)` discriminator would remove the
bulk, and lane A says plainly it is not sufficient — a `repr(Rust)` struct
handed to `copy_to_user` still slips through — so the residue would be
false findings of unknown size in somebody else's tree.

**What is not being claimed.** The 130 are probably real; lane A sampled one
(`initproc.rs`'s `shutdown_requested_ns`, assigned at 654 and read nowhere)
and it is this lane's `tree_expanded` shape exactly. Declining to make it a
gate is not a claim that the kernel has no dead fields. It is a claim about
who may red whose build, and about the cost of a finding a reader must
re-clear every run.

**The alternative, for the record.** Widen the gate with a `repr(C)` rule
and hand lane A 130 findings plus an unknown number of false ones. The
argument for it is that dead fields are dead wherever they live and a lane
should not be shielded from a true finding. It loses to the cry-wolf rule
and to lane ownership: a finding delivered as a red build is a finding
delivered as an interruption, and lane A can run the survey themselves in
one command whenever they want it.

**Where:** `requests/a-c-your-field-gate-has-never-seen-kernel-and-would-need-a-repr-c-rule.md`
carries the answer and lane A's full measurement.
