//! bc — arbitrary-precision calculator language.
//!
//! Usage: bc [-q] [-l] [FILE...]
//!   -q          quiet: suppress the welcome banner
//!   -l          load the math library (defines extra scale and functions)
//!   FILE...     read and execute commands from files before interactive mode
//!
//! Supports:
//!   - Arbitrary-precision integer and fixed-point decimal arithmetic
//!   - Operators: + - * / % ^ (power)
//!   - Comparison: == != < > <= >=
//!   - Assignment: = += -= *= /= %= ^=
//!   - Increment/decrement: ++ --
//!   - Variables: single lowercase letters and multi-character names
//!   - Special variables: scale, ibase, obase, last (or .)
//!   - Control flow: if (cond) { ... } else { ... }
//!   - Loops: while (cond) { ... }
//!   - Built-in functions: sqrt(x), length(x), scale(x), print expr
//!   - Line continuation with backslash
//!   - Comments: /* ... */ and # to end of line
//!   - quit command
//!
//! Interactive mode: reads expressions from stdin when no files are given
//! or after all files have been processed.
//!
//! Exit codes:
//!   0  normal exit
//!   1  error

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ---------------------------------------------------------------------------
// Arbitrary-precision decimal number
// ---------------------------------------------------------------------------

/// A decimal number represented as an arbitrary-precision integer with a
/// fixed-point scale (number of digits after the decimal point).
#[derive(Clone, Debug)]
struct BigDecimal {
    /// The unscaled integer value. The actual value is `digits * 10^(-scale)`.
    /// Stored as a big-endian vector of digits (base 10^9 limbs) plus a sign.
    negative: bool,
    /// Limbs in little-endian order (least significant first), base 10^9.
    limbs: Vec<u64>,
    /// Number of decimal digits after the point.
    scale: usize,
}

const LIMB_BASE: u64 = 1_000_000_000;
const LIMB_DIGITS: usize = 9;

impl BigDecimal {
    fn zero() -> Self {
        BigDecimal {
            negative: false,
            limbs: vec![0],
            scale: 0,
        }
    }

    fn one() -> Self {
        BigDecimal {
            negative: false,
            limbs: vec![1],
            scale: 0,
        }
    }

    fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&l| l == 0)
    }

    /// Parse a decimal string like "123.456" or "-78".
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        let (negative, s) = if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('+') {
            (false, rest)
        } else {
            (false, s)
        };

        // Split into integer and fractional parts.
        let (int_part, frac_part) = if let Some((i, f)) = s.split_once('.') {
            (i, f)
        } else {
            (s, "")
        };

        // Validate: all characters must be digits (or empty).
        if !int_part.chars().all(|c| c.is_ascii_digit()) && !int_part.is_empty() {
            return None;
        }
        if !frac_part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }

        let scale = frac_part.len();

        // Concatenate integer and fractional parts into one digit string.
        let full_digits = format!("{int_part}{frac_part}");
        let full_digits = full_digits.trim_start_matches('0');

        if full_digits.is_empty() {
            return Some(BigDecimal {
                negative: false,
                limbs: vec![0],
                scale,
            });
        }

        // Convert digit string to limbs (base 10^9, little-endian).
        let limbs = digits_to_limbs(full_digits);

        Some(BigDecimal {
            negative,
            limbs,
            scale,
        })
    }

    /// Format as a decimal string.
    fn to_string_with_scale(&self, display_scale: usize) -> String {
        // First, get the full digit string from limbs.
        let digit_str = limbs_to_digits(&self.limbs);

        // The digit string represents the unscaled value.
        // We need to insert a decimal point `self.scale` digits from the right.
        let total_digits = digit_str.len();

        let (int_part, frac_part) = if self.scale == 0 {
            (digit_str.as_str(), "")
        } else if total_digits > self.scale {
            let split = total_digits - self.scale;
            (&digit_str[..split], &digit_str[split..])
        } else {
            // Need leading zeros in fractional part.
            let zeros = self.scale - total_digits;
            let padded = format!("{}{}", "0".repeat(zeros), digit_str);
            // We need to return owned strings, so handle differently.
            return format!(
                "{}0.{}",
                if self.negative { "-" } else { "" },
                truncate_or_pad(&padded, display_scale)
            );
        };

        let int_display = if int_part.is_empty() { "0" } else { int_part };

        if display_scale == 0 {
            format!(
                "{}{}",
                if self.negative && !self.is_zero() {
                    "-"
                } else {
                    ""
                },
                int_display
            )
        } else {
            format!(
                "{}{}.{}",
                if self.negative && !self.is_zero() {
                    "-"
                } else {
                    ""
                },
                int_display,
                truncate_or_pad(frac_part, display_scale)
            )
        }
    }

    /// Normalize: remove leading zero limbs (but keep at least one).
    fn normalize(&mut self) {
        while self.limbs.len() > 1 && self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        if self.is_zero() {
            self.negative = false;
        }
    }

    /// Compare absolute values. Returns Ordering.
    fn cmp_abs(&self, other: &Self) -> std::cmp::Ordering {
        // Equalize scales first.
        let (a, b) = equalize_scales(self, other);
        cmp_limbs(&a.limbs, &b.limbs)
    }

    fn negate(&self) -> Self {
        let mut r = self.clone();
        if !r.is_zero() {
            r.negative = !r.negative;
        }
        r
    }
}

fn truncate_or_pad(frac: &str, target_len: usize) -> String {
    if frac.len() >= target_len {
        frac[..target_len].to_string()
    } else {
        format!("{}{}", frac, "0".repeat(target_len - frac.len()))
    }
}

fn digits_to_limbs(digits: &str) -> Vec<u64> {
    let mut limbs = Vec::new();
    let bytes = digits.as_bytes();
    let len = bytes.len();

    // Process from right to left in chunks of LIMB_DIGITS.
    let mut end = len;
    while end > 0 {
        let start = end.saturating_sub(LIMB_DIGITS);
        let chunk = &digits[start..end];
        let val: u64 = chunk.parse().unwrap_or(0);
        limbs.push(val);
        end = start;
    }

    if limbs.is_empty() {
        limbs.push(0);
    }
    limbs
}

fn limbs_to_digits(limbs: &[u64]) -> String {
    if limbs.is_empty() || (limbs.len() == 1 && limbs[0] == 0) {
        return "0".to_string();
    }

    // Build from most significant limb.
    let mut result = String::new();
    let mut first = true;
    for &limb in limbs.iter().rev() {
        if first {
            if limb > 0 {
                result.push_str(&limb.to_string());
                first = false;
            }
        } else {
            result.push_str(&format!("{limb:09}"));
        }
    }

    if result.is_empty() {
        "0".to_string()
    } else {
        result
    }
}

fn cmp_limbs(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let alen = effective_len(a);
    let blen = effective_len(b);

    if alen != blen {
        return alen.cmp(&blen);
    }

    // Compare from most significant.
    for i in (0..alen).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    std::cmp::Ordering::Equal
}

fn effective_len(limbs: &[u64]) -> usize {
    let mut len = limbs.len();
    while len > 1 && limbs[len - 1] == 0 {
        len -= 1;
    }
    len
}

/// Make two BigDecimals have the same scale by multiplying the one with
/// smaller scale by the appropriate power of 10.
fn equalize_scales(a: &BigDecimal, b: &BigDecimal) -> (BigDecimal, BigDecimal) {
    if a.scale == b.scale {
        return (a.clone(), b.clone());
    }
    if a.scale < b.scale {
        let diff = b.scale - a.scale;
        let mut a2 = a.clone();
        multiply_by_pow10(&mut a2.limbs, diff);
        a2.scale = b.scale;
        (a2, b.clone())
    } else {
        let diff = a.scale - b.scale;
        let mut b2 = b.clone();
        multiply_by_pow10(&mut b2.limbs, diff);
        b2.scale = a.scale;
        (a.clone(), b2)
    }
}

/// Multiply limbs by 10^n.
fn multiply_by_pow10(limbs: &mut Vec<u64>, n: usize) {
    if n == 0 {
        return;
    }

    // Full groups of LIMB_DIGITS shift as whole limb inserts.
    let full_limbs = n / LIMB_DIGITS;
    let remainder = n % LIMB_DIGITS;

    // Insert zero limbs at the low end.
    for _ in 0..full_limbs {
        limbs.insert(0, 0);
    }

    // Multiply all limbs by 10^remainder.
    if remainder > 0 {
        let factor = 10u64.pow(remainder as u32);
        let mut carry = 0u64;
        for limb in limbs.iter_mut() {
            let val = *limb as u128 * factor as u128 + carry as u128;
            *limb = (val % LIMB_BASE as u128) as u64;
            carry = (val / LIMB_BASE as u128) as u64;
        }
        if carry > 0 {
            limbs.push(carry);
        }
    }
}

// ---------------------------------------------------------------------------
// Arithmetic operations
// ---------------------------------------------------------------------------

fn add_abs(a: &[u64], b: &[u64]) -> Vec<u64> {
    let len = a.len().max(b.len());
    let mut result = Vec::with_capacity(len + 1);
    let mut carry = 0u64;

    for i in 0..len {
        let av = if i < a.len() { a[i] } else { 0 };
        let bv = if i < b.len() { b[i] } else { 0 };
        let sum = av + bv + carry;
        result.push(sum % LIMB_BASE);
        carry = sum / LIMB_BASE;
    }
    if carry > 0 {
        result.push(carry);
    }
    result
}

/// Subtract b from a (assumes a >= b in absolute value).
fn sub_abs(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut result = Vec::with_capacity(a.len());
    let mut borrow: i64 = 0;

    for i in 0..a.len() {
        let av = a[i] as i64;
        let bv = if i < b.len() { b[i] as i64 } else { 0 };
        let mut diff = av - bv - borrow;
        if diff < 0 {
            diff += LIMB_BASE as i64;
            borrow = 1;
        } else {
            borrow = 0;
        }
        result.push(diff as u64);
    }

    // Remove leading zeros.
    while result.len() > 1 && result.last() == Some(&0) {
        result.pop();
    }
    result
}

fn mul_abs(a: &[u64], b: &[u64]) -> Vec<u64> {
    if a.len() == 1 && a[0] == 0 || b.len() == 1 && b[0] == 0 {
        return vec![0];
    }

    let mut result = vec![0u64; a.len() + b.len()];

    for i in 0..a.len() {
        let mut carry = 0u128;
        for j in 0..b.len() {
            let prod = a[i] as u128 * b[j] as u128 + result[i + j] as u128 + carry;
            result[i + j] = (prod % LIMB_BASE as u128) as u64;
            carry = prod / LIMB_BASE as u128;
        }
        if carry > 0 {
            result[i + b.len()] += carry as u64;
        }
    }

    while result.len() > 1 && result.last() == Some(&0) {
        result.pop();
    }
    result
}

/// Divide a by b, returning (quotient_limbs, remainder_limbs).
/// Both a and b are treated as non-negative integers (limb arrays).
fn div_abs(a: &[u64], b: &[u64]) -> Option<(Vec<u64>, Vec<u64>)> {
    if b.iter().all(|&l| l == 0) {
        return None; // division by zero
    }

    if cmp_limbs(a, b) == std::cmp::Ordering::Less {
        return Some((vec![0], a.to_vec()));
    }

    // For simplicity, convert to a big decimal string representation,
    // do long division in base 10, then convert back.
    let a_digits = limbs_to_digits(a);
    let b_digits = limbs_to_digits(b);

    let (q, r) = long_division_decimal(&a_digits, &b_digits);

    let q_limbs = if q.is_empty() || q == "0" {
        vec![0]
    } else {
        digits_to_limbs(&q)
    };

    let r_limbs = if r.is_empty() || r == "0" {
        vec![0]
    } else {
        digits_to_limbs(&r)
    };

    Some((q_limbs, r_limbs))
}

/// Long division of two decimal digit strings, returning (quotient, remainder).
fn long_division_decimal(a: &str, b: &str) -> (String, String) {
    let b_val = decimal_str_to_u128(b);
    if b_val == 0 {
        return ("0".to_string(), a.to_string());
    }

    // If both fit in u128, use direct division.
    if a.len() <= 38 && b.len() <= 38 {
        let a_val = decimal_str_to_u128(a);
        return ((a_val / b_val).to_string(), (a_val % b_val).to_string());
    }

    // Schoolbook long division digit by digit.
    let a_bytes = a.as_bytes();
    let mut quotient = String::new();
    let mut remainder = String::new();

    for &digit in a_bytes {
        remainder.push(digit as char);
        // Remove leading zeros from remainder.
        let rem_trimmed = remainder.trim_start_matches('0');
        let rem_str = if rem_trimmed.is_empty() {
            "0"
        } else {
            rem_trimmed
        };

        // How many times does b go into remainder?
        let q_digit = if rem_str.len() < b.len()
            || (rem_str.len() == b.len() && rem_str < b)
        {
            0u8
        } else {
            // Use the leading digits of remainder to estimate.
            estimate_quotient_digit(rem_str, b)
        };

        quotient.push((b'0' + q_digit) as char);

        if q_digit > 0 {
            // remainder = remainder - q_digit * b
            let product = multiply_decimal_by_digit(b, q_digit);
            remainder = subtract_decimal(&remainder, &product);
        }
    }

    // Trim leading zeros.
    let q = quotient.trim_start_matches('0');
    let r = remainder.trim_start_matches('0');

    (
        if q.is_empty() { "0".to_string() } else { q.to_string() },
        if r.is_empty() { "0".to_string() } else { r.to_string() },
    )
}

fn decimal_str_to_u128(s: &str) -> u128 {
    s.parse::<u128>().unwrap_or(0)
}

fn estimate_quotient_digit(remainder: &str, divisor: &str) -> u8 {
    // Binary search for the digit 0..9.
    let mut lo = 0u8;
    let mut hi = 9u8;
    let mut result = 0u8;

    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let product = multiply_decimal_by_digit(divisor, mid);
        let product_trimmed = product.trim_start_matches('0');
        let product_str = if product_trimmed.is_empty() {
            "0"
        } else {
            product_trimmed
        };

        if compare_decimal(product_str, remainder) <= 0 {
            result = mid;
            if mid == 9 {
                break;
            }
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    result
}

fn compare_decimal(a: &str, b: &str) -> i32 {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    let a = if a.is_empty() { "0" } else { a };
    let b = if b.is_empty() { "0" } else { b };

    if a.len() != b.len() {
        return if a.len() < b.len() { -1 } else { 1 };
    }
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

fn multiply_decimal_by_digit(a: &str, digit: u8) -> String {
    if digit == 0 {
        return "0".to_string();
    }
    let a_bytes = a.as_bytes();
    let mut result = Vec::with_capacity(a_bytes.len() + 1);
    let mut carry = 0u32;

    for &b in a_bytes.iter().rev() {
        let d = (b - b'0') as u32;
        let prod = d * digit as u32 + carry;
        result.push((prod % 10) as u8 + b'0');
        carry = prod / 10;
    }
    if carry > 0 {
        result.push(carry as u8 + b'0');
    }

    result.reverse();
    String::from_utf8(result).unwrap_or_else(|_| "0".to_string())
}

fn subtract_decimal(a: &str, b: &str) -> String {
    let a_bytes: Vec<u8> = a.bytes().collect();
    let b_bytes: Vec<u8> = b.bytes().collect();

    let len = a_bytes.len().max(b_bytes.len());
    let mut result = Vec::with_capacity(len);
    let mut borrow: i32 = 0;

    for i in 0..len {
        let ad = if i < a_bytes.len() {
            (a_bytes[a_bytes.len() - 1 - i] - b'0') as i32
        } else {
            0
        };
        let bd = if i < b_bytes.len() {
            (b_bytes[b_bytes.len() - 1 - i] - b'0') as i32
        } else {
            0
        };
        let mut diff = ad - bd - borrow;
        if diff < 0 {
            diff += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        result.push(diff as u8 + b'0');
    }

    result.reverse();
    let s = String::from_utf8(result).unwrap_or_else(|_| "0".to_string());
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn bd_add(a: &BigDecimal, b: &BigDecimal) -> BigDecimal {
    let (a, b) = equalize_scales(a, b);
    let scale = a.scale;

    if a.negative == b.negative {
        let limbs = add_abs(&a.limbs, &b.limbs);
        let mut r = BigDecimal {
            negative: a.negative,
            limbs,
            scale,
        };
        r.normalize();
        r
    } else {
        match cmp_limbs(&a.limbs, &b.limbs) {
            std::cmp::Ordering::Less => {
                let limbs = sub_abs(&b.limbs, &a.limbs);
                let mut r = BigDecimal {
                    negative: b.negative,
                    limbs,
                    scale,
                };
                r.normalize();
                r
            }
            std::cmp::Ordering::Greater => {
                let limbs = sub_abs(&a.limbs, &b.limbs);
                let mut r = BigDecimal {
                    negative: a.negative,
                    limbs,
                    scale,
                };
                r.normalize();
                r
            }
            std::cmp::Ordering::Equal => BigDecimal {
                negative: false,
                limbs: vec![0],
                scale,
            },
        }
    }
}

fn bd_sub(a: &BigDecimal, b: &BigDecimal) -> BigDecimal {
    bd_add(a, &b.negate())
}

fn bd_mul(a: &BigDecimal, b: &BigDecimal, result_scale: usize) -> BigDecimal {
    let limbs = mul_abs(&a.limbs, &b.limbs);
    let raw_scale = a.scale + b.scale;
    let negative = a.negative != b.negative;

    let mut r = BigDecimal {
        negative,
        limbs,
        scale: raw_scale,
    };

    // Truncate to result_scale.
    if raw_scale > result_scale {
        let diff = raw_scale - result_scale;
        // Divide limbs by 10^diff to truncate.
        divide_by_pow10(&mut r.limbs, diff);
        r.scale = result_scale;
    }

    r.normalize();
    r
}

fn divide_by_pow10(limbs: &mut Vec<u64>, n: usize) {
    if n == 0 {
        return;
    }

    let full_limbs = n / LIMB_DIGITS;
    let remainder = n % LIMB_DIGITS;

    // Remove `full_limbs` least-significant limbs.
    for _ in 0..full_limbs {
        if limbs.len() > 1 {
            limbs.remove(0);
        } else {
            limbs[0] = 0;
            return;
        }
    }

    // Divide remaining by 10^remainder.
    if remainder > 0 {
        let divisor = 10u64.pow(remainder as u32);
        let mut carry = 0u64;
        for limb in limbs.iter_mut().rev() {
            let val = carry * LIMB_BASE + *limb;
            *limb = val / divisor;
            carry = val % divisor;
        }
    }

    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }
}

fn bd_div(a: &BigDecimal, b: &BigDecimal, scale: usize) -> Option<BigDecimal> {
    if b.is_zero() {
        return None;
    }

    // To compute a/b with `scale` decimal places, we compute
    // (a * 10^(scale + b.scale - a.scale + extra)) / b_unscaled,
    // then the result has `scale` decimal places.

    let extra = scale + b.scale;
    let total_scale = extra.saturating_sub(a.scale);

    let mut a_limbs = a.limbs.clone();
    multiply_by_pow10(&mut a_limbs, total_scale);

    let (q_limbs, _r_limbs) = div_abs(&a_limbs, &b.limbs)?;

    let negative = a.negative != b.negative;

    let mut r = BigDecimal {
        negative,
        limbs: q_limbs,
        scale,
    };
    r.normalize();
    Some(r)
}

fn bd_modulo(a: &BigDecimal, b: &BigDecimal, scale: usize) -> Option<BigDecimal> {
    // a % b = a - (a/b)*b, where a/b is truncated to integer.
    let q = bd_div(a, b, 0)?;
    let product = bd_mul(&q, b, scale);
    Some(bd_sub(a, &product))
}

fn bd_pow(base: &BigDecimal, exp: &BigDecimal, scale: usize) -> BigDecimal {
    // Only support integer exponents.
    // Convert exp to i64.
    let exp_str = exp.to_string_with_scale(0);
    let exp_val: i64 = exp_str.parse().unwrap_or(0);

    if exp_val == 0 {
        return BigDecimal::one();
    }

    let negative_exp = exp_val < 0;
    let exp_abs = exp_val.unsigned_abs();

    // Exponentiation by squaring.
    let mut result = BigDecimal::one();
    let mut base = base.clone();
    let mut e = exp_abs;

    while e > 0 {
        if e & 1 == 1 {
            result = bd_mul(&result, &base, scale);
        }
        base = bd_mul(&base, &base, scale);
        e >>= 1;
    }

    if negative_exp {
        bd_div(&BigDecimal::one(), &result, scale).unwrap_or_else(BigDecimal::zero)
    } else {
        result
    }
}

fn bd_sqrt(a: &BigDecimal, scale: usize) -> Option<BigDecimal> {
    if a.negative {
        return None; // sqrt of negative
    }
    if a.is_zero() {
        return Some(BigDecimal {
            negative: false,
            limbs: vec![0],
            scale,
        });
    }

    // Newton's method: x_{n+1} = (x_n + a/x_n) / 2
    // Start with a rough estimate.
    let two = BigDecimal {
        negative: false,
        limbs: vec![2],
        scale: 0,
    };

    // Initial guess: use f64 for a rough starting point.
    let a_str = a.to_string_with_scale(a.scale);
    let a_f64: f64 = a_str.parse().unwrap_or(1.0);
    let guess_f64 = a_f64.sqrt();
    let guess_str = format!("{guess_f64:.prec$}", prec = scale + 2);
    let mut x = BigDecimal::parse(&guess_str).unwrap_or_else(BigDecimal::one);

    let work_scale = scale + 4; // extra precision for convergence

    for _ in 0..100 {
        // x_new = (x + a/x) / 2
        let a_over_x = match bd_div(a, &x, work_scale) {
            Some(v) => v,
            None => break,
        };
        let sum = bd_add(&x, &a_over_x);
        let x_new = match bd_div(&sum, &two, work_scale) {
            Some(v) => v,
            None => break,
        };

        // Check convergence: if x_new == x at scale+2 digits, we're done.
        let x_str = x.to_string_with_scale(scale + 2);
        let xn_str = x_new.to_string_with_scale(scale + 2);
        if x_str == xn_str {
            x = x_new;
            break;
        }
        x = x_new;
    }

    // Truncate to requested scale.
    x.scale = work_scale;
    let result_str = x.to_string_with_scale(scale);
    Some(BigDecimal::parse(&result_str).unwrap_or_else(BigDecimal::zero))
}

fn bd_compare(a: &BigDecimal, b: &BigDecimal) -> std::cmp::Ordering {
    if a.negative && !b.negative {
        if a.is_zero() && b.is_zero() {
            return std::cmp::Ordering::Equal;
        }
        return std::cmp::Ordering::Less;
    }
    if !a.negative && b.negative {
        if a.is_zero() && b.is_zero() {
            return std::cmp::Ordering::Equal;
        }
        return std::cmp::Ordering::Greater;
    }

    let abs_cmp = a.cmp_abs(b);
    if a.negative {
        abs_cmp.reverse()
    } else {
        abs_cmp
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(String),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semicolon,
    Newline,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    CaretAssign,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    PlusPlus,
    MinusMinus,
    Not,
    And,
    Or,
    Comma,
    // Keywords
    If,
    Else,
    While,
    For,
    Print,
    Quit,
    Break,
    Continue,
    Return,
    Eof,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Skip whitespace (except newlines).
        if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
            continue;
        }

        // Comments: # to end of line.
        if c == '#' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comments: /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            continue;
        }

        if c == '\n' {
            tokens.push(Token::Newline);
            i += 1;
            continue;
        }

        // Numbers: digits and dots.
        if c.is_ascii_digit() || (c == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            tokens.push(Token::Number(chars[start..i].iter().collect()));
            continue;
        }

        // Identifiers and keywords.
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let tok = match word.as_str() {
                "if" => Token::If,
                "else" => Token::Else,
                "while" => Token::While,
                "for" => Token::For,
                "print" => Token::Print,
                "quit" | "halt" => Token::Quit,
                "break" => Token::Break,
                "continue" => Token::Continue,
                "return" => Token::Return,
                _ => Token::Ident(word),
            };
            tokens.push(tok);
            continue;
        }

        // Two-character operators.
        if i + 1 < len {
            let two: String = chars[i..i + 2].iter().collect();
            let tok = match two.as_str() {
                "+=" => Some(Token::PlusAssign),
                "-=" => Some(Token::MinusAssign),
                "*=" => Some(Token::StarAssign),
                "/=" => Some(Token::SlashAssign),
                "%=" => Some(Token::PercentAssign),
                "^=" => Some(Token::CaretAssign),
                "==" => Some(Token::Eq),
                "!=" => Some(Token::Ne),
                "<=" => Some(Token::Le),
                ">=" => Some(Token::Ge),
                "++" => Some(Token::PlusPlus),
                "--" => Some(Token::MinusMinus),
                "&&" => Some(Token::And),
                "||" => Some(Token::Or),
                _ => None,
            };
            if let Some(t) = tok {
                tokens.push(t);
                i += 2;
                continue;
            }
        }

        // Single-character operators.
        let tok = match c {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '^' => Token::Caret,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            ';' => Token::Semicolon,
            '=' => Token::Assign,
            '<' => Token::Lt,
            '>' => Token::Gt,
            '!' => Token::Not,
            ',' => Token::Comma,
            _ => {
                i += 1;
                continue; // skip unknown characters
            }
        };
        tokens.push(tok);
        i += 1;
    }

    tokens.push(Token::Eof);
    tokens
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

// PrintExpr deliberately keeps the Expr suffix: it is the AST node for bc's
// `print` keyword (which prints a list of expressions). Renaming to `Print`
// would clash with `Stmt::Print` in callers that bring both into scope.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
enum Expr {
    Number(BigDecimal),
    Variable(String),
    BinaryOp(Box<Expr>, BinOp, Box<Expr>),
    UnaryMinus(Box<Expr>),
    UnaryNot(Box<Expr>),
    Assign(String, Box<Expr>),
    CompoundAssign(String, BinOp, Box<Expr>),
    PreIncrement(String),
    PreDecrement(String),
    PostIncrement(String),
    PostDecrement(String),
    FuncCall(String, Vec<Expr>),
    /// Print expression (bc `print` keyword).
    PrintExpr(Vec<Expr>),
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone)]
enum Stmt {
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    For(Option<Expr>, Option<Expr>, Option<Expr>, Vec<Stmt>),
    Break,
    Continue,
    Block(Vec<Stmt>),
    Empty,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos]
        } else {
            &Token::Eof
        }
    }

    fn advance(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            tok
        } else {
            Token::Eof
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    fn expect(&mut self, expected: &Token) -> bool {
        self.skip_newlines();
        if self.peek() == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    fn parse_program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), Token::Eof) {
                break;
            }
            match self.parse_stmt() {
                Some(s) => stmts.push(s),
                None => {
                    // Skip unrecognized tokens.
                    self.advance();
                }
            }
        }
        stmts
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        self.skip_newlines();

        match self.peek().clone() {
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::LBrace => self.parse_block(),
            Token::Quit => {
                self.advance();
                self.skip_terminator();
                Some(Stmt::Expr(Expr::FuncCall("quit".to_string(), vec![])))
            }
            Token::Break => {
                self.advance();
                self.skip_terminator();
                Some(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.skip_terminator();
                Some(Stmt::Continue)
            }
            Token::Print => {
                self.advance();
                let mut exprs = Vec::new();
                loop {
                    if matches!(
                        self.peek(),
                        Token::Newline | Token::Semicolon | Token::Eof
                    ) {
                        break;
                    }
                    if let Some(e) = self.parse_expr() {
                        exprs.push(e);
                    }
                    if !matches!(self.peek(), Token::Comma) {
                        break;
                    }
                    self.advance(); // skip comma
                }
                self.skip_terminator();
                Some(Stmt::Expr(Expr::PrintExpr(exprs)))
            }
            Token::Eof => None,
            Token::Semicolon | Token::Newline => {
                self.advance();
                Some(Stmt::Empty)
            }
            _ => {
                let expr = self.parse_expr()?;
                self.skip_terminator();
                Some(Stmt::Expr(expr))
            }
        }
    }

    fn skip_terminator(&mut self) {
        while matches!(self.peek(), Token::Semicolon | Token::Newline) {
            self.advance();
        }
    }

    fn parse_if(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'if'
        self.expect(&Token::LParen);
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen);
        self.skip_newlines();

        let then_block = if matches!(self.peek(), Token::LBrace) {
            match self.parse_block()? {
                Stmt::Block(stmts) => stmts,
                s => vec![s],
            }
        } else {
            vec![self.parse_stmt()?]
        };

        self.skip_newlines();
        let else_block = if matches!(self.peek(), Token::Else) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::LBrace) {
                match self.parse_block()? {
                    Stmt::Block(stmts) => stmts,
                    s => vec![s],
                }
            } else if matches!(self.peek(), Token::If) {
                vec![self.parse_if()?]
            } else {
                vec![self.parse_stmt()?]
            }
        } else {
            vec![]
        };

        Some(Stmt::If(cond, then_block, else_block))
    }

    fn parse_while(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'while'
        self.expect(&Token::LParen);
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen);
        self.skip_newlines();

        let body = if matches!(self.peek(), Token::LBrace) {
            match self.parse_block()? {
                Stmt::Block(stmts) => stmts,
                s => vec![s],
            }
        } else {
            vec![self.parse_stmt()?]
        };

        Some(Stmt::While(cond, body))
    }

    fn parse_for(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'for'
        self.expect(&Token::LParen);

        let init = if matches!(self.peek(), Token::Semicolon) {
            None
        } else {
            self.parse_expr()
        };
        self.expect(&Token::Semicolon);

        let cond = if matches!(self.peek(), Token::Semicolon) {
            None
        } else {
            self.parse_expr()
        };
        self.expect(&Token::Semicolon);

        let update = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            self.parse_expr()
        };
        self.expect(&Token::RParen);
        self.skip_newlines();

        let body = if matches!(self.peek(), Token::LBrace) {
            match self.parse_block()? {
                Stmt::Block(stmts) => stmts,
                s => vec![s],
            }
        } else {
            vec![self.parse_stmt()?]
        };

        Some(Stmt::For(init, cond, update, body))
    }

    fn parse_block(&mut self) -> Option<Stmt> {
        self.advance(); // consume '{'
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), Token::RBrace | Token::Eof) {
                break;
            }
            if let Some(s) = self.parse_stmt() {
                stmts.push(s);
            } else {
                break;
            }
        }
        self.expect(&Token::RBrace);
        Some(Stmt::Block(stmts))
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Option<Expr> {
        let left = self.parse_or()?;

        match self.peek().clone() {
            Token::Assign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::Assign(name, Box::new(right)))
                } else {
                    // Assignment to non-variable — treat as expression.
                    Some(left)
                }
            }
            Token::PlusAssign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::CompoundAssign(name, BinOp::Add, Box::new(right)))
                } else {
                    Some(left)
                }
            }
            Token::MinusAssign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::CompoundAssign(name, BinOp::Sub, Box::new(right)))
                } else {
                    Some(left)
                }
            }
            Token::StarAssign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::CompoundAssign(name, BinOp::Mul, Box::new(right)))
                } else {
                    Some(left)
                }
            }
            Token::SlashAssign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::CompoundAssign(name, BinOp::Div, Box::new(right)))
                } else {
                    Some(left)
                }
            }
            Token::PercentAssign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::CompoundAssign(name, BinOp::Mod, Box::new(right)))
                } else {
                    Some(left)
                }
            }
            Token::CaretAssign => {
                self.advance();
                if let Expr::Variable(name) = left {
                    let right = self.parse_assignment()?;
                    Some(Expr::CompoundAssign(name, BinOp::Pow, Box::new(right)))
                } else {
                    Some(left)
                }
            }
            _ => Some(left),
        }
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_and_expr()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinaryOp(Box::new(left), BinOp::Or, Box::new(right));
        }
        Some(left)
    }

    fn parse_and_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_comparison()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp(Box::new(left), BinOp::And, Box::new(right));
        }
        Some(left)
    }

    fn parse_comparison(&mut self) -> Option<Expr> {
        let mut left = self.parse_additive()?;

        loop {
            let op = match self.peek() {
                Token::Eq => BinOp::Eq,
                Token::Ne => BinOp::Ne,
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::Le => BinOp::Le,
                Token::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }

        Some(left)
    }

    fn parse_additive(&mut self) -> Option<Expr> {
        let mut left = self.parse_multiplicative()?;

        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }

        Some(left)
    }

    fn parse_multiplicative(&mut self) -> Option<Expr> {
        let mut left = self.parse_power()?;

        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_power()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }

        Some(left)
    }

    fn parse_power(&mut self) -> Option<Expr> {
        let base = self.parse_unary()?;

        if matches!(self.peek(), Token::Caret) {
            self.advance();
            let exp = self.parse_power()?; // right-associative
            Some(Expr::BinaryOp(Box::new(base), BinOp::Pow, Box::new(exp)))
        } else {
            Some(base)
        }
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                Some(Expr::UnaryMinus(Box::new(operand)))
            }
            Token::Not => {
                self.advance();
                let operand = self.parse_unary()?;
                Some(Expr::UnaryNot(Box::new(operand)))
            }
            Token::PlusPlus => {
                self.advance();
                if let Token::Ident(name) = self.peek().clone() {
                    self.advance();
                    Some(Expr::PreIncrement(name))
                } else {
                    None
                }
            }
            Token::MinusMinus => {
                self.advance();
                if let Token::Ident(name) = self.peek().clone() {
                    self.advance();
                    Some(Expr::PreDecrement(name))
                } else {
                    None
                }
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let expr = self.parse_primary()?;

        match self.peek().clone() {
            Token::PlusPlus => {
                if let Expr::Variable(name) = expr {
                    self.advance();
                    Some(Expr::PostIncrement(name))
                } else {
                    Some(expr)
                }
            }
            Token::MinusMinus => {
                if let Expr::Variable(name) = expr {
                    self.advance();
                    Some(Expr::PostDecrement(name))
                } else {
                    Some(expr)
                }
            }
            _ => Some(expr),
        }
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        match self.peek().clone() {
            Token::Number(s) => {
                self.advance();
                let bd = BigDecimal::parse(&s).unwrap_or_else(BigDecimal::zero);
                Some(Expr::Number(bd))
            }
            Token::Ident(name) => {
                self.advance();
                // Check for function call.
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        if let Some(arg) = self.parse_expr() {
                            args.push(arg);
                        }
                        while matches!(self.peek(), Token::Comma) {
                            self.advance();
                            if let Some(arg) = self.parse_expr() {
                                args.push(arg);
                            }
                        }
                    }
                    self.expect(&Token::RParen);
                    Some(Expr::FuncCall(name, args))
                } else {
                    Some(Expr::Variable(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen);
                Some(expr)
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

enum ControlFlow {
    None,
    Break,
    Continue,
    Quit,
}

struct Interpreter {
    variables: HashMap<String, BigDecimal>,
    scale: usize,
    ibase: usize,
    obase: usize,
    last: BigDecimal,
}

impl Interpreter {
    fn new() -> Self {
        Interpreter {
            variables: HashMap::new(),
            scale: 0,
            ibase: 10,
            obase: 10,
            last: BigDecimal::zero(),
        }
    }

    fn get_var(&self, name: &str) -> BigDecimal {
        match name {
            "scale" => BigDecimal::parse(&self.scale.to_string()).unwrap_or_else(BigDecimal::zero),
            "ibase" => BigDecimal::parse(&self.ibase.to_string()).unwrap_or_else(BigDecimal::zero),
            "obase" => BigDecimal::parse(&self.obase.to_string()).unwrap_or_else(BigDecimal::zero),
            "last" | "." => self.last.clone(),
            _ => self.variables.get(name).cloned().unwrap_or_else(BigDecimal::zero),
        }
    }

    fn set_var(&mut self, name: &str, val: BigDecimal) {
        match name {
            "scale" => {
                let s = val.to_string_with_scale(0);
                self.scale = s.parse::<usize>().unwrap_or(0);
            }
            "ibase" => {
                let s = val.to_string_with_scale(0);
                self.ibase = s.parse::<usize>().unwrap_or(10).clamp(2, 16);
            }
            "obase" => {
                let s = val.to_string_with_scale(0);
                self.obase = s.parse::<usize>().unwrap_or(10).clamp(2, 16);
            }
            "last" | "." => {
                self.last = val;
            }
            _ => {
                self.variables.insert(name.to_string(), val);
            }
        }
    }

    fn run_stmts(&mut self, stmts: &[Stmt]) -> ControlFlow {
        for stmt in stmts {
            match self.run_stmt(stmt) {
                ControlFlow::None => {}
                cf => return cf,
            }
        }
        ControlFlow::None
    }

    fn run_stmt(&mut self, stmt: &Stmt) -> ControlFlow {
        match stmt {
            Stmt::Empty => ControlFlow::None,

            Stmt::Expr(expr) => {
                match expr {
                    Expr::PrintExpr(exprs) => {
                        let stdout = io::stdout();
                        let mut out = stdout.lock();
                        for e in exprs {
                            let val = self.eval(e);
                            let _ = write!(out, "{}", val.to_string_with_scale(self.scale));
                        }
                        let _ = writeln!(out);
                        ControlFlow::None
                    }
                    Expr::FuncCall(name, _) if name == "quit" => ControlFlow::Quit,
                    Expr::Assign(_, _)
                    | Expr::CompoundAssign(_, _, _)
                    | Expr::PreIncrement(_)
                    | Expr::PreDecrement(_)
                    | Expr::PostIncrement(_)
                    | Expr::PostDecrement(_) => {
                        self.eval(expr);
                        ControlFlow::None
                    }
                    _ => {
                        let val = self.eval(expr);
                        self.last = val.clone();
                        println!("{}", val.to_string_with_scale(self.scale));
                        ControlFlow::None
                    }
                }
            }

            Stmt::If(cond, then_body, else_body) => {
                let cond_val = self.eval(cond);
                if !cond_val.is_zero() {
                    self.run_stmts(then_body)
                } else {
                    self.run_stmts(else_body)
                }
            }

            Stmt::While(cond, body) => {
                let mut iteration = 0u64;
                loop {
                    let cond_val = self.eval(cond);
                    if cond_val.is_zero() {
                        break;
                    }
                    match self.run_stmts(body) {
                        ControlFlow::Break => break,
                        ControlFlow::Quit => return ControlFlow::Quit,
                        ControlFlow::Continue | ControlFlow::None => {}
                    }
                    iteration += 1;
                    if iteration > 10_000_000 {
                        eprintln!("bc: infinite loop detected (> 10M iterations)");
                        break;
                    }
                }
                ControlFlow::None
            }

            Stmt::For(init, cond, update, body) => {
                if let Some(init_expr) = init {
                    self.eval(init_expr);
                }
                let mut iteration = 0u64;
                loop {
                    if let Some(cond_expr) = cond {
                        let cond_val = self.eval(cond_expr);
                        if cond_val.is_zero() {
                            break;
                        }
                    }
                    match self.run_stmts(body) {
                        ControlFlow::Break => break,
                        ControlFlow::Quit => return ControlFlow::Quit,
                        ControlFlow::Continue | ControlFlow::None => {}
                    }
                    if let Some(update_expr) = update {
                        self.eval(update_expr);
                    }
                    iteration += 1;
                    if iteration > 10_000_000 {
                        eprintln!("bc: infinite loop detected (> 10M iterations)");
                        break;
                    }
                }
                ControlFlow::None
            }

            Stmt::Break => ControlFlow::Break,
            Stmt::Continue => ControlFlow::Continue,

            Stmt::Block(stmts) => self.run_stmts(stmts),
        }
    }

    fn eval(&mut self, expr: &Expr) -> BigDecimal {
        match expr {
            Expr::Number(n) => n.clone(),

            Expr::Variable(name) => self.get_var(name),

            Expr::BinaryOp(left, op, right) => {
                let lv = self.eval(left);
                let rv = self.eval(right);
                self.eval_binop(&lv, *op, &rv)
            }

            Expr::UnaryMinus(inner) => {
                let v = self.eval(inner);
                v.negate()
            }

            Expr::UnaryNot(inner) => {
                let v = self.eval(inner);
                if v.is_zero() {
                    BigDecimal::one()
                } else {
                    BigDecimal::zero()
                }
            }

            Expr::Assign(name, value) => {
                let v = self.eval(value);
                self.set_var(name, v.clone());
                v
            }

            Expr::CompoundAssign(name, op, value) => {
                let current = self.get_var(name);
                let rhs = self.eval(value);
                let result = self.eval_binop(&current, *op, &rhs);
                self.set_var(name, result.clone());
                result
            }

            Expr::PreIncrement(name) => {
                let current = self.get_var(name);
                let result = bd_add(&current, &BigDecimal::one());
                self.set_var(name, result.clone());
                result
            }

            Expr::PreDecrement(name) => {
                let current = self.get_var(name);
                let result = bd_sub(&current, &BigDecimal::one());
                self.set_var(name, result.clone());
                result
            }

            Expr::PostIncrement(name) => {
                let current = self.get_var(name);
                let incremented = bd_add(&current, &BigDecimal::one());
                self.set_var(name, incremented);
                current
            }

            Expr::PostDecrement(name) => {
                let current = self.get_var(name);
                let decremented = bd_sub(&current, &BigDecimal::one());
                self.set_var(name, decremented);
                current
            }

            Expr::FuncCall(name, args) => {
                match name.as_str() {
                    "sqrt" => {
                        if args.is_empty() {
                            eprintln!("bc: sqrt requires an argument");
                            return BigDecimal::zero();
                        }
                        let v = self.eval(&args[0]);
                        bd_sqrt(&v, self.scale).unwrap_or_else(|| {
                            eprintln!("bc: square root of negative number");
                            BigDecimal::zero()
                        })
                    }
                    "length" => {
                        if args.is_empty() {
                            return BigDecimal::zero();
                        }
                        let v = self.eval(&args[0]);
                        let s = v.to_string_with_scale(v.scale);
                        let digits: usize = s
                            .chars()
                            .filter(|c| c.is_ascii_digit())
                            .count();
                        BigDecimal::parse(&digits.to_string())
                            .unwrap_or_else(BigDecimal::zero)
                    }
                    "scale" => {
                        if args.is_empty() {
                            return BigDecimal::parse(&self.scale.to_string())
                                .unwrap_or_else(BigDecimal::zero);
                        }
                        let v = self.eval(&args[0]);
                        BigDecimal::parse(&v.scale.to_string())
                            .unwrap_or_else(BigDecimal::zero)
                    }
                    "quit" => {
                        process::exit(0);
                    }
                    _ => {
                        eprintln!("bc: undefined function: {name}");
                        BigDecimal::zero()
                    }
                }
            }

            Expr::PrintExpr(exprs) => {
                let stdout = io::stdout();
                let mut out = stdout.lock();
                let mut last = BigDecimal::zero();
                for e in exprs {
                    let val = self.eval(e);
                    let _ = write!(out, "{}", val.to_string_with_scale(self.scale));
                    last = val;
                }
                let _ = writeln!(out);
                last
            }
        }
    }

    fn eval_binop(&self, lv: &BigDecimal, op: BinOp, rv: &BigDecimal) -> BigDecimal {
        match op {
            BinOp::Add => bd_add(lv, rv),
            BinOp::Sub => bd_sub(lv, rv),
            BinOp::Mul => bd_mul(lv, rv, self.scale),
            BinOp::Div => match bd_div(lv, rv, self.scale) {
                Some(r) => r,
                None => {
                    eprintln!("bc: division by zero");
                    BigDecimal::zero()
                }
            },
            BinOp::Mod => match bd_modulo(lv, rv, self.scale) {
                Some(r) => r,
                None => {
                    eprintln!("bc: division by zero");
                    BigDecimal::zero()
                }
            },
            BinOp::Pow => bd_pow(lv, rv, self.scale),
            BinOp::Eq => bool_to_bd(bd_compare(lv, rv) == std::cmp::Ordering::Equal),
            BinOp::Ne => bool_to_bd(bd_compare(lv, rv) != std::cmp::Ordering::Equal),
            BinOp::Lt => bool_to_bd(bd_compare(lv, rv) == std::cmp::Ordering::Less),
            BinOp::Gt => bool_to_bd(bd_compare(lv, rv) == std::cmp::Ordering::Greater),
            BinOp::Le => bool_to_bd(bd_compare(lv, rv) != std::cmp::Ordering::Greater),
            BinOp::Ge => bool_to_bd(bd_compare(lv, rv) != std::cmp::Ordering::Less),
            BinOp::And => bool_to_bd(!lv.is_zero() && !rv.is_zero()),
            BinOp::Or => bool_to_bd(!lv.is_zero() || !rv.is_zero()),
        }
    }
}

fn bool_to_bd(b: bool) -> BigDecimal {
    if b {
        BigDecimal::one()
    } else {
        BigDecimal::zero()
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/// Parsed bc command-line arguments.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[derive(Default)]
struct BcArgs {
    quiet: bool,
    math_lib: bool,
    files: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<BcArgs, String> {
    let mut out = BcArgs::default();
    for arg in args {
        match arg.as_str() {
            "-q" | "--quiet" => out.quiet = true,
            "-l" | "--mathlib" => out.math_lib = true,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option: {other}"));
            }
            _ => out.files.push(arg.clone()),
        }
    }
    Ok(out)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let BcArgs { quiet, math_lib, files } = match parse_args(&args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("bc: {msg}");
            process::exit(1);
        }
    };

    if !quiet {
        eprintln!("bc 1.0 (slateos coreutils)");
        eprintln!("Type \"quit\" to exit.");
    }

    let mut interp = Interpreter::new();

    // -l sets scale to 20 and loads math library functions.
    if math_lib {
        interp.scale = 20;
    }

    // Process files.
    for path in &files {
        match fs::read_to_string(path) {
            Ok(content) => {
                if run_input(&mut interp, &content) {
                    return;
                }
            }
            Err(e) => {
                eprintln!("bc: {path}: {e}");
                process::exit(1);
            }
        }
    }

    // Interactive mode: read from stdin.
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());
    // Read lines, handling line continuation with backslash.
    let mut continued = String::new();
    for line in reader.lines() {
        match line {
            Ok(l) => {
                if l.ends_with('\\') {
                    continued.push_str(&l[..l.len() - 1]);
                    continued.push(' ');
                    continue;
                }

                let full_line = if continued.is_empty() {
                    l
                } else {
                    let mut full = std::mem::take(&mut continued);
                    full.push_str(&l);
                    full
                };

                if run_input(&mut interp, &full_line) {
                    return;
                }
                let _ = stdout.lock().flush();
            }
            Err(_) => break,
        }
    }
}

/// Run input through the tokenizer/parser/interpreter.
/// Returns true if quit was requested.
fn run_input(interp: &mut Interpreter, input: &str) -> bool {
    let tokens = tokenize(input);
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse_program();

    for stmt in &stmts {
        if let ControlFlow::Quit = interp.run_stmt(stmt) { return true }
    }
    false
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn bd(text: &str) -> BigDecimal {
        BigDecimal::parse(text).expect("parse")
    }

    fn show(v: &BigDecimal, scale: usize) -> String {
        v.to_string_with_scale(scale)
    }

    // ---------- parse_args ----------

    #[test]
    fn parse_args_empty_defaults() {
        let p = parse_args(&[]).unwrap();
        assert!(!p.quiet);
        assert!(!p.math_lib);
        assert!(p.files.is_empty());
    }

    #[test]
    fn parse_args_quiet() {
        let p = parse_args(&s(&["-q"])).unwrap();
        assert!(p.quiet);
        let p = parse_args(&s(&["--quiet"])).unwrap();
        assert!(p.quiet);
    }

    #[test]
    fn parse_args_math_lib() {
        let p = parse_args(&s(&["-l"])).unwrap();
        assert!(p.math_lib);
        let p = parse_args(&s(&["--mathlib"])).unwrap();
        assert!(p.math_lib);
    }

    #[test]
    fn parse_args_files_accumulate() {
        let p = parse_args(&s(&["a.bc", "b.bc"])).unwrap();
        assert_eq!(p.files, vec!["a.bc".to_string(), "b.bc".to_string()]);
    }

    #[test]
    fn parse_args_combined_flags() {
        let p = parse_args(&s(&["-q", "-l", "a.bc"])).unwrap();
        assert!(p.quiet);
        assert!(p.math_lib);
        assert_eq!(p.files, vec!["a.bc".to_string()]);
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        let err = parse_args(&s(&["-z"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_args_bare_dash_is_treated_as_file() {
        // Single "-" is a conventional "stdin" marker; we just store it.
        let p = parse_args(&s(&["-"])).unwrap();
        assert_eq!(p.files, vec!["-".to_string()]);
    }

    // ---------- BigDecimal::parse ----------

    #[test]
    fn parse_simple_integer() {
        let v = bd("42");
        assert!(!v.negative);
        assert_eq!(v.scale, 0);
        assert_eq!(show(&v, 0), "42");
    }

    #[test]
    fn parse_negative_integer() {
        let v = bd("-7");
        assert!(v.negative);
        assert_eq!(show(&v, 0), "-7");
    }

    #[test]
    fn parse_explicit_positive_sign() {
        let v = bd("+5");
        assert!(!v.negative);
        assert_eq!(show(&v, 0), "5");
    }

    #[test]
    fn parse_decimal_value() {
        let v = bd("3.14");
        assert_eq!(v.scale, 2);
        assert_eq!(show(&v, 2), "3.14");
    }

    #[test]
    fn parse_zero() {
        let v = bd("0");
        assert!(v.is_zero());
        assert!(!v.negative);
    }

    #[test]
    fn parse_negative_zero_renders_as_zero() {
        // After normalization, -0 should render as 0.
        let mut v = bd("-0");
        v.normalize();
        assert_eq!(show(&v, 0), "0");
    }

    #[test]
    fn parse_with_whitespace_trimmed() {
        assert_eq!(show(&bd("  100  "), 0), "100");
    }

    #[test]
    fn parse_empty_string_is_none() {
        assert!(BigDecimal::parse("").is_none());
        assert!(BigDecimal::parse("   ").is_none());
    }

    #[test]
    fn parse_garbage_is_none() {
        assert!(BigDecimal::parse("abc").is_none());
        assert!(BigDecimal::parse("1.2.3").is_none());
    }

    #[test]
    fn parse_leading_zeros_stripped() {
        assert_eq!(show(&bd("00042"), 0), "42");
    }

    #[test]
    fn parse_large_value_above_one_limb() {
        // 10 billion exceeds the 10^9 limb base, so spans two limbs.
        let v = bd("10000000000");
        assert_eq!(show(&v, 0), "10000000000");
    }

    // ---------- to_string_with_scale ----------

    #[test]
    fn display_pads_fractional_zeros() {
        let v = bd("1.5");
        assert_eq!(v.to_string_with_scale(4), "1.5000");
    }

    #[test]
    fn display_truncates_to_lower_scale() {
        let v = bd("1.234567");
        assert_eq!(v.to_string_with_scale(2), "1.23");
    }

    #[test]
    fn display_value_less_than_one() {
        let v = bd("0.5");
        assert_eq!(v.to_string_with_scale(1), "0.5");
    }

    // ---------- truncate_or_pad ----------

    #[test]
    fn truncate_or_pad_shorter_pads_with_zeros() {
        assert_eq!(truncate_or_pad("12", 5), "12000");
    }

    #[test]
    fn truncate_or_pad_longer_truncates() {
        assert_eq!(truncate_or_pad("12345", 2), "12");
    }

    #[test]
    fn truncate_or_pad_exact_passes_through() {
        assert_eq!(truncate_or_pad("abc", 3), "abc");
    }

    // ---------- digits_to_limbs / limbs_to_digits round trip ----------

    #[test]
    fn digits_limbs_roundtrip_single_limb() {
        let limbs = digits_to_limbs("12345");
        assert_eq!(limbs_to_digits(&limbs), "12345");
    }

    #[test]
    fn digits_limbs_roundtrip_multi_limb() {
        let limbs = digits_to_limbs("123456789012345678");
        assert_eq!(limbs_to_digits(&limbs), "123456789012345678");
    }

    #[test]
    fn digits_limbs_roundtrip_large() {
        let big = "1234567890".repeat(10);
        let limbs = digits_to_limbs(&big);
        assert_eq!(limbs_to_digits(&limbs), big);
    }

    // ---------- cmp_limbs ----------

    #[test]
    fn cmp_limbs_orderings() {
        use std::cmp::Ordering;
        assert_eq!(cmp_limbs(&[1], &[2]), Ordering::Less);
        assert_eq!(cmp_limbs(&[2], &[1]), Ordering::Greater);
        assert_eq!(cmp_limbs(&[5, 0], &[5]), Ordering::Equal);
        // Higher limb count with a non-zero top limb beats single-limb.
        assert_eq!(cmp_limbs(&[0, 1], &[999]), Ordering::Greater);
    }

    // ---------- bd_add / bd_sub ----------

    #[test]
    fn add_positive_integers() {
        assert_eq!(show(&bd_add(&bd("12"), &bd("30")), 0), "42");
    }

    #[test]
    fn add_with_decimals_aligns_scale() {
        assert_eq!(show(&bd_add(&bd("1.5"), &bd("2.25")), 2), "3.75");
    }

    #[test]
    fn add_opposite_signs_subtracts() {
        assert_eq!(show(&bd_add(&bd("10"), &bd("-3")), 0), "7");
        assert_eq!(show(&bd_add(&bd("-10"), &bd("3")), 0), "-7");
    }

    #[test]
    fn add_equal_magnitudes_opposite_signs_is_zero() {
        let r = bd_add(&bd("5"), &bd("-5"));
        assert!(r.is_zero());
    }

    #[test]
    fn sub_basic() {
        assert_eq!(show(&bd_sub(&bd("100"), &bd("58")), 0), "42");
    }

    #[test]
    fn sub_negative_minus_negative() {
        // -3 - (-1) = -2
        assert_eq!(show(&bd_sub(&bd("-3"), &bd("-1")), 0), "-2");
    }

    #[test]
    fn sub_borrow_across_limb_boundary() {
        // 10^9 - 1 forces a borrow into the next limb on subtraction.
        assert_eq!(
            show(&bd_sub(&bd("1000000000"), &bd("1")), 0),
            "999999999"
        );
    }

    // ---------- bd_mul ----------

    #[test]
    fn mul_basic_integer() {
        assert_eq!(show(&bd_mul(&bd("6"), &bd("7"), 0), 0), "42");
    }

    #[test]
    fn mul_negative_times_positive_negative() {
        assert_eq!(show(&bd_mul(&bd("-3"), &bd("4"), 0), 0), "-12");
    }

    #[test]
    fn mul_negative_times_negative_positive() {
        assert_eq!(show(&bd_mul(&bd("-3"), &bd("-4"), 0), 0), "12");
    }

    #[test]
    fn mul_decimal_truncates_to_scale() {
        // 1.5 * 1.5 = 2.25, with scale=1 truncates to 2.2.
        assert_eq!(show(&bd_mul(&bd("1.5"), &bd("1.5"), 1), 1), "2.2");
    }

    #[test]
    fn mul_by_zero_is_zero() {
        assert!(bd_mul(&bd("1234"), &bd("0"), 0).is_zero());
    }

    // ---------- bd_div / bd_modulo ----------

    #[test]
    fn div_integer_truncates_with_scale_zero() {
        // 10 / 3 with scale=0 truncates to 3.
        let q = bd_div(&bd("10"), &bd("3"), 0).unwrap();
        assert_eq!(show(&q, 0), "3");
    }

    #[test]
    fn div_with_scale_two() {
        // 10 / 4 with scale=2 = 2.50.
        let q = bd_div(&bd("10"), &bd("4"), 2).unwrap();
        assert_eq!(show(&q, 2), "2.50");
    }

    #[test]
    fn div_by_zero_returns_none() {
        assert!(bd_div(&bd("5"), &bd("0"), 0).is_none());
    }

    #[test]
    fn div_negative_dividend_yields_negative() {
        let q = bd_div(&bd("-12"), &bd("4"), 0).unwrap();
        assert_eq!(show(&q, 0), "-3");
    }

    #[test]
    fn modulo_basic() {
        let m = bd_modulo(&bd("10"), &bd("3"), 0).unwrap();
        assert_eq!(show(&m, 0), "1");
    }

    #[test]
    fn modulo_by_zero_returns_none() {
        assert!(bd_modulo(&bd("5"), &bd("0"), 0).is_none());
    }

    // ---------- bd_pow ----------

    #[test]
    fn pow_zero_exponent_is_one() {
        assert_eq!(show(&bd_pow(&bd("12345"), &bd("0"), 0), 0), "1");
    }

    #[test]
    fn pow_basic_squared() {
        assert_eq!(show(&bd_pow(&bd("7"), &bd("2"), 0), 0), "49");
    }

    #[test]
    fn pow_basic_cubed() {
        assert_eq!(show(&bd_pow(&bd("3"), &bd("4"), 0), 0), "81");
    }

    #[test]
    fn pow_negative_base_odd_exp_is_negative() {
        assert_eq!(show(&bd_pow(&bd("-2"), &bd("3"), 0), 0), "-8");
    }

    #[test]
    fn pow_negative_base_even_exp_is_positive() {
        assert_eq!(show(&bd_pow(&bd("-2"), &bd("4"), 0), 0), "16");
    }

    // ---------- bd_sqrt ----------

    #[test]
    fn sqrt_perfect_square() {
        let r = bd_sqrt(&bd("144"), 0).unwrap();
        assert_eq!(show(&r, 0), "12");
    }

    #[test]
    fn sqrt_of_zero() {
        let r = bd_sqrt(&bd("0"), 5).unwrap();
        assert!(r.is_zero());
    }

    #[test]
    fn sqrt_of_negative_is_none() {
        assert!(bd_sqrt(&bd("-4"), 0).is_none());
    }

    #[test]
    fn sqrt_of_two_approximate() {
        // sqrt(2) ≈ 1.41421356...  Verify the first few digits.
        let r = bd_sqrt(&bd("2"), 5).unwrap();
        let text = show(&r, 5);
        assert!(text.starts_with("1.4142"), "got {text}");
    }

    // ---------- bd_compare ----------

    #[test]
    fn compare_basic_orderings() {
        use std::cmp::Ordering;
        assert_eq!(bd_compare(&bd("1"), &bd("2")), Ordering::Less);
        assert_eq!(bd_compare(&bd("2"), &bd("1")), Ordering::Greater);
        assert_eq!(bd_compare(&bd("1.5"), &bd("1.5")), Ordering::Equal);
    }

    #[test]
    fn compare_negative_vs_positive() {
        use std::cmp::Ordering;
        assert_eq!(bd_compare(&bd("-1"), &bd("1")), Ordering::Less);
        assert_eq!(bd_compare(&bd("-1"), &bd("-2")), Ordering::Greater);
    }

    // ---------- bool_to_bd ----------

    #[test]
    fn bool_true_is_one() {
        assert_eq!(show(&bool_to_bd(true), 0), "1");
    }

    #[test]
    fn bool_false_is_zero() {
        assert_eq!(show(&bool_to_bd(false), 0), "0");
    }

    // ---------- BigDecimal::negate ----------

    #[test]
    fn negate_positive_becomes_negative() {
        let v = bd("5").negate();
        assert!(v.negative);
        assert_eq!(show(&v, 0), "-5");
    }

    #[test]
    fn negate_zero_stays_zero_unsigned() {
        let v = bd("0").negate();
        assert!(!v.negative);
        assert!(v.is_zero());
    }

    // ---------- tokenize ----------

    #[test]
    fn tokenize_number() {
        let t = tokenize("42");
        assert_eq!(t, vec![Token::Number("42".to_string()), Token::Eof]);
    }

    #[test]
    fn tokenize_decimal_number() {
        let t = tokenize("3.14");
        assert_eq!(t, vec![Token::Number("3.14".to_string()), Token::Eof]);
    }

    #[test]
    fn tokenize_arithmetic_operators() {
        let t = tokenize("+-*/^%");
        assert_eq!(
            t,
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Caret,
                Token::Percent,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_grouping() {
        let t = tokenize("(){}");
        assert_eq!(
            t,
            vec![
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_comparison_operators() {
        let t = tokenize("== != <= >= < >");
        assert_eq!(
            t,
            vec![
                Token::Eq,
                Token::Ne,
                Token::Le,
                Token::Ge,
                Token::Lt,
                Token::Gt,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_compound_assignments() {
        let t = tokenize("+= -= *= /= %= ^=");
        assert_eq!(
            t,
            vec![
                Token::PlusAssign,
                Token::MinusAssign,
                Token::StarAssign,
                Token::SlashAssign,
                Token::PercentAssign,
                Token::CaretAssign,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_increment_decrement() {
        let t = tokenize("++ --");
        assert_eq!(t, vec![Token::PlusPlus, Token::MinusMinus, Token::Eof]);
    }

    #[test]
    fn tokenize_keywords() {
        let t = tokenize("if else while for print quit");
        assert_eq!(
            t,
            vec![
                Token::If,
                Token::Else,
                Token::While,
                Token::For,
                Token::Print,
                Token::Quit,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_identifier() {
        let t = tokenize("foo_bar1");
        assert_eq!(t, vec![Token::Ident("foo_bar1".to_string()), Token::Eof]);
    }

    #[test]
    fn tokenize_line_comment_skipped() {
        let t = tokenize("1 # comment\n2");
        assert_eq!(
            t,
            vec![
                Token::Number("1".to_string()),
                Token::Newline,
                Token::Number("2".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_block_comment_skipped() {
        let t = tokenize("1 /* block */ 2");
        assert_eq!(
            t,
            vec![
                Token::Number("1".to_string()),
                Token::Number("2".to_string()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_whitespace_ignored() {
        let t = tokenize("  \t1  \t  2  ");
        assert_eq!(
            t,
            vec![
                Token::Number("1".to_string()),
                Token::Number("2".to_string()),
                Token::Eof,
            ]
        );
    }

    // ---------- run_input (end-to-end via Interpreter) ----------

    #[test]
    fn run_input_quit_returns_true() {
        let mut interp = Interpreter::new();
        assert!(run_input(&mut interp, "quit"));
    }

    #[test]
    fn run_input_simple_assignment_persists_in_vars() {
        let mut interp = Interpreter::new();
        // Assignment is a statement that does not print; check the var is set.
        let quit = run_input(&mut interp, "x = 41 + 1");
        assert!(!quit);
        let v = interp.get_var("x");
        assert_eq!(show(&v, 0), "42");
    }

    #[test]
    fn run_input_sets_scale_special_var() {
        let mut interp = Interpreter::new();
        let _ = run_input(&mut interp, "scale = 5");
        assert_eq!(interp.scale, 5);
    }

    #[test]
    fn run_input_empty_is_noop() {
        let mut interp = Interpreter::new();
        assert!(!run_input(&mut interp, ""));
        assert!(!run_input(&mut interp, "   \n   "));
    }
}
