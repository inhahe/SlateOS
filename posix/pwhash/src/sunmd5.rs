//! SunMD5's rounds: the hashing behind Solaris's `$md5` crypt method, whose
//! settings `posix/src/sunmd5.rs` reads.
//!
//! A port of the loop of libxcrypt 4.4.36's `crypt-sunmd5.c` -- Zack
//! Weinberg's clean-room implementation of Passlib's description, 2-clause
//! BSD; its notice is `sunmd5.rs`'s -- as design-decisions §539 asks
//! (primitives are ported, not written): an MD5 of the password and the
//! setting, then thousands of MD5s of the last digest and the round's
//! number, each round taking in Hamlet's soliloquy, too, when a coin
//! tossed from the last digest says so.

#![allow(clippy::indexing_slicing)] // digest bytes, by indices reduced mod 16 or mod 128 / 8
#![allow(clippy::arithmetic_side_effects)] // the coin's small divisions and shifts

use crate::md5::Md5;

/// Hamlet's soliloquy, as `hamlet_quotation` has it -- Project Gutenberg's
/// older edition, double blank lines as `\n` -- with the NUL that `sizeof`
/// counts, which each round that takes it in takes in too.
const HAMLET: &[u8] = b"To be, or not to be,--that is the question:--\n\
Whether 'tis nobler in the mind to suffer\n\
The slings and arrows of outrageous fortune\n\
Or to take arms against a sea of troubles,\n\
And by opposing end them?--To die,--to sleep,--\n\
No more; and by a sleep to say we end\n\
The heartache, and the thousand natural shocks\n\
That flesh is heir to,--'tis a consummation\n\
Devoutly to be wish'd. To die,--to sleep;--\n\
To sleep! perchance to dream:--ay, there's the rub;\n\
For in that sleep of death what dreams may come,\n\
When we have shuffled off this mortal coil,\n\
Must give us pause: there's the respect\n\
That makes calamity of so long life;\n\
For who would bear the whips and scorns of time,\n\
The oppressor's wrong, the proud man's contumely,\n\
The pangs of despis'd love, the law's delay,\n\
The insolence of office, and the spurns\n\
That patient merit of the unworthy takes,\n\
When he himself might his quietus make\n\
With a bare bodkin? who would these fardels bear,\n\
To grunt and sweat under a weary life,\n\
But that the dread of something after death,--\n\
The undiscover'd country, from whose bourn\n\
No traveller returns,--puzzles the will,\n\
And makes us rather bear those ills we have\n\
Than fly to others that we know not of?\n\
Thus conscience does make cowards of us all;\n\
And thus the native hue of resolution\n\
Is sicklied o'er with the pale cast of thought;\n\
And enterprises of great pith and moment,\n\
With this regard, their currents turn awry,\n\
And lose the name of action.--Soft you now!\n\
The fair Ophelia!--Nymph, in thy orisons\n\
Be all my sins remember'd.\n\0";

/// `get_nth_bit`: bit `n` of the digest, mod 128, the lowest of each byte
/// first.
fn bit(digest: &[u8; 16], n: u32) -> bool {
    let n = n % 128;
    digest[(n / 8) as usize] & (1 << (n % 8)) != 0
}

/// `muffet_coin_toss`: whether round `round` takes in the soliloquy.
fn coin_toss(prev: &[u8; 16], round: u32) -> bool {
    let at = |i: usize| u32::from(prev[i % 16]);
    let (mut x, mut y) = (0u32, 0u32);
    for i in 0..8 {
        for (out, first, second) in [(&mut x, i, i + 3), (&mut y, i + 8, i + 11)] {
            let a = at(first);
            let b = at(second);
            let r = a >> (b % 5);
            let mut v = at(r as usize);
            if b & (1 << (a % 8)) != 0 {
                v /= 2;
            }
            *out |= u32::from(bit(prev, v)) << i;
        }
    }
    if bit(prev, round) {
        x /= 2;
    }
    if bit(prev, round.wrapping_add(64)) {
        y /= 2;
    }
    bit(prev, x) ^ bit(prev, y)
}

/// `n` in decimal, as `snprintf("%u")` writes it, into `buf`: the digits.
fn decimal(mut n: u32, buf: &mut [u8; 10]) -> &[u8] {
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buf[at..]
}

/// SunMD5's digest of `password` under `head` -- the setting through its
/// salt, as `crypt_sunmd5_rn` measures it -- after `rounds` stretching
/// rounds (4096 and the setting's `rounds=`, mod 2^32, as there).
#[must_use]
pub fn digest(password: &[u8], head: &[u8], rounds: u32) -> [u8; 16] {
    let mut h = Md5::new();
    h.update(password);
    h.update(head);
    let mut dg = h.finalize();
    let mut digits = [0u8; 10];
    for i in 0..rounds {
        let mut h = Md5::new();
        h.update(&dg);
        if coin_toss(&dg, i) {
            h.update(HAMLET);
        }
        h.update(decimal(i, &mut digits));
        dg = h.finalize();
    }
    dg
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The soliloquy is `hamlet_quotation`, NUL and all: its 1517 bytes'
    /// MD5, computed from libxcrypt's `crypt-sunmd5.c`.  (Its every byte
    /// counts, and libxcrypt's answers, `crypt.rs`'s oracle, hold them too.)
    #[test]
    fn the_soliloquy() {
        use core::fmt::Write;
        assert_eq!(HAMLET.len(), 1517);
        assert_eq!(HAMLET.split(|&b| b == b'\n').count(), 36, "35 lines");
        let mut h = Md5::new();
        h.update(HAMLET);
        let mut sum = std::string::String::new();
        for b in h.finalize() {
            write!(sum, "{b:02x}").unwrap();
        }
        assert_eq!(sum, "3425548f2607d3747f134e56bd7a1066");
    }

    #[test]
    fn decimals() {
        let mut buf = [0u8; 10];
        assert_eq!(decimal(0, &mut buf), b"0");
        assert_eq!(decimal(4095, &mut buf), b"4095");
        assert_eq!(decimal(u32::MAX, &mut buf), b"4294967295");
    }

    /// The rounds are MD5s in a chain: none is the first MD5.
    #[test]
    fn no_rounds_is_the_first_md5() {
        let mut h = Md5::new();
        h.update(b"pw");
        h.update(b"$md5$salt$");
        assert_eq!(digest(b"pw", b"$md5$salt$", 0), h.finalize());
        assert_ne!(
            digest(b"pw", b"$md5$salt$", 1),
            digest(b"pw", b"$md5$salt$", 0)
        );
    }
}
