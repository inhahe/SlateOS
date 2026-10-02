## §447 — One shared `randrange` crate, with the generator's output permuted as well as its reduction fixed

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Twenty-seven of this tree's applications each contained a
copy-pasted eight-line random number generator, and all twenty-seven copies had
the same bug: asking one for "a number under 4" returned a repeating pattern
of four numbers, for ever, in every game. In `apps/simon` that pattern *was*
the game — the colour sequence a player was asked to memorise was the same four
colours in the same order every time. The decision is to replace all twenty-
seven copies with one shared crate, and — separately — to change the generator
itself rather than only the "pick a number under N" step that was visibly
broken.

### The bug, stated once

The copied generator is a linear congruential generator with modulus
2<sup>64</sup> (`state = state * A + C` on a `u64`, wrapping). In *any* such
generator, bit *k* of the state has period 2<sup>*k*+1</sup>: the bottom bit
alternates, the bottom two bits count through four values, and so on. That is a
property of the construction, not of the constants.

The copied reduction was `state % bound`, which for a power-of-two `bound`
keeps exactly those bottom bits. So every draw from a power-of-two-sized set
was a fixed cycle of that length. An odd bound escapes, because the remainder
then depends on the whole word — which is why the defect survived: `% 7` for
tetrominoes is fine and `% 4` for four colours is not, and in `apps/simon` the
two ideas lived three lines apart.

`apps/simon` had 106 passing tests. None caught it, because the broken draw
uses all four colours **exactly equally** — only the order is degenerate.
*A distribution check cannot see this bug.*

### Decision 1 — one crate, not twenty-seven fixes

| | Fix each copy in place | One shared crate *(chosen)* |
|---|---|---|
| Work now | 27 small edits | 1 crate + 27 one-line dependency changes |
| Next app that needs randomness | copies whichever neighbour it finds | `use randrange::Rng` |
| A future improvement (better generator, unbiased range) | 27 places | 1 |
| Risk | a copy gets missed and stays broken | a bug is shared by everything at once |

Chosen because the evidence for the shared crate is *already in the tree*: the
27 copies are not similar, they are identical down to both magic constants, so
they were never independent implementations that happened to agree. They were
one implementation stored 27 times, and storing it once is the honest
representation. The tree already has four precedents for exactly this move —
`yamldoc`, `textfmt`, `textfind`, `byteread` — each created after the same
discovery, and `textfind`'s note records the same finding in stronger terms:
eight crates had written the same search and made *the same three mistakes*.

The "shared bug" risk is real and is the reason the crate carries 15 tests for
120 lines of code, including one that rebuilds the historical defect inside the
test file and asserts it still reproduces — so the module documentation's claim
about *why* the crate exists is itself checked, rather than being a story in a
comment.

### Decision 2 — permute the output, don't only fix the reduction

Fixing `below` to take the high bits (multiply by the bound, keep the top half
of the 128-bit product) is sufficient for the reductions this crate provides.
The question was whether to stop there.

| | Fix the reduction only | Also permute the output *(chosen)* |
|---|---|---|
| `rng.below(4)` | correct | correct |
| A caller writing `rng.next_u64() % 2` | **alternating counter** | correct |
| Cost | none | three instructions per draw |
| Honesty of the interface | `next_u64` returns a word whose bits are not equal | every bit is equally good |

Chosen because the first column leaves a loaded gun. A `next_u64` whose low
bits are a counter is a correct-but-treacherous interface: it hands out a value
that *looks* like 64 random bits and is not, and the next person to reduce it
themselves gets the bug back — which is precisely how 27 crates acquired it in
the first place. The state still advances by the same LCG, with the same
recognisable constants, and the output is then run through SplitMix64's
xor-shift/multiply finaliser, which mixes the high bits down. This is the shape
of the PCG family: a cheap LCG for the state, a permutation for the bit quality
the LCG cannot provide.

A consequence worth stating: the two defences are independent, and either alone
makes the "no cycle" test pass. That was verified by reverting each separately,
and the test's own doc comment now says so, so a future reader does not mistake
it for pinning the reduction.

### What was deliberately not done

- **Not a cryptographic generator.** The state is 64 bits and recoverable from
  a couple of outputs. The crate says so in its first screen. Password
  generation and key material must not come from here; `apps/passwordgen` is
  therefore *not* a migration target, and its own generator is a separate
  question.
- **Not stream-compatible with the copies it replaces.** Preserving the old
  stream was never on the table: the old stream is the bug. Tests in the
  migrating crates that pinned a specific board layout will need new
  expectations, and each of those is a chance to check whether the test was
  asserting a property or merely a photograph.
- **Not `rand`.** It is a large dependency tree for a machine that has no
  package manager yet, and the parts of it that matter here are 120 lines.

### Where it lives

`randrange/src/lib.rs`, a workspace member (not a default member — same
rationale as `yamldoc`), `no_std` with no `alloc`.
