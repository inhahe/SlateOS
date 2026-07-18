//! Abstract syntax tree for the OSH shell language.
//!
//! The grammar modelled here is the common POSIX-sh / bash core that the
//! parser currently accepts. It intentionally starts small and grows toward
//! the full bash-superset (arrays, `[[ ]]`, `(( ))`, here-docs) — see the
//! crate-level docs and `design-decisions.md §72`.

/// A whole program: a list of and-or lists separated by `;`, `&`, or newlines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    pub items: Vec<Item>,
}

/// One top-level item: an and-or list plus how it was terminated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub list: AndOr,
    /// `true` when the item ended with `&` (run asynchronously).
    pub background: bool,
}

/// A pipeline joined to further pipelines by `&&` / `||`, evaluated
/// left-to-right with short-circuiting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndOr {
    pub first: Pipeline,
    /// Each `(op, pipeline)` continues the chain; `op` gates on the running
    /// exit status.
    pub rest: Vec<(AndOrOp, Pipeline)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndOrOp {
    /// `&&` — run the next pipeline only if the previous succeeded (exit 0).
    And,
    /// `||` — run the next pipeline only if the previous failed (exit != 0).
    Or,
}

/// A sequence of commands connected by `|`; the whole pipeline may be negated
/// with a leading `!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    pub negated: bool,
    pub commands: Vec<Command>,
}

/// A single command node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A simple command: assignments, a possibly-empty argv, and redirections.
    Simple(SimpleCommand),
    /// `if cond; then body; [elif …] [else …] fi`.
    If(IfClause),
    /// `while cond; do body; done` (or `until`).
    Loop(LoopClause),
    /// `for name in words; do body; done`.
    For(ForClause),
    /// `name() { body; }` — a function definition.
    Function(FunctionDef),
    /// `{ list; }` — a brace group (runs in the current shell).
    BraceGroup(Program),
    /// `( list )` — a subshell group.
    Subshell(Program),
}

/// A simple command with variable assignments, words, and redirections.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleCommand {
    /// Leading `NAME=value` assignments (before the command word).
    pub assignments: Vec<Assignment>,
    /// The command word and its arguments, each an unexpanded word.
    pub words: Vec<Word>,
    /// Redirections attached to this command.
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub name: String,
    pub value: Word,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfClause {
    pub cond: Program,
    pub body: Program,
    /// `elif` branches, each `(condition, body)`.
    pub elifs: Vec<(Program, Program)>,
    pub else_body: Option<Program>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopClause {
    /// `true` for `until` (loop while the condition is non-zero).
    pub until: bool,
    pub cond: Program,
    pub body: Program,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForClause {
    pub var: String,
    /// The `in …` word list; `None` means iterate over `"$@"`.
    pub words: Option<Vec<Word>>,
    pub body: Program,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    pub name: String,
    pub body: Program,
}

/// A word: a sequence of parts that concatenate after expansion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

impl Word {
    /// Construct a word from a single literal string (used by tests/helpers).
    #[must_use]
    pub fn literal(s: impl Into<String>) -> Self {
        Word {
            parts: vec![WordPart::Literal(s.into())],
        }
    }
}

/// A fragment of a word. Quoting is captured per-part so field splitting and
/// glob expansion can respect it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordPart {
    /// Unquoted literal text (subject to later splitting/globbing).
    Literal(String),
    /// Single-quoted text (no expansion, no splitting).
    SingleQuoted(String),
    /// Double-quoted run of parts (expansion, but no splitting/globbing).
    DoubleQuoted(Vec<WordPart>),
    /// `$name` / `${name}` parameter reference.
    Param(String),
    /// `${name:-word}`-style parameter expansion with an operator.
    ParamOp {
        name: String,
        op: ParamOp,
        arg: Box<Word>,
    },
    /// `$(command)` / `` `command` `` command substitution.
    CommandSub(Program),
    /// `$(( expr ))` arithmetic substitution (raw expression text for now).
    ArithSub(String),
    /// `${#name}` — the length of the parameter's value.
    Length(String),
}

/// Parameter-expansion operators inside `${name OP word}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamOp {
    /// `:-` use default if unset or null.
    UseDefault,
    /// `:=` assign default if unset or null.
    AssignDefault,
    /// `:+` use alternate if set and non-null.
    UseAlternate,
    /// `:?` error if unset or null.
    ErrorIfUnset,
}

/// A single redirection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// The fd being redirected (defaults filled in by the parser).
    pub fd: i32,
    pub op: RedirectOp,
    pub target: Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectOp {
    /// `> file` — truncate/create.
    Write,
    /// `>> file` — append.
    Append,
    /// `< file` — read.
    Read,
    /// `n>&m` — duplicate an fd (target parsed as the target fd number).
    DupOut,
}
