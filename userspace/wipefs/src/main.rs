//! wipefs -- wipe signatures from a device.
//!
//! A port of util-linux 2.39.3's `misc-utils/wipefs.c`, function by
//! function and with upstream's names, on top of `ulblkid` (the port of
//! libblkid) and `smartcols` (of libsmartcols): every signature libblkid
//! knows is found by stepping `blkid_do_probe` over the device, hiding each
//! one found so the next can be seen, and erased with `blkid_do_wipe`.
//! Measured against `wipefs from util-linux 2.39.3` by
//! `scripts/wipefs-diff.sh`.
//!
//! This replaces a hand-written program that knew fifteen signature types.
//!
//! Upstream's quirks are kept where they show:
//!
//! * An `-o` offset another device already matched is not reported "not
//!   found" for a later device: the mark is kept across devices.
//! * `--backup` writes over an existing backup file without truncating it.
//! * A nested partition table on a partition is left alone (and reported)
//!   unless `--force`.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{ColumnId, JsonType, Table};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::process::ExitCode;
use std::rc::Rc;
use ulblkid::{
    PARTS_FORCE_GPT, PARTS_MAGIC, Probe, SUBLKS_BADCSUM, SUBLKS_LABEL, SUBLKS_MAGIC, SUBLKS_TYPE,
    SUBLKS_USAGE, SUBLKS_UUID,
};
use ulclosestream::{Stdout, stderr_write, warn, warnx};

/// Getopt's errors are only sentences here; the referral follows them.
const WIPEFS: Program = Program::new("wipefs", 1);

/// Upstream's option string.
const SHORTS: &str = "abfhiJnO:o:pqt:V";
/// `OPT_LOCK`: `CHAR_MAX + 1`.
const OPT_LOCK: i32 = 128;

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("backup", Takes::Nothing),
    ("force", Takes::Nothing),
    ("help", Takes::Nothing),
    ("lock", Takes::Optional),
    ("no-act", Takes::Nothing),
    ("offset", Takes::Required),
    ("parsable", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("types", Takes::Required),
    ("version", Takes::Nothing),
    ("json", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("output", Takes::Required),
];
const LONG_VALS: [i32; 14] = [
    b'a' as i32,
    b'b' as i32,
    b'f' as i32,
    b'h' as i32,
    OPT_LOCK,
    b'n' as i32,
    b'o' as i32,
    b'p' as i32,
    b'q' as i32,
    b't' as i32,
    b'V' as i32,
    b'J' as i32,
    b'i' as i32,
    b'O' as i32,
];

/// `excl[]`: `-O`, `-a` and `-o` exclude each other.
const EXCL: [&[i32]; 1] = [&[b'O' as i32, b'a' as i32, b'o' as i32]];

/// `EBUSY`.
#[cfg(unix)]
const EBUSY: i32 = 16;

/// The output columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Uuid,
    Label,
    Len,
    Type,
    Offset,
    Usage,
    Device,
}

/// `struct colinfo`.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    help: &'static str,
}

/// `infos[]`.
const INFOS: [ColInfo; 7] = [
    ColInfo {
        id: Col::Uuid,
        name: "UUID",
        whint: 4.0,
        help: "partition/filesystem UUID",
    },
    ColInfo {
        id: Col::Label,
        name: "LABEL",
        whint: 5.0,
        help: "filesystem LABEL",
    },
    ColInfo {
        id: Col::Len,
        name: "LENGTH",
        whint: 6.0,
        help: "magic string length",
    },
    ColInfo {
        id: Col::Type,
        name: "TYPE",
        whint: 4.0,
        help: "superblok type",
    },
    ColInfo {
        id: Col::Offset,
        name: "OFFSET",
        whint: 5.0,
        help: "magic string offset",
    },
    ColInfo {
        id: Col::Usage,
        name: "USAGE",
        whint: 5.0,
        help: "type description",
    },
    ColInfo {
        id: Col::Device,
        name: "DEVICE",
        whint: 5.0,
        help: "block device name",
    },
];

/// `columns[]`'s capacity: `ARRAY_SIZE(infos) * 2`.
const MAX_COLUMNS: usize = 14;

/// `struct wipe_desc`: one signature.
#[derive(Clone, Debug, Default)]
struct WipeDesc {
    /// `offset`: of the magic string.
    offset: i64,
    /// `len`: of the magic string.
    len: usize,
    /// `magic`.
    magic: Vec<u8>,
    /// `usage`: raid, filesystem...
    usage: Option<Vec<u8>>,
    /// `type`.
    ty: Option<Vec<u8>>,
    /// `label`.
    label: Option<Vec<u8>>,
    /// `uuid`.
    uuid: Option<Vec<u8>>,
    /// `on_disk`.
    on_disk: bool,
    /// `is_parttable`.
    is_parttable: bool,
}

/// `struct wipe_control`.
#[derive(Default)]
struct Ctl {
    devname: Vec<u8>,
    type_pattern: Option<Vec<u8>>,
    lockmode: Option<Vec<u8>>,
    /// `offsets`: `-o` offsets, in the order first given.
    offsets: Vec<WipeDesc>,
    /// `ndevs`: devices still to probe.
    ndevs: usize,
    /// `reread`: devices whose partition table is re-read at the end.
    reread: Vec<Vec<u8>>,
    noact: bool,
    all: bool,
    quiet: bool,
    backup: bool,
    force: bool,
    json: bool,
    no_headings: bool,
    parsable: bool,
}

/// A fatal error: what `err`/`errx` printed, and the status to exit with.
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

/// POSIX `basename(path)` (`libgen.h`'s): trailing slashes off, then the
/// last component; `.` for an empty path, `/` for slashes alone.
fn posix_basename(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }
    let end = path
        .iter()
        .rposition(|&b| b != b'/')
        .map(|i| i.saturating_add(1));
    let Some(end) = end else {
        return b"/".to_vec();
    };
    let trimmed = path.get(..end).unwrap_or_default();
    let start = trimmed
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    trimmed.get(start..).unwrap_or_default().to_vec()
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {} [options] <device>\n", shown(short)));
    t.push('\n');
    t.push_str("Wipe signatures from a device.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -a, --all           wipe all magic strings (BE CAREFUL!)\n");
    t.push_str(" -b, --backup        create a signature backup in $HOME\n");
    t.push_str(" -f, --force         force erasure\n");
    t.push_str(" -i, --noheadings    don't print headings\n");
    t.push_str(" -J, --json          use JSON output format\n");
    t.push_str(" -n, --no-act        do everything except the actual write() call\n");
    t.push_str(" -o, --offset <num>  offset to erase, in bytes\n");
    t.push_str(" -O, --output <list> COLUMNS to display (see below)\n");
    t.push_str(" -p, --parsable      print out in parsable instead of printable format\n");
    t.push_str(" -q, --quiet         suppress output messages\n");
    t.push_str(" -t, --types <list>  limit the set of filesystem, RAIDs or partition tables\n");
    t.push_str("     --lock[=<mode>] use exclusive device lock (yes, no or nonblock)\n");
    t.push_str(&format!(
        "{:<21}{}\n{:<21}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nArguments:\n");
    t.push_str(" <num> arguments may be followed by the suffixes for\n   GiB, TiB, PiB, EiB, ZiB, and YiB (the \"iB\" is optional)\n");
    t.push_str("\nAvailable output columns:\n");
    for i in &INFOS {
        t.push_str(&format!(" {:>8}  {}\n", i.name, i.help));
    }
    t.push_str("\nFor more details see wipefs(8).\n");
    t.into_bytes()
}

/// `column_name_to_id(name, namesz)`: an exact name, in any case. Unknown,
/// it is reported with the rest of the list after it, as upstream's C
/// string runs on to the list's end.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = INFOS
        .iter()
        .find(|i| i.name.as_bytes().eq_ignore_ascii_case(name))
        .map(|i| i.id);
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// The column's `struct colinfo`.
fn info(col: Col) -> &'static ColInfo {
    INFOS.iter().find(|i| i.id == col).unwrap_or(&INFOS[0])
}

/// `init_output(ctl)`.
fn init_output(ctl: &Ctl, columns: &[Col]) -> (Table, Vec<(Col, ColumnId)>) {
    let mut tb = Table::new();
    if ctl.json {
        tb.enable_json(true);
        tb.set_name(b"signatures");
    }
    tb.enable_noheadings(ctl.no_headings);
    if ctl.parsable {
        tb.enable_raw(true);
        tb.set_column_separator(b",");
    }
    let mut cols = Vec::with_capacity(columns.len());
    for &col in columns {
        let i = info(col);
        let id = tb.new_column(i.name.as_bytes(), i.whint, 0);
        if ctl.json && col == Col::Len {
            // The column was made by this table a line ago.
            let _ = tb.column_set_json_type(id, JsonType::Number);
        }
        cols.push((col, id));
    }
    (tb, cols)
}

/// `fill_table_row(ctl, wp)`.
fn fill_table_row(ctl: &Ctl, tb: &mut Table, cols: &[(Col, ColumnId)], wp: &WipeDesc) {
    let Ok(ln) = tb.new_line(None) else {
        return;
    };
    for &(col, id) in cols {
        let data: Option<Vec<u8>> = match col {
            Col::Uuid => wp.uuid.clone(),
            Col::Label => wp.label.clone(),
            Col::Offset => Some(format!("0x{:x}", wp.offset.cast_unsigned()).into_bytes()),
            Col::Len => Some(wp.len.to_string().into_bytes()),
            Col::Usage => wp.usage.clone(),
            Col::Type => wp.ty.clone(),
            Col::Device => Some(posix_basename(&ctl.devname)),
        };
        if let Some(d) = data {
            // The line and the column were both made by this table.
            let _ = tb.line_set_data(ln, id, &d);
        }
    }
}

/// `add_offset(&wp0, offset)`: the entry for the offset, made at the end
/// of the list if there is none.
fn add_offset(wp0: &mut Vec<WipeDesc>, offset: i64) -> usize {
    if let Some(i) = wp0.iter().position(|w| w.offset == offset) {
        return i;
    }
    wp0.push(WipeDesc {
        offset,
        ..WipeDesc::default()
    });
    wp0.len().saturating_sub(1)
}

/// A value, as a C string.
fn value(pr: &Probe, name: &str) -> Option<Vec<u8>> {
    pr.lookup_value(name).map(|v| v.as_c_str().to_vec())
}

/// `get_desc_for_probe(ctl, &wp0, pr, &offset, &len)`: the signature the
/// probe is on, if it passes `-t` and `-o` -- added to `wp0` when given
/// (merged with an entry already at its offset), else returned alone. The
/// offset and length are returned whenever libblkid found something, so a
/// signature filtered out can still be hidden.
fn get_desc_for_probe(
    ctl: &mut Ctl,
    wp0: Option<&mut Vec<WipeDesc>>,
    pr: &Probe,
) -> (Option<WipeDesc>, i64, usize) {
    let mut len = 0usize;
    let (off, ty, mag, mut usage, ispt) = if let Some(ty) = value(pr, "TYPE") {
        let (Some(off), Some(mag)) = (
            value(pr, "SBMAGIC_OFFSET"),
            pr.lookup_value("SBMAGIC").map(|v| v.data().to_vec()),
        ) else {
            if let Some(m) = pr.lookup_value("SBMAGIC")
                && pr.lookup_value("SBMAGIC_OFFSET").is_some()
            {
                len = m.len();
            }
            return (None, 0, len);
        };
        (off, ty, mag, None, false)
    } else if let Some(ty) = value(pr, "PTTYPE") {
        let (Some(off), Some(mag)) = (
            value(pr, "PTMAGIC_OFFSET"),
            pr.lookup_value("PTMAGIC").map(|v| v.data().to_vec()),
        ) else {
            return (None, 0, len);
        };
        (off, ty, mag, Some(b"partition-table".to_vec()), true)
    } else {
        return (None, 0, len);
    };
    len = mag.len();
    // `strtoll(off, NULL, 10)`, `errno` checked.
    let Some(offset) = strtoll(&off) else {
        return (None, 0, len);
    };
    // `-t`.
    if let Some(p) = &ctl.type_pattern
        && !ulstrutils::match_fstype(Some(&ty), Some(p))
    {
        return (None, offset, len);
    }
    // `-o`.
    if !ctl.offsets.is_empty() {
        let Some(w) = ctl.offsets.iter_mut().find(|w| w.offset == offset) else {
            return (None, offset, len);
        };
        w.on_disk = true;
    }
    if usage.is_none() {
        usage = value(pr, "USAGE");
    }
    let desc = WipeDesc {
        offset,
        len,
        magic: mag,
        usage,
        ty: Some(ty),
        label: value(pr, "LABEL"),
        uuid: value(pr, "UUID"),
        on_disk: true,
        is_parttable: ispt,
    };
    match wp0 {
        Some(list) => {
            let i = add_offset(list, offset);
            if let Some(w) = list.get_mut(i) {
                *w = desc.clone();
            }
            (Some(desc), offset, len)
        }
        None => (Some(desc), offset, len),
    }
}

/// `strtoll(s, NULL, 10)` whose caller then checks `errno`: `None` for
/// `ERANGE`. (No digits at all converts to 0 without an error.)
fn strtoll(s: &[u8]) -> Option<i64> {
    let Some(sc) = ulstrutils::scan_integer(s, 10) else {
        return Some(0);
    };
    let limit = if sc.negative {
        1u128 << 63
    } else {
        (1u128 << 63) - 1
    };
    if sc.saturated || sc.magnitude > limit {
        return None;
    }
    let m = i128::try_from(sc.magnitude).ok()?;
    i64::try_from(if sc.negative { m.wrapping_neg() } else { m }).ok()
}

/// `new_probe(devname, mode)`: a probe on the device -- opened read-only by
/// libblkid, or as `mode` asks (read-write, and exclusive unless forced) --
/// looking for every superblock (bad checksums accepted) and partition
/// table (GPT without a protective MBR too). Failing is fatal.
fn new_probe(devname: &[u8], rw_excl: Option<bool>, short: &[u8]) -> Result<Probe, Fatal> {
    let fail = |errno: i32| {
        warn_errno(
            short,
            &format!("error: {}: probing initialization failed", shown(devname)),
            errno,
        );
        Fatal(1)
    };
    let mut pr = match rw_excl {
        None => Probe::from_filename(devname).map_err(fail)?,
        Some(excl) => {
            let file = open_rw(devname, excl).map_err(fail)?;
            let mut pr = Probe::new();
            if pr.set_device(Some(Rc::new(file)), 0, 0) != 0 {
                return Err(fail(pr.errno));
            }
            pr
        }
    };
    pr.enable_superblocks(true);
    pr.set_superblocks_flags(
        SUBLKS_MAGIC | SUBLKS_TYPE | SUBLKS_USAGE | SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_BADCSUM,
    );
    pr.enable_partitions(true);
    pr.set_partitions_flags(PARTS_MAGIC | PARTS_FORCE_GPT);
    Ok(pr)
}

/// `open(devname, O_RDWR | [O_EXCL] | O_NONBLOCK)`.
fn open_rw(devname: &[u8], excl: bool) -> Result<File, i32> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NONBLOCK, and O_EXCL: Linux's values, which SlateOS's C library
        // shares.
        opts.custom_flags(0o4000 | if excl { 0o200 } else { 0 });
    }
    #[cfg(not(unix))]
    {
        let _ = excl;
    }
    opts.open(quoting::os_from_bytes(devname))
        .map_err(|e| ulblkid::errno_of(&e))
}

/// `read_offsets(ctl)`: every signature on the device.
fn read_offsets(ctl: &mut Ctl, short: &[u8]) -> Result<Vec<WipeDesc>, Fatal> {
    let devname = ctl.devname.clone();
    let mut pr = new_probe(&devname, None, short)?;
    let mut wp0 = Vec::new();
    while pr.do_probe() == 0 {
        let (_, offset, len) = get_desc_for_probe(ctl, Some(&mut wp0), &pr);
        // Hide it, and look again.
        if len != 0 {
            pr.hide_range(offset.cast_unsigned(), u64::try_from(len).unwrap_or(0));
            pr.step_back();
        }
    }
    Ok(wp0)
}

/// `do_wipe_real(ctl, pr, w)`: erase the signature the probe is on, and
/// say so.
fn do_wipe_real(
    ctl: &Ctl,
    pr: &mut Probe,
    w: &WipeDesc,
    out: &mut Stdout,
    short: &[u8],
) -> Result<(), Fatal> {
    let ty = w.ty.clone().unwrap_or_default();
    if pr.do_wipe(ctl.noact) != 0 {
        warn_errno(
            short,
            &format!(
                "{}: failed to erase {} magic string at offset 0x{:08x}",
                shown(&ctl.devname),
                shown(&ty),
                w.offset.cast_unsigned()
            ),
            pr.errno,
        );
        return Err(Fatal(1));
    }
    if ctl.quiet {
        return Ok(());
    }
    let (noun, verb) = if w.len == 1 {
        ("byte", "was")
    } else {
        ("bytes", "were")
    };
    let mut line = Vec::new();
    line.extend_from_slice(&ctl.devname);
    line.extend_from_slice(
        format!(
            ": {} {noun} {verb} erased at offset 0x{:08x} (",
            w.len,
            w.offset.cast_unsigned()
        )
        .as_bytes(),
    );
    line.extend_from_slice(&ty);
    line.extend_from_slice(b"): ");
    let hex: Vec<String> = w.magic.iter().map(|b| format!("{b:02x}")).collect();
    line.extend_from_slice(hex.join(" ").as_bytes());
    line.push(b'\n');
    out.write(&line);
    Ok(())
}

/// `do_backup(wp, base)`: the magic string, into `BASE0xOFFSET.bak`.
fn do_backup(wp: &WipeDesc, base: &[u8], short: &[u8]) -> Result<(), Fatal> {
    let mut fname = base.to_vec();
    fname.extend_from_slice(format!("0x{:08x}.bak", wp.offset.cast_unsigned()).as_bytes());
    let mut opts = std::fs::OpenOptions::new();
    // O_CREAT | O_WRONLY, no O_TRUNC.
    opts.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = opts
        .open(quoting::os_from_bytes(&fname))
        .and_then(|mut f| std::io::Write::write_all(&mut f, &wp.magic));
    if let Err(e) = result {
        warn(
            short,
            &format!("{}: failed to create a signature backup", shown(&fname)),
            &e,
        );
        return Err(Fatal(1));
    }
    Ok(())
}

/// `ioctl(fd, BLKRRPART)`: the kernel re-reads the partition table.
#[cfg(unix)]
fn blkrrpart(file: &File) -> i32 {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    /// `BLKRRPART`: `_IO(0x12, 95)`.
    const BLKRRPART: u64 = 0x125f;
    // SAFETY: BLKRRPART takes no argument; the descriptor is open.
    let rc = unsafe { ioctl(file.as_raw_fd(), BLKRRPART) };
    if rc < 0 {
        ulblkid::errno_of(&std::io::Error::last_os_error())
    } else {
        0
    }
}

/// `rereadpt(fd, devname)`: ask the kernel to re-read a block device's
/// partition table, retrying while it is busy, and say how it went.
fn rereadpt(file: &File, devname: &[u8], out: &mut Stdout) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if !file
            .metadata()
            .is_ok_and(|m| m.file_type().is_block_device())
        {
            return;
        }
        let mut errno;
        let mut attempt = 0u32;
        loop {
            // The first re-read without a delay usually fails: the kernel or
            // udevd is still busy with the device.
            std::thread::sleep(std::time::Duration::from_millis(250));
            errno = blkrrpart(file);
            if errno != EBUSY {
                break;
            }
            let again = attempt < 4;
            attempt = attempt.saturating_add(1);
            if !again {
                break;
            }
        }
        let mut line = Vec::new();
        line.extend_from_slice(devname);
        line.extend_from_slice(b": calling ioctl to re-read partition table: ");
        line.extend_from_slice(
            errmsg::strerror(&std::io::Error::from_raw_os_error(errno)).as_bytes(),
        );
        line.push(b'\n');
        out.write(&line);
    }
    #[cfg(not(unix))]
    {
        let _ = (file, devname, out);
    }
}

/// `flock(fd, operation)`.
#[cfg(unix)]
fn flock(file: &File, op: i32) -> Result<(), i32> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    // SAFETY: flock only takes the descriptor, which is open.
    let rc = unsafe { flock(file.as_raw_fd(), op) };
    if rc == 0 {
        Ok(())
    } else {
        Err(ulblkid::errno_of(&std::io::Error::last_os_error()))
    }
}

/// `blkdev_lock(fd, devname, lockmode)` from `lib/blkdev.c`: an exclusive
/// `flock` on the device, as `--lock` (or `$LOCK_BLOCK_DEVICE`) asks --
/// waiting, with a message, when another holds it. 0, or not 0 when the
/// device must be left alone. (The only one of util-linux's ports to need
/// it so far; it moves to a shared crate with the second.)
fn blkdev_lock(file: &File, devname: &[u8], lockmode: Option<&[u8]>, short: &[u8]) -> i32 {
    let env = std::env::var_os("LOCK_BLOCK_DEVICE").map(|v| os_bytes(&v).into_owned());
    let Some(mode) = lockmode.map(<[u8]>::to_vec).or(env) else {
        return 0;
    };
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    let oper = if mode.eq_ignore_ascii_case(b"yes") || mode == b"1" {
        LOCK_EX
    } else if mode.eq_ignore_ascii_case(b"nonblock") {
        LOCK_EX | LOCK_NB
    } else if mode.eq_ignore_ascii_case(b"no") || mode == b"0" {
        return 0;
    } else {
        warnx(short, &format!("unsupported lock mode: {}", shown(&mode)));
        return -22;
    };
    #[cfg(unix)]
    {
        /// `EWOULDBLOCK`.
        const EWOULDBLOCK: i32 = 11;
        let mut msg = false;
        if oper & LOCK_NB == 0 {
            // Without blocking first, to have a message to print.
            match flock(file, oper | LOCK_NB) {
                Ok(()) => return 0,
                Err(EWOULDBLOCK) => {
                    stderr_write(
                        format!(
                            "{}: {}: device already locked, waiting to get lock ... ",
                            shown(short),
                            shown(devname)
                        )
                        .as_bytes(),
                    );
                    msg = true;
                }
                Err(_) => {}
            }
        }
        match flock(file, oper) {
            Ok(()) => {
                if msg {
                    stderr_write(b"OK\n");
                }
                0
            }
            Err(EWOULDBLOCK) => {
                warnx(short, &format!("{}: device already locked", shown(devname)));
                -1
            }
            Err(e) => {
                warn(
                    short,
                    &format!("{}: failed to get lock", shown(devname)),
                    &std::io::Error::from_raw_os_error(e),
                );
                -1
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (file, devname, oper);
        0
    }
}

/// `close(fd)`, its failure reported as upstream reports it.
#[cfg_attr(
    not(unix),
    allow(clippy::unnecessary_wraps, reason = "the unix half can fail; this half cannot")
)]
fn close_checked(file: File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::IntoRawFd;
        unsafe extern "C" {
            fn close(fd: i32) -> i32;
        }
        let fd = file.into_raw_fd();
        // SAFETY: `fd` was just released by the File that owned it, so this
        // is its only close.
        if unsafe { close(fd) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        drop(file);
        Ok(())
    }
}

/// `do_wipe(ctl)`: erase the device's signatures, as `-a`, `-o` and `-t`
/// select them.
fn do_wipe(ctl: &mut Ctl, out: &mut Stdout, short: &[u8]) -> Result<(), Fatal> {
    let devname = ctl.devname.clone();
    let excl = !ctl.force;
    let mut pr = new_probe(&devname, Some(excl), short)?;
    let Some(file) = pr.file().cloned() else {
        return Ok(());
    };
    if blkdev_lock(&file, &devname, ctl.lockmode.as_deref(), short) != 0 {
        return Ok(());
    }
    let mut backup: Option<Vec<u8>> = None;
    if ctl.backup {
        let Some(home) = std::env::var_os("HOME") else {
            warnx(
                short,
                "failed to create a signature backup, $HOME undefined",
            );
            return Err(Fatal(1));
        };
        let mut b = os_bytes(&home).into_owned();
        b.extend_from_slice(b"/wipefs-");
        b.extend_from_slice(&posix_basename(&devname));
        b.push(b'-');
        backup = Some(b);
    }
    let mut reread = false;
    let mut need_force = false;
    while pr.do_probe() == 0 {
        let mut wiped = false;
        let (wp, offset, len) = get_desc_for_probe(ctl, None, &pr);
        if let Some(wp) = wp {
            if !ctl.force && wp.is_parttable && !pr.is_wholedisk() {
                warnx(
                    short,
                    &format!(
                        "{}: ignoring nested \"{}\" partition table on non-whole disk device",
                        shown(&devname),
                        shown(wp.ty.as_deref().unwrap_or_default())
                    ),
                );
                need_force = true;
            } else {
                if let Some(base) = &backup {
                    do_backup(&wp, base, short)?;
                }
                do_wipe_real(ctl, &mut pr, &wp, out, short)?;
                if wp.is_parttable {
                    reread = true;
                }
                wiped = true;
            }
        }
        if !wiped && len != 0 {
            // Not wiped (-t or -o filtered it out): hide it, so that libblkid
            // tries this superblock's other magic strings rather than going
            // on to the next superblock.
            pr.hide_range(offset.cast_unsigned(), u64::try_from(len).unwrap_or(0));
            pr.step_back();
        }
    }
    for w in &ctl.offsets {
        if !w.on_disk && !ctl.quiet {
            warnx(
                short,
                &format!(
                    "{}: offset 0x{:x} not found",
                    shown(&devname),
                    w.offset.cast_unsigned()
                ),
            );
        }
    }
    if need_force {
        warnx(short, "Use the --force option to force erase.");
    }
    if let Err(e) = file.sync_all() {
        warn(
            short,
            &format!("{}: cannot flush modified buffers", shown(&devname)),
            &e,
        );
        return Err(Fatal(1));
    }
    if reread && excl {
        if ctl.ndevs > 1 {
            // More devices to go: re-read once everything is erased, so a
            // disk's table is not re-read before its partitions are done.
            ctl.reread.push(devname.clone());
        } else {
            rereadpt(&file, &devname, out);
        }
    }
    drop(pr);
    if let Ok(f) = Rc::try_unwrap(file)
        && let Err(e) = close_checked(f)
    {
        warn(
            short,
            &format!("{}: close device failed", shown(&devname)),
            &e,
        );
        return Err(Fatal(1));
    }
    Ok(())
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
            .map_or(OsStr::new("wipefs"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv
        .first()
        .map_or(OsStr::new("wipefs"), OsString::as_os_str);
    let mut ctl = Ctl::default();
    let mut outarg: Option<Vec<u8>> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut excl_st = [0i32; 1];

    let own = argv.get(1..).unwrap_or_default();
    for item in WIPEFS.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
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
            return 1;
        }
        let arg = value.as_deref().map(|v| os_bytes(v).into_owned());
        if c == OPT_LOCK {
            ctl.lockmode = Some(match arg {
                // `--lock==mode` is `--lock=mode`.
                Some(a) => a.strip_prefix(b"=").map_or(a.clone(), <[u8]>::to_vec),
                None => b"1".to_vec(),
            });
            continue;
        }
        match u8::try_from(c).unwrap_or(0) {
            b'a' => ctl.all = true,
            b'b' => ctl.backup = true,
            b'f' => ctl.force = true,
            b'J' => ctl.json = true,
            b'i' => ctl.no_headings = true,
            b'O' => outarg = arg,
            b'n' => ctl.noact = true,
            b'o' => {
                let a = arg.unwrap_or_default();
                match ulstrutils::parse_size(&a) {
                    // A uintmax_t kept in a loff_t, as C converts it.
                    Ok(v) => {
                        add_offset(&mut ctl.offsets, v.cast_signed());
                    }
                    Err(e) => {
                        warnx(
                            short,
                            &ulstrutils::size_error_message(
                                "invalid offset argument",
                                &value.clone().unwrap_or_default(),
                                e,
                            ),
                        );
                        return 1;
                    }
                }
            }
            b'p' => {
                ctl.parsable = true;
                ctl.no_headings = true;
            }
            b'q' => ctl.quiet = true,
            b't' => ctl.type_pattern = arg,
            b'h' => {
                out.write(&usage(short));
                return 0;
            }
            b'V' => {
                out.write(format!("{} from util-linux 2.39.3\n", shown(short)).as_bytes());
                return 0;
            }
            _ => return errtryhelp(short),
        }
    }

    if operands.is_empty() {
        warnx(short, "no device specified");
        return errtryhelp(short);
    }
    if ctl.backup && !ctl.all && ctl.offsets.is_empty() {
        warnx(short, "The --backup option is meaningless in this context");
    }

    if !ctl.all && ctl.offsets.is_empty() {
        // Print only.
        let mut columns: Vec<Col> = if ctl.parsable {
            // Kept backward compatible.
            vec![Col::Offset, Col::Uuid, Col::Label, Col::Type]
        } else {
            // The default, which -O may extend.
            vec![Col::Device, Col::Offset, Col::Type, Col::Uuid, Col::Label]
        };
        if let Some(list) = &outarg {
            let added =
                ulstrutils::string_add_to_idarray(list, &mut columns, MAX_COLUMNS, |name, rest| {
                    column_name_to_id(name, rest, short)
                });
            if added.is_err() {
                return 1;
            }
        }
        let (mut tb, cols) = init_output(&ctl, &columns);
        for dev in &operands {
            ctl.devname = dev.clone();
            let wp = match read_offsets(&mut ctl, short) {
                Ok(w) => w,
                Err(Fatal(rc)) => return rc,
            };
            for w in &wp {
                fill_table_row(&ctl, &mut tb, &cols, w);
            }
        }
        // `scols_print_table`'s status is not looked at, as upstream does
        // not look at it: what it printed before any failure is written.
        let mut text = Vec::new();
        let _ = tb.print_into(&mut text);
        out.write(&text);
    } else {
        // Erase.
        ctl.ndevs = operands.len();
        for dev in &operands {
            ctl.devname = dev.clone();
            if let Err(Fatal(rc)) = do_wipe(&mut ctl, out, short) {
                return rc;
            }
            ctl.ndevs = ctl.ndevs.saturating_sub(1);
        }
        // The postponed re-reads, now that everything is erased.
        for devname in std::mem::take(&mut ctl.reread) {
            if let Ok(f) = File::open(quoting::os_from_bytes(&devname)) {
                rereadpt(&f, &devname, out);
            }
        }
    }
    0
}

#[cfg(test)]
mod tests;
