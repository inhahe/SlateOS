//! The shape of a parsed awk program.
//!
//! Names are resolved to **slots** here, not at run time: a global becomes an
//! index into one vector and a function parameter becomes an index into the
//! call frame. A tree-walking interpreter that looks names up in a hash map on
//! every reference spends most of its time hashing `NR`, and awk references
//! variables once per field per line.
//!
//! The one thing deliberately *not* resolved is which names are arrays. awk
//! decides that by use — `x[1]=1` makes `x` an array and `x=1` makes it a
//! scalar, and a function parameter is whichever its caller passed — so it is a
//! property of the value, checked when the value is touched.

use crate::value::Str;
use ere::Regex;
use std::rc::Rc;

/// Where a variable lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarRef {
    /// A slot in the global table. The first [`SPECIAL_COUNT`] are the built-in
    /// variables, at fixed indices, so `NF` can be recognised on assignment.
    Global(usize),
    /// A slot in the current call frame — a function parameter, or one of the
    /// extra parameters awk programs use as local variables.
    Local(usize),
}

/// The built-in variables, in slot order. Their indices are fixed because the
/// interpreter has to *act* on assignment to some of them: writing `NF`
/// rebuilds the record, writing `FS` recompiles the field splitter.
pub const SPECIALS: &[&str] = &[
    "NR", "NF", "FNR", "FS", "OFS", "ORS", "RS", "FILENAME", "SUBSEP", "RSTART", "RLENGTH",
    "CONVFMT", "OFMT", "ENVIRON", "ARGC", "ARGV",
];

pub const V_NR: usize = 0;
pub const V_NF: usize = 1;
pub const V_FNR: usize = 2;
pub const V_FS: usize = 3;
pub const V_OFS: usize = 4;
pub const V_ORS: usize = 5;
pub const V_RS: usize = 6;
pub const V_FILENAME: usize = 7;
pub const V_SUBSEP: usize = 8;
pub const V_RSTART: usize = 9;
pub const V_RLENGTH: usize = 10;
pub const V_CONVFMT: usize = 11;
pub const V_OFMT: usize = 12;
pub const V_ENVIRON: usize = 13;
pub const V_ARGC: usize = 14;
pub const V_ARGV: usize = 15;
pub const SPECIAL_COUNT: usize = 16;

/// The slot constants above are indices into [`SPECIALS`], so a name added to
/// one and not the other would silently shift every constant after it — `FS`
/// would become `OFS` and the interpreter would recompile the field splitter on
/// assignment to the wrong variable. This catches that at compile time.
const _: () = assert!(SPECIALS.len() == SPECIAL_COUNT);

/// Something that can be assigned to.
#[derive(Clone, Debug)]
pub enum Lvalue {
    Var(VarRef),
    /// `$expr`.
    Field(Box<Expr>),
    /// `name[i, j]` — the subscripts are joined with SUBSEP into one key.
    Index(VarRef, Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Length,
    Substr,
    Index,
    Split,
    Sub,
    Gsub,
    Match,
    Sprintf,
    Sin,
    Cos,
    Atan2,
    Exp,
    Log,
    Sqrt,
    Int,
    Rand,
    Srand,
    Tolower,
    Toupper,
    System,
    Close,
    Fflush,
}

/// Where `getline` reads from.
#[derive(Clone, Debug)]
pub enum GetlineSrc {
    /// Plain `getline` — the next record of the main input, so it advances NR
    /// and FNR and can move to the next file.
    Main,
    /// `getline < expr`.
    File(Expr),
    /// `expr | getline`.
    Cmd(Expr),
}

#[derive(Clone, Debug)]
pub struct Getline {
    pub into: Option<Lvalue>,
    pub src: GetlineSrc,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Str(Rc<Str>),
    /// A `/re/` literal. In a value context this means `$0 ~ /re/`; as an
    /// argument to `sub`, `match` or `~` it is the pattern itself.
    Regex(Rc<Regex>),
    Get(Lvalue),
    Assign(Lvalue, Box<Expr>),
    AugAssign(Lvalue, BinOp, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    /// `(i, j) in array`.
    In(Vec<Expr>, VarRef),
    Match {
        neg: bool,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    Concat(Box<Expr>, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    /// Unary `+`: not a no-op, it forces a numeric reading.
    Pos(Box<Expr>),
    Not(Box<Expr>),
    /// `++x` / `--x`; the `f64` is the delta.
    PreIncr(Lvalue, f64),
    /// `x++` / `x--`.
    PostIncr(Lvalue, f64),
    Call(usize, Vec<Expr>),
    Builtin(Builtin, Vec<Expr>),
    Getline(Box<Getline>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirMode {
    Truncate,
    Append,
    Pipe,
}

#[derive(Clone, Debug)]
pub struct Redirect {
    pub mode: RedirMode,
    pub target: Expr,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Expr(Expr),
    Print(Vec<Expr>, Option<Redirect>),
    Printf(Vec<Expr>, Option<Redirect>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Box<Stmt>>,
        body: Box<Stmt>,
    },
    ForIn {
        var: Lvalue,
        array: VarRef,
        body: Box<Stmt>,
    },
    Next,
    NextFile,
    Exit(Option<Expr>),
    Return(Option<Expr>),
    Break,
    Continue,
    /// `delete a[i]`, or `delete a` when the subscripts are empty.
    Delete(VarRef, Vec<Expr>),
    Nop,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Always,
    Expr(Expr),
    /// `expr, expr` — a range, which is stateful: once the first matches the
    /// rule stays on until the second does. `range` indexes the interpreter's
    /// table of which ranges are currently open.
    Range(Expr, Expr, usize),
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub pattern: Pattern,
    /// `None` means the default action, `{ print }`.
    pub action: Option<Vec<Stmt>>,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    /// The parameters the definition declared, in slot order. A call may pass
    /// fewer; the rest are uninitialised locals, which is how awk programs
    /// declare local variables at all.
    ///
    /// The *names* are kept, though the interpreter only ever needs the count,
    /// because `types::resolve` reports its diagnostics against them: "parameter
    /// 2 of f" would send the reader counting commas.
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Default)]
pub struct Program {
    pub begin: Vec<Stmt>,
    pub end: Vec<Stmt>,
    pub rules: Vec<Rule>,
    pub funcs: Vec<Func>,
    /// Total global slots, including the specials.
    pub globals: usize,
    pub global_names: Vec<String>,
    /// How many `expr, expr` range patterns there are, so the interpreter can
    /// size its open-range table.
    pub ranges: usize,
    /// Which global slots hold arrays, decided by `types::resolve` before the
    /// program runs. See that module for why it cannot wait until run time.
    pub global_is_array: Vec<bool>,
    /// The same, per function parameter.
    pub param_is_array: Vec<Vec<bool>>,
}
