//! Recursive-descent parser: tokens → [`ast::Program`].
//!
//! The parser also lowers lexer [`Seg`]s into [`ast::WordPart`]s, recursively
//! parsing command/parameter substitutions (their raw inner source is captured
//! by the lexer).

use crate::ast::{
    AndOr, AndOrOp, Assignment, Command, ForClause, FunctionDef, IfClause, Item, LoopClause,
    ParamOp, Pipeline, Program, Redirect, RedirectOp, SimpleCommand, Word, WordPart,
};
use crate::lexer::{Op, Seg, Tok, tokenize};

/// A parse error with a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parse shell source into a [`Program`].
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    let toks = tokenize(src).map_err(|e| ParseError(e.0))?;
    let mut p = Parser { toks, pos: 0 };
    let prog = p.parse_program(&[])?;
    if p.pos != p.toks.len() {
        // Leftover tokens — typically an unmatched `)` or reserved word.
        return Err(ParseError(format!(
            "unexpected token near position {}",
            p.pos
        )));
    }
    Ok(prog)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

/// Reserved words that terminate a command list or introduce a compound.
const RESERVED: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "in", "{", "}",
    "!", "case", "esac",
];

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn at_op(&self, op: Op) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    /// If the current token is an unquoted single-literal word, return it.
    fn reserved_here(&self) -> Option<String> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
            && RESERVED.contains(&s.as_str())
        {
            return Some(s.clone());
        }
        None
    }

    /// The literal text of a bare word token (single unquoted literal), if any.
    fn bare_word_here(&self) -> Option<String> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
        {
            return Some(s.clone());
        }
        None
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.pos += 1;
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.peek(),
            Some(Tok::Newline) | Some(Tok::Op(Op::Semi))
        ) {
            self.pos += 1;
        }
    }

    /// Parse a command list until EOF or one of `stops` (reserved words) or a
    /// closing `)`.
    fn parse_program(&mut self, stops: &[&str]) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none() || self.at_op(Op::RParen) {
                break;
            }
            if let Some(w) = self.reserved_here()
                && stops.contains(&w.as_str())
            {
                break;
            }
            let list = self.parse_and_or()?;
            let mut background = false;
            match self.peek() {
                Some(Tok::Op(Op::Amp)) => {
                    background = true;
                    self.pos += 1;
                }
                Some(Tok::Op(Op::Semi)) | Some(Tok::Newline) => {
                    self.pos += 1;
                }
                _ => {}
            }
            items.push(Item { list, background });
        }
        Ok(Program { items })
    }

    fn parse_and_or(&mut self) -> Result<AndOr, ParseError> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::AndIf)) => AndOrOp::And,
                Some(Tok::Op(Op::OrIf)) => AndOrOp::Or,
                _ => break,
            };
            self.pos += 1;
            self.skip_newlines();
            let pipe = self.parse_pipeline()?;
            rest.push((op, pipe));
        }
        Ok(AndOr { first, rest })
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let mut negated = false;
        if self.reserved_here().as_deref() == Some("!") {
            negated = true;
            self.pos += 1;
        }
        let mut commands = vec![self.parse_command()?];
        while self.at_op(Op::Pipe) {
            self.pos += 1;
            self.skip_newlines();
            commands.push(self.parse_command()?);
        }
        Ok(Pipeline { negated, commands })
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        if let Some(w) = self.reserved_here() {
            match w.as_str() {
                "if" => return self.parse_if(),
                "while" => return self.parse_loop(false),
                "until" => return self.parse_loop(true),
                "for" => return self.parse_for(),
                "{" => return self.parse_brace_group(),
                other => {
                    return Err(ParseError(format!("unexpected reserved word '{other}'")));
                }
            }
        }
        if self.at_op(Op::LParen) {
            return self.parse_subshell();
        }
        // Function definition: `name ( )`.
        if let Some(name) = self.bare_word_here()
            && is_valid_name(&name)
            && matches!(self.toks.get(self.pos + 1), Some(Tok::Op(Op::LParen)))
            && matches!(self.toks.get(self.pos + 2), Some(Tok::Op(Op::RParen)))
        {
            self.pos += 3;
            self.skip_newlines();
            let body = self.parse_compound_body()?;
            return Ok(Command::Function(FunctionDef { name, body }));
        }
        self.parse_simple()
    }

    /// Parse a `{ … }` or `( … )` body used as a function body.
    fn parse_compound_body(&mut self) -> Result<Program, ParseError> {
        if let Some(w) = self.reserved_here()
            && w == "{"
            && let Command::BraceGroup(p) = self.parse_brace_group()?
        {
            return Ok(p);
        }
        if self.at_op(Op::LParen)
            && let Command::Subshell(p) = self.parse_subshell()?
        {
            return Ok(p);
        }
        Err(ParseError("expected function body".into()))
    }

    fn parse_brace_group(&mut self) -> Result<Command, ParseError> {
        // Consume `{`.
        self.pos += 1;
        let body = self.parse_program(&["}"])?;
        self.expect_reserved("}")?;
        Ok(Command::BraceGroup(body))
    }

    fn parse_subshell(&mut self) -> Result<Command, ParseError> {
        // Consume `(`.
        self.pos += 1;
        let body = self.parse_program(&[])?;
        if !self.at_op(Op::RParen) {
            return Err(ParseError("expected ')'".into()));
        }
        self.pos += 1;
        Ok(Command::Subshell(body))
    }

    fn parse_if(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("if")?;
        let cond = self.parse_program(&["then"])?;
        self.expect_reserved("then")?;
        let body = self.parse_program(&["elif", "else", "fi"])?;
        let mut elifs = Vec::new();
        while self.reserved_here().as_deref() == Some("elif") {
            self.pos += 1;
            let c = self.parse_program(&["then"])?;
            self.expect_reserved("then")?;
            let b = self.parse_program(&["elif", "else", "fi"])?;
            elifs.push((c, b));
        }
        let else_body = if self.reserved_here().as_deref() == Some("else") {
            self.pos += 1;
            Some(self.parse_program(&["fi"])?)
        } else {
            None
        };
        self.expect_reserved("fi")?;
        Ok(Command::If(IfClause {
            cond,
            body,
            elifs,
            else_body,
        }))
    }

    fn parse_loop(&mut self, until: bool) -> Result<Command, ParseError> {
        self.expect_reserved(if until { "until" } else { "while" })?;
        let cond = self.parse_program(&["do"])?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"])?;
        self.expect_reserved("done")?;
        Ok(Command::Loop(LoopClause { until, cond, body }))
    }

    fn parse_for(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("for")?;
        let var = self
            .bare_word_here()
            .ok_or_else(|| ParseError("expected variable name after 'for'".into()))?;
        if !is_valid_name(&var) {
            return Err(ParseError(format!("invalid for-loop variable '{var}'")));
        }
        self.pos += 1;
        self.skip_newlines();
        let words = if self.reserved_here().as_deref() == Some("in") {
            self.pos += 1;
            let mut ws = Vec::new();
            while let Some(Tok::Word(segs)) = self.peek() {
                // Stop at reserved words like `do`.
                if let [Seg::Lit(s)] = segs.as_slice()
                    && RESERVED.contains(&s.as_str())
                {
                    break;
                }
                let segs = segs.clone();
                self.pos += 1;
                ws.push(self.word_from_segs(&segs)?);
            }
            self.skip_separators();
            Some(ws)
        } else {
            self.skip_separators();
            None
        };
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"])?;
        self.expect_reserved("done")?;
        Ok(Command::For(ForClause { var, words, body }))
    }

    fn parse_simple(&mut self) -> Result<Command, ParseError> {
        let mut cmd = SimpleCommand::default();
        let mut seen_word = false;
        loop {
            match self.peek() {
                Some(Tok::Word(segs)) => {
                    // A reserved word ends the simple command (unless it's an
                    // argument position where reserved words are plain words —
                    // but at the start of a command a reserved word was already
                    // dispatched, so here we only stop for list terminators).
                    if !seen_word
                        && let [Seg::Lit(s)] = segs.as_slice()
                        && RESERVED.contains(&s.as_str())
                    {
                        break;
                    }
                    let segs = segs.clone();
                    // Assignment only valid before the first word.
                    if !seen_word
                        && let Some(a) = self.try_assignment(&segs)?
                    {
                        self.pos += 1;
                        cmd.assignments.push(a);
                        continue;
                    }
                    self.pos += 1;
                    cmd.words.push(self.word_from_segs(&segs)?);
                    seen_word = true;
                }
                Some(Tok::Io(_)) | Some(Tok::Op(Op::Less | Op::Great | Op::DGreat | Op::GreatAnd | Op::LessAnd)) => {
                    let r = self.parse_redirect()?;
                    cmd.redirects.push(r);
                }
                _ => break,
            }
        }
        if cmd.words.is_empty() && cmd.assignments.is_empty() && cmd.redirects.is_empty() {
            return Err(ParseError("empty command".into()));
        }
        Ok(Command::Simple(cmd))
    }

    fn parse_redirect(&mut self) -> Result<Redirect, ParseError> {
        let explicit_fd = if let Some(Tok::Io(n)) = self.peek() {
            let n = *n;
            self.pos += 1;
            Some(n)
        } else {
            None
        };
        let op = match self.bump() {
            Some(Tok::Op(Op::Less)) => RedirectOp::Read,
            Some(Tok::Op(Op::Great)) => RedirectOp::Write,
            Some(Tok::Op(Op::DGreat)) => RedirectOp::Append,
            Some(Tok::Op(Op::GreatAnd | Op::LessAnd)) => RedirectOp::DupOut,
            _ => return Err(ParseError("expected redirection operator".into())),
        };
        let fd = explicit_fd.unwrap_or(match op {
            RedirectOp::Read => 0,
            _ => 1,
        });
        let target = match self.bump() {
            Some(Tok::Word(segs)) => self.word_from_segs(&segs)?,
            _ => return Err(ParseError("expected redirection target".into())),
        };
        Ok(Redirect { fd, op, target })
    }

    fn expect_reserved(&mut self, w: &str) -> Result<(), ParseError> {
        if self.reserved_here().as_deref() == Some(w) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ParseError(format!("expected '{w}'")))
        }
    }

    /// Recognise `NAME=value` (before the first command word).
    fn try_assignment(&self, segs: &[Seg]) -> Result<Option<Assignment>, ParseError> {
        let Some(Seg::Lit(first)) = segs.first() else {
            return Ok(None);
        };
        let Some(eq) = first.find('=') else {
            return Ok(None);
        };
        let name = &first[..eq];
        if name.is_empty() || !is_valid_name(name) {
            return Ok(None);
        }
        // Build the value word from the remainder of the first seg plus the
        // rest of the segments.
        let mut value_segs: Vec<Seg> = Vec::new();
        let after = &first[eq + 1..];
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_string()));
        }
        value_segs.extend_from_slice(&segs[1..]);
        Ok(Some(Assignment {
            name: name.to_string(),
            value: self.word_from_segs(&value_segs)?,
        }))
    }

    /// Lower lexer segments into an [`ast::Word`].
    fn word_from_segs(&self, segs: &[Seg]) -> Result<Word, ParseError> {
        word_from_segs(segs)
    }
}

/// Lower lexer segments into an [`ast::Word`] (stateless).
fn word_from_segs(segs: &[Seg]) -> Result<Word, ParseError> {
    let mut parts = Vec::with_capacity(segs.len());
    for s in segs {
        parts.push(seg_to_part(s)?);
    }
    Ok(Word { parts })
}

fn seg_to_part(seg: &Seg) -> Result<WordPart, ParseError> {
    Ok(match seg {
        Seg::Lit(s) => WordPart::Literal(s.clone()),
        Seg::Sq(s) => WordPart::SingleQuoted(s.clone()),
        Seg::Dq(inner) => {
            let mut parts = Vec::with_capacity(inner.len());
            for s in inner {
                parts.push(seg_to_part(s)?);
            }
            WordPart::DoubleQuoted(parts)
        }
        Seg::Param(n) => WordPart::Param(n.clone()),
        Seg::ParamBraced(raw) => parse_braced_param(raw)?,
        Seg::CmdSub(raw) => WordPart::CommandSub(parse(raw)?),
        Seg::Arith(raw) => WordPart::ArithSub(raw.clone()),
    })
}

/// Parse the inner text of a `${ … }` expansion.
fn parse_braced_param(raw: &str) -> Result<WordPart, ParseError> {
    if let Some(rest) = raw.strip_prefix('#') {
        if rest.is_empty() {
            // `${#}` is the positional-parameter count — treat as `$#`.
            return Ok(WordPart::Param("#".into()));
        }
        return Ok(WordPart::Length(rest.to_string()));
    }
    // Read the parameter name (a name, a single special char, or digits).
    let bytes: Vec<char> = raw.chars().collect();
    let mut i = 0;
    if bytes.is_empty() {
        return Err(ParseError("empty '${}' expansion".into()));
    }
    if bytes[0].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else if is_name_start(bytes[0]) {
        while i < bytes.len() && is_name_char(bytes[i]) {
            i += 1;
        }
    } else {
        // A special single-char parameter.
        i = 1;
    }
    let name: String = bytes[..i].iter().collect();
    let remainder: String = bytes[i..].iter().collect();
    if remainder.is_empty() {
        return Ok(WordPart::Param(name));
    }
    // Operator: optional ':' then one of -=+?.
    let mut chs = remainder.chars();
    let mut c = chs.next().unwrap_or('\0');
    if c == ':' {
        c = chs.next().unwrap_or('\0');
    }
    let arg_str: String = chs.collect();
    let op = match c {
        '-' => ParamOp::UseDefault,
        '=' => ParamOp::AssignDefault,
        '+' => ParamOp::UseAlternate,
        '?' => ParamOp::ErrorIfUnset,
        _ => {
            return Err(ParseError(format!(
                "unsupported parameter expansion '${{{raw}}}'"
            )));
        }
    };
    let arg = word_from_source(&arg_str)?;
    Ok(WordPart::ParamOp {
        name,
        op,
        arg: Box::new(arg),
    })
}

/// Build a single [`Word`] from arbitrary source text (used for the argument of
/// a parameter expansion). Words separated by blanks are joined with a literal
/// space — a best-effort reconstruction adequate for `${x:-a b}`.
fn word_from_source(s: &str) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let toks = tokenize(s).map_err(|e| ParseError(e.0))?;
    let mut parts: Vec<WordPart> = Vec::new();
    let mut first = true;
    for t in &toks {
        if let Tok::Word(segs) = t {
            if !first {
                parts.push(WordPart::Literal(" ".into()));
            }
            first = false;
            for seg in segs {
                parts.push(seg_to_part(seg)?);
            }
        }
    }
    Ok(Word { parts })
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_valid_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if is_name_start(c) => {}
        _ => return false,
    }
    it.all(is_name_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_command() {
        let prog = parse("echo hello world").unwrap();
        assert_eq!(prog.items.len(), 1);
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected simple command");
        };
        assert_eq!(sc.words.len(), 3);
    }

    #[test]
    fn pipeline_and_andor() {
        let prog = parse("a | b && c || d").unwrap();
        let ao = &prog.items[0].list;
        assert_eq!(ao.first.commands.len(), 2);
        assert_eq!(ao.rest.len(), 2);
    }

    #[test]
    fn assignment_and_redirect() {
        let prog = parse("FOO=bar cmd arg > out 2> err").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.redirects.len(), 2);
        assert_eq!(sc.redirects[1].fd, 2);
    }

    #[test]
    fn if_clause() {
        let prog = parse("if true; then echo yes; else echo no; fi").unwrap();
        let Command::If(_) = &prog.items[0].list.first.commands[0] else {
            panic!("expected if");
        };
    }

    #[test]
    fn for_loop() {
        let prog = parse("for x in a b c; do echo $x; done").unwrap();
        let Command::For(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected for");
        };
        assert_eq!(f.var, "x");
        assert_eq!(f.words.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn while_loop() {
        let prog = parse("while false; do echo x; done").unwrap();
        let Command::Loop(l) = &prog.items[0].list.first.commands[0] else {
            panic!("expected loop");
        };
        assert!(!l.until);
    }

    #[test]
    fn function_def() {
        let prog = parse("greet() { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "greet");
    }

    #[test]
    fn param_expansions() {
        let prog = parse("echo ${x:-default} ${#y}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(sc.words[1].parts[0], WordPart::ParamOp { .. }));
        assert!(matches!(sc.words[2].parts[0], WordPart::Length(_)));
    }

    #[test]
    fn command_substitution() {
        let prog = parse("echo $(echo nested)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(sc.words[1].parts[0], WordPart::CommandSub(_)));
    }

    #[test]
    fn negated_pipeline() {
        let prog = parse("! false").unwrap();
        assert!(prog.items[0].list.first.negated);
    }
}
