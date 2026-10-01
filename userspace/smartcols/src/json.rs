//! util-linux's `lib/jsonwrt.c`: the JSON writer behind every `--json`
//! option, byte for byte -- three-space indentation, `,\n` between named
//! members but `,` alone between array elements, names lowercased, and a
//! closing root that ends the document with `\n}\n`.

/// What [`JsonWriter::open`] and [`JsonWriter::close`] open and close:
/// `UL_JSON_OBJECT`, `UL_JSON_ARRAY`, `UL_JSON_VALUE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Object,
    Array,
    Value,
}

/// `struct ul_jsonwrt`, writing into a byte buffer.
#[derive(Debug, Default)]
pub struct JsonWriter {
    indent: usize,
    after_close: bool,
}

impl JsonWriter {
    /// `ul_jsonwrt_init(fmt, out, indent)`.
    #[must_use]
    pub fn new(indent: usize) -> Self {
        JsonWriter {
            indent,
            after_close: false,
        }
    }

    /// `ul_jsonwrt_indent`: three spaces a level.
    fn put_indent(&self, out: &mut Vec<u8>) {
        for _ in 0..self.indent {
            out.extend_from_slice(b"   ");
        }
    }

    /// `ul_jsonwrt_open(fmt, name, type)`.
    pub fn open(&mut self, out: &mut Vec<u8>, name: Option<&[u8]>, kind: Kind) {
        if let Some(name) = name {
            if self.after_close {
                out.extend_from_slice(b",\n");
            }
            self.put_indent(out);
            quoted(out, name, Case::Lower);
        } else if self.after_close {
            out.push(b',');
        } else {
            self.put_indent(out);
        }
        let named = name.is_some();
        match kind {
            Kind::Object => {
                out.extend_from_slice(if named { b": {\n" } else { b"{\n" });
                self.indent = self.indent.saturating_add(1);
            }
            Kind::Array => {
                out.extend_from_slice(if named { b": [\n" } else { b"[\n" });
                self.indent = self.indent.saturating_add(1);
            }
            Kind::Value => out.extend_from_slice(if named { b": " } else { b" " }),
        }
        self.after_close = false;
    }

    /// `ul_jsonwrt_close(fmt, type)`. Closing at the first level, whatever
    /// is being closed, ends the document.
    pub fn close(&mut self, out: &mut Vec<u8>, kind: Kind) {
        if self.indent == 1 {
            out.extend_from_slice(b"\n}\n");
            self.indent = 0;
            self.after_close = true;
            return;
        }
        match kind {
            Kind::Object => {
                self.indent = self.indent.saturating_sub(1);
                out.push(b'\n');
                self.put_indent(out);
                out.push(b'}');
            }
            Kind::Array => {
                self.indent = self.indent.saturating_sub(1);
                out.push(b'\n');
                self.put_indent(out);
                out.push(b']');
            }
            Kind::Value => {}
        }
        self.after_close = true;
    }

    /// `ul_jsonwrt_value_raw`: `data` as it is -- a number -- or `null`
    /// when there is none.
    pub fn value_raw(&mut self, out: &mut Vec<u8>, name: Option<&[u8]>, data: &[u8]) {
        self.open(out, name, Kind::Value);
        if data.is_empty() {
            out.extend_from_slice(b"null");
        } else {
            out.extend_from_slice(data);
        }
        self.close(out, Kind::Value);
    }

    /// `ul_jsonwrt_value_s`: `data` as a quoted string, or `null`.
    pub fn value_s(&mut self, out: &mut Vec<u8>, name: Option<&[u8]>, data: &[u8]) {
        self.open(out, name, Kind::Value);
        if data.is_empty() {
            out.extend_from_slice(b"null");
        } else {
            quoted(out, data, Case::Keep);
        }
        self.close(out, Kind::Value);
    }

    /// `ul_jsonwrt_value_boolean`.
    pub fn value_boolean(&mut self, out: &mut Vec<u8>, name: Option<&[u8]>, data: bool) {
        self.open(out, name, Kind::Value);
        out.extend_from_slice(if data { b"true" } else { b"false" });
        self.close(out, Kind::Value);
    }

    /// `ul_jsonwrt_value_null`.
    pub fn value_null(&mut self, out: &mut Vec<u8>, name: Option<&[u8]>) {
        self.open(out, name, Kind::Value);
        out.extend_from_slice(b"null");
        self.close(out, Kind::Value);
    }
}

/// `fputs_quoted_case_json`'s `dir`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Keep,
    Lower,
}

/// `fputs_quoted_case_json`: `"` and `\` escaped, the five short-hand control
/// characters as `\b \t \n \f \r`, any other below 0x20 as `\u00xx`, and
/// every other byte -- UTF-8 included -- as it is; a name has its ASCII
/// letters lowercased.
fn quoted(out: &mut Vec<u8>, data: &[u8], case: Case) {
    out.push(b'"');
    for &c in data.iter().take_while(|&&c| c != 0) {
        match c {
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            0x20.. => out.push(if case == Case::Lower {
                c.to_ascii_lowercase()
            } else {
                c
            }),
            0x08 => out.extend_from_slice(b"\\b"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'\r' => out.extend_from_slice(b"\\r"),
            _ => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.extend_from_slice(b"\\u00");
                out.push(HEX.get(usize::from(c >> 4)).copied().unwrap_or(b'0'));
                out.push(HEX.get(usize::from(c & 0xf)).copied().unwrap_or(b'0'));
            }
        }
    }
    out.push(b'"');
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_table_document_is_util_linuxs() {
        let mut out = Vec::new();
        let mut j = JsonWriter::new(0);
        j.open(&mut out, None, Kind::Object);
        j.open(&mut out, Some(b"MEMORY"), Kind::Array);
        for (range, size) in [(&b"0x0-0xf"[..], &b"128M"[..]), (b"0x10-0x1f", b"")] {
            j.open(&mut out, None, Kind::Object);
            j.value_s(&mut out, Some(b"range"), range);
            j.value_raw(&mut out, Some(b"size"), size);
            j.close(&mut out, Kind::Object);
        }
        j.close(&mut out, Kind::Array);
        j.close(&mut out, Kind::Object);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\n   \"memory\": [\n      {\n         \"range\": \"0x0-0xf\",\n         \"size\": 128M\n      },{\n         \"range\": \"0x10-0x1f\",\n         \"size\": null\n      }\n   ]\n}\n"
        );
    }

    #[test]
    fn strings_are_escaped_as_upstream_escapes_them() {
        let mut out = Vec::new();
        quoted(&mut out, b"a\"b\\c\td\x01e\xc3\xa9", Case::Keep);
        assert_eq!(out, b"\"a\\\"b\\\\c\\td\\u0001e\xc3\xa9\"");
        let mut out = Vec::new();
        quoted(&mut out, b"MAJ:MIN", Case::Lower);
        assert_eq!(out, b"\"maj:min\"");
    }
}
