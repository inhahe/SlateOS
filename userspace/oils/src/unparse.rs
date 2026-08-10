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
    CondExpr, Item, ItemSep, ParamOp, Pipeline, Program, Redirect, RedirectOp, ReplaceAnchor,
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

/// The two flags bash's deparser carries alongside `indentation`, and which
/// between them account for every layout difference in its output.
///
/// bash has exactly **one** command printer — `make_command_string_internal`
/// (print_cmd.c:182–378) — and reaches it three ways: `declare -f`/`type` and
/// the exported-function encoding, both of which set `inside_function_def`;
/// `print_comsub` (print_cmd.c:167–178), which sets `printing_comsub`; and
/// `make_command_string` with neither flag, which is the form `jobs` shows. The
/// flags are independent — a function defined inside a substitution sets both —
/// so they are two fields rather than one mode.
///
/// Everything else about the layout is `indentation`, which travels in
/// [`Indent`].
#[derive(Clone, Copy)]
struct Fmt {
    /// How deep, and in which shape.
    level: Indent,
    /// bash's `inside_function_def`. It decides two things: a `;` connector is
    /// `";\n"` plus the next line's indent rather than `"; "`
    /// (print_cmd.c:314–315), and a `{ … }` group is laid out over several
    /// lines rather than inline (print_cmd.c:698–732).
    in_func_def: bool,
    /// bash's `printing_comsub`. It decides one thing: a newline connector
    /// stays a newline instead of collapsing to `;` (print_cmd.c:302–320). bash
    /// only ever *records* a newline connector while `PST_CMDSUBST` is set
    /// (parse.y `list1: list1 '\n' newline_list list1`), so in bash the two
    /// always travel together; osh records the connector unconditionally and
    /// lets this flag decide, which comes to the same text.
    comsub: bool,
}

impl Fmt {
    /// `declare -f` / `type`: four spaces per level, inside a function
    /// definition.
    const DECLARE: Self = Self { level: Indent::DECLARE, in_func_def: true, comsub: false };
    /// The exported-function encoding: one space at every depth.
    const EXPORTED: Self = Self { level: Indent::EXPORTED, in_func_def: true, comsub: false };
    /// `print_comsub`: the body of `$( … )`, `<( … )` or `>( … )`.
    ///
    /// bash computes this at *parse* time, when `indentation` is still 0 and
    /// `indentation_amount` still its default 4 (print_cmd.c:56–57) — so a
    /// substitution's body is laid out from column 0 however deep the text that
    /// ends up carrying it.
    const COMSUB: Self = Self { level: Indent::DECLARE, in_func_def: false, comsub: true };
    /// `make_command_string` with both flags clear — the form `jobs` prints.
    const PLAIN: Self = Self { level: Indent::DECLARE, in_func_def: false, comsub: false };

    /// One nesting level deeper, keeping the flags — bash's
    /// `indentation += indentation_amount`.
    fn deeper(self) -> Self {
        Self { level: self.level + 1, ..self }
    }

    /// The same shape, now printing a function body — bash's
    /// `inside_function_def++`.
    fn in_function(self) -> Self {
        Self { in_func_def: true, ..self }
    }

    /// The leading whitespace a line at this depth carries.
    fn spaces(self) -> Str {
        self.level.spaces()
    }
}

/// bash's `semicolon()` (print_cmd.c:1512–1521): terminate the statement just
/// printed — unless it already ended in `&` or a newline, each of which is a
/// terminator in its own right. That is why a backgrounded last statement gets
/// no `;`, and why a here-document body (which ends in its own newline) is
/// followed by a blank line rather than a `;`.
fn semicolon(s: &mut Str) {
    if matches!(s.last(), Some(&b'&' | &b'\n')) {
        return;
    }
    s.push(b';');
}

/// bash's `newline()` (print_cmd.c:1486–1494): break the line, indent to the
/// current depth, then write `text`.
fn newline(s: &mut Str, level: Indent, text: &[u8]) {
    s.push(b'\n');
    s.push_str(&level.spaces());
    s.push_str(text);
}

/// The `in …` list of a `for` or `select` head, as bash's deparser writes it.
///
/// bash has no wordless case here. `print_for_command_head` (print_cmd.c:605–610)
/// is unconditional —
///
/// ```c
///   cprintf ("for %s in ", for_command->name->word);
///   command_print_word_list (for_command->map_list, " ");
/// ```
///
/// — because the *grammar* fills the gap: a `for x; do …` with no `in` at all has
/// its `map_list` synthesised as the single word `"$@"` (parse.y:839–854; `select`
/// at 907–922), which is also exactly what the loop iterates. So `None` here (osh
/// records the absence rather than synthesising) prints `"$@"`, while an explicit
/// but empty list prints nothing — leaving the trailing space of `in ` against the
/// `;`, which is why bash really does emit `for i in ;`.
fn in_list_src(words: Option<&[Word]>) -> Str {
    let Some(words) = words else {
        return b"\"$@\"".to_vec();
    };
    let ws: Vec<Str> = words.iter().map(word_src).collect();
    bytes::join(&ws, b" ")
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
    let fmt = Fmt::DECLARE;
    let inner = program_block(body, fmt.deeper(), true);
    if inner.is_empty() {
        // An empty body still needs a no-op so it re-parses.
        s.push_str(&fmt.deeper().spaces());
        s.push(b':');
    } else {
        s.push_str(&inner);
    }
    // Redirections attached to the definition (`f() { …; } >log`) render on
    // the closing-brace line: `} > log`, matching bash's `declare -f`. bash
    // restores the saved `indentation` before `newline ("}")`
    // (print_cmd.c:1362–1377), so the brace lands at the definition's depth.
    newline(&mut s, fmt.level, b"}");
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
    let fmt = Fmt::EXPORTED;
    let mut s = b"() { ".to_vec();
    let inner = program_block(body, fmt.deeper(), true);
    if inner.is_empty() {
        s.push_str(&fmt.deeper().spaces());
        s.push(b':');
    } else {
        s.push_str(&inner);
    }
    // `named_function_string` restores the *saved* `indentation` — the global
    // 0, not the one space this shape uses — before its `newline ("}")`
    // (print_cmd.c:1441–1456), so the closing brace lands at column 0 even
    // though every line of the body carries a space.
    s.push(b'\n');
    s.push(b'}');
    for r in redirects {
        s.push(b' ');
        s.push_str(&redirect_src(r));
    }
    flush_here_docs(&s)
}

/// Render a whole program the way bash's `CONNECTION` printer does
/// (print_cmd.c:257–334) — no trailing separator and no trailing newline, both
/// of which belong to the caller (`semicolon()` then `newline()`).
///
/// `indent_first` is bash's `skip_this_indent` seen from the other side: the
/// callers that glue a body to text already on the line (`if `, `while `, `{ `,
/// `( `, `coproc `) pass `false`.
///
/// The connector rules, transcribed:
///
/// * `&` — ` &` was already written by [`item_stmt`]; the next statement
///   follows on the same line after a space.
/// * `;` — `"; "` normally, `";\n"` plus the next line's indent inside a
///   function definition.
/// * a newline — the same as `;` *unless* a substitution is being re-printed,
///   in which case it stays a literal newline and the next statement loses its
///   indent (bash's `was_newline` guard stops the newline being doubled).
///
/// A here-document parked on the statement's last line stands in for bash's
/// `was_heredoc`: the separator character is dropped (the body must start on
/// the very next line, so there is nowhere to put one) while the line break
/// still happens.
fn program_block(prog: &Program, fmt: Fmt, indent_first: bool) -> Str {
    let mut out = Str::new();
    let mut indent_this = indent_first;
    let n = prog.items.len();
    for (i, item) in prog.items.iter().enumerate() {
        let mut stmt = Str::new();
        if indent_this {
            stmt.push_str(&fmt.spaces());
        }
        indent_this = true;
        stmt.push_str(&item_stmt(item, fmt));
        if i + 1 < n {
            let was_heredoc = last_line_has_here_doc(&stmt);
            match item.sep {
                ItemSep::Amp => {
                    // ` &` is already there; the next statement follows inline.
                    stmt.push(b' ');
                    indent_this = false;
                }
                ItemSep::Semi | ItemSep::Newline => {
                    if was_heredoc {
                        // The parked body has to start on the very next line,
                        // so the statement's line ends here — which is also
                        // where `flush_here_docs` hangs it. bash gets to the
                        // same place from the other direction: the body is
                        // still deferred when the connector runs, so
                        // `print_deferred_heredocs` prints it here and
                        // swallows the `;` on the way past
                        // (print_cmd.c:1043–1058). Whatever the connector goes
                        // on to write lands *after* the body either way.
                        stmt.push(b'\n');
                    }
                    // `s[0] = printing_comsub ? c : ';'`.
                    let c = if fmt.comsub && item.sep == ItemSep::Newline { b'\n' } else { b';' };
                    // `was_newline` records that `s` *was* the line break, so
                    // the branch below must not add a second one.
                    let was_newline = !was_heredoc && c == b'\n';
                    if !was_heredoc {
                        stmt.push(c);
                    }
                    // bash keeps these as two arms —
                    //
                    //     if (inside_function_def) cprintf ("\n");
                    //     else if (printing_comsub && c == '\n' && was_newline == 0)
                    //       cprintf ("\n");
                    //
                    // — but both write the same byte, so they fold into one. The
                    // second fires only when a deferred here-document already
                    // ate the `\n` that `s` would otherwise have been (hence
                    // `was_newline == 0`); `printing_comsub` is implied, because
                    // `c` is `\n` only where `fmt.comsub` chose it just above.
                    if fmt.in_func_def || (c == b'\n' && !was_newline) {
                        stmt.push(b'\n');
                    } else {
                        if c == b';' {
                            stmt.push(b' ');
                        }
                        indent_this = false;
                    }
                }
            }
        }
        out.push_str(&flush_here_docs(&stmt));
    }
    out
}

/// One statement (and-or list, plus a trailing ` &` when backgrounded). The
/// first line carries no leading indent (the caller supplies it); nested lines
/// are indented to `fmt`'s depth.
fn item_stmt(item: &Item, fmt: Fmt) -> Str {
    let mut s = and_or_block(&item.list, fmt);
    if item.is_background() {
        s.push_str(" &");
    }
    s
}

/// And-or list. bash writes ` && ` / ` || ` inline and clears the right-hand
/// side's indent (print_cmd.c:283–294), so however compound the operands are,
/// the operator never breaks the line.
fn and_or_block(ao: &AndOr, fmt: Fmt) -> Str {
    let mut s = pipeline_block(&ao.first, fmt);
    for (op, pl) in &ao.rest {
        s.push_str(match op {
            AndOrOp::And => " && ",
            AndOrOp::Or => " || ",
        });
        s.push_str(&pipeline_block(pl, fmt));
    }
    s
}

/// And-or list as `make_command_string` renders it with both printer flags
/// clear — the form `jobs` shows the command a job was started from.
#[must_use]
pub fn and_or_src(ao: &AndOr) -> Str {
    flush_here_docs(&and_or_block(ao, Fmt::PLAIN))
}

/// A single command in that same form. `and_or_src` is the usual entry point —
/// this one exists for the shapes that are a bare [`Command`] with no list
/// around them, such as `coproc`.
#[must_use]
pub fn command_src(cmd: &Command) -> Str {
    flush_here_docs(&command_block(cmd, Fmt::PLAIN))
}

/// A whole program in that same form.
#[must_use]
pub fn program_src(prog: &Program) -> Str {
    flush_here_docs(&program_block(prog, Fmt::PLAIN, true))
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

/// Pipeline. `|` is written inline and clears the next command's indent, the
/// same way `&` does — they share one arm of bash's connector switch
/// (print_cmd.c:264–281).
fn pipeline_block(pl: &Pipeline, fmt: Fmt) -> Str {
    let mut s = pipeline_prefix(pl);
    let cmds: Vec<Str> = pl.commands.iter().map(|c| command_block(c, fmt)).collect();
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
    fmt: Fmt,
) -> Str {
    // `cprintf ("if "); skip_this_indent++; <test>; semicolon (); " then\n"`
    // (print_cmd.c:821–828) — `then` rides the condition's line, which is why a
    // multi-statement condition inside a function definition still breaks.
    let mut s = b"if ".to_vec();
    s.push_str(&program_block(cond, fmt, false));
    semicolon(&mut s);
    s.push_str(" then\n");
    s.push_str(&program_block(body, fmt.deeper(), true));
    if let Some(((econd, ebody), rest)) = elifs.split_first() {
        // `elif …` becomes `else\n  if … fi;` one indent level deeper.
        semicolon(&mut s);
        newline(&mut s, fmt.level, b"else\n");
        s.push_str(&fmt.deeper().spaces());
        s.push_str(&render_if(econd, ebody, rest, else_body, fmt.deeper()));
        semicolon(&mut s);
    } else if let Some(eb) = else_body {
        semicolon(&mut s);
        newline(&mut s, fmt.level, b"else\n");
        s.push_str(&program_block(eb, fmt.deeper(), true));
        semicolon(&mut s);
    } else {
        semicolon(&mut s);
    }
    newline(&mut s, fmt.level, b"fi");
    s
}

/// Render a command the way `make_command_string_internal` does. The first line
/// carries no leading indent (the caller supplies it, which is bash's
/// `skip_this_indent`); continuation lines sit at `fmt`'s depth and bodies one
/// deeper.
fn command_block(cmd: &Command, fmt: Fmt) -> Str {
    match cmd {
        Command::Simple(sc) => simple_inline(sc),
        Command::If(c) => render_if(&c.cond, &c.body, &c.elifs, c.else_body.as_ref(), fmt),
        Command::Loop(c) => {
            // `while`/`until` keep `do` on the same line as the condition
            // (`cprintf (" do\n")`, print_cmd.c:809 — the comment there notes it
            // "was `newline ("do\n")`"), unlike `for`/`select` below.
            let mut s = if c.until { b"until ".to_vec() } else { b"while ".to_vec() };
            s.push_str(&program_block(&c.cond, fmt, false));
            semicolon(&mut s);
            s.push_str(" do\n");
            s.push_str(&program_block(&c.body, fmt.deeper(), true));
            semicolon(&mut s);
            newline(&mut s, fmt.level, b"done");
            s
        }
        Command::For(c) => {
            // bash's deparser puts `do` on its own line for `for`: the word list
            // is terminated with an unconditional `;` (print_cmd.c:628), then
            // `newline ("do\n")` writes it at the loop's own depth.
            let mut s = bfmt![b"for ", &c.var, b" in ", &in_list_src(c.words.as_deref())];
            s.push(b';');
            newline(&mut s, fmt.level, b"do\n");
            s.push_str(&program_block(&c.body, fmt.deeper(), true));
            semicolon(&mut s);
            newline(&mut s, fmt.level, b"done");
            s
        }
        Command::ForArith(c) => {
            // `for ((init; cond; upd))` with no inner-paren padding and `do` on
            // its own line, matching bash.
            let mut s = bfmt![b"for ((", &c.init, b"; ", &c.cond, b"; ", &c.update, b"))"];
            newline(&mut s, fmt.level, b"do\n");
            s.push_str(&program_block(&c.body, fmt.deeper(), true));
            semicolon(&mut s);
            newline(&mut s, fmt.level, b"done");
            s
        }
        Command::Select(c) => {
            let mut s = bfmt![b"select ", &c.var, b" in ", &in_list_src(c.words.as_deref())];
            s.push(b';');
            newline(&mut s, fmt.level, b"do\n");
            s.push_str(&program_block(&c.body, fmt.deeper(), true));
            semicolon(&mut s);
            newline(&mut s, fmt.level, b"done");
            s
        }
        Command::Function(f) => {
            // A function defined *inside* another command reaches
            // `command_block` (top-level definitions go through
            // `unparse_function`). bash's deparser prefixes every such nested
            // definition with the `function` keyword — regardless of the source
            // syntax — while top-level defs omit it. See known-issues.md
            // TD-OILS-DECLAREF-QUIRKS item 4. The body is printed with
            // `inside_function_def` set (print_cmd.c:1349–1361), which is what
            // splits it over several lines even inside a substitution.
            let mut s = bfmt![b"function ", &f.name, b" () \n"];
            s.push_str(&fmt.spaces());
            s.push_str("{ \n");
            s.push_str(&program_block(&f.body, fmt.deeper().in_function(), true));
            newline(&mut s, fmt.level, b"}");
            for r in &f.redirects {
                s.push(b' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
        Command::Case(c) => {
            // bash prints `case WORD in ` with a trailing space, then
            // `newline ("")` before each clause's patterns
            // (print_cmd.c:762–785). Clause bodies are never terminated: the
            // `;;` follows on its own line with no `semicolon ()` before it.
            let mut s = bfmt![b"case ", &word_src(&c.word), b" in "];
            for item in &c.items {
                let pats: Vec<Str> = item.patterns.iter().map(word_src).collect();
                // `command_print_word_list (clauses->patterns, " | ")`
                // (print_cmd.c:769) — the alternatives are spaced out, not
                // glued the way the source usually writes them.
                newline(&mut s, fmt.level + 1, &bytes::join(&pats, b" | "));
                s.push_str(")\n");
                s.push_str(&program_block(&item.body, fmt.deeper().deeper(), true));
                newline(
                    &mut s,
                    fmt.level + 1,
                    match item.term {
                        crate::ast::CaseTerm::Break => b";;".as_slice(),
                        crate::ast::CaseTerm::FallThrough => b";&",
                        crate::ast::CaseTerm::ContinueMatch => b";;&",
                    },
                );
            }
            newline(&mut s, fmt.level, b"esac");
            s
        }
        Command::BraceGroup(prog) => {
            // `print_group_command` (print_cmd.c:697–732) is the one printer
            // that reads `inside_function_def` for its *shape*: inside a
            // definition the group breaks over several lines, everywhere else
            // it stays on one — `{ echo a; echo b; }`, terminator and all.
            let mut s = b"{ ".to_vec();
            if fmt.in_func_def {
                s.push(b'\n');
                s.push_str(&program_block(prog, fmt.deeper(), true));
                newline(&mut s, fmt.level, b"}");
            } else {
                s.push_str(&program_block(prog, fmt, false));
                semicolon(&mut s);
                s.push_str(" }");
            }
            s
        }
        Command::Subshell(sub) => {
            // `cprintf ("( "); skip_this_indent++; <body>; cprintf (" )")`
            // (print_cmd.c:350–356) — no `semicolon ()`, and the body keeps the
            // subshell's own depth rather than descending, so a `;` connector
            // inside a function definition lines the next statement up under
            // the `(`. (TD-OILS-DECLAREF-QUIRKS item 2.)
            let body = program_block(&sub.body, fmt, false);
            if body.is_empty() {
                return b"( )".to_vec();
            }
            bfmt![b"( ", &body, b" )"]
        }
        Command::Cond(c) => cond_command_src(&c.expr),
        Command::Arith(text) => bfmt![b"((", text, b"))"],
        Command::Coproc { name, body } => {
            let mut s = b"coproc ".to_vec();
            if let Some(n) = name {
                s.push_str(n);
                s.push(b' ');
            }
            s.push_str(&command_block(body, fmt));
            s
        }
        Command::Redirected { inner, redirects } => {
            let mut s = command_block(inner, fmt);
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
        // A move keeps its own direction, unlike a close, but shows the fd
        // whichever way the source was written — `>&3-` comes back `1>&3-` and
        // `<&$v-` comes back `0<&$v-`, where the plain `_word` form would have
        // elided both. `target` still carries the trailing `-`.
        DupSpelling::Number | DupSpelling::MoveNumber | DupSpelling::MoveWord => {
            bfmt![r.fd, op, &target]
        }
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

/// The text bash keeps for a substitution body it parsed: not the source, but
/// `print_comsub`'s re-print of the parse, wrapped back in the delimiters the
/// scan around it already wrote. `parse_comsub` ends
/// `tcmd = print_comsub (parsed_command); … return ret` (parse.y:4219–4241).
///
/// `open` is that opening delimiter — `$(`, `<(` or `>(`, the three spellings
/// parse.y:5028 names and 5042 sends through the one call.
///
/// The leading space is bash's own guard (parse.y:4221–4227): a re-print that
/// starts with `(` gets one prepended so the result cannot be read back as an
/// arithmetic expansion. Without it `$( (echo a) )` would come back `$((`
/// `echo a ))`, which is a different construct — and `<( (echo a) )` would come
/// back as a `<((` that does not parse at all.
pub(crate) fn comsub_reprint(open: &[u8], prog: &Program) -> Str {
    bfmt![open, &comsub_body(prog), b")"]
}

/// The same re-print without the delimiters — the bytes bash keeps *as the
/// body*, and hands to `command_substitute` to read back at expansion time.
///
/// `parse_comsub` builds `ret` as (guard space) + `print_comsub`'s text + `)`
/// and the scan around it has already written the `$(`, so this is `ret` less
/// its closing delimiter. The guard space belongs to the body, not to the
/// delimiter: it is inside the substitution and the re-read sees it.
pub(crate) fn comsub_body(prog: &Program) -> Str {
    let body = flush_here_docs(&program_block(prog, Fmt::COMSUB, true));
    if body.first() == Some(&b'(') {
        return bfmt![b" ", &body];
    }
    body
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

/// Reconstruct the replacement half of a `${name/pat/repl}` — the separator
/// **and** the text, or nothing at all.
///
/// bash prints a word back from the source it saved, so the two bodies that
/// expand alike do not print alike: `${q/ab}` has no separator to print, while
/// `${q/ab/}` has one and an empty replacement after it. Keeping the slash with
/// the `Option` here is what makes the caller unable to print one the source
/// never had.
fn repl_src(repl: &Option<Box<Word>>) -> Str {
    repl.as_deref()
        .map_or_else(Str::new, |w| bfmt![b"/", &word_src(w)])
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

/// Source text for one element of a compound assignment, keyed or not.
///
/// A bare `m=(…)` names an element it refuses by the text it was *written* with
/// — `m=([$e]=v)` reports `[$e]=v` and not the `[]=v` it expanded to — so the
/// brackets and `=` have to be put back around the two halves' own source.
pub(crate) fn elem_src(e: &ArrayElem) -> Str {
    match e {
        ArrayElem::Positional(w) => word_src(w),
        ArrayElem::Keyed { index, value, append } => bfmt![
            b"[",
            &word_src(index),
            if *append { b"]+=".as_slice() } else { b"]=".as_slice() },
            &word_src(value)
        ],
    }
}

/// The whole value list of a compound assignment as bash's reader holds it.
///
/// bash does not keep a compound assignment's elements apart. Its tokenizer
/// reads them with `read_token` under `PST_COMPASSIGN` and
/// `parse_compound_assignment` (parse.y:4715) writes them back out into one
/// string joined by a **single space** — which is why the newlines a multi-line
/// literal was written with are gone from every diagnostic that echoes it, and
/// why `a=(one⏎two)` echoes `one two`. That string is what
/// `assign_compound_array_list` re-parses; see
/// [`crate::interp::Shell::array_assign_reparse_error`].
pub(crate) fn array_listing(items: &[ArrayElem]) -> Str {
    let mut s = Str::new();
    for (i, e) in items.iter().enumerate() {
        if i > 0 {
            s.push(b' ');
        }
        s.push_str(&elem_src(e));
    }
    s
}

/// [`array_listing`] split around the closing `)` of the `k`-th `$( … )` in it:
/// everything before that `)`, and everything after. `None` when the list holds
/// no `k`-th substitution.
///
/// This is where a reader tokenizing the listing stops if that substitution's
/// body will not parse back, so it is what the diagnostic's line number and
/// echoed line are measured from — and both are measured in the *listing*, not
/// in the script, because that string is the reader's `shell_input_line` for as
/// long as the re-parse runs.
///
/// Located the same way [`attach_comsub_tails`] locates a tail — by rendering
/// the list with the one substitution swapped for a sentinel — so the answer
/// comes out of [`array_listing`] itself rather than out of a second renderer
/// written to agree with it. Two identical `$( … )`s in one list render alike
/// and `'$(! )'` renders a literal that looks like one, so searching for the
/// rendered text would be ambiguous twice over.
pub(crate) fn array_listing_split(items: &[ArrayElem], k: usize) -> Option<(Str, Str)> {
    let is_comsub =
        |p: &WordPart| matches!(p, WordPart::CommandSub { body: CmdSubBody::Parsed { .. } });
    let mut items: Vec<ArrayElem> = items.to_vec();
    let mut sent = vec![0u8];
    while contains(&array_listing(&items), &sent) {
        sent.push(0);
    }
    let mut saved = WordPart::Literal(Str::new());
    let total = walk_elems(&mut items, &is_comsub, k, &mut |p| {
        saved = std::mem::replace(p, WordPart::Literal(sent.clone()));
    });
    if k >= total {
        return None;
    }
    let text = array_listing(&items);
    // Put the substitution back, and take its own rendering while it is to
    // hand: the text before the `)` is the listing up to the sentinel followed
    // by all of that rendering but its last character.
    let is_marker = |p: &WordPart| matches!(p, WordPart::Literal(s) if *s == sent);
    let mut whole = Str::new();
    walk_elems(&mut items, &is_marker, 0, &mut |p| {
        *p = std::mem::replace(&mut saved, WordPart::Literal(Str::new()));
        whole = part_src(p);
    });
    let at = find(&text, &sent)?;
    let mut before = text.get(..at)?.to_vec();
    before.push_str(whole.get(..whole.len().checked_sub(1)?)?);
    let after = text.get(at.checked_add(sent.len())?..)?.to_vec();
    Some((before, after))
}

/// Re-attach every `$( … )`'s tail across a whole compound assignment, in place
/// of the per-element tails [`attach_comsub_tails`] gave them.
///
/// A compound assignment is *one* word to bash, so a substitution inside one of
/// its elements is followed by the rest of that element, then the elements after
/// it, then the `)` that closes the literal — and that is what the re-parse of
/// its re-print is handed, so that is what a failure echoes:
///
/// ```text
/// a=( "p$(⏎!⏎)q" r ) echo hi     ->   `! )q" r)'
/// ```
///
/// Only a literal used as a **command prefix** ever shows it. A literal that is
/// really assigned never reaches the per-substitution re-parse at all — the
/// whole list is re-parsed first, under its own name; see
/// [`crate::interp::Shell::array_assign_reparse_error`].
pub(crate) fn attach_compound_comsub_tails(items: &mut [ArrayElem]) {
    let is_comsub =
        |p: &WordPart| matches!(p, WordPart::CommandSub { body: CmdSubBody::Parsed { .. } });
    let total = walk_elems(items, &is_comsub, usize::MAX, &mut |_| {});
    if total == 0 {
        return;
    }
    let mut sent = vec![0u8];
    while contains(&array_listing(items), &sent) {
        sent.push(0);
    }
    let is_marker = |p: &WordPart| matches!(p, WordPart::Literal(s) if *s == sent);
    for k in 0..total {
        let mut saved = WordPart::Literal(Str::new());
        walk_elems(items, &is_comsub, k, &mut |p| {
            saved = std::mem::replace(p, WordPart::Literal(sent.clone()));
        });
        let text = array_listing(items);
        let mut tail = find(&text, &sent)
            .and_then(|i| text.get(i.saturating_add(sent.len())..))
            .unwrap_or_default()
            .to_vec();
        // The `)` the container writes, exactly as `attach_comsub_tails` picks
        // up the `"` that closes a double-quoted run it was standing in.
        tail.push(b')');
        walk_elems(items, &is_marker, 0, &mut |p| {
            *p = std::mem::replace(&mut saved, WordPart::Literal(Str::new()));
            if let WordPart::CommandSub { body: CmdSubBody::Parsed { tail: t, .. } } = p {
                *t = Some(std::mem::take(&mut tail));
            }
        });
    }
}

/// Every `$( … )` in a compound assignment's value list, in the order
/// [`array_listing_split`] numbers them, each given as its stored re-print.
pub(crate) fn array_listing_comsubs(items: &[ArrayElem]) -> Vec<Str> {
    let mut out = Vec::new();
    let mut items: Vec<ArrayElem> = items.to_vec();
    let want = |p: &WordPart| matches!(p, WordPart::CommandSub { body: CmdSubBody::Parsed { .. } });
    let total = walk_elems(&mut items, &want, usize::MAX, &mut |_| {});
    for k in 0..total {
        walk_elems(&mut items, &want, k, &mut |p| {
            if let WordPart::CommandSub { body: CmdSubBody::Parsed { src, .. } } = p {
                out.push(src.clone());
            }
        });
    }
    out
}

/// [`walk_parts`] over every word of a compound assignment's element list, with
/// one shared counter — the order the listing writes them in, a keyed element's
/// subscript before its value.
fn walk_elems(
    items: &mut [ArrayElem],
    want: &dyn Fn(&WordPart) -> bool,
    n: usize,
    act: &mut dyn FnMut(&mut WordPart),
) -> usize {
    let mut i = 0usize;
    for e in items.iter_mut() {
        match e {
            ArrayElem::Positional(w) => walk_parts_in(&mut w.parts, want, n, &mut i, act),
            ArrayElem::Keyed { index, value, .. } => {
                walk_parts_in(&mut index.parts, want, n, &mut i, act);
                walk_parts_in(&mut value.parts, want, n, &mut i, act);
            }
        }
    }
    i
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

/// The way a whole *collection* is spelled: `name[@]` / `name[*]`, or the bare
/// `@` / `*` when the collection is the positional parameters.
///
/// The third of the [`name_sub`] family, for the parts that name a whole array
/// or the positionals in the same breath — [`WordPart::ArraySlice`],
/// [`WordPart::ArrayBulk`], and the diagnostic
/// [`crate::interp::Shell::bulk_elements`] rebuilds for a bad `@` transform,
/// which has to agree with what `part_src` would have printed.
#[must_use]
pub fn name_bulk(name: &str, star: bool) -> Str {
    if name == "@" || name == "*" {
        name.as_bytes().to_vec()
    } else {
        bfmt![name, b"[", if star { "*" } else { "@" }, b"]"]
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
/// reachable via `$'a\'b'` — is spliced out and re-added by
/// [`crate::escape::sh_single_quote`], the one quoter bash prints a word back
/// with, whose `\'` special case shows here as `f() { : $'\x27'; }` printing
/// back `: \'`.
fn quoted_lit_src(text: BStr<'_>, escaped: bool) -> Str {
    if escaped {
        // An escaped run with nothing in it is a backslash that escapes
        // nothing — a *dangling* one. The lexer never builds such a part (a
        // `\c` always carries its `c`); the one thing that does is
        // [`crate::ast::dup_move_source`], which takes the final byte off a
        // `>&x\-` because bash's parser takes it off the raw word. bash prints
        // the remainder back verbatim, backslash and all, so `1>&x\-` reads
        // back as it was written.
        if text.is_empty() {
            return Str::from(&b"\\"[..]);
        }
        let mut s = Str::with_capacity(text.len() * 2);
        for c in bytes::chars(text) {
            s.push(b'\\');
            c.push_to(&mut s);
        }
        return s;
    }
    crate::escape::sh_single_quote(text)
}

/// Fill every [`CmdSubBody::Parsed`]'s `tail` in `word` — the stored word text
/// that follows each `$( … )`'s closing `)`.
///
/// bash never hands a substitution's body to the parser in isolation. At
/// expansion time `expand_word_internal` is walking the *word's* stored string
/// and passes `extract_command_subst` the whole remainder of it, so the input
/// `xparse_dolparen` reads is the body's re-print, then `)`, then everything
/// left of the word. That only shows when the parse fails, because the
/// diagnostic echoes the line it was reading:
///
/// ```text
/// echo "a$(<newline>!<newline>)b$(echo c)d"   ->   `! )b$(echo c)d"'
/// ```
///
/// — the word's remaining text and its closing quote, and nothing of the
/// commands after the word. So the tail is a property of the word rather than
/// of the part, and cannot be filled where the part is built: `seg_to_part`
/// sees one segment and not its siblings. This is that post-pass.
///
/// It works by rendering the word with one substitution at a time swapped for a
/// sentinel, so the answer comes out of [`word_src`] itself rather than out of a
/// second renderer written to agree with it. Rendering the word whole is also
/// what makes the enclosing quote fall out for free — the `"` that closes a
/// [`WordPart::DoubleQuoted`] is written by the container, not by anything
/// inside it, and the same holds for the `}` of a `${x:-$( … )}`.
///
/// A [`CmdSubBody::Parsed`] whose `tail` stays `None` is one there was no
/// re-print for, and so nothing for this pass to measure a remainder against;
/// see [`crate::ast::CmdSubBody::Parsed::tail`]. That is not the same thing as
/// "the parser never saw the word": a word the shell assembles at expansion time
/// (`${x@P}`, `PS4`) has no re-print either, but its substitutions *are* re-read
/// — `extract_command_subst` is walking that string too — so
/// [`crate::parser::dquote_word_from_source`] runs this pass over its
/// [`CmdSubBody::Unread`] bodies exactly as the parser's words are run over
/// theirs.
pub(crate) fn attach_comsub_tails(word: &mut Word) {
    attach_comsub_tails_in(&mut word.parts);
}

/// [`attach_comsub_tails`] for **one string scope**.
///
/// A `[ … ]` subscript is not part of the word's string for this purpose. The
/// `${ … }` scan steps over one whole ([`Nested::Index`]) and the text is
/// expanded later as a string in its own right, so `extract_command_subst` there
/// is walking the *subscript*, and the remainder it echoes is the subscript's.
/// Measured against bash 5.2.37 with `a=(0 1 2)` and `${b@P}`:
///
/// ```text
/// b='A${a[p$(fi)q]}B'   ->   `fi)q'    and   `fi)'
/// ```
///
/// — `q` and nothing else, where the word's remainder would have been
/// `q]}B`. (The second line is the body's own run, one byte shorter because the
/// extent read gave up at the end of the string and `xparse_dolparen` returns
/// `ep - base - 1`; see [`crate::interp::Shell::extent_consumed`].) The word's
/// `B` is still printed, `A0B`, because the scope that was consumed was the
/// subscript's.
fn attach_comsub_tails_in(parts: &mut [WordPart]) {
    // Each subscript first, as its own string; the pass below then leaves them
    // alone.
    index_scopes(parts, &mut attach_comsub_tails_in);
    // A body no parser read is re-read the same way, from the same
    // `extract_command_subst`, so it wants the same remainder — see
    // [`crate::ast::CmdSubBody::Unread`].
    let is_comsub = |p: &WordPart| {
        matches!(
            p,
            WordPart::CommandSub {
                body: CmdSubBody::Parsed { .. } | CmdSubBody::Unread { .. }
            }
        )
    };
    attach_tails_by(parts, &is_comsub, &mut |p, tail| match p {
        WordPart::CommandSub { body: CmdSubBody::Parsed { tail: t, .. } } => *t = Some(tail),
        WordPart::CommandSub { body: CmdSubBody::Unread { tail: t, .. } } => *t = tail,
        _ => {}
    });
    // A `$(( … ))` wants the same remainder, and for a stronger reason: its
    // extent is not the parser's `))` at all but wherever
    // `extract_delimited_string`'s paren count stops, which depends on
    // everything after it. See [`crate::ast::WordPart::ArithSub::tail`].
    //
    // A second pass rather than a second row in `is_comsub`, because
    // [`walk_parts`] does not descend into a part it has matched — and a
    // `$( … )` *inside* the arithmetic needs its own remainder too, which runs
    // past the `))`.
    let is_arith = |p: &WordPart| matches!(p, WordPart::ArithSub { .. });
    attach_tails_by(parts, &is_arith, &mut |p, tail| {
        if let WordPart::ArithSub { tail: t, .. } = p {
            *t = tail;
        }
    });
    // And so does a `$((` whose body osh's lexer read as a command substitution
    // instead: bash makes no such decision at parse time, so the count still has
    // to be run over it here — and it can stop somewhere the lexer's balance did
    // not. See [`crate::ast::CmdSubBody::ArithFallback::tail`].
    //
    // A third pass for the same reason the second is separate from the first: an
    // arithmetic can hold one of these, and [`walk_parts`] stops at the part it
    // matched.
    let is_fallback = |p: &WordPart| {
        matches!(p, WordPart::CommandSub { body: CmdSubBody::ArithFallback { .. } })
    };
    attach_tails_by(parts, &is_fallback, &mut |p, tail| {
        if let WordPart::CommandSub { body: CmdSubBody::ArithFallback { tail: t, .. } } = p {
            *t = tail;
        }
    });
}

/// The sentinel-swap [`attach_comsub_tails_in`] runs once per kind of part that
/// needs to know what follows it: replace the `k`-th part `want` accepts by a
/// marker, render the whole word, and hand `set` everything after the marker.
fn attach_tails_by(
    parts: &mut [WordPart],
    want: &dyn Fn(&WordPart) -> bool,
    set: &mut dyn FnMut(&mut WordPart, Str),
) {
    let total = walk_parts(parts, want, usize::MAX, &mut |_| {});
    if total == 0 {
        return;
    }
    // The sentinel has to be a byte string the rendering does not already
    // contain, and a shell word may hold any byte — so it is grown until it is
    // absent rather than assumed. Locating each substitution by its own rendered
    // text instead would be ambiguous twice over: two identical `$( … )`s in one
    // word render alike, and `'$(! )'` renders a literal that looks like one.
    let mut sent = vec![0u8];
    while contains(&parts_src(parts), &sent) {
        sent.push(0);
    }
    // The swapped-in marker is not a substitution any more, so it is found again
    // by its own shape rather than by the comsub count — which the swap has just
    // changed under us.
    let is_marker = |p: &WordPart| matches!(p, WordPart::Literal(s) if *s == sent);
    for k in 0..total {
        // Swapping the whole part out — not just its body's text — is what
        // makes this exact: `part_src` renders a parsed body from `prog`, so
        // there is nothing in `src` for a marker to ride in on.
        let mut saved = WordPart::Literal(Str::new());
        walk_parts(parts, want, k, &mut |p| {
            saved = std::mem::replace(p, WordPart::Literal(sent.clone()));
        });
        let text = parts_src(parts);
        let mut tail = find(&text, &sent)
            .and_then(|i| text.get(i.saturating_add(sent.len())..))
            .unwrap_or_default()
            .to_vec();
        walk_parts(parts, &is_marker, 0, &mut |p| {
            *p = std::mem::replace(&mut saved, WordPart::Literal(Str::new()));
            set(p, std::mem::take(&mut tail));
        });
    }
}

/// `part` with the tails inside each of its `${ … }` sub-words re-measured
/// against that sub-word alone — or `None` when there is nothing in one to
/// re-measure, which is the usual case and costs no clone.
///
/// [`attach_comsub_tails`] fills every tail against the **word**, because the
/// reader it is filled for is `extract_dollar_brace_string`
/// ([`crate::interp::Shell::brace_extent_scan`]), which is walking the word's
/// own string and reads a `$(` between the braces at any depth. That is the
/// only reader with that view. Every other one is handed a sub-word that the
/// scan already cut out — `parameter_brace_expand_word` gets `value`,
/// `parameter_brace_remove_pattern` gets `patstr` — and calls
/// `expand_word_internal` on *that*, so `extract_command_subst` there walks the
/// sub-word and stops at its end.
///
/// Measured against bash 5.2.37, with `z=zz` and `${b@P}`:
///
/// ```text
/// b='A${z#p$(fi⏎q)r}B'   ->   `fi⏎q)r}B'   from the scan
///                             `fi⏎q)r'     from the pattern's own expansion
///                        and  AzzB
/// ```
///
/// — the `}B` belongs to the first read only. Reusing the word-scoped tail for
/// the second ran its leftover past the `}` and swallowed the word's `B`.
///
/// Applied where the scan is, and for the same reason: the scan is the last
/// reader that sees the word's string, so everything after it is scoped to a
/// sub-word.
pub(crate) fn rescoped_part(part: &WordPart) -> Option<WordPart> {
    if !operand_holds_sub(part) {
        return None;
    }
    let mut out = part.clone();
    for (kind, w) in nested_parts_mut(&mut out) {
        if kind == Nested::Operand {
            attach_comsub_tails_in(w);
        }
    }
    Some(out)
}

/// [`rescoped_part`] for a slice, for the two list recognisers that reach a
/// `${ … }` without going through
/// [`crate::interp::Shell::expand_dynamic_with`].
pub(crate) fn rescoped_parts(parts: &[WordPart]) -> Option<Vec<WordPart>> {
    if !parts.iter().any(operand_holds_sub) {
        return None;
    }
    Some(parts.iter().map(|p| rescoped_part(p).unwrap_or_else(|| p.clone())).collect())
}

/// Whether any `${ … }` sub-word of `part` holds a construct that carries a
/// tail, which is the only thing [`rescoped_part`] would change.
fn operand_holds_sub(part: &WordPart) -> bool {
    nested_parts(part).into_iter().any(|(kind, w)| kind == Nested::Operand && holds_sub(w))
}

/// Whether `parts` holds a `$( … )` or a `$(( … ))` anywhere in this string
/// scope — a `[ … ]` subscript is one of its own and is not looked into, having
/// had its tails measured against itself all along.
fn holds_sub(parts: &[WordPart]) -> bool {
    parts.iter().any(|p| {
        matches!(p, WordPart::CommandSub { .. } | WordPart::ArithSub { .. })
            || nested_parts(p).into_iter().any(|(k, w)| k != Nested::Index && holds_sub(w))
    })
}

/// Whether `hay` contains `needle`.
fn contains(hay: BStr<'_>, needle: BStr<'_>) -> bool {
    find(hay, needle).is_some()
}

/// The offset of `needle` in `hay`, or `None`.
fn find(hay: BStr<'_>, needle: BStr<'_>) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Apply `act` to every `[ … ]` subscript reachable from `parts` **without
/// crossing another one** — the string scopes nested one level inside this one.
///
/// A subscript inside a subscript is not visited here; it is reached by the
/// recursion `act` itself makes, so each scope is walked by the call that owns
/// it. See [`Nested::Index`].
fn index_scopes(parts: &mut [WordPart], act: &mut dyn FnMut(&mut [WordPart])) {
    for p in parts.iter_mut() {
        for (kind, w) in nested_parts_mut(p) {
            if kind == Nested::Index {
                act(w);
            } else {
                index_scopes(w, act);
            }
        }
    }
}

/// Apply `act` to the `n`-th part of `parts` — or of any word nested inside one
/// — that `want` accepts, and return how many such parts there were in all.
/// Passing `usize::MAX` for `n` therefore just counts.
///
/// A `[ … ]` subscript is **not** walked into: it is its own string scope, and
/// [`index_scopes`] hands it to its own pass. See [`attach_comsub_tails_in`].
///
/// The order is the order the parts are written in, which is all this needs:
/// [`attach_comsub_tails`] addresses one substitution at a time and finds it
/// again by its sentinel, so nothing depends on the walk agreeing with
/// `part_src` about *where* a part renders.
fn walk_parts(
    parts: &mut [WordPart],
    want: &dyn Fn(&WordPart) -> bool,
    n: usize,
    act: &mut dyn FnMut(&mut WordPart),
) -> usize {
    let mut i = 0usize;
    walk_parts_in(parts, want, n, &mut i, act);
    i
}

fn walk_parts_in(
    parts: &mut [WordPart],
    want: &dyn Fn(&WordPart) -> bool,
    n: usize,
    i: &mut usize,
    act: &mut dyn FnMut(&mut WordPart),
) {
    for p in parts.iter_mut() {
        if want(p) {
            if *i == n {
                act(p);
            }
            *i = i.saturating_add(1);
            continue;
        }
        for (kind, w) in nested_parts_mut(p) {
            if kind == Nested::Index {
                continue;
            }
            walk_parts_in(w, want, n, i, act);
        }
    }
}

/// Where a nested part list sits inside the part that holds it, which is what
/// decides whether bash's `${ … }` scan walks through it. See [`nested_parts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Nested {
    /// A `[ … ]` subscript. `extract_dollar_brace_string` steps over one whole —
    ///
    /// ```c
    ///   if (c == LBRACK && dolbrace_state == DOLBRACE_PARAM)
    ///     { si = skipsubscript (string, i, 0); … }     /* subst.c:1940-1946 */
    /// ```
    ///
    /// — at any depth, because `${` sets `DOLBRACE_PARAM` again for each nested
    /// brace. The subscript text is expanded later as a string in its own right,
    /// so a `$( … )` in one is read there instead: measured against bash 5.2.37,
    /// `${a[$(fi)]}` reports twice at the *subscript's* scope and the brace still
    /// closes (`A0B`), where `${y:-${a[$(fi)]}}` reports not at all.
    Index,
    /// A sub-word of a `${ … }`: an operand, a pattern, a replacement, the
    /// bounds of a substring. The scan walks *through* one — it is between the
    /// braces, and `extract_dollar_brace_string` reads every `$(` there — but
    /// the expansion does not: `parameter_brace_expand` cuts the sub-word out as
    /// a string (`value`, `patstr`, `substr`) and hands it to a fresh
    /// `expand_word_internal`. So the two readers walk different strings, and a
    /// `$( … )` in here has a different remainder for each. See
    /// [`rescoped_part`].
    Operand,
    /// A `" … "` run, which the scan walks through *and* the expansion does:
    /// there is no second string, the quotes are simply characters
    /// `expand_word_internal` passes over. `echo "a$(⏎!⏎)b$(echo c)d"` echoes
    /// `` `! )b$(echo c)d"' `` — the word's remainder and its closing quote.
    Quoted,
}

/// Define [`nested_parts`] and [`nested_parts_mut`] from one match.
///
/// The two walks want the same answer with opposite mutability, and the match
/// carries a rule that has to be obeyed in exactly one place: it is deliberately
/// exhaustive, because a new [`WordPart`] that can hold a word has to be
/// *considered* here. A `$( … )` inside one this missed would be given no tail
/// by [`attach_comsub_tails`] — and so would echo a truncated line if its
/// re-print ever failed to parse — and would be stepped over by
/// [`crate::interp::Shell::brace_extent_scan`], which is bash's `${ … }` scan
/// and reads every `$(` between the braces however deeply it is nested.
macro_rules! nested_parts_fn {
    ($name:ident, $slice:ident, $deref:ident, $asref:ident, $from:ident, $($m:tt)?) => {
        pub(crate) fn $name(p: &$($m)? WordPart) -> Vec<(Nested, &$($m)? [WordPart])> {
            fn idx(i: &$($m)? Option<Box<Word>>) -> Option<(Nested, &$($m)? [WordPart])> {
                i.$deref().map(|w| (Nested::Index, w.parts.$slice()))
            }
            fn aidx(i: &$($m)? ArrayIndex) -> Option<(Nested, &$($m)? [WordPart])> {
                match i {
                    ArrayIndex::Index(w) => Some((Nested::Index, w.parts.$slice())),
                    ArrayIndex::All | ArrayIndex::Star => None,
                }
            }
            fn arg(p: &$($m)? [WordPart]) -> (Nested, &$($m)? [WordPart]) {
                (Nested::Operand, p)
            }
            match p {
                WordPart::DoubleQuoted(parts) => vec![(Nested::Quoted, parts.$slice())],
                WordPart::ParamOp { index, arg: a, .. } => {
                    idx(index).into_iter().chain([arg(a.parts.$slice())]).collect()
                }
                // `ArrayOp` carries no subscript *word*: it applies to the array
                // as a whole (`[@]`/`[*]`), recorded as the `star` flag.
                WordPart::ArrayOp { arg: a, .. } => vec![arg(a.parts.$slice())],
                WordPart::ParamTrim { index, pattern, .. }
                | WordPart::ParamCase { index, pattern, .. } => {
                    idx(index).into_iter().chain([arg(pattern.parts.$slice())]).collect()
                }
                WordPart::ParamSubstr { index, offset, length, .. } => idx(index)
                    .into_iter()
                    .chain([arg(offset.parts.$slice())])
                    .chain(length.$deref().map(|w| arg(w.parts.$slice())))
                    .collect(),
                WordPart::ParamReplace { index, pattern, replacement, .. } => idx(index)
                    .into_iter()
                    .chain([arg(pattern.parts.$slice())])
                    .chain(replacement.$deref().map(|w| arg(w.parts.$slice())))
                    .collect(),
                WordPart::Indirect { index, .. } => {
                    index.$asref().and_then(aidx).into_iter().collect()
                }
                WordPart::ArrayRef { index, .. } => aidx(index).into_iter().collect(),
                WordPart::IndirectOp { index, target, .. } => index
                    .$asref()
                    .and_then(aidx)
                    .into_iter()
                    .chain([arg(std::slice::$from(target.$asref()))])
                    .collect(),
                WordPart::ParamTransform { index, .. } => idx(index).into_iter().collect(),
                WordPart::BadTransform { index, op, .. } => idx(index)
                    .into_iter()
                    .chain([arg(op.parts.$slice())])
                    .collect(),
                WordPart::ArraySlice { offset, length, .. } => [arg(offset.parts.$slice())]
                    .into_iter()
                    .chain(length.$deref().map(|w| arg(w.parts.$slice())))
                    .collect(),
                WordPart::ArrayBulk { op, .. } => match op {
                    BulkOp::Trim { pattern, .. } | BulkOp::Case { pattern, .. } => {
                        vec![arg(pattern.parts.$slice())]
                    }
                    BulkOp::Replace { pattern, replacement, .. } => {
                        [arg(pattern.parts.$slice())]
                            .into_iter()
                            .chain(replacement.$deref().map(|w| arg(w.parts.$slice())))
                            .collect()
                    }
                    BulkOp::Transform { .. } => Vec::new(),
                    BulkOp::BadTransform { op } => vec![arg(op.parts.$slice())],
                },
                // The arithmetic text in parts — literal runs and the `$( … )`
                // the expansion-time scan reads in it. See
                // [`crate::ast::WordPart::ArithSub::parts`].
                WordPart::ArithSub { parts, .. } => vec![arg(parts.$slice())],
                // A process substitution's body is a `Program`, not a word, and
                // bash never re-reads it through this path anyway.
                WordPart::Literal(_)
                | WordPart::SingleQuoted { .. }
                | WordPart::Param { .. }
                | WordPart::VarNames { .. }
                | WordPart::CommandSub { .. }
                | WordPart::Length(_)
                | WordPart::ArrayKeys { .. }
                | WordPart::BadSubst(_)
                | WordPart::TokenText(_)
                // An unclosed construct holds only the text a diagnostic echoes
                // back.
                | WordPart::Unclosed(_)
                | WordPart::ProcSub { .. } => Vec::new(),
            }
        }
    };
}

nested_parts_fn!(nested_parts, as_slice, as_deref, as_ref, from_ref,);
nested_parts_fn!(nested_parts_mut, as_mut_slice, as_deref_mut, as_mut, from_mut, mut);

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
            // The separator is printed only where the source had one: bash
            // re-prints a word from its saved text, so `${q/ab}` keeps its
            // shape and does not acquire the slash of `${q/ab/}`.
            bfmt![
                b"${",
                &name_sub(name, index),
                op,
                &word_src(pattern),
                &repl_src(replacement),
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
            // Nor was a `$((` that fell back to a substitution — and its text
            // still carries the inner `(` the scan counted, so a plain `$(` … `)`
            // around it reproduces the `$(( … )` the source wrote.
            CmdSubBody::ArithFallback { src, .. } => bfmt![b"$(", src, b")"],
            // Nor was one written in text no parser read as a word — a
            // here-document body. bash prints the here-document back from the
            // very text its reader collected, and `parse_comsub` never ran over
            // any of it, so there is no re-print to print instead.
            // A `$(` with no mate prints back with none either: the source held
            // no `)` and this text *is* the source.
            CmdSubBody::Unread { src, closed, .. } => {
                bfmt![b"$(", src, if *closed { b")".as_slice() } else { b"" }]
            }
            CmdSubBody::Parsed { prog, .. } => comsub_reprint(b"$(", prog),
        },
        // Read by the same `parse_comsub` call the `$( … )` spelling gets —
        // parse.y:5028's comment names all three, `$(...)`, `<(...)` and
        // `>(...)`, and 5042 is the one call — so it is re-printed the same way,
        // leading-space guard included.
        WordPart::ProcSub { input, body } => {
            comsub_reprint(if *input { b"<(" } else { b">(" }, body)
        }
        // Rendered from the parts rather than from `expr`, which is the same
        // bytes: that is what lets `attach_comsub_tails` swap one of the
        // `$( … )` in here for its sentinel and read the remainder off the
        // rendering of the whole word. See
        // [`crate::ast::WordPart::ArithSub::parts`].
        WordPart::ArithSub { bracket, parts, .. } => {
            let text = parts_src(parts);
            if *bracket {
                bfmt![b"$[", &text, b"]"]
            } else {
                bfmt![b"$((", &text, b"))"]
            }
        }
        WordPart::BadSubst(raw) => bfmt![b"${", raw, b"}"],
        // Held as written, delimiters and all — there is no closing one to put
        // back, which is the whole point of it.
        WordPart::Unclosed(u) => u.src().to_vec(),
        // The whole word's text, already cut — nothing to put back around it.
        WordPart::TokenText(raw) => raw.clone(),
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
        // Rebuilt like every other operator's — name, any subscript, the `@`,
        // then the operand text that was not a valid operator (`x@`, `a[0]@Z`).
        WordPart::BadTransform { name, index, op } => {
            bfmt![b"${", &name_sub(name, index), b"@", &word_src(op), b"}"]
        }
        WordPart::ArraySlice { name, star, offset, length } => {
            let sub = name_bulk(name, *star);
            let mut s = bfmt![b"${", &sub, b":", &word_src(offset)];
            if let Some(len) = length {
                s.push(b':');
                s.push_str(&word_src(len));
            }
            s.push(b'}');
            s
        }
        WordPart::ArrayBulk { name, star, op } => {
            let sub = name_bulk(name, *star);
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
                    bfmt![o, &word_src(pattern), &repl_src(replacement)]
                }
                BulkOp::Case { mode, all, pattern } => {
                    bfmt![case_op_src(*mode, *all), &word_src(pattern)]
                }
                BulkOp::Transform { op } => bfmt![b"@", *op],
                BulkOp::BadTransform { op } => bfmt![b"@", &word_src(op)],
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

    /// The elements of `a=( … )` in the one program `src` parses to.
    fn items_of(src: &str) -> Vec<ArrayElem> {
        let prog = parse(src.as_bytes()).expect("parse");
        for item in &prog.items {
            for cmd in &item.list.first.commands {
                if let Command::Simple(sc) = cmd {
                    for a in sc.assignments.iter().chain(sc.decl_arrays.iter().map(|d| &d.assign)) {
                        if let AssignRhs::Array(items) = &a.value {
                            return items.clone();
                        }
                    }
                }
            }
        }
        panic!("no compound assignment in {src:?}");
    }

    /// The `tail` every `$( … )` in a compound assignment was given, in walk
    /// order.
    fn elem_tails(items: &[ArrayElem]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut items = items.to_vec();
        let want =
            |p: &WordPart| matches!(p, WordPart::CommandSub { body: CmdSubBody::Parsed { .. } });
        let total = walk_elems(&mut items, &want, usize::MAX, &mut |_| {});
        for k in 0..total {
            walk_elems(&mut items, &want, k, &mut |p| {
                if let WordPart::CommandSub { body: CmdSubBody::Parsed { tail, .. } } = p {
                    out.push(text(tail.clone().expect("a parsed word has a tail")));
                }
            });
        }
        out
    }

    /// bash keeps a compound assignment as **one word**, joined by single
    /// spaces — `parse_compound_assignment` (parse.y:4715) writes the elements
    /// back that way — so a `$( … )` in it is followed by the rest of the whole
    /// *literal*, closing `)` and all, and not merely by the rest of its own
    /// element. Measured against bash 5.2.37.
    #[test]
    fn a_compound_assignments_tail_runs_to_the_literals_closing_paren() {
        let items = items_of("a=( one\n  two\n  \"p$(\n!\n)q\"\n  four )");
        assert_eq!(text(array_listing(&items)), r#"one two "p$(! )q" four"#);
        assert_eq!(elem_tails(&items), vec![r#"q" four)"#]);

        // What a prefix assignment's failed re-parse echoes, which is where the
        // tail is observable: bash gives `! )q" r)'.
        assert_eq!(elem_tails(&items_of("a=( \"p$(\n!\n)q\" r ) echo hi")), vec![r#"q" r)"#]);

        // A subscript is walked before its value, and the `]=` between them is
        // part of the listing — so a substitution in a *subscript* runs on
        // through the value, and a later one still ends at the same `)`. bash,
        // for `a=( [$(⏎!⏎)]=x $(⏎!⏎) ) echo hi`:  `! )]=x $(! ))'
        let keyed = items_of("a=( [$(echo k)]=$(echo v) )");
        assert_eq!(text(array_listing(&keyed)), "[$(echo k)]=$(echo v)");
        assert_eq!(elem_tails(&keyed), vec!["]=$(echo v))", ")"]);
    }

    /// `assign_compound_array_list` re-reads the listing before expanding any of
    /// it (arrayfunc.c:587), so a substitution whose re-print will not parse is
    /// blamed at *its* place in the listing. The split is what locates it: the
    /// line is the newlines the listing kept before it, and the echoed line runs
    /// from the last of them to the first one after.
    #[test]
    fn a_compound_assignment_is_split_at_the_failing_substitution() {
        let items = items_of("a=( one\n  two\n  \"p$(\n!\n)q\"\n  four )");
        let (before, after) = array_listing_split(&items, 0).expect("one substitution");
        assert_eq!(text(before), r#"one two "p$(! "#);
        assert_eq!(text(after), r#"q" four"#);
        assert_eq!(array_listing_split(&items, 1), None, "there is only the one");
        assert_eq!(comsub_srcs(&items), vec!["! "]);

        // Walk order is subscript before value, and the last substitution's
        // `after` is empty because nothing follows it in the listing.
        let keyed = items_of("a=( [$(echo k)]=$(echo v) )");
        assert_eq!(comsub_srcs(&keyed), vec!["echo k", "echo v"]);
        let (before, after) = array_listing_split(&keyed, 1).expect("two substitutions");
        assert_eq!(text(before), "[$(echo k)]=$(echo v");
        assert_eq!(text(after), "");
    }

    /// The re-print of every `$( … )` in a compound assignment, in walk order.
    fn comsub_srcs(items: &[ArrayElem]) -> Vec<String> {
        array_listing_comsubs(items).into_iter().map(text).collect()
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

    /// A function body is a whole `shell_command`, and `declare -f` shows which
    /// node it became.
    ///
    /// bash's production is `function_body: shell_command | shell_command
    /// redirection_list` (parse.y), so all eleven compound commands define. The
    /// brace group is the one arm `make_function_def` takes the command of
    /// directly, so it comes back as *one* pair of braces and a redirection
    /// written after it lands on the function; every other arm keeps its own
    /// node, and its own redirections, inside the braces the printer always
    /// adds. Every expectation is bash 5.2.37's own `declare -f`.
    #[test]
    fn a_function_body_is_a_whole_shell_command() {
        // The redirection's owner is the whole point: same list, same place in
        // the source, printed outside the brace group and inside everything
        // else.
        assert_eq!(
            dump_fn("f() { echo out; } >/dev/null", "f"),
            "f () \n{ \n    echo out\n} > /dev/null\n"
        );
        assert_eq!(
            dump_fn("f() ( echo out ) >/dev/null", "f"),
            "f () \n{ \n    ( echo out ) > /dev/null\n}\n"
        );
        assert_eq!(dump_fn("f() ((1)) >/dev/null", "f"), "f () \n{ \n    ((1)) > /dev/null\n}\n");
        assert_eq!(
            dump_fn("f() if true; then echo out; fi >/dev/null", "f"),
            "f () \n{ \n    if true; then\n        echo out;\n    fi > /dev/null\n}\n"
        );
        // The arms osh used to reject outright.
        assert_eq!(
            dump_fn("f() case x in x) echo m;; esac", "f"),
            "f () \n{ \n    case x in \n        x)\n            echo m\n        ;;\n    esac\n}\n"
        );
        assert_eq!(dump_fn("f() ((1))", "f"), "f () \n{ \n    ((1))\n}\n");
        // A one-word conditional is re-printed as the `-n` test it means.
        assert_eq!(dump_fn("f() [[ a ]]", "f"), "f () \n{ \n    [[ -n a ]]\n}\n");
        assert_eq!(
            dump_fn("f() while false; do :; done", "f"),
            "f () \n{ \n    while false; do\n        :;\n    done\n}\n"
        );
        assert_eq!(
            dump_fn("f() for x in a b; do echo $x; done", "f"),
            "f () \n{ \n    for x in a b;\n    do\n        echo $x;\n    done\n}\n"
        );
        // The `function` keyword form reaches the same bodies, with or without
        // the optional `()`.
        assert_eq!(
            dump_fn("function f if true; then echo hi; fi", "f"),
            "f () \n{ \n    if true; then\n        echo hi;\n    fi\n}\n"
        );
        assert_eq!(dump_fn("function f () ((1))", "f"), "f () \n{ \n    ((1))\n}\n");
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
                .unwrap_or_else(|e| panic!("re-parse {reparsed:?}: {}", String::from_utf8_lossy(&e.msg())));
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
