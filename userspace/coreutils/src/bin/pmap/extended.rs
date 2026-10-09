//! `print_extended_maps`: `-X`, `-XX` and `-c`, every field of `smaps` --
//! or those `-X` or the rc file picks -- in columns.
//!
//! Upstream reads the file twice. The first pass measures: the widest
//! address, permissions, offset, device and inode, each field's widest
//! figure (under `-q`) or its total, and whether any mapping has
//! `VmFlags`. Then it seeks back to the start and prints. Kept here as it
//! is, with what follows from it:
//!
//! - The list of fields is made by the first process and kept for every
//!   process after it. A later process is held to it -- a field in another
//!   place is `inconsistent detail field in smaps file` -- and a column's
//!   width only grows. The totals start again at 0 after each footer, and
//!   under `-q`, which prints no footer, they never do.
//! - A field is anything `sscanf ("%31[^:]: %20[0-9] kB %c")` reads two
//!   values from, so `THPeligible:    0`, which has no `kB`, is a column.
//! - `VmFlags` is the one field with words; a mapping without the line
//!   shows the flags of the last one that had it.
//! - The mapping's name is cut at its last `/` unless `ShowPath` or `-p`
//!   asks for the path; a name ending in `/` shows nothing.

use super::Pmap;
use super::cfile::CFile;
use coreutils::procps::scanf::Scan;

/// `DETAIL_LENGTH - 1`: a field's name is read into 32 bytes.
const DETL: usize = 31;
/// `NUM_LENGTH - 1`: a figure, or an address, into 21.
const NUML: usize = 20;
/// `VMFLAGS_LENGTH - 1`.
const VMFL: usize = 127;

/// `struct listnode`: one field's column.
pub struct Node {
    /// `description`: the field's name.
    description: Vec<u8>,
    /// `value_str`: this mapping's figure, as `smaps` wrote it.
    value_str: Vec<u8>,
    /// `value`: the same, as `%lu` reads it.
    value: u64,
    /// `total`: the sum over one process, made on the first pass.
    total: u64,
    /// `max_width`.
    max_width: i32,
}

/// What one mapping's first line gave: `sscanf ("%20[0-9a-f]-%20[0-9a-f]
/// %31s %20[0-9a-f] %63[0-9a-f:] %20s %127[^\n]")`.
struct Head {
    count: usize,
    start: Vec<u8>,
    perms: Vec<u8>,
    offset: Vec<u8>,
    dev: Vec<u8>,
    inode: Vec<u8>,
    desc: Vec<u8>,
}

fn is_hex_lower(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
}

fn scan_head(line: &[u8]) -> Head {
    let mut h = Head {
        count: 0,
        start: Vec::new(),
        perms: Vec::new(),
        offset: Vec::new(),
        dev: Vec::new(),
        inode: Vec::new(),
        desc: Vec::new(),
    };
    let mut s = Scan::new(line);
    let Some(v) = s.scanset(NUML, is_hex_lower) else {
        return h;
    };
    (h.start, h.count) = (v.to_vec(), 1);
    // The end is read and not kept.
    if s.lit(b"-")
        .and_then(|()| s.scanset(NUML, is_hex_lower))
        .is_none()
    {
        return h;
    }
    h.count = 2;
    s.ws();
    let Some(v) = s.word(DETL) else { return h };
    (h.perms, h.count) = (v.to_vec(), 3);
    s.ws();
    let Some(v) = s.scanset(NUML, is_hex_lower) else {
        return h;
    };
    (h.offset, h.count) = (v.to_vec(), 4);
    s.ws();
    let Some(v) = s.scanset(63, |c| is_hex_lower(c) || c == b':') else {
        return h;
    };
    (h.dev, h.count) = (v.to_vec(), 5);
    s.ws();
    let Some(v) = s.word(NUML) else { return h };
    (h.inode, h.count) = (v.to_vec(), 6);
    s.ws();
    let Some(v) = s.scanset(VMFL, |c| c != b'\n') else {
        return h;
    };
    (h.desc, h.count) = (v.to_vec(), 7);
    h
}

/// `sscanf ("%31[^:]: %20[0-9] kB %c")`: how many it assigned, the name,
/// and the figure.
fn scan_detail(line: &[u8]) -> (usize, Option<&[u8]>, Option<&[u8]>) {
    let mut s = Scan::new(line);
    let Some(name) = s.scanset(DETL, |c| c != b':') else {
        return (0, None, None);
    };
    let Some(value) = s
        .lit(b": ")
        .and_then(|()| s.scanset(NUML, |c| c.is_ascii_digit()))
    else {
        return (1, Some(name), None);
    };
    // ` kB %c`: a third value only if something follows the unit.
    let third = s.lit(b" kB ").and_then(|()| s.ch()).is_some();
    (if third { 3 } else { 2 }, Some(name), Some(value))
}

/// `sscanf ("VmFlags: %127[a-z ]")`.
fn scan_vmflags(line: &[u8]) -> Option<&[u8]> {
    let mut s = Scan::new(line);
    s.lit(b"VmFlags: ")?;
    s.scanset(VMFL, |c| c.is_ascii_lowercase() || c == b' ')
}

/// The length C's `strlen` gives a field that holds no NUL.
fn len_i32(b: &[u8]) -> i32 {
    i32::try_from(b.len()).unwrap_or(i32::MAX)
}

impl Pmap {
    /// `print_extended_maps (f)`. `Err` carries the status of a fatal
    /// complaint, already made.
    #[allow(clippy::too_many_lines)]
    pub(super) fn print_extended_maps(&mut self, f: &mut CFile) -> Result<(), u8> {
        let (mut maxw1, mut maxw2, mut maxw3, mut maxw4, mut maxw5, mut maxwv) =
            (0i32, 0i32, 0i32, 0i32, 0i32, 0i32);
        let mut has_vmflags = false;
        let mut vmflags: Vec<u8> = Vec::new();
        let mut firstmapping = 2;
        let mut ret = f.fgets(super::MAPBUF);

        while let Some(line) = ret.take() {
            let mapbuf = super::c_str(&line).to_vec();
            let head = scan_head(&mapbuf);
            if head.count < 6 {
                return Err(self.errx(b"Unknown format in smaps file!"));
            }
            // A line too long for the buffer: the rest of it thrown away.
            let mut last = mapbuf.last().copied();
            while last != Some(b'\n') {
                // Upstream tests the `ret` of the line before, not this
                // read's, so only an empty read stops it.
                match f.fgets(super::MAPBUF) {
                    Some(more) if !super::c_str(&more).is_empty() => {
                        last = super::c_str(&more).last().copied();
                    }
                    _ => return Err(self.errx(b"Unknown format in smaps file!")),
                }
            }

            maxw1 = maxw1.max(len_i32(&head.start));
            maxw2 = maxw2.max(len_i32(&head.perms));
            maxw3 = maxw3.max(len_i32(&head.offset));
            maxw4 = maxw4.max(len_i32(&head.dev));
            maxw5 = maxw5.max(len_i32(&head.inode));

            ret = f.fgets(super::MAPBUF);
            let mut node = 0usize;
            while let Some(detail_line) = ret.clone() {
                let detail = super::c_str(&detail_line);
                let (n, name, value) = scan_detail(detail);
                if n != 2 {
                    break;
                }
                let (Some(name), Some(value)) = (name, value) else {
                    break;
                };
                if self.is_enabled(name) {
                    if node == self.list.len() {
                        if firstmapping != 2 {
                            // `assert (firstmapping == 2)`: the file grew a
                            // field between the two passes.
                            let mut m = self.name.clone();
                            m.extend_from_slice(
                                b": src/pmap.c:362: print_extended_maps: \
                                  Assertion `firstmapping == 2' failed.\n",
                            );
                            coreutils::stdfd::diag_bytes_ahead_of_stdout(&m);
                            std::process::abort();
                        }
                        let max_width = if self.o.quiet { 0 } else { len_i32(name) };
                        self.list.push(Node {
                            description: name.to_vec(),
                            value_str: Vec::new(),
                            value: 0,
                            total: 0,
                            max_width,
                        });
                    } else if self.list.get(node).is_some_and(|nd| nd.description != name) {
                        let mut m =
                            b"ERROR: inconsistent detail field in smaps file, line:\n ".to_vec();
                        // The line, newline and all, as upstream prints it;
                        // what it holds that is not printable, escaped.
                        let (body, nl) = match detail.strip_suffix(b"\n") {
                            Some(b) => (b, true),
                            None => (detail, false),
                        };
                        m.extend_from_slice(coreutils::quote::escape_unprintable(body).as_bytes());
                        if nl {
                            m.push(b'\n');
                        }
                        return Err(self.errx(&m));
                    }
                    let quiet = self.o.quiet;
                    if let Some(nd) = self.list.get_mut(node) {
                        nd.value_str = value.to_vec();
                        nd.value = cstrtol::strtoul(value, 10).0;
                        if firstmapping == 2 {
                            nd.total = nd.total.wrapping_add(nd.value);
                            if quiet {
                                nd.max_width = nd.max_width.max(len_i32(&nd.value_str));
                            }
                        }
                    }
                    node = node.saturating_add(1);
                }
                ret = f.fgets(super::MAPBUF);
            }

            if let Some(flags_line) = ret.clone()
                && let Some(flags) = scan_vmflags(super::c_str(&flags_line))
            {
                vmflags = flags.to_vec();
                if vmflags.last() == Some(&b' ') {
                    vmflags.pop();
                }
                maxwv = maxwv.max(len_i32(&vmflags));
                has_vmflags = true;
                ret = f.fgets(super::MAPBUF);
            }

            if firstmapping == 2 {
                // Measuring: nothing printed yet.
                if ret.is_none() {
                    firstmapping = 1;
                    f.rewind();
                    ret = f.fgets(super::MAPBUF);
                    if !self.o.quiet {
                        for nd in &mut self.list {
                            nd.max_width = nd.max_width.max(super::integer_width(nd.total));
                        }
                    }
                }
                continue;
            }

            if firstmapping == 1 && !self.o.quiet {
                maxw1 = self.justify_print(super::NLS_ADDRESS, maxw1, true);
                if self.is_enabled(super::NLS_PERM) {
                    maxw2 = self.justify_print(super::NLS_PERM, maxw2, true);
                }
                if self.is_enabled(super::NLS_OFFSET) {
                    maxw3 = self.justify_print(super::NLS_OFFSET, maxw3, true);
                }
                if self.is_enabled(super::NLS_DEVICE) {
                    maxw4 = self.justify_print(super::NLS_DEVICE, maxw4, true);
                }
                if self.is_enabled(super::NLS_INODE) {
                    maxw5 = self.justify_print(super::NLS_INODE, maxw5, true);
                }
                let heads: Vec<(Vec<u8>, i32)> = self
                    .list
                    .iter()
                    .map(|nd| (nd.description.clone(), nd.max_width))
                    .collect();
                for (d, w) in heads {
                    self.justify_print(&d, w, true);
                }
                if has_vmflags && self.is_enabled(b"VmFlags") {
                    maxwv = self.justify_print(b"VmFlags", maxwv, true);
                }
                if self.is_enabled(super::NLS_MAPPING) {
                    self.justify_print(super::NLS_MAPPING, 0, false);
                } else {
                    self.put(b"\n");
                }
            }

            let mut row = super::padded(&head.start, maxw1, false);
            let columns = [
                (super::NLS_PERM, &head.perms, maxw2),
                (super::NLS_OFFSET, &head.offset, maxw3),
                (super::NLS_DEVICE, &head.dev, maxw4),
                (super::NLS_INODE, &head.inode, maxw5),
            ];
            for (heading, text, width) in columns {
                if self.is_enabled(heading) {
                    row.push(b' ');
                    row.extend(super::padded(text, width, false));
                }
            }
            for nd in &self.list {
                row.push(b' ');
                row.extend(super::padded(&nd.value_str, nd.max_width, false));
            }
            if has_vmflags && self.is_enabled(b"VmFlags") {
                row.push(b' ');
                row.extend(super::padded(&vmflags, maxwv, false));
            }
            if self.is_enabled(super::NLS_MAPPING) {
                row.push(b' ');
                if self.showpath {
                    row.extend_from_slice(&head.desc);
                } else {
                    match head.desc.iter().rposition(|&c| c == b'/') {
                        Some(slash) => row.extend_from_slice(
                            head.desc.get(slash.saturating_add(1)..).unwrap_or_default(),
                        ),
                        None => row.extend_from_slice(&head.desc),
                    }
                }
            }
            row.push(b'\n');
            self.put(&row);
            firstmapping = 0;
        }

        if !self.o.quiet && !self.list.is_empty() {
            let mut gap = maxw1.saturating_add(1);
            for (heading, width) in [
                (super::NLS_PERM, maxw2),
                (super::NLS_OFFSET, maxw3),
                (super::NLS_DEVICE, maxw4),
                (super::NLS_INODE, maxw5),
            ] {
                if self.is_enabled(heading) {
                    gap = gap.saturating_add(width).saturating_add(1);
                }
            }
            let spaces = vec![b' '; usize::try_from(gap).unwrap_or(0)];
            let mut l = spaces.clone();
            for nd in &self.list {
                l.extend(std::iter::repeat_n(
                    b'=',
                    usize::try_from(nd.max_width).unwrap_or(0),
                ));
                l.push(b' ');
            }
            l.push(b'\n');
            l.extend_from_slice(&spaces);
            for nd in &mut self.list {
                l.extend(super::num(nd.total, nd.max_width));
                l.push(b' ');
                nd.total = 0;
            }
            l.extend_from_slice(b"KB \n");
            self.put(&l);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{scan_detail, scan_head, scan_vmflags};

    #[test]
    fn a_mapping_line_gives_six_or_seven_fields() {
        let h = scan_head(b"7f00-7f10 r--p 00000000 08:30 93607    /usr/bin/sleep\n");
        assert_eq!(h.count, 7);
        assert_eq!(h.start, b"7f00");
        assert_eq!(h.dev, b"08:30");
        assert_eq!(h.desc, b"/usr/bin/sleep");
        let anon = scan_head(b"7f00-7f10 rw-p 00000000 00:00 0 \n");
        assert_eq!(anon.count, 6, "no name");
        assert_eq!(scan_head(b"Size:   4 kB\n").count, 0);
    }

    #[test]
    fn a_detail_is_two_values_with_or_without_its_unit() {
        let (n, name, value) = scan_detail(b"Rss:                   8 kB\n");
        assert_eq!((n, name, value), (2, Some(&b"Rss"[..]), Some(&b"8"[..])));
        let (n, name, _) = scan_detail(b"THPeligible:           0\n");
        assert_eq!(
            (n, name),
            (2, Some(&b"THPeligible"[..])),
            "no kB is still two"
        );
        assert_eq!(scan_detail(b"VmFlags: rd mr\n").0, 1);
        assert_eq!(scan_detail(b"Rss: 8 kB extra\n").0, 3);
    }

    #[test]
    fn vmflags_are_lower_case_words() {
        assert_eq!(
            scan_vmflags(b"VmFlags: rd mr mw \n"),
            Some(&b"rd mr mw "[..])
        );
        assert_eq!(scan_vmflags(b"Rss: 8 kB\n"), None);
    }
}
