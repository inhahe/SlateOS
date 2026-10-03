## §463 — Two shared RNG crates merge into the dependency-free one

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Lane C had grown *two* shared "stop hand-rolling random number
generators" libraries at the same time, in different corners of the tree,
neither knowing about the other. One was used by the games, the other by the
desktop and its apps. Only one of them could ask the kernel for real
randomness — and it was the one that lives inside the GUI toolkit, so the
credential service (which has no GUI and must not link one) could not reach
it. They are now one library: the games' one, because it is the only one a
program without a screen can use.

### What was actually wrong

The recurring rule on this project is that *a lint firing many times in
well-factored code is reporting a broken abstraction*. Applied to the
hand-rolled generators, it produced a shared crate. Applied again, one level
up, it reports something worse: **the abstraction meant to fix the duplication
had itself been duplicated.**

| | `randrange` | `guitk::rng` |
|---|---|---|
| Consumers | 14 (the games) | 7 (desktop, paint, netscan, passwordgen, credmanager, spades) |
| `no_std`, no deps | yes | no — inside a widget library |
| Entropy from the kernel | **no** | yes (`SystemRandom`, §462) |
| Trait for "a source of randomness" | no — one concrete struct | yes (`RandomSource`) |
| Method named `below` | a `u64` bound | **an index** |

That last row is the sharp edge: the same word meant two different things in
two crates a single file could plausibly import together.

The cost was not theoretical. `gui/credentials` is a headless service whose
only dependencies are `sha2` and `randrange`. Its `generate_password` takes
the seed as a `u64` *parameter* — the third such defect found in this
sweep — and it could not be fixed in place, because the only wrapper for the
kernel CSPRNG lived in a GUI toolkit. The merge was not tidying; it was the
step that unblocks the fix.

### The decision

Merge into **`randrange`**, and make `guitk::rng` a re-export of it.

- **Direction — into `randrange`.** It is `no_std` and dependency-free, which
  is the property a headless component needs and the one that cannot be added
  to a widget library. The reverse direction would have left the credential
  service exactly where it was.
- **Names follow the games.** `randrange` had twice the call sites, so
  `below`/`between`/`choose` win and the clashing index-flavoured `below`
  becomes the bound-flavoured one. `guitk`'s spellings (`next_f32`,
  `f32_in_range`, `below_usize`, `pick`) were renamed at their seven call
  sites.
- **The trait and the entropy source follow the desktop.** `RandomSource`,
  `SystemRandom` and `EntropyError` move across unchanged, because they are
  the only implementation of either idea in the tree.
- **`guitk::rng` stays as a name.** `pub use randrange as rng;` — seven crates
  already say `guitk::rng::SeededRng`, and moving code is not a reason to
  rewrite their import lines.

### The reduction: Lemire *with* rejection

The two crates had also disagreed about how to turn a `u64` into a bounded
number, and each had been right about a different thing:

- `randrange` used a **widening multiply and kept the high bits**, avoiding
  the low bits of an LCG counter, which are its worst bits.
- `guitk` used a **rejection loop**, making the result exactly uniform rather
  than biased by roughly 2⁻⁶⁴.

The merged version does both — Lemire's method with its rejection step, the
threshold computed as `bound.wrapping_neg() % bound` so no 128-bit division is
needed. The bias one of them accepted is invisible in a card shuffle and
indefensible in the derivation of a password, and after this merge **the same
code path serves both**. Paying for the loop once, in a branch that is not
taken on the overwhelming majority of draws, is cheaper than maintaining an
argument about which callers deserve uniformity.

`SeededRng` also drops `Copy`. A generator that silently duplicates itself on
a move-out hands the same "random" sequence to two callers who each believe
they have their own — the failure is silent, and the value it corrupts looks
fine.

### Consequences

- `gui/credentials` can now reach the kernel CSPRNG through a crate it already
  depends on. `C-GUI-CREDENTIALS-GENERATE-PASSWORD-TAKES-A-SEED` and
  `C-THE-MASTER-PASSWORD-IS-HASHED-ONCE-WITH-A-SALT-EVERY-INSTALL-SHARES` are
  unblocked, as is withdrawing `requests/c-a-userspace-entropy-syscall.md`.
- 21 consumer crates were converted in one change. There is now exactly one
  place to fix a bug in a random number, and exactly one vocabulary for
  describing what one wants from it.
- The next person to reach for a generator has one crate to find, whether they
  have a screen or not — which is the property whose absence caused this.
