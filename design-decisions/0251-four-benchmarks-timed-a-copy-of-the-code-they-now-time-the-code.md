## §251 — Four benchmarks timed a copy of the code; they now time the code

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** Four of the kernel's 86 benchmarks were not measuring the kernel.
Each one had a private copy of the function it claimed to time, written out by
hand inside `bench.rs`, and it timed the copy. So the two TCP-checksum numbers,
the IP-checksum number and the DNS-query number described code that never runs
on a real packet — you could have made the shipping versions ten times slower
and all four would still have reported the same figure and passed. The fix is
to delete the four copies and let the benchmarks call the real functions, which
costs widening three functions from private to crate-visible. The consequence
to expect: three of the four numbers will move when next recorded, because the
real code does more work than the copies did. That movement is not a
regression — it is cost that was always being paid and never being measured.

**What was there.** Found while writing §250's coverage map, which is what
forced the question "which kernel file does this benchmark actually run?" for
every benchmark in turn.

| Benchmark | Timed | Faithful to the real function? |
|---|---|---|
| `net_checksum` | `bench.rs::internet_checksum` | Yes — character-equivalent to `ipv4::ip_checksum` |
| `tcp_checksum_v4` | `bench.rs::tcp_checksum_bench` | **No** |
| `tcp_checksum_v6` | `bench.rs::tcp_checksum_v6_bench` | No |
| `dns_build_query` | `bench.rs::build_dns_query_bench` | **No — differs in behaviour** |

The stated justification, in `tcp_checksum_bench`'s own doc comment, was
"duplicated to avoid depending on tcp module internals"; `internet_checksum`'s
said it measured "pure computation, not module call overhead". Both are
arguments for a benchmark that avoids depending on the code under test, which
is a description of a benchmark that does not test it.

**Why "not faithful" is the load-bearing part.** A faithful copy is merely
redundant — it drifts eventually, but today's number is today's truth. These
were not faithful, and the divergences fell exactly on the thing being measured:

- `tcp_checksum` builds a 12-byte `pseudo` array and walks it with
  `chunks(2)` and `.get(1).copied().unwrap_or(0)`. The copy hand-unrolled the
  pseudo-header into six `wrapping_add`s and never built the array. The
  pseudo-header is the *entire subject* of comparing v4 against v6 — the v6
  benchmark's doc comment says it exists "to show the overhead of the larger
  pseudo-header" — and it was the one part not measured. Both sides of that
  comparison were hand-optimised copies, so the overhead it reported was the
  difference between two functions that do not exist.
- `tcp_checksum_v6` sums source and destination in one interleaved 8-iteration
  loop; the real one walks each address in its own `for i in 0..8`.
- `build_dns_query_bench` lacked `encode_name`'s `.filter(|l| !l.is_empty())`.
  That is not a cost difference but a **behavioural** one: the filter is what
  makes a trailing-dot FQDN (`example.com.`) encode legally instead of emitting
  a zero-length label before the root terminator. The benchmark reported the
  speed of a builder that would produce an invalid packet.

**What changed.** `net::ipv4::ip_checksum` was already `pub`. Three functions
were widened to `pub(crate)`, each with a doc comment saying that `crate::bench`
is the only reason: `net::tcp::tcp_checksum`, `net::tcp::tcp_checksum_v6`,
`net::dns::build_query`. The four copies are deleted.

`net::dns::build_query` rather than `build_query_typed`: the benchmark wants the
A-record path, `build_query` *is* the A-record path, and exposing it keeps
`TYPE_A` and the other qtype constants private. Widening the narrower, more
specific function costs one call site and leaks less.

For the same reason the IPv6 address newtype is wrapped *before* the timed
closure opens, not inside it — putting `Ipv6Addr(src)` in the window would
re-introduce, in the act of fixing this, the exact error §250's map calls out
for `net/interface.rs`.

**On keeping the benchmark names.** The three unfaithful ones will step to a new
level. The alternative was to rename them so the old series ends cleanly. Kept
the names, because a name here denotes the *quantity of interest* ("TCP checksum
over a 1460-byte segment with an IPv4 pseudo-header"), which has not changed —
only the fidelity of the instrument has. Renaming would orphan ~30 runs of
history for a measurement that is conceptually the same one, finally taken
correctly.

The risk in that choice is a silently-reinterpreted series, which is precisely
§250's sin. It is answered by making the discontinuity loud rather than by
renaming: each benchmark's doc comment says which direction to expect and why,
`known-issues.md` records it, and the commit is the boundary. A step that trips
the regression detector on the next `--bench` run is the *correct* outcome and
should be annotated, not tuned away — the numbers genuinely got worse, because
the real code is slower than the copies were.

**Alternatives considered.**

- *Keep the copies and add a test asserting copy and original agree.* Rejected:
  it pins behaviour, not cost, so it would have caught the DNS divergence and
  none of the three timing ones — the copies would still have been the things
  measured.
- *Leave them and note it in the coverage map.* This is what §250 did as an
  interim, and it is honest, but it settles for a permanently blind spot in
  four of 86 benchmarks when the fix is three visibility keywords.
- *Move the real functions into a shared inner module both call.* More
  machinery than the problem needs; `pub(crate)` on the function that already
  exists is the smaller change and leaves the module boundary where the design
  put it.

**How to reverse.** Restore the four `fn *_bench` copies from this commit's
parent, point the four `run(...)` closures back at them, and narrow the three
functions to private. Nothing outside `bench.rs` depends on the wider
visibility.

**What would change this.** If `pub(crate)` on a hot function ever inhibited an
optimisation the private version got — cross-crate inlining is unaffected here,
but if it were measurable — the shared-inner-module alternative becomes the
right answer rather than merely a heavier one.

### Postscript: the fix opened a *new* way to measure nothing

The first run after the change (`fe9882a55`) reported
`tcp_checksum_v6` at **18 ns**, down 99% from ~1604 ns. That is not a
speedup; it is impossible. The segment is 1460 bytes, so the checksum loop
runs 730 iterations, and 18 ns is ~70 cycles — 0.1 cycles per iteration,
against a suite that runs at roughly **8 cycles per iteration** under QEMU's
TCG interpreter, on a host where `rdtsc_overhead` alone measures 138 cycles.
Nor can SIMD explain it: TCG *emulates* vector instructions, so
auto-vectorisation there is slower, not faster.

The call had been hoisted out of the timing loop. **And this change is what
made it hoistable.** The copies took their arguments from mutable locals
built inside `bench.rs`; re-pointing at the real functions meant passing a
loop-invariant immutable local to a pure function, which is exactly the shape
loop-invariant code motion looks for. `black_box` was already wrapped around
the *return value*, and that is the part worth writing down:

> `core::hint::black_box` on a result prevents **dead-code elimination**. It
> does not prevent **hoisting**. To stop LLVM computing something once and
> reusing it, the *input* must be opaque — `black_box(&segment[..])` — which
> denies the optimiser its proof that the pointed-to bytes are unchanged
> between iterations.

All three checksum benchmarks now blackbox their input buffer.
`net_checksum` was demonstrably *not* being hoisted (21 ns = 80 cycles over
10 iterations, right on the suite's 8-cycles/iteration line), and it was
guarded anyway: "not hoisted by this LLVM" is a property of a compiler
version, not of a benchmark, and a future one noticing would collapse the
series silently. The evidence that unguarded form was worth keeping for —
that §251's re-pointing did not by itself move the number — had already been
banked by `fe9882a55`, which ran re-pointed and unguarded. `dns_build_query`
needs no guard: it allocates, and the global allocator is an opaque call the
optimiser cannot move.

The v4 and v6 benchmarks had to be guarded *identically* rather than
individually judged. They exist to be subtracted from each other; if one were
hoisted and the other not, the "cost of the larger IPv6 pseudo-header" would
be the difference between a real call and no call at all — which is the same
species of falsehood §251 was written to remove, arrived at from the opposite
direction.

**The general lesson, which is why this is recorded rather than just fixed:**
a benchmark can fail to measure its subject in two ways, and they pull in
opposite directions. §251's original defect was measuring *something else*
(a copy) — findable only by reading the code. This one is measuring *nothing* —
and it falls out of arithmetic in seconds, because a number implying 0.1 cycles
per iteration cannot be true. **An implausible win is a bug report.**

It should be said plainly that the harness reported this properly:
`bench-history.py` printed `tcp_checksum_v6: 1604ns -> 18ns (-99% vs suite,
-99% raw); its own range is 1595-1610ns (median 1602ns over 8 runs)` under its
`IMPROVED` heading. Detection was never the problem. The problem is that a −99%
move is filed as good news and the run passes — and no amount of extra
statistics would change that, because the number is statistically flawless
(`split 1st=70 2nd=70 (0%)`, a perfectly replicating level shift). Only a
physical argument rejects it.

The cheapest such argument is already sitting in the suite: `self_test_nop`, the
empty-closure control, measured **72 cycles** on that same run, and the
checksum measured **70**. *Nothing real costs less than nothing.* That check is
exact rather than tuned, and is recorded as the follow-up in `known-issues.md`
— along with the version of it that does **not** work ("below `rdtsc_overhead`"
fires on eight legitimate benchmarks, because the harness amortises).

**And this is the third time in this one file.** `measure_access_at`'s doc
comment already records two: the optimiser removed the `write_volatile` stores
being measured (nop=400 vs store=244 — the *store* arm cheaper than the empty
one), and then it unrolled the constant-trip-count empty loop but not the store
loop, so the delta silently included ~11 cycles of scaffolding asymmetry that
moved 4× with `N`. That comment draws the moral itself — *"first the optimiser
removed the thing being measured, then it removed the thing being measured
against"* — and both fixes were local. A third instance in a different
benchmark says the moral is right and the response was too small: the property
"this window measures something" is worth checking mechanically, once, for
every window, rather than reasoned about per site by whoever writes the next
benchmark.
