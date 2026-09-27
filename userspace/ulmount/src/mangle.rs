//! `lib/mangle.c`: how the mount tables write a path holding a blank, a
//! tab, a newline or a backslash -- as `\NNN`, three octal digits -- and
//! read it back.
//!
//! The tables are split at blanks and tabs, so those bytes cannot appear
//! bare in a field; `/mnt/my disk` is `/mnt/my\040disk` in `/proc/self/
//! mountinfo` and in `/etc/fstab`.

/// `is_unwanted_char(x)`: a byte `mangle` escapes.
fn is_unwanted(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\\')
}

/// `isoctal(a)`: `'0'` to `'7'`.
fn is_octal(b: u8) -> bool {
    b & !7 == b'0'
}

/// `mangle(s)`: blanks, tabs, newlines and backslashes as `\NNN`.
#[must_use]
pub fn mangle(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &b in s {
        if is_unwanted(b) {
            out.push(b'\\');
            // Three octal digits, each below 8.
            out.push(b'0'.saturating_add((b & 0o300) >> 6));
            out.push(b'0'.saturating_add((b & 0o70) >> 3));
            out.push(b'0'.saturating_add(b & 0o7));
        } else {
            out.push(b);
        }
    }
    out
}

/// `unmangle_to_buffer(s, buf, len)`: each `\NNN` whose three digits are
/// octal, as the byte they spell; everything else as it is. The result is a
/// C string, so a `\000` ends it.
#[must_use]
pub fn unmangle_bytes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    // Upstream's buffer is the token's length plus the NUL, and an escape
    // is decoded only while `sz + 3 < len - 1` -- that is, only when all
    // four of its bytes are inside the token.
    let len = s.len();
    while let Some(&b) = s.get(i) {
        if b == 0 {
            break;
        }
        let escape = b == b'\\'
            && i.saturating_add(3) < len
            && s.get(i.saturating_add(1)).copied().is_some_and(is_octal)
            && s.get(i.saturating_add(2)).copied().is_some_and(is_octal)
            && s.get(i.saturating_add(3)).copied().is_some_and(is_octal);
        if escape {
            let d = |k: usize| s.get(i.saturating_add(k)).map_or(0, |&c| c & 7);
            let v = (u32::from(d(1)) << 6) | (u32::from(d(2)) << 3) | u32::from(d(3));
            // 64 * (s[1] & 7) + ...: at most 0o777, cut to a byte.
            let byte = (v & 0xff) as u8;
            if byte == 0 {
                break;
            }
            out.push(byte);
            i = i.saturating_add(4);
        } else {
            out.push(b);
            i = i.saturating_add(1);
        }
    }
    out
}

/// `skip_nonspaces(s)`: up to the next blank or tab.
fn field_end(s: &[u8]) -> usize {
    s.iter()
        .position(|&b| b == b' ' || b == b'\t' || b == 0)
        .unwrap_or(s.len())
}

/// `unmangle(s, &end)`: the field at the front of `s` -- up to the next
/// blank or tab -- unmangled, and the index where it ended; `None` for an
/// empty field.
#[must_use]
pub fn unmangle(s: &[u8]) -> (Option<Vec<u8>>, usize) {
    let end = field_end(s);
    if end == 0 {
        return (None, 0);
    }
    (Some(unmangle_bytes(s.get(..end).unwrap_or_default())), end)
}

/// `unhexmangle_to_buffer(s, buf, len)`: each `\xHH` as its byte -- udev's
/// escaping of LABEL and UUID values.
#[must_use]
pub fn unhexmangle(s: &[u8]) -> Vec<u8> {
    let hex = |c: u8| -> u8 {
        match c {
            b'0'..=b'9' => c.saturating_sub(b'0'),
            b'a'..=b'f' => c.saturating_sub(b'a').saturating_add(10),
            b'A'..=b'F' => c.saturating_sub(b'A').saturating_add(10),
            _ => 0,
        }
    };
    let len = s.len();
    let mut out = Vec::with_capacity(len);
    let mut i = 0usize;
    while let Some(&b) = s.get(i) {
        if b == 0 {
            break;
        }
        let escape = b == b'\\'
            && i.saturating_add(3) < len
            && s.get(i.saturating_add(1)) == Some(&b'x')
            && s.get(i.saturating_add(2))
                .is_some_and(u8::is_ascii_hexdigit)
            && s.get(i.saturating_add(3))
                .is_some_and(u8::is_ascii_hexdigit);
        if escape {
            let hi = s.get(i.saturating_add(2)).copied().map_or(0, hex);
            let lo = s.get(i.saturating_add(3)).copied().map_or(0, hex);
            let byte = (hi << 4) | lo;
            if byte == 0 {
                break;
            }
            out.push(byte);
            i = i.saturating_add(4);
        } else {
            out.push(b);
            i = i.saturating_add(1);
        }
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(
            mangle(b"/mnt/my disk\\x"),
            b"/mnt/my\\040disk\\134x".to_vec()
        );
        assert_eq!(
            unmangle_bytes(b"/mnt/my\\040disk\\134x"),
            b"/mnt/my disk\\x".to_vec()
        );
    }

    #[test]
    fn only_whole_octal_escapes_decode() {
        assert_eq!(unmangle_bytes(b"a\\04"), b"a\\04".to_vec());
        assert_eq!(unmangle_bytes(b"a\\048"), b"a\\048".to_vec());
        assert_eq!(unmangle_bytes(b"a\\000b"), b"a".to_vec());
        // 0o777 is cut to a byte.
        assert_eq!(unmangle_bytes(b"\\777"), vec![0xff]);
    }

    #[test]
    fn fields_end_at_blanks() {
        assert_eq!(
            unmangle(b"/dev/sda1 /boot ext4"),
            (Some(b"/dev/sda1".to_vec()), 9)
        );
        assert_eq!(unmangle(b" x"), (None, 0));
        assert_eq!(unmangle(b""), (None, 0));
    }

    #[test]
    fn hex_escapes_decode() {
        assert_eq!(unhexmangle(b"My\\x20Disk"), b"My Disk".to_vec());
        assert_eq!(unhexmangle(b"a\\x2"), b"a\\x2".to_vec());
    }
}
