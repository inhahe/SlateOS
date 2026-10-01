//! The published test vectors for every primitive `seal` stands on.
//!
//! `design-decisions.md` §539 asks that a vendored primitive ship with its
//! published vectors as tests. The vendored crates' own suites cannot run in
//! this workspace (they sit outside it, under `rustcrypto/`), so the vectors
//! are run here, against exactly the copies and the features this tree
//! builds -- and, for the cipher and the password hash, through `seal`'s own
//! API as well as the crate's, so the wrapping is checked too.
//!
//! | Primitive | Vectors |
//! |---|---|
//! | XChaCha20-Poly1305 | draft-irtf-cfrg-xchacha-03 §A.3.1; Project Wycheproof (upstream's data files, read in place) |
//! | ChaCha20-Poly1305 | RFC 8439 §2.8.2; Project Wycheproof |
//! | ChaCha20 | RFC 8439 §2.4.2 |
//! | Poly1305 | RFC 8439 §2.5.2 |
//! | Argon2id | RFC 9106 §5.3; the reference implementation's `password`/`somesalt` vectors |
//! | BLAKE2b | RFC 7693 Appendix A |

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use aead::dev::{TestVector, fail_test, pass_test};
use chacha20poly1305::{ChaCha20Poly1305, XChaCha20Poly1305};
use hex_literal::hex;

// ------------------------------------------------------------ AEAD

/// RFC 8439 §2.8.2 and draft-irtf-cfrg-xchacha-03 §A.3.1 share the key, the
/// associated data and the plaintext.
const AEAD_KEY: [u8; 32] = hex!("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
const AEAD_AAD: [u8; 12] = hex!("50515253c0c1c2c3c4c5c6c7");
const AEAD_PLAINTEXT: &[u8] = b"Ladies and Gentlemen of the class of '99: \
    If I could offer you only one tip for the future, sunscreen would be it.";

#[test]
fn xchacha20_poly1305_draft_a_3_1() {
    let nonce = hex!("404142434445464748494a4b4c4d4e4f5051525354555657");
    let ciphertext = hex!(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb"
        "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452"
        "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9"
        "21f9664c97637da9768812f615c68b13b52e"
    );
    let tag = hex!("c0875924c1c7987947deafd8780acf49");
    let mut sealed = ciphertext.to_vec();
    sealed.extend_from_slice(&tag);

    assert_eq!(
        seal::encrypt(&AEAD_KEY, &nonce, &AEAD_AAD, AEAD_PLAINTEXT).unwrap(),
        sealed
    );
    assert_eq!(
        seal::decrypt(&AEAD_KEY, &nonce, &AEAD_AAD, &sealed).unwrap(),
        AEAD_PLAINTEXT
    );
}

#[test]
fn chacha20_poly1305_rfc_8439_2_8_2() {
    use chacha20poly1305::aead::array::Array;
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};

    let nonce = hex!("070000004041424344454647");
    let ciphertext = hex!(
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6"
        "3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36"
        "92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc"
        "3ff4def08e4b7a9de576d26586cec64b6116"
    );
    let tag = hex!("1ae10b594f09e26a7e902ecbd0600691");
    let cipher = ChaCha20Poly1305::new(&Array(AEAD_KEY));
    let sealed = cipher
        .encrypt(
            &Array(nonce),
            Payload {
                msg: AEAD_PLAINTEXT,
                aad: &AEAD_AAD,
            },
        )
        .unwrap();
    assert_eq!(&sealed[..ciphertext.len()], &ciphertext[..]);
    assert_eq!(&sealed[ciphertext.len()..], &tag[..]);
}

/// Project Wycheproof's vectors for one cipher, in upstream's own data file
/// (`rustcrypto/chacha20poly1305/tests/data`), each run through `check`.
macro_rules! wycheproof {
    ($file:literal, $check:expr) => {{
        aead::dev::blobby::parse_into_structs!(
            include_bytes!(concat!("../../chacha20poly1305/tests/data/", $file, ".blb"));
            static TEST_VECTORS: &[
                TestVector { key, nonce, aad, plaintext, ciphertext }
            ];
        );
        assert!(!TEST_VECTORS.is_empty(), "{} held no vectors", $file);
        for (i, tv) in TEST_VECTORS.iter().enumerate() {
            let check: fn(&TestVector) -> Result<(), &'static str> = $check;
            if let Err(why) = check(tv) {
                panic!("{} #{i}: {why}\n{tv:?}", $file);
            }
        }
    }};
}

#[test]
fn wycheproof_chacha20_poly1305() {
    wycheproof!(
        "wycheproof_chacha20poly1305_pass",
        pass_test::<ChaCha20Poly1305>
    );
    wycheproof!(
        "wycheproof_chacha20poly1305_fail",
        fail_test::<ChaCha20Poly1305>
    );
}

#[test]
fn wycheproof_xchacha20_poly1305_through_the_crate() {
    wycheproof!(
        "wycheproof_xchacha20poly1305_pass",
        pass_test::<XChaCha20Poly1305>
    );
    wycheproof!(
        "wycheproof_xchacha20poly1305_fail",
        fail_test::<XChaCha20Poly1305>
    );
}

/// The same vectors through `seal`'s own two functions: the wrapping passes
/// the key, the nonce and the associated data through unchanged, and a
/// forgery is refused.
#[test]
fn wycheproof_xchacha20_poly1305_through_seal() {
    wycheproof!("wycheproof_xchacha20poly1305_pass", |tv| {
        let key: [u8; 32] = tv.key.try_into().map_err(|_| "key size")?;
        let nonce: [u8; 24] = tv.nonce.try_into().map_err(|_| "nonce size")?;
        let sealed = seal::encrypt(&key, &nonce, tv.aad, tv.plaintext).map_err(|_| "encrypt")?;
        if sealed != tv.ciphertext {
            return Err("encrypted differently");
        }
        let opened = seal::decrypt(&key, &nonce, tv.aad, tv.ciphertext).map_err(|_| "decrypt")?;
        if opened != tv.plaintext {
            return Err("decrypted differently");
        }
        Ok(())
    });
    wycheproof!("wycheproof_xchacha20poly1305_fail", |tv| {
        // A vector whose key or nonce is not the cipher's size cannot reach
        // `seal` at all -- its types refuse it -- which is a refusal too.
        let (Ok(key), Ok(nonce)) = (<[u8; 32]>::try_from(tv.key), <[u8; 24]>::try_from(tv.nonce))
        else {
            return Ok(());
        };
        match seal::decrypt(&key, &nonce, tv.aad, tv.ciphertext) {
            Err(seal::Error::Unauthentic) => Ok(()),
            Err(_) => Err("refused for the wrong reason"),
            Ok(_) => Err("a forgery opened"),
        }
    });
}

// ------------------------------------------------------------ ChaCha20, Poly1305

#[test]
fn chacha20_rfc_8439_2_4_2() {
    use chacha20::ChaCha20;
    use chacha20::cipher::{KeyIvInit, StreamCipher};

    let key = hex!("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
    let nonce = hex!("000000000000004a00000000");
    let expected = hex!(
        "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0b"
        "f91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d8"
        "07ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab7793736"
        "5af90bbf74a35be6b40b8eedf2785e42874d"
    );
    let mut cipher = ChaCha20::new(&key.into(), &nonce.into());
    // The vector starts at block counter 1: the first 64 bytes of the
    // keystream are not part of it.
    let mut skipped = [0u8; 64];
    cipher.apply_keystream(&mut skipped);
    let mut buf = AEAD_PLAINTEXT.to_vec();
    cipher.apply_keystream(&mut buf);
    assert_eq!(buf, expected);
}

#[test]
fn poly1305_rfc_8439_2_5_2() {
    use poly1305::Poly1305;
    use poly1305::universal_hash::KeyInit;

    let key = hex!("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
    let tag = Poly1305::new(&key.into()).compute_unpadded(b"Cryptographic Forum Research Group");
    assert_eq!(tag.as_slice(), hex!("a8061dc1305136c6c22b8baf0c0127a9"));
}

// ------------------------------------------------------------ Argon2id

#[test]
fn argon2id_rfc_9106_5_3() {
    use argon2::{Algorithm, Argon2, AssociatedData, ParamsBuilder, Version};

    let params = ParamsBuilder::new()
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .data(AssociatedData::new(&[0x04; 12]).unwrap())
        .build()
        .unwrap();
    let argon2 =
        Argon2::new_with_secret(&[0x03; 8], Algorithm::Argon2id, Version::V0x13, params).unwrap();
    let mut out = [0u8; 32];
    argon2
        .hash_password_into(&[0x01; 32], &[0x02; 16], &mut out)
        .unwrap();
    assert_eq!(
        out,
        hex!("0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659")
    );
}

/// The reference implementation's Argon2id version-0x13 vectors (its
/// `test.c`): password `password`, salt `somesalt`, a 32-byte tag -- run
/// through `seal::derive_key`, which is Argon2id with no secret and no
/// associated data, exactly their setting.
#[test]
fn argon2id_reference_vectors_through_seal() {
    for (iterations, memory_kib, lanes, expected) in [
        (
            2,
            256,
            1,
            hex!("9dfeb910e80bad0311fee20f9c0e2b12c17987b4cac90c2ef54d5b3021c68bfe"),
        ),
        (
            2,
            256,
            2,
            hex!("6d093c501fd5999645e0ea3bf620d7b8be7fd2db59c20d9fff9539da2bf57037"),
        ),
        (
            1,
            65_536,
            1,
            hex!("f6a5adc1ba723dddef9b5ac1d464e180fcd9dffc9d1cbf76cca2fed795d9ca98"),
        ),
        (
            2,
            65_536,
            1,
            hex!("09316115d5cf24ed5a15a31a3ba326e5cf32edc24702987c02b6566f61913cf7"),
        ),
    ] {
        let params = seal::KdfParams {
            memory_kib,
            iterations,
            lanes,
        };
        assert_eq!(
            seal::derive_key(b"password", b"somesalt", params).unwrap(),
            expected,
            "{params:?}"
        );
    }
}

// ------------------------------------------------------------ BLAKE2b

#[test]
fn blake2b_512_rfc_7693_appendix_a() {
    use blake2::Blake2b512;
    use blake2::digest::Digest;

    assert_eq!(
        Blake2b512::digest(b"abc").as_slice(),
        hex!(
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1"
            "7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        )
    );
}
