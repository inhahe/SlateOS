//! The three encodings a provider's request needs: a URL's percent-encoding,
//! HTTP Basic authentication's base64, and a JSON string.

/// Upper-case hexadecimal digits, for `%XX`.
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Append `value` to `out` percent-encoded: every byte but RFC 3986's
/// unreserved ones (`A-Z a-z 0-9 - . _ ~`) and those in `keep` becomes `%XX`.
///
/// Encoding everything else is right in a query value and in a path segment
/// alike, so a hostname, a token or a password with a `&`, a `/` or a space
/// in it cannot change the shape of the URL it is put into.
pub fn percent_encode_into(out: &mut String, value: &str, keep: &[u8]) {
    for &b in value.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || keep.contains(&b)
        {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(hex_digit(b >> 4)));
            out.push(char::from(hex_digit(b & 0x0f)));
        }
    }
}

/// `value` percent-encoded, as [`percent_encode_into`] with nothing kept.
#[must_use]
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    percent_encode_into(&mut out, value, &[]);
    out
}

/// The hexadecimal digit for a nibble (`n` < 16).
fn hex_digit(n: u8) -> u8 {
    HEX.get(usize::from(n & 0x0f)).copied().unwrap_or(b'0')
}

/// The standard base64 alphabet (RFC 4648 section 4).
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` in standard base64, padded -- what HTTP Basic authentication sends.
#[must_use]
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3).saturating_mul(4));
    for chunk in data.chunks(3) {
        let b0 = chunk.first().copied().unwrap_or(0);
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        let n =
            (u32::from(b0) << 16) | (u32::from(b1.unwrap_or(0)) << 8) | u32::from(b2.unwrap_or(0));
        out.push(b64_digit(n >> 18));
        out.push(b64_digit(n >> 12));
        out.push(if b1.is_some() { b64_digit(n >> 6) } else { '=' });
        out.push(if b2.is_some() { b64_digit(n) } else { '=' });
    }
    out
}

/// The base64 digit for the low six bits of `n`.
fn b64_digit(n: u32) -> char {
    let i = usize::try_from(n & 0x3f).unwrap_or(0);
    char::from(B64.get(i).copied().unwrap_or(b'A'))
}

/// The `Authorization` header's value for HTTP Basic authentication
/// (RFC 7617): `Basic ` and base64 of `user:password`.
#[must_use]
pub fn basic_auth(user: &str, password: &str) -> String {
    let mut pair =
        String::with_capacity(user.len().saturating_add(password.len()).saturating_add(1));
    pair.push_str(user);
    pair.push(':');
    pair.push_str(password);
    let mut out = String::from("Basic ");
    out.push_str(&base64(pair.as_bytes()));
    out
}

/// Append `value` to `out` as a JSON string, quotes included (RFC 8259
/// section 7): `"` and `\` escaped, control characters as `\uXXXX`.
pub fn json_string_into(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let b = u8::try_from(u32::from(c)).unwrap_or(0);
                out.push_str("\\u00");
                out.push(char::from(hex_digit(b >> 4)));
                out.push(char::from(hex_digit(b & 0x0f)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_keeps_only_the_unreserved() {
        assert_eq!(percent_encode("myhome.duckdns.org"), "myhome.duckdns.org");
        assert_eq!(percent_encode("a b&c=d/e?f#g"), "a%20b%26c%3Dd%2Fe%3Ff%23g");
        assert_eq!(percent_encode("~_-."), "~_-.");
        // UTF-8, byte by byte.
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode(""), "");
        let mut kept = String::new();
        percent_encode_into(&mut kept, "abc==&", b"=");
        assert_eq!(kept, "abc==%26");
    }

    /// RFC 4648's own test vectors (section 10).
    #[test]
    fn base64_is_rfc_4648s() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), coded, "{plain:?}");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    /// RFC 7617 section 2's example.
    #[test]
    fn basic_auth_is_rfc_7617s() {
        assert_eq!(
            basic_auth("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[test]
    fn json_strings_escape_what_they_must() {
        let mut out = String::new();
        json_string_into(&mut out, "a\"b\\c\nd\u{1}e/é");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001e/é\"");
    }
}
