//! `/proc/slabinfo`, transcribed from procps-ng 4.0.4's
//! `library/slabinfo.c`, for `vmstat -m`.
//!
//! What the library does and this keeps: the file is opened once and read from
//! its start each time, a line at a time through `fgets` into 2048 bytes. The
//! first line must be `slabinfo - version: 2.x`, or the read fails (`ERANGE`).
//! After it, a line starting with `#` is a heading and skipped, and every
//! other line must make eight conversions of
//! `"%128s %u %u %u %u %u : tunables %*u %*u %*u : slabdata %u %u %*u"` or the
//! read fails the same way. A cache with an empty name is called `unknown`.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};

use super::scanf::Scan;

/// `SLABINFO_FILE`.
pub const SLABINFO_FILE: &str = "/proc/slabinfo";

/// `SLABINFO_LINE_LEN`: `fgets`' buffer, of which one byte is the NUL.
const SLABINFO_LINE_LEN: usize = 2048;

/// `SLABINFO_NAME_LEN`: the `%128s` width.
const SLABINFO_NAME_LEN: usize = 128;

/// One cache: the fields of `struct slabs_node` the parse fills.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub name: Vec<u8>,
    pub nr_active_objs: u32,
    pub nr_objs: u32,
    pub obj_size: u32,
    pub objs_per_slab: u32,
    pub pages_per_slab: u32,
    pub nr_active_slabs: u32,
    pub nr_slabs: u32,
}

/// `ERANGE`.
const ERANGE: i32 = 34;

/// The version line: `sscanf (line, "slabinfo - version: %d.%d")` making both
/// conversions, with a major version of 2.
fn version_ok(line: &[u8]) -> bool {
    let mut scan = Scan::new(line);
    scan.lit(b"slabinfo - version: ").is_some()
        && matches!(scan.int(), Some(2))
        && scan.lit(b".").is_some()
        && scan.int().is_some()
}

/// One cache line through the library's `sscanf`, or `None` when it made
/// fewer than eight conversions.
#[must_use]
pub fn parse_line(line: &[u8]) -> Option<Node> {
    /// `%u`: `strtoul`'s value, cut to `unsigned int`.
    fn uint(scan: &mut Scan<'_>) -> Option<u32> {
        scan.ulong().map(|v| v as u32)
    }
    let mut scan = Scan::new(line);
    let mut node = Node {
        name: scan.word(SLABINFO_NAME_LEN)?.to_vec(),
        nr_active_objs: uint(&mut scan)?,
        nr_objs: uint(&mut scan)?,
        obj_size: uint(&mut scan)?,
        objs_per_slab: uint(&mut scan)?,
        pages_per_slab: uint(&mut scan)?,
        ..Node::default()
    };
    scan.lit(b" : tunables ")?;
    scan.skip_uint()?;
    scan.skip_uint()?;
    scan.skip_uint()?;
    scan.lit(b" : slabdata ")?;
    node.nr_active_slabs = uint(&mut scan)?;
    node.nr_slabs = uint(&mut scan)?;
    if node.name.is_empty() {
        node.name = b"unknown".to_vec();
    }
    Some(node)
}

/// `fgets` into `len` bytes, one call: at most `len - 1` bytes, ending early
/// at a newline, which it keeps. Empty at the end of the file.
fn fgets(reader: &mut impl BufRead, len: usize) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    while line.len() < len.saturating_sub(1) {
        let buf = reader.fill_buf()?;
        let Some(&c) = buf.first() else { break };
        reader.consume(1);
        line.push(c);
        if c == b'\n' {
            break;
        }
    }
    Ok(line)
}

/// procps' `struct slabinfo_info`, cut to what `vmstat` uses.
#[derive(Debug)]
pub struct SlabInfo {
    file: File,
    nodes: Vec<Node>,
}

impl SlabInfo {
    /// `procps_slabinfo_new`, with its priming read.
    ///
    /// # Errors
    ///
    /// The file could not be opened or read, or did not parse.
    pub fn new() -> io::Result<Self> {
        let mut info = Self {
            file: File::open(SLABINFO_FILE)?,
            nodes: Vec::new(),
        };
        info.read()?;
        Ok(info)
    }

    /// `slabinfo_read_failed`, with `parse_slabinfo20`.
    fn read(&mut self) -> io::Result<()> {
        self.nodes.clear();
        self.file.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(&self.file);
        let first = fgets(&mut reader, SLABINFO_LINE_LEN)?;
        if first.is_empty() {
            // `fgets` found nothing, with `errno` from nowhere in particular;
            // the read fails either way.
            return Err(io::Error::from_raw_os_error(ERANGE));
        }
        if !version_ok(&first) {
            return Err(io::Error::from_raw_os_error(ERANGE));
        }
        loop {
            let line = fgets(&mut reader, SLABINFO_LINE_LEN)?;
            if line.is_empty() {
                break;
            }
            if line.first() == Some(&b'#') {
                continue;
            }
            let Some(node) = parse_line(&line) else {
                return Err(io::Error::from_raw_os_error(ERANGE));
            };
            self.nodes.push(node);
        }
        Ok(())
    }

    /// `procps_slabinfo_reap`: read, then every cache, in the file's order.
    ///
    /// # Errors
    ///
    /// As [`SlabInfo::new`].
    pub fn reap(&mut self) -> io::Result<&[Node]> {
        self.read()?;
        Ok(&self.nodes)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const LINE: &[u8] = b"kmalloc-64          12345  13000     64   64    1 : tunables    0    0    0 : slabdata    203    203      0\n";

    #[test]
    fn a_cache_line() {
        let n = parse_line(LINE).unwrap();
        assert_eq!(n.name, b"kmalloc-64");
        assert_eq!(
            (
                n.nr_active_objs,
                n.nr_objs,
                n.obj_size,
                n.objs_per_slab,
                n.pages_per_slab
            ),
            (12345, 13000, 64, 64, 1)
        );
        assert_eq!((n.nr_active_slabs, n.nr_slabs), (203, 203));
    }

    #[test]
    fn a_line_without_its_tunables_does_not_parse() {
        assert!(parse_line(b"cache 1 2 3 4 5\n").is_none());
        assert!(parse_line(b"cache 1 2 3 4 5 : slabdata 1 2 3\n").is_none());
    }

    #[test]
    fn the_version_line_must_say_two() {
        assert!(version_ok(b"slabinfo - version: 2.1\n"));
        assert!(!version_ok(b"slabinfo - version: 1.1\n"));
        assert!(!version_ok(b"slabinfo - version: 2\n"));
        assert!(!version_ok(b"# name <active_objs>\n"));
    }
}
