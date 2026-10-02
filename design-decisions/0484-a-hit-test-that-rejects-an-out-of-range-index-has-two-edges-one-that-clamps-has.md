## §484 — A hit test that rejects an out-of-range index has two edges; one that clamps has one

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** in gomoku a click slightly *outside* the board still lands on a
stone, because stones sit on the boundary lines and half of an edge stone hangs
off the board. The old code decided which line you clicked, threw the answer
away if it was off the board, and only then asked "were you close enough?".
That ordering gave the left and top edges a different rule from the right and
bottom: an 18-pixel overshoot past the left edge placed a stone, the identical
overshoot past the right edge did nothing. The fix is to reorder the two
questions — pin the answer to the board first, then measure the distance — so
there is one rule and it is the same on all four sides.

### The two spellings

```rust
// Before: index, reject, then measure.
let col = (mx / CELL_SIZE + 0.5) as i32;
let row = (my / CELL_SIZE + 0.5) as i32;
if col >= 0 && col < BOARD_SIZE as i32 && row >= 0 && row < BOARD_SIZE as i32 {
    // ... distance check
}

// After: clamp, then measure.
let col = (((x - BOARD_OFFSET_X) / CELL_SIZE).round() as i32).clamp(0, LAST_INDEX);
let row = (((y - BOARD_OFFSET_Y) / CELL_SIZE).round() as i32).clamp(0, LAST_INDEX);
let (ix, iy) = intersection(row, col);
let (dx, dy) = (x - ix, y - iy);
if dx * dx + dy * dy <= CLICK_RADIUS_SQ { Some((row, col)) } else { None }
```

Two questions are being asked — *which* intersection, and *is it near enough* —
and there is exactly one correct order. The distance check is the real
acceptance test; it already refuses everything that is genuinely off the board,
because a point far outside is far from every intersection. The range check is
therefore not a safety net but a second, independent edge policy layered on top
of the first, and nothing makes the two agree.

### Why it came out asymmetric, and why nobody noticed

`CLICK_RADIUS_SQ` is `15² × 1.5`, so the hit test reaches 18.371 px. Take a
click *d* pixels past an edge, with `CELL_SIZE` 36:

| edge | the index the old code computed | verdict |
|---|---|---|
| left, *d* ∈ [0, 18) | `-d/36 + 0.5` → `0` | accepted |
| left, *d* ∈ [18, 18.371] | `-d/36 + 0.5` ∈ [-0.0103, 0] → **`0`** | accepted |
| right, *d* ∈ [0, 18) | `14.5 + d/36` → `14` | accepted |
| right, *d* ∈ [18, 18.371] | `14.5 + d/36` ∈ [15.0, 15.0103] → **`15`** | **rejected** |

The two bold rows are the same click mirrored, and they disagree. The cause of
the mirror-asymmetry is that **a Rust float-to-integer cast truncates toward
zero, not downward**: `-0.0103_f32 as i32` is `0`, not `-1`. So the `col >= 0`
half of the guard was dead code — it could never fire — while the `col < 15`
half fired for real. A guard that is half-dead is worse than one that is
entirely dead, because it looks symmetric in the source.

The visible footprint is narrow: a 0.371 px band, one integer pixel column. It
is reachable — a click exactly 18 px past the right edge does nothing while 18
px past the left places a stone — but it is small enough to explain why it
survived. **That narrowness is not a reason to treat the entry as cosmetic.**
The band's width is `reach - CELL_SIZE/2`; it is small only because the slop
happens to be a hair over half a cell. Widen the slop, shrink the cell, or add
a second board with different constants, and the same code shape yields a wide
dead strip down two sides of the board. The bug is the ordering, not the 0.371.

### Testing: three ways a geometry test can be blind

The mutation sweep over the collapsed code caught 12 of 15 seeded faults on the
first pass. All three survivors were gaps in the tests, and each generalises:

1. **Probing exactly on the target cannot distinguish "nearest" from "at or
   before".** `.round()` and `.floor()` give the same answer for a click dead
   on a crossing, so a test that only clicks crossings passes under both. The
   probes were moved *beside* each crossing — seven offsets at a stone's
   radius, which is under half a cell (so the nearest crossing is unambiguous),
   inside the slop (so a hit is required), and exactly the ink a player aims at.
2. **Collecting where lines *are* says nothing about how far they *go*.** The
   grid helper returned each line's x or y, so a grid with correct spacing and
   truncated length passed. A second test now requires every horizontal to span
   the first vertical to the last, and vice versa.
3. **A board sized one line too wide shifts nothing, so nothing notices.** Every
   element is placed from `intersection`, so widening `BOARD_PIXEL_SIZE` moves
   only the wooden background — which no test looked at. A test now requires
   the background's four margins around the *painted* grid to be positive and
   equal.

After those three, 15 of 15. Note that (2) and (3) are both instances of the
same shape as §483: the expected value has to come from a second, independently
emitted render command, never from the function under test.

### Alternatives considered

| Option | Verdict |
|---|---|
| Keep index-then-reject, widen the guard to `-1..=BOARD_SIZE` and clamp after | Rejected. It makes the edges agree by adding a third constant that has to stay in step with the slop. Two numbers encoding one policy is what the entry is about. |
| Drop the slop entirely: a click hits the nearest intersection, always | Rejected. The board deliberately has gaps — the middle of a cell belongs to no intersection — because dropping a stone half a cell from where the player aimed is worse than a click that does nothing. |
| Clamp, then measure (**chosen**) | One acceptance rule, applied identically on all four sides, with the reach stated once as `CLICK_RADIUS_SQ`. The clamp cannot rescue a bad click, because the distance check runs after it. |

### Not changed: the column letters

gomoku's files are lettered `A`–`O`, which includes `I`. A Go board skips `I`,
running `A`–`H`, `J`–`P`. Whether that convention carries to gomoku could not
be settled from a primary source — the Renju International Federation rules and
Wikipedia's Renju article contain no coordinate notation at all — and the app
has no notation path (no move list, no save format, no engine protocol) where
the choice would be observable to anything but a reader of the board edge. It
is left alone deliberately rather than "fixed" into a second convention.
Contrast §482, where the outside authority *was* available and did settle it.

### Outcome

`apps/gomoku/src/main.rs`: the mapping and its inverse now live in one
`// ── Board geometry ──` section (`intersection`, `intersection_near`,
`BOARD_PIXEL_SIZE`, `LAST_INDEX`, `CLICK_RADIUS_SQ`), read by all ten former
copy sites. 112 tests green, up from 102; 15 of 15 mutations caught; clippy on
the binary unchanged at 77 and the test module's 12 warnings cleared by the
`#[cfg(test)]` allow that `CLAUDE.md` prescribes. One test that asserted
nothing at all — it ran a click and commented "it may or may not place" — now
asserts the stone count is zero (§480 again).
