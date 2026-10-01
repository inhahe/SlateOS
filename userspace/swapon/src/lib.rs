//! What `swapon` and `swapoff` share: util-linux 2.39.3's
//! `sys-utils/swapon-common.c` (the swap and fstab tables, each read once,
//! and the `-L`/`-U` lists) and `lib/swapprober.c` (libblkid asked about a
//! swap area), with the program-name and diagnostic helpers both use.
//!
//! The two programs are `src/main.rs` (`swapon`) and `src/bin/swapoff.rs`.
//! Before these ports one hand-written program answered to both names.

use std::ffi::OsStr;

use ulblkid::Probe;
use ulmount::cache::Cache;
use ulmount::tab::{Direction, Table};
use ulmount::tab_parse;

pub use ulclosestream::{Stdout, stderr_write, warn, warnx};

/// `program_invocation_short_name`: argv[0] past its last `/`.
#[must_use]
pub fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = quoting::os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Bytes shown in a diagnostic: upstream's text, unprintable bytes escaped
/// (design-decisions §370).
#[must_use]
pub fn shown(text: &[u8]) -> String {
    quoting::escape_unprintable(text)
}

/// `warn(msg)` with `errno` as the error.
pub fn warn_errno(short: &[u8], msg: &str, errno: i32) {
    if errno == 0 {
        warnx(short, &format!("{msg}: Success"));
    } else {
        warn(short, msg, &std::io::Error::from_raw_os_error(errno));
    }
}

/// `errtryhelp(status)`: the referral, and the status to exit with.
#[must_use]
pub fn errtryhelp(short: &[u8], status: u8) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    status
}

/// `table_parser_errcb`: a line that does not parse is reported and
/// skipped.
fn parser_errcb(short: &[u8]) -> impl FnMut(&[u8], usize) -> i32 + '_ {
    move |filename: &[u8], line: usize| {
        warnx(
            short,
            &format!("{}: parse error at line {line} -- ignored", shown(filename)),
        );
        1
    }
}

/// A table `get_swaps` or `get_fstab` read, or tried to.
#[derive(Default)]
enum Loaded {
    #[default]
    NotRead,
    /// Read -- and whether reading it failed, which upstream reports only
    /// the first time: its static table is set before it is parsed, so a
    /// second call returns what did parse.
    Read { table: Table },
}

/// swapon-common.c's statics: the tables, the cache they share
/// (`mntcache`), and the `-L` and `-U` lists.
pub struct Common {
    pub short: Vec<u8>,
    swaps: Loaded,
    fstab: Loaded,
    /// `mntcache`.
    pub cache: Cache,
    /// `llist`: the `-L` labels, in order.
    pub labels: Vec<Vec<u8>>,
    /// `ulist`: the `-U` UUIDs, in order.
    pub uuids: Vec<Vec<u8>>,
}

impl Common {
    #[must_use]
    pub fn new(short: Vec<u8>) -> Self {
        Common {
            short,
            swaps: Loaded::NotRead,
            fstab: Loaded::NotRead,
            cache: Cache::new(),
            labels: Vec::new(),
            uuids: Vec::new(),
        }
    }

    /// `get_fstab(filename)`: fstab (`LIBMOUNT_FSTAB`, or `/etc/fstab`,
    /// without a name), read once. `None` when the first reading fails.
    pub fn get_fstab(&mut self, filename: Option<&[u8]>) -> Option<&Table> {
        if matches!(self.fstab, Loaded::NotRead) {
            let mut table = Table::new();
            let mut cb = parser_errcb(&self.short);
            let rc = tab_parse::parse_fstab(&mut table, filename, Some(&mut cb));
            self.fstab = Loaded::Read { table };
            rc.ok()?;
        }
        match &self.fstab {
            Loaded::Read { table } => Some(table),
            Loaded::NotRead => None,
        }
    }

    /// `get_swaps()`: `/proc/swaps` (or `LIBMOUNT_SWAPS`), read once.
    /// `None` when the first reading fails.
    pub fn get_swaps(&mut self) -> Option<&Table> {
        if matches!(self.swaps, Loaded::NotRead) {
            let mut table = Table::new();
            let mut cb = parser_errcb(&self.short);
            let rc = tab_parse::parse_swaps(&mut table, None, Some(&mut cb));
            self.swaps = Loaded::Read { table };
            rc.ok()?;
        }
        match &self.swaps {
            Loaded::Read { table } => Some(table),
            Loaded::NotRead => None,
        }
    }

    /// The swaps table as it was read, without reading it.
    #[must_use]
    pub fn swaps_read(&self) -> Option<&Table> {
        match &self.swaps {
            Loaded::Read { table } => Some(table),
            Loaded::NotRead => None,
        }
    }

    /// `is_active_swap(filename)`: in `/proc/swaps`, by source (a tag, or a
    /// path compared as `mnt_table_find_source` compares it).
    pub fn is_active_swap(&mut self, filename: &[u8]) -> bool {
        if self.get_swaps().is_none() {
            return false;
        }
        let Loaded::Read { table } = &self.swaps else {
            return false;
        };
        table
            .find_source(filename, Direction::Backward, Some(&mut self.cache))
            .is_some()
    }

    /// `cannot_find(special)`: -1, with the warning.
    pub fn cannot_find(&self, special: &[u8]) -> i32 {
        warnx(
            &self.short,
            &format!("cannot find the device for {}", shown(special)),
        );
        -1
    }
}

/// `match_swap(fs)`: a swap area.
#[must_use]
pub fn match_swap(tb: &Table, i: usize) -> bool {
    tb.ents.get(i).is_some_and(ulmount::fs::Fs::is_swaparea)
}

/// `SWAP_VERSION`, as `stringify_value` spells it.
const SWAP_VERSION: &[u8] = b"1";

/// `get_swap_prober(devname)`: libblkid's answer about the swap area on
/// `devname` -- its UUID, LABEL and VERSION -- or `None`, with the reason
/// said: a device that cannot be probed, is ambiguous, holds no swap area,
/// or holds one of a version other than SWAPSPACE2's.
#[must_use]
pub fn get_swap_prober(short: &[u8], devname: &[u8]) -> Option<Probe> {
    let mut pr = match Probe::from_filename(devname) {
        Ok(pr) => pr,
        Err(e) => {
            warn_errno(
                short,
                &format!("{}: unable to probe device", shown(devname)),
                e,
            );
            return None;
        }
    };
    pr.enable_superblocks(true);
    pr.set_superblocks_flags(
        ulblkid::SUBLKS_LABEL | ulblkid::SUBLKS_UUID | ulblkid::SUBLKS_VERSION,
    );
    // Refused only for an unknown flag, which ONLYIN is not.
    let _ = pr.filter_superblocks_type(ulblkid::FLTR_ONLYIN, &[b"swap"]);
    match pr.do_safeprobe() {
        0 => {
            let version = pr.lookup_value("VERSION").map(|v| v.as_c_str().to_vec());
            match version {
                Some(v) if v != SWAP_VERSION => {
                    warnx(
                        short,
                        &format!(
                            "{}: unsupported swap version {}",
                            shown(devname),
                            quoting::escaped_in_quotes(&v)
                        ),
                    );
                    None
                }
                _ => Some(pr),
            }
        }
        -1 => {
            warn_errno(
                short,
                &format!("{}: unable to probe device", shown(devname)),
                pr.errno,
            );
            None
        }
        -2 => {
            warnx(
                short,
                &format!(
                    "{}: ambiguous probing result; use wipefs(8)",
                    shown(devname)
                ),
            );
            None
        }
        1 => {
            warnx(
                short,
                &format!("{}: not a valid swap partition", shown(devname)),
            );
            None
        }
        _ => None,
    }
}

/// A probe's value, as the C string it is.
#[must_use]
pub fn probe_value(pr: &Probe, name: &str) -> Option<Vec<u8>> {
    pr.lookup_value(name).map(|v| v.as_c_str().to_vec())
}

/// `ul_strtou64`-based `str2num_or_err(str, 10, errmesg, INT16_MIN,
/// INT16_MAX)` -- `strtos16_or_err`: the number, or the message and
/// `EXIT_FAILURE`.
///
/// # Errors
///
/// The status to exit with, the message printed.
pub fn strtos16_or_err(s: &[u8], errmesg: &str, short: &[u8]) -> Result<i16, u8> {
    let parsed = ulstrutils::ul_strtos64(s, 10)
        .and_then(|v| i16::try_from(v).map_err(|_| ulstrutils::NumErr::Range));
    parsed.map_err(|e| {
        warnx(
            short,
            &ulstrutils::num_error_message(errmesg, &quoting::os_from_bytes(s), e),
        );
        1
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests unwrap what they built")]
mod tests {
    use super::*;

    #[test]
    fn short_names_are_argv0s_last_component() {
        assert_eq!(short_name(OsStr::new("/sbin/swapon")), b"swapon");
        assert_eq!(short_name(OsStr::new("swapoff")), b"swapoff");
    }

    #[test]
    fn priorities_are_sixteen_bit() {
        assert_eq!(strtos16_or_err(b"5", "x", b"swapon"), Ok(5));
        assert_eq!(strtos16_or_err(b"-1", "x", b"swapon"), Ok(-1));
        assert_eq!(strtos16_or_err(b"32768", "x", b"swapon"), Err(1));
        assert_eq!(strtos16_or_err(b"x", "x", b"swapon"), Err(1));
    }
}
