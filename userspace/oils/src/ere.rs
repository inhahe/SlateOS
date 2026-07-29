//! A small POSIX Extended Regular Expression (ERE) engine for the shell's
//! `[[ str =~ re ]]` operator.
//!
//! ## Why in-tree (not a crate)
//! `osh` targets `x86_64-slateos` where a heavyweight, `std`-only regex crate is
//! awkward, and `bash`'s `=~` semantics are POSIX ERE (matched by the C
//! library's `regexec`) — a focused, dependency-free engine is the right size.
//!
//! ## Why a Pike VM (not backtracking)
//! Naive recursive backtracking is prone to catastrophic backtracking
//! (ReDoS) — a real denial-of-service risk on attacker-shaped patterns/inputs.
//! This engine compiles the pattern to a small instruction program and runs it
//! as a **Thompson/Pike NFA simulation** with capture slots: every input
//! character is scanned once and the set of live NFA states is bounded by the
//! program length, so matching is `O(len(input) × len(program))` with **no**
//! exponential blow-up. Thread priority (higher-priority = added first, deduped
//! per step) yields leftmost, greedy submatches — the common expectation for
//! `=~`.
//!
//! ## Supported syntax
//! `. ^ $`, literals, `\`-escapes (`\.`, `\(`, `\\`, `\n`, `\t`, `\r`, …),
//! grouping `( … )` (capturing → `BASH_REMATCH`), alternation `a|b`, the
//! quantifiers `* + ?` and bounded `{m}` / `{m,}` / `{m,n}` (greedy), and
//! bracket expressions `[...]` / `[^...]` with ranges (`a-z`), literal-`]`/`-`
//! placement, and POSIX classes (`[[:digit:]]`, `[[:alpha:]]`, …). Non-ERE
//! Perl shorthands (`\d`, `\w`, `\s`, non-greedy `*?`, backreferences) are
//! intentionally not provided — `bash`'s `=~` is POSIX ERE, not PCRE.

/// A compile-time error in an ERE pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EreError(pub String);

impl core::fmt::Display for EreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Upper bound on `{m,n}` expansion, to keep the compiled program small and
/// bound compile-time/memory (POSIX `RE_DUP_MAX` is 255; we allow a bit more).
const MAX_REPEAT: usize = 1000;

// ---- AST --------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Node {
    Empty,
    Lit(char),
    Any,
    Class(ClassData),
    Start,
    End,
    /// Capturing group with its 1-based group index.
    Group(usize, Box<Node>),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: usize,
        /// `None` = unbounded (`*`, `+`, `{m,}`).
        max: Option<usize>,
    },
}

#[derive(Debug, Clone)]
struct ClassData {
    negated: bool,
    /// Inclusive character ranges (a single char is `(c, c)`).
    ranges: Vec<(char, char)>,
    posix: Vec<PosixClass>,
}

impl ClassData {
    /// Whether `c` is in the class's *positive* set (before applying negation).
    fn hit_positive(&self, c: char) -> bool {
        self.ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi)
            || self.posix.iter().any(|p| p.matches(c))
    }

    /// Case-aware match. When `ci` is set, the positive set is tested against
    /// the char and its case-folded variants *before* negation is applied, so a
    /// negated class like `[^a-z]` correctly excludes `A` under `nocasematch`.
    fn matches_ci(&self, c: char, ci: bool) -> bool {
        let mut hit = self.hit_positive(c);
        if ci && !hit {
            for alt in c.to_lowercase().chain(c.to_uppercase()) {
                if alt != c && self.hit_positive(alt) {
                    hit = true;
                    break;
                }
            }
        }
        hit ^ self.negated
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PosixClass {
    Alpha,
    Digit,
    Alnum,
    Space,
    Blank,
    Upper,
    Lower,
    Punct,
    Xdigit,
    Cntrl,
    Print,
    Graph,
}

impl PosixClass {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "alpha" => Self::Alpha,
            "digit" => Self::Digit,
            "alnum" => Self::Alnum,
            "space" => Self::Space,
            "blank" => Self::Blank,
            "upper" => Self::Upper,
            "lower" => Self::Lower,
            "punct" => Self::Punct,
            "xdigit" => Self::Xdigit,
            "cntrl" => Self::Cntrl,
            "print" => Self::Print,
            "graph" => Self::Graph,
            _ => return None,
        })
    }

    fn matches(self, c: char) -> bool {
        match self {
            Self::Alpha => c.is_alphabetic(),
            Self::Digit => c.is_ascii_digit(),
            Self::Alnum => c.is_alphanumeric(),
            Self::Space => c.is_whitespace(),
            Self::Blank => c == ' ' || c == '\t',
            Self::Upper => c.is_uppercase(),
            Self::Lower => c.is_lowercase(),
            Self::Punct => c.is_ascii_punctuation(),
            Self::Xdigit => c.is_ascii_hexdigit(),
            Self::Cntrl => c.is_control(),
            Self::Print => !c.is_control(),
            Self::Graph => !c.is_control() && !c.is_whitespace(),
        }
    }
}

// ---- Parser -----------------------------------------------------------------

struct EParser {
    chars: Vec<char>,
    pos: usize,
    ngroups: usize,
}

impl EParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<char> {
        self.chars.get(self.pos + off).copied()
    }

    fn parse(&mut self) -> Result<Node, EreError> {
        // bash rejects an empty `=~` right-hand side outright — `[[ x =~ "" ]]`
        // is status 2, not a match on the empty string. An empty *sub*expression
        // is still fine: `[[ x =~ () ]]` matches.
        if self.chars.is_empty() {
            return Err(EreError("empty regex".into()));
        }
        let node = self.parse_alt()?;
        if self.pos != self.chars.len() {
            // A stray `)` (or other unconsumed input) is a syntax error.
            return Err(EreError(format!(
                "unexpected '{}' in regex",
                self.peek().unwrap_or(' ')
            )));
        }
        Ok(node)
    }

    fn parse_alt(&mut self) -> Result<Node, EreError> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap_or(Node::Empty))
        } else {
            // `a|`, `|a`, `(a||b)`: glibc requires every branch of an
            // alternation to be a real expression, even though a wholly empty
            // pattern between parens (`()`) is fine.
            if branches.iter().any(|b| matches!(b, Node::Empty)) {
                return Err(EreError("empty alternation branch in regex".into()));
            }
            Ok(Node::Alt(branches))
        }
    }

    /// Parse a run of repeated atoms up to `|`, `)` or the end of the pattern.
    ///
    /// A `{0}` / `{0,0}` interval *deletes* the atom it follows rather than
    /// repeating it zero times, and glibc then complains if that leaves the run
    /// with nothing in it — which is why `a{0}` and `(a{0})` are errors while
    /// `a{0}b`, `^a{0}` and the explicitly-empty `()` are not. The distinction
    /// is how the run was written, not what it ends up matching, so it has to be
    /// made here: a run that started with nothing is fine, a run that was
    /// emptied is not.
    fn parse_concat(&mut self) -> Result<Node, EreError> {
        let mut parts = Vec::new();
        let mut deleted = false;
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            match self.parse_repeat()? {
                Some(n) => parts.push(n),
                None => deleted = true,
            }
        }
        match parts.len() {
            0 if deleted => Err(EreError("nothing to repeat in regex".into())),
            0 => Ok(Node::Empty),
            1 => Ok(parts.pop().unwrap_or(Node::Empty)),
            _ => Ok(Node::Concat(parts)),
        }
    }

    /// Parse one atom and its quantifier, if any. `Ok(None)` means the atom was
    /// deleted by a zero-count interval — see [`Self::parse_concat`].
    fn parse_repeat(&mut self) -> Result<Option<Node>, EreError> {
        // A quantifier has to have something to quantify. glibc rejects `*a`,
        // `?a`, `{2}a` and — because an alternation branch or a group starts a
        // fresh expression — `a|*b` and `(*a)` too.
        if is_quantifier_start(self.peek()) {
            return Err(EreError("nothing to repeat in regex".into()));
        }
        let atom = self.parse_atom()?;
        let Some((min, max)) = self.parse_quantifier()? else {
            return Ok(Some(atom));
        };
        // `^` is an assertion, not an atom, so glibc reports `^*` and `a^*b`
        // the same way it reports a leading `*`. `$` it does accept.
        if matches!(atom, Node::Start) {
            return Err(EreError("nothing to repeat in regex".into()));
        }
        // One quantifier per atom: `a**`, `a*?`, `a{1}*` and `a*{1}` are all
        // errors, not a repeat of a repeat.
        if is_quantifier_start(self.peek()) {
            return Err(EreError("repeated quantifier in regex".into()));
        }
        if max == Some(0) {
            return Ok(None);
        }
        Ok(Some(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
        }))
    }

    /// Consume a `*` / `+` / `?` / `{m,n}` quantifier if one is at the cursor.
    fn parse_quantifier(&mut self) -> Result<Option<(usize, Option<usize>)>, EreError> {
        match self.peek() {
            Some('*') => {
                self.pos += 1;
                Ok(Some((0, None)))
            }
            Some('+') => {
                self.pos += 1;
                Ok(Some((1, None)))
            }
            Some('?') => {
                self.pos += 1;
                Ok(Some((0, Some(1))))
            }
            Some('{') => self.parse_brace().map(Some),
            _ => Ok(None),
        }
    }

    /// Parse a `{m}` / `{m,}` / `{m,n}` interval at the cursor.
    ///
    /// An unescaped `{` that does not open a well-formed interval is an error,
    /// not a literal brace: glibc rejects `a{b`, `a{1`, `a{}`, `a{,3}` and
    /// `a{1,2,3}` outright, and only `\{` or `[{]` gets you a literal one.
    fn parse_brace(&mut self) -> Result<(usize, Option<usize>), EreError> {
        let bad = || EreError("invalid interval in regex".into());
        self.pos += 1; // consume '{'
        let Some(min) = self.parse_int() else {
            return Err(bad());
        };
        let max = if self.peek() == Some(',') {
            self.pos += 1;
            if self.peek() == Some('}') {
                None // `{m,}`
            } else {
                Some(self.parse_int().ok_or_else(bad)?)
            }
        } else {
            Some(min) // `{m}`
        };
        if self.peek() != Some('}') {
            return Err(bad());
        }
        self.pos += 1; // consume '}'
        if min > MAX_REPEAT || max.is_some_and(|n| n > MAX_REPEAT) {
            return Err(EreError("repetition count too large".into()));
        }
        if let Some(n) = max
            && min > n
        {
            return Err(EreError(format!("invalid interval {{{min},{n}}}")));
        }
        Ok((min, max))
    }

    fn parse_int(&mut self) -> Option<usize> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<usize>().ok()
    }

    fn parse_atom(&mut self) -> Result<Node, EreError> {
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                self.ngroups += 1;
                let idx = self.ngroups;
                let inner = self.parse_alt()?;
                if self.peek() != Some(')') {
                    return Err(EreError("expected ')' in regex".into()));
                }
                self.pos += 1;
                Ok(Node::Group(idx, Box::new(inner)))
            }
            Some('[') => self.parse_class(),
            Some('.') => {
                self.pos += 1;
                Ok(Node::Any)
            }
            Some('^') => {
                self.pos += 1;
                Ok(Node::Start)
            }
            Some('$') => {
                self.pos += 1;
                Ok(Node::End)
            }
            Some('\\') => {
                self.pos += 1;
                let e = self
                    .peek()
                    .ok_or_else(|| EreError("trailing backslash in regex".into()))?;
                self.pos += 1;
                Ok(Node::Lit(unescape(e)))
            }
            // Only `parse_quantifier` may consume a `{`, and `parse_repeat`
            // rejects one that reaches an atom slot, so this is unreachable —
            // but a literal brace here would silently resurrect the lenient
            // reading glibc does not have.
            Some('{') => Err(EreError("invalid interval in regex".into())),
            Some(c) => {
                self.pos += 1;
                Ok(Node::Lit(c))
            }
            None => Ok(Node::Empty),
        }
    }

    fn parse_class(&mut self) -> Result<Node, EreError> {
        self.pos += 1; // consume '['
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.pos += 1;
        }
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut posix: Vec<PosixClass> = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(EreError("unterminated '[' in regex".into()));
            };
            // A `]` closes the class, except as the very first member where it
            // is a literal (POSIX rule).
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            first = false;

            // POSIX named class `[:name:]`.
            if c == '[' && self.peek_at(1) == Some(':') {
                let saved = self.pos;
                self.pos += 2; // consume '[:'
                let name_start = self.pos;
                while matches!(self.peek(), Some(ch) if ch.is_ascii_alphabetic()) {
                    self.pos += 1;
                }
                let name: String = self.chars[name_start..self.pos].iter().collect();
                if self.peek() == Some(':') && self.peek_at(1) == Some(']') {
                    self.pos += 2; // consume ':]'
                    match PosixClass::from_name(&name) {
                        Some(pc) => {
                            posix.push(pc);
                            continue;
                        }
                        None => {
                            return Err(EreError(format!("unknown character class [:{name}:]")));
                        }
                    }
                }
                // Not actually a named class — rewind and treat '[' literally.
                self.pos = saved;
            }

            let lo = self.class_char()?;
            // A range `a-z`, but a trailing `-` (before `]`) is a literal.
            if self.peek() == Some('-') && self.peek_at(1) != Some(']') && self.peek_at(1).is_some() {
                self.pos += 1; // consume '-'
                let hi = self.class_char()?;
                if lo > hi {
                    return Err(EreError(format!("invalid range {lo}-{hi} in class")));
                }
                ranges.push((lo, hi));
            } else {
                ranges.push((lo, lo));
            }
        }
        Ok(Node::Class(ClassData {
            negated,
            ranges,
            posix,
        }))
    }

    /// Read one character inside a bracket expression, honoring `\`-escapes.
    fn class_char(&mut self) -> Result<char, EreError> {
        let Some(c) = self.peek() else {
            return Err(EreError("unterminated '[' in regex".into()));
        };
        if c == '\\' {
            self.pos += 1;
            let e = self
                .peek()
                .ok_or_else(|| EreError("trailing backslash in class".into()))?;
            self.pos += 1;
            return Ok(unescape(e));
        }
        self.pos += 1;
        Ok(c)
    }
}

/// Whether a character begins a quantifier. `{` counts even when the braces
/// turn out to be malformed — that is an error either way, never a literal.
fn is_quantifier_start(c: Option<char>) -> bool {
    matches!(c, Some('*' | '+' | '?' | '{'))
}

/// Map an escaped character to the literal it denotes (`\n` → newline, etc.).
fn unescape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'f' => '\u{0C}',
        'v' => '\u{0B}',
        '0' => '\0',
        other => other,
    }
}

// ---- Compiler ---------------------------------------------------------------

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Any,
    Class(ClassData),
    Match,
    Jmp(usize),
    Split(usize, usize),
    Save(usize),
    AssertStart,
    AssertEnd,
}

struct Compiler {
    prog: Vec<Inst>,
}

impl Compiler {
    fn emit(&mut self, i: Inst) -> usize {
        self.prog.push(i);
        self.prog.len() - 1
    }

    fn compile(&mut self, node: &Node) {
        match node {
            Node::Empty => {}
            Node::Lit(c) => {
                self.emit(Inst::Char(*c));
            }
            Node::Any => {
                self.emit(Inst::Any);
            }
            Node::Class(d) => {
                self.emit(Inst::Class(d.clone()));
            }
            Node::Start => {
                self.emit(Inst::AssertStart);
            }
            Node::End => {
                self.emit(Inst::AssertEnd);
            }
            Node::Group(idx, inner) => {
                self.emit(Inst::Save(2 * idx));
                self.compile(inner);
                self.emit(Inst::Save(2 * idx + 1));
            }
            Node::Concat(parts) => {
                for p in parts {
                    self.compile(p);
                }
            }
            Node::Alt(branches) => {
                let mut jmp_ends: Vec<usize> = Vec::new();
                let last = branches.len().saturating_sub(1);
                for (i, b) in branches.iter().enumerate() {
                    if i < last {
                        let split = self.emit(Inst::Split(0, 0));
                        let l1 = self.prog.len();
                        self.compile(b);
                        jmp_ends.push(self.emit(Inst::Jmp(0)));
                        let l2 = self.prog.len();
                        self.prog[split] = Inst::Split(l1, l2);
                    } else {
                        self.compile(b);
                    }
                }
                let end = self.prog.len();
                for j in jmp_ends {
                    self.prog[j] = Inst::Jmp(end);
                }
            }
            Node::Repeat { node, min, max } => self.compile_repeat(node, *min, *max),
        }
    }

    fn compile_repeat(&mut self, node: &Node, min: usize, max: Option<usize>) {
        // Mandatory copies.
        for _ in 0..min {
            self.compile(node);
        }
        match max {
            None => {
                // Greedy star: `L: Split(body, out); <body>; Jmp L; out:`.
                let l = self.emit(Inst::Split(0, 0));
                let body = self.prog.len();
                self.compile(node);
                self.emit(Inst::Jmp(l));
                let out = self.prog.len();
                self.prog[l] = Inst::Split(body, out);
            }
            Some(max) => {
                // `max - min` greedy optional copies, each able to jump to `out`.
                let extra = max.saturating_sub(min);
                let mut splits: Vec<usize> = Vec::with_capacity(extra);
                for _ in 0..extra {
                    let s = self.emit(Inst::Split(0, 0));
                    splits.push(s);
                    let body = self.prog.len();
                    self.compile(node);
                    self.prog[s] = Inst::Split(body, 0); // second target patched below
                }
                let out = self.prog.len();
                for s in splits {
                    if let Inst::Split(a, _) = self.prog[s] {
                        self.prog[s] = Inst::Split(a, out);
                    }
                }
            }
        }
    }
}

// ---- Compiled regex + Pike VM ----------------------------------------------

/// A compiled ERE. Compile once with [`Regex::new`], then match repeatedly.
/// Compare an optional input char against a literal `Char` instruction,
/// folding case when `ci` is set. Case folding tries both the upper- and
/// lower-case mappings so any Unicode letter pair (not just ASCII) is handled.
fn char_eq(input: Option<char>, lit: char, ci: bool) -> bool {
    match input {
        Some(ch) if ch == lit => true,
        Some(ch) if ci => ch.eq_ignore_ascii_case(&lit) || char_fold_eq(ch, lit),
        _ => false,
    }
}

/// Unicode-aware case-fold equality: two chars are equal if their lowercase (or
/// uppercase) mappings match. Covers non-ASCII letters that
/// `eq_ignore_ascii_case` misses.
fn char_fold_eq(a: char, b: char) -> bool {
    a.to_lowercase().eq(b.to_lowercase()) || a.to_uppercase().eq(b.to_uppercase())
}

pub struct Regex {
    prog: Vec<Inst>,
    ngroups: usize,
    /// Case-insensitive matching (`shopt -s nocasematch`). When set, `Char`
    /// and `Class` instructions match without regard to letter case.
    ci: bool,
}

/// Per-step NFA thread frontier with a `seen` set for `O(1)` dedupe, so each
/// program counter is added at most once per input position (keeps the run
/// linear and terminates epsilon cycles like `()*`).
struct ThreadList {
    threads: Vec<Thread>,
    seen: Vec<bool>,
}

struct Thread {
    pc: usize,
    caps: Vec<Option<usize>>,
}

impl ThreadList {
    fn new(n: usize) -> Self {
        ThreadList {
            threads: Vec::new(),
            seen: vec![false; n],
        }
    }

    fn clear(&mut self) {
        self.threads.clear();
        for s in &mut self.seen {
            *s = false;
        }
    }
}

impl Regex {
    /// Compile an ERE pattern.
    ///
    /// # Errors
    /// Returns [`EreError`] on a syntax error (unbalanced `(`/`[`, invalid
    /// `{m,n}`, unknown `[:class:]`, trailing `\`, …).
    pub fn new(pattern: &str) -> Result<Regex, EreError> {
        Self::new_flags(pattern, false)
    }

    /// Compile an ERE pattern with optional case-insensitive matching.
    ///
    /// # Errors
    /// Returns [`EreError`] on a syntax error, as [`Regex::new`].
    pub fn new_flags(pattern: &str, ci: bool) -> Result<Regex, EreError> {
        let mut parser = EParser {
            chars: pattern.chars().collect(),
            pos: 0,
            ngroups: 0,
        };
        let ast = parser.parse()?;
        let ngroups = parser.ngroups;

        let mut c = Compiler { prog: Vec::new() };
        // Unanchored search prefix: prefer entering the match at the current
        // position (leftmost) over skipping one char and retrying.
        //   0: Split(real, skip)
        //   1: Any            (skip)
        //   2: Jmp 0
        //   real: Save(0) … Save(1) Match
        let split = c.emit(Inst::Split(0, 0));
        let skip = c.emit(Inst::Any);
        c.emit(Inst::Jmp(split));
        let real = c.prog.len();
        c.prog[split] = Inst::Split(real, skip);
        c.emit(Inst::Save(0));
        c.compile(&ast);
        c.emit(Inst::Save(1));
        c.emit(Inst::Match);

        Ok(Regex {
            prog: c.prog,
            ngroups,
            ci,
        })
    }

    /// Number of capturing groups (excluding the whole-match group 0).
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.ngroups
    }

    /// `true` if the pattern matches anywhere in `text`.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.captures(text).is_some()
    }

    /// Find the leftmost match and return the captured substrings: index `0` is
    /// the whole match, `i` is capture group `i` (`None` if the group did not
    /// participate). Returns `None` if the pattern does not match.
    #[must_use]
    pub fn captures(&self, text: &str) -> Option<Vec<Option<String>>> {
        let chars: Vec<char> = text.chars().collect();
        let slots = self.run(&chars)?;
        let mut out = Vec::with_capacity(self.ngroups + 1);
        for g in 0..=self.ngroups {
            match (slots.get(2 * g).copied().flatten(), slots.get(2 * g + 1).copied().flatten()) {
                (Some(s), Some(e)) if s <= e && e <= chars.len() => {
                    out.push(Some(chars[s..e].iter().collect()));
                }
                _ => out.push(None),
            }
        }
        Some(out)
    }

    /// Run the Pike VM over `input`, returning the winning thread's capture
    /// slots (`2 × (ngroups + 1)` positions) or `None` if no match.
    fn run(&self, input: &[char]) -> Option<Vec<Option<usize>>> {
        let nslots = 2 * (self.ngroups + 1);
        let mut clist = ThreadList::new(self.prog.len());
        let mut nlist = ThreadList::new(self.prog.len());
        let mut matched: Option<Vec<Option<usize>>> = None;

        let mut caps = vec![None; nslots];
        self.add_thread(&mut clist, 0, 0, &mut caps, input);

        for sp in 0..=input.len() {
            if clist.threads.is_empty() {
                break;
            }
            let c = input.get(sp).copied();
            nlist.clear();
            let mut i = 0;
            while i < clist.threads.len() {
                let pc = clist.threads[i].pc;
                match &self.prog[pc] {
                    Inst::Char(ch) if char_eq(c, *ch, self.ci) => {
                        let mut caps = clist.threads[i].caps.clone();
                        self.add_thread(&mut nlist, pc + 1, sp + 1, &mut caps, input);
                    }
                    Inst::Any if c.is_some() => {
                        let mut caps = clist.threads[i].caps.clone();
                        self.add_thread(&mut nlist, pc + 1, sp + 1, &mut caps, input);
                    }
                    Inst::Class(d) if c.is_some_and(|ch| d.matches_ci(ch, self.ci)) => {
                        let mut caps = clist.threads[i].caps.clone();
                        self.add_thread(&mut nlist, pc + 1, sp + 1, &mut caps, input);
                    }
                    Inst::Match => {
                        // Highest-priority thread to reach Match wins; cut the
                        // remaining (lower-priority) threads at this step.
                        matched = Some(clist.threads[i].caps.clone());
                        break;
                    }
                    // Epsilon instructions are expanded by `add_thread`.
                    _ => {}
                }
                i += 1;
            }
            core::mem::swap(&mut clist, &mut nlist);
        }
        matched
    }

    /// Add `pc` (following epsilon transitions) to `list` at input position
    /// `sp`, threading capture slots. Deduped via `list.seen` so the first
    /// (highest-priority) path to each pc wins.
    fn add_thread(
        &self,
        list: &mut ThreadList,
        pc: usize,
        sp: usize,
        caps: &mut Vec<Option<usize>>,
        input: &[char],
    ) {
        if list.seen[pc] {
            return;
        }
        list.seen[pc] = true;
        match &self.prog[pc] {
            Inst::Jmp(x) => self.add_thread(list, *x, sp, caps, input),
            Inst::Split(x, y) => {
                self.add_thread(list, *x, sp, caps, input);
                self.add_thread(list, *y, sp, caps, input);
            }
            Inst::Save(n) => {
                let n = *n;
                let old = caps.get(n).copied().flatten();
                if let Some(slot) = caps.get_mut(n) {
                    *slot = Some(sp);
                }
                self.add_thread(list, pc + 1, sp, caps, input);
                if let Some(slot) = caps.get_mut(n) {
                    *slot = old;
                }
            }
            Inst::AssertStart => {
                if sp == 0 {
                    self.add_thread(list, pc + 1, sp, caps, input);
                }
            }
            Inst::AssertEnd => {
                if sp == input.len() {
                    self.add_thread(list, pc + 1, sp, caps, input);
                }
            }
            // Consuming/terminal instruction — becomes a live thread.
            _ => list.threads.push(Thread {
                pc,
                caps: caps.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pat: &str, s: &str) -> bool {
        Regex::new(pat).unwrap().is_match(s)
    }

    fn mi(pat: &str, s: &str) -> bool {
        Regex::new_flags(pat, true).unwrap().is_match(s)
    }

    #[test]
    fn case_insensitive() {
        // Literals fold case only under the ci flag.
        assert!(!m("^hello$", "HELLO"));
        assert!(mi("^hello$", "HELLO"));
        assert!(mi("^HeLLo$", "hello"));
        // Character-class ranges fold too.
        assert!(!m("^[a-z]+$", "Hello"));
        assert!(mi("^[a-z]+$", "Hello"));
        assert!(mi("^[A-Z]+$", "hello"));
        // Negated classes respect folding: `[^a-z]` should NOT match 'A' when ci.
        assert!(!mi("^[^a-z]+$", "ABC"));
    }

    #[test]
    fn literals_and_anchors() {
        assert!(m("foo", "a foo b"));
        assert!(!m("foo", "fo o"));
        assert!(m("^foo$", "foo"));
        assert!(!m("^foo$", "foobar"));
        assert!(m("^foo", "foobar"));
        assert!(m("bar$", "foobar"));
    }

    #[test]
    fn dot_and_quantifiers() {
        assert!(m("a.c", "axc"));
        assert!(!m("a.c", "ac"));
        assert!(m("ab*c", "ac"));
        assert!(m("ab*c", "abbbc"));
        assert!(m("ab+c", "abc"));
        assert!(!m("ab+c", "ac"));
        assert!(m("ab?c", "ac"));
        assert!(m("ab?c", "abc"));
        assert!(!m("ab?c", "abbc"));
    }

    #[test]
    fn bounded_repeat() {
        assert!(m("^a{2,4}$", "aa"));
        assert!(m("^a{2,4}$", "aaaa"));
        assert!(!m("^a{2,4}$", "a"));
        assert!(!m("^a{2,4}$", "aaaaa"));
        assert!(m("^a{3}$", "aaa"));
        assert!(!m("^a{3}$", "aa"));
        assert!(m("^a{2,}$", "aaaaa"));
        assert!(!m("^a{2,}$", "a"));
        // A zero-count interval deletes the atom rather than repeating it.
        assert!(m("^a{0}b$", "b"));
        assert!(m("^a{0,0}b$", "b"));
    }

    /// glibc — which is what bash's `=~` runs on — rejects a good deal that a
    /// lenient engine would happily accept. Everything here is measured against
    /// bash 5.2: each pattern makes `[[ x =~ … ]]` exit 2.
    #[test]
    fn rejects_what_glibc_rejects() {
        let bad = |pat: &str| {
            assert!(Regex::new(pat).is_err(), "expected {pat} to be rejected");
        };
        // A `{` that opens no well-formed interval is an error, not a literal.
        bad("a{b");
        bad("a{1");
        bad("a{}");
        bad("a{,3}");
        bad("a{1,2,3}");
        bad("{b");
        // Only a backslash or a bracket expression gets a literal brace.
        assert!(m("^a\\{b$", "a{b"));
        assert!(m("^a[{]b$", "a{b"));
        // A quantifier needs an atom before it — in every context.
        bad("*a");
        bad("+a");
        bad("?a");
        bad("{2}a");
        bad("(*a)");
        bad("a|*b");
        // `^` is an assertion, not an atom; `$` glibc does let you quantify.
        bad("^*a");
        bad("a^*b");
        assert!(Regex::new("a$*").is_ok());
        // One quantifier per atom.
        bad("a**");
        bad("a*+");
        bad("a*?");
        bad("a?+");
        bad("a{1}*");
        bad("a*{1}");
        bad("a{1}{2}");
        bad("(a)*+");
        // Every branch of an alternation has to be a real expression …
        bad("|a");
        bad("a|");
        bad("(a|)");
        bad("(|a)");
        bad("(a||b)");
        // … and a run emptied by a zero-count interval counts as no expression,
        // even though an explicitly empty one is fine.
        bad("a{0}");
        bad("(a{0})");
        bad("a{0}|b");
        bad("a{0}b{0}");
        assert!(Regex::new("()").is_ok());
        assert!(Regex::new("^a{0}").is_ok());
        assert!(Regex::new("a{0}$").is_ok());
        // An empty pattern is not an empty match.
        bad("");
    }

    #[test]
    fn classes() {
        assert!(m("[abc]", "x b y"));
        assert!(!m("^[abc]+$", "abd"));
        assert!(m("^[a-z]+$", "hello"));
        assert!(!m("^[a-z]+$", "Hello"));
        assert!(m("^[^0-9]+$", "abc"));
        assert!(!m("^[^0-9]+$", "ab3"));
        // Literal `]` as first class member, and literal `-` at the end.
        assert!(m("^[]a]+$", "]a]"));
        assert!(m("^[a-]+$", "a-a"));
    }

    #[test]
    fn posix_classes() {
        assert!(m("^[[:digit:]]+$", "12345"));
        assert!(!m("^[[:digit:]]+$", "12a45"));
        assert!(m("^[[:alpha:]]+$", "abcXYZ"));
        assert!(m("^[[:alnum:]]+$", "ab12"));
        assert!(m("[[:space:]]", "a b"));
    }

    #[test]
    fn alternation_and_groups() {
        assert!(m("^(cat|dog|bird)$", "dog"));
        assert!(!m("^(cat|dog)$", "cow"));
        assert!(m("^(ab)+$", "ababab"));
        assert!(!m("^(ab)+$", "aba"));
    }

    #[test]
    fn escapes() {
        assert!(m(r"a\.c", "a.c"));
        assert!(!m(r"a\.c", "axc"));
        assert!(m(r"\(x\)", "(x)"));
        assert!(m(r"a\\b", r"a\b"));
    }

    #[test]
    fn captures_extracted() {
        let re = Regex::new(r"([0-9]+)-([0-9]+)").unwrap();
        let caps = re.captures("range 10-25 end").unwrap();
        assert_eq!(caps[0].as_deref(), Some("10-25"));
        assert_eq!(caps[1].as_deref(), Some("10"));
        assert_eq!(caps[2].as_deref(), Some("25"));
    }

    #[test]
    fn leftmost_match() {
        // Leftmost start wins; greedy length at that start.
        let re = Regex::new("a+").unwrap();
        let caps = re.captures("baaa").unwrap();
        assert_eq!(caps[0].as_deref(), Some("aaa"));
    }

    #[test]
    fn no_catastrophic_backtracking() {
        // A classic ReDoS pattern: the Pike VM must handle it in linear time
        // (this returns quickly rather than hanging).
        let re = Regex::new("(a+)+$").unwrap();
        let input = "a".repeat(40) + "!";
        assert!(!re.is_match(&input));
    }

    #[test]
    fn errors() {
        assert!(Regex::new("(unclosed").is_err());
        assert!(Regex::new("[unclosed").is_err());
        assert!(Regex::new(r"trailing\").is_err());
        assert!(Regex::new("a{2,1}").is_err());
        assert!(Regex::new("[[:bogus:]]").is_err());
    }
}
