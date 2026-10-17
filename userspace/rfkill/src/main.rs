//! rfkill -- list, block and unblock wireless devices.
//!
//! A port of util-linux 2.39.3's `sys-utils/rfkill.c`, function by function
//! and with upstream's names, printing its table through `smartcols` (the
//! libsmartcols port); measured against Ubuntu's `rfkill` (util-linux
//! 2.39.3) by `scripts/rfkill-diff.sh`, on a `/dev/rfkill` and a
//! `/sys/class/rfkill` of the harness's own.
//!
//! # How it talks to the kernel
//!
//! `/dev/rfkill` is a stream of 8-byte events (`struct rfkill_event`: the
//! device's index, its type, the operation, the soft and hard block). Opened,
//! it first gives one `ADD` event for every device there is, then one for
//! each change; listing reads it without waiting until `EAGAIN`. Blocking
//! writes one event back -- `CHANGE` for a device, `CHANGE_ALL` for a type or
//! for all -- and logs it to syslog. A device's name and type name are read
//! from `/sys/class/rfkill/rfkillN/`. SlateOS has neither yet, so there every
//! command but `help` says `cannot open /dev/rfkill`, as upstream's does on a
//! Linux without the driver.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **The sysfs reader keeps its last answer.** `get_sys_attr` reads into
//!   one static buffer and returns it whether or not the read worked, so a
//!   device whose `name` cannot be read shows what was read last -- its own
//!   `type`, in the table, whose TYPE column comes first. At most 127 bytes
//!   are read, up to the first newline, and a NUL ends the value.
//! * **A type with no description aborts.** The description of a device
//!   type the table does not know -- 0, which is `all`, or one past NFC -- is
//!   `NULL`, and `xstrdup(NULL)` fails its assertion: the plain `list` and
//!   the TYPE-DESC column both die of `SIGABRT`, what they had printed lost.
//! * **A short event is skipped, and `errno` decides what comes next.** A
//!   read of fewer than 8 bytes is warned about; listing then stops only if
//!   `errno` happens to be `EAGAIN` -- left there by an earlier identifier's
//!   last read -- and otherwise reads on. [`Rfkill::errno`] follows `errno`
//!   through the calls upstream makes, for that test alone.
//! * **`toggle all` toggles everything once per device**: every device's
//!   `ADD` event sends a `CHANGE_ALL` that sets every device to the opposite
//!   of *that* device's soft block.
//! * **`-o` adds to the default columns with `+`, else replaces them**, and
//!   only when `list` is not named alone: `rfkill list` is the old format,
//!   `rfkill` and `rfkill -o ... list` the table.
//! * **`id %d`**: the log message prints the index as a signed `int`.
//! * **A failed write changes nothing**: `write failed: /dev/rfkill` is
//!   warned about, and the status is still 0.
//!
//! # What is not upstream's
//!
//! * **An identifier in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).

stdfdguard::guard_std_fds!();

use getoptlong::{LongVals, Opt, Program, Takes};
use libcall::{EAGAIN, EINVAL};
use quoting::{escape_unprintable, os_bytes, os_from_bytes};
use smartcols::{FL_RIGHT, JsonType, Table};
use std::ffi::{CString, OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulstrutils::{NumErr, err_exclusive_options, num_error_message, string_add_to_idarray};

/// `_PATH_DEV_RFKILL` and `_PATH_SYS_RFKILL`.
const PATH_DEV_RFKILL: &str = "/dev/rfkill";
const PATH_SYS_RFKILL: &str = "/sys/class/rfkill";

/// `RFKILL_EVENT_SIZE_V1`, which is also `sizeof (struct rfkill_event)` in
/// the headers upstream is built with.
const RFKILL_EVENT_SIZE_V1: usize = 8;

/// `enum rfkill_type`.
const RFKILL_TYPE_ALL: u32 = 0;
const RFKILL_TYPE_WLAN: u32 = 1;
const RFKILL_TYPE_BLUETOOTH: u32 = 2;
const RFKILL_TYPE_UWB: u32 = 3;
const RFKILL_TYPE_WIMAX: u32 = 4;
const RFKILL_TYPE_WWAN: u32 = 5;
const RFKILL_TYPE_GPS: u32 = 6;
const RFKILL_TYPE_FM: u32 = 7;
const RFKILL_TYPE_NFC: u32 = 8;
const NUM_RFKILL_TYPES: u32 = 9;

/// `enum rfkill_operation`.
const RFKILL_OP_ADD: u8 = 0;
const RFKILL_OP_CHANGE: u8 = 2;
const RFKILL_OP_CHANGE_ALL: u8 = 3;

/// `struct rfkill_type_str`.
struct TypeStr {
    ty: u32,
    name: Option<&'static str>,
    desc: Option<&'static str>,
}

/// `rfkill_type_strings[]`, in its order -- the first row of a type gives
/// its description -- with its terminator, which `rfkill_type_to_desc`
/// searches too.
const RFKILL_TYPE_STRINGS: [TypeStr; 12] = [
    TypeStr {
        ty: RFKILL_TYPE_ALL,
        name: Some("all"),
        desc: None,
    },
    TypeStr {
        ty: RFKILL_TYPE_WLAN,
        name: Some("wlan"),
        desc: Some("Wireless LAN"),
    },
    TypeStr {
        ty: RFKILL_TYPE_WLAN,
        name: Some("wifi"),
        desc: None,
    },
    TypeStr {
        ty: RFKILL_TYPE_BLUETOOTH,
        name: Some("bluetooth"),
        desc: Some("Bluetooth"),
    },
    TypeStr {
        ty: RFKILL_TYPE_UWB,
        name: Some("uwb"),
        desc: Some("Ultra-Wideband"),
    },
    TypeStr {
        ty: RFKILL_TYPE_UWB,
        name: Some("ultrawideband"),
        desc: None,
    },
    TypeStr {
        ty: RFKILL_TYPE_WIMAX,
        name: Some("wimax"),
        desc: Some("WiMAX"),
    },
    TypeStr {
        ty: RFKILL_TYPE_WWAN,
        name: Some("wwan"),
        desc: Some("Wireless WAN"),
    },
    TypeStr {
        ty: RFKILL_TYPE_GPS,
        name: Some("gps"),
        desc: Some("GPS"),
    },
    TypeStr {
        ty: RFKILL_TYPE_FM,
        name: Some("fm"),
        desc: Some("FM"),
    },
    TypeStr {
        ty: RFKILL_TYPE_NFC,
        name: Some("nfc"),
        desc: Some("NFC"),
    },
    TypeStr {
        ty: NUM_RFKILL_TYPES,
        name: None,
        desc: None,
    },
];

/// `struct rfkill_id`: what an identifier on the command line names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RfkillId {
    /// `RFKILL_IS_INVALID`.
    Invalid,
    /// `RFKILL_IS_TYPE`: every device of a type.
    Type(u32),
    /// `RFKILL_IS_INDEX`: one device, by its index.
    Index(u32),
    /// `RFKILL_IS_ALL`.
    All,
}

/// `struct rfkill_event`, as it comes from and goes to `/dev/rfkill`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Event {
    idx: u32,
    ty: u8,
    op: u8,
    soft: u8,
    hard: u8,
}

impl Event {
    /// The event in the kernel's layout: the index in the machine's order,
    /// then the four bytes.
    fn to_bytes(self) -> [u8; RFKILL_EVENT_SIZE_V1] {
        let [a, b, c, d] = self.idx.to_ne_bytes();
        [a, b, c, d, self.ty, self.op, self.soft, self.hard]
    }

    /// An event from the kernel's layout.
    fn from_bytes(b: [u8; RFKILL_EVENT_SIZE_V1]) -> Self {
        let [a, b0, c, d, ty, op, soft, hard] = b;
        Event {
            idx: u32::from_ne_bytes([a, b0, c, d]),
            ty,
            op,
            soft,
            hard,
        }
    }
}

/// The supported actions, `ACT_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    List,
    Help,
    Event,
    Block,
    Unblock,
    Toggle,
    ListOld,
}

/// `rfkill_actions[]`.
const RFKILL_ACTIONS: [(&str, Action); 6] = [
    ("list", Action::List),
    ("help", Action::Help),
    ("event", Action::Event),
    ("block", Action::Block),
    ("unblock", Action::Unblock),
    ("toggle", Action::Toggle),
];

/// The column IDs, `COL_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Device,
    Id,
    Type,
    Desc,
    Soft,
    Hard,
}

/// `struct colinfo`.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
}

/// `infos[]`, in `enum` order.
const INFOS: [ColInfo; 6] = [
    ColInfo {
        id: Col::Device,
        name: "DEVICE",
        whint: 0.0,
        flags: 0,
        help: "kernel device name",
    },
    ColInfo {
        id: Col::Id,
        name: "ID",
        whint: 2.0,
        flags: FL_RIGHT,
        help: "device identifier value",
    },
    ColInfo {
        id: Col::Type,
        name: "TYPE",
        whint: 0.0,
        flags: 0,
        help: "device type name that can be used as identifier",
    },
    ColInfo {
        id: Col::Desc,
        name: "TYPE-DESC",
        whint: 0.0,
        flags: 0,
        help: "device type description",
    },
    ColInfo {
        id: Col::Soft,
        name: "SOFT",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "status of software block",
    },
    ColInfo {
        id: Col::Hard,
        name: "HARD",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "status of hardware block",
    },
];

/// `columns[ARRAY_SIZE(infos) * 2]`.
const MAX_COLUMNS: usize = 12;

/// Getopt's errors are only sentences here; the referral follows them.
const RFKILL: Program = Program::new("rfkill", 1);

/// Upstream's option string.
const SHORTS: &str = "Jno:rVh";

/// `OPT_LIST_TYPES`, `CHAR_MAX + 1`: `--output-all`.
const OPT_LIST_TYPES: i32 = 128;

/// Upstream's `longopts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("json", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("output", Takes::Required),
    ("output-all", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];
const LONG_VALS: [i32; 7] = [
    b'J' as i32,
    b'n' as i32,
    b'o' as i32,
    OPT_LIST_TYPES,
    b'r' as i32,
    b'V' as i32,
    b'h' as i32,
];

/// [`LONGS`] and [`LONG_VALS`], upstream's `struct option[]`, as one table.
const OPTIONS: LongVals<'static, i32> = LongVals::new(LONGS, &LONG_VALS);

/// `excl[]`.
const EXCL: [&[i32]; 1] = [&[b'J' as i32, b'r' as i32]];

/// Why the program stops before its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// `exit (status)`: `close_stdout` runs.
    Exit(u8),
    /// `abort ()`, after an assertion's message: nothing is flushed.
    Abort,
}

/// The program's state: upstream's globals and the C library's.
struct Rfkill {
    /// `program_invocation_short_name`.
    short: Vec<u8>,
    /// Standard output, buffered as glibc's.
    out: Stdout,
    /// `errno`, as the calls upstream makes leave it -- read once, by the
    /// listing loops after a short event (module docs).
    errno: i32,
    /// `get_sys_attr`'s `static char name[128]`, as a C string.
    sys_attr: Vec<u8>,
    /// `_PATH_DEV_RFKILL`; another file in the tests.
    dev: String,
    /// `_PATH_SYS_RFKILL`; another directory in the tests.
    sys: PathBuf,
    /// `json`, `no_headings` and `raw` of `struct control`.
    json: bool,
    no_headings: bool,
    raw: bool,
    /// `columns[]` and `ncolumns`.
    columns: Vec<Col>,
    /// Where a request is logged: [`syslog_notice`], or a test's own.
    log: fn(&[u8]),
}

/// A request logged as `__rfkill_block` logs it: `openlog ("rfkill", 0,
/// LOG_USER)`, the line at `LOG_NOTICE`, `closelog ()`.
fn syslog_notice(line: &[u8]) {
    libcsyslog::openlog(b"rfkill", 0, libcsyslog::LOG_USER);
    libcsyslog::syslog(libcsyslog::LOG_NOTICE, line);
    libcsyslog::closelog();
}

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("rfkill"), OsString::as_os_str),
    );
    // `setlocale(LC_ALL, "")`, and what it leaves in `errno`.
    let errno = smartcols::tty::setlocale_errno();
    let mut rf = Rfkill::new(short, errno, PATH_DEV_RFKILL, Path::new(PATH_SYS_RFKILL));
    let stop = rf.run(&argv);
    let short = rf.short.clone();
    match stop {
        Stop::Exit(status) => ExitCode::from(rf.out.close(status, &short)),
        Stop::Abort => std::process::abort(),
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

/// `column_name_to_id(name, namesz)`: a column by its name, in any case.
/// Unknown, it is reported with the rest of the list after it, as
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

/// A column's `infos[]` entry.
fn info(col: Col) -> &'static ColInfo {
    // `infos[]` is in `enum` order (a test holds it to that).
    match col {
        Col::Device => &INFOS[0],
        Col::Id => &INFOS[1],
        Col::Type => &INFOS[2],
        Col::Desc => &INFOS[3],
        Col::Soft => &INFOS[4],
        Col::Hard => &INFOS[5],
    }
}

/// `string_to_action(str)`.
fn string_to_action(s: &[u8]) -> Option<Action> {
    RFKILL_ACTIONS
        .iter()
        .find(|(name, _)| name.as_bytes() == s)
        .map(|&(_, a)| a)
}

/// `rfkill_type_to_desc(type)`: the description of the first row of the
/// table with that type -- `None` for `all`, for the terminator's
/// `NUM_RFKILL_TYPES`, and for any type past it.
fn rfkill_type_to_desc(ty: u32) -> Option<&'static str> {
    RFKILL_TYPE_STRINGS
        .iter()
        .find(|t| t.ty == ty)
        .and_then(|t| t.desc)
}

/// `event_match(event, id)`: an `ADD` event of the device or type asked for.
fn event_match(event: &Event, id: RfkillId) -> bool {
    if event.op != RFKILL_OP_ADD {
        return false;
    }
    match id {
        RfkillId::Type(ty) => u32::from(event.ty) == ty,
        RfkillId::Index(index) => event.idx == index,
        // `abort ()` upstream for an invalid one, which no caller passes.
        RfkillId::All | RfkillId::Invalid => true,
    }
}

/// `usage()`'s text.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(short);
    text.extend_from_slice(
        b" [options] command [identifier ...]\n\
\nTool for enabling and disabling wireless devices.\n\
\nOptions:\n\
\x20-J, --json             use JSON output format\n\
\x20-n, --noheadings       don't print headings\n\
\x20-o, --output <list>    define which output columns to use\n\
\x20    --output-all       output all columns\n\
\x20-r, --raw              use the raw output format\n\
\n",
    );
    text.extend_from_slice(format!("{:<24}{}\n", " -h, --help", "display this help").as_bytes());
    text.extend_from_slice(format!("{:<24}{}\n", " -V, --version", "display version").as_bytes());
    text.extend_from_slice(b"\nAvailable output columns:\n");
    for i in &INFOS {
        text.extend_from_slice(format!(" {:<10}  {}\n", i.name, i.help).as_bytes());
    }
    text.extend_from_slice(
        b"\nCommands:\n\
\x20help\n\
\x20event\n\
\x20list   [identifier]\n\
\x20block   identifier\n\
\x20unblock identifier\n\
\x20toggle  identifier\n\
\nFor more details see rfkill(8).\n",
    );
    text
}

/// `strtimeval_iso(&tv, ISO_TIMESTAMP_COMMA, ...)`: `2017-01-16
/// 09:43:41,432447+01:00`, `sec` and `usec` in the local zone.
fn timestamp(zone: &localtime::Zone, sec: i64, usec: u32) -> String {
    let tm = zone.localtime(sec, usec.saturating_mul(1000));
    // `get_gmtoff`, then the minutes always positive -- half an hour west
    // is `+00:30`, as util-linux has it.
    let tmin = tm.gmtoff / 60;
    let zhour = tmin / 60;
    let zmin = (tmin % 60).abs();
    format!(
        "{:4}-{:02}-{:02} {:02}:{:02}:{:02},{:06}{:+03}:{:02}",
        tm.year, tm.month, tm.day, tm.hour, tm.minute, tm.second, usec, zhour, zmin
    )
}

/// A descriptor of `/dev/rfkill`, closed when dropped.
struct Device(i32);

impl Drop for Device {
    fn drop(&mut self) {
        // `close (fd)`: its answer is looked at only by `rfkill_block`,
        // which calls [`Device::close`] instead.
        let _ = libcall::fd::close(self.0);
    }
}

impl Device {
    /// `close (fd)`, its answer kept: 0 or -1, as `rfkill_block` returns it.
    fn close(self) -> i32 {
        let fd = self.0;
        std::mem::forget(self);
        if libcall::fd::close(fd).is_ok() {
            0
        } else {
            -1
        }
    }
}

impl Rfkill {
    /// The program as it starts: `errno` as `setlocale` left it.
    fn new(short: Vec<u8>, errno: i32, dev: &str, sys: &Path) -> Self {
        Rfkill {
            short,
            out: Stdout::new(1),
            errno,
            sys_attr: Vec::new(),
            dev: dev.to_owned(),
            sys: sys.to_path_buf(),
            json: false,
            no_headings: false,
            raw: false,
            columns: Vec::new(),
            log: syslog_notice,
        }
    }

    /// `warnx (msg)`.
    fn warnx(&self, msg: &str) {
        warnx(&self.short, msg);
    }

    /// `warn (msg)`, with the `errno` of the call that failed.
    fn warn(&self, msg: &str, errno: i32) {
        warn(&self.short, msg, &std::io::Error::from_raw_os_error(errno));
    }

    /// `errtryhelp (EXIT_FAILURE)`.
    fn errtryhelp(&self) -> Stop {
        stderr_write(
            format!(
                "Try '{} --help' for more information.\n",
                shown(&self.short)
            )
            .as_bytes(),
        );
        Stop::Exit(1)
    }

    /// `xstrdup (str)` of a string that may be `NULL`: the assertion that
    /// fails for one, and the `abort` after it.
    fn xstrdup(&self, s: Option<&str>) -> Result<Vec<u8>, Stop> {
        match s {
            Some(s) => Ok(s.as_bytes().to_vec()),
            None => {
                stderr_write(
                    format!(
                        "{}: ./include/xalloc.h:68: xstrdup: Assertion `str' failed.\n",
                        shown(&self.short)
                    )
                    .as_bytes(),
                );
                Err(Stop::Abort)
            }
        }
    }

    /// `rfkill_open (rdonly, nonblock)`: the descriptor, or `-errno` after
    /// a warning.
    fn rfkill_open(&mut self, rdonly: bool, nonblock: bool) -> Result<Device, i32> {
        let flags = if rdonly {
            libcall::file::O_RDONLY
        } else {
            libcall::fd::O_RDWR
        };
        // `_PATH_DEV_RFKILL`, or a test's `/proc/self/fd/N`: no NUL in
        // either, so the empty fallback is never taken.
        let path = CString::new(self.dev.as_bytes()).unwrap_or_default();
        let fd = match libcall::file::openat(libcall::file::AT_FDCWD, &path, flags, 0) {
            Ok(fd) => Device(fd),
            Err(e) => {
                self.errno = e;
                self.warn(&format!("cannot open {}", self.dev), e);
                return Err(e.wrapping_neg());
            }
        };
        if nonblock && let Err(e) = libcall::file::set_status_flags(fd.0, libcall::file::O_NONBLOCK)
        {
            self.errno = e;
            self.warn(&format!("cannot set non-blocking {}", self.dev), e);
            return Err(e.wrapping_neg());
        }
        Ok(fd)
    }

    /// `rfkill_read_event (fd, event)`: `Ok(Some)` for an event, `Ok(None)`
    /// for "read again" (nothing there, or a short event), `Err(-errno)` for
    /// a failure.
    fn rfkill_read_event(&mut self, fd: &Device) -> Result<Option<Event>, i32> {
        let mut buf = [0u8; RFKILL_EVENT_SIZE_V1];
        let len = match libcall::fd::read(fd.0, &mut buf) {
            Ok(len) => len,
            Err(e) => {
                self.errno = e;
                if e == EAGAIN {
                    return Ok(None);
                }
                self.warn(&format!("cannot read {}", self.dev), e);
                return Err(e.wrapping_neg());
            }
        };
        if len < RFKILL_EVENT_SIZE_V1 {
            self.warnx(&format!(
                "wrong size of rfkill event: {len} < {RFKILL_EVENT_SIZE_V1}"
            ));
            return Ok(None);
        }
        Ok(Some(Event::from_bytes(buf)))
    }

    /// `rfkill_event ()`: every event, timestamped, until the device or
    /// standard output fails. Always -1.
    fn rfkill_event(&mut self) -> i32 {
        use libcall::fd::{POLLHUP, POLLIN, PollFd};
        let fd = match self.rfkill_open(true, false) {
            Ok(fd) => fd,
            Err(rc) => return rc,
        };
        let zone = localtime::Zone::from_env();
        let mut p = [
            PollFd {
                fd: fd.0,
                events: POLLIN | POLLHUP,
                revents: 0,
            },
            PollFd {
                fd: 1,
                events: 0,
                revents: 0,
            },
        ];
        // Interrupted by a signal only.
        loop {
            if let Err(e) = libcall::fd::poll(&mut p, -1) {
                self.errno = e;
                self.warn(&format!("failed to poll {}", self.dev), e);
                return -1;
            }
            if p[1].revents != 0 {
                // The read end of standard output has gone.
                return -1;
            }
            let event = if p[0].revents == 0 {
                None
            } else {
                match self.rfkill_read_event(&fd) {
                    Ok(event) => event,
                    Err(_) => return -1,
                }
            };
            let Some(event) = event else {
                continue;
            };
            let (sec, usec) = timeval(std::time::SystemTime::now());
            let date = timestamp(&zone, sec, usec);
            self.out.write(
                format!(
                    "{date}: idx {} type {} op {} soft {} hard {}\n",
                    event.idx, event.ty, event.op, event.soft, event.hard
                )
                .as_bytes(),
            );
            self.out.flush();
        }
    }

    /// `get_sys_attr (idx, attr)`: the first line of
    /// `/sys/class/rfkill/rfkillIDX/ATTR`, read into the one buffer every
    /// call shares -- left as it was when the file cannot be read.
    fn get_sys_attr(&mut self, idx: u32, attr: &str) -> Vec<u8> {
        let path = self.sys.join(format!("rfkill{idx}")).join(attr);
        match std::fs::File::open(&path) {
            Ok(file) => {
                // `fgets (name, 128, f)`: at most 127 bytes, to a newline.
                let mut line = Vec::new();
                let read = BufReader::new(file.take(127)).read_until(b'\n', &mut line);
                // A directory opens, and its read fails: `errno` is `EISDIR`.
                if let Err(e) = &read
                    && let Some(e) = e.raw_os_error()
                {
                    self.errno = e;
                }
                if matches!(read, Ok(n) if n > 0) {
                    let line = smartcols::mbs::c_str(&line);
                    let end = line.iter().position(|&b| b == b'\n').unwrap_or(line.len());
                    self.sys_attr = line.get(..end).unwrap_or_default().to_vec();
                }
            }
            Err(e) => {
                if let Some(e) = e.raw_os_error() {
                    self.errno = e;
                }
            }
        }
        self.sys_attr.clone()
    }

    /// `rfkill_id_to_type (s)`: a type's name, `all`, or a device's index
    /// whose `name` exists in sysfs. An index that is not a number ends the
    /// program, as `strtou32_or_err` does.
    fn rfkill_id_to_type(&mut self, s: &[u8]) -> Result<RfkillId, Stop> {
        match s.first() {
            Some(b'a'..=b'z') => {
                for t in &RFKILL_TYPE_STRINGS {
                    let Some(name) = t.name else {
                        break;
                    };
                    if name.as_bytes() == s {
                        return Ok(if name == "all" {
                            RfkillId::All
                        } else {
                            RfkillId::Type(t.ty)
                        });
                    }
                }
                Ok(RfkillId::Invalid)
            }
            Some(b'0'..=b'9') => {
                let index = match self.strtou32_or_err(s) {
                    Ok(index) => index,
                    Err(e) => {
                        let arg = os_from_bytes(s);
                        self.warnx(&num_error_message("invalid identifier", &arg, e));
                        return Err(Stop::Exit(1));
                    }
                };
                let name = self.sys.join(format!("rfkill{index}")).join("name");
                // `access (filename, F_OK)`: there, once links are followed.
                match std::fs::metadata(&name) {
                    Ok(_) => Ok(RfkillId::Index(index)),
                    Err(e) => {
                        if let Some(e) = e.raw_os_error() {
                            self.errno = e;
                        }
                        Ok(RfkillId::Invalid)
                    }
                }
            }
            _ => Ok(RfkillId::Invalid),
        }
    }

    /// `strtou32_or_err (s, ...)`'s conversion: `ul_strtou64`, which leaves
    /// `errno` 0 when it succeeds, then the range of a `uint32_t`.
    fn strtou32_or_err(&mut self, s: &[u8]) -> Result<u32, NumErr> {
        let wide = ulstrutils::ul_strtou64(s, 10)?;
        self.errno = 0;
        u32::try_from(wide).map_err(|_| NumErr::Range)
    }

    /// `fill_table_row (tb, event)`.
    fn fill_table_row(
        &mut self,
        tb: &mut Table,
        ids: &[(Col, smartcols::ColumnId)],
        event: &Event,
    ) -> Result<(), Stop> {
        let Ok(ln) = tb.new_line(None) else {
            self.warnx("failed to allocate output line");
            return Err(Stop::Exit(1));
        };
        for &(col, cl) in ids {
            let data = match col {
                Col::Device => self.get_sys_attr(event.idx, "name"),
                Col::Id => event.idx.to_string().into_bytes(),
                Col::Type => self.get_sys_attr(event.idx, "type"),
                Col::Desc => self.xstrdup(rfkill_type_to_desc(u32::from(event.ty)))?,
                Col::Soft => blocked(event.soft),
                Col::Hard => blocked(event.hard),
            };
            if tb.line_set_data(ln, cl, &data).is_err() {
                self.warnx("failed to add output data");
                return Err(Stop::Exit(1));
            }
        }
        Ok(())
    }

    /// The loop `rfkill_list_old`, `rfkill_list_fill` and `rfkill_toggle`
    /// share: the identifier checked (`None` names all), the device opened
    /// without waiting -- read-only to list, for writing too to toggle --
    /// and its events read until there are no more, `each` called for every
    /// one that adds a device the identifier names. `-EINVAL` for an
    /// identifier that names nothing, the open's or a read's `-errno`, else
    /// 0.
    fn for_each_matching(
        &mut self,
        param: Option<&[u8]>,
        rdonly: bool,
        mut each: impl FnMut(&mut Self, &Device, &Event, RfkillId) -> Result<(), Stop>,
    ) -> Result<i32, Stop> {
        let mut id = RfkillId::All;
        if let Some(param) = param {
            id = self.rfkill_id_to_type(param)?;
            if id == RfkillId::Invalid {
                self.warnx(&format!("invalid identifier: {}", shown(param)));
                return Ok(-EINVAL);
            }
        }
        let fd = match self.rfkill_open(rdonly, true) {
            Ok(fd) => fd,
            Err(rc) => return Ok(rc),
        };
        loop {
            match self.rfkill_read_event(&fd) {
                Err(e) => return Ok(e),
                // Nothing there, or a short event: read again -- unless
                // `errno` says `EAGAIN`, whichever call left it there.
                Ok(None) => {
                    if self.errno == EAGAIN {
                        return Ok(0);
                    }
                }
                Ok(Some(event)) => {
                    if event_match(&event, id) {
                        each(self, &fd, &event, id)?;
                    }
                }
            }
        }
    }

    /// `rfkill_list_old (param)`: the old format, one paragraph a device.
    fn rfkill_list_old(&mut self, param: Option<&[u8]>) -> Result<i32, Stop> {
        self.for_each_matching(param, true, |rf, _, event, _| {
            let name = rf.get_sys_attr(event.idx, "name");
            // Upstream falls back to sysfs's type name when there is no
            // description -- after `xstrdup` has already failed its
            // assertion on the `NULL`.
            let ty = rf.xstrdup(rfkill_type_to_desc(u32::from(event.ty)))?;
            let mut text = format!("{}: ", event.idx).into_bytes();
            text.extend_from_slice(&name);
            text.extend_from_slice(b": ");
            text.extend_from_slice(&ty);
            text.extend_from_slice(
                format!(
                    "\n\tSoft blocked: {}\n\tHard blocked: {}\n",
                    yes_no(event.soft),
                    yes_no(event.hard)
                )
                .as_bytes(),
            );
            rf.out.write(&text);
            Ok(())
        })
    }

    /// `rfkill_list_init (ctrl)`: the table and its columns.
    fn rfkill_list_init(&self) -> (Table, Vec<(Col, smartcols::ColumnId)>) {
        let mut tb = Table::new();
        tb.enable_json(self.json);
        tb.set_name(b"rfkilldevices");
        tb.enable_noheadings(self.no_headings);
        tb.enable_raw(self.raw);
        let mut ids = Vec::with_capacity(self.columns.len());
        for &col in &self.columns {
            let i = info(col);
            let cl = tb.new_column(i.name.as_bytes(), i.whint, i.flags);
            if self.json && col == Col::Id {
                // The column is the table's own.
                let _ = tb.column_set_json_type(cl, JsonType::Number);
            }
            ids.push((col, cl));
        }
        (tb, ids)
    }

    /// `rfkill_list_fill (ctrl, param)`: a row for each device asked for.
    fn rfkill_list_fill(
        &mut self,
        tb: &mut Table,
        ids: &[(Col, smartcols::ColumnId)],
        param: Option<&[u8]>,
    ) -> Result<i32, Stop> {
        self.for_each_matching(param, true, |rf, _, event, _| {
            rf.fill_table_row(tb, ids, event)
        })
    }

    /// `__rfkill_block (fd, id, block, param)`: one request written, and
    /// logged. Upstream's -1 for an invalid identifier, which both callers
    /// have already refused, and its 0 otherwise, are read by neither.
    fn rfkill_block_on(&mut self, fd: &Device, id: RfkillId, block: u8, param: &[u8]) {
        let mut event = Event {
            op: RFKILL_OP_CHANGE_ALL,
            soft: block,
            ..Event::default()
        };
        let message = match id {
            RfkillId::Invalid => {
                self.warnx(&format!("invalid identifier: {}", shown(param)));
                return;
            }
            RfkillId::Type(ty) => {
                event.ty = u8::try_from(ty).unwrap_or(u8::MAX);
                let mut m = b"type ".to_vec();
                m.extend_from_slice(param);
                m
            }
            RfkillId::Index(index) => {
                event.op = RFKILL_OP_CHANGE;
                event.idx = index;
                // `%d` of the index.
                format!("id {}", index.cast_signed()).into_bytes()
            }
            RfkillId::All => b"all".to_vec(),
        };
        match ulallio::write_all(fd.0, &event.to_bytes()) {
            Err(e) => {
                self.errno = e;
                self.warn(&format!("write failed: {}", self.dev), e);
            }
            Ok(()) => {
                // `write_all` sets `errno` to 0 before each write.
                self.errno = 0;
                let mut line = if block == 0 {
                    b"unblock".to_vec()
                } else {
                    b"block".to_vec()
                };
                line.extend_from_slice(b" set for ");
                line.extend_from_slice(&message);
                (self.log)(&line);
            }
        }
    }

    /// `rfkill_block (block, param)`.
    fn rfkill_block(&mut self, block: u8, param: &[u8]) -> Result<i32, Stop> {
        let id = self.rfkill_id_to_type(param)?;
        if id == RfkillId::Invalid {
            self.warnx(&format!("invalid identifier: {}", shown(param)));
            return Ok(-EINVAL);
        }
        let fd = match self.rfkill_open(false, false) {
            Ok(fd) => fd,
            Err(rc) => return Ok(rc),
        };
        self.rfkill_block_on(&fd, id, block, param);
        Ok(fd.close())
    }

    /// `rfkill_toggle (param)`: each matching device's soft block, turned
    /// over -- by a request of the identifier's kind, for each.
    fn rfkill_toggle(&mut self, param: &[u8]) -> Result<i32, Stop> {
        self.for_each_matching(Some(param), false, |rf, fd, event, id| {
            let block = u8::from(event.soft == 0);
            rf.rfkill_block_on(fd, id, block, param);
            Ok(())
        })
    }

    /// `main()`, to the point of its `return`.
    fn run(&mut self, argv: &[OsString]) -> Stop {
        let arg0 = argv
            .first()
            .map_or(OsStr::new("rfkill"), OsString::as_os_str);
        let mut act = Action::List;
        let mut list_all = false;
        let mut outarg: Option<Vec<u8>> = None;
        let mut excl_st = [0i32; 1];
        let mut operands: Vec<Vec<u8>> = Vec::new();

        let own = argv.get(1..).unwrap_or_default();
        for item in RFKILL.parse(own, SHORTS, LONGS) {
            let opt = match item {
                Ok(opt) => opt,
                Err(e) => {
                    // glibc names the program by argv[0] as given.
                    stderr_write(
                        format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes(),
                    );
                    return self.errtryhelp();
                }
            };
            let Some((c, value)) = OPTIONS.code::<i32>(&opt) else {
                if let Opt::Operand(w) = opt {
                    operands.push(os_bytes(w).into_owned());
                }
                continue;
            };
            if let Some(msg) =
                err_exclusive_options(c, &EXCL, &mut excl_st, |c| OPTIONS.name_of(c), &self.short)
            {
                stderr_write(msg.as_bytes());
                return Stop::Exit(1);
            }
            let arg = value
                .as_deref()
                .map(os_bytes)
                .unwrap_or_default()
                .into_owned();
            match c {
                c if c == i32::from(b'J') => self.json = true,
                c if c == i32::from(b'n') => self.no_headings = true,
                c if c == i32::from(b'o') => outarg = Some(arg),
                OPT_LIST_TYPES => list_all = true,
                c if c == i32::from(b'r') => self.raw = true,
                c if c == i32::from(b'V') => {
                    let mut line = self.short.clone();
                    line.extend_from_slice(b" from util-linux 2.39.3\n");
                    self.out.write(&line);
                    return Stop::Exit(0);
                }
                c if c == i32::from(b'h') => {
                    let text = usage(&self.short);
                    self.out.write(&text);
                    return Stop::Exit(0);
                }
                _ => return self.errtryhelp(),
            }
        }

        let mut args = operands.as_slice();
        if let Some((first, rest)) = args.split_first() {
            let Some(a) = string_to_action(first) else {
                return self.errtryhelp();
            };
            act = a;
            args = rest;
            // For backward compatibility the old format, if "list" is named
            // and --output is not.
            if outarg.is_none() && act == Action::List {
                act = Action::ListOld;
            }
        }

        let ret = match self.act(act, args, list_all, outarg.as_deref()) {
            Ok(ret) => ret,
            Err(stop) => return stop,
        };
        Stop::Exit(u8::from(ret != 0))
    }

    /// `main()`'s `switch (act)`: what it ORs into `ret`.
    fn act(
        &mut self,
        act: Action,
        args: &[Vec<u8>],
        list_all: bool,
        outarg: Option<&[u8]>,
    ) -> Result<i32, Stop> {
        let mut ret = 0i32;
        match act {
            Action::ListOld => {
                if args.is_empty() {
                    ret |= self.rfkill_list_old(None)?;
                } else {
                    for a in args {
                        ret |= self.rfkill_list_old(Some(a))?;
                    }
                }
            }
            Action::List => {
                self.columns = vec![Col::Id, Col::Type, Col::Device];
                if list_all {
                    self.columns.push(Col::Desc);
                }
                self.columns.push(Col::Soft);
                self.columns.push(Col::Hard);
                if let Some(list) = outarg {
                    let short = self.short.clone();
                    if string_add_to_idarray(list, &mut self.columns, MAX_COLUMNS, |name, rest| {
                        column_name_to_id(name, rest, &short)
                    })
                    .is_err()
                    {
                        return Err(Stop::Exit(1));
                    }
                }
                let (mut tb, ids) = self.rfkill_list_init();
                if args.is_empty() {
                    ret |= self.rfkill_list_fill(&mut tb, &ids, None)?;
                } else {
                    for a in args {
                        ret |= self.rfkill_list_fill(&mut tb, &ids, Some(a))?;
                    }
                }
                // `scols_print_table`, whose answer is not looked at.
                let mut text = Vec::new();
                let _ = tb.print_into(&mut text);
                self.out.write(&text);
            }
            Action::Event => ret = self.rfkill_event(),
            Action::Help => {
                let text = usage(&self.short);
                self.out.write(&text);
                return Err(Stop::Exit(0));
            }
            Action::Block | Action::Unblock => {
                let block = u8::from(act == Action::Block);
                for a in args {
                    ret |= self.rfkill_block(block, a)?;
                }
            }
            Action::Toggle => {
                for a in args {
                    ret |= self.rfkill_toggle(a)?;
                }
            }
        }
        Ok(ret)
    }
}

/// `blocked` or `unblocked`, for the table.
fn blocked(state: u8) -> Vec<u8> {
    if state == 0 {
        b"unblocked".to_vec()
    } else {
        b"blocked".to_vec()
    }
}

/// `yes` or `no`, for the old format.
fn yes_no(state: u8) -> &'static str {
    if state == 0 { "no" } else { "yes" }
}

/// `gettimeofday`'s `struct timeval` of `t`: seconds and microseconds since
/// the epoch -- the seconds negative, and the microseconds still counting
/// up, for a time before 1970.
fn timeval(t: std::time::SystemTime) -> (i64, u32) {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            d.subsec_micros(),
        ),
        Err(e) => {
            let d = e.duration();
            let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
            match d.subsec_micros() {
                0 => (secs.saturating_neg(), 0),
                us => (
                    secs.saturating_neg().saturating_sub(1),
                    1_000_000u32.saturating_sub(us),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests;
