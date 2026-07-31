//! Abstract syntax tree for the OSH shell language.
//!
//! The grammar modelled here is the common POSIX-sh / bash core that the
//! parser currently accepts. It intentionally starts small and grows toward
//! the full bash-superset (arrays, `[[ ]]`, `(( ))`, here-docs) — see the
//! crate-level docs and `design-decisions.md §72`.
//!
//! # Text vs. bytes
//!
//! Source text that the shell reproduces or re-parses verbatim — literals,
//! quoted runs, here-doc delimiters, arithmetic and command-substitution
//! bodies — is [`Str`] (a byte string), because a shell word can name a file
//! and a SlateOS filename is an arbitrary byte sequence bar `/` and NUL.
//! *Names* stay `String`: variable, function-parameter, `{fd}` and label
//! namespaces are `[A-Za-z_][A-Za-z0-9_]*` by grammar, so text is not an
//! approximation there — it is the truth.

use crate::bytes::Str;

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
    /// 1-based source line on which this item begins. Used to maintain the
    /// `$LINENO` special parameter as the interpreter executes each item. The
    /// line is taken from the lexer's per-token line stamp (see
    /// `Parser::cur_line`), so it stays exact even when earlier tokens swallowed
    /// newlines inside a here-doc body, a multi-line quoted string, or a command
    /// substitution. (Line tracking is per-item, not per-simple-command; see
    /// known-issues TD-OILS20 for the remaining per-command-granularity gap.)
    pub line: u32,
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
    /// non-zero, 1 if zero). The payload is the raw arithmetic text.
    Arith(Str),
    /// `coproc [NAME] command` — run `command` asynchronously with its
    /// stdin/stdout wired to two pipes. Exposes an array `NAME` (default
    /// `COPROC`) where `NAME[0]` reads the coproc's stdout and `NAME[1]`
    /// writes its stdin, plus scalar `NAME_PID`. `name` is `None` when no
    /// explicit name was given (defaults to `COPROC` at runtime).
    Coproc {
        name: Option<String>,
        body: Box<Command>,
    },
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
    Unary(CondUnary, Word),
    /// Binary comparison between two words.
    Binary(Box<Word>, CondBinary, Box<Word>),
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
    /// `( expr )` — an explicit grouping, kept even when it is redundant.
    ///
    /// Evaluation ignores it: the tree it wraps already has the shape the
    /// parentheses forced. It survives only so the expression can be printed
    /// back the way it was written, which bash does verbatim — and dropping it
    /// would be worse than untidy, because `( a || b ) && c` reprinted without
    /// the parentheses is `a || (b && c)`, a different test.
    Group(Box<CondExpr>),
}

/// A `[[ … ]]` unary test operator together with the spelling it was written
/// with.
///
/// bash keeps the operator's source word in the node and echoes it back
/// verbatim — both in a `set -x` trace and when `declare -f` reprints the
/// function — so a synonym must survive parsing: `[[ -h f ]]` comes back out as
/// `-h`, never normalised to its twin `-L`. The [`UnaryOp`] carries the
/// semantics, `text` only the spelling; nothing should dispatch on `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CondUnary {
    /// Which test to perform.
    pub op: UnaryOp,
    /// The operator exactly as written (`-h` vs. `-L`).
    pub text: &'static str,
}

/// A `[[ … ]]` binary operator together with the spelling it was written with —
/// the binary counterpart of [`CondUnary`], so that `[[ a = b ]]` is not
/// reprinted as `[[ a == b ]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CondBinary {
    /// Which comparison to perform.
    pub op: CondBinOp,
    /// The operator exactly as written (`=` vs. `==`).
    pub text: &'static str,
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
    pub decl_arrays: Vec<DeclArray>,
    /// 1-based source line the command word sits on. Used to keep `$LINENO`
    /// and diagnostics correct *per command* — bash advances `$LINENO` as each
    /// simple command executes, so a multi-line pipeline blames the failing
    /// stage's own line rather than the pipeline's first line. `0` for
    /// synthetically-built commands with no source position.
    pub line: u32,
}

/// An array-literal operand of a declaration builtin, together with where it sat
/// among the command's words.
///
/// The operand is kept out of [`SimpleCommand::words`] because it is not a word:
/// its value is bound by the shell during word expansion, and the builtin is
/// handed only the operand's *name*. But its position among the words is still
/// observable in two places — `$BASH_COMMAND` reproduces the operand order as
/// written, and `set -x` traces the builtin's line with a bare name standing in
/// for the operand at its original spot (`+ declare -x SC=1 arr SD=2`). Hence
/// `word_index`, without which both render the operands last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclArray {
    /// The assignment itself, e.g. the `m=([k]=v)` of `declare -A m=([k]=v)`.
    pub assign: Assignment,
    /// How many words preceded the operand: `words[word_index]` is the word that
    /// followed it, so inserting something at that index restores the source
    /// order. Several operands may share an index (consecutive operands, with no
    /// word between them); rendering them in `decl_arrays` order keeps those in
    /// their own relative order too.
    pub word_index: usize,
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
    pub init: Str,
    pub cond: Str,
    pub update: Str,
    pub body: Program,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    /// The name as *written*. bash's grammar accepts any word before `()`, so
    /// this is not restricted to identifiers: `my-func`, `a.b`, `1f` and `f?`
    /// are all real function names. When [`definable`](Self::definable) is
    /// false this is instead the source spelling of a word bash refuses at run
    /// time (`\f`, `"f"`, `$x`), kept verbatim so the error can quote it back
    /// exactly as typed.
    ///
    /// Bytes, not text: bash accepts *any* word here, so a function may be
    /// named after a file, and a SlateOS filename need not be UTF-8.
    pub name: Str,
    /// Whether the name may actually become a function. bash defers this check
    /// to execution: a quoted or expanded name parses fine and then fails with
    /// ``line N: `NAME': not a valid identifier`` — status 1, and the script
    /// carries on. Always true for a name written as a bare word.
    pub definable: bool,
    pub body: Program,
    /// Redirections attached to the function definition itself, e.g.
    /// `f() { …; } >log`. bash applies these every time the function is invoked,
    /// wrapping the body's execution (they are stored with the function, not run
    /// at definition time). Empty for the common redirect-less definition.
    pub redirects: Vec<Redirect>,
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
    pub fn literal(s: impl Into<Str>) -> Self {
        Word {
            parts: vec![WordPart::Literal(s.into())],
        }
    }

    /// Whether expanding this word can be observed — i.e. whether it may run a
    /// command, assign a variable, or open a file.
    ///
    /// Only text and quoting qualify as free of that. Everything else is
    /// conservatively assumed to be observable: a command substitution runs a
    /// command outright, `$(( i++ ))` and `${x:=y}` assign, `<( … )` spawns a
    /// process, and even a plain `$x` can be a nameref whose resolution is worth
    /// not repeating.
    ///
    /// Callers use this to decide whether a word may be expanded *speculatively*
    /// — expanded once to look at, then discarded and expanded again later by
    /// whichever path really runs the command.
    #[must_use]
    pub fn expansion_is_unobservable(&self) -> bool {
        fn parts_ok(parts: &[WordPart]) -> bool {
            parts.iter().all(|p| match p {
                WordPart::Literal(_) | WordPart::SingleQuoted { .. } => true,
                WordPart::DoubleQuoted(inner) => parts_ok(inner),
                _ => false,
            })
        }
        parts_ok(&self.parts)
    }
}

/// A fragment of a word. Quoting is captured per-part so field splitting and
/// glob expansion can respect it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordPart {
    /// Unquoted literal text (subject to later splitting/globbing).
    Literal(Str),
    /// Quoted literal text (no expansion, no splitting): the contents of
    /// `'…'`/`$'…'`, or a single backslash-escaped character, which means the
    /// same thing (`a\*b` ≡ `a'*'b`).
    ///
    /// `escaped` is `true` for the backslash spelling. Expansion treats both
    /// identically; only [`crate::unparse`] cares, because bash prints a
    /// stored function body back in whichever form the source wrote.
    SingleQuoted { text: Str, escaped: bool },
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
        /// `true` for the colon forms (`:-`/`:=`/`:+`/`:?`), which treat an empty
        /// value the same as unset; `false` for the colon-less forms (`-`/`=`/
        /// `+`/`?`), which act only when the parameter is genuinely *unset*.
        colon: bool,
        arg: Box<Word>,
        /// The name a `?`/`:?` complaint quotes, when that is not the name being
        /// read. Only indirection sets it: `${!ref:?msg}` reads whatever `ref`
        /// points at but names `!ref`, because the target's name is an answer
        /// the writer never gave and so is not theirs to be told about.
        label: Option<String>,
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
    /// `${name,,pat}` (lower-case) / `${name~pat}` / `${name~~pat}` (toggle) —
    /// case modification. `all` is the doubled operator (convert every character
    /// whose value matches `pattern`); otherwise only the first character is
    /// considered. `pattern` selects which characters convert (a glob matched
    /// against one character at a time); an empty pattern matches any character.
    ParamCase {
        name: String,
        /// Optional array subscript (`${a[i]^^}`).
        index: Option<Box<Word>>,
        /// Which case transform to apply: `^`→Upper, `,`→Lower, `~`→Toggle.
        mode: CaseMode,
        /// `true` for the doubled form (every matching character).
        all: bool,
        pattern: Box<Word>,
    },
    /// `${!name}` / `${!name[i]}` — indirect expansion: the value of the
    /// variable whose *name* is the value read through the reference (e.g.
    /// `ref=x; x=hi; ${!ref}` → `hi`).
    ///
    /// `refname` is the referring variable and `index` its subscript when the
    /// pointer is an array *element* rather than a plain variable. Both ends of
    /// the indirection may therefore carry a subscript, and they are separate
    /// facts: `index` says where the *name* is read from, while the name read
    /// may itself be an element reference (`ref=a[0]`, `ref=a[@]`).
    Indirect {
        refname: String,
        index: Option<Box<Word>>,
    },
    /// `${!ref<op>}` — indirect expansion *combined with* a modifier, e.g.
    /// `${!ref:-def}`, `${!ref^^}`, `${!ref#pat}`, `${!ref/a/b}`. Bash forms the
    /// target variable name from the value read through the reference, then
    /// applies the rest of the substitution to *that* variable. `refname` and
    /// `index` designate the pointer exactly as in `Indirect`; `target` is the
    /// modifier expansion (a `ParamOp`/`ParamTrim`/`ParamSubstr`/`ParamReplace`/
    /// `ParamCase`/`ParamTransform`) parsed with `refname` as a placeholder name,
    /// rewritten to the resolved target name at expansion time.
    IndirectOp {
        refname: String,
        index: Option<Box<Word>>,
        target: Box<WordPart>,
    },
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
    CommandSub { body: CmdSubBody },
    /// `$(( expr ))` arithmetic substitution (raw expression text for now).
    /// `bracket` records the deprecated `$[ expr ]` spelling, which evaluates
    /// identically but is printed back as written (bash `declare -f`).
    ArithSub { expr: Str, bracket: bool },
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
    /// `${name@}` (empty operator), `${name@Z}` (unknown operator), or
    /// `${name@QU}` (multi-char operator) — an *invalid* parameter
    /// transformation. bash defers the decision to expansion time: if the
    /// parameter is **unset** the result is empty (status 0), but if it is
    /// **set** it is a runtime "bad substitution". `raw` is the exact source
    /// text between `${` and `}` (e.g. `x@`, `a[0]@Z`) for the diagnostic.
    BadTransform {
        name: String,
        index: Option<Box<Word>>,
        raw: Str,
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
    /// `${a[@]:-word}` / `${a[*]:+word}` / `${a[@]:?msg}` — a use/alternate/error
    /// operator applied to a whole-array reference (`[@]`/`[*]`). Bash treats the
    /// array like `$@`: when the reference is "active" (the array is set /
    /// non-null), the elements expand (one field each for `[@]`, joined by the
    /// first `$IFS` char for `[*]`); otherwise the `:-`/`:?` word is substituted,
    /// or the `:+` alternate is used. `${a[@]:=word}` is an error in bash
    /// ("cannot assign in this way") and is reported as such.
    ArrayOp {
        /// The array name (never `@`/`*`, which have no `[…]` subscript — those
        /// go through the scalar [`WordPart::ParamOp`] path).
        name: String,
        /// `true` for the `[*]` form (join with the first `$IFS` char when
        /// quoted); `false` for `[@]` (one field per element).
        star: bool,
        op: ParamOp,
        /// `true` for the colon forms (treat an all-empty array as null).
        colon: bool,
        arg: Box<Word>,
    },
    /// A `${…}` whose inner form the parser recognised as a brace expansion but
    /// could not otherwise interpret (`${x!}`, `${!x*junk}`, `${#a[i]extra}`, …).
    /// bash accepts such text at *parse* time and only rejects it during
    /// expansion as a runtime "bad substitution" (a DISCARD-class word-expansion
    /// error: it prints `${raw}: bad substitution`, sets `$?`=1, and aborts the
    /// current parse unit without exiting the shell). The stored string is the
    /// text *between* the braces, so the diagnostic reproduces `${raw}`.
    BadSubst(Str),
    /// Process substitution `<(cmd)` (input) / `>(cmd)` (output). Expands to the
    /// pathname of a file the shell connects to `cmd`: for `<(cmd)` the file holds
    /// `cmd`'s output (read by the enclosing command); for `>(cmd)` the file's
    /// contents are fed to `cmd`'s stdin after the enclosing command finishes.
    ProcSub {
        /// `true` for `<(cmd)` (the command's output is read); `false` for
        /// `>(cmd)` (data written to the file is sent to the command).
        input: bool,
        body: Program,
    },
}

/// How a fragment's own line numbers become the numbers the shell reports.
///
/// A source lexed on its own numbers its lines from 1, which is wrong whenever
/// that source is a *fragment* of a larger input — a REPL command read from a
/// stdin stream that has already delivered N lines, an `eval` string, a
/// `$( … )` body. Applying the mapping before the parse means every AST node
/// ends up carrying an absolute line, so `$LINENO` inside a function body is
/// right no matter where the function is later called from (bash numbers a body
/// relative to the source that *defined* it, verified against bash 5.2).
///
/// Two rules are needed because bash uses two. Everything but a `$( … )` body
/// is a plain offset; a `$( … )` body is renumbered by *rank* — see
/// [`LineMap::CmdSub`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineMap {
    /// Reported line = raw line + this offset. `Offset(0)` is the identity, for
    /// a source that numbers its own lines from 1.
    Offset(u32),
    /// The `$( … )` rule: reported line = `close_line` + the 0-based **rank** of
    /// the raw line among the body's command-bearing lines.
    ///
    /// bash scans the enclosing command first and only then re-reads the body,
    /// so the body's lines count up from the line the outer scan had already
    /// reached — the substitution's *closing* delimiter. A rank, not an offset:
    /// a blank body line does not advance it, and two commands on one body line
    /// share a number. Measured against bash 5.x over 11 probes; see
    /// `crate::parser::parse_cmdsub_body` for the worked example.
    CmdSub {
        /// Added to a raw line before the lookup. Non-zero only for a *tail* of
        /// the body that has been re-lexed on its own (see
        /// [`LineMap::shifted`]), whose lines restart at 1.
        pre: u32,
        /// The line the body's closing `)` sits on, already absolute.
        close_line: u32,
        /// `(body line, reported line)` for each distinct command-bearing body
        /// line, ascending. Small — one entry per body line — so a linear scan
        /// beats a map.
        ranked: Vec<(u32, u32)>,
    },
}

impl Default for LineMap {
    fn default() -> Self {
        Self::Offset(0)
    }
}

impl From<u32> for LineMap {
    fn from(base: u32) -> Self {
        Self::Offset(base)
    }
}

impl LineMap {
    /// The line `raw` is reported as.
    ///
    /// Under [`LineMap::CmdSub`], a line between two command-bearing lines (a
    /// blank line, or the continuation lines of a multi-line token) takes the
    /// number of the nearest preceding command-bearing line, which is what
    /// makes a newline token report the line its command was on.
    #[must_use]
    pub fn map(&self, raw: u32) -> u32 {
        match self {
            Self::Offset(base) => raw.saturating_add(*base),
            Self::CmdSub { pre, close_line, ranked } => {
                let raw = raw.saturating_add(*pre);
                let mut out = *close_line;
                for &(body_line, reported) in ranked {
                    if body_line > raw {
                        break;
                    }
                    out = reported;
                }
                out
            }
        }
    }

    /// The map for a *tail* of the same source, whose own lines restart at 1
    /// after `n` lines have already been consumed.
    ///
    /// Re-lexing the unconsumed remainder under new options is how a mid-input
    /// `shopt -s extglob` takes effect (see `IncrementalParser::relex`); the
    /// tail's line 1 is the whole source's line `n + 1`, so the mapping has to
    /// compose with that shift rather than be replaced by it.
    #[must_use]
    pub fn shifted(&self, n: u32) -> Self {
        match self {
            Self::Offset(base) => Self::Offset(base.saturating_add(n)),
            Self::CmdSub { pre, close_line, ranked } => Self::CmdSub {
                pre: pre.saturating_add(n),
                close_line: *close_line,
                ranked: ranked.clone(),
            },
        }
    }

    /// The raw line a reported one came from, for echoing the offending source
    /// line back in a diagnostic. `None` when no raw line maps to it.
    ///
    /// The inverse is not total: under [`LineMap::CmdSub`] several raw lines can
    /// share a reported number, and the one wanted is the command-bearing line —
    /// which is exactly the one `ranked` records.
    #[must_use]
    pub fn unmap(&self, reported: u32) -> Option<u32> {
        match self {
            Self::Offset(base) => reported.checked_sub(*base),
            Self::CmdSub { pre, ranked, .. } => ranked
                .iter()
                .find(|&&(_, r)| r == reported)
                .and_then(|&(body_line, _)| body_line.checked_sub(*pre)),
        }
    }

    /// Whether this map leaves every line alone, so applying it can be skipped.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        matches!(self, Self::Offset(0))
    }
}

/// The body of a [`WordPart::CommandSub`], in the form its spelling calls for.
///
/// bash reads the two spellings at different times, and that is observable:
///
/// ```sh
/// if false; then echo $(for); fi   # syntax error — the whole unit fails to parse
/// if false; then echo `for`;  fi   # silence — the body is never read
/// ```
///
/// A `$( … )` body is parsed in the enclosing token stream, so its errors are
/// the enclosing parse's errors. A backtick body is only a *string* until the
/// word is expanded; bash parses it then, per expansion (a substitution in a
/// loop is re-parsed every iteration), which is also what makes a `shopt -s
/// extglob` between two expansions change how it reads.
///
/// bash reads a `$( … )` body *twice*, though: once with the enclosing scan, to
/// find the matching `)` and to raise a syntax error there, and again at
/// expansion time as an input of its own. Only the second pass runs, so both
/// halves are kept here — [`CmdSubBody::Parsed::prog`] is the first pass (whose
/// errors are the enclosing parse's, and which `declare -f` re-prints), and
/// `src`/`map` are what the second pass re-reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdSubBody {
    /// `$( … )` — parsed with the enclosing source, then re-read at expansion
    /// time so a `shopt`/`alias` the body runs affects the rest of the body.
    Parsed {
        /// The eager parse: what the enclosing scan produced, and what
        /// `declare -f` re-prints.
        prog: Program,
        /// The body text, re-read one logical line at a time when the
        /// substitution is expanded.
        src: Str,
        /// How that re-read's own line numbers become the numbers the shell
        /// reports — the rank-based `$( … )` rule, already applied to `prog`.
        map: LineMap,
    },
    /// `` ` … ` `` — parsed at expansion time, by `Shell::command_sub`.
    Backtick {
        /// The body with `` \` ``/`\\`/`\$` unescaped: what actually gets parsed.
        src: Str,
        /// The body exactly as written, for `declare -f`. bash echoes a backtick
        /// body verbatim rather than re-printing it, and re-printing is not
        /// merely untidy — a nested `` \` `` would lose its backslash and the
        /// result would no longer parse.
        verbatim: Str,
        /// The line the closing backtick sits on, in the enclosing source.
        ///
        /// The body's own lines are numbered from `close_line - 1` — a plain
        /// offset, unlike the rank-based renumbering a `$( … )` body gets. Both
        /// are bash's, measured: with the body spread over two lines,
        /// `$LINENO` in `$( … )` reports the closing line and the one after,
        /// while in `` ` … ` `` it reports one more than each.
        close_line: u32,
    },
}

impl CmdSubBody {
    /// The parsed body, or `None` for a backtick body that has not been parsed
    /// (which only happens at expansion time — see the type docs).
    #[must_use]
    pub fn parsed(&self) -> Option<&Program> {
        match self {
            Self::Parsed { prog, .. } => Some(prog),
            Self::Backtick { .. } => None,
        }
    }
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
    /// `${a[@]^pat}` / `^^` / `,` / `,,` / `~` / `~~` — case mod per element.
    Case {
        mode: CaseMode,
        all: bool,
        pattern: Box<Word>,
    },
    /// `${a[@]@Q}` etc. — parameter transformation per element.
    Transform { op: char },
    /// `${a[@]@}` / `${a[@]@Z}` / `${a[@]@QU}` — an *invalid* per-element
    /// transform (empty, unknown, or multi-char operator). Like the scalar
    /// [`WordPart::BadTransform`], bash defers it: a whole-array/positional
    /// reference with **no elements** expands empty, but with one or more
    /// elements it is a runtime "bad substitution". `raw` is the source text
    /// between `${` and `}` for the diagnostic.
    BadTransform { raw: Str },
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

/// Which case transform a `${name^}` / `${name,}` / `${name~}` operator applies
/// to each matching character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    /// `^` / `^^` — force upper-case.
    Upper,
    /// `,` / `,,` — force lower-case.
    Lower,
    /// `~` / `~~` — toggle case (upper↔lower).
    Toggle,
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
    /// The fd being redirected (defaults filled in by the parser). Ignored when
    /// [`Redirect::varfd`] is set — the fd is then allocated at runtime.
    pub fd: i32,
    pub op: RedirectOp,
    pub target: Word,
    /// A varfd prefix `{name}` (`{fd}>file`): the executor allocates a free fd
    /// ≥ 10, applies the redirect to it, and stores the number in shell variable
    /// `name`. `None` for an ordinary numeric/default fd redirect.
    pub varfd: Option<String>,
    /// How the here-document was written, for [`RedirectOp::HereDoc`] only —
    /// `None` for every other operator. Expansion does not need it (the body is
    /// already lowered into `target`), but printing the redirect back does.
    pub here: Option<HereDoc>,
}

/// The parts of a `<<`/`<<-` redirection that are not carried by its body word.
///
/// The delimiter has no effect on what the here-document delivers — the lexer
/// has already consumed the body — but `declare -f` prints a stored function
/// back as source, and that source has to name a delimiter again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HereDoc {
    /// The delimiter word with its quoting removed (`<<'EOF'` → `EOF`).
    pub delim: Str,
    /// The delimiter was quoted in any form (`'EOF'`, `"EOF"`, `\EOF`), which
    /// suppressed expansion of the body. bash prints every spelling back as
    /// `'EOF'`.
    pub quoted: bool,
    /// The `<<-` spelling, which strips leading tabs from the body lines and
    /// from the closing delimiter.
    pub strip: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectOp {
    /// `> file` — truncate/create.
    Write,
    /// `>| file` — truncate/create, overriding `noclobber` (`set -C`).
    Clobber,
    /// `>> file` — append.
    Append,
    /// `&> file` / `>& file` (non-numeric target) — redirect both stdout and
    /// stderr to the file, truncating/creating it.
    WriteBoth,
    /// `&>> file` — redirect both stdout and stderr to the file, appending.
    AppendBoth,
    /// `< file` — read.
    Read,
    /// `<> file` — open the target for both reading and writing (`O_RDWR |
    /// O_CREAT`, no truncation). Default fd is 0.
    ReadWrite,
    /// `n>&m` — duplicate an output fd (target parsed as the target fd number).
    DupOut,
    /// `n<&m` — duplicate an input fd (target parsed as the source fd number).
    /// Distinct from `DupOut` so the redirection direction (input vs output) is
    /// preserved through the AST — `<&` defaults to fd 0, `>&` to fd 1, and the
    /// executor routes an input dup to the command's stdin rather than stdout.
    DupIn,
    /// `<< delim` (or `<<-`) — here-document. The redirect's `target` word holds
    /// the already expansion-lowered body content; a quoted delimiter yields a
    /// single literal part (no expansion).
    HereDoc,
    /// `<<< word` — here-string. The `target` word is expanded and fed to stdin
    /// with a trailing newline.
    HereStr,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command word of the first simple command in `src`.
    fn first_word(src: &str) -> Word {
        let prog = crate::parser::parse(src).expect("parse");
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("not a simple command: {src}");
        };
        sc.words[0].clone()
    }

    #[test]
    fn expansion_is_unobservable_only_for_plain_text() {
        // Text and quoting expand to themselves, so expanding twice is the same
        // as expanding once — including a double-quoted run of pure literals.
        for src in [
            "cat",
            "'cat'",
            r"c\at",
            "\"cat\"",
            "\"ca\"t'x'",
            r"$'ca\tt'",
            "/usr/bin/cat",
        ] {
            assert!(
                first_word(src).expansion_is_unobservable(),
                "expected unobservable: {src}"
            );
        }
        // Anything that consults or changes shell state is not, whether it sits
        // at the top of the word or nested inside double quotes.
        for src in [
            "$cmd",
            "${cmd}",
            "${cmd:-cat}",
            "${cmd:=cat}",
            "$(echo cat)",
            "`echo cat`",
            "$((1+1))",
            "pre$cmd",
            "\"$cmd\"",
            "\"pre$(echo x)\"",
            "${a[0]}",
            "${#cmd}",
        ] {
            assert!(
                !first_word(src).expansion_is_unobservable(),
                "expected observable: {src}"
            );
        }
    }
}
