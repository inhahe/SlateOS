//! Source pretty-printer (unparser) for the OSH AST.
//!
//! Reconstructs re-parseable shell source from a parsed [`Program`] /
//! [`FunctionDef`]. This is what backs `declare -f NAME` / `type NAME` (which
//! print a function's body) and bare `set` (which lists function definitions
//! alongside variables), so that a function defined in the shell can be dumped
//! as text and fed back in.
//!
//! The goal is *faithful, re-parseable* output — not a byte-for-byte match of
//! bash's own formatter. Bodies are printed one statement per line with 4-space
//! indentation per nesting level; conditions and other sub-lists are rendered
//! inline with `;` separators.
//!
//! Here-documents are the one construct that cannot be rendered in place: the
//! operator sits mid-line and the body has to start on the line after. They are
//! parked inline behind marker characters and lifted out by [`flush_here_docs`]
//! once the line is complete — which is where this deliberately diverges from
//! bash, whose printer defers bodies to the end of the enclosing *statement*
//! and so emits output that no longer re-parses when the here-doc is inside an
//! `if` condition. See known-issues TD-OILS-DECLAREF-QUIRKS.

use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseMode, CmdSubBody,
    Command, DupSpelling, dup_spelling,
    CondExpr, Item, ParamOp, Pipeline, Program, Redirect, RedirectOp, ReplaceAnchor,
    SimpleCommand, Word, WordPart,
};
use crate::bfmt;
use crate::bytes::{self, BStr, Str, StrBuf as _};

/// Deparse a `${…}` case-modification operator: `^`/`^^` (upper), `,`/`,,`
/// (lower), `~`/`~~` (toggle); doubled when `all`.
fn case_op_src(mode: CaseMode, all: bool) -> &'static str {
    match (mode, all) {
        (CaseMode::Upper, true) => "^^",
        (CaseMode::Upper, false) => "^",
        (CaseMode::Lower, true) => ",,",
        (CaseMode::Lower, false) => ",",
        (CaseMode::Toggle, true) => "~~",
        (CaseMode::Toggle, false) => "~",
    }
}

/// How deep a body is nested, *and* the shape its indentation takes: level `n`
/// is indented `base + step * n` spaces.
///
/// bash has two function printers with two different shapes. `declare -f` (and
/// `type`) indents four spaces per nesting level — [`Indent::DECLARE`]. The one
/// that encodes an exported function into `BASH_FUNC_<name>%%` puts exactly one
/// space at every depth, however deep — [`Indent::EXPORTED`]. The difference
/// cannot be applied as a post-pass over the finished text, because a newline
/// inside a string literal is emitted verbatim at column 0 in *both* forms and
/// a line-based re-indent would corrupt it. So the shape travels with the depth
/// through the printer, rather than sitting in a global the printer consults.
#[derive(Clone, Copy)]
struct Indent {
    /// Spaces every level carries, however shallow.
    base: usize,
    /// Further spaces per nesting level.
    step: usize,
    /// The current nesting depth.
    level: usize,
}

impl Indent {
    /// `declare -f` / `type` / `set` output: four spaces per nesting level.
    const DECLARE: Self = Self { base: 0, step: 4, level: 0 };
    /// The exported-function encoding: one space at every depth.
    const EXPORTED: Self = Self { base: 1, step: 0, level: 0 };

    /// The leading whitespace a line at this depth carries.
    fn spaces(self) -> Str {
        b" ".repeat(self.base.saturating_add(self.step.saturating_mul(self.level)))
    }
}

/// Descend `n` nesting levels, keeping the shape — so the printer's recursive
/// calls read as the plain `level + 1` they were before the shape existed.
impl std::ops::Add<usize> for Indent {
    type Output = Self;

    fn add(self, n: usize) -> Self {
        Self { level: self.level.saturating_add(n), ..self }
    }
}

/// Render a function definition in bash's `declare -f` form:
///
/// ```text
/// name ()
/// {
///     body
/// }
/// ```
#[must_use]
pub fn unparse_function(name: BStr<'_>, body: &Program, redirects: &[Redirect]) -> Str {
    let mut s = Str::new();
    s.push_str(name);
    // bash prints the opening brace on its own line with a trailing space
    // (`{ \n`), matching `declare -f` / `type` output byte-for-byte.
    s.push_str(" () \n{ \n");
    let shape = Indent::DECLARE;
    let inner = program_block(body, shape + 1, false);
    if inner.is_empty() {
        // An empty body still needs a no-op so it re-parses.
        s.push_str(&(shape + 1).spaces());
        s.push(b':');
        s.push(b'\n');
    } else {
        s.push_str(&inner);
        if !inner.ends_with(b"\n") {
            s.push(b'\n');
        }
    }
    // Redirections attached to the definition (`f() { …; } >log`) render on
    // the closing-brace line: `} > log`, matching bash's `declare -f`.
    s.push(b'}');
    for r in redirects {
        s.push(b' ');
        s.push_str(&redirect_src(r));
    }
    s.push(b'\n');
    // A here-doc attached to the *definition* (`f() { …; } <<EOF`) is still
    // parked on the closing-brace line; nothing inside has markers left.
    flush_here_docs(&s)
}

/// Render a function body in the form bash puts into the environment for an
/// exported function — the value side of `BASH_FUNC_<name>%%`.
///
/// This is bash's `named_function_string(NULL, cmd, 0)`, which differs from the
/// `declare -f` form ([`unparse_function`]) in exactly two ways: the name and
/// the newline before `{` are dropped (`() { ` instead of `NAME () \n{ \n`),
/// and every nesting level is indented one space rather than four-per-level.
/// So `f() { echo hi; }` encodes as `() {  echo hi\n}` — two spaces, one from
/// the header and one from the body's indent.
#[must_use]
pub fn unparse_function_exported(body: &Program, redirects: &[Redirect]) -> Str {
    let shape = Indent::EXPORTED;
    let mut s = b"() { ".to_vec();
    let inner = program_block(body, shape + 1, false);
    if inner.is_empty() {
        s.push_str(&(shape + 1).spaces());
        s.push(b':');
        s.push(b'\n');
    } else {
        s.push_str(&inner);
        if !inner.ends_with(b"\n") {
            s.push(b'\n');
        }
    }
    s.push(b'}');
    for r in redirects {
        s.push(b' ');
        s.push_str(&redirect_src(r));
    }
    flush_here_docs(&s)
}

/// Render a whole program as an indented block: one item per line at `level`.
///
/// `terminate_last` controls the trailing separator on the final statement, to
/// match bash's `declare -f` deparser: a compound *clause* body (`then`/`else`/
/// `do`) terminates every statement — including the last — with `;`, whereas a
/// group body (`{ … }`, a subshell, the function body itself, and `case`
/// clauses) leaves the last statement unterminated. Non-final statements always
/// take a `;` separator (a backgrounded statement's ` &` is its own separator).
fn program_block(prog: &Program, level: Indent, terminate_last: bool) -> Str {
    let mut out = Str::new();
    let n = prog.items.len();
    for (i, item) in prog.items.iter().enumerate() {
        // bash keeps a backgrounded statement and the one that follows it on the
        // same line (`a & b & c`), using ` & ` as an inline connector. So only
        // indent an item that begins a fresh line: the first, or one whose
        // predecessor was not backgrounded. (TD-OILS-DECLAREF-QUIRKS item 3.)
        let mut stmt = Str::new();
        if i == 0 || !prog.items[i - 1].background {
            stmt.push_str(&level.spaces());
        }
        stmt.push_str(&item_stmt(item, level));
        // A here-document parked on the statement's *last* line replaces the
        // statement's separator: bash drops the `;` (the body has to start on
        // the very next line) and leaves a blank line after the delimiter.
        let trailing_here = last_line_has_here_doc(&stmt);
        let is_last = i + 1 == n;
        if item.background {
            // `item_stmt` already emitted the trailing ` &`; connect the next
            // statement inline with a space, and only break the line when this
            // backgrounded item is the last in the block.
            stmt.push(if is_last { b'\n' } else { b' ' });
        } else {
            // Separate with `;`, terminating the last one only in clause-body
            // context (`then`/`else`/`do`); group bodies leave it unterminated.
            if (!is_last || terminate_last) && !trailing_here {
                stmt.push(b';');
            }
            stmt.push(b'\n');
        }
        out.push_str(&flush_here_docs(&stmt));
        if trailing_here {
            out.push(b'\n');
        }
    }
    out
}

/// Render a program inline (single logical line), items joined by `; `. Used for
/// conditions (`if <here>; then …`) and command substitutions.
#[must_use]
pub fn program_inline(prog: &Program) -> Str {
    let mut parts: Vec<Str> = Vec::new();
    for item in &prog.items {
        let mut s = and_or_inline(&item.list);
        if item.background {
            s.push_str(" &");
        }
        parts.push(s);
    }
    bytes::join(&parts, b"; ")
}

/// One statement (and-or list, plus a trailing ` &` when backgrounded). The
/// first line carries no leading indent (the caller supplies it); nested lines
/// are indented to `level`.
fn item_stmt(item: &Item, level: Indent) -> Str {
    let mut s = and_or_block(&item.list, level);
    if item.background {
        s.push_str(" &");
    }
    s
}

/// And-or list where the first pipeline may be a multi-line compound command.
fn and_or_block(ao: &AndOr, level: Indent) -> Str {
    let mut s = pipeline_block(&ao.first, level);
    for (op, pl) in &ao.rest {
        s.push_str(match op {
            AndOrOp::And => " && ",
            AndOrOp::Or => " || ",
        });
        s.push_str(&pipeline_block(pl, level));
    }
    s
}

/// And-or list as bash exposes it in a trap's stored command text: rendered
/// inline, with any here-document body flushed onto its own lines.
#[must_use]
pub fn and_or_src(ao: &AndOr) -> Str {
    flush_here_docs(&and_or_inline(ao))
}

/// A single command rendered inline, the way `jobs` shows the command a job was
/// started from. `and_or_src` is the usual entry point — this one exists for the
/// forms that are a bare [`Command`] with no list around them, such as `coproc`.
#[must_use]
pub fn command_src(cmd: &Command) -> Str {
    flush_here_docs(&command_inline(cmd))
}

/// And-or list rendered strictly inline (for conditions / command subs). Any
/// here-document body stays parked for the caller to flush.
fn and_or_inline(ao: &AndOr) -> Str {
    let mut s = pipeline_src(&ao.first);
    for (op, pl) in &ao.rest {
        s.push_str(match op {
            AndOrOp::And => " && ",
            AndOrOp::Or => " || ",
        });
        s.push_str(&pipeline_src(pl));
    }
    s
}

fn pipeline_prefix(pl: &Pipeline) -> Str {
    let mut s = Str::new();
    if pl.timed {
        s.push_str(if pl.time_posix { "time -p " } else { "time " });
    }
    if pl.negated {
        s.push_str("! ");
    }
    s
}

/// Pipeline where each command may be a multi-line compound command.
fn pipeline_block(pl: &Pipeline, level: Indent) -> Str {
    let mut s = pipeline_prefix(pl);
    let cmds: Vec<Str> = pl.commands.iter().map(|c| command_block(c, level)).collect();
    s.push_str(&bytes::join(&cmds, b" | "));
    s
}

/// Pipeline rendered strictly inline.
fn pipeline_src(pl: &Pipeline) -> Str {
    let mut s = pipeline_prefix(pl);
    let cmds: Vec<Str> = pl.commands.iter().map(command_inline).collect();
    s.push_str(&bytes::join(&cmds, b" | "));
    s
}

/// Render an `if`/`elif`/`else` chain in bash's `declare -f` block form.
///
/// bash's deparser does **not** emit `elif`: it rewrites every `elif` into a
/// nested `else { if … fi; }`, indenting one level deeper per `elif` and
/// terminating each inner `fi` with `;` (the outermost `fi` is left for the
/// caller to terminate). This matches `declare -f`/`type` byte-for-byte. See
/// known-issues.md TD-OILS-DECLAREF-QUIRKS item 1.
fn render_if(
    cond: &Program,
    body: &Program,
    elifs: &[(Program, Program)],
    else_body: Option<&Program>,
    level: Indent,
) -> Str {
    let mut s = b"if ".to_vec();
    s.push_str(&program_inline(cond));
    s.push_str("; then\n");
    s.push_str(&program_block(body, level + 1, true));
    if let Some(((econd, ebody), rest)) = elifs.split_first() {
        // `elif …` becomes `else\n  if … fi;` one indent level deeper.
        s.push_str(&level.spaces());
        s.push_str("else\n");
        s.push_str(&(level + 1).spaces());
        s.push_str(&render_if(econd, ebody, rest, else_body, level + 1));
        s.push_str(";\n");
        s.push_str(&level.spaces());
        s.push_str("fi");
    } else if let Some(eb) = else_body {
        s.push_str(&level.spaces());
        s.push_str("else\n");
        s.push_str(&program_block(eb, level + 1, true));
        s.push_str(&level.spaces());
        s.push_str("fi");
    } else {
        s.push_str(&level.spaces());
        s.push_str("fi");
    }
    s
}

/// Render a command as a (possibly multi-line) block. The first line has no
/// leading indent; continuation lines are indented at `level`, bodies at
/// `level + 1`.
fn command_block(cmd: &Command, level: Indent) -> Str {
    match cmd {
        Command::Simple(sc) => simple_inline(sc),
        Command::If(c) => render_if(&c.cond, &c.body, &c.elifs, c.else_body.as_ref(), level),
        Command::Loop(c) => {
            // `while`/`until` keep `do` on the same line as the condition
            // (`while COND; do`), unlike `for`/`select` (see below).
            let mut s = if c.until { b"until ".to_vec() } else { b"while ".to_vec() };
            s.push_str(&program_inline(&c.cond));
            s.push_str("; do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&level.spaces());
            s.push_str("done");
            s
        }
        Command::For(c) => {
            // bash's deparser puts `do` on its own line for `for` (the word list
            // is terminated with `;`, then `do` at the loop's indent level).
            let mut s = bfmt![b"for ", &c.var];
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(b' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str(";\n");
            s.push_str(&level.spaces());
            s.push_str("do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&level.spaces());
            s.push_str("done");
            s
        }
        Command::ForArith(c) => {
            // `for ((init; cond; upd))` with no inner-paren padding and `do` on
            // its own line, matching bash.
            let mut s = bfmt![b"for ((", &c.init, b"; ", &c.cond, b"; ", &c.update, b"))\n"];
            s.push_str(&level.spaces());
            s.push_str("do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&level.spaces());
            s.push_str("done");
            s
        }
        Command::Select(c) => {
            let mut s = bfmt![b"select ", &c.var];
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(b' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str(";\n");
            s.push_str(&level.spaces());
            s.push_str("do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&level.spaces());
            s.push_str("done");
            s
        }
        Command::Function(f) => {
            // A function defined *inside* another function body reaches
            // `command_block` (top-level definitions go through
            // `unparse_function`). bash's deparser prefixes every such nested
            // definition with the `function` keyword — regardless of the source
            // syntax — while top-level defs omit it. See known-issues.md
            // TD-OILS-DECLAREF-QUIRKS item 4.
            let mut s = bfmt![b"function ", &f.name, b" () \n"];
            s.push_str(&level.spaces());
            s.push_str("{ \n");
            s.push_str(&program_block(&f.body, level + 1, false));
            s.push_str(&level.spaces());
            s.push(b'}');
            for r in &f.redirects {
                s.push(b' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
        Command::Case(c) => {
            // bash prints `case WORD in ` with a trailing space before the
            // newline.
            let mut s = bfmt![b"case ", &word_src(&c.word), b" in \n"];
            for item in &c.items {
                let pats: Vec<Str> = item.patterns.iter().map(word_src).collect();
                s.push_str(&(level + 1).spaces());
                s.push_str(&bytes::join(&pats, b"|"));
                s.push_str(")\n");
                s.push_str(&program_block(&item.body, level + 2, false));
                s.push_str(&(level + 1).spaces());
                s.push_str(match item.term {
                    crate::ast::CaseTerm::Break => ";;",
                    crate::ast::CaseTerm::FallThrough => ";&",
                    crate::ast::CaseTerm::ContinueMatch => ";;&",
                });
                s.push(b'\n');
            }
            s.push_str(&level.spaces());
            s.push_str("esac");
            s
        }
        Command::BraceGroup(prog) => {
            // bash prints the opening brace with a trailing space (`{ `).
            let mut s = b"{ \n".to_vec();
            s.push_str(&program_block(prog, level + 1, false));
            s.push_str(&level.spaces());
            s.push(b'}');
            s
        }
        Command::Subshell(prog) => {
            // bash's deparser keeps a subshell body at the *same* indent as the
            // `(`, glues the first statement to `( ` and the last to ` )`
            // (`( echo a;\n<ind>echo b )`) rather than using a deeper indented
            // block. Render the body as a group (last statement unterminated),
            // then strip the first line's indent and the trailing newline and
            // wrap in `( … )`. (TD-OILS-DECLAREF-QUIRKS item 2.)
            let body = program_block(prog, level, false);
            if body.is_empty() {
                return b"( )".to_vec();
            }
            let indent = level.spaces();
            let trimmed = body.strip_prefix(indent.as_slice()).unwrap_or(body.as_slice());
            let trimmed = trimmed.strip_suffix(b"\n").unwrap_or(trimmed);
            bfmt![b"( ", trimmed, b" )"]
        }
        Command::Cond(expr) => cond_command_src(expr),
        Command::Arith(text) => bfmt![b"((", text, b"))"],
        Command::Coproc { name, body } => {
            let mut s = b"coproc ".to_vec();
            if let Some(n) = name {
                s.push_str(n);
                s.push(b' ');
            }
            s.push_str(&command_block(body, level));
            s
        }
        Command::Redirected { inner, redirects } => {
            let mut s = command_block(inner, level);
            for r in redirects {
                s.push(b' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
    }
}

/// Render a command strictly inline (compound commands still use `;` separators,
/// which is valid bash — just not multi-line).
fn command_inline(cmd: &Command) -> Str {
    match cmd {
        Command::Simple(sc) => simple_inline(sc),
        Command::If(c) => {
            let mut s = b"if ".to_vec();
            s.push_str(&program_inline(&c.cond));
            s.push_str("; then ");
            s.push_str(&program_inline(&c.body));
            s.push(b';');
            for (econd, ebody) in &c.elifs {
                s.push_str(" elif ");
                s.push_str(&program_inline(econd));
                s.push_str("; then ");
                s.push_str(&program_inline(ebody));
                s.push(b';');
            }
            if let Some(eb) = &c.else_body {
                s.push_str(" else ");
                s.push_str(&program_inline(eb));
                s.push(b';');
            }
            s.push_str(" fi");
            s
        }
        Command::Loop(c) => {
            let mut s = if c.until { b"until ".to_vec() } else { b"while ".to_vec() };
            s.push_str(&program_inline(&c.cond));
            s.push_str("; do ");
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::For(c) => {
            let mut s = bfmt![b"for ", &c.var];
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(b' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str("; do ");
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::ForArith(c) => {
            let mut s = bfmt![b"for (( ", &c.init, b"; ", &c.cond, b"; ", &c.update, b" )); do "];
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::Select(c) => {
            let mut s = bfmt![b"select ", &c.var];
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(b' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str("; do ");
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::Function(f) => {
            let mut s = bfmt![&f.name, b" () { "];
            s.push_str(&program_inline(&f.body));
            s.push_str("; }");
            for r in &f.redirects {
                s.push(b' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
        Command::Case(c) => {
            let mut s = bfmt![b"case ", &word_src(&c.word), b" in "];
            for item in &c.items {
                let pats: Vec<Str> = item.patterns.iter().map(word_src).collect();
                s.push_str(&bytes::join(&pats, b"|"));
                s.push_str(") ");
                s.push_str(&program_inline(&item.body));
                s.push(b' ');
                s.push_str(match item.term {
                    crate::ast::CaseTerm::Break => ";;",
                    crate::ast::CaseTerm::FallThrough => ";&",
                    crate::ast::CaseTerm::ContinueMatch => ";;&",
                });
                s.push(b' ');
            }
            s.push_str("esac");
            s
        }
        Command::BraceGroup(prog) => bfmt![b"{ ", &program_inline(prog), b"; }"],
        Command::Subshell(prog) => bfmt![b"( ", &program_inline(prog), b" )"],
        Command::Cond(expr) => cond_command_src(expr),
        Command::Arith(text) => bfmt![b"((", text, b"))"],
        Command::Coproc { name, body } => {
            let mut s = b"coproc ".to_vec();
            if let Some(n) = name {
                s.push_str(n);
                s.push(b' ');
            }
            s.push_str(&command_inline(body));
            s
        }
        Command::Redirected { inner, redirects } => {
            let mut s = command_inline(inner);
            for r in redirects {
                s.push(b' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
    }
}

/// Reconstruct the source text of a simple command (assignments, words,
/// redirections) — used for `$BASH_COMMAND` in DEBUG/ERR traps. One line,
/// except that a here-document body is flushed onto its own lines after it,
/// which is what bash stores there too.
#[must_use]
pub fn simple_src(sc: &SimpleCommand) -> Str {
    flush_here_docs(&simple_inline(sc))
}

/// A simple command rendered on one line, with any here-document body left
/// parked for the caller to flush.
fn simple_inline(sc: &SimpleCommand) -> Str {
    let mut parts: Vec<Str> = Vec::new();
    for a in &sc.assignments {
        parts.push(assignment_src(a));
    }
    // A declaration builtin's array-literal operands live outside `words` but were
    // written among them, so splice each back in at the position the parser
    // recorded: `declare -x SC=1 arr=(9) SD=2` must read back in that order, not
    // with `arr=(9)` shunted to the end.
    let mut decl = sc.decl_arrays.iter().peekable();
    for (i, w) in sc.words.iter().enumerate() {
        while let Some(d) = decl.next_if(|d| d.word_index <= i) {
            parts.push(assignment_src(&d.assign));
        }
        parts.push(word_src(w));
    }
    for d in decl {
        parts.push(assignment_src(&d.assign));
    }
    let mut s = bytes::join(&parts, b" ");
    for r in &sc.redirects {
        if !s.is_empty() {
            s.push(b' ');
        }
        s.push_str(&redirect_src(r));
    }
    s
}

pub(crate) fn assignment_src(a: &Assignment) -> Str {
    let mut s = a.name.as_bytes().to_vec();
    if let Some(idx) = &a.index {
        s.push(b'[');
        s.push_str(&word_src(idx));
        s.push(b']');
    }
    s.push_str(if a.append { "+=" } else { "=" });
    match &a.value {
        AssignRhs::Scalar(w) => s.push_str(&word_src(w)),
        AssignRhs::Array(elems) => {
            s.push(b'(');
            let items: Vec<Str> = elems
                .iter()
                .map(|e| match e {
                    ArrayElem::Positional(w) => word_src(w),
                    ArrayElem::Keyed { index, value, append } => bfmt![
                        b"[",
                        &word_src(index),
                        if *append { b"]+=".as_slice() } else { b"]=".as_slice() },
                        &word_src(value)
                    ],
                })
                .collect();
            s.push_str(&bytes::join(&items, b" "));
            s.push(b')');
        }
    }
    s
}

fn redirect_src(r: &Redirect) -> Str {
    // A varfd prefix `{name}` replaces the numeric fd on the operators that
    // accept one (`{fd}>`, `{fd}>>`, `{fd}<`, `{fd}>&…`).
    if let Some(name) = &r.varfd {
        // File-target operators take a space before the target (`{fd}> log`);
        // fd-duplication operators stay tight (`{fd}>&2`).
        let (op, sep) = match r.op {
            RedirectOp::Write => (">", " "),
            RedirectOp::Clobber => (">|", " "),
            RedirectOp::Append => (">>", " "),
            RedirectOp::Read => ("<", " "),
            RedirectOp::ReadWrite => ("<>", " "),
            RedirectOp::DupOut => (">&", ""),
            RedirectOp::DupIn => ("<&", ""),
            // `{name}` never pairs with here-docs / `&>`; fall back to the plain
            // form for those (unreachable in practice).
            _ => return redirect_src_plain(r),
        };
        return bfmt![b"{", name, b"}", op, sep, &word_src(&r.target)];
    }
    redirect_src_plain(r)
}

fn redirect_src_plain(r: &Redirect) -> Str {
    // bash's `declare -f` deparser separates a redirection operator from a
    // *file/word* target with a single space (`> log`, `2>> err`, `&> both`,
    // `< in`), but writes fd-*duplication* operators tight against their fd
    // (`1>&2`, `0<&3`). Here-strings already carry their own space.
    match r.op {
        RedirectOp::Write => fd_prefixed(r.fd, 1, ">", " ", &word_src(&r.target)),
        RedirectOp::Clobber => fd_prefixed(r.fd, 1, ">|", " ", &word_src(&r.target)),
        RedirectOp::Append => fd_prefixed(r.fd, 1, ">>", " ", &word_src(&r.target)),
        RedirectOp::WriteBoth => bfmt![b"&> ", &word_src(&r.target)],
        RedirectOp::AppendBoth => bfmt![b"&>> ", &word_src(&r.target)],
        RedirectOp::Read => fd_prefixed(r.fd, 0, "<", " ", &word_src(&r.target)),
        // `<>` opens fd 0 by default, but bash's `declare -f` deparser elides the
        // source fd only for fd 1 (`1<> f` → `<> f`), showing it otherwise
        // (`<> f` → `0<> f`, `3<> f` stays `3<> f`). Match that with default 1.
        RedirectOp::ReadWrite => fd_prefixed(r.fd, 1, "<>", " ", &word_src(&r.target)),
        RedirectOp::DupOut => dup_src(r, 1, ">&"),
        RedirectOp::DupIn => dup_src(r, 0, "<&"),
        RedirectOp::HereStr => bfmt![b"<<< ", &word_src(&r.target)],
        RedirectOp::HereDoc => here_doc_src(r),
    }
}

/// A `<&`/`>&` redirect, printed the way bash's `print_redirection` does — which
/// turns on the *parse-time* shape of the target word, not on what it expands
/// to, because bash's parser has already sorted the word into one of three
/// redirect instructions by then and the printer has a case for each:
///
/// * a bare `-` is `r_close_this`, printed `N>&-` with the fd always shown and
///   the operator always `>&` — so `<&-` comes back out as `0>&-`, direction
///   and all;
/// * a bare run of digits is `r_duplicating_input`/`r_duplicating_output`, whose
///   printer likewise always shows the fd (`>&2` → `1>&2`);
/// * anything else — a filename, a *quoted* number, an expansion — is the
///   `…_word` form, and only that form elides the fd when it is the operator's
///   default (`>& out` → `>&out` and `<& in` → `<&in`, but `2>& out` stays
///   `2>&out` and `>& "2"` stays `>&"2"`).
fn dup_src(r: &Redirect, default_fd: i32, op: &str) -> Str {
    let target = word_src(&r.target);
    match dup_spelling(&r.target) {
        DupSpelling::Close => bfmt![r.fd, b">&-"],
        DupSpelling::Number => bfmt![r.fd, op, &target],
        DupSpelling::Word => fd_prefixed(r.fd, default_fd, op, "", &target),
    }
}

/// Bracketing markers for a here-document body deferred to the end of the
/// rendered line (see [`flush_here_docs`]).
///
/// A here-document is written across two places at once: the operator sits in
/// the middle of a command line, while the body has to start on the line after
/// it. The unparser builds its output bottom-up as plain strings, so the body
/// is parked inline behind these markers where the operator was rendered, and
/// lifted out to the end of the line once the line is complete. Control
/// characters are used because shell source has no use for them, so a body can
/// never contain one that would be mistaken for a marker.
const HD_OPEN: u8 = 0x01;
const HD_CLOSE: u8 = 0x02;

/// `<<DELIM` / `<<-'DELIM'` plus the body, parked for [`flush_here_docs`].
///
/// bash normalises every quoted spelling of the delimiter (`'D'`, `"D"`, `\D`)
/// to the single-quoted form, and prints a `<<-` body already stripped of its
/// leading tabs — which is exactly what the lexer stored — so the reprinted
/// `<<-` still re-reads as the same bytes.
fn here_doc_src(r: &Redirect) -> Str {
    let Some(hd) = &r.here else {
        // A here-doc redirect always carries its delimiter from the parser.
        // Nothing else can deliver the body, so fall back to the here-string
        // form, which at least feeds stdin the same bytes.
        return bfmt![b"<<< ", &word_src(&r.target)];
    };
    let delim = if hd.quoted {
        bfmt![b"'", &hd.delim, b"'"]
    } else {
        hd.delim.clone()
    };
    let dash: BStr<'static> = if hd.strip { b"-" } else { b"" };
    let mut body = word_src(&r.target);
    // Every body line the lexer captured ended in a newline; an empty body
    // needs none, and a body whose final line somehow lost its newline still
    // must not run into the delimiter.
    if !body.is_empty() && !body.ends_with(b"\n") {
        body.push(b'\n');
    }
    let fd = if r.fd == 0 { Str::new() } else { bfmt![r.fd] };
    bfmt![
        &fd, b"<<", dash, &delim, HD_OPEN, &body, &hd.delim, b"\n", HD_CLOSE
    ]
}

/// Move every parked here-document body ([`HD_OPEN`]…[`HD_CLOSE`]) out of the
/// line it was rendered on and re-emit it just after that line.
///
/// bash instead defers bodies to the end of the enclosing *statement*, which
/// for a here-doc inside an `if` condition emits a function body that no longer
/// re-parses (the body swallows the `then` clause). Flushing per line keeps the
/// output readable and correct; the divergence is recorded in known-issues.md.
fn flush_here_docs(text: BStr<'_>) -> Str {
    if !text.contains(&HD_OPEN) {
        return text.to_vec();
    }
    let mut out = Str::with_capacity(text.len());
    // Bodies parked on the line being copied out, waiting for its newline. A
    // body spans lines of its own, so the scan cannot work line by line: it
    // walks the text looking for whichever comes first, a marker or a newline.
    let mut parked = Str::new();
    let mut rest = text;
    while let Some(i) = rest.iter().position(|&b| b == HD_OPEN || b == b'\n') {
        out.extend_from_slice(rest.get(..i).unwrap_or_default());
        if rest.get(i) == Some(&b'\n') {
            out.push(b'\n');
            out.append(&mut parked);
            rest = rest.get(i + 1..).unwrap_or_default();
        } else if let Some(end) = marker_end(rest, i) {
            parked.extend_from_slice(rest.get(i + 1..end).unwrap_or_default());
            rest = rest.get(end + 1..).unwrap_or_default();
        } else {
            // Unterminated marker: nothing sane to do but keep the text as-is.
            out.extend_from_slice(rest.get(i..).unwrap_or_default());
            return out;
        }
    }
    out.extend_from_slice(rest);
    if !parked.is_empty() {
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.append(&mut parked);
    }
    out
}

/// Index of the [`HD_CLOSE`] that ends the parked body opened at `open`.
fn marker_end(text: BStr<'_>, open: usize) -> Option<usize> {
    text.get(open..)?
        .iter()
        .position(|&b| b == HD_CLOSE)
        .map(|e| open + e)
}

/// Whether the *last* line of a rendered statement carries a here-document,
/// ignoring the newlines inside any parked body.
///
/// That is the case where the statement's `;` separator has to give way: the
/// body must start on the very next line, so there is nowhere to put one.
fn last_line_has_here_doc(text: BStr<'_>) -> bool {
    let mut found = false;
    let mut rest = text;
    while let Some(i) = rest.iter().position(|&b| b == HD_OPEN || b == b'\n') {
        if rest.get(i) == Some(&b'\n') {
            found = false;
            rest = rest.get(i + 1..).unwrap_or_default();
        } else if let Some(end) = marker_end(rest, i) {
            found = true;
            rest = rest.get(end + 1..).unwrap_or_default();
        } else {
            return true;
        }
    }
    found
}

/// `fd` prefix only when it differs from the operator's default (`>`→1, `<`→0);
/// `sep` is inserted between the operator and target (a space for file targets,
/// empty for fd-duplication operators).
fn fd_prefixed(fd: i32, default: i32, op: &str, sep: &str, target: BStr<'_>) -> Str {
    if fd == default {
        bfmt![op, sep, target]
    } else {
        bfmt![fd, op, sep, target]
    }
}

/// Reconstruct source text for a `[[ … ]]` conditional, brackets included.
///
/// Shared by both command printers and by the DEBUG trap, which announces a
/// conditional with this same spelling — normalised whitespace and a bare word
/// written out as the `-n` test it means, but every operator, quote and
/// unexpanded expansion kept as it was written.
#[must_use]
pub fn cond_command_src(expr: &CondExpr) -> Str {
    bfmt![b"[[ ", &cond_src(expr), b" ]]"]
}

fn cond_src(expr: &CondExpr) -> Str {
    match expr {
        // A bare word is a non-empty test, and bash prints it as the `-n` it
        // means rather than as written — one of the few places its printer
        // normalises instead of echoing the source.
        CondExpr::Word(w) => bfmt![b"-n ", &word_src(w)],
        // Operators print with the spelling they were written with, which the
        // AST kept for exactly this reason: `[[ -h f ]]` and `[[ a = b ]]` must
        // not come back out as `-L` and `==`.
        CondExpr::Unary(op, w) => bfmt![&op.text, b" ", &word_src(w)],
        CondExpr::Binary(l, op, r) => {
            bfmt![&word_src(l), b" ", &op.text, b" ", &word_src(r)]
        }
        CondExpr::Regex(l, r) => bfmt![&word_src(l), b" =~ ", &word_src(r)],
        CondExpr::Not(e) => bfmt![b"! ", &cond_src(e)],
        CondExpr::And(a, b) => bfmt![&cond_src(a), b" && ", &cond_src(b)],
        CondExpr::Or(a, b) => bfmt![&cond_src(a), b" || ", &cond_src(b)],
        CondExpr::Group(e) => bfmt![b"( ", &cond_src(e), b" )"],
    }
}

/// Reconstruct source text for a whole word (all parts concatenated).
#[must_use]
pub fn word_src(w: &Word) -> Str {
    parts_src(&w.parts)
}

/// Reconstruct source text for a run of word parts *without* any enclosing
/// quotes — the inside of a `WordPart::DoubleQuoted`, say. bash's
/// "bad substitution" diagnostic names exactly this: the string its
/// `expand_word_internal` was handed, which for a double-quoted section is the
/// section's contents with the quote characters already stripped.
#[must_use]
pub fn parts_src(parts: &[WordPart]) -> Str {
    let mut s = Str::new();
    for p in parts {
        s.push_str(&part_src(p));
    }
    s
}

/// `$name` when `name` is a plain identifier or a single special parameter,
/// otherwise the braced `${name}` form (always valid).
fn dollar_name(name: &str) -> Str {
    let simple = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let special = name.len() == 1
        && matches!(
            name.chars().next(),
            Some('?' | '@' | '*' | '#' | '$' | '!' | '-' | '0'..='9')
        );
    if simple || special {
        bfmt![b"$", name]
    } else {
        bfmt![b"${", name, b"}"]
    }
}

/// `name` optionally followed by `[index]`.
#[must_use]
pub fn name_sub(name: &str, index: &Option<Box<Word>>) -> Str {
    match index {
        Some(i) => bfmt![name, b"[", &word_src(i), b"]"],
        None => name.as_bytes().to_vec(),
    }
}

/// `name` optionally followed by a whole subscript — `[i]`, `[@]` or `[*]`.
///
/// The wider counterpart of [`name_sub`], for the one place a subscript may be
/// any of the three: an indirection's *pointer* (`${!a[0]}`, `${!a[@]#x}`).
#[must_use]
pub fn name_index(name: &str, index: &Option<ArrayIndex>) -> Str {
    match index {
        Some(ArrayIndex::Index(i)) => bfmt![name, b"[", &word_src(i), b"]"],
        Some(ArrayIndex::All) => bfmt![name, b"[@]"],
        Some(ArrayIndex::Star) => bfmt![name, b"[*]"],
        None => name.as_bytes().to_vec(),
    }
}

/// Re-quote a [`WordPart::SingleQuoted`] run.
///
/// `escaped` text was written with backslashes in the source, so it goes back
/// out that way — one backslash per character, which is what bash's
/// `declare -f` prints (`echo a\ b`, `echo \*`, `echo "a\"b"`). Everything else
/// was written as `'…'` (or `$'…'`, which bash also prints as `'…'`).
///
/// A single-quoted run cannot contain a single quote, so an embedded one — only
/// reachable via `$'a\'b'` — is spliced out and re-added as `'\''`, exactly as
/// bash does.
fn quoted_lit_src(text: BStr<'_>, escaped: bool) -> Str {
    if escaped {
        let mut s = Str::with_capacity(text.len() * 2);
        for c in bytes::chars(text) {
            s.push(b'\\');
            c.push_to(&mut s);
        }
        return s;
    }
    let mut s = Str::with_capacity(text.len() + 2);
    s.push(b'\'');
    for &b in text {
        if b == b'\'' {
            s.extend_from_slice(b"'\\''");
        } else {
            s.push(b);
        }
    }
    s.push(b'\'');
    s
}

fn part_src(p: &WordPart) -> Str {
    match p {
        WordPart::Literal(s) => s.clone(),
        WordPart::SingleQuoted { text, escaped } => quoted_lit_src(text, *escaped),
        WordPart::DoubleQuoted(parts) => {
            let mut s = b"\"".to_vec();
            for p in parts {
                s.push_str(&part_src(p));
            }
            s.push(b'"');
            s
        }
        // bash reproduces a parameter reference with the braces the source
        // wrote — `declare -f` on a body containing `${x}` prints `${x}`, not
        // `$x` — so the spelling is taken from the AST, not re-derived.
        WordPart::Param { name, braced } => {
            if *braced {
                bfmt![b"${", name, b"}"]
            } else {
                dollar_name(name)
            }
        }
        // `label` is not rendered: it only ever holds the reference an indirect
        // expansion goes by, and the `IndirectOp` arm splices that back in
        // itself, from the reference it kept.
        WordPart::ParamOp { name, index, op, colon, arg, label: _ } => {
            let sym = match op {
                ParamOp::UseDefault => "-",
                ParamOp::AssignDefault => "=",
                ParamOp::UseAlternate => "+",
                ParamOp::ErrorIfUnset => "?",
            };
            let colon = if *colon { ":" } else { "" };
            bfmt![b"${", &name_sub(name, index), colon, sym, &word_src(arg), b"}"]
        }
        WordPart::ParamTrim { name, index, suffix, longest, pattern } => {
            let op = match (suffix, longest) {
                (true, true) => "%%",
                (true, false) => "%",
                (false, true) => "##",
                (false, false) => "#",
            };
            bfmt![b"${", &name_sub(name, index), op, &word_src(pattern), b"}"]
        }
        WordPart::ParamSubstr { name, index, offset, length } => {
            let mut s = bfmt![b"${", &name_sub(name, index), b":", &word_src(offset)];
            if let Some(len) = length {
                s.push(b':');
                s.push_str(&word_src(len));
            }
            s.push(b'}');
            s
        }
        WordPart::ParamReplace { name, index, all, anchor, pattern, replacement } => {
            let op = match anchor {
                ReplaceAnchor::Start => "/#",
                ReplaceAnchor::End => "/%",
                ReplaceAnchor::None => {
                    if *all {
                        "//"
                    } else {
                        "/"
                    }
                }
            };
            bfmt![
                b"${",
                &name_sub(name, index),
                op,
                &word_src(pattern),
                b"/",
                &word_src(replacement),
                b"}"
            ]
        }
        WordPart::ParamCase { name, index, mode, all, pattern } => {
            let op = case_op_src(*mode, *all);
            bfmt![b"${", &name_sub(name, index), op, &word_src(pattern), b"}"]
        }
        WordPart::Indirect { refname, index } => {
            bfmt![b"${!", &name_index(refname, index), b"}"]
        }
        WordPart::IndirectOp { refname, index, target } => {
            // The `target` carries the referent name as a bare placeholder, so
            // rendering it yields `${ref<op>}`. Recovering `${!ref[i]<op>}` means
            // splicing in both the indirection `!` and the pointer's own
            // subscript, which the placeholder never held.
            let inner = part_src(target);
            match inner
                .strip_prefix(b"${")
                .and_then(|rest| rest.strip_prefix(refname.as_bytes()))
            {
                Some(op) => bfmt![b"${!", &name_index(refname, index), op],
                None => inner.clone(),
            }
        }
        WordPart::VarNames { prefix, star } => {
            bfmt![b"${!", prefix, if *star { "*" } else { "@" }, b"}"]
        }
        // A substitution is its own source context, so a here-document inside
        // one has to be flushed *within* the parentheses — carrying it out to
        // the enclosing line would leave the body outside the substitution.
        WordPart::CommandSub { body } => match body {
            // A backtick body was never parsed (bash reads it only at expansion
            // time), and even if it had been, re-printing it would drop the
            // backslash from a nested `` \` `` and stop it parsing. Echo it as
            // written, which is what bash does.
            CmdSubBody::Backtick { verbatim, .. } => bfmt![b"`", verbatim, b"`"],
            CmdSubBody::Parsed { prog, .. } => {
                bfmt![b"$(", &flush_here_docs(&program_inline(prog)), b")"]
            }
        },
        WordPart::ProcSub { input, body } => bfmt![
            if *input { b"<" } else { b">" },
            b"(",
            &flush_here_docs(&program_inline(body)),
            b")"
        ],
        WordPart::ArithSub { expr, bracket } => {
            if *bracket {
                bfmt![b"$[", expr, b"]"]
            } else {
                bfmt![b"$((", expr, b"))"]
            }
        }
        WordPart::BadSubst(raw) => bfmt![b"${", raw, b"}"],
        WordPart::Length(name) => bfmt![b"${#", name, b"}"],
        WordPart::ArrayRef { name, index, length } => {
            let idx = match index {
                ArrayIndex::Index(w) => word_src(w),
                ArrayIndex::All => b"@".to_vec(),
                ArrayIndex::Star => b"*".to_vec(),
            };
            if *length {
                bfmt![b"${#", name, b"[", &idx, b"]}"]
            } else {
                bfmt![b"${", name, b"[", &idx, b"]}"]
            }
        }
        WordPart::ArrayKeys { name, star } => {
            bfmt![b"${!", name, b"[", if *star { "*" } else { "@" }, b"]}"]
        }
        WordPart::ParamTransform { name, index, op } => {
            bfmt![b"${", &name_sub(name, index), b"@", *op, b"}"]
        }
        WordPart::BadTransform { raw, .. } => {
            // The raw source already includes the name, any subscript, and the
            // (empty/unknown/multi-char) operator, e.g. `x@`, `a[0]@Z`.
            bfmt![b"${", raw, b"}"]
        }
        WordPart::ArraySlice { name, star, offset, length } => {
            let sub = if name == "@" || name == "*" {
                name.as_bytes().to_vec()
            } else {
                bfmt![name, b"[", if *star { "*" } else { "@" }, b"]"]
            };
            let mut s = bfmt![b"${", &sub, b":", &word_src(offset)];
            if let Some(len) = length {
                s.push(b':');
                s.push_str(&word_src(len));
            }
            s.push(b'}');
            s
        }
        WordPart::ArrayBulk { name, star, op } => {
            // `BadTransform` carries the full raw inner source, so reproduce it
            // verbatim rather than re-synthesising a subscript + operator.
            if let BulkOp::BadTransform { raw } = op {
                return bfmt![b"${", raw, b"}"];
            }
            let sub = if name == "@" || name == "*" {
                name.as_bytes().to_vec()
            } else {
                bfmt![name, b"[", if *star { "*" } else { "@" }, b"]"]
            };
            let opstr = match op {
                BulkOp::Trim { suffix, longest, pattern } => {
                    let o = match (suffix, longest) {
                        (true, true) => "%%",
                        (true, false) => "%",
                        (false, true) => "##",
                        (false, false) => "#",
                    };
                    bfmt![o, &word_src(pattern)]
                }
                BulkOp::Replace { all, anchor, pattern, replacement } => {
                    let o = match anchor {
                        ReplaceAnchor::Start => "/#",
                        ReplaceAnchor::End => "/%",
                        ReplaceAnchor::None => {
                            if *all {
                                "//"
                            } else {
                                "/"
                            }
                        }
                    };
                    bfmt![o, &word_src(pattern), b"/", &word_src(replacement)]
                }
                BulkOp::Case { mode, all, pattern } => {
                    bfmt![case_op_src(*mode, *all), &word_src(pattern)]
                }
                BulkOp::Transform { op } => bfmt![b"@", *op],
                // Short-circuited via the early return above.
                BulkOp::BadTransform { .. } => Str::new(),
            };
            bfmt![b"${", &sub, &opstr, b"}"]
        }
        WordPart::ArrayOp { name, star, op, colon, arg } => {
            let sub = bfmt![name, b"[", if *star { "*" } else { "@" }, b"]"];
            let o = match op {
                ParamOp::UseDefault => "-",
                ParamOp::AssignDefault => "=",
                ParamOp::UseAlternate => "+",
                ParamOp::ErrorIfUnset => "?",
            };
            let colon = if *colon { ":" } else { "" };
            bfmt![b"${", &sub, colon, o, &word_src(arg), b"}"]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    /// Parse `src`, expect exactly one function definition, and unparse it.
    fn dump_fn(src: &str, name: &str) -> String {
        let prog = parse(src.as_bytes()).expect("parse");
        for item in &prog.items {
            for cmd in &item.list.first.commands {
                if let Command::Function(f) = cmd
                    && f.name == name.as_bytes()
                {
                    return text(unparse_function(&f.name, &f.body, &f.redirects));
                }
            }
        }
        panic!("function {name} not found");
    }

    /// The dumps these tests build are ASCII by construction; render one as
    /// text so the assertions can stay written as string literals.
    fn text(s: Str) -> String {
        String::from_utf8(s).expect("test dumps are ASCII")
    }

    /// Unparse a function, re-parse the dump, and unparse again — the two dumps
    /// must be identical (a round-trip stability check).
    fn assert_roundtrip(src: &str, name: &str) {
        let first = dump_fn(src, name);
        // The dump is `name () \n{ … }`; re-parse it as a program.
        let reprog = parse(first.as_bytes()).expect("re-parse dump");
        let f = reprog
            .items
            .iter()
            .flat_map(|i| &i.list.first.commands)
            .find_map(|c| match c {
                Command::Function(f) if f.name == name.as_bytes() => Some(f),
                _ => None,
            })
            .expect("function in dump");
        let second = text(unparse_function(&f.name, &f.body, &f.redirects));
        assert_eq!(first, second, "round-trip differs for {name}");
    }

    #[test]
    fn simple_command_body() {
        let d = dump_fn("f() { echo hello world; }", "f");
        // bash prints the opening brace with a trailing space: `{ \n`.
        assert!(d.starts_with("f () \n{ \n"), "dump: {d:?}");
        assert!(d.contains("echo hello world"), "dump: {d:?}");
        assert!(d.ends_with("}\n"), "dump: {d:?}");
    }

    #[test]
    fn if_and_loop_body() {
        let d = dump_fn("f() { if true; then echo a; else echo b; fi; }", "f");
        assert!(d.contains("if true; then"), "dump: {d:?}");
        assert!(d.contains("echo a"), "dump: {d:?}");
        assert!(d.contains("else"), "dump: {d:?}");
        assert!(d.contains("fi"), "dump: {d:?}");
        assert_roundtrip("f() { if true; then echo a; else echo b; fi; }", "f");
    }

    #[test]
    fn for_and_pipeline_body() {
        let d = dump_fn("g() { for x in 1 2 3; do echo $x | cat; done; }", "g");
        // bash puts `do` on its own line for `for` loops (the word list is
        // terminated with `;`, then `do` at the loop indent).
        assert!(d.contains("for x in 1 2 3;\n"), "dump: {d:?}");
        assert!(d.contains("\n    do\n"), "dump: {d:?}");
        assert!(d.contains("echo $x | cat"), "dump: {d:?}");
        assert_roundtrip("g() { for x in 1 2 3; do echo $x | cat; done; }", "g");
    }

    #[test]
    fn case_body_roundtrips() {
        assert_roundtrip("h() { case $1 in a) echo A ;; b|c) echo BC ;; *) echo other ;; esac; }", "h");
    }

    #[test]
    fn declare_f_matches_bash_layout() {
        // Byte-for-byte parity with bash's `declare -f` deparser for the common
        // constructs: opening `{ ` with trailing space, every statement `;`-
        // terminated except the final one before `}`, `do` on its own line for
        // `for`, and `case WORD in ` with a trailing space.
        assert_eq!(
            dump_fn("f() { echo a; echo b; }", "f"),
            "f () \n{ \n    echo a;\n    echo b\n}\n"
        );
        assert_eq!(
            dump_fn("f() { if true; then echo a; else echo b; fi; }", "f"),
            "f () \n{ \n    if true; then\n        echo a;\n    else\n        echo b;\n    fi\n}\n"
        );
        assert_eq!(
            dump_fn("f() { while false; do echo c; done; }", "f"),
            "f () \n{ \n    while false; do\n        echo c;\n    done\n}\n"
        );
        assert_eq!(
            dump_fn("f() { for i in 1 2; do echo $i; done; }", "f"),
            "f () \n{ \n    for i in 1 2;\n    do\n        echo $i;\n    done\n}\n"
        );
        assert_eq!(
            dump_fn("f() { case $x in a) echo 1;; esac; }", "f"),
            "f () \n{ \n    case $x in \n        a)\n            echo 1\n        ;;\n    esac\n}\n"
        );
    }

    #[test]
    fn param_expansions_roundtrip() {
        assert_roundtrip(r#"p() { echo "${x:-def}" "${y#pre}" "${z//a/b}" "${#w}"; }"#, "p");
    }

    #[test]
    fn redirects_and_assignments() {
        let d = dump_fn("r() { local n=5; echo hi > out.txt 2>&1; }", "r");
        assert!(d.contains("local n=5"), "dump: {d:?}");
        assert!(d.contains("> out.txt"), "dump: {d:?}");
        assert!(d.contains("2>&1"), "dump: {d:?}");
    }

    #[test]
    fn output_dup_shows_explicit_default_fd() {
        // bash's deparser always shows an output dup's source fd, even the
        // default (`>&2` → `1>&2`), and writes fd-dups tight (no space).
        let d = dump_fn("r() { echo x >&2; }", "r");
        assert!(d.contains("1>&2"), "dump: {d:?}");
    }

    #[test]
    fn function_definition_redirect_renders_on_brace() {
        // A redirect attached to the definition itself renders on the closing
        // brace, spaced like bash: `} > log 2>&1`.
        let d = dump_fn("r() { echo hi; } >log 2>&1", "r");
        assert!(d.contains("} > log 2>&1"), "dump: {d:?}");
    }

    #[test]
    fn input_dup_renders_with_explicit_source_fd() {
        // `<&N` (input dup) must render with its direction preserved and the
        // explicit fd `0` shown (`0<&3`), matching bash — not as an output dup
        // `>&3`. Regression: `<&`/`>&` used to collapse to one op.
        let d = dump_fn("r() { read x <&3; cat <&4; }", "r");
        assert!(d.contains("read x 0<&3"), "dump: {d:?}");
        assert!(d.contains("cat 0<&4"), "dump: {d:?}");
        assert!(!d.contains(">&3"), "input dup rendered as output dup: {d:?}");
    }

    /// The *other* two shapes a `<&`/`>&` target can have, which bash's parser
    /// sorts into different redirect instructions and its printer prints
    /// differently from the plain-number form above. All values measured
    /// against bash 5.2.
    #[test]
    fn a_dup_word_elides_the_default_fd_and_a_close_is_always_an_output_dup() {
        // A target that is not a bare number is the `…_word` form, and only it
        // elides the operator's default fd. A *quoted* number is one of these:
        // the classification is the parser's, so it never sees through quotes.
        let d = dump_fn(
            "r() { echo x >& out; echo x 1>& out; echo x 2>& out; echo x >& \"2\"; cat <& in; cat 3<& in; }",
            "r",
        );
        assert!(d.contains("echo x >&out"), "dump: {d:?}");
        assert!(d.contains("echo x 2>&out"), "dump: {d:?}");
        assert!(d.contains("echo x >&\"2\""), "dump: {d:?}");
        assert!(d.contains("cat <&in"), "dump: {d:?}");
        assert!(d.contains("cat 3<&in"), "dump: {d:?}");
        // `1>& out` and `>& out` are the same instruction, so they print alike.
        assert_eq!(d.matches("echo x >&out").count(), 2, "dump: {d:?}");

        // A bare `-` is `r_close_this`, which bash prints as an *output* dup
        // whichever way it was written, and always with the fd.
        let d = dump_fn("r() { cat <&-; cat 3<&-; echo x >&-; echo x 2>&-; }", "r");
        assert!(d.contains("cat 0>&-"), "dump: {d:?}");
        assert!(d.contains("cat 3>&-"), "dump: {d:?}");
        assert!(d.contains("echo x 1>&-"), "dump: {d:?}");
        assert!(d.contains("echo x 2>&-"), "dump: {d:?}");
        assert!(!d.contains("<&-"), "a close printed as an input dup: {d:?}");

        // A quoted `-` is not a close, so it keeps its direction and elides.
        let d = dump_fn("r() { echo x >& '-'; }", "r");
        assert!(d.contains("echo x >&'-'"), "dump: {d:?}");
    }

    #[test]
    fn readwrite_redirect_renders_and_roundtrips() {
        // `<>` (open for read+write) renders with its default source fd 0 shown
        // tight against the operator (`0<> file`), like bash's deparser, and a
        // non-default fd is preserved (`3<> file`).
        let d = dump_fn("r() { cat <> io.txt; exec 3<> log; }", "r");
        assert!(d.contains("cat 0<> io.txt"), "dump: {d:?}");
        assert!(d.contains("3<> log"), "dump: {d:?}");
        assert_roundtrip("r() { cat <> io.txt; exec 3<> log; }", "r");
    }

    #[test]
    fn nested_function_roundtrips() {
        assert_roundtrip("outer() { inner() { echo deep; }; inner; }", "outer");
    }

    #[test]
    fn empty_body_uses_noop() {
        let d = dump_fn("e() { :; }", "e");
        assert!(d.contains(":"), "dump: {d:?}");
        assert_roundtrip("e() { :; }", "e");
    }

    /// Parse `src`, expect the named function, and render its *exported* form.
    fn dump_exported(src: &str, name: &str) -> String {
        let prog = parse(src.as_bytes()).expect("parse");
        for item in &prog.items {
            for cmd in &item.list.first.commands {
                if let Command::Function(f) = cmd
                    && f.name == name.as_bytes()
                {
                    return text(unparse_function_exported(&f.body, &f.redirects));
                }
            }
        }
        panic!("function {name} not found");
    }

    /// The `BASH_FUNC_<name>%%` value side, byte-for-byte as bash 5.2.37 writes
    /// it. These expectations were *measured*: each was read back out of a real
    /// bash child with `printenv 'BASH_FUNC_<name>%%'`, so they pin the exact
    /// one-space-per-level shape (as opposed to `declare -f`'s four-per-level)
    /// that bash's `named_function_string(NULL, cmd, 0)` produces.
    #[test]
    fn exported_form_matches_bash() {
        // Simple body: two spaces before `echo` — one from the `() { ` header,
        // one from the body's own single-space indent.
        assert_eq!(dump_exported("f() { echo hi; }", "f"), "() {  echo hi\n}");

        // Every statement after the first starts at column 1, not column 4.
        assert_eq!(dump_exported("f() { echo a; echo b; }", "f"), "() {  echo a;\n echo b\n}");

        // Nesting does *not* deepen the indent: the brace group and the subshell
        // sit at the same one space as the statements around them.
        assert_eq!(
            dump_exported("f() { { echo x; }; ( echo y ); }", "f"),
            "() {  { \n echo x\n };\n ( echo y )\n}"
        );

        // A redirect on the definition rides the closing-brace line, as in
        // `declare -f` — but with no trailing newline after it.
        assert_eq!(dump_exported("f() { echo z; } > /dev/null", "f"), "() {  echo z\n} > /dev/null");

        // A newline *inside a string literal* is emitted verbatim at column 0.
        // This is why the exported shape has to be threaded through the printer
        // rather than applied as a post-hoc line-based re-indent: such a re-indent
        // would corrupt the literal.
        assert_eq!(
            dump_exported("f() { echo \"line1\nline2\"; }", "f"),
            "() {  echo \"line1\nline2\"\n}"
        );

        // Compound commands: every keyword line is flush at one space.
        assert_eq!(
            dump_exported(
                "f() { if true; then for i in 1 2; do echo $i; done; \
                 else case $x in a) echo A;; esac; fi; }",
                "f"
            ),
            "() {  if true; then\n for i in 1 2;\n do\n echo $i;\n done;\n else\n \
             case $x in \n a)\n echo A\n ;;\n esac;\n fi\n}"
        );
    }

    /// The exported form must re-parse to the same function, since that is
    /// exactly what the importing shell does with `NAME` + the value.
    #[test]
    fn exported_form_reparses() {
        for src in [
            "f() { echo hi; }",
            "f() { echo a; echo b; }",
            "f() { { echo x; }; ( echo y ); }",
            "f() { echo z; } > /dev/null",
            "f() { if true; then echo a; else echo b; fi; }",
            "f() { :; }",
        ] {
            let value = dump_exported(src, "f");
            let reparsed = format!("f {value}");
            let prog = parse(reparsed.as_bytes())
                .unwrap_or_else(|e| panic!("re-parse {reparsed:?}: {}", String::from_utf8_lossy(&e.msg)));
            let f = prog
                .items
                .iter()
                .flat_map(|i| &i.list.first.commands)
                .find_map(|c| match c {
                    Command::Function(f) if f.name == b"f" => Some(f),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no function in {reparsed:?}"));
            // Unparsing the re-parsed definition reproduces the same value: the
            // encoding is a fixed point, so a chain of exec'd shells cannot drift.
            assert_eq!(text(unparse_function_exported(&f.body, &f.redirects)), value);
        }
    }

    /// An empty body still needs a no-op so the importing shell can parse it.
    #[test]
    fn exported_empty_body_uses_noop() {
        assert_eq!(dump_exported("e() { :; }", "e"), "() {  :\n}");
    }
}
