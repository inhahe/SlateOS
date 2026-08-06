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
//!
//! Everything here is **byte-native**. Arithmetic syntax is entirely ASCII, so
//! the lexer reads bytes and a byte that decodes to no character is simply one
//! it has no token for — which is also what bash, reading the same bytes,
//! reports. Two things in an expression are nevertheless not ASCII: an
//! *associative subscript*, which is a literal key and may hold any byte the
//! array's own `m[$k]=v` could store, and a variable *value*, which is
//! recursively evaluated and echoed back in the diagnostic when it fails.

use crate::bfmt;
use crate::bytes::{self, BStr, Str};

/// Resolves and mutates variables during arithmetic evaluation.
///
/// The read methods (`get`/`get_index`/`get_assoc`) return `None` for an unset
/// variable/element (the evaluator treats that as `0`). The write methods have
/// empty defaults so a read-only implementor need not provide them.
pub trait VarLookup {
    /// Answer the `set -u` question for the operand `name` is about to be read
    /// from, and refuse the whole expression if it is unset.
    ///
    /// bash asks this in `expr_streval` (expr.c:1180) and it is **not** the
    /// question a word expansion asks. Arithmetic reads a name as a *variable*,
    /// so what matters is whether the variable exists and is visible, not
    /// whether the thing addressed within it has a value: `declare -a a=();
    /// echo "${a[0]}"` is an unbound-variable error while `(( a[0] ))` is a
    /// silent 0, and conversely a bare `declare -a a` — which declares without
    /// assigning, so bash holds it *invisible* — is unbound in arithmetic even
    /// though `a[0]` was never a value either way. An existing variable holding
    /// the empty string is 0 without complaint, because bash checks the
    /// variable and only then looks at its text.
    ///
    /// `subscripted` says the operand was written `name[…]`. bash reads that
    /// form through `array_variable_part` rather than `find_variable`, which
    /// changes two things: the question becomes "is there an array here" (an
    /// index the array does not hold is a silent 0), and a failure is named
    /// after the *written* base rather than after whatever a nameref would have
    /// reached. The check also runs **before** the subscript is evaluated, so
    /// `(( nada[nope] ))` names `nada`.
    ///
    /// Called for every operand the evaluator actually reaches and no others —
    /// which is what makes `(( 0 && nope ))` and `(( 1 ? 2 : nope ))` silent.
    /// bash gets the same effect one level up, by returning from `expr_streval`
    /// before the check when `noeval` is set.
    ///
    /// The `Err` abandons the expression where it stands, as bash's `longjmp`
    /// out of `expr_streval` does, so only the first unset name of
    /// `(( nope1 + nope2 ))` is ever reported and `(( nope / 0 ))` never reaches
    /// the division. It is an [`ArithError::silent`] one because the implementor
    /// has already printed the diagnostic bash prints.
    ///
    /// The default accepts everything, for implementors with no such option.
    fn note_arith_unbound(&mut self, name: &str, subscripted: bool) -> Result<(), ArithError> {
        let _ = (name, subscripted);
        Ok(())
    }

    /// Return the scalar variable's raw value, or `None` if unset (treated as
    /// `0`). The value is not a plain integer: bash recursively evaluates it as
    /// an arithmetic expression, so `b=a; a=5; $((b))` yields `5` and
    /// `x="2+3"; $((x))` yields `5`. The evaluator performs that recursion
    /// (with a depth guard for cycles like `x=x`); implementors just return
    /// what is stored.
    ///
    /// Bytes, because a shell value is bytes. One that is not text is not an
    /// arithmetic expression either, but it is still what the diagnostic must
    /// echo back — see [`ArithError::expr_override`].
    ///
    /// `&mut` because answering can *run* shell code: a nameref whose target
    /// carries a subscript (`declare -n r='n[$(f)]'`) has that subscript
    /// expanded afresh at every read, command substitution and all.
    fn get_str(&mut self, name: &str) -> Option<Str>;

    /// Return the raw value of the array element `name[index]`, or `None` if
    /// unset/out-of-range (treated as `0`). `index` has already been evaluated
    /// arithmetically (so `(( a[i+1] ))` and negative indices work). Like
    /// [`VarLookup::get_str`], the value is recursively arithmetic-evaluated.
    /// The default ignores subscripts — array-backed implementors override it.
    ///
    /// `&mut` for [`VarLookup::get_str`]'s reason and one more: a scalar answers
    /// `name[0]`, and a scalar whose value is *computed* answers it by computing
    /// — so `$(( PPID[0] ))` is the pid and `$(( RANDOM[0] ))` draws a number,
    /// exactly as the unsubscripted spellings do.
    fn get_index_str(&mut self, name: &str, index: i64) -> Option<Str> {
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

    /// Return the raw value of associative element `name[key]`, or `None` if
    /// unset (treated as `0`). `key` is the raw, already-expanded subscript
    /// (bash does not arithmetic-evaluate associative subscripts), so it is
    /// **bytes**: the same arbitrary key `m[$k]=v` would have stored. The value
    /// is recursively arithmetic-evaluated. Only consulted when
    /// [`VarLookup::is_assoc`] returns `true`.
    fn get_assoc_str(&self, name: &str, key: BStr<'_>) -> Option<Str> {
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
    fn set_assoc(&mut self, name: &str, key: BStr<'_>, value: i64) -> Result<(), ArithError> {
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

    /// Refuse a whole-array subscript (`(( a[@] ))`, `(( a[*]=9 ))`) — bash
    /// prints `NAME[@]: bad array subscript`, untagged, once per read and once
    /// per store, and then carries on: the read is worth 0 and the store is
    /// dropped. See [`Expr::WholeSub`].
    ///
    /// Unlike the empty-subscript hooks above, bash gets as far as *finding*
    /// the array before it refuses the subscript, so an implementation that
    /// reports anything about resolving the name (a circular nameref) does that
    /// first. The name is blamed as written, not as the reference resolves it.
    fn refuse_whole_array_subscript(&mut self, name: &str, sym: u8) {
        let _ = (name, sym);
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
    /// The offending token (bash's `error token is "…"`), if known. A slice of
    /// the source, so bytes: echoing it approximately would name something
    /// other than what the shell actually read.
    pub token: Option<Str>,
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
    /// Bytes for the same reason [`ArithError::token`] is — and more sharply,
    /// since a variable's value is exactly the place a non-text byte arrives
    /// from.
    pub expr_override: Option<Str>,
    /// The *name* the diagnostic is about, when the failure is a property of a
    /// variable rather than of the expression's text. A refused write to a
    /// readonly variable reads `bash: line 1: x: readonly variable` — the
    /// subject is `x`, not the `x=5` that was being evaluated — and carries
    /// neither the `((`/`let` builtin tag nor an `(error token is …)` suffix,
    /// since neither the command nor any token is what went wrong. `None` for
    /// the ordinary errors, which are about the expression and echo it.
    pub subject: Option<String>,
    /// Set when the failure carries no diagnostic at all. A write refused
    /// because the shell *maintains* the variable (`(( GROUPS = 5 ))`) fails
    /// exactly as a readonly one does — the expression is abandoned where it
    /// stands, so earlier assignments in a comma list stand and later ones never
    /// happen; `(( ))` and `let` report 1; an arithmetic *expansion* is fatal to
    /// the command list — but says nothing, because bash's `att_noassign`
    /// refusal is silent wherever it happens. [`Shell::emit_arith_error`] honours
    /// this by printing nothing; every other consequence is unchanged.
    pub silent: bool,
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
            silent: false,
            in_subscript: false,
        }
    }

    /// A diagnostic carrying bash's `(error token is "…")` suffix.
    fn with_token(msg: impl Into<String>, token: impl Into<Str>) -> Self {
        Self {
            msg: msg.into(),
            token: Some(token.into()),
            truncate_leading: false,
            expr_override: None,
            subject: None,
            silent: false,
            in_subscript: false,
        }
    }

    /// A number-literal lexer error whose token is a complete literal; the
    /// echoed source is truncated at the literal's end (bash behaviour).
    fn lexeme_error(msg: impl Into<String>, lexeme: impl Into<Str>) -> Self {
        Self {
            msg: msg.into(),
            token: Some(lexeme.into()),
            truncate_leading: true,
            expr_override: None,
            subject: None,
            silent: false,
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
            silent: false,
            in_subscript: false,
        }
    }

    /// A refusal with no diagnostic at all — see [`ArithError::silent`]. Used
    /// for the variables the shell maintains, whose refusal bash never reports.
    ///
    /// The message is filled in anyway, so that an error escaping the silencing
    /// would read as something rather than as an empty line.
    #[must_use]
    pub fn silently_refused(name: impl Into<String>) -> Self {
        Self {
            silent: true,
            ..Self::about_var(name, "cannot be assigned to")
        }
    }

    /// The diagnostic body bash prints after its `<expr>:` prefix — the message
    /// and, when there is one, the `(error token is "…")` suffix.
    ///
    /// A method returning bytes rather than a `Display` impl, because the error
    /// token is a slice of the source and the source may hold any byte. A
    /// `Display` would have to approximate it, and the whole point of naming
    /// the offending token is that the name is exact.
    #[must_use]
    pub fn body(&self) -> Str {
        match &self.token {
            Some(t) => bfmt![&self.msg, b" (error token is \"", t, b"\")"],
            None => self.msg.clone().into_bytes(),
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
    /// Associative array element `name[key]` (subscript is a literal key, and
    /// so arbitrary bytes).
    Assoc(String, Str),
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
    /// A subscript that is exactly `@` or `*` — `a[@]`. Legal *bytes* as far as
    /// the parser is concerned, and an associative array reads them as ordinary
    /// keys, but an indexed one has no index there and refuses them at lookup
    /// time. Like [`Expr::EmptySub`] the refusal is a complaint rather than an
    /// error (the read is worth 0, the store is dropped), and like it the check
    /// is purely lexical — but on the *exact* bytes: bash rejects `a[ @]` and
    /// `a['@']` as ordinary syntax errors. The byte is kept because it is echoed
    /// back in the complaint.
    WholeSub(String, u8),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    BitNot(Box<Expr>),
    /// A binary operation; the operator is one of the [`apply`]/short-circuit
    /// tokens (`+`, `-`, `*`, `/`, `%`, `**`, `<<`, `>>`, comparisons, `&`,
    /// `^`, `|`, `&&`, `||`). The final field is bash's "error token" for an
    /// eval-time failure — a slice of the source running to the end of the
    /// expression, whose start bash picks differently per operator (see
    /// `parse_binary`): the right operand's for `/` and `%`, the token
    /// *following* the exponent for `**`. `None` for operators that cannot fail
    /// at evaluation.
    Bin(String, Box<Expr>, Box<Expr>, Option<Box<[u8]>>),
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
    Assoc(String, Str),
    /// `a[] = …` — see [`Expr::EmptySub`]. Assignable only in the sense that
    /// bash parses it and then drops the store.
    EmptySub(String),
    /// `a[@] = …` — see [`Expr::WholeSub`]. Assignable in the same nominal
    /// sense: parsed, complained about, and dropped.
    WholeSub(String, u8),
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
    raw: Str,
}

impl Sub {
    /// Parse `raw` as a subscript expression. A *parse* failure is a subscript
    /// failure too: `((a[1+]=9))` reports `1+: syntax error: operand expected`.
    fn parse(raw: BStr<'_>, vars: &dyn VarLookup) -> Result<Self, ArithError> {
        let expr = parse(raw, vars).map_err(|e| tag_subscript(e, raw))?;
        Ok(Self {
            expr,
            raw: raw.to_vec(),
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
fn tag_subscript(mut e: ArithError, raw: BStr<'_>) -> ArithError {
    if e.expr_override.is_none() {
        e.expr_override = Some(raw.to_vec());
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
    Assoc(String, Str),
    /// `a[]` — see [`Expr::EmptySub`]. There is no index to resolve.
    EmptySub(String),
    /// `a[@]` — see [`Expr::WholeSub`]. There is no index to resolve.
    WholeSub(String, u8),
}

/// Evaluate an arithmetic expression string against a mutable variable
/// environment (assignment/increment operators mutate `vars`).
///
/// # Errors
/// Returns [`ArithError`] on a syntax error, division/modulo by zero, a
/// negative exponent, or assignment to a non-lvalue.
pub fn eval(expr: BStr<'_>, vars: &mut dyn VarLookup) -> Result<i64, ArithError> {
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
fn str_to_val(s: BStr<'_>, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
    // Only the *front* is trimmed. bash's lexer skips leading whitespace as a
    // matter of course, and its diagnostic skips it again when echoing the
    // failing value — but trailing whitespace is simply part of the string
    // being lexed, so an error token that runs to the end of the value carries
    // it: `x='1 + '; $(( x ))` reports `1 + : … (error token is "+ ")`, with
    // both the echoed expression and the token keeping the final space.
    let t = bytes::trim_start(s);
    // The fast paths ask about the value's *content*, so they look past the
    // trailing whitespace that only the diagnostics care about.
    let core = bytes::trim_end(t);
    if core.is_empty() {
        return Ok(0);
    }
    // Fast path: a plain decimal literal (the overwhelmingly common case — loop
    // counters, sizes) needs no re-parse. A leading zero means octal, so defer
    // those (and hex / `base#n` / sub-expressions) to the full parser below.
    if let Some(n) = plain_decimal(core) {
        return Ok(n);
    }
    if depth >= RECURSION_LIMIT {
        // bash reports the offending value token here, and uses the innermost
        // value as the `<expr>:` prefix (recorded via `expr_override`).
        let mut e = ArithError::with_token("expression recursion level exceeded", t);
        e.expr_override = Some(t.to_vec());
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
                e.expr_override = Some(t.to_vec());
            }
            e
        })
}

/// Parse `t` as a plain decimal integer (optionally signed), returning `None`
/// for anything that needs the full arithmetic parser: empty, non-digits, or a
/// leading-zero form (`010`) which arithmetic treats as octal.
fn plain_decimal(t: BStr<'_>) -> Option<i64> {
    let digits = match t.split_first() {
        Some((b'+' | b'-', rest)) => rest,
        _ => t,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits.len() > 1 && digits.first() == Some(&b'0') {
        return None; // octal — let the full parser apply base rules
    }
    // Every byte is an ASCII digit or a sign by now, so reading the run back as
    // text is exact rather than an approximation.
    bytes::as_str(t)?.parse::<i64>().ok()
}

/// Parse an arithmetic expression into an AST (no evaluation, no mutation).
fn parse(expr: BStr<'_>, vars: &dyn VarLookup) -> Result<Expr, ArithError> {
    let mut p = AParser {
        // Quotes reach here only if something *upstream* left them, and then
        // they are ordinary (invalid) characters. It is the expansion pass in
        // front of the evaluator that removes double quotes, not the evaluator:
        // `$(( "3" + "4" ))` is 7 because expansion hands over `3 + 4`, while a
        // value the evaluator reads for itself keeps them and is rejected —
        // `x='"3"'; $(( x+1 ))` is `"3": syntax error: operand expected`, as is
        // `let 'y="3"+4'`, whose argument no expansion pass ever saw. Single
        // quotes are never removed by either.
        src: expr,
        pos: 0,
        last_op_start: 0,
        last_atom_start: 0,
        last_tok_start: 0,
        vars,
    };
    p.skip_ws();
    // An empty (or whitespace-only) arithmetic expression is `0` in bash:
    // `$(( ))`, and — after expansion — `n=; echo $((n))` / `$(( $x ))`.
    if p.pos == p.src.len() {
        return Ok(Expr::Num(0));
    }
    let e = p.parse_comma()?;
    p.skip_ws();
    if p.pos != p.src.len() {
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
    /// The expression source. Bytes, not characters: every token arithmetic has
    /// is ASCII, so a byte that begins no character begins no token either.
    src: BStr<'a>,
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
fn is_arith_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || b"+-*/%|^&<>=!~()?:,".contains(&c)
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
        while matches!(self.src.get(self.pos), Some(&c) if bytes::is_space(c)) {
            self.pos += 1;
        }
    }

    /// The de-quoted source from `start` to the end of the expression — the
    /// slice bash reports as its `(error token is "…")`.
    fn rest_from(&self, start: usize) -> Str {
        self.src.get(start..).unwrap_or_default().to_vec()
    }

    /// The source slice from `start` to the cursor — a lexeme just consumed.
    fn lexeme(&self, start: usize) -> Str {
        self.src.get(start..self.pos).unwrap_or_default().to_vec()
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

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// The longest operator token at the cursor (without consuming). Recognises
    /// 3-, 2-, and 1-character operators, including assignment and
    /// increment/decrement forms so the binary-operator parser can tell `+`
    /// from `+=`/`++`.
    ///
    /// Answers with the table's own `&'static str` rather than a slice of the
    /// source, so `binop_bp`/`is_assign_op`/`apply` keep matching on text
    /// without any byte of the input ever being converted. Sound because every
    /// operator is ASCII, hence its own spelling.
    fn read_op(&self) -> Option<&'static str> {
        let rest = self.src.get(self.pos..)?;
        // Longest match first: `<<=` before `<<` before `<`.
        [
            "<<=", ">>=", "**", "==", "!=", "<=", ">=", "<<", ">>", "&&", "||", "++", "--", "+=",
            "-=", "*=", "/=", "%=", "&=", "|=", "^=", "+", "-", "*", "/", "%", "|", "^", "&", "<",
            ">", "=", "!", "~",
        ]
        .into_iter()
        .find(|op| rest.starts_with(op.as_bytes()))
    }

    /// Comma operator (`e1, e2, …`) — the loosest-binding arithmetic operator.
    fn parse_comma(&mut self) -> Result<Expr, ArithError> {
        let mut e = self.parse_assign()?;
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
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
            && is_assign_op(op)
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
            self.pos += op.len();
            let rhs = self.parse_assign()?;
            return Ok(Expr::Assign(lv, assign_base(op), Box::new(rhs)));
        }
        Ok(lhs)
    }

    /// Ternary conditional `cond ? then : else` — right-associative.
    fn parse_ternary(&mut self) -> Result<Expr, ArithError> {
        let cond = self.parse_binary(0)?;
        self.skip_ws();
        if self.peek() != Some(b'?') {
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
            Some(b':') => {
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
        if self.peek() != Some(b':') {
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
            let op = match op {
                "++" => "+",
                "--" => "-",
                other => other,
            };
            let Some((bp, right)) = binop_bp(op) else {
                break;
            };
            if bp < min_bp {
                break;
            }
            self.mark_op();
            self.pos += op.len();
            let next_min = if right { bp } else { bp + 1 };
            self.skip_ws();
            // `/` and `%` report a division by zero against the right operand:
            // bash's `exp2` saves the cursor just before reading it and points
            // the diagnostic back there, so `1/0/0` names `0/0`, not the `0`
            // that was actually zero. Capture the same cursor.
            let rhs_tok = matches!(op, "/" | "%").then(|| self.rest_from(self.pos));
            let rhs = self.parse_binary(next_min)?;
            // `**` saves no such cursor — `exp_power` just calls `evalerror` —
            // so a negative exponent is reported against whatever token the
            // lexer happens to be holding. bash reads one token ahead, so that
            // is the token *following* the exponent: `2**-1+9` names `+9` and
            // `2**-1*8` names `*8`, neither of which is part of the exponent at
            // all. At end of input the lexer reads no further token and keeps
            // the previous one, so the exponent's own last token is named
            // instead (`2**-1` names `1`, and `2**(-1)` names the `)`).
            let rhs_tok = if op == "**" {
                self.skip_ws();
                Some(if self.pos == self.src.len() {
                    self.rest_from(self.last_tok_start)
                } else {
                    self.rest_from(self.pos)
                })
            } else {
                rhs_tok
            };
            lhs = Expr::Bin(
                op.to_string(),
                Box::new(lhs),
                Box::new(rhs),
                rhs_tok.map(Vec::into_boxed_slice),
            );
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
            Some(b'-') => {
                self.mark_op();
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            Some(b'+') => {
                self.mark_op();
                self.pos += 1;
                self.parse_unary()
            }
            Some(b'!') => {
                self.mark_op();
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            Some(b'~') => {
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
        while matches!(self.src.get(i), Some(&c) if bytes::is_space(c)) {
            i += 1;
        }
        matches!(self.src.get(i), Some(c) if c.is_ascii_alphabetic() || *c == b'_')
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
            Some(b'(') => {
                self.mark_tok(atom_start);
                self.pos += 1;
                // A parenthesised group is a full expression: ternary, comma,
                // and assignment are allowed inside.
                let e = self.parse_comma()?;
                self.skip_ws();
                if self.peek() != Some(b')') {
                    // bash names the token it is standing on when the `)` fails
                    // to appear (`( a b` → `b`, `((1)(2)` → `(2)`). At end of
                    // input it is standing on nothing, so it names the last
                    // token it did lex — which is why `(2+3` names `3` but
                    // `((2+3)` names the `)`.
                    let token = if self.pos == self.src.len() {
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
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                self.mark_atom(atom_start);
                // A shell identifier is ASCII by its own syntax, so reading the
                // name back as text is exact however the rest of the expression
                // is spelled.
                let name_start = self.pos;
                while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
                    self.pos += 1;
                }
                let name = self
                    .src
                    .get(name_start..self.pos)
                    .and_then(bytes::as_str)
                    .unwrap_or_default()
                    .to_string();
                // Array subscript `name[sub]`: for an indexed array the
                // subscript is an arithmetic expression (`a[i+1]`, negatives);
                // for an associative array it is a literal string key
                // (`m[foo]`). Capture the raw bracketed text (balanced
                // brackets), then dispatch on the array kind. No whitespace is
                // allowed between the name and `[`.
                if self.peek() == Some(b'[') {
                    self.pos += 1;
                    let sub_start = self.pos;
                    let mut depth = 1usize;
                    while let Some(c) = self.peek() {
                        match c {
                            b'[' => depth += 1,
                            b']' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                    if self.peek() != Some(b']') {
                        // bash: "bad array subscript"; the error token runs from
                        // the array name (`foo[` → token `foo[`).
                        return Err(ArithError::with_token(
                            "bad array subscript",
                            self.rest_from(atom_start),
                        ));
                    }
                    let raw = self.src.get(sub_start..self.pos).unwrap_or_default();
                    self.pos += 1; // consume the closing ']'
                    if raw.is_empty() {
                        // `a[]` — see `Expr::EmptySub`. Deliberately ahead of the
                        // indexed/associative split, because bash refuses it
                        // without looking at the name.
                        return Ok(Expr::EmptySub(name));
                    }
                    if self.vars.is_assoc(&name) {
                        // An associative subscript is a literal *key*, not an
                        // expression, so it may hold any byte — the same key
                        // the `m[$k]=v` that stored the element carried.
                        return Ok(Expr::Assoc(name, bytes::trim(raw).to_vec()));
                    }
                    // `a[@]`, `a[*]` — see `Expr::WholeSub`. Necessarily after
                    // the question above, since those are perfectly good
                    // *keys*, and matched on the exact bytes: bash reads `a[ @]`
                    // and `a['@']` as ordinary expressions and fails them as
                    // syntax errors.
                    if let [sym @ (b'@' | b'*')] = raw {
                        return Ok(Expr::WholeSub(name, *sym));
                    }
                    // Indexed: parse the subscript as its own arithmetic
                    // expression (evaluated later against the live environment).
                    let sub = Sub::parse(raw, self.vars)?;
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
        if self.peek() == Some(b'0') && matches!(self.src.get(self.pos + 1), Some(b'x' | b'X')) {
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
                let lexeme = self.lexeme(start);
                return Err(ArithError::lexeme_error("value too great for base", lexeme));
            }
            // A prefixed literal (`0x…`) cannot serve as the base of a
            // `base#num` construct: bash's strlong() sets `foundbase` on the
            // `0x` prefix and rejects a subsequent `#` as "invalid number"
            // (`0x8#1`). Consume the rest of the token so the error names the
            // whole literal, matching bash.
            if self.peek() == Some(b'#') {
                self.pos += 1;
                while matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                    self.pos += 1;
                }
                let lexeme = self.lexeme(start);
                return Err(ArithError::lexeme_error("invalid number", lexeme));
            }
            let hex = self.src.get(hstart..self.pos).unwrap_or_default();
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
            for &c in hex {
                if let Some(d) = digit_value(c, 16) {
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
        if self.peek() == Some(b'#') {
            let base_str = self.src.get(start..self.pos).unwrap_or_default();
            self.pos += 1; // consume '#'
            let dstart = self.pos;
            // Consume the whole digit lexeme (every char that is a digit in
            // *some* base: 0-9, a-z, A-Z, @, _) so the error token spans the
            // full literal exactly as bash reports it — `5+2#12+9` blames
            // `2#12`, not `2` or `2#12+9`.
            while matches!(self.peek(), Some(c) if digit_value(c, 64).is_some()) {
                self.pos += 1;
            }
            let lexeme = self.lexeme(start);
            // A base written with a leading `0` is an octal-prefixed literal, so
            // bash's strlong() sets `foundbase` while reading it and then rejects
            // the `#` as "invalid number" (`064#1`, `0#1`). It reads the base
            // digits in octal first, however, so a non-octal digit in that prefix
            // is diagnosed earlier as "value too great for base" (`08#1`). A bare
            // `0` base (len 1) falls through to the base-range check below, where
            // `base == 0` also yields "invalid number".
            if base_str.len() > 1 && base_str.first() == Some(&b'0') {
                for &c in base_str.get(1..).unwrap_or_default() {
                    if digit_value(c, 8).is_none() {
                        return Err(ArithError::lexeme_error(
                            "value too great for base",
                            lexeme,
                        ));
                    }
                }
                return Err(ArithError::lexeme_error("invalid number", lexeme));
            }
            // The base is a run of ASCII digits by construction, so reading it
            // back as text is exact; only its *value* can be out of range.
            let base: u32 = bytes::as_str(base_str)
                .and_then(|b| b.parse().ok())
                .ok_or_else(|| {
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
            for &c in self.src.get(dstart..self.pos).unwrap_or_default() {
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
            let lexeme = self.lexeme(start);
            return Err(ArithError::lexeme_error("value too great for base", lexeme));
        }
        let text = self.src.get(start..self.pos).unwrap_or_default();
        // A leading zero (other than bare "0") denotes octal. bash reports a
        // non-octal digit (`099`, `0778`) as "value too great for base", but an
        // octal literal that overflows i64 *wraps* rather than erroring
        // (`$((077777777777777777777777777))` → -1), matching C accumulation.
        if text.len() > 1 && text.first() == Some(&b'0') {
            let mut val: i64 = 0;
            for &c in text {
                let Some(d) = digit_value(c, 8) else {
                    return Err(ArithError::lexeme_error("value too great for base", text));
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
        for &c in text {
            if let Some(d) = digit_value(c, 10) {
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
fn digit_value(c: u8, base: u32) -> Option<u32> {
    let v = match c {
        b'0'..=b'9' => u32::from(c.wrapping_sub(b'0')),
        b'a'..=b'z' => 10 + u32::from(c.wrapping_sub(b'a')),
        b'A'..=b'Z' => {
            if base <= 36 {
                10 + u32::from(c.wrapping_sub(b'A'))
            } else {
                36 + u32::from(c.wrapping_sub(b'A'))
            }
        }
        b'@' => 62,
        b'_' => 63,
        _ => return None,
    };
    if v < base { Some(v) } else { None }
}

/// Is `e` assignable — that is, would [`lvalue_of`] accept it? Lets the parser
/// ask before committing an expression it may still need.
fn is_lvalue(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Var(_) | Expr::Index(..) | Expr::Assoc(..) | Expr::EmptySub(_) | Expr::WholeSub(..)
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
        Expr::WholeSub(n, s) => Ok(Lvalue::WholeSub(n, s)),
        _ => Err(ArithError::new("attempted assignment to non-variable")),
    }
}

fn eval_expr(e: &Expr, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
    match e {
        Expr::Num(n) => Ok(*n),
        // A variable read resolves the raw value string and (like bash)
        // recursively evaluates it as an arithmetic expression. Every one of
        // the three is a *variable* read and so answers to `set -u` first — see
        // [`VarLookup::note_arith_unbound`], and note that the subscripted forms
        // ask before evaluating their subscript.
        Expr::Var(n) => {
            vars.note_arith_unbound(n, false)?;
            match vars.get_str(n) {
                Some(s) => str_to_val(&s, vars, depth),
                None => Ok(0),
            }
        }
        Expr::Index(n, ix) => {
            vars.note_arith_unbound(n, true)?;
            let i = ix.eval(vars, depth)?;
            match vars.get_index_str(n, i) {
                Some(s) => str_to_val(&s, vars, depth),
                None => Ok(0),
            }
        }
        Expr::Assoc(n, k) => {
            vars.note_arith_unbound(n, true)?;
            match vars.get_assoc_str(n, k) {
                Some(s) => str_to_val(&s, vars, depth),
                None => Ok(0),
            }
        }
        // Complained about only when actually reached, so a short-circuited
        // `(( 1 ? 7 : a[] ))` is silent.
        //
        // Deliberately *not* asked about `set -u`, unlike the whole-array
        // subscript below. bash does ask, but its `array_variable_name` returns
        // a null pointer for a subscript it has already refused, so what comes
        // out is the literal text `(null): unbound variable` — a printed C null,
        // which is a defect rather than a behaviour. See known-issues
        // TD-OILS-AN-EMPTY-ARITHMETIC-SUBSCRIPT-IS-NAMED-(null)-BY-BASH.
        Expr::EmptySub(n) => {
            vars.warn_empty_subscript_read(n);
            Ok(0)
        }
        // Same shape as the empty subscript beside it: reached only when the
        // operand is actually evaluated, so `(( 1 ? 7 : a[@] ))` is silent.
        //
        // `set -u` is asked first and *replaces* the refusal when it fires,
        // because bash reaches the array's own variable before it looks at what
        // the subscript says: `declare -a a; (( a[@] ))` is `a: unbound
        // variable` alone, where the same expression with `a` assigned — or with
        // nounset off — is the bad-subscript refusal alone.
        Expr::WholeSub(n, s) => {
            vars.note_arith_unbound(n, true)?;
            vars.refuse_whole_array_subscript(n, *s);
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
                        e.token = Some(t.to_vec());
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
            if base.is_some() {
                note_lv_unbound(lv, vars)?;
            }
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
            note_lv_unbound(lv, vars)?;
            let loc = resolve_lv(lv, vars, depth)?;
            let step = if *inc { 1 } else { -1 };
            let v = load_rlv(&loc, vars, depth)?.wrapping_add(step);
            store_rlv(&loc, v, vars)?;
            Ok(v)
        }
        Expr::PostIncr(lv, inc) => {
            note_lv_unbound(lv, vars)?;
            let loc = resolve_lv(lv, vars, depth)?;
            let old = load_rlv(&loc, vars, depth)?;
            let step = if *inc { 1 } else { -1 };
            store_rlv(&loc, old.wrapping_add(step), vars)?;
            Ok(old)
        }
    }
}

/// The `set -u` check a read-modify-write owes for the variable it is about to
/// read, asked *before* the location is resolved.
///
/// The order is bash's and it is observable: `(( nada[nope] += 1 ))` reads the
/// whole `nada[nope]` through `expr_streval` and so reports the missing array
/// `nada`, while the plain `(( nada[nope] = 1 ))` beside it never reads and
/// reaches the subscript first, reporting `nope`.
fn note_lv_unbound(lv: &Lvalue, vars: &mut dyn VarLookup) -> Result<(), ArithError> {
    match lv {
        Lvalue::Var(n) => vars.note_arith_unbound(n, false),
        Lvalue::Index(n, _) | Lvalue::Assoc(n, _) => vars.note_arith_unbound(n, true),
        // Neither addresses an element; the complaint each earns is its own and
        // is made where it is read.
        Lvalue::EmptySub(_) | Lvalue::WholeSub(..) => Ok(()),
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
        Lvalue::WholeSub(n, s) => ResolvedLv::WholeSub(n.clone(), *s),
    })
}

fn load_rlv(loc: &ResolvedLv, vars: &mut dyn VarLookup, depth: u32) -> Result<i64, ArithError> {
    // The `set -u` question this read owes was asked by `note_lv_unbound`
    // before the location was resolved, because bash asks about the variable
    // before it evaluates the subscript.
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
        ResolvedLv::WholeSub(n, s) => {
            vars.refuse_whole_array_subscript(n, *s);
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
        // The same complaint as the read, and a second time: `(( a[@]++ ))`
        // reads and stores, and bash prints the line for each.
        ResolvedLv::WholeSub(n, s) => {
            vars.refuse_whole_array_subscript(n, *s);
            Ok(())
        }
    }
}

/// `base ** exp` the way bash computes it: binary exponentiation over the full
/// 64-bit exponent, every multiplication wrapping.
///
/// `exp` is a whole `intmax_t` in bash, not a narrow count — there is no
/// "exponent too large", because the squaring loop simply runs out of bits.
/// The result is the same as the mathematical power reduced modulo 2⁶⁴, so
/// `$(( 3**4294967296 ))` is a perfectly ordinary (if meaningless) number
/// rather than an error. `exp` is non-negative at every call site, so the
/// arithmetic right shift terminates.
fn ipow(base: i64, exp: i64) -> i64 {
    let (mut base, mut exp, mut result) = (base, exp, 1i64);
    while exp != 0 {
        if exp & 1 != 0 {
            result = result.wrapping_mul(base);
        }
        exp >>= 1;
        base = base.wrapping_mul(base);
    }
    result
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
            ipow(a, b)
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
        fn get_str(&mut self, name: &str) -> Option<Str> {
            self.0.get(name).map(|v| v.to_string().into_bytes())
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
        fn get_str(&mut self, name: &str) -> Option<Str> {
            self.scalars.get(name).map(|v| v.to_string().into_bytes())
        }
        fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
            self.scalars.insert(name.to_string(), value);
            Ok(())
        }
        fn get_index_str(&mut self, name: &str, index: i64) -> Option<Str> {
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
                .map(|v| v.to_string().into_bytes())
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
        assert_eq!(eval(b"a[0]", &mut m).unwrap(), 10);
        assert_eq!(eval(b"a[i]", &mut m).unwrap(), 30); // i = 2
        assert_eq!(eval(b"a[i+1] + 1", &mut m).unwrap(), 41); // a[3]=40, +1
        assert_eq!(eval(b"a[-1]", &mut m).unwrap(), 40); // negative from end
        assert_eq!(eval(b"a[10]", &mut m).unwrap(), 0); // out of range → 0
        // Missing ']' is a syntax error.
        assert!(eval(b"a[1", &mut m).is_err());
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
            let e = eval(src.as_bytes(), &mut m).expect_err(src);
            assert_eq!(e.expr_override.as_deref(), Some(b"1/0".as_slice()), "{src}");
            assert!(e.in_subscript, "{src}");
            assert_eq!(e.msg, "division by 0");
        }
        // A *parse* failure inside the subscript counts too, and the raw text is
        // kept verbatim — trailing blanks and all, which is what reproduces
        // bash's `1/0  : division by 0 (error token is "0  ")`.
        let e = eval(b"a[1+] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(b"1+".as_slice()));
        assert!(e.in_subscript);
        let e = eval(b"a[  1/0  ] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(b"  1/0  ".as_slice()));
        // The innermost subscript wins…
        let e = eval(b"a[a[1/0]] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(b"1/0".as_slice()));
        // …and a failure deeper still — a variable whose *value* is a bad
        // expression — keeps the value `str_to_val` recorded.
        m.scalars.insert("x".to_string(), 0);
        let e = eval(b"a[x/0] = 9", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(b"x/0".as_slice()));
        // An error *outside* any subscript is untouched: it blames the whole
        // expression (the shell's caller-supplied source) and is not fatal.
        let e = eval(b"a[0] = 1/0", &mut m).unwrap_err();
        assert_eq!(e.expr_override, None);
        assert!(!e.in_subscript);
    }

    #[test]
    fn indexed_assignment_and_incr() {
        let mut m = ArrMap {
            scalars: HashMap::new(),
            a: vec![10, 20, 30],
        };
        assert_eq!(eval(b"a[0] = 99", &mut m).unwrap(), 99);
        assert_eq!(m.a[0], 99);
        assert_eq!(eval(b"a[1] += 5", &mut m).unwrap(), 25);
        assert_eq!(m.a[1], 25);
        // Post-increment yields the old value, then mutates.
        assert_eq!(eval(b"a[2]++", &mut m).unwrap(), 30);
        assert_eq!(m.a[2], 31);
    }

    /// A lookup that refuses to be written through — the shape a readonly
    /// variable presents to the evaluator.
    #[derive(Default)]
    struct NoWrite(HashMap<String, i64>);
    impl VarLookup for NoWrite {
        fn get_str(&mut self, name: &str) -> Option<Str> {
            self.0.get(name).map(|v| v.to_string().into_bytes())
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
        let e = eval(b"ro = 5", &mut m).unwrap_err();
        assert_eq!(e.subject.as_deref(), Some("ro"));
        assert_eq!(e.msg, "readonly variable");
        assert_eq!(e.token, None);
        // Evaluation stops at the refusal: what came before it stands, what
        // comes after never happens.
        assert!(eval(b"a = 1, ro = 2, b = 3", &mut m).is_err());
        assert_eq!(m.0.get("a"), Some(&1));
        assert_eq!(m.0.get("b"), None);
        // Read-modify-write and the increments go through the same store.
        assert!(eval(b"ro += 1", &mut m).is_err());
        assert!(eval(b"ro++", &mut m).is_err());
        assert!(eval(b"++ro", &mut m).is_err());
        // An untaken branch never reaches the store, so it is no error at all.
        assert_eq!(eval(b"0 ? ro = 9 : 7", &mut m).unwrap(), 7);
    }

    /// A lookup with one associative array `m`, keyed — as bash keys one — by
    /// an arbitrary byte string rather than by text.
    #[derive(Default)]
    struct AssocMap(HashMap<Str, i64>);
    impl VarLookup for AssocMap {
        fn get_str(&mut self, _name: &str) -> Option<Str> {
            None
        }
        fn is_assoc(&self, name: &str) -> bool {
            name == "m"
        }
        fn get_assoc_str(&self, name: &str, key: BStr<'_>) -> Option<Str> {
            if name != "m" {
                return None;
            }
            self.0.get(key).map(|v| v.to_string().into_bytes())
        }
        fn set_assoc(&mut self, name: &str, key: BStr<'_>, value: i64) -> Result<(), ArithError> {
            if name == "m" {
                self.0.insert(key.to_vec(), value);
            }
            Ok(())
        }
    }

    #[test]
    fn associative_subscripts() {
        let mut kv = HashMap::new();
        kv.insert(b"foo".to_vec(), 7);
        kv.insert(b"bar".to_vec(), 13);
        let mut m = AssocMap(kv);
        // The subscript is a literal string key, not arithmetic.
        assert_eq!(eval(b"m[foo]", &mut m).unwrap(), 7);
        assert_eq!(eval(b"m[bar] + 1", &mut m).unwrap(), 14);
        // A key that looks like an operator expression is still literal.
        assert_eq!(eval(b"m[missing]", &mut m).unwrap(), 0); // unset → 0
        // Whitespace around the key is trimmed.
        assert_eq!(eval(b"m[ foo ]", &mut m).unwrap(), 7);
        // Assignment to an associative element.
        assert_eq!(eval(b"m[foo] = 100", &mut m).unwrap(), 100);
        assert_eq!(m.0.get(b"foo".as_slice()), Some(&100));
    }

    #[test]
    fn an_associative_subscript_may_hold_any_byte() {
        // A subscript of an associative array is a *literal key*, not
        // arithmetic — so it is not text, and nothing about it need be UTF-8.
        // osh used to gate the whole evaluator on the source decoding as UTF-8,
        // so `k=$'\xa9'; m[$k]=7; (( m[$k] ))` failed with "syntax error:
        // invalid arithmetic operator" — losing the read *and*, for
        // `(( m[$k] = 9 ))`, silently dropping the store. bash answers 7, then 9.
        let mut kv = HashMap::new();
        kv.insert(b"\xa9".to_vec(), 7);
        let mut m = AssocMap(kv);
        assert_eq!(eval(b"m[\xa9]", &mut m).unwrap(), 7);
        assert_eq!(eval(b"m[\xa9] = 9", &mut m).unwrap(), 9);
        assert_eq!(m.0.get(b"\xa9".as_slice()), Some(&9));
        // A key that is only *partly* undecodable is still exactly its bytes.
        assert_eq!(eval(b"m[ a\xffb ] = 4", &mut m).unwrap(), 4);
        assert_eq!(m.0.get(b"a\xffb".as_slice()), Some(&4));
    }

    /// A string-backed scalar lookup, so recursive value evaluation (a value
    /// that is itself a variable name or an expression) can be exercised.
    #[derive(Default)]
    struct StrMap(HashMap<String, Str>);
    impl VarLookup for StrMap {
        fn get_str(&mut self, name: &str) -> Option<Str> {
            self.0.get(name).cloned()
        }
        fn set(&mut self, name: &str, value: i64) -> Result<(), ArithError> {
            self.0.insert(name.to_string(), value.to_string().into_bytes());
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
        assert_eq!(eval(b"b", &mut m).unwrap(), 5);
        assert_eq!(eval(b"c", &mut m).unwrap(), 5);
        assert_eq!(eval(b"expr", &mut m).unwrap(), 5);
        assert_eq!(eval(b"expr * 2", &mut m).unwrap(), 10);
        assert_eq!(eval(b"mixed", &mut m).unwrap(), 10);
        // A value naming an unset variable evaluates to 0.
        m.0.insert("u".into(), "missing".into());
        assert_eq!(eval(b"u + 1", &mut m).unwrap(), 1);
        // A leading-zero value keeps octal semantics through the recursion.
        m.0.insert("oct".into(), "010".into());
        assert_eq!(eval(b"oct", &mut m).unwrap(), 8);
    }

    #[test]
    fn a_values_trailing_whitespace_survives_into_the_diagnostic() {
        // bash lexes a variable's value exactly as stored. Its lexer skips
        // leading whitespace, and `evalerror` skips it again when echoing the
        // value — but trailing whitespace is simply part of the string, so an
        // error token running to the end of the value carries it. Trimming the
        // tail (as trimming the head would suggest) loses a space bash prints:
        // `x='1 + '; $(( x ))` reports `1 + : … (error token is "+ ")`.
        let mut m = StrMap::default();
        m.0.insert("x".into(), "  1 +  ".into());
        let e = eval(b"x", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(&b"1 +  "[..]));
        assert_eq!(e.body(), b"syntax error: operand expected (error token is \"+  \")");
        // Same for a failure raised during evaluation rather than parsing.
        m.0.insert("d".into(), " 1/0 ".into());
        let e = eval(b"d", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(&b"1/0 "[..]));
        assert_eq!(e.body(), b"division by 0 (error token is \"0 \")");
        // …and for the recursion guard, whose token is the value itself.
        m.0.insert("r".into(), " r ".into());
        let e = eval(b"r", &mut m).unwrap_err();
        assert_eq!(e.expr_override.as_deref(), Some(&b"r "[..]));
        assert_eq!(e.token.as_deref(), Some(&b"r "[..]));
        // The whitespace is cosmetic only: a value that evaluates still does,
        // including through the octal and plain-decimal fast paths.
        m.0.insert("w".into(), " 010 ".into());
        assert_eq!(eval(b"w", &mut m).unwrap(), 8);
        m.0.insert("p".into(), " 5 ".into());
        assert_eq!(eval(b"p", &mut m).unwrap(), 5);
        m.0.insert("b".into(), "   ".into());
        assert_eq!(eval(b"b", &mut m).unwrap(), 0);
    }

    #[test]
    fn recursive_variable_cycle_is_bounded() {
        let mut m = StrMap::default();
        m.0.insert("x".into(), "x".into()); // self-reference
        let e = eval(b"x", &mut m).unwrap_err();
        assert!(e.msg.contains("recursion level exceeded"), "{}", e.msg);
        // Mutual cycle a -> b -> a.
        let mut m2 = StrMap::default();
        m2.0.insert("a".into(), "b".into());
        m2.0.insert("b".into(), "a".into());
        assert!(eval(b"a", &mut m2).is_err());
    }

    fn ev(s: &str) -> i64 {
        eval(s.as_bytes(), &mut Map::default()).unwrap()
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
        assert!(eval(b"099", &mut Map::default()).is_err());
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
            let e = eval(bad.as_bytes(), &mut Map::default()).unwrap_err();
            assert_eq!(e.msg, "syntax error: operand expected", "{bad}");
        }
        // …and one after a complete operand is an unexpected *operator*, which is
        // how bash words `1"2"3` too.
        let e = eval(br#"1"2"3"#, &mut Map::default()).unwrap_err();
        assert_eq!(e.msg, "syntax error: invalid arithmetic operator");
        assert_eq!(e.token.as_deref(), Some(br#""2"3"#.as_slice()));
        // The error token starts at the quote, so the diagnostic shows it.
        let e = eval(br#"y="3"+4"#, &mut Map::default()).unwrap_err();
        assert_eq!(e.token.as_deref(), Some(br#""3"+4"#.as_slice()));
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
        assert!(eval(b"2 ** -1", &mut Map::default()).is_err());
    }

    #[test]
    fn an_exponent_is_never_too_large() {
        // bash's `ipow` squares its way through the *whole* 64-bit exponent, so
        // an exponent past 2³² is arithmetic, not an error: the result is the
        // mathematical power reduced modulo 2⁶⁴. (Narrowing the exponent to a
        // `u32` first, as `i64::wrapping_pow` requires, would have to invent a
        // refusal bash does not have.) Values checked against bash 5.2.37.
        assert_eq!(ev("2**4294967296"), 0);
        assert_eq!(ev("3**4294967296"), 2_491_309_678_558_969_857);
        assert_eq!(ev("7**4294967297"), -5_799_782_102_597_631_993);
        assert_eq!(ev("1**4294967296"), 1);
        assert_eq!(ev("0**4294967296"), 0);
        assert_eq!(ev("(-1)**4294967297"), -1);
        assert_eq!(ev("2**9223372036854775807"), 0);
        // …and the ordinary cases keep the sign and wrapping they always had.
        assert_eq!(ev("2**63"), i64::MIN);
        assert_eq!(ev("2**64"), 0);
        assert_eq!(ev("0**0"), 1);
        assert_eq!(ev("(-3)**3"), -27);
        assert_eq!(ev("(-3)**4"), 81);
        assert_eq!(ev("10**19"), -8_446_744_073_709_551_616);
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
        assert!(eval(b"2#12", &mut Map::default()).is_err()); // '2' not valid in base 2
        assert!(eval(b"1#0", &mut Map::default()).is_err()); // base < 2
        assert!(eval(b"65#0", &mut Map::default()).is_err()); // base > 64
        assert!(eval(b"099", &mut Map::default()).is_err()); // bad octal digit
    }

    #[test]
    fn variables() {
        let mut m = HashMap::new();
        m.insert("x".to_string(), 10);
        m.insert("y".to_string(), 4);
        assert_eq!(eval(b"x * y + 2", &mut Map(m)).unwrap(), 42);
    }

    #[test]
    fn assignment_scalars() {
        let mut m = Map::default();
        assert_eq!(eval(b"x = 5", &mut m).unwrap(), 5);
        assert_eq!(m.get("x"), Some(5));
        // Compound assignment.
        assert_eq!(eval(b"x += 3", &mut m).unwrap(), 8);
        assert_eq!(eval(b"x *= 2", &mut m).unwrap(), 16);
        assert_eq!(eval(b"x -= 1", &mut m).unwrap(), 15);
        assert_eq!(eval(b"x /= 5", &mut m).unwrap(), 3);
        assert_eq!(m.get("x"), Some(3));
        // Right-associative chained assignment: y = z = 7.
        assert_eq!(eval(b"y = z = 7", &mut m).unwrap(), 7);
        assert_eq!(m.get("y"), Some(7));
        assert_eq!(m.get("z"), Some(7));
        // Assigning to a literal is an error.
        assert!(eval(b"3 = 4", &mut Map::default()).is_err());
    }

    #[test]
    fn increment_decrement() {
        let mut m = Map::default();
        m.put("x", 5);
        // Pre-increment yields the new value.
        assert_eq!(eval(b"++x", &mut m).unwrap(), 6);
        assert_eq!(m.get("x"), Some(6));
        // Post-increment yields the old value.
        assert_eq!(eval(b"x++", &mut m).unwrap(), 6);
        assert_eq!(m.get("x"), Some(7));
        // Pre/post decrement.
        assert_eq!(eval(b"--x", &mut m).unwrap(), 6);
        assert_eq!(eval(b"x--", &mut m).unwrap(), 6);
        assert_eq!(m.get("x"), Some(5));
        // Increment on an unset variable starts from 0.
        assert_eq!(eval(b"++fresh", &mut m).unwrap(), 1);
    }

    /// `++` and `--` are increment operators only where an increment is
    /// possible; everywhere else the two characters are simply two operators in
    /// a row. Every expectation here is bash 5.2.37's own answer.
    #[test]
    fn increment_operators_need_an_lvalue() {
        let mut m = Map::default();
        m.put("v", 5);
        // Nothing assignable follows, so `--2` is `-(-2)` and `++2` is `+(+2)`.
        assert_eq!(eval(b"--2", &mut m).unwrap(), 2);
        assert_eq!(eval(b"++2", &mut m).unwrap(), 2);
        assert_eq!(eval(b"--(3)", &mut m).unwrap(), 3);
        assert_eq!(eval(b"++3+1", &mut m).unwrap(), 4);
        // Nor is the operand on the *left* assignable, so these are a binary
        // operator followed by a unary one: `2 - (-3)`, `3 - (-(-2))`.
        assert_eq!(eval(b"2--3", &mut m).unwrap(), 5);
        assert_eq!(eval(b"3---2", &mut m).unwrap(), 1);
        // A name may still follow across whitespace, and a real decrement wins
        // over the reading above wherever one is possible.
        assert_eq!(eval(b"-- v", &mut m).unwrap(), 4);
        assert_eq!(eval(b"v---3", &mut m).unwrap(), 1); // (v--) - 3, v now 4
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
        eval(b"0 && (y = 9)", &mut m).unwrap();
        assert_eq!(m.get("y"), None);
        eval(b"1 || (z = 9)", &mut m).unwrap();
        assert_eq!(m.get("z"), None);
        // The taken branch of a ternary runs; the other doesn't.
        eval(b"1 ? (a = 1) : (b = 2)", &mut m).unwrap();
        assert_eq!(m.get("a"), Some(1));
        assert_eq!(m.get("b"), None);
    }

    #[test]
    fn div_zero() {
        assert!(eval(b"1 / 0", &mut Map::default()).is_err());
    }

    #[test]
    fn zero_division_messages_match_bash() {
        // bash reports both `/` and `%` by zero with the exact text "division by 0"
        // (not "division by zero"/"modulo by zero"), and exponent-by-negative with
        // "exponent less than 0". Keep the wording verbatim for bash-superset parity.
        let div = eval(b"1 / 0", &mut Map::default()).unwrap_err();
        assert_eq!(div.msg, "division by 0");
        assert_eq!(div.body(), b"division by 0 (error token is \"0\")");
        let modulo = eval(b"1 % 0", &mut Map::default()).unwrap_err();
        assert_eq!(modulo.msg, "division by 0");
        let exp = eval(b"5 ** -1", &mut Map::default()).unwrap_err();
        assert_eq!(exp.msg, "exponent less than 0");
    }

    #[test]
    fn error_bodies_and_tokens_match_bash() {
        // The full body (message + `(error token is "…")`) reproduces bash's
        // arithmetic diagnostic body byte-for-byte across the common cases. The
        // enclosing shell prepends the `<name>: line N: <expr>:` prefix.
        let cases: &[(&str, &str)] = &[
            ("1/0", "division by 0 (error token is \"0\")"),
            ("1%0", "division by 0 (error token is \"0\")"),
            ("1/(0)", "division by 0 (error token is \"(0)\")"),
            ("1/0/0", "division by 0 (error token is \"0/0\")"),
            ("1/0+5", "division by 0 (error token is \"0+5\")"),
            // `**` is the one eval-time failure bash does *not* report against
            // its right operand: `exp_power` saves no cursor, so the token is
            // whichever one the lexer is holding — and it reads one ahead, so
            // that is the token *following* the exponent.
            ("2**-1+9", "exponent less than 0 (error token is \"+9\")"),
            ("2**-1*8", "exponent less than 0 (error token is \"*8\")"),
            ("1+2**-3+4", "exponent less than 0 (error token is \"+4\")"),
            ("2**-1==0", "exponent less than 0 (error token is \"==0\")"),
            ("1?2**-1:0", "exponent less than 0 (error token is \":0\")"),
            // …and at end of input there is no next token, so the lexer keeps
            // holding the exponent's own last one.
            ("2**-1", "exponent less than 0 (error token is \"1\")"),
            ("5 ** -1 ", "exponent less than 0 (error token is \"1 \")"),
            ("2**(-1)", "exponent less than 0 (error token is \")\")"),
            ("2**-(1)", "exponent less than 0 (error token is \")\")"),
            // Right-associative, so the inner `**` fails first and names its own.
            ("2**2**-1", "exponent less than 0 (error token is \"1\")"),
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
            let e = eval(src.as_bytes(), &mut Map::default()).unwrap_err();
            assert_eq!(e.body().as_slice(), want.as_bytes(), "expr {src:?}");
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
            let e = eval(src.as_bytes(), &mut Map::default()).unwrap_err();
            assert_eq!(e.body().as_slice(), want.as_bytes(), "expr {src:?}");
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
            let e = eval(src.as_bytes(), &mut Map::default()).unwrap_err();
            assert!(e.truncate_leading, "expr {src:?} should truncate leading");
        }
        for src in ["1/0", "5 +", "3 3"] {
            let e = eval(src.as_bytes(), &mut Map::default()).unwrap_err();
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
        assert_eq!(eval(b"1 ? a=1, b=2, a+b : 0", &mut m).unwrap(), 3);
        assert_eq!(m.get("a"), Some(1));
        assert_eq!(m.get("b"), Some(2));
        // The else branch recurses at ternary level, so a trailing comma binds
        // to the enclosing expression: `1 ? 2 : 4,5` == `(1?2:4),5` == 5.
        assert_eq!(ev("1 ? 2 : 4,5"), 5);
        // Missing ':' is a syntax error.
        assert!(eval(b"1 ? 2", &mut Map::default()).is_err());
    }

    #[test]
    fn comma() {
        assert_eq!(ev("1, 2, 3"), 3);
        assert_eq!(ev("(1 + 1, 2 * 3)"), 6);
        // Comma binds looser than ternary.
        assert_eq!(ev("1 ? 5 : 9, 7"), 7);
        // Comma sequences assignments (the C-style for-loop update idiom).
        let mut m = Map::default();
        assert_eq!(eval(b"i = 0, j = 10", &mut m).unwrap(), 10);
        assert_eq!(m.get("i"), Some(0));
        assert_eq!(m.get("j"), Some(10));
    }

    #[test]
    fn variables_in_ternary() {
        let mut m = HashMap::new();
        m.insert("x".to_string(), 5);
        m.insert("y".to_string(), 0);
        let mut vars = Map(m);
        assert_eq!(eval(b"x ? x * 2 : -1", &mut vars).unwrap(), 10);
        assert_eq!(eval(b"y ? 99 : x + 1", &mut vars).unwrap(), 6);
    }
}
