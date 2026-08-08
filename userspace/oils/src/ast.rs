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

/// How one item is joined to the next — bash's `CONNECTION` connector.
///
/// bash builds a list as a tree of `Connection` nodes whose `connector` is one
/// of `&`, `;` or `\n`, and its deparser prints all three differently
/// (print_cmd.c:296–326): `&` runs the next command in inline (` & `), `;` is
/// `"; "` outside a function definition and `";\n"` inside one, and a `\n` is
/// *kept* as a newline while the printer is re-printing a command substitution
/// (`printing_comsub`). So `$(echo a; echo b)` comes back on one line and
/// `$(echo a<newline>echo b)` comes back on two, and the difference is not
/// recoverable from the line numbers: a `;` followed by a newline is still a
/// `;` connector.
///
/// osh's list is flat, so the separator is kept on the item it *followed*,
/// which carries the same information. The last item's separator is never a
/// connector — bash's grammar reduces a trailing `;` through `list_terminator`,
/// building no node — so it only ever matters for a non-final item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSep {
    /// `;`, or no separator at all: the two parse alike and print alike.
    Semi,
    /// A newline.
    Newline,
    /// `&` — the item runs asynchronously.
    Amp,
}

/// One top-level item: an and-or list plus how it was terminated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub list: AndOr,
    /// How this item was separated from the one after it.
    pub sep: ItemSep,
    /// 1-based source line on which this item begins. Used to maintain the
    /// `$LINENO` special parameter as the interpreter executes each item. The
    /// line is taken from the lexer's per-token line stamp (see
    /// `Parser::cur_line`), so it stays exact even when earlier tokens swallowed
    /// newlines inside a here-doc body, a multi-line quoted string, or a command
    /// substitution. (Line tracking is per-item, not per-simple-command; see
    /// known-issues TD-OILS20 for the remaining per-command-granularity gap.)
    pub line: u32,
}

impl Item {
    /// Whether the item runs asynchronously — it ended with `&`.
    #[must_use]
    pub fn is_background(&self) -> bool {
        self.sep == ItemSep::Amp
    }
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
    Cond(CondClause),
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

/// `[[ expr ]]` — the expression together with the line the shell stands on
/// while it is evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondClause {
    pub expr: CondExpr,
    /// The line this `[[ … ]]` is *executed* at.
    ///
    /// bash has no single stamp for the construct: `make_cond_node` records
    /// `line_number` as it builds **each** node (make_cmd.c:463) and
    /// `make_cond_command` keeps the **root**'s (make_cmd.c:486), which
    /// `execute_cond_command` then installs with
    /// `SET_LINE_NUMBER (cond_command->line)` (execute_cmd.c:4029). Since the
    /// root is whatever was built last, the line depends on the expression's
    /// shape:
    ///
    /// * a term — a binary or unary test, a bare word, or a `( … )` group — is
    ///   built the instant its last token has been read and *before* the
    ///   `cond_skip_newlines()` that ends `cond_term` (parse.y:4669, 4702,
    ///   4770, 4786), so it carries the line that token ended on;
    /// * `&&` and `||` are built by `cond_and`/`cond_or` (parse.y:4603, 4617)
    ///   *after* the right-hand term's own newline skip has already fetched the
    ///   token behind it — so they carry the line of whatever closes the
    ///   expression, normally the `]]`;
    /// * `!` builds nothing at all: it flips `CMD_INVERT_RETURN` on the node it
    ///   was given (parse.y:4676-4678), which keeps that node's line.
    ///
    /// So it is *not* simply "the `]]`'s line". All four of these were
    /// measured:
    ///
    /// ```text
    /// [[ ${nope?bad} == x⏎]]                    # line 1 — the term's
    /// [[ ${nope?bad} == x &&⏎y == y ]]          # line 2 — the `]]`'s
    /// [[ y == y &&⏎${nope?bad} == x⏎]]          # line 3 — the `]]`'s
    /// [[ ( ${nope?bad} == x && y == y )⏎]]      # line 1 — the `)`'s
    /// ```
    ///
    /// `0` means "not recorded", as in [`CaseClause::line`].
    pub line: u32,
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

/// A `[[ … ]]` unary test operator, held as the spelling it was written with.
///
/// bash keeps the operator's source word in the node and echoes it back
/// verbatim — both in a `set -x` trace and when `declare -f` reprints the
/// function — so a synonym must survive parsing: `[[ -h f ]]` comes back out as
/// `-h`, never normalised to its twin `-L`.
///
/// The spelling is *all* the node carries, because the spelling is what selects
/// the test: `[[ … ]]` and the `test`/`[` builtin recognise one and the same set
/// of primaries ([`unary_op_text`]) and evaluate them with one and the same
/// code. Lowering to a separate semantic enum here would mean a second table to
/// keep in step with the builtin's — which is exactly how `[[ -R nr ]]` came to
/// be a syntax error while `[ -R nr ]` worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CondUnary {
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

/// The unary test primaries, and the *only* table of them.
///
/// bash draws `[[ … ]]` and the `test`/`[` builtin from one list — measured:
/// every one of these 26 parses in both, and nothing else parses in either — so
/// this is one list here too. `unary_op_from` in the parser and
/// `is_test_unary_op` in the interpreter are both this function, which is what
/// keeps the two surfaces from drifting apart.
///
/// Each primary is a single letter after `-`, and the two synonym pairs
/// (`-e`/`-a`, `-L`/`-h`) are separate entries so that each keeps its own
/// spelling for a `set -x` trace or a `declare -f` reprint.
///
/// | | |
/// |---|---|
/// | `-e` `-a` `-f` `-d` `-s` | exists / regular file / directory / non-empty |
/// | `-r` `-w` `-x` | readable / writable / executable by us |
/// | `-b` `-c` `-p` `-S` | block / character / FIFO / socket |
/// | `-u` `-g` `-k` | setuid / setgid / sticky bit |
/// | `-O` `-G` | owned by our effective uid / gid |
/// | `-L` `-h` | symbolic link (final component not followed) |
/// | `-N` | modified since it was last read |
/// | `-t` | descriptor is a terminal |
/// | `-z` `-n` | string is empty / non-empty |
/// | `-v` `-o` `-R` | variable set / option enabled / name is a nameref |
///
/// Returns the matched spelling as a `&'static str` so the caller can store it
/// in a [`CondUnary`] without borrowing the source.
#[must_use]
pub fn unary_op_text(s: &[u8]) -> Option<&'static str> {
    const OPS: &[&str] = &[
        "-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-n", "-o", "-p", "-r", "-s", "-t",
        "-u", "-v", "-w", "-x", "-z", "-G", "-L", "-N", "-O", "-R", "-S",
    ];
    OPS.iter().copied().find(|t| t.as_bytes() == s)
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
    ///
    /// `append` is set by the `[sub]+=value` spelling, which concatenates onto
    /// whatever the slot holds when the element is bound (or *adds* to it under
    /// the `-i` attribute) instead of replacing it.
    Keyed { index: Word, value: Word, append: bool },
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
    /// The loop variable as *written*. bash's grammar accepts any word where the
    /// control variable goes and checks it for identifier-ness at run time, so
    /// `'a[0]'`, `a=b`, `1x` and `$v` all parse and then fail with ``line N:
    /// `WORD': not a valid identifier``. The check is on the spelling and not on
    /// what it would expand to — `"x"` is refused though `x` is a fine name — so
    /// the source spelling is what must be stored, both to make the decision and
    /// to quote back in the error.
    ///
    /// Bytes, not text, for the same reason as [`FunctionDef::name`]: a word
    /// that is not an identifier need not be UTF-8 either.
    pub var: Str,
    /// The `in …` word list; `None` means iterate over `"$@"`.
    pub words: Option<Vec<Word>>,
    pub body: Program,
    /// The line this loop is *executed* at — the line the loop **variable**
    /// ends on, not the one the `for` keyword is on. See [`CaseClause::line`],
    /// which bash stamps by the same rule and from the same place.
    pub line: u32,
}

/// `select var [in words]; do body; done` — bash's interactive menu loop.
/// Prints the numbered word list to stderr, reads a selection line from stdin
/// (with the `PS3` prompt), sets `var` to the chosen word (empty on bad input),
/// stores the raw line in `REPLY`, and runs the body until EOF or `break`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectClause {
    /// The menu variable as written — see [`ForClause::var`]. `select` checks it
    /// the same way and at the same moment, with one difference: posix mode does
    /// not make the refusal fatal here, only in `for`.
    pub var: Str,
    /// The `in …` word list; `None` means iterate over `"$@"`.
    pub words: Option<Vec<Word>>,
    pub body: Program,
    /// The line this loop is *executed* at — the line the menu **variable**
    /// ends on. See [`CaseClause::line`].
    pub line: u32,
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
    /// The line the `((` was read on — bash's `arith_for_lineno`, stamped in
    /// `parse_dparen` before the header is scanned at all (parse.y:4469) and
    /// alongside the `word_lineno` the other three take (see
    /// [`CaseClause::line`]).
    ///
    /// `execute_arith_for_command` keeps it in a local and restores the
    /// enclosing line around *each* of the three expressions rather than
    /// holding it for the whole loop (execute_cmd.c:3120, 3139-3141,
    /// 3171-3174), so the body runs on its own lines and only the header's
    /// arithmetic is blamed here:
    ///
    /// ```text
    /// for (( i=0;⏎i<2;⏎i+=${nope?bad} )); do echo b; done   # the `((`'s line
    /// ```
    ///
    /// `0` means "not recorded", as in [`CaseClause::line`].
    pub line: u32,
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
    /// The line this `case` is *executed* at, which is **not** the line the
    /// `case` keyword is on: it is the line the subject word *ends* on.
    ///
    /// bash stamps `case`, `for` and `select` alike, and it is the lexer that
    /// does it rather than the reduction. `read_token_word` sees that the token
    /// it has just finished follows a `CASE`/`SELECT`/`FOR` and records the line
    /// it ended on (parse.y:5352-5357):
    ///
    /// ```c
    ///     case CASE:
    ///     case SELECT:
    ///     case FOR:
    ///       if (word_top < MAX_CASE_NEST)
    ///         word_top++;
    ///       word_lineno[word_top] = line_number;
    /// ```
    ///
    /// Every one of the three commands' productions then hands that to its
    /// `make_*_command` (parse.y:839, 907, 949). So the number is where the
    /// **controlling word** ends — the `case` subject, or the loop variable —
    /// and the executors set the shell's one `line_number` from it before
    /// expanding anything (`line_number = case_command->line`,
    /// execute_cmd.c:3545, and the same in `execute_for_command` /
    /// `execute_select_command`), restoring it after. Every diagnostic raised
    /// while a subject, a pattern or a word list is expanded therefore carries
    /// that line and not the keyword's. (All three assign `line_number`
    /// directly, where a simple command and `[[ … ]]` go through
    /// `SET_LINE_NUMBER`, so these three do not also move the line an ERR trap
    /// reports.)
    ///
    /// The two coincide unless something stands between the keyword and the end
    /// of the controlling word — a word written across lines, or a line
    /// continuation before it — which is the only way to see it:
    ///
    /// ```text
    /// case "a⏎b" in "y${nope?bad}") ;; esac        # line 2, not 1
    /// for \⏎  x \⏎  in "${nope?bad}"; do :; done   # line 2, not 1
    /// ```
    ///
    /// `0` means "not recorded" and leaves the enclosing item's line standing,
    /// exactly as [`SimpleCommand::line`] does.
    pub line: u32,
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
    /// `$name` / `${name}` parameter reference. `braced` records which of the
    /// two spellings the source used. The two expand alike, but the spelling
    /// survives into what the shell prints and says: `declare -f` reproduces a
    /// function body as written, and a nounset diagnostic names the parameter
    /// the same way (`$1` from `$1`, but `1` from `${1}`).
    Param { name: String, braced: bool },
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
        label: Option<Str>,
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
    ///
    /// The pointer's subscript is a full [`ArrayIndex`] rather than a plain
    /// index expression because `[@]`/`[*]` may point too — with an operator
    /// after it, which is the only way to reach this variant with such a
    /// subscript, since a bare `${!a[@]}` is the key listing
    /// ([`WordPart::ArrayKeys`]) instead. The name is then read from the whole
    /// list, joined as the parameter's own elements are.
    Indirect {
        refname: String,
        index: Option<ArrayIndex>,
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
        index: Option<ArrayIndex>,
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

impl WordPart {
    /// The first `$(( … ))` body bash's *word scanner* would reach in this
    /// part, among those `hides_closer` accepts.
    ///
    /// bash decides some things about a word before expanding any of it, by
    /// scanning the source text left to right. The one such decision osh needs
    /// is whether a comment inside a `$(( … ))` has swallowed the closing `))`
    /// — see `Shell::arith_comment_hides_closer`. It has to be answered up
    /// front because bash answers it up front: `x=5; echo "${x:-$(( #5 ))}"`
    /// complains even though the `:-` branch is never taken.
    ///
    /// "Would reach" is the whole content of this function, and it is not the
    /// same as "contains":
    ///
    /// * A `$( … )` body is **not** reached. Its text is the command
    ///   substitution's own problem, raised when that command's words are
    ///   expanded — which is why `${x:-$( echo $(( #5 )) )}` names the inner
    ///   `$(( #5 ))` and not the outer word.
    /// * A `<( … )` / `>( … )` body is not reached either, and for the same
    ///   reason: the report comes from the child, after the `/dev/fd/N` has
    ///   already been printed.
    /// * Everything else is, including every operand, pattern, replacement,
    ///   subscript and slice bound of a `${ … }`, and the contents of a
    ///   double-quoted run.
    ///
    /// The match below is deliberately **exhaustive** — no `_ =>` arm. A new
    /// [`WordPart`] variant that can hold a [`Word`] must then be considered
    /// here rather than silently skipped, because a miss is invisible: the
    /// shell simply fails to report something bash reports.
    ///
    /// One known under-report: [`WordPart::BadSubst`] holds *unparsed* text, so
    /// a `$((` inside `${x!$(( #5 ))}` is characters rather than a node and
    /// there is nothing here to find. Matching it would mean re-scanning raw
    /// source, which can false-positive on a *valid* word; under-reporting
    /// cannot. See `TD-OILS-AN-UNPARSED-BRACE-BODY-IS-TEXT-THE-WORD-WALK-CANNOT-SEE`
    /// in `known-issues.md`.
    pub fn first_scanned_arith<'a>(
        &'a self,
        hides_closer: &mut dyn FnMut(&'a [u8]) -> bool,
    ) -> Option<&'a Str> {
        // Helpers, so each arm below is a list of its own sub-words.
        fn in_word<'a>(
            w: &'a Word,
            f: &mut dyn FnMut(&'a [u8]) -> bool,
        ) -> Option<&'a Str> {
            w.parts.iter().find_map(|p| p.first_scanned_arith(f))
        }
        fn in_opt<'a>(
            w: &'a Option<Box<Word>>,
            f: &mut dyn FnMut(&'a [u8]) -> bool,
        ) -> Option<&'a Str> {
            w.as_deref().and_then(|w| in_word(w, f))
        }
        fn in_index<'a>(
            i: &'a Option<ArrayIndex>,
            f: &mut dyn FnMut(&'a [u8]) -> bool,
        ) -> Option<&'a Str> {
            match i {
                Some(ArrayIndex::Index(w)) => in_word(w, f),
                Some(ArrayIndex::All | ArrayIndex::Star) | None => None,
            }
        }

        match self {
            // The one that answers the question.
            WordPart::ArithSub { expr, bracket } => {
                // `$[ … ]` is read by an extractor with no comment rule at all,
                // so only the `$(( … ))` spelling can lose its closer this way.
                (!*bracket && hides_closer(expr)).then_some(expr)
            }

            // Reached, and carrying sub-words.
            WordPart::DoubleQuoted(parts) => {
                parts.iter().find_map(|p| p.first_scanned_arith(hides_closer))
            }
            WordPart::ParamOp { index, arg, .. } => {
                in_opt(index, hides_closer).or_else(|| in_word(arg, hides_closer))
            }
            WordPart::ParamTrim { index, pattern, .. }
            | WordPart::ParamCase { index, pattern, .. } => in_opt(index, hides_closer)
                .or_else(|| in_word(pattern, hides_closer)),
            WordPart::ParamSubstr {
                index,
                offset,
                length,
                ..
            } => in_opt(index, hides_closer)
                .or_else(|| in_word(offset, hides_closer))
                .or_else(|| in_opt(length, hides_closer)),
            WordPart::ParamReplace {
                index,
                pattern,
                replacement,
                ..
            } => in_opt(index, hides_closer)
                .or_else(|| in_word(pattern, hides_closer))
                .or_else(|| in_word(replacement, hides_closer)),
            WordPart::ParamTransform { index, .. } | WordPart::BadTransform { index, .. } => {
                in_opt(index, hides_closer)
            }
            WordPart::Indirect { index, .. } => in_index(index, hides_closer),
            WordPart::IndirectOp { index, target, .. } => in_index(index, hides_closer)
                .or_else(|| target.first_scanned_arith(hides_closer)),
            WordPart::ArrayRef { index, .. } => match index {
                ArrayIndex::Index(w) => in_word(w, hides_closer),
                ArrayIndex::All | ArrayIndex::Star => None,
            },
            WordPart::ArraySlice { offset, length, .. } => in_word(offset, hides_closer)
                .or_else(|| in_opt(length, hides_closer)),
            WordPart::ArrayBulk { op, .. } => match op {
                BulkOp::Trim { pattern, .. } | BulkOp::Case { pattern, .. } => {
                    in_word(pattern, hides_closer)
                }
                BulkOp::Replace {
                    pattern,
                    replacement,
                    ..
                } => in_word(pattern, hides_closer)
                    .or_else(|| in_word(replacement, hides_closer)),
                BulkOp::Transform { .. } | BulkOp::BadTransform { .. } => None,
            },
            WordPart::ArrayOp { arg, .. } => in_word(arg, hides_closer),

            // Reached, but with nothing inside that a `$((` could hide in: the
            // parser has already resolved these to names and literal text.
            WordPart::Literal(_)
            | WordPart::SingleQuoted { .. }
            | WordPart::Param { .. }
            | WordPart::VarNames { .. }
            | WordPart::Length(_)
            | WordPart::ArrayKeys { .. }
            // `BadSubst` holds unparsed source, so a `$((` in it is text rather
            // than a node. bash does see it (`${x!$(( #5 ))}` reports "no
            // closing" rather than "bad substitution"), but finding it here
            // would mean re-scanning raw source — see the note above.
            | WordPart::BadSubst(_) => None,

            // Not reached: raised by the substitution's own expansion instead.
            WordPart::CommandSub { .. } | WordPart::ProcSub { .. } => None,
        }
    }

    /// Rename the parameter a scalar modifier works on.
    ///
    /// The modifiers of a `${!ref<op>}` are parsed against a placeholder name
    /// and only later learn which variable they really read, so both the parser
    /// (putting the referent back after parsing against a stand-in) and the
    /// expander (substituting the name the reference resolved to) have to swap
    /// the name out of an already-built node. Anything that is not one of those
    /// modifiers has no such name, and is left alone.
    pub fn set_param_name(&mut self, new_name: String) {
        match self {
            WordPart::ParamOp { name, .. }
            | WordPart::ParamTrim { name, .. }
            | WordPart::ParamSubstr { name, .. }
            | WordPart::ParamReplace { name, .. }
            | WordPart::ParamCase { name, .. }
            | WordPart::ParamTransform { name, .. } => *name = new_name,
            _ => {}
        }
    }
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
/// One rule covers every fragment: a plain offset. A `$( … )` body needed a
/// second one only for as long as osh re-read the *source* of the body — bash
/// re-reads its re-print, whose first command is on its line 1, so the blank and
/// continuation lines that made a plain offset wrong are not there to skip. See
/// [`CmdSubBody::Parsed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineMap {
    /// Reported line = raw line + this offset. `Offset(0)` is the identity, for
    /// a source that numbers its own lines from 1.
    Offset(u32),
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
    #[must_use]
    pub fn map(&self, raw: u32) -> u32 {
        let Self::Offset(base) = self;
        raw.saturating_add(*base)
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
        let Self::Offset(base) = self;
        Self::Offset(base.saturating_add(n))
    }

    /// The raw line a reported one came from, for echoing the offending source
    /// line back in a diagnostic. `None` when no raw line maps to it.
    #[must_use]
    pub fn unmap(&self, reported: u32) -> Option<u32> {
        let Self::Offset(base) = self;
        reported.checked_sub(*base)
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
/// if false; then echo $(for);     fi   # syntax error — the whole unit fails to parse
/// if false; then echo `for`;      fi   # silence — the body is never read
/// if false; then echo $(( for ) ); fi  # silence — nor is this one
/// ```
///
/// The third is a `$((` that turned out not to hold an expression, which bash
/// runs through the backtick's path — see [`CmdSubBody::ArithFallback`].
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
/// `src` is what the second pass re-reads.
///
/// `src` is **not the source**. `parse_comsub` disposes the first parse and
/// keeps `print_comsub`'s re-print of it (parse.y:4219–4233), so the bytes the
/// second pass reads are the deparse, not what was written. That is observable
/// wherever the two differ in *length*: a compound command re-prints over
/// several lines, so `$LINENO` after one inside the same body sits that much
/// lower.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdSubBody {
    /// `$( … )` — parsed with the enclosing source, then re-read at expansion
    /// time so a `shopt`/`alias` the body runs affects the rest of the body.
    Parsed {
        /// The eager parse: what the enclosing scan produced, and what
        /// `declare -f` re-prints.
        prog: Program,
        /// The re-print of `prog`, re-read one logical line at a time when the
        /// substitution is expanded. See [`crate::unparse::comsub_body`].
        src: Str,
        /// The line the closing `)` sits on, in the enclosing source. `src`'s
        /// own lines are numbered from `close_line - 1`, exactly as a backtick
        /// body's are — the re-print puts the body's first command on its line 1,
        /// so no rank rule is needed to skip the blank and continuation lines the
        /// source had.
        close_line: u32,
        /// The rest of the enclosing *word*, as the second pass sees it: what
        /// follows this substitution's `)` up to the end of the word, closing
        /// quotes included.
        ///
        /// bash does not hand `command_substitute` a body in isolation. At
        /// expansion time `expand_word_internal` is walking the stored word
        /// string and passes `extract_command_subst` the whole remainder of it,
        /// so the input that second parse reads is `src` + `)` + this — and
        /// when that parse fails, this is part of the line the diagnostic
        /// echoes back. `echo "A[$(⏎!⏎)]B" more args` reports `` `! )]B"' ``:
        /// the word's `]B` and its closing quote, and none of `more args`.
        ///
        /// `None` means there is **no** second parse — this body was never
        /// stored as a re-print, so nothing re-reads it. That is the case for
        /// every word the shell builds at *expansion* time rather than reading
        /// with the parser: `${x@P}` and `PS4` go through `expand_string`,
        /// whose `expand_word_internal` hands `command_substitute` the raw text
        /// straight off, so `parse_comsub` — and with it the re-print — never
        /// runs. `x=$'$(⏎!⏎)'; echo "${x@P}"` is therefore silent in bash where
        /// the same substitution written in the script is a syntax error.
        ///
        /// `Some` and empty is the common case for one that *is* re-read: a
        /// substitution ending its word. It is filled by a post-pass over the
        /// assembled word rather than where the part is built, because a part
        /// cannot see its own siblings — see `unparse::attach_comsub_tails`,
        /// which runs only on the parser's own words and so is also what draws
        /// this distinction.
        tail: Option<Str>,
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
        /// The line the closing backtick sits on, in the enclosing source. The
        /// body's own lines are numbered from `close_line - 1`.
        close_line: u32,
    },
    /// `$(( … )` — a `$((` whose body did not read as an arithmetic expression,
    /// so bash ran it as a command substitution instead.
    ///
    /// The fallback is `param_expand`'s (subst.c:10580): the `$((` scan only
    /// found the *extent*, and it is `chk_arithsub` at **expansion** time that
    /// asks whether the text is an expression at all. When it is not, bash hands
    /// that same text to `command_substitute` — the call a backtick body makes —
    /// so this behaves as [`CmdSubBody::Backtick`] does in everything that is
    /// observable, the line numbering included. What differs is only the
    /// spelling `declare -f` prints back, which is why it is a variant of its
    /// own rather than a backtick body with the delimiters swapped.
    ArithFallback {
        /// The body text, parsed afresh on every expansion. There is no separate
        /// verbatim form: nothing is unescaped on the way in, so the text that
        /// runs is also the text that is printed back.
        src: Str,
        /// The line the closing `)` sits on, in the enclosing source. The body's
        /// own lines are numbered from `close_line - 1`, the same plain offset a
        /// backtick body gets.
        close_line: u32,
    },
}

impl CmdSubBody {
    /// The parsed body, or `None` for a body bash does not read until the word
    /// is expanded (see the type docs).
    #[must_use]
    pub fn parsed(&self) -> Option<&Program> {
        match self {
            Self::Parsed { prog, .. } => Some(prog),
            Self::Backtick { .. } | Self::ArithFallback { .. } => None,
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
    /// `&> file` — redirect both stdout and stderr to the file,
    /// truncating/creating it.
    ///
    /// Not `>& file`: that stays a [`RedirectOp::DupOut`] whose target turns out
    /// to name a file, which is the shape bash keeps it in too
    /// (`r_duplicating_output_word`, converted to `r_err_and_out` only at
    /// redirection time). The two behave alike but do not *print* alike, and in
    /// posix mode they do not expand alike either.
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

/// How the word after a `<&`/`>&` was *written* — the sort bash's parser does
/// before it knows what the word expands to.
///
/// bash turns one operator into three redirect instructions here, and the
/// distinction outlives parsing: it decides how the redirect prints back
/// ([`crate::unparse`]), and in posix mode whether the word is globbed. What it
/// does *not* decide is the meaning of a [`DupSpelling::Word`] target, which is
/// settled at redirection time by what it expands to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupSpelling {
    /// A bare `-` — bash's `r_close_this`: close the descriptor.
    Close,
    /// A bare run of digits — bash's `r_duplicating_input`/`_output`.
    Number,
    /// Anything else: a filename, a *quoted* number, an expansion — bash's
    /// `r_duplicating_input_word`/`_output_word`. The classification is the
    /// parser's, so it never sees through quotes: `>&"2"` is a word.
    Word,
    /// `N>&M-` — bash's `r_move_output`/`r_move_input`: duplicate `M` onto `N`
    /// and then close `M`. The source is a bare run of digits.
    MoveNumber,
    /// `N>&$v-` — bash's `r_move_*_word`, the same thing with a source that has
    /// to be expanded first.
    MoveWord,
}

impl DupSpelling {
    /// Whether this spelling closes the source descriptor after duplicating it.
    #[must_use]
    pub fn is_move(self) -> bool {
        matches!(self, DupSpelling::MoveNumber | DupSpelling::MoveWord)
    }
}

/// The source word of a *move* redirection — `N>&M-` with the trailing `-`
/// taken off — or `None` if the target is not a move at all.
///
/// The `-` is part of the *source text*, never of an expansion: bash sorts the
/// word at parse time, so `>&$v-` is a move whatever `$v` holds, while `>&$v`
/// with `v=3-` is not one and ends up naming a file called `3-`.
///
/// The test is on the word's **raw last byte**, before quote removal, which is
/// blunter than it looks. `>&3"-"` is not a move — its last byte is the closing
/// quote — but `>&3\-` *is* one, because a backslash puts the dash last after
/// all. And there is no exception for a dash that is already doubled: `>&$v--`
/// is a move whose source is `$v-`. What keeps a lone `>&-` out of here is not
/// a rule of this function's but the lexer's, which takes an unquoted leading
/// `-` as a close token of its own before any word is collected.
#[must_use]
pub fn dup_move_source(target: &Word) -> Option<Word> {
    let (last, rest) = target.parts.split_last()?;
    let tail = match last {
        // The ordinary spelling: the `-` is plain text at the end of the word.
        WordPart::Literal(s) => {
            let s = s.strip_suffix(b"-")?;
            (!s.is_empty()).then(|| WordPart::Literal(s.to_vec()))
        }
        // `>&x\-`. bash's test is on the *raw* final byte of the word, with
        // quoting not yet removed, so an escaped dash ends a move just as well
        // as a bare one — see `make_redirection` (`make_cmd.c`), whose own
        // comment on the test is `/* Yuck */`:
        //
        //     wlen = strlen (w->word) - 1;
        //     if (w->word[wlen] == '-')
        //       { w->word[wlen] = '\0'; … }
        //
        // Taking the byte off a `\-` leaves the backslash dangling, and bash
        // then expands *that* as the source: quote removal drops it, so the
        // source is empty. An escaped run with nothing left in it is exactly
        // that dangling backslash — it expands to nothing and prints as `\`.
        //
        // Only `'-'` and `"-"` escape the rule, because their raw final byte is
        // the closing quote rather than the dash.
        WordPart::SingleQuoted {
            text,
            escaped: true,
        } => Some(WordPart::SingleQuoted {
            text: text.strip_suffix(b"-")?.to_vec(),
            escaped: true,
        }),
        _ => return None,
    };
    let mut parts = rest.to_vec();
    parts.extend(tail);
    // Unreachable by construction: the only word that could leave nothing
    // behind is a bare `-`, and the lexer takes that as a close token before a
    // word is ever collected (see the `<&-` arm of [`crate::lexer`]). Kept so
    // that a `Word` with no parts can never escape from here.
    if parts.is_empty() {
        return None;
    }
    Some(Word { parts })
}

/// Sort a `<&`/`>&` target the way bash's parser does. See [`DupSpelling`].
#[must_use]
pub fn dup_spelling(target: &Word) -> DupSpelling {
    if let Some(src) = dup_move_source(target) {
        return match dup_spelling_plain(&src) {
            DupSpelling::Number => DupSpelling::MoveNumber,
            _ => DupSpelling::MoveWord,
        };
    }
    dup_spelling_plain(target)
}

/// [`dup_spelling`] without the move check — the sort bash's parser does once it
/// has already taken any trailing `-` off.
fn dup_spelling_plain(target: &Word) -> DupSpelling {
    let [WordPart::Literal(s)] = target.parts.as_slice() else {
        return DupSpelling::Word;
    };
    if s.as_slice() == b"-" {
        DupSpelling::Close
    } else if !s.is_empty() && s.iter().all(u8::is_ascii_digit) {
        DupSpelling::Number
    } else {
        DupSpelling::Word
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The target word of the first redirect on the first simple command.
    fn first_redirect_target(src: &str) -> Word {
        let prog = crate::parser::parse(src.as_bytes()).expect("parse");
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("not a simple command: {src}");
        };
        sc.redirects[0].target.clone()
    }

    /// The words of the first simple command, `>&-`'s own `-` included.
    fn first_command_words(src: &str) -> Vec<Word> {
        let prog = crate::parser::parse(src.as_bytes()).expect("parse");
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("not a simple command: {src}");
        };
        sc.words.clone()
    }

    #[test]
    fn a_dup_targets_leading_dash_is_a_token_of_its_own_and_the_rest_is_a_word() {
        // bash's lexer returns the `-` after `<&`/`>&` before it ever collects a
        // word, so what follows starts a fresh one — `1>&--` closes fd 1 and
        // passes `-` along as an argument.
        let lit = |s: &str| Word {
            parts: vec![WordPart::Literal(s.as_bytes().to_vec())],
        };
        for (src, want) in [
            ("true 1>&--", vec![lit("true"), lit("-")]),
            ("true 1>&-x", vec![lit("true"), lit("x")]),
            ("true 1>&-abc", vec![lit("true"), lit("abc")]),
            // Blanks before it make no difference: bash skips them first.
            ("true 1>& --", vec![lit("true"), lit("-")]),
            // Nothing follows the close here, so there is no extra word.
            ("true 1>&-", vec![lit("true")]),
        ] {
            assert_eq!(first_command_words(src), want, "words of {src}");
            assert_eq!(
                dup_spelling(&first_redirect_target(src)),
                DupSpelling::Close,
                "spelling of {src}"
            );
        }

        // Only a *leading* dash. A `2` starts an ordinary word, which then
        // swallows the rest — so `1>&2-x` is one target word, not a close.
        assert_eq!(first_command_words("true 1>&2-x"), vec![lit("true")]);
    }

    #[test]
    fn a_move_is_sorted_by_how_the_dash_was_written_not_by_what_it_expands_to() {
        // The `-` belongs to the source text, so an expansion can neither supply
        // it nor take it away, and quoting it takes the move away entirely.
        for (src, want) in [
            ("true >&3-", DupSpelling::MoveNumber),
            ("true 1>&3-", DupSpelling::MoveNumber),
            ("true <&3-", DupSpelling::MoveNumber),
            ("true >&$v-", DupSpelling::MoveWord),
            ("true >&${v}-", DupSpelling::MoveWord),
            // Only the `-` itself has to be bare: `>&"3"-` is still a move, but
            // of the *word* `"3"`, by the same rule that keeps `>&"2"` a word.
            ("true >&\"3\"-", DupSpelling::MoveWord),
            // Quoted dash: not a move at all, just the filename `3-`. bash
            // tests the word's raw last *byte*, which for these is the closing
            // quote.
            ("true >&3\"-\"", DupSpelling::Word),
            ("true >&3'-'", DupSpelling::Word),
            // …but a backslash leaves the dash last after all, so this one is a
            // move — of the dangling `\`, which expands to nothing.
            (r"true >&3\-", DupSpelling::MoveWord),
            (r"true >&\-", DupSpelling::MoveWord),
            (r"true >&x\-", DupSpelling::MoveWord),
            // The dash cannot come out of an expansion.
            ("true >&$v", DupSpelling::Word),
            // A bare `-` is a close — but that is the lexer's doing, not this
            // function's: it returns the `-` as a token of its own, so `>&--`
            // is a close plus an argument and never reaches the word path.
            ("true >&-", DupSpelling::Close),
            ("true >&--", DupSpelling::Close),
            ("true >&3", DupSpelling::Number),
            // There is no exception for an already-doubled dash once the word
            // path *is* reached: only the final byte is taken off.
            ("true >&x--", DupSpelling::MoveWord),
            ("true >&$v--", DupSpelling::MoveWord),
            ("true >&\"x\"--", DupSpelling::MoveWord),
        ] {
            assert_eq!(
                dup_spelling(&first_redirect_target(src)),
                want,
                "spelling of {src}"
            );
        }
    }

    #[test]
    fn a_moves_source_is_the_word_with_the_dash_taken_off() {
        let src = dup_move_source(&first_redirect_target("true 1>&3-")).expect("a move");
        assert_eq!(src.parts, vec![WordPart::Literal(b"3".to_vec())]);

        // `>&"3"-` loses only the dash: the quoted part is left as it stands, so
        // the source still classifies as a word rather than a number.
        let src = dup_move_source(&first_redirect_target("true 1>&\"3\"-")).expect("a move");
        assert_eq!(dup_spelling_plain(&src), DupSpelling::Word);

        // Taking the dash off a `\-` leaves the backslash dangling, which is an
        // escaped run with nothing in it: it expands to the empty string and
        // prints back as `\`, so `1>&x\-` reads back exactly as written.
        let src = dup_move_source(&first_redirect_target(r"true 1>&x\-")).expect("a move");
        assert_eq!(
            src.parts,
            vec![
                WordPart::Literal(b"x".to_vec()),
                WordPart::SingleQuoted {
                    text: Vec::new(),
                    escaped: true
                },
            ]
        );
        // …and with nothing before it, the source is that backslash alone.
        let src = dup_move_source(&first_redirect_target(r"true 1>&\-")).expect("a move");
        assert_eq!(
            src.parts,
            vec![WordPart::SingleQuoted {
                text: Vec::new(),
                escaped: true
            }]
        );

        // Nothing to take off. A bare `-` is not in this list because it cannot
        // get here: the lexer takes it as a close token before a word is built.
        for src in ["true 1>&3", "true 1>&$v", "true 1>&3\"-\""] {
            assert!(
                dup_move_source(&first_redirect_target(src)).is_none(),
                "{src} is not a move"
            );
        }
    }

    /// The command word of the first simple command in `src`.
    fn first_word(src: &str) -> Word {
        let prog = crate::parser::parse(src.as_bytes()).expect("parse");
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
