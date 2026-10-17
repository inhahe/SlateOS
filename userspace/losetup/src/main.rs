//! losetup -- set up and control loop devices.
//!
//! A port of util-linux 2.39.3's `sys-utils/losetup.c`, function by function
//! and with upstream's names, on `ulsysfs::loopdev` (`lib/loopdev.c`) and
//! printing its table through `smartcols` (the libsmartcols port); measured
//! against Ubuntu's `losetup` by `scripts/losetup-diff.sh`.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **What a loop device is known by.** The list comes from sysfs:
//!   `/sys/block`'s `loopN` entries that have a `loop/backing_file`, in the
//!   order the directory lists them -- not by number. Offsets, flags and the
//!   sector size are sysfs's too; only the backing file's device and inode
//!   (`BACK-MAJ:MIN`, `BACK-INO`, and `--all`'s `[dev]:ino`) need the
//!   `LOOP_GET_STATUS64` ioctl, and so the device opened -- which a user
//!   outside the `disk` group cannot, and then gets `[]`.
//! * **`-f` without `/dev/loop-control`'s permission** still finds a device:
//!   the first of `loop0`..`loop7` sysfs says is free, then the rest of
//!   `/dev`.
//! * **A failed command names the `errno` the failure left**, not the one it
//!   returned: `losetup -d /dev/loop0` as a user is "detach failed:
//!   Permission denied" (the open), though the call returned `EINVAL`.
//! * **`-d A B C` goes on after a name that is no loop device**, with the
//!   previous device still the current one -- which it then tries to detach
//!   again.
//! * **`-O` with `-D` is not refused**: `excl[]`'s third row is not in ASCII
//!   order (`O` sorts before `c`), and the scan stops before reaching it.
//!
//! # What is not upstream's
//!
//! * **A device or file name in a diagnostic** has its unprintable bytes
//!   escaped (design-decisions §370).

stdfdguard::guard_std_fds!();

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{FL_NOEXTREMES, FL_RIGHT, JsonType, Table};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{err_exclusive_options, string_add_to_idarray};
use ulsysfs::loopdev::{
    LO_CRYPT_NONE, LO_FLAGS_AUTOCLEAR, LO_FLAGS_DIRECT_IO, LO_FLAGS_PARTSCAN,
    LO_FLAGS_READ_ONLY, LOOPDEV_FL_OFFSET, LOOPDEV_FL_SIZELIMIT, LOOPDEV_MAJOR, LOOPITER_FL_USED,
    LoopCxt, dev_ino, is_loopdev,
};

/// `ENOENT` and `EBUSY`, `EAGAIN`.
const ENOENT: i32 = 2;
const EBUSY: i32 = 16;
const EAGAIN: i32 = 11;

/// `_PATH_DEV_LOOPCTL`.
const PATH_DEV_LOOPCTL: &std::ffi::CStr = c"/dev/loop-control";

/// The actions, `A_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    None,
    Create,
    Delete,
    DeleteAll,
    Show,
    ShowOne,
    FindFree,
    SetCapacity,
    SetDirectIo,
    SetBlocksize,
}

/// The columns, `COL_*`, in `infos[]`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Name,
    Autoclr,
    BackFile,
    BackIno,
    BackMajmin,
    Majmin,
    Offset,
    Partscan,
    Ro,
    Sizelimit,
    Dio,
    Logsec,
}

/// `struct colinfo`.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
    json_type: JsonType,
}

/// `infos[]`, in `enum` order.
const INFOS: [ColInfo; 12] = [
    ColInfo {
        id: Col::Name,
        name: "NAME",
        whint: 0.25,
        flags: 0,
        help: "loop device name",
        json_type: JsonType::String,
    },
    ColInfo {
        id: Col::Autoclr,
        name: "AUTOCLEAR",
        whint: 1.0,
        flags: FL_RIGHT,
        help: "autoclear flag set",
        json_type: JsonType::Boolean,
    },
    ColInfo {
        id: Col::BackFile,
        name: "BACK-FILE",
        whint: 0.3,
        flags: FL_NOEXTREMES,
        help: "device backing file",
        json_type: JsonType::String,
    },
    ColInfo {
        id: Col::BackIno,
        name: "BACK-INO",
        whint: 4.0,
        flags: FL_RIGHT,
        help: "backing file inode number",
        json_type: JsonType::Number,
    },
    ColInfo {
        id: Col::BackMajmin,
        name: "BACK-MAJ:MIN",
        whint: 6.0,
        flags: 0,
        help: "backing file major:minor device number",
        json_type: JsonType::String,
    },
    ColInfo {
        id: Col::Majmin,
        name: "MAJ:MIN",
        whint: 3.0,
        flags: 0,
        help: "loop device major:minor number",
        json_type: JsonType::String,
    },
    ColInfo {
        id: Col::Offset,
        name: "OFFSET",
        whint: 5.0,
        flags: FL_RIGHT,
        help: "offset from the beginning",
        json_type: JsonType::Number,
    },
    ColInfo {
        id: Col::Partscan,
        name: "PARTSCAN",
        whint: 1.0,
        flags: FL_RIGHT,
        help: "partscan flag set",
        json_type: JsonType::Boolean,
    },
    ColInfo {
        id: Col::Ro,
        name: "RO",
        whint: 1.0,
        flags: FL_RIGHT,
        help: "read-only device",
        json_type: JsonType::Boolean,
    },
    ColInfo {
        id: Col::Sizelimit,
        name: "SIZELIMIT",
        whint: 5.0,
        flags: FL_RIGHT,
        help: "size limit of the file in bytes",
        json_type: JsonType::Number,
    },
    ColInfo {
        id: Col::Dio,
        name: "DIO",
        whint: 1.0,
        flags: FL_RIGHT,
        help: "access backing file with direct-io",
        json_type: JsonType::Boolean,
    },
    ColInfo {
        id: Col::Logsec,
        name: "LOG-SEC",
        whint: 4.0,
        flags: FL_RIGHT,
        help: "logical sector size in bytes",
        json_type: JsonType::Number,
    },
];

/// `columns[ARRAY_SIZE(infos) * 2]`.
const MAX_COLUMNS: usize = 24;

/// A column's `infos[]` entry: the table is in `enum` order (a test holds
/// it to that).
fn info(col: Col) -> &'static ColInfo {
    match col {
        Col::Name => &INFOS[0],
        Col::Autoclr => &INFOS[1],
        Col::BackFile => &INFOS[2],
        Col::BackIno => &INFOS[3],
        Col::BackMajmin => &INFOS[4],
        Col::Majmin => &INFOS[5],
        Col::Offset => &INFOS[6],
        Col::Partscan => &INFOS[7],
        Col::Ro => &INFOS[8],
        Col::Sizelimit => &INFOS[9],
        Col::Dio => &INFOS[10],
        Col::Logsec => &INFOS[11],
    }
}

/// Getopt's errors are only sentences here; the referral follows them.
const LOSETUP: Program = Program::new("losetup", 1);

/// Upstream's option string.
const SHORTS: &str = "ab:c:d:Dfhj:JlLno:O:PrvV";

/// `OPT_SIZELIMIT` and the rest: `CHAR_MAX + 1` on.
const OPT_SIZELIMIT: i32 = 128;
const OPT_SHOW: i32 = 129;
const OPT_RAW: i32 = 130;
const OPT_DIO: i32 = 131;
const OPT_OUTPUT_ALL: i32 = 132;

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("set-capacity", Takes::Required),
    ("detach", Takes::Required),
    ("detach-all", Takes::Nothing),
    ("find", Takes::Nothing),
    ("nooverlap", Takes::Nothing),
    ("help", Takes::Nothing),
    ("associated", Takes::Required),
    ("json", Takes::Nothing),
    ("list", Takes::Nothing),
    ("sector-size", Takes::Required),
    ("noheadings", Takes::Nothing),
    ("offset", Takes::Required),
    ("output", Takes::Required),
    ("output-all", Takes::Nothing),
    ("sizelimit", Takes::Required),
    ("partscan", Takes::Nothing),
    ("read-only", Takes::Nothing),
    ("direct-io", Takes::Optional),
    ("raw", Takes::Nothing),
    ("show", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
];
const LONG_VALS: [i32; 23] = [
    b'a' as i32,
    b'c' as i32,
    b'd' as i32,
    b'D' as i32,
    b'f' as i32,
    b'L' as i32,
    b'h' as i32,
    b'j' as i32,
    b'J' as i32,
    b'l' as i32,
    b'b' as i32,
    b'n' as i32,
    b'o' as i32,
    b'O' as i32,
    OPT_OUTPUT_ALL,
    OPT_SIZELIMIT,
    b'P' as i32,
    b'r' as i32,
    OPT_DIO,
    OPT_RAW,
    OPT_SHOW,
    b'v' as i32,
    b'V' as i32,
];

/// `excl[]`, as upstream has it -- its third row out of ASCII order, which
/// is what keeps `-O` and `-D` from being refused together.
const EXCL: [&[i32]; 4] = [
    &[
        b'D' as i32,
        b'a' as i32,
        b'c' as i32,
        b'd' as i32,
        b'f' as i32,
        b'j' as i32,
    ],
    &[b'D' as i32, b'c' as i32, b'd' as i32, b'f' as i32, b'l' as i32],
    &[b'D' as i32, b'c' as i32, b'd' as i32, b'f' as i32, b'O' as i32],
    &[b'J' as i32, OPT_RAW],
];

/// `option_to_longopt (c, longopts)`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
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

/// `column_name_to_id (name, namesz)`: a column by its whole name, in any
/// case. Unknown, it is reported with the rest of the list after it, as
/// upstream's C string runs on to the list's end.
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

/// `usage ()`'s text.
fn usage(short: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(short);
    let mut text = format!(
        "\nUsage:\n {s} [options] [<loopdev>]\n {s} [options] -f | <loopdev> <file>\n"
    );
    text.push_str(
        "\nSet up and control loop devices.\n\
\nOptions:\n\
\x20-a, --all                     list all used devices\n\
\x20-d, --detach <loopdev>...     detach one or more devices\n\
\x20-D, --detach-all              detach all used devices\n\
\x20-f, --find                    find first unused device\n\
\x20-c, --set-capacity <loopdev>  resize the device\n\
\x20-j, --associated <file>       list all devices associated with <file>\n\
\x20-L, --nooverlap               avoid possible conflict between devices\n\
\n\
\x20-o, --offset <num>            start at offset <num> into file\n\
\x20    --sizelimit <num>         device is limited to <num> bytes of the file\n\
\x20-b, --sector-size <num>       set the logical sector size to <num>\n\
\x20-P, --partscan                create a partitioned loop device\n\
\x20-r, --read-only               set up a read-only loop device\n\
\x20    --direct-io[=<on|off>]    open backing file with O_DIRECT\n\
\x20    --show                    print device name after setup (with -f)\n\
\x20-v, --verbose                 verbose mode\n\
\n\
\x20-J, --json                    use JSON --list output format\n\
\x20-l, --list                    list info about all or specified (default)\n\
\x20-n, --noheadings              don't print headings for --list output\n\
\x20-O, --output <cols>           specify columns to output for --list\n\
\x20    --output-all              output all columns\n\
\x20    --raw                     use raw --list output format\n\
\n",
    );
    text.push_str(&format!("{:<31}{}\n", " -h, --help", "display this help"));
    text.push_str(&format!("{:<31}{}\n", " -V, --version", "display version"));
    text.push_str("\nAvailable output columns:\n");
    for i in &INFOS {
        text.push_str(&format!(" {:>12}  {}\n", i.name, i.help));
    }
    text.push_str("\nFor more details see losetup(8).\n");
    // The program's name is the only part that is not text, and is written
    // as its bytes.
    let mut out = Vec::with_capacity(text.len());
    let mut parts = text.split(&*s);
    if let Some(first) = parts.next() {
        out.extend_from_slice(first.as_bytes());
    }
    for part in parts {
        out.extend_from_slice(short);
        out.extend_from_slice(part.as_bytes());
    }
    out
}

/// `parse_switch (arg, errmesg, "on", "off", NULL)`: 1 for `on`, 0 for
/// `off`, or the message to exit with.
fn parse_switch(arg: &[u8], errmesg: &str) -> Result<u64, String> {
    match arg {
        b"on" => Ok(1),
        b"off" => Ok(0),
        _ => Err(format!("{errmesg}: '{}'", shown(arg))),
    }
}

/// `warn_size (filename, size, offset, flags)`'s complaint, if any: about a
/// file -- or the part of it from `offset` -- smaller than a sector, or not
/// a whole number of them. `st_size` is the file's size, `None` when it
/// cannot be had or it is a block device.
fn warn_size_message(filename: &[u8], size: u64, offset: u64, flags: i32, st_size: Option<u64>) -> Option<String> {
    let mut size = size;
    if size == 0 {
        size = st_size?;
        if flags & LOOPDEV_FL_OFFSET != 0 {
            // `uint64_t`: an offset past the end wraps, as upstream's does.
            size = size.wrapping_sub(offset);
        }
    }
    if size < 512 {
        Some(format!(
            "{}: Warning: file is smaller than 512 bytes; the loop device may be useless or invisible for system tools.",
            shown(filename)
        ))
    } else if !size.is_multiple_of(512) {
        Some(format!(
            "{}: Warning: file does not fit into a 512-byte sector; the end of the file will be ignored.",
            shown(filename)
        ))
    } else {
        None
    }
}

/// `stat (path)`'s device and inode, if it can be had.
fn stat_dev_ino(path: &[u8]) -> Option<(u64, u64)> {
    std::fs::metadata(ulsysfs::path_of(path)).ok().map(|m| dev_ino(&m))
}

/// How `losetup` stops before its end: the status to exit with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Exit(u8);

/// The program's state: upstream's globals.
struct Losetup {
    /// `program_invocation_short_name`.
    short: Vec<u8>,
    /// Standard output, buffered as glibc's.
    out: Stdout,
    no_headings: bool,
    raw: bool,
    json: bool,
    /// `columns[]` and `ncolumns`.
    columns: Vec<Col>,
}

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("losetup"), OsString::as_os_str),
    );
    let mut l = Losetup {
        short,
        out: Stdout::new(1),
        no_headings: false,
        raw: false,
        json: false,
        columns: Vec::new(),
    };
    let status = match l.run(&argv) {
        Ok(status) | Err(Exit(status)) => status,
    };
    let short = l.short.clone();
    ExitCode::from(l.out.close(status, &short))
}

impl Losetup {
    /// `warnx (msg)`.
    fn warnx(&self, msg: &str) {
        warnx(&self.short, msg);
    }

    /// `warn (msg)`, with the `errno` the failure left.
    fn warn(&self, msg: &str, errno: i32) {
        warn(&self.short, msg, &std::io::Error::from_raw_os_error(errno));
    }

    /// `errx (EXIT_FAILURE, msg)`.
    fn errx(&self, msg: &str) -> Exit {
        self.warnx(msg);
        Exit(1)
    }

    /// `err (EXIT_FAILURE, msg)`.
    fn err(&self, msg: &str, errno: i32) -> Exit {
        self.warn(msg, errno);
        Exit(1)
    }

    /// `errtryhelp (EXIT_FAILURE)`.
    fn errtryhelp(&self) -> Exit {
        stderr_write(
            format!(
                "Try '{} --help' for more information.\n",
                shown(&self.short)
            )
            .as_bytes(),
        );
        Exit(1)
    }

    /// `strtosize_or_err (arg, errmesg)`.
    fn strtosize_or_err(&self, arg: &OsStr, errmesg: &str) -> Result<u64, Exit> {
        ulstrutils::parse_size(&os_bytes(arg)).map_err(|e| {
            self.warnx(&ulstrutils::size_error_message(errmesg, arg, e));
            Exit(1)
        })
    }

    /// `is_loopdev (dev) || loopcxt_set_device (lc, dev)` failing: the
    /// `errno` to name, or none when `dev` may be used.
    fn use_device(lc: &mut LoopCxt, dev: &[u8]) -> Result<(), i32> {
        is_loopdev(dev)?;
        lc.set_device(Some(dev))
    }

    /// `printf_loopdev (lc)`: `--all`'s line for the current device -- the
    /// backing file's device and inode in brackets when they can be had
    /// (`[]` when the device cannot be opened), then the file, the offset
    /// and the size limit if not 0, and an encryption if any.
    ///
    /// # Errors
    ///
    /// The `errno` of not finding the backing file.
    fn printf_loopdev(&mut self, lc: &mut LoopCxt) -> Result<(), i32> {
        let fname = lc.backing_file()?;
        let device = lc.device().unwrap_or_default().to_vec();
        let mut dev = 0u64;
        let mut ino = 0u64;
        if let Ok(d) = lc.backing_devno() {
            dev = d;
            if let Ok(i) = lc.backing_inode() {
                ino = i;
            }
        }
        let mut line = device;
        if dev == 0 && ino == 0 {
            // "Probably non-root user (no permissions to call
            // LOOP_GET_STATUS ioctls)."
            line.extend_from_slice(b": []: (");
            line.extend_from_slice(&fname);
            line.push(b')');
            Self::offset_and_limit(lc, &mut line);
        } else {
            // `[%04jd]:%ju`: the device number signed, at least four digits.
            line.extend_from_slice(format!(": [{:04}]:{ino} (", dev.cast_signed()).as_bytes());
            line.extend_from_slice(&fname);
            line.push(b')');
            Self::offset_and_limit(lc, &mut line);
            if let Ok(ty) = lc.encrypt_type() {
                let mut e = lc.crypt_name().unwrap_or_default();
                if e.is_empty() && ty == 1 {
                    e = b"XOR".to_vec();
                }
                if !e.is_empty() {
                    line.extend_from_slice(b", encryption ");
                    line.extend_from_slice(&e);
                    line.extend_from_slice(format!(" (type {ty})").as_bytes());
                }
            }
        }
        line.push(b'\n');
        self.out.write(&line);
        Ok(())
    }

    /// `printf_loopdev`'s `, offset N` and `, sizelimit N`, for those not 0.
    fn offset_and_limit(lc: &mut LoopCxt, line: &mut Vec<u8>) {
        if let Ok(x) = lc.offset()
            && x != 0
        {
            line.extend_from_slice(format!(", offset {x}").as_bytes());
        }
        if let Ok(x) = lc.sizelimit()
            && x != 0
        {
            line.extend_from_slice(format!(", sizelimit {x}").as_bytes());
        }
    }

    /// The loop the listings share: every used device, or with `file` those
    /// on it -- by its device and inode, else by its name, else by its
    /// canonical name -- at `offset` with `LOOPDEV_FL_OFFSET`; `each` for
    /// each.
    fn each_used(
        &mut self,
        lc: &mut LoopCxt,
        file: Option<&[u8]>,
        offset: u64,
        flags: i32,
        mut each: impl FnMut(&mut Self, &mut LoopCxt) -> Result<(), i32>,
    ) -> Result<(), i32> {
        lc.init_iterator(LOOPITER_FL_USED);
        let st = file.and_then(stat_dev_ino);
        let mut cn_file: Option<Vec<u8>> = None;
        let mut rc = Ok(());
        while lc.next_device() {
            if let Some(file) = file {
                let bf = cn_file.as_deref().unwrap_or(file);
                let mut used = lc.is_used(st, Some(bf), offset, 0, flags);
                if !used && cn_file.is_none() {
                    cn_file = ulsysfs::canonicalize::canonicalize_path(file);
                    used = lc.is_used(st, cn_file.as_deref(), offset, 0, flags);
                }
                if !used {
                    continue;
                }
            }
            rc = each(self, lc);
            if rc.is_err() {
                break;
            }
        }
        lc.deinit_iterator();
        rc
    }

    /// `show_all_loops (lc, file, offset, flags)`: `--all`'s lines.
    fn show_all_loops(&mut self, lc: &mut LoopCxt, file: Option<&[u8]>, offset: u64, flags: i32) {
        // `printf_loopdev`'s failure is not looked at here.
        let _ = self.each_used(lc, file, offset, flags, |l, lc| {
            let _ = l.printf_loopdev(lc);
            Ok(())
        });
    }

    /// `delete_loop (lc)`: the current device detached -- 0, or -1 after
    /// saying why not.
    fn delete_loop(&self, lc: &mut LoopCxt) -> i32 {
        match lc.delete_device() {
            Ok(()) => 0,
            Err(e) => {
                let device = lc.device().unwrap_or_default().to_vec();
                self.warn(&format!("{}: detach failed", shown(&device)), e);
                -1
            }
        }
    }

    /// `delete_all_loops (lc)`: every used device detached -- how many could
    /// not be, negated.
    fn delete_all_loops(&self, lc: &mut LoopCxt) -> i32 {
        let mut res = 0i32;
        lc.init_iterator(LOOPITER_FL_USED);
        while lc.next_device() {
            res = res.saturating_add(self.delete_loop(lc));
        }
        lc.deinit_iterator();
        res
    }

    /// `set_scols_data (lc, ln)`: the current device's row.
    fn set_scols_data(
        &self,
        lc: &mut LoopCxt,
        tb: &mut Table,
        ids: &[(Col, smartcols::ColumnId)],
    ) -> Result<(), Exit> {
        let Ok(ln) = tb.new_line(None) else {
            return Err(self.err("failed to allocate output line", ENOENT));
        };
        let short = self.raw || self.json;
        for &(col, cl) in ids {
            let data: Option<Vec<u8>> = match col {
                Col::Name => lc.device().map(<[u8]>::to_vec),
                Col::BackFile => lc.backing_file().ok(),
                Col::Offset => lc.offset().ok().map(|x| x.cast_signed().to_string().into_bytes()),
                Col::Sizelimit => lc.sizelimit().ok().map(|x| x.cast_signed().to_string().into_bytes()),
                Col::BackMajmin => lc.backing_devno().ok().filter(|&d| d != 0).map(|d| {
                    let (ma, mi) = (ulsysfs::major(d), ulsysfs::minor(d));
                    if short {
                        format!("{ma}:{mi}")
                    } else {
                        format!("{ma:8}:{mi:<3}")
                    }
                    .into_bytes()
                }),
                Col::Majmin => lc
                    .device()
                    .and_then(|d| std::fs::metadata(ulsysfs::path_of(d)).ok())
                    .and_then(|m| block_rdev(&m))
                    .filter(|&r| ulsysfs::major(r) == LOOPDEV_MAJOR)
                    .map(|r| {
                        let (ma, mi) = (ulsysfs::major(r), ulsysfs::minor(r));
                        if short {
                            format!("{ma}:{mi}")
                        } else {
                            format!("{ma:3}:{mi:<3}")
                        }
                        .into_bytes()
                    }),
                Col::BackIno => lc
                    .backing_inode()
                    .ok()
                    .filter(|&i| i != 0)
                    .map(|i| i.to_string().into_bytes()),
                Col::Autoclr => Some(flag(lc.is_autoclear())),
                Col::Ro => Some(flag(lc.is_readonly())),
                Col::Dio => Some(flag(lc.is_dio())),
                Col::Partscan => Some(flag(lc.is_partscan())),
                Col::Logsec => lc.blocksize().ok().map(|x| x.cast_signed().to_string().into_bytes()),
            };
            if let Some(data) = data
                && tb.line_set_data(ln, cl, &data).is_err()
            {
                return Err(self.err("failed to add output data", ENOENT));
            }
        }
        Ok(())
    }

    /// `show_table (lc, file, offset, flags)`: the table -- of the current
    /// device if there is one, else of every used one (on `file`, if given).
    fn show_table(&mut self, lc: &mut LoopCxt, file: Option<&[u8]>, offset: u64, flags: i32) -> Result<i32, Exit> {
        let mut tb = Table::new();
        tb.enable_raw(self.raw);
        tb.enable_json(self.json);
        tb.enable_noheadings(self.no_headings);
        if self.json {
            tb.set_name(b"loopdevices");
        }
        let mut ids = Vec::with_capacity(self.columns.len());
        for &col in &self.columns {
            let ci = info(col);
            let cl = tb.new_column(ci.name.as_bytes(), ci.whint, ci.flags);
            if self.json {
                // The column is the table's own.
                let _ = tb.column_set_json_type(cl, ci.json_type);
            }
            ids.push((col, cl));
        }
        if lc.has_device() {
            // "only one loopdev requested (already assigned to loopdev_cxt)"
            self.set_scols_data(lc, &mut tb, &ids)?;
        } else {
            let mut failed = None;
            let _ = self.each_used(lc, file, offset, flags, |l, lc| match l.set_scols_data(lc, &mut tb, &ids) {
                Ok(()) => Ok(()),
                Err(e) => {
                    failed = Some(e);
                    Err(0)
                }
            });
            if let Some(e) = failed {
                return Err(e);
            }
        }
        // `scols_print_table`.
        let mut text = Vec::new();
        let rc = i32::from(tb.print_into(&mut text).is_err());
        self.out.write(&text);
        Ok(rc)
    }

    /// `find_unused (lc)`: a free device made the current one, or 1 after
    /// saying none could be found -- with the `errno` of `/dev/loop-control`
    /// not being writable where it is there and is not, else the search's.
    fn find_unused(&self, lc: &mut LoopCxt) -> i32 {
        let Err(rc) = lc.find_unused() else {
            return 0;
        };
        let errno = if libcall::file::access(PATH_DEV_LOOPCTL, libcall::file::F_OK).is_ok() {
            match libcall::file::access(PATH_DEV_LOOPCTL, libcall::file::W_OK) {
                Err(e) => e,
                Ok(()) => rc,
            }
        } else {
            rc
        };
        self.warn("cannot find an unused loop device", errno);
        1
    }

    /// `create_loop (lc, nooverlap, lo_flags, flags, file, offset,
    /// sizelimit, blocksize)`: a device set up on `file` -- the current one,
    /// or the first free one -- or, with `--nooverlap`, one already on the
    /// same part of it reused.
    #[allow(
        clippy::too_many_arguments,
        reason = "upstream's signature, kept to be read against it"
    )]
    fn create_loop(
        &self,
        lc: &mut LoopCxt,
        nooverlap: bool,
        lo_flags: u32,
        flags: i32,
        file: &[u8],
        offset: u64,
        sizelimit: u64,
        blocksize: u64,
    ) -> Result<i32, Exit> {
        let hasdev = lc.has_device();
        let shown_file = shown(file);
        // "losetup --find --noverlap file.img"
        if !hasdev && nooverlap {
            match lc.find_overlap(file, offset, sizelimit) {
                0 => {}
                1 => return Err(self.errx(&format!("{shown_file}: overlapping loop device exists"))),
                2 => {
                    // "Once a loop is initialized RO, there is no way to
                    // change its parameters."
                    if lc.is_readonly() != 0 && lo_flags & LO_FLAGS_READ_ONLY == 0 {
                        return Err(self.errx(&format!(
                            "{shown_file}: overlapping read-only loop device exists"
                        )));
                    }
                    if let Ok(ty) = lc.encrypt_type()
                        && ty != LO_CRYPT_NONE
                    {
                        return Err(self.errx(&format!(
                            "{shown_file}: overlapping encrypted loop device exists"
                        )));
                    }
                    lc.clear_flag(LO_FLAGS_AUTOCLEAR);
                    if lc.ioctl_status().is_err() {
                        return Err(self.errx(&format!("{shown_file}: failed to re-use loop device")));
                    }
                    // "success, re-use"
                    return Ok(0);
                }
                _ => return Err(self.errx("failed to inspect loop devices")),
            }
        }
        if hasdev {
            // A device that is already there makes this fail; the setup
            // says what matters.
            let _ = lc.add_device();
        }
        // "losetup --noverlap /dev/loopN file.img"
        if hasdev && nooverlap {
            let mut lc2 = LoopCxt::new(0);
            let rc = lc2.find_overlap(file, offset, sizelimit);
            if rc > 0 {
                return Err(self.errx(&format!("{shown_file}: overlapping loop device exists")));
            }
        }
        let mut ntries = 0u32;
        loop {
            if !hasdev {
                let rc = self.find_unused(lc);
                if rc != 0 {
                    return Ok(rc);
                }
            }
            if flags & LOOPDEV_FL_OFFSET != 0 {
                lc.set_offset(offset);
            }
            if flags & LOOPDEV_FL_SIZELIMIT != 0 {
                lc.set_sizelimit(sizelimit);
            }
            if lo_flags != 0 {
                lc.set_flags(lo_flags);
            }
            if blocksize > 0 {
                lc.set_blocksize(blocksize);
            }
            if let Err(e) = lc.set_backing_file(file) {
                self.warn(&format!("{shown_file}: failed to use backing file"), e);
                return Ok(-1);
            }
            let Err(mut errno) = lc.setup_device() else {
                return Ok(0);
            };
            if (errno == EBUSY || errno == EAGAIN) && !hasdev && ntries < 64 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                ntries = ntries.saturating_add(1);
                continue;
            }
            // "errors": named by the device when it cannot even be opened --
            // and that open's `errno` is the one printed.
            let errpre = if hasdev {
                match lc.fd().map(|_| ()) {
                    Ok(()) => shown_file.clone(),
                    Err(e) => {
                        errno = e;
                        shown(lc.device().unwrap_or_default())
                    }
                }
            } else {
                shown_file.clone()
            };
            self.warn(&format!("{errpre}: failed to set up loop device"), errno);
            return Ok(errno.saturating_neg());
        }
    }

    /// `main ()`, to the point of its `return`: the status.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's main, kept in one piece so it can be read against it"
    )]
    fn run(&mut self, argv: &[OsString]) -> Result<u8, Exit> {
        let arg0 = argv
            .first()
            .map_or(OsStr::new("losetup"), OsString::as_os_str);
        let mut lc = LoopCxt::new(0);
        let mut act = Act::None;
        let mut flags = 0i32;
        let mut no_overlap = false;
        let mut file: Option<Vec<u8>> = None;
        let (mut offset, mut sizelimit, mut blocksize) = (0u64, 0u64, 0u64);
        let mut showdev = false;
        let mut lo_flags = 0u32;
        let mut outarg: Option<Vec<u8>> = None;
        let mut list = false;
        let (mut use_dio, mut set_dio, mut set_blocksize) = (0u64, false, false);
        let mut excl_st = [0i32; 4];
        let mut operands: Vec<Vec<u8>> = Vec::new();

        let own = argv.get(1..).unwrap_or_default();
        for item in LOSETUP.parse(own, SHORTS, LONGS) {
            let opt = match item {
                Ok(opt) => opt,
                Err(e) => {
                    // glibc names the program by argv[0] as given.
                    stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                    return Err(self.errtryhelp());
                }
            };
            let Some((c, value)) = option_code(&opt) else {
                if let Opt::Operand(w) = opt {
                    operands.push(os_bytes(w).into_owned());
                }
                continue;
            };
            if let Some(msg) = err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, &self.short) {
                stderr_write(msg.as_bytes());
                return Err(Exit(1));
            }
            let arg = value.clone().unwrap_or_default();
            let argb = os_bytes(&arg).into_owned();
            match c {
                OPT_SIZELIMIT => {
                    sizelimit = self.strtosize_or_err(&arg, "failed to parse size")?;
                    flags |= LOOPDEV_FL_SIZELIMIT;
                }
                OPT_SHOW => showdev = true,
                OPT_RAW => self.raw = true,
                OPT_DIO => {
                    use_dio = 1;
                    set_dio = true;
                    if value.is_some() {
                        use_dio = parse_switch(&argb, "argument error").map_err(|m| self.errx(&m))?;
                    }
                    if use_dio != 0 {
                        lo_flags |= LO_FLAGS_DIRECT_IO;
                    }
                }
                OPT_OUTPUT_ALL => {
                    self.columns = INFOS.iter().map(|i| i.id).collect();
                }
                _ => match u8::try_from(c).unwrap_or(0) {
                    b'a' => act = Act::Show,
                    b'b' => {
                        set_blocksize = true;
                        blocksize = self.strtosize_or_err(&arg, "failed to parse logical block size")?;
                    }
                    b'c' => {
                        act = Act::SetCapacity;
                        if let Err(e) = Self::use_device(&mut lc, &argb) {
                            return Err(self.err(&format!("{}: failed to use device", shown(&argb)), e));
                        }
                    }
                    b'r' => lo_flags |= LO_FLAGS_READ_ONLY,
                    b'd' => {
                        act = Act::Delete;
                        if let Err(e) = Self::use_device(&mut lc, &argb) {
                            return Err(self.err(&format!("{}: failed to use device", shown(&argb)), e));
                        }
                    }
                    b'D' => act = Act::DeleteAll,
                    b'f' => act = Act::FindFree,
                    b'J' => self.json = true,
                    b'j' => {
                        act = Act::Show;
                        file = Some(argb);
                    }
                    b'l' => list = true,
                    b'L' => no_overlap = true,
                    b'n' => self.no_headings = true,
                    b'o' => {
                        offset = self.strtosize_or_err(&arg, "failed to parse offset")?;
                        flags |= LOOPDEV_FL_OFFSET;
                    }
                    b'O' => {
                        outarg = Some(argb);
                        list = true;
                    }
                    b'P' => lo_flags |= LO_FLAGS_PARTSCAN,
                    b'v' => {}
                    b'h' => {
                        let text = usage(&self.short);
                        self.out.write(&text);
                        return Ok(0);
                    }
                    b'V' => {
                        let mut line = self.short.clone();
                        line.extend_from_slice(b" from util-linux 2.39.3\n");
                        self.out.write(&line);
                        return Ok(0);
                    }
                    _ => return Err(self.errtryhelp()),
                },
            }
        }

        let mut ops = operands.into_iter().peekable();
        // "default is --list --all"
        if argv.len() == 1 {
            act = Act::Show;
            list = true;
        }
        if act == Act::None && argv.len() == 2 && (self.raw || self.json) {
            act = Act::Show;
            list = true;
        }
        // "default --list output columns"
        if list && self.columns.is_empty() {
            self.columns = vec![
                Col::Name,
                Col::Sizelimit,
                Col::Offset,
                Col::Autoclr,
                Col::Ro,
                Col::BackFile,
                Col::Dio,
                Col::Logsec,
            ];
        }
        if act == Act::FindFree && ops.peek().is_some() {
            // "losetup -f <backing_file>"
            act = Act::Create;
            file = ops.next();
            if ops.peek().is_some() {
                return Err(self.errx("unexpected arguments"));
            }
        }
        if list && act == Act::None && ops.peek().is_none() {
            // "losetup --list defaults to --all"
            act = Act::Show;
        }
        if act == Act::None && ops.len() == 1 {
            // "losetup [--list] <device>", or a change to one.
            if set_dio {
                act = Act::SetDirectIo;
                lo_flags &= !LO_FLAGS_DIRECT_IO;
            } else if set_blocksize {
                act = Act::SetBlocksize;
            } else {
                act = Act::ShowOne;
            }
            let dev = ops.next().unwrap_or_default();
            if let Err(e) = Self::use_device(&mut lc, &dev) {
                return Err(self.err(&format!("{}: failed to use device", shown(&dev)), e));
            }
        }
        if act == Act::None {
            // "losetup <loopdev> <backing_file>"
            act = Act::Create;
            let Some(dev) = ops.next() else {
                return Err(self.errx("no loop device specified"));
            };
            // "don't use is_loopdev() here, the device does not have exist yet"
            if let Err(e) = lc.set_device(Some(&dev)) {
                return Err(self.err(&format!("{}: failed to use device", shown(&dev)), e));
            }
            let Some(f) = ops.next() else {
                return Err(self.errx("no file specified"));
            };
            file = Some(f);
        }
        if act != Act::Create && (sizelimit != 0 || lo_flags != 0 || showdev) {
            return Err(self.errx(
                "the options --{sizelimit,partscan,read-only,show} are allowed during loop device setup only",
            ));
        }
        if flags & LOOPDEV_FL_OFFSET != 0 && act != Act::Create && (act != Act::Show || file.is_none()) {
            return Err(self.errx("the option --offset is not allowed in this context"));
        }
        if let Some(list) = outarg {
            let short = self.short.clone();
            if string_add_to_idarray(&list, &mut self.columns, MAX_COLUMNS, |name, rest| {
                column_name_to_id(name, rest, &short)
            })
            .is_err()
            {
                return Ok(1);
            }
        }

        let res: i32 = match act {
            Act::Create => {
                let file = file.unwrap_or_default();
                let res = self.create_loop(
                    &mut lc, no_overlap, lo_flags, flags, &file, offset, sizelimit, blocksize,
                )?;
                if res == 0 {
                    if showdev {
                        let mut line = lc.device().unwrap_or_default().to_vec();
                        line.push(b'\n');
                        self.out.write(&line);
                    }
                    let st_size = std::fs::metadata(ulsysfs::path_of(&file))
                        .ok()
                        .filter(|m| block_rdev(m).is_none())
                        .map(|m| m.len());
                    if let Some(m) = warn_size_message(&file, sizelimit, offset, flags, st_size) {
                        self.warnx(&m);
                    }
                }
                res
            }
            Act::Delete => {
                let mut res = self.delete_loop(&mut lc);
                for dev in ops {
                    if let Err(e) = Self::use_device(&mut lc, &dev) {
                        self.warn(&format!("{}: failed to use device", shown(&dev)), e);
                    }
                    res = res.saturating_add(self.delete_loop(&mut lc));
                }
                res
            }
            Act::DeleteAll => self.delete_all_loops(&mut lc),
            Act::FindFree => {
                let res = self.find_unused(&mut lc);
                if res == 0 {
                    let mut line = lc.device().unwrap_or_default().to_vec();
                    line.push(b'\n');
                    self.out.write(&line);
                }
                res
            }
            Act::Show => {
                if list {
                    self.show_table(&mut lc, file.as_deref(), offset, flags)?
                } else {
                    self.show_all_loops(&mut lc, file.as_deref(), offset, flags);
                    0
                }
            }
            Act::ShowOne => {
                let res = if list {
                    match self.show_table(&mut lc, None, 0, 0)? {
                        0 => Ok(()),
                        _ => Err(ENOENT),
                    }
                } else {
                    self.printf_loopdev(&mut lc)
                };
                match res {
                    Ok(()) => 0,
                    Err(e) => {
                        let dev = lc.device().unwrap_or_default().to_vec();
                        self.warn(&shown(&dev), e);
                        -1
                    }
                }
            }
            Act::SetCapacity => self.device_call(&mut lc, "set capacity failed", LoopCxt::ioctl_capacity),
            Act::SetDirectIo => {
                self.device_call(&mut lc, "set direct io failed", |lc| lc.ioctl_dio(use_dio))
            }
            Act::SetBlocksize => self.device_call(&mut lc, "set logical block size failed", |lc| {
                lc.ioctl_blocksize(blocksize)
            }),
            Act::None => {
                self.warnx("bad usage");
                return Err(self.errtryhelp());
            }
        };
        Ok(u8::from(res != 0))
    }

    /// One change to the current device: `call`, and on its failure the
    /// device named with `what` -- 0, or -1.
    fn device_call(&self, lc: &mut LoopCxt, what: &str, call: impl FnOnce(&mut LoopCxt) -> Result<(), i32>) -> i32 {
        match call(lc) {
            Ok(()) => 0,
            Err(e) => {
                let dev = lc.device().unwrap_or_default().to_vec();
                self.warn(&format!("{}: {what}", shown(&dev)), e);
                -1
            }
        }
    }
}

/// `"1"` or `"0"`, for a flag column.
fn flag(v: i32) -> Vec<u8> {
    if v != 0 { b"1".to_vec() } else { b"0".to_vec() }
}

/// Whether `m` is a block device, and its device number.
fn block_rdev(m: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        m.file_type().is_block_device().then(|| m.rdev())
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        None
    }
}

#[cfg(test)]
mod tests;
