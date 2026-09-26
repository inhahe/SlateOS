//! util-linux's `include/carefulputc.h`: the two escapers the parsable
//! formats use. Both work byte by byte with the C locale's `isprint`, which a
//! UTF-8 locale shares for single bytes -- so every byte above 0x7f is
//! written `\x??`, `café` as `caf\xc3\xa9`.

use crate::mbs::{is_cntrl, is_print};

/// `\x%02x`.
fn hex(out: &mut Vec<u8>, b: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.extend_from_slice(b"\\x");
    out.push(HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0'));
    out.push(HEX.get(usize::from(b & 0xf)).copied().unwrap_or(b'0'));
}

/// `fputs_quoted(data, out)`, the export (`NAME="value"`) format: in double
/// quotes, with `"`, `\`, `` ` ``, `$` and every non-printable byte escaped,
/// so the line can be `eval`ed by a shell.
pub fn fputs_quoted(out: &mut Vec<u8>, data: &[u8]) {
    out.push(b'"');
    for &b in data.iter().take_while(|&&b| b != 0) {
        if matches!(b, b'"' | b'\\' | b'`' | b'$') || !is_print(b) || is_cntrl(b) {
            hex(out, b);
        } else {
            out.push(b);
        }
    }
    out.push(b'"');
}

/// `fputs_nonblank(data, out)`, the raw format: blanks (space and tab), `\`
/// and every non-printable byte escaped, so a value never contains the
/// column separator.
pub fn fputs_nonblank(out: &mut Vec<u8>, data: &[u8]) {
    for &b in data.iter().take_while(|&&b| b != 0) {
        if b == b' ' || b == b'\t' || b == b'\\' || !is_print(b) || is_cntrl(b) {
            hex(out, b);
        } else {
            out.push(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_values_can_be_evaled() {
        let mut out = Vec::new();
        fputs_quoted(&mut out, b"a b\"$`\\\xc3\xa9");
        assert_eq!(out, b"\"a b\\x22\\x24\\x60\\x5c\\xc3\\xa9\"");
    }

    #[test]
    fn raw_values_never_hold_a_blank() {
        let mut out = Vec::new();
        fputs_nonblank(&mut out, b"a b\tc\\d\xc3\xa9");
        assert_eq!(out, b"a\\x20b\\x09c\\x5cd\\xc3\\xa9");
    }
}
