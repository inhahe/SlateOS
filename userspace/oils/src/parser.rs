//! Recursive-descent parser: tokens → [`ast::Program`].
//!
//! The parser also lowers lexer [`Seg`]s into [`ast::WordPart`]s, recursively
//! parsing command/parameter substitutions (their raw inner source is captured
//! by the lexer).

use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, CaseClause, CaseItem, Command,
    CondBinOp,
    CondExpr, ForClause, FunctionDef, IfClause, Item, LoopClause, ParamOp, Pipeline, Program,
    Redirect, RedirectOp, ReplaceAnchor, SimpleCommand, UnaryOp, Word, WordPart,
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
            let cmd = match w.as_str() {
                "if" => self.parse_if()?,
                "while" => self.parse_loop(false)?,
                "until" => self.parse_loop(true)?,
                "for" => self.parse_for()?,
                "case" => self.parse_case()?,
                "{" => self.parse_brace_group()?,
                other => {
                    return Err(ParseError(format!("unexpected reserved word '{other}'")));
                }
            };
            return self.with_redirects(cmd);
        }
        if self.at_op(Op::LParen) {
            let cmd = self.parse_subshell()?;
            return self.with_redirects(cmd);
        }
        // `(( expr ))` arithmetic command (lexed as a single token).
        if let Some(Tok::ArithCmd(raw)) = self.peek() {
            let raw = raw.clone();
            self.pos += 1;
            return self.with_redirects(Command::Arith(raw));
        }
        // `[[ expr ]]` conditional expression.
        if self.bare_word_here().as_deref() == Some("[[") {
            let cmd = self.parse_cond()?;
            return self.with_redirects(cmd);
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

    /// Is the current token the start of a redirection (`<`, `>`, `>>`, `2>`, …)?
    fn at_redirect_start(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Io(_))
                | Some(Tok::Op(
                    Op::Less
                        | Op::Great
                        | Op::DGreat
                        | Op::GreatAnd
                        | Op::LessAnd
                        | Op::DLess
                        | Op::DLessDash
                        | Op::TLess,
                ))
        )
    }

    /// Attach any trailing redirections to a compound command, wrapping it in a
    /// [`Command::Redirected`] when at least one is present.
    fn with_redirects(&mut self, inner: Command) -> Result<Command, ParseError> {
        let mut redirects = Vec::new();
        while self.at_redirect_start() {
            redirects.push(self.parse_redirect()?);
        }
        if redirects.is_empty() {
            Ok(inner)
        } else {
            Ok(Command::Redirected {
                inner: Box::new(inner),
                redirects,
            })
        }
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

    fn parse_case(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("case")?;
        let Some(Tok::Word(segs)) = self.peek() else {
            return Err(ParseError("expected word after 'case'".into()));
        };
        let word = self.word_from_segs(&segs.clone())?;
        self.pos += 1;
        self.skip_newlines();
        self.expect_reserved("in")?;
        self.skip_newlines();
        let mut items = Vec::new();
        while self.reserved_here().as_deref() != Some("esac") {
            if self.peek().is_none() {
                return Err(ParseError("unterminated 'case' (expected 'esac')".into()));
            }
            // Optional leading '(' before the pattern list.
            if self.at_op(Op::LParen) {
                self.pos += 1;
            }
            // Pattern list: word ['|' word]*.
            let mut patterns = Vec::new();
            loop {
                let Some(Tok::Word(segs)) = self.peek() else {
                    return Err(ParseError("expected pattern in 'case'".into()));
                };
                patterns.push(self.word_from_segs(&segs.clone())?);
                self.pos += 1;
                if self.at_op(Op::Pipe) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
            if !self.at_op(Op::RParen) {
                return Err(ParseError("expected ')' after 'case' pattern".into()));
            }
            self.pos += 1;
            let body = self.parse_case_body()?;
            items.push(CaseItem { patterns, body });
            if self.at_op(Op::DSemi) {
                self.pos += 1;
                self.skip_newlines();
            } else {
                // Only `esac` may legitimately follow a `;;`-less arm body.
                self.skip_newlines();
            }
        }
        self.expect_reserved("esac")?;
        Ok(Command::Case(CaseClause { word, items }))
    }

    /// Parse a `case`-arm body: a command list terminated by `;;` or `esac`.
    fn parse_case_body(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none()
                || self.at_op(Op::DSemi)
                || self.reserved_here().as_deref() == Some("esac")
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

    /// Parse a `[[ … ]]` conditional expression. The opening `[[` word is at
    /// the current position; parsing stops at the matching `]]` word.
    fn parse_cond(&mut self) -> Result<Command, ParseError> {
        // Consume `[[`.
        self.pos += 1;
        let expr = self.parse_cond_or()?;
        if self.bare_word_here().as_deref() != Some("]]") {
            return Err(ParseError("expected ']]' to close '[['".into()));
        }
        self.pos += 1;
        Ok(Command::Cond(expr))
    }

    fn parse_cond_or(&mut self) -> Result<CondExpr, ParseError> {
        let mut left = self.parse_cond_and()?;
        while self.at_op(Op::OrIf) {
            self.pos += 1;
            let right = self.parse_cond_and()?;
            left = CondExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_and(&mut self) -> Result<CondExpr, ParseError> {
        let mut left = self.parse_cond_not()?;
        while self.at_op(Op::AndIf) {
            self.pos += 1;
            let right = self.parse_cond_not()?;
            left = CondExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_not(&mut self) -> Result<CondExpr, ParseError> {
        if self.bare_word_here().as_deref() == Some("!") {
            self.pos += 1;
            let inner = self.parse_cond_not()?;
            return Ok(CondExpr::Not(Box::new(inner)));
        }
        self.parse_cond_primary()
    }

    fn parse_cond_primary(&mut self) -> Result<CondExpr, ParseError> {
        // Parenthesised sub-expression.
        if self.at_op(Op::LParen) {
            self.pos += 1;
            let inner = self.parse_cond_or()?;
            if !self.at_op(Op::RParen) {
                return Err(ParseError("expected ')' in '[[ … ]]'".into()));
            }
            self.pos += 1;
            return Ok(inner);
        }
        // Unary operator: `-f WORD`, `-z WORD`, …
        if let Some(text) = self.bare_word_here()
            && let Some(op) = unary_op_from(&text)
        {
            self.pos += 1;
            let operand = self.expect_cond_word()?;
            return Ok(CondExpr::Unary(op, operand));
        }
        // Otherwise: WORD [ binop WORD ].
        let left = self.expect_cond_word()?;
        if let Some(op) = self.peek_cond_binop() {
            if matches!(op, RawBinOp::Regex) {
                return Err(ParseError(
                    "'=~' (regex match) is not yet supported in '[[ … ]]'".into(),
                ));
            }
            self.advance_cond_binop();
            let right = self.expect_cond_word()?;
            return Ok(CondExpr::Binary(
                Box::new(left),
                op.into_bin_op(),
                Box::new(right),
            ));
        }
        Ok(CondExpr::Word(left))
    }

    /// Expect a word operand inside `[[ … ]]` (not an operator/closer).
    fn expect_cond_word(&mut self) -> Result<Word, ParseError> {
        if let Some(Tok::Word(segs)) = self.peek() {
            let segs = segs.clone();
            // `]]` is the closer, never an operand.
            if let [Seg::Lit(s)] = segs.as_slice()
                && s == "]]"
            {
                return Err(ParseError("unexpected ']]' (expected operand)".into()));
            }
            self.pos += 1;
            return self.word_from_segs(&segs);
        }
        Err(ParseError("expected operand in '[[ … ]]'".into()))
    }

    /// Peek at a binary operator following an operand, without consuming.
    fn peek_cond_binop(&self) -> Option<RawBinOp> {
        match self.peek() {
            Some(Tok::Op(Op::Less)) => Some(RawBinOp::StrLt),
            Some(Tok::Op(Op::Great)) => Some(RawBinOp::StrGt),
            Some(Tok::Word(segs)) => {
                if let [Seg::Lit(s)] = segs.as_slice() {
                    raw_binop_from(s)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn advance_cond_binop(&mut self) {
        self.pos += 1;
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
                Some(Tok::ArrayAssign { .. }) => {
                    if seen_word {
                        return Err(ParseError(
                            "array assignment is only valid before the command word".into(),
                        ));
                    }
                    let Some(Tok::ArrayAssign {
                        name,
                        append,
                        elems,
                    }) = self.bump()
                    else {
                        unreachable!("peek matched ArrayAssign");
                    };
                    let mut items = Vec::with_capacity(elems.len());
                    for segs in &elems {
                        items.push(parse_array_elem(segs)?);
                    }
                    cmd.assignments.push(Assignment {
                        name,
                        index: None,
                        append,
                        value: AssignRhs::Array(items),
                    });
                }
                Some(Tok::Io(_))
                | Some(Tok::Op(
                    Op::Less
                    | Op::Great
                    | Op::DGreat
                    | Op::GreatAnd
                    | Op::LessAnd
                    | Op::DLess
                    | Op::DLessDash
                    | Op::TLess,
                )) => {
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
            Some(Tok::Op(Op::DLess | Op::DLessDash)) => RedirectOp::HereDoc,
            Some(Tok::Op(Op::TLess)) => RedirectOp::HereStr,
            _ => return Err(ParseError("expected redirection operator".into())),
        };
        let fd = explicit_fd.unwrap_or(match op {
            RedirectOp::Read | RedirectOp::HereDoc | RedirectOp::HereStr => 0,
            _ => 1,
        });
        let target = match self.bump() {
            Some(Tok::Word(segs)) => self.word_from_segs(&segs)?,
            // The lexer emits the here-doc body as its own token right after the
            // `<<`/`<<-` operator.
            Some(Tok::HereDoc(segs)) => self.word_from_segs(&segs)?,
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

    /// Recognise `NAME=value`, `NAME+=value`, or `NAME[index]=value` (before the
    /// first command word).
    fn try_assignment(&self, segs: &[Seg]) -> Result<Option<Assignment>, ParseError> {
        let Some(Seg::Lit(first)) = segs.first() else {
            return Ok(None);
        };
        // A subscript containing expansions spans multiple segments, e.g.
        // `m[$k]=v` → [Lit("m["), Param("k"), Lit("]=v")]. The first segment
        // then has `[` but no closing `]`, so `=` isn't in it — handle here.
        if let Some(open) = first.find('[')
            && !first[open..].contains(']')
        {
            return self.spanning_subscript_assignment(segs, first, open);
        }
        let Some(eq) = first.find('=') else {
            return Ok(None);
        };
        let mut lhs = &first[..eq];
        // `+=` append.
        let append = lhs.ends_with('+');
        if append {
            lhs = &lhs[..lhs.len() - 1];
        }
        // Optional `[index]` subscript.
        let (name, index) = if let Some(open) = lhs.find('[') {
            if !lhs.ends_with(']') {
                return Ok(None);
            }
            let name = &lhs[..open];
            let idx_src = &lhs[open + 1..lhs.len() - 1];
            if name.is_empty() || !is_valid_name(name) || idx_src.is_empty() {
                return Ok(None);
            }
            (name, Some(word_from_source(idx_src)?))
        } else {
            if lhs.is_empty() || !is_valid_name(lhs) {
                return Ok(None);
            }
            (lhs, None)
        };
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
            index,
            append,
            value: AssignRhs::Scalar(self.word_from_segs(&value_segs)?),
        }))
    }

    /// Lower lexer segments into an [`ast::Word`].
    fn word_from_segs(&self, segs: &[Seg]) -> Result<Word, ParseError> {
        word_from_segs(segs)
    }

    /// Parse `name[SUBSCRIPT]=value` / `name[SUBSCRIPT]+=value` where the
    /// subscript spans multiple segments (contains `$…` expansions). `open` is
    /// the byte offset of `[` in the first (literal) segment.
    fn spanning_subscript_assignment(
        &self,
        segs: &[Seg],
        first: &str,
        open: usize,
    ) -> Result<Option<Assignment>, ParseError> {
        let name = &first[..open];
        if name.is_empty() || !is_valid_name(name) {
            return Ok(None);
        }
        // Subscript segments: the first seg's text after `[`, then whole
        // segments, up to the segment that carries the closing `]`.
        let mut sub_segs: Vec<Seg> = Vec::new();
        let after_open = &first[open + 1..];
        if !after_open.is_empty() {
            sub_segs.push(Seg::Lit(after_open.to_string()));
        }
        let mut value_segs: Vec<Seg> = Vec::new();
        let mut append = false;
        let mut found = false;
        for seg in &segs[1..] {
            if found {
                value_segs.push(seg.clone());
                continue;
            }
            if let Seg::Lit(s) = seg
                && let Some(close) = s.find(']')
            {
                let before = &s[..close];
                if !before.is_empty() {
                    sub_segs.push(Seg::Lit(before.to_string()));
                }
                let rest = &s[close + 1..];
                let val_lit = if let Some(v) = rest.strip_prefix("+=") {
                    append = true;
                    v
                } else if let Some(v) = rest.strip_prefix('=') {
                    v
                } else {
                    // `]` not immediately followed by `=` — not an assignment.
                    return Ok(None);
                };
                if !val_lit.is_empty() {
                    value_segs.push(Seg::Lit(val_lit.to_string()));
                }
                found = true;
                continue;
            }
            sub_segs.push(seg.clone());
        }
        if !found || sub_segs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Assignment {
            name: name.to_string(),
            index: Some(self.word_from_segs(&sub_segs)?),
            append,
            value: AssignRhs::Scalar(self.word_from_segs(&value_segs)?),
        }))
    }
}

/// Parse one array-literal element: either `[sub]=value` (keyed) or a bare
/// positional value. A keyed element is recognised when the first segment is a
/// literal that starts with `[` and contains `]=` (so the subscript is literal
/// text — an expanded key like `[$k]=v` inside a literal falls back to
/// positional; use element assignment `m[$k]=v` for that).
fn parse_array_elem(segs: &[Seg]) -> Result<ArrayElem, ParseError> {
    if let Some(Seg::Lit(first)) = segs.first()
        && first.starts_with('[')
        && let Some(close_eq) = first.find("]=")
    {
        let index = word_from_source(&first[1..close_eq])?;
        let mut value_segs: Vec<Seg> = Vec::new();
        let after = &first[close_eq + 2..];
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_string()));
        }
        value_segs.extend_from_slice(&segs[1..]);
        return Ok(ArrayElem::Keyed {
            index,
            value: word_from_segs(&value_segs)?,
        });
    }
    Ok(ArrayElem::Positional(word_from_segs(segs)?))
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
/// Split a `${…}` body into `(name, optional-subscript, remaining-chars)`.
///
/// The name is a run of name chars, a run of digits, or a single special
/// character. If a `[…]` subscript immediately follows the name, it is parsed
/// into an [`ArrayIndex`] and the characters after the closing `]` are returned
/// as the remainder (for operator forms). The closing bracket is taken as the
/// last `]` in the body so arithmetic subscripts like `arr[i+1]` still parse.
fn split_name_subscript(
    bytes: &[char],
) -> Result<(String, Option<ArrayIndex>, Vec<char>), ParseError> {
    if bytes.is_empty() {
        return Err(ParseError("empty '${}' expansion".into()));
    }
    let mut i = 0;
    if bytes[0].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else if is_name_start(bytes[0]) {
        while i < bytes.len() && is_name_char(bytes[i]) {
            i += 1;
        }
    } else {
        // A special single-char parameter (`@`, `*`, `?`, `#`, `!`, `$`, …).
        i = 1;
    }
    let name: String = bytes[..i].iter().collect();
    if bytes.get(i) == Some(&'[')
        && let Some(rel) = bytes[i..].iter().rposition(|&c| c == ']')
    {
        let close = i + rel;
        let inner: String = bytes[i + 1..close].iter().collect();
        let index = match inner.as_str() {
            "@" => ArrayIndex::All,
            "*" => ArrayIndex::Star,
            "" => return Err(ParseError("empty array subscript '[]'".into())),
            _ => ArrayIndex::Index(Box::new(word_from_source(&inner)?)),
        };
        return Ok((name, Some(index), bytes[close + 1..].to_vec()));
    }
    Ok((name, None, bytes[i..].to_vec()))
}

fn parse_braced_param(raw: &str) -> Result<WordPart, ParseError> {
    if let Some(after_hash) = raw.strip_prefix('#') {
        if after_hash.is_empty() {
            // `${#}` is the positional-parameter count — treat as `$#`.
            return Ok(WordPart::Param("#".into()));
        }
        let bytes: Vec<char> = after_hash.chars().collect();
        let (name, subscript, remaining) = split_name_subscript(&bytes)?;
        if let Some(index) = subscript {
            if !remaining.is_empty() {
                return Err(ParseError(format!(
                    "unsupported parameter expansion '${{{raw}}}'"
                )));
            }
            // `${#name[@]}` / `${#name[i]}` — array element count / element length.
            return Ok(WordPart::ArrayRef {
                name,
                index,
                length: true,
            });
        }
        return Ok(WordPart::Length(after_hash.to_string()));
    }
    if let Some(after_bang) = raw.strip_prefix('!') {
        // `${!name[@]}` / `${!name[*]}` — the keys/indices of an array.
        let bytes: Vec<char> = after_bang.chars().collect();
        let (name, subscript, remaining) = split_name_subscript(&bytes)?;
        if let Some(index) = subscript
            && remaining.is_empty()
            && matches!(index, ArrayIndex::All | ArrayIndex::Star)
        {
            return Ok(WordPart::ArrayKeys {
                name,
                star: matches!(index, ArrayIndex::Star),
            });
        }
        // `${!name}` indirection and `${!prefix*}` matching are not supported.
        return Err(ParseError(format!(
            "unsupported parameter expansion '${{{raw}}}'"
        )));
    }
    let bytes: Vec<char> = raw.chars().collect();
    let (name, subscript, rest) = split_name_subscript(&bytes)?;
    if let Some(index) = subscript {
        if !rest.is_empty() {
            // Combining a subscript with a `:-`/`#`/`/` operator is not yet supported.
            return Err(ParseError(format!(
                "unsupported parameter expansion '${{{raw}}}'"
            )));
        }
        return Ok(WordPart::ArrayRef {
            name,
            index,
            length: false,
        });
    }
    if rest.is_empty() {
        return Ok(WordPart::Param(name));
    }
    match rest[0] {
        // Prefix / suffix trimming: `#`, `##`, `%`, `%%`.
        '#' | '%' => {
            let suffix = rest[0] == '%';
            let longest = rest.get(1) == Some(&rest[0]);
            let pat_start = if longest { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(WordPart::ParamTrim {
                name,
                suffix,
                longest,
                pattern: Box::new(word_from_source(&pat)?),
            })
        }
        // Pattern substitution: `/pat/repl`, `//pat/repl`, `/#…`, `/%…`.
        '/' => parse_param_replace(name, &rest[1..]),
        // Substring `:offset[:length]` — but `:` followed by one of -=+? is the
        // use/assign/alt/error operator, handled below.
        ':' if !matches!(rest.get(1), Some('-' | '=' | '+' | '?')) => {
            let body: String = rest[1..].iter().collect();
            let (off_str, len_str) = match body.find(':') {
                Some(idx) => (body[..idx].to_string(), Some(body[idx + 1..].to_string())),
                None => (body, None),
            };
            let length = match len_str {
                Some(s) => Some(Box::new(word_from_source(&s)?)),
                None => None,
            };
            Ok(WordPart::ParamSubstr {
                name,
                offset: Box::new(word_from_source(&off_str)?),
                length,
            })
        }
        // `:-`, `:=`, `:+`, `:?` and the colon-less `-=+?` forms.
        _ => {
            let mut chs = rest.iter();
            let mut c = *chs.next().unwrap_or(&'\0');
            if c == ':' {
                c = *chs.next().unwrap_or(&'\0');
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
            Ok(WordPart::ParamOp {
                name,
                op,
                arg: Box::new(word_from_source(&arg_str)?),
            })
        }
    }
}

/// Parse the body of a `${name/…}` substitution (chars after the first `/`).
fn parse_param_replace(name: String, body: &[char]) -> Result<WordPart, ParseError> {
    let mut i = 0;
    let mut all = false;
    let mut anchor = ReplaceAnchor::None;
    match body.first() {
        Some('/') => {
            all = true;
            i = 1;
        }
        Some('#') => {
            anchor = ReplaceAnchor::Start;
            i = 1;
        }
        Some('%') => {
            anchor = ReplaceAnchor::End;
            i = 1;
        }
        _ => {}
    }
    // Pattern runs to the next unescaped '/'; the remainder is the replacement.
    let mut pattern = String::new();
    let mut replacement = String::new();
    let mut in_repl = false;
    while i < body.len() {
        let c = body[i];
        if !in_repl && c == '\\' && body.get(i + 1) == Some(&'/') {
            pattern.push('/');
            i += 2;
            continue;
        }
        if !in_repl && c == '/' {
            in_repl = true;
            i += 1;
            continue;
        }
        if in_repl {
            replacement.push(c);
        } else {
            pattern.push(c);
        }
        i += 1;
    }
    Ok(WordPart::ParamReplace {
        name,
        all,
        anchor,
        pattern: Box::new(word_from_source(&pattern)?),
        replacement: Box::new(word_from_source(&replacement)?),
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

/// Map a `[[ … ]]` unary operator string to its [`UnaryOp`].
fn unary_op_from(s: &str) -> Option<UnaryOp> {
    Some(match s {
        "-e" => UnaryOp::Exists,
        "-f" => UnaryOp::File,
        "-d" => UnaryOp::Dir,
        "-r" => UnaryOp::Readable,
        "-w" => UnaryOp::Writable,
        "-x" => UnaryOp::Executable,
        "-s" => UnaryOp::NonEmptyFile,
        "-z" => UnaryOp::ZeroLen,
        "-n" => UnaryOp::NonZeroLen,
        _ => return None,
    })
}

/// Raw binary operator recognised inside `[[ … ]]` (before lowering; `Regex`
/// is recognised so it can be rejected with a clear message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawBinOp {
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    Regex,
    NumEq,
    NumNe,
    NumLt,
    NumLe,
    NumGt,
    NumGe,
}

impl RawBinOp {
    fn into_bin_op(self) -> CondBinOp {
        match self {
            RawBinOp::StrEq => CondBinOp::StrEq,
            RawBinOp::StrNe => CondBinOp::StrNe,
            RawBinOp::StrLt => CondBinOp::StrLt,
            RawBinOp::StrGt => CondBinOp::StrGt,
            // `Regex` is rejected before lowering; map defensively to StrEq.
            RawBinOp::Regex => CondBinOp::StrEq,
            RawBinOp::NumEq => CondBinOp::NumEq,
            RawBinOp::NumNe => CondBinOp::NumNe,
            RawBinOp::NumLt => CondBinOp::NumLt,
            RawBinOp::NumLe => CondBinOp::NumLe,
            RawBinOp::NumGt => CondBinOp::NumGt,
            RawBinOp::NumGe => CondBinOp::NumGe,
        }
    }
}

/// Map a `[[ … ]]` binary operator word to its [`RawBinOp`].
fn raw_binop_from(s: &str) -> Option<RawBinOp> {
    Some(match s {
        "==" | "=" => RawBinOp::StrEq,
        "!=" => RawBinOp::StrNe,
        "=~" => RawBinOp::Regex,
        "-eq" => RawBinOp::NumEq,
        "-ne" => RawBinOp::NumNe,
        "-lt" => RawBinOp::NumLt,
        "-le" => RawBinOp::NumLe,
        "-gt" => RawBinOp::NumGt,
        "-ge" => RawBinOp::NumGe,
        _ => return None,
    })
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
    fn case_statement() {
        let prog =
            parse("case $x in a|b) echo ab;; *.txt) echo text;; *) echo default;; esac").unwrap();
        let Command::Case(c) = &prog.items[0].list.first.commands[0] else {
            panic!("expected case");
        };
        assert_eq!(c.items.len(), 3);
        assert_eq!(c.items[0].patterns.len(), 2);
        assert_eq!(c.items[2].patterns.len(), 1);
    }

    #[test]
    fn case_empty_and_final_no_dsemi() {
        // Last arm may omit `;;`; an empty body is allowed.
        let prog = parse("case y in x) ;; y) echo hit\nesac").unwrap();
        let Command::Case(c) = &prog.items[0].list.first.commands[0] else {
            panic!("expected case");
        };
        assert_eq!(c.items.len(), 2);
        assert!(c.items[0].body.items.is_empty());
    }

    #[test]
    fn here_doc_and_here_string() {
        let prog = parse("cat <<EOF\nhi\nEOF\n").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected simple");
        };
        assert_eq!(sc.redirects.len(), 1);
        assert!(matches!(sc.redirects[0].op, RedirectOp::HereDoc));
        assert_eq!(sc.redirects[0].fd, 0);

        let prog2 = parse("cat <<< hello").unwrap();
        let Command::Simple(sc2) = &prog2.items[0].list.first.commands[0] else {
            panic!("expected simple");
        };
        assert!(matches!(sc2.redirects[0].op, RedirectOp::HereStr));
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
    fn array_ref_parsing() {
        let prog = parse("echo ${a[0]} ${a[@]} ${a[*]} ${#a[@]}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(
            sc.words[1].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::Index(_),
                length: false,
                ..
            }
        ));
        assert!(matches!(
            sc.words[2].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::All,
                length: false,
                ..
            }
        ));
        assert!(matches!(
            sc.words[3].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::Star,
                ..
            }
        ));
        assert!(matches!(
            sc.words[4].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::All,
                length: true,
                ..
            }
        ));
    }

    #[test]
    fn array_assignment_parsing() {
        let prog = parse("a=(one two three)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        let AssignRhs::Array(words) = &sc.assignments[0].value else {
            panic!("expected array assignment");
        };
        assert_eq!(words.len(), 3);
        assert!(!sc.assignments[0].append);
    }

    #[test]
    fn array_append_and_index_assignment() {
        let prog = parse("a+=(x); a[2]=y").unwrap();
        let Command::Simple(app) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(app.assignments[0].append);
        let idx = parse("a[2]=y").unwrap();
        let Command::Simple(sc) = &idx.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments[0].index.is_some());
        assert!(matches!(sc.assignments[0].value, AssignRhs::Scalar(_)));
    }

    #[test]
    fn assoc_keyed_literal_and_keys_parsing() {
        // `${!m[@]}` → ArrayKeys.
        let prog = parse("echo ${!m[@]} ${!m[*]}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(
            sc.words[1].parts[0],
            WordPart::ArrayKeys { star: false, .. }
        ));
        assert!(matches!(
            sc.words[2].parts[0],
            WordPart::ArrayKeys { star: true, .. }
        ));
        // Keyed array-literal element `[k]=v`.
        let prog = parse("m=([a]=1 plain)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        let AssignRhs::Array(items) = &sc.assignments[0].value else {
            panic!("expected array literal");
        };
        assert!(matches!(items[0], ArrayElem::Keyed { .. }));
        assert!(matches!(items[1], ArrayElem::Positional(_)));
    }

    #[test]
    fn spanning_subscript_assignment_parsing() {
        // `m[$k]=v` — subscript spans segments; still recognised as assignment.
        let prog = parse("m[$k]=v").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments[0].index.is_some());
        assert!(!sc.assignments[0].append);
        assert!(sc.words.is_empty());
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

    #[test]
    fn cond_expression() {
        let prog = parse("[[ $x == foo ]]").unwrap();
        let Command::Cond(CondExpr::Binary(_, op, _)) = &prog.items[0].list.first.commands[0]
        else {
            panic!("expected cond binary");
        };
        assert_eq!(*op, CondBinOp::StrEq);
    }

    #[test]
    fn cond_logical_precedence() {
        // `||` binds looser than `&&`: a || b && c parses as a || (b && c).
        let prog = parse("[[ 1 -eq 1 || 2 -eq 2 && 3 -eq 3 ]]").unwrap();
        let Command::Cond(CondExpr::Or(_, right)) = &prog.items[0].list.first.commands[0] else {
            panic!("expected top-level Or");
        };
        assert!(matches!(**right, CondExpr::And(_, _)));
    }

    #[test]
    fn cond_regex_rejected() {
        assert!(parse("[[ $x =~ foo ]]").is_err());
    }

    #[test]
    fn arith_command() {
        let prog = parse("(( x + 1 ))").unwrap();
        let Command::Arith(raw) = &prog.items[0].list.first.commands[0] else {
            panic!("expected arith command");
        };
        assert_eq!(raw.trim(), "x + 1");
    }

    #[test]
    fn double_paren_vs_nested_subshell() {
        // `((` = arithmetic; `( (` = nested subshell.
        assert!(matches!(
            parse("(( 1 ))").unwrap().items[0].list.first.commands[0],
            Command::Arith(_)
        ));
        assert!(matches!(
            parse("( ( echo ) )").unwrap().items[0].list.first.commands[0],
            Command::Subshell(_)
        ));
    }
}
