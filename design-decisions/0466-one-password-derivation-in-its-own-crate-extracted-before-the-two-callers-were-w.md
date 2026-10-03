## §466 — One password derivation, in its own crate, extracted before the two callers were wired together

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** two different programs in this tree each turned a password into
a stored value, and they did it *differently*. The credential vault salted and
stretched (§464); the lock screen stored a plain one-pass `SHA-256` of the
password — a value that is identical on every machine in the world for any
given password, so one precomputed table opens every account. Fixing the lock
screen meant writing the good derivation a second time, which is how the two
came to disagree in the first place. Instead the derivation moved out into a
small crate, `pwkdf`, that both now call. The timing is the decision: the two
components do not talk to each other *yet*, and that is the only moment when
making them agree costs nothing.

### The situation

`gui/credentials` had been fixed (§464) to draw a per-vault salt from the
kernel and iterate 100 000 rounds. `apps/lockscreen` stored
`sha256(password.as_bytes())` — no salt, no stretching — with a comment saying
the hash "would come from the credential store via IPC". So the two were
already destined to be connected, and were already incompatible: a verifier
produced by one can never be checked by the other.

That incompatibility, not the weakness, is what made this worth a crate. The
weakness alone could have been fixed in place, in twenty lines. But on the day
someone wires the lock screen to the vault they will find the formats
disagree, and the *cheapest* way to make them agree is to weaken the vault to
match the screen — a commit that reads like plumbing and undoes §464 without
mentioning it. There is no point in that future where the strong option is the
easy one.

### The decision

Extract `pwkdf`: `KdfParams` (salt + rounds, travelling together because they
must be *stored* together), `stretch`, `derive_key`, `verifier_for(key,
domain)`, and a `PasswordVerifier` bundle for callers that only need to check
a password rather than hold a key. `gui/credentials` keeps `derive_session_key`
and `verifier_for` as three-line adapters and loses ~150 lines;
`apps/lockscreen` uses `PasswordVerifier` directly.

The verifier is **domain-separated by a per-caller label**, so sharing a
derivation does not mean sharing a value: a lock-screen verifier and a vault
verifier for the same password under the same salt are different bytes, and
neither can be replayed against the other. The label is stored *in*
`PasswordVerifier` rather than passed to `check`, because a caller that passed
one string at creation and another at check would reject every correct
password with nothing to say why.

### Alternatives considered

- **Fix `apps/lockscreen` in place.** Smallest diff, and it is what the bug
  report literally asks for. Rejected: it produces a *third* correct-looking
  derivation, and correctness is not the property at issue — agreement is.
  Two independently-correct derivations still cannot check each other's
  verifiers.
- **Put the KDF in `sha2`.** It is already the shared hash and both callers
  depend on it. Rejected on the same grounds `randrange` was kept out of it:
  `sha2` has 15 consumers including the kernel, `kernel/build.rs`, bare-metal
  services and `userspace/sha256sum`, and is deliberately `alloc`-free. A KDF
  needs `alloc` for the stretch buffer and an entropy source for fresh salts.
  `sha256sum` has no business linking either.
- **Make `gui/credentials` a library and have the lock screen depend on it.**
  Avoids a new crate. Rejected: the lock screen would then link a vault, an
  IPC surface, a stream cipher and an auto-lock timer to hash one password,
  and the dependency runs the wrong way — a screen that must work before
  anything is unlocked should not depend on the thing that unlocks.
- **Wait until the two are actually wired together, then reconcile.**
  Rejected explicitly: that is the moment the cheap fix is the wrong one. The
  whole value of doing it now is that neither side has a stored format to
  migrate, so agreement is free.

### Consequences

- **`pwkdf::stretch` is PBKDF2's *shape*, not PBKDF2** — iterated bare
  SHA-256, not HMAC, and far weaker than a memory-hard function. That is
  inherited from the code it was lifted out of, and the module doc says so.
  Whether this tree writes its own primitives or ports vetted ones is
  `open-questions.md` → **C-Q5**, the operator's call. Consolidating first is
  what makes that answer cheap to act on: one function to replace instead of
  one per caller.
- **`userspace/cryptsetup` deliberately does not use `pwkdf`.** Its PBKDF2 is
  real HMAC-based PBKDF2 because the LUKS on-disk format specifies exactly
  that. Its duplication is a format obligation, not a copy, and folding it in
  would be a bug.
- **`KdfParams::fresh` refuses rather than falling back**, per §465: a salt is
  chosen once and lives as long as the account, so a predictable one is a
  permanent weakness nothing later repairs. Both callers propagate the error.
- **Anything that persists a verifier must round-trip the salt *and* the round
  count.** Losing either rejects the owner's own password with no diagnostic.
  Nothing persists one yet; the type exists to make that hard to get wrong,
  but it cannot be enforced until there is a persistence layer.
