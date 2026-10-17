//! `script.c`: sfdisk scripts -- a partition table described as text,
//! read to create a layout and written (as text or JSON) to dump one.
//!
//! A script is headers (`label: gpt`, `unit: sectors`, ...) and a [`Table`]
//! of partitions, one per line, either `name=value` pairs
//! (`/dev/sda1 : start=2048, size=4096, type=L`) or bare comma-separated
//! values (`2048,4096,L,*`). Headers are only read before the first
//! partition line.
//!
//! # How it differs from the C
//!
//! * C's script holds a reference to a context and reaches it for label
//!   names, the sector size and messages; a context in turn holds the
//!   script it follows. Rust cannot have both owners, so the script holds
//!   none, and each function that needs one takes the context as an
//!   argument -- the context sfdisk created the script with.
//! * `fdisk_script_set_fgets` and `fdisk_script_set_userdata`, a callback
//!   and its data for reading a line, are the [`ScriptInput`] trait: the
//!   caller passes its own reader where C passes a `FILE` and a hook.
//! * The parser works, as upstream's does, in place in the caller's line
//!   buffer: it writes NULs into it to cut tokens, and one malformed line
//!   (a value list ending `, ` with no newline at the end of the file)
//!   makes it read past a line's terminator into what the previous, longer
//!   line left in the buffer. Keeping the buffer is what keeps that
//!   outcome upstream's.

use std::cell::RefCell;
use std::rc::Rc;

use smartcols::careful::fputs_quoted;
use smartcols::json::{JsonWriter, Kind};

use crate::context::Context;
use crate::errno::{EINVAL, ENOTSUP, ERANGE};
use crate::item::{ItemData, LabelItem};
use crate::partition::Partition;
use crate::parttype::{
    PARSE_ALIAS, PARSE_DATA, PARSE_DATALAST, PARSE_DEPRECATED, PARSE_NAME, PARSE_SHORTCUT,
};
use crate::table::Table;
use crate::utils::{c_isspace, c_str, div, errno_of, partname, rem, strcasecmp, strtol};
use crate::{
    GPT_NPARTITIONS_DEFAULT, LabelType, MOVE_DOWN, MOVE_NONE, MOVE_UP, RESIZE_ENLARGE, RESIZE_NONE,
    RESIZE_REDUCE,
};

/// `BUFSIZ`: the line buffer [`Script::read_file`] reads into.
pub const BUFSIZ: usize = 8192;

/// A script shared between its owner and a context following it
/// ([`Context::set_script`]).
pub type ScriptRef = Rc<RefCell<Script>>;

/// `FDISK_SCRIPT_PARTTYPE_PARSE_FLAGS`: how a `type=` value is read.
const PARTTYPE_PARSE_FLAGS: i32 =
    PARSE_DATA | PARSE_DATALAST | PARSE_SHORTCUT | PARSE_ALIAS | PARSE_NAME | PARSE_DEPRECATED;

/// The headers a script may have.
const SUPPORTED: [&[u8]; 9] = [
    b"label",
    b"unit",
    b"label-id",
    b"device",
    b"grain",
    b"first-lba",
    b"last-lba",
    b"table-length",
    b"sector-size",
];

/// `struct fdisk_script`.
#[derive(Clone, Debug, Default)]
pub struct Script {
    /// The partitions.
    table: Table,
    /// The headers, `(name, value)`, in the order they were set.
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Lines read so far, blank lines and comments included.
    nlines: usize,
    /// The label partition types are read for: the position, in the
    /// context, of the driver the `label` header names (or of the
    /// context's current label without one), looked up when first needed.
    label: Option<usize>,
    /// The `sector-size` header, 0 without one.
    sector_size: u64,
    /// Dump as JSON.
    json: bool,
    /// A `label` header was read.
    force_label: bool,
}

/// Where [`Script::read_line`] reads from: what `fgets` and `feof` do to a
/// `FILE`, or what a program's `fdisk_script_set_fgets` hook does instead.
pub trait ScriptInput {
    /// `fgets (buf, sizeof buf, f)`: the next line, through its `\n`, or
    /// the first `buf.len() - 1` bytes of it, stored at the front of `buf`
    /// and followed by a NUL, the rest of `buf` untouched. False when the
    /// file ends before a byte is read, or on a read error.
    fn fgets(&mut self, buf: &mut [u8]) -> bool;

    /// `feof (f)`: whether a read has met the end of the file.
    fn feof(&self) -> bool;
}

/// A [`ScriptInput`] reading any byte stream as `fgets` reads a `FILE`.
#[derive(Debug)]
pub struct FileInput<R> {
    reader: std::io::BufReader<R>,
    eof: bool,
}

impl<R: std::io::Read> FileInput<R> {
    /// Reads from `reader`.
    pub fn new(reader: R) -> Self {
        Self {
            reader: std::io::BufReader::new(reader),
            eof: false,
        }
    }
}

impl<R: std::io::Read> ScriptInput for FileInput<R> {
    fn fgets(&mut self, buf: &mut [u8]) -> bool {
        use std::io::BufRead;
        let Some(max) = buf.len().checked_sub(1) else {
            return false;
        };
        let mut n = 0usize;
        let mut failed = false;
        while n < max {
            let byte = match self.reader.fill_buf() {
                Ok(data) => {
                    let Some(&b) = data.first() else {
                        self.eof = true;
                        break;
                    };
                    b
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            self.reader.consume(1);
            put(buf, n, byte);
            n = n.wrapping_add(1);
            if byte == b'\n' {
                break;
            }
        }
        if failed || (n == 0 && self.eof) {
            return false;
        }
        put(buf, n, 0);
        true
    }

    fn feof(&self) -> bool {
        self.eof
    }
}

/// A byte of the line buffer as C reads it: NUL past the end.
fn at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

/// Stores a byte into the line buffer; past its end, nothing.
fn put(b: &mut [u8], i: usize, v: u8) {
    if let Some(c) = b.get_mut(i) {
        *c = v;
    }
}

/// The C string at `i`.
fn cstr_at(b: &[u8], i: usize) -> &[u8] {
    c_str(b.get(i..).unwrap_or(&[]))
}

/// `strchr (b + i, c)`, for a `c` other than NUL.
fn strchr(b: &[u8], i: usize, c: u8) -> Option<usize> {
    cstr_at(b, i)
        .iter()
        .position(|&x| x == c)
        .map(|k| i.wrapping_add(k))
}

/// `isblank`.
fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// `skip_blank (b + i)`.
fn skip_blank(b: &[u8], mut i: usize) -> usize {
    while is_blank(at(b, i)) {
        i = i.wrapping_add(1);
    }
    i
}

/// `rtrim_whitespace (b + i)`: the string's new length.
fn rtrim_whitespace(b: &mut [u8], i: usize) -> usize {
    let mut n = cstr_at(b, i).len();
    while n > 0 {
        n = n.wrapping_sub(1);
        if !c_isspace(at(b, i.wrapping_add(n))) {
            n = n.wrapping_add(1);
            break;
        }
    }
    put(b, i.wrapping_add(n), 0);
    n
}

/// `ltrim_whitespace (b + i)`: the string moved to the front of its place.
fn ltrim_whitespace(b: &mut [u8], i: usize) {
    let mut p = i;
    while at(b, p) != 0 && c_isspace(at(b, p)) {
        p = p.wrapping_add(1);
    }
    let len = cstr_at(b, p).len();
    if p > i {
        if p.wrapping_add(len) <= b.len() {
            b.copy_within(p..p.wrapping_add(len), i);
        }
        put(b, i.wrapping_add(len), 0);
    }
}

/// Whether `s` begins with `prefix`, ignoring ASCII case (`strncasecmp`).
fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.get(..prefix.len())
        .is_some_and(|h| h.eq_ignore_ascii_case(prefix))
}

/// `is_header_line (s)`: `name: value` with no `=` anywhere.
fn is_header_line(b: &[u8], s: usize) -> bool {
    let Some(p) = strchr(b, s, b':') else {
        return false;
    };
    !(p == s || at(b, p.wrapping_add(1)) == 0 || strchr(b, s, b'=').is_some())
}

/// `partno_from_devname (s)`: the partition a device name's trailing
/// number names, counted from 0, or -1.
fn partno_from_devname(b: &mut [u8], s: usize) -> i32 {
    if at(b, s) == 0 {
        return -1;
    }
    let end = s.wrapping_add(rtrim_whitespace(b, s));
    let mut p = end;
    while p > s && at(b, p.wrapping_sub(1)).is_ascii_digit() {
        p = p.wrapping_sub(1);
    }
    if p == end {
        return -1;
    }
    let (num, used, err) = strtol(cstr_at(b, p), 10);
    if err != 0 || used == 0 {
        return -1;
    }
    match i32::try_from(num) {
        Ok(n) => n.wrapping_sub(1),
        Err(_) => -1,
    }
}

/// `next_token (&str)`: the next token -- up to a blank, `,` or `;`, or
/// between double quotes -- cut with a NUL where it ends; its start, with
/// `*str` moved past it and its terminator. None for a token with no
/// terminator.
fn next_token(b: &mut [u8], str: &mut usize) -> Option<usize> {
    let mut tk_begin: Option<usize> = None;
    let mut tk_end: Option<usize> = None;
    let mut open_quote = false;
    let mut p = *str;
    while at(b, p) != 0 {
        let c = at(b, p);
        if tk_begin.is_none() {
            if is_blank(c) {
                p = p.wrapping_add(1);
                continue;
            }
            tk_begin = Some(if c == b'"' { p.wrapping_add(1) } else { p });
        }
        if c == b'"' {
            open_quote = !open_quote;
        }
        if open_quote {
            p = p.wrapping_add(1);
            continue;
        }
        if is_blank(c) || c == b',' || c == b';' || c == b'"' {
            tk_end = Some(p);
        } else if at(b, p.wrapping_add(1)) == 0 {
            tk_end = Some(p.wrapping_add(1));
        }
        if tk_begin.is_some() && tk_end.is_some() {
            break;
        }
        p = p.wrapping_add(1);
    }
    let tk_end = tk_end?;
    let mut end = tk_end;
    let mut terminated = false;
    // The closing quote.
    if at(b, end) == b'"' {
        end = end.wrapping_add(1);
    }
    // Ended by blanks, perhaps before a `,` or `;`.
    if is_blank(at(b, end)) {
        end = skip_blank(b, end);
        terminated = true;
    }
    if at(b, end) == b',' || at(b, end) == b';' {
        end = end.wrapping_add(1);
        terminated = true;
    } else if at(b, end) == 0 {
        terminated = true;
    }
    if !terminated {
        return None;
    }
    end = skip_blank(b, end);
    put(b, tk_end, 0);
    *str = end;
    tk_begin
}

/// `is_default_value (&str)`: `-`, a blank, or nothing before the next
/// `,` or `;` (or the end): the field's default. `*str` is moved past it.
fn is_default_value(b: &[u8], str: &mut usize) -> bool {
    let mut p = skip_blank(b, *str);
    let mut blank = false;
    if at(b, p) == b'-' {
        let x = p.wrapping_add(1);
        p = skip_blank(b, x);
        blank = x < p;
    }
    if at(b, p) == b';' || at(b, p) == b',' {
        *str = p.wrapping_add(1);
        return true;
    }
    if at(b, p) == 0 || blank {
        *str = p;
        return true;
    }
    false
}

/// `next_string (&s, &str)`: the next token, copied.
fn next_string(b: &mut [u8], s: &mut usize) -> Result<Vec<u8>, i32> {
    next_token(b, s)
        .map(|tk| cstr_at(b, tk).to_vec())
        .ok_or(-EINVAL)
}

/// `skip_optional_sign (&str)`: a leading `-` or `+` (after blanks)
/// skipped and returned, or 0 with `*str` as it was.
fn skip_optional_sign(b: &[u8], str: &mut usize) -> u8 {
    let p = skip_blank(b, *str);
    let c = at(b, p);
    if c == b'-' || c == b'+' {
        *str = p.wrapping_add(1);
        return c;
    }
    0
}

/// `parse_size (str, &num, &pow)`: a size, and the power of its suffix
/// (0 without one); upstream's negative errno on failure.
fn parse_size(s: &[u8]) -> Result<(u64, i32), i32> {
    match ulstrutils::parse_size_power(s) {
        (Ok(v), _, pow) => Ok((v, pow)),
        (Err(ulstrutils::NumErr::Range), _, _) => Err(-ERANGE),
        (Err(_), _, _) => Err(-EINVAL),
    }
}

/// `unhexmangle_string (s)`: `\xHH` escapes decoded, cut at a decoded NUL
/// as the C string is.
fn unhexmangle(s: &[u8]) -> Vec<u8> {
    let hex = |c: u8| {
        char::from(c)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        let escape = if c == b'\\' && at(s, i.wrapping_add(1)) == b'x' {
            hex(at(s, i.wrapping_add(2))).zip(hex(at(s, i.wrapping_add(3))))
        } else {
            None
        };
        if let Some((h, l)) = escape {
            out.push((h << 4) | l);
            i = i.wrapping_add(4);
        } else {
            out.push(c);
            i = i.wrapping_add(1);
        }
    }
    c_str(&out).to_vec()
}

/// `script_get_label (dp)`: the position of the driver `type=` values are
/// read for, looked up on first use.
fn script_label(
    label: &mut Option<usize>,
    headers: &[(Vec<u8>, Vec<u8>)],
    cxt: &Context,
) -> Option<usize> {
    if label.is_none() {
        *label = cxt.get_label(header(headers, b"label"));
    }
    *label
}

/// The value of header `name` (names compared ignoring case).
fn header<'a>(headers: &'a [(Vec<u8>, Vec<u8>)], name: &[u8]) -> Option<&'a [u8]> {
    headers
        .iter()
        .find(|(n, _)| strcasecmp(n, name) == 0)
        .map(|(_, v)| v.as_slice())
}

/// Whether `pa`'s attributes are dumped: always but on an MBR, whose
/// attribute 0x80 is the `bootable` flag written instead.
fn dumps_attrs(label: &mut Option<usize>, headers: &[(Vec<u8>, Vec<u8>)], cxt: &Context) -> bool {
    script_label(label, headers, cxt)
        .and_then(|i| cxt.label_at(i))
        .is_none_or(|lb| lb.id != LabelType::Dos)
}

impl Script {
    /// `fdisk_new_script (cxt)`: an empty script.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `fdisk_reset_script`: no partitions and no headers.
    fn reset(&mut self) {
        self.table.reset();
        self.headers.clear();
    }

    /// `fdisk_script_get_header (dp, name)`: the value of header `name`,
    /// whose case does not matter.
    #[must_use]
    pub fn get_header(&self, name: &[u8]) -> Option<&[u8]> {
        header(&self.headers, name)
    }

    /// `fdisk_script_set_header (dp, name, data)`: header `name` set to
    /// `data`, added at the end if new, or removed when `data` is None.
    pub fn set_header(&mut self, name: &[u8], data: Option<&[u8]>) -> i32 {
        let fi = self
            .headers
            .iter()
            .position(|(n, _)| strcasecmp(n, name) == 0);
        match (fi, data) {
            (None, None) => return 0,
            (Some(i), None) => {
                self.headers.remove(i);
                return 0;
            }
            (None, Some(d)) => self.headers.push((name.to_vec(), d.to_vec())),
            (Some(i), Some(d)) => {
                if let Some(h) = self.headers.get_mut(i) {
                    h.1 = d.to_vec();
                }
            }
        }
        if name == b"label" {
            self.label = None;
        }
        0
    }

    /// `fdisk_script_get_table (dp)`: the partitions.
    pub fn get_table(&mut self) -> &mut Table {
        &mut self.table
    }

    /// `fdisk_script_set_table (dp, tb)`: the partitions replaced.
    pub fn set_table(&mut self, tb: Table) {
        self.table = tb;
    }

    /// `fdisk_script_get_nlines (dp)`: lines read so far.
    #[must_use]
    pub fn get_nlines(&self) -> usize {
        self.nlines
    }

    /// `fdisk_script_has_force_label (dp)`: whether a `label` header was
    /// read.
    #[must_use]
    pub fn has_force_label(&self) -> bool {
        self.force_label
    }

    /// `fdisk_script_enable_json (dp, json)`: dump as JSON.
    pub fn enable_json(&mut self, json: bool) {
        self.json = json;
    }

    /// `fdisk_script_read_context (dp, cxt)`: the script made from `cxt`'s
    /// current label -- its partitions, and the headers that recreate it.
    pub fn read_context(&mut self, cxt: &mut Context) -> i32 {
        self.reset();
        let Some(name) = cxt
            .get_label(None)
            .and_then(|i| cxt.label_at(i))
            .map(|l| l.name)
        else {
            return -EINVAL;
        };
        let rc = cxt.get_partitions(&mut self.table);
        if rc != 0 {
            return rc;
        }
        let mut rc = self.set_header(b"label", Some(name.as_bytes()));
        if rc == 0 {
            if let Ok(Some(id)) = cxt.get_disklabel_id() {
                rc = self.set_header(b"label-id", Some(&id));
            }
        }
        if rc == 0 {
            if let Some(dev) = cxt.dev_path.clone() {
                rc = self.set_header(b"device", Some(&dev));
            }
        }
        if rc == 0 {
            rc = self.set_header(b"unit", Some(b"sectors"));
        }
        if rc == 0 && cxt.is_labeltype(LabelType::Gpt) {
            let num64 = |it: &LabelItem| match it.data {
                ItemData::U64(v) => v,
                _ => 0,
            };
            let mut item = LabelItem::default();
            rc = cxt.get_disklabel_item(crate::gpt::item::FIRSTLBA, &mut item);
            if rc == 0 {
                rc = self.set_header(b"first-lba", Some(num64(&item).to_string().as_bytes()));
            }
            if rc == 0 {
                rc = cxt.get_disklabel_item(crate::gpt::item::LASTLBA, &mut item);
            }
            if rc == 0 {
                rc = self.set_header(b"last-lba", Some(num64(&item).to_string().as_bytes()));
            }
            if rc == 0 {
                let n = cxt.get_npartitions();
                if u32::try_from(n).ok() != Some(GPT_NPARTITIONS_DEFAULT) {
                    rc = self.set_header(b"table-length", Some(n.to_string().as_bytes()));
                }
            }
        }
        if rc == 0 && cxt.get_grain_size() != 2048 * 512 {
            rc = self.set_header(b"grain", Some(cxt.get_grain_size().to_string().as_bytes()));
        }
        if rc == 0 {
            rc = self.set_header(
                b"sector-size",
                Some(cxt.get_sector_size().to_string().as_bytes()),
            );
        }
        rc
    }

    /// `fdisk_script_write_file (dp, f)`: the script, as JSON or as an
    /// sfdisk dump, appended to `out`.
    pub fn write_file(&mut self, cxt: &Context, out: &mut Vec<u8>) -> i32 {
        if self.json {
            self.write_file_json(cxt, out)
        } else {
            self.write_file_sfdisk(cxt, out)
        }
    }

    /// `write_file_json (dp, f)`.
    fn write_file_json(&mut self, cxt: &Context, out: &mut Vec<u8>) -> i32 {
        let Self {
            table,
            headers,
            label,
            ..
        } = self;
        let mut json = JsonWriter::new(0);
        json.open(out, None, Kind::Object);
        json.open(out, Some(b"partitiontable"), Kind::Object);

        let mut devname: Option<&[u8]> = None;
        for (n, d) in headers.iter() {
            let (name, num): (&[u8], bool) = match n.as_slice() {
                b"first-lba" => (b"firstlba", true),
                b"last-lba" => (b"lastlba", true),
                b"sector-size" => (b"sectorsize", true),
                b"label-id" => (b"id", false),
                other => (other, false),
            };
            if num {
                json.value_raw(out, Some(name), d);
            } else {
                json.value_s(out, Some(name), d);
            }
            if name == b"device" {
                devname = Some(d);
            }
        }

        if !table.is_empty() {
            json.open(out, Some(b"partitions"), Kind::Array);
            for pa in table.iter() {
                json.open(out, None, Kind::Object);
                if let Some(p) = devname.and_then(|d| partname(Some(d), pa.partno.wrapping_add(1)))
                {
                    json.value_s(out, Some(b"node"), &p);
                }
                if pa.has_start() {
                    json.value_raw(out, Some(b"start"), pa.start.to_string().as_bytes());
                }
                if pa.has_size() {
                    json.value_raw(out, Some(b"size"), pa.size.to_string().as_bytes());
                }
                if let Some(t) = pa.ty.as_ref() {
                    if let Some(s) = t.get_string() {
                        json.value_s(out, Some(b"type"), s);
                    } else {
                        json.open(out, Some(b"type"), Kind::Value);
                        out.extend_from_slice(format!("\"{:x}\"", t.code).as_bytes());
                        json.close(out, Kind::Value);
                    }
                }
                if let Some(u) = pa.uuid.as_deref() {
                    json.value_s(out, Some(b"uuid"), u);
                }
                if let Some(n) = pa.name.as_deref().filter(|n| !n.is_empty()) {
                    json.value_s(out, Some(b"name"), n);
                }
                if let Some(a) = pa.attrs.as_deref() {
                    if dumps_attrs(label, headers, cxt) {
                        json.value_s(out, Some(b"attrs"), a);
                    }
                }
                if pa.is_bootable() {
                    json.value_boolean(out, Some(b"bootable"), true);
                }
                json.close(out, Kind::Object);
            }
            json.close(out, Kind::Array);
        }
        json.close(out, Kind::Object);
        json.close(out, Kind::Object);
        0
    }

    /// `write_file_sfdisk (dp, f)`.
    fn write_file_sfdisk(&mut self, cxt: &Context, out: &mut Vec<u8>) -> i32 {
        let Self {
            table,
            headers,
            label,
            ..
        } = self;
        let mut devname: Option<&[u8]> = None;
        for (n, d) in headers.iter() {
            out.extend_from_slice(n);
            out.extend_from_slice(b": ");
            out.extend_from_slice(d);
            out.push(b'\n');
            if n == b"device" {
                devname = Some(d);
            }
        }
        if table.is_empty() {
            return 0;
        }
        out.push(b'\n');
        for pa in table.iter() {
            let num = pa.partno.wrapping_add(1);
            if let Some(p) = devname.and_then(|d| partname(Some(d), num)) {
                out.extend_from_slice(&p);
                out.extend_from_slice(b" :");
            } else {
                out.extend_from_slice(format!("{num} :").as_bytes());
            }
            if pa.has_start() {
                out.extend_from_slice(format!(" start={:12}", pa.start).as_bytes());
            }
            if pa.has_size() {
                out.extend_from_slice(format!(", size={:12}", pa.size).as_bytes());
            }
            if let Some(t) = pa.ty.as_ref() {
                if let Some(s) = t.get_string() {
                    out.extend_from_slice(b", type=");
                    out.extend_from_slice(s);
                } else {
                    out.extend_from_slice(format!(", type={:x}", t.code).as_bytes());
                }
            }
            if let Some(u) = pa.uuid.as_deref() {
                out.extend_from_slice(b", uuid=");
                out.extend_from_slice(c_str(u));
            }
            if let Some(n) = pa.name.as_deref().filter(|n| !n.is_empty()) {
                out.extend_from_slice(b", name=");
                fputs_quoted(out, n);
            }
            if let Some(a) = pa.attrs.as_deref() {
                if dumps_attrs(label, headers, cxt) {
                    out.extend_from_slice(b", attrs=\"");
                    out.extend_from_slice(c_str(a));
                    out.push(b'"');
                }
            }
            if pa.is_bootable() {
                out.extend_from_slice(b", bootable");
            }
            out.push(b'\n');
        }
        0
    }

    /// `parse_line_header (dp, s)`: a `name: value` header line.
    fn parse_line_header(&mut self, cxt: &mut Context, b: &mut [u8], s: usize) -> i32 {
        if at(b, s) == 0 {
            return -EINVAL;
        }
        let name = s;
        let Some(colon) = strchr(b, s, b':') else {
            return -EINVAL;
        };
        put(b, colon, 0);
        let value = colon.wrapping_add(1);
        ltrim_whitespace(b, name);
        rtrim_whitespace(b, name);
        ltrim_whitespace(b, value);
        rtrim_whitespace(b, value);
        let (name, value) = (cstr_at(b, name).to_vec(), cstr_at(b, value).to_vec());
        if name.is_empty() || value.is_empty() {
            return -EINVAL;
        }
        if !SUPPORTED.contains(&name.as_slice()) {
            return -ENOTSUP;
        }
        match name.as_slice() {
            b"label" => {
                if cxt.get_label(Some(&value)).is_none() {
                    // An unknown label.
                    return -EINVAL;
                }
                self.force_label = true;
            }
            b"sector-size" => {
                let Ok(x) = ulstrutils::ul_strtou64(&value, 10) else {
                    return -EINVAL;
                };
                if x % 512 != 0 {
                    return -ERANGE;
                }
                self.sector_size = x;
                if self.sector_size != 0
                    && cxt.sector_size != 0
                    && self.sector_size != cxt.sector_size
                {
                    cxt.warnx(
                        b"The script and device sector size differ; the sizes will be recalculated to match the \
                          device.",
                    );
                }
            }
            // Only sectors.
            b"unit" if value != b"sectors" => return -EINVAL,
            _ => {}
        }
        self.set_header(&name, Some(&value))
    }

    /// `recount_script2device_sectors (dp, &num)`: a number of the
    /// script's sectors in the device's.
    fn recount_script2device_sectors(&self, cxt: &Context, num: &mut u64) -> i32 {
        if self.sector_size == 0 || cxt.sector_size == 0 {
            return 0;
        }
        if self.sector_size > cxt.sector_size {
            *num = num.wrapping_mul(div(self.sector_size, cxt.sector_size));
        } else if self.sector_size < cxt.sector_size {
            let x = div(cxt.sector_size, self.sector_size);
            if rem(*num, x) != 0 {
                return -EINVAL;
            }
            *num = div(*num, x);
        }
        0
    }

    /// `parse_start_value (dp, pa, &str)`: `start=`'s value, or the first
    /// of a value list.
    fn parse_start_value(
        &self,
        cxt: &mut Context,
        pa: &mut Partition,
        b: &mut [u8],
        str: &mut usize,
    ) -> i32 {
        if is_default_value(b, str) {
            pa.set_start_follow_default(true);
            return 0;
        }
        let Some(mut tk) = next_token(b, str) else {
            return -EINVAL;
        };
        if cstr_at(b, tk) == b"+" {
            pa.set_start_follow_default(true);
            pa.movestart = MOVE_DOWN;
            return 0;
        }
        let sign = skip_optional_sign(b, &mut tk);
        let rc = match parse_size(cstr_at(b, tk)) {
            Ok((mut num, pow)) => {
                if pow != 0 {
                    // `<num><suffix>`: bytes.
                    if cxt.sector_size == 0 {
                        return -EINVAL;
                    }
                    num = div(num, cxt.sector_size);
                } else {
                    let rc = self.recount_script2device_sectors(cxt, &mut num);
                    if rc != 0 {
                        cxt.warnx(b"Can't recalculate partition start to the device sectors");
                        return rc;
                    }
                }
                // UNDEF cannot be set; a start of all ones stays unset.
                let _ = pa.set_start(num);
                pa.movestart = match sign {
                    b'-' => MOVE_DOWN,
                    b'+' => MOVE_UP,
                    _ => MOVE_NONE,
                };
                0
            }
            Err(rc) => rc,
        };
        pa.set_start_follow_default(false);
        rc
    }

    /// `parse_size_value (dp, pa, &str)`: `size=`'s value, or the second
    /// of a value list.
    fn parse_size_value(
        &self,
        cxt: &mut Context,
        pa: &mut Partition,
        b: &mut [u8],
        str: &mut usize,
    ) -> i32 {
        if is_default_value(b, str) {
            pa.end_follow_default = true;
            return 0;
        }
        let Some(mut tk) = next_token(b, str) else {
            return -EINVAL;
        };
        if cstr_at(b, tk) == b"+" {
            pa.end_follow_default = true;
            pa.resize = RESIZE_ENLARGE;
            return 0;
        }
        // `[+-]<number>[<suffix>]`
        let sign = skip_optional_sign(b, &mut tk);
        let rc = match parse_size(cstr_at(b, tk)) {
            Ok((mut num, pow)) => {
                if pow != 0 {
                    // `<size><suffix>`: bytes.
                    if cxt.sector_size == 0 {
                        return -EINVAL;
                    }
                    num = div(num, cxt.sector_size);
                } else {
                    // A number of sectors.
                    pa.set_size_explicit(true);
                    let rc = self.recount_script2device_sectors(cxt, &mut num);
                    if rc != 0 {
                        cxt.warnx(b"Can't recalculate partition size to the device sectors");
                        return rc;
                    }
                }
                // UNDEF cannot be set; a size of all ones stays unset.
                let _ = pa.set_size(num);
                pa.resize = match sign {
                    b'-' => RESIZE_REDUCE,
                    b'+' => RESIZE_ENLARGE,
                    _ => RESIZE_NONE,
                };
                0
            }
            Err(rc) => rc,
        };
        pa.end_follow_default = false;
        rc
    }

    /// A `type=` value, or the third of a value list, read for the
    /// script's label.
    fn parse_type(&mut self, cxt: &Context, t: &[u8]) -> Option<crate::PartType> {
        let li = script_label(&mut self.label, &self.headers, cxt)?;
        cxt.label_at(li)?.advparse_parttype(t, PARTTYPE_PARSE_FLAGS)
    }

    /// `parse_line_nameval (dp, s)`: `[<device> :] name=value, ...`.
    fn parse_line_nameval(&mut self, cxt: &mut Context, b: &mut [u8], s: usize) -> i32 {
        let mut pa = Partition::new();
        pa.set_start_follow_default(true);
        pa.end_follow_default = true;
        pa.set_partno_follow_default(true);

        // The partition number, from the device name.
        let colon = strchr(b, s, b':');
        let eq = strchr(b, s, b'=');
        let mut p = s;
        if let Some(c) = colon.filter(|&c| eq.is_none_or(|x| c < x)) {
            put(b, c, 0);
            p = c.wrapping_add(1);
            if let Ok(pno) = usize::try_from(partno_from_devname(b, s)) {
                pa.set_partno_follow_default(false);
                // Never UNDEF: it came from an `int`.
                let _ = pa.set_partno(pno);
            }
        }

        let mut rc = 0;
        while rc == 0 && at(b, p) != 0 {
            p = skip_blank(b, p);
            let rest = cstr_at(b, p);
            if starts_with_ci(rest, b"start=") {
                p = p.wrapping_add(6);
                rc = self.parse_start_value(cxt, &mut pa, b, &mut p);
            } else if starts_with_ci(rest, b"size=") {
                p = p.wrapping_add(5);
                rc = self.parse_size_value(cxt, &mut pa, b, &mut p);
            } else if starts_with_ci(rest, b"bootable") {
                // next_token skips any blanks after it.
                match next_token(b, &mut p) {
                    Some(tk) if strcasecmp(cstr_at(b, tk), b"bootable") == 0 => pa.boot = 1,
                    _ => rc = -EINVAL,
                }
            } else if starts_with_ci(rest, b"attrs=") {
                p = p.wrapping_add(6);
                pa.attrs = None;
                match next_string(b, &mut p) {
                    Ok(v) => pa.attrs = Some(v),
                    Err(e) => rc = e,
                }
            } else if starts_with_ci(rest, b"uuid=") {
                p = p.wrapping_add(5);
                pa.uuid = None;
                match next_string(b, &mut p) {
                    Ok(v) => pa.uuid = Some(v),
                    Err(e) => rc = e,
                }
            } else if starts_with_ci(rest, b"name=") {
                p = p.wrapping_add(5);
                pa.name = None;
                match next_string(b, &mut p) {
                    Ok(v) => pa.name = Some(unhexmangle(&v)),
                    Err(e) => rc = e,
                }
            } else if starts_with_ci(rest, b"type=") || starts_with_ci(rest, b"Id=") {
                // `Id=` for backward compatibility.
                pa.ty = None;
                p = p.wrapping_add(if matches!(at(b, p), b'I' | b'i') {
                    3
                } else {
                    5
                });
                match next_string(b, &mut p) {
                    Ok(t) => {
                        pa.ty = self.parse_type(cxt, &t);
                        if pa.ty.is_none() {
                            rc = -EINVAL;
                        }
                    }
                    Err(e) => rc = e,
                }
            } else {
                // An unknown field.
                rc = -EINVAL;
                break;
            }
        }
        if rc == 0 {
            self.table.add_partition(pa);
        }
        rc
    }

    /// `parse_line_valcommas (dp, s)`: `<start>, <size>, <type>,
    /// <bootable>`.
    fn parse_line_valcommas(&mut self, cxt: &mut Context, b: &mut [u8], s: usize) -> i32 {
        let mut pa = Partition::new();
        pa.set_start_follow_default(true);
        pa.end_follow_default = true;
        pa.set_partno_follow_default(true);

        let mut rc = 0;
        let mut p = s;
        let mut item: i32 = -1;
        while rc == 0 && at(b, p) != 0 {
            p = skip_blank(b, p);
            item = item.wrapping_add(1);
            let begin = p;
            match item {
                0 => rc = self.parse_start_value(cxt, &mut pa, b, &mut p),
                1 => rc = self.parse_size_value(cxt, &mut pa, b, &mut p),
                2 => {
                    pa.ty = None;
                    // Nothing, `-` or a blank: the default type.
                    if !(at(b, p) == b',' || at(b, p) == b';' || is_default_value(b, &mut p)) {
                        match next_string(b, &mut p) {
                            Ok(t) => {
                                pa.ty = self.parse_type(cxt, &t);
                                if pa.ty.is_none() {
                                    rc = -EINVAL;
                                }
                            }
                            Err(e) => rc = e,
                        }
                    }
                }
                3 => {
                    if !(at(b, p) == b',' || at(b, p) == b';') {
                        match next_token(b, &mut p).map(|tk| cstr_at(b, tk)) {
                            Some(b"*" | b"+") => pa.boot = 1,
                            Some(b"-") => pa.boot = 0,
                            _ => rc = -EINVAL,
                        }
                    }
                }
                _ => {}
            }
            if begin == p {
                p = p.wrapping_add(1);
            }
        }
        if rc == 0 {
            self.table.add_partition(pa);
        }
        rc
    }

    /// `fdisk_script_read_buffer (dp, s)`: one line, its newline cut off:
    /// a header (before the first partition), a `name=value` partition, or
    /// a value-list one.
    fn read_buffer(&mut self, cxt: &mut Context, b: &mut [u8], s: usize) -> i32 {
        let s = skip_blank(b, s);
        if at(b, s) == 0 {
            return 0;
        }
        if self.table.is_empty() && is_header_line(b, s) {
            self.parse_line_header(cxt, b, s)
        } else if strchr(b, s, b'=').is_some() {
            self.parse_line_nameval(cxt, b, s)
        } else {
            self.parse_line_valcommas(cxt, b, s)
        }
    }

    /// `fdisk_script_read_line (dp, f, buf, bufsz)`: the next line that
    /// is not blank or a `#` comment, read into `buf` and parsed. 0, a
    /// negative errno (`-ENOTSUP` for an unknown header, which is usually
    /// safe to ignore), or 1 at the end of the input.
    pub fn read_line(
        &mut self,
        cxt: &mut Context,
        input: &mut dyn ScriptInput,
        buf: &mut [u8],
    ) -> i32 {
        loop {
            put(buf, 0, 0);
            if !input.fgets(buf) {
                return 1;
            }
            self.nlines = self.nlines.wrapping_add(1);
            let s = match strchr(buf, 0, b'\n') {
                Some(nl) => nl,
                // A last line with no newline is fine; a line too long
                // for the buffer means a corrupt file.
                None if input.feof() => cstr_at(buf, 0).len(),
                None => return -EINVAL,
            };
            put(buf, s, 0);
            if s > 0 && at(buf, s.wrapping_sub(1)) == b'\r' {
                put(buf, s.wrapping_sub(1), 0);
            }
            let s = skip_blank(buf, 0);
            if at(buf, s) != 0 && at(buf, s) != b'#' {
                return self.read_buffer(cxt, buf, s);
            }
        }
    }

    /// `fdisk_script_read_file (dp, f)`: every line of `input`; an unknown
    /// header is skipped.
    pub fn read_file(&mut self, cxt: &mut Context, input: &mut dyn ScriptInput) -> i32 {
        let mut buf = vec![0u8; BUFSIZ];
        let mut rc = 1;
        while !input.feof() {
            rc = self.read_line(cxt, input, &mut buf);
            if rc != 0 && rc != -ENOTSUP {
                break;
            }
        }
        if rc == 1 { 0 } else { rc }
    }
}

/// `fdisk_new_script_from_file (cxt, filename)`: a script read from a
/// file; on failure, the (positive) errno.
///
/// # Errors
///
/// The file's errno when it cannot be opened, or the parser's.
pub fn new_script_from_file(cxt: &mut Context, filename: &[u8]) -> Result<Script, i32> {
    let f = std::fs::File::open(ulsysfs::path_of(filename)).map_err(|e| errno_of(&e))?;
    let mut dp = Script::new();
    let rc = dp.read_file(cxt, &mut FileInput::new(f));
    if rc != 0 {
        return Err(rc.wrapping_neg());
    }
    Ok(dp)
}

impl Context {
    /// `fdisk_set_script (cxt, dp)`: the script the label drivers follow
    /// (its headers override their defaults, and it stops the dialogs), or
    /// none.
    pub fn set_script(&mut self, dp: Option<ScriptRef>) {
        self.script = dp;
    }

    /// `fdisk_get_script (cxt)`: the script being followed.
    #[must_use]
    pub fn get_script(&self) -> Option<ScriptRef> {
        self.script.clone()
    }

    /// `fdisk_apply_script_headers (cxt, dp)`: `dp` followed, and a new
    /// empty label made as its headers say.
    pub fn apply_script_headers(&mut self, dp: &ScriptRef) -> i32 {
        self.set_script(Some(Rc::clone(dp)));

        let ss = dp.borrow().sector_size;
        if ss != 0 && self.sector_size != ss {
            // A first or last LBA in the script's sectors could be
            // recalculated, but GPT's are relative to other offsets, so
            // the library's defaults are the safer choice.
            for (h, msg) in [
                (
                    &b"first-lba"[..],
                    &b"Ignore \"first-lba\" header due to sector size mismatch."[..],
                ),
                (
                    b"last-lba",
                    b"Ignore \"last-lba\" header due to sector size mismatch.",
                ),
            ] {
                if dp.borrow().get_header(h).is_some() {
                    dp.borrow_mut().set_header(h, None);
                    self.info(msg);
                }
            }
        }

        let grain = dp.borrow().get_header(b"grain").map(<[u8]>::to_vec);
        if let Some(s) = grain {
            let rc = match ulstrutils::parse_size(&s) {
                Ok(sz) => self.save_user_grain(sz),
                Err(ulstrutils::NumErr::Range) => -ERANGE,
                Err(_) => -EINVAL,
            };
            if rc != 0 {
                return rc;
            }
        }
        if self.has_user_device_properties() {
            self.apply_user_device_properties();
        }

        // The new, empty label.
        let name = dp.borrow().get_header(b"label").map(<[u8]>::to_vec);
        let Some(name) = name else { return -EINVAL };
        let rc = self.create_disklabel(Some(&name));
        if rc != 0 {
            return rc;
        }

        let len = dp.borrow().get_header(b"table-length").map(<[u8]>::to_vec);
        match len.map(|s| ulstrutils::parse_size(&s)) {
            None => 0,
            // `size_t` passed as `uint32_t`.
            Some(Ok(sz)) => crate::gpt::set_npartitions(self, crate::utils::u64_as_u32(sz)),
            Some(Err(ulstrutils::NumErr::Range)) => -ERANGE,
            Some(Err(_)) => -EINVAL,
        }
    }

    /// `fdisk_apply_script (cxt, dp)`: a new label and its partitions,
    /// made in memory as `dp` describes; [`Context::write_disklabel`]
    /// writes them.
    pub fn apply_script(&mut self, dp: &ScriptRef) -> i32 {
        let old = self.get_script();
        let mut rc = self.apply_script_headers(dp);
        if rc == 0 {
            // The table is lent out while it is applied: the drivers read
            // the script's headers as they add each partition.
            let mut tb = std::mem::take(&mut dp.borrow_mut().table);
            rc = self.apply_table(&mut tb);
            dp.borrow_mut().table = tb;
        }
        self.set_script(old);
        rc
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    /// Upstream's `--tokens` test program: every token `s` splits into.
    fn tokens(s: &[u8]) -> Vec<Vec<u8>> {
        let mut b = s.to_vec();
        b.push(0);
        let mut p = 0;
        let mut out = Vec::new();
        while at(&b, p) != 0 {
            let Some(tk) = next_token(&mut b, &mut p) else {
                break;
            };
            out.push(cstr_at(&b, tk).to_vec());
        }
        out
    }

    #[test]
    fn tokens_end_at_blanks_commas_and_semicolons_but_not_inside_quotes() {
        assert_eq!(tokens(b"a b, c;d"), [&b"a"[..], b"b", b"c", b"d"]);
        assert_eq!(tokens(b"a  ,b"), [&b"a"[..], b"b"]);
        assert_eq!(tokens(b"\"a b\", c"), [&b"a b"[..], b"c"]);
        assert_eq!(tokens(b"x \"y z\""), [&b"x"[..], b"y z"]);
        // A quote never closed ends nothing.
        assert!(tokens(b"abc\"def").is_empty());
        // Nor does a closing quote followed by more of the token.
        assert!(tokens(b"\"ab\"cd").is_empty());
    }

    #[test]
    fn a_device_names_its_partition_by_its_trailing_number() {
        let pno = |s: &[u8]| {
            let mut b = s.to_vec();
            b.push(0);
            partno_from_devname(&mut b, 0)
        };
        assert_eq!(pno(b"/dev/sda1"), 0);
        assert_eq!(pno(b"/dev/nvme0n1p12 \t"), 11);
        assert_eq!(pno(b"/dev/sda"), -1);
        assert_eq!(pno(b"0"), -1);
        assert_eq!(pno(b"/dev/sda99999999999"), -1);
    }

    #[test]
    fn names_have_their_hex_escapes_decoded() {
        assert_eq!(unhexmangle(br"a\x20b"), b"a b");
        assert_eq!(unhexmangle(br"\x4"), br"\x4");
        assert_eq!(unhexmangle(br"\xg0"), br"\xg0");
        // A decoded NUL ends the C string.
        assert_eq!(unhexmangle(br"a\x00z"), b"a");
    }

    #[test]
    fn a_header_line_has_a_name_a_colon_a_value_and_no_equals_sign() {
        let hdr = |s: &[u8]| {
            let mut b = s.to_vec();
            b.push(0);
            is_header_line(&b, 0)
        };
        assert!(hdr(b"label: gpt"));
        assert!(!hdr(b": gpt"));
        assert!(!hdr(b"label:"));
        assert!(!hdr(b"/dev/sda1 : start=2048"));
    }
}
