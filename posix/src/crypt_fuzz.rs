//! Hostile input for the password-hashing code: `crypt`'s settings,
//! `stored_method`'s entries and `crypt_gensalt_rn`'s arguments, mutated
//! from every method's own shapes by a fixed-seed generator.
//!
//! `crypt` is reachable from the network -- `sshd` and `ftpd` hand a
//! password and the `/etc/shadow` entry to it through `authlib` -- and its
//! parsers index what they have measured, with clippy's indexing lint
//! allowed per module.  A setting no oracle thought of must not panic (an
//! abort, in the libc: the login service gone), and no output may pass the
//! room it was given.  The oracles (`crypt.rs`'s and `gensalt.rs`'s
//! `libxcrypt_answers`) say what the right answers are; this says the
//! wrong inputs are survived.
//!
//! The generator keeps away from what would only be slow: a setting asking
//! for a large cost -- yescrypt's memory, bcrypt's rounds, a long count of
//! iterations -- is left out before it is hashed, by a reading of the
//! setting that errs towards leaving out.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::crypt::{CRYPT_DATA_SIZE, crypt_r, crypt_rn, hash_into, stored_method, verify};
use crate::gensalt::crypt_gensalt_rn;

/// xorshift64*: deterministic, so a failure is the same failure again.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a [u8]]) -> &'a [u8] {
        items[self.below(items.len())]
    }
}

/// Settings of every method, as they are written -- the seeds the
/// mutations start from.
const SEEDS: &[&[u8]] = &[
    b"$y$j75$LdJMENpBABJJ3hIHjB1Bi.",
    b"$y$j75$LdJMENpBABJJ3hIHjB1Bi.$tUlUF19mIl6XpRTpX7LBp5ABKS8KSmDfP1gXFrZ6Sy8",
    b"$gy$j75$LdJMENpBABJJ3hIHjB1Bi.",
    b"$7$06..../....SodiumChloride",
    b"$2b$04$abcdefghijklmnopqrstuu",
    b"$2a$04$CCCCCCCCCCCCCCCCCCCCC.",
    b"$2x$04$abcdefghijklmnopqrstuu",
    b"$6$saltstring",
    b"$6$rounds=1000$saltstring$",
    b"$5$rounds=1001$short",
    b"$1$abcdefgh$",
    b"$sha1$5$GGXpNqoJvglVTkGU",
    b"$sha1$7$salt$pU8aHrwuAV0GVarTzIZC1P8JMqOX",
    b"$md5$1xMeE.at$",
    b"$md5,rounds=3$salt$$",
    b"$3$",
    b"$3$$8846f7eaee8fb117ad06bdd830b7586c",
    b"_J9..abcd",
    b"_/...CCCCBeguG7nmIew",
    b"ab",
    b"abJnggxhB/yWI",
    b"CC..............",
    b"",
];

/// What a mutation inserts: the characters settings are made of, and the
/// ones they must refuse.
const PIECES: &[&[u8]] = &[
    b"$",
    b"$$",
    b",",
    b"rounds=",
    b".",
    b"/",
    b"0",
    b"9",
    b"a",
    b"z",
    b"A",
    b"Z",
    b"_",
    b"-",
    b"+",
    b"*",
    b"!",
    b":",
    b";",
    b"\\",
    b" ",
    b"\x7f",
    b"\xc3\xa9",
    b"\x01",
    b"y",
    b"gy",
    b"7",
    b"2b",
    b"sha1",
    b"md5",
    b"3",
];

fn mutate(rng: &mut Rng, setting: &mut Vec<u8>) {
    for _ in 0..1 + rng.below(4) {
        let at = if setting.is_empty() {
            0
        } else {
            rng.below(setting.len() + 1)
        };
        match rng.below(5) {
            0 => {
                let piece = rng.pick(PIECES);
                for (k, &b) in piece.iter().enumerate() {
                    setting.insert(at + k, b);
                }
            }
            1 if at < setting.len() => {
                setting.remove(at);
            }
            2 if at < setting.len() => {
                let piece = rng.pick(PIECES);
                setting[at] = piece[0];
            }
            3 => setting.truncate(at),
            _ => {
                let extra: Vec<u8> = (0..rng.below(40))
                    .map(|_| b"./0aZz$"[rng.below(7)])
                    .collect();
                setting.extend_from_slice(&extra);
            }
        }
    }
}

/// Whether hashing `setting` could take long: a run of more than three
/// digits (an iteration count, a `rounds=`), a bcrypt cost past 06, a BSDi
/// count past a thousand-odd, or a yescrypt-family setting whose
/// parameters are not the seeds' own.
fn expensive(setting: &[u8]) -> bool {
    let digits = setting
        .split(|c| !c.is_ascii_digit())
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0);
    if digits > 3 {
        return true;
    }
    if setting.starts_with(b"$2") && setting.len() >= 6 {
        let cost = &setting[4..6];
        if cost.iter().all(u8::is_ascii_digit) && (cost[0] != b'0' || cost[1] > b'6') {
            return true;
        }
    }
    if setting.first() == Some(&b'_') {
        // The count's four six-bit digits, the lowest first.
        let value = |c: u8| match c {
            b'.'..=b'9' => u32::from(c - b'.'),
            b'A'..=b'Z' => u32::from(c - b'A') + 12,
            b'a'..=b'z' => u32::from(c - b'a') + 38,
            _ => 0,
        };
        let count: u32 = setting
            .iter()
            .skip(1)
            .take(4)
            .enumerate()
            .map(|(i, &c)| value(c) << (6 * i))
            .sum();
        if count > 5000 {
            return true;
        }
    }
    for head in [&b"$y$j75$"[..], b"$gy$j75$", b"$7$06..../...."] {
        let method = &head[..head.iter().skip(1).position(|&c| c == b'$').unwrap() + 2];
        if setting.starts_with(method) && !setting.starts_with(head) {
            return true;
        }
    }
    false
}

/// A password: printable, high bytes, or long.
fn password(rng: &mut Rng) -> Vec<u8> {
    let len = [0, 1, 7, 8, 9, 16, 64, 72, 73, 200, 511][rng.below(11)];
    (0..len).map(|_| (rng.next() % 255 + 1) as u8).collect()
}

/// `crypt_r`, `crypt_rn`, `hash_into`, `verify` and `stored_method` over
/// mutated settings: none panics, and none writes past its room.
#[test]
fn crypt_survives_mutated_settings() {
    let mut rng = Rng(0x5EED_CAFE_F00D_0001);
    let mut hashed = 0;
    for round in 0..3000 {
        let mut setting = rng.pick(SEEDS).to_vec();
        mutate(&mut rng, &mut setting);
        if expensive(&setting) {
            continue;
        }
        let key = password(&mut rng);
        let mut c_key = key.clone();
        c_key.retain(|&b| b != 0);
        c_key.push(0);
        let mut c_setting = setting.clone();
        c_setting.retain(|&b| b != 0);
        c_setting.push(0);

        // crypt_r into a buffer with a fence past its 256 bytes.
        let mut data = vec![0xA5u8; 512];
        let r = crypt_r(c_key.as_ptr(), c_setting.as_ptr(), data.as_mut_ptr());
        assert_eq!(r, data.as_mut_ptr(), "{setting:?}");
        assert!(
            data[256..].iter().all(|&b| b == 0xA5),
            "past the room: {setting:?}"
        );
        let out = &data[..data.iter().position(|&b| b == 0).unwrap()];
        if out[0] != b'*' {
            hashed += 1;
            // A hash names its method and verifies itself -- while it is a
            // setting the room takes: a yescrypt-family setting wants 45
            // bytes past its own length (libxcrypt has the same limit, at
            // its 384), and a hash is longer than its setting.
            if out.len() <= 211 {
                assert!(stored_method(out).is_some(), "{setting:?} -> {out:?}");
                assert!(
                    verify(&c_key[..c_key.len() - 1], out),
                    "{setting:?} -> {out:?}"
                );
            }
        }
        // stored_method and verify read anything an /etc/shadow could hold
        // -- authlib's path, asked of every entry.
        let _ = stored_method(&setting);
        let _ = verify(&key, &setting);
        // Every eighth, the other doors in: crypt_rn, which agrees with
        // crypt_r, and the Rust API, which takes the bytes as they are,
        // NULs included.  (Each is another hash, and some are SunMD5's
        // 4096 rounds: all of them every time would be slow, not stronger.)
        if round % 8 == 0 {
            let mut big = vec![0u8; CRYPT_DATA_SIZE];
            let rn = crypt_rn(
                c_key.as_ptr(),
                c_setting.as_ptr(),
                big.as_mut_ptr(),
                i32::try_from(CRYPT_DATA_SIZE).unwrap(),
            );
            assert_eq!(rn.is_null(), out[0] == b'*', "{setting:?}");
            let mut buf = crate::crypt::buf();
            let _ = hash_into(&key, &setting, &mut buf);
        }
    }
    // The mutations reach the methods, not only their refusals.
    assert!(hashed > 500, "{hashed} hashed");
}

/// `crypt_gensalt_rn` over every prefix, mutated, any cost, any count of
/// random bytes and any room: none panics, nothing is written past the
/// room, and what is returned is a C string within it -- a setting `crypt`
/// takes, or NULL with the failure token.
#[test]
fn gensalt_survives_any_arguments() {
    let mut rng = Rng(0x5EED_CAFE_F00D_0002);
    let prefixes: &[&[u8]] = &[
        b"$y$", b"$gy$", b"$7$", b"$2b$", b"$2y$", b"$2a$", b"$2x$", b"$6$", b"$5$", b"$sha1",
        b"$md5", b"$1$", b"$3$", b"_", b"", b"ab",
    ];
    let counts = [
        0u64,
        1,
        3,
        4,
        5,
        11,
        12,
        31,
        999,
        1000,
        5000,
        1 << 20,
        u64::from(u32::MAX),
        u64::MAX,
    ];
    let bytes: Vec<u8> = (0..600u32).map(|i| (i * 131 % 251) as u8).collect();
    let mut made = 0;
    for _ in 0..20000 {
        let mut prefix = rng.pick(prefixes).to_vec();
        if rng.below(4) == 0 {
            mutate(&mut rng, &mut prefix);
        }
        prefix.retain(|&b| b != 0);
        prefix.push(0);
        let count = counts[rng.below(counts.len())];
        let nrbytes = rng.below(300);
        let size = rng.below(420);
        let mut out = vec![0x5Au8; 512];
        let given = if rng.below(8) == 0 {
            core::ptr::null()
        } else {
            bytes.as_ptr()
        };
        // SAFETY: a C string, `nrbytes` of `bytes`' 600, and `size` of `out`'s 512.
        let r = unsafe {
            crypt_gensalt_rn(
                prefix.as_ptr(),
                count,
                given,
                i32::try_from(nrbytes).unwrap(),
                out.as_mut_ptr(),
                i32::try_from(size).unwrap(),
            )
        };
        assert!(
            out[size..].iter().all(|&b| b == 0x5A),
            "past the room: {prefix:?} {count} {nrbytes} {size}"
        );
        if r.is_null() {
            continue;
        }
        made += 1;
        let end = out[..size]
            .iter()
            .position(|&b| b == 0)
            .expect("a C string within the room");
        let setting = &out[..end];
        // Every setting made is one crypt takes, cheaply enough here: the
        // costs gensalt makes are bounded, but some are slow -- hash only
        // the cheap ones.
        if !expensive(setting) && !setting.starts_with(b"$7$") && !setting.starts_with(b"$md5") {
            let mut c = setting.to_vec();
            c.push(0);
            let mut data = vec![0u8; 256];
            crypt_r(b"pw\0".as_ptr(), c.as_ptr(), data.as_mut_ptr());
            assert_ne!(data[0], b'*', "{setting:?} does not hash");
        }
    }
    assert!(made > 2000, "{made} made");
}
