//! The hashing behind `posix`'s `crypt`: SHA-256 and SHA-512 ([`sha2`]),
//! MD5 ([`md5`]), the SHA-crypt and md5crypt rounds ([`shacrypt`],
//! [`md5crypt`]), yescrypt's and scrypt's KDF ([`yescrypt`]), and bcrypt's
//! Eksblowfish ([`bcrypt`]).
//!
//! ## Why a crate and not modules of `posix`
//!
//! Speed.  The libc is compiled for size -- `opt-level = "s"`, the
//! workspace's release setting, because every program carries it
//! (design-decisions §100) -- and hash loops are what that costs most:
//! `crypt`'s yescrypt took 3.7 times libxcrypt's time for the same hash, and
//! its SHA-512 crypt twice
//! (`known-issues-resolved/D-CRYPT-HASHES-RUN-AT-THE-LIBCS-SIZE-OPTIMISATION.md`).
//! A crate can have an opt-level of its own, as the kernel and the shell
//! do; a module cannot.  This one is compiled at 3 (the root `Cargo.toml`'s
//! `[profile.release.package.pwhash]`), and holds only the hashing, so the
//! rest of the libc is as small as it was.
//!
//! What stays in `posix`: the C ABI (`crypt`, `crypt_r`), the settings --
//! their parsing and their base-64 -- and memory.  This crate allocates
//! nothing; `posix` gives yescrypt its work area from its own `calloc`.
//!
//! **Nothing on a hot path here is `#[inline]` or generic across the crate
//! boundary.**  Either is compiled in its caller's crate, at its caller's
//! opt-level, which would give back what this crate exists for: so
//! [`shacrypt`] offers `sha256` and `sha512`, not its generic core.

#![no_std]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation, // the hashes take words apart into their halves and bytes on purpose
    clippy::many_single_char_names,   // a..h, w, x, j: the specifications' own names
    clippy::similar_names,            // the same
    clippy::module_name_repetitions,  // `yescrypt::YESCRYPT_RW`: libxcrypt's names
    clippy::doc_markdown,             // FIPS 180-4, RFC 1321 and libxcrypt identifiers throughout
)]
// Tests may panic on what they did not expect, and index what they built.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
    )
)]

#[cfg(test)]
extern crate std;

pub mod bcrypt;
pub mod md5;
pub mod md5crypt;
pub mod sha2;
pub mod shacrypt;
pub mod yescrypt;
