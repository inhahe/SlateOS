## The same broken reduction is copy-pasted into 27 crates, and `randrange` now exists to replace it (lane C)

**Status: CLOSED 2026-08-20 — the sweep finished; no private copy remains.**
The status line below sat at "fourteen of 27" from 2026-08-16 while the
migration actually ran to completion (`7d147cb83` retired the last three
private `Lcg`s), so the entry was stale rather than open. Verified two ways on
2026-08-20: `build/scratch/lcg_scan.py` — the scan that produced the 27-crate
figure in the first place — now prints **nothing** across `apps/**` and
`gui/**`, and a plain grep for the LCG multiplier over `apps`, `gui`, `net*`
and `pkg` finds exactly **one** hit, inside `apps/breakout`'s `#[cfg(test)]`
`opening_angle_lattice_points`, which *deliberately* reimplements the old
`Lcg::next_u64` so the new generator can be compared against the old one on a
single lattice. That is a regression test *about* the defect, not a surviving
instance of it, and it should stay. 16 crates now depend on `randrange`.

Original status line, kept for the record: *OPEN 2026-08-16 — fourteen of 27
crates fixed (`simon`, `battleship`, `sliding`, `asteroids`, `yahtzee`,
`hearts`, `solitaire`, `freecell`, `minesweeper`, `flood`, `snake`,
`wordsearch`, `pacman`, `breakout`); the shared crate that the rest should
move to is written and green.*

The defect above is not `simon`'s. A scan of the tree
(`build/scratch/lcg_scan.py`) finds the same LCG constants in **~36 places** and
the same low-bit `%` reduction in **27 crates**. It is one piece of code that
was pasted 27 times, so it has 27 copies of one bug.

Whether a given copy is visibly broken depends only on whether its bound is a
power of two, because an odd factor in the bound makes the remainder depend on
the whole word again and the period comes back. But **an even bound is enough
to matter even when it is not a power of two**, because `x % n` for even `n`
preserves the parity of `x` — so any draw with an even bound has a *parity* that
alternates with the draw counter, whatever else it does. That is how the
battleship defect below works, with a bound of 10.

Confirmed degenerate call sites, with the measured symptom where one has been
measured:

| crate | bound | what it did | status |
|---|---:|---|---|
| `apps/simon` | 4 | dealt Green, Red, Yellow, Blue for ever, at every seed | fixed `8d135ad07` |
| `apps/battleship` | 2, 10 | every AI ship's bow on an odd `row + col` — one colour of the checkerboard, over all 1999 seeds tried | fixed `7544fdb3c` |
| `apps/sliding` | 4 | 499 seeds produced **two** distinct 4×4 boards between them | fixed `de4280a98` |
| `apps/asteroids` | 4 | every asteroid of every wave entered from **one** edge; the seed only chose which | fixed `762afae1f` |
| `apps/yahtzee` | 6 | adjacent dice locked to opposite parity, so a Yahtzee was impossible — zero in 15 000 rolls, ~12 expected | fixed `81b88fc5e` |
| `apps/hearts` | 2..52 | the two of hearts reached seat 2 44.3% of deals and seat 0 9.5%; 40 of 52 cards misdealt by >2 points; seat 0 led 33.6% of hands | fixed `63066a45b` |
| `apps/solitaire` | 2..52 | the two of hearts was the leftmost face-up card in 17.6% of deals, against 1.9% | fixed `ea4560541` |
| `apps/freecell` | 2..52 | the ace of hearts was the deepest card on the board in 17.4% of deals; ace depth alternated 8.5 / 15.5 / 9.7 / 16.2% | fixed `e1fa03154` |
| `apps/minesweeper` | 256 | an Intermediate board was a window into one 256-cell cycle: 5000 seeds gave 252 layouts, and the 257th new game replayed the first | fixed `3f28501a6` |
| `apps/flood` | 6 | no two cells in a row ever shared a colour — zero matches in 61 200 pairs; each column used half the palette | fixed `15d7265d6` |
| `apps/snake` | 20, 20 | the food could reach 50 of the 400 cells, in a fixed diagonal lattice | fixed `4d448a617` |
| `apps/wordsearch` | 26, 2..30 | every second filler letter came from {A,C,…,Y} and the ones between from {B,D,…,Z} — 0 repeats in 2000 draws, 76 expected; and BISON was picked for 14% of puzzles against ZEBRA's 41% | fixed `4cce605f8` |
| `apps/pacman` | 31, 28 | each frightened ghost could flee to 7 of the 28 columns and 217 of the 868 cells; all four together to 14 columns, the seed choosing only *which* 14 | fixed `481f36e8d` |
| `apps/breakout` | 1000 | every game in a session opened at the same launch parity: 500 of the 1000 angles and 4 of the 8 residues mod 8 over a chain of 5000 new games | fixed `589045fe1` |

**The three card games are one shape and worth reading together.** All three
shuffle 52 cards with a correct downward Fisher–Yates and draw the partner with
`state % (i + 1)`. Half of those 51 bounds are even, so on 25 of the swaps the
partner index carried a fixed parity — set by the parity of the draw counter,
which is set by the seed and nothing else. `solitaire` and `freecell` make it
worse than `hearts` does, because their `new_game` *reseeds* (`Rng::new(self
.rng.next_u64())`): the draw counter restarts at zero every deal, so every deal
repeats the same pattern of fixed parities and only which parity varies. That
is why one card could own the same slot in a sixth of all games.

`apps/pipes` was in this table and should not have been: its `next_bounded`
shifts (`>> 33`) before the `%`, so it never touches the low-bit counter. The
same correction applies to `apps/game2048`, `apps/mahjong`, `apps/spades`
(`>> 32`), `apps/videoplayer`, `apps/speedtest` (`>> 33`) and
`apps/credmanager` (an xorshift finaliser) — **all seven are safe**, and the
scan that flagged them matched the LCG constants rather than the reduction.
Confirm the reduction, not the constants, before adding a crate here.

`apps/life`'s `next_bool` uses `% 100`; 100 = 4 × 25, and the odd factor 25
restores a long period, so it is mild. That is exactly why the defect survived
so long: most call sites use an odd or non-power-of-two bound and look fine,
and the ones that do not still pass every distribution test written against
them.

Still degenerate, not yet migrated: `dots`, `hangman`, `life`, `lightsout`,
`match3`, `maze`, `memory`, `pinball`, `sudoku`, `tetris`, `wordle`.

**Those eleven are deliberately parked, not merely pending.** Every one of them
is a game, and as of 2026-08-16 no game is reachable from the desktop:
`gui/desktop/src/launcher.rs` hardcodes eighteen entries, not one of which is an
`apps/` game, and its `Category` enum (`Application`, `System`, `Setting`,
`File`, `Command`) has no games variant to put one in. No game name appears
anywhere outside `apps/`. Running one requires knowing the binary name and
typing it into a shell, so the audience for these eleven defects is currently
zero — which is the ground the migration was stopped on at fourteen of 27, not
any technical one.

**Trigger to resume: the first time a game gets a launcher entry, fix the
remaining eleven before shipping that entry.** The moment a game is discoverable
it has players, and this defect lands hardest exactly where a player would
notice — a fixed piece order in `tetris`, a fixed word in `wordle`/`hangman`, a
fixed layout in `maze`/`sudoku`. The already-fixed cases show the ceiling:
`simon` dealt Green-Red-Yellow-Blue for ever at every seed, and `sliding`
produced two distinct boards across 499 seeds. Each remaining crate is a
measure-migrate-test cycle of the same shape as the fourteen already done, so
the whole tail is a few hours of work rather than a project.

**`breakout` is the case for not clearing a crate because its bound has an odd
factor.** Its only bound was 1000, and by the rule of thumb above the factor 125
should have restored the period — the *values* are uniform and a histogram of
them is perfect. But 8 also divides 1000, and that is enough: `(state % 1000) %
8` is exactly `state % 8`, so the draw's bottom three bits are the raw LCG's,
whatever the other factor does. **Check a bound for a power-of-two divisor, not
for being a power of two.** `1000 = 8 × 125` reads as safe and is not; the same
reasoning re-opens `life`'s `% 100` (= 4 × 25), which this entry has been
calling mild, and it should be re-measured rather than assumed when its turn
comes.

**`breakout` is also the clearest case of a reseed turning fine structure into
half the range.** A period-8 cycle in the bottom bits is usually invisible,
because a game draws often enough to walk the whole cycle. What made it
permanent is that `start_game` reseeds from the running generator and the fresh
generator's *first* output is the opening launch angle — `init_bricks` draws
nothing in between. So the one draw a player meets at the start of every single
game was pinned to a fixed stride through that cycle: over 5000 consecutive new
games the opening angle held one parity for ever and reached 4 of the 8 residues
mod 8. **When a crate reseeds per game, look hardest at the first draw after the
reseed** — it is both the most exposed to the low-bit cycle and the one the
player sees most often. `solitaire` and `freecell` are the same shape.

The corollary for the remaining eleven: *counting distinct values is not enough
to clear a draw.* Breakout's chain produced 500 distinct opening angles out of
1000, which looks plentiful and would pass any "does it vary?" test. The parity
is the part that had to be asserted, and it is one line away.

**`pacman` is the cleanest case of the two-draws-per-consumer stride**, and it
is the one to remember when a crate draws for several actors in a round-robin.
Its two bounds sit side by side in one expression — 31 for the row, 28 for the
column — and the odd one behaved perfectly while the even one lost most of the
board. Because four ghosts draw a pair each per tick, every ghost saw a draw
counter of the *same* parity for ever, so the parity coupling never averaged
out the way it does for a single consumer. Counting the reach of all four
ghosts together hides this: 14 columns of 28 looks merely halved, when each
individual ghost is confined to 7.

**`wordsearch` is the first crate whose two call sites broke in two different
ways, and it is the argument for checking every bound rather than the worst
one.** Its filler alphabet (`% 26`) failed loudly — a visible comb across the
grid. Its Fisher–Yates over the 30-word category list failed quietly: the word
list still looked random, and only counting over 5000 games showed one animal
in 14% of puzzles and another in 41%. A crate can be cleared on the symptom you
went looking for and still be wrong in the way you did not.

**Three bound shapes account for every symptom found so far**, and it is worth
checking a call site against all three rather than only the first:

1. **A power-of-two bound** makes the draw the low *n* bits, which are an LCG
   of the same shape modulo 2^*n* — period 2^*n*, and a permutation of the
   whole range. `simon` (4), `sliding` (4), `asteroids` (4), `minesweeper`
   (256). This is the one that makes a *distribution* test pass perfectly
   while the sequence is worthless.
2. **Any even bound** preserves the parity of the state, so the draw's parity
   alternates with the draw counter. `battleship` (10), `yahtzee` (6), `flood`
   (6), the card games (the even half of `2..52`). Costs half the range when
   two draws are compared to each other.
3. **An even bound with a power-of-two factor** does both. `snake` (20 = 4×5)
   lost half the board to the parity coupling and half of what was left to
   `row % 4` having period four.

The replacement is **`randrange/`**, a new top-level `no_std`,
dependency-free crate on the same pattern as `textfind`, `byteread` and
`yamldoc`. It fixes the bug twice over, deliberately:

1. **The reduction uses the high bits** (the Lemire multiply above), so no
   caller can pick up the low-bit counter through the API.
2. **The generator's output is permuted** by SplitMix64's finaliser before it
   is returned, so `next_u64()` has no weak bits *at all* — a caller who
   ignores `below()` and writes their own `% 2` is still fine.

Either defence alone makes the cycle test pass, which is why the crate also
carries `the_original_defect_still_cycles_when_reproduced`: it rebuilds the
historical `state % bound` body inside the test and asserts it *does* cycle, so
the test pins the claim rather than the implementation. See
`design-decisions.md` §447 for why one crate rather than 27 local fixes, and
for what `randrange` deliberately is not (it is **not** cryptographic —
`apps/passwordgen` must not migrate to it).

**Migration is the open part.** The remaining 26 crates each need their local
`Lcg` deleted and `randrange::Rng` used instead. `randrange` is *not*
stream-compatible with the code it replaces — the same seed gives a different
sequence, necessarily, since that is the fix — so any test that pinned a
specific board layout, shuffle or spawn pattern will need a new expectation.
Each of those is worth reading rather than re-baselining: a test that pins a
layout is asserting a photograph, and the question it should have been asking
(is the board solvable, are the ships non-overlapping, does the sequence vary)
is usually one line away and would have survived the fix.

**Four ways a test can look like it covers this and not.** All four were found
while migrating, and all four are worth checking for in the remaining twenty:

- **A distribution check cannot see it.** `simon`'s broken draw used all four
  colours exactly equally; only the order was fixed. A histogram is the right
  answer and the sequence is still worthless.
- **A marginal check cannot see it.** `battleship`'s rows were uniform and its
  columns were uniform. Only the *joint* distribution — row against column —
  was degenerate, and that is where the parity coupling lives.
- **Two samples cannot see it.** `sliding`'s `test_shuffle_different_seeds`
  compared seed 1 against seed 2 and asserted they differed. They did. There
  were only ever two boards in the whole seed space, and the test happened to
  draw one of each. "Are these two different?" cannot distinguish two outcomes
  from a thousand; count distinct results over a hundred seeds instead.
- **Pooling across seeds cannot see it.** The first draft of `asteroids`'
  spawn-edge test gathered edges from twenty seeds into *one* set and asserted
  all four appeared. It passed against the broken generator, because the defect
  was per-game: each seed picked one edge and used it for every asteroid, and
  twenty seeds between them covered all four. The defect is only visible inside
  a single game, so build the set per seed and assert inside the loop. This one
  produced a false green that was caught only by running the revert.

### Sweep progress: `contacts` 83 → 0, all lint classes (2026-08-16)

Twelfth crate, seventh to reach **zero warnings of every class** across
`--all-targets`. Tests 190 → 195.

The most lopsided split of the sweep so far: of 83 warnings, **82 were in the
test module and exactly one was in production code** — a single
`indexing_slicing` on a twelve-element lookup table. Following that one warning
found two user-visible defects and a third latent one, which is the sweep's
recurring lesson stated as starkly as it gets: *the count of warnings in a
crate says nothing about the number of defects in it.*
