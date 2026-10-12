//! `/proc/vmstat`, transcribed from procps-ng 4.0.4's `library/vmstat.c`, for
//! the four counters the `vmstat` program prints: pages paged in and out, and
//! pages swapped in and out.
//!
//! What the library does and this keeps: one `read` of at most 8191 bytes --
//! a longer file is cut there, not read on -- split into `KEY VALUE` lines,
//! each value through `strtoul`; a key it has no field for costs nothing; an
//! empty read is `EIO`. The file is opened once and read again from its start
//! each time. And `procps_vmstat_get` reads it again only when `time()` has
//! moved on by a whole second since the last read, so every value asked for
//! within one second comes from one reading.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use super::scanf::{c_str, strchr_from, strtoul};

/// `VMSTAT_FILE`.
pub const VMSTAT_FILE: &str = "/proc/vmstat";

/// `VMSTAT_BUFF`, of which one byte is kept for the terminating NUL.
const VMSTAT_BUFF: usize = 8192;

/// The counters the program prints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Data {
    pub pgpgin: u64,
    pub pgpgout: u64,
    pub pswpin: u64,
    pub pswpout: u64,
}

/// `vmstat_read_failed`'s parse of the text, a C string -- a NUL ends it:
/// a key runs to the next space, wherever that is, its value is `strtoul`
/// from just after the space, and the next key starts after the first
/// newline after that space.
#[must_use]
pub fn parse(text: &[u8]) -> Data {
    let text = c_str(text);
    let mut data = Data::default();
    let mut head = 0usize;
    while let Some(space) = strchr_from(text, head, b' ') {
        let key = text.get(head..space).unwrap_or_default();
        head = space.saturating_add(1);
        let slot = match key {
            b"pgpgin" => Some(&mut data.pgpgin),
            b"pgpgout" => Some(&mut data.pgpgout),
            b"pswpin" => Some(&mut data.pswpin),
            b"pswpout" => Some(&mut data.pswpout),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = strtoul(text.get(head..).unwrap_or_default()).0;
        }
        let Some(nl) = strchr_from(text, head, b'\n') else {
            break;
        };
        head = nl.saturating_add(1);
    }
    data
}

/// procps' `struct vmstat_info`, cut to what the program uses.
#[derive(Debug)]
pub struct VmStat {
    file: File,
    new: Data,
    /// `sav_secs`: when the file was last read for a `get`, by `time()`.
    sav_secs: i64,
}

impl VmStat {
    /// `procps_vmstat_new`, with its priming read.
    ///
    /// # Errors
    ///
    /// The file could not be opened or read, or read empty.
    pub fn new() -> io::Result<Self> {
        let mut vmstat = Self {
            file: File::open(VMSTAT_FILE)?,
            new: Data::default(),
            sav_secs: 0,
        };
        vmstat.read()?;
        Ok(vmstat)
    }

    /// `vmstat_read_failed`.
    fn read(&mut self) -> io::Result<()> {
        /// `EIO`.
        const EIO: i32 = 5;
        self.file.seek(SeekFrom::Start(0))?;
        let mut buf = vec![0u8; VMSTAT_BUFF.saturating_sub(1)];
        let size = loop {
            match self.file.read(&mut buf) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) => {}
                other => break other?,
            }
        };
        if size == 0 {
            return Err(io::Error::from_raw_os_error(EIO));
        }
        self.new = parse(buf.get(..size).unwrap_or_default());
        Ok(())
    }

    /// `procps_vmstat_get`: the counters, read again first if the clock has
    /// moved on a second since the last read.
    ///
    /// # Errors
    ///
    /// The read that was due failed.
    pub fn get(&mut self) -> io::Result<Data> {
        let cur_secs = now_secs();
        if cur_secs.saturating_sub(self.sav_secs) >= 1 {
            self.read()?;
            self.sav_secs = cur_secs;
        }
        Ok(self.new)
    }
}

/// `time(NULL)`.
#[must_use]
pub fn now_secs() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        // Before the epoch: `time` is negative there too.
        Err(e) => i64::try_from(e.duration().as_secs())
            .ok()
            .and_then(i64::checked_neg)
            .unwrap_or(i64::MIN),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_four_counters_and_nothing_else() {
        let d = parse(
            b"nr_free_pages 123\npgpgin 4567\npgpgout 89\npswpin 1\npswpout 2\npgfault 999\n",
        );
        assert_eq!(
            d,
            Data {
                pgpgin: 4567,
                pgpgout: 89,
                pswpin: 1,
                pswpout: 2
            }
        );
    }

    #[test]
    fn a_line_without_a_space_ends_the_parse_and_a_bad_value_is_zero() {
        assert_eq!(parse(b"pgpgin x\npgpgout 5\n").pgpgin, 0);
        assert_eq!(parse(b"pgpgin x\npgpgout 5\n").pgpgout, 5);
        // No space anywhere after the first line: nothing more is read.
        assert_eq!(parse(b"pgpgin 3\nnospace\npgpgout\n").pgpgout, 0);
        // A line with no space joins the next line's key, which is lost.
        assert_eq!(
            parse(b"pgpgin 3\nnospace\npgpgout 4\npswpin 5\n").pgpgout,
            0
        );
        assert_eq!(parse(b"pgpgin 3\nnospace\npgpgout 4\npswpin 5\n").pswpin, 5);
    }

    #[test]
    fn a_nul_ends_the_text() {
        assert_eq!(parse(b"pgpgin 3\0\npgpgout 4\n").pgpgout, 0);
        assert_eq!(parse(b"pgpgin 3\0\npgpgout 4\n").pgpgin, 3);
    }
}
