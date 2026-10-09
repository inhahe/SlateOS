//! `/proc/slabinfo`, transcribed from procps-ng 4.0.4's
//! `library/slabinfo.c`, for `vmstat -m` and `slabtop`.
//!
//! What the library does and this keeps: the file is opened once and read from
//! its start each time, a line at a time through `fgets` into 2048 bytes. The
//! first line must be `slabinfo - version: 2.x`, or the read fails (`ERANGE`).
//! After it, a line starting with `#` is a heading and skipped, and every
//! other line must make eight conversions of
//! `"%128s %u %u %u %u %u : tunables %*u %*u %*u : slabdata %u %u %*u"` or the
//! read fails the same way. A cache with an empty name is called `unknown`.
//!
//! Each read also does `parse_slabinfo20`'s arithmetic, in its types: a
//! cache's size is its slabs times its pages per slab times the page size, in
//! `unsigned long`; its use is `100 * ((float) active / objects)`, made an
//! `unsigned int` as gcc makes one; and the totals over every cache are
//! `unsigned int` sums -- wrapping as C's do -- but for the two sizes, which
//! are `unsigned long`. The smallest object starts at `INT_MAX`, which is what
//! a file with no caches reports.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};

use super::cvt;
use super::scanf::Scan;

/// `SLABINFO_FILE`.
pub const SLABINFO_FILE: &str = "/proc/slabinfo";

/// `SLABINFO_LINE_LEN`: `fgets`' buffer, of which one byte is the NUL.
const SLABINFO_LINE_LEN: usize = 2048;

/// `SLABINFO_NAME_LEN`: the `%128s` width.
const SLABINFO_NAME_LEN: usize = 128;

/// One cache: `struct slabs_node`.
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
    /// `cache_size`: the upper limit of the memory the cache uses.
    pub cache_size: u64,
    /// `use`: the percentage of its objects in use.
    pub percent_used: u32,
}

/// The totals over every cache: `struct slabs_summ`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub nr_objs: u32,
    pub nr_active_objs: u32,
    pub nr_pages: u32,
    pub nr_slabs: u32,
    pub nr_active_slabs: u32,
    pub nr_caches: u32,
    pub nr_active_caches: u32,
    pub avg_obj_size: u32,
    pub min_obj_size: u32,
    pub max_obj_size: u32,
    pub active_size: u64,
    pub total_size: u64,
}

/// `ERANGE`.
const ERANGE: i32 = 34;

/// `INT_MAX`, where the smallest object size starts.
const INT_MAX: u32 = 0x7fff_ffff;

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
/// fewer than eight conversions. Only what the line says is filled in; the
/// derived fields are [`derive`]'s.
#[must_use]
pub fn parse_line(line: &[u8]) -> Option<Node> {
    /// `%u`: `strtoul`'s value, cut to `unsigned int`.
    fn uint(scan: &mut Scan<'_>) -> Option<u32> {
        scan.ulong().map(|v| {
            let [a, b, c, d, ..] = v.to_le_bytes();
            u32::from_le_bytes([a, b, c, d])
        })
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

/// `parse_slabinfo20`'s work on one cache: its size and use filled in, and
/// it added to `summary`.
pub fn derive(node: &mut Node, summary: &mut Summary, page_size: u64) {
    if node.obj_size < summary.min_obj_size {
        summary.min_obj_size = node.obj_size;
    }
    if node.obj_size > summary.max_obj_size {
        summary.max_obj_size = node.obj_size;
    }
    node.cache_size = u64::from(node.nr_slabs)
        .wrapping_mul(u64::from(node.pages_per_slab))
        .wrapping_mul(page_size);
    if node.nr_objs == 0 {
        node.percent_used = 0;
    } else {
        // `(unsigned int)(100 * ((float)active / objs))`: in `float`
        // throughout, then gcc's conversion of a `float` to `unsigned int`
        // -- the 64-bit one, of which the low half is kept.
        let ratio = cvt::flt(node.nr_active_objs) / cvt::flt(node.nr_objs);
        node.percent_used = cvt::cvt_u32(f64::from(100.0f32 * ratio));
        summary.nr_active_caches = summary.nr_active_caches.wrapping_add(1);
    }
    summary.nr_objs = summary.nr_objs.wrapping_add(node.nr_objs);
    summary.nr_active_objs = summary.nr_active_objs.wrapping_add(node.nr_active_objs);
    summary.total_size = summary
        .total_size
        .wrapping_add(u64::from(node.nr_objs).wrapping_mul(u64::from(node.obj_size)));
    summary.active_size = summary
        .active_size
        .wrapping_add(u64::from(node.nr_active_objs).wrapping_mul(u64::from(node.obj_size)));
    summary.nr_pages = summary
        .nr_pages
        .wrapping_add(node.nr_slabs.wrapping_mul(node.pages_per_slab));
    summary.nr_slabs = summary.nr_slabs.wrapping_add(node.nr_slabs);
    summary.nr_active_slabs = summary.nr_active_slabs.wrapping_add(node.nr_active_slabs);
    summary.nr_caches = summary.nr_caches.wrapping_add(1);
}

/// `parse_slabinfo20`'s close: the average object size, when there are
/// objects to average -- an `unsigned long` quotient kept to an `unsigned
/// int`.
pub fn finish(summary: &mut Summary) {
    if let Some(avg) = summary.total_size.checked_div(u64::from(summary.nr_objs)) {
        let [a, b, c, d, ..] = avg.to_le_bytes();
        summary.avg_obj_size = u32::from_le_bytes([a, b, c, d]);
    }
}

/// `getpagesize ()`.
#[must_use]
pub fn page_size() -> u64 {
    /// `_SC_PAGESIZE`.
    const SC_PAGESIZE: i32 = 30;
    // The page size is always known; 4096 stands for a C library that
    // cannot say, which `getpagesize` never is.
    u64::try_from(libcall::conf::sysconf(SC_PAGESIZE)).unwrap_or(4096)
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

/// Why a read of the file failed, as `errno` then has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A call that sets `errno` failed, or the library set it: `ERANGE` for
    /// a file it cannot parse, `EINVAL` for a library with no file.
    Errno(i32),
    /// The file ended before its version line. `fgets` sets nothing then,
    /// so `errno` is whatever it was before -- 0, in a program that has met
    /// no failure yet.
    Silent,
}

impl Failure {
    /// `errno`, as a program that has met no other failure has it.
    #[must_use]
    pub fn errno(self) -> i32 {
        match self {
            Failure::Errno(e) => e,
            Failure::Silent => 0,
        }
    }
}

/// The open file and what its last read found.
#[derive(Debug)]
struct Open {
    file: File,
    nodes: Vec<Node>,
    summary: Summary,
}

impl Open {
    /// `slabinfo_read_failed`, with `parse_slabinfo20`.
    fn read(&mut self) -> Result<(), Failure> {
        let os = |e: io::Error| Failure::Errno(e.raw_os_error().unwrap_or(ERANGE));
        self.nodes.clear();
        self.summary = Summary {
            min_obj_size: INT_MAX,
            ..Summary::default()
        };
        self.file.seek(SeekFrom::Start(0)).map_err(os)?;
        let mut reader = BufReader::new(&self.file);
        let first = fgets(&mut reader, SLABINFO_LINE_LEN).map_err(os)?;
        if first.is_empty() {
            return Err(Failure::Silent);
        }
        if !version_ok(&first) {
            return Err(Failure::Errno(ERANGE));
        }
        let page_size = page_size();
        loop {
            // `while (fgets (...))`: a read that fails ends the caches as the
            // end of the file does.
            let line = fgets(&mut reader, SLABINFO_LINE_LEN).unwrap_or_default();
            if line.is_empty() {
                break;
            }
            if line.first() == Some(&b'#') {
                continue;
            }
            let Some(mut node) = parse_line(&line) else {
                return Err(Failure::Errno(ERANGE));
            };
            derive(&mut node, &mut self.summary, page_size);
            self.nodes.push(node);
        }
        finish(&mut self.summary);
        Ok(())
    }
}

/// procps' `struct slabinfo_info`, or the null pointer that stands in for
/// one (see [`SlabInfo::new`]).
#[derive(Debug)]
pub struct SlabInfo {
    open: Option<Open>,
}

/// `EINVAL`: what the library says when it is handed no info at all.
const EINVAL: i32 = 22;

impl SlabInfo {
    /// `procps_slabinfo_new`, with its priming read.
    ///
    /// A priming read that fails without setting `errno` -- a file that ends
    /// before its version line -- makes upstream return `-errno`, which is 0,
    /// success, after it has freed the info and set nothing: the caller goes
    /// on with a null pointer, which every later call refuses with `EINVAL`.
    /// So does this: `Ok`, with nothing behind it.
    ///
    /// # Errors
    ///
    /// The `errno` of a file that could not be opened or read, or did not
    /// parse (`ERANGE`).
    pub fn new() -> Result<Self, i32> {
        Self::open(SLABINFO_FILE)
    }

    /// [`SlabInfo::new`] on another file, for the tests.
    ///
    /// # Errors
    ///
    /// As [`SlabInfo::new`].
    pub fn open(path: &str) -> Result<Self, i32> {
        let file = File::open(path).map_err(|e| e.raw_os_error().unwrap_or(EINVAL))?;
        let mut open = Open {
            file,
            nodes: Vec::new(),
            summary: Summary::default(),
        };
        match open.read() {
            Ok(()) => Ok(Self { open: Some(open) }),
            Err(Failure::Silent) => Ok(Self { open: None }),
            Err(Failure::Errno(e)) => Err(e),
        }
    }

    /// Whether this is the null pointer a first read that set no `errno`
    /// left (see [`SlabInfo::new`]). Upstream's `procps_slabinfo_new`
    /// returned `-errno` then, so a caller whose own earlier calls left
    /// `errno` set saw a failure, reported with that `errno`, and only one
    /// with `errno` 0 went on to be refused by the next read.
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.open.is_none()
    }

    /// `procps_slabinfo_reap`: read, then every cache, in the file's order.
    ///
    /// # Errors
    ///
    /// What the read failed with; `EINVAL` with no info.
    pub fn reap(&mut self) -> Result<&[Node], Failure> {
        let open = self.open.as_mut().ok_or(Failure::Errno(EINVAL))?;
        open.read()?;
        Ok(&open.nodes)
    }

    /// `procps_slabinfo_select` of the totals: read again, then what the
    /// read found over every cache.
    ///
    /// # Errors
    ///
    /// As [`SlabInfo::reap`].
    pub fn select(&mut self) -> Result<Summary, Failure> {
        let open = self.open.as_mut().ok_or(Failure::Errno(EINVAL))?;
        open.read()?;
        Ok(open.summary)
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

    #[test]
    fn each_cache_is_sized_and_counted_in_cs_types() {
        let mut summary = Summary {
            min_obj_size: INT_MAX,
            ..Summary::default()
        };
        let mut a = parse_line(LINE).unwrap();
        derive(&mut a, &mut summary, 4096);
        assert_eq!(a.cache_size, 203 * 4096);
        // 12345 / 13000 in `float`, times 100: 94.96..., truncated.
        assert_eq!(a.percent_used, 94);
        let mut empty = parse_line(b"empty 0 0 8 0 1 : tunables 0 0 0 : slabdata 0 0 0\n").unwrap();
        derive(&mut empty, &mut summary, 4096);
        assert_eq!(empty.percent_used, 0);
        // More in use than there are: the ratio is over one, and kept.
        let mut odd =
            parse_line(b"odd 300 100 4294967295 1 2 : tunables 0 0 0 : slabdata 1 4294967295 0\n")
                .unwrap();
        derive(&mut odd, &mut summary, 4096);
        assert_eq!(odd.percent_used, 300);
        finish(&mut summary);
        assert_eq!(summary.nr_caches, 3);
        assert_eq!(summary.nr_active_caches, 2);
        assert_eq!(summary.nr_objs, 13100);
        assert_eq!(summary.min_obj_size, 8);
        assert_eq!(summary.max_obj_size, u32::MAX);
        // 203 + 0 + (4294967295 * 2, wrapped) pages.
        assert_eq!(
            summary.nr_pages,
            203u32.wrapping_add(u32::MAX.wrapping_mul(2))
        );
        assert_eq!(summary.nr_slabs, 203u32.wrapping_add(u32::MAX));
        let total = 13000 * 64 + 100 * u64::from(u32::MAX);
        assert_eq!(summary.total_size, total);
        assert_eq!(summary.avg_obj_size, u32::try_from(total / 13100).unwrap());
    }

    #[test]
    fn no_caches_leave_the_smallest_object_at_int_max() {
        let mut summary = Summary {
            min_obj_size: INT_MAX,
            ..Summary::default()
        };
        finish(&mut summary);
        assert_eq!(summary.min_obj_size, 0x7fff_ffff);
        assert_eq!(summary.avg_obj_size, 0);
    }
}
