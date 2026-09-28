//! The exclusion patterns: gitignore-shaped globs over a path's own bytes.
//!
//! Moved whole from `apps/backup` on 2026-09-27 with the rest of the store,
//! so the backup tool and System Restore exclude by one matcher.

use std::path::Path;

/// Matches a path component against a glob pattern.
///
/// Supports `*` (any run of characters except `/`), `?` (one character except
/// `/`), `**` (any number of path segments) and `[abc]` / `[a-z]` / `[!a-z]`
/// character classes.
///
/// # Character classes, and why they arrived late
///
/// Until 2026-09-13 a `[` here was an ordinary character, so `cache[1]/`
/// excluded a directory *literally named* `cache[1]`. That was recorded as a
/// deliberate dialect difference from the search tools, and it was wrong in a
/// way that only measuring showed: this program documents itself as reading
/// gitignore-shaped patterns, and real gitignore, `rsync --exclude`,
/// `tar --exclude` and our own `posix::fnmatch` all have classes. Backup was
/// not speaking a second dialect, it was missing a feature of the one it
/// claims. See `design-decisions.md` and C-Q9.
///
/// The change is not free: an exclude line someone already wrote as
/// `cache[1]/` now means "cache1" instead of "cache[1]", and a backup that
/// quietly starts including a directory it used to skip is not an error
/// anybody sees. That cost was weighed against a count -- zero patterns in
/// this tree's shipped defaults, fixtures or YAML use a bracket, and the
/// operator confirmed the same of their own rules -- so nothing that exists
/// changes meaning.
/// Text against text; the tests' way in. The program matches paths' own
/// bytes (`is_excluded`), with `glob_match_recursive` itself.
#[cfg(test)]
fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_match_recursive(pattern.as_bytes(), path.as_bytes())
}

/// The length in bytes of the UTF-8 character starting at `i`, or 1 if the byte
/// there does not begin a well-formed sequence.
///
/// Matching runs over bytes, which is right: our paths are byte strings and are
/// not required to be valid UTF-8. But `?` is documented as "single char except
/// /", and a character is not a byte. Advancing one byte per `?` meant
/// `?.txt` did not match `日.txt` — the `?` ate a third of the kanji and the
/// literal `.` was then compared against a continuation byte. `?` has no
/// backtracking (only `*` does), so the match simply failed.
///
/// In a backup tool that is not cosmetic. These patterns drive include and
/// exclude lists, so a pattern that silently fails to match either backs up a
/// file the user meant to exclude or, worse, skips one they believed was
/// covered — and they find out when they try to restore it.
///
/// Only `?` needs this. `*` is byte-greedy but can only *succeed* on a
/// character boundary, because UTF-8 is self-synchronising: the literal that
/// follows a `*` comes from a `&str`, so it is well-formed, and a well-formed
/// sequence can never match starting inside another one. `/` is likewise safe
/// to test byte-wise, as an ASCII byte cannot occur inside a multi-byte
/// character.
fn utf8_char_len(text: &[u8], i: usize) -> usize {
    let Some(&b) = text.get(i) else {
        return 1;
    };
    let want = if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        // A continuation byte or an invalid lead: not a character start, so
        // consume one byte and let the literal comparison decide.
        return 1;
    };
    // Only treat this as a multi-byte character if the sequence it announces is
    // actually there and well-formed. Clamping to what remains instead would be
    // wrong in a way that matters: for the bytes `[0xE6, b'/']` a lead byte
    // claiming three bytes would consume both, and `?` would have crossed a
    // separator — the one thing it must never do. Falling back to a single byte
    // for an ill-formed sequence keeps that invariant and still guarantees
    // forward progress.
    let follows_are_continuations = (1..want).all(|k| {
        text.get(i.saturating_add(k))
            .is_some_and(|&c| (0x80..=0xBF).contains(&c))
    });
    if follows_are_continuations { want } else { 1 }
}

/// The scalar value of the character starting at `i`, and its length in bytes.
///
/// Character classes compare *characters*, not bytes, for the reason `?` does
/// -- see [`utf8_char_len`]. A range is only meaningful over scalar values, so
/// both ends of `[a-z]` and the subject are decoded the same way.
///
/// A byte that does not begin a well-formed sequence decodes as itself, one
/// byte wide. That keeps the matcher total over the arbitrary bytes a path may
/// contain: such a byte simply fails to equal any well-formed class member,
/// which is the honest answer, and never consumes more than it should.
fn decode_char(text: &[u8], i: usize) -> (u32, usize) {
    let len = utf8_char_len(text, i);
    let fallback = || (u32::from(*text.get(i).unwrap_or(&0)), 1usize);
    let Some(chunk) = text.get(i..i.saturating_add(len)) else {
        return fallback();
    };
    match core::str::from_utf8(chunk) {
        Ok(valid) => valid
            .chars()
            .next()
            .map_or_else(fallback, |c| (u32::from(c), len)),
        Err(_) => fallback(),
    }
}

/// The index of the `]` that closes the class opening at `open`, if there is
/// one.
///
/// `None` means the `[` is an ordinary character. That is not leniency, it is
/// what `fnmatch(3)` and gitignore both do: an unterminated `[` is a literal,
/// so `file[1` matches a file called `file[1`. A backup exclude list is data a
/// user typed, and refusing the whole pattern -- or worse, silently matching
/// nothing -- would drop files from a backup over a typo.
///
/// Two positions are special and both are POSIX rules rather than inventions:
/// a `!` or `^` straight after the `[` negates the class, and a `]` in the
/// first member position is a literal `]` rather than the terminator, so
/// `[]]` is the class containing one right bracket.
fn class_end(pattern: &[u8], open: usize) -> Option<usize> {
    let mut i = open.checked_add(1)?;
    if matches!(pattern.get(i), Some(&b'!') | Some(&b'^')) {
        i = i.checked_add(1)?;
    }
    if pattern.get(i) == Some(&b']') {
        i = i.checked_add(1)?;
    }
    while let Some(&b) = pattern.get(i) {
        if b == b']' {
            return Some(i);
        }
        i = i.checked_add(1)?;
    }
    None
}

/// Whether the character at `text[ti..]` is a member of the class running from
/// `open` to `close` in `pattern`.
///
/// A `-` that has nothing after it inside the class is a literal `-`, so
/// `[a-]` is the two characters `a` and `-`. The separator is handled by the
/// caller, not here: no class matches `/`, which is gitignore's rule and the
/// one that keeps `[a-z]*` from reaching across a directory boundary.
fn class_matches(pattern: &[u8], open: usize, close: usize, text: &[u8], ti: usize) -> bool {
    let (subject, _) = decode_char(text, ti);
    let mut i = open.saturating_add(1);
    let negated = matches!(pattern.get(i), Some(&b'!') | Some(&b'^'));
    if negated {
        i = i.saturating_add(1);
    }
    let mut hit = false;
    while i < close {
        let (low, low_len) = decode_char(pattern, i);
        let after_low = i.saturating_add(low_len);
        let is_range = pattern.get(after_low) == Some(&b'-') && after_low.saturating_add(1) < close;
        if is_range {
            let high_at = after_low.saturating_add(1);
            let (high, high_len) = decode_char(pattern, high_at);
            if subject >= low && subject <= high {
                hit = true;
            }
            i = high_at.saturating_add(high_len);
        } else {
            if subject == low {
                hit = true;
            }
            i = after_low;
        }
    }
    hit != negated
}

/// The one recovery this matcher has: make the last single `*` swallow one
/// more byte.
///
/// `None` means either that no `*` has been seen yet, or that the star cannot
/// legally grow -- past the end of the text, or over a `/`, which a single
/// star never crosses. Both are an outright failure to match.
///
/// Extracted so the class arm and the literal arm recover identically. They
/// did not have to before, because there was only one failing arm; two copies
/// of a backtrack that must agree is how the `**` mutual recursion above got
/// its infinite loop.
fn backtrack(text: &[u8], star_pi: Option<usize>, star_ti: &mut usize) -> Option<(usize, usize)> {
    let resume_at = star_pi?;
    *star_ti = star_ti.saturating_add(1);
    let swallowed = star_ti.checked_sub(1).and_then(|k| text.get(k));
    if *star_ti > text.len() || swallowed == Some(&b'/') {
        return None;
    }
    Some((resume_at.saturating_add(1), *star_ti))
}

/// Whether `text` matches `pattern`, both as bytes: the matcher every
/// exclusion and restore filter goes through. `**` spans path segments; see
/// the module's other functions for the rest of the syntax.
#[must_use]
pub fn glob_match_recursive(pattern: &[u8], text: &[u8]) -> bool {
    // Check for ** at the start — matches any number of path segments
    if pattern.starts_with(b"**") {
        let rest = if pattern.get(2) == Some(&b'/') {
            pattern.get(3..).unwrap_or_default()
        } else if pattern.len() == 2 {
            // `**` with nothing after it: there is no remainder to match.
            &[]
        } else {
            // `**` followed by something other than `/` is not a globstar.
            // Handing it back to `glob_match_simple` is safe now that
            // `glob_match_simple` agrees about that and degrades it to a
            // single `*`; while the two disagreed this line was half of an
            // infinite mutual recursion. See the note there.
            return glob_match_simple(pattern, text);
        };

        // "**" matches zero or more path segments
        if rest.is_empty() {
            return true;
        }

        // Try matching rest against every suffix of text starting at path
        // boundaries. `i` runs to `text.len()` inclusive, so the suffix may be
        // empty — that is the zero-segment case.
        for i in 0..=text.len() {
            let at_boundary = i == 0 || i.checked_sub(1).and_then(|k| text.get(k)) == Some(&b'/');
            if at_boundary && glob_match_recursive(rest, text.get(i..).unwrap_or_default()) {
                return true;
            }
        }
        // Also try without consuming any leading slash
        return glob_match_recursive(rest, text);
    }

    glob_match_simple(pattern, text)
}

fn glob_match_simple(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0usize;
    let mut ti = 0usize;
    // `None` until a single `*` has been seen. This was `usize::MAX` as a
    // sentinel, which is a valid index the loop could in principle reach;
    // `Option` says the same thing without borrowing a value from the range.
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;

    // Matching `text.get(ti)` rather than testing `ti < text.len()` and then
    // indexing gives the loop its bound and its byte in one step, so there is
    // no window in which the two could disagree.
    while let Some(&t) = text.get(ti) {
        match pattern.get(pi) {
            // `?` matches one character that is not a separator.
            Some(&b'?') if t != b'/' => {
                pi = pi.saturating_add(1);
                // One character, not one byte — see `utf8_char_len`.
                ti = ti.saturating_add(utf8_char_len(text, ti));
            }
            Some(&b'*') => {
                // Hand `**` to the recursive matcher, but only for a `**` that
                // is a whole path segment — `**` at the end of the pattern, or
                // `**/`. That is exactly the condition
                // `glob_match_recursive` accepts, and the two MUST agree.
                //
                // They did not. This delegated on any `**` while
                // `glob_match_recursive` accepted only a whole segment, so
                // `**a` bounced between the two forever: `recursive` saw a `**`
                // it would not handle and passed the pattern to `simple`
                // unchanged, `simple` saw `**` and passed it straight back, and
                // neither consumed a byte. `backup create --exclude '**a'`
                // overflowed the stack and took the backup down with it.
                //
                // A `**` that is not a whole segment is not a globstar; bash's
                // `globstar` and gitignore both degrade it to a single `*`, and
                // falling through to the ordinary-star arm below does that —
                // `**a` behaves as `*a`, which also guarantees progress because
                // `pi` advances every time.
                let is_whole_segment = pattern.get(pi.saturating_add(1)) == Some(&b'*')
                    && matches!(pattern.get(pi.saturating_add(2)), None | Some(&b'/'));
                if is_whole_segment {
                    return glob_match_recursive(
                        pattern.get(pi..).unwrap_or_default(),
                        text.get(ti..).unwrap_or_default(),
                    );
                }
                // Single * — match anything except /
                star_pi = Some(pi);
                star_ti = ti;
                pi = pi.saturating_add(1);
            }
            // A character class, but only a well-formed one. `class_end`
            // returning `None` means the `[` never closes, and then it is an
            // ordinary character that the literal arm below should compare --
            // so the guard is on the arm, not inside it.
            Some(&b'[') if class_end(pattern, pi).is_some() => {
                // `is_some` was just checked; the `else` branch cannot be
                // taken, and `unwrap` is not allowed in this tree.
                let Some(close) = class_end(pattern, pi) else {
                    return false;
                };
                // No class matches a separator. Without this, `[a-z]*` would
                // reach across a directory boundary and an exclude of
                // `[a-z]*.log` would start eating `logs/x.log`.
                if t != b'/' && class_matches(pattern, pi, close, text, ti) {
                    pi = close.saturating_add(1);
                    // One character, not one byte -- as for `?`.
                    ti = ti.saturating_add(utf8_char_len(text, ti));
                } else {
                    let Some((resume_pi, resume_ti)) = backtrack(text, star_pi, &mut star_ti)
                    else {
                        return false;
                    };
                    pi = resume_pi;
                    ti = resume_ti;
                }
            }
            // The literal arm comes AFTER the class arm, and the order is
            // load-bearing. Written the other way round -- which it was, for
            // about ten minutes -- a pattern `cache[1]` against the text
            // `cache[1]` matches `[` against `[` as an ordinary byte and never
            // reaches the class arm at all, so the class silently stops being
            // a class exactly when the subject happens to contain a bracket.
            // `a_bracket_in_an_old_exclude_line_now_means_a_class` is what
            // caught it.
            Some(&p) if p == t => {
                pi = pi.saturating_add(1);
                ti = ti.saturating_add(1);
            }
            // Either the pattern ran out or this byte does not match. Both are
            // recoverable only by making the last single `*` swallow one more
            // byte -- and no star yet is the outright failure.
            _ => {
                let Some((resume_pi, resume_ti)) = backtrack(text, star_pi, &mut star_ti) else {
                    return false;
                };
                pi = resume_pi;
                ti = resume_ti;
            }
        }
    }

    // Consume trailing stars
    while pattern.get(pi) == Some(&b'*') {
        pi = pi.saturating_add(1);
    }

    pi == pattern.len()
}

/// Check if a path should be excluded based on exclude patterns.
#[must_use]
pub fn is_excluded(path: &Path, patterns: &[String]) -> bool {
    // Matched against the path's own bytes, which is what the matcher works
    // on: a byte that is not text is one no pattern names, and nothing is
    // decoded -- the lossy rendering this matched before turned such bytes
    // into U+FFFD, which a pattern holding that character would then match.
    let path = path.as_os_str().as_encoded_bytes();
    // The file's own name as well as the whole path.
    let name = path.rsplit(|&b| b == b'/').next().unwrap_or(path);
    patterns.iter().any(|pattern| {
        glob_match_recursive(pattern.as_bytes(), path)
            || glob_match_recursive(pattern.as_bytes(), name)
    })
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did it
    // -- that is the diagnosis. The defensive lints keep panics out of code
    // that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    /// Exclusions match the path's own bytes: a pattern still excludes a
    /// file whose name is not text, and a pattern holding the replacement
    /// character no longer matches one -- as it did against the lossy form.
    #[cfg(unix)]
    #[test]
    fn an_exclusion_matches_the_paths_own_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"dir/caf\xe9.tmp"));
        assert!(is_excluded(odd, &[String::from("*.tmp")]));
        assert!(
            is_excluded(odd, &[String::from("caf?.tmp")]),
            "a byte that is not text is one character to `?`"
        );
        assert!(
            !is_excluded(odd, &[String::from("caf\u{FFFD}.tmp")]),
            "matched the lossy form"
        );
    }

    /// The name alone, and the whole path, are both tried.
    #[test]
    fn an_exclusion_matches_the_whole_path_or_the_name() {
        assert!(is_excluded(
            Path::new("dir/a.tmp"),
            &[String::from("*.tmp")]
        ));
        assert!(is_excluded(
            Path::new("x/y/a.tmp"),
            &[String::from("a.tmp")]
        ));
        assert!(!is_excluded(
            Path::new("x/y/a.txt"),
            &[String::from("*.tmp")]
        ));
    }

    // --- Glob Pattern Matching Tests ---

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("*.txt", "file.txt"));
        assert!(glob_matches("*.txt", "long.name.txt"));
        assert!(!glob_matches("*.txt", "file.rs"));
        assert!(!glob_matches("*.txt", "dir/file.txt")); // * doesn't cross /
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_matches("file?.txt", "file1.txt"));
        assert!(glob_matches("file?.txt", "fileA.txt"));
        assert!(!glob_matches("file?.txt", "file12.txt"));
        assert!(!glob_matches("file?.txt", "file.txt"));
    }

    #[test]
    fn a_question_mark_matches_one_character_not_one_byte() {
        // `?` is documented as "single char except /". Every one of these is a
        // single character, and every one of them is more than a single byte.
        for ch in ["\u{e9}", "\u{65e5}", "\u{03b1}", "\u{0440}", "\u{1f600}"] {
            assert!(
                glob_matches("?.txt", &format!("{ch}.txt")),
                "?.txt should match {ch}.txt"
            );
            assert!(
                glob_matches("file?.txt", &format!("file{ch}.txt")),
                "file?.txt should match file{ch}.txt"
            );
            // ...and still exactly one of them.
            assert!(
                !glob_matches("?.txt", &format!("{ch}{ch}.txt")),
                "?.txt should not match two characters"
            );
            assert!(
                glob_matches("??.txt", &format!("{ch}{ch}.txt")),
                "??.txt should match two characters"
            );
        }
        // Mixed widths in one name, and `?` against an ASCII character still
        // consumes exactly one byte.
        assert!(glob_matches("?x?.txt", "\u{65e5}x\u{672c}.txt"));
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "ab"));
        // `?` must not swallow a separator, whatever its width.
        assert!(!glob_matches("a?c", "a/c"));
    }

    #[test]
    fn a_non_ascii_name_is_matched_and_excluded_consistently() {
        // The two entry points a pattern actually reaches: whole-path matching
        // and the filename-only fallback in `is_excluded`.
        let patterns = vec!["**/?.log".to_string()];
        assert!(is_excluded(Path::new("var/\u{65e5}.log"), &patterns));
        assert!(!is_excluded(
            Path::new("var/\u{65e5}\u{672c}.log"),
            &patterns
        ));
        let patterns = vec!["\u{65e5}*".to_string()];
        assert!(is_excluded(
            Path::new("dir/\u{65e5}\u{672c}\u{8a9e}.txt"),
            &patterns
        ));
    }

    #[test]
    fn a_truncated_utf8_sequence_does_not_run_past_the_end() {
        // Paths are byte strings and need not be well-formed UTF-8. An
        // ill-formed sequence falls back to one byte per `?`, so a lead byte
        // whose continuations are missing is just a byte.
        assert!(glob_match_recursive(b"?", &[0xE6]));
        assert!(!glob_match_recursive(b"?", &[0xE6, 0x97]));
        assert!(glob_match_recursive(b"??", &[0xE6, 0x97]));
        // A complete sequence is one character.
        assert!(glob_match_recursive(b"?", &[0xE6, 0x97, 0xA5]));
        // A stray continuation byte is consumed one byte at a time.
        assert!(glob_match_recursive(b"?", &[0x97]));
        assert!(glob_match_recursive(b"??", &[0x97, 0xA5]));
    }

    #[test]
    fn a_question_mark_never_crosses_a_separator() {
        // The invariant that made clamping-to-what-remains the wrong fallback:
        // a lead byte announcing three bytes, followed by a separator, must not
        // let `?` consume the separator.
        // This is the case that actually pins it. A lead byte claiming three
        // bytes, with only a separator behind it: consuming "what remains"
        // would swallow the `/` and report a match for a name that contains
        // one. Validating the continuations rejects it.
        assert!(!glob_match_recursive(b"?", &[0xE6, b'/']));
        assert!(!glob_match_recursive(b"?c", &[0xE6, b'/', b'c']));
        assert!(!glob_match_recursive(b"??c", &[0xE6, b'/', b'c']));
        // Well-formed input, same rule.
        assert!(!glob_matches("a?c", "a/c"));
        assert!(!glob_matches("?", "/"));
    }

    #[test]
    fn test_glob_doublestar() {
        assert!(glob_matches("**/*.txt", "file.txt"));
        assert!(glob_matches("**/*.txt", "dir/file.txt"));
        assert!(glob_matches("**/*.txt", "a/b/c/file.txt"));
        assert!(!glob_matches("**/*.rs", "file.txt"));
    }

    #[test]
    fn test_glob_exact() {
        assert!(glob_matches("Makefile", "Makefile"));
        assert!(!glob_matches("Makefile", "makefile"));
        assert!(!glob_matches("Makefile", "dir/Makefile"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_matches("src/**/*.rs", "src/main.rs"));
        assert!(glob_matches("src/**/*.rs", "src/sub/mod.rs"));
        assert!(!glob_matches("src/**/*.rs", "lib/main.rs"));
    }

    #[test]
    fn test_glob_star_prefix() {
        assert!(glob_matches("test_*", "test_foo"));
        assert!(glob_matches("test_*", "test_"));
        assert!(!glob_matches("test_*", "test"));
    }

    // --- Exclusion Tests ---

    #[test]
    fn test_is_excluded_simple() {
        let patterns = vec!["*.tmp".to_string(), "*.log".to_string()];
        assert!(is_excluded(Path::new("file.tmp"), &patterns));
        assert!(is_excluded(Path::new("debug.log"), &patterns));
        assert!(!is_excluded(Path::new("file.txt"), &patterns));
    }

    #[test]
    fn test_is_excluded_directory_pattern() {
        let patterns = vec!["**/node_modules/**".to_string()];
        assert!(is_excluded(
            Path::new("project/node_modules/pkg/index.js"),
            &patterns
        ));
    }

    #[test]
    fn test_is_excluded_filename_fallback() {
        // Pattern matches just the filename component
        let patterns = vec![".gitignore".to_string()];
        assert!(is_excluded(Path::new("project/.gitignore"), &patterns));
    }

    // --- Glob matching ---

    /// `glob_match_recursive` and `glob_match_simple` disagreed about which
    /// `**` was a globstar: `simple` delegated on any `**`, `recursive`
    /// accepted only a whole path segment and handed anything else straight
    /// back. Neither consumed a byte, so `--exclude '**a'` recursed until the
    /// stack ran out and killed the backup mid-run.
    ///
    /// Every pattern here reached that loop before the fix. The test asserts
    /// results rather than merely returning, but the real assertion is that it
    /// terminates at all — a regression hangs the suite rather than failing it,
    /// which is the loudest signal available for this shape of bug.
    #[test]
    fn a_double_star_that_is_not_a_path_segment_terminates() {
        // Degraded to a single `*`, per bash's `globstar` and gitignore.
        assert!(glob_matches("**a", "ba"));
        assert!(glob_matches("**a", "a"));
        assert!(!glob_matches("**a", "b"));
        assert!(glob_matches("**.tmp", "scratch.tmp"));
        assert!(!glob_matches("**.tmp", "scratch.txt"));
        // A single `*` does not cross a separator, and the degraded `**`
        // inherits that.
        assert!(!glob_matches("**a", "x/a"));
        assert!(glob_matches("***", "ab"));
    }

    /// The whole-segment forms must keep their globstar meaning — the fix
    /// narrowed which `**` is special, so this pins that it did not narrow it
    /// to nothing.
    #[test]
    fn a_double_star_that_is_a_path_segment_still_spans_directories() {
        assert!(glob_matches("**/*.txt", "a/b/c/note.txt"));
        assert!(glob_matches("src/**/mod.rs", "src/a/b/mod.rs"));
        assert!(glob_matches("src/**/mod.rs", "src/mod.rs"));
        assert!(glob_matches("src/**", "src/a/b"));
        assert!(!glob_matches("src/**/mod.rs", "lib/a/mod.rs"));
    }

    /// Character classes: the feature C-Q9 decided to add on 2026-09-13.
    ///
    /// Every assertion here is a POSIX `fnmatch(3)` rule that gitignore also
    /// follows, rather than a choice made for this program. The ones most
    /// worth having are the four that look like mistakes: an unclosed `[` is a
    /// literal, a `]` first is a member, a `-` last is a member, and a class
    /// never matches a separator.
    #[test]
    fn character_classes_follow_the_fnmatch_rules() {
        assert!(glob_matches("file[123].txt", "file2.txt"));
        assert!(!glob_matches("file[123].txt", "file4.txt"));
        assert!(glob_matches("[a-z]og", "dog"));
        assert!(!glob_matches("[a-z]og", "Dog"));
        assert!(glob_matches("*.[ch]", "main.c"));
        assert!(glob_matches("*.[ch]", "main.h"));
        assert!(!glob_matches("*.[ch]", "main.rs"));

        // Negation, both spellings.
        assert!(glob_matches("[!0-9]*", "alpha"));
        assert!(!glob_matches("[!0-9]*", "1alpha"));
        assert!(glob_matches("[^0-9]*", "alpha"));
        assert!(!glob_matches("[^0-9]*", "1alpha"));

        // A `]` in the first member position is a member, not the terminator.
        assert!(glob_matches("[]]", "]"));
        assert!(glob_matches("[!]]", "a"));
        assert!(!glob_matches("[!]]", "]"));

        // A `-` with nothing after it inside the class is a literal `-`.
        assert!(glob_matches("[a-]", "-"));
        assert!(glob_matches("[a-]", "a"));
        assert!(!glob_matches("[a-]", "b"));

        // An unterminated `[` is an ordinary character. This is the case that
        // keeps a typo in an exclude list from silently dropping files: the
        // pattern still means something, and it means what it looks like.
        assert!(glob_matches("file[1", "file[1"));
        assert!(!glob_matches("file[1", "file1"));

        // No class crosses a separator, so an exclude cannot leak into a
        // subdirectory.
        assert!(!glob_matches("[a-z]", "/"));
        assert!(!glob_matches("a[a-z]c", "a/c"));
        assert!(!glob_matches("[a-z]*.log", "logs/x.log"));

        // A class is one *character*, not one byte -- the rule `?` already
        // follows. Without it the range check would see a continuation byte.
        assert!(glob_matches("[a-z]*", "abc"));
        assert!(!glob_matches("[a-z]", "日"));
        assert!(glob_matches("[日]", "日"));
        assert!(glob_matches("[!a-z]", "日"));
    }

    /// A class must be able to fail *and let a preceding `*` try again*.
    ///
    /// The class arm is the second place in this matcher that can fail to
    /// consume a character, and the recovery it needs is the one the literal
    /// arm already had. Both now call `backtrack`. If the class arm returned
    /// `false` outright instead, every assertion here would fail, and none of
    /// them is exotic: each is a star followed by a class that does not match
    /// at the first position it is tried.
    #[test]
    fn a_star_can_still_grow_after_a_class_fails() {
        assert!(glob_matches("*[0-9]", "file7"));
        assert!(glob_matches("*[0-9].txt", "report2024.txt"));
        assert!(!glob_matches("*[0-9]", "file"));
        // The star may not grow across a separator to rescue the class.
        assert!(!glob_matches("*[0-9]", "a7/b"));
    }

    /// What the change costs, stated as a test rather than only as prose.
    ///
    /// This is the one behaviour C-Q9 knowingly broke: an exclude line written
    /// as `cache[1]/` used to mean a directory literally named `cache[1]` and
    /// now means `cache1`. It is here so that the next person to read the
    /// matcher finds the cost pinned next to the feature, rather than having
    /// to reconstruct it from a decision record.
    ///
    /// It also earned its keep immediately, which a test written only to
    /// document a cost has no right to expect. The class arm was placed after
    /// the literal arm, so `[` matched `[` as an ordinary byte and the class
    /// arm was never reached -- the feature quietly switched itself off for
    /// exactly the subjects that contain a bracket. Every other assertion in
    /// this file passed.
    #[test]
    fn a_bracket_in_an_old_exclude_line_now_means_a_class() {
        assert!(glob_matches("cache[1]", "cache1"));
        assert!(!glob_matches("cache[1]", "cache[1]"));
    }
}
