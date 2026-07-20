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
    /// Return the scalar variable's raw *string* value, or `None` if unset
    /// (treated as `0`). The value is not a plain integer: bash recursively
    /// evaluates it as an arithmetic expression, so `b=a; a=5; $((b))` yields
    /// `5` and `x="2+3"; $((x))` yields `5`. The evaluator performs that
    /// recursion (with a depth guard for cycles like `x=x`); implementors just
    /// return the stored text.
    fn get_str(&self, name: &str) -> Option<String>;

    /// Return the raw *string* value of the array element `name[index]`, or
    /// `None` if unset/out-of-range (treated as `0`). `index` has already been
    /// evaluated arithmetically (so `(( a[i+1] ))` and negative indices work).
    /// Like [`VarLookup::get_str`], the value is recursively arithmetic-
    /// evaluated. The default ignores subscripts — array-backed implementors
    /// override it.
    fn get_index_str(&self, name: &str, index: i64) -> Option<String> {
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

    /// Return the raw *string* value of associative element `name[key]`, or
    /// `None` if unset (treated as `0`). `key` is the raw, already-expanded
    /// subscript text (bash does not arithmetic-evaluate associative
    /// subscripts). The value string is recursively arithmetic-evaluated. Only
    /// consulted when [`VarLookup::is_assoc`] returns `true`.
    fn get_assoc_str(&self, name: &str, key: &str) -> Option<String> {
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
///
/// `msg` is the human-readable body (matching bash's wording, e.g. `division by
/// 0`, `syntax error: operand expected`). `token` — when known — is the
/// offending "error token": the de-quoted source text from the point where the
/// error was detected to the end of the expression, which bash appends as
/// `(error token is "…")`. Together they reproduce bash's arithmetic diagnostic
/// body (the enclosing shell prepends the `<name>: line N: <expr>:` prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArithError {
    /// The error body, matching bash's wording.
    pub msg: String,
    /// The offending token (bash's `error token is "…"`), if known.
    pub token: Option<String>,
    /// When `true`, the token is a self-contained *number literal* the lexer
    /// rejected (`2#12`, `099`, `65#5`), so bash truncates the echoed source at
    /// the end of that literal (`5+2#12+9` is reported as `5+2#12`). For
    /// ordinary parse/eval errors this is `false` and bash echoes the whole
    /// source with the token being the unparsed remainder. See
    /// `Shell::emit_arith_error`.
    pub truncate_leading: bool,
}

impl ArithError {
    /// A diagnostic with no specific error token.
    fn new(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: None,
            truncate_leading: false,
        }
    }

    /// A diagnostic carrying bash's `(error token is "…")` suffix.
    fn with_token(msg: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: Some(token.into()),
            truncate_leading: false,
        }
    }

    /// A number-literal lexer error whose token is a complete literal; the
    /// echoed source is truncated at the literal's end (bash behaviour).
    fn lexeme_error(msg: impl Into<String>, lexeme: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: Some(lexeme.into()),
            truncate_leading: true,
        }
    }
}

impl core::fmt::Display for ArithError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.token {
            Some(t) => write!(f, "{} (error token is \"{t}\")", self.msg),
            None => write!(f, "{}", self.msg),
        }
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
    /// `^`, `|`, `&&`, `||`). The final field is the RHS's source text (from
    /// the right operand's start to the end of the expression) — used as bash's
    /// "error token" for an eval-time failure such as division by zero; `None`
    /// for operators that cannot fail at evaluation.
    Bin(String, Box<Expr>, Box<Expr>, Option<Box<str>>),
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
    eval_expr(&ast, vars, 0)
}

/// Maximum depth of recursive variable evaluation (`b=a; a=b` would loop
/// forever). bash reports "expression recursion level exceeded" at a similar
/// bound. Each level re-parses the value string (itself a recursive-descent
/// walk), so this is kept well below what would overflow the native stack; no
/// legitimate variable-indirection chain approaches it.
const RECURSION_LIMIT: u32 = 128;

/// Evaluate a variable's raw string *value* as an arithmetic expression, the
/// way bash does: `b="a"` with `a=5` yields `5`, `x="2+3"` yields `5`, and an
/// unset/empty value yields `0`. `depth` guards against reference cycles.
fn str_to_val(s: &str, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(0);
    }
    // Fast path: a plain decimal literal (the overwhelmingly common case — loop
    // counters, sizes) needs no re-parse. A leading zero means octal, so defer
    // those (and hex / `base#n` / sub-expressions) to the full parser below.
    if let Some(n) = plain_decimal(t) {
        return Ok(n);
    }
    if depth >= RECURSION_LIMIT {
        // bash reports the offending value token here. (bash also uses the
        // innermost value as the `<expr>:` prefix; osh's caller supplies the
        // top-level expression instead — a documented divergence for the rare
        // reference-cycle case, see known-issues.md TD-OILS-ARITH-ERRFMT.)
        return Err(ArithError::with_token(
            "expression recursion level exceeded",
            t.to_string(),
        ));
    }
    let expr = parse(t, vars)?;
    eval_expr(&expr, vars, depth + 1)
}

/// Parse `t` as a plain decimal integer (optionally signed), returning `None`
/// for anything that needs the full arithmetic parser: empty, non-digits, or a
/// leading-zero form (`010`) which arithmetic treats as octal.
fn plain_decimal(t: &str) -> Option<i64> {
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None; // octal — let the full parser apply base rules
    }
    t.parse::<i64>().ok()
}

/// Parse an arithmetic expression into an AST (no evaluation, no mutation).
fn parse(expr: &str, vars: &dyn VarLookup) -> Result<Expr, ArithError> {
    let mut p = AParser {
        // bash deletes double quotes from an arithmetic expression before
        // evaluating it: `$(( "3" + "4" ))` → 7 and `$(( 1"2"3 ))` → 123 (the
        // quotes are removed, not treated as whitespace, so adjacent digits
        // fuse). Single quotes stay literal (and thus an error, as in bash).
        chars: expr.chars().filter(|&c| c != '"').collect(),
        pos: 0,
        last_op_start: 0,
        last_atom_start: 0,
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
        // A complete expression parsed, but more input follows. bash splits
        // this into two diagnostics: a leftover token the lexer *recognises*
        // (a number, `)`, `:`, `!`, …) is "syntax error in expression"; one it
        // cannot even tokenise (`;`, `@`, `.`) is "invalid arithmetic operator".
        let token = p.rest_from(p.pos);
        let body = match p.peek() {
            Some(c) if is_arith_token_char(c) => "syntax error in expression",
            _ => "syntax error: invalid arithmetic operator",
        };
        return Err(ArithError::with_token(body, token));
    }
    Ok(e)
}

struct AParser<'a> {
    chars: Vec<char>,
    pos: usize,
    /// Start position of the most recently consumed *operator* token. When an
    /// operand is expected but the input ends (`3 +`), bash's "error token" is
    /// that trailing operator, not the (empty) text at the cursor — so the
    /// operand-expected diagnostic falls back to this position at EOF.
    last_op_start: usize,
    /// Start position of the most recently begun leaf atom (number/variable).
    /// bash's error token for a missing `)` is the last operand it parsed.
    last_atom_start: usize,
    vars: &'a dyn VarLookup,
}

/// Does `c` begin a token bash's arithmetic lexer recognises? Used to classify
/// a trailing-input error as "syntax error in expression" (recognised token)
/// versus "invalid arithmetic operator" (an untokenisable character).
fn is_arith_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || "+-*/%|^&<>=!~()[]?:,".contains(c)
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

    /// The de-quoted source from `start` to the end of the expression — the
    /// substring bash reports as its `(error token is "…")`.
    fn rest_from(&self, start: usize) -> String {
        self.chars[start..].iter().collect()
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
                // Record the comma as the last operator so a missing operand
                // after it (`3 ,`) reports bash's error token `, ` (from the
                // comma) rather than the whole expression.
                self.last_op_start = self.pos;
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
            // The assignment operator's position is the error token bash reports
            // when the left side is not an lvalue (`1 = 2` → token `= 2`).
            let lv = lvalue_of(lhs).map_err(|_| {
                ArithError::with_token(
                    "attempted assignment to non-variable",
                    self.rest_from(self.pos),
                )
            })?;
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
        let qpos = self.pos;
        self.pos += 1; // consume '?'
        self.skip_ws();
        // bash inspects the token right after `?` (EXP_HIGHEST's first token):
        // an immediate `:` or end of input is an empty true branch, reported as
        // "expression expected" *before* attempting to parse an operand. The
        // error token is the `:` itself, or the `?` when the input ends there.
        match self.peek() {
            Some(':') => {
                return Err(ArithError::with_token(
                    "expression expected",
                    self.rest_from(self.pos),
                ));
            }
            None => {
                return Err(ArithError::with_token(
                    "expression expected",
                    self.rest_from(qpos),
                ));
            }
            _ => {}
        }
        let then_start = self.pos;
        // The middle branch is a full expression: bash parses it with
        // EXP_HIGHEST (expcomma), so it may be an assignment or even a comma
        // expression (`1 ? 2,3 : 4` → 3, `c ? x = 1 : y`). The else branch, by
        // contrast, recurses at ternary level (right-associative), so a trailing
        // comma there belongs to the enclosing expression (`1 ? 2 : 4,5` → 5).
        let then_e = self.parse_comma()?;
        self.skip_ws();
        if self.peek() != Some(':') {
            // bash: "`:' expected for conditional expression"; the error token is
            // the then-branch source (`1 ? 2` → `2`).
            return Err(ArithError::with_token(
                "`:' expected for conditional expression",
                self.rest_from(then_start),
            ));
        }
        let colon_pos = self.pos;
        self.pos += 1; // consume ':'
        self.skip_ws();
        // An empty false branch (end of input right after `:`) is likewise
        // "expression expected", with the `:` as the error token. A malformed
        // (but present) else operand falls through to the normal operand-expected
        // diagnostic below.
        if self.peek().is_none() {
            return Err(ArithError::with_token(
                "expression expected",
                self.rest_from(colon_pos),
            ));
        }
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
            self.last_op_start = self.pos;
            self.pos += op.chars().count();
            let next_min = if right { bp } else { bp + 1 };
            self.skip_ws();
            // Capture the RHS source (from here to end of input) for the
            // operators that can fail at evaluation — bash reports it as the
            // "error token" of a division-by-zero / negative-exponent failure.
            let rhs_tok = matches!(op.as_str(), "/" | "%" | "**").then(|| self.rest_from(self.pos));
            let rhs = self.parse_binary(next_min)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs), rhs_tok.map(String::into_boxed_str));
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
        // Remember where this atom begins: a missing `)` reports the last atom
        // parsed as its error token (`(2+3` → token `3`), and `name[` reports
        // the subscript expression from the name.
        let atom_start = self.pos;
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                // A parenthesised group is a full expression: ternary, comma,
                // and assignment are allowed inside.
                let e = self.parse_comma()?;
                self.skip_ws();
                if self.peek() != Some(')') {
                    // bash: "missing `)'"; the error token is the source of the
                    // last operand parsed inside the group.
                    return Err(ArithError::with_token(
                        "missing `)'",
                        self.rest_from(self.last_atom_start),
                    ));
                }
                self.pos += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() => {
                self.last_atom_start = atom_start;
                Ok(Expr::Num(self.parse_number()?))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                self.last_atom_start = atom_start;
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
                        // bash: "bad array subscript"; the error token runs from
                        // the array name (`foo[` → token `foo[`).
                        return Err(ArithError::with_token(
                            "bad array subscript",
                            self.rest_from(atom_start),
                        ));
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
            other => {
                // bash: "syntax error: operand expected". The error token is
                // the offending character to end of input; at end-of-input
                // (`3 +`) it is instead the trailing operator that consumed its
                // operand slot.
                let token = if other.is_some() {
                    self.rest_from(self.pos)
                } else {
                    self.rest_from(self.last_op_start)
                };
                Err(ArithError::with_token(
                    "syntax error: operand expected",
                    token,
                ))
            }
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
            // bash lexes a numeric literal as a *maximal* run of base-64 digit
            // characters (`0-9a-zA-Z@_`) and only then validates it against the
            // radix. A trailing non-hex digit char (`0xg`, `0x1g`) therefore
            // belongs to the same token and yields "value too great for base",
            // not a hex literal followed by a stray identifier.
            if matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                while matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                    self.pos += 1;
                }
                let lexeme: String = self.chars[start..self.pos].iter().collect();
                return Err(ArithError::lexeme_error("value too great for base", lexeme));
            }
            let hex: String = self.chars[hstart..self.pos].iter().collect();
            // bash accepts a bare `0x`/`0X` with no following hex digits as 0
            // (e.g. `$((0x))` → 0, `$((1 + 0x))` → 1). Only a genuinely malformed
            // digit run reaches `from_str_radix`, so match bash's leniency here.
            if hex.is_empty() {
                return Ok(0);
            }
            // A hex literal that overflows i64 wraps rather than erroring
            // (`$((0xFFFFFFFFFFFFFFFFF))` → -1), matching bash. Every char is a
            // valid hex digit here (the run above only consumed hex digits).
            let mut val: i64 = 0;
            for c in hex.chars() {
                if let Some(d) = c.to_digit(16) {
                    val = val.wrapping_mul(16).wrapping_add(i64::from(d));
                }
            }
            return Ok(val);
        }
        // Collect the leading decimal run. It is either the whole number, an
        // octal literal (leading zero), or the base of a `base#num` literal.
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        // base#num — bash arbitrary-base literals, base 2..=64.
        if self.peek() == Some('#') {
            let base_str: String = self.chars[start..self.pos].iter().collect();
            self.pos += 1; // consume '#'
            let dstart = self.pos;
            // Consume the whole digit lexeme (every char that is a digit in
            // *some* base: 0-9, a-z, A-Z, @, _) so the error token spans the
            // full literal exactly as bash reports it — `5+2#12+9` blames
            // `2#12`, not `2` or `2#12+9`.
            while matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                self.pos += 1;
            }
            let lexeme: String = self.chars[start..self.pos].iter().collect();
            let base: u32 = base_str.parse().map_err(|_| {
                ArithError::lexeme_error("invalid arithmetic base", lexeme.clone())
            })?;
            // bash distinguishes base 0 ("invalid number") from base 1 / >64
            // ("invalid arithmetic base").
            if base == 0 {
                return Err(ArithError::lexeme_error("invalid number", lexeme));
            }
            if !(2..=64).contains(&base) {
                return Err(ArithError::lexeme_error("invalid arithmetic base", lexeme));
            }
            if self.pos == dstart {
                // `2#` with no following digits.
                return Err(ArithError::lexeme_error("invalid integer constant", lexeme));
            }
            let mut val: i64 = 0;
            for &c in &self.chars[dstart..self.pos] {
                let Some(d) = digit_value(c, base) else {
                    // A digit valid in some base but not in *this* one is bash's
                    // "value too great for base" (`2#12`, `16#gz`, `10#0a`).
                    return Err(ArithError::lexeme_error("value too great for base", lexeme));
                };
                val = val
                    .wrapping_mul(i64::from(base))
                    .wrapping_add(i64::from(d));
            }
            return Ok(val);
        }
        // Not a `base#literal`. bash still consumes any trailing base-64 digit
        // characters (letters, `_`, `@`) into the same numeric token, so `0b100`,
        // `123abc` and `123_` are each a single "value too great for base" token
        // rather than a number followed by a stray identifier / syntax error.
        if matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
            while matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                self.pos += 1;
            }
            let lexeme: String = self.chars[start..self.pos].iter().collect();
            return Err(ArithError::lexeme_error("value too great for base", lexeme));
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // A leading zero (other than bare "0") denotes octal. bash reports a
        // non-octal digit (`099`, `0778`) as "value too great for base", but an
        // octal literal that overflows i64 *wraps* rather than erroring
        // (`$((077777777777777777777777777))` → -1), matching C accumulation.
        if text.len() > 1 && text.starts_with('0') {
            let mut val: i64 = 0;
            for c in text.chars() {
                let Some(d) = c.to_digit(8) else {
                    return Err(ArithError::lexeme_error(
                        "value too great for base",
                        text.clone(),
                    ));
                };
                val = val.wrapping_mul(8).wrapping_add(i64::from(d));
            }
            return Ok(val);
        }
        // Decimal. bash accumulates digits with i64 wraparound rather than
        // erroring on overflow (`$((9999999999999999999999))` →
        // 1864712049423024127), so reproduce that instead of a parse error.
        // The lexer only consumed ASCII digits, so every char is a valid digit.
        let mut val: i64 = 0;
        for c in text.chars() {
            if let Some(d) = c.to_digit(10) {
                val = val.wrapping_mul(10).wrapping_add(i64::from(d));
            }
        }
        Ok(val)
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
        _ => Err(ArithError::new("attempted assignment to non-variable")),
    }
}

fn eval_expr(e: &Expr, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
    match e {
        Expr::Num(n) => Ok(*n),
        // A variable read resolves the raw value string and (like bash)
        // recursively evaluates it as an arithmetic expression.
        Expr::Var(n) => match vars.get_str(n) {
            Some(s) => str_to_val(&s, vars, depth),
            None => Ok(0),
        },
        Expr::Index(n, ix) => {
            let i = eval_expr(ix, vars, depth)?;
            match vars.get_index_str(n, i) {
                Some(s) => str_to_val(&s, vars, depth),
                None => Ok(0),
            }
        }
        Expr::Assoc(n, k) => match vars.get_assoc_str(n, k) {
            Some(s) => str_to_val(&s, vars, depth),
            None => Ok(0),
        },
        Expr::Neg(x) => Ok(eval_expr(x, vars, depth)?.wrapping_neg()),
        Expr::Not(x) => Ok(i64::from(eval_expr(x, vars, depth)? == 0)),
        Expr::BitNot(x) => Ok(!eval_expr(x, vars, depth)?),
        Expr::Bin(op, l, r, rhs_tok) => match op.as_str() {
            // Short-circuit: the right operand's side effects only happen when
            // the left doesn't already decide the result.
            "&&" => {
                if eval_expr(l, vars, depth)? == 0 {
                    Ok(0)
                } else {
                    Ok(i64::from(eval_expr(r, vars, depth)? != 0))
                }
            }
            "||" => {
                if eval_expr(l, vars, depth)? != 0 {
                    Ok(1)
                } else {
                    Ok(i64::from(eval_expr(r, vars, depth)? != 0))
                }
            }
            _ => {
                let a = eval_expr(l, vars, depth)?;
                let b = eval_expr(r, vars, depth)?;
                // Attach the RHS source as bash's "error token" for an eval-time
                // failure (division by zero, negative exponent).
                apply(op, a, b).map_err(|mut e| {
                    if e.token.is_none()
                        && let Some(t) = rhs_tok
                    {
                        e.token = Some(t.to_string());
                    }
                    e
                })
            }
        },
        Expr::Ternary(c, t, f) => {
            if eval_expr(c, vars, depth)? != 0 {
                eval_expr(t, vars, depth)
            } else {
                eval_expr(f, vars, depth)
            }
        }
        Expr::Comma(l, r) => {
            eval_expr(l, vars, depth)?;
            eval_expr(r, vars, depth)
        }
        Expr::Assign(lv, base, rhs) => {
            let loc = resolve_lv(lv, vars, depth)?;
            let v = match base {
                None => eval_expr(rhs, vars, depth)?,
                Some(op) => {
                    let cur = load_rlv(&loc, vars, depth)?;
                    let b = eval_expr(rhs, vars, depth)?;
                    apply(op, cur, b)?
                }
            };
            store_rlv(&loc, v, vars);
            Ok(v)
        }
        Expr::PreIncr(lv, inc) => {
            let loc = resolve_lv(lv, vars, depth)?;
            let step = if *inc { 1 } else { -1 };
            let v = load_rlv(&loc, vars, depth)?.wrapping_add(step);
            store_rlv(&loc, v, vars);
            Ok(v)
        }
        Expr::PostIncr(lv, inc) => {
            let loc = resolve_lv(lv, vars, depth)?;
            let old = load_rlv(&loc, vars, depth)?;
            let step = if *inc { 1 } else { -1 };
            store_rlv(&loc, old.wrapping_add(step), vars);
            Ok(old)
        }
    }
}

/// Resolve an lvalue's location once (evaluating an index subscript), so a
/// read-modify-write op doesn't evaluate the subscript twice.
fn resolve_lv(lv: &Lvalue, vars: &mut dyn VarLookup, depth: u32) -> Result<ResolvedLv, ArithError> {
    Ok(match lv {
        Lvalue::Var(n) => ResolvedLv::Var(n.clone()),
        Lvalue::Index(n, ix) => {
            let i = eval_expr(ix, vars, depth)?;
            ResolvedLv::Index(n.clone(), i)
        }
        Lvalue::Assoc(n, k) => ResolvedLv::Assoc(n.clone(), k.clone()),
    })
}

fn load_rlv(loc: &ResolvedLv, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
    match loc {
        ResolvedLv::Var(n) => match vars.get_str(n) {
            Some(s) => str_to_val(&s, vars, depth),
            None => Ok(0),
        },
        ResolvedLv::Index(n, i) => match vars.get_index_str(n, *i) {
            Some(s) => str_to_val(&s, vars, depth),
            None => Ok(0),
        },
        ResolvedLv::Assoc(n, k) => match vars.get_assoc_str(n, k) {
            Some(s) => str_to_val(&s, vars, depth),
            None => Ok(0),
        },
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
                return Err(ArithError::new("exponent less than 0"));
            }
            let exp = u32::try_from(b).map_err(|_| ArithError::new("exponent too large"))?;
            a.wrapping_pow(exp)
        }
        "/" => {
            if b == 0 {
                // Match bash's wording verbatim (`division by 0`), not "division by zero".
                return Err(ArithError::new("division by 0"));
            }
            a.wrapping_div(b)
        }
        "%" => {
            if b == 0 {
                // bash reports modulo-by-zero with the same "division by 0" text as `/`.
                return Err(ArithError::new("division by 0"));
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
        _ => return Err(ArithError::new(format!("unknown operator '{op}'"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Map(HashMap<String, i64>);
    impl Map {
        /// Test-only convenience: read a scalar back as an integer to assert on
        /// values stored by arithmetic assignment.
        fn get(&self, name: &str) -> Option<i64> {
            self.0.get(name).copied()
        }
    }
    impl VarLookup for Map {
        fn get_str(&self, name: &str) -> Option<String> {
            self.0.get(name).map(i64::to_string)
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
        fn get_str(&self, name: &str) -> Option<String> {
            self.scalars.get(name).map(i64::to_string)
        }
        fn set(&mut self, name: &str, value: i64) {
            self.scalars.insert(name.to_string(), value);
        }
        fn get_index_str(&self, name: &str, index: i64) -> Option<String> {
            if name != "a" {
                return None;
            }
            let real = if index < 0 {
                i64::try_from(self.a.len()).ok()? + index
            } else {
                index
            };
            usize::try_from(real)
                .ok()
                .and_then(|i| self.a.get(i))
                .map(i64::to_string)
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
        fn get_str(&self, _name: &str) -> Option<String> {
            None
        }
        fn is_assoc(&self, name: &str) -> bool {
            name == "m"
        }
        fn get_assoc_str(&self, name: &str, key: &str) -> Option<String> {
            if name != "m" {
                return None;
            }
            self.0.get(key).map(i64::to_string)
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

    /// A string-backed scalar lookup, so recursive value evaluation (a value
    /// that is itself a variable name or an expression) can be exercised.
    #[derive(Default)]
    struct StrMap(HashMap<String, String>);
    impl VarLookup for StrMap {
        fn get_str(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
        fn set(&mut self, name: &str, value: i64) {
            self.0.insert(name.to_string(), value.to_string());
        }
    }

    #[test]
    fn recursive_variable_evaluation() {
        let mut m = StrMap::default();
        m.0.insert("a".into(), "5".into());
        m.0.insert("b".into(), "a".into()); // b -> a -> 5
        m.0.insert("c".into(), "b".into()); // c -> b -> a -> 5
        m.0.insert("expr".into(), "2+3".into()); // value is an expression
        m.0.insert("mixed".into(), "a * 2".into()); // uses another var
        assert_eq!(eval("b", &mut m).unwrap(), 5);
        assert_eq!(eval("c", &mut m).unwrap(), 5);
        assert_eq!(eval("expr", &mut m).unwrap(), 5);
        assert_eq!(eval("expr * 2", &mut m).unwrap(), 10);
        assert_eq!(eval("mixed", &mut m).unwrap(), 10);
        // A value naming an unset variable evaluates to 0.
        m.0.insert("u".into(), "missing".into());
        assert_eq!(eval("u + 1", &mut m).unwrap(), 1);
        // A leading-zero value keeps octal semantics through the recursion.
        m.0.insert("oct".into(), "010".into());
        assert_eq!(eval("oct", &mut m).unwrap(), 8);
    }

    #[test]
    fn recursive_variable_cycle_is_bounded() {
        let mut m = StrMap::default();
        m.0.insert("x".into(), "x".into()); // self-reference
        let e = eval("x", &mut m).unwrap_err();
        assert!(e.msg.contains("recursion level exceeded"), "{}", e.msg);
        // Mutual cycle a -> b -> a.
        let mut m2 = StrMap::default();
        m2.0.insert("a".into(), "b".into());
        m2.0.insert("b".into(), "a".into());
        assert!(eval("a", &mut m2).is_err());
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
    fn oversized_literal_wraps_like_bash() {
        // A decimal literal exceeding i64 accumulates with wraparound rather
        // than erroring, matching bash (`$((9999999999999999999999))`).
        assert_eq!(ev("9999999999999999999999"), 1_864_712_049_423_024_127);
        // Octal literals wrap too (`$((0777…))` → -1 once it overflows).
        assert_eq!(ev("077777777777777777777777777"), -1);
        // Hex literals wrap as well (`$((0xF…F))` → -1).
        assert_eq!(ev("0xFFFFFFFFFFFFFFFFF"), -1);
        // A non-octal digit in a leading-zero literal is still an error.
        assert!(eval("099", &mut Map::default()).is_err());
    }

    #[test]
    fn double_quotes_are_stripped() {
        // bash deletes double quotes from an arithmetic expression before
        // evaluating: quoted operands and even quotes mid-number are removed.
        assert_eq!(ev(r#""3" + "4""#), 7);
        assert_eq!(ev(r#"2 + "3 * 4""#), 14);
        assert_eq!(ev(r#""3"4"#), 34);
        assert_eq!(ev(r#"1"2"3"#), 123);
        assert_eq!(ev(r#"""+5"#), 5);
        // Adjacent quoted numbers with no operator are still a syntax error
        // (the quotes vanish but leave `3 4`).
        assert!(eval(r#""3" "4""#, &mut Map::default()).is_err());
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
        // Hexadecimal.
        assert_eq!(ev("0x1f"), 31);
        assert_eq!(ev("0XFF"), 255);
        // bash accepts a bare `0x`/`0X` (no hex digits) as 0.
        assert_eq!(ev("0x"), 0);
        assert_eq!(ev("0X"), 0);
        assert_eq!(ev("1 + 0x"), 1);
        assert_eq!(ev("0x1 + 0x"), 1);
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
    fn zero_division_messages_match_bash() {
        // bash reports both `/` and `%` by zero with the exact text "division by 0"
        // (not "division by zero"/"modulo by zero"), and exponent-by-negative with
        // "exponent less than 0". Keep the wording verbatim for bash-superset parity.
        let div = eval("1 / 0", &mut Map::default()).unwrap_err();
        assert_eq!(div.msg, "division by 0");
        assert_eq!(div.to_string(), "division by 0 (error token is \"0\")");
        let modulo = eval("1 % 0", &mut Map::default()).unwrap_err();
        assert_eq!(modulo.msg, "division by 0");
        let exp = eval("5 ** -1", &mut Map::default()).unwrap_err();
        assert_eq!(exp.msg, "exponent less than 0");
    }

    #[test]
    fn error_bodies_and_tokens_match_bash() {
        // The full `Display` (body + `(error token is "…")`) reproduces bash's
        // arithmetic diagnostic body byte-for-byte across the common cases. The
        // enclosing shell prepends the `<name>: line N: <expr>:` prefix.
        let cases: &[(&str, &str)] = &[
            ("1/0", "division by 0 (error token is \"0\")"),
            ("1%0", "division by 0 (error token is \"0\")"),
            ("1/(0)", "division by 0 (error token is \"(0)\")"),
            ("1/0/0", "division by 0 (error token is \"0/0\")"),
            ("5 +", "syntax error: operand expected (error token is \"+\")"),
            ("3 * ", "syntax error: operand expected (error token is \"* \")"),
            ("* 3", "syntax error: operand expected (error token is \"* 3\")"),
            ("@", "syntax error: operand expected (error token is \"@\")"),
            ("3 3", "syntax error in expression (error token is \"3\")"),
            ("a b c", "syntax error in expression (error token is \"b c\")"),
            (
                "1 ;",
                "syntax error: invalid arithmetic operator (error token is \";\")",
            ),
            (
                "1 = 2",
                "attempted assignment to non-variable (error token is \"= 2\")",
            ),
            (
                "1 ? 2",
                "`:' expected for conditional expression (error token is \"2\")",
            ),
            // A missing operand after a comma reports the comma as the error
            // token (bash: `3 ,` → `, `), not the whole expression.
            (
                "3 ,",
                "syntax error: operand expected (error token is \",\")",
            ),
            // Empty ternary branches are "expression expected" (not "operand
            // expected"), reported at the `:` — or the `?` when input ends there.
            (
                "1 ? : 3",
                "expression expected (error token is \": 3\")",
            ),
            ("1 ? 2 :", "expression expected (error token is \":\")"),
            ("1 ? :", "expression expected (error token is \":\")"),
            ("1 ?", "expression expected (error token is \"?\")"),
            // A trailing comma inside the true branch is mid-expression, so the
            // `:` triggers the ordinary operand-expected diagnostic.
            (
                "1 ? 2,3, : 4",
                "syntax error: operand expected (error token is \": 4\")",
            ),
            ("a[", "bad array subscript (error token is \"a[\")"),
            ("1#", "invalid arithmetic base (error token is \"1#\")"),
            ("2#5", "value too great for base (error token is \"2#5\")"),
            // A digit valid in *some* base but not in this one — even when it is
            // not the first digit — is "value too great for base", and the token
            // spans the whole literal (not just the offending digit).
            ("2#12", "value too great for base (error token is \"2#12\")"),
            ("10#0a", "value too great for base (error token is \"10#0a\")"),
            ("5+2#12+9", "value too great for base (error token is \"2#12\")"),
            ("16#gz+1", "value too great for base (error token is \"16#gz\")"),
            // Leading-zero octal with a non-octal digit.
            ("099", "value too great for base (error token is \"099\")"),
            ("0778", "value too great for base (error token is \"0778\")"),
            // A plain numeric literal consumes a maximal run of base-64 digit
            // chars (`0-9a-zA-Z@_`) before validating, so a trailing letter,
            // `_` or `@` makes the whole token "value too great for base" —
            // bash never splits it into a number plus a stray identifier.
            // (`0b100` has no binary-literal syntax in bash: leading `0` = octal,
            // and `b` is an out-of-range digit.)
            ("0b100", "value too great for base (error token is \"0b100\")"),
            ("123abc", "value too great for base (error token is \"123abc\")"),
            ("5+123abc", "value too great for base (error token is \"123abc\")"),
            ("123_", "value too great for base (error token is \"123_\")"),
            ("123@", "value too great for base (error token is \"123@\")"),
            ("1e3", "value too great for base (error token is \"1e3\")"),
            // The same rule applies after a `0x`/`0X` hex prefix: a trailing
            // non-hex digit char is part of the token, not a new one.
            ("0xg", "value too great for base (error token is \"0xg\")"),
            ("0x1g+5", "value too great for base (error token is \"0x1g\")"),
            // Base edge cases: 0 → "invalid number", >64 → "invalid arithmetic
            // base", `N#` with no digits → "invalid integer constant".
            ("0#5", "invalid number (error token is \"0#5\")"),
            ("65#5", "invalid arithmetic base (error token is \"65#5\")"),
            ("2#", "invalid integer constant (error token is \"2#\")"),
        ];
        for (src, want) in cases {
            let e = eval(src, &mut Map::default()).unwrap_err();
            assert_eq!(&e.to_string(), want, "expr {src:?}");
        }
    }

    #[test]
    fn number_literal_errors_flag_leading_truncation() {
        // Number-literal lexer errors set `truncate_leading` so the shell echoes
        // the source up to the literal's end (`5+2#12+9` → `5+2#12`). Ordinary
        // parse/eval errors leave it clear (the whole source is echoed).
        for src in ["2#12", "099", "65#5", "2#", "0#5", "5+2#12+9"] {
            let e = eval(src, &mut Map::default()).unwrap_err();
            assert!(e.truncate_leading, "expr {src:?} should truncate leading");
        }
        for src in ["1/0", "5 +", "3 3"] {
            let e = eval(src, &mut Map::default()).unwrap_err();
            assert!(!e.truncate_leading, "expr {src:?} should not truncate");
        }
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
        // The true branch is a full expression (bash EXP_HIGHEST), so a comma
        // expression is allowed there and yields its last value.
        assert_eq!(ev("1 ? 2,3 : 4"), 3);
        assert_eq!(ev("0 ? 2,3 : 4"), 4);
        // A comma-separated assignment sequence works in the true branch too.
        let mut m = Map::default();
        assert_eq!(eval("1 ? a=1, b=2, a+b : 0", &mut m).unwrap(), 3);
        assert_eq!(m.get("a"), Some(1));
        assert_eq!(m.get("b"), Some(2));
        // The else branch recurses at ternary level, so a trailing comma binds
        // to the enclosing expression: `1 ? 2 : 4,5` == `(1?2:4),5` == 5.
        assert_eq!(ev("1 ? 2 : 4,5"), 5);
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
