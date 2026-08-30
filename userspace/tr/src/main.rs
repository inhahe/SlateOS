//! Slate OS character translation utility (`tr`)
//!
//! Translates, squeezes, or deletes characters read from stdin, writing the
//! result to stdout. Operates on raw bytes, so it handles binary data and
//! arbitrary encodings correctly.

use std::env;
use std::io::{self, Read, Write};
use std::process;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn die(msg: &str) -> ! {
    let _ = writeln!(io::stderr(), "tr: {msg}");
    process::exit(1);
}

fn usage() -> ! {
    let _ = writeln!(
        io::stderr(),
        "Usage: tr [-cdstC] [--complement] [--delete] [--squeeze-repeats] [--truncate-set1] SET1 [SET2]"
    );
    process::exit(1);
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

struct Opts {
    complement: bool,
    delete: bool,
    squeeze: bool,
    truncate: bool,
    set1_str: Vec<u8>,
    set2_str: Option<Vec<u8>>,
}

fn parse_args() -> Opts {
    let args: Vec<String> = env::args().collect();
    let mut complement = false;
    let mut delete = false;
    let mut squeeze = false;
    let mut truncate = false;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            i += 1;
            // Everything after -- is positional.
            while i < args.len() {
                positional.push(args[i].clone());
                i += 1;
            }
            break;
        }
        if arg.starts_with("--") {
            match arg.as_str() {
                "--complement" => complement = true,
                "--delete" => delete = true,
                "--squeeze-repeats" => squeeze = true,
                "--truncate-set1" => truncate = true,
                _ => die(&format!("unrecognized option: {arg}")),
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Short flags: -cCdst, can be combined like -ds.
            for ch in arg[1..].chars() {
                match ch {
                    'c' | 'C' => complement = true,
                    'd' => delete = true,
                    's' => squeeze = true,
                    't' => truncate = true,
                    _ => die(&format!("unrecognized option: -{ch}")),
                }
            }
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }

    if positional.is_empty() {
        usage();
    }

    let set1_str = positional[0].as_bytes().to_vec();
    let set2_str = if positional.len() > 1 {
        Some(positional[1].as_bytes().to_vec())
    } else {
        None
    };

    // Validate option combinations.
    if delete && squeeze {
        // -ds requires both SET1 and SET2.
        if set2_str.is_none() {
            die("when both -d and -s are specified, SET2 must be given");
        }
    } else if delete {
        // -d alone: SET2 must not be given (GNU tr allows it with -s, but not alone).
        if set2_str.is_some() {
            die("extra operand when deleting without squeezing");
        }
    } else if !delete {
        // Translating or squeezing: SET2 is required when translating.
        if !squeeze && set2_str.is_none() {
            die("two operands are required when translating");
        }
    }

    Opts {
        complement,
        delete,
        squeeze,
        truncate,
        set1_str,
        set2_str,
    }
}

// ---------------------------------------------------------------------------
// Set expansion
// ---------------------------------------------------------------------------

/// Expand a set specification (e.g., `a-z`, `[:alpha:]`, `\n`, `[a*5]`) into a
/// flat list of bytes.
fn expand_set(spec: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let len = spec.len();
    let mut i = 0;

    while i < len {
        // Check for POSIX class [:name:]
        if i + 2 < len
            && spec[i] == b'['
            && spec[i + 1] == b':'
            && let Some(end) = find_posix_class_end(spec, i)
        {
            let class_name = &spec[i + 2..end - 1]; // between [: and :]
            expand_posix_class(class_name, &mut result);
            i = end + 1; // skip past the closing ]
            continue;
        }

        // Check for equivalence class [=c=]
        if i + 4 < len
            && spec[i] == b'['
            && spec[i + 1] == b'='
            && spec[i + 3] == b'='
            && spec[i + 4] == b']'
        {
            result.push(spec[i + 2]);
            i += 5;
            continue;
        }

        // Check for repeat [c*N] or [c*]
        if i + 3 < len
            && spec[i] == b'['
            && let Some((ch, count, consumed)) = parse_repeat(spec, i)
        {
            for _ in 0..count {
                result.push(ch);
            }
            i += consumed;
            continue;
        }

        // Escape sequences
        if spec[i] == b'\\' && i + 1 < len {
            let (byte, consumed) = parse_escape(spec, i);
            result.push(byte);
            i += consumed;
            continue;
        }

        // Range: a-z.  `spec[i]` is the low endpoint, `spec[i + 1]` the dash,
        // and `spec[i + 2]` the high endpoint.  A dash is only a range
        // separator when a high endpoint follows it (`i + 2 < len`); a dash at
        // the very end of the set is literal (it falls through to the
        // push-single path below), and a leading dash is handled the same way
        // because then `spec[i]` itself is the dash, not `spec[i + 1]`.
        if i + 2 < len && spec[i + 1] == b'-' {
            let lo = spec[i];
            let hi = spec[i + 2];
            if lo <= hi {
                for b in lo..=hi {
                    result.push(b);
                }
                i += 3;
                continue;
            }
            // If lo > hi, GNU tr treats this as an error.
            die(&format!(
                "range-endpoints of '{}-{}' are in wrong order",
                char::from(lo),
                char::from(hi)
            ));
        }

        result.push(spec[i]);
        i += 1;
    }

    result
}

/// Search for the end of a POSIX character class `[:name:]`.
/// `start` points at the opening `[`. Returns the index of the closing `]` if
/// found, or `None`.
fn find_posix_class_end(spec: &[u8], start: usize) -> Option<usize> {
    // We expect the pattern [:NAME:] where NAME is alphabetic.
    let mut j = start + 2; // skip past [:
    while j < spec.len() {
        if spec[j] == b':' && j + 1 < spec.len() && spec[j + 1] == b']' {
            return Some(j + 1); // index of the ]
        }
        if !spec[j].is_ascii_alphabetic() {
            return None;
        }
        j += 1;
    }
    None
}

/// Expand a POSIX character class by name.
fn expand_posix_class(name: &[u8], out: &mut Vec<u8>) {
    match name {
        b"upper" => {
            for b in b'A'..=b'Z' {
                out.push(b);
            }
        }
        b"lower" => {
            for b in b'a'..=b'z' {
                out.push(b);
            }
        }
        b"alpha" => {
            for b in b'A'..=b'Z' {
                out.push(b);
            }
            for b in b'a'..=b'z' {
                out.push(b);
            }
        }
        b"digit" => {
            for b in b'0'..=b'9' {
                out.push(b);
            }
        }
        b"alnum" => {
            for b in b'A'..=b'Z' {
                out.push(b);
            }
            for b in b'a'..=b'z' {
                out.push(b);
            }
            for b in b'0'..=b'9' {
                out.push(b);
            }
        }
        b"space" => {
            // space, tab, newline, vertical tab, form feed, carriage return
            out.extend_from_slice(&[b' ', b'\t', b'\n', 0x0B, 0x0C, b'\r']);
        }
        b"blank" => {
            out.extend_from_slice(b" \t");
        }
        b"punct" => {
            for b in 0x21u8..=0x2F {
                out.push(b);
            }
            for b in 0x3Au8..=0x40 {
                out.push(b);
            }
            for b in 0x5Bu8..=0x60 {
                out.push(b);
            }
            for b in 0x7Bu8..=0x7E {
                out.push(b);
            }
        }
        b"cntrl" => {
            for b in 0u8..=0x1F {
                out.push(b);
            }
            out.push(0x7F);
        }
        b"graph" => {
            // printable non-space: 0x21..=0x7E
            for b in 0x21u8..=0x7E {
                out.push(b);
            }
        }
        b"print" => {
            // printable including space: 0x20..=0x7E
            for b in 0x20u8..=0x7E {
                out.push(b);
            }
        }
        b"xdigit" => {
            for b in b'0'..=b'9' {
                out.push(b);
            }
            for b in b'A'..=b'F' {
                out.push(b);
            }
            for b in b'a'..=b'f' {
                out.push(b);
            }
        }
        _ => {
            // Unknown class name -- emit as literal for robustness.
            let name_str = String::from_utf8_lossy(name);
            die(&format!("invalid character class '{name_str}'"));
        }
    }
}

/// Parse a repeat expression `[c*N]` or `[c*]` starting at index `start`.
/// Returns `(character, repeat_count, bytes_consumed)` or `None` if the
/// pattern does not match.
///
/// `[c*]` with no count returns count 0 as a sentinel meaning "fill to
/// length of SET1" -- the caller must handle this.
fn parse_repeat(spec: &[u8], start: usize) -> Option<(u8, usize, usize)> {
    // Minimum: [c*] = 4 bytes
    if start + 3 >= spec.len() {
        return None;
    }
    if spec[start] != b'[' {
        return None;
    }

    let ch = spec[start + 1];

    if spec[start + 2] != b'*' {
        return None;
    }

    let mut j = start + 3;

    // Check for immediate close: [c*]
    if j < spec.len() && spec[j] == b']' {
        return Some((ch, 0, j - start + 1));
    }

    // Parse decimal or octal count.
    let mut count = 0usize;
    let has_digits = j < spec.len() && spec[j].is_ascii_digit();
    if !has_digits {
        return None;
    }

    // Octal if leading 0.
    let octal = j < spec.len() && spec[j] == b'0';

    while j < spec.len() && spec[j].is_ascii_digit() {
        let digit = (spec[j] - b'0') as usize;
        if octal {
            count = count.wrapping_mul(8).wrapping_add(digit);
        } else {
            count = count.wrapping_mul(10).wrapping_add(digit);
        }
        j += 1;
    }

    if j < spec.len() && spec[j] == b']' {
        Some((ch, count, j - start + 1))
    } else {
        None
    }
}

/// Parse an escape sequence starting at `spec[start]` (which must be `\`).
/// Returns `(byte_value, bytes_consumed)`.
fn parse_escape(spec: &[u8], start: usize) -> (u8, usize) {
    debug_assert!(spec[start] == b'\\');
    if start + 1 >= spec.len() {
        return (b'\\', 1);
    }
    match spec[start + 1] {
        b'n' => (b'\n', 2),
        b't' => (b'\t', 2),
        b'r' => (b'\r', 2),
        b'a' => (0x07, 2), // bell
        b'b' => (0x08, 2), // backspace
        b'f' => (0x0C, 2), // form feed
        b'v' => (0x0B, 2), // vertical tab
        b'\\' => (b'\\', 2),
        // Octal: \NNN (1-3 digits)
        d if (b'0'..=b'7').contains(&d) => {
            let mut val = (d - b'0') as u16;
            let mut consumed = 2;
            for off in 2..=3 {
                if start + off < spec.len()
                    && spec[start + off] >= b'0'
                    && spec[start + off] <= b'7'
                {
                    val = val * 8 + (spec[start + off] - b'0') as u16;
                    consumed += 1;
                } else {
                    break;
                }
            }
            // Clamp to byte.
            ((val & 0xFF) as u8, consumed)
        }
        other => (other, 2),
    }
}

// ---------------------------------------------------------------------------
// Translation / deletion / squeeze logic
// ---------------------------------------------------------------------------

/// Build a 256-entry membership table: `table[b] == true` if byte `b` is in the set.
fn build_membership(set: &[u8]) -> [bool; 256] {
    let mut table = [false; 256];
    for &b in set {
        table[b as usize] = true;
    }
    table
}

/// Build a 256-entry translation table that maps each input byte to its output
/// byte.
fn build_translate_table(set1: &[u8], set2: &[u8], truncate: bool) -> [u8; 256] {
    let mut table: [u8; 256] = [0; 256];
    // Identity mapping by default.
    for (i, entry) in table.iter_mut().enumerate() {
        *entry = i as u8;
    }

    if set2.is_empty() {
        return table;
    }

    let effective_len = if truncate {
        set1.len().min(set2.len())
    } else {
        set1.len()
    };

    for i in 0..effective_len {
        // If SET2 is shorter than SET1 and we are not truncating, the last
        // character of SET2 is reused for all remaining SET1 characters.
        let s2_byte = if i < set2.len() {
            set2[i]
        } else {
            // This branch is only reachable when !truncate and i >= set2.len().
            set2[set2.len() - 1]
        };
        table[set1[i] as usize] = s2_byte;
    }

    table
}

/// Run the main processing loop.
fn run(opts: &Opts) -> io::Result<()> {
    let set1 = expand_set(&opts.set1_str);
    let set2 = opts.set2_str.as_deref().map(expand_set);

    // Handle [c*] fill-to-length sentinel: if set2 contains a (byte, 0) repeat
    // entry, replace count 0 with enough repetitions to match set1 length.
    let set2 = set2.map(|mut s2| {
        fill_repeats(&set1, &mut s2, &opts.set2_str);
        s2
    });

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = io::BufWriter::new(stdout.lock());

    const BUF_SIZE: usize = 8192;
    let mut buf = [0u8; BUF_SIZE];
    let mut last_out: Option<u8> = None;

    if opts.delete && opts.squeeze {
        // -ds: delete chars in SET1, then squeeze chars in SET2.
        let del_member = build_membership(&apply_complement(&set1, opts.complement));
        let sq_set = set2.as_deref().unwrap_or(&[]);
        let sq_member = build_membership(sq_set);

        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if del_member[b as usize] {
                    continue; // deleted
                }
                if sq_member[b as usize] && last_out == Some(b) {
                    continue; // squeezed
                }
                last_out = Some(b);
                writer.write_all(&[b])?;
            }
        }
    } else if opts.delete {
        // -d: delete chars in SET1.
        let del_member = build_membership(&apply_complement(&set1, opts.complement));

        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if !del_member[b as usize] {
                    writer.write_all(&[b])?;
                }
            }
        }
    } else if opts.squeeze && set2.is_none() {
        // -s SET1 only (no SET2): squeeze repeated chars that are in SET1.
        let sq_member = build_membership(&apply_complement(&set1, opts.complement));

        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if sq_member[b as usize] && last_out == Some(b) {
                    continue;
                }
                last_out = Some(b);
                writer.write_all(&[b])?;
            }
        }
    } else {
        // Translate (possibly with squeeze).
        let effective_set1 = apply_complement(&set1, opts.complement);
        let s2 = set2.as_deref().unwrap_or(&[]);
        let table = build_translate_table(&effective_set1, s2, opts.truncate);

        if opts.squeeze {
            // Squeeze applies to SET2 membership in the output.
            let sq_member = build_membership(s2);

            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    let out = table[b as usize];
                    if sq_member[out as usize] && last_out == Some(out) {
                        continue;
                    }
                    last_out = Some(out);
                    writer.write_all(&[out])?;
                }
            }
        } else {
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                for i in 0..n {
                    buf[i] = table[buf[i] as usize];
                }
                writer.write_all(&buf[..n])?;
            }
        }
    }

    writer.flush()?;
    Ok(())
}

/// When complement is requested, return all 256 byte values NOT in the original
/// set, sorted ascending. Otherwise return the original set unchanged.
fn apply_complement(set: &[u8], complement: bool) -> Vec<u8> {
    if !complement {
        return set.to_vec();
    }
    let member = build_membership(set);
    let mut out = Vec::new();
    for i in 0u16..=255 {
        if !member[i as usize] {
            out.push(i as u8);
        }
    }
    out
}

/// Handle `[c*]` fill-to-length repeat sentinels in SET2.
///
/// When expand_set encounters `[c*]` (no count), it emits a single copy of `c`
/// with count=0 as a sentinel. This function re-expands SET2 from the raw spec
/// to replace that sentinel with enough copies to pad SET2 to SET1's length.
///
/// This is only meaningful in SET2 for translate mode.
fn fill_repeats(set1: &[u8], set2: &mut Vec<u8>, raw_set2: &Option<Vec<u8>>) {
    let raw = match raw_set2 {
        Some(r) => r,
        None => return,
    };

    // Check if there is a [c*] (zero-count repeat) in the raw spec.
    // We need to find it and figure out how many chars are before and after it
    // in the expanded set2, then compute the fill count.
    let mut has_fill = false;
    let mut i = 0;
    while i < raw.len() {
        if i + 3 < raw.len() && raw[i] == b'[' && raw[i + 2] == b'*' && raw[i + 3] == b']' {
            has_fill = true;
            break;
        }
        i += 1;
    }

    if !has_fill {
        return;
    }

    // Re-expand with the fill count calculated.
    // Strategy: expand the parts before and after the [c*] token, then compute
    // how many copies of c are needed to make the total length == set1.len().
    let fill_char = raw[i + 1];
    let before = expand_set(&raw[..i]);
    let after = if i + 4 < raw.len() {
        expand_set(&raw[i + 4..])
    } else {
        Vec::new()
    };

    let existing = before.len() + after.len();
    let fill_count = if set1.len() > existing {
        set1.len() - existing
    } else {
        1 // At minimum, emit the character once.
    };

    let mut new_set2 = before;
    for _ in 0..fill_count {
        new_set2.push(fill_char);
    }
    new_set2.extend_from_slice(&after);
    *set2 = new_set2;
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let opts = parse_args();
    if let Err(e) = run(&opts) {
        // EPIPE and similar I/O errors on stdout are expected when piped.
        if e.kind() != io::ErrorKind::BrokenPipe {
            let _ = writeln!(io::stderr(), "tr: {e}");
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- expand_set -----------------------------------------------------------

    #[test]
    fn test_expand_literal() {
        assert_eq!(expand_set(b"abc"), vec![b'a', b'b', b'c']);
    }

    #[test]
    fn test_expand_range() {
        let result = expand_set(b"a-f");
        assert_eq!(result, vec![b'a', b'b', b'c', b'd', b'e', b'f']);
    }

    #[test]
    fn test_expand_digit_range() {
        let result = expand_set(b"0-9");
        assert_eq!(result, (b'0'..=b'9').collect::<Vec<u8>>());
    }

    #[test]
    fn test_expand_upper_range() {
        let result = expand_set(b"A-Z");
        assert_eq!(result, (b'A'..=b'Z').collect::<Vec<u8>>());
    }

    #[test]
    fn test_expand_posix_upper() {
        let result = expand_set(b"[:upper:]");
        assert_eq!(result, (b'A'..=b'Z').collect::<Vec<u8>>());
    }

    #[test]
    fn test_expand_posix_lower() {
        let result = expand_set(b"[:lower:]");
        assert_eq!(result, (b'a'..=b'z').collect::<Vec<u8>>());
    }

    #[test]
    fn test_expand_posix_digit() {
        let result = expand_set(b"[:digit:]");
        assert_eq!(result, (b'0'..=b'9').collect::<Vec<u8>>());
    }

    #[test]
    fn test_expand_posix_space() {
        let result = expand_set(b"[:space:]");
        assert_eq!(result, vec![b' ', b'\t', b'\n', 0x0B, 0x0C, b'\r']);
    }

    #[test]
    fn test_expand_posix_blank() {
        let result = expand_set(b"[:blank:]");
        assert_eq!(result, vec![b' ', b'\t']);
    }

    #[test]
    fn test_expand_posix_xdigit() {
        let result = expand_set(b"[:xdigit:]");
        let mut expected: Vec<u8> = (b'0'..=b'9').collect();
        expected.extend(b'A'..=b'F');
        expected.extend(b'a'..=b'f');
        assert_eq!(result, expected);
    }

    #[test]
    fn test_expand_escape_newline() {
        assert_eq!(expand_set(b"\\n"), vec![b'\n']);
    }

    #[test]
    fn test_expand_escape_tab() {
        assert_eq!(expand_set(b"\\t"), vec![b'\t']);
    }

    #[test]
    fn test_expand_escape_backslash() {
        assert_eq!(expand_set(b"\\\\"), vec![b'\\']);
    }

    #[test]
    fn test_expand_escape_octal() {
        // \101 = 65 = 'A'
        assert_eq!(expand_set(b"\\101"), vec![b'A']);
    }

    #[test]
    fn test_expand_equivalence_class() {
        assert_eq!(expand_set(b"[=a=]"), vec![b'a']);
    }

    #[test]
    fn test_expand_repeat_count() {
        // [x*3] should produce x, x, x
        assert_eq!(expand_set(b"[x*3]"), vec![b'x', b'x', b'x']);
    }

    #[test]
    fn test_expand_repeat_zero_sentinel() {
        // [x*] produces a single x (sentinel count=0, caller resolves fill).
        assert_eq!(expand_set(b"[x*]"), vec![]);
    }

    #[test]
    fn test_expand_mixed() {
        // a-c + literal 'X' + [:digit:]
        let result = expand_set(b"a-cX[:digit:]");
        let mut expected = vec![b'a', b'b', b'c', b'X'];
        expected.extend(b'0'..=b'9');
        assert_eq!(result, expected);
    }

    // -- build_translate_table ------------------------------------------------

    #[test]
    fn test_translate_table_basic() {
        let set1 = vec![b'a', b'b', b'c'];
        let set2 = vec![b'x', b'y', b'z'];
        let table = build_translate_table(&set1, &set2, false);
        assert_eq!(table[b'a' as usize], b'x');
        assert_eq!(table[b'b' as usize], b'y');
        assert_eq!(table[b'c' as usize], b'z');
        // Unmapped bytes stay identity.
        assert_eq!(table[b'd' as usize], b'd');
    }

    #[test]
    fn test_translate_table_set2_shorter_no_truncate() {
        let set1 = vec![b'a', b'b', b'c'];
        let set2 = vec![b'x'];
        let table = build_translate_table(&set1, &set2, false);
        assert_eq!(table[b'a' as usize], b'x');
        // b and c should map to last char of set2.
        assert_eq!(table[b'b' as usize], b'x');
        assert_eq!(table[b'c' as usize], b'x');
    }

    #[test]
    fn test_translate_table_truncate() {
        let set1 = vec![b'a', b'b', b'c'];
        let set2 = vec![b'x'];
        let table = build_translate_table(&set1, &set2, true);
        assert_eq!(table[b'a' as usize], b'x');
        // b and c should remain unmapped (identity) because truncate limits to
        // min(set1.len, set2.len).
        assert_eq!(table[b'b' as usize], b'b');
        assert_eq!(table[b'c' as usize], b'c');
    }

    // -- build_membership / apply_complement ----------------------------------

    #[test]
    fn test_membership() {
        let set = vec![b'a', b'b'];
        let table = build_membership(&set);
        assert!(table[b'a' as usize]);
        assert!(table[b'b' as usize]);
        assert!(!table[b'c' as usize]);
    }

    #[test]
    fn test_complement() {
        let set = vec![b'a'];
        let comp = apply_complement(&set, true);
        assert_eq!(comp.len(), 255); // everything except 'a'
        assert!(!comp.contains(&b'a'));
        assert!(comp.contains(&b'b'));
        assert!(comp.contains(&0));
    }

    #[test]
    fn test_no_complement() {
        let set = vec![b'a', b'b'];
        let result = apply_complement(&set, false);
        assert_eq!(result, set);
    }

    // -- parse_escape ---------------------------------------------------------

    #[test]
    fn test_parse_escape_named() {
        assert_eq!(parse_escape(b"\\n", 0), (b'\n', 2));
        assert_eq!(parse_escape(b"\\t", 0), (b'\t', 2));
        assert_eq!(parse_escape(b"\\r", 0), (b'\r', 2));
        assert_eq!(parse_escape(b"\\\\", 0), (b'\\', 2));
        assert_eq!(parse_escape(b"\\a", 0), (0x07, 2));
        assert_eq!(parse_escape(b"\\b", 0), (0x08, 2));
        assert_eq!(parse_escape(b"\\f", 0), (0x0C, 2));
        assert_eq!(parse_escape(b"\\v", 0), (0x0B, 2));
    }

    #[test]
    fn test_parse_escape_octal() {
        // \141 = 97 = 'a'
        assert_eq!(parse_escape(b"\\141", 0), (b'a', 4));
        // \0 = NUL
        assert_eq!(parse_escape(b"\\0", 0), (0, 2));
        // \377 = 255
        assert_eq!(parse_escape(b"\\377", 0), (255, 4));
    }

    // -- parse_repeat ---------------------------------------------------------

    #[test]
    fn test_parse_repeat_with_count() {
        assert_eq!(parse_repeat(b"[a*5]", 0), Some((b'a', 5, 5)));
    }

    #[test]
    fn test_parse_repeat_fill_sentinel() {
        assert_eq!(parse_repeat(b"[a*]", 0), Some((b'a', 0, 4)));
    }

    #[test]
    fn test_parse_repeat_octal_count() {
        // [a*010] -- octal 010 = 8
        assert_eq!(parse_repeat(b"[a*010]", 0), Some((b'a', 8, 7)));
    }

    #[test]
    fn test_parse_repeat_no_match() {
        assert_eq!(parse_repeat(b"abc", 0), None);
        assert_eq!(parse_repeat(b"[ab]", 0), None);
    }

    // -- fill_repeats ---------------------------------------------------------

    #[test]
    fn test_fill_repeats_pads_to_set1_len() {
        let set1 = vec![b'a', b'b', b'c', b'd', b'e'];
        let raw_set2 = Some(b"[x*]".to_vec());
        let mut set2 = expand_set(b"[x*]");
        fill_repeats(&set1, &mut set2, &raw_set2);
        assert_eq!(set2, vec![b'x', b'x', b'x', b'x', b'x']);
    }

    #[test]
    fn test_fill_repeats_with_surrounding() {
        let set1 = vec![b'a', b'b', b'c', b'd', b'e'];
        let raw_set2 = Some(b"A[x*]Z".to_vec());
        let mut set2 = expand_set(b"A[x*]Z");
        fill_repeats(&set1, &mut set2, &raw_set2);
        // A + 3 x's + Z = 5 total to match set1 length.
        assert_eq!(set2, vec![b'A', b'x', b'x', b'x', b'Z']);
    }

    // -- posix class: graph, print, punct, cntrl, alnum -----------------------

    #[test]
    fn test_expand_posix_graph() {
        let result = expand_set(b"[:graph:]");
        assert_eq!(result.len(), 94); // 0x21..=0x7E
        assert!(result.contains(&b'!'));
        assert!(result.contains(&b'~'));
        assert!(!result.contains(&b' '));
    }

    #[test]
    fn test_expand_posix_print() {
        let result = expand_set(b"[:print:]");
        assert_eq!(result.len(), 95); // 0x20..=0x7E
        assert!(result.contains(&b' '));
        assert!(result.contains(&b'~'));
    }

    #[test]
    fn test_expand_posix_cntrl() {
        let result = expand_set(b"[:cntrl:]");
        assert_eq!(result.len(), 33); // 0..=0x1F plus 0x7F
        assert!(result.contains(&0));
        assert!(result.contains(&0x1F));
        assert!(result.contains(&0x7F));
        assert!(!result.contains(&b' '));
    }

    #[test]
    fn test_expand_posix_alnum() {
        let result = expand_set(b"[:alnum:]");
        assert_eq!(result.len(), 62); // 26+26+10
    }

    #[test]
    fn test_expand_posix_punct() {
        let result = expand_set(b"[:punct:]");
        assert!(result.contains(&b'!'));
        assert!(result.contains(&b'@'));
        assert!(result.contains(&b'['));
        assert!(result.contains(&b'{'));
        assert!(result.contains(&b'~'));
        assert!(!result.contains(&b'a'));
        assert!(!result.contains(&b'0'));
    }
}
