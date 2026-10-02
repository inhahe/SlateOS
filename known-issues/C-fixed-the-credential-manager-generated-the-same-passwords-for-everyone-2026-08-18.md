## [C] FIXED — the credential manager generated the same passwords for everyone (2026-08-18)

**In short:** `apps/credmanager` — the app whose whole job is to make and keep
strong passwords — built every password from a fixed starting number, `12345`,
that was bumped by one each time you pressed Generate. The result was not
random at all: the first password it ever made for you was the first it made
for everyone who ever ran it, the second was the second, and so on. Anyone who
had the program could work out the passwords it would produce. Meanwhile the
meter beside the password said things like "94 bits of entropy", which is the
strength of a password picked at random — a true statement about the *settings*
and a false one about the password on screen, whose real strength was zero.

This is the same defect as the `apps/passwordgen` entry above, found by the
follow-up sweep that entry's "still to do" list should have done and didn't.
It is worse in one respect: passwordgen makes a password you look at, while a
credential manager makes one you *save and use*, so a bad password here is
still protecting an account tomorrow.

### What was wrong

- `PasswordGenerator::new()` set `seed: 12345`. There was no other source of
  randomness in the type.
- `pseudo_random(bound, offset)` was not even a running generator — it was a
  stateless integer hash of `seed + offset`. So the *n*th password was a pure
  function of `n`, published in the source. No state to recover, nothing to
  observe: you could compute the whole sequence from the repository.
- It reduced with `% bound`, so characters early in each pool came up slightly
  more often — the least of the problems here.
- `entropy_bits()` reported `length x log2(pool_size)` regardless. That number
  is a property of a password drawn uniformly at random; attached to this one
  it was a false assurance shown in the UI.

### The fix

The same shape as `passwordgen`, which is the point — one pattern, so the next
reviewer recognises it:

- `CredRandom` with exactly two live variants, `System(Box<SystemRandom>)` and
  `Unavailable`, plus a `#[cfg(test)]` `Seeded`. There is deliberately no
  fallback between them: a fallback is how the original defect survives the
  fix, and nobody can tell a predictable password from an unpredictable one by
  looking at it.
- `CredRandom::secret` checks health on both sides of a draw, so a kernel that
  fails on a refill halfway through a password yields a refusal rather than a
  password whose second half is zeroes.
- `generate()` returns `Option<String>`. The three call sites now go through
  one `regenerate_password` helper, which on refusal clears the password that
  was showing and records the message — leaving the old one on screen beside
  "cannot generate" reads as an offer to keep using it.
- The refusal is rendered in red *where the password would be*. Left in the
  "Click Generate to create a password" state it would read as "you have not
  pressed the button yet", which is the wrong thing to tell someone whose
  generator cannot generate.
- The test module gained the house lint-allow block it never had, which is why
  seven `expect_used` findings in it had been showing up as real ones.

Nine tests: every mode refuses without entropy, two generators never agree,
pressing Generate twice gives two different passwords, a refusal clears the
displayed password, a success clears an earlier refusal, and the refusal
reaches the screen.

### Related, not yet fixed

`gui/credentials/src/main.rs` has a *third* `generate_password`, taking the
seed as a `u64` parameter. Nothing outside its own tests calls it yet, so no
user has been given a password from it — but it is public API in a credentials
crate, and the next caller to wire it up will pass it a clock or a counter.
It should draw from the kernel and fail closed like the other two before it
acquires a caller. Tracked as `C-GUI-CREDENTIALS-GENERATE-PASSWORD-TAKES-A-SEED`.
**Fixed 2026-08-18 in `7b9a1bace`, still without a caller** — see the closing
section of `C-THE-SHARED-RNG-CRATE-WAS-ITSELF-DUPLICATED`.

`gui/credentials` also carries a comment saying "there is no source of
unpredictable numbers in userspace yet", which is why
`KEY_DERIVATION_SALT` is a constant every install shares. That statement is
now out of date — `guitk::rng::SystemRandom` reaches the kernel CSPRNG through
the posix `getrandom` symbol, which is the route this batch used. The
per-vault salt in
`C-THE-MASTER-PASSWORD-IS-HASHED-ONCE-WITH-A-SALT-EVERY-INSTALL-SHARES` is
therefore unblocked. **Both comments are deleted and the salt is fixed as of
`6771f575e`.**
