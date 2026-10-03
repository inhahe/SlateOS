//! procps-ng 4.0.4's `library/devname.c`: a terminal's device number to its
//! name.
//!
//! `/proc/<pid>/stat` gives a process's controlling terminal as a number --
//! `34817` is major 136, minor 1 -- and `ps` prints `pts/1`. The name is not
//! computed from the number; it is *found*, by four guesses tried in order,
//! each of which must name a file in `/dev` whose device number is the one
//! wanted:
//!
//! 1. `driver_name`: `/proc/tty/drivers` says which driver owns the major
//!    and what its nodes are called (`/dev/pts`, `/dev/ttyS`); the node is
//!    `/dev/<name><minor>`, else `/dev/<name>/<minor>`, else `/dev/<name>`.
//! 2. `link_name` on `/proc/<pid>/fd/2`: wherever the process's standard
//!    error points, if that is the terminal.
//! 3. `guess_name`: a table of Linux's assignments (`4` is `tty<n>` below 64
//!    and `ttyS<n-64>` above, `136`-`143` are `pts/<n>`, ...).
//! 4. `link_name` on `/proc/<pid>/fd/255`, where bash keeps its terminal.
//!
//! If none of them finds a file with the right number, the name is `?`. On a
//! system whose `stat` reports no device numbers, that is every terminal --
//! which is right: a name that was not checked could be another terminal's.

use std::fs;
use std::path::{Path, PathBuf};

use super::scanf;

/// `ABBREV_DEV`: drop a leading `/dev/`.
pub const ABBREV_DEV: u32 = 1;
/// `ABBREV_TTY`: drop a leading `tty`.
pub const ABBREV_TTY: u32 = 2;
/// `ABBREV_PTS`: drop a leading `pts/`.
pub const ABBREV_PTS: u32 = 4;

/// `TTY_NAME_SIZE`: the buffer the name is built in.
const TTY_NAME_SIZE: usize = 128;

/// glibc's `major` on a 64-bit device number.
#[must_use]
pub fn major(dev: u64) -> u32 {
    let m = ((dev & 0x0000_0000_000f_ff00) >> 8) | ((dev & 0xffff_f000_0000_0000) >> 32);
    u32::try_from(m).unwrap_or(u32::MAX)
}

/// glibc's `minor` on a 64-bit device number.
#[must_use]
pub fn minor(dev: u64) -> u32 {
    let m = (dev & 0x0000_0000_0000_00ff) | ((dev & 0x0000_0fff_fff0_0000) >> 12);
    u32::try_from(m).unwrap_or(u32::MAX)
}

/// One line of `/proc/tty/drivers`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Driver {
    /// A `%d` was cut from the end of the name (devfs style): never try the
    /// bare name.
    devfs: bool,
    major: u16,
    first: u32,
    last: u32,
    name: Vec<u8>,
}

/// `load_drivers`, on the text it reads: the entries, *last line first* --
/// upstream pushes each onto the front of its list -- so a later line for
/// the same numbers wins.
fn parse_drivers(text: &[u8]) -> Vec<Driver> {
    let buf = super::readproc::cstr(text);
    let mut out: Vec<Driver> = Vec::new();
    let mut p = 0usize;
    let find = |from: usize, needle: &[u8]| {
        buf.get(from..)
            .and_then(|r| r.windows(needle.len()).position(|w| w == needle))
            .map(|n| from.saturating_add(n))
    };
    while let Some(at) = find(p, b" /dev/") {
        p = at.saturating_add(6);
        let Some(end) = buf
            .get(p..)
            .and_then(|r| r.iter().position(|&b| b == b' '))
            .map(|n| p.saturating_add(n))
        else {
            continue;
        };
        let mut len = end.saturating_sub(p);
        let mut devfs = false;
        if len >= 3 && buf.get(end.saturating_sub(2)..end) == Some(b"%d") {
            len = len.saturating_sub(2);
            devfs = true;
        }
        len = len.min(15);
        let name = buf
            .get(p..p.saturating_add(len))
            .unwrap_or_default()
            .to_vec();
        p = end;
        while buf.get(p) == Some(&b' ') {
            p = p.saturating_add(1);
        }
        let rest = buf.get(p..).unwrap_or_default();
        // `unsigned short` from `atoi`: the low sixteen bits.
        let major = scanf::low_u16(scanf::atoi(rest));
        p = p.saturating_add(rest.iter().take_while(|b| b.is_ascii_digit()).count());
        while buf.get(p) == Some(&b' ') {
            p = p.saturating_add(1);
        }
        // `sscanf (p, "%u-%u", ...)`.
        let mut sc = scanf::Scan::new(buf.get(p..).unwrap_or_default());
        let low32 = |v: u64| scanf::low_u32(i64::from_le_bytes(v.to_le_bytes()));
        let Some(first) = sc.ulong().map(low32) else {
            continue;
        };
        let last = if sc.lit(b"-").is_some() {
            sc.ulong().map_or(first, low32)
        } else {
            first
        };
        out.insert(
            0,
            Driver {
                devfs,
                major,
                first,
                last,
                name,
            },
        );
    }
    out
}

/// What `stat (path).st_rdev` is, or `None` where `stat` fails.
pub type RdevFn = dyn Fn(&Path) -> Option<u64>;

/// The `/dev` name of a terminal device, found as upstream finds it.
pub struct Devname {
    /// Where `/proc` is.
    root: PathBuf,
    /// `tty_map`: `None` until loaded; then the drivers, possibly none.
    drivers: Option<Vec<Driver>>,
    /// `stat (path).st_rdev`, or `None` where `stat` fails.
    rdev: Box<RdevFn>,
}

/// `stat`'s device number for `path`, following links.
#[cfg(unix)]
fn real_rdev(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| m.rdev())
}

/// The host build has no device numbers; it never reads a real `/proc`.
#[cfg(not(unix))]
fn real_rdev(_path: &Path) -> Option<u64> {
    None
}

impl Devname {
    /// Names found against the real `/dev`, with `/proc` at `root`.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self::with_rdev(root, Box::new(real_rdev))
    }

    /// Names found against whatever `rdev` says a path's device number is.
    #[must_use]
    pub fn with_rdev(root: PathBuf, rdev: Box<RdevFn>) -> Self {
        Self {
            root,
            drivers: None,
            rdev,
        }
    }

    /// Whether `path` is the device `maj:min`.
    fn is_dev(&self, path: &[u8], maj: u32, min: u32) -> Option<bool> {
        let dev = (self.rdev)(&PathBuf::from(crate::quote::os_from_bytes(path)))?;
        Some(minor(dev) == min && major(dev) == maj)
    }

    /// `driver_name`.
    fn driver_name(&mut self, maj: u32, min: u32) -> Option<Vec<u8>> {
        if self.drivers.is_none() {
            // `load_drivers`: a file that cannot be opened or read is no
            // drivers at all (`tty_map = -1`), and names come from the other
            // three guesses -- which is what SlateOS, with no such file, gets.
            let drivers = match read_once(&self.root.join("tty/drivers"), 9999) {
                Some(text) => parse_drivers(&text),
                None => Vec::new(),
            };
            self.drivers = Some(drivers);
        }
        let tmn = self
            .drivers
            .as_ref()?
            .iter()
            .find(|d| u32::from(d.major) == maj && d.first <= min && d.last >= min)?
            .clone();
        let mut buf = [b"/dev/".as_slice(), &tmn.name, min.to_string().as_bytes()].concat();
        if (self.rdev)(&PathBuf::from(crate::quote::os_from_bytes(&buf))).is_none() {
            buf = [
                b"/dev/".as_slice(),
                &tmn.name,
                b"/",
                min.to_string().as_bytes(),
            ]
            .concat();
            if (self.rdev)(&PathBuf::from(crate::quote::os_from_bytes(&buf))).is_none() {
                if tmn.devfs {
                    return None;
                }
                buf = [b"/dev/".as_slice(), &tmn.name].concat();
            }
        }
        self.is_dev(&buf, maj, min)?.then_some(buf)
    }

    /// `link_name`: where `/proc/<pid>/<name>` points, if that is the device.
    fn link_name(&self, maj: u32, min: u32, pid: i32, name: &str) -> Option<Vec<u8>> {
        let target = fs::read_link(self.root.join(format!("{pid}/{name}"))).ok()?;
        let buf = crate::quote::os_bytes(target.as_os_str()).into_owned();
        if buf.is_empty() || buf.len() >= TTY_NAME_SIZE.saturating_sub(1) {
            return None;
        }
        self.is_dev(&buf, maj, min)?.then_some(buf)
    }

    /// `dev_to_tty`: the name of terminal `dev` (a `tty_nr`), for process
    /// `pid`, cut to `chop` characters, abbreviated as `flags` say. A byte that
    /// is a space or a control or not ASCII becomes `?`. No terminal at all
    /// is `?` too -- or nothing, when `chop` is 0.
    pub fn dev_to_tty(&mut self, chop: usize, dev: u32, pid: i32, flags: u32) -> Vec<u8> {
        let maj = major(u64::from(dev));
        let min = minor(u64::from(dev));
        let found = if dev == 0 {
            None
        } else {
            self.driver_name(maj, min)
                .or_else(|| self.link_name(maj, min, pid, "fd/2"))
                .or_else(|| guess_name(maj, min).filter(|b| self.is_dev(b, maj, min) == Some(true)))
                .or_else(|| self.link_name(maj, min, pid, "fd/255"))
        };
        let Some(name) = found else {
            return if chop >= 1 { b"?".to_vec() } else { Vec::new() };
        };
        let mut tmp: &[u8] = &name;
        if flags & ABBREV_DEV != 0 && tmp.starts_with(b"/dev/") && tmp.len() > 5 {
            tmp = tmp.get(5..).unwrap_or_default();
        }
        if flags & ABBREV_TTY != 0 && tmp.starts_with(b"tty") && tmp.len() > 3 {
            tmp = tmp.get(3..).unwrap_or_default();
        }
        if flags & ABBREV_PTS != 0 && tmp.starts_with(b"pts/") && tmp.len() > 4 {
            tmp = tmp.get(4..).unwrap_or_default();
        }
        tmp.iter()
            .take(chop)
            .map(|&c| if c <= b' ' || c > 126 { b'?' } else { c })
            .collect()
    }
}

/// One `read (fd, buf, max)`.
fn read_once(path: &Path, max: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

/// Major 204's minors: "low-density serial ports", one name each.
const LOW_DENSITY: &[&str] = &[
    "LU0", "LU1", "LU2", "LU3", "FB0", "SA0", "SA1", "SA2", "SC0", "SC1", "SC2", "SC3", "FW0",
    "FW1", "FW2", "FW3", "AM0", "AM1", "AM2", "AM3", "AM4", "AM5", "AM6", "AM7", "AM8", "AM9",
    "AM10", "AM11", "AM12", "AM13", "AM14", "AM15", "DB0", "DB1", "DB2", "DB3", "DB4", "DB5",
    "DB6", "DB7", "SG0", "SMX0", "SMX1", "SMX2", "MM0", "MM1", "CPM0", "CPM1", "CPM2", "CPM3",
    "IOC0", "IOC1", "IOC2", "IOC3", "IOC4", "IOC5", "IOC6", "IOC7", "IOC8", "IOC9", "IOC10",
    "IOC11", "IOC12", "IOC13", "IOC14", "IOC15", "IOC16", "IOC17", "IOC18", "IOC19", "IOC20",
    "IOC21", "IOC22", "IOC23", "IOC24", "IOC25", "IOC26", "IOC27", "IOC28", "IOC29", "IOC30",
    "IOC31", "VR0", "VR1", "IOC84", "IOC85", "IOC86", "IOC87", "IOC88", "IOC89", "IOC90", "IOC91",
    "IOC92", "IOC93", "IOC94", "IOC95", "IOC96", "IOC97", "IOC98", "IOC99", "IOC100", "IOC101",
    "IOC102", "IOC103", "IOC104", "IOC105", "IOC106", "IOC107", "IOC108", "IOC109", "IOC110",
    "IOC111", "IOC112", "IOC113", "IOC114", "IOC115", "SIOC0", "SIOC1", "SIOC2", "SIOC3", "SIOC4",
    "SIOC5", "SIOC6", "SIOC7", "SIOC8", "SIOC9", "SIOC10", "SIOC11", "SIOC12", "SIOC13", "SIOC14",
    "SIOC15", "SIOC16", "SIOC17", "SIOC18", "SIOC19", "SIOC20", "SIOC21", "SIOC22", "SIOC23",
    "SIOC24", "SIOC25", "SIOC26", "SIOC27", "SIOC28", "SIOC29", "SIOC30", "SIOC31", "PSC0", "PSC1",
    "PSC2", "PSC3", "PSC4", "PSC5", "AT0", "AT1", "AT2", "AT3", "AT4", "AT5", "AT6", "AT7", "AT8",
    "AT9", "AT10", "AT11", "AT12", "AT13", "AT14", "AT15", "NX0", "NX1", "NX2", "NX3", "NX4",
    "NX5", "NX6", "NX7", "NX8", "NX9", "NX10", "NX11", "NX12", "NX13", "NX14", "NX15", "J0", "UL0",
    "UL1", "UL2", "UL3", "xvc0", "PZ0", "PZ1", "PZ2", "PZ3", "TX0", "TX1", "TX2", "TX3", "TX4",
    "TX5", "TX6", "TX7", "SC0", "SC1", "SC2", "SC3", "MAX0", "MAX1", "MAX2", "MAX3",
];

/// `guess_name`'s candidate for `maj:min`, before the check that it is that
/// device: Linux's own assignments, or `None` for a major it does not know.
#[must_use]
pub fn guess_name(maj: u32, min: u32) -> Option<Vec<u8>> {
    let prefix = |p: &str| Some(format!("/dev/{p}{min}").into_bytes());
    match maj {
        3 => {
            if min > 255 {
                return None;
            }
            let hi = b"pqrstuvwxyzabcde"
                .get(usize::try_from(min >> 4).ok()?)
                .copied()?;
            let lo = b"0123456789abcdef"
                .get(usize::try_from(min & 0x0f).ok()?)
                .copied()?;
            Some([b"/dev/tty".as_slice(), &[hi, lo]].concat())
        }
        4 => {
            if min < 64 {
                prefix("tty")
            } else {
                Some(format!("/dev/ttyS{}", min.wrapping_sub(64)).into_bytes())
            }
        }
        11 => prefix("ttyB"),
        17 => prefix("ttyH"),
        19 => prefix("ttyC"),
        22 | 23 => prefix("ttyD"),
        24 => prefix("ttyE"),
        32 => prefix("ttyX"),
        43 => prefix("ttyI"),
        46 => prefix("ttyR"),
        48 => prefix("ttyL"),
        57 => prefix("ttyP"),
        71 => prefix("ttyF"),
        75 => prefix("ttyW"),
        78 | 112 => prefix("ttyM"),
        105 => prefix("ttyV"),
        136..=143 => {
            // `%d` of `min + (maj - 136) * 256`, in `unsigned` arithmetic.
            let n = min.wrapping_add(maj.wrapping_sub(136).wrapping_mul(256));
            Some(format!("/dev/pts/{}", i32::from_le_bytes(n.to_le_bytes())).into_bytes())
        }
        148 => prefix("ttyT"),
        154 => prefix("ttySR"),
        156 => Some(format!("/dev/ttySR{}", min.wrapping_add(256)).into_bytes()),
        164 => prefix("ttyCH"),
        166 => prefix("ttyACM"),
        172 => prefix("ttyMX"),
        174 => prefix("ttySI"),
        188 => prefix("ttyUSB"),
        204 => {
            let name = LOW_DENSITY.get(usize::try_from(min).ok()?)?;
            Some(format!("/dev/tty{name}").into_bytes())
        }
        208 => prefix("ttyU"),
        216 => prefix("ttyUB"),
        224 => prefix("ttyY"),
        227 => prefix("3270/tty"),
        229 => prefix("iseries/vtty"),
        256 => prefix("ttyEQ"),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// `makedev` as glibc's: a device number from a major and a minor.
    fn makedev(maj: u64, min: u64) -> u64 {
        ((maj & 0xfff) << 8) | ((maj & !0xfff) << 32) | (min & 0xff) | ((min & !0xff) << 12)
    }

    fn fake_dev(entries: &[(&'static str, u64)]) -> Box<RdevFn> {
        let entries: Vec<(&'static str, u64)> = entries.to_vec();
        Box::new(move |p: &Path| {
            entries
                .iter()
                .find(|(name, _)| Path::new(name) == p)
                .map(|&(_, d)| d)
        })
    }

    #[test]
    fn numbers_split_as_glibc_splits_them() {
        let d = makedev(136, 300);
        assert_eq!((major(d), minor(d)), (136, 300));
        assert_eq!((major(34817), minor(34817)), (136, 1));
        assert_eq!((major(1025), minor(1025)), (4, 1));
    }

    #[test]
    fn drivers_are_read_last_line_first() {
        let text = b"/dev/tty             /dev/tty        5       0 system:/dev/tty\n\
                     serial               /dev/ttyS       4 64-95 serial\n\
                     pty_slave            /dev/pts      136 0-1048575 pty:slave\n\
                     devfs                /dev/tts/%d     4 64-95 serial\n\
                     junk                 /dev/junk       9 none\n";
        let d = parse_drivers(text);
        let names: Vec<&[u8]> = d.iter().map(|x| x.name.as_slice()).collect();
        assert_eq!(names, [&b"tts/"[..], b"pts", b"ttyS", b"tty"]);
        assert!(d.first().unwrap().devfs);
        assert_eq!((d[1].major, d[1].first, d[1].last), (136, 0, 1_048_575));
        assert_eq!((d[3].first, d[3].last), (0, 0));
    }

    #[test]
    fn a_name_is_only_given_if_the_device_has_that_number() {
        let dir = std::env::temp_dir().join("lane-b-devname-none");
        let mut dn = Devname::with_rdev(
            dir.clone(),
            fake_dev(&[("/dev/tty3", makedev(4, 3)), ("/dev/pts/7", makedev(4, 7))]),
        );
        // guess_name: 4:3 is /dev/tty3, and the device agrees.
        assert_eq!(dn.dev_to_tty(64, 1027, 1, ABBREV_DEV), b"tty3");
        assert_eq!(dn.dev_to_tty(64, 1027, 1, ABBREV_DEV | ABBREV_TTY), b"3");
        assert_eq!(dn.dev_to_tty(64, 1027, 1, 0), b"/dev/tty3");
        // /dev/pts/7 exists but is 4:7, not 136:7.
        assert_eq!(
            dn.dev_to_tty(64, makedev(136, 7) as u32, 1, ABBREV_DEV),
            b"?"
        );
        // No terminal.
        assert_eq!(dn.dev_to_tty(64, 0, 1, ABBREV_DEV), b"?");
        assert_eq!(dn.dev_to_tty(0, 0, 1, ABBREV_DEV), b"");
        // Cut to `chop`.
        assert_eq!(dn.dev_to_tty(4, 1027, 1, 0), b"/dev");
    }

    #[test]
    fn guesses_follow_the_table() {
        assert_eq!(guess_name(3, 0x1f).unwrap(), b"/dev/ttyqf");
        assert_eq!(guess_name(3, 256), None);
        assert_eq!(guess_name(4, 64).unwrap(), b"/dev/ttyS0");
        assert_eq!(guess_name(137, 2).unwrap(), b"/dev/pts/258");
        assert_eq!(guess_name(204, 0).unwrap(), b"/dev/ttyLU0");
        assert_eq!(guess_name(204, 186).unwrap(), b"/dev/ttyJ0");
        assert_eq!(guess_name(204, 211).unwrap(), b"/dev/ttyMAX3");
        assert_eq!(guess_name(204, 212), None);
        assert_eq!(guess_name(229, 1).unwrap(), b"/dev/iseries/vtty1");
        assert_eq!(guess_name(5, 1), None);
    }
}
