//! Authenticated encryption and password-key derivation, for everything in
//! SlateOS that keeps a secret at rest.
//!
//! # Why this crate exists
//!
//! `design-decisions.md` §539 -- the operator's answer to C-Q5 -- settled
//! that cryptographic *primitives* are ported from implementations other
//! people have spent years attacking, and that only the format and the
//! plumbing around them are this project's own. The reason is the one failure
//! this project's testing cannot see: code that computes the right answer and
//! still leaks the secret through how long it took. No test we can write
//! notices that; the people who wrote these crates designed against it.
//!
//! The primitives are the crates beside this one in `rustcrypto/`, copied as
//! RustCrypto published them to crates.io -- `rustcrypto/README.md` records
//! each one's version, checksum and upstream revision. This crate is the
//! plumbing: one plain API that every caller uses, so the password manager,
//! the system keyring and the disk-encryption key derivation share one cipher
//! and one password hash instead of growing three.
//!
//! # What it offers
//!
//! - [`encrypt`] and [`decrypt`]: **XChaCha20-Poly1305**, an authenticated
//!   cipher. A ciphertext that has been altered in any bit -- or whose
//!   associated data or nonce has, or that is opened under another key --
//!   does not decrypt: [`decrypt`] answers [`Error::Unauthentic`] and hands
//!   back nothing.
//! - [`derive_key`]: **Argon2id** (RFC 9106), the memory-hard password hash,
//!   turning a password and a salt into a key with explicit [`KdfParams`], so
//!   that guessing a password costs an attacker memory as well as time.
//!
//! # The nonce is the caller's
//!
//! XChaCha20-Poly1305's nonce is 24 bytes: large enough to draw at random for
//! every message, with no realistic chance two ever collide. This crate does
//! not draw it, because it has no source of randomness of its own -- it builds
//! for the kernel as well as for programs. The caller draws [`NONCE_LEN`]
//! bytes from the system's secure random source for **every** encryption and
//! stores them beside the ciphertext. Encrypting two messages under one key
//! with one nonce gives both away; nothing here can detect that it happened.
//!
//! # What this is not
//!
//! - **Not a file format.** A caller's format keeps the salt, the
//!   [`KdfParams`] and the nonce beside the ciphertext; this crate says what
//!   they are and how long, not where they go.
//! - **Not zeroising.** Keys are plain arrays. A caller that keeps one for
//!   long should overwrite it when done; the vendored crates' `zeroize`
//!   features are off because the crate that provides it is not vendored.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};

/// Bytes in a key.
pub const KEY_LEN: usize = 32;
/// Bytes in an XChaCha20-Poly1305 nonce.
pub const NONCE_LEN: usize = 24;
/// Bytes the authentication tag adds to every ciphertext.
pub const TAG_LEN: usize = 16;
/// Bytes of salt a caller should draw for a new key: RFC 9106 §3.1's
/// recommendation for password hashing.
pub const SALT_LEN: usize = 16;
/// The shortest salt [`derive_key`] accepts -- Argon2's own minimum, so a
/// salt some other program wrote can still be read.
pub const MIN_SALT_LEN: usize = argon2::MIN_SALT_LEN;

/// A 256-bit key.
pub type Key = [u8; KEY_LEN];
/// An XChaCha20-Poly1305 nonce.
pub type Nonce = [u8; NONCE_LEN];

/// Why something here did not work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The sealed bytes are not what was sealed under this key with this nonce
    /// and associated data: altered, cut short, or opened with the wrong key.
    /// One answer for all of them on purpose -- telling them apart would tell
    /// an attacker which.
    Unauthentic,
    /// A message past what XChaCha20-Poly1305 can encrypt under one nonce
    /// (256 GiB).
    TooLong,
    /// Argon2 does not accept these parameters: too little memory for the
    /// lanes, no passes, no lanes, or too many.
    BadParams,
    /// The salt is shorter than [`MIN_SALT_LEN`] or longer than Argon2 takes.
    BadSalt,
    /// The password is longer than Argon2 takes (4 GiB).
    PasswordTooLong,
    /// There is not enough memory for the parameters asked.
    OutOfMemory,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Unauthentic => {
                "the data is not what was sealed: altered, cut short, or the wrong key"
            }
            Self::TooLong => "the message is too long to encrypt under one nonce",
            Self::BadParams => "the key-derivation parameters are not ones Argon2 accepts",
            Self::BadSalt => "the salt is too short or too long",
            Self::PasswordTooLong => "the password is too long",
            Self::OutOfMemory => "there is not enough memory for the key derivation asked",
        })
    }
}

impl core::error::Error for Error {}

/// Encrypt `plaintext` under `key` and `nonce`, binding `aad` to it.
///
/// Returns the ciphertext with the [`TAG_LEN`]-byte tag after it -- always
/// `plaintext.len() + TAG_LEN` bytes. `aad` ("associated data") is not
/// encrypted and not included in the result; it is authenticated, so
/// [`decrypt`] refuses the ciphertext unless handed the same `aad`. A file
/// format puts its header there, so a header edited to point at other
/// parameters is caught.
///
/// **`nonce` must never be used twice with one key.** See the crate docs.
///
/// # Errors
///
/// [`Error::TooLong`] for a message past 256 GiB.
pub fn encrypt(key: &Key, nonce: &Nonce, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    XChaCha20Poly1305::new(&Array(*key))
        .encrypt(
            &Array(*nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::TooLong)
}

/// Decrypt what [`encrypt`] produced, checking it first.
///
/// # Errors
///
/// [`Error::Unauthentic`] if `sealed`, `aad` or `nonce` differ in any bit
/// from what was encrypted under `key`, or `sealed` is shorter than a tag.
/// Nothing of the plaintext is returned then, not even a part.
pub fn decrypt(key: &Key, nonce: &Nonce, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, Error> {
    XChaCha20Poly1305::new(&Array(*key))
        .decrypt(&Array(*nonce), Payload { msg: sealed, aad })
        .map_err(|_| Error::Unauthentic)
}

/// How much an Argon2id derivation costs: the memory, the passes over it and
/// the lanes it is split into.
///
/// Every value here changes the key that comes out, so a format keeps the
/// parameters it derived with beside the salt, and reads them back rather
/// than assuming today's [`RECOMMENDED`](Self::RECOMMENDED).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory, in KiB. At least eight per lane.
    pub memory_kib: u32,
    /// Passes over the memory. At least one.
    pub iterations: u32,
    /// Lanes ("parallelism"). Computed one after another here -- the result
    /// depends on the number, not on whether they ran at once.
    pub lanes: u32,
}

impl KdfParams {
    /// RFC 9106 §4's second recommended setting -- the one for when the
    /// first's 2 GiB is too much: 64 MiB, three passes, four lanes.
    pub const RECOMMENDED: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        lanes: 4,
    };
}

/// Derive a [`KEY_LEN`]-byte key from `password` and `salt` with Argon2id
/// (version 0x13, RFC 9106).
///
/// The salt should be [`SALT_LEN`] fresh random bytes per key, stored beside
/// what the key protects; it is not secret.
///
/// # Errors
///
/// [`Error::BadSalt`], [`Error::BadParams`], [`Error::PasswordTooLong`], or
/// [`Error::OutOfMemory`] when the memory the parameters ask for cannot be
/// had.
pub fn derive_key(password: &[u8], salt: &[u8], params: KdfParams) -> Result<Key, Error> {
    let argon2_params = argon2::Params::new(
        params.memory_kib,
        params.iterations,
        params.lanes,
        Some(KEY_LEN),
    )
    .map_err(|_| Error::BadParams)?;
    let argon2 = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2_params,
    );
    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| match e {
            argon2::Error::SaltTooShort | argon2::Error::SaltTooLong => Error::BadSalt,
            argon2::Error::PwdTooLong => Error::PasswordTooLong,
            argon2::Error::OutOfMemory => Error::OutOfMemory,
            _ => Error::BadParams,
        })?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    const KEY: Key = [7; KEY_LEN];
    const NONCE: Nonce = [9; NONCE_LEN];

    #[test]
    fn what_is_sealed_opens_with_the_same_key_nonce_and_data() {
        let sealed = encrypt(&KEY, &NONCE, b"header", b"secret").unwrap();
        assert_eq!(sealed.len(), b"secret".len() + TAG_LEN);
        assert_ne!(&sealed[..6], b"secret", "the plaintext is in the clear");
        assert_eq!(
            decrypt(&KEY, &NONCE, b"header", &sealed).unwrap(),
            b"secret"
        );
    }

    #[test]
    fn nothing_opens_that_was_changed_in_any_way() {
        let sealed = encrypt(&KEY, &NONCE, b"header", b"secret").unwrap();
        for i in 0..sealed.len() {
            let mut bent = sealed.clone();
            bent[i] ^= 1;
            assert_eq!(
                decrypt(&KEY, &NONCE, b"header", &bent),
                Err(Error::Unauthentic),
                "a flipped bit at {i} opened"
            );
        }
        assert_eq!(
            decrypt(&KEY, &NONCE, b"Header", &sealed),
            Err(Error::Unauthentic)
        );
        let mut other_nonce = NONCE;
        other_nonce[23] ^= 1;
        assert_eq!(
            decrypt(&KEY, &other_nonce, b"header", &sealed),
            Err(Error::Unauthentic)
        );
        let mut other_key = KEY;
        other_key[0] ^= 1;
        assert_eq!(
            decrypt(&other_key, &NONCE, b"header", &sealed),
            Err(Error::Unauthentic)
        );
        assert_eq!(
            decrypt(&KEY, &NONCE, b"header", &sealed[..sealed.len() - 1]),
            Err(Error::Unauthentic)
        );
        assert_eq!(
            decrypt(&KEY, &NONCE, b"header", &[]),
            Err(Error::Unauthentic)
        );
    }

    #[test]
    fn an_empty_message_still_carries_a_tag() {
        let sealed = encrypt(&KEY, &NONCE, b"", b"").unwrap();
        assert_eq!(sealed.len(), TAG_LEN);
        assert_eq!(decrypt(&KEY, &NONCE, b"", &sealed).unwrap(), b"");
    }

    const CHEAP: KdfParams = KdfParams {
        memory_kib: 64,
        iterations: 1,
        lanes: 1,
    };

    #[test]
    fn a_key_depends_on_the_password_the_salt_and_every_parameter() {
        let salt = [1u8; SALT_LEN];
        let key = derive_key(b"password", &salt, CHEAP).unwrap();
        assert_eq!(key, derive_key(b"password", &salt, CHEAP).unwrap());
        assert_ne!(key, derive_key(b"passworD", &salt, CHEAP).unwrap());
        assert_ne!(
            key,
            derive_key(b"password", &[2u8; SALT_LEN], CHEAP).unwrap()
        );
        for other in [
            KdfParams {
                memory_kib: 128,
                ..CHEAP
            },
            KdfParams {
                iterations: 2,
                ..CHEAP
            },
            KdfParams { lanes: 2, ..CHEAP },
        ] {
            assert_ne!(
                key,
                derive_key(b"password", &salt, other).unwrap(),
                "{other:?}"
            );
        }
    }

    #[test]
    fn parameters_argon2_refuses_are_refused() {
        let salt = [1u8; SALT_LEN];
        for bad in [
            KdfParams {
                memory_kib: 7,
                ..CHEAP
            },
            KdfParams {
                iterations: 0,
                ..CHEAP
            },
            KdfParams { lanes: 0, ..CHEAP },
            // Eight KiB a lane at the least.
            KdfParams {
                memory_kib: 64,
                iterations: 1,
                lanes: 9,
            },
        ] {
            assert_eq!(
                derive_key(b"pw", &salt, bad),
                Err(Error::BadParams),
                "{bad:?}"
            );
        }
        assert_eq!(
            derive_key(b"pw", &[1u8; MIN_SALT_LEN - 1], CHEAP),
            Err(Error::BadSalt)
        );
        assert!(derive_key(b"pw", &[1u8; MIN_SALT_LEN], CHEAP).is_ok());
    }

    #[test]
    fn the_recommended_setting_is_rfc_9106s_second() {
        assert_eq!(
            (
                KdfParams::RECOMMENDED.memory_kib,
                KdfParams::RECOMMENDED.iterations,
                KdfParams::RECOMMENDED.lanes
            ),
            (65_536, 3, 4)
        );
    }
}
