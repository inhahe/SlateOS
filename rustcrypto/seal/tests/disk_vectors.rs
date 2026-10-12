//! The published vectors for the disk-encryption primitives: AES and
//! XTS-AES, which lane A's encrypting block layer stands on (AES-256 in XTS
//! mode, the sector number as the tweak -- what LUKS2 and dm-crypt's
//! `aes-xts-plain64` use).
//!
//! `seal` itself uses neither. The vectors run here for the reason the rest of
//! this directory's do (`vectors.rs`): the vendored crates' own suites cannot
//! run in this workspace, so their vectors are run against exactly the copies
//! the tree builds.
//!
//! | Primitive | Vectors |
//! |---|---|
//! | AES-128/192/256 | FIPS-197 Appendix C; NESSIE (upstream `aes`'s own data files, read in place) |
//! | XTS-AES-256 | NIST CAVP `XTSGenAES256.rsp`, the data-unit-sequence-number form (`tests/data/`, as NIST published it) |
//!
//! NIST's file holds 1000 vectors: data units of 256 and 384 bits (two and
//! three AES blocks), and of 140 and 250 bits. The last two end inside a
//! byte, and XTS as a disk uses it -- and as `xts-mode` implements it -- works
//! on whole bytes, so those 400 are counted and skipped, and the count is
//! checked so that a misread file cannot pass by running nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use aes::cipher::{Block, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use aes::{Aes128, Aes192, Aes256};
use hex_literal::hex;
use xts_mode::{Xts128, get_tweak_default};

// ---------------------------------------------------------------------------
// AES
// ---------------------------------------------------------------------------

/// One block through `C` both ways: `pt` must become `ct`, and back.
fn one_block<C: BlockCipherEncrypt + BlockCipherDecrypt + KeyInit>(
    key: &[u8],
    pt: [u8; 16],
    ct: [u8; 16],
) {
    let cipher = C::new_from_slice(key).expect("a key of the cipher's size");
    let mut block = Block::<C>::try_from(&pt[..]).expect("a block of the cipher's size");
    cipher.encrypt_block(&mut block);
    assert_eq!(block.as_slice(), &ct, "encryption");
    cipher.decrypt_block(&mut block);
    assert_eq!(block.as_slice(), &pt, "decryption");
}

/// FIPS-197 Appendix C.1, C.2 and C.3: the example vectors of the standard
/// itself, one per key size.
#[test]
fn aes_fips_197_appendix_c() {
    let pt = hex!("00112233445566778899aabbccddeeff");
    one_block::<Aes128>(
        &hex!("000102030405060708090a0b0c0d0e0f"),
        pt,
        hex!("69c4e0d86a7b0430d8cdb78070b4c55a"),
    );
    one_block::<Aes192>(
        &hex!("000102030405060708090a0b0c0d0e0f1011121314151617"),
        pt,
        hex!("dda97ca4864cdfe06eaf70a0ec0d7191"),
    );
    one_block::<Aes256>(
        &hex!("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"),
        pt,
        hex!("8ea2b7ca516745bfeafc49904b496089"),
    );
}

// Upstream's NESSIE vectors, through upstream's own reader (`cipher::dev`).
// The macro reads `data/<name>.blb` beside this file, so the name climbs out
// of `tests/data/` to `aes`'s copy.
cipher::block_cipher_test!(aes128_nessie, "../../../aes/tests/data/aes128", aes::Aes128);
cipher::block_cipher_test!(aes192_nessie, "../../../aes/tests/data/aes192", aes::Aes192);
cipher::block_cipher_test!(aes256_nessie, "../../../aes/tests/data/aes256", aes::Aes256);

// ---------------------------------------------------------------------------
// XTS-AES-256
// ---------------------------------------------------------------------------

/// NIST CAVP's XTS-AES-256 vectors, data-unit-sequence-number form.
const XTS_AES_256: &str = include_str!("data/XTSGenAES256.rsp");

/// One vector of the file.
struct XtsVector {
    encrypt: bool,
    count: u32,
    bits: usize,
    key: Vec<u8>,
    sequence: u128,
    pt: Vec<u8>,
    ct: Vec<u8>,
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex: {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Every vector in `text`, in order, with the section it is in.
fn xts_vectors(text: &str) -> Vec<XtsVector> {
    let mut out = Vec::new();
    let mut encrypt = true;
    let mut current: Option<XtsVector> = None;
    for line in text.lines().map(str::trim) {
        if line == "[ENCRYPT]" {
            encrypt = true;
            continue;
        }
        if line == "[DECRYPT]" {
            encrypt = false;
            continue;
        }
        let Some((name, value)) = line.split_once(" = ") else {
            continue;
        };
        match name {
            "COUNT" => {
                if let Some(v) = current.take() {
                    out.push(v);
                }
                current = Some(XtsVector {
                    encrypt,
                    count: value.parse().expect("COUNT"),
                    bits: 0,
                    key: Vec::new(),
                    sequence: 0,
                    pt: Vec::new(),
                    ct: Vec::new(),
                });
            }
            _ => {
                let v = current.as_mut().expect("a field before the first COUNT");
                match name {
                    "DataUnitLen" => v.bits = value.parse().expect("DataUnitLen"),
                    "Key" => v.key = unhex(value),
                    "DataUnitSeqNumber" => v.sequence = value.parse().expect("DataUnitSeqNumber"),
                    "PT" => v.pt = unhex(value),
                    "CT" => v.ct = unhex(value),
                    other => panic!("an unknown field {other:?}"),
                }
            }
        }
    }
    out.extend(current);
    out
}

/// An XTS-AES-256 instance from a 64-byte XTS key: the first half encrypts
/// the data, the second the tweak (IEEE 1619 §5.1).
fn xts_aes_256(key: &[u8]) -> Xts128<Aes256> {
    assert_eq!(key.len(), 64, "an XTS-AES-256 key is two AES-256 keys");
    let data = Aes256::new_from_slice(&key[..32]).expect("data key");
    let tweak = Aes256::new_from_slice(&key[32..]).expect("tweak key");
    Xts128::new(data, tweak)
}

#[test]
fn xts_aes_256_nist_cavp() {
    let vectors = xts_vectors(XTS_AES_256);
    assert_eq!(vectors.len(), 1000, "the file was misread");
    let (mut ran, mut skipped) = (0, 0);
    for v in &vectors {
        if !v.bits.is_multiple_of(8) {
            skipped += 1;
            continue;
        }
        let what = format!(
            "{} COUNT = {} ({} bits)",
            if v.encrypt { "ENCRYPT" } else { "DECRYPT" },
            v.count,
            v.bits
        );
        assert_eq!(v.pt.len() * 8, v.bits, "{what}: the plaintext's length");
        let xts = xts_aes_256(&v.key);
        let tweak = get_tweak_default(v.sequence);
        // Both directions for every vector, whichever section it is in: the
        // section says which NIST checked, and the mode must agree with
        // itself either way.
        let mut buf = v.pt.clone();
        xts.encrypt_sector(&mut buf, tweak);
        assert_eq!(buf, v.ct, "{what}: encryption");
        xts.decrypt_sector(&mut buf, tweak);
        assert_eq!(buf, v.pt, "{what}: decryption");
        ran += 1;
    }
    assert_eq!(
        (ran, skipped),
        (600, 400),
        "every whole-byte vector runs; only the bit-length ones are skipped"
    );
}

/// The tweak is the sector number as a 128-bit little-endian integer, as
/// IEEE 1619 and dm-crypt's `plain64` have it -- the convention the NIST
/// vectors above are in, pinned here so a change to it shows by name.
#[test]
fn the_tweak_is_the_sector_number_little_endian() {
    let tweak = get_tweak_default(0x0102_0304);
    assert_eq!(tweak.as_slice(), &hex!("04030201000000000000000000000000"));
}

/// A disk sector of 512 bytes, the size lane A encrypts, round trips through
/// `encrypt_area`, and two sectors with the same contents encrypt differently
/// (the tweak is what makes them differ).
#[test]
fn whole_sectors_round_trip_and_differ_by_sector_number() {
    let key: Vec<u8> = (0_u8..64).collect();
    let xts = xts_aes_256(&key);
    let plain = vec![0x5a_u8; 1024];
    let mut buf = plain.clone();
    xts.encrypt_area(&mut buf, 512, 7, get_tweak_default);
    assert_ne!(buf, plain);
    assert_ne!(buf[..512], buf[512..], "sectors 7 and 8 encrypted alike");
    xts.decrypt_area(&mut buf, 512, 7, get_tweak_default);
    assert_eq!(buf, plain);
}
