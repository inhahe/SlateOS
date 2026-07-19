//! Integer arithmetic evaluator for `$(( … ))` and `(( … ))`.
//!
//! Supports the operator set bash arithmetic exposes: `+ - * / % **`,
//! comparisons, `&& || !`, bitwise `& | ^ ~ << >>`, the ternary conditional
//! `?:`, the comma operator, parentheses, unary `+`/`-`, **assignment**
//! (`= += -= *= /= %= <<= >>= &= |= ^=`), **pre/post increment/decrement**
//! (`++x`, `x++`, `--x`, `x--`), and bare variable names (which resolve to
//! their integer value, defaulting to `0`). Array elements (`a[i]` arithmetic
//! index, `m[key]` associative string key) resolve and assign via
//! [`VarLookup`]. Numbers are 64-bit signed.
//!
//! Expressions are parsed into a small [`Expr`] AST and then evaluated against
//! a mutable [`VarLookup`]. The two-phase design is what makes assignment
//! possible: an lvalue (`x`, `a[i]`, `m[key]`) can be recognised structurally
//! before its right-hand side is evaluated, and `&&`/`||`/`?:` short-circuit so
//! side effects only happen on the branch actually taken.

/// Resolves and mutates variables during arithmetic evaluation.
///
/// The read methods (`get`/`get_index`/`get_assoc`) return `None` for an unset
/// variable/element (the evaluator treats that as `0`). The write methods have
/// empty defaults so a read-only implementor need not provide them.
pub trait VarLookup {
    /// Return the scalar variable's value, or `None` if unset (treated as `0`).
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

    /// Assign `value` to the scalar variable `name` (arithmetic `x = …`).
    fn set(&mut self, name: &str, value: i64) {
        let _ = (name, value);
    }

    /// Assign `value` to the indexed element `name[index]` (`a[i] = …`).
    fn set_index(&mut self, name: &str, index: i64, value: i64) {
        let _ = (name, index, value);
    }

    /// Assign `value` to the associative element `name[key]` (`m[key] = …`).
    fn set_assoc(&mut self, name: &str, key: &str, value: i64) {
        let _ = (name, key, value);
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

/// A parsed arithmetic expression.
#[derive(Debug, Clone)]
enum Expr {
    Num(i64),
    /// Bare scalar variable.
    Var(String),
    /// Indexed array element `name[index]` (subscript is arithmetic).
    Index(String, Box<Expr>),
    /// Associative array element `name[key]` (subscript is a literal key).
    Assoc(String, String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    BitNot(Box<Expr>),
    /// A binary operation; the operator is one of the [`apply`]/short-circuit
    /// tokens (`+`, `-`, `*`, `/`, `%`, `**`, `<<`, `>>`, comparisons, `&`,
    /// `^`, `|`, `&&`, `||`).
    Bin(String, Box<Expr>, Box<Expr>),
    /// `cond ? then : else`.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `left , right` — evaluate both, yield `right`.
    Comma(Box<Expr>, Box<Expr>),
    /// Assignment. `op` is `None` for plain `=`, or `Some(base)` for a compound
    /// assignment whose base binary operator is `base` (e.g. `+=` → `"+"`).
    Assign(Lvalue, Option<String>, Box<Expr>),
    /// Pre-increment/decrement (`++x`/`--x`): mutate, then yield the new value.
    /// `true` = increment, `false` = decrement.
    PreIncr(Lvalue, bool),
    /// Post-increment/decrement (`x++`/`x--`): yield the old value, then mutate.
    PostIncr(Lvalue, bool),
}

/// An assignable location (the left side of `=`, `+=`, `++`, …).
#[derive(Debug, Clone)]
enum Lvalue {
    Var(String),
    Index(String, Box<Expr>),
    Assoc(String, String),
}

/// A resolved location — array subscripts already evaluated to a concrete
/// index/key, so load and store don't re-evaluate (and re-trigger side
/// effects) the subscript expression.
enum ResolvedLv {
    Var(String),
    Index(String, i64),
    Assoc(String, String),
}

/// Evaluate an arithmetic expression string against a mutable variable
/// environment (assignment/increment operators mutate `vars`).
///
/// # Errors
/// Returns [`ArithError`] on a syntax error, division/modulo by zero, a
/// negative exponent, or assignment to a non-lvalue.
pub fn eval(expr: &str, vars: &mut dyn VarLookup) -> Result<i64, ArithError> {
    // Parse with an immutable borrow, then evaluate with the mutable borrow.
    let ast = parse(expr, &*vars)?;
    eval_expr(&ast, vars)
}

/// Parse an arithmetic expression into an AST (no evaluation, no mutation).
fn parse(expr: &str, vars: &dyn VarLookup) -> Result<Expr, ArithError> {
    let mut p = AParser {
        chars: expr.chars().collect(),
        pos: 0,
        vars,
    };
    p.skip_ws();
    // An empty (or whitespace-only) arithmetic expression is `0` in bash:
    // `$(( ))`, and — after expansion — `n=; echo $((n))` / `$(( $x ))`.
    if p.pos == p.chars.len() {
        return Ok(Expr::Num(0));
    }
    let e = p.parse_comma()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(ArithError(format!(
            "unexpected trailing input in arithmetic: '{expr}'"
        )));
    }
    Ok(e)
}

struct AParser<'a> {
    chars: Vec<char>,
    pos: usize,
    vars: &'a dyn VarLookup,
}

/// Binding power (and right-associativity) of a binary operator, or `None` if
/// `sym` is not a binary operator. Higher power binds tighter.
fn binop_bp(sym: &str) -> Option<(u8, bool)> {
    Some(match sym {
        "||" => (1, false),
        "&&" => (2, false),
        "|" => (3, false),
        "^" => (4, false),
        "&" => (5, false),
        "==" | "!=" => (6, false),
        "<" | ">" | "<=" | ">=" => (7, false),
        "<<" | ">>" => (8, false),
        "+" | "-" => (9, false),
        "*" | "/" | "%" => (10, false),
        "**" => (11, true), // exponentiation, right-associative
        _ => return None,
    })
}

/// Is `sym` an assignment operator (`=`, `+=`, `<<=`, …)?
fn is_assign_op(sym: &str) -> bool {
    matches!(
        sym,
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "|=" | "^="
    )
}

/// The base binary operator of an assignment operator (`+=` → `Some("+")`),
/// or `None` for plain `=`.
fn assign_base(sym: &str) -> Option<String> {
    match sym {
        "=" => None,
        other => Some(other.trim_end_matches('=').to_string()),
    }
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

    /// The longest operator token at the cursor (without consuming). Recognises
    /// 3-, 2-, and 1-character operators, including assignment and
    /// increment/decrement forms so the binary-operator parser can tell `+`
    /// from `+=`/`++`.
    fn read_op(&self) -> Option<String> {
        let three: String = self.chars[self.pos..].iter().take(3).collect();
        if matches!(three.as_str(), "<<=" | ">>=") {
            return Some(three);
        }
        let two: String = self.chars[self.pos..].iter().take(2).collect();
        if matches!(
            two.as_str(),
            "**" | "==" | "!=" | "<=" | ">=" | "<<" | ">>" | "&&" | "||" | "++" | "--" | "+="
                | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^="
        ) {
            return Some(two);
        }
        let one = self.peek()?;
        if "+-*/%|^&<>=!~".contains(one) {
            return Some(one.to_string());
        }
        None
    }

    /// Comma operator (`e1, e2, …`) — the loosest-binding arithmetic operator.
    fn parse_comma(&mut self) -> Result<Expr, ArithError> {
        let mut e = self.parse_assign()?;
        loop {
            self.skip_ws();
            if self.peek() == Some(',') {
                self.pos += 1;
                let r = self.parse_assign()?;
                e = Expr::Comma(Box::new(e), Box::new(r));
            } else {
                break;
            }
        }
        Ok(e)
    }

    /// Assignment (`lv = e`, `lv += e`, …) — right-associative, binds looser
    /// than the ternary. If no assignment operator follows, this is just the
    /// ternary it parsed.
    fn parse_assign(&mut self) -> Result<Expr, ArithError> {
        let lhs = self.parse_ternary()?;
        self.skip_ws();
        if let Some(op) = self.read_op()
            && is_assign_op(&op)
        {
            let lv = lvalue_of(lhs)?;
            self.pos += op.chars().count();
            let rhs = self.parse_assign()?;
            return Ok(Expr::Assign(lv, assign_base(&op), Box::new(rhs)));
        }
        Ok(lhs)
    }

    /// Ternary conditional `cond ? then : else` — right-associative.
    fn parse_ternary(&mut self) -> Result<Expr, ArithError> {
        let cond = self.parse_binary(0)?;
        self.skip_ws();
        if self.peek() != Some('?') {
            return Ok(cond);
        }
        self.pos += 1; // consume '?'
        // The middle may itself be an assignment (`c ? x = 1 : y`).
        let then_e = self.parse_assign()?;
        self.skip_ws();
        if self.peek() != Some(':') {
            return Err(ArithError("expected ':' in ternary expression".into()));
        }
        self.pos += 1; // consume ':'
        let else_e = self.parse_ternary()?;
        Ok(Expr::Ternary(
            Box::new(cond),
            Box::new(then_e),
            Box::new(else_e),
        ))
    }

    /// Precedence-climbing parse of binary operators (`||` … `**`).
    fn parse_binary(&mut self, min_bp: u8) -> Result<Expr, ArithError> {
        let mut lhs = self.parse_unary()?;
        loop {
            self.skip_ws();
            let Some(op) = self.read_op() else { break };
            let Some((bp, right)) = binop_bp(&op) else {
                break;
            };
            if bp < min_bp {
                break;
            }
            self.pos += op.chars().count();
            let next_min = if right { bp } else { bp + 1 };
            let rhs = self.parse_binary(next_min)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ArithError> {
        self.skip_ws();
        if let Some(op) = self.read_op()
            && (op == "++" || op == "--")
        {
            self.pos += 2;
            let operand = self.parse_unary()?;
            let lv = lvalue_of(operand)?;
            return Ok(Expr::PreIncr(lv, op == "++"));
        }
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            Some('+') => {
                self.pos += 1;
                self.parse_unary()
            }
            Some('!') => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            Some('~') => {
                self.pos += 1;
                Ok(Expr::BitNot(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_postfix(),
        }
    }

    /// A primary atom followed by an optional postfix `++`/`--`.
    fn parse_postfix(&mut self) -> Result<Expr, ArithError> {
        let e = self.parse_atom()?;
        self.skip_ws();
        if let Some(op) = self.read_op()
            && (op == "++" || op == "--")
        {
            let lv = lvalue_of(e)?;
            self.pos += 2;
            return Ok(Expr::PostIncr(lv, op == "++"));
        }
        Ok(e)
    }

    fn parse_atom(&mut self) -> Result<Expr, ArithError> {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                // A parenthesised group is a full expression: ternary, comma,
                // and assignment are allowed inside.
                let e = self.parse_comma()?;
                self.skip_ws();
                if self.peek() != Some(')') {
                    return Err(ArithError("expected ')'".into()));
                }
                self.pos += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() => Ok(Expr::Num(self.parse_number()?)),
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
                // Array subscript `name[sub]`: for an indexed array the
                // subscript is an arithmetic expression (`a[i+1]`, negatives);
                // for an associative array it is a literal string key
                // (`m[foo]`). Capture the raw bracketed text (balanced
                // brackets), then dispatch on the array kind. No whitespace is
                // allowed between the name and `[`.
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
                        return Ok(Expr::Assoc(name, raw.trim().to_string()));
                    }
                    // Indexed: parse the subscript as its own arithmetic
                    // expression (evaluated later against the live environment).
                    let sub_ast = parse(&raw, self.vars)?;
                    return Ok(Expr::Index(name, Box::new(sub_ast)));
                }
                Ok(Expr::Var(name))
            }
            other => Err(ArithError(format!(
                "unexpected character in arithmetic: {other:?}"
            ))),
        }
    }

    fn parse_number(&mut self) -> Result<i64, ArithError> {
        let start = self.pos;
        // 0x / 0X hexadecimal.
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
        // Collect the leading decimal run. It is either the whole number, an
        // octal literal (leading zero), or the base of a `base#num` literal.
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        // base#num — bash arbitrary-base literals, base 2..=64.
        if self.peek() == Some('#') {
            let base_str: String = self.chars[start..self.pos].iter().collect();
            let base: u32 = base_str
                .parse()
                .map_err(|_| ArithError(format!("bad arithmetic base '{base_str}'")))?;
            if !(2..=64).contains(&base) {
                return Err(ArithError(format!("invalid arithmetic base ({base})")));
            }
            self.pos += 1; // consume '#'
            let dstart = self.pos;
            let mut val: i64 = 0;
            while let Some(c) = self.peek() {
                let Some(d) = digit_value(c, base) else { break };
                self.pos += 1;
                val = val
                    .wrapping_mul(i64::from(base))
                    .wrapping_add(i64::from(d));
            }
            if self.pos == dstart {
                return Err(ArithError(format!("missing digits in base-{base} literal")));
            }
            return Ok(val);
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // A leading zero (other than bare "0") denotes octal.
        if text.len() > 1 && text.starts_with('0') {
            return i64::from_str_radix(&text, 8)
                .map_err(|_| ArithError(format!("bad octal literal '{text}'")));
        }
        text.parse::<i64>()
            .map_err(|_| ArithError(format!("bad number '{text}'")))
    }
}

/// Value of `c` as a digit in `base` (bash `base#num` semantics), or `None` if
/// `c` is not a valid digit for that base. Digits above 9 use the lowercase
/// letters, then the uppercase letters, then `@`, then `_`. For bases <= 36 the
/// letter cases are interchangeable; for larger bases lowercase is 10..=35 and
/// uppercase is 36..=61.
fn digit_value(c: char, base: u32) -> Option<u32> {
    let v = match c {
        '0'..='9' => c as u32 - '0' as u32,
        'a'..='z' => 10 + (c as u32 - 'a' as u32),
        'A'..='Z' => {
            if base <= 36 {
                10 + (c as u32 - 'A' as u32)
            } else {
                36 + (c as u32 - 'A' as u32)
            }
        }
        '@' => 62,
        '_' => 63,
        _ => return None,
    };
    if v < base { Some(v) } else { None }
}

/// Convert a parsed expression into an lvalue, or error if it is not
/// assignable (bash: "attempted assignment to non-variable").
fn lvalue_of(e: Expr) -> Result<Lvalue, ArithError> {
    match e {
        Expr::Var(n) => Ok(Lvalue::Var(n)),
        Expr::Index(n, ix) => Ok(Lvalue::Index(n, ix)),
        Expr::Assoc(n, k) => Ok(Lvalue::Assoc(n, k)),
        _ => Err(ArithError("attempted assignment to non-variable".into())),
    }
}

fn eval_expr(e: &Expr, vars: &mut dyn VarLookup) -> Result<i64, ArithError> {
    match e {
        Expr::Num(n) => Ok(*n),
        Expr::Var(n) => Ok(vars.get(n).unwrap_or(0)),
        Expr::Index(n, ix) => {
            let i = eval_expr(ix, vars)?;
            Ok(vars.get_index(n, i).unwrap_or(0))
        }
        Expr::Assoc(n, k) => Ok(vars.get_assoc(n, k).unwrap_or(0)),
        Expr::Neg(x) => Ok(eval_expr(x, vars)?.wrapping_neg()),
        Expr::Not(x) => Ok(i64::from(eval_expr(x, vars)? == 0)),
        Expr::BitNot(x) => Ok(!eval_expr(x, vars)?),
        Expr::Bin(op, l, r) => match op.as_str() {
            // Short-circuit: the right operand's side effects only happen when
            // the left doesn't already decide the result.
            "&&" => {
                if eval_expr(l, vars)? == 0 {
                    Ok(0)
                } else {
                    Ok(i64::from(eval_expr(r, vars)? != 0))
                }
            }
            "||" => {
                if eval_expr(l, vars)? != 0 {
                    Ok(1)
                } else {
                    Ok(i64::from(eval_expr(r, vars)? != 0))
                }
            }
            _ => {
                let a = eval_expr(l, vars)?;
                let b = eval_expr(r, vars)?;
                apply(op, a, b)
            }
        },
        Expr::Ternary(c, t, f) => {
            if eval_expr(c, vars)? != 0 {
                eval_expr(t, vars)
            } else {
                eval_expr(f, vars)
            }
        }
        Expr::Comma(l, r) => {
            eval_expr(l, vars)?;
            eval_expr(r, vars)
        }
        Expr::Assign(lv, base, rhs) => {
            let loc = resolve_lv(lv, vars)?;
            let v = match base {
                None => eval_expr(rhs, vars)?,
                Some(op) => {
                    let cur = load_rlv(&loc, vars);
                    let b = eval_expr(rhs, vars)?;
                    apply(op, cur, b)?
                }
            };
            store_rlv(&loc, v, vars);
            Ok(v)
        }
        Expr::PreIncr(lv, inc) => {
            let loc = resolve_lv(lv, vars)?;
            let step = if *inc { 1 } else { -1 };
            let v = load_rlv(&loc, vars).wrapping_add(step);
            store_rlv(&loc, v, vars);
            Ok(v)
        }
        Expr::PostIncr(lv, inc) => {
            let loc = resolve_lv(lv, vars)?;
            let old = load_rlv(&loc, vars);
            let step = if *inc { 1 } else { -1 };
            store_rlv(&loc, old.wrapping_add(step), vars);
            Ok(old)
        }
    }
}

/// Resolve an lvalue's location once (evaluating an index subscript), so a
/// read-modify-write op doesn't evaluate the subscript twice.
fn resolve_lv(lv: &Lvalue, vars: &mut dyn VarLookup) -> Result<ResolvedLv, ArithError> {
    Ok(match lv {
        Lvalue::Var(n) => ResolvedLv::Var(n.clone()),
        Lvalue::Index(n, ix) => {
            let i = eval_expr(ix, vars)?;
            ResolvedLv::Index(n.clone(), i)
        }
        Lvalue::Assoc(n, k) => ResolvedLv::Assoc(n.clone(), k.clone()),
    })
}

fn load_rlv(loc: &ResolvedLv, vars: &dyn VarLookup) -> i64 {
    match loc {
        ResolvedLv::Var(n) => vars.get(n).unwrap_or(0),
        ResolvedLv::Index(n, i) => vars.get_index(n, *i).unwrap_or(0),
        ResolvedLv::Assoc(n, k) => vars.get_assoc(n, k).unwrap_or(0),
    }
}

fn store_rlv(loc: &ResolvedLv, v: i64, vars: &mut dyn VarLookup) {
    match loc {
        ResolvedLv::Var(n) => vars.set(n, v),
        ResolvedLv::Index(n, i) => vars.set_index(n, *i, v),
        ResolvedLv::Assoc(n, k) => vars.set_assoc(n, k, v),
    }
}

fn apply(op: &str, a: i64, b: i64) -> Result<i64, ArithError> {
    Ok(match op {
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "**" => {
            if b < 0 {
                return Err(ArithError("exponent less than 0".into()));
            }
            let exp = u32::try_from(b).map_err(|_| ArithError("exponent too large".into()))?;
            a.wrapping_pow(exp)
        }
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

    #[derive(Default)]
    struct Map(HashMap<String, i64>);
    impl VarLookup for Map {
        fn get(&self, name: &str) -> Option<i64> {
            self.0.get(name).copied()
        }
        fn set(&mut self, name: &str, value: i64) {
            self.0.insert(name.to_string(), value);
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
        fn set(&mut self, name: &str, value: i64) {
            self.scalars.insert(name.to_string(), value);
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
        fn set_index(&mut self, name: &str, index: i64, value: i64) {
            if name != "a" {
                return;
            }
            if let Ok(i) = usize::try_from(index)
                && i < self.a.len()
            {
                self.a[i] = value;
            }
        }
    }

    #[test]
    fn array_subscripts() {
        let mut scalars = HashMap::new();
        scalars.insert("i".to_string(), 2);
        let mut m = ArrMap {
            scalars,
            a: vec![10, 20, 30, 40],
        };
        assert_eq!(eval("a[0]", &mut m).unwrap(), 10);
        assert_eq!(eval("a[i]", &mut m).unwrap(), 30); // i = 2
        assert_eq!(eval("a[i+1] + 1", &mut m).unwrap(), 41); // a[3]=40, +1
        assert_eq!(eval("a[-1]", &mut m).unwrap(), 40); // negative from end
        assert_eq!(eval("a[10]", &mut m).unwrap(), 0); // out of range → 0
        // Missing ']' is a syntax error.
        assert!(eval("a[1", &mut m).is_err());
    }

    #[test]
    fn indexed_assignment_and_incr() {
        let mut m = ArrMap {
            scalars: HashMap::new(),
            a: vec![10, 20, 30],
        };
        assert_eq!(eval("a[0] = 99", &mut m).unwrap(), 99);
        assert_eq!(m.a[0], 99);
        assert_eq!(eval("a[1] += 5", &mut m).unwrap(), 25);
        assert_eq!(m.a[1], 25);
        // Post-increment yields the old value, then mutates.
        assert_eq!(eval("a[2]++", &mut m).unwrap(), 30);
        assert_eq!(m.a[2], 31);
    }

    /// A lookup with one associative array `m` keyed by strings.
    #[derive(Default)]
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
        fn set_assoc(&mut self, name: &str, key: &str, value: i64) {
            if name == "m" {
                self.0.insert(key.to_string(), value);
            }
        }
    }

    #[test]
    fn associative_subscripts() {
        let mut kv = HashMap::new();
        kv.insert("foo".to_string(), 7);
        kv.insert("bar".to_string(), 13);
        let mut m = AssocMap(kv);
        // The subscript is a literal string key, not arithmetic.
        assert_eq!(eval("m[foo]", &mut m).unwrap(), 7);
        assert_eq!(eval("m[bar] + 1", &mut m).unwrap(), 14);
        // A key that looks like an operator expression is still literal.
        assert_eq!(eval("m[missing]", &mut m).unwrap(), 0); // unset → 0
        // Whitespace around the key is trimmed.
        assert_eq!(eval("m[ foo ]", &mut m).unwrap(), 7);
        // Assignment to an associative element.
        assert_eq!(eval("m[foo] = 100", &mut m).unwrap(), 100);
        assert_eq!(m.0.get("foo"), Some(&100));
    }

    fn ev(s: &str) -> i64 {
        eval(s, &mut Map::default()).unwrap()
    }

    #[test]
    fn empty_expression_is_zero() {
        // bash: `$(( ))` and, after expansion, `$(( $unset ))` → 0.
        assert_eq!(ev(""), 0);
        assert_eq!(ev("   "), 0);
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
    fn exponent() {
        assert_eq!(ev("2 ** 10"), 1024);
        assert_eq!(ev("3 ** 0"), 1);
        // Right-associative: 2 ** 3 ** 2 == 2 ** (3 ** 2) == 2 ** 9 == 512.
        assert_eq!(ev("2 ** 3 ** 2"), 512);
        // Binds tighter than unary minus applies to the base? -2 ** 2 = -(2**2).
        assert_eq!(ev("2 ** 2 * 3"), 12);
        // Negative exponent is an error.
        assert!(eval("2 ** -1", &mut Map::default()).is_err());
    }

    #[test]
    fn number_bases() {
        // Leading-zero octal.
        assert_eq!(ev("017"), 15);
        assert_eq!(ev("0"), 0);
        assert_eq!(ev("010 + 1"), 9);
        // base#num arbitrary bases.
        assert_eq!(ev("2#1010"), 10);
        assert_eq!(ev("16#ff"), 255);
        assert_eq!(ev("16#FF"), 255); // case-insensitive for base <= 36
        assert_eq!(ev("8#17"), 15);
        assert_eq!(ev("36#z"), 35);
        // base > 36: uppercase continues past lowercase, then @ and _.
        assert_eq!(ev("64#_"), 63);
        assert_eq!(ev("64#A"), 36);
        // Combined with arithmetic.
        assert_eq!(ev("2#101 * 16#a"), 50);
        // Errors.
        assert!(eval("2#12", &mut Map::default()).is_err()); // '2' not valid in base 2
        assert!(eval("1#0", &mut Map::default()).is_err()); // base < 2
        assert!(eval("65#0", &mut Map::default()).is_err()); // base > 64
        assert!(eval("099", &mut Map::default()).is_err()); // bad octal digit
    }

    #[test]
    fn variables() {
        let mut m = HashMap::new();
        m.insert("x".to_string(), 10);
        m.insert("y".to_string(), 4);
        assert_eq!(eval("x * y + 2", &mut Map(m)).unwrap(), 42);
    }

    #[test]
    fn assignment_scalars() {
        let mut m = Map::default();
        assert_eq!(eval("x = 5", &mut m).unwrap(), 5);
        assert_eq!(m.get("x"), Some(5));
        // Compound assignment.
        assert_eq!(eval("x += 3", &mut m).unwrap(), 8);
        assert_eq!(eval("x *= 2", &mut m).unwrap(), 16);
        assert_eq!(eval("x -= 1", &mut m).unwrap(), 15);
        assert_eq!(eval("x /= 5", &mut m).unwrap(), 3);
        assert_eq!(m.get("x"), Some(3));
        // Right-associative chained assignment: y = z = 7.
        assert_eq!(eval("y = z = 7", &mut m).unwrap(), 7);
        assert_eq!(m.get("y"), Some(7));
        assert_eq!(m.get("z"), Some(7));
        // Assigning to a literal is an error.
        assert!(eval("3 = 4", &mut Map::default()).is_err());
    }

    #[test]
    fn increment_decrement() {
        let mut m = Map::default();
        m.set("x", 5);
        // Pre-increment yields the new value.
        assert_eq!(eval("++x", &mut m).unwrap(), 6);
        assert_eq!(m.get("x"), Some(6));
        // Post-increment yields the old value.
        assert_eq!(eval("x++", &mut m).unwrap(), 6);
        assert_eq!(m.get("x"), Some(7));
        // Pre/post decrement.
        assert_eq!(eval("--x", &mut m).unwrap(), 6);
        assert_eq!(eval("x--", &mut m).unwrap(), 6);
        assert_eq!(m.get("x"), Some(5));
        // Increment on an unset variable starts from 0.
        assert_eq!(eval("++fresh", &mut m).unwrap(), 1);
    }

    #[test]
    fn short_circuit_side_effects() {
        // The right operand of && is skipped when the left is false, so its
        // assignment side effect must not happen.
        let mut m = Map::default();
        eval("0 && (y = 9)", &mut m).unwrap();
        assert_eq!(m.get("y"), None);
        eval("1 || (z = 9)", &mut m).unwrap();
        assert_eq!(m.get("z"), None);
        // The taken branch of a ternary runs; the other doesn't.
        eval("1 ? (a = 1) : (b = 2)", &mut m).unwrap();
        assert_eq!(m.get("a"), Some(1));
        assert_eq!(m.get("b"), None);
    }

    #[test]
    fn div_zero() {
        assert!(eval("1 / 0", &mut Map::default()).is_err());
    }

    #[test]
    fn ternary() {
        assert_eq!(ev("1 ? 10 : 20"), 10);
        assert_eq!(ev("0 ? 10 : 20"), 20);
        // Condition is a full comparison expression.
        assert_eq!(ev("3 > 2 ? 100 : 200"), 100);
        // Right-associative: a ? b : c ? d : e == a ? b : (c ? d : e).
        assert_eq!(ev("0 ? 1 : 0 ? 2 : 3"), 3);
        assert_eq!(ev("0 ? 1 : 1 ? 2 : 3"), 2);
        // Nested in a larger expression / parentheses.
        assert_eq!(ev("(1 ? 2 : 3) + 4"), 6);
        // Missing ':' is a syntax error.
        assert!(eval("1 ? 2", &mut Map::default()).is_err());
    }

    #[test]
    fn comma() {
        assert_eq!(ev("1, 2, 3"), 3);
        assert_eq!(ev("(1 + 1, 2 * 3)"), 6);
        // Comma binds looser than ternary.
        assert_eq!(ev("1 ? 5 : 9, 7"), 7);
        // Comma sequences assignments (the C-style for-loop update idiom).
        let mut m = Map::default();
        assert_eq!(eval("i = 0, j = 10", &mut m).unwrap(), 10);
        assert_eq!(m.get("i"), Some(0));
        assert_eq!(m.get("j"), Some(10));
    }

    #[test]
    fn variables_in_ternary() {
        let mut m = HashMap::new();
        m.insert("x".to_string(), 5);
        m.insert("y".to_string(), 0);
        let mut vars = Map(m);
        assert_eq!(eval("x ? x * 2 : -1", &mut vars).unwrap(), 10);
        assert_eq!(eval("y ? 99 : x + 1", &mut vars).unwrap(), 6);
    }
}
