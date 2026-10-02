## [C] FIXED — the password generator produced the same passwords for everyone (2026-08-18)

**In short:** `apps/passwordgen` generated passwords, PINs and passphrases
from a *pseudo*-random generator seeded with the literal number 42. A seed is
the number a pseudo-random generator starts from; the same seed always gives
the same sequence. So every user, on every machine, got the same first
password, the same second one, and so on forever. Anyone who ran the program
once knew every password it would ever hand out to anybody.

Fixed in this batch. Recorded here rather than merely closed because the shape
of the mistake is worth keeping: nothing in the code *said* it was a toy
generator, so a reviewer reading `generate_password(&opts, &mut rng)` saw
nothing wrong with it.

### What was wrong

- `apps/passwordgen/src/main.rs` carried its own `struct Rng` — Marsaglia
  xorshift64. Fast, tiny, and trivially invertible: recovering the state from
  a single 64-bit output is a few lines of algebra, so one generated password
  reveals every other password of that session.
- `fn main()` called `PasswordApp::new(42)`.
- Even reseeded from a clock it would still have been wrong. The defect is the
  *kind* of generator, not the seed.
- `Rng::next_usize` reduced with `%`, so characters early in each pool came up
  slightly more often than late ones.

### The fix

- `guitk::rng` now owns the generator abstraction: `RandomSource` (unbiased
  `below` by rejection sampling; `next_f32` from 24 bits so it cannot reach
  1.0; Fisher-Yates `shuffle`), `SeededRng` for variety, and `SystemRandom`
  for secrets — bytes from the kernel CSPRNG via the posix `getrandom` symbol,
  the same route `userspace/ssh-keygen` takes for key material.
- `SystemRandom::open()` proves the kernel answers before returning a
  generator at all, and `is_healthy()` latches false if a later refill fails.
- `passwordgen`'s `AppRandom` makes the choice explicit and checkable.
  `PasswordApp::new()` opens the kernel CSPRNG; `PasswordApp::with_seed()`
  exists for tests and is named so it cannot be reached by accident.
- **It fails closed.** With no entropy, every Generate button clears the
  display, records no history, and shows "Cannot generate: the system random
  number generator is unavailable" in red where the password would be. A bulk
  run that loses entropy part-way discards the whole batch rather than return
  a short list. Six regression tests pin this.

### Still to do

**Corrected 2026-08-18.** This section originally listed five generators and
said "none of them generate secrets, so this is tidying rather than security."
That was wrong on both counts. The survey behind it grepped for one xorshift
constant and missed everything written differently — and one of the things it
missed, `apps/credmanager`, generates credentials people store and use. See
the entry below it.

The lesson is worth keeping: a "have I found them all?" sweep that greps for
the *implementation* finds only the copies that look like the one you started
from. The sweep that found the rest looked for `wrapping_mul` with either LCG
constant, for every shift triple, and for the word `seed` — and still turned
up a generator in a *third* password generator that had been converted to a
crate and so matched none of them.

Converted since: `gui/desktop/src/power.rs`, `gui/desktop/src/wallpaper.rs`
(the slideshow shuffle), `apps/paint`, `apps/netscan`, `apps/spades`,
`apps/credmanager`.

Still outstanding, none of which generate secrets:

| Where | What |
|---|---|
| `gui/desktop/src/wallpaper.rs:758` | a second LCG, separate from the shuffle |
| `gui/toolkit/src/listview.rs:427` | an LCG |
| `apps/speedtest/src/main.rs:414,546` | an LCG, twice |
| `apps/videoplayer/src/main.rs:1291` | an LCG |
