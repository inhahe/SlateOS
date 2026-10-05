## §464 — A vault's salt is drawn from the kernel at creation, and a vault cannot be created without one

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the credential manager mixed a fixed word — the same word on
every SlateOS machine on earth — into the master password before hashing it.
That word is called a salt, and its whole job is to be *different per vault*,
so that an attacker who precomputes a giant table of password→hash pairs has
to redo the work for each victim instead of once for everybody. A shared salt
does none of that. Each vault now draws sixteen random bytes from the kernel
when it is created and stores them next to the password verifier. The cost of
this: if the kernel's random number generator cannot be reached, creating a
vault now **fails** instead of quietly using a predictable salt.

### What changed

`KEY_DERIVATION_SALT` (a `&str` constant) is deleted. In its place:

```rust
pub struct KdfParams { salt: [u8; 16], rounds: u32 }
```

- `KdfParams::fresh(rounds)` opens `SystemRandom` and draws the salt through
  `SecretSource::secret`, so the generator's health is checked on both sides
  of the draw (§462). It returns `Result<_, CredentialError::EntropyUnavailable>`.
- `CredentialStore` holds a `KdfParams` where it held a bare `kdf_rounds: u32`.
- `derive_session_key(password, &KdfParams)` and
  `IdentityVerifier::verify(.., &KdfParams)` take the pair, not the count.
- `set_master_password` re-draws the salt on every call, including a change of
  an existing password. The old password is verified and the stored secrets
  re-encrypted under the *old* parameters first; salt, rounds, verifier and
  session key then move together. A rejected change touches none of them.

### Why salt and cost travel together

They were separable in principle and are not in practice. Both are properties
of the *stored verifier* rather than of the program: a vault written under
100 000 rounds must keep opening after the default moves, and a vault written
under salt S can only ever be opened with salt S. Every real password-hashing
format — PBKDF2, bcrypt, scrypt, Argon2 — writes both beside the hash for
exactly this reason. Keeping them in one type makes it impossible to persist
one and forget the other, which is a failure mode with no recovery: a vault
that loses its salt is not slow to open, it is unopenable.

### The actual decision: refuse, or fall back to a clock?

This is the tradeoff, and it is a real one.

**Falling back** (draw the salt from the system clock, or a counter, when the
CSPRNG is unreachable) means a vault can always be created. The application
never has an unexplainable failure, and a clock-derived salt still varies
between machines, so it is *better than the shared constant* it replaces.

**Refusing** means a machine whose entropy source is broken or not yet
present cannot create a credential vault at all — a hard, visible failure in
a place users will not expect one.

Refusing wins, and the asymmetry is about *time*, not about strength:

- A salt is chosen **once** and then lives as long as the vault does — years.
  Nothing later re-examines it, and there is no natural moment at which a weak
  one gets upgraded, because upgrading it means re-deriving the key and
  rewriting every stored secret.
- A predictable salt is therefore a **permanent, silent** weakness. The vault
  works perfectly. Nothing ever reports it. The user learns about it when the
  table that opens their vault also opens everyone else's.
- A refusal is **loud and recoverable**: the user sees an error, the vault is
  not created, and creating it again once entropy is available produces a
  vault with no defect at all.

The general rule this instantiates is §462's — a generator that cannot reach
the kernel refuses rather than inventing — but the argument here is stronger
than it is for a generated password. A bad password can be regenerated in a
second by pressing the button again. A bad salt cannot be replaced without
rewriting the entire vault, and nobody will ever know to try.

### Why the games do not follow this rule

`apps/pinball` and `apps/spades` seed from `SystemRandom` and **do** fall back
to a fixed seed. That is not an inconsistency: what they lose when entropy is
down is *variety*, not confidentiality, and a pinball machine that refuses to
start because the entropy pool is empty is plainly the worse failure. The
distinguishing question is whether an adversary who predicts the value gets
anything. For a table layout, no. For a salt, the whole vault.

### The test seam, and why it is not a hole

A host build (`cargo test` on Windows) has no SlateOS kernel, so
`KdfParams::fresh` correctly fails there — which would have made
`CredentialStore` untestable. The tests reach a `#[cfg(test)]`
`set_master_password_keeping_salt` that reuses the store's existing
parameters, and construct stores with `with_kdf_params(uid, KdfParams::new(
TEST_SALT, rounds))`.

`KdfParams::new` is `pub`, which looks like a way to route around the refusal.
It is not, and cannot be removed: a persistence layer *must* be able to
reconstruct a `KdfParams` from bytes it read off disk, and that is the same
operation. What matters is that the only path that *invents* a salt is
`fresh`, and `fresh` has no fallback. Two tests pin the refusal itself — one
on the direct API, one on the `handle_request` IPC surface — so a future
change that adds a fallback fails the suite rather than passing it quietly.

### Consequences left open

- `apps/lockscreen` has the identical defect (single SHA-256 pass, shared
  constant) and is untouched. Tracked in `known-issues.md`.
- Nothing persists a `CredentialStore` yet. When something does, the salt is
  now a second field whose loss is unrecoverable; the type exists partly to
  make that hard to get wrong, but it cannot be enforced until there is a
  persistence layer to enforce it against.
