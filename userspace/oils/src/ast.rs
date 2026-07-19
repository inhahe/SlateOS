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
    /// The `time` reserved word prefixed the pipeline: report elapsed timing on
    /// stderr after it completes.
    pub timed: bool,
    /// `time -p` was used: POSIX-format output (three lines, seconds with two
    /// decimals) instead of bash's default `real\tNmM.SSSs` form.
    pub time_posix: bool,
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
    /// `for (( init; cond; update )); do body; done` — C-style arithmetic for
    /// loop. Each section holds the raw arithmetic text (empty = omitted).
    ForArith(ForArithClause),
    /// `select name [in words]; do body; done` — interactive menu loop.
    Select(SelectClause),
    /// `name() { body; }` — a function definition.
    Function(FunctionDef),
    /// `case word in pat) body ;; … esac`.
    Case(CaseClause),
    /// `{ list; }` — a brace group (runs in the current shell).
    BraceGroup(Program),
    /// `( list )` — a subshell group.
    Subshell(Program),
    /// `[[ expr ]]` — bash conditional expression (exit 0 if true, 1 if false).
    Cond(CondExpr),
    /// `(( expr ))` — bash arithmetic command (exit 0 if the result is
    /// non-zero, 1 if zero). The `String` holds the raw arithmetic text.
    Arith(String),
    /// A compound command with trailing redirections, e.g.
    /// `while read l; do …; done < file` or `{ …; } > out`. Simple commands
    /// carry their own redirects; this wraps the non-simple forms.
    Redirected {
        inner: Box<Command>,
        redirects: Vec<Redirect>,
    },
}

/// A `[[ … ]]` conditional expression tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondExpr {
    /// A single word — true if it expands to a non-empty string.
    Word(Word),
    /// Unary file/string test: `-e -f -d -r -w -x -s` (file), `-z -n` (string).
    Unary(UnaryOp, Word),
    /// Binary comparison between two words.
    Binary(Box<Word>, CondBinOp, Box<Word>),
    /// `lhs =~ rhs` — POSIX-ERE regex match. The RHS undergoes parameter
    /// expansion; on a successful match the interpreter populates the
    /// `BASH_REMATCH` array with the whole match and capture groups.
    Regex(Box<Word>, Box<Word>),
    /// `! expr` — logical negation.
    Not(Box<CondExpr>),
    /// `expr && expr` — logical AND (short-circuiting).
    And(Box<CondExpr>, Box<CondExpr>),
    /// `expr || expr` — logical OR (short-circuiting).
    Or(Box<CondExpr>, Box<CondExpr>),
}

/// Unary test operators inside `[[ … ]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-e` — path exists.
    Exists,
    /// `-f` — exists and is a regular file.
    File,
    /// `-d` — exists and is a directory.
    Dir,
    /// `-r` — readable.
    Readable,
    /// `-w` — writable.
    Writable,
    /// `-x` — executable.
    Executable,
    /// `-s` — exists and has non-zero size.
    NonEmptyFile,
    /// `-z` — string has zero length.
    ZeroLen,
    /// `-n` — string has non-zero length.
    NonZeroLen,
    /// `-v` — the named shell variable (or array element) is set.
    VarSet,
    /// `-o` — the named shell option is enabled.
    OptionSet,
    /// `-L`/`-h` — path exists and is a symbolic link.
    Symlink,
    /// `-t` — the file descriptor (0/1/2) is open and refers to a terminal.
    Terminal,
}

/// Binary comparison operators inside `[[ … ]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondBinOp {
    /// `==` / `=` — glob-pattern match (RHS is a pattern unless quoted).
    StrEq,
    /// `!=` — negated glob-pattern match.
    StrNe,
    /// `<` — left string sorts before right (byte order).
    StrLt,
    /// `>` — left string sorts after right (byte order).
    StrGt,
    /// `-eq` — numeric equality.
    NumEq,
    /// `-ne` — numeric inequality.
    NumNe,
    /// `-lt` — numeric less-than.
    NumLt,
    /// `-le` — numeric less-than-or-equal.
    NumLe,
    /// `-gt` — numeric greater-than.
    NumGt,
    /// `-ge` — numeric greater-than-or-equal.
    NumGe,
    /// `-nt` — left file is newer than right (by mtime), or left exists and
    /// right does not.
    FileNewer,
    /// `-ot` — left file is older than right (by mtime), or right exists and
    /// left does not.
    FileOlder,
    /// `-ef` — left and right refer to the same file (same canonical path).
    SameFile,
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
    /// Array-literal operands appearing *after* a declaration command word,
    /// e.g. the `m=([k]=v)` in `declare -A m=([k]=v)`. Only populated when the
    /// command word is a declaration builtin (`declare`/`typeset`/`local`);
    /// the interpreter applies these with the declared array kind.
    pub decl_arrays: Vec<Assignment>,
}

/// A variable assignment: `name=value`, `name+=value`, `name[i]=value`, or an
/// array assignment `name=(w1 w2 …)` / `name+=(…)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub name: String,
    /// `name[index]=…` — the (arithmetic) subscript, if present. Only valid for
    /// scalar right-hand sides.
    pub index: Option<Word>,
    /// `+=` (append) rather than `=` (replace).
    pub append: bool,
    pub value: AssignRhs,
}

/// The right-hand side of an [`Assignment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignRhs {
    /// `name=word` — a scalar value (no field splitting or globbing).
    Scalar(Word),
    /// `name=(w1 w2 …)` — an array literal; each element is a positional value
    /// (split/globbed like a command argument) or a keyed `[sub]=value` pair.
    Array(Vec<ArrayElem>),
}

/// One element of an array literal `(…)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrayElem {
    /// A bare value word — assigned to the next index (indexed arrays) or an
    /// error for associative arrays (bash requires keys there).
    Positional(Word),
    /// `[sub]=value` — an explicit subscript. For an indexed array `sub` is an
    /// arithmetic index; for an associative array it is a string key.
    Keyed { index: Word, value: Word },
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

/// `select var [in words]; do body; done` — bash's interactive menu loop.
/// Prints the numbered word list to stderr, reads a selection line from stdin
/// (with the `PS3` prompt), sets `var` to the chosen word (empty on bad input),
/// stores the raw line in `REPLY`, and runs the body until EOF or `break`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectClause {
    pub var: String,
    /// The `in …` word list; `None` means iterate over `"$@"`.
    pub words: Option<Vec<Word>>,
    pub body: Program,
}

/// `for (( init; cond; update ))` — the C-style arithmetic for loop. Each
/// section is the raw arithmetic-expression text; an empty string means the
/// section was omitted (an omitted condition is treated as always-true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForArithClause {
    pub init: String,
    pub cond: String,
    pub update: String,
    pub body: Program,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    pub name: String,
    pub body: Program,
}

/// `case WORD in … esac` — match `word` against each item's patterns in order,
/// running the body of the first matching item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseClause {
    pub word: Word,
    pub items: Vec<CaseItem>,
}

/// How a `case` arm terminates, controlling control flow after its body runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseTerm {
    /// `;;` — stop after this arm (the normal case).
    Break,
    /// `;&` — fall through and run the *next* arm's body unconditionally.
    FallThrough,
    /// `;;&` — resume pattern testing with the following arms.
    ContinueMatch,
}

/// One `pat[|pat…]) body ;;` arm of a `case` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseItem {
    /// Alternative glob patterns (`|`-separated); a match on any runs the body.
    pub patterns: Vec<Word>,
    pub body: Program,
    /// Terminator determining control flow after the body (bash `;;`/`;&`/`;;&`).
    pub term: CaseTerm,
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
        /// Optional array subscript: `${a[i]:-word}` operates on element `i`.
        /// `None` for a plain scalar/`${name:-word}`.
        index: Option<Box<Word>>,
        op: ParamOp,
        arg: Box<Word>,
    },
    /// `${name#pat}` / `${name##pat}` / `${name%pat}` / `${name%%pat}` — remove
    /// a matching prefix (`#`) or suffix (`%`); doubled operator = longest match.
    ParamTrim {
        name: String,
        /// Optional array subscript (`${a[i]#pat}`).
        index: Option<Box<Word>>,
        /// `true` for `%`/`%%` (suffix); `false` for `#`/`##` (prefix).
        suffix: bool,
        /// `true` for the doubled form (longest match).
        longest: bool,
        pattern: Box<Word>,
    },
    /// `${name:offset}` / `${name:offset:length}` — substring (offset/length are
    /// arithmetic; a negative offset counts from the end).
    ParamSubstr {
        name: String,
        /// Optional array subscript (`${a[i]:off:len}`).
        index: Option<Box<Word>>,
        offset: Box<Word>,
        length: Option<Box<Word>>,
    },
    /// `${name/pat/repl}` (first) / `${name//pat/repl}` (all) /
    /// `${name/#pat/repl}` (anchored at start) / `${name/%pat/repl}` (anchored at
    /// end) — pattern substitution.
    ParamReplace {
        name: String,
        /// Optional array subscript (`${a[i]/pat/repl}`).
        index: Option<Box<Word>>,
        all: bool,
        anchor: ReplaceAnchor,
        pattern: Box<Word>,
        replacement: Box<Word>,
    },
    /// `${name^pat}` / `${name^^pat}` (upper-case) / `${name,pat}` /
    /// `${name,,pat}` (lower-case) — case modification. `all` is the doubled
    /// operator (convert every character whose value matches `pattern`);
    /// otherwise only the first character is considered. `pattern` selects
    /// which characters convert (a glob matched against one character at a
    /// time); an empty pattern matches any character.
    ParamCase {
        name: String,
        /// Optional array subscript (`${a[i]^^}`).
        index: Option<Box<Word>>,
        /// `true` for `^`/`^^` (upper); `false` for `,`/`,,` (lower).
        upper: bool,
        /// `true` for the doubled form (every matching character).
        all: bool,
        pattern: Box<Word>,
    },
    /// `${!name}` — indirect expansion: the value of the variable whose *name*
    /// is the value of `name` (e.g. `ref=x; x=hi; ${!ref}` → `hi`). The stored
    /// string is the referring variable's name; the target may itself carry an
    /// array subscript (`ref=a[0]`).
    Indirect(String),
    /// `${!prefix*}` / `${!prefix@}` — the names of all set variables that begin
    /// with `prefix`. Unquoted, both field-split; the `*`/`@` distinction only
    /// matters inside double quotes (`*` joins with the first IFS char, `@`
    /// yields one field per name).
    VarNames {
        prefix: String,
        /// `true` for the `*` form, `false` for the `@` form.
        star: bool,
    },
    /// `$(command)` / `` `command` `` command substitution.
    CommandSub(Program),
    /// `$(( expr ))` arithmetic substitution (raw expression text for now).
    ArithSub(String),
    /// `${#name}` — the length of the parameter's value.
    Length(String),
    /// `${name[index]}`, `${name[@]}`, `${name[*]}`, and their `${#…}` length
    /// forms — indexed-array references.
    ArrayRef {
        name: String,
        index: ArrayIndex,
        /// `true` for the `${#…}` form: element count for `@`/`*`, or the length
        /// of a specific element for an index.
        length: bool,
    },
    /// `${!name[@]}` / `${!name[*]}` — the *keys* (associative array) or
    /// *indices* (indexed array) of `name`.
    ArrayKeys {
        name: String,
        /// `true` for `[*]` (join with the first IFS char when quoted); `false`
        /// for `[@]` (one field per key).
        star: bool,
    },
    /// `${name@op}` — parameter transformation. `op` is a single operator
    /// character: `Q` (quote for reuse), `U`/`u`/`L` (upper-all/upper-first/
    /// lower-all), `E` (expand ANSI-C backslash escapes), `a` (attribute flags).
    ParamTransform {
        name: String,
        /// Optional array subscript (`${a[i]@Q}`).
        index: Option<Box<Word>>,
        op: char,
    },
    /// `${name[@]:off:len}` / `${name[*]:off:len}` — array slice, and the
    /// positional-parameter forms `${@:off:len}` / `${*:off:len}`. Selects a
    /// contiguous run of elements (by position, 0-based) rather than a substring.
    ArraySlice {
        /// The array name, or `@`/`*` for positional parameters.
        name: String,
        /// `true` for the `[*]` / `$*` form (join into one field when quoted);
        /// `false` for `[@]` / `$@` (one field per element).
        star: bool,
        offset: Box<Word>,
        length: Option<Box<Word>>,
    },
    /// A pattern/case/substitution operator applied to *every* element of an
    /// array (`${a[@]#pat}`, `${a[@]/x/y}`, `${a[@]^^}`, `${a[@]@Q}`) or to every
    /// positional parameter (`${@#pat}`, …). The scalar equivalents live in
    /// `ParamTrim`/`ParamReplace`/`ParamCase`/`ParamTransform`.
    ArrayBulk {
        /// The array name, or `@`/`*` for positional parameters.
        name: String,
        /// `true` for the `[*]` / `$*` form (join into one field when quoted).
        star: bool,
        op: BulkOp,
    },
}

/// The operator carried by [`WordPart::ArrayBulk`], applied element-wise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkOp {
    /// `${a[@]#pat}` / `##` / `%` / `%%` — prefix/suffix removal per element.
    Trim {
        suffix: bool,
        longest: bool,
        pattern: Box<Word>,
    },
    /// `${a[@]/pat/repl}` — pattern substitution per element.
    Replace {
        all: bool,
        anchor: ReplaceAnchor,
        pattern: Box<Word>,
        replacement: Box<Word>,
    },
    /// `${a[@]^pat}` / `^^` / `,` / `,,` — case modification per element.
    Case {
        upper: bool,
        all: bool,
        pattern: Box<Word>,
    },
    /// `${a[@]@Q}` etc. — parameter transformation per element.
    Transform { op: char },
}

/// An array subscript inside `${name[…]}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrayIndex {
    /// `[expr]` — a specific element (the expression is evaluated arithmetically).
    Index(Box<Word>),
    /// `[@]` — all elements, each a separate word when quoted.
    All,
    /// `[*]` — all elements joined by the first IFS character when quoted.
    Star,
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

/// Where a `${name/pat/repl}` substitution is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceAnchor {
    /// Match anywhere (`/` or `//`).
    None,
    /// Anchored at the start of the value (`/#`).
    Start,
    /// Anchored at the end of the value (`/%`).
    End,
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
    /// `>| file` — truncate/create, overriding `noclobber` (`set -C`).
    Clobber,
    /// `>> file` — append.
    Append,
    /// `< file` — read.
    Read,
    /// `n>&m` — duplicate an fd (target parsed as the target fd number).
    DupOut,
    /// `<< delim` (or `<<-`) — here-document. The redirect's `target` word holds
    /// the already expansion-lowered body content; a quoted delimiter yields a
    /// single literal part (no expansion).
    HereDoc,
    /// `<<< word` — here-string. The `target` word is expanded and fed to stdin
    /// with a trailing newline.
    HereStr,
}
