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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// Convenience wrapper: parse a full expression from a tokenized array
    /// and assert the whole input was consumed.
    fn eval(tokens: &[&str]) -> String {
        let mut pos = 0;
        let result = parse_or(tokens, &mut pos);
        assert_eq!(pos, tokens.len(), "expected all tokens consumed");
        result
    }

    // ---------------- arithmetic ----------------

    #[test]
    fn addition() {
        assert_eq!(eval(&["2", "+", "3"]), "5");
    }

    #[test]
    fn subtraction() {
        assert_eq!(eval(&["10", "-", "4"]), "6");
    }

    #[test]
    fn multiplication() {
        assert_eq!(eval(&["3", "*", "4"]), "12");
    }

    #[test]
    fn division_integer() {
        assert_eq!(eval(&["7", "/", "2"]), "3");
    }

    #[test]
    fn modulo() {
        assert_eq!(eval(&["7", "%", "3"]), "1");
    }

    #[test]
    fn precedence_mul_before_add() {
        // 2 + 3 * 4 = 14
        assert_eq!(eval(&["2", "+", "3", "*", "4"]), "14");
    }

    #[test]
    fn left_associative_subtraction() {
        // (10 - 3) - 2 = 5, not 10 - (3 - 2) = 9
        assert_eq!(eval(&["10", "-", "3", "-", "2"]), "5");
    }

    #[test]
    fn parentheses_override_precedence() {
        // (2 + 3) * 4 = 20
        assert_eq!(eval(&["(", "2", "+", "3", ")", "*", "4"]), "20");
    }

    #[test]
    fn negative_result() {
        assert_eq!(eval(&["3", "-", "10"]), "-7");
    }

    // ---------------- comparison ----------------

    #[test]
    fn integer_equality_true() {
        assert_eq!(eval(&["5", "=", "5"]), "1");
    }

    #[test]
    fn integer_equality_false() {
        assert_eq!(eval(&["5", "=", "6"]), "0");
    }

    #[test]
    fn integer_not_equal() {
        assert_eq!(eval(&["5", "!=", "6"]), "1");
        assert_eq!(eval(&["5", "!=", "5"]), "0");
    }

    #[test]
    fn integer_less_than() {
        assert_eq!(eval(&["3", "<", "5"]), "1");
        assert_eq!(eval(&["5", "<", "5"]), "0");
    }

    #[test]
    fn integer_less_or_equal() {
        assert_eq!(eval(&["5", "<=", "5"]), "1");
        assert_eq!(eval(&["6", "<=", "5"]), "0");
    }

    #[test]
    fn integer_greater_than() {
        assert_eq!(eval(&["6", ">", "5"]), "1");
        assert_eq!(eval(&["5", ">", "5"]), "0");
    }

    #[test]
    fn string_comparison_lexicographic() {
        assert_eq!(eval(&["apple", "<", "banana"]), "1");
        assert_eq!(eval(&["banana", "<", "apple"]), "0");
    }

    #[test]
    fn string_equality() {
        assert_eq!(eval(&["foo", "=", "foo"]), "1");
        assert_eq!(eval(&["foo", "=", "bar"]), "0");
    }

    // ---------------- logical ----------------

    #[test]
    fn or_returns_first_truthy() {
        assert_eq!(eval(&["hello", "|", "world"]), "hello");
    }

    #[test]
    fn or_falls_through_falsy() {
        // 0 is falsy in expr; expression yields the second operand.
        assert_eq!(eval(&["0", "|", "world"]), "world");
        assert_eq!(eval(&["", "|", "world"]), "world");
    }

    #[test]
    fn and_returns_first_when_both_truthy() {
        assert_eq!(eval(&["foo", "&", "bar"]), "foo");
    }

    #[test]
    fn and_returns_zero_when_either_falsy() {
        assert_eq!(eval(&["0", "&", "bar"]), "0");
        assert_eq!(eval(&["foo", "&", "0"]), "0");
        assert_eq!(eval(&["", "&", "bar"]), "0");
    }

    // ---------------- length ----------------

    #[test]
    fn length_of_string() {
        assert_eq!(eval(&["length", "hello"]), "5");
    }

    #[test]
    fn length_empty_string() {
        assert_eq!(eval(&["length", ""]), "0");
    }

    // ---------------- primary / literal ----------------

    #[test]
    fn bare_literal_passes_through() {
        assert_eq!(eval(&["hello"]), "hello");
        assert_eq!(eval(&["42"]), "42");
    }

    #[test]
    fn parens_around_literal() {
        assert_eq!(eval(&["(", "42", ")"]), "42");
    }
}
