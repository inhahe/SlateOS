//! What becomes a message: util-linux 2.39.3's `logger_command_line` and
//! `logger_stdin` (`misc-utils/logger.c`), ported with their exact
//! boundaries. Both hand each message to `emit`; framing is the caller's.

/// `LOG_FACMASK`.
pub const LOG_FACMASK: i32 = 0x03f8;

/// `logger_command_line`: the words of argv joined by single spaces into
/// messages of at most `max` bytes.
///
/// A word that would overflow the current message flushes it first; a word
/// longer than `max` on its own is truncated to `max` and sent as a message of
/// its own. Upstream generates ONE header before this runs, so every chunk
/// carries the same timestamp -- the caller does the same.
///
/// `max` may be 0 (`-S 0`), where upstream's end pointer sits before its
/// buffer; the arithmetic below is signed so that case keeps its C meaning:
/// every non-empty word becomes an empty message, and empty words vanish.
pub fn command_line(words: &[&[u8]], max: usize, emit: &mut dyn FnMut(&[u8])) {
    let cap = i128::try_from(max).unwrap_or(i128::MAX);
    // `endp = buf + max - 1`, as an offset from `buf`.
    let endp = cap.saturating_sub(1);
    let mut buf: Vec<u8> = Vec::new();
    for word in words {
        // `strlen`: a word ends at its first NUL (argv cannot carry one, but
        // the rule is upstream's and costs nothing to keep).
        let word = word.split(|&b| b == 0).next().unwrap_or(word);
        let len = i128::try_from(word.len()).unwrap_or(i128::MAX);
        let p = i128::try_from(buf.len()).unwrap_or(i128::MAX);
        if endp < p.saturating_add(len) && !buf.is_empty() {
            emit(&buf);
            buf.clear();
        }
        if cap < len {
            emit(word.get(..max).unwrap_or(word));
            continue;
        }
        if !buf.is_empty() {
            buf.push(b' ');
        }
        buf.extend_from_slice(word);
    }
    if !buf.is_empty() {
        emit(&buf);
    }
}

/// `logger_stdin`: one message per line of `input`, lines longer than `max`
/// continuing as further messages, with `--prio-prefix` parsing when
/// `prio_prefix` is set.
///
/// `pri` is upstream's `ctl->pri` and is updated in place, because upstream
/// updates it in place: a line with a valid `<PRI>` prefix sets the priority
/// for the lines after it too, until another line that starts with `<`
/// changes it -- a prefix is sticky, not per-line. `emit` receives the
/// priority in force for each message.
///
/// A message ends at its first NUL byte, as `write_output`'s `strlen` ends it,
/// though the rest of the line is still consumed.
pub fn stdin_messages(
    input: impl IntoIterator<Item = u8>,
    max: usize,
    prio_prefix: bool,
    skip_empty: bool,
    pri: &mut i32,
    emit: &mut dyn FnMut(i32, &[u8]),
) {
    let default_priority = *pri;
    // A stream, not a slice: `tail -f file | logger` must send each line as
    // it arrives, as upstream's getchar loop does, not after end of input.
    let mut it = input.into_iter();
    // `c = getchar()`, with `None` for EOF.
    let mut c = it.next();
    while let Some(first) = c {
        let mut buf: Vec<u8> = Vec::new();
        if prio_prefix && first == b'<' {
            let mut value: i64 = 0;
            buf.push(first);
            loop {
                c = it.next();
                // `isdigit(c = getchar()) && pri <= 191`: the digit is read
                // BEFORE the bound is tested, so the digit that crosses 191
                // is consumed here and stored just below.
                match c {
                    Some(d) if d.is_ascii_digit() && value <= 191 => {
                        buf.push(d);
                        value = value
                            .saturating_mul(10)
                            .saturating_add(i64::from(d.wrapping_sub(b'0')));
                    }
                    _ => break,
                }
            }
            // Upstream writes these digits into a buffer of `max + 4` bytes
            // with no bound check, so a prefix of many zeros -- whose value
            // never exceeds 191 -- runs past it: a heap overflow in
            // util-linux 2.39.3. This buffer grows instead, which gives the
            // output 2.39.3 gives whenever the overflow does not crash it.
            // (Master has since bounded the loop by `max`, and in doing so
            // stores the last digit twice when that bound is what stops it;
            // the reference this port is measured against is 2.39.3.)
            if let Some(ch) = c
                && ch != b'\n'
            {
                buf.push(ch);
            }
            if c == Some(b'>') && (0..=191).contains(&value) {
                buf.clear();
                let mut v = i32::try_from(value).unwrap_or(0);
                if v & LOG_FACMASK == 0 {
                    v |= default_priority & LOG_FACMASK;
                }
                *pri = v;
            } else {
                *pri = default_priority;
            }
            if c.is_some_and(|ch| ch != b'\n') {
                c = it.next();
            }
        }

        while let Some(ch) = c {
            if ch == b'\n' || buf.len() >= max {
                break;
            }
            buf.push(ch);
            c = it.next();
        }
        // `-S 0`: upstream's loop above can store nothing, so it never
        // consumes the byte it stopped on, and the outer loop sends empty
        // messages forever -- in 2.39.3 and still on util-linux master. A
        // hang is not behaviour worth porting: here each line becomes one
        // empty message, the same thing `-S 0` makes of an argv word.
        if max == 0 {
            while c.is_some_and(|ch| ch != b'\n') {
                c = it.next();
            }
        }

        if !buf.is_empty() || !skip_empty {
            let msg = buf.split(|&b| b == 0).next().unwrap_or(&buf);
            emit(*pri, msg);
        }

        if c == Some(b'\n') {
            c = it.next();
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn words(max: usize, argv: &[&str]) -> Vec<String> {
        let w: Vec<&[u8]> = argv.iter().map(|s| s.as_bytes()).collect();
        let mut out = Vec::new();
        command_line(&w, max, &mut |m| {
            out.push(String::from_utf8_lossy(m).into_owned());
        });
        out
    }

    #[test]
    fn words_join_with_single_spaces() {
        assert_eq!(words(1024, &["hello", "world"]), ["hello world"]);
    }

    #[test]
    fn a_word_that_would_overflow_starts_the_next_message() {
        // max 10: endp = 9; "aaaa bbbb" is 9 bytes, "cc" would make 12.
        assert_eq!(words(10, &["aaaa", "bbbb", "cc"]), ["aaaa bbbb", "cc"]);
    }

    #[test]
    fn a_word_longer_than_max_goes_alone_truncated() {
        assert_eq!(words(4, &["ab", "abcdefgh", "cd"]), ["ab", "abcd", "cd"]);
    }

    #[test]
    fn size_zero_sends_empty_messages_for_words_and_drops_empty_words() {
        assert_eq!(words(0, &["a", "", "b"]), ["", ""]);
    }

    fn lines(input: &str, max: usize, prefix: bool, skip: bool) -> Vec<(i32, String)> {
        let mut pri = 13;
        let mut out = Vec::new();
        stdin_messages(input.bytes(), max, prefix, skip, &mut pri, &mut |p, m| {
            out.push((p, String::from_utf8_lossy(m).into_owned()));
        });
        out
    }

    #[test]
    fn one_message_per_line_and_the_last_needs_no_newline() {
        assert_eq!(
            lines("a\nb", 1024, false, false),
            [(13, "a".into()), (13, "b".into())]
        );
    }

    #[test]
    fn empty_lines_are_messages_unless_skipped() {
        assert_eq!(lines("a\n\nb\n", 1024, false, false).len(), 3);
        assert_eq!(lines("a\n\nb\n", 1024, false, true).len(), 2);
    }

    #[test]
    fn a_long_line_continues_as_further_messages() {
        assert_eq!(
            lines("abcdefg\n", 3, false, false),
            [(13, "abc".into()), (13, "def".into()), (13, "g".into())]
        );
    }

    #[test]
    fn a_valid_prefix_is_stripped_and_sticks_to_later_lines() {
        // <11> is user.err with no facility bits?  11 = 1<<3 | 3 -> user.err.
        assert_eq!(
            lines("<11>a\nb\n<3>c\n", 1024, true, false),
            [(11, "a".into()), (11, "b".into()), (11, "c".into())]
        );
    }

    #[test]
    fn a_prefix_without_facility_takes_the_default_facility() {
        // default 13 = user.notice; <3> has facility bits 0 -> 8 | 3 = 11.
        assert_eq!(lines("<3>c\n", 1024, true, false), [(11, "c".into())]);
    }

    #[test]
    fn an_invalid_prefix_is_kept_as_text_and_resets_the_priority() {
        assert_eq!(
            lines("<1234>msg\n", 1024, true, false),
            [(13, "<1234>msg".into())]
        );
        assert_eq!(
            lines("<1999>\n", 1024, true, false),
            [(13, "<1999>".into())]
        );
        assert_eq!(lines("<13\n", 1024, true, false), [(13, "<13".into())]);
    }

    #[test]
    fn a_line_that_is_only_a_prefix_is_empty() {
        assert_eq!(lines("<13>\n", 1024, true, true), []);
    }

    /// Upstream loops forever here; see the comment in `stdin_messages`.
    #[test]
    fn size_zero_on_stdin_is_one_empty_message_per_line_not_a_hang() {
        assert_eq!(
            lines("abc\nde\n", 0, false, false),
            [(13, String::new()), (13, String::new())]
        );
    }

    #[test]
    fn a_nul_ends_the_message_but_not_the_line() {
        assert_eq!(
            lines("ab\0cd\nef\n", 1024, false, false),
            [(13, "ab".into()), (13, "ef".into())]
        );
    }
}
