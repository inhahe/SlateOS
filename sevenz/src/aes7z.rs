//! 7z's encryption as 7-Zip decrypts it (method `06F10701`):
//! `CPP/7zip/Crypto/7zAes.cpp` from the LZMA SDK 26.00 (Igor Pavlov, public
//! domain), ported, over the workspace's `aes` and `sha2`.
//!
//! The data is AES-256 in CBC mode. The key is SHA-256 over 2^n rounds of
//! salt, password and a round counter -- n, the salt and the initial
//! vector are the coder's properties -- or, for n = 63, salt and password
//! themselves. The password is its UTF-16LE bytes, as 7-Zip hands it over.
//!
//! The coder runs inside 7-Zip's `FilterCoder`, whose rules for a block
//! cipher at the end of a stream are kept: whole 16-byte blocks are
//! decrypted, output stops at the coder's size, and a last block short of
//! 16 bytes is a damaged stream.

use alloc::vec::Vec;

use crate::lzma_coder::Coded;

/// `k_NumCyclesPower_Supported_MAX`
const CYCLES_POWER_MAX: u32 = 24;
/// The power that means "no hashing": the key is salt and password.
const CYCLES_POWER_RAW: u32 = 0x3F;
/// `kKeySize`
const KEY_SIZE: usize = 32;

/// The coder's properties (`CKeyInfo` and the initial vector).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Props {
    pub(crate) cycles_power: u32,
    pub(crate) salt: Vec<u8>,
    pub(crate) iv: [u8; aes::BLOCK_LEN],
}

/// `CDecoder::SetDecoderProperties2`. `None` is `E_INVALIDARG` or
/// `E_NOTIMPL`, both "Unsupported Method" in a 7z folder.
///
/// One deliberate difference: 7-Zip checks the rounds-power only when there
/// is a salt or a vector, so a lone property byte of up to 62 is accepted
/// and the key derivation runs 2^62 rounds -- an archive that hangs its
/// reader. Here a power above 24 (other than 63, no hashing) is refused
/// either way. 7-Zip writes 19; nothing writes more than 24.
pub(crate) fn parse_props(data: &[u8]) -> Option<Props> {
    let mut props = Props {
        cycles_power: 0,
        salt: Vec::new(),
        iv: [0; aes::BLOCK_LEN],
    };
    let Some((&b0, rest)) = data.split_first() else {
        return Some(props);
    };
    props.cycles_power = u32::from(b0 & 0x3F);
    if props.cycles_power > CYCLES_POWER_MAX && props.cycles_power != CYCLES_POWER_RAW {
        return None;
    }
    if b0 & 0xC0 == 0 {
        return rest.is_empty().then_some(props);
    }
    let (&b1, rest) = rest.split_first()?;
    // At most 1 + 15 each.
    let salt_size = usize::from((b0 >> 7) & 1).saturating_add(usize::from(b1 >> 4));
    let iv_size = usize::from((b0 >> 6) & 1).saturating_add(usize::from(b1 & 0x0F));
    if rest.len() != salt_size.checked_add(iv_size)? {
        return None;
    }
    let (salt, iv) = rest.split_at(salt_size);
    props.salt = salt.to_vec();
    props.iv.get_mut(..iv_size)?.copy_from_slice(iv);
    Some(props)
}

/// `CKeyInfo::CalcKey`: the AES key for `password` (UTF-16LE bytes).
pub(crate) fn derive_key(props: &Props, password: &[u8]) -> [u8; KEY_SIZE] {
    let mut key = [0u8; KEY_SIZE];
    if props.cycles_power == CYCLES_POWER_RAW {
        for (k, &b) in key.iter_mut().zip(props.salt.iter().chain(password)) {
            *k = b;
        }
        return key;
    }
    let mut sha = sha2::Sha256::new();
    let rounds = 1u64 << props.cycles_power;
    for round in 0..rounds {
        sha.update(&props.salt);
        sha.update(password);
        sha.update(&round.to_le_bytes());
    }
    key.copy_from_slice(&sha.finalize());
    key
}

/// The AES coder in its `FilterCoder`: `input` decrypted with `key` from
/// `props`' initial vector, to at most `out_size` bytes.
pub(crate) fn decrypt(props: &Props, key: &[u8; KEY_SIZE], input: &[u8], out_size: usize) -> Coded {
    let Ok(cipher) = aes::Aes::new(key) else {
        return Coded {
            out: Vec::new(),
            ok: false,
        };
    };
    let whole = input.len().saturating_sub(input.len() % aes::BLOCK_LEN);
    let mut out = Vec::with_capacity(whole.min(out_size));
    let mut prev = props.iv;
    for chunk in input.chunks_exact(aes::BLOCK_LEN) {
        if out.len() >= out_size {
            break;
        }
        let mut block = [0u8; aes::BLOCK_LEN];
        block.copy_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for (b, p) in block.iter_mut().zip(prev) {
            *b ^= p;
        }
        prev.copy_from_slice(chunk);
        let take = out_size.saturating_sub(out.len()).min(aes::BLOCK_LEN);
        out.extend_from_slice(block.get(..take).unwrap_or(&[]));
    }
    // A last block short of 16 bytes: "non-full last block", S_FALSE --
    // `FilterCoder` reads the stream before it writes, so the partial block
    // is seen even when the output is full before it.
    let ok = whole == input.len();
    Coded { out, ok }
}

/// A password as 7-Zip hands it to the coder: its UTF-16LE bytes.
pub(crate) fn password_bytes(password: &str) -> Vec<u8> {
    password.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use alloc::vec;

    use super::*;

    #[test]
    fn properties_are_read_as_7zip_reads_them() {
        // None at all: no salt, no vector, one round of hashing.
        assert_eq!(parse_props(&[]).unwrap().cycles_power, 0);
        // One byte, no salt or vector.
        let p = parse_props(&[19]).unwrap();
        assert_eq!((p.cycles_power, p.salt.len()), (19, 0));
        assert!(parse_props(&[19, 0]).is_none());
        // 7-Zip's own: 19 rounds-power, a 16-byte vector, no salt.
        let mut data = vec![0x40 | 19, 0x0F];
        data.extend(1..=16u8);
        let p = parse_props(&data).unwrap();
        assert_eq!(p.iv, core::array::from_fn(|i| i as u8 + 1));
        // The sizes must add up.
        assert!(parse_props(&data[..17]).is_none());
        // A salt of 1 + 15 bytes.
        let mut salted = vec![0x80 | 0x40 | 5, 0xF0];
        salted.extend([7u8; 16]);
        salted.push(9);
        let p = parse_props(&salted).unwrap();
        assert_eq!((p.salt.len(), p.iv[0]), (16, 9));
        // Past 24 rounds-power, other than 63: not supported -- with or
        // without a salt and vector (7-Zip checks only with).
        assert!(parse_props(&[25]).is_none());
        assert!(parse_props(&[62]).is_none());
        assert!(parse_props(&[0x40 | 25, 0x00, 0]).is_none());
        assert!(parse_props(&[0x3F]).is_some());
    }

    #[test]
    fn the_raw_key_is_salt_and_password() {
        let props = Props {
            cycles_power: 0x3F,
            salt: vec![1, 2],
            iv: [0; 16],
        };
        let key = derive_key(&props, &[3, 4, 5]);
        assert_eq!(&key[..6], &[1, 2, 3, 4, 5, 0]);
    }

    #[test]
    fn the_hashed_key_is_sha256_over_its_rounds() {
        let props = Props {
            cycles_power: 2,
            salt: vec![0xAA],
            iv: [0; 16],
        };
        let pw = password_bytes("pw");
        let mut all = Vec::new();
        for round in 0..4u64 {
            all.push(0xAA);
            all.extend_from_slice(&pw);
            all.extend_from_slice(&round.to_le_bytes());
        }
        assert_eq!(derive_key(&props, &pw), sha2::sha256(&all));
    }

    #[test]
    fn a_short_last_block_is_damage() {
        let props = parse_props(&[]).unwrap();
        let key = [0u8; 32];
        let c = decrypt(&props, &key, &[0; 32], 32);
        assert!(c.ok && c.out.len() == 32);
        let c = decrypt(&props, &key, &[0; 33], 33);
        assert!(!c.ok);
        assert_eq!(c.out.len(), 32);
        // The output cut at its size -- and the short block still seen.
        let c = decrypt(&props, &key, &[0; 33], 20);
        assert!(!c.ok);
        assert_eq!(c.out.len(), 20);
        let c = decrypt(&props, &key, &[0; 32], 20);
        assert!(c.ok);
        assert_eq!(c.out.len(), 20);
    }
}
