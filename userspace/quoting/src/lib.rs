//! How a utility renders a name, or any other untrusted text, inside a
//! diagnostic.
//!
//! ## Why a diagnostic cannot just print the name
//!
//! A path on this system may hold every byte except `/` and NUL. That includes
//! the newline. So a utility that writes
//!
//! ```text
//! cp: cannot stat: {name}: No such file or directory
//! ```
//!
//! with the name pasted in raw hands whoever chose the name a way to write
//! *whole lines* of its own into the utility's error stream. A file called
//! `a\ncp: /etc/shadow: Permission denied` makes `cp` appear to report a second
//! failure that never happened. Every log reader, every `2>&1 | grep`, and
//! every person scanning a terminal is fooled by it, and nothing in the utility
//! is at fault except that it printed the name unquoted.
//!
//! The second, duller reason is ambiguity: `cannot stat: my file` does not say
//! whether that is one name with a space or the start of a sentence, and a
//! trailing space or a control character is invisible entirely.
//!
//! ## The styles, and which to use
//!
//! Four of these are the ones nearly every diagnostic reaches for, and the
//! difference between them is visible enough that matching it matters — a test
//! or a script that reads our diagnostic next to GNU's should see the same
//! bytes.
//!
//! | Function | GNU's name | Used for | `abc` | `a b` | `a'b` |
//! |---|---|---|---|---|---|
//! | [`quotef`] | `quotef` | a name that **ends the message** | `abc` | `'a b'` | `"a'b"` |
//! | [`quoteaf`] | `quoteaf` | a name **inside a sentence** | `'abc'` | `'a b'` | `"a'b"` |
//! | [`quote`] | `quote` | **option arguments**, and anything else | `‘abc’` | `‘a b’` | `‘a'b’` |
//! | [`quote_glibc`] | — (glibc inlines `'%s'`) | **option names**, in getopt's own errors | `'abc'` | `'a b'` | `'a\'b'` |
//!
//! Only the third row has curly marks, and that is not a typo: of GNU's three
//! *gnulib* styles exactly one moves with the locale's character set, and it is
//! that one. File names — the text most likely to be pasted back into a shell
//! or matched by a script — keep the straight marks in every locale. See
//! `design-decisions.md` §351.
//!
//! ## The fourth row is not gnulib's, and that is the whole point
//!
//! The first three come from gnulib's `quotearg.c`. The fourth comes from
//! **glibc**, whose `getopt_long` writes its own diagnostics with the quotes
//! spelled out in the format string:
//!
//! ```c
//! fprintf (stderr, _("%s: unrecognized option '%s'\n"), argv[0], argv[optind]);
//! ```
//!
//! Nothing there consults the locale, so those messages keep straight marks
//! even where `quote()` has gone curly — and a single GNU command line can
//! print one of each:
//!
//! ```text
//! $ LC_ALL=C.UTF-8 sort --key             # glibc getopt
//! sort: option '--key' requires an argument
//! $ LC_ALL=C.UTF-8 sort --sort=zzz        # gnulib argmatch
//! sort: invalid argument ‘zzz’ for ‘--sort’
//! ```
//!
//! Before §351 both families rendered identically, so routing them through one
//! function was invisible; the moment [`quote`] went curly it became six wrong
//! diagnostics. [`quote_glibc`] exists so the distinction is written down in
//! the type system rather than remembered.
//!
//! **glibc is not the only source of a straight mark, and "which library wrote
//! it" is therefore not the test.** gnulib's own `xstrtol-error.c` spells its
//! quotes into the format string exactly as glibc's `getopt_long` does, so the
//! diagnostic an option-taking caller prints for a bad number is straight while
//! the one a *quantity*-taking caller prints for the same bad number is curly.
//! Measured, GNU 9.4, `LC_ALL=C.UTF-8`:
//!
//! ```text
//! $ od -j x f      ->  od: invalid -j argument 'x'
//! $ sort -S x f    ->  sort: invalid -S argument 'x'
//! $ head -n x f    ->  head: invalid number of lines: ‘x’
//! $ split -b x f   ->  split: invalid number of bytes: ‘x’
//! ```
//!
//! Both pairs reject the same argument for the same reason. See
//! [`crate::xnum::strtol_fatal`], which is the straight one.
//!
//! The last style, [`quote_c_maybe_colon`], is a C-string rendering rather than
//! a shell one, and is rare: `paste` uses it for its delimiter list and `ls`
//! offers it as `--quoting-style=c-maybe`. See its own documentation.
//!
//! The first two produce a shell word: what they print can be pasted back into
//! a shell to name the same file. They differ only in whether an already-safe
//! name keeps its quotes, and **which one to use is decided by the sentence,
//! not by the utility**:
//!
//! ```text
//! wc:   missing.txt: No such file or directory                    <- quotef
//! head: cannot open 'missing.txt' for reading: No such file ...   <- quoteaf
//! ```
//!
//! A name that ends the message can be bare, because nothing follows it to run
//! into; one with words after it cannot, because `cannot open missing files
//! for reading` reads as a phrase rather than as a name. So the rule is: use
//! [`quotef`] where the name is the last thing on the line before the `:
//! reason`, [`quoteaf`] where anything follows it.
//!
//! [`quote`] always quotes, and escapes in C rather than shell style; it is for
//! text that was never a shell word to begin with, where the quotes are
//! punctuation marking where the quoted thing starts and stops. That is why
//! `sort: invalid argument ‘bogus’ for ‘--sort’` uses it.
//!
//! ## Where the rules come from
//!
//! Every table and every branch below was **measured**, not recalled: see
//! `scripts/quote-probe.py`, which drives GNU `sort` and `head` over every
//! byte in every position that turns out to matter and records what came out.
//! The result is `tests/quotearg-gnu.txt`, 8719 rows, and `tests/quotearg.rs`
//! asserts this module reproduces all of them.
//!
//! That method is the whole point. Reading gnulib's `quotearg.c` and
//! reimplementing what it appears to say produces something that looks right
//! and differs in a dozen corners — the lone `{`, the `#` that is special only
//! at the front, the `~` that is allowed inside double quotes only at the
//! front, and gnulib's stray `''` prefix. None of those would have been
//! guessed, and each was found by comparing bytes.

/// Bytes a shell needs no quoting for, wherever they appear in a word.
///
/// `:` is here because a shell does not mind a colon — `ls
/// --quoting-style=shell-escape` leaves `a:z` bare. The callers which reach
/// [`quotef`] *do* mind: they are writing `prog: what happened: NAME`, and a
/// colon inside `NAME` would read as another layer of that structure. gnulib
/// gives them `quotearg_style_colon`, which adds `:` to `quote_these_too` —
/// the set of bytes that force the outer quotes on. That is expressed here as
/// the `allow_bare` argument [`quotef`] computes, not as a hole in this table;
/// see [`Style::quote_with`] for why the two are the same thing. Measured, and
/// the *only* difference between the two renderings: of 2904 names rendered
/// both ways, the four that disagree are `:`, `a:`, `:z` and `a:z`. See
/// `scripts/ls-quote-probe.py`.
const SAFE: &[u8] = b"%+,-./0123456789:@ABCDEFGHIJKLMNOPQRSTUVWXYZ]_\
                      abcdefghijklmnopqrstuvwxyz{}";

/// Safe too, but not as the first byte: `#` starts a comment and `~` starts a
/// home-directory expansion only at the front of a word.
const SAFE_NOT_FIRST: &[u8] = b"#~";

/// Safe unless it is the *whole* word. A lone `{` or `}` is a bash reserved
/// word; one inside a word is an ordinary character. (Brace *expansion* needs
/// a comma or a `..` between them, both of which force quoting anyway.)
const SAFE_UNLESS_ALONE: &[u8] = b"{}";

/// Bytes that may sit inside the `"..."` form.
const DQ_SAFE: &[u8] = b" %'+,-.0123456789:@ABCDEFGHIJKLMNOPQRSTUVWXYZ]_\
                         abcdefghijklmnopqrstuvwxyz";

/// Allowed inside `"..."` too — but only as the *first* byte, which is the
/// opposite way round from [`SAFE_NOT_FIRST`] and is not a transcription
/// error. GNU does this, `scripts/quote-probe.py` shows it, and the fixture
/// pins it: `~a'z` comes out `"~a'z"` while `a'z~` comes out `'a'\''z~'`.
const DQ_SAFE_FIRST_ONLY: &[u8] = b"#~";

/// One step of reading a byte string that is *usually* UTF-8: either a
/// character, or a byte that begins no valid sequence.
///
/// Every escaping loop below walks these rather than bytes. That is the whole
/// difference between rendering `é` as `é` and rendering it as `\303\251`, and
/// it is not a cosmetic one — a name a user typed is text, and a diagnostic
/// that spells it back in octal is a diagnostic that cannot be matched against
/// the name it is about.
#[derive(Clone, Copy)]
enum Piece {
    /// A character, and how many bytes its encoding occupied.
    Char(char, usize),
    /// A byte at a position where UTF-8 does not decode. Always rendered as an
    /// escape: there is no character to print, so there is nothing else it
    /// *could* be rendered as without inventing one.
    Byte(u8),
}

impl Piece {
    /// How far past this piece the next one starts.
    const fn len(self) -> usize {
        match self {
            Self::Char(_, n) => n,
            Self::Byte(_) => 1,
        }
    }

    /// Whether this piece can appear in a rendering as itself.
    fn printable(self) -> bool {
        match self {
            Self::Char(c, _) => printable_char(c),
            Self::Byte(_) => false,
        }
    }
}

/// Whether a character prints as itself.
///
/// This is the rule `design-decisions.md` §101/§104 already settled for osh's
/// `%q`, extended by exactly two code points; §357 records the extension. In
/// full: a character is printable unless it is a Unicode **control** (`Cc` —
/// the C0 range, DEL, and the C1 range `U+0080..=U+009F`) or one of the two
/// **line/paragraph separators** `U+2028`/`U+2029`.
///
/// ## Why not a real `iswprint` table
///
/// glibc's `iswprint` under `C.UTF-8` is 709 ranges derived from that
/// release's `UnicodeData.txt`, and copying it would mean carrying a table
/// that drifts every Unicode release — where "drift" means a name that
/// rendered one way last year renders another way this year for no reason the
/// user did anything about. Measured by `scripts/printable-audit.py` over all
/// 1,112,064 code points, the rule above agrees with glibc 2.39's table on
/// every **assigned** character without exception; the 824,718 it disagrees on
/// are precisely the **unassigned** ones (`Cn`, e.g. `U+0378`), which glibc
/// escapes and we print. That is the one deliberate divergence, and
/// `tests/quotearg.rs` and `tests/c_maybe.rs` assert it *stays* a divergence so
/// the reason cannot go quietly stale.
///
/// ## Why the two separators are added
///
/// `Zl`/`Zp` is a set of exactly two characters, so naming them costs nothing
/// and needs no table. They are added because a terminal, a log reader, and
/// most `2>&1 | grep` pipelines treat U+2028 as ending a line — which is
/// precisely the line-forgery this module exists to prevent, arriving in a
/// character that is not `Cc`. Escaping them is not merely matching glibc; it
/// would be right even if glibc printed them.
///
/// Note this makes coreutils stricter than osh, whose `needs_ansi_c_quote`
/// (§101) leaves U+2028 raw. The two are allowed to differ: osh is quoting
/// *for re-execution by a shell*, where a separator is an ordinary character,
/// and coreutils is quoting *for a human reading a line-oriented stream*.
fn printable_char(c: char) -> bool {
    !c.is_control() && c != '\u{2028}' && c != '\u{2029}'
}

/// One step of `mbrtowc`, with its two failures kept apart.
///
/// C's `mbrtowc` answers three ways and callers act differently on each, so a
/// decoder that collapses the two failures into "not a character" cannot serve
/// them all. `ls`'s `-q` is the caller that needs the distinction: an **invalid**
/// byte costs one `?` and the scan resumes at the next byte, while an
/// **incomplete** sequence at the end of the name costs one `?` for the whole
/// remaining tail. `ls -q` on a name ending in a lone `\xc3` therefore prints
/// one `?`, and on `\xc3\xc3` prints two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mb {
    /// A character and the number of bytes it occupied. `mbrtowc`'s positive
    /// return.
    Char(char, usize),
    /// A byte that begins no valid sequence — `mbrtowc`'s `(size_t) -1`.
    /// Consumes exactly one byte, whatever follows it.
    Invalid,
    /// A valid *prefix* cut short by the end of the string — `(size_t) -2`.
    /// Consumes the rest of the string, because there is no more input coming.
    Incomplete,
}

/// The [`Mb`] at the front of `text`, or `None` if `text` is empty.
///
/// ```
/// use coreutils::quote::{Mb, next_mb};
/// assert_eq!(next_mb(b"abc"), Some(Mb::Char('a', 1)));
/// assert_eq!(next_mb("é".as_bytes()), Some(Mb::Char('é', 2)));
/// // `\xff` can begin no sequence at all, so it is invalid rather than short.
/// assert_eq!(next_mb(b"\xff"), Some(Mb::Invalid));
/// // `\xc3` is a valid two-byte lead, so alone at the end it is incomplete...
/// assert_eq!(next_mb(b"\xc3"), Some(Mb::Incomplete));
/// // ...but followed by a byte that cannot continue it, it is invalid.
/// assert_eq!(next_mb(b"\xc3("), Some(Mb::Invalid));
/// assert_eq!(next_mb(b""), None);
/// ```
#[must_use]
pub fn next_mb(text: &[u8]) -> Option<Mb> {
    text.first()?;
    // Four bytes is the longest a UTF-8 character can be, so a window that
    // size can never cut a valid one short -- which is what lets the decode
    // below be a plain `from_utf8` rather than a hand-rolled state machine.
    let head = text.get(..text.len().min(4)).unwrap_or(text);
    let valid = match std::str::from_utf8(head) {
        Ok(t) => t,
        Err(e) => {
            // `valid_up_to() == 0` is the interesting case: the string does
            // not decode *here*, so the answer is one of the two failures.
            // Anything else means the window held a character followed by
            // trouble, and the character is ours.
            let n = e.valid_up_to();
            if n == 0 {
                // `error_len() == None` is std's spelling of "ran out of
                // input", which is exactly `mbrtowc`'s `-2`.
                return Some(if e.error_len().is_none() {
                    Mb::Incomplete
                } else {
                    Mb::Invalid
                });
            }
            head.get(..n)
                .and_then(|v| std::str::from_utf8(v).ok())
                .unwrap_or("")
        }
    };
    Some(match valid.chars().next() {
        Some(c) => Mb::Char(c, c.len_utf8()),
        None => Mb::Invalid,
    })
}

/// The piece beginning at byte `i`, or `None` at the end of the string.
///
/// The escaping styles do not care *why* a byte failed to decode — both
/// failures render as one escape per byte — so [`Mb`]'s two failures collapse
/// back into one [`Piece::Byte`] here.
fn piece_at(s: &[u8], i: usize) -> Option<Piece> {
    let rest = s.get(i..)?;
    let &first = rest.first()?;
    Some(match next_mb(rest)? {
        Mb::Char(c, n) => Piece::Char(c, n),
        Mb::Invalid | Mb::Incomplete => Piece::Byte(first),
    })
}

/// Every piece of `s`, each with the byte offset it starts at.
///
/// The offset is not decoration: [`bare_ok`] needs "is this the first?" and
/// [`c_always`] needs to look at the byte after a NUL.
fn pieces(s: &[u8]) -> impl Iterator<Item = (usize, Piece)> + '_ {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        let p = piece_at(s, i)?;
        let at = i;
        i = i.saturating_add(p.len());
        Some((at, p))
    })
}

/// Escape a whole piece: one C escape per byte of it.
///
/// Per *byte*, not per character, even when the piece is a character. That is
/// what GNU does — `U+0080` comes out `\302\200`, its two UTF-8 bytes — and it
/// is the only rendering that round-trips, since the reader of a `$'...'` word
/// is a shell decoding bytes.
fn escape_piece(p: Piece, out: &mut String) {
    match p {
        Piece::Byte(b) => c_escape(b, out),
        Piece::Char(c, _) => {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                c_escape(b, out);
            }
        }
    }
}

/// `name` rendered as itself, if every piece is a character `ok` accepts.
///
/// `None` says at least one piece was not, which is the signal to fall through
/// to a quoted form. A `Piece::Byte` is never accepted, so a `Some` result is
/// also a proof that `name` was valid UTF-8.
fn all_bare(name: &[u8], ok: impl Fn(usize, Piece) -> bool) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    for (i, p) in pieces(name) {
        match p {
            Piece::Char(c, _) if ok(i, p) => out.push(c),
            _ => return None,
        }
    }
    Some(out)
}

/// The C escape for a byte that does not print: the named one where there is
/// one, otherwise three octal digits.
///
/// Always three digits, never fewer: `\1` followed by a literal `2` would read
/// back as `\12`, so a shortened escape can change the string it decodes to.
fn c_escape(b: u8, out: &mut String) {
    let named = match b {
        0x07 => 'a',
        0x08 => 'b',
        0x09 => 't',
        0x0a => 'n',
        0x0b => 'v',
        0x0c => 'f',
        0x0d => 'r',
        _ => {
            octal_escape(b, out);
            return;
        }
    };
    out.push('\\');
    out.push(named);
}

/// A byte as `\` and three octal digits, most significant first.
///
/// Always three, never fewer: `\1` followed by a literal `2` would read back
/// as `\12`, so a shortened escape can change the string it decodes to.
///
/// Written out rather than formatted so this stays allocation-free and
/// obviously total — `b >> 6` is at most 3, so every digit is in range by
/// construction and there is nothing to check.
fn octal_escape(b: u8, out: &mut String) {
    out.push('\\');
    out.push((b'0' + (b >> 6)) as char);
    out.push((b'0' + ((b >> 3) & 7)) as char);
    out.push((b'0' + (b & 7)) as char);
}

/// The marks [`quote`] wraps its argument in: U+2018 and U+2019, the curly
/// typographic pair.
///
/// GNU picks these from the locale's character set — straight `'...'` where it
/// is ASCII, curly `‘...’` where it is UTF-8. We have no ASCII branch on
/// purpose: `design-decisions.md` §351, resting on Q38, settles that the string
/// layer here is UTF-8 full stop, so a branch on a locale that cannot occur
/// would be dead code that looks load-bearing.
///
/// Note these are the *only* place a non-ASCII byte enters a rendering from
/// this module. [`quotef`] and [`quoteaf`] keep straight marks in every locale
/// — that is GNU's behaviour, measured, and it is what keeps a file name
/// something you can paste back into a shell.
const LEFT_QUOTE: char = '\u{2018}';
const RIGHT_QUOTE: char = '\u{2019}';

/// The first character of `text`, and how many bytes it occupied.
///
/// `None` where `text` is empty or begins with a byte that starts no valid
/// UTF-8 sequence — which the caller must handle, because the byte is real and
/// something has to be done with it.
///
/// This is exported rather than private because `printf`'s `'x` character
/// constant asks exactly this question, and a second decoder written for it
/// would be a second place for "what is a character here?" to be answered.
/// Measured, GNU 9.4 under `LC_ALL=C.UTF-8`: `printf %d "'é"` prints `233`,
/// the code point — not `195`, its first byte.
///
/// ```
/// use coreutils::quote::first_char;
/// assert_eq!(first_char(b"abc"), Some(('a', 1)));
/// assert_eq!(first_char("é".as_bytes()), Some(('é', 2)));
/// assert_eq!(first_char("😀".as_bytes()), Some(('😀', 4)));
/// assert_eq!(first_char(b"\xff"), None);
/// assert_eq!(first_char(b""), None);
/// ```
#[must_use]
pub fn first_char(text: &[u8]) -> Option<(char, usize)> {
    match piece_at(text, 0)? {
        Piece::Char(c, n) => Some((c, n)),
        Piece::Byte(_) => None,
    }
}

/// Render `text` with every printable character as itself and everything else
/// as one **three-digit octal escape per byte** — no quote marks, and no named
/// escapes.
///
/// This is for a diagnostic that has already punctuated the untrusted text some
/// other way, so adding marks would double them up. `printf`'s two are the
/// motivating case: `%q: invalid conversion specification` puts the directive
/// where the reader can see it is a directive, and the character-constant
/// warning is followed by a colon.
///
/// The escapes are octal even where a named one exists, which is the one place
/// this differs from every other style here. It is deliberate: this rendering
/// is read by a person, not parsed by a shell, and `\012` next to `\302\200`
/// reads as one uniform mechanism where `\n` next to `\302\200` reads as two.
///
/// ```
/// use coreutils::quote::escape_unprintable;
/// assert_eq!(escape_unprintable(b"abc"), "abc");
/// assert_eq!(escape_unprintable(b"a\nb"), r"a\012b");
/// assert_eq!(escape_unprintable("é".as_bytes()), "é");
/// assert_eq!(escape_unprintable(b"\xff"), r"\377");
/// assert_eq!(escape_unprintable("\u{2028}".as_bytes()), r"\342\200\250");
/// ```
#[must_use]
pub fn escape_unprintable(text: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    for (_, p) in pieces(text) {
        match p {
            Piece::Char(c, _) if printable_char(c) => out.push(c),
            Piece::Byte(b) => octal_escape(b, &mut out),
            Piece::Char(c, _) => {
                let mut buf = [0u8; 4];
                for &b in c.encode_utf8(&mut buf).as_bytes() {
                    octal_escape(b, &mut out);
                }
            }
        }
    }
    out
}

/// Render `arg` the way GNU's `quote()` does: always inside `‘...’`, with C
/// escapes.
///
/// This is for text that is not a file name — an option's argument, a word
/// from a configuration file, a field from input being echoed back. The quotes
/// are always present because their job here is to mark where the quoted thing
/// starts and stops, which a bare word does not do.
///
/// **An ASCII `'` is not escaped**, which is the part most likely to be
/// misremembered: it needed escaping when it *was* the delimiter, and now that
/// the delimiter is `’` it cannot be confused with one, so GNU leaves it bare.
/// A backslash is still doubled, because it still introduces the escapes.
///
/// **The closing mark `’` *is* escaped**, for the reason the ASCII `'` is not:
/// a `’` inside the quoted text would be indistinguishable from the end of it.
/// The opening `‘` is left bare, because nothing is looking for one. Both are
/// measured; see the fixture rows for `\xe2\x80\x98` and `\xe2\x80\x99`.
///
/// The result is UTF-8, and printable apart from the marks themselves. A
/// character that [`printable_char`] rejects, and every byte that is not valid
/// UTF-8 at all, comes back as one three-digit octal escape *per byte*.
///
/// ```
/// use coreutils::quote::quote;
/// assert_eq!(quote(b"bogus"), "\u{2018}bogus\u{2019}");
/// assert_eq!(quote(b"a b"), "\u{2018}a b\u{2019}");
/// assert_eq!(quote(b"it's"), "\u{2018}it's\u{2019}");
/// assert_eq!(quote(b"a\\b"), "\u{2018}a\\\\b\u{2019}");
/// assert_eq!(quote(b"a\tb"), "\u{2018}a\\tb\u{2019}");
/// assert_eq!(quote(b"\xff"), "\u{2018}\\377\u{2019}");
/// // A character prints as itself; the closing mark is escaped first.
/// assert_eq!(quote("é".as_bytes()), "\u{2018}é\u{2019}");
/// assert_eq!(quote("’".as_bytes()), "\u{2018}\\\u{2019}\u{2019}");
/// ```
#[must_use]
pub fn quote(arg: &[u8]) -> String {
    locale_quote(arg, b"")
}

/// [`quote`], parameterised by gnulib's `quote_these_too` set — see
/// [`Style::quote_with`]. The locale styles escape rather than elide, so a
/// byte in `extra` comes back with a `\` in front of it.
fn locale_quote(arg: &[u8], extra: &[u8]) -> String {
    // Six bytes of delimiter, not two: each curly mark is three bytes of UTF-8.
    let mut out = String::with_capacity(arg.len().saturating_add(6));
    out.push(LEFT_QUOTE);
    for (_, p) in pieces(arg) {
        match p {
            Piece::Char('\\', _) => out.push_str("\\\\"),
            Piece::Char(RIGHT_QUOTE, _) => {
                out.push('\\');
                out.push(RIGHT_QUOTE);
            }
            Piece::Char(c, _) if printable_char(c) => push_maybe_escaped(c, extra, &mut out),
            other => escape_piece(other, &mut out),
        }
    }
    out.push(RIGHT_QUOTE);
    out
}

/// Emit one printable character, with a `\` in front if it is in `extra`.
///
/// This is gnulib's `store_escape` for the non-eliding styles: `START_ESC()`
/// stores a backslash and the character follows it unchanged. It is reached
/// only for characters that fall off the end of `quotearg_buffer_restyled`'s
/// switch, which is to say the printable ones — a control byte was already
/// escaped by its own `case` and never consults the set.
fn push_maybe_escaped(c: char, extra: &[u8], out: &mut String) {
    // `extra` is a set of ASCII bytes, so a non-ASCII character can never be
    // in it, and `c as u8` of one would be wrong rather than merely useless.
    if c.is_ascii() && extra.contains(&(c as u8)) {
        out.push('\\');
    }
    out.push(c);
}

/// Render `arg` inside straight `'...'`, the way **glibc's `getopt_long`**
/// spells its own diagnostics — plus the C escaping glibc omits.
///
/// This is [`quote`]'s straight-marked sibling, and it exists because the two
/// families of GNU diagnostic quote differently. gnulib's `quote()` follows the
/// locale's character set and goes curly under UTF-8; glibc's getopt writes
/// `'%s'` into the format string and so never does. Use this one for the text
/// glibc names — a short option's letter, a long option's spelling, an
/// unrecognized argument as typed — and [`quote`] for everything else. See this
/// module's header for the worked example where one command line prints both.
///
/// ## Where this deliberately differs from glibc
///
/// glibc performs **no escaping whatever**, which is not a style choice we can
/// copy: it is the line-forging bug described at the top of this module, live
/// in the C library. Measured, under `LC_ALL=C.UTF-8`:
///
/// ```text
/// $ wc --no$'\n'pe
/// wc: unrecognized option '--no
/// pe'
/// ```
///
/// Two lines, the second of which the user chose. A `'` fares no better —
/// `wc -\'` prints `invalid option -- '''`, where the marks can no longer be
/// told from the content. So this function escapes: a byte that does not print
/// becomes a C escape, and the `'` that *is* the delimiter here becomes `\'`.
/// For every ordinary option name — which is to say every one that is not an
/// attack — the two agree byte for byte, and that is the case the comparison
/// with GNU actually rests on.
///
/// This is not the same rule as [`quote`], which leaves `'` bare precisely
/// because `’` has taken over as its delimiter. The escaping follows the
/// delimiter, and the delimiters differ.
///
/// The result is always printable, and ASCII except where `arg` itself held a
/// printable character that was not — an option spelled `--café` keeps its
/// `é`, exactly as glibc would print it.
///
/// ```
/// use coreutils::quote::quote_glibc;
/// assert_eq!(quote_glibc(b"--key"), "'--key'");
/// assert_eq!(quote_glibc(b"x"), "'x'");
/// assert_eq!(quote_glibc(b"it's"), r"'it\'s'");
/// assert_eq!(quote_glibc(b"a\\b"), r"'a\\b'");
/// assert_eq!(quote_glibc(b"a\tb"), r"'a\tb'");
/// assert_eq!(quote_glibc(b"\xff"), r"'\377'");
/// assert_eq!(quote_glibc("--café".as_bytes()), "'--café'");
/// ```
#[must_use]
pub fn quote_glibc(arg: &[u8]) -> String {
    let mut out = String::with_capacity(arg.len().saturating_add(2));
    out.push('\'');
    for (_, p) in pieces(arg) {
        match p {
            Piece::Char(c @ ('\\' | '\''), _) => {
                out.push('\\');
                out.push(c);
            }
            Piece::Char(c, _) if printable_char(c) => out.push(c),
            other => escape_piece(other, &mut out),
        }
    }
    out.push('\'');
    out
}

/// Render `text` the way GNU's `c_maybe_quoting_style` does: as a **C string
/// literal**, with the outer `"` left off when nothing inside needed them.
///
/// This is not a shell rendering. Where [`quotef`] produces something you
/// could paste back into a shell, this produces something you could paste into
/// a C or Rust source file, and the two disagree loudly: a space, a `'`, a
/// `$`, a `` ` `` and even a backslash are all left bare here, because none of
/// them means anything to a C compiler outside quotes.
///
/// The only things that pull the quotes back on are the ones a C literal could
/// not hold as itself: `"`, anything [`printable_char`] rejects, and any byte
/// that is not valid UTF-8. Once the quotes are on, *everything* is escaped
/// C-style, including the backslash that was bare a moment earlier.
///
/// ```
/// use coreutils::quote::quote_c_maybe;
/// assert_eq!(quote_c_maybe(br"a b\"), r"a b\");
/// assert_eq!(quote_c_maybe(b"a'b"), "a'b");
/// assert_eq!(quote_c_maybe(br#"a"b\"#), r#""a\"b\\""#);
/// assert_eq!(quote_c_maybe(b"a\tb"), r#""a\tb""#);
/// assert_eq!(quote_c_maybe(b"a:b"), "a:b");
/// // Measured: `ls --quoting-style=c-maybe` leaves a character bare.
/// assert_eq!(quote_c_maybe("aéz".as_bytes()), "aéz");
/// ```
#[must_use]
pub fn quote_c_maybe(text: &[u8]) -> String {
    c_maybe(text, b"")
}

/// [`quote_c_maybe`], plus `:` in the set of bytes that force the quotes on.
///
/// This is the combination GNU actually uses — `quotearg_n_style_colon (0,
/// c_maybe_quoting_style, …)` — and it is the one to reach for. The colon is
/// added for a reason worth restating: these renderings go into diagnostics
/// whose shape is `prog: what happened: THING`, so a `THING` holding a colon
/// of its own would read as another layer of that structure. Quoting it makes
/// the boundary unambiguous.
///
/// ```
/// use coreutils::quote::quote_c_maybe_colon;
/// assert_eq!(quote_c_maybe_colon(br"a b\"), r"a b\");
/// assert_eq!(quote_c_maybe_colon(br"a:b\"), r#""a:b\\""#);
/// assert_eq!(quote_c_maybe_colon(b":"), r#"":""#);
/// ```
#[must_use]
pub fn quote_c_maybe_colon(text: &[u8]) -> String {
    c_maybe(text, b":")
}

/// The body of both, parameterised by gnulib's `quote_these_too` set.
///
/// gnulib expresses "maybe" by rendering optimistically and restarting from
/// scratch the moment it meets a byte it cannot render bare (`goto
/// force_outer_quoting_style`). [`all_bare`] is the same thing said forwards:
/// it builds the optimistic rendering and hands back `None` instead of a
/// half-built one, which it can because that rendering is just the text.
fn c_maybe(text: &[u8], quote_these_too: &[u8]) -> String {
    let bare = |_i: usize, p: Piece| match p {
        // `quote_these_too` is a set of ASCII bytes, so a non-ASCII character
        // can never be in it -- and asking `c as u8` of one would be wrong,
        // not merely useless.
        Piece::Char(c, _) => {
            c != '"' && printable_char(c) && !(c.is_ascii() && quote_these_too.contains(&(c as u8)))
        }
        Piece::Byte(_) => false,
    };
    all_bare(text, bare).unwrap_or_else(|| c_always(text, b""))
}

/// The forced pass: gnulib's `c_quoting_style` with the outer quotes present.
///
/// Note what `quote_these_too` does *not* do when [`c_maybe`] restarts here.
/// gnulib drops it — "don't reuse quote_these_too, since the addition of outer
/// quotes sufficiently quotes the specified characters" — so a `:` that forced
/// the quotes on then appears inside them as a plain colon, not as an escape.
/// That is why `c_maybe` passes an empty `extra` and [`Style::C`], which never
/// elides and so never restarts, passes its own.
fn c_always(text: &[u8], extra: &[u8]) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for (i, p) in pieces(text) {
        match p {
            Piece::Char(c @ ('"' | '\\'), _) => {
                out.push('\\');
                out.push(c);
            }
            Piece::Char('\0', _) => nul_escape(text, i, &mut out),
            Piece::Char(c, _) if printable_char(c) => push_maybe_escaped(c, extra, &mut out),
            other => escape_piece(other, &mut out),
        }
    }
    out.push('"');
    out
}

/// Render `name` the way GNU's `quotef()` does: quoted only where a shell
/// would need it, and in a form that can be pasted back into a shell.
///
/// This is for file names. An ordinary one comes back unchanged, which is what
/// keeps the common diagnostic readable; anything a shell would treat
/// specially — a space, a metacharacter, a control byte — is quoted or
/// escaped, which is what stops a name from forging a line of its own.
///
/// The result is always printable, whatever `name` held — every piece
/// [`printable_char`] rejects, and every byte that is not valid UTF-8, becomes
/// a visible escape. It is *not* always ASCII: a name that was text comes back
/// as that text, which is the point. A name that was not comes back in octal.
///
/// ```
/// use coreutils::quote::quotef;
/// assert_eq!(quotef(b"notes.txt"), "notes.txt");
/// assert_eq!(quotef(b"my notes.txt"), "'my notes.txt'");
/// assert_eq!(quotef(b"it's"), "\"it's\"");
/// assert_eq!(quotef(b"two\nlines"), r"'two'$'\n''lines'");
/// assert_eq!(quotef(b""), "''");
/// // A character is a character; a byte that decodes to none is octal.
/// assert_eq!(quotef("café.txt".as_bytes()), "café.txt");
/// assert_eq!(quotef(b"caf\xe9.txt"), r"'caf'$'\351''.txt'");
/// ```
#[must_use]
pub fn quotef(name: &[u8]) -> String {
    // gnulib's `quotearg_style_colon`: `:` in `quote_these_too`. Under an
    // eliding style that set does exactly one thing -- force the quotes on --
    // so it is spelled here as the condition rather than as a rule inside the
    // renderer. See [`Style::quote_with`].
    render(name, !name.contains(&b':'))
}

/// Render `name` the way GNU's `quoteaf()` does: the same shell-pasteable form
/// as [`quotef`], except that the quotes are never left off.
///
/// The choice between this and [`quotef`] is made by the *shape of the
/// sentence*, not by the utility. A name that ends the message can be bare,
/// because nothing follows it to run into:
///
/// ```text
/// wc: missing.txt: No such file or directory
/// ```
///
/// A name embedded in a sentence cannot, because a bare one would blur into
/// the words around it — `cannot open missing files for reading` reads as a
/// phrase rather than as a name:
///
/// ```text
/// head: cannot open 'missing.txt' for reading: No such file or directory
/// ```
///
/// ```
/// use coreutils::quote::quoteaf;
/// assert_eq!(quoteaf(b"notes.txt"), "'notes.txt'");
/// assert_eq!(quoteaf(b"my notes.txt"), "'my notes.txt'");
/// assert_eq!(quoteaf(b"it's"), "\"it's\"");
/// assert_eq!(quoteaf(b""), "''");
/// ```
#[must_use]
pub fn quoteaf(name: &[u8]) -> String {
    // No colon set here, and gnulib does not pass one either: `quoteaf` is
    // plain `quotearg_style (shell_escape_always_quoting_style, …)`. It would
    // make no difference if it did -- the quotes are already on.
    render(name, false)
}

/// The body of both shell-escaping styles.
///
/// `allow_bare` is the difference between them: gnulib calls it "elide outer
/// quotes", and it is a property of the sentence the name is going into, not
/// of the name.
fn render(name: &[u8], allow_bare: bool) -> String {
    if allow_bare && !name.is_empty() {
        // Safe as it stands. This is the overwhelmingly common case and the
        // reason `quotef` exists rather than everything using `quote`.
        if let Some(bare) = all_bare(name, |i, p| bare_ok(name, i, p)) {
            return bare;
        }
    }
    // A `'` is a single byte in valid UTF-8 and can be no part of a multi-byte
    // sequence, so looking for one bytewise is exact.
    if !name.is_empty()
        && name.contains(&b'\'')
        && let Some(inner) = all_bare(name, dq_ok)
    {
        // A name holding a single quote reads far better wrapped in double
        // quotes than spliced with '\'' at every occurrence.
        let mut out = String::with_capacity(inner.len().saturating_add(2));
        out.push('"');
        out.push_str(&inner);
        out.push('"');
        return out;
    }
    shell_escape(name)
}

/// Whether a piece needs no shell quoting at all, at offset `i` of `name`.
///
/// The tables it consults are ASCII, and deliberately: every character a shell
/// gives meaning to is ASCII, so a printable character above it is an ordinary
/// one and goes bare. Measured — GNU's `quotef` prints `é` as `é`.
fn bare_ok(name: &[u8], i: usize, p: Piece) -> bool {
    let Piece::Char(c, _) = p else { return false };
    if !c.is_ascii() {
        return printable_char(c);
    }
    let b = c as u8;
    if SAFE_UNLESS_ALONE.contains(&b) {
        return name.len() > 1;
    }
    SAFE.contains(&b) || (i > 0 && SAFE_NOT_FIRST.contains(&b))
}

/// Whether a piece may sit inside the `"..."` form, at offset `i`.
fn dq_ok(i: usize, p: Piece) -> bool {
    let Piece::Char(c, _) = p else { return false };
    if !c.is_ascii() {
        return printable_char(c);
    }
    let b = c as u8;
    DQ_SAFE.contains(&b) || (i == 0 && DQ_SAFE_FIRST_ONLY.contains(&b))
}

/// The general form: a run of literal text inside `'...'`, a `'` spliced as
/// `'\''`, and a run of unprintable pieces inside `$'...'`.
fn shell_escape(name: &[u8]) -> String {
    let mut out = String::with_capacity(name.len().saturating_mul(2).saturating_add(2));

    // gnulib emits two more quotes here than the rendering needs, for exactly
    // these inputs. It is a wart -- `''` is an empty shell word, so the result
    // means the same thing either way -- but it is reproduced so that our
    // diagnostics are byte-identical to GNU's rather than *nearly* identical,
    // which is the difference between a comparison being a check and being a
    // source of noise. 163 of the fixture's rows depend on it; do not "fix" it
    // without regenerating `tests/quotearg-gnu.txt` and finding it gone.
    //
    // "Ends unprintable" is a fact about the last *piece*, not the last byte:
    // a name ending in `é` ends in a printable character whose final byte
    // (`0xa9`) is not one on its own.
    if name.contains(&b'\'')
        && pieces(name).last().is_some_and(|(_, p)| !p.printable())
        && name.first() != Some(&b'\'')
    {
        out.push_str("''");
    }

    out.push('\'');
    let mut open = true;
    let mut it = pieces(name).peekable();
    while let Some((_, p)) = it.next() {
        match p {
            Piece::Char('\'', _) => {
                // Close, escape the quote outside any quoting, reopen.
                if open {
                    out.push('\'');
                }
                out.push_str("\\''");
                open = true;
            }
            Piece::Char(c, _) if printable_char(c) => {
                if !open {
                    out.push('\'');
                    open = true;
                }
                out.push(c);
            }
            other => {
                // A whole run of unprintable pieces shares one `$'...'`.
                if open {
                    out.push('\'');
                }
                out.push_str("$'");
                escape_piece(other, &mut out);
                while let Some(&(_, q)) = it.peek() {
                    if q.printable() {
                        break;
                    }
                    escape_piece(q, &mut out);
                    it.next();
                }
                out.push('\'');
                open = false;
            }
        }
    }
    if open {
        out.push('\'');
    }
    out
}

/// gnulib's ten quoting styles: the set `ls --quoting-style` names.
///
/// Everything above is a *diagnostic* renderer, and a diagnostic gets no say —
/// the sentence it sits in decides. `ls` is the other kind of caller: it prints
/// names as its output rather than inside a message, and it lets the user pick
/// the rendering, so it needs all ten by name. `cp -v`, `mv -v` and `install`
/// have the same shape and can use this too.
///
/// Three of these are the functions above under gnulib's own names, and the
/// naming is worth stating because one of the three is *not* an alias:
///
/// | Variant | Same as | |
/// |---|---|---|
/// | [`Style::ShellEscapeAlways`] | [`quoteaf`] | exactly |
/// | [`Style::Locale`], [`Style::Clocale`] | [`quote`] | exactly |
/// | [`Style::ShellEscape`] | [`quotef`] | **except for `:`** — see [`SAFE`] |
///
/// [`Style::Clocale`] is a separate word `ls` accepts and, under a UTF-8
/// locale, an identical rendering to [`Style::Locale`]; it is kept as its own
/// variant so that `--quoting-style=clocale` round-trips through
/// [`Style::WORDS`] rather than silently becoming a different word in `--help`
/// output or an error list.
///
/// ## What the seven new ones do that the three did not
///
/// [`Style::Literal`] and the two `Shell` ones can emit bytes that are not
/// valid UTF-8 — that is the point of `literal`, and `shell` quotes without
/// escaping — so the rendering is a [`Vec<u8>`] rather than a [`String`]. The
/// diagnostic renderers can return a [`String`] only because they escape
/// everything unprintable, which is exactly what these do not.
///
/// Every rule below was measured; see `scripts/ls-quote-probe.py` and
/// `tests/quotearg-ls-gnu.txt`, which records all ten answers for each of 2905
/// names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// The name unchanged. `ls`'s default when standard output is not a
    /// terminal, and the only style that can print a control byte.
    Literal,
    /// Quoted where a shell would need it, and **nothing escaped**: a name
    /// holding a tab comes back as `'a<TAB>b'`, with the tab still a tab.
    Shell,
    /// [`Style::Shell`], never bare.
    ShellAlways,
    /// Quoted where a shell would need it, with unprintable bytes escaped into
    /// `$'...'`. `ls`'s default when standard output is a terminal.
    ShellEscape,
    /// [`Style::ShellEscape`], never bare. `-Q`'s companion in spirit, though
    /// `-Q` is [`Style::C`].
    ShellEscapeAlways,
    /// A C string literal, quotes and all: `"a\tb"`. What `-Q` selects.
    C,
    /// [`Style::C`] with the quotes left off where nothing needs them.
    CMaybe,
    /// C escapes with no surrounding quotes at all. What `-b` selects.
    ///
    /// A space comes back as a space, not as `\ `. `ls -b` does print `a\ b`,
    /// but that is `ls` adding `' '` to `quote_these_too` for file names only
    /// — pass it through [`Style::quote_with`], as `ls` does, and not through
    /// this style.
    Escape,
    /// The locale's quotation marks — `‘...’` here — around C escapes.
    Locale,
    /// The same, from the C locale's table rather than the message catalogue's.
    /// Indistinguishable from [`Style::Locale`] in every locale we have.
    Clocale,
}

impl Style {
    /// The words `--quoting-style` accepts, in the order `ls` lists them when
    /// it rejects one.
    ///
    /// Shaped for [`crate::getopt::Program::argmatch`], which prefix-matches
    /// and prints this list back on a miss — so the order here *is* the order
    /// in the diagnostic, and the values are what decides whether a prefix is
    /// ambiguous. Every value is distinct except the last two, which means
    /// `--quoting-style=c` is exact, `--quoting-style=cl` resolves (both
    /// spellings agree), and `--quoting-style=s` does not.
    pub const WORDS: &'static [(&'static str, Self)] = &[
        ("literal", Self::Literal),
        ("shell", Self::Shell),
        ("shell-always", Self::ShellAlways),
        ("shell-escape", Self::ShellEscape),
        ("shell-escape-always", Self::ShellEscapeAlways),
        ("c", Self::C),
        ("c-maybe", Self::CMaybe),
        ("escape", Self::Escape),
        ("locale", Self::Locale),
        ("clocale", Self::Clocale),
    ];

    /// `text` rendered in this style.
    ///
    /// ```
    /// use coreutils::quote::Style;
    /// assert_eq!(Style::Literal.quote(b"a\tb"), b"a\tb");
    /// assert_eq!(Style::Shell.quote(b"a\tb"), b"'a\tb'");
    /// assert_eq!(Style::ShellEscape.quote(b"a\tb"), br"'a'$'\t''b'");
    /// assert_eq!(Style::C.quote(b"a\tb"), br#""a\tb""#);
    /// assert_eq!(Style::Escape.quote(b"a\tb"), br"a\tb");
    /// assert_eq!(Style::Locale.quote(b"a\tb"), "‘a\\tb’".as_bytes());
    /// // The one place a name that is not text survives the round trip.
    /// assert_eq!(Style::Literal.quote(b"caf\xe9"), b"caf\xe9");
    /// ```
    #[must_use]
    pub fn quote(self, text: &[u8]) -> Vec<u8> {
        self.quote_with(text, b"")
    }

    /// [`Style::quote`], plus gnulib's `quote_these_too` — the set of extra
    /// bytes a caller wants singled out.
    ///
    /// This is `set_char_quoting`, which `ls` calls three times and which has
    /// no equivalent in the plain rendering. `ls` needs it because the
    /// characters it appends to a name are ordinary characters that a name
    /// could also contain:
    ///
    /// | `ls` sets | on | so that |
    /// |---|---|---|
    /// | `' '` | file names, `--quoting-style=escape` | a space in a name cannot end the word |
    /// | `*=>@\|` | `--file-type` | a name ending in one is not read as the indicator |
    /// | `=>@\|` | `-F` (`--classify`) | the same, minus `*`, which `-F` does not append |
    /// | `':'` | the `dir:` header line | a colon in a directory name is not the separator |
    ///
    /// ## The set does two different things, and for three styles it does nothing
    ///
    /// gnulib consults it only when `(backslash_escapes && quoting_style !=
    /// shell_always) || elide_outer_quotes`, and what happens then depends on
    /// which half of that was true:
    ///
    /// - **Eliding styles** ([`Style::Shell`], [`Style::ShellEscape`],
    ///   [`Style::CMaybe`]) jump to `force_outer_quoting_style`, which
    ///   *re-renders from scratch with the set dropped*. So the set only ever
    ///   turns the quotes on; inside them the byte appears as itself. That is
    ///   why the three are spelled here as an `allow_bare` argument rather
    ///   than as a rule inside the renderer, and why [`quotef`] can express
    ///   `quotearg_style_colon` the same way.
    /// - **Escaping styles** ([`Style::C`], [`Style::Escape`],
    ///   [`Style::Locale`], [`Style::Clocale`]) put a `\` before the byte and
    ///   leave everything else alone.
    /// - **[`Style::Literal`], [`Style::ShellAlways`] and
    ///   [`Style::ShellEscapeAlways`] ignore it entirely.** The first two
    ///   because neither escapes nor elides; the third because the switch
    ///   rewrites `quoting_style` to `shell_always` before the guard reads it,
    ///   and its quotes are already on. `ls` passes the set to all ten; for
    ///   these three it is measurably a no-op.
    ///
    /// ```
    /// use coreutils::quote::Style;
    /// // Eliding: the set forces the quotes on, and is gone inside them.
    /// assert_eq!(Style::Shell.quote_with(b"a=b", b"="), b"'a=b'");
    /// assert_eq!(Style::CMaybe.quote_with(b"a=b", b"="), br#""a=b""#);
    /// // Escaping: the byte keeps a backslash.
    /// assert_eq!(Style::C.quote_with(b"a=b", b"="), br#""a\=b""#);
    /// assert_eq!(Style::Escape.quote_with(b"a=b", b"="), br"a\=b");
    /// // `ls -b`'s space is this and nothing else; the style itself has none.
    /// assert_eq!(Style::Escape.quote(b"a b"), b"a b");
    /// assert_eq!(Style::Escape.quote_with(b"a b", b" "), br"a\ b");
    /// // Ignored outright.
    /// assert_eq!(Style::Literal.quote_with(b"a=b", b"="), b"a=b");
    /// assert_eq!(Style::ShellEscapeAlways.quote_with(b"a=b", b"="), b"'a=b'");
    /// ```
    #[must_use]
    pub fn quote_with(self, text: &[u8], extra: &[u8]) -> Vec<u8> {
        // An ASCII byte is a whole character wherever it appears in valid
        // UTF-8, and a standalone byte where the text is not UTF-8 at all, so
        // scanning bytewise for an ASCII set cannot match half a character.
        let forced = || text.iter().any(|b| extra.contains(b));
        match self {
            Self::Literal => text.to_vec(),
            Self::Shell => shell_unescaped(text, !forced()),
            Self::ShellAlways => shell_unescaped(text, false),
            // Forcing here restarts as `shell_escape_always`, which is what
            // `render` with the quotes on already is.
            Self::ShellEscape => render(text, !forced()).into_bytes(),
            Self::ShellEscapeAlways => render(text, false).into_bytes(),
            Self::C => c_always(text, extra).into_bytes(),
            Self::CMaybe => c_maybe(text, extra).into_bytes(),
            Self::Escape => escape_style(text, extra).into_bytes(),
            Self::Locale | Self::Clocale => locale_quote(text, extra).into_bytes(),
        }
    }
}

/// The `shell` and `shell-always` styles: quoted, but never escaped.
///
/// The shape is [`render`]'s — bare if it can be, else `"..."` when the only
/// obstacle is a `'`, else `'...'` with each `'` spliced as `'\''` — with the
/// one difference that a byte which does not print is emitted **as itself**
/// rather than as a `$'...'` escape. That is why the result is bytes: a name
/// that was never text does not become text by being quoted.
///
/// Two consequences that look like bugs and are not, both measured:
///
/// * `\x01` alone comes back **bare**. Under this style an unprintable byte is
///   not a reason to quote; only a byte a *shell* gives meaning to is. See
///   [`shell_bare_ok`].
/// * There is no `''` prefix. gnulib's stray-quote wart (see [`shell_escape`])
///   is emitted beside a `$'...'`, and this style never writes one.
fn shell_unescaped(name: &[u8], allow_bare: bool) -> Vec<u8> {
    if allow_bare
        && !name.is_empty()
        && name
            .iter()
            .enumerate()
            .all(|(i, &b)| shell_bare_ok(name, i, b))
    {
        return name.to_vec();
    }
    // `all_bare` accepts only characters, so a `Some` here also proves `name`
    // is valid UTF-8 and its bytes are the rendering.
    if !name.is_empty() && name.contains(&b'\'') && all_bare(name, dq_ok).is_some() {
        let mut out = Vec::with_capacity(name.len().saturating_add(2));
        out.push(b'"');
        out.extend_from_slice(name);
        out.push(b'"');
        return out;
    }
    let mut out = Vec::with_capacity(name.len().saturating_add(2));
    out.push(b'\'');
    for &b in name {
        if b == b'\'' {
            // Close, escape the quote outside any quoting, reopen.
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

/// Whether byte `b` at offset `i` of `name` needs no quoting under
/// [`Style::Shell`].
///
/// Bytewise rather than piecewise, and that is the measured rule rather than a
/// simplification: under this style `a\u{0378}z`, `a\u{2028}z` and `a\xffz` all
/// come back bare, so no decoding happens and printability is never consulted.
/// The only bytes above the ASCII range that could matter are ones a shell
/// gives meaning to, and there are none.
///
/// Below the ASCII range the rule is the surprising half: **every** control
/// byte goes bare except `\t`, `\n` and `\r`, which are the three a shell
/// splits words on. `\a`, `\b`, `\v`, `\f` and `\x7f` do not, and are left
/// bare. Measured over all 255 usable byte values; `\0` is unreachable in a
/// path and follows the same branch.
fn shell_bare_ok(name: &[u8], i: usize, b: u8) -> bool {
    if b >= 0x80 || b == 0x7f {
        return true;
    }
    if b < 0x20 {
        return !matches!(b, b'\t' | b'\n' | b'\r');
    }
    if SAFE_UNLESS_ALONE.contains(&b) {
        return name.len() > 1;
    }
    SAFE.contains(&b) || (i > 0 && SAFE_NOT_FIRST.contains(&b))
}

/// The `escape` style, which `ls -b` selects: C escapes and no quotes at all.
///
/// The backslash that introduces an escape is the only printable byte escaped
/// for its own sake. `"` and `'` are left alone, which [`c_always`] does not
/// do for `"`: there, the double quote is the delimiter and must be escaped;
/// here there is no delimiter.
///
/// **A space is left bare**, which looks wrong and is what gnulib does. `ls
/// -b` prints `a\ b` only because `decode_switches` calls `set_char_quoting
/// (filename_quoting_options, ' ', 1)` for this style — so it is `extra`'s
/// job, not this function's. The distinction is measurable rather than
/// pedantic: `ls -b` gives the *directory header* its own options, with `:` in
/// the set and no space, and prints a directory called `d e` as `d e:` while
/// printing a file called `a b` as `a\ b`.
fn escape_style(text: &[u8], extra: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    for (at, p) in pieces(text) {
        match p {
            Piece::Char('\\', _) => out.push_str("\\\\"),
            Piece::Char('\0', _) => nul_escape(text, at, &mut out),
            Piece::Char(c, _) if printable_char(c) => push_maybe_escaped(c, extra, &mut out),
            other => escape_piece(other, &mut out),
        }
    }
    out
}

/// gnulib's `\0` shorthand, and the padding that keeps it unambiguous.
///
/// `\0` then a literal `7` would read back as the single byte `\07`, so a NUL
/// followed by a digit is written `\000` instead. (A path cannot hold a NUL;
/// this is reachable only through the byte-slice API, and is here because the
/// two callers must agree.)
fn nul_escape(text: &[u8], at: usize, out: &mut String) {
    out.push_str("\\0");
    if text
        .get(at.saturating_add(1))
        .is_some_and(u8::is_ascii_digit)
    {
        out.push_str("00");
    }
}

/// The bytes behind an `OsStr`.
///
/// On the target this is the string itself — a path is bytes there. On a
/// Windows *host* it is a lossy conversion, because there is no byte view of a
/// `OsStr` that round-trips; that only affects developing and testing on
/// Windows, never a running SlateOS.
#[cfg(unix)]
#[must_use]
pub fn os_bytes(s: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(s.as_bytes())
}

#[cfg(not(unix))]
#[must_use]
pub fn os_bytes(s: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    match s.to_string_lossy() {
        std::borrow::Cow::Borrowed(t) => std::borrow::Cow::Borrowed(t.as_bytes()),
        std::borrow::Cow::Owned(t) => std::borrow::Cow::Owned(t.into_bytes()),
    }
}

/// [`os_bytes`]'s inverse: a name that has been taken apart as bytes, put back
/// together as something a syscall will accept.
///
/// A utility that does path *arithmetic* — `rmdir -p` walking to a parent,
/// `dirname`, `basename` — has to cut the name somewhere, and the only place it
/// may cut is a byte boundary, because on this OS a path is bytes and `/` is
/// the one separator (`design.txt`). Cutting `Path::parent`-wise instead is not
/// a stylistic difference: measured, `rmdir -p ./cc` removes `cc` and then
/// tries to remove `.`, which `Path::parent` would never produce.
///
/// Round-tripping through this pair is exact on the target, where an `OsStr`
/// *is* its bytes. On a Windows host it is not — [`os_bytes`] is lossy there —
/// so a name the host cannot represent comes back with replacement characters.
/// That only affects developing and testing on Windows, and it is the same
/// limitation [`os_bytes`] already documents; the alternative is having no byte
/// view at all, which would mean no path arithmetic that is correct on the
/// target.
#[cfg(unix)]
#[must_use]
pub fn os_from_bytes(b: &[u8]) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(b.to_vec())
}

#[cfg(not(unix))]
#[must_use]
pub fn os_from_bytes(b: &[u8]) -> std::ffi::OsString {
    // The bytes a non-unix `os_bytes` hands out came from a `String`, so they
    // are valid UTF-8 and this second conversion loses nothing further.
    std::ffi::OsString::from(String::from_utf8_lossy(b).into_owned())
}

/// [`quotef`] for a path, a `String`, or anything else a call site already
/// holds — which is the form nearly every caller wants.
///
/// The bound is `AsRef<OsStr>` rather than `&OsStr` because the call sites are
/// a mix of `&str`, `String`, `&Path` and `&OsStr`, and requiring each of them
/// to convert first would put a conversion in front of every diagnostic in the
/// tree. A quoting call has to be the path of least resistance or it will be
/// skipped, and a skipped one is the bug this module exists to prevent.
///
/// ```
/// use coreutils::quote::quotef_os;
/// use std::path::Path;
/// assert_eq!(quotef_os("a b"), "'a b'");
/// assert_eq!(quotef_os(Path::new("notes.txt")), "notes.txt");
/// ```
#[must_use]
pub fn quotef_os<S: AsRef<std::ffi::OsStr>>(s: S) -> String {
    quotef(&os_bytes(s.as_ref()))
}

/// [`quoteaf`] for a path, a `String`, or anything else a call site holds.
///
/// ```
/// use coreutils::quote::quoteaf_os;
/// assert_eq!(quoteaf_os("a.txt"), "'a.txt'");
/// ```
#[must_use]
pub fn quoteaf_os<S: AsRef<std::ffi::OsStr>>(s: S) -> String {
    quoteaf(&os_bytes(s.as_ref()))
}

/// [`quote`] for a path, a `String`, or anything else a call site holds.
///
/// ```
/// use coreutils::quote::quote_os;
/// assert_eq!(quote_os("--sort"), "‘--sort’");
/// ```
#[must_use]
pub fn quote_os<S: AsRef<std::ffi::OsStr>>(s: S) -> String {
    quote(&os_bytes(s.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_left_alone() {
        for name in [
            &b"notes.txt"[..],
            b"Makefile",
            b"a-b_c.d",
            b"/usr/local/bin",
            b"100%",
            b"a#b",
            b"a~b",
            b"a{b}c",
            b"-",
            b"--",
        ] {
            assert_eq!(quotef(name), String::from_utf8_lossy(name), "{name:?}");
        }
    }

    #[test]
    fn shell_metacharacters_force_quotes() {
        assert_eq!(quotef(b"a b"), "'a b'");
        assert_eq!(quotef(b"a*b"), "'a*b'");
        assert_eq!(quotef(b"a|b"), "'a|b'");
        assert_eq!(quotef(b"a;b"), "'a;b'");
        assert_eq!(quotef(b"a$b"), "'a$b'");
        assert_eq!(quotef(b"a:b"), "'a:b'");
        assert_eq!(quotef(b"a?b"), "'a?b'");
        // Only at the front, where the shell reads them.
        assert_eq!(quotef(b"#a"), "'#a'");
        assert_eq!(quotef(b"~a"), "'~a'");
        // A lone brace is a bash reserved word; one inside a word is not.
        assert_eq!(quotef(b"{"), "'{'");
        assert_eq!(quotef(b"}"), "'}'");
        assert_eq!(quotef(b"a{"), "a{");
    }

    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        // This is the reason the module exists. Neither rendering contains a
        // newline, so neither can appear to be two messages.
        let forged = b"x\ncp: /etc/shadow: Permission denied";
        assert!(!quotef(forged).contains('\n'));
        assert!(!quote(forged).contains('\n'));
        assert_eq!(
            quotef(b"a\nb"),
            r"'a'$'\n''b'",
            "a newline becomes a visible escape"
        );
    }

    /// Whether a byte is printable ASCII.
    ///
    /// This is *not* the module's rule — that one is [`printable_char`] and is
    /// about characters. It is the property several tests below assert of a
    /// whole rendering, in the one case where the two coincide: an input built
    /// from single bytes, in which every high byte decodes to nothing and so
    /// comes back as octal.
    const fn printable_ascii(b: u8) -> bool {
        b >= 0x20 && b < 0x7f
    }

    /// The shapes the byte sweeps below use. None of them can put two high
    /// bytes together, so no input they build is ever valid UTF-8 above ASCII
    /// — which is exactly what makes "the rendering is ASCII" the right
    /// assertion for them, and why the multi-byte cases live in the fixture
    /// (`tests/quotearg.rs`) rather than here.
    fn byte_shapes(b: u8) -> [Vec<u8>; 3] {
        [vec![b], vec![b'a', b, b'z'], vec![b, b'\'']]
    }

    #[test]
    fn every_rendering_of_a_lone_byte_is_printable_ascii() {
        // `quote` is deliberately absent: since §351 its delimiters are U+2018
        // and U+2019, so it is the one style that is *not* ASCII. Its contents
        // still are, which is what the next test checks — the distinction
        // matters, because "printable ASCII" is what makes a rendering safe to
        // paste into a terminal, and only the marks give that up.
        for b in 0u8..=255 {
            for shape in byte_shapes(b) {
                for rendered in [quotef(&shape), quoteaf(&shape), quote_glibc(&shape)] {
                    assert!(
                        rendered.bytes().all(printable_ascii),
                        "{b:#04x} rendered as {rendered:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn quotes_contents_are_printable_ascii_even_though_its_marks_are_not() {
        // The marks are the *only* non-ASCII this module emits for an input
        // that held no character of its own. Strip them and what is left must
        // still be safe, or an unprintable byte could reach the terminal
        // between them.
        for b in 0u8..=255 {
            for shape in byte_shapes(b) {
                let rendered = quote(&shape);
                let inner = rendered
                    .strip_prefix(LEFT_QUOTE)
                    .and_then(|t| t.strip_suffix(RIGHT_QUOTE))
                    .unwrap_or_else(|| panic!("{b:#04x} lost its marks: {rendered:?}"));
                assert!(
                    inner.bytes().all(printable_ascii),
                    "{b:#04x} rendered as {rendered:?}"
                );
            }
        }
    }

    #[test]
    fn single_quotes_pick_the_double_quoted_form_where_gnu_does() {
        assert_eq!(quotef(b"it's"), "\"it's\"");
        assert_eq!(quotef(b"'"), "\"'\"");
        assert_eq!(quotef(b"~a'z"), "\"~a'z\"");
        // ...but not once something appears that a double quote would not
        // protect.
        assert_eq!(quotef(b"a'z$"), r"'a'\''z$'");
        assert_eq!(quotef(b"a'z~"), r"'a'\''z~'");
        assert_eq!(quotef(b"a'z\""), "'a'\\''z\"'");
    }

    #[test]
    fn unprintable_runs_share_one_escape() {
        assert_eq!(quotef(b"a\n\tz"), r"'a'$'\n\t''z'");
        assert_eq!(quotef(b"\n"), r"''$'\n'");
        assert_eq!(quotef(b"a\xffz"), r"'a'$'\377''z'");
    }

    #[test]
    fn octal_escapes_are_always_three_digits() {
        // \1 followed by a literal 2 would read back as \12, a different byte.
        assert_eq!(quote(b"\x01" as &[u8]), "\u{2018}\\001\u{2019}");
        // Ten bytes, not six: four for the escape, three for each curly mark.
        assert_eq!(quote(b"\x01" as &[u8]).len(), 10);
        assert_eq!(quote_glibc(b"\x01" as &[u8]), r"'\001'");
        assert_eq!(quote_glibc(b"\x01" as &[u8]).len(), 6);
        assert_eq!(quotef(b"\x012"), r"''$'\001''2'");
    }

    #[test]
    fn the_empty_string_is_a_pair_of_quotes_in_every_style() {
        assert_eq!(quotef(b""), "''");
        assert_eq!(quoteaf(b""), "''");
        assert_eq!(quote_glibc(b""), "''");
        // The one style whose pair is not two ASCII bytes.
        assert_eq!(quote(b""), "\u{2018}\u{2019}");
    }

    #[test]
    fn c_maybe_leaves_bare_what_a_shell_style_would_not() {
        // The whole point of the style: it renders a C literal, so the bytes
        // that matter to a *shell* are irrelevant to it. Contrast with
        // `quotef`, which quotes every one of these.
        for text in [&b"a b"[..], b"a'b", b"a$b", b"a`b", b"a|b", b"a*b", b"a\\b"] {
            assert_eq!(
                quote_c_maybe(text),
                String::from_utf8_lossy(text),
                "{text:?}"
            );
            assert_ne!(quotef(text), String::from_utf8_lossy(text), "{text:?}");
        }
    }

    #[test]
    fn c_maybe_forces_quotes_for_exactly_the_unrenderable_bytes() {
        // Measured set, from `scripts/c-maybe-probe.py`: a `"` (which would
        // close the literal) and anything unprintable. A *lone* byte above
        // 0x7f decodes to no character at all, so the old ASCII-shaped
        // expectation is still the right one here — see
        // `c_maybe_leaves_a_character_bare_but_not_a_stray_byte` for the case
        // where the two rules part company.
        for b in 1u8..=255 {
            let forced = quote_c_maybe(&[b]).starts_with('"');
            let expected = b == b'"' || !(0x20..0x7f).contains(&b);
            assert_eq!(forced, expected, "byte {b:#04x} -> {}", quote_c_maybe(&[b]));
        }
    }

    /// Measured: `LC_ALL=C.UTF-8 ls -1 --quoting-style=c-maybe` on real files
    /// with these names, GNU coreutils 9.4.
    ///
    /// There is no fixture for this style — `quote-probe.py` drives `sort` and
    /// `head`, neither of which offers it — so the expectations are written out
    /// with the command that produced them beside them.
    #[test]
    fn c_maybe_leaves_a_character_bare_but_not_a_stray_byte() {
        // Bare: the character prints, so a C literal can hold it as itself.
        assert_eq!(quote_c_maybe("aéz".as_bytes()), "aéz");
        assert_eq!(quote_c_maybe("é".as_bytes()), "é");
        assert_eq!(quote_c_maybe("βé".as_bytes()), "βé");
        // Quoted: one octal escape per byte of the encoding, not per character.
        assert_eq!(quote_c_maybe("\u{80}".as_bytes()), r#""\302\200""#);
        assert_eq!(quote_c_maybe("\u{2028}".as_bytes()), r#""\342\200\250""#);
        assert_eq!(quote_c_maybe(b"\xff"), r#""\377""#);
        // A byte that decodes to nothing drags the whole name into quotes,
        // where the characters around it are still themselves.
        assert_eq!(quote_c_maybe("é".as_bytes()), "é");
        assert_eq!(quote_c_maybe(b"\xc3"), r#""\303""#);
        assert_eq!(
            quote_c_maybe("a\u{2028}é".as_bytes()),
            "\"a\\342\\200\\250é\""
        );
    }

    /// The printability rule, against glibc's `iswprint` under `C.UTF-8`.
    ///
    /// Every character here was measured — the table was enumerated out of
    /// glibc 2.39 — and each is a different reason to be near the boundary:
    /// a letter, a combining mark, a no-break space, a soft hyphen, a
    /// zero-width space, a byte-order mark, a private-use code point, a CJK
    /// ideograph, the C1 range, DEL, and the two separators.
    #[test]
    fn printable_char_agrees_with_glibc_except_where_it_means_to() {
        for c in [
            '\u{00a0}', '\u{00ad}', '\u{00e9}', '\u{0301}', '\u{200b}', '\u{feff}', '\u{e000}',
            '\u{4e00}', 'a', ' ',
        ] {
            assert!(printable_char(c), "U+{:04X} should print", u32::from(c));
        }
        for c in [
            '\u{007f}', '\u{0080}', '\u{009f}', '\u{2028}', '\u{2029}', '\0', '\n',
        ] {
            assert!(!printable_char(c), "U+{:04X} should not", u32::from(c));
        }
        // The one deliberate divergence: unassigned (`Cn`). glibc escapes it,
        // we print it, because the alternative is a Unicode assignment table
        // that changes what a diagnostic looks like every release. See §357,
        // and `tests/quotearg.rs`, which asserts this keeps differing.
        assert!(printable_char('\u{0378}'));
    }

    #[test]
    fn a_character_survives_every_style_that_can_carry_one() {
        // The bug this replaced: every one of these came back as octal.
        assert_eq!(quotef("café.txt".as_bytes()), "café.txt");
        assert_eq!(quoteaf("café.txt".as_bytes()), "'café.txt'");
        assert_eq!(quote("café.txt".as_bytes()), "\u{2018}café.txt\u{2019}");
        assert_eq!(quote_glibc("--café".as_bytes()), "'--café'");
        assert_eq!(quote_c_maybe("café.txt".as_bytes()), "café.txt");
        // ...and the closing mark, which `quote` must escape or lose track of
        // where its own quoting ends.
        assert_eq!(
            quote("a\u{2019}b".as_bytes()),
            "\u{2018}a\\\u{2019}b\u{2019}"
        );
        // The opening one is not escaped: nothing is looking for one.
        assert_eq!(quote("a\u{2018}b".as_bytes()), "\u{2018}a\u{2018}b\u{2019}");
    }

    #[test]
    fn a_separator_cannot_forge_a_line_either() {
        // U+2028 is not a `Cc` control, so a rule that only escaped controls
        // would let it through — and a terminal, a log reader and most
        // `2>&1 | grep` pipelines all treat it as ending a line. That is the
        // module's whole premise arriving in a character rather than a byte.
        for text in ["a\u{2028}b", "a\u{2029}b"] {
            let name = text.as_bytes();
            assert!(!quotef(name).contains(text), "{text:?} survived quotef");
            assert!(!quote(name).contains(text), "{text:?} survived quote");
            assert!(
                !quote_c_maybe(name).contains(text),
                "{text:?} survived c-maybe"
            );
        }
        assert_eq!(quotef("a\u{2028}b".as_bytes()), r"'a'$'\342\200\250''b'");
    }

    #[test]
    fn an_undecodable_byte_beside_a_character_escapes_only_itself() {
        // The decoder's real job: `\xff` is not part of `é`, and `\xc3` is not
        // the start of one when a `z` follows it.
        assert_eq!(
            quotef(
                "aéb"
                    .as_bytes()
                    .iter()
                    .chain(b"\xffc")
                    .copied()
                    .collect::<Vec<_>>()
                    .as_slice()
            ),
            r"'aéb'$'\377''c'"
        );
        assert_eq!(quotef(b"\xc3z"), r"''$'\303''z'");
        assert_eq!(quotef(b"\xc3"), r"''$'\303'");
        // Overlong, surrogate and out-of-range sequences decode to nothing, so
        // every byte of them is escaped separately — but they still share one
        // `$'...'`, because the run is a run of *unprintable pieces*.
        assert_eq!(quotef(b"\xc0\xaf"), r"''$'\300\257'");
        assert_eq!(quotef(b"\xed\xa0\x80"), r"''$'\355\240\200'");
        assert_eq!(quotef(b"\xf4\x90\x80\x80"), r"''$'\364\220\200\200'");
    }

    #[test]
    fn c_maybe_handles_the_inputs_no_gnu_oracle_could_be_given() {
        // NOT measured: an `argv` string cannot hold a NUL and a file name can
        // hold neither a NUL nor a `/`, so neither `paste` nor `ls` can be
        // asked about these. They come from reading `quotearg.c`, and are
        // written down here so that is visible rather than assumed.
        //
        // The empty string elides its quotes: gnulib's "is the result empty?"
        // retry is guarded by `quoting_style == shell_always_quoting_style`,
        // so it does not fire for this style.
        assert_eq!(quote_c_maybe(b""), "");
        assert_eq!(quote_c_maybe_colon(b""), "");
        // A NUL is unprintable, so it forces the quotes; inside them it is
        // `\0` — padded to `\000` before a digit, because `\0` then `7` would
        // read back as the one byte `\07`.
        assert_eq!(quote_c_maybe(b"a\0b"), r#""a\0b""#);
        assert_eq!(quote_c_maybe(b"a\0" as &[u8]), r#""a\0""#);
        assert_eq!(quote_c_maybe(b"a\x007b"), r#""a\0007b""#);
        // Which is *not* the rule the shell styles use: `quote` pads every
        // octal escape to three digits unconditionally.
        assert_eq!(quote(b"a\0" as &[u8]), "\u{2018}a\\000\u{2019}");
        // A `/` is nothing special to a C literal.
        assert_eq!(quote_c_maybe(b"a/b"), "a/b");
        assert_eq!(quote_c_maybe(b"a/\tb"), r#""a/\tb""#);
    }

    #[test]
    fn c_maybe_escapes_everything_once_the_quotes_are_on() {
        // The backslash is the surprise: bare a moment ago, escaped now.
        assert_eq!(quote_c_maybe(b"a b\\"), "a b\\");
        assert_eq!(quote_c_maybe(b"a\tb\\"), r#""a\tb\\""#);
        assert_eq!(
            quote_c_maybe(b"\x07\x08\x09\x0a\x0b\x0c\x0d"),
            r#""\a\b\t\n\v\f\r""#
        );
        assert_eq!(quote_c_maybe(b"\xff"), r#""\377""#);
        assert_eq!(quote_c_maybe(b"\x7f"), r#""\177""#);
        assert_eq!(quote_c_maybe(b"a\"b"), "\"a\\\"b\"");
    }

    /// The two families, side by side, in the shapes GNU actually prints.
    ///
    /// Every expectation here was measured under `LC_ALL=C.UTF-8` against real
    /// GNU coreutils, not recalled — the command that produced each is in the
    /// comment beside it. This test is the one that would have caught the
    /// original bug: before §351 both families rendered identically, so a call
    /// site using the wrong one was invisible until `quote` went curly.
    #[test]
    fn the_two_families_disagree_about_their_marks() {
        // gnulib — follows the locale's character set, so curly here.
        // $ sort --sort=zzz   -> invalid argument ‘zzz’ for ‘--sort’
        assert_eq!(quote(b"zzz"), "\u{2018}zzz\u{2019}");
        // $ head -c zzz       -> invalid number of bytes: ‘zzz’
        assert_eq!(quote(b"--sort"), "\u{2018}--sort\u{2019}");

        // glibc — spells its quotes into the C format string, so straight,
        // always, in every locale.
        // $ wc -Z             -> invalid option -- 'Z'
        assert_eq!(quote_glibc(b"Z"), "'Z'");
        // $ sort --key        -> option '--key' requires an argument
        assert_eq!(quote_glibc(b"--key"), "'--key'");
        // $ sort --r          -> ... possibilities: '--random-sort' ...
        assert_eq!(quote_glibc(b"--random-sort"), "'--random-sort'");

        // The two never agree on a mark, which is the whole distinction.
        for text in [&b"x"[..], b"--key", b"", b"a b"] {
            assert_ne!(quote(text), quote_glibc(text), "{text:?}");
        }
    }

    /// Where `quote_glibc` knowingly departs from glibc, and why.
    ///
    /// glibc escapes nothing at all, which is not a style we can copy: it is a
    /// live line-forging bug. Measured, `wc --no$'\n'pe` really does print two
    /// lines, the second of which the user chose.
    #[test]
    fn quote_glibc_escapes_where_glibc_would_forge_a_line() {
        // The attack glibc is open to. Ours cannot span lines.
        let forged = b"--no\npe";
        assert_eq!(quote_glibc(forged), r"'--no\npe'");
        assert!(!quote_glibc(forged).contains('\n'));
        assert_eq!(quote_glibc(forged).lines().count(), 1);

        // glibc prints `invalid option -- '''` for a quote, where the marks
        // cannot be told from the content. Ours escapes it, because here the
        // `'` *is* the delimiter.
        assert_eq!(quote_glibc(b"'"), r"'\''");
        // Which is the opposite of `quote`, whose delimiter is `’` — there a
        // bare `'` is unambiguous, so GNU leaves it alone and so do we.
        assert_eq!(quote(b"'"), "\u{2018}'\u{2019}");

        // For every ordinary option name the two agree byte for byte, and that
        // is the case the comparison against GNU actually rests on.
        for name in [&b"x"[..], b"--key", b"--random-sort", b"Z", b"0"] {
            let rendered = quote_glibc(name);
            assert_eq!(
                rendered,
                format!("'{}'", String::from_utf8_lossy(name)),
                "{name:?} should need no escaping"
            );
        }
    }

    #[test]
    fn quoteaf_differs_from_quotef_only_where_quotef_would_go_bare() {
        // The one case: a name that needs nothing.
        assert_eq!(quotef(b"notes.txt"), "notes.txt");
        assert_eq!(quoteaf(b"notes.txt"), "'notes.txt'");
        // Everywhere else the two agree, because once a name needs quoting at
        // all there is no "outer quote" left to elide.
        for name in [
            &b"a b"[..],
            b"it's",
            b"a\nb",
            b"a'z$",
            b"#a",
            b"{",
            b"a\x01'z",
            b"",
        ] {
            assert_eq!(quotef(name), quoteaf(name), "{name:?}");
        }
    }
}
