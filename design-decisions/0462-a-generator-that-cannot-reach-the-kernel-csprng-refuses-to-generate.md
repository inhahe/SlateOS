## §462 — A generator that cannot reach the kernel CSPRNG refuses to generate

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The password generator needs real randomness, which on this
system means asking the kernel. If the kernel cannot be asked — say the
program is running on a build where that path does not exist — the program
has to do *something*. It could fall back to a simpler home-made generator
and produce a password anyway, or it could refuse and tell the user why. It
refuses. A password the user believes is unguessable and is not is worse than
no password at all, because they will go and use it somewhere.

### The problem

`RandomSource::next_u64` returns a `u64`. It has nowhere to put an error. So
when a `SystemRandom` cannot refill from the kernel, the trait signature
offers three ways out, none of them free:

| | What happens | Cost |
|---|---|---|
| Panic | the app dies | forbidden by the workspace lints, and a denial of service if input can shape it |
| Fall back to a `SeededRng` | a password appears | **this is the original defect, re-introduced by the fix** |
| Return a fixed value, and record the failure | a password appears, but a bad one | the caller must remember to check |

The third is the only one available, and its weakness is real: a caller that
forgets the check gets a password made of repeated first-characters.

### The decision

Take the third, and make forgetting hard rather than impossible-in-principle.

- `SystemRandom::open()` is fallible and draws its first buffer eagerly. A
  caller that cannot proceed without entropy finds out when it asks for the
  generator, not half-way through a secret.
- `is_healthy()` latches false at the first failed refill and never returns
  true again, so the condition cannot be missed by sampling at the wrong
  moment.
- `next_u64` on a poisoned source returns **zero**, not a plausible-looking
  value. If a check is ever missed, the output is visibly not random rather
  than subtly not random.
- `AppRandom::secret` wraps the whole draw and checks health on *both* sides
  of it, so the check is made once, in one place, by the type that owns the
  generator — and a source that fails part-way through a password discards the
  password rather than returning a partly-predictable one.

### Alternatives considered

**Make the whole trait fallible** (`fn try_next_u64(&mut self) -> Result<…>`
as the only method). Correct for secrets, miserable for everything else: the
screen saver, the wallpaper shuffler and the card game all want an infallible
draw, and threading a `Result` through a starfield's position update buys
nothing — the source there is arithmetic that cannot fail. Rejected because it
would make eight harmless call sites worse to make one better.

**A separate secret-only trait** that only `SystemRandom` implements, with the
generators written twice. Rejected because the tests would then exercise a
different code path from the shipped one, which is exactly the property that
lets a defect like the original hide.

**Panic on failure** — what `rand`'s `OsRng` does. Defensible in a library
whose users can catch it; not here, where the workspace forbids `panic` in
production code precisely so that a hostile input cannot take a process down.

### Consequences

Callers generating a secret must check `is_healthy` after drawing, not only
before. That obligation is documented on the type, discharged by
`AppRandom::secret` for the one caller that exists, and pinned by six tests in
`apps/passwordgen`. Any future caller that wants real entropy should route
through a `secret`-shaped wrapper rather than calling `next_u64` directly.
