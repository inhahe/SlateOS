//! DES as crypt(3) uses it: the cipher behind `posix`'s traditional DES,
//! bigcrypt and BSDi extended-DES crypt methods, whose settings
//! `posix/src/des.rs` reads, and behind POSIX's `encrypt` and `setkey`.
//!
//! A port of libxcrypt 4.4.36's `alg-des.c` -- David Burren's FreeSec --
//! as design-decisions §539 asks (primitives are ported, not written):
//! `des_set_key`, `des_set_salt` and `des_crypt_block`.  crypt's salt is
//! its perturbation of DES: each of its 24 set bits swaps a pair of the
//! expansion's output bits, so that no DES hardware or table of DES built
//! for another purpose computes it.  The crypt methods encrypt a block
//! many times over -- 25 for traditional DES -- which runs with the
//! permutations between the encryptions left out, each undoing the other.
//!
//! The cipher runs on OR-mask tables -- the initial and final permutations
//! a byte at a time, the key's permutations seven bits at a time, two
//! S-boxes and the permutation P twelve bits at a time -- which FreeSec's
//! `gen-des-tables.c` builds from DES's own tables (FIPS 46-3's IP, PC-1,
//! PC-2, S-boxes and P).  They are built here the same way, by `const fn`
//! at compile time, rather than carried as libxcrypt's `alg-des-tables.c`
//! carries them: 68 KiB of numbers no one has to read, each held to that
//! file by a digest (the tests).
//!
//! FreeSec's notice, which its licence asks to be kept with it:
//!
//! ```text
//! FreeSec: libcrypt for NetBSD
//!
//! Copyright (c) 1994 David Burren
//! All rights reserved.
//!
//! Adapted for FreeBSD-2.0 by Geoffrey M. Rehmet
//!      this file should now *only* export crypt(), in order to make
//!      binaries of libcrypt exportable from the USA
//!
//! Adapted for FreeBSD-4.0 by Mark R V Murray
//!      this file should now *only* export crypt_des(), in order to make
//!      a module that can be optionally included in libcrypt.
//!
//! Adapted for libxcrypt by Zack Weinberg, 2017
//!      writable global data eliminated; type-punning eliminated;
//!      des_init() run at build time (see des-mktables.c);
//!      made into a libxcrypt algorithm module (see des-crypt.c);
//!      functionality required to support the legacy encrypt() and
//!      setkey() primitives re-exposed (see des-obsolete.c).
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted provided that the following conditions
//! are met:
//! 1. Redistributions of source code must retain the above copyright
//!    notice, this list of conditions and the following disclaimer.
//! 2. Redistributions in binary form must reproduce the above copyright
//!    notice, this list of conditions and the following disclaimer in the
//!    documentation and/or other materials provided with the distribution.
//! 3. Neither the name of the author nor the names of other contributors
//!    may be used to endorse or promote products derived from this software
//!    without specific prior written permission.
//!
//! THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
//! ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
//! IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
//! ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
//! FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
//! DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
//! OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
//! HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
//! LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
//! OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
//! SUCH DAMAGE.
//!
//! This is an original implementation of the DES and the crypt(3) interfaces
//! by David Burren <davidb@werj.com.au>.
//! ```

#![allow(clippy::indexing_slicing)] // every index masked to its table's size, or a loop's bound below an array's length
#![allow(clippy::arithmetic_side_effects)] // the tables' small loop counters; the cipher's shifts are of fixed, in-range amounts

// ---------------------------------------------------------------------------
// DES's own tables (FIPS 46-3), as gen-des-tables.c has them: 1-based bit
// numbers, the most significant bit first.
// ---------------------------------------------------------------------------

/// The initial permutation, IP: the input bit each output bit takes.
const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, //
    62, 54, 46, 38, 30, 22, 14, 6, 64, 56, 48, 40, 32, 24, 16, 8, //
    57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, //
    61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

/// Permuted choice 1, PC-1: the key's 56 bits, its parity bits left out.
const KEY_PERM: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, //
    10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60, 52, 44, 36, //
    63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, //
    14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 28, 20, 12, 4,
];

/// Permuted choice 2, PC-2: a round's 48 key bits, of the 56.
const COMP_PERM: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, //
    23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, //
    41, 52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48, //
    44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

/// The eight S-boxes, each four rows of sixteen.
const SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, //
        0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8, //
        4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, //
        15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, //
        3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5, //
        0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, //
        13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, //
        13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, //
        13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, //
        1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, //
        13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9, //
        10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, //
        3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, //
        14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6, //
        4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, //
        11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, //
        10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8, //
        9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, //
        4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, //
        13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6, //
        1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, //
        6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, //
        1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2, //
        7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, //
        2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// The permutation P, of the S-boxes' 32 output bits.
const PBOX: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, //
    2, 8, 24, 14, 32, 27, 3, 9, 19, 13, 30, 6, 22, 11, 4, 25,
];

/// How far each round rotates the key's two 28-bit halves.
const KEY_SHIFTS: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

// ---------------------------------------------------------------------------
// gen-des-tables.c's des_init, a table at a time
// ---------------------------------------------------------------------------

/// Bit `i` of a 32-bit word, counting from the most significant:
/// FreeSec's `bits32`.  Its `bits28` and `bits24` are this from 4 and 8.
const fn bit32(i: usize) -> u32 {
    0x8000_0000 >> i
}

/// The final permutation, the inverse of IP, 0-based: `final_perm`.
const FINAL_PERM: [u8; 64] = {
    let mut p = [0u8; 64];
    let mut i = 0;
    while i < 64 {
        p[i] = IP[i] - 1;
        i += 1;
    }
    p
};

/// IP itself, 0-based and inverted to say where each input bit goes:
/// `init_perm`.
const INIT_PERM: [u8; 64] = {
    let mut p = [0u8; 64];
    let mut i = 0;
    while i < 64 {
        p[FINAL_PERM[i] as usize] = i as u8;
        i += 1;
    }
    p
};

/// Where PC-1 puts each key bit, 255 for the parity bits it drops:
/// `inv_key_perm`.
const INV_KEY_PERM: [u8; 64] = {
    let mut p = [255u8; 64];
    let mut i = 0;
    while i < 56 {
        p[(KEY_PERM[i] - 1) as usize] = i as u8;
        i += 1;
    }
    p
};

/// Where PC-2 puts each of the 56 bits, 255 for the eight it drops:
/// `inv_comp_perm`.
const INV_COMP_PERM: [u8; 56] = {
    let mut p = [255u8; 56];
    let mut i = 0;
    while i < 48 {
        p[(COMP_PERM[i] - 1) as usize] = i as u8;
        i += 1;
    }
    p
};

/// Where P puts each S-box output bit: `un_pbox`.
const UN_PBOX: [u8; 32] = {
    let mut p = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        p[(PBOX[i] - 1) as usize] = i as u8;
        i += 1;
    }
    p
};

/// The S-boxes, read by their six input bits in order rather than by
/// DES's row (the outer two) and column (the inner four): `u_sbox`.
const U_SBOX: [[u8; 64]; 8] = {
    let mut u = [[0u8; 64]; 8];
    let mut i = 0;
    while i < 8 {
        let mut j = 0;
        while j < 64 {
            let b = (j & 0x20) | ((j & 1) << 4) | ((j >> 1) & 0xf);
            u[i][j] = SBOX[i][b];
            j += 1;
        }
        i += 1;
    }
    u
};

/// Each pair of S-boxes as one: twelve bits in, both boxes' four out, the
/// first's on top: `m_sbox`.
static M_SBOX: [[u8; 4096]; 4] = {
    let mut m = [[0u8; 4096]; 4];
    let mut b = 0;
    while b < 4 {
        let mut i = 0;
        while i < 64 {
            let mut j = 0;
            while j < 64 {
                m[b][(i << 6) | j] = (U_SBOX[b << 1][i] << 4) | U_SBOX[(b << 1) + 1][j];
                j += 1;
            }
            i += 1;
        }
        b += 1;
    }
    m
};

/// The OR-masks of a 64-bit permutation given as each input bit's
/// destination (`dest`), for the half of the output `right` names: entry
/// `[k][v]` is where the set bits of `v`, as the input's byte `k`, land.
/// `ip_maskl` and `ip_maskr` of `init_perm`, `fp_maskl` and `fp_maskr` of
/// `final_perm`.
const fn byte_masks(dest: &[u8; 64], right: bool) -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut k = 0;
    while k < 8 {
        let mut v = 0;
        while v < 256 {
            let mut mask = 0u32;
            let mut j = 0;
            while j < 8 {
                if v & (0x80 >> j) != 0 {
                    let obit = dest[8 * k + j] as usize;
                    if !right && obit < 32 {
                        mask |= bit32(obit);
                    } else if right && obit >= 32 {
                        mask |= bit32(obit - 32);
                    }
                }
                j += 1;
            }
            t[k][v] = mask;
            v += 1;
        }
        k += 1;
    }
    t
}

/// The OR-masks of a key permutation, seven bits at a time: entry `[k][v]`
/// is where the set bits of `v`, as the input's bits `stride * k` on, land
/// in the half `right` names -- of `half` bits each, `offset` bits down
/// from the top of the word.  `key_perm_maskl` and `key_perm_maskr` of PC-1
/// (bytes of the key, seven bits each, into 28-bit halves);
/// `comp_maskl` and `comp_maskr` of PC-2 (seven-bit groups of the 56, into
/// 24-bit halves).  A dropped bit (255) goes nowhere.
const fn seven_bit_masks(
    dest: &[u8],
    stride: usize,
    half: usize,
    offset: usize,
    right: bool,
) -> [[u32; 128]; 8] {
    let mut t = [[0u32; 128]; 8];
    let mut k = 0;
    while k < 8 {
        let mut v = 0;
        while v < 128 {
            let mut mask = 0u32;
            let mut j = 0;
            while j < 7 {
                if v & (0x80 >> (j + 1)) != 0 {
                    let obit = dest[stride * k + j] as usize;
                    if obit != 255 {
                        if !right && obit < half {
                            mask |= bit32(obit + offset);
                        } else if right && obit >= half {
                            mask |= bit32(obit - half + offset);
                        }
                    }
                }
                j += 1;
            }
            t[k][v] = mask;
            v += 1;
        }
        k += 1;
    }
    t
}

static IP_MASKL: [[u32; 256]; 8] = byte_masks(&INIT_PERM, false);
static IP_MASKR: [[u32; 256]; 8] = byte_masks(&INIT_PERM, true);
static FP_MASKL: [[u32; 256]; 8] = byte_masks(&FINAL_PERM, false);
static FP_MASKR: [[u32; 256]; 8] = byte_masks(&FINAL_PERM, true);
static KEY_PERM_MASKL: [[u32; 128]; 8] = seven_bit_masks(&INV_KEY_PERM, 8, 28, 4, false);
static KEY_PERM_MASKR: [[u32; 128]; 8] = seven_bit_masks(&INV_KEY_PERM, 8, 28, 4, true);
static COMP_MASKL: [[u32; 128]; 8] = seven_bit_masks(&INV_COMP_PERM, 7, 24, 8, false);
static COMP_MASKR: [[u32; 128]; 8] = seven_bit_masks(&INV_COMP_PERM, 7, 24, 8, true);

/// P applied to a pair of S-boxes' eight output bits, for each of the four
/// pairs: `psbox`.
static PSBOX: [[u32; 256]; 4] = {
    let mut t = [[0u32; 256]; 4];
    let mut b = 0;
    while b < 4 {
        let mut v = 0;
        while v < 256 {
            let mut mask = 0u32;
            let mut j = 0;
            while j < 8 {
                if v & (0x80 >> j) != 0 {
                    mask |= bit32(UN_PBOX[8 * b + j] as usize);
                }
                j += 1;
            }
            t[b][v] = mask;
            v += 1;
        }
        b += 1;
    }
    t
};

// ---------------------------------------------------------------------------
// The cipher
// ---------------------------------------------------------------------------

/// A key schedule and a salt: `struct des_ctx`.  Wiped when dropped -- it
/// is the key.
pub struct Ctx {
    keysl: [u32; 16],
    keysr: [u32; 16],
    saltbits: u32,
}

impl Ctx {
    /// All zero: no key set and no salt -- what POSIX's `encrypt` uses
    /// before any `setkey`, as libxcrypt's and musl's static schedules are.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            keysl: [0; 16],
            keysr: [0; 16],
            saltbits: 0,
        }
    }

    /// `des_set_key`: the sixteen rounds' keys of `key`, whose bytes'
    /// lowest bits -- DES's parity bits -- are ignored.
    pub fn set_key(&mut self, key: &[u8; 8]) {
        let raw0 = u32::from_be_bytes([key[0], key[1], key[2], key[3]]);
        let raw1 = u32::from_be_bytes([key[4], key[5], key[6], key[7]]);
        let at = |table: &[[u32; 128]; 8]| {
            table[0][((raw0 >> 25) & 0x7f) as usize]
                | table[1][((raw0 >> 17) & 0x7f) as usize]
                | table[2][((raw0 >> 9) & 0x7f) as usize]
                | table[3][((raw0 >> 1) & 0x7f) as usize]
                | table[4][((raw1 >> 25) & 0x7f) as usize]
                | table[5][((raw1 >> 17) & 0x7f) as usize]
                | table[6][((raw1 >> 9) & 0x7f) as usize]
                | table[7][((raw1 >> 1) & 0x7f) as usize]
        };
        // PC-1, into two 28-bit halves.
        let k0 = at(&KEY_PERM_MASKL);
        let k1 = at(&KEY_PERM_MASKR);
        // Each round: the halves rotated, then PC-2.  The rotation leaves
        // bits above the 28 that PC-2's masks never read.
        let mut shifts = 0;
        for (round, &shift) in KEY_SHIFTS.iter().enumerate() {
            shifts += shift;
            let t0 = (k0 << shifts) | (k0 >> (28 - shifts));
            let t1 = (k1 << shifts) | (k1 >> (28 - shifts));
            let comp = |table: &[[u32; 128]; 8]| {
                table[0][((t0 >> 21) & 0x7f) as usize]
                    | table[1][((t0 >> 14) & 0x7f) as usize]
                    | table[2][((t0 >> 7) & 0x7f) as usize]
                    | table[3][(t0 & 0x7f) as usize]
                    | table[4][((t1 >> 21) & 0x7f) as usize]
                    | table[5][((t1 >> 14) & 0x7f) as usize]
                    | table[6][((t1 >> 7) & 0x7f) as usize]
                    | table[7][(t1 & 0x7f) as usize]
            };
            self.keysl[round] = comp(&COMP_MASKL);
            self.keysr[round] = comp(&COMP_MASKR);
        }
    }

    /// `des_set_salt`: crypt's 24-bit salt, its lowest bit first, as the
    /// mask of the expansion's bits it swaps.
    pub fn set_salt(&mut self, salt: u32) {
        let mut saltbits = 0;
        for i in 0..24 {
            if salt & (1 << i) != 0 {
                saltbits |= 0x80_0000 >> i;
            }
        }
        self.saltbits = saltbits;
    }

    /// `des_crypt_block`: `input` encrypted -- or decrypted -- `count` times
    /// over (once for 0), with the salt: the initial permutation before the
    /// first and the final one after the last only, since between two
    /// encryptions they cancel.
    #[must_use]
    pub fn crypt_block(&self, input: &[u8; 8], count: u32, decrypt: bool) -> [u8; 8] {
        let l_in = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
        let r_in = u32::from_be_bytes([input[4], input[5], input[6], input[7]]);
        let permute = |left: &[[u32; 256]; 8], right: &[[u32; 256]; 8], l: u32, r: u32| {
            let half = |t: &[[u32; 256]; 8]| {
                t[0][(l >> 24) as usize]
                    | t[1][((l >> 16) & 0xff) as usize]
                    | t[2][((l >> 8) & 0xff) as usize]
                    | t[3][(l & 0xff) as usize]
                    | t[4][(r >> 24) as usize]
                    | t[5][((r >> 16) & 0xff) as usize]
                    | t[6][((r >> 8) & 0xff) as usize]
                    | t[7][(r & 0xff) as usize]
            };
            (half(left), half(right))
        };
        let (mut l, mut r) = permute(&IP_MASKL, &IP_MASKR, l_in, r_in);
        let saltbits = self.saltbits;
        let mut f = 0;
        for _ in 0..count.max(1) {
            for round in 0..16 {
                let k = if decrypt { 15 - round } else { round };
                // The expansion E, to 48 bits in two halves of 24.
                let r48l = ((r & 0x0000_0001) << 23)
                    | ((r & 0xf800_0000) >> 9)
                    | ((r & 0x1f80_0000) >> 11)
                    | ((r & 0x01f8_0000) >> 13)
                    | ((r & 0x001f_8000) >> 15);
                let r48r = ((r & 0x0001_f800) << 7)
                    | ((r & 0x0000_1f80) << 5)
                    | ((r & 0x0000_01f8) << 3)
                    | ((r & 0x0000_001f) << 1)
                    | ((r & 0x8000_0000) >> 31);
                // The salt's swaps, and the round's key.
                let swap = (r48l ^ r48r) & saltbits;
                let r48l = r48l ^ swap ^ self.keysl[k];
                let r48r = r48r ^ swap ^ self.keysr[k];
                // The S-boxes and P at once.
                f = PSBOX[0][usize::from(M_SBOX[0][(r48l >> 12) as usize])]
                    | PSBOX[1][usize::from(M_SBOX[1][(r48l & 0xfff) as usize])]
                    | PSBOX[2][usize::from(M_SBOX[2][(r48r >> 12) as usize])]
                    | PSBOX[3][usize::from(M_SBOX[3][(r48r & 0xfff) as usize])];
                f ^= l;
                l = r;
                r = f;
            }
            r = l;
            l = f;
        }
        let (l_out, r_out) = permute(&FP_MASKL, &FP_MASKR, l, r);
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(&l_out.to_be_bytes());
        out[4..].copy_from_slice(&r_out.to_be_bytes());
        out
    }
}

impl Default for Ctx {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        let words = self.keysl.iter_mut().chain(self.keysr.iter_mut());
        for word in words.chain(core::iter::once(&mut self.saltbits)) {
            // SAFETY: a valid, aligned `u32` of this struct's own: written
            // volatile so that the compiler cannot drop the store as dead.
            unsafe { core::ptr::write_volatile(word, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::{Digest, Sha256};

    fn hex(bytes: &[u8]) -> std::string::String {
        use core::fmt::Write;
        let mut s = std::string::String::new();
        for b in bytes {
            write!(s, "{b:02x}").unwrap();
        }
        s
    }

    fn digest_u8<const N: usize>(table: &[[u8; N]]) -> std::string::String {
        let mut h = Sha256::new();
        for row in table {
            h.update(row);
        }
        let mut out = [0u8; 32];
        h.finalize_into(&mut out);
        hex(&out)
    }

    fn digest_u32<const N: usize>(table: &[[u32; N]]) -> std::string::String {
        let mut h = Sha256::new();
        for row in table {
            for word in row {
                h.update(&word.to_le_bytes());
            }
        }
        let mut out = [0u8; 32];
        h.finalize_into(&mut out);
        hex(&out)
    }

    /// The generated tables are libxcrypt's `alg-des-tables.c`, entry for
    /// entry: each one's SHA-256, row by row, the `u32`s little-endian, as
    /// computed from that file.
    #[test]
    fn the_tables_are_libxcrypts() {
        assert_eq!(
            digest_u8(&M_SBOX),
            "76568b54cd6f40dedcb19d19d2c74e48d79ce549f1b2c7375fe172baccb0cf4e"
        );
        for (table, want) in [
            (
                &IP_MASKL,
                "9804d3131aa04f5454cc61d18aeea72fc2825c7d510dee39048ea31d9df06190",
            ),
            (
                &IP_MASKR,
                "9758f6765799767f9cfb714faee590b0b7cac7d20fd7c44d333b2f3017b9f403",
            ),
            (
                &FP_MASKL,
                "356d1718dbaf6d6c658c9b3f7845fbb54363fa0c85c69d012a4fafac36241cd4",
            ),
            (
                &FP_MASKR,
                "c18a8addb66bb554fab2759edcc664a5074b7ede12780b155074fb47933b5128",
            ),
        ] {
            assert_eq!(digest_u32(table), want);
        }
        for (table, want) in [
            (
                &KEY_PERM_MASKL,
                "8ead2f3570376a8708a9765c1f29b7755044dd542cc6eab5f569648a35636014",
            ),
            (
                &KEY_PERM_MASKR,
                "1c20e544dfef03fb6fd391de7e129a029758473118c1ff4e09919a011f1aa022",
            ),
            (
                &COMP_MASKL,
                "1594cf4e825e38a11daf91bc941d3179957d4ef947c740857011ab6b493287e1",
            ),
            (
                &COMP_MASKR,
                "e10ebad0ae50833065fdb2af4fd00eea3a1638e1a0d95619ccaf7d38f8c75b9f",
            ),
        ] {
            assert_eq!(digest_u32(table), want);
        }
        assert_eq!(
            digest_u32(&PSBOX),
            "7f3f4c0d67e118ad96c1c75a6c596594fc36e57df67ed7f8b9d163eeb293be16"
        );
    }

    fn block(hex: &str) -> [u8; 8] {
        let mut b = [0u8; 8];
        for (i, byte) in b.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
        }
        b
    }

    /// DES itself, with no salt: the classic worked example (key
    /// 133457799BBCDFF1), FIPS 81's "Now is t" under 0123456789ABCDEF, and
    /// NBS's first variable-plaintext answer, all-zero key.
    #[test]
    fn des_known_answers() {
        for (key, plain, cipher) in [
            ("133457799bbcdff1", "0123456789abcdef", "85e813540f0ab405"),
            ("0123456789abcdef", "4e6f772069732074", "3fa40e8a984d4815"),
            ("0101010101010101", "8000000000000000", "95f8a5e5dd31d900"),
            ("0101010101010101", "95f8a5e5dd31d900", "8000000000000000"),
        ] {
            let mut ctx = Ctx::new();
            ctx.set_key(&block(key));
            assert_eq!(
                ctx.crypt_block(&block(plain), 1, false),
                block(cipher),
                "{key} {plain}"
            );
            assert_eq!(
                ctx.crypt_block(&block(cipher), 1, true),
                block(plain),
                "{key} {cipher}"
            );
        }
    }

    /// The parity bits -- each key byte's lowest -- are not the key's.
    #[test]
    fn parity_bits_are_ignored() {
        let mut a = Ctx::new();
        a.set_key(&block("0123456789abcdef"));
        let mut b = Ctx::new();
        b.set_key(&block("0022446688aaccee"));
        let plain = block("4e6f772069732074");
        assert_eq!(
            a.crypt_block(&plain, 1, false),
            b.crypt_block(&plain, 1, false)
        );
    }

    /// `count` encryptions in a row are that many separate ones -- the
    /// permutations left out between them undo each other -- 0 is 1, and
    /// decryption undoes the chain.
    #[test]
    fn count_is_encryptions_in_a_row() {
        let mut ctx = Ctx::new();
        ctx.set_key(&block("0123456789abcdef"));
        ctx.set_salt(0x5a5);
        let plain = block("0000000000000000");
        assert_eq!(
            ctx.crypt_block(&plain, 0, false),
            ctx.crypt_block(&plain, 1, false)
        );
        let mut one_at_a_time = plain;
        for _ in 0..25 {
            one_at_a_time = ctx.crypt_block(&one_at_a_time, 1, false);
        }
        let chained = ctx.crypt_block(&plain, 25, false);
        assert_eq!(chained, one_at_a_time);
        assert_eq!(ctx.crypt_block(&chained, 25, true), plain);
    }

    /// The salt changes the cipher; with no bit set it is DES's.
    #[test]
    fn the_salt_perturbs() {
        let mut ctx = Ctx::new();
        ctx.set_key(&block("133457799bbcdff1"));
        let plain = block("0123456789abcdef");
        ctx.set_salt(0);
        assert_eq!(ctx.crypt_block(&plain, 1, false), block("85e813540f0ab405"));
        ctx.set_salt(1);
        let salted = ctx.crypt_block(&plain, 1, false);
        assert_ne!(salted, block("85e813540f0ab405"));
        assert_eq!(ctx.crypt_block(&salted, 1, true), plain);
    }
}
