## §465 — Randomness has two tiers: a secret refuses without entropy, novelty falls back

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Programs want unpredictable numbers for two very different
reasons — to make a password nobody can guess, and to make a maze that isn't
the same maze every time. Both ask the kernel for entropy (unpredictable bits
only the kernel can supply). The question is what to do when the kernel can't
be reached. A password generator must refuse and say so. A maze generator must
draw a maze anyway. The same answer for both is wrong in one direction or the
other, so `randrange` gives the two tiers different functions and lets the
choice of function *be* the decision.

### The problem

§462 settled the secret half: a generator that cannot reach the kernel CSPRNG
refuses to generate. Applying that rule everywhere is what the sweep in
`known-issues.md` (`C-TWENTY-MORE-HAND-ROLLED-LCGS`) actually had to do, across
~25 crates — and it immediately produced a case where refusing is clearly
worse. A pinball table that will not start is a broken program. A pinball table
that is predictable is a slightly boring program. Refusing to run a game
because the entropy pool was not ready is a self-inflicted denial of service in
exchange for a property nobody wanted.

But the converse is worse. If falling back is the default, one crate that
should have refused — and there are three of them here (`apps/passwordgen`,
`apps/credmanager`, `gui/credentials`) — silently ships a guessable secret,
and it looks identical to a working one.

### The decision

Two named entry points, so the tier is chosen at the call site and is visible
in a diff:

| Tier | Entry point | On no entropy | Used by |
|---|---|---|---|
| **Secret** | `SystemRandom::open` + `SecretSource::secret` | **refuses** — returns `None` | password generation, vault salts |
| **Novelty** | `seeded_from_system(fallback)` / `seed_from_system(fallback)` | falls back to `fallback` | mazes, shuffles, card deals, wallpapers, spectrum animations, simulated meters |

**The fallback seed is a per-crate parameter, not a constant inside
`randrange`.** This is the part that is a real decision rather than an obvious
one. A single shared fallback constant would be simpler and would read fine —
but on a boot where entropy is unavailable, *every* program that fell back
would start from the same number. The solitaire deal and the maze and the
wallpaper rotation would all be correlated, and worse, they would be correlated
in a way that recurs identically on every such boot. Making it a parameter
costs each crate one `const FALLBACK_SEED` line (spelled as ASCII in hex —
`0x4D55_5349_4350_4C52` is "MUSICPLR" — so it is self-evidently per-crate) and
buys independence between programs in the degraded case.

**Two forms of the novelty entry point**, because some callers store the seed.
`seeded_from_system` returns a generator; `seed_from_system` returns the `u64`.
Games that keep their seed so "new board" can be `with_seed(self.seed + 1)`
need the latter — mahjong, sudoku and simon do. A crate holding only a
generator would have to reseed one generator from another's output, which
correlates them silently.

### Alternatives considered

**One function with a `Result`, and let each caller decide.** This is what the
code looked like before the rule was named: each of the three secret-drawing
crates had independently written the same fail-closed guard, and each had
written it slightly differently. Rejected because it makes the *safe* choice
the one that requires remembering — and the failure mode of forgetting is
invisible. Naming the tiers moves the decision to a place a reviewer can see.

**Refuse everywhere, uniformly.** Simplest rule to state, and it is what §462
says in isolation. Rejected on the pinball case above: it converts a cosmetic
degradation into a program that will not run, in ~22 crates, to protect a
property none of them have.

**Fall back everywhere, uniformly.** Rejected outright — it is the original
defect that §462 exists to prevent.

**A single shared fallback constant.** Rejected for the boot-correlation
argument above. The counter-argument is genuine: per-crate constants are 25
places to get wrong, and a crate that copies another's constant re-creates the
problem invisibly. That risk is accepted because the constants are written as
readable ASCII, which makes a copied one look wrong on sight.

### Consequences

- The tier is now visible at every call site in lane C, and a crate that draws
  a secret through `seeded_from_system` is a reviewable mistake rather than an
  invisible one.
- Seeding must be tested by asserting **which** seed, not that two runs differ.
  A host `cargo test` has no SlateOS kernel, so `seeded_from_system` takes the
  fallback and two fresh objects *are* identical — exactly as they were under
  the hardcoded `42` this sweep removed. A variety check therefore passes on
  the broken code. Those tests assert `fresh == with_seed(FALLBACK_SEED)` and
  `fresh != with_seed(<the old literal>)`, and are `#[cfg(not(unix))]`.
- `SecretSource` lives in `randrange` but the `System`/`Seeded`/`Unavailable`
  enums stay in each consumer: their `Seeded` variant is `#[cfg(test)]`, and
  `cfg(test)` does not cross a crate boundary, so a `Seeded` hoisted into
  `randrange` would be reachable from production code everywhere.
