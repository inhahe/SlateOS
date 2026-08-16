//! Byte-string splitting helpers — the `str` methods that `[u8]` lacks.
//!
//! ## Why this exists
//!
//! `CLAUDE.md`: *"Never force UTF-8 on filesystem paths, environment
//! variables, or pipe data."* [`crate::fs::path`] already moved the VFS to
//! byte paths, but `kernel/src/kshell.rs` still holds the command line as a
//! `String` and parses it with `str` methods, so a file named `re\xffport.txt`
//! is listable but cannot be typed or tab-completed. See `known-issues.md` §
//! `TD-KSHELL-LINE-EDITOR-IS-UTF8`.
//!
//! Converting that parser is blocked on a small gap: of the ~1520 `str`-method
//! call sites reachable from `kshell::execute`, **631** (`trim*`,
//! `starts_with`, `ends_with`, `strip_prefix`) need no new code at all —
//! `[u8]` already has them — and the rest reduce to three missing operations,
//! which is what this module supplies:
//!
//! | `str` method | sites | here |
//! |---|---:|---|
//! | `split_whitespace` / `split_ascii_whitespace` | 552 | [`ByteStrExt::split_ascii_whitespace`] |
//! | `splitn` | 51 | [`ByteStrExt::splitn_byte`] |
//! | `split_once` | 15 | [`ByteStrExt::split_once_byte`] / [`ByteStrExt::split_once_str`] |
//!
//! ## An extension trait, not a newtype
//!
//! A newtype would force wrap/unwrap noise across all ~1500 sites while
//! buying no invariant that `[u8]` does not already provide — there *is* no
//! invariant here, which is the entire point: any byte sequence is legal. The
//! trait is `impl`'d directly on `[u8]` so `Vec<u8>`, `&[u8]` and
//! [`crate::fs::path::Path::as_bytes`] all get it by deref.
//!
//! Written in-house rather than pulling in `bstr`: the kernel is `no_std` on a
//! custom target, and `fs/path.rs` already shows the in-house pattern carries
//! its weight.
//!
//! ## Semantics are `str`'s, deliberately
//!
//! Every method here matches its `str` counterpart *exactly*, including the
//! awkward edges — `splitn(0, …)` yields nothing, `splitn(1, …)` yields the
//! whole input unsplit, and `split_ascii_whitespace` never yields an empty
//! slice. That is not gold-plating. The conversion this module exists to
//! enable is a ~1500-site mechanical diff, and a mechanical diff is only safe
//! if the replacement behaves identically at every site; a helper that
//! differed from `str` in one edge case would plant a bug at whichever of
//! those sites happened to hit it, with nothing in the diff to show for it.
//! [`self_test`] pins each edge down against the documented `str` behaviour.
//!
//! One name is chosen for the same reason: [`ByteStrExt::split_ascii_whitespace`]
//! keeps `str`'s exact spelling, so all 552 of those call sites need **no
//! edit at all** once the buffer type changes. The two that take a delimiter
//! are suffixed (`_byte` / `_str`) instead of overloading a `Pattern`
//! equivalent, because a `Pattern` trait would be far more machinery than
//! three call shapes justify — and because a bare `split_once` on `[u8]`
//! risks silently resolving to a future inherent slice method, where an
//! inherent method would win over this trait and change behaviour with no
//! compile error.

#![allow(dead_code)]

// ---------------------------------------------------------------------------
// Iterators
// ---------------------------------------------------------------------------

/// Iterator over ASCII-whitespace-separated runs. See
/// [`ByteStrExt::split_ascii_whitespace`].
#[derive(Clone, Debug)]
pub struct SplitAsciiWhitespace<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for SplitAsciiWhitespace<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        // Skip any leading separator run. When nothing but separators remain,
        // `position` returns None and the iterator is exhausted — which is
        // what makes trailing whitespace yield no final empty item.
        let start = self.rest.iter().position(|b| !b.is_ascii_whitespace())?;
        let rest = self.rest.get(start..)?;
        let end = rest
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(rest.len());
        let word = rest.get(..end)?;
        self.rest = rest.get(end..).unwrap_or(&[]);
        Some(word)
    }
}

/// Iterator over at most `n` delimiter-separated fields. See
/// [`ByteStrExt::splitn_byte`].
#[derive(Clone, Debug)]
pub struct SplitNByte<'a> {
    /// `None` once exhausted. Distinct from `Some(&[])`, which is a real
    /// empty final field (`"a,".splitn(2, ',')` yields `"a"` then `""`).
    rest: Option<&'a [u8]>,
    delim: u8,
    /// Fields still allowed to be produced.
    remaining: usize,
}

impl<'a> Iterator for SplitNByte<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let rest = self.rest?;
        // On the last permitted field, stop splitting and hand back whatever
        // is left, delimiters and all — this is the property callers rely on
        // for "split off the command, keep the argument string intact".
        if self.remaining <= 1 {
            self.rest = None;
            return if self.remaining == 1 { Some(rest) } else { None };
        }
        match rest.iter().position(|&b| b == self.delim) {
            Some(idx) => {
                let head = rest.get(..idx)?;
                self.rest = Some(rest.get(idx.saturating_add(1)..).unwrap_or(&[]));
                self.remaining = self.remaining.saturating_sub(1);
                Some(head)
            }
            None => {
                self.rest = None;
                self.remaining = 0;
                Some(rest)
            }
        }
    }
}

/// Iterator over every delimiter-separated field. See
/// [`ByteStrExt::split_byte`].
#[derive(Clone, Debug)]
pub struct SplitByte<'a> {
    rest: Option<&'a [u8]>,
    delim: u8,
}

impl<'a> Iterator for SplitByte<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let rest = self.rest?;
        match rest.iter().position(|&b| b == self.delim) {
            Some(idx) => {
                let head = rest.get(..idx)?;
                self.rest = Some(rest.get(idx.saturating_add(1)..).unwrap_or(&[]));
                Some(head)
            }
            None => {
                self.rest = None;
                Some(rest)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The extension trait
// ---------------------------------------------------------------------------

/// The `str` splitting methods that `[u8]` does not have.
///
/// See the module docs for why this is a trait on `[u8]` rather than a
/// newtype, and why the semantics track `str` exactly.
pub trait ByteStrExt {
    /// Split on runs of ASCII whitespace, skipping leading and trailing runs.
    ///
    /// Never yields an empty slice, exactly like
    /// `str::split_ascii_whitespace`. Note this differs from
    /// [`ByteStrExt::split_byte`], which *does* yield empties.
    fn split_ascii_whitespace(&self) -> SplitAsciiWhitespace<'_>;

    /// Split on every occurrence of `delim`, like `str::split(char)`.
    ///
    /// Yields empty slices for adjacent, leading or trailing delimiters.
    fn split_byte(&self, delim: u8) -> SplitByte<'_>;

    /// Split into at most `n` fields, like `str::splitn`.
    ///
    /// `n == 0` yields nothing at all; the `n`th field is the unsplit
    /// remainder. Named `_byte` rather than shadowing `slice::splitn`, whose
    /// signature takes a *predicate* and whose semantics differ.
    fn splitn_byte(&self, n: usize, delim: u8) -> SplitNByte<'_>;

    /// Split at the first `delim`, like `str::split_once(char)`.
    ///
    /// The delimiter appears in neither half. `None` if it does not occur.
    fn split_once_byte(&self, delim: u8) -> Option<(&[u8], &[u8])>;

    /// Split at the last `delim`, like `str::rsplit_once(char)`.
    fn rsplit_once_byte(&self, delim: u8) -> Option<(&[u8], &[u8])>;

    /// Split at the first occurrence of `delim`, like
    /// `str::split_once(&str)`.
    fn split_once_str(&self, delim: &[u8]) -> Option<(&[u8], &[u8])>;

    /// Byte offset of the first occurrence of `needle`, like `str::find`.
    ///
    /// An empty needle matches at 0, matching `str::find("")`.
    fn find_str(&self, needle: &[u8]) -> Option<usize>;

    /// Byte offset of the first occurrence of `byte`.
    fn find_byte(&self, byte: u8) -> Option<usize>;
}

impl ByteStrExt for [u8] {
    fn split_ascii_whitespace(&self) -> SplitAsciiWhitespace<'_> {
        SplitAsciiWhitespace { rest: self }
    }

    fn split_byte(&self, delim: u8) -> SplitByte<'_> {
        SplitByte { rest: Some(self), delim }
    }

    fn splitn_byte(&self, n: usize, delim: u8) -> SplitNByte<'_> {
        SplitNByte { rest: Some(self), delim, remaining: n }
    }

    fn split_once_byte(&self, delim: u8) -> Option<(&[u8], &[u8])> {
        let idx = self.iter().position(|&b| b == delim)?;
        Some((self.get(..idx)?, self.get(idx.saturating_add(1)..)?))
    }

    fn rsplit_once_byte(&self, delim: u8) -> Option<(&[u8], &[u8])> {
        let idx = self.iter().rposition(|&b| b == delim)?;
        Some((self.get(..idx)?, self.get(idx.saturating_add(1)..)?))
    }

    fn split_once_str(&self, delim: &[u8]) -> Option<(&[u8], &[u8])> {
        let idx = self.find_str(delim)?;
        Some((
            self.get(..idx)?,
            self.get(idx.saturating_add(delim.len())..)?,
        ))
    }

    fn find_str(&self, needle: &[u8]) -> Option<usize> {
        // `windows(0)` panics, and an empty needle matching at 0 is what
        // `str::find("")` does, so this guard is semantics as well as safety.
        if needle.is_empty() {
            return Some(0);
        }
        if needle.len() > self.len() {
            return None;
        }
        self.windows(needle.len()).position(|w| w == needle)
    }

    fn find_byte(&self, byte: u8) -> Option<usize> {
        self.iter().position(|&b| b == byte)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise every documented edge. Wired into the boot battery in `main.rs`,
/// because the kernel binary sets `test = false` and so has no host
/// `cargo test` to run these under.
///
/// The assertions are written as "this is what `str` does", because that
/// equivalence — not the implementation — is the contract the ~1500-site
/// kshell conversion will lean on.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;
    use alloc::vec::Vec;

    serial_println!("  bytestr::self_test 1: split_ascii_whitespace");
    let words: Vec<&[u8]> = b"  a  bb \t c \n".split_ascii_whitespace().collect();
    assert_eq!(words, alloc::vec![&b"a"[..], &b"bb"[..], &b"c"[..]]);
    // Never an empty item, and an all-whitespace or empty input yields none.
    assert_eq!(b"".split_ascii_whitespace().count(), 0);
    assert_eq!(b" \t\r\n".split_ascii_whitespace().count(), 0);
    assert_eq!(b"solo".split_ascii_whitespace().count(), 1);
    // The case the module exists for: a non-UTF-8 byte is an ordinary
    // non-separator and must survive the split intact.
    let weird: Vec<&[u8]> = b"re\xffport a\x80b".split_ascii_whitespace().collect();
    assert_eq!(weird, alloc::vec![&b"re\xffport"[..], &b"a\x80b"[..]]);
    // 0x85 (NEL) and 0xA0 (NBSP) are Unicode whitespace but NOT ASCII
    // whitespace; `is_ascii_whitespace` must not treat them as separators.
    assert_eq!(b"a\x85b\xa0c".split_ascii_whitespace().count(), 1);

    serial_println!("  bytestr::self_test 2: split_byte keeps empties");
    let f: Vec<&[u8]> = b"a,,b".split_byte(b',').collect();
    assert_eq!(f, alloc::vec![&b"a"[..], &b""[..], &b"b"[..]]);
    // Leading and trailing delimiters produce empty fields, like `str::split`.
    let f: Vec<&[u8]> = b",a,".split_byte(b',').collect();
    assert_eq!(f, alloc::vec![&b""[..], &b"a"[..], &b""[..]]);
    // An empty input yields exactly one empty field, not zero.
    let f: Vec<&[u8]> = b"".split_byte(b',').collect();
    assert_eq!(f, alloc::vec![&b""[..]]);

    serial_println!("  bytestr::self_test 3: splitn_byte edges");
    // n == 0 yields nothing at all — the edge most likely to be got wrong.
    assert_eq!(b"a,b".splitn_byte(0, b',').count(), 0);
    // n == 1 yields the whole input, unsplit.
    let f: Vec<&[u8]> = b"a,b,c".splitn_byte(1, b',').collect();
    assert_eq!(f, alloc::vec![&b"a,b,c"[..]]);
    // The final field keeps its delimiters: this is the property that lets a
    // caller peel off a command word and leave the argument string intact.
    let f: Vec<&[u8]> = b"a,b,c".splitn_byte(2, b',').collect();
    assert_eq!(f, alloc::vec![&b"a"[..], &b"b,c"[..]]);
    // n larger than the field count is not an error; it just runs out.
    let f: Vec<&[u8]> = b"a,b".splitn_byte(9, b',').collect();
    assert_eq!(f, alloc::vec![&b"a"[..], &b"b"[..]]);
    // A trailing delimiter yields a real empty final field, distinct from
    // exhaustion — the reason `rest` is an Option rather than a slice.
    let f: Vec<&[u8]> = b"a,".splitn_byte(2, b',').collect();
    assert_eq!(f, alloc::vec![&b"a"[..], &b""[..]]);

    serial_println!("  bytestr::self_test 4: split_once / rsplit_once");
    assert_eq!(b"key=val=ue".split_once_byte(b'='), Some((&b"key"[..], &b"val=ue"[..])));
    assert_eq!(b"key=val=ue".rsplit_once_byte(b'='), Some((&b"key=val"[..], &b"ue"[..])));
    assert_eq!(b"nodelim".split_once_byte(b'='), None);
    // A delimiter at either end gives an empty half, not None.
    assert_eq!(b"=v".split_once_byte(b'='), Some((&b""[..], &b"v"[..])));
    assert_eq!(b"k=".split_once_byte(b'='), Some((&b"k"[..], &b""[..])));
    // Non-UTF-8 bytes are ordinary content on both sides of the split.
    assert_eq!(
        b"\xff=\x80".split_once_byte(b'='),
        Some((&b"\xff"[..], &b"\x80"[..]))
    );

    serial_println!("  bytestr::self_test 5: multi-byte delimiters and find");
    assert_eq!(
        b"a::b::c".split_once_str(b"::"),
        Some((&b"a"[..], &b"b::c"[..]))
    );
    assert_eq!(b"abc".find_str(b"bc"), Some(1));
    assert_eq!(b"abc".find_str(b"bd"), None);
    // A needle longer than the haystack must not panic in `windows`.
    assert_eq!(b"ab".find_str(b"abc"), None);
    assert_eq!(b"".find_str(b"a"), None);
    // Empty needle matches at 0, like `str::find("")`.
    assert_eq!(b"abc".find_str(b""), Some(0));
    assert_eq!(b"".find_str(b""), Some(0));
    assert_eq!(b"abc".find_byte(b'c'), Some(2));
    assert_eq!(b"abc".find_byte(b'z'), None);

    serial_println!("  bytestr::self_test PASSED");
    Ok(())
}
