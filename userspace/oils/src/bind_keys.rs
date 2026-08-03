//! How readline spells a key sequence — the translation between the text a
//! `bind` command is written with and the bytes readline matches on, and back
//! again for the listings.
//!
//! Both directions are needed the moment the tables stop being constants. A
//! binding is *matched* on bytes, so that is what a live table has to hold: it
//! is the only spelling in which `"\C-y"`, `"\C-Y"` and `"\031"` are the one
//! binding they really are, and the only one that sorts the way readline lists
//! them. But a binding is *written* and *printed* in an escaped text form, and
//! the two are not the same text — readline prints the form it would have
//! chosen, not the one it was given. So `bind '"\ey": yank'` is answered by
//! `bind -q yank` with `\M-y`.
//!
//! This mirrors readline's `rl_translate_keyseq` (in) and `_rl_get_keyname`
//! (out). The round-trip is pinned against readline's own compiled-in tables:
//! every binding in [`crate::bind_tables`] decodes and prints back to exactly
//! the text it was captured as — see the tests at the foot of this file.

/// Escape, which readline uses for two unrelated things: a key in its own
/// right, and the prefix that stands in for a meta modifier.
pub const ESC: u8 = 0x1b;
/// Delete, which readline names `\C-?` rather than by the letter its bit
/// pattern would give.
const RUBOUT: u8 = 0x7f;

/// The byte readline's `\C-` prefix produces, as its `CTRL()` macro does it:
/// the character uppercased and then stripped of its top three bits. That is
/// why `\C-a` and `\C-A` are the same key (1) and `\C-@` is NUL.
fn ctrl(c: u8) -> u8 {
    c.to_ascii_uppercase() & 0x1f
}

/// Decode one escape *body* — everything after the backslash, starting at
/// `i` — into the byte it stands for, and say how many bytes of `spec` it ate.
///
/// The set is readline's: the C escapes it shares with the shell, `\d` for
/// delete (which C spells nothing), `\e` for escape, an octal run of up to
/// three digits and a hex run of up to two. Anything else stands for itself,
/// so `\q` is `q` — readline does not diagnose an unknown escape.
fn one_escape(spec: &[u8], i: usize) -> (u8, usize) {
    // A backslash at the very end of the sequence is a byte like any other.
    let Some(&c) = spec.get(i) else {
        return (b'\\', 0);
    };
    match c {
        b'a' => (7, 1),
        b'b' => (8, 1),
        b'd' => (RUBOUT, 1),
        b'e' => (ESC, 1),
        b'f' => (12, 1),
        b'n' => (b'\n', 1),
        b'r' => (b'\r', 1),
        b't' => (b'\t', 1),
        b'v' => (11, 1),
        b'0'..=b'7' => {
            // Up to three octal digits in all, and the value wraps into a byte
            // rather than being refused — `\400` is `\000`.
            let mut v: u32 = u32::from(c - b'0');
            let mut n = 1usize;
            while n < 3 {
                match spec.get(i + n) {
                    Some(&d @ b'0'..=b'7') => {
                        v = v * 8 + u32::from(d - b'0');
                        n += 1;
                    }
                    _ => break,
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            (v as u8, n)
        }
        b'x' => {
            // `\x` with no digit behind it is just an `x`; readline reads at
            // most two digits, so `\x41B` is `AB`.
            let mut v: u32 = 0;
            let mut n = 1usize;
            while n <= 2 {
                match spec.get(i + n).and_then(|d| (*d as char).to_digit(16)) {
                    Some(d) => {
                        v = v * 16 + d;
                        n += 1;
                    }
                    None => break,
                }
            }
            if n == 1 {
                (b'x', 1)
            } else {
                #[allow(clippy::cast_possible_truncation)]
                (v as u8, n)
            }
        }
        _ => (c, 1),
    }
}

/// Decode the text of a key sequence into the bytes readline matches on, as
/// `rl_translate_keyseq` does.
///
/// `spec` is the sequence alone — a caller that was handed `"…": command` has
/// already taken the quotes and the target off.
///
/// `\M-` is not a modifier bit here: readline turns it into a leading escape
/// byte, which is what makes `\ey` and `\M-y` the same two bytes and is why a
/// listing spells an escape-prefixed sequence with `\M-`. `\C-` *is* a bit
/// operation, and the two nest — `\C-\M-g` is escape followed by `\C-g`,
/// exactly like `\M-\C-g`.
#[must_use]
pub fn decode(spec: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spec.len());
    let mut i = 0usize;
    while let Some(&c) = spec.get(i) {
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        // `\C-` and `\M-` modify what follows rather than standing for a byte,
        // so they are read before the escape table.
        if matches!(spec.get(i), Some(b'C' | b'M')) && spec.get(i + 1) == Some(&b'-') {
            let modifier = spec.get(i).copied().unwrap_or(b'C');
            i += 2;
            if modifier == b'M' {
                // Meta is a prefix byte, and the rest of the sequence is read
                // as if it had been written on its own.
                out.push(ESC);
                continue;
            }
            // `\C-\M-x` — the two written the other way round, meaning the
            // same thing.
            if spec.get(i) == Some(&b'\\')
                && spec.get(i + 1) == Some(&b'M')
                && spec.get(i + 2) == Some(&b'-')
            {
                out.push(ESC);
                i += 3;
            }
            let Some(&k) = spec.get(i) else {
                // `\C-` with nothing to control: readline has nothing to
                // produce and neither has this.
                break;
            };
            i += 1;
            if k == b'?' {
                // The one control name that is not its letter's bit pattern.
                out.push(RUBOUT);
            } else if k == b'\\' {
                // The controlled character is itself written as an escape, as
                // in `\C-\\` for the byte 0x1c.
                let (b, used) = one_escape(spec, i);
                i += used;
                out.push(ctrl(b));
            } else {
                out.push(ctrl(k));
            }
            continue;
        }
        let (b, used) = one_escape(spec, i);
        i += used;
        out.push(b);
    }
    out
}

/// Render a key sequence the way readline's listings spell it.
///
/// `is_prefix` says whether a *longer* sequence is also bound in the same
/// keymap. It has to be asked because readline keeps a bound prefix in the
/// slot its continuation map reserves for "and nothing further", and prints
/// that slot as a trailing `\000`: binding `\C-x` alone in the emacs map —
/// where `\C-x\C-e` and friends live — is listed as `\C-x\000`, and the escape
/// bound by itself in `vi-insert` is `\M-\000`. The same rule decides the one
/// place escape is not written `\M-`: it is the prefix spelling everywhere
/// except at the end of a sequence that is nobody's prefix, where it is `\e`.
#[must_use]
pub fn encode(seq: &[u8], is_prefix: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(seq.len().saturating_mul(4));
    let last = seq.len().saturating_sub(1);
    for (i, &b) in seq.iter().enumerate() {
        match b {
            ESC if i < last || is_prefix => out.extend_from_slice(b"\\M-"),
            ESC => out.extend_from_slice(b"\\e"),
            RUBOUT => out.extend_from_slice(b"\\C-?"),
            0x00..=0x1f => {
                out.extend_from_slice(b"\\C-");
                // The inverse of `ctrl`: put the top bits back and lowercase
                // the letters, which leaves `@[\]^_` alone. Only the backslash
                // needs escaping again on the way out.
                let c = (b | 0x40).to_ascii_lowercase();
                if c == b'\\' {
                    out.push(b'\\');
                }
                out.push(c);
            }
            // A byte with the top bit set has no name, so it is printed in
            // octal — always three digits, so it cannot run into a digit of
            // the sequence behind it.
            0x80..=0xff => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            // The listing quotes the whole sequence, so these two have to be
            // escaped inside it.
            b'\\' | b'"' => {
                out.push(b'\\');
                out.push(b);
            }
            _ => out.push(b),
        }
    }
    if is_prefix {
        out.extend_from_slice(b"\\000");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use crate::bind_tables::KEYMAPS;

    /// A binding printed with a trailing `\000` is the prefix by itself, and
    /// the marker is not part of the sequence — see [`encode`].
    fn table_seq(text: &str) -> (Vec<u8>, bool) {
        match text.strip_suffix("\\000") {
            Some(head) => (decode(head.as_bytes()), true),
            None => (decode(text.as_bytes()), false),
        }
    }

    #[test]
    fn every_readline_binding_survives_a_round_trip() {
        for map in &KEYMAPS {
            for (text, func) in map.bindings {
                let (seq, is_prefix) = table_seq(text);
                assert!(!seq.is_empty(), "{}: {text} decoded to nothing", func);
                let back = encode(&seq, is_prefix);
                assert_eq!(
                    String::from_utf8_lossy(&back),
                    *text,
                    "{}/{func}: {text} -> {seq:?} -> {}",
                    map.names.first().copied().unwrap_or("?"),
                    String::from_utf8_lossy(&back)
                );
            }
        }
    }

    /// The `\000` marker is not an arbitrary label: it is exactly the
    /// sequences that another binding in the same keymap extends. Deriving it
    /// rather than reading it is what lets a *live* table print a prefix
    /// binding correctly after the tables have been changed.
    #[test]
    fn the_prefix_marker_is_derivable_from_the_keymap() {
        for map in &KEYMAPS {
            let seqs: Vec<Vec<u8>> = map.bindings.iter().map(|(t, _)| table_seq(t).0).collect();
            for ((text, func), seq) in map.bindings.iter().zip(&seqs) {
                let derived = seqs
                    .iter()
                    .any(|o| o.len() > seq.len() && o.starts_with(seq));
                assert_eq!(
                    String::from_utf8_lossy(&encode(seq, derived)),
                    *text,
                    "{}/{func}",
                    map.names.first().copied().unwrap_or("?")
                );
            }
        }
    }

    /// readline lists a function's key sequences in the order its keymap walks
    /// them, which is by the *bytes* — so a live table kept sorted that way
    /// lists in readline's order without having to remember the capture order.
    #[test]
    fn the_tables_are_already_in_byte_order_within_each_function() {
        for map in &KEYMAPS {
            for (name, _) in map.bindings {
                let of_func: Vec<Vec<u8>> = map
                    .bindings
                    .iter()
                    .filter(|(_, f)| f == name)
                    .map(|(t, _)| table_seq(t).0)
                    .collect();
                let mut sorted = of_func.clone();
                sorted.sort();
                assert_eq!(of_func, sorted, "{name}");
            }
        }
    }

    #[test]
    fn the_ways_of_writing_one_key_all_decode_to_it() {
        // The same byte spelled five ways: control letter (either case), the
        // octal it is, the hex it is, and the escape readline prints.
        for spec in [r"\C-y", r"\C-Y", r"\031", r"\x19"] {
            assert_eq!(decode(spec.as_bytes()), vec![0x19], "{spec}");
        }
        // Meta is an escape prefix, so all three of these are two bytes.
        for spec in [r"\M-y", r"\ey", r"\033y"] {
            assert_eq!(decode(spec.as_bytes()), vec![0x1b, b'y'], "{spec}");
        }
        // The two modifiers nest either way round.
        for spec in [r"\M-\C-g", r"\C-\M-g"] {
            assert_eq!(decode(spec.as_bytes()), vec![0x1b, 0x07], "{spec}");
        }
        assert_eq!(decode(br"\C-?"), vec![0x7f]);
        assert_eq!(decode(br"\d"), vec![0x7f]);
        assert_eq!(decode(br"\C-@"), vec![0x00]);
        assert_eq!(decode(br"\C-\\"), vec![0x1c]);
        // An unknown escape is the letter itself, and a trailing backslash is
        // a backslash.
        assert_eq!(decode(br"\q"), vec![b'q']);
        assert_eq!(decode(br"a\"), vec![b'a', b'\\']);
        // `\x` with nothing behind it is an `x`.
        assert_eq!(decode(br"\xz"), vec![b'x', b'z']);
    }

    #[test]
    fn a_sequence_is_printed_the_way_readline_would_have_written_it() {
        assert_eq!(encode(&[0x19], false), b"\\C-y");
        assert_eq!(encode(&[0x1b, b'y'], false), b"\\M-y");
        assert_eq!(encode(&[0x1b], false), b"\\e");
        assert_eq!(encode(&[0x1b], true), b"\\M-\\000");
        assert_eq!(encode(&[0x18], true), b"\\C-x\\000");
        assert_eq!(encode(&[0x00], false), b"\\C-@");
        assert_eq!(encode(&[0x1c], false), b"\\C-\\\\");
        assert_eq!(encode(&[0x7f], false), b"\\C-?");
        assert_eq!(encode(&[0xe6], false), b"\\346");
        assert_eq!(encode(b"\"", false), b"\\\"");
        assert_eq!(encode(b"\\", false), b"\\\\");
        assert_eq!(encode(b"zq", false), b"zq");
    }
}
