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
//! inline with `;` separators. One deliberate simplification: here-documents are
//! re-emitted as here-strings (`<<< …`), which deliver the same bytes to stdin;
//! see known-issues TD-OILS16/TD-OILS18.

use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseMode, Command,
    CondBinOp, CondExpr, Item, ParamOp, Pipeline, Program, Redirect, RedirectOp, ReplaceAnchor,
    SimpleCommand, UnaryOp, Word, WordPart,
};

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

/// One indentation level (bash uses 4 spaces in `declare -f` output).
fn ind(level: usize) -> String {
    "    ".repeat(level)
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
pub fn unparse_function(name: &str, body: &Program, redirects: &[Redirect]) -> String {
    let mut s = String::new();
    s.push_str(name);
    // bash prints the opening brace on its own line with a trailing space
    // (`{ \n`), matching `declare -f` / `type` output byte-for-byte.
    s.push_str(" () \n{ \n");
    let inner = program_block(body, 1, false);
    if inner.is_empty() {
        // An empty body still needs a no-op so it re-parses.
        s.push_str(&ind(1));
        s.push(':');
        s.push('\n');
    } else {
        s.push_str(&inner);
        if !inner.ends_with('\n') {
            s.push('\n');
        }
    }
    // Redirections attached to the definition (`f() { …; } >log`) render on
    // the closing-brace line: `} > log`, matching bash's `declare -f`.
    s.push('}');
    for r in redirects {
        s.push(' ');
        s.push_str(&redirect_src(r));
    }
    s.push('\n');
    s
}

/// Render a whole program as an indented block: one item per line at `level`.
///
/// `terminate_last` controls the trailing separator on the final statement, to
/// match bash's `declare -f` deparser: a compound *clause* body (`then`/`else`/
/// `do`) terminates every statement — including the last — with `;`, whereas a
/// group body (`{ … }`, a subshell, the function body itself, and `case`
/// clauses) leaves the last statement unterminated. Non-final statements always
/// take a `;` separator (a backgrounded statement's ` &` is its own separator).
#[must_use]
pub fn program_block(prog: &Program, level: usize, terminate_last: bool) -> String {
    let mut out = String::new();
    let n = prog.items.len();
    for (i, item) in prog.items.iter().enumerate() {
        out.push_str(&ind(level));
        out.push_str(&item_stmt(item, level));
        let is_last = i + 1 == n;
        // `&` already terminates a backgrounded statement; otherwise separate
        // with `;`, and terminate the last one only in clause-body context.
        if !item.background && (!is_last || terminate_last) {
            out.push(';');
        }
        out.push('\n');
    }
    out
}

/// Render a program inline (single logical line), items joined by `; `. Used for
/// conditions (`if <here>; then …`) and command substitutions.
#[must_use]
pub fn program_inline(prog: &Program) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in &prog.items {
        let mut s = and_or_src(&item.list);
        if item.background {
            s.push_str(" &");
        }
        parts.push(s);
    }
    parts.join("; ")
}

/// One statement (and-or list, plus a trailing ` &` when backgrounded). The
/// first line carries no leading indent (the caller supplies it); nested lines
/// are indented to `level`.
fn item_stmt(item: &Item, level: usize) -> String {
    let mut s = and_or_block(&item.list, level);
    if item.background {
        s.push_str(" &");
    }
    s
}

/// And-or list where the first pipeline may be a multi-line compound command.
fn and_or_block(ao: &AndOr, level: usize) -> String {
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

/// And-or list rendered strictly inline (for conditions / command subs).
fn and_or_src(ao: &AndOr) -> String {
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

fn pipeline_prefix(pl: &Pipeline) -> String {
    let mut s = String::new();
    if pl.timed {
        s.push_str(if pl.time_posix { "time -p " } else { "time " });
    }
    if pl.negated {
        s.push_str("! ");
    }
    s
}

/// Pipeline where each command may be a multi-line compound command.
fn pipeline_block(pl: &Pipeline, level: usize) -> String {
    let mut s = pipeline_prefix(pl);
    let cmds: Vec<String> = pl.commands.iter().map(|c| command_block(c, level)).collect();
    s.push_str(&cmds.join(" | "));
    s
}

/// Pipeline rendered strictly inline.
fn pipeline_src(pl: &Pipeline) -> String {
    let mut s = pipeline_prefix(pl);
    let cmds: Vec<String> = pl.commands.iter().map(command_inline).collect();
    s.push_str(&cmds.join(" | "));
    s
}

/// Render a command as a (possibly multi-line) block. The first line has no
/// leading indent; continuation lines are indented at `level`, bodies at
/// `level + 1`.
fn command_block(cmd: &Command, level: usize) -> String {
    match cmd {
        Command::Simple(sc) => simple_src(sc),
        Command::If(c) => {
            let mut s = String::from("if ");
            s.push_str(&program_inline(&c.cond));
            s.push_str("; then\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            for (econd, ebody) in &c.elifs {
                s.push_str(&ind(level));
                s.push_str("elif ");
                s.push_str(&program_inline(econd));
                s.push_str("; then\n");
                s.push_str(&program_block(ebody, level + 1, true));
            }
            if let Some(eb) = &c.else_body {
                s.push_str(&ind(level));
                s.push_str("else\n");
                s.push_str(&program_block(eb, level + 1, true));
            }
            s.push_str(&ind(level));
            s.push_str("fi");
            s
        }
        Command::Loop(c) => {
            // `while`/`until` keep `do` on the same line as the condition
            // (`while COND; do`), unlike `for`/`select` (see below).
            let mut s = String::from(if c.until { "until " } else { "while " });
            s.push_str(&program_inline(&c.cond));
            s.push_str("; do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&ind(level));
            s.push_str("done");
            s
        }
        Command::For(c) => {
            // bash's deparser puts `do` on its own line for `for` (the word list
            // is terminated with `;`, then `do` at the loop's indent level).
            let mut s = format!("for {}", c.var);
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str(";\n");
            s.push_str(&ind(level));
            s.push_str("do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&ind(level));
            s.push_str("done");
            s
        }
        Command::ForArith(c) => {
            // `for ((init; cond; upd))` with no inner-paren padding and `do` on
            // its own line, matching bash.
            let mut s = format!("for (({}; {}; {}))\n", c.init, c.cond, c.update);
            s.push_str(&ind(level));
            s.push_str("do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&ind(level));
            s.push_str("done");
            s
        }
        Command::Select(c) => {
            let mut s = format!("select {}", c.var);
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str(";\n");
            s.push_str(&ind(level));
            s.push_str("do\n");
            s.push_str(&program_block(&c.body, level + 1, true));
            s.push_str(&ind(level));
            s.push_str("done");
            s
        }
        Command::Function(f) => {
            let mut s = format!("{} () \n", f.name);
            s.push_str(&ind(level));
            s.push_str("{ \n");
            s.push_str(&program_block(&f.body, level + 1, false));
            s.push_str(&ind(level));
            s.push('}');
            for r in &f.redirects {
                s.push(' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
        Command::Case(c) => {
            // bash prints `case WORD in ` with a trailing space before the
            // newline.
            let mut s = format!("case {} in \n", word_src(&c.word));
            for item in &c.items {
                let pats: Vec<String> = item.patterns.iter().map(word_src).collect();
                s.push_str(&ind(level + 1));
                s.push_str(&pats.join("|"));
                s.push_str(")\n");
                s.push_str(&program_block(&item.body, level + 2, false));
                s.push_str(&ind(level + 1));
                s.push_str(match item.term {
                    crate::ast::CaseTerm::Break => ";;",
                    crate::ast::CaseTerm::FallThrough => ";&",
                    crate::ast::CaseTerm::ContinueMatch => ";;&",
                });
                s.push('\n');
            }
            s.push_str(&ind(level));
            s.push_str("esac");
            s
        }
        Command::BraceGroup(prog) => {
            // bash prints the opening brace with a trailing space (`{ `).
            let mut s = String::from("{ \n");
            s.push_str(&program_block(prog, level + 1, false));
            s.push_str(&ind(level));
            s.push('}');
            s
        }
        Command::Subshell(prog) => {
            let mut s = String::from("(\n");
            s.push_str(&program_block(prog, level + 1, false));
            s.push_str(&ind(level));
            s.push(')');
            s
        }
        Command::Cond(expr) => format!("[[ {} ]]", cond_src(expr)),
        Command::Arith(text) => format!("(( {text} ))"),
        Command::Coproc { name, body } => {
            let mut s = String::from("coproc ");
            if let Some(n) = name {
                s.push_str(n);
                s.push(' ');
            }
            s.push_str(&command_block(body, level));
            s
        }
        Command::Redirected { inner, redirects } => {
            let mut s = command_block(inner, level);
            for r in redirects {
                s.push(' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
    }
}

/// Render a command strictly inline (compound commands still use `;` separators,
/// which is valid bash — just not multi-line).
fn command_inline(cmd: &Command) -> String {
    match cmd {
        Command::Simple(sc) => simple_src(sc),
        Command::If(c) => {
            let mut s = String::from("if ");
            s.push_str(&program_inline(&c.cond));
            s.push_str("; then ");
            s.push_str(&program_inline(&c.body));
            s.push(';');
            for (econd, ebody) in &c.elifs {
                s.push_str(" elif ");
                s.push_str(&program_inline(econd));
                s.push_str("; then ");
                s.push_str(&program_inline(ebody));
                s.push(';');
            }
            if let Some(eb) = &c.else_body {
                s.push_str(" else ");
                s.push_str(&program_inline(eb));
                s.push(';');
            }
            s.push_str(" fi");
            s
        }
        Command::Loop(c) => {
            let mut s = String::from(if c.until { "until " } else { "while " });
            s.push_str(&program_inline(&c.cond));
            s.push_str("; do ");
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::For(c) => {
            let mut s = format!("for {}", c.var);
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str("; do ");
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::ForArith(c) => {
            let mut s = format!("for (( {}; {}; {} )); do ", c.init, c.cond, c.update);
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::Select(c) => {
            let mut s = format!("select {}", c.var);
            if let Some(words) = &c.words {
                s.push_str(" in");
                for w in words {
                    s.push(' ');
                    s.push_str(&word_src(w));
                }
            }
            s.push_str("; do ");
            s.push_str(&program_inline(&c.body));
            s.push_str("; done");
            s
        }
        Command::Function(f) => {
            let mut s = format!("{} () {{ ", f.name);
            s.push_str(&program_inline(&f.body));
            s.push_str("; }");
            for r in &f.redirects {
                s.push(' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
        Command::Case(c) => {
            let mut s = format!("case {} in ", word_src(&c.word));
            for item in &c.items {
                let pats: Vec<String> = item.patterns.iter().map(word_src).collect();
                s.push_str(&pats.join("|"));
                s.push_str(") ");
                s.push_str(&program_inline(&item.body));
                s.push(' ');
                s.push_str(match item.term {
                    crate::ast::CaseTerm::Break => ";;",
                    crate::ast::CaseTerm::FallThrough => ";&",
                    crate::ast::CaseTerm::ContinueMatch => ";;&",
                });
                s.push(' ');
            }
            s.push_str("esac");
            s
        }
        Command::BraceGroup(prog) => format!("{{ {}; }}", program_inline(prog)),
        Command::Subshell(prog) => format!("( {} )", program_inline(prog)),
        Command::Cond(expr) => format!("[[ {} ]]", cond_src(expr)),
        Command::Arith(text) => format!("(( {text} ))"),
        Command::Coproc { name, body } => {
            let mut s = String::from("coproc ");
            if let Some(n) = name {
                s.push_str(n);
                s.push(' ');
            }
            s.push_str(&command_inline(body));
            s
        }
        Command::Redirected { inner, redirects } => {
            let mut s = command_inline(inner);
            for r in redirects {
                s.push(' ');
                s.push_str(&redirect_src(r));
            }
            s
        }
    }
}

/// Reconstruct the source text of a simple command (assignments, words,
/// redirections) on one line — used for `$BASH_COMMAND` in DEBUG/ERR traps.
pub fn simple_src(sc: &SimpleCommand) -> String {
    let mut parts: Vec<String> = Vec::new();
    for a in &sc.assignments {
        parts.push(assignment_src(a));
    }
    for w in &sc.words {
        parts.push(word_src(w));
    }
    for a in &sc.decl_arrays {
        parts.push(assignment_src(a));
    }
    let mut s = parts.join(" ");
    for r in &sc.redirects {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&redirect_src(r));
    }
    s
}

pub(crate) fn assignment_src(a: &Assignment) -> String {
    let mut s = a.name.clone();
    if let Some(idx) = &a.index {
        s.push('[');
        s.push_str(&word_src(idx));
        s.push(']');
    }
    s.push_str(if a.append { "+=" } else { "=" });
    match &a.value {
        AssignRhs::Scalar(w) => s.push_str(&word_src(w)),
        AssignRhs::Array(elems) => {
            s.push('(');
            let items: Vec<String> = elems
                .iter()
                .map(|e| match e {
                    ArrayElem::Positional(w) => word_src(w),
                    ArrayElem::Keyed { index, value } => {
                        format!("[{}]={}", word_src(index), word_src(value))
                    }
                })
                .collect();
            s.push_str(&items.join(" "));
            s.push(')');
        }
    }
    s
}

fn redirect_src(r: &Redirect) -> String {
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
            RedirectOp::DupOut => (">&", ""),
            RedirectOp::DupIn => ("<&", ""),
            // `{name}` never pairs with here-docs / `&>`; fall back to the plain
            // form for those (unreachable in practice).
            _ => return redirect_src_plain(r),
        };
        return format!("{{{name}}}{op}{sep}{}", word_src(&r.target));
    }
    redirect_src_plain(r)
}

fn redirect_src_plain(r: &Redirect) -> String {
    // bash's `declare -f` deparser separates a redirection operator from a
    // *file/word* target with a single space (`> log`, `2>> err`, `&> both`,
    // `< in`), but writes fd-*duplication* operators tight against their fd
    // (`1>&2`, `0<&3`). Here-strings already carry their own space.
    match r.op {
        RedirectOp::Write => fd_prefixed(r.fd, 1, ">", " ", &word_src(&r.target)),
        RedirectOp::Clobber => fd_prefixed(r.fd, 1, ">|", " ", &word_src(&r.target)),
        RedirectOp::Append => fd_prefixed(r.fd, 1, ">>", " ", &word_src(&r.target)),
        RedirectOp::WriteBoth => format!("&> {}", word_src(&r.target)),
        RedirectOp::AppendBoth => format!("&>> {}", word_src(&r.target)),
        RedirectOp::Read => fd_prefixed(r.fd, 0, "<", " ", &word_src(&r.target)),
        // bash always shows the explicit source fd on an output dup, including
        // the default (`>&2` → `1>&2`), so pass a default that never elides it.
        RedirectOp::DupOut => fd_prefixed(r.fd, -1, ">&", "", &word_src(&r.target)),
        // Likewise an input dup renders with its explicit source fd
        // (`0<&3`, never `<&3`).
        RedirectOp::DupIn => fd_prefixed(r.fd, -1, "<&", "", &word_src(&r.target)),
        // Here-docs are re-emitted as here-strings (same bytes to stdin); a
        // here-string is likewise `<<<`. See the module docs / TD-OILS16.
        RedirectOp::HereDoc | RedirectOp::HereStr => {
            let body = word_src(&r.target);
            format!("<<< {body}")
        }
    }
}

/// `fd` prefix only when it differs from the operator's default (`>`→1, `<`→0);
/// `sep` is inserted between the operator and target (a space for file targets,
/// empty for fd-duplication operators).
fn fd_prefixed(fd: i32, default: i32, op: &str, sep: &str, target: &str) -> String {
    if fd == default {
        format!("{op}{sep}{target}")
    } else {
        format!("{fd}{op}{sep}{target}")
    }
}

fn cond_src(expr: &CondExpr) -> String {
    match expr {
        CondExpr::Word(w) => word_src(w),
        CondExpr::Unary(op, w) => format!("{} {}", unary_op_str(*op), word_src(w)),
        CondExpr::Binary(l, op, r) => {
            format!("{} {} {}", word_src(l), bin_op_str(*op), word_src(r))
        }
        CondExpr::Regex(l, r) => format!("{} =~ {}", word_src(l), word_src(r)),
        CondExpr::Not(e) => format!("! {}", cond_src(e)),
        CondExpr::And(a, b) => format!("{} && {}", cond_src(a), cond_src(b)),
        CondExpr::Or(a, b) => format!("{} || {}", cond_src(a), cond_src(b)),
    }
}

fn unary_op_str(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Exists => "-e",
        UnaryOp::File => "-f",
        UnaryOp::Dir => "-d",
        UnaryOp::Readable => "-r",
        UnaryOp::Writable => "-w",
        UnaryOp::Executable => "-x",
        UnaryOp::NonEmptyFile => "-s",
        UnaryOp::ZeroLen => "-z",
        UnaryOp::NonZeroLen => "-n",
        UnaryOp::VarSet => "-v",
        UnaryOp::OptionSet => "-o",
        UnaryOp::Symlink => "-L",
        UnaryOp::Terminal => "-t",
    }
}

fn bin_op_str(op: CondBinOp) -> &'static str {
    match op {
        CondBinOp::StrEq => "==",
        CondBinOp::StrNe => "!=",
        CondBinOp::StrLt => "<",
        CondBinOp::StrGt => ">",
        CondBinOp::NumEq => "-eq",
        CondBinOp::NumNe => "-ne",
        CondBinOp::NumLt => "-lt",
        CondBinOp::NumLe => "-le",
        CondBinOp::NumGt => "-gt",
        CondBinOp::NumGe => "-ge",
        CondBinOp::FileNewer => "-nt",
        CondBinOp::FileOlder => "-ot",
        CondBinOp::SameFile => "-ef",
    }
}

/// Reconstruct source text for a whole word (all parts concatenated).
#[must_use]
pub fn word_src(w: &Word) -> String {
    let mut s = String::new();
    for p in &w.parts {
        s.push_str(&part_src(p));
    }
    s
}

/// `$name` when `name` is a plain identifier or a single special parameter,
/// otherwise the braced `${name}` form (always valid).
fn dollar_name(name: &str) -> String {
    let simple = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let special = name.len() == 1
        && matches!(
            name.chars().next(),
            Some('?' | '@' | '*' | '#' | '$' | '!' | '-' | '0'..='9')
        );
    if simple || special {
        format!("${name}")
    } else {
        format!("${{{name}}}")
    }
}

/// `name` optionally followed by `[index]`.
#[must_use]
pub fn name_sub(name: &str, index: &Option<Box<Word>>) -> String {
    match index {
        Some(i) => format!("{name}[{}]", word_src(i)),
        None => name.to_string(),
    }
}

fn part_src(p: &WordPart) -> String {
    match p {
        WordPart::Literal(s) => s.clone(),
        WordPart::SingleQuoted(s) => format!("'{s}'"),
        WordPart::DoubleQuoted(parts) => {
            let mut s = String::from("\"");
            for p in parts {
                s.push_str(&part_src(p));
            }
            s.push('"');
            s
        }
        WordPart::Param(name) => dollar_name(name),
        WordPart::ParamOp { name, index, op, colon, arg } => {
            let sym = match op {
                ParamOp::UseDefault => "-",
                ParamOp::AssignDefault => "=",
                ParamOp::UseAlternate => "+",
                ParamOp::ErrorIfUnset => "?",
            };
            let colon = if *colon { ":" } else { "" };
            format!("${{{}{}{}{}}}", name_sub(name, index), colon, sym, word_src(arg))
        }
        WordPart::ParamTrim { name, index, suffix, longest, pattern } => {
            let op = match (suffix, longest) {
                (true, true) => "%%",
                (true, false) => "%",
                (false, true) => "##",
                (false, false) => "#",
            };
            format!("${{{}{}{}}}", name_sub(name, index), op, word_src(pattern))
        }
        WordPart::ParamSubstr { name, index, offset, length } => {
            let mut s = format!("${{{}:{}", name_sub(name, index), word_src(offset));
            if let Some(len) = length {
                s.push(':');
                s.push_str(&word_src(len));
            }
            s.push('}');
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
            format!(
                "${{{}{}{}/{}}}",
                name_sub(name, index),
                op,
                word_src(pattern),
                word_src(replacement)
            )
        }
        WordPart::ParamCase { name, index, mode, all, pattern } => {
            let op = case_op_src(*mode, *all);
            format!("${{{}{}{}}}", name_sub(name, index), op, word_src(pattern))
        }
        WordPart::Indirect(name) => format!("${{!{name}}}"),
        WordPart::IndirectOp { target, .. } => {
            // The `target` carries the referent name as a placeholder, so
            // rendering it yields `${ref<op>}`; splice the indirection `!` in
            // after the opening `${` to recover `${!ref<op>}`.
            let inner = part_src(target);
            match inner.strip_prefix("${") {
                Some(rest) => format!("${{!{rest}"),
                None => inner,
            }
        }
        WordPart::VarNames { prefix, star } => {
            format!("${{!{prefix}{}}}", if *star { "*" } else { "@" })
        }
        WordPart::CommandSub(prog) => format!("$({})", program_inline(prog)),
        WordPart::ProcSub { input, body } => {
            format!("{}({})", if *input { '<' } else { '>' }, program_inline(body))
        }
        WordPart::ArithSub(text) => format!("$(( {text} ))"),
        WordPart::BadSubst(raw) => format!("${{{raw}}}"),
        WordPart::Length(name) => format!("${{#{name}}}"),
        WordPart::ArrayRef { name, index, length } => {
            let idx = match index {
                ArrayIndex::Index(w) => word_src(w),
                ArrayIndex::All => "@".to_string(),
                ArrayIndex::Star => "*".to_string(),
            };
            if *length {
                format!("${{#{name}[{idx}]}}")
            } else {
                format!("${{{name}[{idx}]}}")
            }
        }
        WordPart::ArrayKeys { name, star } => {
            format!("${{!{name}[{}]}}", if *star { "*" } else { "@" })
        }
        WordPart::ParamTransform { name, index, op } => {
            format!("${{{}@{op}}}", name_sub(name, index))
        }
        WordPart::ArraySlice { name, star, offset, length } => {
            let sub = if name == "@" || name == "*" {
                name.clone()
            } else {
                format!("{name}[{}]", if *star { "*" } else { "@" })
            };
            let mut s = format!("${{{sub}:{}", word_src(offset));
            if let Some(len) = length {
                s.push(':');
                s.push_str(&word_src(len));
            }
            s.push('}');
            s
        }
        WordPart::ArrayBulk { name, star, op } => {
            let sub = if name == "@" || name == "*" {
                name.clone()
            } else {
                format!("{name}[{}]", if *star { "*" } else { "@" })
            };
            let opstr = match op {
                BulkOp::Trim { suffix, longest, pattern } => {
                    let o = match (suffix, longest) {
                        (true, true) => "%%",
                        (true, false) => "%",
                        (false, true) => "##",
                        (false, false) => "#",
                    };
                    format!("{o}{}", word_src(pattern))
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
                    format!("{o}{}/{}", word_src(pattern), word_src(replacement))
                }
                BulkOp::Case { mode, all, pattern } => {
                    format!("{}{}", case_op_src(*mode, *all), word_src(pattern))
                }
                BulkOp::Transform { op } => format!("@{op}"),
            };
            format!("${{{sub}{opstr}}}")
        }
        WordPart::ArrayOp { name, star, op, colon, arg } => {
            let sub = format!("{name}[{}]", if *star { "*" } else { "@" });
            let o = match op {
                ParamOp::UseDefault => "-",
                ParamOp::AssignDefault => "=",
                ParamOp::UseAlternate => "+",
                ParamOp::ErrorIfUnset => "?",
            };
            let colon = if *colon { ":" } else { "" };
            format!("${{{sub}{colon}{o}{}}}", word_src(arg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    /// Parse `src`, expect exactly one function definition, and unparse it.
    fn dump_fn(src: &str, name: &str) -> String {
        let prog = parse(src).expect("parse");
        for item in &prog.items {
            for cmd in &item.list.first.commands {
                if let Command::Function(f) = cmd
                    && f.name == name
                {
                    return unparse_function(&f.name, &f.body, &f.redirects);
                }
            }
        }
        panic!("function {name} not found");
    }

    /// Unparse a function, re-parse the dump, and unparse again — the two dumps
    /// must be identical (a round-trip stability check).
    fn assert_roundtrip(src: &str, name: &str) {
        let first = dump_fn(src, name);
        // The dump is `name () \n{ … }`; re-parse it as a program.
        let reprog = parse(&first).expect("re-parse dump");
        let f = reprog
            .items
            .iter()
            .flat_map(|i| &i.list.first.commands)
            .find_map(|c| match c {
                Command::Function(f) if f.name == name => Some(f),
                _ => None,
            })
            .expect("function in dump");
        let second = unparse_function(&f.name, &f.body, &f.redirects);
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
}
