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
//! Built only on unix-family targets (our x86_64-ouros presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.

#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
fn main() {
    eprintln!("test: unix-only utility; not supported on this platform");
    std::process::exit(2);
}

#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::process;

#[cfg(unix)]
fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = &args[0];

    // If invoked as "[", the last arg must be "]".
    let test_args = if prog.ends_with('[') || prog == "[" {
        if args.last().map_or(true, |a| a != "]") {
            eprintln!("[: missing ']'");
            process::exit(2);
        }
        &args[1..args.len() - 1]
    } else {
        &args[1..]
    };

    let result = evaluate(test_args);
    process::exit(if result { 0 } else { 1 });
}

#[cfg(unix)]
fn evaluate(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }

    // Handle ! (negation)
    if args[0] == "!" {
        return !evaluate(&args[1..]);
    }

    // Three-argument forms
    if args.len() == 3 {
        let a = &args[0];
        let op = &args[1];
        let b = &args[2];

        match op.as_str() {
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

    // Look for -a / -o (lowest precedence binary operators)
    // -o has lower precedence than -a
    for (i, arg) in args.iter().enumerate() {
        if arg == "-o" {
            return evaluate(&args[..i]) || evaluate(&args[i + 1..]);
        }
    }
    for (i, arg) in args.iter().enumerate() {
        if arg == "-a" {
            return evaluate(&args[..i]) && evaluate(&args[i + 1..]);
        }
    }

    // Two-argument forms (unary tests)
    if args.len() == 2 {
        let op = &args[0];
        let operand = &args[1];

        match op.as_str() {
            "-e" => return fs::symlink_metadata(operand).is_ok(),
            "-f" => return fs::metadata(operand).map_or(false, |m| m.is_file()),
            "-d" => return fs::metadata(operand).map_or(false, |m| m.is_dir()),
            "-L" | "-h" => return fs::symlink_metadata(operand)
                .map_or(false, |m| m.file_type().is_symlink()),
            "-r" => return fs::metadata(operand)
                .map_or(false, |m| m.permissions().mode() & 0o444 != 0),
            "-w" => return fs::metadata(operand)
                .map_or(false, |m| m.permissions().mode() & 0o222 != 0),
            "-x" => return fs::metadata(operand)
                .map_or(false, |m| m.permissions().mode() & 0o111 != 0),
            "-s" => return fs::metadata(operand).map_or(false, |m| m.len() > 0),
            "-n" => return !operand.is_empty(),
            "-z" => return operand.is_empty(),
            _ => {}
        }
    }

    // Single argument: true if non-empty string.
    if args.len() == 1 {
        return !args[0].is_empty();
    }

    // Fallback: unknown expression → false
    false
}

#[cfg(unix)]
fn int_cmp(a: &str, b: &str, cmp: impl Fn(i64, i64) -> bool) -> bool {
    let x = a.parse::<i64>().unwrap_or(0);
    let y = b.parse::<i64>().unwrap_or(0);
    cmp(x, y)
}
