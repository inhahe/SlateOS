## C-THE-SHARED-RNG-CRATE-WAS-ITSELF-DUPLICATED (lane C, 2026-08-18) — FIXED

Two crates existed to end hand-rolled random number generators, written at
roughly the same time in different corners of lane C, neither aware of the
other: `randrange` (14 consumers, all games, `no_std` and dependency-free, no
entropy source) and `guitk::rng` (7 consumers, `RandomSource` + `SystemRandom`,
inside the GUI toolkit). They used the same method name `below` for two
different things — a `u64` bound in one, an index in the other.

This was not a cosmetic overlap. `gui/credentials` is a headless service whose
dependencies are `sha2` and `randrange`; the only wrapper for the kernel
CSPRNG lived in a widget library it must not link. The entry above says its
third `generate_password` "should draw from the kernel" — it could not, until
the entropy source moved somewhere a program without a screen can reach it.

Merged into `randrange` (see design-decisions.md §463): names follow the games,
the trait and the entropy source follow the desktop, `guitk::rng` becomes
`pub use randrange as rng;` so no call site changes meaning. The bounded
reduction now uses Lemire's high-bit multiply *with* a rejection step, so the
one code path is exactly uniform — it is what a password draws through now,
not just a card shuffle. `SeededRng` drops `Copy`, because a generator that
duplicates itself on a move-out gives two callers the same sequence silently.

21 consumer crates converted; randrange 25 tests, all affected crates green.

### The sweep missed an eighth generator, again

`apps/pinball/src/main.rs` carried a private `struct Rng` — the literal
copy-pasted LCG that `randrange`'s module docs dissect, reduced with
`% bound` — and **two** earlier sweeps for hand-rolled generators did not find
it. It surfaced only incidentally, while grepping for consumers during this
merge.

Two defects in it:

- `Pinball::new()` seeded a fixed `42`, so every session's first table was
  identical: the same multiball throw, every launch, forever. Nobody reported
  it because a pinball table looks random the first time you see it.
- `next_bounded` reduced with `%`, which on this LCG biases toward the low end
  of the range. It was only ever called from tests, so the bias never reached
  play — which is why it survived.

Fixed: seeds from `SystemRandom`, falling back to a fixed seed rather than
refusing to start (the spades precedent — a table may be predictable; a machine
that will not start is the worse failure, and this is a loss of *variety*, not
of confidentiality). `with_seed` became `with_rng` so a new table carries the
old generator forward instead of reseeding one from the other's output. Its
four generator tests were deleted and replaced with three that test what the
*game* needs from randomness — chiefly that multiball throws its extra balls
both ways across 40 seeds.

**The lesson for the next sweep:** grepping for the *name* of a thing finds
the copies that kept the name. Both misses here were crates that had renamed
the struct or inlined the arithmetic. Grep for the constants instead — the
multiplier `6364136223846793005` and its LCG siblings are far harder to
disguise than the word `Rng`.

### Both of those are now done (2026-08-18, `7b9a1bace` and `6771f575e`)

`C-GUI-CREDENTIALS-GENERATE-PASSWORD-TAKES-A-SEED` and
`C-THE-MASTER-PASSWORD-IS-HASHED-ONCE-WITH-A-SALT-EVERY-INSTALL-SHARES` were
unblocked in the strongest sense — `gui/credentials` already depended on
`randrange`, which now carries `SystemRandom` — and both are fixed:

- **The seed parameter is gone.** `generate_password` draws from the kernel
  and returns `Option<String>`, refusing rather than falling back. Fixed
  before it ever acquired a caller, which was the point.
- **`KEY_DERIVATION_SALT` is gone.** Every vault draws its own 16 bytes; see
  the marked-up entry above and design-decisions §464.
- Both stale "there is no source of unpredictable numbers in userspace yet"
  comments are deleted, and `requests/c-a-userspace-entropy-syscall.md` is
  marked **resolved** rather than deleted — it turns out the syscall it asked
  for had existed the whole time, and that is worth keeping a record of. The
  correction is written up under
  `C-THERE-IS-NO-RANDOMNESS-SOURCE-FOR-USERSPACE`.

**A third copy of the same wrapper appeared while fixing this**, which is the
signal the top of this entry is about: `apps/passwordgen`, `apps/credmanager`
and `gui/credentials` had each independently written the same fail-closed
`secret()` guard. The *rule* moved into `randrange::SecretSource`; the
`System`/`Seeded`/`Unavailable` enums stay in each consumer, because their
`Seeded` variant is `#[cfg(test)]` and `cfg(test)` does not cross a crate
boundary — a `Seeded` defined in `randrange` would be reachable from
production code in every caller, which is the one thing those enums exist to
prevent.

### The constant grep, run immediately, found twenty more

The list here originally read "four non-secret hand-rolled generators remain".
That number came from the name-based sweeps. Running the constant-based grep
this entry recommends — one command, thirty seconds — turned up **twenty**, in
twenty-one files (one of which, `apps/breakout`, is a deliberate reproduction
of the historical generator inside its own tests, and is not a defect).
Tracked as `C-TWENTY-MORE-HAND-ROLLED-LCGS`.

Fourteen carry a whole private `struct Lcg` / `struct Rng`, copy-pasted:
`apps/{dots,game2048,hangman,life,lightsout,mahjong,match3,maze,memory,pipes,
simon,sudoku,tetris,wordle}`. Four have the arithmetic inlined with no type at
all — `apps/{flashcards,radio,speedtest,videoplayer}` — which is why a grep for
`Rng` could never have found them. Two are in the toolkit and the desktop:
`gui/toolkit/src/listview.rs:427` and `gui/desktop/src/wallpaper.rs:758` (a
*second* LCG in that file, separate from the shuffle already fixed).

Nearly all of them repeat pinball's *other* defect as well: `new()` seeds a
literal `42` (or `123`, or `12345`), so every session of that game is the same
session — the same maze, the same tetromino order, the same word. That is the
more visible bug of the two, and it has been shipping in fourteen games.

`% bound` appears in most of them and `(next() >> 33) as usize % max` in the
rest — the same reduction `randrange`'s module docs exist to explain, in
crates that were never told the crate exists.

#### All fourteen struct-carrying crates are done (2026-08-18)

Every crate that carried a private `struct Lcg` / `struct Rng` now uses
`guitk::rng` (`= randrange`), seeds from the system with a per-crate fallback
constant, and has tests for the hazard its own draw pattern was exposed to:
`apps/{dots,game2048,hangman,life,lightsout,mahjong,match3,maze,memory,pipes,
simon,sudoku,tetris,wordle}`. A constant grep across `apps/ gui/ net*/ pkg/`
now returns only the inlined sites listed below plus `apps/breakout:2129`,
which is the deliberate historical reproduction inside its own tests.

**What the sweep actually found, measured rather than assumed.** Each rewrite
was checked by putting the old reduction back into `randrange` and re-running
the new tests against it, and the results were not uniform:

| Crate | Draw pattern | Old reduction's effect |
|---|---|---|
| `apps/maze` | 4-element shuffle + 1 neighbour draw = 4 draws/step | **3 of 24** orderings, all ending on the same direction |
| `apps/hangman` | 1–2 draws per round | free reveal ran a fixed 4-cycle at Medium |
| `apps/tetris` | 7-element bag shuffle | 6 dead cells in the 7×7 piece/slot table |
| `apps/sudoku` | 9-digit shuffle inside the solver | corner-digit histogram `[19,17,11,3,9,5,15,2,9]` where fair is 10 each |
| `apps/mahjong` | 144-tile shuffle | **none measurable** — 143 draws at mostly odd bounds wash the counter out |
| `apps/game2048`, `apps/pipes` | — | **none** — these copies shifted (`>> 33`) before reducing |

The bottom two rows are the point of the whole entry rather than an
embarrassment to it. Three of the sixteen copies were harmless, and there was
no way to know *which* three without reading all sixteen and measuring each.
The copy is the defect; a good copy is still a copy.

Two lessons worth keeping, both learned by first writing a test that passed on
broken code:

1. **Sample along one generator's stream, never across fresh seeds.** A
   low-bit defect is a counter *within* a stream; different seeds have
   different low bits, so re-seeding between samples hides it completely. A
   hangman test that sampled 200 freshly-seeded generators found zero missing
   letters under the broken reduction.
2. **Reproduce the caller's whole draw pattern, not just the shuffle.**
   `apps/maze`'s shuffle in isolation reaches 12 of 24 orderings; add the one
   extra neighbour draw the caller makes each step and it collapses to 3. A
   test of the shuffle alone would have missed the bug entirely, which is how
   it survived.

Seeding is tested by asserting **which** seed, not that two games differ: a
host `cargo test` has no SlateOS kernel, so `seed_from_system` takes the
fallback and two fresh games *are* identical — exactly as they were under the
old hardcoded `42`. A variety check therefore passes on the broken code and
fails on the fix. Those tests are `#[cfg(not(unix))]` for that reason.

~~**Still open**, all with the arithmetic inlined and no type at all — the ones
a name-based grep could never find:~~

- ~~`apps/flashcards:356` (`1_664_525` / `1_013_904_223`, so even the
  constant grep recommended above misses it — a *third* set of constants)~~
- ~~`apps/radio:553` (`1103515245` / `12345`)~~
- ~~`apps/speedtest:414,546`~~
- ~~`apps/videoplayer:1291,1295`~~
- ~~`gui/toolkit/src/listview.rs:427`~~
- ~~`gui/desktop/src/wallpaper.rs:758`~~

#### CLOSED 2026-08-18 — and the inventory itself was the last defect

All six inlined sites are converted (`5af5b1c9e`, `d9deeb3a4`, `55ef7b558`).
A constant grep across `apps/ gui/ net*/ pkg/` now returns `apps/breakout:2129`
— the deliberate historical reproduction inside its own tests — and two doc
comments that name the old constants in order to explain what was removed.

**The list above was wrong, and it was wrong for a structural reason.** It was
assembled by grepping for `6364136223846793005`, exactly as the "lesson for the
next sweep" three sections up recommends. That lesson was an improvement on
grepping for the name `Rng`, and it is still what caught fourteen of the
crates — but it inherits the same flaw one level down. **Keying an inventory on
any token keys it on the copies that kept that token.** The entry even records
its own counter-example without drawing the conclusion: flashcards is listed
with a note that it uses a *third* set of constants and "even the constant grep
recommended above misses it". It was in the list only because it had been found
some other way.

Two crates were missed entirely, and both were worse than any of the twenty
counted:

- **`apps/musicplayer`** — shuffle was `(idx * 7 + 3) % len`, which is not a
  generator at all but an affine map, and its orbit is a fixed cycle. Measured
  on a real album: **a 12-track album shuffled between exactly 2 of its
  tracks**, and a 7-track list reached **1**. The visualiser was
  `(t * 31 + i * 7) % 100` — a sliding staircase, not noise. The shuffle
  branch also never consulted `repeat_mode`, so shuffle could never end.
- **`apps/mixer`** — peak meters were `(id * 7 + tick * 13) % 100`: stream 1
  read 20, 33, 46, 59, 72, 85, 98, 11, 24 — thirteen hundredths per tick,
  period exactly 100, and every stream ran the same ramp offset by 7 per id.
  Eight channels displayed eight copies of one climbing bar.

Neither contains a single LCG constant, so no constant grep at any width would
have found them. What found them was **grepping for the behaviour**: the
comment phrases people write when they roll their own —
`pseudo.rand|pseudo-rand|pseudorandom|simple random|fake random|deterministic
shuffle|poor man's random|cheap random` — plus a broad `wrapping_mul` scan.
Four lines of grep, and it found the two worst sites in the lane.

**The lesson, superseding the one above:** an inventory keyed on a token
measures how uniform the copying was, not how much of it there is. Grep for
what the code *does* and for how a person *describes* doing it. Both misses
here announced themselves in a comment.

**And not all of it is even a generator.** Three distinct failure shapes turned
up in this tail, none of them the low-bit LCG bias the entry was written about:

| Shape | Example | What it actually is |
|---|---|---|
| Affine map | `(idx * 7 + 3) % len` | a fixed cycle; visits `len / gcd(7, len)` values |
| Arithmetic ramp | `(t * 31 + i * 7) % 100` | a sawtooth with a known period |
| Width mismatch | `(state >> 33) / u32::MAX` | 31 bits over a 32-bit maximum — a "fraction" that never reaches 0.5 |

The third is the one to watch for, because it looks like arithmetic rather than
randomness and reads as correct. It made **speedtest's simulated jitter
one-sided**: `frac - 0.5` was always negative, so 0 of 20 latency probes landed
above the base and 0 of 60 throughput steps above the target. A progress bar
that only ever undershoots.

**Also fixed while here**, each found by the same behaviour sweep rather than by
any list: `apps/videoplayer`'s shuffle was a fixed permutation (1 distinct order
over 10 rebuilds; only track 5 ever came first over 60 runs), and
`gui/desktop`'s `populate_slideshow_paths` demanded its shuffle seed from a
caller that did not exist yet — the same defect `random_wallpaper` in that file
had already been fixed for, surviving because the two were read separately
(`1003a4dae`).

**Two tests in this tail passed on broken code and were caught only by the
reintroduce-the-defect step**, which is the strongest argument for keeping that
step mandatory:

- `the_visualizer_is_not_an_arithmetic_ramp` first asked that bar-to-bar steps
  "not all be equal" — but the ramp's modulo wrap supplies a second step value,
  so it passed. Counting *distinct* steps bucketed to a hundredth fails it:
  "bar heights take only 2 distinct steps -- a ramp, not a level".
- `different_streams_have_independent_peak_levels` first asked for a spread of
  differences between two meters — but the smoothing is asymmetric (attack 0.6,
  decay 0.15), so a constant target-offset still yields a varying level-offset,
  and it passed. Counting direction *disagreements* fails it: "the two meters
  moved the same way on all but 18 of 120 ticks -- lockstep".

Both near-misses are recorded in the comments above the tests that replaced
them, so the next reader learns the shape rather than just the assertion.
