//! `/proc/diskstats`, transcribed from procps-ng 4.0.4's
//! `library/diskstats.c`, for `vmstat -d`, `-D` and `-p`.
//!
//! What the library does and this keeps:
//!
//! - The file is opened once and read from its start each time, a line at a
//!   time through `fgets` into 1024 bytes, and every line must make all 14 of
//!   `"%d %d %34s %lu …"`'s conversions or the whole read fails (`ERANGE`) --
//!   a name longer than 34 bytes is cut there and its tail fails the number
//!   after it.
//! - Each device is a node kept from read to read, in the order it was first
//!   seen. A new node is classified once: a disk if `/sys/block` holds an
//!   entry of that name, or if `/sys/block` cannot be read at all; otherwise
//!   a partition.
//! - A node is stamped with `time()` at each read that sees it, and one whose
//!   stamp is neither of the last two reads' is dropped when it is next looked
//!   up by name -- and, if the read doing the looking up sees it again, made
//!   anew at the end of the list. [`DiskStats::reap`] hands back every node
//!   still listed, stale ones included, as `procps_diskstats_reap` does.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};

use super::scanf::{Scan, low_i32};
use super::vmstat::now_secs;

/// `DISKSTATS_FILE`.
pub const DISKSTATS_FILE: &str = "/proc/diskstats";

/// `SYSBLOCK_DIR`.
const SYSBLOCK_DIR: &str = "/sys/block";

/// `DISKSTATS_LINE_LEN`: `fgets`' buffer, of which one byte is the NUL.
const DISKSTATS_LINE_LEN: usize = 1024;

/// `DISKSTATS_NAME_LEN`: the `%34s` width.
const DISKSTATS_NAME_LEN: usize = 34;

/// `DISKSTATS_TYPE_DISK` and `DISKSTATS_TYPE_PARTITION`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Disk,
    Partition,
}

/// One device: `struct dev_node`, its newest reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub major: i32,
    pub minor: i32,
    pub name: Vec<u8>,
    pub reads: u64,
    pub reads_merged: u64,
    pub read_sectors: u64,
    pub read_time: u64,
    pub writes: u64,
    pub writes_merged: u64,
    pub write_sectors: u64,
    pub write_time: u64,
    pub io_inprogress: u64,
    pub io_time: u64,
    pub io_wtime: u64,
    pub kind: Kind,
    stamped: i64,
}

/// One line through the library's `sscanf`, or `None` when it did not make all
/// 14 conversions.
#[must_use]
pub fn parse_line(line: &[u8]) -> Option<Node> {
    let mut scan = Scan::new(line);
    let major = scan.int()?;
    let minor = scan.int()?;
    let name = scan.word(DISKSTATS_NAME_LEN)?.to_vec();
    let mut values = [0u64; 11];
    for v in &mut values {
        *v = scan.ulong()?;
    }
    let [
        reads,
        reads_merged,
        read_sectors,
        read_time,
        writes,
        writes_merged,
        write_sectors,
        write_time,
        io_inprogress,
        io_time,
        io_wtime,
    ] = values;
    Some(Node {
        major,
        minor,
        name,
        reads,
        reads_merged,
        read_sectors,
        read_time,
        writes,
        writes_merged,
        write_sectors,
        write_time,
        io_inprogress,
        io_time,
        io_wtime,
        kind: Kind::Partition,
        stamped: 0,
    })
}

/// `node_classify`: a disk if `/sys/block` lists `name`, or if `/sys/block`
/// cannot be read.
///
/// Upstream's `readdir` also lists `.` and `..`, which `read_dir` leaves out,
/// so a device of either name is a disk whenever the directory opens.
fn classify(name: &[u8]) -> Kind {
    let Ok(dir) = std::fs::read_dir(SYSBLOCK_DIR) else {
        return Kind::Disk;
    };
    if name == b"." || name == b".." {
        return Kind::Disk;
    }
    for entry in dir.flatten() {
        if crate::quote::os_bytes(&entry.file_name()).as_ref() == name {
            return Kind::Disk;
        }
    }
    Kind::Partition
}

/// procps' `struct diskstats_info`, cut to what `vmstat` uses.
#[derive(Debug)]
pub struct DiskStats {
    file: File,
    nodes: Vec<Node>,
    old_stamp: i64,
    new_stamp: i64,
}

impl DiskStats {
    /// `procps_diskstats_new`, with its priming read.
    ///
    /// # Errors
    ///
    /// The file could not be opened or read, or a line of it did not parse.
    pub fn new() -> io::Result<Self> {
        let mut stats = Self {
            file: File::open(DISKSTATS_FILE)?,
            nodes: Vec::new(),
            old_stamp: 0,
            new_stamp: 0,
        };
        stats.read()?;
        Ok(stats)
    }

    /// `diskstats_read_failed`.
    fn read(&mut self) -> io::Result<()> {
        /// `ERANGE`.
        const ERANGE: i32 = 34;
        self.file.seek(SeekFrom::Start(0))?;
        self.old_stamp = self.new_stamp;
        self.new_stamp = now_secs();
        let lines = {
            let mut reader = BufReader::new(&self.file);
            let mut lines = Vec::new();
            loop {
                let mut line = Vec::new();
                // `fgets` into DISKSTATS_LINE_LEN: at most 1023 bytes, ending
                // early at a newline, which it keeps.
                let mut taken = 0usize;
                while taken < DISKSTATS_LINE_LEN.saturating_sub(1) {
                    let buf = reader.fill_buf()?;
                    let Some(&c) = buf.first() else { break };
                    reader.consume(1);
                    line.push(c);
                    taken = taken.saturating_add(1);
                    if c == b'\n' {
                        break;
                    }
                }
                if line.is_empty() {
                    break;
                }
                lines.push(line);
            }
            lines
        };
        for line in lines {
            let Some(mut node) = parse_line(&line) else {
                return Err(io::Error::from_raw_os_error(ERANGE));
            };
            node.stamped = self.new_stamp;
            self.update(node);
        }
        Ok(())
    }

    /// `node_get`: the node of that name -- unless its stamp is neither of
    /// the last two reads', in which case it is dropped and there is none.
    fn find(&mut self, name: &[u8]) -> Option<usize> {
        let at = self.nodes.iter().position(|n| n.name == name)?;
        let stamped = self.nodes.get(at)?.stamped;
        if stamped != self.old_stamp && stamped != self.new_stamp {
            self.nodes.remove(at);
            return None;
        }
        Some(at)
    }

    /// `node_update`.
    fn update(&mut self, mut node: Node) {
        match self.find(&node.name) {
            Some(at) => {
                if let Some(target) = self.nodes.get_mut(at) {
                    node.kind = target.kind;
                    *target = node;
                }
            }
            None => {
                node.kind = classify(&node.name);
                self.nodes.push(node);
            }
        }
    }

    /// `procps_diskstats_reap`: read, then every node still listed.
    ///
    /// # Errors
    ///
    /// As [`DiskStats::new`].
    pub fn reap(&mut self) -> io::Result<&[Node]> {
        self.read()?;
        Ok(&self.nodes)
    }

    /// `procps_diskstats_get`: the node of that name, the file read again
    /// first only if the clock has moved on a second since the last read.
    ///
    /// # Errors
    ///
    /// The read that was due failed.
    pub fn get(&mut self, name: &[u8]) -> io::Result<Option<&Node>> {
        if now_secs().saturating_sub(self.new_stamp) >= 1 {
            self.read()?;
        }
        Ok(self.find(name).and_then(|at| self.nodes.get(at)))
    }

    /// `procps_diskstats_select`: read, then the node of that name.
    ///
    /// # Errors
    ///
    /// As [`DiskStats::new`].
    pub fn select(&mut self, name: &[u8]) -> io::Result<Option<&Node>> {
        self.read()?;
        Ok(self.find(name).and_then(|at| self.nodes.get(at)))
    }
}

/// The in-progress count as `vmstat` reads it: `DISKSTATS_IO_INPROGRESS` is
/// an `s_int`, so the `unsigned long` is cut to a C `int` first.
#[must_use]
pub fn io_inprogress_int(node: &Node) -> i32 {
    low_i32(node.io_inprogress as i64)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_line_of_a_newer_kernel_parses_its_first_fourteen_fields() {
        let n =
            parse_line(b"   8       0 sda 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18\n").unwrap();
        assert_eq!((n.major, n.minor, n.name.as_slice()), (8, 0, &b"sda"[..]));
        assert_eq!((n.reads, n.io_inprogress, n.io_wtime), (1, 9, 11));
    }

    #[test]
    fn a_short_line_and_an_overlong_name_do_not_parse() {
        assert!(parse_line(b"8 0 sda 1 2 3\n").is_none());
        let long = format!("8 0 {} 1 2 3 4 5 6 7 8 9 10 11\n", "x".repeat(40));
        assert!(parse_line(long.as_bytes()).is_none());
        let fits = format!("8 0 {} 1 2 3 4 5 6 7 8 9 10 11\n", "x".repeat(34));
        assert!(parse_line(fits.as_bytes()).is_some());
    }

    #[test]
    fn in_progress_is_cut_to_an_int() {
        let mut n = parse_line(b"8 0 sda 1 2 3 4 5 6 7 8 9 10 11\n").unwrap();
        n.io_inprogress = 0x1_0000_0005;
        assert_eq!(io_inprogress_int(&n), 5);
    }
}
