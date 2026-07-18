//! Integer arithmetic evaluator for `$(( … ))` and (later) `(( … ))`.
//!
//! Supports the common C-like operators bash arithmetic exposes: `+ - * / %`,
//! comparisons, `&& || !`, bitwise `& | ^ ~ << >>`, parentheses, unary `+`/`-`,
//! and bare variable names (which resolve to their integer value, defaulting to
//! `0`). Numbers are 64-bit signed.

/// Resolves a bare variable name to its integer value during evaluation.
pub trait VarLookup {
    /// Return the variable's value, or `None` if unset (treated as `0`).
    fn get(&self, name: &str) -> Option<i64>;

    /// Return the integer value of the array element `name[index]`, or `None`
    /// if unset/out-of-range (treated as `0`). `index` has already been
    /// evaluated arithmetically (so `(( a[i+1] ))` and negative indices work).
    /// The default ignores subscripts — implementors backed by arrays override
    /// it.
    fn get_index(&self, name: &str, index: i64) -> Option<i64> {
        let _ = (name, index);
        None
    }

    /// Return `true` if `name` is an associative array. Bash evaluates the
    /// subscript of an associative array as a *string key* (not arithmetic),
    /// so the evaluator consults this before deciding how to read `name[sub]`.
    /// The default (`false`) means every array is treated as indexed.
    fn is_assoc(&self, name: &str) -> bool {
        let _ = name;
        false
    }

    /// Return the integer value of associative element `name[key]`, or `None`
    /// if unset (treated as `0`). `key` is the raw, already-expanded subscript
    /// text (bash does not arithmetic-evaluate associative subscripts). Only
    /// consulted when [`VarLookup::is_assoc`] returns `true`.
    fn get_assoc(&self, name: &str, key: &str) -> Option<i64> {
        let _ = (name, key);
        None
    }
}

/// An arithmetic evaluation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArithError(pub String);

impl core::fmt::Display for ArithError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Evaluate an arithmetic expression string.
///
/// # Errors
/// Returns [`ArithError`] on a syntax error or division by zero.
pub fn eval(expr: &str, vars: &dyn VarLookup) -> Result<i64, ArithError> {
    let mut p = AParser {
        chars: expr.chars().collect(),
        pos: 0,
        vars,
    };
    p.skip_ws();
    let v = p.parse_expr(0)?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(ArithError(format!(
            "unexpected trailing input in arithmetic: '{expr}'"
        )));
    }
    Ok(v)
}

struct AParser<'a> {
    chars: Vec<char>,
    pos: usize,
    vars: &'a dyn VarLookup,
}

/// Binary operator table entry: (token, left binding power, is right-assoc).
fn binop(sym: &str) -> Option<u8> {
    Some(match sym {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    })
}

impl AParser<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.chars.get(self.pos), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Read the operator symbol at the cursor (longest match), without
    /// consuming. Returns the symbol text.
    fn peek_op(&self) -> Option<String> {
        let two: String = self.chars[self.pos..].iter().take(2).collect();
        if matches!(
            two.as_str(),
            "||" | "&&" | "==" | "!=" | "<=" | ">=" | "<<" | ">>"
        ) {
            return Some(two);
        }
        let one = self.peek()?;
        if "+-*/%|^&<>".contains(one) {
            return Some(one.to_string());
        }
        None
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<i64, ArithError> {
        let mut lhs = self.parse_unary()?;
        loop {
            self.skip_ws();
            let Some(op) = self.peek_op() else { break };
            let Some(bp) = binop(&op) else { break };
            if bp < min_bp {
                break;
            }
            self.pos += op.chars().count();
            self.skip_ws();
            let rhs = self.parse_expr(bp + 1)?;
            lhs = apply(&op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<i64, ArithError> {
        self.skip_ws();
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                Ok(self.parse_unary()?.wrapping_neg())
            }
            Some('+') => {
                self.pos += 1;
                self.parse_unary()
            }
            Some('!') => {
                self.pos += 1;
                Ok(i64::from(self.parse_unary()? == 0))
            }
            Some('~') => {
                self.pos += 1;
                Ok(!self.parse_unary()?)
            }
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> Result<i64, ArithError> {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                let v = self.parse_expr(0)?;
                self.skip_ws();
                if self.peek() != Some(')') {
                    return Err(ArithError("expected ')'".into()));
                }
                self.pos += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() => self.parse_number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                // Array subscript: `name[sub]`. For an *indexed* array the
                // subscript is an arithmetic expression (`a[i+1]`, negative
                // indices); for an *associative* array it is a literal string
                // key (`m[foo]`). We first capture the raw bracketed text with
                // balanced-bracket matching, then dispatch on the array kind.
                // No whitespace is allowed between the name and `[`.
                if self.peek() == Some('[') {
                    self.pos += 1;
                    let sub_start = self.pos;
                    let mut depth = 1usize;
                    while let Some(c) = self.peek() {
                        match c {
                            '[' => depth += 1,
                            ']' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                    if self.peek() != Some(']') {
                        return Err(ArithError("expected ']' in array subscript".into()));
                    }
                    let raw: String = self.chars[sub_start..self.pos].iter().collect();
                    self.pos += 1; // consume the closing ']'
                    if self.vars.is_assoc(&name) {
                        // Associative: the (trimmed) raw text is the key.
                        return Ok(self.vars.get_assoc(&name, raw.trim()).unwrap_or(0));
                    }
                    // Indexed: evaluate the subscript arithmetically.
                    let idx = eval(&raw, self.vars)?;
                    return Ok(self.vars.get_index(&name, idx).unwrap_or(0));
                }
                Ok(self.vars.get(&name).unwrap_or(0))
            }
            other => Err(ArithError(format!(
                "unexpected character in arithmetic: {other:?}"
            ))),
        }
    }

    fn parse_number(&mut self) -> Result<i64, ArithError> {
        let start = self.pos;
        // Support 0x.. hex and plain decimal.
        if self.peek() == Some('0') && matches!(self.chars.get(self.pos + 1), Some('x' | 'X')) {
            self.pos += 2;
            let hstart = self.pos;
            while matches!(self.peek(), Some(c) if c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let hex: String = self.chars[hstart..self.pos].iter().collect();
            return i64::from_str_radix(&hex, 16)
                .map_err(|_| ArithError(format!("bad hex literal '0x{hex}'")));
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        let num: String = self.chars[start..self.pos].iter().collect();
        num.parse::<i64>()
            .map_err(|_| ArithError(format!("bad number '{num}'")))
    }
}

fn apply(op: &str, a: i64, b: i64) -> Result<i64, ArithError> {
    Ok(match op {
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" => {
            if b == 0 {
                return Err(ArithError("division by zero".into()));
            }
            a.wrapping_div(b)
        }
        "%" => {
            if b == 0 {
                return Err(ArithError("modulo by zero".into()));
            }
            a.wrapping_rem(b)
        }
        "<<" => a.wrapping_shl(u32::try_from(b).unwrap_or(0)),
        ">>" => a.wrapping_shr(u32::try_from(b).unwrap_or(0)),
        "<" => i64::from(a < b),
        ">" => i64::from(a > b),
        "<=" => i64::from(a <= b),
        ">=" => i64::from(a >= b),
        "==" => i64::from(a == b),
        "!=" => i64::from(a != b),
        "&" => a & b,
        "^" => a ^ b,
        "|" => a | b,
        "&&" => i64::from(a != 0 && b != 0),
        "||" => i64::from(a != 0 || b != 0),
        _ => return Err(ArithError(format!("unknown operator '{op}'"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Map(HashMap<String, i64>);
    impl VarLookup for Map {
        fn get(&self, name: &str) -> Option<i64> {
            self.0.get(name).copied()
        }
    }

    /// A lookup with one indexed array `a` plus scalar variables, so the
    /// subscript path can be exercised in isolation.
    struct ArrMap {
        scalars: HashMap<String, i64>,
        a: Vec<i64>,
    }
    impl VarLookup for ArrMap {
        fn get(&self, name: &str) -> Option<i64> {
            self.scalars.get(name).copied()
        }
        fn get_index(&self, name: &str, index: i64) -> Option<i64> {
            if name != "a" {
                return None;
            }
            let real = if index < 0 {
                i64::try_from(self.a.len()).ok()? + index
            } else {
                index
            };
            usize::try_from(real).ok().and_then(|i| self.a.get(i).copied())
        }
    }

    #[test]
    fn array_subscripts() {
        let mut scalars = HashMap::new();
        scalars.insert("i".to_string(), 2);
        let m = ArrMap {
            scalars,
            a: vec![10, 20, 30, 40],
        };
        assert_eq!(eval("a[0]", &m).unwrap(), 10);
        assert_eq!(eval("a[i]", &m).unwrap(), 30); // i = 2
        assert_eq!(eval("a[i+1] + 1", &m).unwrap(), 41); // a[3]=40, +1
        assert_eq!(eval("a[-1]", &m).unwrap(), 40); // negative from end
        assert_eq!(eval("a[10]", &m).unwrap(), 0); // out of range → 0
        // Missing ']' is a syntax error.
        assert!(eval("a[1", &m).is_err());
    }

    /// A lookup with one associative array `m` keyed by strings, to exercise
    /// the associative subscript path (`m[key]` uses the raw text as key, not
    /// an arithmetic expression).
    struct AssocMap(HashMap<String, i64>);
    impl VarLookup for AssocMap {
        fn get(&self, _name: &str) -> Option<i64> {
            None
        }
        fn is_assoc(&self, name: &str) -> bool {
            name == "m"
        }
        fn get_assoc(&self, name: &str, key: &str) -> Option<i64> {
            if name != "m" {
                return None;
            }
            self.0.get(key).copied()
        }
    }

    #[test]
    fn associative_subscripts() {
        let mut kv = HashMap::new();
        kv.insert("foo".to_string(), 7);
        kv.insert("bar".to_string(), 13);
        let m = AssocMap(kv);
        // The subscript is a literal string key, not arithmetic.
        assert_eq!(eval("m[foo]", &m).unwrap(), 7);
        assert_eq!(eval("m[bar] + 1", &m).unwrap(), 14);
        // A key that looks like an operator expression is still literal.
        assert_eq!(eval("m[missing]", &m).unwrap(), 0); // unset → 0
        // Whitespace around the key is trimmed.
        assert_eq!(eval("m[ foo ]", &m).unwrap(), 7);
    }

    fn ev(s: &str) -> i64 {
        eval(s, &Map(HashMap::new())).unwrap()
    }

    #[test]
    fn precedence() {
        assert_eq!(ev("1 + 2 * 3"), 7);
        assert_eq!(ev("(1 + 2) * 3"), 9);
        assert_eq!(ev("10 % 3"), 1);
        assert_eq!(ev("2 * 3 == 6"), 1);
        assert_eq!(ev("1 < 2 && 3 > 2"), 1);
    }

    #[test]
    fn unary_and_bits() {
        assert_eq!(ev("-5 + 3"), -2);
        assert_eq!(ev("!0"), 1);
        assert_eq!(ev("~0"), -1);
        assert_eq!(ev("1 << 4"), 16);
        assert_eq!(ev("0xff & 0x0f"), 15);
    }

    #[test]
    fn variables() {
        let mut m = HashMap::new();
        m.insert("x".to_string(), 10);
        m.insert("y".to_string(), 4);
        assert_eq!(eval("x * y + 2", &Map(m)).unwrap(), 42);
    }

    #[test]
    fn div_zero() {
        assert!(eval("1 / 0", &Map(HashMap::new())).is_err());
    }
}
