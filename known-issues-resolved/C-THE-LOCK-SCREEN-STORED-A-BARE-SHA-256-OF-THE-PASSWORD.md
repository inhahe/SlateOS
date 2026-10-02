## C-THE-LOCK-SCREEN-STORED-A-BARE-SHA-256-OF-THE-PASSWORD (lane C, 2026-08-18) — FIXED

**In short:** the program that guards a locked session checked the typed
password by hashing it once and comparing the result to a stored number. No
salt, no repetition. That makes the stored value the *same* number on every
machine for any given password, so one precomputed table cracks every
SlateOS user at once, and a graphics card can try billions of guesses a
second against it. It now uses a shared derivation (`pwkdf`) with a random
per-install salt and 100,000 rounds, and the same crate is what
`gui/credentials` will use, so there is exactly one answer to "how is a
password stored here".

### What the code did

`apps/lockscreen/src/main.rs`'s `PasswordValidator` held a `[u8; 32]` that
was `sha256(password.as_bytes())` and nothing else, compared with a
constant-time equality. Two properties follow immediately:

- **No salt.** `sha256("hunter2")` is a fixed 32-byte value known to anyone
  who has ever computed it. A stolen store needs no cracking at all for any
  password that appears in a wordlist — it needs a lookup. It also leaks
  equality: two users with the same password have byte-identical entries, so
  an administrator can see who shares a password with whom without breaking
  either.
- **No stretching.** One SHA-256 compression is the cheapest possible guess.
  The cost of the defence should be tuned to be *inconvenient* — the point of
  a work factor is that the attacker pays it billions of times and the user
  pays it once, on a keystroke they already expected to wait for.

### Why it survived review

The crate had five tests of the hash: FIPS vectors for `sha256` and a
constant-time-comparison test. They all passed, and they were all testing
`sha2`, which is correct and already owns those exact vectors
(`sha2/src/lib.rs:489,497,645`). Nothing tested the property that actually
mattered — *that the stored value is not a bare hash of the password* — so a
suite of 60-odd tests was fully green on a store with no salt in it. The five
duplicated tests are now deleted and replaced by three that test what
lockscreen is responsible for: `the_stored_value_is_not_a_bare_hash_of_the_password`,
`two_installs_store_different_values_for_one_password`, and a round trip
through the stored salt and verifier.

### The fix

A new crate, `pwkdf`, holds the derivation once: a 16-byte salt drawn from
the kernel entropy source, 100,000 rounds folding password and salt in every
round, and a domain-separated verifier so that the value stored on disk is
not itself usable as a key. `PasswordValidator::enrol` returns
`Result<_, KdfError>` and **refuses** when entropy is unavailable, per
design-decisions §465 — a secret is the tier that refuses rather than falling
back.

The reason it is a crate rather than a function in `apps/lockscreen` is that
`gui/credentials` had *independently* grown the same derivation. Two
independently-correct derivations are still *incompatible*, and on the day
someone wires the lock screen to the credential store, the cheap way to
reconcile them is to weaken the store to match the screen. Extracting the
crate while neither is wired to the other costs nothing; doing it afterwards
costs an argument about a migration.

`userspace/cryptsetup` deliberately keeps its own PBKDF2 and does **not**
use `pwkdf`: LUKS specifies the derivation on disk, so that one is a format
obligation, not a duplicate.

### Still to do

- ~~**`gui/credentials` is not yet on `pwkdf`.**~~ **Done the same day.** Its
  `SALT_LEN`, `KdfParams`, `stretch` and the body of `DEFAULT_KDF_ROUNDS` are
  gone; `derive_session_key` and `verifier_for` are now three-line adapters
  over `pwkdf::derive_key` and `pwkdf::verifier_for` (net −150 lines). What
  it keeps is `VERIFIER_DOMAIN` — the label that stops a vault verifier from
  being replayable against a lock screen — and a `From<pwkdf::KdfError>`
  written as an exhaustive match rather than a `map_err`, so that a second
  `pwkdf` failure mode would stop compiling here instead of being silently
  folded into "no entropy". Two new tests pin the migration: the stored
  format is still `sha256(key ‖ "slateos-credential-verifier")`, and a vault
  verifier differs from a lock-screen one derived from the same key. Both
  verified by flipping the domain constant. 83 tests, clippy clean.
- **`pwkdf::stretch` is PBKDF2's *shape*, not PBKDF2**, and the module doc
  says so. Whether to keep a hand-written construction or port a vetted one
  is open question **C-Q5**, which is the operator's call; the crate is
  written so that swapping the interior is a change to one function.

### Two unrelated production risks fixed in the same file

- **`LockoutTimer::tick` looped instead of dividing.** `while ms >= 1000 { ms
  -= 1000; secs -= 1 }` needed two guards to keep its subtractions from
  underflowing, neither visible to the compiler, and it spun once per elapsed
  second on a single large `dt_ms` — the frame after a suspend, or a debugger
  pause. Replaced with `%` and `saturating_sub`.
- **`active_user` had a "defensive" fallback that panicked in exactly the
  case it defended against**: `self.users.get(i).unwrap_or_else(|| &self.users[0])`
  indexes an empty vector precisely when `get` returned `None` because the
  vector was empty. The invariant ("there is always a user") was real but
  conventional, established in one constructor and relied on by nine callers.
  It is now structural: `UserList` splits the first user out of the `Vec`, so
  there is no empty state to defend against and `first()` is total.

**Discovered:** 2026-08-18, while converting `apps/lockscreen` off its
hand-rolled hashing.
