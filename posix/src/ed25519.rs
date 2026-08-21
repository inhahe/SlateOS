//! Ed25519 (RFC 8032) — real signature generation and verification.
//!
//! # Why this exists
//!
//! Before this module, three programs in this tree claimed to do Ed25519 and
//! none of them did any of the mathematics:
//!
//! | Site | What it actually did |
//! |---|---|
//! | `userspace/sshd` `HostKey::sign` | HMAC-SHA256 with the seed, zero-padded to 64 bytes, labelled `ssh-ed25519` on the wire |
//! | `userspace/sshd` `handle_pubkey_auth` | compared the offered *public* key against `authorized_keys` and returned success without looking at the signature at all |
//! | `userspace/ssh` `handle_kex_reply` | `// In a production client, we would verify the host key signature here.` — and then did not |
//!
//! Each of those is an authentication bypass rather than an incompleteness. A
//! public key is public: it is transmitted in the clear during the SSH key
//! exchange and it sits world-readable in `~/.ssh/authorized_keys`. A server
//! that accepts *possession of a public key* as proof of identity accepts
//! anyone who has ever seen the user log in. And a client that skips the
//! host-key signature check makes `known_hosts` decorative — a
//! man-in-the-middle replays the recorded public key blob, signs nothing, and
//! the client reports "host key matches known_hosts".
//!
//! There was no signature primitive to call, which is how three separate sites
//! each ended up faking one. This module is that primitive.
//!
//! # What this is not
//!
//! It is a hand-written implementation of a cryptographic algorithm, which is
//! a thing this project does with its eyes open (`sha2/src/lib.rs` makes the
//! same argument, and whether we should be doing it at all is
//! `open-questions.md` → C-Q5). Writing it once, here, is not the same as it
//! being vetted — it is the precondition for it ever *being* vetted, and it
//! replaces three fakes with one auditable thing.
//!
//! ## Constants are derived, not transcribed
//!
//! Every curve constant below is computed from its *definition* in RFC 8032
//! §5.1 rather than pasted in as a block of hexadecimal:
//!
//! - `d` is computed as `-121665 * inverse(121666)`,
//! - the base point `B` is decompressed from `y = 4 * inverse(5)`,
//! - `sqrt(-1)` is computed as `2^((p-1)/4)`.
//!
//! This costs a few field inversions per operation and buys the property that
//! a typo in a constant is impossible rather than invisible. A wrong curve
//! constant does not fail loudly — it silently defines a *different curve*, on
//! which signing and verification agree with each other and with nothing else
//! in the world. That failure mode is exactly the one that a self-test cannot
//! catch unless the self-test uses external vectors, so both defences are
//! here: derived constants *and* the RFC 8032 §7.1 vectors in the tests.
//!
//! The one constant that cannot be derived is the group order `L`, which is
//! stated in the specification; a test asserts its published decimal value.
//!
//! # Constant-time-ness
//!
//! Scalar multiplication is a fixed 256-iteration double-and-add that performs
//! the addition on every iteration and selects the result with an arithmetic
//! mask, so the loop's control flow and memory-access pattern do not depend on
//! the scalar. Scalar reduction mod `L` is likewise a fixed 260-iteration
//! conditional-subtract with masked selection. This matters for [`sign`],
//! whose scalar is derived from the secret seed.
//!
//! [`verify`] operates entirely on public values, so its variable-time
//! rejection paths (a malformed point, an out-of-range `S`) leak nothing.
//!
//! What is *not* defended against here is anything below the language: the
//! compiler is free to turn a masked select back into a branch, and this makes
//! no attempt to stop it. A production system would use a vetted
//! implementation with assembly for the field arithmetic.
//!
//! # Usage
//!
//! ```
//! # use posix::ed25519;
//! let seed = [7u8; 32];
//! let public = ed25519::public_key(&seed);
//! let sig = ed25519::sign(&seed, b"message");
//! assert!(ed25519::verify(&public, b"message", &sig));
//! assert!(!ed25519::verify(&public, b"tampered", &sig));
//! ```

#![allow(clippy::arithmetic_side_effects)] // Modular arithmetic over bounded limbs; see LIMB_BITS.
#![allow(clippy::indexing_slicing)] // Fixed-size arrays indexed by compile-time-bounded loop counters.
#![allow(clippy::cast_possible_truncation)] // Narrowing happens only at explicit limb boundaries.
// `add`/`sub`/`mul`/`neg` here are field operations with limb-bound
// preconditions that `core::ops` cannot state. Implementing the operator
// traits would let `+` be written on values whose limbs have not been shown to
// satisfy those bounds, which is exactly the bug that a named method makes
// visible at the call site.
#![allow(clippy::should_implement_trait)]

use crate::sha2::{Digest, Sha512};

/// Bytes in an Ed25519 seed (the secret key proper, RFC 8032 §5.1.5).
pub const SEED_LEN: usize = 32;

/// Bytes in an encoded Ed25519 public key (a compressed curve point).
pub const PUBLIC_KEY_LEN: usize = 32;

/// Bytes in an Ed25519 signature: the encoded point `R` followed by `S`.
pub const SIGNATURE_LEN: usize = 64;

// ===========================================================================
// Field arithmetic modulo p = 2^255 - 19
// ===========================================================================
//
// A field element is five 51-bit limbs, little-endian: the value is
// l0 + l1*2^51 + l2*2^102 + l3*2^153 + l4*2^204.
//
// Limbs are allowed to grow past 51 bits between operations. `mul` and `sq`
// accept inputs up to 2^54 and return limbs below 2^51 + 2^13, so a chain of
// multiplications never overflows a u64, and the u128 accumulators in `mul`
// hold at most 5 * 19 * 2^54 * 2^54 < 2^119. Addition is only ever applied to
// multiplication outputs (below 2^52), so its result stays below 2^53 and is
// still a legal multiplication input.

/// Bits per field-element limb.
const LIMB_BITS: u32 = 51;

/// Mask selecting the low [`LIMB_BITS`] of a limb.
const LIMB_MASK: u64 = (1u64 << LIMB_BITS) - 1;

/// An element of GF(2^255 - 19).
#[derive(Clone, Copy)]
struct Fe([u64; 5]);

/// The additive identity.
const FE_ZERO: Fe = Fe([0, 0, 0, 0, 0]);

/// The multiplicative identity.
const FE_ONE: Fe = Fe([1, 0, 0, 0, 0]);

impl Fe {
    /// The field element equal to the integer `n`.
    ///
    /// Splits `n` across the first two limbs rather than dropping it whole
    /// into limb 0: every other operation here assumes its inputs respect the
    /// limb bound, and a constructor that can hand out a 64-bit limb makes
    /// `add` overflow on the very next call.
    const fn from_u64(n: u64) -> Self {
        Self([n & LIMB_MASK, n >> LIMB_BITS, 0, 0, 0])
    }

    /// Interpret 32 little-endian bytes as a field element.
    ///
    /// Bit 255 is ignored, matching RFC 8032 §5.1.2: in a compressed point
    /// that bit carries the sign of `x`, not part of `y`.
    fn from_bytes(bytes: &[u8; 32]) -> Self {
        // Each limb is read from an unaligned 64-bit window and shifted down
        // to its 51-bit boundary.
        let load = |offset: usize| -> u64 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[offset..offset + 8]);
            u64::from_le_bytes(buf)
        };
        Self([
            load(0) & LIMB_MASK,
            (load(6) >> 3) & LIMB_MASK,
            (load(12) >> 6) & LIMB_MASK,
            (load(19) >> 1) & LIMB_MASK,
            (load(24) >> 12) & LIMB_MASK,
        ])
    }

    /// Reduce every limb below 2^51, folding the top carry back in via
    /// 2^255 ≡ 19 (mod p). The result may still be at or above p.
    fn weak_reduce(self) -> Self {
        let l = self.0;
        let c0 = l[0] >> LIMB_BITS;
        let c1 = l[1] >> LIMB_BITS;
        let c2 = l[2] >> LIMB_BITS;
        let c3 = l[3] >> LIMB_BITS;
        let c4 = l[4] >> LIMB_BITS;
        Self([
            (l[0] & LIMB_MASK) + c4 * 19,
            (l[1] & LIMB_MASK) + c0,
            (l[2] & LIMB_MASK) + c1,
            (l[3] & LIMB_MASK) + c2,
            (l[4] & LIMB_MASK) + c3,
        ])
    }

    /// Encode as 32 little-endian bytes, fully reduced into `[0, p)`.
    fn to_bytes(self) -> [u8; 32] {
        let mut l = self.weak_reduce().weak_reduce().0;

        // Conditionally subtract p by adding 19 and inspecting the carry out
        // of the top limb: q is 1 exactly when the value is at least p.
        let mut q = (l[0] + 19) >> LIMB_BITS;
        q = (l[1] + q) >> LIMB_BITS;
        q = (l[2] + q) >> LIMB_BITS;
        q = (l[3] + q) >> LIMB_BITS;
        q = (l[4] + q) >> LIMB_BITS;
        l[0] += 19 * q;

        l[1] += l[0] >> LIMB_BITS;
        l[0] &= LIMB_MASK;
        l[2] += l[1] >> LIMB_BITS;
        l[1] &= LIMB_MASK;
        l[3] += l[2] >> LIMB_BITS;
        l[2] &= LIMB_MASK;
        l[4] += l[3] >> LIMB_BITS;
        l[3] &= LIMB_MASK;
        l[4] &= LIMB_MASK;

        // Repack the 5x51 limbs into four 64-bit words: limb boundaries fall
        // at bits 51, 102, 153 and 204, so each word straddles two limbs.
        let mut out = [0u8; 32];
        let words = [
            l[0] | (l[1] << 51),
            (l[1] >> 13) | (l[2] << 38),
            (l[2] >> 26) | (l[3] << 25),
            (l[3] >> 39) | (l[4] << 12),
        ];
        for (i, w) in words.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    /// Field addition.
    ///
    /// Both operands must respect the limb bound described above (below 2^54);
    /// every producer in this module does.
    fn add(self, rhs: Self) -> Self {
        Self([
            self.0[0] + rhs.0[0],
            self.0[1] + rhs.0[1],
            self.0[2] + rhs.0[2],
            self.0[3] + rhs.0[3],
            self.0[4] + rhs.0[4],
        ])
    }

    /// Field subtraction.
    ///
    /// Adds `2p` (as `[2^52-38, 2^52-2, ...]`, each limb a multiple of the
    /// limb radix) before subtracting so no limb can go negative. The inputs
    /// are always multiplication outputs, whose limbs are below 2^52, so the
    /// added constant dominates.
    fn sub(self, rhs: Self) -> Self {
        let r = rhs.weak_reduce().0;
        Self([
            self.0[0] + 0x000f_ffff_ffff_ffda - r[0],
            self.0[1] + 0x000f_ffff_ffff_fffe - r[1],
            self.0[2] + 0x000f_ffff_ffff_fffe - r[2],
            self.0[3] + 0x000f_ffff_ffff_fffe - r[3],
            self.0[4] + 0x000f_ffff_ffff_fffe - r[4],
        ])
    }

    /// Field negation.
    fn neg(self) -> Self {
        FE_ZERO.sub(self)
    }

    /// Field multiplication.
    fn mul(self, rhs: Self) -> Self {
        let a = self.weak_reduce().0;
        let b = rhs.weak_reduce().0;

        // Terms that wrap past 2^255 are folded back with the 19 factor.
        let b1_19 = u128::from(b[1]) * 19;
        let b2_19 = u128::from(b[2]) * 19;
        let b3_19 = u128::from(b[3]) * 19;
        let b4_19 = u128::from(b[4]) * 19;
        let (a0, a1, a2, a3, a4) = (
            u128::from(a[0]),
            u128::from(a[1]),
            u128::from(a[2]),
            u128::from(a[3]),
            u128::from(a[4]),
        );
        let (b0, b1, b2, b3, b4) = (
            u128::from(b[0]),
            u128::from(b[1]),
            u128::from(b[2]),
            u128::from(b[3]),
            u128::from(b[4]),
        );

        let c0 = a0 * b0 + a4 * b1_19 + a3 * b2_19 + a2 * b3_19 + a1 * b4_19;
        let mut c1 = a1 * b0 + a0 * b1 + a4 * b2_19 + a3 * b3_19 + a2 * b4_19;
        let mut c2 = a2 * b0 + a1 * b1 + a0 * b2 + a4 * b3_19 + a3 * b4_19;
        let mut c3 = a3 * b0 + a2 * b1 + a1 * b2 + a0 * b3 + a4 * b4_19;
        let mut c4 = a4 * b0 + a3 * b1 + a2 * b2 + a1 * b3 + a0 * b4;

        let mut out = [0u64; 5];
        c1 += c0 >> LIMB_BITS;
        out[0] = (c0 as u64) & LIMB_MASK;
        c2 += c1 >> LIMB_BITS;
        out[1] = (c1 as u64) & LIMB_MASK;
        c3 += c2 >> LIMB_BITS;
        out[2] = (c2 as u64) & LIMB_MASK;
        c4 += c3 >> LIMB_BITS;
        out[3] = (c3 as u64) & LIMB_MASK;
        let carry = (c4 >> LIMB_BITS) as u64;
        out[4] = (c4 as u64) & LIMB_MASK;

        out[0] += carry * 19;
        out[1] += out[0] >> LIMB_BITS;
        out[0] &= LIMB_MASK;
        Self(out)
    }

    /// Field squaring.
    fn sq(self) -> Self {
        self.mul(self)
    }

    /// `self` raised to the power given by 32 little-endian exponent bytes.
    ///
    /// Square-and-multiply, most-significant bit first. The exponents used
    /// here are all public constants, so the data-dependent multiply is not a
    /// leak.
    fn pow(self, exponent: &[u8; 32]) -> Self {
        let mut acc = FE_ONE;
        for bit in (0..256).rev() {
            acc = acc.sq();
            if (exponent[bit / 8] >> (bit % 8)) & 1 == 1 {
                acc = acc.mul(self);
            }
        }
        acc
    }

    /// Multiplicative inverse, as `self^(p-2)` by Fermat's little theorem.
    ///
    /// `0^(p-2) = 0`, which is what the decompression path below relies on to
    /// turn a division by zero into a point it then rejects.
    fn invert(self) -> Self {
        // p - 2 = 2^255 - 21.
        let mut exponent = [0xffu8; 32];
        exponent[0] = 0xeb;
        exponent[31] = 0x7f;
        self.pow(&exponent)
    }

    /// True when the element is zero modulo p.
    fn is_zero(self) -> bool {
        self.to_bytes() == [0u8; 32]
    }

    /// True when the two elements are equal modulo p.
    fn eq(self, rhs: Self) -> bool {
        self.to_bytes() == rhs.to_bytes()
    }

    /// The low bit of the canonical encoding — the "sign" of `x` in RFC 8032.
    fn is_odd(self) -> bool {
        self.to_bytes()[0] & 1 == 1
    }

    /// Branch-free select: `a` when `mask` is all ones, `b` when it is zero.
    fn select(a: Self, b: Self, mask: u64) -> Self {
        let mut out = [0u64; 5];
        for i in 0..5 {
            out[i] = (a.0[i] & mask) | (b.0[i] & !mask);
        }
        Self(out)
    }
}

/// A square root of -1 in the field, computed as `2^((p-1)/4)`.
///
/// Used to correct the candidate root in point decompression when the curve
/// equation is satisfied only up to sign.
fn sqrt_minus_one() -> Fe {
    // (p - 1) / 4 = 2^253 - 5.
    let mut exponent = [0xffu8; 32];
    exponent[0] = 0xfb;
    exponent[31] = 0x1f;
    Fe::from_u64(2).pow(&exponent)
}

/// The curve parameter `d = -121665 / 121666` (RFC 8032 §5.1).
fn curve_d() -> Fe {
    Fe::from_u64(121_665).neg().mul(Fe::from_u64(121_666).invert())
}

// ===========================================================================
// Group arithmetic on the twisted Edwards curve -x^2 + y^2 = 1 + d x^2 y^2
// ===========================================================================

/// A curve point in extended homogeneous coordinates, where `x = X/Z`,
/// `y = Y/Z` and `T = XY/Z`.
#[derive(Clone, Copy)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// The identity element of the group, `(0, 1)`.
const IDENTITY: Point = Point {
    x: FE_ZERO,
    y: FE_ONE,
    z: FE_ONE,
    t: FE_ZERO,
};

impl Point {
    /// Point addition (`add-2008-hwcd-3`, valid for `a = -1` and unified — it
    /// is correct for doubling and for the identity, so no special cases).
    // Single-letter names follow the published formula; auditability against
    // the reference matters more here than descriptive names.
    #[allow(clippy::many_single_char_names)]
    fn add(self, rhs: Self, d2: Fe) -> Self {
        let a = self.y.sub(self.x).mul(rhs.y.sub(rhs.x));
        let b = self.y.add(self.x).mul(rhs.y.add(rhs.x));
        let c = self.t.mul(d2).mul(rhs.t);
        let dd = self.z.mul(rhs.z);
        let dd = dd.add(dd);
        let e = b.sub(a);
        let f = dd.sub(c);
        let g = dd.add(c);
        let h = b.add(a);
        Self {
            x: e.mul(f),
            y: g.mul(h),
            t: e.mul(h),
            z: f.mul(g),
        }
    }

    /// Point doubling (`dbl-2008-hwcd` with `a = -1`).
    #[allow(clippy::many_single_char_names)]
    fn double(self) -> Self {
        let a = self.x.sq();
        let b = self.y.sq();
        let c = self.z.sq();
        let c = c.add(c);
        let d = a.neg();
        let e = self.x.add(self.y).sq().sub(a).sub(b);
        let g = d.add(b);
        let f = g.sub(c);
        let h = d.sub(b);
        Self {
            x: e.mul(f),
            y: g.mul(h),
            t: e.mul(h),
            z: f.mul(g),
        }
    }

    /// Compress to 32 bytes: the canonical `y`, with the low bit of `x` in
    /// bit 255 (RFC 8032 §5.1.2).
    fn compress(self) -> [u8; 32] {
        let z_inv = self.z.invert();
        let x = self.x.mul(z_inv);
        let y = self.y.mul(z_inv);
        let mut out = y.to_bytes();
        out[31] |= u8::from(x.is_odd()) << 7;
        out
    }

    /// Decompress a 32-byte encoding, or `None` if it does not name a point.
    ///
    /// Recovers `x` from `x^2 = (y^2 - 1) / (d y^2 + 1)` by computing the
    /// candidate root `(u v^3)(u v^7)^((p-5)/8)` and correcting it by
    /// `sqrt(-1)` if the curve equation holds only up to sign, per RFC 8032
    /// §5.1.3.
    fn decompress(bytes: &[u8; 32], d: Fe) -> Option<Self> {
        let sign = bytes[31] >> 7;
        let y = Fe::from_bytes(bytes);

        let y2 = y.sq();
        let u = y2.sub(FE_ONE);
        let v = y2.mul(d).add(FE_ONE);

        let v3 = v.sq().mul(v);
        let v7 = v3.sq().mul(v);
        // (p - 5) / 8 = 2^252 - 3.
        let mut exponent = [0xffu8; 32];
        exponent[0] = 0xfd;
        exponent[31] = 0x0f;
        let mut x = u.mul(v3).mul(u.mul(v7).pow(&exponent));

        let check = v.mul(x.sq());
        if !check.eq(u) {
            if check.eq(u.neg()) {
                x = x.mul(sqrt_minus_one());
            } else {
                return None;
            }
        }

        // The encoding of the identity's x is 0, which has no sign; asking for
        // the negative root of zero names no point.
        if x.is_zero() && sign == 1 {
            return None;
        }
        if x.is_odd() != (sign == 1) {
            x = x.neg();
        }

        Some(Self {
            x,
            y,
            z: FE_ONE,
            t: x.mul(y),
        })
    }

    /// Branch-free select between two points.
    fn select(a: Self, b: Self, mask: u64) -> Self {
        Self {
            x: Fe::select(a.x, b.x, mask),
            y: Fe::select(a.y, b.y, mask),
            z: Fe::select(a.z, b.z, mask),
            t: Fe::select(a.t, b.t, mask),
        }
    }

    /// `scalar * self`, as a fixed 256-step double-and-add.
    ///
    /// The addition is performed on every iteration and discarded by an
    /// arithmetic mask when the bit is clear, so neither the branch structure
    /// nor the memory-access pattern depends on the scalar.
    fn mul_scalar(self, scalar: &[u8; 32], d2: Fe) -> Self {
        let mut acc = IDENTITY;
        for bit in (0..256).rev() {
            acc = acc.double();
            let sum = acc.add(self, d2);
            let set = u64::from((scalar[bit / 8] >> (bit % 8)) & 1);
            // 1 -> all ones, 0 -> all zeros.
            let mask = set.wrapping_neg();
            acc = Self::select(sum, acc, mask);
        }
        acc
    }
}

/// The generator `B`: the point with `y = 4/5` and even `x` (RFC 8032 §5.1).
fn base_point(d: Fe) -> Option<Point> {
    let y = Fe::from_u64(4).mul(Fe::from_u64(5).invert());
    let mut encoded = y.to_bytes();
    encoded[31] &= 0x7f; // sign bit 0: the root with even x.
    Point::decompress(&encoded, d)
}

// ===========================================================================
// Scalar arithmetic modulo the group order L
// ===========================================================================
//
// L = 2^252 + 27742317777372353535851937790883648493, the order of the prime
// order subgroup generated by B. Scalars are four little-endian 64-bit limbs;
// intermediate products are eight.

/// The group order `L`, little-endian 64-bit limbs.
const L: [u64; 4] = [
    0x5812_631a_5cf5_d3ed,
    0x14de_f9de_a2f7_9cd6,
    0x0000_0000_0000_0000,
    0x1000_0000_0000_0000,
];

/// Highest shift `i` for which `L << i` still fits in 512 bits.
///
/// `L < 2^253`, so `L << 259 < 2^512`.
const MAX_L_SHIFT: usize = 259;

/// Subtract two 512-bit values, returning the difference and the borrow out.
fn wide_sub(a: &[u64; 8], b: &[u64; 8]) -> ([u64; 8], u64) {
    let mut out = [0u64; 8];
    let mut borrow = 0u64;
    for i in 0..8 {
        let (d, b1) = a[i].overflowing_sub(b[i]);
        let (d, b2) = d.overflowing_sub(borrow);
        out[i] = d;
        borrow = u64::from(b1) + u64::from(b2);
    }
    (out, borrow)
}

/// Shift a 512-bit value right by one bit.
fn wide_shr1(a: &[u64; 8]) -> [u64; 8] {
    let mut out = [0u64; 8];
    for i in 0..8 {
        let high = if i == 7 { 0 } else { a[i + 1] << 63 };
        out[i] = (a[i] >> 1) | high;
    }
    out
}

/// `L << shift` as a 512-bit value.
fn l_shifted(shift: usize) -> [u64; 8] {
    let mut out = [0u64; 8];
    let words = shift / 64;
    let bits = (shift % 64) as u32;
    for i in 0..4 {
        let lo = L[i] << bits;
        let hi = if bits == 0 { 0 } else { L[i] >> (64 - bits) };
        out[words + i] |= lo;
        if words + i + 1 < 8 {
            out[words + i + 1] |= hi;
        }
    }
    out
}

/// Reduce a 512-bit little-endian value modulo `L`.
///
/// Binary long division: subtract `L << i` for `i` from [`MAX_L_SHIFT`] down
/// to zero, keeping the difference only when it did not borrow. Every
/// iteration performs the subtraction and selects with a mask, so the running
/// time does not depend on the value — which matters because [`sign`] reduces
/// a quantity derived from the secret scalar.
fn scalar_reduce_wide(value: &[u64; 8]) -> [u8; 32] {
    let mut x = *value;
    let mut m = l_shifted(MAX_L_SHIFT);
    for _ in 0..=MAX_L_SHIFT {
        let (diff, borrow) = wide_sub(&x, &m);
        // borrow == 0 means x >= m, so take the difference.
        let mask = borrow.wrapping_sub(1);
        for i in 0..8 {
            x[i] = (diff[i] & mask) | (x[i] & !mask);
        }
        m = wide_shr1(&m);
    }

    let mut out = [0u8; 32];
    for i in 0..4 {
        out[i * 8..i * 8 + 8].copy_from_slice(&x[i].to_le_bytes());
    }
    out
}

/// Reduce 64 little-endian bytes modulo `L`, giving a 32-byte scalar.
fn scalar_from_hash(hash: &[u8; 64]) -> [u8; 32] {
    let mut wide = [0u64; 8];
    for i in 0..8 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&hash[i * 8..i * 8 + 8]);
        wide[i] = u64::from_le_bytes(buf);
    }
    scalar_reduce_wide(&wide)
}

/// Compute `(a * b + c) mod L` for 32-byte little-endian scalars.
fn scalar_mul_add(a: &[u8; 32], b: &[u8; 32], c: &[u8; 32]) -> [u8; 32] {
    let load = |bytes: &[u8; 32]| -> [u64; 4] {
        let mut limbs = [0u64; 4];
        for i in 0..4 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[i * 8..i * 8 + 8]);
            limbs[i] = u64::from_le_bytes(buf);
        }
        limbs
    };
    let (a, b, c) = (load(a), load(b), load(c));

    // Schoolbook 256x256 -> 512.
    let mut product = [0u64; 8];
    for i in 0..4 {
        let mut carry = 0u128;
        for j in 0..4 {
            let acc =
                u128::from(product[i + j]) + u128::from(a[i]) * u128::from(b[j]) + carry;
            product[i + j] = acc as u64;
            carry = acc >> 64;
        }
        // i + 4 <= 7, so this never runs off the end.
        let acc = u128::from(product[i + 4]) + carry;
        product[i + 4] = acc as u64;
    }

    // Add c. a*b < 2^507 and c < 2^253, so this cannot carry out of 512 bits.
    let mut carry = 0u128;
    for i in 0..8 {
        let addend = if i < 4 { u128::from(c[i]) } else { 0 };
        let acc = u128::from(product[i]) + addend + carry;
        product[i] = acc as u64;
        carry = acc >> 64;
    }

    scalar_reduce_wide(&product)
}

/// True when a 32-byte little-endian scalar is strictly below `L`.
fn scalar_is_canonical(scalar: &[u8; 32]) -> bool {
    for i in (0..4).rev() {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&scalar[i * 8..i * 8 + 8]);
        let limb = u64::from_le_bytes(buf);
        if limb > L[i] {
            return false;
        }
        if limb < L[i] {
            return true;
        }
    }
    // Exactly equal to L.
    false
}

// ===========================================================================
// The RFC 8032 signature scheme
// ===========================================================================

/// Hash `parts` with SHA-512, concatenated in order.
fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    for part in parts {
        hasher.update(part);
    }
    let mut out = [0u8; 64];
    hasher.finalize_into(&mut out);
    out
}

/// Expand a seed into the secret scalar and the nonce prefix (RFC 8032
/// §5.1.5): the scalar is the low half of `SHA-512(seed)` with the bottom
/// three bits cleared, bit 254 set and bit 255 cleared.
fn expand_seed(seed: &[u8; SEED_LEN]) -> ([u8; 32], [u8; 32]) {
    let h = sha512(&[seed]);
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&h[..32]);
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
    let mut prefix = [0u8; 32];
    prefix.copy_from_slice(&h[32..]);
    (scalar, prefix)
}

/// Derive the public key for a 32-byte seed.
///
/// `y = 4/5` is on the curve by construction, so the base point always
/// decompresses; the identity fallback exists so that a regression there
/// produces a key that verifies nothing rather than a panic inside a network
/// daemon. `base_point_matches_its_published_encoding` is the test that would
/// catch such a regression.
#[must_use]
pub fn public_key(seed: &[u8; SEED_LEN]) -> [u8; PUBLIC_KEY_LEN] {
    let d = curve_d();
    let d2 = d.add(d);
    let base = base_point(d).unwrap_or(IDENTITY);
    let (scalar, _) = expand_seed(seed);
    base.mul_scalar(&scalar, d2).compress()
}

/// Sign `message` with the 32-byte seed, returning `R || S`.
#[must_use]
pub fn sign(seed: &[u8; SEED_LEN], message: &[u8]) -> [u8; SIGNATURE_LEN] {
    let d = curve_d();
    let d2 = d.add(d);
    let base = base_point(d).unwrap_or(IDENTITY);

    let (scalar, prefix) = expand_seed(seed);
    let public = base.mul_scalar(&scalar, d2).compress();

    let r = scalar_from_hash(&sha512(&[&prefix, message]));
    let r_point = base.mul_scalar(&r, d2).compress();

    let k = scalar_from_hash(&sha512(&[&r_point, &public, message]));
    let s = scalar_mul_add(&k, &scalar, &r);

    let mut signature = [0u8; SIGNATURE_LEN];
    signature[..32].copy_from_slice(&r_point);
    signature[32..].copy_from_slice(&s);
    signature
}

/// Verify a signature over `message` under `public`.
///
/// Returns `false` for every rejection reason — a public key that is not a
/// curve point, an `R` that is not a curve point, an `S` at or above the group
/// order, or a signature that simply does not match. Callers get one bit
/// because there is nothing useful and safe to say about *why* a signature
/// failed.
///
/// This is the cofactorless check `[S]B = R + [k]A`, which is what OpenSSH,
/// NaCl and RFC 8032's own reference code do.
#[must_use]
pub fn verify(
    public: &[u8; PUBLIC_KEY_LEN],
    message: &[u8],
    signature: &[u8; SIGNATURE_LEN],
) -> bool {
    let d = curve_d();
    let d2 = d.add(d);
    let Some(base) = base_point(d) else {
        return false;
    };

    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&signature[..32]);
    let mut s_bytes = [0u8; 32];
    s_bytes.copy_from_slice(&signature[32..]);

    // A non-canonical S is a malleable re-encoding of a valid signature; RFC
    // 8032 §5.1.7 requires rejecting it.
    if !scalar_is_canonical(&s_bytes) {
        return false;
    }

    let Some(a_point) = Point::decompress(public, d) else {
        return false;
    };
    let Some(r_point) = Point::decompress(&r_bytes, d) else {
        return false;
    };

    let k = scalar_from_hash(&sha512(&[&r_bytes, public, message]));

    // [S]B  ==  R + [k]A
    let lhs = base.mul_scalar(&s_bytes, d2);
    let rhs = r_point.add(a_point.mul_scalar(&k, d2), d2);
    lhs.compress() == rhs.compress()
}

/// Verify a signature given slices rather than fixed-size arrays.
///
/// Wire formats hand us `&[u8]`, and a length check that lives in the caller
/// is a length check that one caller will forget. Returns `false` for any
/// length that is not exactly right.
#[must_use]
pub fn verify_slices(public: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let (Ok(public), Ok(signature)) = (
        <[u8; PUBLIC_KEY_LEN]>::try_from(public),
        <[u8; SIGNATURE_LEN]>::try_from(signature),
    ) else {
        return false;
    };
    verify(&public, message, &signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a hex string into a fixed-size array.
    fn hex<const N: usize>(s: &str) -> [u8; N] {
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), N * 2, "hex literal is the wrong length");
        let mut out = [0u8; N];
        for i in 0..N {
            let hi = (bytes[i * 2] as char).to_digit(16).expect("hex digit");
            let lo = (bytes[i * 2 + 1] as char).to_digit(16).expect("hex digit");
            out[i] = (hi * 16 + lo) as u8;
        }
        out
    }

    // -- Field arithmetic --------------------------------------------------

    #[test]
    fn field_round_trips_through_bytes() {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        bytes[31] &= 0x7f;
        assert_eq!(Fe::from_bytes(&bytes).to_bytes(), bytes);
    }

    #[test]
    fn field_reduces_p_to_zero() {
        // p = 2^255 - 19.
        let mut p = [0xffu8; 32];
        p[0] = 0xed;
        p[31] = 0x7f;
        assert!(Fe::from_bytes(&p).is_zero());
    }

    #[test]
    fn field_addition_and_subtraction_invert_each_other() {
        let a = Fe::from_u64(0x1234_5678_9abc_def0);
        let b = Fe::from_u64(0x0fed_cba9_8765_4321);
        assert!(a.add(b).sub(b).eq(a));
        assert!(a.sub(b).add(b).eq(a));
    }

    #[test]
    fn field_negation_sums_to_zero() {
        let a = Fe::from_u64(999_983);
        assert!(a.add(a.neg()).is_zero());
    }

    #[test]
    fn field_inversion_is_a_right_inverse() {
        for n in [1u64, 2, 3, 121_666, u64::MAX] {
            let a = Fe::from_u64(n);
            assert!(a.mul(a.invert()).eq(FE_ONE), "inverse failed for {n}");
        }
    }

    #[test]
    fn field_inversion_of_zero_is_zero() {
        // Relied on by decompression: a zero denominator must not produce a
        // point, and it does not, because 0^(p-2) = 0 makes the curve check
        // fail rather than dividing by zero.
        assert!(FE_ZERO.invert().is_zero());
    }

    #[test]
    fn field_multiplication_is_associative_and_distributive() {
        let a = Fe::from_u64(0xdead_beef_cafe_babe);
        let b = Fe::from_u64(0x0123_4567_89ab_cdef);
        let c = Fe::from_u64(0xfeed_face_1234_5678);
        assert!(a.mul(b).mul(c).eq(a.mul(b.mul(c))));
        assert!(a.mul(b.add(c)).eq(a.mul(b).add(a.mul(c))));
    }

    #[test]
    fn sqrt_minus_one_squares_to_minus_one() {
        assert!(sqrt_minus_one().sq().eq(FE_ONE.neg()));
    }

    #[test]
    fn curve_d_matches_its_published_value() {
        // The one place a transcribed constant appears, precisely so that the
        // derived one can be checked against it. RFC 8032 section 5.1.
        let published: [u8; 32] = hex(
            "a3785913ca4deb75abd841414d0a700098e879777940c78c73fe6f2bee6c0352",
        );
        assert_eq!(curve_d().to_bytes(), published);
    }

    // -- Group arithmetic --------------------------------------------------

    #[test]
    fn base_point_matches_its_published_encoding() {
        let d = curve_d();
        let base = base_point(d).expect("base point decompresses");
        let published: [u8; 32] = hex(
            "5866666666666666666666666666666666666666666666666666666666666666",
        );
        assert_eq!(base.compress(), published);
    }

    #[test]
    fn compress_and_decompress_round_trip() {
        let d = curve_d();
        let d2 = d.add(d);
        let base = base_point(d).expect("base point");
        let mut p = base;
        for _ in 0..8 {
            p = p.add(base, d2);
            let encoded = p.compress();
            let decoded = Point::decompress(&encoded, d).expect("round trip");
            assert_eq!(decoded.compress(), encoded);
        }
    }

    #[test]
    fn identity_compresses_to_one() {
        let mut expected = [0u8; 32];
        expected[0] = 1;
        assert_eq!(IDENTITY.compress(), expected);
    }

    #[test]
    fn doubling_agrees_with_addition() {
        let d = curve_d();
        let d2 = d.add(d);
        let base = base_point(d).expect("base point");
        assert_eq!(base.double().compress(), base.add(base, d2).compress());
    }

    #[test]
    fn scalar_multiplication_is_repeated_addition() {
        let d = curve_d();
        let d2 = d.add(d);
        let base = base_point(d).expect("base point");

        let mut sum = IDENTITY;
        for n in 0u8..16 {
            let mut scalar = [0u8; 32];
            scalar[0] = n;
            assert_eq!(
                base.mul_scalar(&scalar, d2).compress(),
                sum.compress(),
                "{n} * B disagreed with {n} additions"
            );
            sum = sum.add(base, d2);
        }
    }

    #[test]
    fn multiplying_by_the_group_order_gives_the_identity() {
        let d = curve_d();
        let d2 = d.add(d);
        let base = base_point(d).expect("base point");
        let mut order = [0u8; 32];
        for i in 0..4 {
            order[i * 8..i * 8 + 8].copy_from_slice(&L[i].to_le_bytes());
        }
        assert_eq!(base.mul_scalar(&order, d2).compress(), IDENTITY.compress());
    }

    #[test]
    fn a_non_curve_point_fails_to_decompress() {
        let d = curve_d();
        // y = 2 is not the y-coordinate of any curve point.
        let mut bytes = [0u8; 32];
        bytes[0] = 2;
        assert!(Point::decompress(&bytes, d).is_none());
    }

    // -- Scalar arithmetic -------------------------------------------------

    #[test]
    fn group_order_matches_its_published_value() {
        // L = 2^252 + 27742317777372353535851937790883648493 (RFC 8032 5.1).
        assert_eq!(L[3], 1u64 << 60, "the 2^252 term");
        assert_eq!(L[2], 0);
        let low = (u128::from(L[1]) << 64) | u128::from(L[0]);
        assert_eq!(low, 27_742_317_777_372_353_535_851_937_790_883_648_493u128);
    }

    #[test]
    fn reducing_below_the_order_is_the_identity() {
        let mut wide = [0u64; 8];
        wide[0] = 12345;
        let reduced = scalar_reduce_wide(&wide);
        let mut expected = [0u8; 32];
        expected[0..2].copy_from_slice(&12345u16.to_le_bytes());
        assert_eq!(reduced, expected);
    }

    #[test]
    fn reducing_the_order_gives_zero() {
        let mut wide = [0u64; 8];
        wide[..4].copy_from_slice(&L);
        assert_eq!(scalar_reduce_wide(&wide), [0u8; 32]);
    }

    #[test]
    fn reducing_the_order_plus_one_gives_one() {
        let mut wide = [0u64; 8];
        wide[..4].copy_from_slice(&L);
        wide[0] += 1;
        let mut expected = [0u8; 32];
        expected[0] = 1;
        assert_eq!(scalar_reduce_wide(&wide), expected);
    }

    #[test]
    fn mul_add_matches_small_arithmetic() {
        let mut a = [0u8; 32];
        a[0] = 7;
        let mut b = [0u8; 32];
        b[0] = 11;
        let mut c = [0u8; 32];
        c[0] = 5;
        let mut expected = [0u8; 32];
        expected[0] = 7 * 11 + 5;
        assert_eq!(scalar_mul_add(&a, &b, &c), expected);
    }

    #[test]
    fn mul_add_wraps_at_the_group_order() {
        // (L - 1) * 1 + 1 == 0 mod L.
        let mut minus_one = [0u8; 32];
        for i in 0..4 {
            minus_one[i * 8..i * 8 + 8].copy_from_slice(&L[i].to_le_bytes());
        }
        minus_one[0] -= 1;
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(scalar_mul_add(&minus_one, &one, &one), [0u8; 32]);
    }

    #[test]
    fn canonicity_check_brackets_the_order() {
        let mut order = [0u8; 32];
        for i in 0..4 {
            order[i * 8..i * 8 + 8].copy_from_slice(&L[i].to_le_bytes());
        }
        assert!(!scalar_is_canonical(&order), "L itself is not canonical");

        let mut below = order;
        below[0] -= 1;
        assert!(scalar_is_canonical(&below));

        let mut above = order;
        above[0] += 1;
        assert!(!scalar_is_canonical(&above));

        assert!(scalar_is_canonical(&[0u8; 32]));
        assert!(!scalar_is_canonical(&[0xffu8; 32]));
    }

    // -- RFC 8032 section 7.1 test vectors ---------------------------------
    //
    // These are the authority. Everything above checks internal consistency,
    // which a self-consistent implementation of the *wrong curve* would also
    // pass; only an external vector rules that out.

    fn check_vector(seed_hex: &str, public_hex: &str, message_hex: &str, sig_hex: &str) {
        let seed: [u8; 32] = hex(seed_hex);
        let expected_public: [u8; 32] = hex(public_hex);
        let expected_sig: [u8; 64] = hex(sig_hex);

        let message: Vec<u8> = (0..message_hex.len() / 2)
            .map(|i| {
                u8::from_str_radix(&message_hex[i * 2..i * 2 + 2], 16).expect("hex byte")
            })
            .collect();

        assert_eq!(public_key(&seed), expected_public, "public key");
        assert_eq!(sign(&seed, &message), expected_sig, "signature");
        assert!(verify(&expected_public, &message, &expected_sig), "verify");
    }

    #[test]
    fn rfc8032_test_1_empty_message() {
        check_vector(
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "",
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        );
    }

    #[test]
    fn rfc8032_test_2_one_byte() {
        check_vector(
            "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            "72",
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        );
    }

    #[test]
    fn rfc8032_test_3_two_bytes() {
        check_vector(
            "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
            "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
            "af82",
            "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
        );
    }

    #[test]
    fn rfc8032_test_sha_abc() {
        check_vector(
            "833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42",
            "ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf",
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            "dc2a4459e7369633a52b1bf277839a00201009a3efbf3ecb69bea2186c26b58909351fc9ac90b3ecfdfbc7c66431e0303dca179c138ac17ad9bef1177331a704",
        );
    }

    // -- Rejection -----------------------------------------------------------

    #[test]
    fn a_tampered_message_does_not_verify() {
        let seed = [3u8; 32];
        let public = public_key(&seed);
        let sig = sign(&seed, b"transfer 10");
        assert!(verify(&public, b"transfer 10", &sig));
        assert!(!verify(&public, b"transfer 99", &sig));
    }

    #[test]
    fn another_key_does_not_verify() {
        let sig = sign(&[3u8; 32], b"hello");
        assert!(!verify(&public_key(&[4u8; 32]), b"hello", &sig));
    }

    #[test]
    fn every_single_bit_flip_in_the_signature_is_rejected() {
        let seed = [11u8; 32];
        let public = public_key(&seed);
        let sig = sign(&seed, b"payload");
        for byte in 0..SIGNATURE_LEN {
            for bit in 0..8 {
                let mut bad = sig;
                bad[byte] ^= 1 << bit;
                assert!(
                    !verify(&public, b"payload", &bad),
                    "bit {bit} of byte {byte} flipped and still verified"
                );
            }
        }
    }

    #[test]
    fn an_all_zero_signature_is_rejected() {
        let public = public_key(&[5u8; 32]);
        assert!(!verify(&public, b"anything", &[0u8; SIGNATURE_LEN]));
    }

    #[test]
    fn a_non_canonical_s_is_rejected() {
        // S = L is a re-encoding of S = 0 and must not be accepted, or
        // signatures become malleable.
        let seed = [13u8; 32];
        let public = public_key(&seed);
        let mut sig = sign(&seed, b"m");
        for i in 0..4 {
            sig[32 + i * 8..32 + i * 8 + 8].copy_from_slice(&L[i].to_le_bytes());
        }
        assert!(!verify(&public, b"m", &sig));
    }

    #[test]
    fn a_public_key_that_is_not_a_point_is_rejected() {
        let mut bad_public = [0u8; 32];
        bad_public[0] = 2; // y = 2 is not on the curve.
        assert!(!verify(&bad_public, b"m", &[0u8; SIGNATURE_LEN]));
    }

    #[test]
    fn wrong_lengths_are_rejected_by_the_slice_form() {
        let seed = [17u8; 32];
        let public = public_key(&seed);
        let sig = sign(&seed, b"m");
        assert!(verify_slices(&public, b"m", &sig));
        assert!(!verify_slices(&public[..31], b"m", &sig));
        assert!(!verify_slices(&public, b"m", &sig[..63]));
        assert!(!verify_slices(&[], b"m", &[]));
    }

    #[test]
    fn signing_is_deterministic() {
        // RFC 8032 derives the nonce from the key and message, with no
        // randomness: the same input must always give the same signature.
        let seed = [23u8; 32];
        assert_eq!(sign(&seed, b"same"), sign(&seed, b"same"));
    }

    #[test]
    fn long_messages_verify() {
        let seed = [29u8; 32];
        let public = public_key(&seed);
        let message: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let sig = sign(&seed, &message);
        assert!(verify(&public, &message, &sig));
    }
}
