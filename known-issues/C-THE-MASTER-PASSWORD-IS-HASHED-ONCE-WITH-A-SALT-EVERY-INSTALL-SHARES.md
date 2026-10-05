## C-THE-MASTER-PASSWORD-IS-HASHED-ONCE-WITH-A-SALT-EVERY-INSTALL-SHARES (lane C, 2026-08-17)

**In short:** the credential manager turns the user's master password into a
key with a single pass of SHA-256, mixed with a fixed word that is compiled
into the program and is therefore identical on every SlateOS machine. Both
halves of that are wrong in the same direction: a single pass means an
attacker can try billions of candidate passwords per second on a GPU, and a
shared fixed word means one precomputed table cracks every user in the world
rather than having to be rebuilt per user.

**Where:** `gui/credentials/src/main.rs`, `derive_session_key` and the
`KEY_DERIVATION_SALT` constant.

```rust
const KEY_DERIVATION_SALT: &str = "slateos_credential_salt";

fn derive_session_key(master_password: &str) -> [u8; 32] {
    let mut input = master_password.as_bytes().to_vec();
    input.extend_from_slice(KEY_DERIVATION_SALT.as_bytes());
    sha256(&input)
}
```

A password-to-key function is supposed to be *deliberately slow* (key
stretching) and *per-user distinct* (a random salt stored with the vault).
This is neither. `apps/lockscreen` derives its stored hash the same way and
has the same problem.

**Proper fix:** iterate. Even a plain `for _ in 0..N { h = sha256(h ‖ pw) }`
with N in the hundreds of thousands is an enormous improvement and needs
nothing new — that is essentially PBKDF2's structure. The per-vault random
salt needs a randomness source the crate does not have (see below), but a
salt that merely varies per *installation* already defeats a shared table, so
it should not wait for one. The end state wants a memory-hard KDF (scrypt or
Argon2id), which is gated on the same open question as the cipher.

**Half resolved 2026-08-17 — the stretching. The salt is still shared.**

`derive_session_key(password, rounds)` now runs `rounds` iterations of
`SHA-256(acc ‖ password ‖ salt)`. The password and salt are folded back in on
*every* round rather than the accumulator merely being rehashed: a chain of
the form `h = SHA-256(h)` is the same chain for every password, so an attacker
could walk it once and test candidates against any point on it. Mixing the
password in each round is what forces the full cost per guess.

The count was picked by measurement, not by taste: one SHA-256 of a ~70-byte
input on this machine is **1.278 µs release** (8.564 µs debug), so
`DEFAULT_KDF_ROUNDS = 100_000` is ~130 ms per unlock.

**A second defect had to be fixed for the first fix to be worth anything.**
The store kept `master_password_hash = SHA-256(password)` to check unlock
attempts against. An attacker holding the vault would have tested guesses
against *that* — one SHA-256 each — and never called `derive_session_key` at
all, so the stretching would have been decorative. The field is now
`master_password_verifier`, `SHA-256(stretched key ‖ label)`, so a guess costs
the full derivation. `IdentityVerifier::verify` had the identical bug on the
re-verification path and is routed through the same value.
Both comparisons use `constant_time_eq` rather than `==`, which returns early
on the first differing byte and so leaks how many leading bytes matched.

**Why the round count is stored rather than compiled in.** `kdf_rounds` is a
field of `CredentialStore` (`with_kdf_rounds`, default `DEFAULT_KDF_ROUNDS`).
The right number rises with hardware, and a vault written under the old number
must keep opening after the default moves — it can only do that if it
remembers what the old number was. Every real password-hashing format records
its cost parameters beside the hash for this reason. **This too must
round-trip through any persistence layer.** It also lets the test module run
at 4 rounds instead of putting the suite in the minutes;
`default_kdf_rounds_are_usable` exercises the shipped number once.

**Still open:** the salt. `KEY_DERIVATION_SALT` is still a compile-time
constant shared by every install, because a per-vault salt needs entropy that
userspace cannot obtain — see the next entry and
`requests/c-a-userspace-entropy-syscall.md`. `apps/lockscreen` still derives
its stored hash with a single SHA-256 pass and has not been touched.

### The salt half is fixed too, 2026-08-18 (`6771f575e`)

`KEY_DERIVATION_SALT` is **gone**. Every vault now draws 16 bytes from the
kernel CSPRNG and stores them beside the verifier. The paragraph above says
the salt had to wait for entropy userspace could not obtain; that premise was
false when it was written — see the next entry's correction.

Salt and cost travel together in one `KdfParams { salt: [u8; 16], rounds:
u32 }`, for the same reason the round count was stored in the first place:
both are properties of the stored verifier, and a vault that loses either can
never be opened again. `KdfParams` is what `CredentialStore` holds now
(`with_kdf_params`, `kdf_params()`), what `derive_session_key` takes, and what
`IdentityVerifier::verify` takes. **All of it must round-trip through any
persistence layer** — the same warning the round count already carried, now
covering the salt as well, and a vault whose salt is lost is unopenable rather
than merely slow.

`KdfParams::fresh` **refuses** (`CredentialError::EntropyUnavailable`) when
the kernel cannot be reached, rather than falling back to a clock. A salt is
chosen once and then lives as long as the vault does, so a predictable one is
a permanent weakness that nothing later will notice or repair; refusing to
create the vault is the recoverable outcome. Recorded as design-decisions
§464.

The salt is re-drawn on *every* `set_master_password`, including a change of
an existing password — the old key is derived under the old parameters to
check the old password and to re-encrypt, then both change together. A change
that is rejected leaves the old salt in place
(`a_rejected_password_change_leaves_the_salt_alone`).

Six new tests pin it: two vaults do not share a salt, the salt changes the
key, a vault reloaded with the wrong salt does not open, changing the password
changes the salt, the rejected-change case, and the refusal on the
`handle_request` IPC surface. Tests reach a `#[cfg(test)]`
`set_master_password_keeping_salt` seam, because a host build has no kernel
and would otherwise be unable to construct a store at all.

**Still untouched:** `apps/lockscreen` still derives its stored hash with a
single SHA-256 pass and a shared constant. It is a separate crate with the
same defect and now has no excuse either.
