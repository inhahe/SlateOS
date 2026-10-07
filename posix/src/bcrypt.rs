//! bcrypt: the `$2a$`, `$2b$`, `$2x$` and `$2y$` methods of `crypt`.
//!
//! OpenBSD's default, openSUSE's for years, and what `htpasswd -B` writes.
//! This module is the setting -- `$2b$05$` and 22 characters of salt --
//! bcrypt's base-64, and libxcrypt's run-time self-test; the hashing is
//! `pwhash::bcrypt`'s, compiled for speed (`pwhash`'s crate docs).  Both are
//! a port of libxcrypt 4.4.36's `crypt-bcrypt.c`, Solar Designer's
//! crypt_blowfish, whose notice `pwhash::bcrypt` carries.
//!
//! The four prefixes are one algorithm and three histories: `$2b$` and
//! `$2y$` are the correct one; `$2x$` hashes as a sign-extension bug once
//! did, so that hashes made by it still verify; `$2a$` is the correct one
//! with a guard against the keys that bug made collide (`pwhash::bcrypt::
//! set_key`).

// The setting is measured before it is indexed; a cost is at most 31.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

/// `$2b$05$` and the salt's 22 characters.
const SETTING_LEN: usize = 7 + 22;
/// The result, without its NUL: the setting and the hash's 31 characters.
const HASH_LEN: usize = SETTING_LEN + 31;

/// bcrypt's base-64 alphabet (`BF_itoa64`): `crypt`'s characters, in
/// another order.
const ITOA64: &[u8; 64] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// `BF_atoi64`: a character's value, or `None` for one outside the alphabet.
fn atoi64(c: u8) -> Option<u8> {
    match c {
        b'.' => Some(0),
        b'/' => Some(1),
        b'A'..=b'Z' => Some(c - b'A' + 2),
        b'a'..=b'z' => Some(c - b'a' + 28),
        b'0'..=b'9' => Some(c - b'0' + 54),
        _ => None,
    }
}

/// `BF_decode(dst, src, 16)`: the salt's 22 characters to 16 bytes, three
/// to four; the last character gives only its top two bits.
fn decode_salt(src: &[u8]) -> Option<[u8; 16]> {
    let mut chars = src.iter().copied();
    let mut next = || chars.next().and_then(atoi64);
    let mut out = [0u8; 16];
    let mut at = 0;
    loop {
        let (c1, c2) = (next()?, next()?);
        out[at] = (c1 << 2) | ((c2 & 0x30) >> 4);
        at += 1;
        if at == 16 {
            return Some(out);
        }
        let c3 = next()?;
        out[at] = ((c2 & 0x0f) << 4) | ((c3 & 0x3c) >> 2);
        at += 1;
        let c4 = next()?;
        out[at] = ((c3 & 0x03) << 6) | c4;
        at += 1;
    }
}

/// `BF_encode(dst, src, size)`: bytes to characters, three to four, a short
/// last group to as few as hold it.  The characters written.
fn encode(out: &mut [u8], src: &[u8]) -> usize {
    let mut n = 0;
    let mut put = |v: u8| {
        out[n] = ITOA64[usize::from(v)];
        n += 1;
    };
    for group in src.chunks(3) {
        let c1 = group[0];
        put(c1 >> 2);
        let Some(&c2) = group.get(1) else {
            put((c1 & 0x03) << 4);
            break;
        };
        put(((c1 & 0x03) << 4) | (c2 >> 4));
        let Some(&c3) = group.get(2) else {
            put((c2 & 0x0f) << 2);
            break;
        };
        put(((c2 & 0x0f) << 2) | (c3 >> 6));
        put(c3 & 0x3f);
    }
    n
}

/// A bcrypt setting's parts: its variant's flags, its rounds (2^cost) and
/// its salt -- `BF_crypt`'s checks, in its order.  `None` for one it
/// refuses: a prefix other than `$2a$`, `$2b$`, `$2x$` or `$2y$`, a cost
/// outside 00 to 31 or giving fewer than `min` rounds, or a salt that is
/// not 22 characters of the alphabet.  What follows the salt is not read.
fn parse(setting: &[u8], min: u32) -> Option<(u8, u32, [u8; 16])> {
    let s = setting.get(..SETTING_LEN)?;
    if s[0] != b'$' || s[1] != b'2' || s[3] != b'$' || s[6] != b'$' {
        return None;
    }
    let flags = pwhash::bcrypt::flags(s[2])?;
    let (tens, ones) = (s[4], s[5]);
    if !(b'0'..=b'3').contains(&tens) || !ones.is_ascii_digit() || (tens == b'3' && ones > b'1') {
        return None;
    }
    let count = 1u32 << (u32::from(tens - b'0') * 10 + u32::from(ones - b'0'));
    if count < min {
        return None;
    }
    Some((flags, count, decode_salt(&s[7..])?))
}

/// `BF_crypt(key, setting, output, data, min)`: the 60-character result.
fn bf_crypt(key: &[u8], setting: &[u8], min: u32) -> Option<[u8; HASH_LEN]> {
    let (flags, count, salt) = parse(setting, min)?;
    let raw = pwhash::bcrypt::hash(key, &salt, count, flags);
    let mut out = [0u8; HASH_LEN];
    out[..SETTING_LEN - 1].copy_from_slice(&setting[..SETTING_LEN - 1]);
    // The salt's last character, with the bits it carries and no others.
    out[SETTING_LEN - 1] = ITOA64[usize::from(atoi64(setting[SETTING_LEN - 1])? & 0x30)];
    // 23 of the 24 bytes: OpenBSD's bcrypt encodes no more, and this must
    // agree with it.
    encode(&mut out[SETTING_LEN..], &raw[..23]);
    Some(out)
}

/// libxcrypt's run-time self-test (`BF_full_crypt`), run after every hash:
/// the same variant on a fixed key and salt at cost 0, then the key setup's
/// safety logic on a key built for it.  It guards against a miscompile --
/// which this target's build, not the host's that the unit tests run,
/// would have -- and its work overwrites what the real hash left behind.
fn self_test(subtype: u8) -> bool {
    let Some(flags) = pwhash::bcrypt::flags(subtype) else {
        return false;
    };
    let mut setting = *b"$2a$00$abcdefghijklmnopqrstuu";
    setting[2] = subtype;
    let want: &[u8; 31] = if flags & pwhash::bcrypt::BUG != 0 {
        b"VUrPmXD6q/nVSSp7pNDhCR9071IfIRe"
    } else {
        b"i1D709vfamulimlGcq0qq3UvuUasvEa"
    };
    let hashed = bf_crypt(b"8b \xd0\xc1\xd2\xcf\xcc\xd8", &setting, 1)
        .is_some_and(|out| out[..SETTING_LEN] == setting[..] && out[SETTING_LEN..] == want[..]);
    // A key whose `$2a$` and `$2y$` words agree, as the bug would have
    // made them collide: `$2a$` must differ from `$2y$` in the safety bit
    // and nowhere else.
    let key = b"\xff\xa334\xff\xff\xff\xa3345";
    let (ae, mut ai) = pwhash::bcrypt::set_key(key, pwhash::bcrypt::SAFETY);
    let (ye, yi) = pwhash::bcrypt::set_key(key, 4);
    ai[0] ^= 0x10000;
    hashed && ai[0] == 0xdb9c_59bc && ye[17] == 0x3334_3500 && ae == ye && ai == yi
}

/// Whether `setting` names bcrypt, the method [`crypt`] computes.
pub(crate) fn names_method(setting: &[u8]) -> bool {
    matches!(
        setting.get(..4),
        Some(b"$2a$" | b"$2b$" | b"$2x$" | b"$2y$")
    )
}

/// `crypt` for a bcrypt setting (`crypt_bcrypt_rn` and its siblings): the
/// 60-character result into `out`, which has room for it and a NUL.  The
/// bytes written; `None` if the setting is refused or the self-test fails,
/// both `EINVAL` in libxcrypt.
pub(crate) fn crypt(key: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    let hashed = bf_crypt(key, setting, 16)?;
    if !self_test(setting[2]) {
        return None;
    }
    out.get_mut(..HASH_LEN)?.copy_from_slice(&hashed);
    Some(HASH_LEN)
}

/// Whether `stored` is a bcrypt hash `crypt` would reproduce: a setting it
/// takes, with its salt's last character as `crypt` writes it, then 31
/// characters of hash, the last of them too -- it carries four bits, so
/// its two low bits are zero.
pub(crate) fn is_stored_hash(stored: &[u8]) -> bool {
    stored.len() == HASH_LEN
        && parse(stored, 16).is_some()
        && atoi64(stored[SETTING_LEN - 1]).is_some_and(|v| v.is_multiple_of(16))
        && stored[SETTING_LEN..].iter().all(|&c| atoi64(c).is_some())
        && atoi64(stored[HASH_LEN - 1]).is_some_and(|v| v.is_multiple_of(4))
}

/// Whether `salt` is one a new bcrypt setting can carry as given: 22
/// characters of the alphabet, the last one holding only the two bits it
/// can (`.`, `O`, `e` or `u`), so that the hash written repeats it.
pub(crate) fn is_salt(salt: &[u8]) -> bool {
    salt.len() == 22
        && salt.iter().all(|&c| atoi64(c).is_some())
        && atoi64(salt[21]).is_some_and(|v| v.is_multiple_of(16))
}

/// `BF_gensalt`: a new `$2a$`, `$2b$` or `$2y$` setting at cost `count` (4
/// to 31; 0 is the default, 5), salted with the first 16 of `rbytes`.  The
/// characters written into `out`, with room left for a NUL.  No new `$2x$`:
/// that is the bug's, kept only to verify.
pub(crate) fn gensalt(
    subtype: u8,
    count: u64,
    rbytes: &[u8],
    out: &mut [u8],
) -> Result<usize, crate::gensalt::Refused> {
    use crate::gensalt::Refused;
    let count = if count == 0 { 5 } else { count };
    // libxcrypt asks for EINVAL's reasons before ERANGE's, here only.
    if rbytes.len() < 16 || !(4..=31).contains(&count) || !matches!(subtype, b'a' | b'b' | b'y') {
        return Err(Refused::Invalid);
    }
    if out.len() < SETTING_LEN + 1 {
        return Err(Refused::Range);
    }
    let count = count as u8;
    out[..7].copy_from_slice(&[
        b'$',
        b'2',
        subtype,
        b'$',
        b'0' + count / 10,
        b'0' + count % 10,
        b'$',
    ]);
    encode(&mut out[7..SETTING_LEN], &rbytes[..16]);
    Ok(SETTING_LEN)
}

// The hashes themselves are checked against libxcrypt through `crypt`
// (`crypt.rs`, `libxcrypt_answers`); these check the parts.
#[cfg(test)]
mod tests {
    use super::*;

    /// The salt's 22 characters are 16 bytes and back: the last character
    /// carries two bits, so 16 bytes encode to 22 characters exactly.
    #[test]
    fn salts_round_trip() {
        for seed in 0u8..20 {
            let bytes: [u8; 16] =
                core::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(seed));
            let mut chars = [0u8; 22];
            assert_eq!(encode(&mut chars, &bytes), 22);
            assert_eq!(decode_salt(&chars), Some(bytes));
            assert!(is_salt(&chars));
        }
        // A character outside the alphabet, and one short.
        assert_eq!(decode_salt(b"abcdefghijklmnopqrst-u"), None);
        assert_eq!(decode_salt(b"abcdefghijklmnopqrstu"), None);
    }

    /// `BF_crypt`'s refusals, in its order.
    #[test]
    fn settings_parse() {
        let salt = "abcdefghijklmnopqrstuu";
        for good in ["$2a$04$", "$2b$05$", "$2x$10$", "$2y$31$"] {
            assert!(
                parse(std::format!("{good}{salt}").as_bytes(), 16).is_some(),
                "{good}"
            );
        }
        for bad in [
            "$2c$05$", "$2B$05$", "$3b$05$", "$2b$32$", "$2b$40$", "$2b$5$x", "$2b$05x", "$2b$x5$",
        ] {
            assert!(
                parse(std::format!("{bad}{salt}").as_bytes(), 16).is_none(),
                "{bad}"
            );
        }
        // Cost 3 is 8 rounds, below `crypt`'s 16; the self-test takes 0.
        assert!(parse(std::format!("$2b$03${salt}").as_bytes(), 16).is_none());
        assert!(parse(std::format!("$2b$00${salt}").as_bytes(), 1).is_some());
        assert!(parse(b"$2b$05$abc", 16).is_none());
    }

    /// The self-test passes for every variant and fails for no other.
    #[test]
    fn the_self_test_passes() {
        for subtype in [b'a', b'b', b'x', b'y'] {
            assert!(self_test(subtype), "{}", char::from(subtype));
        }
        assert!(!self_test(b'c'));
    }

    /// A stored hash's shape: its salt's last character and its hash's last
    /// character as `crypt` writes them.
    #[test]
    fn stored_hashes() {
        let good = b"$2b$05$CCCCCCCCCCCCCCCCCCCCC.E5YPO9kmyuRGyh0XouQYb4YMJKvyOeW";
        assert!(is_stored_hash(good));
        let mut salt_tail = *good;
        salt_tail[28] = b'/';
        assert!(!is_stored_hash(&salt_tail));
        let mut hash_tail = *good;
        hash_tail[59] = b'X';
        assert!(!is_stored_hash(&hash_tail));
        assert!(!is_stored_hash(&good[..59]));
        assert!(!is_stored_hash(
            b"$2b$03$CCCCCCCCCCCCCCCCCCCCC.E5YPO9kmyuRGyh0XouQYb4YMJKvyOeW"
        ));
    }
}
