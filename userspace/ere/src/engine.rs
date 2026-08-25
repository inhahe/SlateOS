//! A small POSIX Extended Regular Expression (ERE) engine.
//!
//! This is the crate's matcher. Its first caller was the shell's
//! `[[ str =~ re ]]`, whose semantics are POSIX ERE (bash matches it with the C
//! library's `regexec`); `grep -E`, `sed`, `awk` and `expr` want the same
//! engine, reached through [`crate::bre`] where the dialect is Basic rather
//! than Extended. See the crate docs for why they share one.
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
//! `=~`, and the behaviour POSIX requires of `grep` and `sed`.
//!
//! The ReDoS argument is stronger for the utilities than it was for the shell:
//! `grep -f patterns.txt` and `sed -f script.sed` take their pattern from a
//! *file*, so the pattern is as much untrusted input as the subject is.
//!
//! ## Supported syntax
//! `. ^ $`, literals, `\`-escapes (`\.`, `\(`, `\\`, `\n`, `\t`, `\r`, …),
//! grouping `( … )` (capturing → `BASH_REMATCH`), alternation `a|b`, the
//! quantifiers `* + ?` and bounded `{m}` / `{m,}` / `{m,n}` (greedy), and
//! bracket expressions `[...]` / `[^...]` with ranges (`a-z`), literal-`]`/`-`
//! placement, and POSIX classes (`[[:digit:]]`, `[[:alpha:]]`, …). Non-ERE
//! Perl shorthands (`\d`, `\w`, `\s`, non-greedy `*?`, backreferences) are
//! intentionally not provided — `bash`'s `=~` is POSIX ERE, not PCRE.
//!
//! ## Characters, not bytes and not `char`s
//! A shell value is bytes, and a SlateOS path may hold any byte but `/` and
//! NUL, so both the subject and the pattern can contain a byte that begins no
//! valid UTF-8 sequence. The engine therefore scans [`Ch`] — a decoded scalar
//! *or* one undecodable byte — exactly as the glob engine does. That is the
//! only reading that makes `.` match such a byte as **one** character rather
//! than as a third of an `é`, and it is what lets `[[ $f =~ ^a ]]` answer for a
//! filename the locale cannot decode. ERE *syntax* is entirely ASCII, so every
//! metacharacter test goes through [`Ch::as_ascii`] and no encoding question
//! arises in the parser.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::ch::{self as bytes, BStr, Ch, Str};

/// Concatenate the pieces of a diagnostic that quotes bytes back.
///
/// The shell builds these with its `bfmt!` macro; exactly two messages here
/// need it and nothing else does, so this crate spells the concatenation out
/// rather than carrying a macro across a crate boundary for two call sites.
fn cat(parts: &[BStr<'_>]) -> Str {
    let mut out = Str::new();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// POSIX's classification of a pattern that would not compile — the `REG_*`
/// codes, with glibc's own English for each.
///
/// This exists because "what is wrong with the pattern" and "what does a GNU
/// utility print about it" are two different questions, and this crate had only
/// been answering the first. Every consumer here is a work-alike of a GNU tool
/// whose diagnostics are compared against the original, and all of them report a
/// bad pattern by printing back whatever glibc's `re_compile_pattern` returned
/// — one of the fourteen fixed sentences below and nothing else. Carrying the
/// code rather than the sentence keeps [`EreError::detail`]'s more specific
/// wording available for tests and for any caller that would rather be helpful
/// than compatible.
///
/// The strings are transcribed from glibc's `re_error_msgid`, and the mapping
/// from pattern to code is *measured* against findutils 4.9.0 on glibc 2.39 —
/// see the `-regex` block in `scripts/find-diff.sh`, which pins every one of
/// them. It is measured rather than reasoned because it is not reasonable: a
/// pattern of just `[` is `REG_BADPAT`, while `[a` is `REG_EBRACK`, since glibc
/// reaches its "premature end of pattern" path before it decides the bracket
/// was the problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegCode {
    /// `REG_BADPAT`, which glibc also uses for a pattern that simply ran out.
    BadPattern,
    /// `REG_ECTYPE`: `[[:nosuch:]]`.
    BadCharClass,
    /// `REG_EESCAPE`: the pattern ends in a backslash.
    TrailingBackslash,
    /// `REG_ESUBREG`: `\9` with no ninth group.
    BadBackReference,
    /// `REG_EBRACK`: a `[` that is never closed.
    UnmatchedBracket,
    /// `REG_EPAREN`: a `(` or `\(` that is never closed.
    UnmatchedParen,
    /// `REG_ERPAREN`: a `\)` with nothing open. (Plain `)` is an *ordinary
    /// character* in a POSIX ERE, not an error.)
    UnmatchedRightParen,
    /// `REG_EBRACE`: a `{` or `\{` that is never closed.
    UnmatchedBrace,
    /// `REG_BADBR`: closed, but the counts inside are not usable — `a{1,0}`.
    BadBraceContent,
    /// `REG_ERANGE`: `[z-a]`.
    BadRangeEnd,
    /// `REG_BADRPT`: a quantifier with nothing in front of it.
    BadRepeat,
    /// `REG_ESIZE`: the compiled program would be too large.
    TooBig,
}

impl RegCode {
    /// glibc's sentence for this code, byte for byte.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::BadPattern => "Invalid regular expression",
            Self::BadCharClass => "Invalid character class name",
            Self::TrailingBackslash => "Trailing backslash",
            Self::BadBackReference => "Invalid back reference",
            Self::UnmatchedBracket => "Unmatched [, [^, [:, [., or [=",
            Self::UnmatchedParen => "Unmatched ( or \\(",
            Self::UnmatchedRightParen => "Unmatched ) or \\)",
            Self::UnmatchedBrace => "Unmatched \\{",
            Self::BadBraceContent => "Invalid content of \\{\\}",
            Self::BadRangeEnd => "Invalid range end",
            Self::BadRepeat => "Invalid preceding regular expression",
            Self::TooBig => "Regular expression too big",
        }
    }
}

/// A compile-time error in an ERE pattern.
///
/// [`Self::detail`] is bytes because two of them quote a slice of the pattern
/// back — the offending character of a stray-`)` error and the endpoints of an
/// invalid range — and a pattern character need not be text. There is
/// deliberately no `Display`: a caller has to choose between the two wordings,
/// and which one is right depends on whether it is imitating a GNU tool
/// ([`Self::message`]) or explaining itself to a human ([`Self::detail`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EreError {
    /// This crate's own wording: more specific than POSIX's, and not stable.
    pub detail: Str,
    /// What glibc would have called this failure.
    pub code: RegCode,
}

impl EreError {
    /// Build one from a code and this crate's own wording.
    pub(crate) fn new(code: RegCode, detail: impl Into<Str>) -> Self {
        Self {
            detail: detail.into(),
            code,
        }
    }

    /// glibc's sentence for this failure — what a GNU work-alike should print.
    #[must_use]
    pub fn message(&self) -> &'static str {
        self.code.message()
    }
}

/// A search that was abandoned because it exceeded its backtracking budget.
///
/// Only a pattern containing a backreference can produce one: everything else
/// runs on the Pike VM, whose cost is `O(len(subject) × len(program))` with no
/// search at all. Matching a backreference is NP-hard in general, so the
/// backtracker that handles those patterns has a step budget
/// ([`Regex::backtrack_budget`]) and gives up rather than running for ever.
///
/// It is a distinct outcome from "did not match" on purpose, and the whole
/// reason this crate's matching API returns `Result`. `sed '/re/!d'` deletes
/// every line the pattern does *not* match; if an abandoned search were
/// reported as "no match", that would delete the user's data on the strength of
/// a question we declined to answer. A caller has to be told the difference so
/// it can say so and stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchLimit;

/// One match's capture groups as byte spans: index `0` is the whole match, `i`
/// is group `i`, `None` for a group that did not participate.
///
/// Named because it appears in four signatures and inside a `Result` in all of
/// them; spelled out, the type is long enough that the reader stops reading it.
pub type GroupSpans = Vec<Option<(usize, usize)>>;

impl core::fmt::Display for MatchLimit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("backreference matching exceeded its step limit")
    }
}

// `core::error::Error`, not `std::error::Error` — they are the same trait since
// Rust 1.81, `std` merely re-exporting `core`'s, so a `std` caller can still put
// this in a `Box<dyn std::error::Error>` and a `no_std` one can still ask it for
// a source.
impl core::error::Error for MatchLimit {}

/// The fixed part of a backreference search's step budget.
///
/// Sized so that an honestly-written backreference pattern never reaches it.
/// The classic one — `sed '$!N;/^\(.*\)\n\1$/!P;D'`, the `sed` spelling of
/// `uniq` — costs about `len²` steps on a pair of lines, so a thousand-character
/// line is ~10⁶.
const BACKTRACK_BUDGET_BASE: u64 = 1_000_000;

/// Extra budget per character of subject, so a long subject is not refused
/// merely for being long: the honest patterns' cost grows with it too.
const BACKTRACK_BUDGET_PER_CHAR: u64 = 1_000;

/// Ceiling on the whole budget.
///
/// Without it a large subject would buy an arbitrarily long search, and a
/// pathological pattern would turn a bounded refusal back into a hang that
/// merely takes longer to notice. At roughly ten million steps a second this is
/// a few seconds at the very worst, and is only reachable by a pattern built to
/// reach it.
const BACKTRACK_BUDGET_MAX: u64 = 100_000_000;

/// Upper bound on a *single* `{m,n}` count: glibc's `RE_DUP_MAX`, which is what
/// a caller's pattern was written against. (POSIX only requires 255.)
///
/// Exceeding it is `REG_ESIZE`, "Regular expression too big" — the one error
/// the count itself can produce, as distinct from the two a malformed interval
/// produces. Matching glibc's number rather than picking a smaller one matters
/// because the difference is silent in the only direction that hurts: a pattern
/// GNU compiles and we reject looks to the caller like a broken tool.
///
/// This bounds one interval and nothing else. Intervals compose by
/// multiplication — `(a{1000}){1000}` is a million copies of `a` and
/// `((a{1000}){1000}){1000}` is a billion — so the thing that actually bounds
/// compilation is [`MAX_PROG`], not this.
const MAX_REPEAT: usize = 32_767;

/// Upper bound on the size of the compiled program, in instructions.
///
/// [`MAX_REPEAT`] alone does not bound anything: repetition counts *multiply*
/// under nesting, so `((a{1000}){1000}){1000}` asks for ~10⁹ instructions —
/// tens of gigabytes — from a 24-byte pattern. That is a denial of service in
/// every caller, and the pattern is not always the operator's own text:
/// `grep -f patterns.txt` and `sed -f script.sed` read it from a file, and
/// osh's `[[ $s =~ $re ]]` takes it from a variable. Refusing to compile is the
/// only answer that stays inside the process.
///
/// The cap is also what bounds *matching*, not just compilation: the Pike VM
/// visits every live instruction at every input position, so the cost of a
/// search is `O(len(input) × len(prog))`. A program this size is already the
/// most a caller can force; without the cap there is no upper bound on either
/// axis.
///
/// 65 536 is far above any pattern written to be read — `a{1000}b{1000}` is
/// 2000 instructions, and a hand-written pattern rarely reaches 100 — and small
/// enough that the program itself is a couple of megabytes at worst.
const MAX_PROG: usize = 65_536;

// ---- AST --------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Node {
    Empty,
    Lit(Ch),
    Any,
    Class(ClassData),
    Start,
    End,
    /// A zero-width assertion about the word characters either side of the
    /// current position: `\b`, `\B`, `\<` or `\>`.
    Word(WordAssert),
    /// Capturing group with its 1-based group index.
    Group(usize, Box<Node>),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    /// `\1`–`\9`: match the same text a capturing group already matched.
    ///
    /// The number is the 1-based group index, exactly as [`Node::Group`] counts
    /// them. A pattern holding one of these cannot be run by the Pike VM at all
    /// — see [`Regex::backtrack_at`] — so its presence is what selects the
    /// engine, not just an extra instruction.
    Backref(usize),
    Repeat {
        node: Box<Node>,
        min: usize,
        /// `None` = unbounded (`*`, `+`, `{m,}`).
        max: Option<usize>,
    },
}

/// Which of the four GNU word assertions a [`Node::Word`] is.
///
/// All four are decided by the same two facts — whether the character before
/// the position is a word character, and whether the one after it is — so they
/// share an instruction and differ only in how they combine them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordAssert {
    /// `\b` — one side is a word character and the other is not.
    Boundary,
    /// `\B` — the two sides agree, *including* when neither is a word
    /// character. That is why `\B` matches inside a run of spaces, and matches
    /// the empty subject: both sides are "off the end", which is not a word
    /// character, so they agree.
    NotBoundary,
    /// `\<` — the start of a word.
    Start,
    /// `\>` — the end of a word.
    End,
}

/// Whether `c` is a word character, for [`WordAssert`].
///
/// The same set `\w` abbreviates, and deliberately the same *code path*: if the
/// two ever disagreed, `\bfoo` and `\Wfoo` would draw the word's edge in
/// different places, and no one reading either would suspect it.
///
/// An undecodable byte is not a word character. In a UTF-8 locale glibc agrees,
/// since such a byte is not alphanumeric there either — `sed 's/\b/|/g'` on
/// `café` puts no bar inside the `é` under `C.UTF-8`, and puts one before its
/// bytes under `C`, which is that locale reading them as non-letters.
fn is_word_ch(c: Ch) -> bool {
    PosixClass::Alnum.matches(c) || c == Ch::U('_')
}

/// The class `\w` abbreviates — `[[:alnum:]_]` — or `\W`, its negation.
fn word_class(negated: bool) -> ClassData {
    ClassData {
        negated,
        ranges: vec![(Ch::U('_'), Ch::U('_'))],
        posix: vec![PosixClass::Alnum],
    }
}

/// The class `\s` abbreviates — `[[:space:]]` — or `\S`, its negation.
fn space_class(negated: bool) -> ClassData {
    ClassData {
        negated,
        ranges: Vec::new(),
        posix: vec![PosixClass::Space],
    }
}

impl WordAssert {
    /// Whether this assertion holds at character position `sp` of `input`.
    ///
    /// Off either end counts as "not a word character", which is what makes
    /// `\bx\b` match the whole of a one-character subject.
    fn holds_at(self, input: &[Ch], sp: usize) -> bool {
        let before = sp
            .checked_sub(1)
            .and_then(|i| input.get(i))
            .copied()
            .is_some_and(is_word_ch);
        let after = input.get(sp).copied().is_some_and(is_word_ch);
        match self {
            WordAssert::Boundary => before != after,
            WordAssert::NotBoundary => before == after,
            WordAssert::Start => !before && after,
            WordAssert::End => before && !after,
        }
    }
}

#[derive(Debug, Clone)]
struct ClassData {
    negated: bool,
    /// Inclusive character ranges (a single character is `(c, c)`).
    ///
    /// A range is compared with [`Ch`]'s derived `Ord`, which orders every
    /// decoded scalar below every undecodable byte. So an undecodable byte falls
    /// in no written range — right, because it is not a letter and no collation
    /// would place it among them — while `[^a-z]` still matches it, as bash in
    /// the C locale does.
    ranges: Vec<(Ch, Ch)>,
    posix: Vec<PosixClass>,
}

impl ClassData {
    /// Whether `c` is in the class's *positive* set (before applying negation).
    fn hit_positive(&self, c: Ch) -> bool {
        self.ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi)
            || self.posix.iter().any(|p| p.matches(c))
    }

    /// Case-aware match. When `ci` is set, the positive set is tested against
    /// the character and its case-folded variants *before* negation is applied,
    /// so a negated class like `[^a-z]` correctly excludes `A` under
    /// `nocasematch`. An undecodable byte has no case, so it folds to itself and
    /// this reduces to the plain test.
    fn matches_ci(&self, c: Ch, ci: bool) -> bool {
        let mut hit = self.hit_positive(c);
        if ci && !hit {
            for alt in c.to_lowercase().into_iter().chain(c.to_uppercase()) {
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

    /// A byte that decodes to no character belongs to no class: it is not a
    /// letter, not a digit and not printable. bash's `iswalpha` family answers
    /// the same way for a byte the locale cannot decode, and the glob engine's
    /// `posix_class_matches` is written the same way.
    fn matches(self, c: Ch) -> bool {
        let Some(c) = c.as_char() else {
            return false;
        };
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

/// Which ERE dialect a pattern is written in.
///
/// The grammar is the same in both; what differs is what happens to two kinds
/// of nonsense. That is not a distinction this crate invented — it is glibc's
/// `reg_syntax_t` bits, and our callers genuinely want different ones:
/// `osh`'s `[[ =~ ]]`, `find -regextype posix-extended`, `sed -E` and `awk`
/// want [`Syntax::POSIX_EXTENDED`], while `grep -E` wants [`Syntax::EGREP`].
///
/// The two-caller problem is real and was measured, not assumed. Against
/// glibc 2.39 / findutils 4.9.0 / grep 3.11, in `C.UTF-8`:
///
/// | pattern | `find -regextype posix-extended` | `grep -E` |
/// |---|---|---|
/// | `*a` `+a` `?a` `{2}a` `{b}a` `{}a` `{1,2,3}a` `{a` | `REG_BADRPT` | accepted, warned about, matches `a` |
/// | `a{b}` `a{` `a{2` `a{1,` `a{1,b}` `a{,b}` | `REG_EBRACE`/`REG_BADBR` | accepted, the `{` is a literal |
/// | `a{}` `a{1,2,3}` `a{1,0}` | `REG_BADBR` | `REG_BADBR` — same |
/// | `a{99999999}` | `REG_ESIZE` | `REG_ESIZE` — same |
/// | `(` `((a)` | `REG_EPAREN` | `REG_EPAREN` — same |
/// | `)` `a)` `a**` `a{,3}` `a|` `|a` `(|a)` | accepted | accepted — same |
///
/// A `grep` built on the POSIX-extended reading refuses patterns GNU `grep`
/// runs, which is the failure the difference exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Syntax {
    /// A quantifier with nothing to quantify repeats the empty expression
    /// instead of being an error, so `*a` is `a` and `{2}a` is `a`.
    ///
    /// glibc's `RE_CONTEXT_INDEP_OPS` without `RE_CONTEXT_INVALID_OPS`.
    pub context_indep_ops: bool,
    /// A `{` that does not open a well-formed interval is a literal brace
    /// instead of an error, so `a{b}` matches the four characters `a{b}`.
    ///
    /// glibc's `RE_INVALID_INTERVAL_ORD`. Note that it does *not* excuse every
    /// malformed interval: a `{…}` whose content is present but wrong — `a{}`,
    /// `a{1,2,3}`, `a{1,0}` — is still `REG_BADBR` under both dialects, and
    /// `a{99999999}` is still `REG_ESIZE`. Only the forms glibc *rolls back*
    /// become literals; see [`EParser::parse_brace`].
    pub invalid_interval_ord: bool,
}

impl Syntax {
    /// `RE_SYNTAX_POSIX_EXTENDED`: what `osh`, `find`, `sed -E` and `awk` use.
    pub const POSIX_EXTENDED: Syntax = Syntax {
        context_indep_ops: false,
        invalid_interval_ord: false,
    };

    /// `RE_SYNTAX_EGREP` as GNU `grep -E` applies it.
    pub const EGREP: Syntax = Syntax {
        context_indep_ops: true,
        invalid_interval_ord: true,
    };
}

impl Default for Syntax {
    fn default() -> Self {
        Self::POSIX_EXTENDED
    }
}

/// One bound of a `{m,n}` interval as [`EParser::fetch_number`] read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BraceNum {
    Value(usize),
    /// No digits at all: the `{,3}` and `{}` case.
    Absent,
    /// Something that is not a count — a non-digit, or the end of the pattern.
    Invalid,
}

/// What ended one bound of a `{m,n}` interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BraceStop {
    Comma,
    Close,
    /// The pattern ran out before the interval was closed.
    End,
}

struct EParser {
    chars: Vec<Ch>,
    pos: usize,
    ngroups: usize,
    /// How many `(` are open at the cursor.
    ///
    /// It exists only to answer whether a `)` closes a group or is an ordinary
    /// character — see [`EParser::parse_concat`].
    depth: usize,
    /// Which dialect's answer to give for the two kinds of nonsense that
    /// distinguish them — see [`Syntax`].
    syntax: Syntax,
}

impl EParser {
    fn peek(&self) -> Option<Ch> {
        self.chars.get(self.pos).copied()
    }

    /// Consume `n` characters.
    ///
    /// Saturating, and every advance in this parser goes through it. The cursor
    /// only moves past a character [`Self::peek`] returned, so it never leads
    /// `chars.len()` by more than the two-character `[:`/`:]` step and could not
    /// overflow — but a parser is exactly where an off-by-one becomes a panic
    /// deep in someone else's `grep`, and one saturating add per character is
    /// not a cost worth arguing about.
    fn bump(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n);
    }

    /// The cursor's character *if it is ASCII*, which is what every syntax test
    /// below asks for: ERE metacharacters are all ASCII, and no byte of a
    /// multi-byte character — decodable or not — can be mistaken for one.
    fn peek_ascii(&self) -> Option<char> {
        self.peek().and_then(Ch::as_ascii)
    }

    fn peek_ascii_at(&self, off: usize) -> Option<char> {
        self.chars
            .get(self.pos.saturating_add(off))
            .copied()
            .and_then(Ch::as_ascii)
    }

    fn parse(&mut self) -> Result<Node, EreError> {
        // An empty pattern is legal and matches the empty string, which is not
        // what the comment here used to claim. Measured on bash 5.2.21 and
        // findutils 4.9.0/glibc 2.39: `[[ x =~ "" ]]` is status 0, and `find
        // -regextype posix-extended -regex ''` compiles and simply matches
        // nothing (because `-regex` must match a whole path, and no path is
        // empty). See the acceptance test for the rest of that survey.
        let node = self.parse_alt()?;
        if self.pos != self.chars.len() {
            // Nothing else may be left. A stray `)` cannot reach here — it is
            // an ordinary character, see `parse_concat` — so this is the
            // unreachable-in-practice backstop rather than the `)` path it
            // used to be. The character is quoted back as its own bytes: it is
            // a slice of the pattern, which need not be text.
            let at = self.peek().map(Ch::to_str).unwrap_or_default();
            return Err(EreError::new(
                RegCode::BadPattern,
                cat(&[b"unexpected '", &at, b"' in regex"]),
            ));
        }
        Ok(node)
    }

    fn parse_alt(&mut self) -> Result<Node, EreError> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek_ascii() == Some('|') {
            self.bump(1);
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap_or(Node::Empty))
        } else {
            // An empty branch is legal: `a|` matches "a" or nothing, and `|a`
            // and `(a||b)` likewise. Measured; the previous rejection here was
            // wrong on all three.
            Ok(Node::Alt(branches))
        }
    }

    /// Parse a run of repeated atoms up to `|`, an *enclosing* `)`, or the end
    /// of the pattern.
    ///
    /// Whether a `)` ends the run is exactly the question of whether a group is
    /// open, because a POSIX ERE gives `)` no meaning of its own:
    /// `RE_UNMATCHED_RIGHT_PAREN_ORD` is set in `RE_SYNTAX_POSIX_EXTENDED`, so
    /// `find -regex 'a)'` matches a file called `a)` rather than failing to
    /// compile. That is why the depth has to be tracked rather than simply
    /// breaking on the character.
    fn parse_concat(&mut self) -> Result<Node, EreError> {
        let mut parts = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || (c == ')' && self.depth > 0) {
                break;
            }
            parts.push(self.parse_repeat()?);
        }
        match parts.len() {
            0 => Ok(Node::Empty),
            1 => Ok(parts.pop().unwrap_or(Node::Empty)),
            _ => Ok(Node::Concat(parts)),
        }
    }

    /// Parse one atom and every quantifier stacked on it.
    fn parse_repeat(&mut self) -> Result<Node, EreError> {
        if is_quantifier_start(self.peek_ascii()) {
            // A quantifier has to have something to quantify — under
            // `RE_SYNTAX_POSIX_EXTENDED`, where `RE_CONTEXT_INVALID_OPS` makes
            // it `REG_BADRPT`. glibc rejects `*a`, `?a`, `{2}a` and — because
            // an alternation branch and a group each start a fresh expression
            // — `a|*b` and `(*a)` too. Every *malformed* interval in this
            // position gets the same `REG_BADRPT` rather than its own brace
            // error, `{b}a` and `{}a` and `{a` included: measured against
            // findutils 4.9.0, which reports "Invalid preceding regular
            // expression" for all of them.
            if !self.syntax.context_indep_ops {
                return Err(nothing_to_repeat());
            }
            // Under egrep syntax the quantifier repeats the *empty*
            // expression, which is grep's dfa.c reading (its `atom()` emits
            // `EMPTY` when the token is already a repetition operator) and is
            // what grep actually prints: `grep -E '{2}a'` warns and then
            // matches every line containing `a`, exactly as `grep -E 'a'`
            // does.
            //
            // A brace that will not parse is a literal `{` here, never an
            // error, even for the `a{}` and `a{1,2,3}` shapes that *are*
            // errors after a real atom. That asymmetry is GNU's, not ours:
            // grep decides accept/reject with glibc's `regcomp`, which at the
            // start of an expression skips the offending token instead of
            // judging it, and decides what *matches* with dfa.c, which makes
            // it a literal. `grep -E '{}a'` and `grep -E '{1,2,3}a'` therefore
            // exit 1 with no diagnostic, while `grep -E 'a{}'` exits 2 —
            // measured, all three.
            let mark = self.pos;
            match self.parse_quantifier() {
                Ok(Some((min, max))) => {
                    let node = repeat(Node::Empty, min, max);
                    return self.stack_quantifiers(node);
                }
                // A `{` that rolled back to a literal; fall through to
                // `parse_atom`, which will take it as one.
                Ok(None) => {}
                Err(_) => self.pos = mark,
            }
        }
        let atom = self.parse_atom()?;
        self.stack_quantifiers(atom)
    }

    /// Apply every quantifier at the cursor to `node`, innermost first.
    ///
    /// A *second* quantifier is not an error. glibc simply wraps the expression
    /// again, so `a**` is `(a*)*`, `a*?` is `(a*)?` and `a{1}{2}` is `(a{1}){2}`;
    /// all three compile, and `find -regex 'a**'` accordingly runs. (The extra
    /// layer is a no-op for matching in every case that can be written, since
    /// repeating an already-repeated expression widens nothing — but what
    /// matters here is that it *compiles*, because the previous reading of
    /// "quantifier already applied" as an error made those four patterns fail.)
    fn stack_quantifiers(&mut self, mut node: Node) -> Result<Node, EreError> {
        while let Some((min, max)) = self.parse_quantifier()? {
            // `^` is an assertion, not an atom, so glibc reports `^*` and
            // `a^*b` the way it reports a leading `*`. `$` it does accept.
            // Under egrep syntax nothing here is an error at all, and `^*`
            // becomes the anchor repeated zero-or-more times — which is why
            // `grep -E 'a^*b'` matches "ab": zero repetitions of an assertion
            // that can never hold is the empty string.
            if matches!(node, Node::Start) && !self.syntax.context_indep_ops {
                return Err(nothing_to_repeat());
            }
            node = repeat(node, min, max);
        }
        Ok(node)
    }

    /// Consume a `*` / `+` / `?` / `{m,n}` quantifier if one is at the cursor.
    fn parse_quantifier(&mut self) -> Result<Option<(usize, Option<usize>)>, EreError> {
        match self.peek_ascii() {
            Some('*') => {
                self.bump(1);
                Ok(Some((0, None)))
            }
            Some('+') => {
                self.bump(1);
                Ok(Some((1, None)))
            }
            Some('?') => {
                self.bump(1);
                Ok(Some((0, Some(1))))
            }
            // `parse_brace` answers `None` when the `{` is not an interval at
            // all, which under egrep syntax means it is an ordinary character.
            // That is the same `None` this function uses for "no quantifier
            // here", and deliberately so: to the caller the two are the same
            // fact — the cursor has not moved and the next thing is an atom.
            Some('{') => self.parse_brace(),
            _ => Ok(None),
        }
    }

    /// Parse a `{m}` / `{m,}` / `{,n}` / `{m,n}` interval at the cursor.
    ///
    /// An unescaped `{` that does not open a well-formed interval is an error,
    /// not a literal brace — `RE_INVALID_INTERVAL_ORD` is clear in
    /// `RE_SYNTAX_POSIX_EXTENDED` — but *which* error it is depends on
    /// something less obvious than it looks, so this is written as a
    /// transcription of glibc's `parse_dup_op` rather than as its own reading:
    ///
    /// | pattern | glibc | why |
    /// |---|---|---|
    /// | `a{`, `a{2`, `a{b` | `REG_EBRACE` | the pattern ended before a `}` |
    /// | `a{}`, `a{x}`, `a{1,2,3}` | `REG_BADBR` | a `}` was found; what preceded it was not a count |
    /// | `a{99999999}` | `REG_ESIZE` | a count above `RE_DUP_MAX` |
    ///
    /// The dividing line is "did a `}` ever turn up", not "was the content
    /// sane" — which is why the scan below runs to the `}` before judging
    /// anything. An absent lower bound is zero (`{,3}` is `{0,3}` and `{,}` is
    /// `{0,}`), which POSIX does not require and glibc nonetheless does.
    ///
    /// Under [`Syntax::EGREP`] the first two rows are not errors: glibc's
    /// `RE_INVALID_INTERVAL_ORD` makes it *roll back* to where the `{` was and
    /// hand the brace on as an ordinary character, which is reported here as
    /// `Ok(None)`. Only the `-2` ("this is not a count") cases roll back;
    /// `a{}`, `a{1,2,3}`, `a{1,0}` and `a{99999999}` stay errors in both
    /// dialects, because glibc has already committed to reading an interval by
    /// the time it checks them. That is exactly why the rollback is expressed
    /// as a return value from the middle of this function rather than as "try
    /// it and reset the cursor if anything at all goes wrong" — the two halves
    /// of glibc's error set behave differently and the difference is the whole
    /// point.
    fn parse_brace(&mut self) -> Result<Option<(usize, Option<usize>)>, EreError> {
        let unmatched = || {
            EreError::new(
                RegCode::UnmatchedBrace,
                b"unterminated '{' in regex".to_vec(),
            )
        };
        let bad = || {
            EreError::new(
                RegCode::BadBraceContent,
                b"invalid interval in regex".to_vec(),
            )
        };
        let open = self.pos;
        // Rewind to the `{` and report it as no interval at all.
        macro_rules! rollback {
            () => {{
                self.pos = open;
                return Ok(None);
            }};
        }
        self.bump(1); // consume '{'

        let (first, stop) = self.fetch_number();
        let min = match first {
            BraceNum::Value(n) => n,
            // `{,n}` — an absent lower bound is zero, but only when a comma is
            // what ended it. `{}` has no comma and is an error.
            BraceNum::Absent if stop == BraceStop::Comma => 0,
            BraceNum::Absent => return Err(bad()),
            BraceNum::Invalid => {
                if self.syntax.invalid_interval_ord {
                    rollback!();
                }
                return Err(if stop == BraceStop::End {
                    unmatched()
                } else {
                    bad()
                });
            }
        };
        let (max, stop) = match stop {
            // `{m}` is `{m,m}`.
            BraceStop::Close => (Some(min), stop),
            BraceStop::Comma => {
                let (second, stop) = self.fetch_number();
                match second {
                    BraceNum::Value(n) => (Some(n), stop),
                    // `{m,}` — an absent upper bound is no bound.
                    BraceNum::Absent => (None, stop),
                    BraceNum::Invalid => {
                        if self.syntax.invalid_interval_ord {
                            rollback!();
                        }
                        return Err(if stop == BraceStop::End {
                            unmatched()
                        } else {
                            bad()
                        });
                    }
                }
            }
            // Unreachable: `fetch_number` only stops at the end of the pattern
            // by returning `Invalid`, which the match above has consumed.
            BraceStop::End => return Err(unmatched()),
        };
        // A second comma (`{1,2,3}`) leaves the scan stopped somewhere other
        // than the closing brace.
        if stop != BraceStop::Close {
            return Err(bad());
        }
        // Order before size, as glibc checks them: `{5,3}` is `REG_BADBR` even
        // though both bounds are enormous.
        if max.is_some_and(|n| min > n) {
            return Err(bad());
        }
        if max.unwrap_or(min) > MAX_REPEAT {
            return Err(EreError::new(
                RegCode::TooBig,
                b"repetition count too large".to_vec(),
            ));
        }
        Ok(Some((min, max)))
    }

    /// glibc's `fetch_number`: read one bound of an interval, consuming the
    /// `,` or `}` that ends it.
    ///
    /// It reports the value *and* what stopped it, because the caller needs
    /// both — see [`EParser::parse_brace`]. Note that it keeps scanning past a
    /// character that is not a digit rather than stopping there, so `{b}`
    /// consumes its `}` and is distinguishable from `{b`.
    fn fetch_number(&mut self) -> (BraceNum, BraceStop) {
        let mut num = BraceNum::Absent;
        loop {
            let Some(c) = self.peek() else {
                // The pattern ended inside the braces. Whatever digits were
                // read are discarded: glibc reports the missing `}` and not
                // the count, so `{2` and `{b` are the same error.
                return (BraceNum::Invalid, BraceStop::End);
            };
            self.bump(1);
            match c.as_ascii() {
                Some('}') => return (num, BraceStop::Close),
                Some(',') => return (num, BraceStop::Comma),
                Some(d @ '0'..='9') => {
                    let v = (d as usize).saturating_sub('0' as usize);
                    num = match num {
                        BraceNum::Absent => BraceNum::Value(v),
                        // Saturating at one above the cap, exactly as glibc's
                        // `MIN (RE_DUP_MAX + 1, …)` does: the digits after it
                        // cannot change the answer, and an unbounded
                        // accumulation would overflow on a long enough run.
                        BraceNum::Value(n) => BraceNum::Value(
                            n.saturating_mul(10)
                                .saturating_add(v)
                                .min(MAX_REPEAT.saturating_add(1)),
                        ),
                        BraceNum::Invalid => BraceNum::Invalid,
                    };
                }
                // A non-digit — including a byte that is not ASCII at all —
                // spoils the number without ending the scan.
                _ => num = BraceNum::Invalid,
            }
        }
    }

    fn parse_atom(&mut self) -> Result<Node, EreError> {
        match self.peek_ascii() {
            Some('(') => {
                self.bump(1);
                self.ngroups = self.ngroups.saturating_add(1);
                let idx = self.ngroups;
                self.depth = self.depth.saturating_add(1);
                let inner = self.parse_alt()?;
                self.depth = self.depth.saturating_sub(1);
                if self.peek_ascii() != Some(')') {
                    return Err(EreError::new(
                        RegCode::UnmatchedParen,
                        b"expected ')' in regex".to_vec(),
                    ));
                }
                self.bump(1);
                Ok(Node::Group(idx, Box::new(inner)))
            }
            Some('[') => self.parse_class(),
            Some('.') => {
                self.bump(1);
                Ok(Node::Any)
            }
            Some('^') => {
                self.bump(1);
                Ok(Node::Start)
            }
            Some('$') => {
                self.bump(1);
                Ok(Node::End)
            }
            Some('\\') => {
                self.bump(1);
                let e = self.peek().ok_or_else(|| {
                    EreError::new(
                        RegCode::TrailingBackslash,
                        b"trailing backslash in regex".to_vec(),
                    )
                })?;
                self.bump(1);
                // `\1`–`\9` is a backreference, not the digit. POSIX puts them
                // in BRE only, but glibc honours them in ERE too and every
                // GNU-era script assumes it; since `bre` translates to this
                // dialect, refusing them here would refuse them everywhere.
                //
                // A reference to a group the pattern has not opened yet is a
                // compile error rather than a never-matching atom, which is what
                // glibc reports too ("Invalid back reference"). The alternative
                // — treating `\7` in a pattern with two groups as the literal
                // `7` — is the silent-wrong-answer shape this crate exists to
                // avoid.
                if let Some(d @ '1'..='9') = e.as_ascii() {
                    let n = (d as usize).saturating_sub('0' as usize);
                    if n > self.ngroups {
                        return Err(EreError::new(
                            RegCode::BadBackReference,
                            cat(&[b"invalid backreference \\", &[d as u8], b" in regex"]),
                        ));
                    }
                    return Ok(Node::Backref(n));
                }
                // The GNU operators. glibc honours these in both dialects —
                // `RE_NO_GNU_OPS` is off for every syntax grep, sed and awk
                // use — so they are handled here, once, rather than in `bre`'s
                // translation. Before this they fell through to the literal arm
                // below, which made `grep -E '\w'` search for a `w`: a silent
                // wrong answer, and the shape this crate exists to avoid.
                if let Some(op) = e.as_ascii() {
                    match op {
                        'w' | 'W' => return Ok(Node::Class(word_class(op == 'W'))),
                        's' | 'S' => return Ok(Node::Class(space_class(op == 'S'))),
                        'b' => return Ok(Node::Word(WordAssert::Boundary)),
                        'B' => return Ok(Node::Word(WordAssert::NotBoundary)),
                        '<' => return Ok(Node::Word(WordAssert::Start)),
                        '>' => return Ok(Node::Word(WordAssert::End)),
                        _ => {}
                    }
                }
                Ok(Node::Lit(unescape(e)))
            }
            // Under POSIX-extended syntax only `parse_quantifier` may consume
            // a `{`, and `parse_repeat` rejects one that reaches an atom slot,
            // so this is unreachable — but a literal brace here would silently
            // resurrect the lenient reading glibc does not have.
            // `UnmatchedBrace` is the right code for it anyway: a `{` in this
            // position had no `}` to reach.
            //
            // Under egrep syntax it is reachable and it *is* a literal: the
            // brace got here because `parse_brace` rolled back, which is
            // `RE_INVALID_INTERVAL_ORD` saying the character was never an
            // interval. Falling through to the literal arm below is the whole
            // of the difference for `grep -E 'a{b}'`.
            Some('{') if !self.syntax.invalid_interval_ord => Err(EreError::new(
                RegCode::UnmatchedBrace,
                b"invalid interval in regex".to_vec(),
            )),
            // Anything that is not one of the ASCII metacharacters above is a
            // literal — including a character that is not ASCII and a byte that
            // decodes to no character at all.
            _ => match self.peek() {
                Some(c) => {
                    self.bump(1);
                    Ok(Node::Lit(c))
                }
                None => Ok(Node::Empty),
            },
        }
    }

    fn parse_class(&mut self) -> Result<Node, EreError> {
        self.bump(1); // consume '['
        let mut negated = false;
        if self.peek_ascii() == Some('^') {
            negated = true;
            self.bump(1);
        }
        let mut ranges: Vec<(Ch, Ch)> = Vec::new();
        let mut posix: Vec<PosixClass> = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(EreError::new(
                    if first {
                        RegCode::BadPattern
                    } else {
                        RegCode::UnmatchedBracket
                    },
                    b"unterminated '[' in regex".to_vec(),
                ));
            };
            // A `]` closes the class, except as the very first member where it
            // is a literal (POSIX rule).
            if c == ']' && !first {
                self.bump(1);
                break;
            }
            first = false;

            // POSIX named class `[:name:]`. A class name is ASCII letters, so
            // the scan below can never stop inside a multi-byte character.
            if c == '[' && self.peek_ascii_at(1) == Some(':') {
                let saved = self.pos;
                self.bump(2); // consume '[:'
                let name_start = self.pos;
                while self.peek_ascii().is_some_and(|ch| ch.is_ascii_alphabetic()) {
                    self.bump(1);
                }
                let name: String = self
                    .chars
                    .get(name_start..self.pos)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|c| c.as_ascii())
                    .collect();
                if self.peek_ascii() == Some(':') && self.peek_ascii_at(1) == Some(']') {
                    self.bump(2); // consume ':]'
                    match PosixClass::from_name(&name) {
                        Some(pc) => {
                            posix.push(pc);
                            continue;
                        }
                        None => {
                            return Err(EreError::new(
                                RegCode::BadCharClass,
                                format!("unknown character class [:{name}:]").into_bytes(),
                            ));
                        }
                    }
                }
                // Not actually a named class — rewind and treat '[' literally.
                self.pos = saved;
            }

            let lo = self.class_char()?;
            // A range `a-z`, but a trailing `-` (before `]`) is a literal.
            if self.peek_ascii() == Some('-')
                && self.peek_ascii_at(1) != Some(']')
                && self.chars.get(self.pos.saturating_add(1)).is_some()
            {
                self.bump(1); // consume '-'
                let hi = self.class_char()?;
                if lo > hi {
                    // Both endpoints are slices of the pattern, so the message
                    // is bytes.
                    return Err(EreError::new(
                        RegCode::BadRangeEnd,
                        cat(&[
                            b"invalid range ",
                            &lo.to_str(),
                            b"-",
                            &hi.to_str(),
                            b" in class",
                        ]),
                    ));
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
    fn class_char(&mut self) -> Result<Ch, EreError> {
        let Some(c) = self.peek() else {
            return Err(EreError::new(
                RegCode::UnmatchedBracket,
                b"unterminated '[' in regex".to_vec(),
            ));
        };
        if c == '\\' {
            self.bump(1);
            let e = self.peek().ok_or_else(|| {
                EreError::new(
                    RegCode::TrailingBackslash,
                    b"trailing backslash in class".to_vec(),
                )
            })?;
            self.bump(1);
            return Ok(unescape(e));
        }
        self.bump(1);
        Ok(c)
    }
}

/// Whether a character begins a quantifier. `{` counts even when the braces
/// turn out to be malformed — that is an error either way, never a literal.
fn is_quantifier_start(c: Option<char>) -> bool {
    matches!(c, Some('*' | '+' | '?' | '{'))
}

/// Wrap `node` in a `{min,max}` repetition.
///
/// `a{0}` is not an error and does not delete anything: it is an expression
/// matching the empty string, which is why `[[ b =~ a{0} ]]` succeeds and
/// `a{0}b` matches "b". An earlier reading — that the atom was deleted and an
/// emptied run was then an error — made `a{0}` and `(a{0})` fail to compile,
/// which neither bash nor findutils does.
fn repeat(node: Node, min: usize, max: Option<usize>) -> Node {
    if max == Some(0) {
        Node::Empty
    } else {
        Node::Repeat {
            node: Box::new(node),
            min,
            max,
        }
    }
}

/// The error for a quantifier with nothing to quantify.
fn nothing_to_repeat() -> EreError {
    EreError::new(RegCode::BadRepeat, b"nothing to repeat in regex".to_vec())
}

/// Map an escaped character to the literal it denotes (`\n` → newline, etc.).
///
/// Only the ASCII escape letters mean anything; every other character —
/// including a byte that decodes to none — denotes itself, which is what makes
/// `\` the way to write a metacharacter literally.
fn unescape(c: Ch) -> Ch {
    match c.as_ascii() {
        Some('n') => Ch::U('\n'),
        Some('t') => Ch::U('\t'),
        Some('r') => Ch::U('\r'),
        Some('f') => Ch::U('\u{0C}'),
        Some('v') => Ch::U('\u{0B}'),
        Some('0') => Ch::U('\0'),
        _ => c,
    }
}

// ---- Compiler ---------------------------------------------------------------

#[derive(Debug, Clone)]
enum Inst {
    Char(Ch),
    Any,
    Class(ClassData),
    Match,
    Jmp(usize),
    Split(usize, usize),
    Save(usize),
    AssertStart,
    AssertEnd,
    /// A word assertion: zero-width, like the two above, and decided from the
    /// characters either side of the position rather than from the position
    /// alone. See [`WordAssert`].
    AssertWord(WordAssert),
    /// Match the text group `n` captured. Consumes as many characters as that
    /// group holds — a width the *program* does not know, which is precisely
    /// why the Pike VM cannot run it.
    Backref(usize),
}

struct Compiler {
    prog: Vec<Inst>,
    /// Whether the program contains a [`Inst::Backref`], and so has to be run
    /// by the backtracker rather than the Pike VM.
    has_backref: bool,
    /// Set once the program has passed [`MAX_PROG`]. Every loop that can
    /// multiply — the repetition expansions — checks it and unwinds, so a
    /// pattern asking for 10⁹ instructions costs the cap plus one iteration per
    /// nesting level rather than 10⁹ no-ops. `new_flags` turns it into an error.
    over: bool,
}

impl Compiler {
    fn emit(&mut self, i: Inst) -> usize {
        // The index of what was just pushed. Written before the push so it is
        // an addition on a length rather than a subtraction that would be wrong
        // (and would underflow) if the push had not happened.
        let at = self.prog.len();
        if at >= MAX_PROG {
            self.over = true;
            return at;
        }
        self.prog.push(i);
        at
    }

    fn compile(&mut self, node: &Node) {
        if self.over {
            return;
        }
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
            Node::Word(w) => {
                self.emit(Inst::AssertWord(*w));
            }
            Node::Group(idx, inner) => {
                // The two slots of group `idx`. `idx` counts opening parens in
                // the pattern, so `2·idx + 1` is bounded by the pattern's own
                // length and cannot overflow; `saturating` says so without
                // asking the reader to reconstruct the argument.
                let slot = idx.saturating_mul(2);
                self.emit(Inst::Save(slot));
                self.compile(inner);
                self.emit(Inst::Save(slot.saturating_add(1)));
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
                        self.patch(split, Inst::Split(l1, l2));
                    } else {
                        self.compile(b);
                    }
                }
                let end = self.prog.len();
                for j in jmp_ends {
                    self.patch(j, Inst::Jmp(end));
                }
            }
            Node::Backref(n) => {
                self.has_backref = true;
                self.emit(Inst::Backref(*n));
            }
            Node::Repeat { node, min, max } => self.compile_repeat(node, *min, *max),
        }
    }

    /// Fill in a forward branch whose target was not known when it was emitted.
    ///
    /// A `get_mut` rather than an index because [`Self::emit`] stops emitting
    /// once the program passes [`MAX_PROG`] and hands back a slot that was never
    /// written: the compilation is being abandoned, and abandoning it must not
    /// take the process with it.
    fn patch(&mut self, at: usize, i: Inst) {
        if let Some(slot) = self.prog.get_mut(at) {
            *slot = i;
        }
    }

    fn compile_repeat(&mut self, node: &Node, min: usize, max: Option<usize>) {
        // Mandatory copies. The `over` check belongs *in* the loop, not only at
        // the top of `compile`: these loops nest and multiply, so a body that
        // merely returned early would still be entered 10⁹ times for
        // `((a{1000}){1000}){1000}`. Breaking here costs one iteration per
        // nesting level instead.
        for _ in 0..min {
            if self.over {
                return;
            }
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
                self.patch(l, Inst::Split(body, out));
            }
            Some(max) => {
                // `max - min` greedy optional copies, each able to jump to `out`.
                let extra = max.saturating_sub(min);
                // Not `with_capacity(extra)`: `extra` is attacker-chosen up to
                // `MAX_REPEAT`, and the loop below may stop far short of it.
                let mut splits: Vec<usize> = Vec::new();
                for _ in 0..extra {
                    if self.over {
                        break;
                    }
                    let s = self.emit(Inst::Split(0, 0));
                    splits.push(s);
                    let body = self.prog.len();
                    self.compile(node);
                    self.patch(s, Inst::Split(body, 0)); // second target patched below
                }
                let out = self.prog.len();
                for s in splits {
                    if let Some(&Inst::Split(a, _)) = self.prog.get(s) {
                        self.patch(s, Inst::Split(a, out));
                    }
                }
            }
        }
    }
}

// ---- Compiled regex + Pike VM ----------------------------------------------

/// Compare an optional input character against a literal `Char` instruction,
/// folding case when `ci` is set. Case folding tries both the upper- and
/// lower-case mappings so any Unicode letter pair (not just ASCII) is handled.
fn char_eq(input: Option<Ch>, lit: Ch, ci: bool) -> bool {
    match input {
        Some(ch) if ch == lit => true,
        Some(ch) if ci => char_fold_eq(ch, lit),
        _ => false,
    }
}

/// Unicode-aware case-fold equality: two characters are equal if their lowercase
/// (or uppercase) mappings match. Covers non-ASCII letters that an ASCII-only
/// fold misses.
///
/// A byte that decodes to no character has no case — [`Ch::to_lowercase`] maps
/// it to itself — so two *different* undecodable bytes never fold together, and
/// none of them ever folds into a letter.
fn char_fold_eq(a: Ch, b: Ch) -> bool {
    a.to_ascii_lowercase() == b.to_ascii_lowercase()
        || a.to_lowercase() == b.to_lowercase()
        || a.to_uppercase() == b.to_uppercase()
}

/// A compiled ERE. Compile once with [`Regex::new`], then match repeatedly.
pub struct Regex {
    prog: Vec<Inst>,
    ngroups: usize,
    /// The first instruction of the pattern proper — past the unanchored search
    /// prefix. Entering here matches only at the position the search is seeded
    /// at, which is what the longest-match pass in [`Regex::run`] needs. Stored
    /// rather than assumed to be a constant so that changing the prefix cannot
    /// silently leave the second pass entering the wrong instruction.
    entry: usize,
    /// Case-insensitive matching (`shopt -s nocasematch`). When set, `Char`
    /// and `Class` instructions match without regard to letter case.
    ci: bool,
    /// Whether the program contains a backreference, which decides *which*
    /// matcher runs: the linear Pike VM for everything else, the budgeted
    /// backtracker for this. See [`Regex::backtrack_at`].
    has_backref: bool,
}

/// A compiled regex prints as its shape, not its program.
///
/// It exists so a caller can `#[derive(Debug)]` a structure that holds one —
/// awk's syntax tree does, and a tree that cannot be printed cannot be debugged.
/// The pattern text is not kept (nothing else needs it, and keeping a copy per
/// regex to serve `{:?}` would be paying for the debugger in production), and
/// dumping two hundred instructions where the reader expected `/^a.*b$/` would
/// bury the rest of the tree; the counts are enough to tell two regexes apart.
impl core::fmt::Debug for Regex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Regex")
            .field("insts", &self.prog.len())
            .field("groups", &self.ngroups)
            .field("ci", &self.ci)
            .finish_non_exhaustive()
    }
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

/// One point the backreference backtracker may return to.
///
/// It carries its own captures because a branch must not see what an abandoned
/// sibling wrote — which is the difference between a backtracker and the Pike
/// VM, where captures are threaded through a frontier instead.
struct BtFrame {
    pc: usize,
    sp: usize,
    caps: Vec<Option<usize>>,
    /// Back-edge targets already taken at [`Self::back_edges_sp`] on this path,
    /// which is what stops an empty loop body from looping for ever. Only the
    /// current input position is kept: a back edge taken at an earlier position
    /// cannot repeat without the path returning there, and returning is a
    /// different frame.
    back_edges: Vec<usize>,
    back_edges_sp: usize,
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
    pub fn new(pattern: BStr<'_>) -> Result<Regex, EreError> {
        Self::new_flags(pattern, false)
    }

    /// Compile an ERE pattern with optional case-insensitive matching.
    ///
    /// # Errors
    /// Returns [`EreError`] on a syntax error, as [`Regex::new`].
    pub fn new_flags(pattern: BStr<'_>, ci: bool) -> Result<Regex, EreError> {
        Self::new_syntax(pattern, ci, Syntax::POSIX_EXTENDED)
    }

    /// Compile an ERE pattern in a chosen dialect.
    ///
    /// Only `grep -E` wants anything but [`Syntax::POSIX_EXTENDED`]; see
    /// [`Syntax`] for the measured table of what differs and why one engine
    /// cannot serve both callers with one answer.
    ///
    /// # Errors
    /// Returns [`EreError`] on a syntax error, as [`Regex::new`] — though
    /// which patterns are syntax errors is part of what `syntax` selects.
    pub fn new_syntax(pattern: BStr<'_>, ci: bool, syntax: Syntax) -> Result<Regex, EreError> {
        let mut parser = EParser {
            chars: bytes::chars(pattern).collect(),
            pos: 0,
            ngroups: 0,
            depth: 0,
            syntax,
        };
        let ast = parser.parse()?;
        let ngroups = parser.ngroups;

        let mut c = Compiler {
            prog: Vec::new(),
            has_backref: false,
            over: false,
        };
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
        c.patch(split, Inst::Split(real, skip));
        c.emit(Inst::Save(0));
        c.compile(&ast);
        c.emit(Inst::Save(1));
        c.emit(Inst::Match);
        if c.over {
            // Reported as a compile error rather than a truncated program: a
            // program that stopped part-way would match the wrong language, and
            // silently answering a different question is worse than refusing.
            return Err(EreError::new(RegCode::TooBig, b"regex too large".to_vec()));
        }

        Ok(Regex {
            prog: c.prog,
            ngroups,
            entry: real,
            ci,
            has_backref: c.has_backref,
        })
    }

    /// Number of capturing groups (excluding the whole-match group 0).
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.ngroups
    }

    /// Number of instructions in the compiled program.
    ///
    /// Exposed for callers that decide how much work to allow: a search costs
    /// `O(len(subject) × len(prog))`, and a program near [`MAX_PROG`] on a
    /// large file is the one shape that is slow without being wrong.
    #[must_use]
    pub fn program_len(&self) -> usize {
        self.prog.len()
    }

    /// Whether the pattern contains a backreference (`\1`–`\9`).
    ///
    /// Such a pattern is matched by a budgeted backtracker rather than the Pike
    /// VM, so it — and only it — can answer [`MatchLimit`]. Exposed so a caller
    /// that wants the linear guarantee can insist on it, and so a test can
    /// assert which matcher a pattern reached.
    #[must_use]
    pub fn has_backref(&self) -> bool {
        self.has_backref
    }

    /// `true` if the pattern matches anywhere in `text`.
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search exceeded its budget. A pattern
    /// without a backreference cannot return one.
    pub fn is_match(&self, text: BStr<'_>) -> Result<bool, MatchLimit> {
        Ok(self.captures(text)?.is_some())
    }

    /// Find the leftmost match and return the captured substrings: index `0` is
    /// the whole match, `i` is capture group `i` (`None` if the group did not
    /// participate). `Ok(None)` if the pattern does not match.
    ///
    /// The capture slots are *character* offsets, so a group's bytes are
    /// reassembled from the decoded characters rather than sliced out of `text`
    /// — which keeps a group boundary from ever landing inside a character.
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search exceeded its budget.
    pub fn captures(&self, text: BStr<'_>) -> Result<Option<Vec<Option<Str>>>, MatchLimit> {
        let chars: Vec<Ch> = bytes::chars(text).collect();
        let Some(slots) = self.run(&chars, 0)? else {
            return Ok(None);
        };
        let mut out = Vec::with_capacity(self.ngroups.saturating_add(1));
        for g in 0..=self.ngroups {
            // The open and close slots of group `g`. A group is an opening paren
            // in the pattern, so `2·g + 1` is bounded by the pattern's length;
            // the slots are read with `get`, so even a saturated index is a
            // missing capture rather than a panic.
            let (open, close) = (g.saturating_mul(2), g.saturating_mul(2).saturating_add(1));
            match (
                slots.get(open).copied().flatten(),
                slots.get(close).copied().flatten(),
            ) {
                (Some(s), Some(e)) if s <= e && e <= chars.len() => {
                    let span = chars.get(s..e).unwrap_or_default();
                    out.push(Some(bytes::from_chars(span.iter().copied())));
                }
                _ => out.push(None),
            }
        }
        Ok(Some(out))
    }

    /// Where the leftmost match begins and ends, as **byte** offsets into
    /// `text`, or `Ok(None)` if the pattern does not match.
    ///
    /// [`Regex::captures`] hands back the matched *bytes*, which answers "what
    /// did it match" but not "where" — and `grep -o`, `sed`'s `s///` and awk's
    /// `sub`/`gsub`/`match` all need the position, because they have to rebuild
    /// the subject around the match.
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search exceeded its budget.
    pub fn find(&self, text: BStr<'_>) -> Result<Option<(usize, usize)>, MatchLimit> {
        self.find_at(text, 0)
    }

    /// The leftmost match at or after byte offset `from`.
    ///
    /// `^` still means the start of `text`, not the start of the search — a
    /// continued search is looking for the *next* match in one subject, not
    /// matching a new subject that happens to begin at `from`. (POSIX spells
    /// this `REG_NOTBOL`.) So `sed 's/^a//g'` removes one leading `a` and not
    /// one per position, which is what every other implementation does.
    ///
    /// `from` is rounded forward to a character boundary, so a caller that
    /// resumes from an arbitrary byte cannot start a match inside a character.
    ///
    /// Prefer [`Regex::find_iter`] for a scan: this decodes `text` on every
    /// call, so stepping through a long subject with it is quadratic.
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search exceeded its budget.
    pub fn find_at(
        &self,
        text: BStr<'_>,
        from: usize,
    ) -> Result<Option<(usize, usize)>, MatchLimit> {
        let scan = Scan::new(text);
        let Some(slots) = self.run(&scan.chars, scan.char_index(from))? else {
            return Ok(None);
        };
        let (Some(s), Some(e)) = (
            slots.first().copied().flatten(),
            slots.get(1).copied().flatten(),
        ) else {
            return Ok(None);
        };
        Ok(scan.span(s, e))
    }

    /// Every non-overlapping match, left to right, as byte offsets.
    ///
    /// Decodes `text` once and reuses it, which is the difference between a
    /// linear `gsub` and a quadratic one. An *empty* match advances the scan by
    /// one character rather than staying put, so a pattern that can match
    /// nothing — `x*`, `()` — yields a match at each position and terminates.
    pub fn find_iter(&self, text: BStr<'_>) -> Matches<'_> {
        Matches {
            re: self,
            cur: Cursor::new(text),
        }
    }

    /// Every non-overlapping match's capture groups, left to right, as byte
    /// spans — [`Regex::find_iter`] for a caller that needs the groups.
    ///
    /// `sed`'s `s/\(a\)\(b\)/\2\1/g` needs both halves of this at once: where
    /// each match sits in the subject, and where its groups sit inside it.
    /// Getting them by calling [`Regex::capture_spans_at`] in a loop re-decodes
    /// the subject for every match, which turns a linear substitution into a
    /// quadratic one on exactly the files where it matters.
    pub fn capture_spans_iter(&self, text: BStr<'_>) -> CaptureMatches<'_> {
        CaptureMatches {
            re: self,
            cur: Cursor::new(text),
        }
    }

    /// The leftmost match's capture groups as **byte** spans: index `0` is the
    /// whole match, `i` is group `i`, `None` for a group that did not
    /// participate.
    ///
    /// This is what a replacement text needs. `sed`'s `s/\(a*\)b/[\1]/` has to
    /// splice group 1 into the output *and* know which bytes of the subject the
    /// whole match consumed; [`Regex::captures`] gives the first and not the
    /// second.
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search exceeded its budget.
    pub fn capture_spans(&self, text: BStr<'_>) -> Result<Option<GroupSpans>, MatchLimit> {
        self.capture_spans_at(text, 0)
    }

    /// [`Regex::capture_spans`], resumed at byte offset `from`. `^` keeps
    /// meaning the start of `text` — see [`Regex::find_at`].
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search exceeded its budget.
    pub fn capture_spans_at(
        &self,
        text: BStr<'_>,
        from: usize,
    ) -> Result<Option<GroupSpans>, MatchLimit> {
        let scan = Scan::new(text);
        let Some(slots) = self.run(&scan.chars, scan.char_index(from))? else {
            return Ok(None);
        };
        Ok(Some(self.spans_from_slots(&scan, &slots)))
    }

    /// Turn a winning thread's character slots into byte spans, one per group.
    fn spans_from_slots(&self, scan: &Scan, slots: &[Option<usize>]) -> GroupSpans {
        let mut out = Vec::with_capacity(self.ngroups.saturating_add(1));
        for g in 0..=self.ngroups {
            let (open, close) = (g.saturating_mul(2), g.saturating_mul(2).saturating_add(1));
            let span = match (
                slots.get(open).copied().flatten(),
                slots.get(close).copied().flatten(),
            ) {
                (Some(s), Some(e)) => scan.span(s, e),
                _ => None,
            };
            out.push(span);
        }
        out
    }

    /// The leftmost-longest match at or after character index `start`.
    ///
    /// POSIX requires the **longest** match among those that start leftmost,
    /// and that is not what a priority-ordered Pike VM gives you: thread
    /// priority answers `a|ab` against `ab` with `a`, because the first
    /// alternative is tried first and wins as soon as it reaches `Match`. Perl
    /// and the `regex` crate are defined that way; `grep`, `sed` and `awk` are
    /// not, and a `grep -o 'a\|ab'` that printed `a` would be quietly wrong in
    /// a way no test of ours had ever asked about.
    ///
    /// So the search is two passes over one program:
    ///
    /// 1. an unanchored, priority-ordered pass, stopped at the first `Match` —
    ///    which is the *leftmost* start, since a thread that entered the
    ///    pattern earlier outranks one that skipped further first;
    /// 2. an anchored pass from that start which does *not* stop at `Match`,
    ///    keeping the last one reached — the longest end.
    ///
    /// The second pass usually costs almost nothing: its threads are seeded at
    /// one position, so it stops the moment they all die, which for most
    /// patterns is a few characters in.
    ///
    /// Group captures within the chosen match stay priority-ordered (greedy),
    /// which is where this stops short of full POSIX submatch rules — those
    /// require longest-first at every level of nesting. GNU's engines do not
    /// implement them either, and the utilities in this tree do not ask.
    ///
    /// A pattern with a backreference takes the other matcher entirely; see
    /// [`Regex::backtrack_at`] for why it cannot take this one.
    ///
    /// # Errors
    /// [`MatchLimit`] if a backreference search ran out of budget. A pattern
    /// without a backreference never returns one.
    fn run(&self, input: &[Ch], start: usize) -> Result<Option<Vec<Option<usize>>>, MatchLimit> {
        if self.has_backref {
            return self.run_backtrack(input, start);
        }
        let Some(first) = self.scan(input, start, 0, false) else {
            return Ok(None);
        };
        let Some(at) = first.first().copied().flatten() else {
            return Ok(None);
        };
        // `or(first)` is unreachable — the anchored pass repeats a match that
        // was just found at exactly that position — and is written out rather
        // than unwrapped so a future change to the prefix cannot turn a shorter
        // answer into no answer at all.
        Ok(self.scan(input, at, self.entry, true).or(Some(first)))
    }

    /// How many steps one search of `input` may take before it is abandoned.
    ///
    /// Saturating throughout: the product is attacker-influenced through the
    /// subject's length, and a budget that wrapped to a small number would turn
    /// a large file into a spurious [`MatchLimit`].
    fn backtrack_budget(input_len: usize) -> u64 {
        let per_char = u64::try_from(input_len)
            .unwrap_or(u64::MAX)
            .saturating_mul(BACKTRACK_BUDGET_PER_CHAR);
        BACKTRACK_BUDGET_BASE
            .saturating_add(per_char)
            .min(BACKTRACK_BUDGET_MAX)
    }

    /// The leftmost-longest match at or after `start`, for a pattern that has a
    /// backreference.
    ///
    /// Leftmost is obtained by trying each start position in turn rather than by
    /// the compiled search prefix: the backtracker enters at [`Regex::entry`],
    /// so every attempt is anchored and the first position that matches at all
    /// is the leftmost one. That also keeps the budget meaningful — an
    /// unanchored program would let one hopeless start position spend the whole
    /// allowance on behalf of the ones after it.
    ///
    /// # Errors
    /// [`MatchLimit`] when the budget runs out. The budget is shared across all
    /// the start positions of one search, so the cost of a call is bounded, not
    /// just the cost of each attempt within it.
    fn run_backtrack(
        &self,
        input: &[Ch],
        start: usize,
    ) -> Result<Option<Vec<Option<usize>>>, MatchLimit> {
        let mut budget = Self::backtrack_budget(input.len());
        let start = start.min(input.len());
        for at in start..=input.len() {
            if let Some(caps) = self.backtrack_at(input, at, &mut budget)? {
                return Ok(Some(caps));
            }
        }
        Ok(None)
    }

    /// The longest match *beginning exactly at* `start`, by backtracking.
    ///
    /// ## Why this exists at all
    ///
    /// The Pike VM advances every alternative of the pattern through the subject
    /// together, one character per step. That is what makes it immune to
    /// catastrophic backtracking, and it is also what makes a backreference
    /// impossible: `\1` consumes as much text as group 1 happened to capture,
    /// which differs between the alternatives that are all live at once, so
    /// there is no single "the" capture to compare against and no single width
    /// to advance by. The construct is not missing from that engine; it is
    /// outside what that engine *is*.
    ///
    /// So patterns with a backreference — and only those — run here instead,
    /// which is what glibc does too. Every other pattern keeps the linear
    /// guarantee untouched, because the choice is made once per pattern at
    /// compile time, not per subject line.
    ///
    /// ## Shape
    ///
    /// An explicit stack of frames rather than recursion: the recursion depth of
    /// a backtracking matcher grows with the number of repetitions matched, so
    /// `\(a\)\1a*` on a megabyte of `a` would be a stack overflow — which is a
    /// crash in five programs, not a wrong answer in one.
    ///
    /// A frame is a point the search may return to. `Split` pushes its
    /// lower-priority branch and continues down the higher-priority one, so the
    /// stack is explored in exactly the order a recursive matcher would take,
    /// and captures are per-frame so a branch cannot see what an abandoned one
    /// wrote.
    ///
    /// The pass does not stop at the first `Match`: POSIX wants the longest
    /// match at the leftmost start, and the first one a backtracker reaches is
    /// the *highest-priority* one, which for `a|ab` is the shorter. It stops
    /// early only when a match reaches the end of the subject, where no longer
    /// one can exist.
    ///
    /// ## Termination
    ///
    /// Two things stop it running for ever. An empty loop — `\(\)*`, or any body
    /// that can match nothing — is caught by refusing to take a backward jump
    /// twice at the same input position on the same path, which is the same
    /// answer Perl gives (an iteration that consumed nothing ends the loop). And
    /// the budget bounds the genuinely exponential patterns, which no structural
    /// rule can.
    fn backtrack_at(
        &self,
        input: &[Ch],
        start: usize,
        budget: &mut u64,
    ) -> Result<Option<Vec<Option<usize>>>, MatchLimit> {
        let nslots = self.ngroups.saturating_add(1).saturating_mul(2);
        let mut best: Option<Vec<Option<usize>>> = None;
        let mut best_end: Option<usize> = None;
        let mut stack: Vec<BtFrame> = vec![BtFrame {
            pc: self.entry,
            sp: start,
            caps: vec![None; nslots],
            back_edges: Vec::new(),
            back_edges_sp: start,
        }];

        while let Some(mut f) = stack.pop() {
            // One path, followed until it fails, loops, or matches.
            loop {
                *budget = match budget.checked_sub(1) {
                    Some(left) => left,
                    None => return Err(MatchLimit),
                };
                // A `pc` naming no instruction can only be a compiler bug; the
                // path dying is the same containment the Pike VM applies to it.
                let Some(inst) = self.prog.get(f.pc) else {
                    break;
                };
                // The next instruction and the next input position. Neither can
                // overflow in a program that compiled: `Match` is always last,
                // and `sp` is an index into `input`.
                let next_pc = f.pc.saturating_add(1);
                match inst {
                    Inst::Char(ch) if char_eq(input.get(f.sp).copied(), *ch, self.ci) => {
                        f.pc = next_pc;
                        f.sp = f.sp.saturating_add(1);
                    }
                    Inst::Any if f.sp < input.len() => {
                        f.pc = next_pc;
                        f.sp = f.sp.saturating_add(1);
                    }
                    Inst::Class(d)
                        if input.get(f.sp).is_some_and(|c| d.matches_ci(*c, self.ci)) =>
                    {
                        f.pc = next_pc;
                        f.sp = f.sp.saturating_add(1);
                    }
                    Inst::Backref(n) => match self.backref_step(input, &f, *n, budget)? {
                        Some(sp) => {
                            f.pc = next_pc;
                            f.sp = sp;
                        }
                        None => break,
                    },
                    Inst::Save(n) => {
                        if let Some(slot) = f.caps.get_mut(*n) {
                            *slot = Some(f.sp);
                        }
                        f.pc = next_pc;
                    }
                    Inst::Jmp(x) => {
                        // A backward jump is a loop's back edge. Taking it twice
                        // without having consumed anything means the body
                        // matched empty, and taking it a third time would do the
                        // same for ever.
                        if *x <= f.pc {
                            if f.back_edges_sp != f.sp {
                                f.back_edges.clear();
                                f.back_edges_sp = f.sp;
                            }
                            if f.back_edges.contains(x) {
                                break;
                            }
                            f.back_edges.push(*x);
                        }
                        f.pc = *x;
                    }
                    Inst::Split(x, y) => {
                        stack.push(BtFrame {
                            pc: *y,
                            sp: f.sp,
                            caps: f.caps.clone(),
                            back_edges: f.back_edges.clone(),
                            back_edges_sp: f.back_edges_sp,
                        });
                        f.pc = *x;
                    }
                    Inst::AssertStart if f.sp == 0 => f.pc = next_pc,
                    Inst::AssertEnd if f.sp == input.len() => f.pc = next_pc,
                    Inst::AssertWord(w) if w.holds_at(input, f.sp) => f.pc = next_pc,
                    Inst::Match => {
                        if best_end.is_none_or(|e| f.sp > e) {
                            best = Some(f.caps.clone());
                            best_end = Some(f.sp);
                        }
                        // A match reaching the end of the subject cannot be
                        // beaten, so the remaining alternatives are work with no
                        // possible answer.
                        if f.sp == input.len() {
                            return Ok(best);
                        }
                        break;
                    }
                    // Every remaining case is a guard above that did not hold —
                    // a character that did not match, an anchor that did not
                    // hold — and ends this path.
                    _ => break,
                }
            }
        }
        Ok(best)
    }

    /// Match `\n` at the frame's position: the input position after it, or
    /// `None` if it does not match.
    ///
    /// A group that has not participated — `\(a\)\?b\1` where the `\(a\)` was
    /// skipped — makes the backreference fail rather than match the empty
    /// string. POSIX leaves it undefined and glibc, Perl and PCRE all fail it;
    /// matching empty would silently turn `\(x\)\?y\1` into "y optionally
    /// followed by x", which is not what anyone writing it meant.
    ///
    /// The comparison is charged to the budget by its length, so a pattern that
    /// re-compares a long capture many times pays for it. Charging one step per
    /// instruction alone would make `\(.*\)\1\1\1…` almost free to the budget
    /// and expensive to the machine, which is the wrong way round.
    fn backref_step(
        &self,
        input: &[Ch],
        f: &BtFrame,
        n: usize,
        budget: &mut u64,
    ) -> Result<Option<usize>, MatchLimit> {
        let (open, close) = (n.saturating_mul(2), n.saturating_mul(2).saturating_add(1));
        let (Some(s), Some(e)) = (
            f.caps.get(open).copied().flatten(),
            f.caps.get(close).copied().flatten(),
        ) else {
            return Ok(None);
        };
        let Some(len) = e.checked_sub(s) else {
            return Ok(None);
        };
        *budget = match budget.checked_sub(u64::try_from(len).unwrap_or(u64::MAX)) {
            Some(left) => left,
            None => return Err(MatchLimit),
        };
        let (Some(want), Some(have)) = (input.get(s..e), input.get(f.sp..f.sp.saturating_add(len)))
        else {
            return Ok(None);
        };
        if want
            .iter()
            .zip(have)
            .all(|(&w, &h)| char_eq(Some(h), w, self.ci))
        {
            Ok(Some(f.sp.saturating_add(len)))
        } else {
            Ok(None)
        }
    }

    /// One pass of the Pike VM: threads seeded at `seed_pc` and character index
    /// `start`, returning the winning thread's capture slots (`2 × (ngroups +
    /// 1)` positions). With `longest`, a thread reaching `Match` records its
    /// slots and the pass continues, so a later — longer — match supersedes it;
    /// without it the pass stops at the first, highest-priority `Match`.
    fn scan(
        &self,
        input: &[Ch],
        start: usize,
        seed_pc: usize,
        longest: bool,
    ) -> Option<Vec<Option<usize>>> {
        // Two slots — open and close — for every group plus the whole match.
        let nslots = self.ngroups.saturating_add(1).saturating_mul(2);
        let mut clist = ThreadList::new(self.prog.len());
        let mut nlist = ThreadList::new(self.prog.len());
        let mut matched: Option<Vec<Option<usize>>> = None;
        // Where the recorded match ends, so that among the threads reaching
        // `Match` at one position the first — the highest-priority one — is the
        // one kept, while a `Match` at a later position still supersedes it.
        let mut matched_at: Option<usize> = None;

        let mut caps = vec![None; nslots];
        // `start` past the end is not an error — it is a scan that has run off
        // the subject, which `find_iter` does on its last step.
        let start = start.min(input.len());
        self.add_thread(&mut clist, seed_pc, start, &mut caps, input);

        for sp in start..=input.len() {
            if clist.threads.is_empty() {
                break;
            }
            let c = input.get(sp).copied();
            nlist.clear();
            let mut i = 0;
            // Indexed rather than iterated because the threads are consulted in
            // priority order and the loop may stop early at `Match`; `get`
            // rather than `[]` so the bound is a condition and not a panic.
            while let Some(th) = clist.threads.get(i) {
                let pc = th.pc;
                // A `pc` past the end can only mean a compiler bug — every
                // branch target is patched to a real instruction, and a program
                // that ran out of budget is refused rather than returned. If one
                // ever appeared, the thread dying is a wrong answer; the process
                // dying is a wrong answer *and* an outage in five programs.
                let Some(inst) = self.prog.get(pc) else {
                    i = i.saturating_add(1);
                    continue;
                };
                // The successor of a consuming instruction. Neither can
                // overflow: `Char`/`Any`/`Class` are never the last instruction
                // (`Match` is), and `sp` is an index into `input`.
                let next = (pc.saturating_add(1), sp.saturating_add(1));
                match inst {
                    Inst::Char(ch) if char_eq(c, *ch, self.ci) => {
                        let mut caps = th.caps.clone();
                        self.add_thread(&mut nlist, next.0, next.1, &mut caps, input);
                    }
                    Inst::Any if c.is_some() => {
                        let mut caps = th.caps.clone();
                        self.add_thread(&mut nlist, next.0, next.1, &mut caps, input);
                    }
                    Inst::Class(d) if c.is_some_and(|ch| d.matches_ci(ch, self.ci)) => {
                        let mut caps = th.caps.clone();
                        self.add_thread(&mut nlist, next.0, next.1, &mut caps, input);
                    }
                    Inst::Match => {
                        if matched_at.is_none_or(|at| sp > at) {
                            matched = Some(th.caps.clone());
                            matched_at = Some(sp);
                        }
                        if !longest {
                            // Highest-priority thread to reach Match wins; cut
                            // the remaining (lower-priority) threads here.
                            break;
                        }
                        // Under `longest` the pass runs on: a lower-priority
                        // thread at this step may still be consuming, and the
                        // match it reaches later is the longer one.
                    }
                    // Epsilon instructions are expanded by `add_thread`.
                    //
                    // `Backref` also lands here, and the thread dies. That is
                    // unreachable — `run` sends a program containing one to the
                    // backtracker instead, which is the whole reason
                    // `has_backref` is recorded at compile time — and dying is
                    // the containment to have if the dispatch is ever broken: a
                    // pattern that stops matching is a visible bug, whereas a
                    // `Backref` treated as "matches anything" would silently
                    // widen what a `sed` script edits.
                    _ => {}
                }
                i = i.saturating_add(1);
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
        input: &[Ch],
    ) {
        // `seen` is sized to the program, so a `pc` it cannot index is one no
        // instruction names — a compiler bug rather than an input. Declining to
        // add the thread is the same answer as its dying immediately, and is
        // reached by no pattern; see the matching note in [`Self::run`].
        let Some(seen) = list.seen.get_mut(pc) else {
            return;
        };
        if *seen {
            return;
        }
        *seen = true;
        let Some(inst) = self.prog.get(pc) else {
            return;
        };
        // The instruction after this one. Every epsilon instruction below is
        // followed by at least a `Match`, so this is always a real address; a
        // saturated one would be rejected by the `get` at the top of the
        // recursive call rather than indexing out of range.
        let next = pc.saturating_add(1);
        match inst {
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
                self.add_thread(list, next, sp, caps, input);
                if let Some(slot) = caps.get_mut(n) {
                    *slot = old;
                }
            }
            Inst::AssertStart => {
                if sp == 0 {
                    self.add_thread(list, next, sp, caps, input);
                }
            }
            Inst::AssertEnd => {
                if sp == input.len() {
                    self.add_thread(list, next, sp, caps, input);
                }
            }
            Inst::AssertWord(w) => {
                if w.holds_at(input, sp) {
                    self.add_thread(list, next, sp, caps, input);
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

/// A subject decoded once, with the map back to its bytes.
///
/// The engine counts in *characters* — that is what makes `.` match an
/// undecodable byte as one thing rather than as however many bytes it spans —
/// but every caller of this crate counts in bytes, because that is what it
/// will slice the subject with. This is the translation, and it is built once
/// per scan rather than once per match.
struct Scan {
    chars: Vec<Ch>,
    /// Byte offset of each character, then the subject's length. Holding that
    /// extra final entry is what makes `offs[s]..offs[e]` right for *every*
    /// `s <= e <= chars.len()`, including a match that ends at the end.
    offs: Vec<usize>,
}

impl Scan {
    fn new(text: BStr<'_>) -> Scan {
        let mut chars = Vec::new();
        let mut offs = Vec::new();
        for (at, c) in bytes::char_positions(text) {
            offs.push(at);
            chars.push(c);
        }
        offs.push(text.len());
        Scan { chars, offs }
    }

    /// The character index at or after byte offset `at`.
    ///
    /// Rounding *forward* is what keeps a resumed search from starting inside a
    /// character: a caller that computed `at` by adding a byte count can land
    /// mid-character, and the alternative — rounding back — would let the scan
    /// re-match text it had already consumed and loop.
    fn char_index(&self, at: usize) -> usize {
        self.offs.partition_point(|&o| o < at)
    }

    /// The bytes a character span covers, or `None` if either end is not a
    /// character boundary this subject has.
    fn span(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        Some((*self.offs.get(start)?, *self.offs.get(end)?))
    }
}

/// Where a scan of one subject has got to. Shared by the two iterators so they
/// cannot disagree about what "the next match" means.
struct Cursor {
    scan: Scan,
    /// Character index the next search starts from.
    next: usize,
    /// Where the previous match ended, so an empty match butted against it can
    /// be recognised and dropped.
    last_end: Option<usize>,
    done: bool,
}

impl Cursor {
    fn new(text: BStr<'_>) -> Cursor {
        Cursor {
            scan: Scan::new(text),
            next: 0,
            last_end: None,
            done: false,
        }
    }

    /// Advance to the next match and return its capture slots.
    ///
    /// `None` ends the scan; `Some(Err(_))` ends it too, but says the scan was
    /// abandoned rather than finished — a distinction the caller has to keep,
    /// because a truncated `gsub` that reported success would silently write
    /// half a substitution.
    fn step(&mut self, re: &Regex) -> Option<Result<Vec<Option<usize>>, MatchLimit>> {
        loop {
            if self.done {
                return None;
            }
            let slots = match re.run(&self.scan.chars, self.next) {
                Ok(Some(slots)) => slots,
                Ok(None) => {
                    self.done = true;
                    return None;
                }
                Err(limit) => {
                    self.done = true;
                    return Some(Err(limit));
                }
            };
            let (start, end) = match (
                slots.first().copied().flatten(),
                slots.get(1).copied().flatten(),
            ) {
                (Some(s), Some(e)) => (s, e),
                // A match that did not record its own extent cannot be stepped
                // past, so continuing would return it for ever. It is
                // unreachable — every program is wrapped in the
                // `Save(0) … Save(1)` pair — and ending the scan is the one
                // answer that cannot hang the caller.
                _ => {
                    self.done = true;
                    return None;
                }
            };
            if end > start {
                self.next = end;
                self.last_end = Some(end);
                if self.next > self.scan.chars.len() {
                    self.done = true;
                }
                return Some(Ok(slots));
            }
            // An empty match is at a position, not over one, so it would be
            // found again at the same place. Stepping one character past it is
            // what `sed 's/x*/-/g'` does: a replacement between every pair of
            // characters, and then an end.
            //
            // But an empty match *touching the end of the previous match* is
            // not a second place; it is the same place, reachable because the
            // pattern can also match nothing. `s/a*/-/g` on `aaa` is `-`, not
            // `--`, and `grep -o` agrees. Dropping it here rather than in each
            // caller is what keeps them agreeing.
            let butts_previous = self.last_end == Some(start);
            self.next = start.saturating_add(1);
            if self.next > self.scan.chars.len() {
                self.done = true;
            }
            if butts_previous {
                continue;
            }
            self.last_end = Some(end);
            return Some(Ok(slots));
        }
    }
}

/// Every non-overlapping match of one pattern in one subject, as byte spans.
/// Built by [`Regex::find_iter`].
pub struct Matches<'r> {
    re: &'r Regex,
    cur: Cursor,
}

/// The item is a `Result` because a backreference search can be abandoned
/// part-way through a scan. Ending the iteration silently would leave a `gsub`
/// looking finished when it had only got as far as the budget allowed.
impl Iterator for Matches<'_> {
    type Item = Result<(usize, usize), MatchLimit>;

    fn next(&mut self) -> Option<Self::Item> {
        let slots = match self.cur.step(self.re)? {
            Ok(slots) => slots,
            Err(limit) => return Some(Err(limit)),
        };
        let start = slots.first().copied().flatten()?;
        let end = slots.get(1).copied().flatten()?;
        self.cur.scan.span(start, end).map(Ok)
    }
}

/// Every non-overlapping match's groups, as byte spans. Built by
/// [`Regex::capture_spans_iter`].
pub struct CaptureMatches<'r> {
    re: &'r Regex,
    cur: Cursor,
}

/// `Result`-valued for the same reason as [`Matches`].
impl Iterator for CaptureMatches<'_> {
    type Item = Result<GroupSpans, MatchLimit>;

    fn next(&mut self) -> Option<Self::Item> {
        let slots = match self.cur.step(self.re)? {
            Ok(slots) => slots,
            Err(limit) => return Some(Err(limit)),
        };
        Some(Ok(self.re.spans_from_slots(&self.cur.scan, &slots)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `to_string`, which is the `Display` impl above seen from the outside. It
    // needs importing because the crate is `no_std`, so the prelude in scope
    // here is `core`'s and does not carry `ToString`.
    use alloc::string::ToString;

    /// Every malformed pattern is classified the way glibc classifies it.
    ///
    /// The expected codes are *measured*, not derived: each row was run through
    /// findutils 4.9.0 on glibc 2.39 (`find t -regextype posix-extended -regex
    /// PAT`) and the code is the one whose [`RegCode::message`] matches what
    /// GNU printed. `scripts/find-diff.sh` re-checks the same rows end to end
    /// against the real binary; this test is the fast copy that runs on a
    /// machine with no GNU find on it.
    ///
    /// The `[` / `[a` pair is the reason this is a table and not a rule: they
    /// differ only in a trailing character and glibc gives them different
    /// codes, because a pattern that ends immediately after `[` trips its
    /// "premature end" path before it ever decides the bracket was at fault.
    #[test]
    fn a_bad_pattern_is_classified_the_way_glibc_classifies_it() {
        let cases: &[(&str, RegCode)] = &[
            ("[", RegCode::BadPattern),
            ("[^", RegCode::BadPattern),
            ("[a", RegCode::UnmatchedBracket),
            ("[[:alpha:]", RegCode::UnmatchedBracket),
            ("[.", RegCode::UnmatchedBracket),
            ("[=", RegCode::UnmatchedBracket),
            ("[[:foo:]]", RegCode::BadCharClass),
            ("[z-a]", RegCode::BadRangeEnd),
            ("*", RegCode::BadRepeat),
            ("{1}", RegCode::BadRepeat),
            ("(", RegCode::UnmatchedParen),
            ("a(", RegCode::UnmatchedParen),
            ("((a)", RegCode::UnmatchedParen),
            // Everything that reaches an atom slot with a quantifier in it.
            // A group and an alternation branch each start a fresh
            // expression, which is why the last two are errors and not
            // repetitions of what precedes them.
            ("+", RegCode::BadRepeat),
            ("?", RegCode::BadRepeat),
            ("^*", RegCode::BadRepeat),
            ("(*a)", RegCode::BadRepeat),
            ("a|*b", RegCode::BadRepeat),
            // The interval split: no `}` in the pattern is a different error
            // from a `}` with nonsense in front of it.
            ("a{", RegCode::UnmatchedBrace),
            ("a{1", RegCode::UnmatchedBrace),
            ("a{2", RegCode::UnmatchedBrace),
            ("a{b", RegCode::UnmatchedBrace),
            ("a{}", RegCode::BadBraceContent),
            ("a{b}", RegCode::BadBraceContent),
            ("a{1,2,3}", RegCode::BadBraceContent),
            ("a{1,0}", RegCode::BadBraceContent),
            ("a{99999999}", RegCode::TooBig),
            ("a\\", RegCode::TrailingBackslash),
        ];
        for &(pat, want) in cases {
            let got = Regex::new(pat.as_bytes())
                .expect_err("pattern should not compile")
                .code;
            assert_eq!(got, want, "{pat:?}");
        }
    }

    /// Every pattern glibc *accepts* compiles here too.
    ///
    /// This is the other half of the table above and the more easily got
    /// wrong: a parser written from the grammar rejects most of these, and
    /// each rejection is a `find -regex` that fails to run against a pattern
    /// GNU runs happily. Measured the same way — `find t -regextype
    /// posix-extended -regex PAT` exiting 0 — and cross-checked against bash
    /// 5.2.21's `[[ x =~ PAT ]]`, which uses the same
    /// `RE_SYNTAX_POSIX_EXTENDED` and agrees on every row.
    #[test]
    fn patterns_glibc_accepts_compile_here_too() {
        let cases: &[&str] = &[
            // The empty pattern is an expression matching the empty string.
            "",
            // A quantifier may be stacked on an already-quantified
            // expression; glibc just wraps it again.
            "a**", "a*+", "a*?", "a{1}{2}",
            // `)` has no meaning of its own in a POSIX ERE
            // (`RE_UNMATCHED_RIGHT_PAREN_ORD`), so an unopened one is the
            // literal character.
            ")", "a)",
            // An absent interval bound is zero on the left and unbounded on
            // the right.
            "a{,}", "a{,3}", "a{2,}",
            // An empty branch, and an empty group, match the empty string.
            "()", "a|", "|a", "(|a)", "(a||b)",
            // `{0}` does not delete the atom; it yields an expression
            // matching the empty string.
            "a{0}", "(a{0})", "a{0}b",
        ];
        for &pat in cases {
            assert!(
                Regex::new(pat.as_bytes()).is_ok(),
                "{pat:?} should compile: {:?}",
                Regex::new(pat.as_bytes()).err()
            );
        }
    }

    /// The two patterns whose *meaning* the acceptance above could get wrong
    /// by accepting them as the wrong thing.
    #[test]
    fn the_accepted_oddities_mean_what_glibc_means_by_them() {
        // `a{0}b` matches "b", not "ab": the atom is not deleted, it is made
        // optional-zero-times.
        assert!(m("^a{0}b$", "b"));
        assert!(!m("^a{0}b$", "ab"));
        // `a|` is "a, or nothing".
        assert!(m("^(a|)$", "a"));
        assert!(m("^(a|)$", ""));
        // `a)` is a two-character literal.
        assert!(m("^a)$", "a)"));
        // `{,3}` is `{0,3}`, so it matches nothing through three.
        assert!(m("^a{,3}$", ""));
        assert!(m("^a{,3}$", "aaa"));
        assert!(!m("^a{,3}$", "aaaa"));
        // `a**` is `(a*)*`, which is still "any number of a".
        assert!(m("^a**$", "aaaa"));
        assert!(!m("^a**$", "b"));
    }

    /// The sentences are glibc's, byte for byte.
    ///
    /// Pinned as literals because they are an *interface*: they are what our
    /// `find` prints, so a script that greps for `Unmatched \{` keeps working
    /// only as long as nobody tidies the wording.
    #[test]
    fn the_messages_are_glibcs_own_words() {
        assert_eq!(RegCode::BadPattern.message(), "Invalid regular expression");
        assert_eq!(
            RegCode::BadCharClass.message(),
            "Invalid character class name"
        );
        assert_eq!(RegCode::TrailingBackslash.message(), "Trailing backslash");
        assert_eq!(
            RegCode::BadBackReference.message(),
            "Invalid back reference"
        );
        assert_eq!(
            RegCode::UnmatchedBracket.message(),
            "Unmatched [, [^, [:, [., or [="
        );
        assert_eq!(RegCode::UnmatchedParen.message(), r"Unmatched ( or \(");
        assert_eq!(RegCode::UnmatchedRightParen.message(), r"Unmatched ) or \)");
        assert_eq!(RegCode::UnmatchedBrace.message(), r"Unmatched \{");
        assert_eq!(
            RegCode::BadBraceContent.message(),
            r"Invalid content of \{\}"
        );
        assert_eq!(RegCode::BadRangeEnd.message(), "Invalid range end");
        assert_eq!(
            RegCode::BadRepeat.message(),
            "Invalid preceding regular expression"
        );
        assert_eq!(RegCode::TooBig.message(), "Regular expression too big");
    }

    /// The cases below spell patterns and subjects as Rust string literals,
    /// which are UTF-8 by construction, while the engine is byte-typed. These
    /// two adapters keep them readable; the cases that are *about* bytes which
    /// are not text pass byte literals to [`Regex`] directly.
    /// The `unwrap` on the match itself is the budget: a test pattern that
    /// exhausted it would be a bug in the budget, and saying so loudly is what
    /// a test is for.
    fn m(pat: &str, s: &str) -> bool {
        Regex::new(pat.as_bytes())
            .unwrap()
            .is_match(s.as_bytes())
            .unwrap()
    }

    fn mi(pat: &str, s: &str) -> bool {
        Regex::new_flags(pat.as_bytes(), true)
            .unwrap()
            .is_match(s.as_bytes())
            .unwrap()
    }

    /// [`Regex::new`] over a text pattern, for the cases that only ask whether
    /// it compiled.
    fn compile(pat: &str) -> Result<Regex, EreError> {
        Regex::new(pat.as_bytes())
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
        // A zero-count interval yields an expression matching the empty
        // string, so the atom before it is not required — and not forbidden
        // either, which is why `^a{0}b$` does not match "ab".
        assert!(m("^a{0}b$", "b"));
        assert!(m("^a{0,0}b$", "b"));
        assert!(!m("^a{0}b$", "ab"));
        // The open-ended forms, including the one whose lower bound is absent.
        assert!(m("^a{,2}$", ""));
        assert!(m("^a{,2}$", "aa"));
        assert!(!m("^a{,2}$", "aaa"));
        assert!(m("^a{,}$", "aaaaaaa"));
    }

    /// glibc — which is what bash's `=~` runs on — rejects a good deal that a
    /// lenient engine would happily accept. Everything here is measured against
    /// bash 5.2.21: each pattern makes `[[ x =~ … ]]` exit 2.
    ///
    /// The *accept* side of the same survey is
    /// [`patterns_glibc_accepts_compile_here_too`], and it is worth reading the
    /// two together: this test used to claim a dozen of those patterns were
    /// rejected, on the strength of the grammar rather than a measurement, and
    /// every one of those claims was wrong.
    #[test]
    fn rejects_what_glibc_rejects() {
        let bad = |pat: &str| {
            assert!(compile(pat).is_err(), "expected {pat} to be rejected");
        };
        // A `{` that opens no well-formed interval is an error, not a literal.
        // (`a{,3}` *is* well-formed — see the accept side.)
        bad("a{b");
        bad("a{1");
        bad("a{}");
        bad("a{1,2,3}");
        bad("{b");
        // Only a backslash or a bracket expression gets a literal brace.
        assert!(m("^a\\{b$", "a{b"));
        assert!(m("^a[{]b$", "a{b"));
        // A quantifier needs an atom before it — in every context. A group and
        // an alternation branch each begin a fresh expression, so the last two
        // have nothing before them either.
        bad("*a");
        bad("+a");
        bad("?a");
        bad("{2}a");
        bad("(*a)");
        bad("a|*b");
        // `^` is an assertion, not an atom; `$` glibc does let you quantify.
        bad("^*a");
        bad("a^*b");
        assert!(compile("a$*").is_ok());
        // A parenthesis has to be closed. An *unopened* `)` does not have to
        // be — it is an ordinary character — which is the accept side's job.
        bad("(");
        bad("((a)");
        bad("(a");
    }

    /// Compile in egrep syntax.
    fn ge(pat: &str) -> Result<Regex, EreError> {
        Regex::new_syntax(pat.as_bytes(), false, Syntax::EGREP)
    }

    /// Match in egrep syntax — see [`m`] for why the `unwrap`s are the point.
    fn me(pat: &str, s: &str) -> bool {
        ge(pat).unwrap().is_match(s.as_bytes()).unwrap()
    }

    /// Under egrep syntax a quantifier with nothing to quantify repeats the
    /// empty expression instead of being an error.
    ///
    /// Measured against GNU grep 3.11 in `C.UTF-8`: each pattern below exits 0
    /// (after a `warning: … at start of expression` on stderr) and prints every
    /// line the same pattern *without* the leading operator would print. The
    /// same patterns are `REG_BADRPT` under `RE_SYNTAX_POSIX_EXTENDED`, which
    /// [`rejects_what_glibc_rejects`] pins — the two tests are the same survey
    /// run in the two dialects and are meant to be read together.
    #[test]
    fn egrep_lets_a_quantifier_repeat_nothing() {
        for pat in ["*a", "+a", "?a", "{2}a", "{,}a"] {
            assert!(ge(pat).is_ok(), "{pat:?} should compile under egrep");
            assert!(me(pat, "xa"), "{pat:?} should match \"xa\"");
            assert!(!me(pat, "b"), "{pat:?} should not match \"b\"");
            assert!(
                Regex::new(pat.as_bytes()).is_err(),
                "{pat:?} must stay an error under posix-extended"
            );
        }
        // A group and an alternation branch each begin a fresh expression, so
        // the operator has nothing before it there either.
        assert!(me("(*a)", "xa"));
        assert!(me("a|*b", "b"));
        // `^*` is the anchor repeated, and zero repetitions of an assertion is
        // the empty string — which is why `grep -E 'a^*b'` matches "ab" even
        // though a `^` in the middle of a pattern can never hold.
        assert!(me("^*", "b"));
        assert!(me("a^*b", "ab"));
    }

    /// Under egrep syntax a `{` that opens no interval is an ordinary
    /// character.
    ///
    /// glibc's `RE_INVALID_INTERVAL_ORD`. The dividing line is not "is this
    /// interval sensible" but "did glibc get far enough to commit to reading
    /// one": `a{b}` rolls back to a literal, `a{}` does not. Measured — every
    /// row below is a `grep -E` run whose output was the literal line and
    /// nothing else, or an exit-2 diagnostic.
    #[test]
    fn egrep_reads_a_malformed_interval_as_a_literal_brace() {
        for pat in [
            "a{b}", "a{", "a{2", "a{1,b}", "a{1,", "a{,b}", "a{1,2b}", "{b}a",
        ] {
            assert!(ge(pat).is_ok(), "{pat:?} should compile under egrep");
            assert!(me(pat, pat), "{pat:?} should match itself literally");
            assert!(
                Regex::new(pat.as_bytes()).is_err(),
                "{pat:?} must stay an error under posix-extended"
            );
        }
        // `{}a` and `{1,2,3}a` are the two shapes that are an error *after* an
        // atom and a literal *before* one. GNU is two engines — glibc decides
        // whether to reject, dfa.c decides what matches — and at the start of
        // an expression glibc skips the token instead of judging it, so no
        // rejection ever happens and dfa's literal reading is what runs.
        for pat in ["{}a", "{1,2,3}a"] {
            assert!(me(pat, pat), "{pat:?} should match itself literally");
        }
        // Committed-to intervals stay errors in both dialects.
        for pat in ["a{}", "a{1,2,3}", "a{1,0}", "a{99999999}"] {
            assert!(ge(pat).is_err(), "{pat:?} must stay an error under egrep");
            assert!(Regex::new(pat.as_bytes()).is_err());
        }
        // And a well-formed interval still means what it means.
        assert!(me("^a{2,}$", "aa"));
        assert!(!me("^a{2,}$", "a"));
        assert!(me("^a{1}{2}$", "aa"));
        assert!(me("^a{,3}$", ""));
    }

    /// Everything the two dialects agree about, checked in both.
    ///
    /// Worth its own test because the temptation when adding a dialect is to
    /// fork more than actually differs. Only two things do; these are the
    /// neighbours of those two, and they must not move.
    #[test]
    fn the_dialects_differ_in_exactly_two_places() {
        for pat in [")", "a)", "a**", "a{,3}", "a{2,}", "a|", "|a", "(|a)", ""] {
            assert!(ge(pat).is_ok(), "{pat:?} should compile under egrep");
            assert!(
                Regex::new(pat.as_bytes()).is_ok(),
                "{pat:?} should compile under posix-extended"
            );
        }
        for pat in ["(", "((a)", "(a", "a[", "a\\"] {
            assert!(ge(pat).is_err(), "{pat:?} should fail under egrep");
            assert!(
                Regex::new(pat.as_bytes()).is_err(),
                "{pat:?} should fail under posix-extended"
            );
        }
        // An unopened `)` is the literal character in both.
        assert!(me("^a)$", "a)"));
        assert!(m("^a)$", "a)"));
        // The default is the strict dialect, so nothing that does not ask for
        // egrep can drift into it.
        assert_eq!(Syntax::default(), Syntax::POSIX_EXTENDED);
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

    /// The four GNU shorthands, read by the ERE parser itself.
    ///
    /// glibc honours these in both dialects — `RE_NO_GNU_OPS` is off for every
    /// syntax grep, sed and awk use — so they belong here rather than in
    /// `bre`'s translation. Before 2026-08-24 only the BRE side had them, by
    /// rewriting; in ERE they fell through to the literal arm, so `grep -E
    /// '\w'` searched for the letter `w`.
    #[test]
    fn the_gnu_shorthand_classes() {
        assert!(m(r"^\w+$", "ab_12"));
        assert!(!m(r"^\w+$", "a b"));
        assert!(!m(r"\w", "   "));
        assert!(m(r"^\W$", "-"));
        assert!(!m(r"^\W$", "_"));
        assert!(m(r"^\s$", " "));
        assert!(m(r"^\s$", "\t"));
        assert!(!m(r"^\s$", "x"));
        assert!(m(r"^\S$", "x"));
        assert!(!m(r"^\S$", " "));
        // A shorthand is an atom, so it quantifies and concatenates.
        assert!(m(r"^\w\s\w$", "a b"));
        assert!(m(r"^\w{2,}$", "ab"));
        // It is *not* one inside a bracket: POSIX gives a backslash no meaning
        // there, and GNU agrees — measured, `grep -E '^[\w-]+$'` matches `w\-`
        // and not `a-b_1`.
        assert!(!m(r"^[\w-]+$", "a-b_1"));
    }

    /// The four GNU word assertions, against measured GNU output.
    ///
    /// | script | input | GNU |
    /// |---|---|---|
    /// | `s/\b/\|/g` | `a1 2b` | `\|a1\| \|2b\|` |
    /// | `s/\B/\|/g` | `abc` | `a\|b\|c` |
    /// | `s/\B/\|/g` | `a  b` | `a \| b` |
    /// | `s/\B/\|/g` | (empty) | `\|` |
    /// | `s/\</[/g` | `foo bar` | `[foo [bar` |
    /// | `s/\>/]/g` | `foo bar` | `foo] bar]` |
    #[test]
    fn the_gnu_word_assertions() {
        // `\b`: the two sides disagree about being word characters.
        assert!(m(r"\bfoo\b", "foo bar"));
        assert!(!m(r"\bfo\b", "foo bar"));
        assert!(m(r"\bx\b", "x"));
        assert!(m(r"\b", "café"));
        // `\B`: they agree — including when *neither* is a word character,
        // which is why `\B` matches inside a run of spaces, and matches the
        // empty subject: both sides are off the end.
        assert!(m(r"a\Bb", "abc"));
        assert!(!m(r"b\Bc", "ab c"));
        assert!(m(r"\B", ""));
        assert!(m(r" \B ", "a  b"));
        assert!(!m(r"\Bfoo", "foo"));
        // `\<` and `\>`: one specific end of a word.
        assert!(m(r"\<bar\>", "foo bar baz"));
        assert!(!m(r"\<ar\>", "foo bar baz"));
        assert!(!m(r"\<ba\>", "foo bar baz"));
        assert!(m(r"\>", "a "));
        assert!(!m(r"\>", "  "));
        // `_` is a word character; a letter outside ASCII is one too.
        assert!(m(r"^\<a_b\>$", "a_b"));
        assert!(m(r"^\<café\>$", "café"));
    }

    /// Zero-width, so a word assertion has nothing for a quantifier to repeat
    /// and matches without consuming: `\<*` is a repeat of an assertion (ERE
    /// lets a quantifier repeat nothing — see [`egrep_lets_a_quantifier_repeat_nothing`]),
    /// and `\b\b` is the same test twice.
    #[test]
    fn a_word_assertion_consumes_nothing() {
        let re = compile(r"\<foo").unwrap();
        let caps = re.captures(b"a foo").unwrap().unwrap();
        assert_eq!(caps[0].as_deref(), Some(&b"foo"[..]));
        assert!(m(r"\b\bfoo", "foo"));
        assert!(m(r"^\<\<foo\>\>$", "foo"));
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
        let re = compile(r"([0-9]+)-([0-9]+)").unwrap();
        let caps = re.captures(b"range 10-25 end").unwrap().unwrap();
        assert_eq!(caps[0].as_deref(), Some(&b"10-25"[..]));
        assert_eq!(caps[1].as_deref(), Some(&b"10"[..]));
        assert_eq!(caps[2].as_deref(), Some(&b"25"[..]));
    }

    #[test]
    fn leftmost_match() {
        // Leftmost start wins; greedy length at that start.
        let re = compile("a+").unwrap();
        let caps = re.captures(b"baaa").unwrap().unwrap();
        assert_eq!(caps[0].as_deref(), Some(&b"aaa"[..]));
    }

    #[test]
    fn no_catastrophic_backtracking() {
        // A classic ReDoS pattern: the Pike VM must handle it in linear time
        // (this returns quickly rather than hanging).
        let re = compile("(a+)+$").unwrap();
        let input = "a".repeat(40) + "!";
        assert!(!re.is_match(input.as_bytes()).unwrap());
    }

    /// A `=~` subject is a shell value and the pattern is a shell word, so
    /// either may hold a byte that begins no valid UTF-8 sequence — a SlateOS
    /// path admits every byte but `/` and NUL. The engine used to be `&str`-typed
    /// and `cond_regex` had to refuse: such a subject matched nothing, and such
    /// a pattern was reported as an uncompilable right-hand side.
    #[test]
    fn matches_a_subject_and_a_pattern_that_are_not_text() {
        // `.` matches one *character*, and an undecodable byte is one — not a
        // third of an `é`, and not nothing.
        assert!(Regex::new(b"^a.b$").unwrap().is_match(b"a\xffb").unwrap());
        assert!(
            Regex::new(b"^a.b$")
                .unwrap()
                .is_match("aéb".as_bytes())
                .unwrap()
        );
        assert!(
            !Regex::new(b"^a..b$")
                .unwrap()
                .is_match("aéb".as_bytes())
                .unwrap()
        );
        // The case from the tracked issue: an anchored match on such a subject.
        assert!(Regex::new(b"^a").unwrap().is_match(b"a\xffb").unwrap());
        assert!(!Regex::new(b"^b").unwrap().is_match(b"a\xffb").unwrap());

        // The byte can be written in the *pattern* too — bare…
        assert!(Regex::new(b"\xff").unwrap().is_match(b"a\xffb").unwrap());
        assert!(!Regex::new(b"\xff").unwrap().is_match(b"a\xfeb").unwrap());
        // …after a backslash, which denotes it rather than escaping anything…
        assert!(
            Regex::new(b"^a\\\xffb$")
                .unwrap()
                .is_match(b"a\xffb")
                .unwrap()
        );
        // …and inside a bracket expression.
        assert!(
            Regex::new(b"^a[\xff\xfe]b$")
                .unwrap()
                .is_match(b"a\xffb")
                .unwrap()
        );
        assert!(
            !Regex::new(b"^a[\xfe]b$")
                .unwrap()
                .is_match(b"a\xffb")
                .unwrap()
        );

        // It falls in no written range and in no POSIX class: it is not a
        // letter, and no collation would place it among them…
        assert!(!Regex::new(b"[a-z]").unwrap().is_match(b"\xff").unwrap());
        assert!(
            !Regex::new(b"[[:alpha:]]")
                .unwrap()
                .is_match(b"\xff")
                .unwrap()
        );
        assert!(
            !Regex::new(b"[[:print:]]")
                .unwrap()
                .is_match(b"\xff")
                .unwrap()
        );
        // …so a negated class does match it, as bash in the C locale does.
        assert!(Regex::new(b"^[^a-z]$").unwrap().is_match(b"\xff").unwrap());

        // A quantifier counts it as one character.
        assert!(
            Regex::new(b"^\xff{3}$")
                .unwrap()
                .is_match(b"\xff\xff\xff")
                .unwrap()
        );
        assert!(
            !Regex::new(b"^\xff{3}$")
                .unwrap()
                .is_match(b"\xff\xff")
                .unwrap()
        );

        // It has no case, so under `nocasematch` it folds only to itself — it
        // can become neither a letter nor a different byte.
        assert!(
            Regex::new_flags(b"^\xff$", true)
                .unwrap()
                .is_match(b"\xff")
                .unwrap()
        );
        assert!(
            !Regex::new_flags(b"^\xff$", true)
                .unwrap()
                .is_match(b"\xfe")
                .unwrap()
        );

        // A capture hands back the bytes, not an approximation of them — this
        // is what reaches `BASH_REMATCH`.
        let caps = Regex::new(b"^a(.+)b$")
            .unwrap()
            .captures(b"a\xff\xfeb")
            .unwrap()
            .unwrap();
        assert_eq!(caps[1].as_deref(), Some(&b"\xff\xfe"[..]));
    }

    #[test]
    fn errors() {
        assert!(compile("(unclosed").is_err());
        assert!(compile("[unclosed").is_err());
        assert!(compile(r"trailing\").is_err());
        assert!(compile("a{2,1}").is_err());
        assert!(compile("[[:bogus:]]").is_err());
    }

    /// Nested intervals multiply, and a 24-byte pattern can ask for 10⁹
    /// instructions. This test is as much about *terminating* as about the
    /// error: before [`MAX_PROG`] it would have run until the allocator gave
    /// up. It is written with a deadline so a regression fails by name rather
    /// than by hanging the suite.
    #[test]
    fn nested_intervals_are_refused_rather_than_expanded() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send([
                compile("((a{1000}){1000}){1000}").is_err(),
                compile("(a{1000}){1000}").is_err(),
                compile("(a{500}){500}").is_err(),
            ]);
        });
        let got = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("compiling a nested interval did not finish: MAX_PROG is not bounding it");
        assert_eq!(got, [true, true, true]);
    }

    /// The other half of the cap: a pattern anyone would actually write must
    /// still compile. A limit that rejected `a{1000}b{1000}` would have traded
    /// one failure mode for another.
    #[test]
    fn a_pattern_worth_writing_still_fits() {
        assert!(compile("a{1000}b{1000}").is_ok());
        assert!(compile("([0-9]{1,3}\\.){3}[0-9]{1,3}").is_ok());
        assert!(compile(&"(ab|cd)*".repeat(200)).is_ok());
        let re = Regex::new(b"(a{100}){10}").expect("10 * 100 copies is 1000 instructions");
        assert!(re.is_match(&b"a".repeat(1000)).unwrap());
        assert!(!re.is_match(&b"a".repeat(999)).unwrap());
    }

    #[test]
    fn the_longest_match_wins_not_the_first_alternative() {
        // POSIX is leftmost-*longest*; Perl and the `regex` crate are
        // leftmost-first. Priority ordering alone answers these with the short
        // arm, which is what `grep -o` and `sed` would then have printed.
        let cap = |pat: &str, s: &str| {
            compile(pat)
                .unwrap()
                .captures(s.as_bytes())
                .unwrap()
                .unwrap()[0]
                .clone()
                .unwrap()
        };
        assert_eq!(cap("a|ab", "ab"), b"ab");
        assert_eq!(cap("ab|a", "ab"), b"ab");
        assert_eq!(cap("a|ab|abc", "abcd"), b"abc");
        assert_eq!(cap("(a|ab)(c|bcd)", "abcd"), b"abcd");
        // Leftmost still beats longer: the match at 1 is not preferred to the
        // one at 0 for being longer.
        assert_eq!(compile("a|bb").unwrap().find(b"abb").unwrap(), Some((0, 1)));
        // And the rule reaches the scanning API, which is what actually feeds
        // `grep -o`.
        let spans = compile("a|ab")
            .unwrap()
            .find_iter(b"abab")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(spans, vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn a_longest_match_still_reports_its_groups() {
        // The second pass re-runs the pattern, so the capture slots it hands
        // back have to be the winning thread's, not the first pass's.
        let re = compile("(a+)(b*)").unwrap();
        let caps = re.captures(b"xaaabb").unwrap().unwrap();
        assert_eq!(caps[0].as_deref(), Some(&b"aaabb"[..]));
        assert_eq!(caps[1].as_deref(), Some(&b"aaa"[..]));
        assert_eq!(caps[2].as_deref(), Some(&b"bb"[..]));
        assert_eq!(
            re.capture_spans(b"xaaabb").unwrap().unwrap(),
            vec![Some((1, 6)), Some((1, 4)), Some((4, 6)),]
        );
    }

    // ---- byte offsets ----------------------------------------------------
    //
    // `captures` answers "what did it match"; these answer "where", which is
    // what `grep -o`, `sed`'s `s///` and awk's `sub`/`gsub` need in order to
    // rebuild the subject around the match rather than just report it.

    fn re(pat: &str) -> Regex {
        Regex::new(pat.as_bytes()).unwrap()
    }

    #[test]
    fn a_match_reports_the_bytes_it_covers() {
        assert_eq!(re("b+").find(b"aabbbcc").unwrap(), Some((2, 5)));
        assert_eq!(re("^a").find(b"aab").unwrap(), Some((0, 1)));
        assert_eq!(re("c$").find(b"abc").unwrap(), Some((2, 3)));
        assert_eq!(re("z").find(b"abc").unwrap(), None);
        // The span is a slice of the subject, which is the whole point of
        // returning it rather than the text.
        let (s, e) = re("b.d").find(b"xxabcdyy").unwrap().unwrap();
        assert_eq!(&b"xxabcdyy"[s..e], b"bcd");
    }

    #[test]
    fn an_offset_is_measured_in_bytes_not_characters() {
        // é is two bytes, so a character count and a byte count disagree from
        // the second character on — the bug this API exists to make impossible.
        let hay = "aébé".as_bytes();
        let (s, e) = re("b").find(hay).unwrap().unwrap();
        assert_eq!((s, e), (3, 4));
        assert_eq!(&hay[s..e], b"b");
        // And a match *of* a multi-byte character spans all of its bytes.
        assert_eq!(re("é").find(hay).unwrap(), Some((1, 3)));
    }

    #[test]
    fn a_resumed_search_still_anchors_to_the_start_of_the_subject() {
        // POSIX spells this REG_NOTBOL. It is why `sed 's/^a//g'` strips one
        // leading `a` rather than one at every position it resumes from.
        let bol = re("^a");
        assert_eq!(bol.find_at(b"aaa", 0).unwrap(), Some((0, 1)));
        assert_eq!(bol.find_at(b"aaa", 1).unwrap(), None);
        // The end anchor is the mirror image: still the end of the subject.
        assert_eq!(re("a$").find_at(b"aaa", 1).unwrap(), Some((2, 3)));
    }

    #[test]
    fn a_resume_point_inside_a_character_rounds_forward() {
        // Landing mid-character is what a caller that adds byte counts does;
        // rounding back would re-match text already consumed and loop.
        let hay = "éab".as_bytes();
        assert_eq!(re("a").find_at(hay, 1).unwrap(), Some((2, 3)));
        assert_eq!(
            re("é").find_at(hay, 1).unwrap(),
            None,
            "the character at 0..2 is behind us"
        );
    }

    #[test]
    fn a_scan_yields_every_match_left_to_right() {
        let hay = b"ab12cd345ef";
        let spans = re("[0-9]+")
            .find_iter(hay)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(spans, vec![(2, 4), (6, 9)]);
        let texts: Vec<&[u8]> = spans.iter().map(|&(s, e)| &hay[s..e]).collect();
        assert_eq!(texts, vec![&b"12"[..], &b"345"[..]]);
    }

    #[test]
    fn a_scan_of_a_pattern_that_can_match_nothing_terminates() {
        // `sed 's/x*/-/g'` on "axb" is "-a-b-": a match at each position the
        // previous one did not already reach, and then a stop. A scan that did
        // not step past an empty match would hang instead.
        let spans = re("x*")
            .find_iter(b"axb")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(spans, vec![(0, 0), (1, 2), (3, 3)]);
        // (An *empty* pattern is a compile error here, as it is in glibc, so
        // the pattern that matches nothing has to be spelled with a `*`.)
        assert_eq!(re("z*").find_iter(b"ab").count(), 3);
    }

    #[test]
    fn an_empty_match_touching_the_previous_one_is_not_a_second_match() {
        // GNU agrees on both of these, and they are the same question:
        //   $ echo aaa | sed 's/a*/-/g'   ->  -
        //   $ echo aaa | grep -o 'a*'     ->  aaa
        // After `a*` has consumed `aaa` there is an empty match available at
        // offset 3, because `a*` also matches nothing. Reporting it would give
        // `--` and a spurious second `grep -o` line.
        assert_eq!(
            re("a*")
                .find_iter(b"aaa")
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![(0, 3)]
        );
        // Only the *touching* empty match goes. `axa` reports the two runs and
        // neither of the empty matches that sit against their ends — which is
        // `sed 's/a*/-/g'` giving `-x-` and `grep -o 'a*'` giving two lines.
        assert_eq!(
            re("a*")
                .find_iter(b"axa")
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![(0, 1), (2, 3)]
        );
    }

    #[test]
    fn a_scan_does_not_overlap_its_own_matches() {
        assert_eq!(
            re("aa")
                .find_iter(b"aaaa")
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![(0, 2), (2, 4)]
        );
    }

    #[test]
    fn group_spans_locate_the_parts_of_a_match() {
        let hay = b"key=value";
        let spans = re("([a-z]+)=([a-z]+)").capture_spans(hay).unwrap().unwrap();
        assert_eq!(spans, vec![Some((0, 9)), Some((0, 3)), Some((4, 9))]);
        // A group that did not participate has no span — as distinct from an
        // empty one, which does.
        let spans = re("(a)|(b)").capture_spans(b"b").unwrap().unwrap();
        assert_eq!(spans, vec![Some((0, 1)), None, Some((0, 1))]);
        assert_eq!(
            re("(x*)y").capture_spans(b"y").unwrap().unwrap()[1],
            Some((0, 0))
        );
    }

    #[test]
    fn group_spans_are_reported_for_every_match_of_a_scan() {
        let hay = b"a1 b22 c3";
        let got: Vec<_> = re("([a-z])([0-9]+)")
            .capture_spans_iter(hay)
            .map(|g| {
                let g = g.unwrap();
                (g[1].unwrap(), g[2].unwrap())
            })
            .collect();
        assert_eq!(
            got,
            vec![((0, 1), (1, 2)), ((3, 4), (4, 6)), ((7, 8), (8, 9))]
        );
    }

    #[test]
    fn a_span_can_end_at_the_end_of_the_subject() {
        // The off-by-one this API is easiest to get wrong at: the byte-offset
        // table needs one entry more than there are characters.
        assert_eq!(re("c$").find(b"abc").unwrap(), Some((2, 3)));
        assert_eq!(re("$").find(b"ab").unwrap(), Some((2, 2)));
        assert_eq!(re("x*").find(b"").unwrap(), Some((0, 0)));
        assert_eq!(
            re("x*")
                .find_iter(b"")
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![(0, 0)]
        );
    }

    #[test]
    fn an_undecodable_byte_is_one_character_wide() {
        // 0xFF begins no valid UTF-8 sequence, so it is its own character and
        // a span must not split it or skip it.
        let hay: &[u8] = &[b'a', 0xFF, b'b'];
        assert_eq!(re("b").find(hay).unwrap(), Some((2, 3)));
        assert_eq!(re("a.b").find(hay).unwrap(), Some((0, 3)));
    }

    // ---- backreferences --------------------------------------------------
    //
    // A backreference cannot be a Pike VM instruction: the VM advances every
    // alternative together, so there is no single "the" capture to compare
    // against and no single width to advance by. These patterns — and only
    // these — take the backtracker instead.

    #[test]
    fn a_backreference_matches_what_its_group_matched() {
        assert!(m("(a)\\1", "aa"));
        assert!(!m("(a)\\1", "ab"));
        assert!(m("(ab|cd)x\\1", "abxab"));
        assert!(!m("(ab|cd)x\\1", "abxcd"));
        // The engine has to *retry* the alternative: `cd` is the branch that
        // makes the whole pattern match, and a matcher that committed to `ab`
        // on the first pass would answer "no".
        assert!(m("^(ab|cd)x\\1$", "cdxcd"));
        // Nine groups are addressable, and the number is the group index — not
        // a position in the pattern.
        assert!(m("(a)(b)(c)(d)(e)(f)(g)(h)(i)\\9\\1", "abcdefghiia"));
    }

    #[test]
    fn only_a_pattern_with_a_backreference_leaves_the_pike_vm() {
        // The choice is made once, at compile time, so every pattern that does
        // not use one keeps the linear guarantee untouched.
        assert!(!compile("(a)b").unwrap().has_backref());
        assert!(compile("(a)\\1").unwrap().has_backref());
        // An escape that is not a digit is still a literal, not a reference.
        assert!(!compile("a\\.b").unwrap().has_backref());
    }

    #[test]
    fn a_backreference_to_a_group_that_does_not_exist_is_a_compile_error() {
        // Left as a literal digit it would be a wrong answer with no
        // diagnostic — `(a)\2` would quietly match `a2`.
        let e = compile("(a)\\2").unwrap_err();
        assert!(
            String::from_utf8_lossy(&e.detail).contains("invalid backreference"),
            "{}",
            String::from_utf8_lossy(&e.detail)
        );
        assert!(compile("\\1").is_err());
        // Forward references are refused for the same reason: the group is not
        // yet open when the reference is read.
        assert!(compile("\\1(a)").is_err());
    }

    #[test]
    fn a_backreference_to_a_group_that_did_not_participate_does_not_match() {
        // `(a)|b` leaves group 1 unset when it takes the `b` branch. An unset
        // group is not an empty one: POSIX says the reference fails, where
        // treating it as "" would make the whole pattern match `b`.
        assert!(!m("^((a)|b)\\2$", "b"));
        assert!(m("^((a)|b)\\2$", "aa"));
    }

    #[test]
    fn a_backreference_folds_case_when_the_pattern_does() {
        assert!(mi("(ab)\\1", "AbaB"));
        assert!(!m("(ab)\\1", "AbaB"));
    }

    #[test]
    fn a_backreference_reports_its_spans_like_any_other_match() {
        let re = Regex::new(b"(a+)b\\1").unwrap();
        assert_eq!(re.find(b"xaabaay").unwrap(), Some((1, 6)));
        let caps = re.capture_spans(b"xaabaay").unwrap().unwrap();
        assert_eq!(caps, vec![Some((1, 6)), Some((1, 3))]);
        // And the scan keeps working, which is what `grep -o` runs.
        let spans = Regex::new(b"(.)\\1")
            .unwrap()
            .find_iter(b"aabxcc")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(spans, vec![(0, 2), (4, 6)]);
    }

    #[test]
    fn the_backtracker_prefers_the_longest_match_like_the_pike_vm_does() {
        // POSIX leftmost-longest is the whole crate's rule; the second engine
        // must not quietly become a leftmost-first (Perl) matcher.
        assert_eq!(
            Regex::new(b"(a*)\\1").unwrap().find(b"aaaa").unwrap(),
            Some((0, 4))
        );
        assert_eq!(
            Regex::new(b"(a|ab)\\1?c").unwrap().find(b"ababc").unwrap(),
            Some((0, 5))
        );
    }

    #[test]
    fn a_backreference_pattern_that_can_loop_forever_terminates() {
        // `(a*)*` can take its outer loop having consumed nothing, which is an
        // infinite path unless a back edge is refused twice at one position.
        assert!(m("^((a*)*)\\1$", "aa"));
        assert!(m("^(x*)*\\1$", ""));
        assert!(!m("^(a*)*b\\1$", "aac"));
    }

    #[test]
    fn a_pathological_backreference_gives_up_rather_than_hanging() {
        // Backreference matching is NP-hard, so the budget is the only thing
        // standing between a shaped pattern and a wedged `grep`. What matters
        // is that it comes back *and says which* — a caller that read this as
        // "no match" would, under `sed '/re/!d'`, delete the line.
        let re = Regex::new(b"(a*)(a*)(a*)(a*)(a*)\\1\\2\\3\\4\\5b").unwrap();
        assert!(re.has_backref());
        let hay = vec![b'a'; 200];
        assert_eq!(re.is_match(&hay), Err(MatchLimit));
        // The error is the same one every entry point reports.
        assert_eq!(re.find(&hay), Err(MatchLimit));
        assert_eq!(re.captures(&hay), Err(MatchLimit));
        assert!(MatchLimit.to_string().contains("step limit"));
    }

    #[test]
    fn a_deep_backreference_match_does_not_overflow_the_stack() {
        // The backtracker keeps its frames in a `Vec`, not on the call stack:
        // recursion depth would grow with the repetitions matched, so this
        // pattern would be a crash in five programs rather than an answer.
        let hay = vec![b'a'; 20_000];
        let re = Regex::new(b"^(a)\\1*$").unwrap();
        assert_eq!(re.is_match(&hay), Ok(true));
    }
}
