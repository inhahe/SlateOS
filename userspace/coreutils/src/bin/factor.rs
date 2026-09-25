//! `factor` — print the prime factors of each NUMBER.
//!
//! ```text
//! Usage: factor [OPTION] [NUMBER]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/factor.c`. There was one before, a
//! personality of the `shuf` crate that no link ever reached, which worked in
//! `u64` and rejected anything larger.
//!
//! # What is ported, and what is not
//!
//! The *output* is upstream's to the byte, and it is fixed by arithmetic, not
//! by method: every number's prime factorisation, ascending. So the algorithms
//! are this port's own, chosen to be fast where upstream is fast:
//!
//! * below 2^127 -- upstream's "single-precision" range, where the top bit of
//!   a two-word number is clear -- trial division by the primes under 1000,
//!   then Pollard-Brent rho in Montgomery form over `u128`, with a BPSW test
//!   (Miller-Rabin to base 2, then a strong Lucas test) for primality. BPSW has
//!   no known counterexample; below 3.3e24 the Miller-Rabin bases used before
//!   it are also a proof on their own;
//! * from 2^127 up, upstream switches to GMP, and this to
//!   [`coreutils::bignat::Nat`]: the same trial division, Pollard-Brent over
//!   `Nat`, and Miller-Rabin to the first 25 prime bases.
//!
//! What is kept exactly is everything a user sees:
//!
//! * the number is read as upstream reads it: spaces (only spaces) and one
//!   `+` skipped, then decimal digits and nothing else, normalised on output
//!   (`0012` is `12:`); anything else is `‘x’ is not a valid positive integer`,
//!   the run goes on, and the status is 1;
//! * the two output paths. A number below 2^127 is written into upstream's own
//!   line buffer, flushed in at-most-512-byte chunks at line ends (every line,
//!   when standard input or output is a terminal) and at exit; a larger one
//!   goes through stdio and is flushed at once. So `factor 12 <huge>` prints
//!   the huge one *first*, and a full disk is reported three ways: a small
//!   run's `write error: No space left on device` once, a run past 512 bytes's
//!   twice (the flush that failed mid-run is retried by the exit handler), and
//!   a large-only run's bare `write error` (stdio's `close_stream`);
//! * `-h`/`--exponents`: `p^e` for a repeated factor.
//!
//! `---debug`, upstream's undocumented trace of which algorithm ran, is
//! accepted, and the trace is not written: the algorithms are not upstream's,
//! so a transcript of them would describe something that did not happen.
//!
//! # Checked against GNU
//!
//! `scripts/factor-diff.sh`.

use coreutils::bignat::Nat;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::stdfd::{self, Stream};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::num::{NonZeroU32, NonZeroU128};
use std::process::ExitCode;

coreutils::guard_std_fds!();

const FACTOR: Program = Program::new("factor", 1);

/// Upstream's `long_options[]`. `-debug` is spelled `---debug`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("exponents", Takes::Nothing),
    ("-debug", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run {
        exponents: bool,
        operands: Vec<OsString>,
    },
}

fn help_text() -> &'static str {
    "\
Usage: factor [OPTION] [NUMBER]...
Print the prime factors of each specified integer NUMBER.  If none
are specified on the command line, read them from standard input.

  -h, --exponents   print repeated factors in form p^e unless e is 1
      --help        display this help and exit
      --version     output version information and exit
"
}

/// Upstream's `getopt_long` loop, `short_opts = "h"`.
///
/// # Errors
///
/// An unknown option -- which includes a negative number, `-5`, since that is
/// what `getopt` makes of it.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut exponents = false;
    let mut operands: Vec<OsString> = Vec::new();
    for item in FACTOR.parse(args, "h", LONG_OPTIONS) {
        match item? {
            Opt::Short(b'h', _) | Opt::Long("exponents", _) => exponents = true,
            // The developer trace; see the module documentation.
            Opt::Long("-debug", _) => {}
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => operands.push(x.clone()),
            // Unreachable: every name in the table is handled above.
            Opt::Long(other, _) => {
                return Err(FACTOR.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(FACTOR.invalid_option(c)),
        }
    }
    Ok(Request::Run {
        exponents,
        operands,
    })
}

// ------------------------------------------------------------------ parsing ---

/// What `print_factors` makes of one token.
#[derive(Debug, PartialEq, Eq)]
enum Number {
    /// Below 2^127: upstream's single-precision path.
    Small(u128),
    /// At or above it: upstream's GMP path.
    Large(Nat),
    Invalid,
}

/// `print_factors`' reading of INPUT: skip spaces and one `+`, then
/// `strto2uintmax` -- digits only, at least one.
fn parse_number(input: &[u8]) -> Number {
    let mut s = input;
    while let [b' ', rest @ ..] = s {
        s = rest;
    }
    if let [b'+', rest @ ..] = s {
        s = rest;
    }
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return Number::Invalid;
    }
    let mut value: u128 = 0;
    for &c in s {
        let digit = u128::from(c.wrapping_sub(b'0'));
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => value = v,
            None => return Number::Large(Nat::from_decimal(s)),
        }
    }
    // "checks that the most significant bit of the two-word number is clear".
    if value >> 127 == 0 {
        Number::Small(value)
    } else {
        Number::Large(Nat::from_decimal(s))
    }
}

// ------------------------------------------------------- the small primes ---

/// The primes below 1000, for trial division before anything cleverer.
fn small_primes() -> Vec<u32> {
    let limit = 1000usize;
    let mut composite = vec![false; limit];
    let mut out = Vec::new();
    for n in 2..limit {
        if composite.get(n).copied().unwrap_or(true) {
            continue;
        }
        out.push(u32::try_from(n).unwrap_or(2));
        let mut m = n.saturating_mul(n);
        while m < limit {
            if let Some(slot) = composite.get_mut(m) {
                *slot = true;
            }
            m = m.saturating_add(n);
        }
    }
    out
}

// ---------------------------------------------- u128, in Montgomery form ---

/// `a * b` as a 256-bit `(high, low)` pair.
fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    const LO: u128 = (1 << 64) - 1;
    let (a1, a0) = (a >> 64, a & LO);
    let (b1, b0) = (b >> 64, b & LO);
    // Each partial product is below 2^128, and each sum below is bounded by
    // the column it adds -- the wrapping is spelled out, never exercised.
    let p00 = a0.wrapping_mul(b0);
    let p01 = a0.wrapping_mul(b1);
    let p10 = a1.wrapping_mul(b0);
    let p11 = a1.wrapping_mul(b1);
    let mid = (p00 >> 64).wrapping_add(p01 & LO).wrapping_add(p10 & LO);
    let lo = (p00 & LO) | (mid << 64);
    let hi = p11
        .wrapping_add(p01 >> 64)
        .wrapping_add(p10 >> 64)
        .wrapping_add(mid >> 64);
    (hi, lo)
}

/// Arithmetic modulo an odd `n` below 2^127, in Montgomery form with
/// R = 2^128. The bound on `n` is what keeps every intermediate in a `u128`:
/// a reduction's sum is below 2n.
struct Mont {
    n: u128,
    /// `-n^-1 mod 2^128`.
    ninv: u128,
    /// `R^2 mod n`, for converting in.
    r2: u128,
}

impl Mont {
    // Every `%` and `/` below divides by a modulus that is odd and at least 3
    // (only such a `Mont` is ever built) or by a value just tested nonzero.
    #[allow(clippy::arithmetic_side_effects)]
    fn new(n: u128) -> Self {
        // Newton's iteration for the inverse modulo 2^128: an odd `n` is its
        // own inverse modulo 8, and each step doubles the correct bits.
        let mut x = n;
        for _ in 0..6 {
            x = x.wrapping_mul(2u128.wrapping_sub(n.wrapping_mul(x)));
        }
        // R mod n, then doubled 128 times: R^2 mod n, with no product wider
        // than 2n < 2^128.
        let mut r2 = u128::MAX.wrapping_rem(n).wrapping_add(1).wrapping_rem(n);
        for _ in 0..128 {
            r2 <<= 1;
            if r2 >= n {
                r2 = r2.wrapping_sub(n);
            }
        }
        Mont {
            n,
            ninv: x.wrapping_neg(),
            r2,
        }
    }

    /// REDC of a 256-bit value below `n * R`.
    fn redc(&self, hi: u128, lo: u128) -> u128 {
        let m = lo.wrapping_mul(self.ninv);
        let (mh, _) = mul_wide(m, self.n);
        // `lo + m*n` is 0 modulo 2^128 by construction, so it carries exactly
        // when `lo` is not zero.
        let t = hi.wrapping_add(mh).wrapping_add(u128::from(lo != 0));
        if t >= self.n {
            t.wrapping_sub(self.n)
        } else {
            t
        }
    }

    fn mul(&self, a: u128, b: u128) -> u128 {
        let (hi, lo) = mul_wide(a, b);
        self.redc(hi, lo)
    }

    // Every `%` and `/` below divides by a modulus that is odd and at least 3
    // (only such a `Mont` is ever built) or by a value just tested nonzero.
    #[allow(clippy::arithmetic_side_effects)]
    fn to(&self, a: u128) -> u128 {
        self.mul(a.wrapping_rem(self.n), self.r2)
    }

    fn from(&self, a: u128) -> u128 {
        self.redc(0, a)
    }

    fn add(&self, a: u128, b: u128) -> u128 {
        // Both below n < 2^127: the sum fits.
        let s = a.wrapping_add(b);
        if s >= self.n {
            s.wrapping_sub(self.n)
        } else {
            s
        }
    }

    fn sub(&self, a: u128, b: u128) -> u128 {
        if a >= b {
            a.wrapping_sub(b)
        } else {
            a.wrapping_add(self.n).wrapping_sub(b)
        }
    }

    fn pow(&self, base: u128, mut exp: u128) -> u128 {
        let mut acc = self.to(1);
        let mut b = base;
        while exp > 0 {
            if exp & 1 == 1 {
                acc = self.mul(acc, b);
            }
            b = self.mul(b, b);
            exp >>= 1;
        }
        acc
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while let Some(divisor) = NonZeroU128::new(b) {
        let t = a % divisor;
        a = b;
        b = t;
    }
    a
}

/// A strong probable prime to `base`, for an odd `n > 2`.
fn strong_probable_prime(m: &Mont, base: u128) -> bool {
    let n = m.n;
    let n1 = n.wrapping_sub(1);
    let s = n1.trailing_zeros();
    let d = n1 >> s;
    let one = m.to(1);
    let minus_one = m.to(n1);
    let mut x = m.pow(m.to(base), d);
    if x == one || x == minus_one {
        return true;
    }
    for _ in 1..s {
        x = m.mul(x, x);
        if x == minus_one {
            return true;
        }
        if x == one {
            return false;
        }
    }
    false
}

/// The Jacobi symbol (a/n) for an odd positive `n`, with `a` given as a
/// possibly negative `i128`.
// `n` is odd and positive on entry, and each swap hands it the `a` the loop
// has just found nonzero: no `%` here can divide by zero.
#[allow(clippy::arithmetic_side_effects)]
fn jacobi(a: i128, n: u128) -> i32 {
    let mut a = if a < 0 {
        // n - (|a| mod n)
        let m = a.unsigned_abs().wrapping_rem(n);
        if m == 0 { 0 } else { n.wrapping_sub(m) }
    } else {
        a.unsigned_abs().wrapping_rem(n)
    };
    let mut n = n;
    let mut result = 1i32;
    while a != 0 {
        while a % 2 == 0 {
            a /= 2;
            let r = n % 8;
            if r == 3 || r == 5 {
                result = result.wrapping_neg();
            }
        }
        std::mem::swap(&mut a, &mut n);
        if a % 4 == 3 && n % 4 == 3 {
            result = result.wrapping_neg();
        }
        a = a.wrapping_rem(n);
    }
    if n == 1 { result } else { 0 }
}

/// Is `n` a perfect square? Its root, by Newton's method on `u128`.
// Newton's iterate is at least 1 throughout, and at most 2^64, so neither the
// division nor the sum can fault.
#[allow(clippy::arithmetic_side_effects)]
fn is_square(n: u128) -> bool {
    if n < 2 {
        return true;
    }
    let mut x = 1u128 << (n.ilog2() >> 1).wrapping_add(1);
    loop {
        let y = x.wrapping_add(n.wrapping_div(x)) >> 1;
        if y >= x {
            break;
        }
        x = y;
    }
    x.wrapping_mul(x) == n
}

/// The strong Lucas probable-prime test with Selfridge's parameters, for an
/// odd `n > 2` that is not a perfect square.
// Reductions are modulo `m.n`, odd and at least 3; `D` stays small (it is
// abandoned past a million).
#[allow(clippy::arithmetic_side_effects)]
fn strong_lucas(m: &Mont) -> bool {
    let n = m.n;
    // D = 5, -7, 9, -11, ... until (D/n) = -1.
    let mut d: i128 = 5;
    loop {
        let j = jacobi(d, n);
        if j == -1 {
            break;
        }
        if j == 0 && !d.unsigned_abs().is_multiple_of(n) {
            // A factor of n: not prime.
            return false;
        }
        d = if d > 0 {
            d.wrapping_add(2).wrapping_neg()
        } else {
            d.wrapping_sub(2).wrapping_neg()
        };
        if d.unsigned_abs() > 1_000_000 {
            // Unreachable for a non-square n; refuse rather than loop.
            return false;
        }
    }
    let p: u128 = 1;
    // Q = (1 - D) / 4, reduced into [0, n).
    // Exact: D is 1 modulo 4 by construction.
    let q_signed = 1i128.wrapping_sub(d).wrapping_div(4);
    let reduce = |v: i128| -> u128 {
        if v < 0 {
            let r = v.unsigned_abs().wrapping_rem(n);
            if r == 0 { 0 } else { n.wrapping_sub(r) }
        } else {
            v.unsigned_abs().wrapping_rem(n)
        }
    };
    let q = m.to(reduce(q_signed));
    let dm = m.to(reduce(d));
    let n1 = n.wrapping_add(1); // n < 2^127, so n + 1 fits
    let s = n1.trailing_zeros();
    let k = n1 >> s;

    // Binary Lucas chain for U_k, V_k, Q^k, all in Montgomery form.
    let half = |x: u128| -> u128 {
        // x / 2 mod n, for x in Montgomery form (division by 2 commutes).
        if x & 1 == 0 {
            x >> 1
        } else {
            (x >> 1).wrapping_add((n >> 1).wrapping_add(1))
        }
    };
    let mut u = m.to(1);
    let mut v = m.to(p);
    let mut qk = q;
    let bits = 128u32.wrapping_sub(k.leading_zeros());
    for i in (0..bits.saturating_sub(1)).rev() {
        // Doubling: U_2j = U_j V_j, V_2j = V_j^2 - 2 Q^j.
        u = m.mul(u, v);
        v = m.sub(m.mul(v, v), m.add(qk, qk));
        qk = m.mul(qk, qk);
        if (k >> i) & 1 == 1 {
            // Add one: U_{j+1} = (P U_j + V_j)/2, V_{j+1} = (D U_j + P V_j)/2.
            let pu = u; // P = 1
            let new_u = half(m.add(pu, v));
            let new_v = half(m.add(m.mul(dm, u), v));
            u = new_u;
            v = new_v;
            qk = m.mul(qk, q);
        }
    }
    let zero = 0u128;
    if u == zero || v == zero {
        return true;
    }
    for _ in 1..s {
        v = m.sub(m.mul(v, v), m.add(qk, qk));
        if v == zero {
            return true;
        }
        qk = m.mul(qk, qk);
    }
    false
}

/// Deterministic Miller-Rabin bases below 3.3e24 (2^81).
const MR_BASES: [u128; 13] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41];

fn is_prime_small(n: u128, primes: &[u32]) -> bool {
    if n < 2 {
        return false;
    }
    for &p in primes {
        let p = u128::from(p);
        if n == p {
            return true;
        }
        if n.is_multiple_of(p) {
            return false;
        }
    }
    // Every n that reaches here is odd and has no factor below 1000.
    if n < 1_000_000 {
        return true;
    }
    let m = Mont::new(n);
    for &base in &MR_BASES {
        if !strong_probable_prime(&m, base) {
            return false;
        }
    }
    if n < 3_317_044_064_679_887_385_961_981 {
        return true;
    }
    !is_square(n) && strong_lucas(&m)
}

/// Pollard-Brent rho: a non-trivial divisor of an odd composite `n`.
fn rho(n: u128) -> u128 {
    let m = Mont::new(n);
    let mut c: u128 = 1;
    loop {
        let cm = m.to(c);
        let f = |x: u128| m.add(m.mul(x, x), cm);
        let mut y = m.to(2);
        let mut r: u64 = 1;
        let mut q = m.to(1);
        let mut g = 1u128;
        let mut x = y;
        let mut ys = y;
        while g == 1 {
            x = y;
            for _ in 0..r {
                y = f(y);
            }
            let mut k: u64 = 0;
            while k < r && g == 1 {
                ys = y;
                let batch = 128u64.min(r.saturating_sub(k));
                for _ in 0..batch {
                    y = f(y);
                    q = m.mul(q, m.sub(x.max(y), x.min(y)));
                }
                g = gcd(m.from(q), n);
                k = k.saturating_add(batch);
            }
            r = r.saturating_mul(2);
        }
        if g == n {
            // The batch overshot: step one at a time from its start.
            loop {
                ys = f(ys);
                g = gcd(m.from(m.sub(x.max(ys), x.min(ys))), n);
                if g != 1 {
                    break;
                }
            }
        }
        if g != n && g != 1 {
            return g;
        }
        c = c.wrapping_add(1);
    }
}

/// The prime factors of `n` (below 2^127), ascending.
// Divides only by primes, and by divisors `rho` returns, which are above 1.
#[allow(clippy::arithmetic_side_effects)]
fn factor_small(n: u128, primes: &[u32]) -> Vec<u128> {
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    let mut m = n;
    for &p in primes {
        let p = u128::from(p);
        if p.saturating_mul(p) > m {
            break;
        }
        while m.is_multiple_of(p) {
            out.push(p);
            m = m.wrapping_div(p);
        }
    }
    let mut stack = vec![m];
    while let Some(x) = stack.pop() {
        if x == 1 {
            continue;
        }
        if is_prime_small(x, primes) {
            out.push(x);
            continue;
        }
        let d = rho(x);
        stack.push(d);
        stack.push(x.wrapping_div(d));
    }
    out.sort_unstable();
    out
}

// ----------------------------------------------------------- the Nat path ---

fn nat(v: u64) -> Nat {
    Nat::from_u64(v)
}

fn nat_mod(a: &Nat, n: &Nat) -> Nat {
    a.divmod(n).1
}

fn nat_mulmod(a: &Nat, b: &Nat, n: &Nat) -> Nat {
    nat_mod(&a.mul(b), n)
}

fn nat_powmod(base: &Nat, exp: &Nat, n: &Nat) -> Nat {
    let mut acc = nat(1);
    let mut b = nat_mod(base, n);
    for i in 0..exp.bit_len() {
        if exp.bit(i) {
            acc = nat_mulmod(&acc, &b, n);
        }
        b = nat_mulmod(&b, &b, n);
    }
    acc
}

fn nat_gcd(a: &Nat, b: &Nat) -> Nat {
    let (mut a, mut b) = (a.clone(), b.clone());
    while !b.is_zero() {
        let r = nat_mod(&a, &b);
        a = b;
        b = r;
    }
    a
}

fn nat_absdiff(a: &Nat, b: &Nat) -> Nat {
    if a.cmp(b) == Ordering::Less {
        b.sub(a)
    } else {
        a.sub(b)
    }
}

/// Miller-Rabin to the first 25 prime bases, for an odd `n` with no factor
/// below 1000 -- what upstream's GMP path settles for too, in spirit: a
/// probable-prime verdict whose error bound is below 4^-25.
fn is_prime_nat(n: &Nat) -> bool {
    let one = nat(1);
    let n1 = n.sub(&one);
    let mut s = 0usize;
    while !n1.bit(s) {
        s = s.saturating_add(1);
    }
    let d = n1.shr(s);
    'bases: for &base in &[
        2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83,
        89, 97,
    ] {
        let mut x = nat_powmod(&nat(base), &d, n);
        if x == one || x == n1 {
            continue;
        }
        for _ in 1..s {
            x = nat_mulmod(&x, &x, n);
            if x == n1 {
                continue 'bases;
            }
            if x == one {
                return false;
            }
        }
        return false;
    }
    true
}

fn rho_nat(n: &Nat) -> Nat {
    let mut c: u64 = 1;
    loop {
        let cn = nat(c);
        let f = |x: &Nat| nat_mod(&x.mul(x).add(&cn), n);
        let mut x = nat(2);
        let mut y = nat(2);
        let mut g = nat(1);
        let one = nat(1);
        while g == one {
            x = f(&x);
            y = f(&f(&y));
            g = nat_gcd(&nat_absdiff(&x, &y), n);
        }
        if g != *n {
            return g;
        }
        c = c.saturating_add(1);
    }
}

/// The prime factors of a `Nat` (at least 2^127), ascending.
fn factor_nat(n: &Nat, primes: &[u32]) -> Vec<Nat> {
    let mut out: Vec<Nat> = Vec::new();
    let mut m = n.clone();
    for &p in primes {
        let Some(pz) = NonZeroU32::new(p) else {
            continue;
        };
        loop {
            let (q, r) = m.divmod_small(pz);
            if r != 0 {
                break;
            }
            out.push(nat(u64::from(p)));
            m = q;
        }
    }
    let mut stack = vec![m];
    while let Some(x) = stack.pop() {
        if x == nat(1) {
            continue;
        }
        // Small enough for the fast path now?
        if x.bit_len() < 127 {
            let v = nat_to_u128(&x);
            out.extend(factor_small(v, primes).into_iter().map(u128_to_nat));
            continue;
        }
        if is_prime_nat(&x) {
            out.push(x);
            continue;
        }
        let d = rho_nat(&x);
        let (q, _) = x.divmod(&d);
        stack.push(d);
        stack.push(q);
    }
    out.sort();
    out
}

fn nat_to_u128(n: &Nat) -> u128 {
    let lo = u128::from(n.low_u64());
    let hi = u128::from(n.shr(64).low_u64());
    (hi << 64) | lo
}

fn u128_to_nat(v: u128) -> Nat {
    let hi = u64::try_from(v >> 64).unwrap_or(0);
    let lo = u64::try_from(v & u128::from(u64::MAX)).unwrap_or(0);
    nat(hi).shl(64).add(&nat(lo))
}

// ------------------------------------------------------------------ output ---

/// `p p p q` as upstream writes it, or `p^3 q` with `-h`.
fn render_factors<T: PartialEq>(
    factors: &[T],
    exponents: bool,
    show: impl Fn(&T) -> Vec<u8>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(p) = factors.get(i) {
        let mut e = 1usize;
        while factors.get(i.saturating_add(e)) == Some(p) {
            e = e.saturating_add(1);
        }
        if exponents && e > 1 {
            out.push(b' ');
            out.extend_from_slice(&show(p));
            out.extend_from_slice(format!("^{e}").as_bytes());
        } else {
            for _ in 0..e {
                out.push(b' ');
                out.extend_from_slice(&show(p));
            }
        }
        i = i.saturating_add(e);
    }
    out
}

/// Upstream's `lbuf`: whole lines, written with `full_write` at line ends
/// once 512 bytes are held (every line, when either end is a terminal) and at
/// exit.
struct Lbuf {
    buf: Vec<u8>,
    line_buffered: bool,
}

/// `FACTOR_PIPE_BUF`: the most a pipe reader is guaranteed to get atomically.
const PIPE_BUF: usize = 512;

impl Lbuf {
    /// Buffer one whole line. `Err` is a flush that failed, with how much of
    /// the buffer it was writing -- upstream's exit handler writes the same
    /// bytes again.
    fn line(&mut self, bytes: &[u8]) -> Result<(), (io::Error, usize)> {
        self.buf.extend_from_slice(bytes);
        if self.line_buffered {
            let all = self.buf.len();
            return self.flush_upto(all);
        }
        if self.buf.len() >= PIPE_BUF {
            // The last line end within the first 512 bytes.
            let cut = self
                .buf
                .get(..PIPE_BUF)
                .and_then(|head| head.iter().rposition(|&b| b == b'\n'))
                .map_or(self.buf.len(), |p| p.saturating_add(1));
            return self.flush_upto(cut);
        }
        Ok(())
    }

    fn flush_upto(&mut self, cut: usize) -> Result<(), (io::Error, usize)> {
        let head = self.buf.get(..cut).unwrap_or_default();
        match stdfd::write_all(1, head) {
            Ok(()) => {
                self.buf.drain(..cut);
                Ok(())
            }
            Err(e) => Err((e, cut)),
        }
    }
}

/// Everything the run does, `atexit` handlers included, from option parsing
/// to status.
fn run() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (exponents, operands) = match parse_args(&args) {
        Ok(Request::Run {
            exponents,
            operands,
        }) => (exponents, operands),
        Ok(Request::Help) => {
            let mut out = Stream::stdout();
            // Deliberately unread: `Stream` records a failure and
            // `close_stdout` reports it.
            let _ = out.write_all(help_text().as_bytes());
            return stdfd::close_stdout("factor", out, ExitCode::SUCCESS);
        }
        Ok(Request::Version) => {
            let mut out = Stream::stdout();
            // Deliberately unread, as above.
            let _ = out.write_all(b"factor (SlateOS coreutils) 0.1.0\n");
            return stdfd::close_stdout("factor", out, ExitCode::SUCCESS);
        }
        Err(e) => {
            FACTOR.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let primes = small_primes();
    let mut lbuf = Lbuf {
        buf: Vec::with_capacity(PIPE_BUF.saturating_mul(2)),
        line_buffered: io::stdin().is_terminal() || io::stdout().is_terminal(),
    };
    // Upstream's stdio `stdout`, which only the large path writes through.
    let mut stdio = Stream::stdout();
    let mut ok = true;

    let mut each =
        |token: &[u8], lbuf: &mut Lbuf, stdio: &mut Stream| -> Result<(), (io::Error, usize)> {
            match parse_number(token) {
                Number::Invalid => {
                    coreutils::diag!("factor: {} is not a valid positive integer", quote(token));
                    ok = false;
                    Ok(())
                }
                Number::Small(n) => {
                    let factors = factor_small(n, &primes);
                    let mut line = format!("{n}:").into_bytes();
                    line.extend(render_factors(&factors, exponents, |p| {
                        p.to_string().into_bytes()
                    }));
                    line.push(b'\n');
                    lbuf.line(&line)
                }
                Number::Large(n) => {
                    let factors = factor_nat(&n, &primes);
                    let mut line = n.to_decimal();
                    line.push(b':');
                    line.extend(render_factors(&factors, exponents, Nat::to_decimal));
                    line.push(b'\n');
                    // Deliberately unread: a failure is `stdio`'s to report at the
                    // close, as stdio's is upstream.
                    let _ = stdio.write_all(&line);
                    let _ = stdio.flush();
                    Ok(())
                }
            }
        };

    let fed: Result<(), Stop> = if operands.is_empty() {
        read_tokens(|t| each(t, &mut lbuf, &mut stdio))
    } else {
        operands
            .iter()
            .try_for_each(|o| each(&os_bytes(o), &mut lbuf, &mut stdio))
            .map_err(Stop::from)
    };
    let mut status = if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    match fed {
        Ok(()) => {}
        // `error (EXIT_FAILURE, errno, ...)`: the message, then the same exit
        // handlers as a normal end.
        Err(Stop::Read(e)) => {
            coreutils::diag!(
                "factor: error reading input: {}",
                coreutils::errmsg::strerror(&e)
            );
            status = ExitCode::FAILURE;
        }
        Err(Stop::Write(e, cut)) => {
            // `write_error ()`: the message, stdio purged and cleared so its
            // own close stays quiet, `exit (EXIT_FAILURE)` -- whose handler
            // flushes the same bytes again, and says so again if it fails.
            stdfd::write_error("factor", &e);
            stdio.abandon();
            if let Err((e, _)) = lbuf.flush_upto(cut) {
                stdfd::write_error("factor", &e);
            }
            return stdfd::close_stdout("factor", stdio, ExitCode::FAILURE);
        }
    }
    // The exit handlers, `lbuf_flush` first (registered last).
    let all = lbuf.buf.len();
    if let Err((e, _)) = lbuf.flush_upto(all) {
        stdfd::write_error("factor", &e);
        stdio.abandon();
        return stdfd::close_stdout("factor", stdio, ExitCode::FAILURE);
    }
    stdfd::close_stdout("factor", stdio, status)
}

/// Why the run stopped early.
enum Stop {
    /// A line-buffer flush failed: the error, and how much it was writing.
    Write(io::Error, usize),
    /// Standard input failed: `error reading input`.
    Read(io::Error),
}

impl From<(io::Error, usize)> for Stop {
    fn from((e, cut): (io::Error, usize)) -> Self {
        Stop::Write(e, cut)
    }
}

/// `do_stdin`: tokens separated by any of newline, tab and space.
fn read_tokens(mut each: impl FnMut(&[u8]) -> Result<(), (io::Error, usize)>) -> Result<(), Stop> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut token: Vec<u8> = Vec::new();
    loop {
        let buf = match input.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            // Upstream's `readtoken` gives up at the error without handing
            // back a partial token.
            Err(e) => return Err(Stop::Read(e)),
        };
        if buf.is_empty() {
            break;
        }
        let len = buf.len();
        for &b in buf {
            if b == b'\n' || b == b'\t' || b == b' ' {
                if !token.is_empty() {
                    each(&token)?;
                    token.clear();
                }
            } else {
                token.push(b);
            }
        }
        input.consume(len);
    }
    if !token.is_empty() {
        each(&token)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    stdfd::close_stderr(run(), 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn small(n: u128) -> Vec<u128> {
        factor_small(n, &small_primes())
    }

    #[test]
    fn parsing_as_upstream_reads() {
        assert_eq!(parse_number(b"12"), Number::Small(12));
        assert_eq!(parse_number(b"  +0012"), Number::Small(12));
        assert_eq!(parse_number(b"\t12"), Number::Invalid);
        assert_eq!(parse_number(b"++1"), Number::Invalid);
        assert_eq!(parse_number(b"12x"), Number::Invalid);
        assert_eq!(parse_number(b""), Number::Invalid);
        assert_eq!(parse_number(b"-5"), Number::Invalid);
        // 2^127 - 1 stays small; 2^127 goes large.
        assert_eq!(
            parse_number(b"170141183460469231731687303715884105727"),
            Number::Small((1u128 << 127) - 1)
        );
        assert!(matches!(
            parse_number(b"170141183460469231731687303715884105728"),
            Number::Large(_)
        ));
    }

    #[test]
    fn small_factorisations() {
        assert_eq!(small(0), Vec::<u128>::new());
        assert_eq!(small(1), Vec::<u128>::new());
        assert_eq!(small(360), vec![2, 2, 2, 3, 3, 5]);
        assert_eq!(small(997), vec![997]);
        assert_eq!(small(1_000_003 * 999_983), vec![999_983, 1_000_003]);
        assert_eq!(
            small(u128::from(u64::MAX)),
            vec![3, 5, 17, 257, 641, 65537, 6_700_417]
        );
        // 2^127 - 1, a Mersenne prime.
        assert_eq!(small((1u128 << 127) - 1), vec![(1u128 << 127) - 1]);
        // A semiprime of two 40-bit primes: rho's real work, about a
        // million steps.
        let p = 1_099_511_627_689u128;
        let q = 1_099_511_627_609u128;
        assert_eq!(small(p * q), vec![q, p]);
    }

    #[test]
    fn the_nat_path() {
        let primes = small_primes();
        // 2^128 + 3 = 3 * ...; checked against GNU: 340282366920938463463374607431768211459.
        let n = Nat::from_decimal(b"340282366920938463463374607431768211456");
        let f = factor_nat(&n, &primes);
        assert_eq!(f.len(), 128);
        assert!(f.iter().all(|p| *p == nat(2)));
        let n = Nat::from_decimal(b"170141183460469231731687303715884105729");
        let f: Vec<Vec<u8>> = factor_nat(&n, &primes)
            .iter()
            .map(Nat::to_decimal)
            .collect();
        assert_eq!(
            f,
            vec![
                b"3".to_vec(),
                b"56713727820156410577229101238628035243".to_vec()
            ]
        );
    }

    #[test]
    fn exponents() {
        let f = small(360);
        assert_eq!(
            render_factors(&f, false, |p| p.to_string().into_bytes()),
            b" 2 2 2 3 3 5"
        );
        assert_eq!(
            render_factors(&f, true, |p| p.to_string().into_bytes()),
            b" 2^3 3^2 5"
        );
    }

    #[test]
    fn lucas_agrees_with_trial_division() {
        // Every odd non-square below 20000 with no factor under 1000 is prime
        // exactly when the strong Lucas test says so, on this range.
        let primes = small_primes();
        for n in (1_000_001u128..1_020_000).step_by(2) {
            if is_square(n) {
                continue;
            }
            let truth = factor_small(n, &primes) == vec![n];
            let m = Mont::new(n);
            if truth {
                assert!(strong_lucas(&m), "{n} is prime");
            }
        }
    }

    #[test]
    fn command_lines() {
        let a = |v: &[&str]| parse_args(&v.iter().map(OsString::from).collect::<Vec<_>>());
        assert_eq!(
            a(&["-h", "12"]).unwrap(),
            Request::Run {
                exponents: true,
                operands: vec![OsString::from("12")]
            }
        );
        assert!(a(&["-5"]).is_err());
        assert_eq!(a(&["--help", "-x"]).unwrap(), Request::Help);
    }
}
