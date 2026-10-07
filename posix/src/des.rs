//! The DES-based crypt(3) methods, and POSIX's `encrypt` and `setkey`.
//!
//! - **Traditional DES** -- Seventh Edition Unix's: a setting of two salt
//!   characters, a password of eight characters at most (the rest is not
//!   read), and a hash of thirteen characters, the salt's two first.
//! - **bigcrypt** -- Digital UNIX's: as traditional DES for each eight
//!   characters of a longer password, up to 128, each block's hash salted
//!   by the one before's; eleven characters more of hash for each block.
//! - **BSDi's extended DES** -- `_`, a 24-bit count of encryptions and a
//!   24-bit salt in four characters each, and the whole password, folded
//!   into one key.
//!
//! These are the oldest settings an `/etc/shadow` may hold: what Linux
//! systems hashed with until MD5 crypt, about 2000, and the BSDs with BSDi's
//! method.  None should hash a new password -- a DES key is 56 bits -- and
//! `crypt_checksalt` calls them legacy methods, but a system that cannot
//! verify them locks out every account brought from one.
//!
//! Ported from libxcrypt 4.4.36's `crypt-des.c` and `crypt-des-obsolete.c`
//! (David Burren's FreeSec, BSD-3-Clause; its notice is with the cipher, in
//! `pwhash/src/des.rs`): the settings, the hashes' encoding, the methods'
//! `crypt_gensalt`s, and `encrypt` and `setkey`.  As libxcrypt dispatches
//! them, a setting beginning `_` is BSDi's; and -- after every method with
//! a prefix -- an empty setting, or one whose first two characters are
//! salt characters, is bigcrypt's, which hands a password of more than
//! eight characters with a setting of thirteen characters or fewer (a
//! traditional hash) to traditional DES, so that it is truncated, as it was
//! when the hash was made.
//!
//! `encrypt` and `setkey` are POSIX's DES block cipher, a bit to a byte:
//! `setkey` sets a key schedule for the process, and `encrypt` encrypts or
//! decrypts a block with it, in place.  musl's and glibc's are real DES, and
//! so is this: until 2026-10-06 they answered `ENOSYS`.

// The salt and the hashes' characters are indexed by values masked to six
// bits, and the blocks by their fixed lengths; every sum is of a few small
// lengths.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use pwhash::des::Ctx;

use crate::errno;
use crate::gensalt::Refused;

/// The crypt base-64 alphabet: `ascii64`.
const ASCII64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Traditional DES's hash: the two salt characters and eleven of the
/// encrypted block.
const DES_HASH_LEN: usize = 13;
/// A block's hash: 64 bits in eleven characters.
const BLOCK_CHARS: usize = 11;
/// bigcrypt's most blocks: 128 characters of password.
const BIGCRYPT_BLOCKS: usize = 16;
/// BSDi's setting: `_`, four characters of count and four of salt.
const BSDI_SETTING_LEN: usize = 9;

/// `ascii_to_bin`: a character's value in the crypt base-64 alphabet.
fn ascii_to_bin(c: u8) -> Option<u32> {
    match c {
        b'.'..=b'9' => Some(u32::from(c - b'.')),
        b'A'..=b'Z' => Some(u32::from(c - b'A') + 12),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 38),
        _ => None,
    }
}

/// `is_des_salt_char`: a character of the crypt base-64 alphabet.
fn is_salt_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'.' || c == b'/'
}

/// Whether `setting` is bigcrypt's (and traditional DES's), as libxcrypt's
/// `get_hashfn` matches the methods with no prefix: an empty setting, or
/// two salt characters first.  Asked after every method with a prefix,
/// none of which begins with a salt character.
pub(crate) fn names_unprefixed(setting: &[u8]) -> bool {
    match setting {
        [] => true,
        [a, b, ..] => is_salt_char(*a) && is_salt_char(*b),
        [_] => false,
    }
}

/// Whether `setting` is one of these methods': BSDi's, by its `_`, or
/// [`names_unprefixed`].
pub(crate) fn names_method(setting: &[u8]) -> bool {
    setting.first() == Some(&b'_') || names_unprefixed(setting)
}

/// The next eight characters of a password as a DES key, each shifted up a
/// bit (DES ignores each byte's lowest, its parity bit; and the top bit of
/// an eight-bit character is lost): `crypt-des.c`'s loops, which pad with
/// zeros once the password is done.  `phrase` is advanced past them.
fn next_key(phrase: &mut &[u8]) -> [u8; 8] {
    let mut key = [0u8; 8];
    for byte in &mut key {
        let Some((&c, rest)) = phrase.split_first() else {
            break;
        };
        *byte = c << 1;
        *phrase = rest;
    }
    key
}

/// `des_gen_hash`: the zero block encrypted `count` times, in eleven
/// characters into `out` -- six bits each from the top, the last holding
/// the final four.
fn gen_hash(ctx: &Ctx, count: u32, out: &mut [u8]) {
    let mut cipher = ctx.crypt_block(&[0u8; 8], count, false);
    let bits = u64::from_be_bytes(cipher);
    for (i, c) in out[..BLOCK_CHARS].iter_mut().enumerate() {
        // Character i holds bits 63-6i down; the last, bits 3..0 shifted
        // up two, as the original's last partial group.
        let value = if i < 10 {
            (bits >> (58 - 6 * i)) & 0x3f
        } else {
            (bits & 0x0f) << 2
        };
        *c = ASCII64[value as usize];
    }
    crate::crypt::wipe(&mut cipher);
}

/// A setting's first two characters as traditional DES's twelve-bit salt.
fn two_char_salt(setting: &[u8]) -> Option<u32> {
    let low = ascii_to_bin(*setting.first()?)?;
    let high = ascii_to_bin(*setting.get(1)?)?;
    Some(low | (high << 6))
}

/// `crypt_descrypt_rn`: traditional DES, into `out` -- its length.
fn descrypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    let salt = two_char_salt(setting)?;
    // The salt as it reads, rewritten rather than copied: libxcrypt's
    // guard against a setting shorter than it looks.
    out[0] = ASCII64[(salt & 0x3f) as usize];
    out[1] = ASCII64[((salt >> 6) & 0x3f) as usize];
    let mut rest = phrase;
    let mut key = next_key(&mut rest);
    let mut ctx = Ctx::new();
    ctx.set_key(&key);
    ctx.set_salt(salt);
    gen_hash(&ctx, 25, &mut out[2..DES_HASH_LEN]);
    crate::crypt::wipe(&mut key);
    Some(DES_HASH_LEN)
}

/// `crypt_bigcrypt_rn`: bigcrypt -- or, for a password of more than eight
/// characters and a setting of at most thirteen, traditional DES -- into
/// `out`: its length.
fn bigcrypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    if phrase.len() > 8 && setting.len() <= DES_HASH_LEN {
        return descrypt(phrase, setting, out);
    }
    let mut salt = two_char_salt(setting)?;
    out[0] = ASCII64[(salt & 0x3f) as usize];
    out[1] = ASCII64[((salt >> 6) & 0x3f) as usize];
    let mut len = 2;
    let mut rest = phrase;
    let mut ctx = Ctx::new();
    for _ in 0..BIGCRYPT_BLOCKS {
        let mut key = next_key(&mut rest);
        ctx.set_key(&key);
        crate::crypt::wipe(&mut key);
        ctx.set_salt(salt);
        let block = &mut out[len..len + BLOCK_CHARS];
        gen_hash(&ctx, 25, block);
        // The next block's salt is this one's first two characters, always
        // the alphabet's.
        salt = two_char_salt(block)?;
        len += BLOCK_CHARS;
        if rest.is_empty() {
            break;
        }
    }
    Some(len)
}

/// `crypt_bsdicrypt_rn`: BSDi's extended DES, into `out` -- its length.
fn bsdicrypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    // `_`, and nine characters at least: those after are not read.
    let head = setting.get(..BSDI_SETTING_LEN)?;
    if head[0] != b'_' {
        return None;
    }
    let mut count = 0;
    for (i, &c) in head[1..5].iter().enumerate() {
        count |= ascii_to_bin(c)? << (6 * i);
    }
    let mut salt = 0;
    for (i, &c) in head[5..9].iter().enumerate() {
        salt |= ascii_to_bin(c)? << (6 * i);
    }
    out[..BSDI_SETTING_LEN].copy_from_slice(head);

    // The password folded into one key, eight characters at a time: each
    // block XORed with the last key's encryption of itself, unsalted.
    let mut ctx = Ctx::new();
    ctx.set_salt(0);
    let mut chained = [0u8; 8];
    let mut rest = phrase;
    loop {
        let mut key = next_key(&mut rest);
        for (k, c) in key.iter_mut().zip(chained) {
            *k ^= c;
        }
        ctx.set_key(&key);
        if rest.is_empty() {
            crate::crypt::wipe(&mut key);
            break;
        }
        chained = ctx.crypt_block(&key, 1, false);
        crate::crypt::wipe(&mut key);
    }
    crate::crypt::wipe(&mut chained);

    ctx.set_salt(salt);
    gen_hash(
        &ctx,
        count,
        &mut out[BSDI_SETTING_LEN..BSDI_SETTING_LEN + BLOCK_CHARS],
    );
    Some(BSDI_SETTING_LEN + BLOCK_CHARS)
}

/// The hash of `phrase` by `setting`, one [`names_method`] names, into
/// `out` -- its length, room left for a NUL -- or `None` for a setting
/// these methods refuse (`EINVAL`).  `phrase` is read as a C string: to its
/// first NUL, if it has one.  `out` has room for bigcrypt's longest, 178
/// characters.
pub(crate) fn crypt(phrase: &[u8], setting: &[u8], out: &mut [u8]) -> Option<usize> {
    let phrase = phrase
        .iter()
        .position(|&b| b == 0)
        .map_or(phrase, |end| &phrase[..end]);
    if out.len() <= 2 + BIGCRYPT_BLOCKS * BLOCK_CHARS {
        return None;
    }
    if setting.first() == Some(&b'_') {
        bsdicrypt(phrase, setting, out)
    } else {
        bigcrypt(phrase, setting, out)
    }
}

/// Which of these methods a stored entry is a hash of, by its exact shape:
/// `Some(false)` traditional DES or bigcrypt -- two salt characters and
/// one to sixteen blocks of eleven -- `Some(true)` BSDi's -- `_`, eight
/// characters of count and salt, and one block -- `None` neither.  Each
/// block's last character holds four bits, so it is one of the sixteen
/// characters whose value is a multiple of four: `crypt` writes no other.
pub(crate) fn stored_kind(stored: &[u8]) -> Option<bool> {
    let block_ok = |block: &[u8]| {
        block.iter().all(|&c| is_salt_char(c))
            && block
                .last()
                .and_then(|&c| ascii_to_bin(c))
                .is_some_and(|v| v % 4 == 0)
    };
    if let Some(rest) = stored.strip_prefix(b"_") {
        let ok = stored.len() == BSDI_SETTING_LEN + BLOCK_CHARS
            && rest[..BSDI_SETTING_LEN - 1]
                .iter()
                .all(|&c| is_salt_char(c))
            && block_ok(&rest[BSDI_SETTING_LEN - 1..]);
        return ok.then_some(true);
    }
    let (salt, blocks) = stored.split_at_checked(2)?;
    let ok = salt.iter().all(|&c| is_salt_char(c))
        && !blocks.is_empty()
        && blocks.len().is_multiple_of(BLOCK_CHARS)
        && blocks.len() / BLOCK_CHARS <= BIGCRYPT_BLOCKS
        && blocks.chunks(BLOCK_CHARS).all(block_ok);
    ok.then_some(false)
}

/// `gensalt_descrypt_rn` (and bigcrypt's, which is it): two salt
/// characters, from the random bytes' low six bits.  No cost to ask for.
pub(crate) fn gensalt(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    if out.len() < 3 {
        return Err(Refused::Range);
    }
    let [a, b, ..] = *rbytes else {
        return Err(Refused::Invalid);
    };
    if count != 0 {
        return Err(Refused::Invalid);
    }
    out[0] = ASCII64[usize::from(a & 0x3f)];
    out[1] = ASCII64[usize::from(b & 0x3f)];
    Ok(2)
}

/// `gensalt_bsdicrypt_rn`: `_`, the count -- 725 for 0, at most 2^24 - 1,
/// and odd, since an even one shows weak DES keys in the hash -- and 24
/// bits of salt, four characters each, the lowest bits first.
pub(crate) fn gensalt_bsdi(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    if out.len() < BSDI_SETTING_LEN + 1 {
        return Err(Refused::Range);
    }
    let [a, b, c, ..] = *rbytes else {
        return Err(Refused::Invalid);
    };
    let count = if count == 0 {
        725
    } else {
        count.min(0xff_ffff)
    } | 1;
    let salt = u64::from(a) | (u64::from(b) << 8) | (u64::from(c) << 16);
    out[0] = b'_';
    for i in 0..4 {
        out[1 + i] = ASCII64[((count >> (6 * i)) & 0x3f) as usize];
        out[5 + i] = ASCII64[((salt >> (6 * i)) & 0x3f) as usize];
    }
    Ok(BSDI_SETTING_LEN)
}

// ---------------------------------------------------------------------------
// encrypt and setkey
// ---------------------------------------------------------------------------

/// `encrypt`'s key schedule, which `setkey` sets: libxcrypt's
/// `nr_encrypt_ctx`, all zero until then, as musl's is too.  The process's
/// one, as POSIX has it; a lock keeps two threads from tearing it.
struct Schedule {
    held: AtomicBool,
    ctx: UnsafeCell<Ctx>,
}

// SAFETY: `ctx` is reached only through `Schedule::with`, while `held` is
// this thread's.
unsafe impl Sync for Schedule {}

static SCHEDULE: Schedule = Schedule {
    held: AtomicBool::new(false),
    ctx: UnsafeCell::new(Ctx::new()),
};

/// [`Schedule`]'s lock, held for a scope.
struct Held<'a>(&'a AtomicBool);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Schedule {
    /// `f` with the schedule, the lock held.
    fn with<R>(&self, f: impl FnOnce(&mut Ctx) -> R) -> R {
        while self
            .held
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let _held = Held(&self.held);
        // SAFETY: the lock is ours until `_held` drops, after `f`: no other
        // reference to the schedule exists meanwhile.
        f(unsafe { &mut *self.ctx.get() })
    }
}

/// `pack_bits`: a block of 64 bytes, each standing for its lowest bit, as
/// eight -- the first byte the top bit.
fn pack(bytes: &[u8; 64]) -> [u8; 8] {
    let mut bits = [0u8; 8];
    for (byte, chunk) in bits.iter_mut().zip(bytes.chunks_exact(8)) {
        for &b in chunk {
            *byte = (*byte << 1) | (b & 1);
        }
    }
    bits
}

/// `unpack_bits`: eight bytes as 64, each 0 or 1.
fn unpack(bits: &[u8; 8]) -> [u8; 64] {
    let mut bytes = [0u8; 64];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = (bits[i / 8] >> (7 - i % 8)) & 1;
    }
    bytes
}

/// `encrypt` -- POSIX's DES: `block`, 64 bytes each standing for its lowest
/// bit, encrypted in place with the key [`setkey`] set -- or decrypted, for
/// a nonzero `edflag`, as libxcrypt and musl take any.  Before any `setkey`
/// the schedule is all zero, as theirs is.  `errno` is untouched, as POSIX
/// asks of a success; a NULL `block` is `EFAULT`, where they fault.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn encrypt(block: *mut u8, edflag: i32) {
    if block.is_null() {
        errno::set_errno(errno::EFAULT);
        return;
    }
    // SAFETY: POSIX's `char block[64]`, the caller's to read and write.
    let block = unsafe { &mut *block.cast::<[u8; 64]>() };
    let mut input = pack(block);
    let mut output = SCHEDULE.with(|ctx| ctx.crypt_block(&input, 1, edflag != 0));
    *block = unpack(&output);
    crate::crypt::wipe(&mut input);
    crate::crypt::wipe(&mut output);
}

/// `setkey` -- the key [`encrypt`] uses: `key`, 64 bytes each standing for
/// its lowest bit, every eighth (DES's parity bits) ignored.  A NULL `key`
/// is `EFAULT`, where libxcrypt and musl fault.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setkey(key: *const u8) {
    if key.is_null() {
        errno::set_errno(errno::EFAULT);
        return;
    }
    // SAFETY: POSIX's `const char key[64]`, the caller's to read.
    let mut bits = pack(unsafe { &*key.cast::<[u8; 64]>() });
    // `do_setkey_r`: no salt, and the key's schedule.
    let mut fresh = Ctx::new();
    fresh.set_salt(0);
    fresh.set_key(&bits);
    crate::crypt::wipe(&mut bits);
    SCHEDULE.with(|ctx| *ctx = fresh);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashed(phrase: &[u8], setting: &[u8]) -> Option<std::string::String> {
        let mut out = [0u8; 256];
        let n = crypt(phrase, setting, &mut out)?;
        Some(std::string::String::from_utf8(out[..n].to_vec()).unwrap())
    }

    /// libxcrypt's known answers (`test/ka-table.inc`), a few of each: the
    /// oracle (`crypt.rs`'s `libxcrypt_answers`) has the rest.
    #[test]
    fn known_answers() {
        for (setting, phrase, want) in [
            ("CC", "", "CCHYKxYMMLQN2"),
            ("ab", "", "abmF1QH4PEr.E"),
            ("CC", "a", "CCu3ZS/UCwMaA"),
            ("ab", "abc", "abFZSxKKdq5s6"),
            ("CC..............", "", "CCHYKxYMMLQN2"),
            ("_/...CCCC", "", "_/...CCCCBeguG7nmIew"),
            ("_B...abcd", " ", "_B...abcdU2HCHlvuzWM"),
        ] {
            assert_eq!(
                hashed(phrase.as_bytes(), setting.as_bytes()).as_deref(),
                Some(want),
                "{setting} {phrase:?}"
            );
        }
    }

    /// Traditional DES reads eight characters, and the setting's first two;
    /// bigcrypt hashes every eight, unless the setting is a traditional
    /// one, and stops at 128; BSDi's reads the whole password.
    #[test]
    fn what_each_reads() {
        let long = b"0123456789abcdefghij";
        let des = hashed(long, b"ab").unwrap();
        assert_eq!(des, hashed(&long[..8], b"ab").unwrap());
        assert_eq!(
            des,
            hashed(long, b"abXXXXXXXXXXX").unwrap(),
            "13 characters"
        );
        let big = hashed(long, b"abXXXXXXXXXXXX").unwrap();
        assert_eq!(big.len(), 2 + 3 * 11, "{big}");
        assert_eq!(big[..13], des[..]);
        let most = hashed(&[b'x'; 200], b"ab..............").unwrap();
        assert_eq!(most.len(), 2 + 16 * 11);
        assert_eq!(most, hashed(&[b'x'; 128], b"ab..............").unwrap());
        assert_ne!(
            hashed(long, b"_J9..abcd").unwrap(),
            hashed(&long[..19], b"_J9..abcd").unwrap()
        );
        // The setting's tenth character on is not read.
        assert_eq!(
            hashed(long, b"_J9..abcdXYZ").unwrap(),
            hashed(long, b"_J9..abcd").unwrap()
        );
    }

    /// A character's top bit is lost; a NUL ends the password.
    #[test]
    fn the_keys_bytes() {
        assert_eq!(hashed(b"\xe1", b"ab"), hashed(b"a", b"ab"));
        assert_eq!(hashed(b"ab\0cd", b"ab"), hashed(b"ab", b"ab"));
    }

    #[test]
    fn refusals() {
        for setting in [
            &b""[..],
            b"a",
            b"a{",
            b"_",
            b"_J9..abc",
            b"_J9.{abcd",
            b"_J9..abc{",
        ] {
            assert_eq!(hashed(b"pw", setting), None, "{setting:?}");
        }
    }

    #[test]
    fn names_its_settings() {
        for setting in [&b""[..], b"ab", b"./", b"zz$", b"_", b"_J9..abcd"] {
            assert!(names_method(setting), "{setting:?}");
        }
        for setting in [&b"a"[..], b"a$", b"$1$", b"{a"] {
            assert!(!names_method(setting), "{setting:?}");
        }
    }

    #[test]
    fn stored_shapes() {
        let des = hashed(b"pw", b"ab").unwrap();
        assert_eq!(stored_kind(des.as_bytes()), Some(false));
        let big = hashed(b"0123456789", b"ab..............").unwrap();
        assert_eq!(stored_kind(big.as_bytes()), Some(false));
        let bsdi = hashed(b"pw", b"_J9..abcd").unwrap();
        assert_eq!(stored_kind(bsdi.as_bytes()), Some(true));
        // A block's last character holds four bits: 'B' (13) is not a
        // multiple of four.
        let mut odd = des.clone().into_bytes();
        odd[12] = b'B';
        assert_eq!(stored_kind(&odd), None);
        for bad in [
            &b"ab"[..],
            &des.as_bytes()[..12],
            b"_J9..abcd",
            b"a$cdefghijklm",
        ] {
            assert_eq!(stored_kind(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn gensalts() {
        let mut out = [0u8; 16];
        assert_eq!(gensalt(0, &[0x41, 0xff], &mut out), Ok(2));
        assert_eq!(&out[..2], b"/z");
        assert_eq!(gensalt(1, &[1, 2], &mut out), Err(Refused::Invalid));
        assert_eq!(gensalt(0, &[1], &mut out), Err(Refused::Invalid));
        assert_eq!(gensalt(0, &[1, 2], &mut out[..2]), Err(Refused::Range));
        assert_eq!(gensalt_bsdi(0, &[0x07, 0x24, 0x41], &mut out), Ok(9));
        assert_eq!(&out[..9], b"_J9..5EGE");
        assert_eq!(gensalt_bsdi(2, &[0, 0, 0], &mut out), Ok(9));
        assert_eq!(&out[..5], b"_1...", "even counts made odd");
        assert_eq!(gensalt_bsdi(u64::MAX, &[0, 0, 0], &mut out), Ok(9));
        assert_eq!(&out[..5], b"_zzzz");
        assert_eq!(gensalt_bsdi(0, &[1, 2], &mut out), Err(Refused::Invalid));
        assert_eq!(
            gensalt_bsdi(0, &[1, 2, 3], &mut out[..9]),
            Err(Refused::Range)
        );
    }

    /// `encrypt` and `setkey`: DES, a bit to a byte -- encryption undone by
    /// decryption, any nonzero `edflag` decrypting, only each byte's lowest
    /// bit read -- and NULL refused.  (libxcrypt's own answers are
    /// `libxcrypt_encrypt_answers`.)
    #[test]
    fn encrypt_and_setkey() {
        let _g = ENCRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 133457799BBCDFF1, the classic example, as bytes '0' and '1'.
        let key_bits = 0x1334_5779_9BBC_DFF1_u64;
        let mut key = [0u8; 64];
        for (i, k) in key.iter_mut().enumerate() {
            *k = b'0' + ((key_bits >> (63 - i)) & 1) as u8;
        }
        // POSIX: errno is untouched by a success.
        errno::set_errno(12345);
        setkey(key.as_ptr());
        let plain = unpack(&0x0123_4567_89AB_CDEF_u64.to_be_bytes());
        let mut block = plain;
        encrypt(block.as_mut_ptr(), 0);
        assert_eq!(pack(&block), 0x85E8_1354_0F0A_B405_u64.to_be_bytes());
        encrypt(block.as_mut_ptr(), -7);
        assert_eq!(block, plain);
        assert_eq!(errno::get_errno(), 12345);

        errno::set_errno(0);
        encrypt(core::ptr::null_mut(), 0);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        setkey(core::ptr::null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// `encrypt`'s schedule is the process's: tests that set it take turns.
    static ENCRYPT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn bytes64(hex: &str) -> [u8; 64] {
        let mut out = [0u8; 64];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    /// libxcrypt's own `encrypt` and `setkey`
    /// (`posix/tools/oracle/crypt_harness.py`), in order: blocks under the
    /// schedule before any key, then keys each followed by blocks, every
    /// `edflag` from -1 to 2, bytes whose bits above the lowest vary.  The
    /// block after, and `errno`, which neither touches.
    #[test]
    fn libxcrypt_encrypt_answers() {
        const ORACLE: &str = include_str!("encrypt_oracle.txt");
        let _g = ENCRYPT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // The schedule before any `setkey`: all zero.
        SCHEDULE.with(|ctx| *ctx = Ctx::new());
        let mut lines = 0;
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            lines += 1;
            let (probe, answer) = line.split_once(" = ").unwrap();
            let words: std::vec::Vec<&str> = probe.split(' ').collect();
            errno::set_errno(0);
            match words[0] {
                "setkey" => {
                    let key = bytes64(words[1]);
                    setkey(key.as_ptr());
                    assert_eq!(answer, "0", "{line}");
                }
                "encrypt" => {
                    let mut block = bytes64(words[1]);
                    encrypt(block.as_mut_ptr(), words[2].parse().unwrap());
                    let (after, errno_name) = answer.split_once(' ').unwrap();
                    assert_eq!(block, bytes64(after), "{line}");
                    assert_eq!(errno_name, "0", "{line}");
                }
                other => panic!("a probe this test does not know: {other}"),
            }
            assert_eq!(errno::get_errno(), 0, "{line}");
        }
        assert_eq!(lines, 156, "the oracle's every line");
    }
}
