## `apps/simon` dealt the same four colours in the same order, in every game, at every seed (lane C)

**Status: FIXED 2026-08-16** in `8d135ad07`. Verified by reverting the
reduction alone and watching the new test fail.

`Lcg::next_bounded` reduced the generator's output with `val % bound`. This
generator is a linear congruential generator with modulus 2^64 — i.e. it
multiplies and adds in wrapping `u64` arithmetic — and in such a generator
**bit *k* of the state has period 2^(*k*+1)**. Bit 0 alternates 0,1,0,1. Bit 1
has period 4. The low bits of a power-of-two LCG are not merely "weaker" in the
folklore sense; they are a counter. `val % bound` for a power-of-two `bound`
returns exactly those bits and nothing else.

Simon draws from four colours, so `val % 4` read the low two bits, period
exactly 4. Every game, at every seed, dealt out Green, Red, Yellow, Blue, then
Green, Red, Yellow, Blue, for ever. The step from each colour to the next was
always the same step. There was nothing to memorise — the game's whole content
was gone — and the only thing a player would notice is that they could not lose
after the first round.

**106 existing tests passed against it**, and that is the part worth keeping:
the broken draw is *perfectly uniform*. Every colour appears exactly a quarter
of the time, the mean is right, a chi-squared test on the counts is clean. Only
the *order* is degenerate. **A distribution check cannot see this bug**, so the
new test asserts that the *step* between consecutive colours varies, which is a
property of a sequence rather than of a histogram.

The fix takes the **high** bits instead, by multiplying the 64-bit output by
the bound as a 128-bit product and keeping the top half (Lemire 2019). That is
nearly unbiased, needs no rejection loop, and is total: a bound of zero returns
0 where the old body divided by zero.

Three further findings from the same read, all instances of patterns this sweep
keeps hitting: `start_next_round` asked for `next_bounded(4)` and then mapped
the answer through `from_index`, *stating the number of colours twice*;
`advance_playback` tested `step_index >= sequence.len()` and then indexed with
`step_index`, *asking one question twice*; `player_press` indexed the sequence
on an invariant maintained by four other methods. All three are now a single
`get`.
