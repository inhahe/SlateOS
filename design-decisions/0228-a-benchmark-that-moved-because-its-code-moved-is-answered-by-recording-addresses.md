## §228 — A benchmark that moved because its code moved is answered by recording addresses, not by pinning layout

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** One of our benchmarks suddenly reported that SHA-256 had got four
times slower. It had not: the SHA-256 machine code was byte-for-byte the same.
The function had merely ended up at a different *address* in the kernel image,
and the emulator we measure under (QEMU) happens to run code at that particular
address several times slower. Any other address was fine. The question was what
the project should do about a benchmark that can swing 4x for a reason that has
nothing to do with the code being measured. The answer taken: make the swing
*recognisable* by writing down where the hot functions live in each run, and
stop trying to make it not happen.

### The problem

QEMU's TCG (its just-in-time translator — it turns guest x86 into host code and
caches the result) indexes some of its internal caches by guest address. A
function whose address collides badly in one of those structures is emulated
several-fold slower with identical machine code. Which structure was not pinned
down; it needs QEMU source we do not have checked out, and knowing would not
change anything below.

This is not hypothetical or marginal. Measured on this tree: `crypto::compress`
at `…80afce00` gave 30048 cycles for `crypto_sha256_64B`; the same source built
so the function landed at `…80364980` gave 7188. `crypto_sha512_64B` —
near-identical code, same file, different function — was *faster* across the
same boundary. `net_tcp_checksum_v6_1460b` hit 12.5x at one commit and healed by
itself two commits later as later edits shifted `.text` again.

It bites the project's own rule. CLAUDE.md says a benchmark that regresses more
than 10% must be investigated before merging. That rule is right, and this is a
class of trigger for it where the correct outcome of the investigation is "no
change was made to this code". The first occurrence cost a multi-hour bisect
that terminated on a commit editing only `audio_mixer.rs`.

### Options

**A. Record the addresses of known placement-sensitive functions in every
benchmark record.** *What changes:* each run in `bench/history.jsonl` gains a
`hot_symbols` map; a future 4x swing is diagnosed by reading one line instead of
running a bisect.

**B. Normalise each benchmark against an in-run reference.** *What changes:* the
reported number for a benchmark becomes a ratio to some reference benchmark
measured in the same boot, so a whole-suite emulator effect divides out.

**C. Mark the affected benchmarks comparable only within a single build.**
*What changes:* `crypto_sha256_64B` and its siblings stop being tracked across
commits at all, and only ever compare against themselves in the same binary.

**D. Pin the layout so the address cannot move** — align `compress`, pad
`.text`, or fix the link order.

### Why A

**B fails on the shape of the effect.** The suite already removes whole-suite
drift by dividing by the median ratio, and it did not help here: the median
ratio across 98 benchmarks was 0.995. This is not a uniform slowdown to divide
out — it is one function moving 4x while its neighbours do not move at all.
Normalising against a reference that was unaffected leaves the 4x exactly where
it was; normalising against one that *was* affected would hide a real
regression in the same code.

**C throws away the measurement to avoid the noise.** These are the benchmarks
for the kernel's hash primitives; cross-commit tracking is the entire reason
they exist. Silencing the four benchmarks that route through `compress` would
also silence a genuine 4x regression in SHA-256, which is a far more expensive
error than a false positive that costs one lookup.

**D is luck dressed as engineering.** It was tried: `#[rustc_align(4096)]` on
`compress` restored every affected benchmark to baseline and looked like a
clean causal fix. A control — pristine source, built the same way — matched it
to within 0.1%, proving the alignment attribute did nothing and the *build
flag* had reshuffled the layout. Any layout pin holds only until the next
commit shifts `.text`, moves the problem to whichever hot function lands on the
bad address next, and is meaningless on real hardware where the effect almost
certainly does not exist. Shipping it would have been shipping a remedy for
something alignment has no part in.

**A is the only option that adds information rather than removing it.** It
changes no kernel code, hides no regression, and converts a multi-hour bisect
into a one-line correlation: the crypto benchmarks jumped *and* `compress`
moved, in the same record.

A second, smaller decision falls out of the same investigation and is recorded
here rather than separately: **runs of deliberately-modified binaries are kept
but excluded from baselines** (`--experiment` / `BENCH_EXPERIMENT`). Deleting
them was rejected for the same reason C was — they are the evidence for the
finding — but five probe runs spanning 4x in one 8-run window would have widened
that benchmark's outlier fence past the effect it is meant to catch.

### Cost

`hot_symbols` is three addresses per record and a hand-rolled ELF symbol-table
reader (~50 lines) that must degrade quietly, since it is bookkeeping attached
to a measurement that took nine minutes to produce. The list of tracked symbols
is manual and will go stale: it names the functions we have *seen* be
placement-sensitive, not the ones that are. That is accepted — a stale entry
costs a useless address in a record, while the alternative (dump every symbol)
would put a megabyte of addresses in the history per run.

### How to reverse

Drop `--kernel-elf` from `scripts/boot-test.sh` and the `hot_symbols` block from
`scripts/bench-history.py`. Existing records keep the field; nothing reads it
programmatically, so nothing breaks. The `--experiment` exclusion is separable
and lives in `comparable_records`.

**Reference:** `known-issues.md` →
`A-A-4x-CRYPTO-"REGRESSION"-BISECTS-TO-A-COMMIT-THAT-ONLY-EDITS-audio_mixer.rs`,
which carries the bisect log, the three-build table and the near-miss.
