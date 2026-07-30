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
    ///
    /// A write can be *refused* — the shell's readonly attribute is the reason
    /// bash has — in which case the error aborts the expression where it stands,
    /// leaving whatever earlier assignments it already made in place
    /// (`(( y=1, x=2 ))` against a readonly `x` still assigns `y`).
    fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
        let _ = (name, value);
        Ok(())
    }

    /// Assign `value` to the indexed element `name[index]` (`a[i] = …`).
    fn set_index(&mut self, name: &str, index: i64, value: i64) -> Result<(), ArithError> {
        let _ = (name, index, value);
        Ok(())
    }

    /// Assign `value` to the associative element `name[key]` (`m[key] = …`).
    fn set_assoc(&mut self, name: &str, key: &str, value: i64) -> Result<(), ArithError> {
        let _ = (name, key, value);
        Ok(())
    }

    /// Complain about *reading* through an empty subscript (`(( a[] ))`) — bash
    /// prints `NAME[]: bad array subscript`, untagged, and the read then yields
    /// 0. See [`Expr::EmptySub`].
    ///
    /// bash emits that line **twice** for each such read (it validates the
    /// subscript once when resolving the reference and again when fetching the
    /// value), so an implementation mirroring bash writes two lines per call.
    /// The evaluator calls this once per read reached.
    fn warn_empty_subscript_read(&mut self, name: &str) {
        let _ = name;
    }

    /// Refuse a *store* through an empty subscript (`(( a[]=9 ))`) — bash prints
    /// `` `NAME[]': not a valid identifier ``, tagged with the enclosing builtin,
    /// and drops the store while letting the expression keep its value. See
    /// [`Expr::EmptySub`].
    fn refuse_empty_subscript_store(&mut self, name: &str) {
        let _ = name;
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
    /// The expression string to echo as the `<expr>:` prefix, when it should
    /// differ from the top-level arithmetic source. bash, when a failure occurs
    /// while recursively evaluating a *variable's value* as arithmetic (`x="5
    /// apples"; $(( x ))`), echoes the resolved value (`5 apples`) rather than
    /// the variable reference (`x`). `str_to_val` records the innermost failing
    /// value here so [`Shell::emit_arith_error`] can prefer it. `None` for a
    /// direct expression, where the caller-supplied source is already correct.
    pub expr_override: Option<String>,
    /// The *name* the diagnostic is about, when the failure is a property of a
    /// variable rather than of the expression's text. A refused write to a
    /// readonly variable reads `bash: line 1: x: readonly variable` — the
    /// subject is `x`, not the `x=5` that was being evaluated — and carries
    /// neither the `((`/`let` builtin tag nor an `(error token is …)` suffix,
    /// since neither the command nor any token is what went wrong. `None` for
    /// the ordinary errors, which are about the expression and echo it.
    pub subject: Option<String>,
    /// Set when the failure happened while evaluating an array *subscript*.
    /// bash evaluates a subscript through a separate entry point from the
    /// expression around it, and every diagnostic from there differs twice
    /// over: no builtin tag is applied (`((a[1/0]=9))` reports plain
    /// `1/0: division by 0`, not `((: a[1/0]=9: …`), and the failure is fatal
    /// to the command list the way an expansion error is, rather than merely
    /// giving `let`/`(( ))` a non-zero status. See [`Sub`].
    pub in_subscript: bool,
}

impl ArithError {
    /// A diagnostic with no specific error token.
    fn new(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: None,
            truncate_leading: false,
            expr_override: None,
            subject: None,
            in_subscript: false,
        }
    }

    /// A diagnostic carrying bash's `(error token is "…")` suffix.
    fn with_token(msg: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: Some(token.into()),
            truncate_leading: false,
            expr_override: None,
            subject: None,
            in_subscript: false,
        }
    }

    /// A number-literal lexer error whose token is a complete literal; the
    /// echoed source is truncated at the literal's end (bash behaviour).
    fn lexeme_error(msg: impl Into<String>, lexeme: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: Some(lexeme.into()),
            truncate_leading: true,
            expr_override: None,
            subject: None,
            in_subscript: false,
        }
    }

    /// A diagnostic about a *variable* rather than about the expression — see
    /// [`ArithError::subject`]. Implementors of [`VarLookup`] use this to refuse
    /// a write.
    pub fn about_var(name: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            token: None,
            truncate_leading: false,
            expr_override: None,
            subject: Some(name.into()),
            in_subscript: false,
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
    Index(String, Box<Sub>),
    /// Associative array element `name[key]` (subscript is a literal key).
    Assoc(String, String),
    /// A reference whose subscript is *lexically empty* — `a[]`. bash refuses
    /// this rather than reading it as index 0, and the refusal is a complaint
    /// rather than an error: the expression carries on with the value 0 (a read)
    /// or with the store dropped (a write). Which of the two it is only becomes
    /// clear later, so the emptiness is recorded here and acted on by
    /// [`eval_expr`] / [`store_rlv`]. The check is purely lexical, so `a[  ]` is
    /// *not* this — whitespace arithmetic-evaluates to index 0 as usual — and it
    /// does not depend on the name at all: an unset name, a scalar and an
    /// associative array are all refused identically.
    EmptySub(String),
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
    Index(String, Box<Sub>),
    Assoc(String, String),
    /// `a[] = …` — see [`Expr::EmptySub`]. Assignable only in the sense that
    /// bash parses it and then drops the store.
    EmptySub(String),
}

/// An array subscript: the parsed expression together with its raw source text.
///
/// bash evaluates a subscript through an entry point of its own rather than as
/// part of the expression around it, and that shows in every diagnostic it
/// produces — see [`ArithError::in_subscript`]. Keeping the raw text beside the
/// AST is what lets [`Sub`] restore the context the separate entry point would
/// have carried.
#[derive(Debug, Clone)]
struct Sub {
    expr: Expr,
    raw: Box<str>,
}

impl Sub {
    /// Parse `raw` as a subscript expression. A *parse* failure is a subscript
    /// failure too: `((a[1+]=9))` reports `1+: syntax error: operand expected`.
    fn parse(raw: &str, vars: &dyn VarLookup) -> Result<Self, ArithError> {
        let expr = parse(raw, vars).map_err(|e| tag_subscript(e, raw))?;
        Ok(Self {
            expr,
            raw: raw.into(),
        })
    }

    fn eval(&self, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
        eval_expr(&self.expr, vars, depth).map_err(|e| tag_subscript(e, &self.raw))
    }
}

/// Mark `e` as having come from evaluating a subscript, and blame the
/// subscript's own text.
///
/// The innermost subscript wins — `a[b[1/0]]` blames `1/0`, not `b[1/0]` — and a
/// failure deeper still keeps what it recorded: for `x="1/0"; ((a[x]=9))` bash
/// blames the *value* `1/0` that `str_to_val` recorded, not the subscript `x`.
fn tag_subscript(mut e: ArithError, raw: &str) -> ArithError {
    if e.expr_override.is_none() {
        e.expr_override = Some(raw.to_string());
    }
    e.in_subscript = true;
    e
}

/// A resolved location — array subscripts already evaluated to a concrete
/// index/key, so load and store don't re-evaluate (and re-trigger side
/// effects) the subscript expression.
enum ResolvedLv {
    Var(String),
    Index(String, i64),
    Assoc(String, String),
    /// `a[]` — see [`Expr::EmptySub`]. There is no index to resolve.
    EmptySub(String),
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

/// [`eval`] on an expression that arrives as *bytes* — the ordinary case for the
/// shell, whose arithmetic text is assembled from variable values and command
/// substitutions and so may hold any byte at all.
///
/// A byte that decodes to no character cannot be part of a well-formed
/// expression: every arithmetic operator, digit and identifier character is
/// ASCII, so such a byte is exactly the "invalid arithmetic operator" the lexer
/// rejects — which is also what bash, reading the same bytes in the C locale,
/// reports. Answering that is the honest result; converting the byte to U+FFFD
/// and evaluating the mangled text could instead yield a *number*, and a wrong
/// number is worse than an error.
///
/// TD-OILS-BYTE-STRINGS step 9 makes the lexer itself byte-native, at which
/// point this becomes a thin wrapper and the diagnostic regains its
/// `(error token is "…")` suffix, which cannot be built from a `String` today.
///
/// # Errors
/// As [`eval`], plus a syntax error for an expression that is not text.
pub fn eval_bytes(expr: crate::bytes::BStr<'_>, vars: &mut dyn VarLookup) -> Result<i64, ArithError> {
    let Some(expr) = crate::bytes::as_str(expr) else {
        return Err(ArithError::new("syntax error: invalid arithmetic operator"));
    };
    eval(expr, vars)
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
        // bash reports the offending value token here, and uses the innermost
        // value as the `<expr>:` prefix (recorded via `expr_override`).
        let mut e = ArithError::with_token("expression recursion level exceeded", t.to_string());
        e.expr_override = Some(t.to_string());
        return Err(e);
    }
    // When evaluating a variable's *value* as arithmetic fails, bash echoes that
    // resolved value as the `<expr>:` prefix (`x="5 apples"; $(( x ))` reports
    // `5 apples:`, not `x:`). Record the innermost failing value so the shell's
    // diagnostic matches — the deepest level sets it first as errors unwind, and
    // outer levels leave it in place.
    parse(t, vars)
        .and_then(|expr| eval_expr(&expr, vars, depth + 1))
        .map_err(|mut e| {
            if e.expr_override.is_none() {
                e.expr_override = Some(t.to_string());
            }
            e
        })
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
        // Quotes reach here only if something *upstream* left them, and then
        // they are ordinary (invalid) characters. It is the expansion pass in
        // front of the evaluator that removes double quotes, not the evaluator:
        // `$(( "3" + "4" ))` is 7 because expansion hands over `3 + 4`, while a
        // value the evaluator reads for itself keeps them and is rejected —
        // `x='"3"'; $(( x+1 ))` is `"3": syntax error: operand expected`, as is
        // `let 'y="3"+4'`, whose argument no expansion pass ever saw. Single
        // quotes are never removed by either.
        chars: expr.chars().collect(),
        pos: 0,
        last_op_start: 0,
        last_atom_start: 0,
        last_tok_start: 0,
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
    /// Used for the `name[` subscript diagnostic, which reports from the name.
    last_atom_start: usize,
    /// Start position of the most recently consumed token of *any* kind —
    /// operand, operator or parenthesis. A missing `)` reported at end of input
    /// names this token, because that is the one bash's lexer is still holding:
    /// `(2+3` names `3`, but `((2+3)` names the `)`.
    last_tok_start: usize,
    vars: &'a dyn VarLookup,
}

/// Does `c` begin a token bash's arithmetic lexer recognises? Used to classify
/// a trailing-input error as "syntax error in expression" (recognised token)
/// versus "invalid arithmetic operator" (an untokenisable character).
///
/// Brackets are *not* recognised: `a[0]` is lexed as one name-with-subscript
/// token, so a `[` or `]` reached in operator position is a character bash's
/// lexer has no token for — `let '2+3]'` is "invalid arithmetic operator",
/// while `let '2+3:'` (a real token) is "syntax error in expression".
fn is_arith_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || "+-*/%|^&<>=!~()?:,".contains(c)
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

    /// Record the operator token starting at the cursor.
    fn mark_op(&mut self) {
        self.last_op_start = self.pos;
        self.last_tok_start = self.pos;
    }

    /// Record the leaf atom (number, or name with its optional subscript)
    /// starting at `start`.
    fn mark_atom(&mut self, start: usize) {
        self.last_atom_start = start;
        self.last_tok_start = start;
    }

    /// Record a token that is neither operand nor operator — a parenthesis.
    fn mark_tok(&mut self, start: usize) {
        self.last_tok_start = start;
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
                self.mark_op();
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
            // Record the assignment operator's position: if its right-hand side
            // is missing (`x = `, `y += `), bash's operand-expected error token
            // runs from the operator (`= `, `+= `), not the whole expression.
            self.mark_op();
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
            // A `++`/`--` that reached here is not an increment — `parse_postfix`
            // declined it because the operand slot to its left is not
            // assignable. bash's lexer would never have formed the two-character
            // token at all, so read only its first character as the binary
            // operator and leave the second to the operand that follows.
            let op = if op == "++" || op == "--" {
                op[..1].to_string()
            } else {
                op
            };
            let Some((bp, right)) = binop_bp(&op) else {
                break;
            };
            if bp < min_bp {
                break;
            }
            self.mark_op();
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
        // A unary operator that consumes its operand slot becomes the "last
        // operator": if the operand is missing (`1 + + `, `~ `), bash's
        // operand-expected error token runs from this unary operator, not from
        // an earlier binary operator or the start of the expression.
        //
        // bash forms a prefix `++`/`--` only when an lvalue follows: its lexer
        // looks ahead for a variable name and, not finding one, hands back a
        // plain `+`/`-`. So `--v` decrements, but `--2` is `-(-2)` = 2 and
        // `--(3)` is 3. Falling through to the single-character cases below
        // reproduces that — and with it bash's error token for a missing
        // operand (`-- ` → `- `, `++ ` → `+ `), which starts one char in
        // precisely because that is where the second operator sits.
        if let Some(op) = self.read_op()
            && (op == "++" || op == "--")
            && self.lvalue_follows(2)
        {
            self.mark_op();
            self.pos += 2;
            let operand = self.parse_unary()?;
            let lv = lvalue_of(operand)?;
            return Ok(Expr::PreIncr(lv, op == "++"));
        }
        match self.peek() {
            Some('-') => {
                self.mark_op();
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            Some('+') => {
                self.mark_op();
                self.pos += 1;
                self.parse_unary()
            }
            Some('!') => {
                self.mark_op();
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            Some('~') => {
                self.mark_op();
                self.pos += 1;
                Ok(Expr::BitNot(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_postfix(),
        }
    }

    /// Does an assignable name begin `n` characters past the cursor?
    ///
    /// bash's arithmetic lexer looks ahead like this before it will read `++`
    /// or `--` as an increment operator: only a variable name may follow one,
    /// so anything else means the two characters are separate operators.
    fn lvalue_follows(&self, n: usize) -> bool {
        let mut i = self.pos + n;
        while matches!(self.chars.get(i), Some(c) if c.is_whitespace()) {
            i += 1;
        }
        matches!(self.chars.get(i), Some(c) if c.is_ascii_alphabetic() || *c == '_')
    }

    /// A primary atom followed by an optional postfix `++`/`--`.
    fn parse_postfix(&mut self) -> Result<Expr, ArithError> {
        let e = self.parse_atom()?;
        self.skip_ws();
        // Only an lvalue can be incremented, and bash does not make the attempt
        // otherwise: `2--3` is `2 - (-3)` = 5, not a decrement of `2`. Leaving
        // the operator unconsumed hands it to `parse_binary`, which reads its
        // first character as the binary operator it is.
        if let Some(op) = self.read_op()
            && (op == "++" || op == "--")
            && is_lvalue(&e)
        {
            let lv = lvalue_of(e)?;
            self.pos += 2;
            return Ok(Expr::PostIncr(lv, op == "++"));
        }
        Ok(e)
    }

    fn parse_atom(&mut self) -> Result<Expr, ArithError> {
        self.skip_ws();
        // Remember where this atom begins: `name[` reports the subscript
        // expression from the name, and a name with a subscript is one token.
        let atom_start = self.pos;
        match self.peek() {
            Some('(') => {
                self.mark_tok(atom_start);
                self.pos += 1;
                // A parenthesised group is a full expression: ternary, comma,
                // and assignment are allowed inside.
                let e = self.parse_comma()?;
                self.skip_ws();
                if self.peek() != Some(')') {
                    // bash names the token it is standing on when the `)` fails
                    // to appear (`( a b` → `b`, `((1)(2)` → `(2)`). At end of
                    // input it is standing on nothing, so it names the last
                    // token it did lex — which is why `(2+3` names `3` but
                    // `((2+3)` names the `)`.
                    let token = if self.pos == self.chars.len() {
                        self.rest_from(self.last_tok_start)
                    } else {
                        self.rest_from(self.pos)
                    };
                    // A character the lexer has no token for never reaches the
                    // missing-`)` check in bash: the lexer rejects it first, so
                    // `(1 @` and `(2+3]` are "invalid arithmetic operator".
                    let body = match self.peek() {
                        Some(c) if !is_arith_token_char(c) => {
                            "syntax error: invalid arithmetic operator"
                        }
                        _ => "missing `)'",
                    };
                    return Err(ArithError::with_token(body, token));
                }
                self.mark_tok(self.pos);
                self.pos += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() => {
                self.mark_atom(atom_start);
                Ok(Expr::Num(self.parse_number()?))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                self.mark_atom(atom_start);
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
                    if raw.is_empty() {
                        // `a[]` — see `Expr::EmptySub`. Deliberately ahead of the
                        // indexed/associative split, because bash refuses it
                        // without looking at the name.
                        return Ok(Expr::EmptySub(name));
                    }
                    if self.vars.is_assoc(&name) {
                        return Ok(Expr::Assoc(name, raw.trim().to_string()));
                    }
                    // Indexed: parse the subscript as its own arithmetic
                    // expression (evaluated later against the live environment).
                    let sub = Sub::parse(&raw, self.vars)?;
                    return Ok(Expr::Index(name, Box::new(sub)));
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
            // A prefixed literal (`0x…`) cannot serve as the base of a
            // `base#num` construct: bash's strlong() sets `foundbase` on the
            // `0x` prefix and rejects a subsequent `#` as "invalid number"
            // (`0x8#1`). Consume the rest of the token so the error names the
            // whole literal, matching bash.
            if self.peek() == Some('#') {
                self.pos += 1;
                while matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                    self.pos += 1;
                }
                let lexeme: String = self.chars[start..self.pos].iter().collect();
                return Err(ArithError::lexeme_error("invalid number", lexeme));
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
            // A base written with a leading `0` is an octal-prefixed literal, so
            // bash's strlong() sets `foundbase` while reading it and then rejects
            // the `#` as "invalid number" (`064#1`, `0#1`). It reads the base
            // digits in octal first, however, so a non-octal digit in that prefix
            // is diagnosed earlier as "value too great for base" (`08#1`). A bare
            // `0` base (len 1) falls through to the base-range check below, where
            // `base == 0` also yields "invalid number".
            if base_str.len() > 1 && base_str.starts_with('0') {
                for c in base_str[1..].chars() {
                    if c.to_digit(8).is_none() {
                        return Err(ArithError::lexeme_error(
                            "value too great for base",
                            lexeme,
                        ));
                    }
                }
                return Err(ArithError::lexeme_error("invalid number", lexeme));
            }
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

/// Is `e` assignable — that is, would [`lvalue_of`] accept it? Lets the parser
/// ask before committing an expression it may still need.
fn is_lvalue(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Var(_) | Expr::Index(..) | Expr::Assoc(..) | Expr::EmptySub(_)
    )
}

/// Convert a parsed expression into an lvalue, or error if it is not
/// assignable (bash: "attempted assignment to non-variable").
fn lvalue_of(e: Expr) -> Result<Lvalue, ArithError> {
    match e {
        Expr::Var(n) => Ok(Lvalue::Var(n)),
        Expr::Index(n, ix) => Ok(Lvalue::Index(n, ix)),
        Expr::Assoc(n, k) => Ok(Lvalue::Assoc(n, k)),
        Expr::EmptySub(n) => Ok(Lvalue::EmptySub(n)),
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
            let i = ix.eval(vars, depth)?;
            match vars.get_index_str(n, i) {
                Some(s) => str_to_val(&s, vars, depth),
                None => Ok(0),
            }
        }
        Expr::Assoc(n, k) => match vars.get_assoc_str(n, k) {
            Some(s) => str_to_val(&s, vars, depth),
            None => Ok(0),
        },
        // Complained about only when actually reached, so a short-circuited
        // `(( 1 ? 7 : a[] ))` is silent.
        Expr::EmptySub(n) => {
            vars.warn_empty_subscript_read(n);
            Ok(0)
        }
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
            store_rlv(&loc, v, vars)?;
            Ok(v)
        }
        Expr::PreIncr(lv, inc) => {
            let loc = resolve_lv(lv, vars, depth)?;
            let step = if *inc { 1 } else { -1 };
            let v = load_rlv(&loc, vars, depth)?.wrapping_add(step);
            store_rlv(&loc, v, vars)?;
            Ok(v)
        }
        Expr::PostIncr(lv, inc) => {
            let loc = resolve_lv(lv, vars, depth)?;
            let old = load_rlv(&loc, vars, depth)?;
            let step = if *inc { 1 } else { -1 };
            store_rlv(&loc, old.wrapping_add(step), vars)?;
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
            let i = ix.eval(vars, depth)?;
            ResolvedLv::Index(n.clone(), i)
        }
        Lvalue::Assoc(n, k) => ResolvedLv::Assoc(n.clone(), k.clone()),
        Lvalue::EmptySub(n) => ResolvedLv::EmptySub(n.clone()),
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
        // A read-modify-write (`a[]++`, `a[]+=2`) reads first, so it earns the
        // read complaint *and* the store refusal `store_rlv` adds below — which
        // is what bash prints for it too.
        ResolvedLv::EmptySub(n) => {
            vars.warn_empty_subscript_read(n);
            Ok(0)
        }
    }
}

/// Write through a resolved lvalue. The store can be refused (a readonly
/// target), and the refusal propagates as an ordinary evaluation error, so the
/// rest of the expression is abandoned — which is what makes `(( y=1, x=2 ))`
/// against a readonly `x` leave `y` assigned and `x` alone.
fn store_rlv(loc: &ResolvedLv, v: i64, vars: &mut dyn VarLookup) -> Result<(), ArithError> {
    match loc {
        ResolvedLv::Var(n) => vars.set(n, v),
        ResolvedLv::Index(n, i) => vars.set_index(n, *i, v),
        ResolvedLv::Assoc(n, k) => vars.set_assoc(n, k, v),
        // Nowhere to store — but not an error: bash complains and carries on
        // with the value, so `x=$(( a[]=3 ))` still yields 3 and succeeds.
        ResolvedLv::EmptySub(n) => {
            vars.refuse_empty_subscript_store(n);
            Ok(())
        }
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

        /// Test-only convenience: seed a value directly, without going through
        /// the refusable [`VarLookup::set`] (nothing here ever refuses).
        fn put(&mut self, name: &str, value: i64) {
            self.0.insert(name.to_string(), value);
        }
    }
    impl VarLookup for Map {
        fn get_str(&self, name: &str) -> Option<String> {
            self.0.get(name).map(i64::to_string)
        }
        fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
            self.0.insert(name.to_string(), value);
            Ok(())
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
        fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
            self.scalars.insert(name.to_string(), value);
            Ok(())
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
        fn set_index(&mut self, name: &str, index: i64, value: i64) -> Result<(), ArithError> {
            if name != "a" {
                return Ok(());
            }
            if let Ok(i) = usize::try_from(index)
                && i < self.a.len()
            {
                self.a[i] = value;
            }
            Ok(())
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
    fn a_subscript_failure_blames_the_subscript() {
        // bash evaluates a subscript through an entry point of its own, so an
        // error raised inside one is *about the subscript*: the text blamed is
        // the subscript's, not the expression's, and the flag that tells the
        // shell to drop its builtin tag and abandon the command list is set.
        let mut m = ArrMap {
            scalars: HashMap::new(),
            a: vec![10, 20, 30],
        };
        for src in ["a[1/0] = 9", "a[1/0]", "b = a[1/0]", "a[1/0]++", "a[1/0] += 2"] {
            let e = eval(src, &mut m).expect_err(src);
            assert_eq!(e.expr_override.as_deref(), Some("1/0"), "{src}");
            assert!(e.in_subscript, "{src}");
            assert_eq!(e.msg, "division by 0");
        }
        // A *parse* failure inside the subscript counts too, and the raw text is
        // kept verbatim — trailing blanks and all, which is what reproduces
        // bash's `1/0  : division by 0 (error token is "0  ")`.
        let e = eval("a[1+] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some("1+"));
        assert!(e.in_subscript);
        let e = eval("a[  1/0  ] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some("  1/0  "));
        // The innermost subscript wins…
        let e = eval("a[a[1/0]] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some("1/0"));
        // …and a failure deeper still — a variable whose *value* is a bad
        // expression — keeps the value `str_to_val` recorded.
        m.scalars.insert("x".to_string(), 0);
        let e = eval("a[x/0] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some("x/0"));
        // An error *outside* any subscript is untouched: it blames the whole
        // expression (the shell's caller-supplied source) and is not fatal.
        let e = eval("a[0] = 1/0", &mut m).unwrap_err();
        assert_eq!(e.expr_override, None);
        assert!(!e.in_subscript);
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

    /// A lookup that refuses to be written through — the shape a readonly
    /// variable presents to the evaluator.
    #[derive(Default)]
    struct NoWrite(HashMap<String, i64>);
    impl VarLookup for NoWrite {
        fn get_str(&self, name: &str) -> Option<String> {
            self.0.get(name).map(i64::to_string)
        }
        fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
            if name == "ro" {
                return Err(ArithError::about_var(name, "readonly variable"));
            }
            self.0.insert(name.to_string(), value);
            Ok(())
        }
    }

    #[test]
    fn a_refused_write_stops_the_expression_where_it_stands() {
        let mut m = NoWrite::default();
        // The refusal is the expression's error, and it names the variable
        // rather than any token of the source.
        let e = eval("ro = 5", &mut m).unwrap_err();
        assert_eq!(e.subject.as_deref(), Some("ro"));
        assert_eq!(e.msg, "readonly variable");
        assert_eq!(e.token, None);
        // Evaluation stops at the refusal: what came before it stands, what
        // comes after never happens.
        assert!(eval("a = 1, ro = 2, b = 3", &mut m).is_err());
        assert_eq!(m.0.get("a"), Some(&1));
        assert_eq!(m.0.get("b"), None);
        // Read-modify-write and the increments go through the same store.
        assert!(eval("ro += 1", &mut m).is_err());
        assert!(eval("ro++", &mut m).is_err());
        assert!(eval("++ro", &mut m).is_err());
        // An untaken branch never reaches the store, so it is no error at all.
        assert_eq!(eval("0 ? ro = 9 : 7", &mut m).unwrap(), 7);
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
        fn set_assoc(&mut self, name: &str, key: &str, value: i64) -> Result<(), ArithError> {
            if name == "m" {
                self.0.insert(key.to_string(), value);
            }
            Ok(())
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
        fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
            self.0.insert(name.to_string(), value.to_string());
            Ok(())
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
    fn quotes_are_not_the_evaluators_to_remove() {
        // It is the *expansion* pass in front of the evaluator that deletes
        // double quotes, not the evaluator — so `$(( "3" + "4" ))` is 7 only
        // because expansion hands over `3 + 4` (see
        // `interp::tests::an_arithmetic_string_is_dequoted_by_whoever_expands_it`).
        // Text the evaluator reads for itself keeps its quotes and is rejected,
        // which is what makes `x='"3"'; $(( x+1 ))` and `let 'y="3"+4'` errors in
        // bash where osh used to answer 4 and 7.
        // A quote where an operand belongs is an unexpected operand…
        for bad in [r#""3" + "4""#, r#""3""#, r#"'3'"#] {
            let e = eval(bad, &mut Map::default()).unwrap_err();
            assert_eq!(e.msg, "syntax error: operand expected", "{bad}");
        }
        // …and one after a complete operand is an unexpected *operator*, which is
        // how bash words `1"2"3` too.
        let e = eval(r#"1"2"3"#, &mut Map::default()).unwrap_err();
        assert_eq!(e.msg, "syntax error: invalid arithmetic operator");
        assert_eq!(e.token.as_deref(), Some(r#""2"3"#));
        // The error token starts at the quote, so the diagnostic shows it.
        let e = eval(r#"y="3"+4"#, &mut Map::default()).unwrap_err();
        assert_eq!(e.token.as_deref(), Some(r#""3"+4"#));
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
        m.put("x", 5);
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

    /// `++` and `--` are increment operators only where an increment is
    /// possible; everywhere else the two characters are simply two operators in
    /// a row. Every expectation here is bash 5.2.37's own answer.
    #[test]
    fn increment_operators_need_an_lvalue() {
        let mut m = Map::default();
        m.put("v", 5);
        // Nothing assignable follows, so `--2` is `-(-2)` and `++2` is `+(+2)`.
        assert_eq!(eval("--2", &mut m).unwrap(), 2);
        assert_eq!(eval("++2", &mut m).unwrap(), 2);
        assert_eq!(eval("--(3)", &mut m).unwrap(), 3);
        assert_eq!(eval("++3+1", &mut m).unwrap(), 4);
        // Nor is the operand on the *left* assignable, so these are a binary
        // operator followed by a unary one: `2 - (-3)`, `3 - (-(-2))`.
        assert_eq!(eval("2--3", &mut m).unwrap(), 5);
        assert_eq!(eval("3---2", &mut m).unwrap(), 1);
        // A name may still follow across whitespace, and a real decrement wins
        // over the reading above wherever one is possible.
        assert_eq!(eval("-- v", &mut m).unwrap(), 4);
        assert_eq!(eval("v---3", &mut m).unwrap(), 1); // (v--) - 3, v now 4
        assert_eq!(m.get("v"), Some(3));
        // With no operand at all, the error token starts at the *second*
        // character — which is just where the one operator bash did read
        // begins. See `error_tokens_match_bash` for that pair of cases.
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
            // A missing operand after an assignment operator reports the operator
            // as the error token (bash: `x = ` → `= `, `y += ` → `+= `), not the
            // whole expression.
            ("x = ", "syntax error: operand expected (error token is \"= \")"),
            ("y += ", "syntax error: operand expected (error token is \"+= \")"),
            // A missing operand after a prefix unary reports from the unary
            // operator; bash's error pointer for `++`/`--` lands one char in.
            ("1 + + ", "syntax error: operand expected (error token is \"+ \")"),
            ("++ ", "syntax error: operand expected (error token is \"+ \")"),
            ("-- ", "syntax error: operand expected (error token is \"- \")"),
            ("~ ", "syntax error: operand expected (error token is \"~ \")"),
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
            // A base with an octal (`0…`) or hex (`0x…`) prefix cannot precede
            // `#`: bash's strlong sets `foundbase` on the prefix and rejects the
            // `#` as "invalid number" — regardless of the prefixed value being an
            // otherwise-valid base (`064` = 52, `0x8` = 8). A non-octal digit in
            // the octal prefix is diagnosed earlier as "value too great".
            ("064#1", "invalid number (error token is \"064#1\")"),
            ("065#1", "invalid number (error token is \"065#1\")"),
            ("08#1", "value too great for base (error token is \"08#1\")"),
            ("0x8#1", "invalid number (error token is \"0x8#1\")"),
        ];
        for (src, want) in cases {
            let e = eval(src, &mut Map::default()).unwrap_err();
            assert_eq!(&e.to_string(), want, "expr {src:?}");
        }
    }

    #[test]
    fn a_missing_close_paren_reports_from_the_cursor() {
        // The error token is the source from wherever the parser was standing
        // when the `)` failed to appear — not the group's first token, and not
        // the last operand it happened to parse.
        let cases: &[(&str, &str)] = &[
            ("( a b", "missing `)' (error token is \"b\")"),
            ("(1 2", "missing `)' (error token is \"2\")"),
            ("(2+3 4", "missing `)' (error token is \"4\")"),
            ("(1,2 3", "missing `)' (error token is \"3\")"),
            ("(1?2:3 4", "missing `)' (error token is \"4\")"),
            ("(x=1 2", "missing `)' (error token is \"2\")"),
            ("(a[0] b", "missing `)' (error token is \"b\")"),
            ("((1)(2)", "missing `)' (error token is \"(2)\")"),
            ("(2 3)", "missing `)' (error token is \"3)\")"),
            // A nested group fails first and reports from inside itself.
            ("( (1 2) 3", "missing `)' (error token is \"2) 3\")"),
            ("( ( a b ) )", "missing `)' (error token is \"b ) )\")"),
            ("1 + ( a b )", "missing `)' (error token is \"b )\")"),
            ("( a b ) + 1", "missing `)' (error token is \"b ) + 1\")"),
            // At end of input the parser stands on nothing, so the token is the
            // last one it read. For `(2+3` that is the operand…
            ("(2+3", "missing `)' (error token is \"3\")"),
            ("(1", "missing `)' (error token is \"1\")"),
            ("(a", "missing `)' (error token is \"a\")"),
            // …the `-` is an operator, so the last token is the `1` after it…
            ("(-1", "missing `)' (error token is \"1\")"),
            // …a name with a subscript is a single token…
            ("(a[0]", "missing `)' (error token is \"a[0]\")"),
            // …and here it is a close paren rather than an operand.
            ("((2+3)", "missing `)' (error token is \")\")"),
            // A character the lexer has no token for is rejected before the
            // missing-`)` check is ever reached.
            (
                "(1 @",
                "syntax error: invalid arithmetic operator (error token is \"@\")",
            ),
            (
                "(1;2",
                "syntax error: invalid arithmetic operator (error token is \";2\")",
            ),
            // Brackets belong to `a[0]`, which is lexed as one token, so a
            // bracket reached in operator position is not a token at all —
            // inside a group or out of one.
            (
                "(2+3]",
                "syntax error: invalid arithmetic operator (error token is \"]\")",
            ),
            (
                "2+3]",
                "syntax error: invalid arithmetic operator (error token is \"]\")",
            ),
            (
                "2+3[",
                "syntax error: invalid arithmetic operator (error token is \"[\")",
            ),
            // `:` *is* a token, so it stays an expression error.
            ("2+3:", "syntax error in expression (error token is \":\")"),
            ("(2+3))", "syntax error in expression (error token is \")\")"),
        ];
        for (src, want) in cases {
            let e = eval(src, &mut Map::default()).unwrap_err();
            assert_eq!(&e.to_string(), want, "expr {src:?}");
        }
        // A balanced group still evaluates.
        assert_eq!(ev("(2+3)"), 5);
        assert_eq!(ev("((2+3))"), 5);
        assert_eq!(ev("(1,2 )"), 2);
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
