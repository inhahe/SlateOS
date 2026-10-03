## §252 — The Internet checksum exists once, because seven identical copies were not identical code

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** Every network protocol needs the same little "add up all the
bytes" routine to detect corrupted packets. This code had written it out seven
separate times — same twelve lines, copied into TCP, UDP, ICMP and the IPv4
header check. The copies all behaved the same, so it looked like nothing worse
than untidiness. It was not: the compiler optimised some copies harder than
others, so two functions that were *supposed* to be a controlled comparison
differed by 34% for reasons that had nothing to do with what was being
compared — and that difference was read as a real finding and acted on. The
decision is to keep exactly one copy, in `kernel/src/net/checksum.rs`, and have
all seven callers use it.

**The concrete failure.** `net_tcp_checksum_v4_1460b` and
`net_tcp_checksum_v6_1460b` exist as a *pair*: they run the same 1460-byte
segment through the same loop, differing only in which pseudo-header (the small
block of address/length fields folded into the sum) precedes it. The pair's
entire purpose is to price one pseudo-header against the other.

On the `e1de4aaaa` kernel, v4 measured 7208 cycles and v6 measured 5364 — a
1844-cycle, 34% gap, in the direction that says IPv4's *smaller* (12-byte)
pseudo-header costs far more than IPv6's (40-byte) one. Disassembling that exact
binary showed why, and it was not the pseudo-header:

- `tcp_checksum_v6` survives as an out-of-line symbol and its segment loop is
  **unrolled 2×** — `addq $0x4, %rcx`, two 16-bit words per iteration.
- `tcp_checksum` has no symbol at all. It was inlined into its callers, and the
  copy that landed in the benchmark was **not** unrolled.

1460 bytes is 730 words: 365 iterations against 730. The benchmark pair was
reporting an inlining decision and calling it a protocol difference.

**Why duplication was the root cause and not merely an aggravating factor.**
Seven verbatim copies of the loop existed (`ipv4.rs` ×3, `ipv6.rs` ×2,
`tcp.rs` ×2), plus four copies of the IPv4 pseudo-header prologue and four of
the IPv6 one. To LLVM each copy is an independent function body, free to be
unrolled or not according to its own inlining context. Nothing anywhere
asserted they agreed, and nothing could have: they agreed on *value*, which is
all any test checks, while differing on *cost*, which is what the benchmark
measured.

The sharper point is about inference rather than compilers. **Duplicated code
that is supposed to be identical invites the reader to attribute any measured
difference to the one thing that visibly differs.** The two functions differed
visibly only in their pseudo-headers, so a 34% gap "obviously" meant the
pseudo-header cost 34%. That inference is valid; its premise — that the rest was
the same code — was false, and was false in a way that no amount of reading the
source could expose. Identical source is not identical code.

**Alternatives considered.**

| Option | Why not |
|---|---|
| Leave it; just annotate both functions `#[inline(never)]` | Fixes this pair and nothing else. The other five copies stay free to diverge, and the next person to compare two of them re-runs the same mistake. It treats the symptom in the two places we happened to look. |
| Leave it; add a benchmark that asserts the two loops emit the same code | Enormous machinery (disassembly parsing in the test suite) to defend a property that costs nothing to simply *have*. |
| Unify, but keep a thin per-protocol copy for "clarity" | This is what the seven copies already were. Each was individually clearer than a call to a shared helper; collectively they cost a wrong measurement and a wrong fix. |
| **Unify into one `net::checksum` module (chosen)** | One loop, compiled once. The TCP pair now differs by exactly its pseudo-headers, which is what it always claimed to measure. |

**The cost being accepted.** A shared `sum_bytes` is a call rather than
straight-line code, which matters most for the *shortest* inputs — the
20-byte IPv4 header check is 10 words, where call overhead is a real fraction.
It is not being forced either way with `#[inline(never)]`/`#[inline(always)]`
yet: LLVM may still inline it per-site and, in principle, re-diverge. That is
deliberate — pinning inlining is a decision that should be made from a
measurement, not in anticipation of one, and the next bench run supplies it.
What has definitely changed is that if divergence recurs it is now one
annotation on one function, not a reconciliation of seven bodies.

**RESOLVED, 2026-08-21, and the answer is "leave it unpinned."** The measurement
came from `llvm-objdump` rather than the bench, which is both faster and more
direct. LLVM does inline `sum_bytes` at every site — there is no `sum_bytes`
symbol in the kernel, and `tcp_checksum` has none either — so the literal worry
above came true. What did *not* come true is the divergence: all three checksum
benchmark sites now carry byte-identical loops, unrolled 4× with a 1×
remainder, matching the still-out-of-line `tcp_checksum_v6`. Before unification
the same three were 2×, not-at-all, and not-at-all. So `#[inline(never)]` would
buy nothing and would cost a call on every short checksum in the stack — the
exact cost this section was worried about — while removing the inlining that
makes the 20-byte case cheap.

The distinction worth keeping, since blurring it is what hid the original bug:
sharing a source function did not make the copies *share code*. It made them
present the optimiser with the same input, which is what made its decisions
agree. That is a weaker guarantee than one out-of-line body, so it is checked
rather than assumed — `llvm-objdump -d --disassemble-symbols=` is the check,
recorded in `known-issues.md` §251 Postscript 2, and `#[inline(never)]` remains
the one-line fix if a future toolchain diverges again.

**One further consequence, which is really a separate finding.** The new module
has no `#[cfg(test)] mod tests`, on purpose: `kernel/Cargo.toml` sets
`test = false` for the kernel binary (it supplies its own `panic_impl` and
other `no_std` lang items and cannot link against host `std`), so such a module
would never be compiled, never run, and quietly rot — while being counted as
coverage. **Tests that cannot run are worse than no tests.** The suite therefore
lives in `checksum::self_test()`, invoked at boot, and pins RFC 1071 §3's own
worked example (an external vector, so it cannot share a bug with a test derived
from this code), the high-side padding of an odd trailing byte, split-invariance
of the accumulator, the stamp-and-verify round trip, fold boundaries and
idempotence, and the 64 KiB worst case on which the module's no-overflow claim
rests. This is worth the target-side cost precisely *because* of the
unification: every checksum in the stack now funnels through one loop, so a
single wrong fold would corrupt TCP, UDP, ICMP, ICMPv6 and the IPv4 header check
at once.
