## C-THE-CREDENTIAL-MANAGER-CHECKED-ITS-MASTER-PASSWORD-WITH-DJB2 (lane C, 2026-08-18) — FIXED

**In short:** `apps/credmanager` — the password manager — checked the password
that opens the vault against a 64-bit djb2 hash. Not a weak password hash: not
a password hash at all. No salt, no cost, and narrow enough that the vault
opened for *any* string colliding with the owner's password, not just the
password itself. It now uses the shared `pwkdf` derivation, like the lock
screen and the credential service.

### What the code did

```rust
fn simple_hash(input: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in input.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    hash
}

struct Vault { master_password_hash: u64, /* … */ }

fn unlock(&mut self, password: &str, now: u64) -> bool {
    if simple_hash(password) == self.master_password_hash { /* unlocked */ }
}
```

Three independent failures, of which only the first is the obvious one:

- **No cost.** Testing a guess took two arithmetic operations per character,
  so an attacker holding the stored value ran the plausible-password space at
  memory speed.
- **No salt.** The same password produced the same 64 bits in every vault on
  every machine, so one precomputed table opened all of them — and equal
  stored values told the attacker which vaults to try it against.
- **64 bits, non-cryptographic.** djb2 collisions are constructible, not
  theoretical, and `unlock` accepted any colliding string.

### Why it survived review

Its three tests passed, and none of them could have failed for the reason the
code was wrong: `simple_hash` *was* deterministic, *did* map `"abc"` and
`"def"` apart, and *did* start from 5381. They asserted the properties the
implementation happened to have rather than the ones its caller depended on.
This is the same shape as `apps/lockscreen`'s five FIPS-vector tests over a
store that had no salt in it.

### The fix

`master_password_hash: u64` is now `master: PasswordVerifier` — the salt, the
round count and the stored value together, because all three must agree and a
persistence layer that writes down only the last has locked the owner out.
Three constructors replace one: `create` (fresh salt, fallible per §465's
*secret* tier — it refuses rather than falling back to a predictable salt),
`from_stored` (reopen), and `#[cfg(test)] for_test` (named salt, `TEST_ROUNDS
= 16`, so a 139-test suite does not spend ~130 ms per vault). `unlock` is now
`self.master.check(password.as_bytes())`.

Five tests replace the three; each was verified by reintroducing the defect it
guards against and confirming it, and only it, fails.

### Also removed: a hardcoded master password in non-test code

`AppState::new()` built its vault with `Vault::new("My Vault", "master123")` —
in a credential manager, outside `#[cfg(test)]`. It never shipped only because
`main` is still empty, which is not a property to rely on. `new` now takes an
already-opened vault; the demo password lives in `#[cfg(test)] for_test`.

### Four unrelated defects fixed in the same file

- **The strength meter counted bytes, not characters.** `password.len()`
  scored a four-character non-ASCII password as a twelve-character one —
  an overstatement, the one direction a strength meter must not err in.
- **The generator's alphabet was defined twice**: the punctuation string in
  `build_charset`, and a hand-written `26 + 26 + 10 + 30` in `pool_size`. They
  agreed by coincidence; adding one symbol would have made every generated
  password score against a pool it was not drawn from, with no failing test.
  `pool_size` is now `build_charset().len()`, and the symbol count has one
  definition shared with the strength meter.
- **`navigate_entry_list` walked the index through `i32`**, clamping with
  `len() as i32 - 1` — correct only because the empty case returns early, and
  silently wrong above `i32::MAX`. Now walked in `usize` with saturating ops.
- **12 `arithmetic_side_effects` warnings** cleared, which is all of them; the
  crate is clippy-clean at `--all-targets`.

**Discovered:** 2026-08-18, while auditing the tree for the third copy of a
password check after extracting `pwkdf` (§466).
