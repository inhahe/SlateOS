## TD-B-BIGNAT-ADD-BACK-IS-UNREACHED-BY-ANY-TEST — 2026-09-15 — FIXED same day

**In short:** the big-number division in `coreutils` has a rare correction step
that fixes up an answer which came out one too big. No test in the suite ever
makes it run. Deleting the line that does the fixing leaves all 23 tests
passing, so if that step is wrong, nothing here would say so.

**Where.** `userspace/coreutils/src/bignat.rs`, `Nat::divmod` -- Knuth 4.2
Algorithm D, step D6, the `if t < 0` branch that does `q[j] -= 1` and adds the
divisor back into the running remainder.

**How it was found, and it was not by looking for it.** I was about to put a
narrow `#[allow(clippy::indexing_slicing)]` on `divmod`, justified by "the
algorithm's loop bounds keep the indices in range, and the tests check the
outcome". Before writing that I checked what the tests actually cover. Two
probes:

    delete `q[j] -= 1`            -> 23/23 tests still pass
    put `panic!()` in the branch  -> 23/23 tests still pass

The second is conclusive: the branch does not execute during the suite.

**It is not reachable by chance, either.** Knuth puts the probability of
needing D6 at about `2/b`, which for 32-bit limbs is one division in two
billion. A search of 20,000 divisions shaped to be hard -- divisor's top limb
in the upper half of its range, dividend one limb longer -- fired it zero
times, which is what that probability predicts.

**What the two new tests DO cover, measured rather than assumed.** Disabling
D3's estimate-correction loop fails
`long_division_corrects_an_estimate_of_a_whole_limb` and
`long_division_identity_holds_beyond_u128`, and no other test in the module.
So before those two, D3's correction was as untested as D6 is now. The
`u128`-based tests cannot reach either: they cap at four limbs because they
need a native reference to compare against.

**A correction worth recording.** Hacker's Delight gives `u = 2^95`,
`v = 2^63 + 1` as its `divmnu` ADD-BACK vector, and I added it under that
name. It is not an add-back case for this implementation: after D3 walks the
estimate back from `2^32` to `0xFFFF_FFFF`, the multiply-subtract stays
non-negative and D6 never runs. The test is renamed to what it demonstrably
exercises. Had the probe not been run, the suite would carry a test whose name
claims coverage it does not have -- which is worse than the gap, because it
stops anyone else looking.

**FIXED** by `long_division_exercises_the_add_back`, and the reason the first
attempt failed is the useful part.

**D6 IS UNREACHABLE FOR A TWO-LIMB DIVISOR, for a structural reason.** D3's
correction loop tests `qhat * v[n-2]` against `rhat * b + u[j+n-2]`. When
`n == 2` those are `v[0]` and `u[j]` -- the whole of the divisor and the
whole of the window -- so the estimate D3 leaves is exact and the
multiply-subtract cannot go negative. No pair of two-limb operands can reach
D6. That is why Hacker's Delight's `2^95 / (2^63 + 1)`, which that book gives
as an add-back vector for its own `divmnu`, only exercises D3 here. **Three
limbs is the smallest divisor that leaves a limb D3 cannot see.**

**The vector**, found by enumerating the corners of that shape rather than by
random search, since D6's probability is about `2/b`:

    n = 170141183381241069217422966122340155392   (4 limbs)
    d =          39614081257132168801066942463    (3 limbs)
    estimate 4294967294, true digit 4294967293 -> D6 gives back exactly 1
    q = 4294967293, r = 39614081238685424740242292733

**Verified by the same two probes that found the gap**, now reversed:

    panic!() inside the branch  -> 23 pass, ONLY this test fails
    delete `q[j] -= 1`          -> 23 pass, ONLY this test fails

The first says the test reaches D6; the second says it would notice if D6 were
wrong. Nothing else in the module does either.

**A method note worth keeping.** Before committing the vector I re-simulated
`divmod` in Python -- normalisation, the real correction loop, the borrow
arithmetic -- and checked it against the case whose answer was already
measured: it predicted 0 firings for the Hacker's Delight vector and 1 for
this one. A model that reproduces a result you have independently confirmed is
worth believing about a result you have not. The first simulation, which
clamped `qhat` to `b - 1` instead of decrementing, would have found nothing.

**Where it lives:** `userspace/coreutils/src/bignat.rs`, `Nat::divmod`.
