//! expr — evaluate expressions.
//!
//! Usage: expr EXPRESSION
//!   Supports:
//!     Arithmetic: +, -, *, /, %
//!     Comparison: =, !=, <, <=, >, >=
//!     String:     match STRING REGEX, length STRING
//!     Logical:    |, &
//!
//!   Returns the result on stdout. Exit 0 if result is non-null/non-zero,
//!   exit 1 otherwise.

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("expr: missing operand");
        process::exit(2);
    }

    let tokens: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let mut pos = 0;
    let result = parse_or(&tokens, &mut pos);

    if pos != tokens.len() {
        eprintln!("expr: syntax error");
        process::exit(2);
    }

    println!("{result}");

    // Exit 1 if result is 0 or empty string, 0 otherwise.
    if result == "0" || result.is_empty() {
        process::exit(1);
    }
}

fn parse_or(tokens: &[&str], pos: &mut usize) -> String {
    let mut left = parse_and(tokens, pos);
    while *pos < tokens.len() && tokens[*pos] == "|" {
        *pos += 1;
        let right = parse_and(tokens, pos);
        if left != "0" && !left.is_empty() {
            // left is truthy — keep it
        } else {
            left = right;
        }
    }
    left
}

fn parse_and(tokens: &[&str], pos: &mut usize) -> String {
    let mut left = parse_comparison(tokens, pos);
    while *pos < tokens.len() && tokens[*pos] == "&" {
        *pos += 1;
        let right = parse_comparison(tokens, pos);
        if (left == "0" || left.is_empty()) || (right == "0" || right.is_empty()) {
            left = "0".to_string();
        }
        // else keep left
    }
    left
}

fn parse_comparison(tokens: &[&str], pos: &mut usize) -> String {
    let left = parse_add(tokens, pos);
    if *pos >= tokens.len() {
        return left;
    }

    let op = tokens[*pos];
    match op {
        "=" | "==" | "!=" | "<" | "<=" | ">" | ">=" => {
            *pos += 1;
            let right = parse_add(tokens, pos);

            // Try integer comparison first
            if let (Ok(l), Ok(r)) = (left.parse::<i64>(), right.parse::<i64>()) {
                let result = match op {
                    "=" | "==" => l == r,
                    "!=" => l != r,
                    "<" => l < r,
                    "<=" => l <= r,
                    ">" => l > r,
                    ">=" => l >= r,
                    _ => false,
                };
                return if result { "1" } else { "0" }.to_string();
            }

            // Fall back to string comparison
            let result = match op {
                "=" | "==" => left == right,
                "!=" => left != right,
                "<" => left < right,
                "<=" => left <= right,
                ">" => left > right,
                ">=" => left >= right,
                _ => false,
            };
            if result { "1" } else { "0" }.to_string()
        }
        _ => left,
    }
}

fn parse_add(tokens: &[&str], pos: &mut usize) -> String {
    let mut left = parse_mul(tokens, pos);
    while *pos < tokens.len() {
        let op = tokens[*pos];
        if op == "+" || op == "-" {
            *pos += 1;
            let right = parse_mul(tokens, pos);
            let l: i64 = left.parse().unwrap_or(0);
            let r: i64 = right.parse().unwrap_or(0);
            left = match op {
                "+" => (l + r).to_string(),
                "-" => (l - r).to_string(),
                _ => left,
            };
        } else {
            break;
        }
    }
    left
}

fn parse_mul(tokens: &[&str], pos: &mut usize) -> String {
    let mut left = parse_primary(tokens, pos);
    while *pos < tokens.len() {
        let op = tokens[*pos];
        if op == "*" || op == "/" || op == "%" {
            *pos += 1;
            let right = parse_primary(tokens, pos);
            let l: i64 = left.parse().unwrap_or(0);
            let r: i64 = right.parse().unwrap_or(0);
            left = match op {
                "*" => (l * r).to_string(),
                "/" => {
                    if r == 0 {
                        eprintln!("expr: division by zero");
                        process::exit(2);
                    }
                    (l / r).to_string()
                }
                "%" => {
                    if r == 0 {
                        eprintln!("expr: division by zero");
                        process::exit(2);
                    }
                    (l % r).to_string()
                }
                _ => left,
            };
        } else {
            break;
        }
    }
    left
}

fn parse_primary(tokens: &[&str], pos: &mut usize) -> String {
    if *pos >= tokens.len() {
        eprintln!("expr: syntax error");
        process::exit(2);
    }

    let token = tokens[*pos];

    // Parenthesized expression
    if token == "(" {
        *pos += 1;
        let result = parse_or(tokens, pos);
        if *pos < tokens.len() && tokens[*pos] == ")" {
            *pos += 1;
        } else {
            eprintln!("expr: syntax error: missing ')'");
            process::exit(2);
        }
        return result;
    }

    // "length" keyword
    if token == "length" && *pos + 1 < tokens.len() {
        *pos += 2;
        return tokens[*pos - 1].len().to_string();
    }

    // Literal value
    *pos += 1;
    token.to_string()
}
