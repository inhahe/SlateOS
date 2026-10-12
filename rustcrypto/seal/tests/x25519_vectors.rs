//! The published vectors for X25519 (RFC 7748), which `ssh` and `sshd`
//! agree on a session key with: the `curve25519-sha256` key exchange of
//! RFC 8731 (`requests/b-e-vendor-x25519-for-ssh.md`).
//!
//! `seal` itself does not use it. The vectors run here for the reason the
//! rest of this directory's do (`vectors.rs`): the vendored crates' own
//! suites cannot run in this workspace, so their vectors are run against
//! exactly the copies the tree builds -- `x25519-dalek` over
//! `curve25519-dalek`, on whichever backend this machine selects.
//!
//! | Vectors | What they hold |
//! |---|---|
//! | RFC 7748 §5.2 | the function on two given scalars and points, and iterated 1 and 1 000 times from the base point |
//! | RFC 7748 §6.1 | a whole Diffie-Hellman exchange: both public keys and the shared secret |
//!
//! And the three calls lane B's key exchange makes, in the shape it makes
//! them, plus the check RFC 8731 §3 requires of the shared secret (not all
//! zero, as a small-order public key would make it).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use hex_literal::hex;
use x25519_dalek::{PublicKey, StaticSecret, X25519_BASEPOINT_BYTES, x25519};

/// RFC 7748 §5.2, the two worked examples of the function itself: a scalar
/// and a u-coordinate in, a u-coordinate out. The scalar is clamped and the
/// top bit of the point masked inside the function, as the RFC says.
#[test]
fn rfc7748_section_5_2_the_function() {
    let cases = [
        (
            hex!("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
            hex!("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
            hex!("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"),
        ),
        (
            hex!("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d"),
            hex!("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493"),
            hex!("95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957"),
        ),
    ];
    for (scalar, u, out) in cases {
        assert_eq!(x25519(scalar, u), out);
    }
}

/// RFC 7748 §5.2, iterated: k and u both start as the base point (9); each
/// round, k becomes the function of the two and u the old k. After one round
/// and after a thousand, k is as given. (The million-round value is left to
/// upstream: it takes minutes in a debug build.)
#[test]
fn rfc7748_section_5_2_iterated() {
    let mut k = X25519_BASEPOINT_BYTES;
    let mut u = X25519_BASEPOINT_BYTES;
    for round in 1..=1000 {
        let next = x25519(k, u);
        u = k;
        k = next;
        if round == 1 {
            assert_eq!(
                k,
                hex!("422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079"),
                "after one round"
            );
        }
    }
    assert_eq!(
        k,
        hex!("684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51"),
        "after a thousand rounds"
    );
}

/// RFC 7748 §6.1: Alice and Bob each make a public key from a private one,
/// and each arrives at the same shared secret from the other's public key --
/// through the three calls the ssh key exchange makes.
#[test]
fn rfc7748_section_6_1_diffie_hellman() {
    let alice = StaticSecret::from(hex!(
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"
    ));
    let bob = StaticSecret::from(hex!(
        "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb"
    ));
    let alice_public = PublicKey::from(&alice);
    let bob_public = PublicKey::from(&bob);
    assert_eq!(
        alice_public.as_bytes(),
        &hex!("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
    );
    assert_eq!(
        bob_public.as_bytes(),
        &hex!("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
    );
    let shared = hex!("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
    // Each side from the other's public key as it arrives on the wire: 32
    // bytes.
    let at_alice = alice.diffie_hellman(&PublicKey::from(*bob_public.as_bytes()));
    let at_bob = bob.diffie_hellman(&PublicKey::from(*alice_public.as_bytes()));
    assert_eq!(at_alice.as_bytes(), &shared);
    assert_eq!(at_bob.as_bytes(), &shared);
    assert!(at_alice.was_contributory());
}

/// RFC 8731 §3: a shared secret of all zeros -- what a public key of small
/// order gives, whatever the private key, since a clamped scalar is a
/// multiple of eight -- must be refused. u = 0 and u = 1 (points of order two
/// and four) are two such keys; `was_contributory` is how a caller tells.
#[test]
fn a_small_order_public_key_gives_a_secret_that_is_refused() {
    let secret = StaticSecret::from(hex!(
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"
    ));
    let mut one = [0u8; 32];
    one[0] = 1;
    for u in [[0u8; 32], one] {
        let shared = secret.diffie_hellman(&PublicKey::from(u));
        assert_eq!(shared.as_bytes(), &[0u8; 32], "u = {u:?}");
        assert!(!shared.was_contributory(), "u = {u:?}");
    }
}
