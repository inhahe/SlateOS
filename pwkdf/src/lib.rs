//! Turning a password into a key, once, for everything that checks a password.
//!
//! A password has perhaps 30–40 bits of real entropy, which is nothing. Two
//! things stand between that and an attacker holding a copy of the stored
//! verifier, and this crate exists because both of them are easy to get wrong
//! in ways that look fine:
//!
//! - **A per-target salt**, so that an attacker who precomputes a table of
//!   hashed passwords must redo the work per victim rather than once for the
//!   world. A *constant* salt — or none — means one table opens every account
//!   on every machine.
//! - **Key stretching**, so each guess costs the attacker time. A single
//!   SHA-256 pass lets commodity GPU hardware try billions of candidates per
//!   second; at [`DEFAULT_ROUNDS`] the same hardware manages tens of thousands.
//!
//! # Why this is a crate rather than a function in two places
//!
//! It was a function in two places, and they disagreed. `gui/credentials` was
//! fixed to salt and stretch (design-decisions §464) while `apps/lockscreen`
//! went on storing a bare `SHA-256(password)` with no salt at all. Those two
//! are not merely unequal in strength — they are *incompatible*, because a
//! verifier produced by one can never be checked by the other.
//!
//! That is the dangerous part, and it is why this was worth a crate. The two
//! components are not wired together yet; the lock screen's own comment says
//! the hash "would come from the credential store via IPC". On the day someone
//! connects them they will find the formats disagree, and the cheap way to
//! make them agree is to weaken the store to match the lock screen — undoing
//! §464 in a commit that reads like plumbing. Sharing the derivation *before*
//! that wiring exists is the only moment when agreement is free.
//!
//! # What this crate is not
//!
//! [`stretch`] is iterated SHA-256 in PBKDF2's *shape*, not PBKDF2 itself: it
//! is not HMAC-based and is not interchangeable with a standard
//! implementation. That is a deliberate inheritance from the code it was
//! lifted out of, not an endorsement — whether SlateOS writes its own
//! primitives or ports vetted ones is `open-questions.md` → **C-Q5**, which is
//! the operator's call and is not settled here. Consolidating first is what
//! makes that decision cheap to act on: when it is answered there is one
//! function to replace instead of one per caller.
//!
//! `userspace/cryptsetup` deliberately does **not** use this crate. It
//! implements real PBKDF2-SHA256 because the LUKS on-disk format requires
//! exactly that; its duplication is a format obligation, not a copy.
//!
//! # Example
//!
//! ```
//! use pwkdf::{KdfParams, PasswordVerifier, DEFAULT_ROUNDS};
//!
//! // A test names its salt so assertions are stable; a real caller uses
//! // `KdfParams::fresh`, which draws one from the kernel.
//! let params = KdfParams::new([7u8; pwkdf::SALT_LEN], 64);
//! let stored = PasswordVerifier::create(b"correct horse", params, b"example-domain");
//!
//! assert!(stored.check(b"correct horse"));
//! assert!(!stored.check(b"Correct horse"));
//! ```

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use randrange::{RandomSource, SecretSource, SystemRandom};
use sha2::{eq_constant_time, sha256};

/// How many bytes of salt a fresh derivation gets.
///
/// Sixteen is the usual figure (PBKDF2's RFC 8018 calls for at least eight;
/// Argon2's reference implementation defaults to sixteen). The requirement is
/// only that two targets never collide by chance, which 128 bits settles.
pub const SALT_LEN: usize = 16;

/// How many iterations turn a password into a key, for a verifier created
/// today.
///
/// The number is chosen the standard way — by measurement, so that one
/// derivation costs enough to hurt an attacker and not enough for a user to
/// notice. On the development machine one SHA-256 of a ~70-byte input takes
/// 1.28 µs in a release build, so 100 000 rounds is ~130 ms per unlock.
///
/// It is a *default* rather than a constant of the format because the right
/// number rises with hardware, and a verifier written under the old number
/// must keep verifying after the default moves. The cost is a property of the
/// stored verifier, not of the code that reads it, which is why [`KdfParams`]
/// carries it rather than this constant being consulted at check time.
pub const DEFAULT_ROUNDS: u32 = 100_000;

/// Why a fresh derivation could not be set up.
///
/// One variant, because there is one way to fail: the kernel could not supply
/// a salt. Deliberately not convertible into "use a fallback salt" — see
/// [`KdfParams::fresh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KdfError {
    /// The kernel CSPRNG could not be reached, so no salt could be drawn.
    ///
    /// Callers should propagate this. As of 2026-08-18 the kernel blocks until
    /// its entropy pool has 256 credited bits and then *fails* rather than
    /// returning uncredited bytes, so this means "no entropy", never "weak
    /// entropy" (see `requests/a-c-getrandom-is-available.md`).
    EntropyUnavailable,
}

/// Everything a derivation is pinned to: its salt and its cost.
///
/// These travel together because they are used together and must be *stored*
/// together. Every real password-hashing format writes its salt and its cost
/// parameters beside the hash for the same reason: both are properties of the
/// stored verifier, not of the code that reads it, and a verifier that has
/// lost either one can never be checked again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// This target's salt. **A persistence layer must round-trip it.** A
    /// verifier reloaded with a different salt derives a different key from
    /// the same password, which is indistinguishable from a wrong password.
    salt: [u8; SALT_LEN],
    /// The iteration count this verifier was derived at. **A persistence layer
    /// must round-trip this too** — see [`DEFAULT_ROUNDS`] for why something
    /// written under an older, cheaper number still has to verify.
    rounds: u32,
}

impl KdfParams {
    /// Parameters for a *new* target: a fresh salt drawn from the kernel.
    ///
    /// Fails with [`KdfError::EntropyUnavailable`] rather than falling back to
    /// a clock, a counter, or a constant. A salt is chosen once and then lives
    /// as long as the account does, so a predictable one is a permanent
    /// weakness that nothing later will notice or repair; refusing to create
    /// the account is the recoverable outcome.
    ///
    /// This is the *secret* tier of design-decisions §465, which is why it
    /// refuses where `randrange::seeded_from_system` would fall back.
    ///
    /// # Errors
    ///
    /// [`KdfError::EntropyUnavailable`] if the kernel CSPRNG cannot be reached,
    /// or fails part-way through the draw.
    pub fn fresh(rounds: u32) -> Result<Self, KdfError> {
        let mut source = SystemRandom::open().map_err(|_| KdfError::EntropyUnavailable)?;
        let salt = source
            .secret(|rng| {
                let mut salt = [0u8; SALT_LEN];
                for chunk in salt.chunks_mut(8) {
                    let word = rng.next_u64().to_le_bytes();
                    // `chunk` is at most 8 bytes, so the zip stops at its end.
                    for (slot, byte) in chunk.iter_mut().zip(word) {
                        *slot = byte;
                    }
                }
                salt
            })
            .ok_or(KdfError::EntropyUnavailable)?;
        Ok(Self { salt, rounds })
    }

    /// Parameters whose salt was chosen earlier — reconstructing something
    /// stored, or a test naming a salt so its assertions are stable.
    ///
    /// This is **not** how a new target gets its salt. Use [`Self::fresh`].
    #[must_use]
    pub const fn new(salt: [u8; SALT_LEN], rounds: u32) -> Self {
        Self { salt, rounds }
    }

    /// This target's salt, for a persistence layer to write down.
    #[must_use]
    pub const fn salt(&self) -> [u8; SALT_LEN] {
        self.salt
    }

    /// The iteration count this derivation costs.
    #[must_use]
    pub const fn rounds(&self) -> u32 {
        self.rounds
    }
}

/// Iterated SHA-256 over `password` and `salt` — PBKDF2's shape.
///
/// The password and salt are folded back in on *every* round rather than the
/// accumulator merely being hashed with itself. That matters: a chain of the
/// form `h = SHA-256(h)` is the same chain for every password, so an attacker
/// could walk it once and then test candidates against any point on it. Mixing
/// the password in each round makes the whole chain password-specific, which
/// is what forces the attacker to pay the full cost per guess.
#[must_use]
pub fn stretch(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut buf =
        Vec::with_capacity(32usize.saturating_add(password.len()).saturating_add(salt.len()));
    buf.extend_from_slice(password);
    buf.extend_from_slice(salt);
    let mut acc = sha256(&buf);

    for _ in 0..rounds {
        buf.clear();
        buf.extend_from_slice(&acc);
        buf.extend_from_slice(password);
        buf.extend_from_slice(salt);
        acc = sha256(&buf);
    }
    acc
}

/// Derive a 32-byte key from `password` under `params`.
///
/// Both the salt and the cost come from `params` rather than from constants —
/// see [`KdfParams`] for why the salt must differ per target, and
/// [`DEFAULT_ROUNDS`] for why the cost must be stored rather than assumed.
#[must_use]
pub fn derive_key(password: &[u8], params: &KdfParams) -> [u8; 32] {
    stretch(password, &params.salt, params.rounds)
}

/// The value stored to check a password against, derived from the *stretched
/// key* rather than from the password.
///
/// This distinction is the whole point, and it is the one `apps/lockscreen`
/// was missing. Storing `SHA-256(password)` means an attacker holding the
/// stored value can test a candidate for the price of one SHA-256 and never
/// touch [`derive_key`] at all — so stretching would have bought exactly
/// nothing. Deriving the verifier from the key instead puts the full
/// [`KdfParams::rounds`] between the attacker and each guess.
///
/// `domain` keeps this from being a value the key itself could collide with:
/// the verifier is public (it is stored beside whatever it guards) and must
/// not be usable as, or derivable into, the key. It also separates callers —
/// a lock-screen verifier and a vault verifier for the same password are
/// different values, so neither can be replayed against the other.
#[must_use]
pub fn verifier_for(key: &[u8; 32], domain: &[u8]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32usize.saturating_add(domain.len()));
    buf.extend_from_slice(key);
    buf.extend_from_slice(domain);
    sha256(&buf)
}

/// A stored password verifier: the parameters it was derived under, the
/// domain that separates it from other callers', and the value itself.
///
/// Bundling the three is what makes this hard to misuse. They must agree for a
/// check to succeed, and a mismatch in any of them is indistinguishable from a
/// wrong password — so a caller holding them as three loose values gets a
/// silent, permanent authentication failure for a mistake the compiler could
/// have caught. In particular `domain` is stored rather than passed to
/// [`Self::check`], because a caller that passed one string at creation and a
/// different one at check would reject every correct password with no
/// indication why.
#[derive(Clone, Copy, Debug)]
pub struct PasswordVerifier {
    /// Salt and cost. Must be round-tripped by any persistence layer.
    params: KdfParams,
    /// The caller's domain-separation label, fixed at creation.
    domain: &'static [u8],
    /// `verifier_for(derive_key(password, params), domain)`.
    verifier: [u8; 32],
}

impl PasswordVerifier {
    /// Derive and store a verifier for `password`.
    ///
    /// Costs `params.rounds()` SHA-256 iterations — at [`DEFAULT_ROUNDS`],
    /// deliberately around 130 ms. Callers on a UI thread should not do this
    /// per keystroke.
    #[must_use]
    pub fn create(password: &[u8], params: KdfParams, domain: &'static [u8]) -> Self {
        let key = derive_key(password, &params);
        Self {
            params,
            domain,
            verifier: verifier_for(&key, domain),
        }
    }

    /// Reconstruct a verifier that was stored earlier.
    ///
    /// `domain` must be the same label [`Self::create`] was given, and
    /// `params` the same salt and cost, or every check fails.
    #[must_use]
    pub const fn from_parts(
        params: KdfParams,
        domain: &'static [u8],
        verifier: [u8; 32],
    ) -> Self {
        Self {
            params,
            domain,
            verifier,
        }
    }

    /// Whether `candidate` is the password this verifier was created from.
    ///
    /// The comparison is constant-time: a byte-by-byte early return leaks how
    /// many leading bytes matched, which lets an attacker who can time it
    /// recover the stored verifier one byte at a time instead of guessing all
    /// 32 at once.
    #[must_use]
    pub fn check(&self, candidate: &[u8]) -> bool {
        let key = derive_key(candidate, &self.params);
        eq_constant_time(&verifier_for(&key, self.domain), &self.verifier)
    }

    /// The parameters this verifier was derived under, for a persistence layer
    /// to write down alongside [`Self::verifier`].
    #[must_use]
    pub const fn params(&self) -> KdfParams {
        self.params
    }

    /// The stored value, for a persistence layer to write down.
    #[must_use]
    pub const fn verifier(&self) -> [u8; 32] {
        self.verifier
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// Cheap enough to run hundreds of times, and the property under test is
    /// never the *number* of rounds.
    const TEST_ROUNDS: u32 = 16;
    const DOMAIN: &[u8] = b"pwkdf-test-domain";

    fn params(salt_byte: u8) -> KdfParams {
        KdfParams::new([salt_byte; SALT_LEN], TEST_ROUNDS)
    }

    #[test]
    fn the_right_password_verifies_and_a_wrong_one_does_not() {
        let v = PasswordVerifier::create(b"correct horse", params(1), DOMAIN);
        assert!(v.check(b"correct horse"));
        assert!(!v.check(b"correct horse "));
        assert!(!v.check(b"Correct horse"));
        assert!(!v.check(b""));
    }

    #[test]
    fn the_same_password_under_two_salts_gives_two_verifiers() {
        // The entire purpose of a salt. If this fails, one precomputed table
        // opens every account on every machine -- which is what
        // `apps/lockscreen` did by having no salt at all.
        let a = PasswordVerifier::create(b"hunter2", params(1), DOMAIN);
        let b = PasswordVerifier::create(b"hunter2", params(2), DOMAIN);
        assert_ne!(a.verifier(), b.verifier());
    }

    #[test]
    fn a_verifier_reconstructed_with_the_wrong_salt_rejects_the_right_password() {
        // Stated as a test because it is the failure a persistence layer will
        // cause if it stores the verifier and forgets the salt, and because
        // the symptom -- "correct password rejected" -- does not point at it.
        let v = PasswordVerifier::create(b"hunter2", params(1), DOMAIN);
        let wrong = PasswordVerifier::from_parts(params(9), DOMAIN, v.verifier());
        assert!(!wrong.check(b"hunter2"));
    }

    #[test]
    fn a_verifier_reconstructed_with_the_wrong_round_count_rejects_the_right_password() {
        // Same hazard as the salt, and the reason `rounds` is stored in
        // `KdfParams` rather than read from `DEFAULT_ROUNDS` at check time:
        // when that default rises, everything already stored must keep working.
        let v = PasswordVerifier::create(b"hunter2", params(1), DOMAIN);
        let bumped = KdfParams::new([1u8; SALT_LEN], TEST_ROUNDS + 1);
        let wrong = PasswordVerifier::from_parts(bumped, DOMAIN, v.verifier());
        assert!(!wrong.check(b"hunter2"));
    }

    #[test]
    fn two_callers_with_different_domains_do_not_share_a_verifier() {
        // A lock-screen verifier must not be replayable against a vault, even
        // for a user who reuses one password under one salt.
        let screen = PasswordVerifier::create(b"hunter2", params(1), b"lockscreen");
        let vault = PasswordVerifier::create(b"hunter2", params(1), b"vault");
        assert_ne!(screen.verifier(), vault.verifier());
    }

    #[test]
    fn the_verifier_is_not_the_key_it_was_derived_from() {
        // The verifier is stored in the clear beside what it guards. If it
        // equalled the key, storing it would be publishing the key.
        let p = params(3);
        let key = derive_key(b"hunter2", &p);
        let v = PasswordVerifier::create(b"hunter2", p, DOMAIN);
        assert_ne!(key, v.verifier());
    }

    #[test]
    fn the_verifier_is_not_a_bare_hash_of_the_password() {
        // The defect this crate was extracted to fix. `apps/lockscreen` stored
        // exactly `sha256(password)`, so an attacker holding it could test a
        // guess for one SHA-256 and never pay the stretching at all.
        let v = PasswordVerifier::create(b"hunter2", params(1), DOMAIN);
        assert_ne!(v.verifier(), sha256(b"hunter2"));
    }

    #[test]
    fn stretching_folds_the_password_in_every_round() {
        // Guards the property the `stretch` doc explains: an accumulator that
        // merely re-hashed itself would walk a password-independent chain, so
        // `stretch(p, s, n)` would be reachable from `stretch(q, s, m)`. Here,
        // changing only the round count must not land on a value another
        // password's chain also visits -- and more basically, more rounds must
        // actually change the answer.
        let a = stretch(b"hunter2", b"salt", 4);
        let b = stretch(b"hunter2", b"salt", 5);
        let c = stretch(b"other", b"salt", 5);
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn rounds_are_iterations_not_a_one_shot() {
        // Zero rounds still hashes once (the pre-loop seed), so a caller that
        // passes 0 gets something salted but unstretched rather than the raw
        // password back. Pinned because `0` is what an uninitialised
        // `KdfParams` would carry.
        let zero = stretch(b"hunter2", b"salt", 0);
        let one = stretch(b"hunter2", b"salt", 1);
        assert_ne!(zero, one);
        assert_ne!(zero, sha256(b"hunter2"));
    }

    #[test]
    fn a_named_salt_is_reproducible() {
        // `KdfParams::new` must be deterministic, or nothing stored could ever
        // be reloaded.
        let a = PasswordVerifier::create(b"hunter2", params(5), DOMAIN);
        let b = PasswordVerifier::create(b"hunter2", params(5), DOMAIN);
        assert_eq!(a.verifier(), b.verifier());
    }

    #[test]
    fn params_round_trip_through_their_accessors() {
        // A persistence layer can only write down what it can read back out.
        let salt = [0xABu8; SALT_LEN];
        let p = KdfParams::new(salt, 1234);
        assert_eq!(p.salt(), salt);
        assert_eq!(p.rounds(), 1234);

        let v = PasswordVerifier::create(b"pw", p, DOMAIN);
        let reloaded = PasswordVerifier::from_parts(v.params(), DOMAIN, v.verifier());
        assert!(reloaded.check(b"pw"));
    }

    #[test]
    fn a_long_password_is_not_truncated() {
        // `stretch` sizes its buffer from the password length; a bug there
        // would most likely show up as two long passwords colliding.
        let a = [b'a'; 4096];
        let mut b = [b'a'; 4096];
        b[4095] = b'b';
        assert_ne!(stretch(&a, b"salt", 4), stretch(&b, b"salt", 4));
    }

    #[test]
    fn an_empty_password_and_an_empty_salt_still_derive() {
        // Neither is a sensible input, but both are reachable, and a panic
        // here would be a denial of service on the lock screen.
        let _ = stretch(b"", b"", 4);
        let v = PasswordVerifier::create(b"", KdfParams::new([0u8; SALT_LEN], 4), DOMAIN);
        assert!(v.check(b""));
        assert!(!v.check(b"x"));
    }
}
