//! tr — translate or delete characters.
//!
//! Usage: tr [-d] SET1 [SET2]
//!   -d  delete characters in SET1 (no SET2 needed)
//!   Without -d: translate SET1 chars to corresponding SET2 chars.
//!   Reads from stdin, writes to stdout.

use std::env;
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut delete = false;
    let mut sets: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-d" {
            delete = true;
        } else {
            sets.push(arg.clone());
        }
    }

    if sets.is_empty() {
        eprintln!("tr: missing operand");
        process::exit(1);
    }

    let set1 = expand_set(&sets[0]);

    let mut input = Vec::new();
    let _ = io::stdin().read_to_end(&mut input);
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if delete {
        let output = delete_bytes(&input, &set1);
        let _ = out.write_all(&output);
    } else {
        if sets.len() < 2 {
            eprintln!("tr: missing SET2");
            process::exit(1);
        }
        let set2 = expand_set(&sets[1]);
        let table = build_translate_table(&set1, &set2);
        let output = translate(&input, &table);
        let _ = out.write_all(&output);
    }
}

/// Expand a set string, handling ranges like a-z.
fn expand_set(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if i + 2 < bytes.len() && bytes[i + 1] == b'-' {
            let start = bytes[i];
            let end = bytes[i + 2];
            if start <= end {
                for b in start..=end {
                    result.push(b);
                }
            } else {
                for b in (end..=start).rev() {
                    result.push(b);
                }
            }
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    result
}

/// Build the byte-to-byte translation table for `set1 -> set2`. Bytes not
/// in set1 are passed through unchanged. If set1 is longer than set2, the
/// extra bytes all map to set2's last byte (POSIX behavior).
fn build_translate_table(set1: &[u8], set2: &[u8]) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        // i is in 0..256 — fits in u8.
        *slot = u8::try_from(i).unwrap_or(0);
    }
    for (i, &from) in set1.iter().enumerate() {
        let to = if i < set2.len() {
            // Bounds-checked by the i < set2.len() branch.
            *set2.get(i).unwrap_or(&from)
        } else {
            *set2.last().unwrap_or(&from)
        };
        table[from as usize] = to;
    }
    table
}

/// Translate `input` byte-by-byte through `table`.
fn translate(input: &[u8], table: &[u8; 256]) -> Vec<u8> {
    input.iter().map(|&b| table[b as usize]).collect()
}

/// Delete all bytes in `input` that are in `set`.
fn delete_bytes(input: &[u8], set: &[u8]) -> Vec<u8> {
    input.iter().copied().filter(|b| !set.contains(b)).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- expand_set ----------------

    #[test]
    fn expand_set_literal_chars() {
        assert_eq!(expand_set("abc"), b"abc");
    }

    #[test]
    fn expand_set_simple_range() {
        assert_eq!(expand_set("a-e"), b"abcde");
    }

    #[test]
    fn expand_set_uppercase_range() {
        assert_eq!(expand_set("A-Z"), b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    }

    #[test]
    fn expand_set_digit_range() {
        assert_eq!(expand_set("0-9"), b"0123456789");
    }

    #[test]
    fn expand_set_reverse_range() {
        assert_eq!(expand_set("e-a"), b"edcba");
    }

    #[test]
    fn expand_set_mixed_literal_and_range() {
        assert_eq!(expand_set("a-cXY1-3"), b"abcXY123");
    }

    #[test]
    fn expand_set_empty() {
        assert!(expand_set("").is_empty());
    }

    #[test]
    fn expand_set_dash_at_end_is_literal() {
        // No range form when '-' is the last char.
        assert_eq!(expand_set("ab-"), b"ab-");
    }

    #[test]
    fn expand_set_single_char_range() {
        assert_eq!(expand_set("a-a"), b"a");
    }

    // ---------------- build_translate_table ----------------

    #[test]
    fn translate_table_identity_for_unmapped() {
        let t = build_translate_table(&[], &[]);
        for i in 0..=255u8 {
            assert_eq!(t[i as usize], i);
        }
    }

    #[test]
    fn translate_table_basic_mapping() {
        let t = build_translate_table(b"abc", b"xyz");
        assert_eq!(t[b'a' as usize], b'x');
        assert_eq!(t[b'b' as usize], b'y');
        assert_eq!(t[b'c' as usize], b'z');
        // Unmapped bytes pass through.
        assert_eq!(t[b'd' as usize], b'd');
        assert_eq!(t[0], 0);
    }

    #[test]
    fn translate_table_set1_longer_pads_with_set2_last() {
        // POSIX: extras map to last byte of set2.
        let t = build_translate_table(b"abcde", b"xy");
        assert_eq!(t[b'a' as usize], b'x');
        assert_eq!(t[b'b' as usize], b'y');
        assert_eq!(t[b'c' as usize], b'y');
        assert_eq!(t[b'd' as usize], b'y');
        assert_eq!(t[b'e' as usize], b'y');
    }

    #[test]
    fn translate_table_set2_longer_extras_ignored() {
        // Extra bytes in set2 don't map anything.
        let t = build_translate_table(b"ab", b"xyzw");
        assert_eq!(t[b'a' as usize], b'x');
        assert_eq!(t[b'b' as usize], b'y');
        assert_eq!(t[b'z' as usize], b'z');
    }

    // ---------------- translate ----------------

    #[test]
    fn translate_simple() {
        let t = build_translate_table(b"abc", b"xyz");
        assert_eq!(translate(b"a-b-c", &t), b"x-y-z");
    }

    #[test]
    fn translate_uppercase() {
        let set1 = expand_set("a-z");
        let set2 = expand_set("A-Z");
        let t = build_translate_table(&set1, &set2);
        assert_eq!(translate(b"Hello, World!", &t), b"HELLO, WORLD!");
    }

    #[test]
    fn translate_empty_input() {
        let t = build_translate_table(b"a", b"b");
        assert!(translate(b"", &t).is_empty());
    }

    #[test]
    fn translate_preserves_unmapped_bytes() {
        let t = build_translate_table(b"x", b"y");
        assert_eq!(translate(b"abcd", &t), b"abcd");
    }

    // ---------------- delete_bytes ----------------

    #[test]
    fn delete_removes_listed_bytes() {
        assert_eq!(delete_bytes(b"hello world", b"l"), b"heo word");
    }

    #[test]
    fn delete_multiple_set_chars() {
        assert_eq!(delete_bytes(b"hello world", b"lo"), b"he wrd");
    }

    #[test]
    fn delete_nothing_when_set_disjoint() {
        assert_eq!(delete_bytes(b"hello", b"xyz"), b"hello");
    }

    #[test]
    fn delete_with_range_set() {
        // Delete all vowels.
        let set = expand_set("aeiou");
        assert_eq!(delete_bytes(b"hello world", &set), b"hll wrld");
    }

    #[test]
    fn delete_empty_input() {
        assert!(delete_bytes(b"", b"abc").is_empty());
    }

    #[test]
    fn delete_empty_set_keeps_all() {
        assert_eq!(delete_bytes(b"hello", b""), b"hello");
    }
}
