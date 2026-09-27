//! lsblk -- list block devices.
//!
//! A port of util-linux 2.39.3's `misc-utils/lsblk.c`, `lsblk-devtree.c`
//! ([`devtree`]), `lsblk-mnt.c` ([`mnt`]) and `lsblk-properties.c`
//! ([`props`]), function by function and with upstream's names, on top of
//! `smartcols` (libsmartcols), `ulmount` (libmount and libudev's database),
//! `ulblkid` (libblkid) and `ulsysfs` (`lib/sysfs.c` and `lib/path.c`).
//! Measured against `lsblk from util-linux 2.39.3` by
//! `scripts/lsblk-diff.sh`: on util-linux's own `--sysroot` snapshots and on
//! the live system.
//!
//! This replaces a hand-written program that recognised filesystems by its
//! own code and parsed its options by hand.
//!
//! Devices are found in sysfs (`/sys/block`, or `/sys/dev/block` for
//! `--inverse`), each with its partitions and its holders (or slaves) --
//! one device object however many ways it is reached -- then written into
//! the table as a tree from the roots, sorted by MAJ:MIN unless `--sort`
//! names another column.
//!
//! Upstream's quirks are kept where they show:
//!
//! * A device found in the mount table only by its canonical source path is
//!   marked as swap (and so has no FSSIZE), while a real swap area is not
//!   (and asks `statvfs("[SWAP]")`).
//! * With `--sysroot`, some reads are still of the running system: the
//!   device node's owner, the size of a mounted filesystem, a loop device's
//!   backing file, and -- for a device named on the command line -- its
//!   name and whole disk.
//! * `-T` with a column name that does not exist warns and draws no tree.
//!
//! # What is not upstream's
//!
//! * **udev is asked only where it runs** (`props`' documentation).
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **`LSBLK_DEBUG`** and the libraries' debug output are not ported.

mod devtree;
mod mnt;
mod parttypes;
mod props;
mod sys;

#[cfg(test)]
mod tests;

use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use std::rc::Rc;

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{CellView, ColumnId, JsonType, LineId, Table};
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulsysfs::PathCxt;

use devtree::{DevId, Devtree};
use mnt::Mnt;

/// Getopt's errors are only sentences here; the referral follows them.
const LSBLK: Program = Program::new("lsblk", 1);

/// Upstream's option string.
const SHORTS: &str = "AabdDzE:e:fhJlNnMmo:OpPiI:rstVvST::w:x:y";
/// `OPT_SYSROOT`: `CHAR_MAX + 1`.
const OPT_SYSROOT: i32 = 128;

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("bytes", Takes::Nothing),
    ("nodeps", Takes::Nothing),
    ("noempty", Takes::Nothing),
    ("discard", Takes::Nothing),
    ("dedup", Takes::Required),
    ("zoned", Takes::Nothing),
    ("help", Takes::Nothing),
    ("json", Takes::Nothing),
    ("output", Takes::Required),
    ("output-all", Takes::Nothing),
    ("merge", Takes::Nothing),
    ("perms", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("list", Takes::Nothing),
    ("ascii", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("inverse", Takes::Nothing),
    ("fs", Takes::Nothing),
    ("exclude", Takes::Required),
    ("include", Takes::Required),
    ("topology", Takes::Nothing),
    ("paths", Takes::Nothing),
    ("pairs", Takes::Nothing),
    ("scsi", Takes::Nothing),
    ("nvme", Takes::Nothing),
    ("virtio", Takes::Nothing),
    ("sort", Takes::Required),
    ("sysroot", Takes::Required),
    ("shell", Takes::Nothing),
    ("tree", Takes::Optional),
    ("version", Takes::Nothing),
    ("width", Takes::Required),
];
const LONG_VALS: [i32; 33] = [
    b'a' as i32,
    b'b' as i32,
    b'd' as i32,
    b'A' as i32,
    b'D' as i32,
    b'E' as i32,
    b'z' as i32,
    b'h' as i32,
    b'J' as i32,
    b'o' as i32,
    b'O' as i32,
    b'M' as i32,
    b'm' as i32,
    b'n' as i32,
    b'l' as i32,
    b'i' as i32,
    b'r' as i32,
    b's' as i32,
    b'f' as i32,
    b'e' as i32,
    b'I' as i32,
    b't' as i32,
    b'p' as i32,
    b'P' as i32,
    b'S' as i32,
    b'N' as i32,
    b'v' as i32,
    b'x' as i32,
    OPT_SYSROOT,
    b'y' as i32,
    b'T' as i32,
    b'V' as i32,
    b'w' as i32,
];

/// `excl[]`: rows and columns in ASCII order.
const EXCL: [&[i32]; 9] = [
    &[b'D' as i32, b'O' as i32],
    &[b'I' as i32, b'e' as i32],
    &[b'J' as i32, b'P' as i32, b'r' as i32],
    &[b'O' as i32, b'S' as i32],
    &[b'O' as i32, b'f' as i32],
    &[b'O' as i32, b'm' as i32],
    &[b'O' as i32, b'o' as i32],
    &[b'O' as i32, b't' as i32],
    &[b'P' as i32, b'T' as i32, b'l' as i32, b'r' as i32],
];

/// `LSBLK_EXIT_SOMEOK`: some of the devices named were listed.
const EXIT_SOMEOK: u8 = 64;
/// `LSBLK_EXIT_ALLFAILED`: none of them was.
const EXIT_ALLFAILED: u8 = 32;

/// `LOOPDEV_MAJOR`.
const LOOPDEV_MAJOR: u32 = 7;
/// `EINVAL`, for `__process_one_device`'s status.
const EINVAL: i32 = 22;
/// `ENOENT`, when a failure's own `errno` is not known.
const ENOENT: i32 = 2;
/// `ERANGE`.
const ERANGE: i32 = 34;

/// `LSBLK_*`: the basic table settings.
const LSBLK_ASCII: u32 = 1 << 0;
const LSBLK_RAW: u32 = 1 << 1;
const LSBLK_NOHEADINGS: u32 = 1 << 2;
const LSBLK_EXPORT: u32 = 1 << 3;
const LSBLK_TREE: u32 = 1 << 4;
const LSBLK_JSON: u32 = 1 << 5;
const LSBLK_SHELLVAR: u32 = 1 << 6;

/// The column IDs, `COL_*`, in upstream's order -- which is also the order
/// `--help` lists them and `-O` adds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Alioff,
    Idlink,
    Id,
    Dalign,
    Dax,
    Dgran,
    Diskseq,
    Dmax,
    Dzero,
    Fsavail,
    Fsroots,
    Fssize,
    Fstype,
    Fsused,
    Fsuseperc,
    Fsversion,
    Group,
    Hctl,
    Hotplug,
    Kname,
    Label,
    Logsec,
    Majmin,
    Minio,
    Mode,
    Model,
    Mq,
    Name,
    Optio,
    Owner,
    Partflags,
    Partlabel,
    Partn,
    Parttype,
    Parttypename,
    Partuuid,
    Path,
    Physec,
    Pkname,
    Pttype,
    Ptuuid,
    Ra,
    Rand,
    Rev,
    Rm,
    Ro,
    Rota,
    RqSize,
    Sched,
    Serial,
    Size,
    Start,
    State,
    Subsys,
    Target,
    Targets,
    Transport,
    Type,
    Uuid,
    Vendor,
    Wsame,
    Wwn,
    Zoned,
    ZoneSz,
    ZoneWgran,
    ZoneApp,
    ZoneNr,
    ZoneOmax,
    ZoneAmax,
}

/// Every column, in `COL_*` order.
const COLS: [Col; 69] = [
    Col::Alioff,
    Col::Idlink,
    Col::Id,
    Col::Dalign,
    Col::Dax,
    Col::Dgran,
    Col::Diskseq,
    Col::Dmax,
    Col::Dzero,
    Col::Fsavail,
    Col::Fsroots,
    Col::Fssize,
    Col::Fstype,
    Col::Fsused,
    Col::Fsuseperc,
    Col::Fsversion,
    Col::Group,
    Col::Hctl,
    Col::Hotplug,
    Col::Kname,
    Col::Label,
    Col::Logsec,
    Col::Majmin,
    Col::Minio,
    Col::Mode,
    Col::Model,
    Col::Mq,
    Col::Name,
    Col::Optio,
    Col::Owner,
    Col::Partflags,
    Col::Partlabel,
    Col::Partn,
    Col::Parttype,
    Col::Parttypename,
    Col::Partuuid,
    Col::Path,
    Col::Physec,
    Col::Pkname,
    Col::Pttype,
    Col::Ptuuid,
    Col::Ra,
    Col::Rand,
    Col::Rev,
    Col::Rm,
    Col::Ro,
    Col::Rota,
    Col::RqSize,
    Col::Sched,
    Col::Serial,
    Col::Size,
    Col::Start,
    Col::State,
    Col::Subsys,
    Col::Target,
    Col::Targets,
    Col::Transport,
    Col::Type,
    Col::Uuid,
    Col::Vendor,
    Col::Wsame,
    Col::Wwn,
    Col::Zoned,
    Col::ZoneSz,
    Col::ZoneWgran,
    Col::ZoneApp,
    Col::ZoneNr,
    Col::ZoneOmax,
    Col::ZoneAmax,
];

/// `columns[]`'s capacity: `ARRAY_SIZE(infos) * 2`.
const MAX_COLUMNS: usize = COLS.len() * 2;

/// `COLTYPE_*`: how a column sorts, and what it is in JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColType {
    /// A string.
    Str,
    /// Always a number.
    Num,
    /// A string on output, a number to sort by.
    SortNum,
    /// A string, or a number with `--bytes`.
    Size,
    /// 0 or 1.
    Bool,
}

/// `struct colinfo`.
struct ColInfo {
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
    ty: ColType,
}

use smartcols::{FL_NOEXTREMES as NOEXT, FL_RIGHT as RIGHT, FL_TRUNC as TRUNC, FL_WRAP as WRAP};

/// `infos[]`.
fn info(col: Col) -> ColInfo {
    let c = |name, whint, flags, help, ty| ColInfo {
        name,
        whint,
        flags,
        help,
        ty,
    };
    use ColType::{Bool, Num, Size, SortNum, Str};
    match col {
        Col::Alioff => c("ALIGNMENT", 6.0, RIGHT, "alignment offset", Num),
        Col::Id => c("ID", 0.1, NOEXT, "udev ID (based on ID-LINK)", Str),
        Col::Idlink => c(
            "ID-LINK",
            0.1,
            NOEXT,
            "the shortest udev /dev/disk/by-id link name",
            Str,
        ),
        Col::Dalign => c("DISC-ALN", 6.0, RIGHT, "discard alignment offset", Num),
        Col::Dax => c("DAX", 1.0, RIGHT, "dax-capable device", Bool),
        Col::Dgran => c("DISC-GRAN", 6.0, RIGHT, "discard granularity", Size),
        Col::Diskseq => c("DISK-SEQ", 1.0, RIGHT, "disk sequence number", Num),
        Col::Dmax => c("DISC-MAX", 6.0, RIGHT, "discard max bytes", Size),
        Col::Dzero => c("DISC-ZERO", 1.0, RIGHT, "discard zeroes data", Bool),
        Col::Fsavail => c("FSAVAIL", 5.0, RIGHT, "filesystem size available", Size),
        Col::Fsroots => c("FSROOTS", 0.1, WRAP, "mounted filesystem roots", Str),
        Col::Fssize => c("FSSIZE", 5.0, RIGHT, "filesystem size", Size),
        Col::Fstype => c("FSTYPE", 0.1, TRUNC, "filesystem type", Str),
        Col::Fsused => c("FSUSED", 5.0, RIGHT, "filesystem size used", Size),
        Col::Fsuseperc => c("FSUSE%", 3.0, RIGHT, "filesystem use percentage", Str),
        Col::Fsversion => c("FSVER", 0.1, TRUNC, "filesystem version", Str),
        Col::Group => c("GROUP", 0.1, TRUNC, "group name", Str),
        Col::Hctl => c("HCTL", 10.0, 0, "Host:Channel:Target:Lun for SCSI", Str),
        Col::Hotplug => c(
            "HOTPLUG",
            1.0,
            RIGHT,
            "removable or hotplug device (usb, pcmcia, ...)",
            Bool,
        ),
        Col::Kname => c("KNAME", 0.3, 0, "internal kernel device name", Str),
        Col::Label => c("LABEL", 0.1, 0, "filesystem LABEL", Str),
        Col::Logsec => c("LOG-SEC", 7.0, RIGHT, "logical sector size", Num),
        Col::Majmin => c("MAJ:MIN", 6.0, 0, "major:minor device number", SortNum),
        Col::Minio => c("MIN-IO", 6.0, RIGHT, "minimum I/O size", Num),
        Col::Model => c("MODEL", 0.1, TRUNC, "device identifier", Str),
        Col::Mode => c("MODE", 10.0, 0, "device node permissions", Str),
        Col::Mq => c("MQ", 3.0, RIGHT, "device queues", Str),
        Col::Name => c("NAME", 0.25, NOEXT, "device name", Str),
        Col::Optio => c("OPT-IO", 6.0, RIGHT, "optimal I/O size", Num),
        Col::Owner => c("OWNER", 0.1, TRUNC, "user name", Str),
        Col::Partflags => c("PARTFLAGS", 36.0, 0, "partition flags", Str),
        Col::Partlabel => c("PARTLABEL", 0.1, 0, "partition LABEL", Str),
        Col::Partn => c(
            "PARTN",
            2.0,
            RIGHT,
            "partition number as read from the partition table",
            Num,
        ),
        Col::Parttypename => c("PARTTYPENAME", 0.1, 0, "partition type name", Str),
        Col::Parttype => c("PARTTYPE", 36.0, 0, "partition type code or UUID", Str),
        Col::Partuuid => c("PARTUUID", 36.0, 0, "partition UUID", Str),
        Col::Path => c("PATH", 0.3, 0, "path to the device node", Str),
        Col::Physec => c("PHY-SEC", 7.0, RIGHT, "physical sector size", Num),
        Col::Pkname => c("PKNAME", 0.3, 0, "internal parent kernel device name", Str),
        Col::Pttype => c("PTTYPE", 0.1, 0, "partition table type", Str),
        Col::Ptuuid => c(
            "PTUUID",
            36.0,
            0,
            "partition table identifier (usually UUID)",
            Str,
        ),
        Col::Rand => c("RAND", 1.0, RIGHT, "adds randomness", Bool),
        Col::Ra => c("RA", 3.0, RIGHT, "read-ahead of the device", Num),
        Col::Rev => c("REV", 4.0, RIGHT, "device revision", Str),
        Col::Rm => c("RM", 1.0, RIGHT, "removable device", Bool),
        Col::Rota => c("ROTA", 1.0, RIGHT, "rotational device", Bool),
        Col::Ro => c("RO", 1.0, RIGHT, "read-only device", Bool),
        Col::RqSize => c("RQ-SIZE", 5.0, RIGHT, "request queue size", Num),
        Col::Sched => c("SCHED", 0.1, 0, "I/O scheduler name", Str),
        Col::Serial => c("SERIAL", 0.1, TRUNC, "disk serial number", Str),
        Col::Size => c("SIZE", 5.0, RIGHT, "size of the device", Size),
        Col::Start => c("START", 5.0, RIGHT, "partition start offset", Num),
        Col::State => c("STATE", 7.0, TRUNC, "state of the device", Str),
        Col::Subsys => c(
            "SUBSYSTEMS",
            0.1,
            NOEXT,
            "de-duplicated chain of subsystems",
            Str,
        ),
        Col::Targets => c(
            "MOUNTPOINTS",
            0.10,
            WRAP | NOEXT,
            "all locations where device is mounted",
            Str,
        ),
        Col::Target => c(
            "MOUNTPOINT",
            0.10,
            TRUNC | NOEXT,
            "where the device is mounted",
            Str,
        ),
        Col::Transport => c("TRAN", 6.0, 0, "device transport type", Str),
        Col::Type => c("TYPE", 4.0, 0, "device type", Str),
        Col::Uuid => c("UUID", 36.0, 0, "filesystem UUID", Str),
        Col::Vendor => c("VENDOR", 0.1, TRUNC, "device vendor", Str),
        Col::Wsame => c("WSAME", 6.0, RIGHT, "write same max bytes", Size),
        Col::Wwn => c("WWN", 18.0, 0, "unique storage identifier", Str),
        Col::Zoned => c("ZONED", 0.3, 0, "zone model", Str),
        Col::ZoneSz => c("ZONE-SZ", 9.0, RIGHT, "zone size", Size),
        Col::ZoneWgran => c("ZONE-WGRAN", 10.0, RIGHT, "zone write granularity", Size),
        Col::ZoneApp => c("ZONE-APP", 11.0, RIGHT, "zone append max bytes", Size),
        Col::ZoneNr => c("ZONE-NR", 8.0, RIGHT, "number of zones", Num),
        Col::ZoneOmax => c(
            "ZONE-OMAX",
            10.0,
            RIGHT,
            "maximum number of open zones",
            Num,
        ),
        Col::ZoneAmax => c(
            "ZONE-AMAX",
            10.0,
            RIGHT,
            "maximum number of active zones",
            Num,
        ),
    }
}

/// A fatal error: what `err`/`errx` printed, and the status to exit with.
#[derive(Debug)]
struct Fatal(u8);

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Bytes shown in a diagnostic: upstream's text, unprintable bytes escaped.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// `warn(msg)` with `errno` as the error.
fn warn_errno(short: &[u8], msg: &str, errno: i32) {
    if errno == 0 {
        warnx(short, &format!("{msg}: Success"));
    } else {
        warn(short, msg, &std::io::Error::from_raw_os_error(errno));
    }
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {} [options] [<device> ...]\n", shown(short)));
    t.push('\n');
    t.push_str("List information about block devices.\n");
    t.push_str("\nOptions:\n");
    for line in [
        " -A, --noempty        don't print empty devices",
        " -D, --discard        print discard capabilities",
        " -E, --dedup <column> de-duplicate output by <column>",
        " -I, --include <list> show only devices with specified major numbers",
        " -J, --json           use JSON output format",
        " -M, --merge          group parents of sub-trees (usable for RAIDs, Multi-path)",
        " -O, --output-all     output all columns",
        " -P, --pairs          use key=\"value\" output format",
        " -S, --scsi           output info about SCSI devices",
        " -N, --nvme           output info about NVMe devices",
        " -v, --virtio         output info about virtio devices",
        " -T, --tree[=<column>] use tree format output",
        " -a, --all            print all devices",
        " -b, --bytes          print SIZE in bytes rather than in human readable format",
        " -d, --nodeps         don't print slaves or holders",
        " -e, --exclude <list> exclude devices by major number (default: RAM disks)",
        " -f, --fs             output info about filesystems",
        " -i, --ascii          use ascii characters only",
        " -l, --list           use list format output",
        " -m, --perms          output info about permissions",
        " -n, --noheadings     don't print headings",
        " -o, --output <list>  output columns",
        " -p, --paths          print complete device path",
        " -r, --raw            use raw output format",
        " -s, --inverse        inverse dependencies",
        " -t, --topology       output info about topology",
        " -w, --width <num>    specifies output width as number of characters",
        " -x, --sort <column>  sort output by <column>",
        " -y, --shell          use column names to be usable as shell variable identifiers",
        " -z, --zoned          print zone related information",
        "     --sysroot <dir>  use specified directory as system root",
    ] {
        t.push_str(line);
        t.push('\n');
    }
    t.push('\n');
    t.push_str(&format!(
        "{:<22}{}\n{:<22}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nAvailable output columns:\n");
    for col in COLS {
        let i = info(col);
        t.push_str(&format!(" {:>12}  {}\n", i.name, i.help));
    }
    t.push_str("\nFor more details see lsblk(8).\n");
    t.into_bytes()
}

/// `column_name_to_id(name, namesz)`: an exact name, in any case. An
/// unknown one is reported with the rest of the list after it, as
/// upstream's C string runs on to the list's end.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = COLS
        .iter()
        .copied()
        .find(|&c| info(c).name.as_bytes().eq_ignore_ascii_case(name));
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// `strtoul(str, &end, 10)`: the value (negated modulo 2^64 after a `-`,
/// `ULONG_MAX` on overflow), where the number ended, and whether it
/// overflowed (`ERANGE`). `None` when no digit was read.
fn strtoul(s: &[u8]) -> Option<(u64, usize, bool)> {
    let sc = ulstrutils::scan_integer(s, 10)?;
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return Some((u64::MAX, sc.end, true));
    }
    let v = u64::try_from(sc.magnitude).unwrap_or(u64::MAX);
    Some((
        if sc.negative { v.wrapping_neg() } else { v },
        sc.end,
        false,
    ))
}

/// `parse_excludes(str0)` and `parse_includes(str0)`: major numbers,
/// separated by commas, onto `list` -- 255 of them at most.
fn parse_majors(str0: &[u8], list: &mut Vec<i32>, what: &str, short: &[u8]) -> Result<(), Fatal> {
    let failed = |range: bool| {
        let msg = format!("failed to parse list '{}'", shown(str0));
        if range {
            warn_errno(short, &msg, ERANGE);
        } else {
            warnx(short, &msg);
        }
        Fatal(1)
    };
    let mut rest = str0;
    while !rest.is_empty() {
        let Some((n, end, overflow)) = strtoul(rest) else {
            return Err(failed(false));
        };
        let after = rest.get(end..).unwrap_or_default();
        if after.first().is_some_and(|&c| c != b',') {
            return Err(failed(false));
        }
        if overflow {
            return Err(failed(true));
        }
        // `excludes[nexcludes++] = n`: the unsigned long cut to an int.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "C stores the unsigned long in an int"
        )]
        list.push(n as i32);
        if list.len() == 256 {
            warnx(
                short,
                &format!("the list of {what} devices is too large (limit is 256 devices)"),
            );
            return Err(Fatal(1));
        }
        rest = after.get(1..).unwrap_or_default();
    }
    Ok(())
}

/// `str2u64(str, &data)`: a whole decimal number into `data`; anything
/// else leaves it.
fn str2u64(s: Option<&[u8]>, data: &mut u64) {
    let Some(s) = s.filter(|s| !s.is_empty()) else {
        return;
    };
    if let Some((v, end, overflow)) = strtoul(s)
        && !overflow
        && end == s.len()
    {
        *data = v;
    }
}

/// `%.0f` of a percentage: rounded to the nearest integer, a half to the
/// even one, as glibc's `printf` rounds the exact binary value.
fn percent_0f(v: f64) -> String {
    const EXACT: f64 = 4_503_599_627_370_496.0; // 2^52
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".to_string()
        } else if v > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    if v.abs() >= EXACT {
        // Already an integer; its decimal expansion is exact.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a double of at least 2^52 is an integer, and fits i128"
        )]
        return format!("{}", v as i128);
    }
    let fl = v.floor();
    // Exact: below 2^52 the fraction is `v`'s own bits, and a half is a
    // half.
    let frac = v - fl;
    #[allow(clippy::float_cmp, reason = "a tie is exactly one half")]
    let tie = frac == 0.5;
    let r = if frac > 0.5 || (tie && fl.rem_euclid(2.0) != 0.0) {
        fl + 1.0
    } else {
        fl
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "below 2^52 in magnitude, so exact in i64"
    )]
    let n = r as i64;
    if n == 0 && v.is_sign_negative() {
        "-0".to_string()
    } else {
        n.to_string()
    }
}

/// `xstrmode(mode, str)`: `ls -l`'s type and permission letters.
fn xstrmode(mode: u32) -> Vec<u8> {
    const S_IFMT: u32 = 0o170_000;
    let mut s = Vec::with_capacity(10);
    match mode & S_IFMT {
        0o040_000 => s.push(b'd'),
        0o120_000 => s.push(b'l'),
        0o020_000 => s.push(b'c'),
        0o060_000 => s.push(b'b'),
        0o140_000 => s.push(b's'),
        0o010_000 => s.push(b'p'),
        0o100_000 => s.push(b'-'),
        _ => {}
    }
    let bit = |m: u32, c: u8| if mode & m != 0 { c } else { b'-' };
    let special =
        |special: u32, exec: u32, set: u8, unset: u8| match (mode & special != 0, mode & exec != 0)
        {
            (true, true) => set,
            (true, false) => unset,
            (false, true) => b'x',
            (false, false) => b'-',
        };
    s.push(bit(0o400, b'r'));
    s.push(bit(0o200, b'w'));
    s.push(special(0o4000, 0o100, b's', b'S'));
    s.push(bit(0o040, b'r'));
    s.push(bit(0o020, b'w'));
    s.push(special(0o2000, 0o010, b's', b'S'));
    s.push(bit(0o004, b'r'));
    s.push(bit(0o002, b'w'));
    s.push(special(0o1000, 0o001, b't', b'T'));
    s
}

/// lsblk's `cmp_u64_cells`: by the numbers kept with the cells, a cell
/// without one first.
fn cmp_u64_cells(a: CellView<'_>, b: CellView<'_>) -> i32 {
    match (a.userdata(), b.userdata()) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(x), Some(y)) => {
            if x == y {
                0
            } else if x >= y {
                1
            } else {
                -1
            }
        }
    }
}

/// Which of a device's properties a column shows.
type PropGet = fn(&props::DevProp) -> Option<&Vec<u8>>;

/// `struct lsblk`, the global handler, with what `main` keeps beside it.
struct Lsblk {
    short: Vec<u8>,
    table: Table,
    /// `sort_col`.
    sort_col: Option<ColumnId>,
    /// `sort_id`, `tree_id`, `dedup_id`: `None` for upstream's -1.
    sort_id: Option<Col>,
    tree_id: Option<Col>,
    dedup_id: Option<Col>,
    sysroot: Option<Vec<u8>>,
    flags: u32,
    all_devices: bool,
    bytes: bool,
    inverse: bool,
    merge: bool,
    nodeps: bool,
    scsi: bool,
    nvme: bool,
    virtio: bool,
    paths: bool,
    sort_hidden: bool,
    dedup_hidden: bool,
    force_tree_order: bool,
    noempty: bool,
    /// `columns[]`: the output columns, by ID.
    columns: Vec<Col>,
    /// `excludes[]` and `includes[]`: major numbers.
    excludes: Vec<i32>,
    includes: Vec<i32>,
    tr: Devtree,
    mnt: Mnt,
}

impl Lsblk {
    fn new(short: Vec<u8>) -> Self {
        Lsblk {
            short,
            table: Table::new(),
            sort_col: None,
            sort_id: None,
            tree_id: Some(Col::Name),
            dedup_id: None,
            sysroot: None,
            flags: LSBLK_TREE,
            all_devices: false,
            bytes: false,
            inverse: false,
            merge: false,
            nodeps: false,
            scsi: false,
            nvme: false,
            virtio: false,
            paths: false,
            sort_hidden: false,
            dedup_hidden: false,
            force_tree_order: false,
            noempty: false,
            columns: Vec::new(),
            excludes: Vec::new(),
            includes: Vec::new(),
            tr: Devtree::default(),
            mnt: Mnt::default(),
        }
    }

    /// `add_column(id)`.
    fn add_column(&mut self, id: Col) -> Result<(), Fatal> {
        if self.columns.len() >= MAX_COLUMNS {
            warnx(
                &self.short,
                &format!(
                    "too many columns specified, the limit is {} columns",
                    MAX_COLUMNS.saturating_sub(1)
                ),
            );
            return Err(Fatal(1));
        }
        self.columns.push(id);
        Ok(())
    }

    /// `add_uniq_column(id)`.
    fn add_uniq_column(&mut self, id: Col) -> Result<(), Fatal> {
        if self.column_id_to_number(id).is_none() {
            self.add_column(id)?;
        }
        Ok(())
    }

    /// `column_id_to_number(id)`.
    fn column_id_to_number(&self, id: Col) -> Option<usize> {
        self.columns.iter().position(|&c| c == id)
    }

    /// `is_maj_excluded(maj)`.
    fn is_maj_excluded(&self, maj: u32) -> bool {
        self.excludes.iter().any(|&e| e as u32 == maj)
    }

    /// `is_maj_included(maj)`: everything, when there is no list.
    fn is_maj_included(&self, maj: u32) -> bool {
        self.includes.is_empty() || self.includes.iter().any(|&e| e as u32 == maj)
    }

    /// `is_parsable(lsblk)`.
    fn is_parsable(&self) -> bool {
        self.table.is_raw() || self.table.is_export() || self.table.is_json()
    }

    fn sysroot(&self) -> Option<&[u8]> {
        self.sysroot.as_deref()
    }

    fn sysfs(&self, id: DevId) -> Option<Rc<PathCxt>> {
        self.tr.dev(id).sysfs.clone()
    }

    /// `ul_path_read_string(dev->sysfs, &str, path)`: the attribute's text,
    /// or nothing.
    fn read_str(&self, id: DevId, path: &[u8]) -> Option<Vec<u8>> {
        self.sysfs(id)?.read_string(path).ok().flatten()
    }

    /// `mk_name(name)`: `/dev/NAME` with `--paths`; a `!` read as `/`.
    fn mk_name(&self, name: &[u8]) -> Vec<u8> {
        let p = if self.paths {
            [&b"/dev/"[..], name].concat()
        } else {
            name.to_vec()
        };
        ulsysfs::devname_sys_to_dev(&p)
    }

    /// `mk_dm_name(name)`: `/dev/mapper/NAME` with `--paths`.
    fn mk_dm_name(&self, name: &[u8]) -> Vec<u8> {
        if self.paths {
            [&b"/dev/mapper/"[..], name].concat()
        } else {
            name.to_vec()
        }
    }

    /// `get_device_path(dev)`: a device-mapper device's `/dev/mapper` name,
    /// any other's `/dev/NAME`.
    fn get_device_path(&self, name: &[u8]) -> Option<Vec<u8>> {
        if name.starts_with(b"dm-") {
            return ulsysfs::canonicalize::canonicalize_dm_name_in(self.sysroot(), name);
        }
        let mut path = b"/dev/".to_vec();
        path.extend_from_slice(name);
        path.truncate(ulsysfs::PATH_MAX.saturating_sub(1));
        Some(ulsysfs::devname_sys_to_dev(&path))
    }

    /// `is_readonly_device(dev)`: the `ro` attribute, or the `BLKROGET`
    /// ioctl when there is none.
    fn is_readonly_device(&self, id: DevId) -> bool {
        if let Some(ro) = self.sysfs(id).and_then(|pc| pc.read_s32(b"ro")) {
            return ro != 0;
        }
        self.tr
            .dev(id)
            .filename
            .as_deref()
            .and_then(sys::blkroget)
            .is_some_and(|ro| ro != 0)
    }

    /// `get_scheduler(dev)`: the name in brackets in `queue/scheduler`.
    fn get_scheduler(&self, id: DevId) -> Option<Vec<u8>> {
        let (n, buf) = self.sysfs(id)?.read_buffer(128, b"queue/scheduler").ok()?;
        if n == 0 {
            return None;
        }
        let open = buf.iter().position(|&b| b == b'[')?;
        let res = buf.get(open.saturating_add(1)..)?;
        let close = res.iter().position(|&b| b == b']')?;
        Some(res.get(..close)?.to_vec())
    }

    /// `get_type(dev)`: `part`, a device-mapper device's owner from its
    /// UUID, `loop`, an md device's level, or the SCSI type -- lower case.
    fn get_type(&self, id: DevId) -> Vec<u8> {
        let dev = self.tr.dev(id);
        if dev.is_partition() {
            return b"part".to_vec();
        }
        let mut res = if dev.name.starts_with(b"dm-") {
            let prefix = self.read_str(id, b"dm/uuid").map(|uuid| {
                let mut p = uuid
                    .split(|&b| b == b'-')
                    .next()
                    .unwrap_or_default()
                    .to_vec();
                // kpartx: the partition number off.
                if p.len() >= 4 && p.get(..4).is_some_and(|h| h.eq_ignore_ascii_case(b"part")) {
                    p.truncate(4);
                }
                p
            });
            prefix.unwrap_or_else(|| b"dm".to_vec())
        } else if dev.name.starts_with(b"loop") {
            b"loop".to_vec()
        } else if dev.name.starts_with(b"md") {
            self.read_str(id, b"md/level")
                .unwrap_or_else(|| b"md".to_vec())
        } else {
            let ty = self
                .sysfs(id)
                .and_then(|pc| pc.read_s32(b"device/type"))
                .and_then(ulsysfs::blkdev::scsi_type_to_name);
            ty.unwrap_or("disk").as_bytes().to_vec()
        };
        res.make_ascii_lowercase();
        res
    }

    /// `get_transport(dev)`: the transport lsscsi would name.
    fn get_transport(&self, id: DevId) -> Option<&'static str> {
        let pc = self.sysfs(id)?;
        let name = &self.tr.dev(id).name;
        if pc.blkdev_scsi_host_is(b"spi") {
            Some("spi")
        } else if pc.blkdev_scsi_host_is(b"fc") {
            let attr = pc.blkdev_scsi_host_attribute(b"fc", b"symbolic_name")?;
            Some(if attr.windows(6).any(|w| w == b" over ") {
                "fcoe"
            } else {
                "fc"
            })
        } else if pc.blkdev_scsi_host_is(b"sas") || pc.blkdev_scsi_has_attribute(b"sas_device") {
            Some("sas")
        } else if pc.blkdev_scsi_has_attribute(b"ieee1394_id") {
            Some("sbp")
        } else if pc.blkdev_scsi_host_is(b"iscsi") {
            Some("iscsi")
        } else if pc.blkdev_scsi_path_contains(b"usb") {
            Some("usb")
        } else if pc.blkdev_scsi_host_is(b"scsi") {
            let attr = pc.blkdev_scsi_host_attribute(b"scsi", b"proc_name")?;
            if attr.starts_with(b"ahci") || attr.starts_with(b"sata") {
                Some("sata")
            } else if attr.windows(3).any(|w| w == b"ata") {
                Some("ata")
            } else {
                None
            }
        } else if name.starts_with(b"nvme") {
            Some("nvme")
        } else if name.starts_with(b"vd") {
            Some("virtio")
        } else if name.starts_with(b"mmcblk") {
            Some("mmc")
        } else {
            None
        }
    }

    /// `get_subsystems(dev)`: the subsystems up the device's chain, joined
    /// by `:`, a repeat of the last one left out.
    fn get_subsystems(&self, id: DevId) -> Option<Vec<u8>> {
        let mut chain = self.sysfs(id)?.blkdev_devchain()?;
        let mut res: Option<Vec<u8>> = None;
        let mut last = 0usize;
        while let Some(sub) = ulsysfs::next_subsystem(&mut chain) {
            let r = res.get_or_insert_with(Vec::new);
            if !r.is_empty() && r.get(last..) == Some(sub.as_slice()) {
                continue;
            }
            if !r.is_empty() {
                r.push(b':');
            }
            last = r.len();
            r.extend_from_slice(&sub);
        }
        res
    }

    /// `device_get_stat(dev)`: `stat` of the node, kept once it gives a
    /// device number.
    fn device_get_stat(&mut self, id: DevId) -> Option<sys::Stat> {
        let dev = self.tr.dev(id);
        if let Some(st) = dev.st
            && st.rdev != 0
        {
            return Some(st);
        }
        let st = sys::stat(dev.filename.as_deref()?)?;
        self.tr.dev_mut(id).st = Some(st);
        Some(st)
    }

    /// `is_removable_device(dev, parent)`: the device's `removable`
    /// attribute -- or its whole disk's, or its sysfs parent's.
    fn is_removable_device(&mut self, id: DevId, parent: Option<DevId>) -> i32 {
        if self.tr.dev(id).removable == -1 {
            let pc = self.sysfs(id);
            let mut removable = pc
                .as_ref()
                .and_then(|pc| pc.read_s32(b"removable"))
                .unwrap_or(0);
            if removable == 0
                && let Some(parent) = parent
                && let Some(ppc) = pc.as_ref().and_then(|pc| pc.blkdev_parent().cloned())
            {
                let parent_sysfs = self.sysfs(parent);
                removable = if parent_sysfs.is_some_and(|p| Rc::ptr_eq(&p, &ppc)) {
                    // A partition, and its parent in the tree is its disk.
                    self.is_removable_device(parent, None)
                } else {
                    ppc.read_s32(b"removable").unwrap_or(0)
                };
            }
            self.tr.dev_mut(id).removable = removable;
        }
        let dev = self.tr.dev_mut(id);
        if dev.removable == -1 {
            dev.removable = 0;
        }
        dev.removable
    }

    /// `device_get_discard_granularity(dev)`.
    fn device_get_discard_granularity(&mut self, id: DevId) -> u64 {
        if self.tr.dev(id).discard_granularity == u64::MAX {
            let v = self
                .sysfs(id)
                .and_then(|pc| pc.read_u64(b"queue/discard_granularity"))
                .unwrap_or(0);
            self.tr.dev_mut(id).discard_granularity = v;
        }
        self.tr.dev(id).discard_granularity
    }

    /// `size_to_human_string(SIZE_SUFFIX_1LETTER, x)`.
    fn human(x: u64) -> Vec<u8> {
        ulstrutils::size_to_human_string(ulstrutils::SIZE_SUFFIX_1LETTER, x).into_bytes()
    }

    /// `device_read_bytes(dev, path, &str, sortdata)`.
    fn device_read_bytes(
        &self,
        id: DevId,
        path: &[u8],
        sortdata: Option<&mut u64>,
    ) -> Option<Vec<u8>> {
        if self.bytes {
            let s = self.read_str(id, path);
            if let Some(sd) = sortdata {
                str2u64(s.as_deref(), sd);
            }
            return s;
        }
        let x = self.sysfs(id)?.read_u64(path)?;
        if let Some(sd) = sortdata {
            *sd = x;
        }
        Some(Self::human(x))
    }

    /// `get_vfs_attribute(dev, id)`: FSSIZE, FSAVAIL, FSUSED or FSUSE% of
    /// the filesystem mounted from the device.
    fn get_vfs_attribute(&mut self, id: DevId, col: Col) -> Option<Vec<u8>> {
        if self.tr.dev(id).fsstat.f_blocks == 0 {
            let mnt =
                self.mnt
                    .get_mountpoint(&mut self.tr, id, self.sysroot.as_deref(), &self.short)?;
            if self.tr.dev(id).is_swap {
                return None;
            }
            let st = sys::stat_vfs(&mnt)?;
            self.tr.dev_mut(id).fsstat = st;
        }
        let st = self.tr.dev(id).fsstat;
        let vfs_attr = match col {
            Col::Fssize => st.f_frsize.wrapping_mul(st.f_blocks),
            Col::Fsavail => st.f_frsize.wrapping_mul(st.f_bavail),
            Col::Fsused => st
                .f_frsize
                .wrapping_mul(st.f_blocks.wrapping_sub(st.f_bfree)),
            Col::Fsuseperc => {
                if st.f_blocks == 0 {
                    return Some(b"-".to_vec());
                }
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "C's conversion of the counts to double"
                )]
                let pct = st.f_blocks.wrapping_sub(st.f_bfree) as f64 / st.f_blocks as f64 * 100.0;
                return Some(format!("{}%", percent_0f(pct)).into_bytes());
            }
            _ => 0,
        };
        Some(if vfs_attr == 0 {
            b"0".to_vec()
        } else if self.bytes {
            vfs_attr.to_string().into_bytes()
        } else {
            Self::human(vfs_attr)
        })
    }

    /// A property of the device's, as `lsblk_device_get_properties` finds
    /// it.
    fn prop(&mut self, id: DevId, get: PropGet) -> Option<Vec<u8>> {
        props::get_properties(&mut self.tr, id, self.sysroot.as_deref())
            .and_then(|p| get(p).cloned())
    }

    /// The filesystems on the device, as `lsblk_device_get_filesystems`
    /// finds them, joined by newlines: each one's target (`[SWAP]` for a
    /// swap area), or -- for FSROOTS -- each one's root, swap areas left
    /// out.
    fn filesystems_joined(&mut self, id: DevId, roots: bool) -> Option<Vec<u8>> {
        let fss = self
            .mnt
            .get_filesystems(&mut self.tr, id, self.sysroot.as_deref(), &self.short);
        let n = fss.len();
        let mut buf: Option<Vec<u8>> = None;
        // `ul_buffer_append_string` appends nothing for NULL or "", and the
        // buffer has no data until something is appended.
        let append = |data: &[u8], buf: &mut Option<Vec<u8>>| {
            if !data.is_empty() {
                buf.get_or_insert_with(Vec::new).extend_from_slice(data);
            }
        };
        for (i, &r) in fss.iter().enumerate() {
            let Some(fs) = self.mnt.fs(r) else {
                continue;
            };
            if roots {
                if fs.is_swaparea() {
                    continue;
                }
                append(fs.root.as_deref().unwrap_or(b"/"), &mut buf);
            } else if fs.is_swaparea() {
                append(b"[SWAP]", &mut buf);
            } else {
                append(fs.target.as_deref().unwrap_or_default(), &mut buf);
            }
            if i.saturating_add(1) < n {
                append(b"\n", &mut buf);
            }
        }
        buf
    }

    /// `device_get_data(dev, parent, id, sortdata)`: the text of the
    /// device's cell in column `col` -- and, for the sort column, the
    /// number it sorts by.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's one switch, kept whole to be read against it"
    )]
    fn device_get_data(
        &mut self,
        id: DevId,
        parent: Option<DevId>,
        col: Col,
        mut sortdata: Option<&mut u64>,
    ) -> Option<Vec<u8>> {
        let sort_str = |s: &Option<Vec<u8>>, sd: &mut Option<&mut u64>| {
            if let Some(sd) = sd.as_deref_mut() {
                str2u64(s.as_deref(), sd);
            }
        };
        match col {
            Col::Name => {
                let dev = self.tr.dev(id);
                Some(match &dev.dm_name {
                    Some(dm) => self.mk_dm_name(dm),
                    None => self.mk_name(&dev.name),
                })
            }
            Col::Kname => Some(self.mk_name(&self.tr.dev(id).name)),
            Col::Pkname => parent.map(|p| self.mk_name(&self.tr.dev(p).name)),
            Col::Path => self.tr.dev(id).filename.clone(),
            Col::Owner | Col::Group | Col::Mode => {
                let prop = if self.sysroot.is_some() {
                    self.prop(
                        id,
                        match col {
                            Col::Owner => |p| p.owner.as_ref(),
                            Col::Group => |p| p.group.as_ref(),
                            _ => |p| p.mode.as_ref(),
                        },
                    )
                } else {
                    None
                };
                if prop.is_some() {
                    return prop;
                }
                let st = self.device_get_stat(id)?;
                match col {
                    Col::Owner => sys::user_name(st.uid),
                    Col::Group => sys::group_name(st.gid),
                    _ => Some(xstrmode(st.mode)),
                }
            }
            Col::Majmin => {
                let dev = self.tr.dev(id);
                let s = if self.is_parsable() {
                    format!("{}:{}", dev.maj, dev.min)
                } else {
                    format!("{:>3}:{:<3}", dev.maj, dev.min)
                };
                if let Some(sd) = sortdata {
                    *sd = ulsysfs::makedev(dev.maj, dev.min);
                }
                Some(s.into_bytes())
            }
            Col::Fstype => self.prop(id, |p| p.fstype.as_ref()),
            Col::Fssize | Col::Fsavail | Col::Fsused | Col::Fsuseperc => {
                self.get_vfs_attribute(id, col)
            }
            Col::Fsversion => self.prop(id, |p| p.fsversion.as_ref()),
            Col::Target => {
                self.mnt
                    .get_mountpoint(&mut self.tr, id, self.sysroot.as_deref(), &self.short)
            }
            Col::Targets => self.filesystems_joined(id, false),
            Col::Fsroots => self.filesystems_joined(id, true),
            Col::Label => self.prop(id, |p| p.label.as_ref()),
            Col::Uuid => self.prop(id, |p| p.uuid.as_ref()),
            Col::Ptuuid => self.prop(id, |p| p.ptuuid.as_ref()),
            Col::Pttype => self.prop(id, |p| p.pttype.as_ref()),
            Col::Parttype => self.prop(id, |p| p.parttype.as_ref()),
            Col::Parttypename => {
                let p = props::get_properties(&mut self.tr, id, self.sysroot.as_deref())?;
                let name =
                    props::parttype_code_to_string(p.parttype.as_ref()?, p.pttype.as_ref()?)?;
                Some(name.as_bytes().to_vec())
            }
            Col::Partlabel => self.prop(id, |p| p.partlabel.as_ref()),
            Col::Partuuid => self.prop(id, |p| p.partuuid.as_ref()),
            Col::Partflags => self.prop(id, |p| p.partflags.as_ref()),
            Col::Partn => self.prop(id, |p| p.partn.as_ref()),
            Col::Wwn => self.prop(id, |p| p.wwn.as_ref()),
            Col::Idlink => self.prop(id, |p| p.idlink.as_ref()),
            Col::Id => {
                let link = self.prop(id, |p| p.idlink.as_ref())?;
                // The bus/subsystem prefix skipped.
                match link.iter().position(|&b| b == b'-') {
                    Some(i) if link.get(i.saturating_add(1)).is_some() => {
                        Some(link.get(i.saturating_add(1)..).unwrap_or_default().to_vec())
                    }
                    _ => Some(link),
                }
            }
            Col::Ra => {
                let s = self.read_str(id, b"queue/read_ahead_kb");
                sort_str(&s, &mut sortdata);
                s
            }
            Col::Ro => Some(
                if self.is_readonly_device(id) {
                    b"1"
                } else {
                    b"0"
                }
                .to_vec(),
            ),
            Col::Rm => Some(
                if self.is_removable_device(id, parent) != 0 {
                    b"1"
                } else {
                    b"0"
                }
                .to_vec(),
            ),
            Col::Hotplug => Some(
                if self.sysfs(id).is_some_and(|pc| pc.blkdev_is_hotpluggable()) {
                    b"1"
                } else {
                    b"0"
                }
                .to_vec(),
            ),
            Col::Rota => self.read_str(id, b"queue/rotational"),
            Col::Rand => self.read_str(id, b"queue/add_random"),
            Col::Model | Col::Serial | Col::Rev => {
                let dev = self.tr.dev(id);
                if dev.is_partition() || dev.nslaves != 0 {
                    return None;
                }
                let (get, attr): (PropGet, &[u8]) = match col {
                    Col::Model => (|p| p.model.as_ref(), b"device/model"),
                    Col::Serial => (|p| p.serial.as_ref(), b"device/serial"),
                    _ => (|p| p.revision.as_ref(), b"device/rev"),
                };
                self.prop(id, get).or_else(|| self.read_str(id, attr))
            }
            Col::Vendor => {
                let dev = self.tr.dev(id);
                if dev.is_partition() || dev.nslaves != 0 {
                    return None;
                }
                self.read_str(id, b"device/vendor")
            }
            Col::Size => {
                let size = self.tr.dev(id).size;
                if let Some(sd) = sortdata {
                    *sd = size;
                }
                Some(if self.bytes {
                    size.to_string().into_bytes()
                } else {
                    Self::human(size)
                })
            }
            Col::Start => {
                let s = self.read_str(id, b"start");
                sort_str(&s, &mut sortdata);
                s
            }
            Col::State => {
                let dev = self.tr.dev(id);
                if !dev.is_partition() && dev.dm_name.is_none() {
                    self.read_str(id, b"device/state")
                } else if dev.dm_name.is_some() {
                    let x = self.sysfs(id)?.read_s32(b"dm/suspended")?;
                    Some(
                        if x != 0 {
                            &b"suspended"[..]
                        } else {
                            b"running"
                        }
                        .to_vec(),
                    )
                } else {
                    None
                }
            }
            Col::Alioff | Col::Minio | Col::Optio | Col::Physec | Col::Logsec | Col::RqSize => {
                let attr: &[u8] = match col {
                    Col::Alioff => b"alignment_offset",
                    Col::Minio => b"queue/minimum_io_size",
                    Col::Optio => b"queue/optimal_io_size",
                    Col::Physec => b"queue/physical_block_size",
                    Col::Logsec => b"queue/logical_block_size",
                    _ => b"queue/nr_requests",
                };
                let s = self.read_str(id, attr);
                sort_str(&s, &mut sortdata);
                s
            }
            Col::Sched => self.get_scheduler(id),
            Col::Type => Some(self.get_type(id)),
            Col::Hctl => {
                let [h, c, t, l] = self.sysfs(id)?.blkdev_scsi_hctl().ok()?;
                Some(format!("{h}:{c}:{t}:{l}").into_bytes())
            }
            Col::Transport => self.get_transport(id).map(|t| t.as_bytes().to_vec()),
            Col::Subsys => self.get_subsystems(id),
            Col::Dalign => {
                let mut s = None;
                if self.device_get_discard_granularity(id) > 0 {
                    s = self.read_str(id, b"discard_alignment");
                }
                let s = Some(s.unwrap_or_else(|| b"0".to_vec()));
                sort_str(&s, &mut sortdata);
                s
            }
            Col::Dgran => {
                if self.bytes {
                    let s = self.read_str(id, b"queue/discard_granularity");
                    sort_str(&s, &mut sortdata);
                    s
                } else {
                    let x = self.device_get_discard_granularity(id);
                    if let Some(sd) = sortdata {
                        *sd = x;
                    }
                    Some(Self::human(x))
                }
            }
            Col::Dmax => self.device_read_bytes(id, b"queue/discard_max_bytes", sortdata),
            Col::Dzero => {
                let mut s = None;
                if self.device_get_discard_granularity(id) > 0 {
                    s = self.read_str(id, b"queue/discard_zeroes_data");
                }
                Some(s.unwrap_or_else(|| b"0".to_vec()))
            }
            Col::Wsame => Some(
                self.device_read_bytes(id, b"queue/write_same_max_bytes", sortdata)
                    .unwrap_or_else(|| b"0".to_vec()),
            ),
            Col::Zoned => self.read_str(id, b"queue/zoned"),
            Col::ZoneSz => {
                let x = self
                    .sysfs(id)?
                    .read_u64(b"queue/chunk_sectors")?
                    .wrapping_shl(9);
                if let Some(sd) = sortdata {
                    *sd = x;
                }
                Some(if self.bytes {
                    x.to_string().into_bytes()
                } else {
                    Self::human(x)
                })
            }
            Col::ZoneWgran => self.device_read_bytes(id, b"queue/zone_write_granularity", sortdata),
            Col::ZoneApp => self.device_read_bytes(id, b"queue/zone_append_max_bytes", sortdata),
            Col::ZoneNr => {
                let s = self.read_str(id, b"queue/nr_zones");
                sort_str(&s, &mut sortdata);
                s
            }
            Col::ZoneOmax | Col::ZoneAmax => {
                let attr: &[u8] = if col == Col::ZoneOmax {
                    b"queue/max_open_zones"
                } else {
                    b"queue/max_active_zones"
                };
                let s = Some(self.read_str(id, attr).unwrap_or_else(|| b"0".to_vec()));
                sort_str(&s, &mut sortdata);
                s
            }
            Col::Dax => self.read_str(id, b"queue/dax"),
            Col::Mq => {
                let queues = self.sysfs(id).map_or(0, |pc| pc.count_dirents(Some(b"mq")));
                Some(if queues == 0 {
                    b"1".to_vec()
                } else {
                    format!("{queues:>3}").into_bytes()
                })
            }
            Col::Diskseq => {
                let s = self.read_str(id, b"diskseq");
                sort_str(&s, &mut sortdata);
                s
            }
        }
    }

    /// `device_to_scols(dev, parent, tab, parent_line)`: the device's line,
    /// under its parent's -- or, with `--merge`, for a device of several
    /// parents, once, when its last parent is reached: its parents grouped,
    /// and the line the group's child -- then its children's.
    fn device_to_scols(&mut self, id: DevId, parent: Option<DevId>, parent_line: Option<LineId>) {
        let parent = parent.or(self.tr.dev(id).wholedisk);
        if self.flags & LSBLK_TREE == 0 && !self.force_tree_order && self.tr.dev(id).is_printed {
            return;
        }
        let mut link_group = false;
        if self.merge && self.tr.parents(id).len() > 1 {
            if !self.tr.is_last_parent(id, parent) {
                return;
            }
            link_group = true;
        }
        let Ok(ln) = self
            .table
            .new_line(if link_group { None } else { parent_line })
        else {
            return;
        };
        self.tr.dev_mut(id).is_printed = true;
        if link_group && let Some(gr) = parent_line {
            for p in self.tr.parents(id) {
                let Some(pln) = self.tr.dev(p).scols_line else {
                    continue;
                };
                // Refused only for a line not the table's, which none is.
                let _ = self.table.group_lines(Some(pln), gr);
            }
            // As above.
            let _ = self.table.line_link_group(ln, gr);
        }
        for (i, col) in self.columns.clone().into_iter().enumerate() {
            let data = if self.sort_id == Some(col) {
                let mut sortdata = u64::MAX;
                let data = self.device_get_data(id, parent, col, Some(&mut sortdata));
                if data.is_some() && sortdata != u64::MAX {
                    // The line and its cell are the table's.
                    let _ = self.table.cell_set_userdata(ln, i, sortdata);
                }
                data
            } else {
                self.device_get_data(id, parent, col, None)
            };
            if let Some(d) = data {
                // As above.
                let _ = self.table.line_refer_data(ln, i, &d);
            }
        }
        self.tr.dev_mut(id).scols_line = Some(ln);
        for child in self.tr.children(id) {
            self.device_to_scols(child, Some(id), Some(ln));
        }
    }

    /// `devtree_to_scols(tr, tab)`: every root's tree into the table.
    fn devtree_to_scols(&mut self) {
        for root in self.tr.roots() {
            self.device_to_scols(root, None, None);
        }
    }

    /// `ignore_empty(dev)`: a device of no size, with `--noempty`, or a
    /// loop device with nothing behind it.
    fn ignore_empty(&self, id: DevId) -> bool {
        let dev = self.tr.dev(id);
        if dev.size != 0 {
            return false;
        }
        if self.noempty {
            return true;
        }
        dev.maj == LOOPDEV_MAJOR
            && !dev
                .filename
                .as_deref()
                .is_some_and(ulsysfs::loopdev::has_backing_file)
    }

    /// `initialize_device(dev, wholedisk, name)`: what sysfs says of the
    /// device; `false` for one to leave out.
    fn initialize_device(&mut self, id: DevId, wholedisk: Option<DevId>, name: &[u8]) -> bool {
        if ulsysfs::devname_is_hidden(self.sysroot(), name) {
            return false;
        }
        self.tr.dev_mut(id).name = name.to_vec();
        if let Some(disk) = wholedisk {
            self.tr.dev_mut(id).wholedisk = Some(disk);
            self.tr.ref_device(disk);
        }
        let Some(filename) = self.get_device_path(name) else {
            return false;
        };
        self.tr.dev_mut(id).filename = Some(filename);
        let disk_name = wholedisk.map(|d| self.tr.dev(d).name.clone());
        let devno = ulsysfs::devname_to_devno_in(self.sysroot(), name, disk_name.as_deref());
        if devno == 0 {
            return false;
        }
        let parent_pc = wholedisk.and_then(|d| self.sysfs(d));
        let Some(pc) = ulsysfs::new_sysfs_path(devno, parent_pc, self.sysroot()) else {
            return false;
        };
        let pc = Rc::new(pc);
        let size = pc.read_u64(b"size").map_or(0, |s| s.wrapping_shl(9));
        {
            let dev = self.tr.dev_mut(id);
            dev.sysfs = Some(pc.clone());
            dev.maj = ulsysfs::major(devno);
            dev.min = ulsysfs::minor(devno);
            dev.size = size;
        }
        if !self.all_devices && self.ignore_empty(id) {
            return false;
        }
        if name.starts_with(b"dm-") {
            let dm = pc.read_string(b"dm/name").ok().flatten();
            if dm.is_none() {
                return false;
            }
            self.tr.dev_mut(id).dm_name = dm;
        }
        {
            let npartitions = pc.blkdev_count_partitions(Some(name));
            let nholders = pc.count_dirents(Some(b"holders"));
            let nslaves = pc.count_dirents(Some(b"slaves"));
            let dev = self.tr.dev_mut(id);
            dev.npartitions = npartitions;
            dev.nholders = nholders;
            dev.nslaves = nslaves;
        }
        if self.scsi && pc.blkdev_scsi_hctl().is_err() {
            return false;
        }
        if self.nvme && self.get_transport(id) != Some("nvme") {
            return false;
        }
        if self.virtio && self.get_transport(id) != Some("virtio") {
            return false;
        }
        true
    }

    /// `devtree_get_device_or_new(tr, disk, name)`: the device of that
    /// name, made (and put in the tree) if it is not there yet.
    fn devtree_get_device_or_new(&mut self, disk: Option<DevId>, name: &[u8]) -> Option<DevId> {
        if let Some(dev) = self.tr.get_device(name) {
            return Some(dev);
        }
        let dev = self.tr.new_device();
        if !self.initialize_device(dev, disk, name) {
            self.tr.unref_device(dev);
            return None;
        }
        self.tr.add_device(dev);
        // Kept referenced by the tree only.
        self.tr.unref_device(dev);
        Some(dev)
    }

    /// `devtree_pktcdvd_get_dep(tr, dev, want_slave)`.
    fn devtree_pktcdvd_get_dep(&mut self, id: DevId, want_slave: bool) -> Option<DevId> {
        let dev = self.tr.dev(id);
        let devno = self
            .tr
            .pktcdvd_get_mate(ulsysfs::makedev(dev.maj, dev.min), !want_slave);
        if devno == 0 {
            return None;
        }
        let name = ulsysfs::devno_to_devname(devno, ulsysfs::PATH_MAX)?;
        self.devtree_get_device_or_new(None, &name)
    }

    /// `process_partitions(tr, disk)`: the disk's partitions into the tree,
    /// each with what depends on it.
    fn process_partitions(&mut self, disk: DevId) -> Result<(), Fatal> {
        let d = self.tr.dev(disk);
        if d.npartitions == 0 || d.is_partition() {
            return Ok(());
        }
        let Some(pc) = self.sysfs(disk) else {
            return Ok(());
        };
        let (dir, entries) = match pc.opendir_at(None) {
            Ok(v) => v,
            Err(e) => {
                warn_errno(&self.short, "failed to open device directory in sysfs", e);
                return Err(Fatal(1));
            }
        };
        let disk_name = self.tr.dev(disk).name.clone();
        for d in &entries {
            if !ulsysfs::is_partition_dirent(&dir, d, Some(&disk_name)) {
                continue;
            }
            let Some(part) = self.devtree_get_device_or_new(Some(disk), &d.name) else {
                continue;
            };
            if self.tr.new_dependence(disk, part) {
                self.process_dependencies(part, false)?;
            }
        }
        Ok(())
    }

    /// `process_dependencies(tr, dev, do_partitions)`: what depends on the
    /// device (what it depends on, with `--inverse`) into the tree, and on,
    /// recursively; and its partitions, if asked.
    fn process_dependencies(&mut self, id: DevId, do_partitions: bool) -> Result<(), Fatal> {
        if self.nodeps {
            return Ok(());
        }
        if do_partitions && self.tr.dev(id).npartitions != 0 {
            self.process_partitions(id)?;
        }
        let dev = self.tr.dev(id);
        let count = if self.inverse {
            dev.nslaves
        } else {
            dev.nholders
        };
        let depname: &[u8] = if self.inverse { b"slaves" } else { b"holders" };
        if count != 0
            && let Some(pc) = self.sysfs(id)
            && let Ok((dir, entries)) = pc.opendir_at(Some(depname))
        {
            for d in &entries {
                if ulsysfs::is_partition_dirent(&dir, d, None) {
                    let diskname = get_wholedisk_from_partition_dirent(&dir, &d.name);
                    let Some(disk) =
                        diskname.and_then(|n| self.devtree_get_device_or_new(None, &n))
                    else {
                        continue;
                    };
                    let Some(dep) = self.devtree_get_device_or_new(Some(disk), &d.name) else {
                        continue;
                    };
                    if self.tr.new_dependence(id, dep) {
                        self.process_dependencies(dep, true)?;
                    }
                    if self.inverse && self.tr.new_dependence(dep, disk) {
                        self.process_dependencies(disk, false)?;
                    }
                } else {
                    let Some(dep) = self.devtree_get_device_or_new(None, &d.name) else {
                        continue;
                    };
                    if self.tr.new_dependence(id, dep) {
                        // For an inverse tree, no partitions of a whole
                        // disk that is the dependence.
                        self.process_dependencies(dep, !self.inverse)?;
                    }
                }
            }
        }
        if let Some(dep) = self.devtree_pktcdvd_get_dep(id, self.inverse)
            && self.tr.new_dependence(id, dep)
        {
            self.tr.remove_root(dep);
            self.process_dependencies(dep, !self.inverse)?;
        }
        Ok(())
    }

    /// `__process_one_device(tr, devname, devno)`: the device (named, or
    /// by number) as a root, and what depends on it. 0, or `-EINVAL` for a
    /// device that could not be listed.
    fn process_one_device(&mut self, devname: Option<&[u8]>, devno: u64) -> Result<i32, Fatal> {
        let devno = match devname {
            Some(path) if devno == 0 => match sys::stat(path) {
                Some(st) if st.mode & 0o170_000 == 0o060_000 => st.rdev,
                _ => {
                    warnx(&self.short, &format!("{}: not a block device", shown(path)));
                    return Ok(-EINVAL);
                }
            },
            _ => devno,
        };
        let name = match devno_to_devname(devno) {
            Ok(n) => n,
            Err(e) => {
                if let Some(path) = devname {
                    warn_errno(
                        &self.short,
                        &format!("{}: failed to get sysfs name", shown(path)),
                        e,
                    );
                }
                return Ok(-EINVAL);
            }
        };
        let mut real_part = None;
        if !name.starts_with(b"dm-") {
            match ulsysfs::devno_to_wholedisk(devno, ulsysfs::PATH_MAX.saturating_add(1)) {
                Some((diskname, diskno)) => {
                    if devno != diskno {
                        real_part = Some(diskname);
                    }
                }
                None => {
                    warn_errno(
                        &self.short,
                        &format!("{}: failed to get whole-disk device number", shown(&name)),
                        ENOENT,
                    );
                    return Ok(-EINVAL);
                }
            }
        }
        match real_part {
            None => {
                let Some(dev) = self.devtree_get_device_or_new(None, &name) else {
                    return Ok(-EINVAL);
                };
                self.tr.add_root(dev);
                self.process_dependencies(dev, !self.inverse)?;
            }
            Some(diskname) => {
                let Some(disk) = self.devtree_get_device_or_new(None, &diskname) else {
                    return Ok(-EINVAL);
                };
                let Some(dev) = self.devtree_get_device_or_new(Some(disk), &name) else {
                    return Ok(-EINVAL);
                };
                self.tr.add_root(dev);
                self.process_dependencies(dev, true)?;
                if self.inverse && self.tr.new_dependence(dev, disk) {
                    self.process_dependencies(disk, false)?;
                }
            }
        }
        Ok(0)
    }

    /// `process_all_devices_inverse(tr)`: each device in `/sys/dev/block`
    /// that nothing holds and that has no partitions, as a root.
    fn process_all_devices_inverse(&mut self) -> Result<(), Fatal> {
        let mut pc = PathCxt::new(Some(ulsysfs::PATH_SYS_DEVBLOCK));
        pc.set_prefix(self.sysroot());
        let Ok(entries) = pc.opendir(None) else {
            return Ok(());
        };
        for d in &entries {
            let Some((maj, min)) = scan_majmin(&d.name) else {
                continue;
            };
            #[allow(
                clippy::cast_sign_loss,
                reason = "makedev takes the ints as unsigned, as C converts them"
            )]
            let devno = ulsysfs::makedev(maj as u32, min as u32);
            #[allow(clippy::cast_sign_loss, reason = "as above")]
            let umaj = maj as u32;
            if self.is_maj_excluded(umaj) || !self.is_maj_included(umaj) {
                continue;
            }
            let holders = [d.name.as_slice(), b"/holders"].concat();
            if pc.count_dirents(Some(&holders)) != 0 {
                continue;
            }
            if ulsysfs::devno_count_partitions(devno) != 0 {
                continue;
            }
            self.process_one_device(None, devno)?;
        }
        Ok(())
    }

    /// `process_all_devices(tr)`: each device in `/sys/block` that is not
    /// built on another, as a root, and what depends on it.
    fn process_all_devices(&mut self) -> Result<(), Fatal> {
        let mut pc = PathCxt::new(Some(ulsysfs::PATH_SYS_BLOCK));
        pc.set_prefix(self.sysroot());
        let Ok(entries) = pc.opendir(None) else {
            return Ok(());
        };
        for d in &entries {
            let Some(dev) = self.devtree_get_device_or_new(None, &d.name) else {
                continue;
            };
            let maj = self.tr.dev(dev).maj;
            if self.is_maj_excluded(maj) || !self.is_maj_included(maj) {
                self.tr.remove_device(dev);
                continue;
            }
            if self.tr.dev(dev).nslaves != 0 {
                continue;
            }
            self.tr.add_root(dev);
            self.process_dependencies(dev, true)?;
        }
        Ok(())
    }

    /// `device_set_dedupkey(dev, parent, id)`: the key of the device and of
    /// everything under it.
    fn device_set_dedupkey(&mut self, id: DevId, parent: Option<DevId>, col: Col) {
        let key = self.device_get_data(id, parent, col, None);
        self.tr.dev_mut(id).dedupkey = key;
        for child in self.tr.children(id) {
            self.device_set_dedupkey(child, Some(id), col);
        }
    }

    /// `devtree_set_dedupkeys(tr, id)`.
    fn devtree_set_dedupkeys(&mut self, col: Col) {
        for root in self.tr.roots() {
            self.device_set_dedupkey(root, None, col);
        }
    }
}

/// `sysfs_devno_to_devname(devno, buf, sizeof(buf))`, with the `errno` a
/// failure leaves for `warn`.
fn devno_to_devname(devno: u64) -> Result<Vec<u8>, i32> {
    let mut pc = PathCxt::new(None);
    pc.blkdev_init(devno, None)?;
    pc.blkdev_name(ulsysfs::PATH_MAX.saturating_add(1))
        .ok_or(EINVAL)
}

/// `get_wholedisk_from_partition_dirent(dir, d, buf, bufsz)`: the name of
/// the directory holding the partition the link `d` leads to.
fn get_wholedisk_from_partition_dirent(dir: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    let path = [dir, b"/", name].concat();
    let link = std::fs::read_link(quoting::os_from_bytes(&path)).ok()?;
    let mut buf = os_bytes(link.as_os_str()).into_owned();
    buf.truncate(ulsysfs::PATH_MAX.saturating_sub(1));
    // `.../<disk>/<partition>`: the last component off, then the one
    // before it.
    let slash = buf.iter().rposition(|&b| b == b'/')?;
    buf.truncate(slash);
    let slash = buf.iter().rposition(|&b| b == b'/')?;
    Some(buf.get(slash.saturating_add(1)..)?.to_vec())
}

/// `sscanf(name, "%d:%d")`.
fn scan_majmin(text: &[u8]) -> Option<(i32, i32)> {
    let int = |t: &[u8]| -> Option<(i32, usize)> {
        let sc = ulstrutils::scan_integer(t, 10)?;
        let max = u128::from(i64::MAX.unsigned_abs());
        let v: i64 = if sc.saturated || sc.magnitude > max.saturating_add(u128::from(sc.negative)) {
            if sc.negative { i64::MIN } else { i64::MAX }
        } else {
            let m = i128::try_from(sc.magnitude).unwrap_or(i128::MAX);
            let v = if sc.negative { m.saturating_neg() } else { m };
            i64::try_from(v).unwrap_or(i64::MAX)
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "C stores the long in an int"
        )]
        Some((v as i32, sc.end))
    };
    let (maj, end) = int(text)?;
    let rest = text.get(end..)?;
    let rest = rest.strip_prefix(b":")?;
    let (min, _) = int(rest)?;
    Some((maj, min))
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(i32, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((i32::from(*c), value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// `option_to_longopt(c, opts)`: the first long option with this `val`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("lsblk"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = match run(&argv, &short, &mut out) {
        Ok(s) | Err(Fatal(s)) => s,
    };
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> Result<u8, Fatal> {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("lsblk"), OsString::as_os_str);
    let mut ls = Lsblk::new(short.to_vec());
    let mut outarg: Option<Vec<u8>> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut width: u32 = 0;
    let mut force_tree = false;
    let mut excl_st = [0i32; EXCL.len()];

    let own = argv.get(1..).unwrap_or_default();
    for item in LSBLK.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return Err(Fatal(errtryhelp(short)));
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(os_bytes(o).into_owned());
            }
            continue;
        };
        if let Some(msg) =
            ulstrutils::err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, short)
        {
            stderr_write(msg.as_bytes());
            return Err(Fatal(1));
        }
        let arg = value.as_deref().map(|v| os_bytes(v).into_owned());
        if c == OPT_SYSROOT {
            ls.sysroot = arg;
            continue;
        }
        match u8::try_from(c).unwrap_or(0) {
            b'A' => ls.noempty = true,
            b'a' => ls.all_devices = true,
            b'b' => ls.bytes = true,
            b'd' => ls.nodeps = true,
            b'D' => {
                for col in [Col::Name, Col::Dalign, Col::Dgran, Col::Dmax, Col::Dzero] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'z' => {
                for col in [
                    Col::Name,
                    Col::Zoned,
                    Col::ZoneSz,
                    Col::ZoneNr,
                    Col::ZoneAmax,
                    Col::ZoneOmax,
                    Col::ZoneApp,
                    Col::ZoneWgran,
                ] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'e' => parse_majors(
                &arg.unwrap_or_default(),
                &mut ls.excludes,
                "excluded",
                short,
            )?,
            b'J' => ls.flags |= LSBLK_JSON,
            b'l' => ls.flags &= !LSBLK_TREE,
            b'M' => ls.merge = true,
            b'n' => ls.flags |= LSBLK_NOHEADINGS,
            b'o' => outarg = arg,
            b'O' => ls.columns = COLS.to_vec(),
            b'p' => ls.paths = true,
            b'P' => {
                ls.flags |= LSBLK_EXPORT;
                ls.flags &= !LSBLK_TREE;
            }
            b'y' => ls.flags |= LSBLK_SHELLVAR,
            b'i' => ls.flags |= LSBLK_ASCII,
            b'I' => parse_majors(
                &arg.unwrap_or_default(),
                &mut ls.includes,
                "included",
                short,
            )?,
            b'r' => {
                ls.flags &= !LSBLK_TREE;
                ls.flags |= LSBLK_RAW;
            }
            b's' => ls.inverse = true,
            b'f' => {
                for col in [
                    Col::Name,
                    Col::Fstype,
                    Col::Fsversion,
                    Col::Label,
                    Col::Uuid,
                    Col::Fsavail,
                    Col::Fsuseperc,
                    Col::Targets,
                ] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'm' => {
                for col in [Col::Name, Col::Size, Col::Owner, Col::Group, Col::Mode] {
                    ls.add_uniq_column(col)?;
                }
            }
            b't' => {
                for col in [
                    Col::Name,
                    Col::Alioff,
                    Col::Minio,
                    Col::Optio,
                    Col::Physec,
                    Col::Logsec,
                    Col::Rota,
                    Col::Sched,
                    Col::RqSize,
                    Col::Ra,
                    Col::Wsame,
                ] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'S' => {
                ls.nodeps = true;
                ls.scsi = true;
                for col in [
                    Col::Name,
                    Col::Hctl,
                    Col::Type,
                    Col::Vendor,
                    Col::Model,
                    Col::Rev,
                    Col::Serial,
                    Col::Transport,
                ] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'N' => {
                ls.nodeps = true;
                ls.nvme = true;
                for col in [
                    Col::Name,
                    Col::Type,
                    Col::Model,
                    Col::Serial,
                    Col::Rev,
                    Col::Transport,
                    Col::RqSize,
                    Col::Mq,
                ] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'v' => {
                ls.nodeps = true;
                ls.virtio = true;
                for col in [
                    Col::Name,
                    Col::Type,
                    Col::Transport,
                    Col::Size,
                    Col::RqSize,
                    Col::Mq,
                ] {
                    ls.add_uniq_column(col)?;
                }
            }
            b'T' => {
                force_tree = true;
                if let Some(a) = arg {
                    let name = a.strip_prefix(b"=").unwrap_or(&a);
                    ls.tree_id = column_name_to_id(name, name, short);
                }
            }
            b'E' => {
                let a = arg.unwrap_or_default();
                ls.dedup_id = column_name_to_id(&a, &a, short);
                if ls.dedup_id.is_none() {
                    return Err(Fatal(errtryhelp(short)));
                }
            }
            b'w' => {
                let a = arg.unwrap_or_default();
                let parsed = ulstrutils::ul_strtou64(&a, 10)
                    .and_then(|v| u32::try_from(v).map_err(|_| ulstrutils::NumErr::Range));
                match parsed {
                    Ok(v) => width = v,
                    Err(e) => {
                        warnx(
                            short,
                            &ulstrutils::num_error_message(
                                "invalid output width number argument",
                                &quoting::os_from_bytes(&a),
                                e,
                            ),
                        );
                        return Err(Fatal(1));
                    }
                }
            }
            b'x' => {
                ls.flags &= !LSBLK_TREE;
                let a = arg.unwrap_or_default();
                ls.sort_id = column_name_to_id(&a, &a, short);
                if ls.sort_id.is_none() {
                    return Err(Fatal(errtryhelp(short)));
                }
            }
            b'h' => {
                out.write(&usage(short));
                return Ok(0);
            }
            b'V' => {
                out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
                return Ok(0);
            }
            _ => return Err(Fatal(errtryhelp(short))),
        }
    }

    if force_tree {
        ls.flags |= LSBLK_TREE;
    }

    // `check_sysdevblock()`: the running system's, --sysroot or not.
    if let Err(e) = sys::access_r(ulsysfs::PATH_SYS_DEVBLOCK) {
        warn_errno(short, "failed to access sysfs directory: /sys/dev/block", e);
        return Err(Fatal(1));
    }

    if ls.columns.is_empty() {
        for col in [
            Col::Name,
            Col::Majmin,
            Col::Rm,
            Col::Size,
            Col::Ro,
            Col::Type,
            Col::Targets,
        ] {
            ls.add_column(col)?;
        }
    }
    if let Some(list) = &outarg
        && ulstrutils::string_add_to_idarray(list, &mut ls.columns, MAX_COLUMNS, |name, rest| {
            column_name_to_id(name, rest, short)
        })
        .is_err()
    {
        return Err(Fatal(1));
    }

    // By default, no RAM disks.
    if !ls.all_devices && ls.excludes.is_empty() && ls.includes.is_empty() {
        ls.excludes.push(1);
    }
    // Since Linux 4.8 /sys is no longer sorted: MAJ:MIN is the default.
    if ls.sort_id.is_none() {
        ls.sort_id = Some(Col::Majmin);
    }
    // --inverse, --raw and --pairs in a list still follow parent->child.
    if ls.flags & LSBLK_TREE == 0
        && (ls.inverse || ls.flags & LSBLK_EXPORT != 0 || ls.flags & LSBLK_RAW != 0)
    {
        ls.force_tree_order = true;
    }
    if let Some(id) = ls.sort_id
        && ls.column_id_to_number(id).is_none()
    {
        // Not among the output columns: added, hidden.
        ls.add_column(id)?;
        ls.sort_hidden = true;
    }
    if let Some(id) = ls.dedup_id
        && ls.column_id_to_number(id).is_none()
    {
        ls.add_column(id)?;
        ls.dedup_hidden = true;
    }

    let tb = &mut ls.table;
    tb.enable_raw(ls.flags & LSBLK_RAW != 0);
    tb.enable_export(ls.flags & LSBLK_EXPORT != 0);
    tb.enable_shellvar(ls.flags & LSBLK_SHELLVAR != 0);
    tb.enable_ascii(ls.flags & LSBLK_ASCII != 0);
    tb.enable_json(ls.flags & LSBLK_JSON != 0);
    tb.enable_noheadings(ls.flags & LSBLK_NOHEADINGS != 0);
    if ls.flags & LSBLK_JSON != 0 {
        tb.set_name(b"blockdevices");
    }
    if width != 0 {
        tb.set_termwidth(usize::try_from(width).unwrap_or(usize::MAX));
        tb.set_termforce(smartcols::TermForce::Always);
    }

    let mut has_tree_col = false;
    let ncolumns = ls.columns.len();
    for (i, &id) in ls.columns.iter().enumerate() {
        let ci = info(id);
        let mut fl = ci.flags;
        if ls.flags & LSBLK_TREE != 0 && !has_tree_col && Some(id) == ls.tree_id {
            fl |= smartcols::FL_TREE;
            fl &= !smartcols::FL_RIGHT;
            has_tree_col = true;
        }
        if ls.sort_hidden && ls.sort_id == Some(id) {
            fl |= smartcols::FL_HIDDEN;
        }
        if ls.dedup_hidden && ls.dedup_id == Some(id) {
            fl |= smartcols::FL_HIDDEN;
        }
        if force_tree
            && ls.flags & LSBLK_JSON != 0
            && !has_tree_col
            && i.saturating_add(1) == ncolumns
        {
            // --tree --json with no tree column: the last one draws it.
            fl |= smartcols::FL_TREE;
        }
        let cl = ls.table.new_column(ci.name.as_bytes(), ci.whint, fl);
        // Each call below refuses only a column not the table's, and `cl`
        // was made by it a line ago.
        if ls.sort_col.is_none() && ls.sort_id == Some(id) {
            ls.sort_col = Some(cl);
            let f: smartcols::CmpFunc = match ci.ty {
                ColType::Num | ColType::Size | ColType::SortNum => cmp_u64_cells,
                ColType::Str | ColType::Bool => smartcols::cmpstr_cells,
            };
            let _ = ls.table.column_set_cmpfunc(cl, f);
        }
        if fl & smartcols::FL_WRAP != 0 {
            // Multi-line cells (MOUNTPOINTS): one line per newline.
            let _ = ls.table.column_set_wrapnl(cl);
            let _ = ls.table.column_set_safechars(cl, b"\n");
        }
        if ls.flags & LSBLK_JSON != 0 {
            let ty = match ci.ty {
                ColType::Size if !ls.bytes => None,
                ColType::Size | ColType::Num => Some(JsonType::Number),
                ColType::Bool => Some(JsonType::Boolean),
                ColType::Str | ColType::SortNum => Some(if fl & smartcols::FL_WRAP != 0 {
                    JsonType::ArrayString
                } else {
                    JsonType::String
                }),
            };
            if let Some(ty) = ty {
                let _ = ls.table.column_set_json_type(cl, ty);
            }
        }
    }

    let status = if operands.is_empty() {
        let rc = if ls.inverse {
            ls.process_all_devices_inverse()
        } else {
            ls.process_all_devices()
        };
        rc?;
        0
    } else {
        let (mut cnt, mut cnt_err) = (0usize, 0usize);
        for dev in &operands {
            if ls.process_one_device(Some(dev), 0)? != 0 {
                cnt_err = cnt_err.saturating_add(1);
            }
            cnt = cnt.saturating_add(1);
        }
        if cnt == 0 {
            1
        } else if cnt == cnt_err {
            EXIT_ALLFAILED
        } else if cnt_err > 0 {
            EXIT_SOMEOK
        } else {
            0
        }
    };

    if let Some(id) = ls.dedup_id {
        ls.devtree_set_dedupkeys(id);
        ls.tr.deduplicate_devices();
    }

    ls.devtree_to_scols();

    if let Some(cl) = ls.sort_col {
        // Refused only without a comparison function, which it was given.
        let _ = ls.table.sort(Some(cl));
    }
    if ls.force_tree_order {
        ls.table.sort_by_tree();
    }

    // `scols_print_table`'s status is not looked at upstream: what it
    // printed before any failure is written.
    let mut text = Vec::new();
    let _ = ls.table.print_into(&mut text);
    out.write(&text);
    Ok(status)
}
