//! test — evaluate conditional expressions.
//!
//! Usage: test EXPRESSION
//!    or: [ EXPRESSION ]
//!
//! Supports:
//!   File tests: -e FILE, -f FILE, -d FILE, -r FILE, -w FILE, -x FILE,
//!               -s FILE (non-empty), -L FILE (symlink)
//!   String tests: -n STRING (non-empty), -z STRING (empty),
//!                 STR1 = STR2, STR1 != STR2
//!   Integer tests: N1 -eq N2, -ne, -lt, -le, -gt, -ge
//!   Logical: ! EXPR, EXPR -a EXPR, EXPR -o EXPR
//!
//! Permission tests (`-r`, `-w`, `-x`) rely on Unix mode bits and are
//! always false on non-Unix hosts.  All other ops (including string
//! and integer comparisons) are platform-independent and exercised by
//! the unit tests on every host.

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().cloned().unwrap_or_default();

    // If invoked as "[", the last arg must be "]".
    let test_args: Vec<String> = if prog.ends_with('[') || prog == "[" {
        if args.last().is_none_or(|a| a != "]") {
            eprintln!("[: missing ']'");
            process::exit(2);
        }
        let end = args.len().saturating_sub(1);
        args.get(1..end).unwrap_or(&[]).to_vec()
    } else {
        args.get(1..).unwrap_or(&[]).to_vec()
    };

    let result = evaluate(&test_args);
    process::exit(i32::from(!result));
}

/// Evaluate a `test` expression.  Pure with respect to args: filesystem
/// state is the only side input.  Returns true (exit 0) or false (exit 1).
fn evaluate(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }

    // Handle ! (negation)
    if args.first().is_some_and(|a| a == "!") {
        return !evaluate(args.get(1..).unwrap_or(&[]));
    }

    // Three-argument forms (binary string/integer comparisons)
    if args.len() == 3 {
        let a = args.first().map(String::as_str).unwrap_or("");
        let op = args.get(1).map(String::as_str).unwrap_or("");
        let b = args.get(2).map(String::as_str).unwrap_or("");

        match op {
            "=" | "==" => return a == b,
            "!=" => return a != b,
            "-eq" => return int_cmp(a, b, |x, y| x == y),
            "-ne" => return int_cmp(a, b, |x, y| x != y),
            "-lt" => return int_cmp(a, b, |x, y| x < y),
            "-le" => return int_cmp(a, b, |x, y| x <= y),
            "-gt" => return int_cmp(a, b, |x, y| x > y),
            "-ge" => return int_cmp(a, b, |x, y| x >= y),
            _ => {}
        }
    }

    // Look for -a / -o (lowest precedence binary operators).
    // -o has lower precedence than -a, so split on -o first.
    for (i, arg) in args.iter().enumerate() {
        if arg == "-o" {
            return evaluate(args.get(..i).unwrap_or(&[]))
                || evaluate(args.get(i.saturating_add(1)..).unwrap_or(&[]));
        }
    }
    for (i, arg) in args.iter().enumerate() {
        if arg == "-a" {
            return evaluate(args.get(..i).unwrap_or(&[]))
                && evaluate(args.get(i.saturating_add(1)..).unwrap_or(&[]));
        }
    }

    // Two-argument forms (unary tests)
    if args.len() == 2 {
        let op = args.first().map(String::as_str).unwrap_or("");
        let operand = args.get(1).map(String::as_str).unwrap_or("");

        match op {
            "-e" => return fs::symlink_metadata(operand).is_ok(),
            "-f" => return fs::metadata(operand).is_ok_and(|m| m.is_file()),
            "-d" => return fs::metadata(operand).is_ok_and(|m| m.is_dir()),
            "-L" | "-h" => {
                return fs::symlink_metadata(operand).is_ok_and(|m| m.file_type().is_symlink());
            }
            "-r" => return check_mode(operand, 0o444),
            "-w" => return check_mode(operand, 0o222),
            "-x" => return check_mode(operand, 0o111),
            "-s" => return fs::metadata(operand).is_ok_and(|m| m.len() > 0),
            "-n" => return !operand.is_empty(),
            "-z" => return operand.is_empty(),
            _ => {}
        }
    }

    // Single argument: true if non-empty string.
    if args.len() == 1 {
        return args.first().is_some_and(|a| !a.is_empty());
    }

    // Fallback: unknown expression → false
    false
}

/// Parse two strings as i64 (treating non-numeric as 0, matching the
/// historical `test` behaviour) and compare with `cmp`.
fn int_cmp(a: &str, b: &str, cmp: impl Fn(i64, i64) -> bool) -> bool {
    let x = a.parse::<i64>().unwrap_or(0);
    let y = b.parse::<i64>().unwrap_or(0);
    cmp(x, y)
}

/// Unix mode-bit check for `-r`/`-w`/`-x`.  On non-Unix hosts this
/// always returns false (Windows ACLs are not POSIX permission bits).
#[cfg(unix)]
fn check_mode(path: &str, mask: u32) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|m| m.permissions().mode() & mask != 0)
}

#[cfg(not(unix))]
fn check_mode(_path: &str, _mask: u32) -> bool {
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- empty / single ----------------

    #[test]
    fn empty_args_is_false() {
        assert!(!evaluate(&s(&[])));
    }

    #[test]
    fn single_nonempty_string_is_true() {
        assert!(evaluate(&s(&["hello"])));
    }

    #[test]
    fn single_empty_string_is_false() {
        assert!(!evaluate(&s(&[""])));
    }

    // ---------------- string tests ----------------

    #[test]
    fn string_equal() {
        assert!(evaluate(&s(&["foo", "=", "foo"])));
        assert!(!evaluate(&s(&["foo", "=", "bar"])));
    }

    #[test]
    fn string_equal_double_equals() {
        assert!(evaluate(&s(&["foo", "==", "foo"])));
    }

    #[test]
    fn string_not_equal() {
        assert!(evaluate(&s(&["foo", "!=", "bar"])));
        assert!(!evaluate(&s(&["foo", "!=", "foo"])));
    }

    #[test]
    fn n_string_is_nonempty() {
        assert!(evaluate(&s(&["-n", "hello"])));
        assert!(!evaluate(&s(&["-n", ""])));
    }

    #[test]
    fn z_string_is_empty() {
        assert!(evaluate(&s(&["-z", ""])));
        assert!(!evaluate(&s(&["-z", "hello"])));
    }

    // ---------------- integer tests ----------------

    #[test]
    fn int_eq() {
        assert!(evaluate(&s(&["5", "-eq", "5"])));
        assert!(!evaluate(&s(&["5", "-eq", "6"])));
    }

    #[test]
    fn int_ne() {
        assert!(evaluate(&s(&["5", "-ne", "6"])));
        assert!(!evaluate(&s(&["5", "-ne", "5"])));
    }

    #[test]
    fn int_lt_le_gt_ge() {
        assert!(evaluate(&s(&["3", "-lt", "5"])));
        assert!(evaluate(&s(&["5", "-le", "5"])));
        assert!(evaluate(&s(&["7", "-gt", "5"])));
        assert!(evaluate(&s(&["5", "-ge", "5"])));
        assert!(!evaluate(&s(&["5", "-lt", "3"])));
    }

    #[test]
    fn int_negative_numbers() {
        assert!(evaluate(&s(&["-3", "-lt", "0"])));
        assert!(evaluate(&s(&["-5", "-eq", "-5"])));
    }

    #[test]
    fn int_nonnumeric_treated_as_zero() {
        // Matches historical test(1): non-numeric parses as 0 silently.
        assert!(evaluate(&s(&["abc", "-eq", "0"])));
    }

    #[test]
    fn int_cmp_helper() {
        assert!(int_cmp("5", "5", |x, y| x == y));
        assert!(!int_cmp("5", "6", |x, y| x == y));
        assert!(int_cmp("nope", "0", |x, y| x == y));
    }

    // ---------------- negation ----------------

    #[test]
    fn negate_true_string() {
        assert!(!evaluate(&s(&["!", "hello"])));
    }

    #[test]
    fn negate_false_string() {
        assert!(evaluate(&s(&["!", ""])));
    }

    #[test]
    fn double_negate() {
        assert!(evaluate(&s(&["!", "!", "hello"])));
    }

    #[test]
    fn negate_with_compare() {
        assert!(evaluate(&s(&["!", "foo", "=", "bar"])));
        assert!(!evaluate(&s(&["!", "foo", "=", "foo"])));
    }

    // ---------------- logical -a / -o ----------------

    #[test]
    fn logical_and_both_true() {
        assert!(evaluate(&s(&["foo", "-a", "bar"])));
    }

    #[test]
    fn logical_and_one_false() {
        assert!(!evaluate(&s(&["foo", "-a", ""])));
        assert!(!evaluate(&s(&["", "-a", "bar"])));
    }

    #[test]
    fn logical_or_one_true() {
        assert!(evaluate(&s(&["foo", "-o", ""])));
        assert!(evaluate(&s(&["", "-o", "bar"])));
    }

    #[test]
    fn logical_or_both_false() {
        assert!(!evaluate(&s(&["", "-o", ""])));
    }

    #[test]
    fn logical_or_lower_precedence_than_and() {
        // `a -o b -a c` ≡ `a -o (b -a c)`.  With a=true, result is true.
        assert!(evaluate(&s(&["yes", "-o", "", "-a", ""])));
        // a=false, b=true, c=true: false -o (true -a true) = true
        assert!(evaluate(&s(&["", "-o", "yes", "-a", "yes"])));
        // a=false, b=true, c=false: false -o (true -a false) = false
        assert!(!evaluate(&s(&["", "-o", "yes", "-a", ""])));
    }

    // ---------------- file tests (best-effort across hosts) ----------------

    #[test]
    fn file_test_e_nonexistent_is_false() {
        assert!(!evaluate(&s(&["-e", "/this/does/not/exist/__nope__"])));
    }

    #[test]
    fn file_test_f_nonexistent_is_false() {
        assert!(!evaluate(&s(&["-f", "/this/does/not/exist/__nope__"])));
    }

    #[test]
    fn file_test_d_nonexistent_is_false() {
        assert!(!evaluate(&s(&["-d", "/this/does/not/exist/__nope__"])));
    }

    #[test]
    fn file_test_s_nonexistent_is_false() {
        assert!(!evaluate(&s(&["-s", "/this/does/not/exist/__nope__"])));
    }

    // ---------------- unknown / malformed ----------------

    #[test]
    fn unknown_two_arg_op_is_false() {
        assert!(!evaluate(&s(&["-Q", "anything"])));
    }

    #[test]
    fn unknown_three_arg_op_is_false() {
        // Unknown infix operator: falls through past the 3-arg match
        // and through -a/-o checks; ends up false.
        assert!(!evaluate(&s(&["foo", "-bogus", "bar"])));
    }

    #[test]
    fn check_mode_on_nonexistent_is_false() {
        assert!(!check_mode("/this/does/not/exist/__nope__", 0o444));
    }
}
