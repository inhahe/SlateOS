//! Report the memory map of each process named: procps-ng 4.0.4's, ported.
//! Ubuntu ships `src/pmap.c` unpatched, so this is the program Ubuntu
//! ships too.
//!
//! ```text
//! pmap [options] PID [PID ...]
//! ```
//!
//! Four formats over `/proc/PID/maps` and `smaps`: one line a mapping with
//! its size (the default), `-x` with the resident and dirty sizes, `-d`
//! with the offset and device, and `-X`/`-XX`/`-c`, every field `smaps`
//! has -- or the ones the rc file names -- in columns measured on a first
//! pass over the file and printed on a second.
//!
//! A transcription. What upstream does and this keeps:
//!
//! - With no argument at all the usage goes to standard error before any
//!   option is looked at; `-r` is warned about and ignored; `-X` twice is
//!   `-XX`.
//! - A process is a number as `strtoul` reads it at base 0 -- `0x1f` and
//!   `017` are processes 31 and 15 -- from 1 to `0x7fffffff`, or its
//!   `/proc/` path; `/proc/` followed by anything but a digit is skipped
//!   without a word, and anything else that is not such a number is the
//!   usage, status 1.
//! - The library selects at most 255 processes and at least one: past that,
//!   or with every argument skipped, `library failed pids statistics`.
//! - A process that is not there is left out and the status gets 42; one
//!   whose `maps` cannot be opened still has its first line printed, and the
//!   status gets 1.
//! - Each file is read with `fgets` into 1024 bytes, so a line longer than
//!   1023 arrives in pieces, and a piece is scanned as if it were a line:
//!   the address and the permissions it fails to supply are the last ones
//!   read (see [`cfile`]).
//! - `-x`'s line for a mapping is printed when its `Swap:` line is read.
//! - `-X`, `-XX` and `-c` read `smaps` twice, and the list of fields their
//!   first process made is the one every later process is held to: a field
//!   it does not have, or has in another order, is `inconsistent detail
//!   field`. Its column widths only grow from one process to the next.
//! - A shared-memory segment is named by its id, `[ shmid=0x… ]`: `pmap`
//!   makes a segment of its own and finds it in its own map to learn the
//!   device number the kernel gives them all.
//! - Standard output is closed as procps' own `close_stdout` closes it.
//!
//! # Deliberately different
//!
//! - `-V`/`--version` names this build: `pmap from SlateOS coreutils 0.1.0`.
//! - An argument echoed back in a diagnostic cannot rewrite the terminal:
//!   `-A`'s, between apostrophes, goes through `quoteaf`, which prints the
//!   same `'abc'` for anything printable without an apostrophe in it (`free`'s
//!   divergence 6).

mod cfile;
mod extended;
mod rc;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::pwcache::Pwcache;
use coreutils::procps::readproc::{Fill, Proc, Reader};
use coreutils::procps::scanf::Scan;
use coreutils::quote::{os_bytes, os_from_bytes, quoteaf};
use coreutils::stdfd::{self, Stream};

use cfile::CFile;

/// The parser. Its name is never printed: getopt's complaints carry
/// `argv[0]`.
const PMAP: Program = Program::new("pmap", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "xXrdqA:hVcC:nN:p";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("extended", Takes::Nothing),
    ("device", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("range", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("read-rc", Takes::Nothing),
    ("read-rc-from", Takes::Required),
    ("create-rc", Takes::Nothing),
    ("create-rc-to", Takes::Required),
    ("show-path", Takes::Nothing),
];

/// The usage text after the program's name.
const USAGE_REST: &str = concat!(
    " [options] PID [PID ...]\n",
    "\n",
    "Options:\n",
    " -x, --extended              show details\n",
    " -X                          show even more details\n",
    "            WARNING: format changes according to /proc/PID/smaps\n",
    " -XX                         show everything the kernel provides\n",
    " -c, --read-rc               read the default rc\n",
    " -C, --read-rc-from=<file>   read the rc from file\n",
    " -n, --create-rc             create new default rc\n",
    " -N, --create-rc-to=<file>   create new rc to file\n",
    "            NOTE: pid arguments are not allowed with -n, -N\n",
    " -d, --device                show the device format\n",
    " -q, --quiet                 do not display header and footer\n",
    " -p, --show-path             show path in the mapping\n",
    " -A, --range=<low>[,<high>]  limit results to the given range\n",
    "\n",
    " -h, --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "For more details see pmap(1).\n",
);

/// `FILL_ID_MAX`: the most processes the library selects at once.
const FILL_ID_MAX: usize = 255;

/// `mapbuf`'s size.
const MAPBUF: usize = 1024;

/// The column headings, as `nls_initialize` names them.
const NLS_ADDRESS: &[u8] = b"Address";
const NLS_OFFSET: &[u8] = b"Offset";
const NLS_DEVICE: &[u8] = b"Device";
const NLS_MAPPING: &[u8] = b"Mapping";
const NLS_PERM: &[u8] = b"Perm";
const NLS_INODE: &[u8] = b"Inode";
const NLS_KBYTES: &[u8] = b"Kbytes";
const NLS_MODE: &[u8] = b"Mode";
const NLS_RSS: &[u8] = b"RSS";
const NLS_DIRTY: &[u8] = b"Dirty";

/// The options, as `main`'s flags hold them.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct Options {
    /// `-c`.
    read_rc: bool,
    /// `-C`.
    read_rc_from: bool,
    /// `-d`.
    device: bool,
    /// `-n`.
    create_rc: bool,
    /// `-N`.
    create_rc_to: bool,
    /// `-q`.
    quiet: bool,
    /// `-x`.
    extended: bool,
    /// `-X`, counted: 2 is `-XX`.
    more: u32,
}

/// Everything upstream keeps in globals.
pub struct Pmap {
    /// `program_invocation_short_name`.
    name: Vec<u8>,
    out: Stream,
    o: Options,
    /// `map_desc_showpath`.
    showpath: bool,
    range_low: u64,
    range_high: u64,
    /// `shm_minor`: the device minor of shared-memory mappings, or `~0u`.
    shm_minor: u32,
    /// `cnf_listhead`: the fields the rc file names.
    rc_fields: Vec<Vec<u8>>,
    /// `listhead`: the `-X` columns, kept from one process to the next.
    list: Vec<extended::Node>,
    /// `start_To_Avoid_Warning`: the last mapping's start, kept from one line
    /// -- and one process -- to the next.
    start: u64,
    /// The `/proc` the processes are read from.
    root: PathBuf,
    utf8: bool,
}

/// `program_invocation_short_name`: what follows `argv[0]`'s last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or(argv0)
}

/// `isprint` in the C and UTF-8 locales: a byte of printable ASCII.
fn is_print(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// The bytes before a C string's NUL.
fn c_str(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b.get(..end).unwrap_or_default()
}

/// `printf ("%*s")` / `("%-*s")`: `s` padded with spaces to `width`.
fn padded(s: &[u8], width: i32, left: bool) -> Vec<u8> {
    let pad = usize::try_from(width).unwrap_or(0).saturating_sub(s.len());
    let mut v = Vec::with_capacity(s.len().saturating_add(pad));
    if left {
        v.extend_from_slice(s);
        v.resize(v.len().saturating_add(pad), b' ');
    } else {
        v.resize(pad, b' ');
        v.extend_from_slice(s);
    }
    v
}

/// `printf ("%0*lx")`: lower-case hexadecimal, zero-padded to `width`.
fn hex_zero(v: u64, width: i32) -> Vec<u8> {
    let w = usize::try_from(width).unwrap_or(0);
    format!("{v:0w$x}").into_bytes()
}

/// `printf ("%*lu")` and kin: a number right-aligned in `width`.
fn num(v: impl std::fmt::Display, width: i32) -> Vec<u8> {
    let w = usize::try_from(width).unwrap_or(0);
    format!("{v:>w$}").into_bytes()
}

/// `integer_width`: how many digits `number` has, 1 for 0.
fn integer_width(number: u64) -> i32 {
    let mut result = i32::from(number == 0);
    let mut n = number;
    while n != 0 {
        result = result.saturating_add(1);
        n /= 10;
    }
    result
}

/// The low 32 bits, as C's assignment to an `unsigned int` keeps them.
fn low32(v: u64) -> u32 {
    u32::try_from(v & u64::from(u32::MAX)).unwrap_or(u32::MAX)
}

impl Pmap {
    fn new(argv0: &[u8]) -> Self {
        Self {
            name: short_name(argv0).to_vec(),
            out: Stream::stdout(),
            o: Options::default(),
            showpath: false,
            range_low: 0,
            range_high: u64::MAX,
            shm_minor: u32::MAX,
            rc_fields: Vec::new(),
            list: Vec::new(),
            start: 0,
            root: PathBuf::from("/proc"),
            utf8: coreutils::locale::ctype_is_utf8(),
        }
    }

    /// Bytes to standard output. `Stream` never fails a write: a failure is
    /// kept for `close_stdout_procps`, as stdio's error flag is.
    fn put(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }

    /// `xwarnx`, glibc's `error (0, 0, …)`: standard output delivered first,
    /// then `pmap: MESSAGE`.
    fn warnx(&mut self, msg: &[u8]) {
        let mut m = self.name.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(msg);
        m.push(b'\n');
        stdfd::diag_bytes(&m);
    }

    /// `xerrx (EXIT_FAILURE, …)`: [`Pmap::warnx`], and the status.
    fn errx(&mut self, msg: &[u8]) -> u8 {
        self.warnx(msg);
        1
    }

    /// `usage (out)`, and the status `exit` is then given.
    fn usage(&mut self, to_stdout: bool) -> u8 {
        let mut text = b"\nUsage:\n ".to_vec();
        text.extend_from_slice(&self.name);
        text.extend_from_slice(USAGE_REST.as_bytes());
        if to_stdout {
            self.put(&text);
            0
        } else {
            // `fputs (…, stderr)`, which flushes nothing; nothing is waiting
            // on standard output by the time it is called.
            stdfd::diag_bytes_ahead_of_stdout(&text);
            1
        }
    }

    /// `justify_print`: `s` in a column of `width`, then a space -- or, for
    /// a width below 1, `puts (s)`. The width the column took.
    fn justify_print(&mut self, s: &[u8], width: i32, right: bool) -> i32 {
        if width < 1 {
            self.put(s);
            self.put(b"\n");
            return width;
        }
        let len = i32::try_from(s.len()).unwrap_or(i32::MAX);
        let width = width.max(len);
        let mut v = padded(s, width, !right);
        v.push(b' ');
        self.put(&v);
        width
    }

    /// `range_arguments`: `-A LOW[,HIGH]`, each half hexadecimal as
    /// `strtoul` reads it, an empty half left as it was, and one number
    /// alone both halves.
    fn range_arguments(&mut self, arg: &[u8]) -> Result<(), u8> {
        let (first, second) = match arg.iter().position(|&c| c == b',') {
            Some(comma) => (
                arg.get(..comma).unwrap_or_default(),
                arg.get(comma.saturating_add(1)..).unwrap_or_default(),
            ),
            None => (arg, arg),
        };
        let mut rest_first: &[u8] = first;
        let mut rest_second: &[u8] = second;
        if !first.is_empty() {
            let (v, used) = cstrtol::strtoul(first, 16);
            self.range_low = v;
            rest_first = first.get(used..).unwrap_or_default();
        }
        if !second.is_empty() {
            let (v, used) = cstrtol::strtoul(second, 16);
            self.range_high = v;
            rest_second = second.get(used..).unwrap_or_default();
        }
        if !rest_first.is_empty() || !rest_second.is_empty() {
            let mut m = b"failed to parse argument: ".to_vec();
            m.extend_from_slice(quoteaf(arg).as_bytes());
            return Err(self.errx(&m));
        }
        Ok(())
    }

    /// `is_enabled`: whether a column is shown.
    fn is_enabled(&self, s: &[u8]) -> bool {
        if self.o.more == 1 {
            return !is_unimportant(s);
        }
        if self.o.read_rc {
            return self.rc_fields.iter().any(|f| f == s);
        }
        true
    }

    /// `get_default_rc_filename`: `$HOME/.NAMErc`, or `None` -- said -- when
    /// there is no `HOME`.
    fn default_rc_filename(&mut self) -> Option<Vec<u8>> {
        let Some(home) = std::env::var_os("HOME") else {
            self.warnx(b"HOME variable undefined");
            return None;
        };
        let mut p = os_bytes(&home).into_owned();
        p.extend_from_slice(b"/.");
        p.extend_from_slice(&self.name);
        p.extend_from_slice(b"rc");
        Some(p)
    }

    /// `~/.NAMErc`, as the messages about it spell it.
    fn tilde_rc(&self) -> Vec<u8> {
        let mut t = b"~/.".to_vec();
        t.extend_from_slice(&self.name);
        t.extend_from_slice(b"rc");
        t
    }

    /// `discover_shm_minor`: make a shared-memory segment, find it in this
    /// process's own map, and keep the device minor it is shown with.
    fn discover_shm_minor(&mut self) {
        let Ok(mut maps) = CFile::open(Path::new("/proc/self/maps")) else {
            return;
        };
        let Ok(id) = libcall::shm::shmget(
            libcall::shm::IPC_PRIVATE,
            42,
            libcall::shm::IPC_CREAT | 0o666,
        ) else {
            return;
        };
        if let Ok(seg) = libcall::shm::attach_read_only(id) {
            let addr = u64::try_from(seg.address()).unwrap_or(u64::MAX);
            while let Some(raw) = maps.fgets(256) {
                let line = c_str(&raw);
                let m = scan_mapping(line);
                if m.count < 6 {
                    continue;
                }
                let mut text: Vec<u8> = match line.iter().position(|&c| c == b'\n') {
                    Some(nl) => line.get(..nl).unwrap_or_default().to_vec(),
                    None => line.to_vec(),
                };
                for b in &mut text {
                    if !is_print(*b) {
                        *b = b'?';
                    }
                }
                if m.start > addr || m.dev_major != 0 || m.perms.get(3) != Some(&b's') {
                    continue;
                }
                if contains(&text, b"/SYSV") {
                    self.shm_minor = m.dev_minor;
                    break;
                }
            }
            if let Err(errno) = libcall::shm::detach(seg) {
                perror(b"shared memory detach", errno);
            }
        }
        // EINVAL: already gone, which is what was wanted.
        if let Err(errno) = libcall::shm::remove(id)
            && errno != EINVAL
        {
            perror(b"shared memory remove", errno);
        }
    }

    /// `mapping_name`: what the last column says of a mapping.
    #[allow(clippy::too_many_arguments)]
    fn mapping_name(
        &self,
        p: &Proc,
        addr: u64,
        len: u64,
        line: &[u8],
        dev_major: u32,
        dev_minor: u32,
        inode: u64,
    ) -> Vec<u8> {
        if dev_major == 0 && dev_minor == self.shm_minor && contains(line, b"/SYSV") {
            return format!("  [ shmid=0x{inode:x} ]").into_bytes();
        }
        if let Some(last) = line.iter().rposition(|&c| c == b'/') {
            if self.showpath {
                let first = line.iter().position(|&c| c == b'/').unwrap_or(last);
                return line.get(first..).unwrap_or_default().to_vec();
            }
            return if last.saturating_add(1) < line.len() {
                line.get(last.saturating_add(1)..)
                    .unwrap_or_default()
                    .to_vec()
            } else {
                b"/".to_vec()
            };
        }
        if p.start_stack >= addr && p.start_stack <= addr.wrapping_add(len) {
            b"  [ stack ]".to_vec()
        } else {
            b"  [ anon ]".to_vec()
        }
    }

    /// `one_proc`: one process's map, and 1 when its file would not open.
    #[allow(clippy::too_many_lines)]
    fn one_proc(&mut self, p: &Proc) -> Result<u8, u8> {
        let tgid = p.tgid.cast_unsigned();
        let mut first = format!("{tgid}:   ").into_bytes();
        first.extend_from_slice(p.cmdline.as_deref().unwrap_or(b"(null)"));
        first.push(b'\n');
        self.put(&first);

        let smaps = self.o.extended || self.o.more > 0 || self.o.read_rc;
        let file = self
            .root
            .join(format!("{tgid}/{}", if smaps { "smaps" } else { "maps" }));
        let Ok(mut f) = CFile::open(&file) else {
            return Ok(1);
        };
        if self.o.more > 0 || self.o.read_rc {
            self.print_extended_maps(&mut f)?;
            return Ok(0);
        }

        let mut total_shared: u64 = 0;
        let mut total_private_readonly: u64 = 0;
        let mut total_private_writeable: u64 = 0;
        let mut diff: u64 = 0;
        let mut end: u64 = 0;
        let mut perms = [0u8; 32];
        let mut cp2: Option<Vec<u8>> = None;
        let mut rss: u64 = 0;
        let mut private_dirty: u64 = 0;
        let mut shared_dirty: u64 = 0;
        let mut total_rss: u64 = 0;
        let mut total_private_dirty: u64 = 0;
        let mut total_shared_dirty: u64 = 0;
        let (mut maxw1, mut maxw2, mut maxw3, mut maxw4, mut maxw5) = (0, 0, 0, 0, 0);
        // Declared inside upstream's loop and never initialised: a line that
        // fails to supply them prints what the last one did.
        let (mut file_offset, mut inode, mut dev_major, mut dev_minor) = (0u64, 0u64, 0u32, 0u32);
        let q = self.o.quiet;

        if self.o.extended {
            maxw1 = 16;
            (maxw2, maxw3, maxw4) = (7, 7, 7);
            maxw5 = 5;
            if !q {
                maxw1 = self.justify_print(NLS_ADDRESS, maxw1, false);
                maxw2 = self.justify_print(NLS_KBYTES, maxw2, true);
                maxw3 = self.justify_print(NLS_RSS, maxw3, true);
                maxw4 = self.justify_print(NLS_DIRTY, maxw4, true);
                maxw5 = self.justify_print(NLS_MODE, maxw5, false);
                self.justify_print(NLS_MAPPING, 0, false);
            }
        }
        if self.o.device {
            (maxw1, maxw2, maxw3, maxw4, maxw5) = (16, 7, 5, 16, 9);
            if !q {
                maxw1 = self.justify_print(NLS_ADDRESS, maxw1, false);
                maxw2 = self.justify_print(NLS_KBYTES, maxw2, true);
                maxw3 = self.justify_print(NLS_MODE, maxw3, false);
                maxw4 = self.justify_print(NLS_OFFSET, maxw4, false);
                maxw5 = self.justify_print(NLS_DEVICE, maxw5, false);
                self.justify_print(NLS_MAPPING, 0, false);
            }
        }

        while let Some(raw) = f.fgets(MAPBUF) {
            let mapbuf = c_str(&raw);
            if mapbuf.first().is_some_and(u8::is_ascii_uppercase) {
                // A key: `%20[^:]: %llu`.
                let mut s = Scan::new(mapbuf);
                let key = s.scanset(20, |c| c != b':');
                let value = key.and_then(|_| s.lit(b": ")).and_then(|()| s.ulong());
                if let (Some(key), Some(value)) = (key, value) {
                    match key {
                        b"Rss" => {
                            rss = value;
                            total_rss = total_rss.wrapping_add(value);
                        }
                        b"Shared_Dirty" => {
                            shared_dirty = value;
                            total_shared_dirty = total_shared_dirty.wrapping_add(value);
                        }
                        b"Private_Dirty" => {
                            private_dirty = value;
                            total_private_dirty = total_private_dirty.wrapping_add(value);
                        }
                        b"Swap" => {
                            if let Some(name) = cp2.take() {
                                let mut l = hex_zero(self.start, maxw1);
                                l.push(b' ');
                                l.extend(num(diff >> 10, maxw2));
                                l.push(b' ');
                                l.extend(num(rss, maxw3));
                                l.push(b' ');
                                l.extend(num(private_dirty.wrapping_add(shared_dirty), maxw4));
                                l.push(b' ');
                                l.extend(padded(c_str(&perms), maxw5, false));
                                l.push(b' ');
                                l.extend_from_slice(&name);
                                l.push(b'\n');
                                self.put(&l);
                            }
                            (rss, shared_dirty, private_dirty) = (0, 0, 0);
                            (diff, end) = (0, 0);
                            perms[0] = 0;
                        }
                        _ => {}
                    }
                }
                continue;
            }

            let m = scan_mapping(mapbuf);
            if let Some(v) = m.start_set {
                self.start = v;
            }
            if let Some(v) = m.end_set {
                end = v;
            }
            if let Some(w) = m.perms_set {
                // `%31s`: the word and its NUL; what lay beyond stays.
                let n = w.len().min(31);
                if let Some(dst) = perms.get_mut(..n) {
                    dst.copy_from_slice(w.get(..n).unwrap_or_default());
                }
                if let Some(nul) = perms.get_mut(n) {
                    *nul = 0;
                }
            }
            if let Some(v) = m.offset_set {
                file_offset = v;
            }
            if let Some(v) = m.major_set {
                dev_major = v;
            }
            if let Some(v) = m.minor_set {
                dev_minor = v;
            }
            if let Some(v) = m.inode_set {
                inode = v;
            }

            if end.wrapping_sub(1) < self.range_low {
                continue;
            }
            if self.range_high < self.start {
                break;
            }

            let mut line: Vec<u8> = match mapbuf.iter().position(|&c| c == b'\n') {
                Some(nl) => mapbuf.get(..nl).unwrap_or_default().to_vec(),
                None => mapbuf.to_vec(),
            };
            for b in &mut line {
                if !is_print(*b) {
                    *b = b'?';
                }
            }

            diff = end.wrapping_sub(self.start);
            if perms[3] == b's' {
                total_shared = total_shared.wrapping_add(diff);
            }
            if perms[3] == b'p' {
                perms[3] = b'-';
                if perms[1] == b'w' {
                    total_private_writeable = total_private_writeable.wrapping_add(diff);
                } else {
                    total_private_readonly = total_private_readonly.wrapping_add(diff);
                }
            }
            // Solaris's `R` column: always `-` here.
            perms[4] = b'-';
            perms[5] = 0;

            let name = self.mapping_name(p, self.start, diff, &line, dev_major, dev_minor, inode);
            if self.o.extended {
                // Printed with the keys, at `Swap:`.
                cp2 = Some(name);
                continue;
            }
            if self.o.device {
                let mut l = hex_zero(self.start, maxw1);
                l.push(b' ');
                l.extend(num(diff >> 10, maxw2));
                l.push(b' ');
                l.extend(padded(c_str(&perms), maxw3, false));
                l.push(b' ');
                l.extend(hex_zero(file_offset, maxw4));
                l.push(b' ');
                // `%*.*s` of " " at `maxw5 - 9` for both width and
                // precision: that many blanks, which is none, `maxw5` being 9.
                let gap = usize::try_from(maxw5.saturating_sub(9)).unwrap_or(0);
                l.extend(std::iter::repeat_n(b' ', gap));
                l.extend(format!("{dev_major:03x}:{dev_minor:05x} ").into_bytes());
                l.extend_from_slice(&name);
                l.push(b'\n');
                self.put(&l);
            } else {
                let mut l = format!("{:016x} {:>6}K ", self.start, diff >> 10).into_bytes();
                l.extend_from_slice(c_str(&perms));
                l.push(b' ');
                l.extend_from_slice(&name);
                l.push(b'\n');
                self.put(&l);
            }
        }
        drop(f);

        if !q {
            let mapped = total_shared
                .wrapping_add(total_private_writeable)
                .wrapping_add(total_private_readonly);
            if self.o.extended {
                self.justify_print(b"----------------", maxw1, false);
                self.justify_print(b"-------", maxw2, true);
                self.justify_print(b"-------", maxw3, true);
                self.justify_print(b"-------", maxw4, true);
                self.put(b"\n");
                let mut l = padded(b"total kB", maxw1, true);
                l.push(b' ');
                l.extend(num(signed(mapped >> 10), maxw2));
                l.push(b' ');
                l.extend(num(total_rss, maxw3));
                l.push(b' ');
                l.extend(num(
                    total_shared_dirty.wrapping_add(total_private_dirty),
                    maxw4,
                ));
                l.push(b'\n');
                self.put(&l);
            }
            if self.o.device {
                let l = format!(
                    "mapped: {}K    writeable/private: {}K    shared: {}K\n",
                    signed(mapped >> 10),
                    signed(total_private_writeable >> 10),
                    signed(total_shared >> 10)
                );
                self.put(l.as_bytes());
            }
            if !self.o.extended && !self.o.device {
                let l = format!(" total {:>16}K\n", signed(mapped >> 10));
                self.put(l.as_bytes());
            }
        }
        Ok(0)
    }

    /// `main`, after `atexit (close_stdout)`: the status it exits with.
    #[allow(clippy::too_many_lines)]
    fn run(&mut self, argv: &[OsString]) -> u8 {
        if argv.len() < 2 {
            return self.usage(false);
        }
        let argv0 = argv
            .first()
            .map(|a| os_bytes(a).into_owned())
            .unwrap_or_default();
        let words = argv.get(1..).unwrap_or(&[]);
        let mut rc_filename: Option<Vec<u8>> = None;
        let mut operands: Vec<Vec<u8>> = Vec::new();
        for item in PMAP.parse(words, SHORT_OPTIONS, LONG_OPTIONS) {
            match item {
                Ok(Opt::Short(b'x', _) | Opt::Long("extended", _)) => self.o.extended = true,
                Ok(Opt::Short(b'X', _)) => self.o.more = self.o.more.saturating_add(1),
                Ok(Opt::Short(b'r', _)) => {
                    self.warnx(b"option -r is ignored as SunOS compatibility");
                }
                Ok(Opt::Short(b'd', _) | Opt::Long("device", _)) => self.o.device = true,
                Ok(Opt::Short(b'q', _) | Opt::Long("quiet", _)) => self.o.quiet = true,
                Ok(Opt::Short(b'A', Some(v)) | Opt::Long("range", Some(v))) => {
                    if let Err(status) = self.range_arguments(&os_bytes(&v)) {
                        return status;
                    }
                }
                Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => return self.usage(true),
                Ok(Opt::Short(b'V', _) | Opt::Long("version", _)) => {
                    // `printf (PROCPS_NG_VERSION)`; see "Deliberately
                    // different".
                    let mut v = self.name.clone();
                    v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                    self.put(&v);
                    return 0;
                }
                Ok(Opt::Short(b'c', _) | Opt::Long("read-rc", _)) => self.o.read_rc = true,
                Ok(Opt::Short(b'C', Some(v)) | Opt::Long("read-rc-from", Some(v))) => {
                    self.o.read_rc_from = true;
                    rc_filename = Some(os_bytes(&v).into_owned());
                }
                Ok(Opt::Short(b'n', _) | Opt::Long("create-rc", _)) => self.o.create_rc = true,
                Ok(Opt::Short(b'N', Some(v)) | Opt::Long("create-rc-to", Some(v))) => {
                    self.o.create_rc_to = true;
                    rc_filename = Some(os_bytes(&v).into_owned());
                }
                Ok(Opt::Short(b'p', _) | Opt::Long("show-path", _)) => self.showpath = true,
                Ok(Opt::Operand(o)) => operands.push(os_bytes(o).into_owned()),
                // Nothing else is in the tables.
                Ok(Opt::Short(..) | Opt::Long(..)) => return self.usage(false),
                Err(e) => {
                    // getopt's own complaint, `argv[0]` and all, then the
                    // usage.
                    let mut m = argv0.clone();
                    m.extend_from_slice(b": ");
                    m.extend_from_slice(e.sentence.as_bytes());
                    m.push(b'\n');
                    stdfd::diag_bytes_ahead_of_stdout(&m);
                    return self.usage(false);
                }
            }
        }

        let o = &self.o;
        let modes = [
            o.read_rc,
            o.read_rc_from,
            o.device,
            o.create_rc,
            o.create_rc_to,
            o.extended,
            o.more > 0,
        ];
        if modes.iter().filter(|&&m| m).count() > 1 {
            return self.errx(b"options -c, -C, -d, -n, -N, -x, -X are mutually exclusive");
        }
        let creating = self.o.create_rc || self.o.create_rc_to;
        if creating && (self.o.quiet || self.showpath) {
            return self.errx(b"options -p, -q are mutually exclusive with -n, -N");
        }
        if creating && !operands.is_empty() {
            return self.errx(b"too many arguments");
        }

        if self.o.create_rc_to {
            let path = os_from_bytes(rc_filename.as_deref().unwrap_or_default());
            let mut warnings = Vec::new();
            let made = rc::create(Path::new(&path), &mut |w| warnings.push(w));
            for w in warnings {
                self.warnx(w.as_bytes());
            }
            if made {
                self.warnx(b"rc file successfully created, feel free to edit the content");
                return 0;
            }
            return self.errx(b"couldn't create the rc file");
        }
        if self.o.create_rc {
            let Some(path) = self.default_rc_filename() else {
                return 1;
            };
            let mut warnings = Vec::new();
            let made = rc::create(Path::new(&os_from_bytes(&path)), &mut |w| warnings.push(w));
            for w in warnings {
                self.warnx(w.as_bytes());
            }
            let mut m = self.tilde_rc();
            if made {
                m.extend_from_slice(b" file successfully created, feel free to edit the content");
                self.warnx(&m);
                return 0;
            }
            let mut e = b"couldn't create ".to_vec();
            e.extend_from_slice(&m);
            return self.errx(&e);
        }

        if operands.is_empty() {
            return self.errx(b"argument missing");
        }
        if self.o.read_rc_from {
            self.o.read_rc = true;
        }
        if self.o.read_rc {
            let path = if self.o.read_rc_from {
                rc_filename.clone().unwrap_or_default()
            } else {
                match self.default_rc_filename() {
                    Some(p) => p,
                    None => return 1,
                }
            };
            let mut warnings = Vec::new();
            let mut fields = std::mem::take(&mut self.rc_fields);
            let mut showpath = self.showpath;
            let read = rc::read(
                Path::new(&os_from_bytes(&path)),
                &mut rc::Settings {
                    fields: &mut fields,
                    showpath: &mut showpath,
                },
                &mut |w| warnings.push(w),
            );
            self.rc_fields = fields;
            self.showpath = showpath;
            for w in warnings {
                self.warnx(w.as_bytes());
            }
            if !read {
                if self.o.read_rc_from {
                    return self.errx(b"couldn't read the rc file");
                }
                let mut m = b"couldn't read ".to_vec();
                m.extend_from_slice(&self.tilde_rc());
                self.warnx(&m);
                return 1;
            }
        }

        let mut pids: Vec<u32> = Vec::new();
        for arg in &operands {
            let mut walk: &[u8] = arg;
            if let Some(rest) = walk.strip_prefix(b"/proc/") {
                walk = rest;
                // `pmap /proc/PID`; anything else under /proc is passed over.
                if !walk.first().is_some_and(u8::is_ascii_digit) {
                    continue;
                }
            }
            if !walk.first().is_some_and(u8::is_ascii_digit) {
                return self.usage(false);
            }
            let (pid, used, _) = cstrtol::strtoull(walk, 0);
            if pid < 1 || pid > 0x7fff_ffff || used != walk.len() {
                return self.usage(false);
            }
            pids.push(low32(pid));
        }

        self.discover_shm_minor();

        if pids.is_empty() || pids.len() > FILL_ID_MAX {
            return self.errx(b"library failed pids statistics");
        }
        let fill = Fill {
            stat: true,
            cmdline: true,
            ..Fill::default()
        };
        let mut reader = Reader::new(
            self.root.clone(),
            fill,
            self.utf8,
            Pwcache::with_db(pwdb::Db::default()),
        );
        let procs = reader.select(&pids);

        let mut ret = 0u8;
        for p in &procs {
            match self.one_proc(p) {
                Ok(r) => ret |= r,
                Err(status) => return status,
            }
        }
        if procs.len() < pids.len() {
            // Not every process asked for was found.
            ret |= 42;
        }
        ret
    }
}

/// `is_unimportant`: the `smaps` fields `-X` leaves to `-XX`.
fn is_unimportant(s: &[u8]) -> bool {
    matches!(
        s,
        b"AnonHugePages"
            | b"KernelPageSize"
            | b"MMUPageSize"
            | b"Shared_Dirty"
            | b"Private_Dirty"
            | b"Shared_Clean"
            | b"Private_Clean"
            | b"VmFlags"
    )
}

/// `EINVAL`.
const EINVAL: i32 = 22;

/// `perror (what)`: `what: REASON` on standard error -- no program name.
fn perror(what: &[u8], errno: i32) {
    let mut m = what.to_vec();
    m.extend_from_slice(b": ");
    m.extend_from_slice(
        coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno)).as_bytes(),
    );
    m.push(b'\n');
    stdfd::diag_bytes_ahead_of_stdout(&m);
}

/// `strstr (hay, needle) != NULL`.
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A `%ld` of an `unsigned long` that has been shifted right ten places:
/// never above `LONG_MAX`.
fn signed(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// One maps line as `sscanf ("%lx-%lx %31s %llx %x:%x %llu")` reads it:
/// each field it assigned, in order, up to the first it could not.
struct Mapping<'a> {
    count: usize,
    start: u64,
    dev_major: u32,
    dev_minor: u32,
    perms: &'a [u8],
    start_set: Option<u64>,
    end_set: Option<u64>,
    perms_set: Option<&'a [u8]>,
    offset_set: Option<u64>,
    major_set: Option<u32>,
    minor_set: Option<u32>,
    inode_set: Option<u64>,
}

fn scan_mapping(line: &[u8]) -> Mapping<'_> {
    let mut m = Mapping {
        count: 0,
        start: 0,
        dev_major: 0,
        dev_minor: 0,
        perms: &[],
        start_set: None,
        end_set: None,
        perms_set: None,
        offset_set: None,
        major_set: None,
        minor_set: None,
        inode_set: None,
    };
    let mut s = Scan::new(line);
    let Some(v) = s.hex() else { return m };
    (m.start_set, m.start, m.count) = (Some(v), v, 1);
    let Some(v) = s.lit(b"-").and_then(|()| s.hex()) else {
        return m;
    };
    (m.end_set, m.count) = (Some(v), 2);
    let Some(w) = s.word(31) else { return m };
    (m.perms_set, m.perms, m.count) = (Some(w), w, 3);
    let Some(v) = s.hex() else { return m };
    (m.offset_set, m.count) = (Some(v), 4);
    let Some(v) = s.hex() else { return m };
    (m.major_set, m.dev_major, m.count) = (Some(low32(v)), low32(v), 5);
    let Some(v) = s.lit(b":").and_then(|()| s.hex()) else {
        return m;
    };
    (m.minor_set, m.dev_minor, m.count) = (Some(low32(v)), low32(v), 6);
    let Some(v) = s.ulong() else { return m };
    (m.inode_set, m.count) = (Some(v), 7);
    m
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let mut pmap = Pmap::new(&argv0);
    let status = pmap.run(&argv);
    let name = pmap.name.clone();
    stdfd::close_stdout_procps(&name, pmap.out, ExitCode::from(status))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{integer_width, is_unimportant, padded, scan_mapping, short_name};

    #[test]
    fn the_name_is_what_follows_the_last_slash() {
        assert_eq!(short_name(b"/usr/bin/pmap"), b"pmap");
        assert_eq!(short_name(b"bin/"), b"");
    }

    #[test]
    fn integer_width_counts_digits_and_gives_0_one() {
        assert_eq!(integer_width(0), 1);
        assert_eq!(integer_width(9), 1);
        assert_eq!(integer_width(10), 2);
        assert_eq!(integer_width(u64::MAX), 20);
    }

    #[test]
    fn padding_is_printf_width() {
        assert_eq!(padded(b"ab", 4, false), b"  ab");
        assert_eq!(padded(b"ab", 4, true), b"ab  ");
        assert_eq!(padded(b"abcdef", 4, false), b"abcdef", "never cut");
        assert_eq!(padded(b"ab", -1, false), b"ab");
    }

    #[test]
    fn a_map_line_is_read_as_sscanf_reads_it() {
        let m = scan_mapping(b"7f00-7f10 r-xp 00001000 08:30 93607   /usr/bin/x\n");
        assert_eq!(m.count, 7);
        assert_eq!((m.start, m.end_set), (0x7f00, Some(0x7f10)));
        assert_eq!(m.perms, b"r-xp");
        assert_eq!((m.dev_major, m.dev_minor), (8, 0x30));
        assert_eq!(m.inode_set, Some(93607));
        // A second piece of a long line: hex digits, then no `-`.
        let piece = scan_mapping(b"ffffffffffffffffffff/mapped\n");
        assert_eq!(piece.count, 1);
        assert_eq!(
            piece.start_set,
            Some(u64::MAX),
            "saturated, as strtoul does"
        );
        assert_eq!(piece.end_set, None);
        let text = scan_mapping(b"gggg/mapped\n");
        assert_eq!(text.count, 0);
        assert_eq!(text.start_set, None, "nothing assigned");
    }

    #[test]
    fn x_leaves_eight_fields_to_xx() {
        assert!(is_unimportant(b"VmFlags"));
        assert!(is_unimportant(b"Private_Dirty"));
        assert!(!is_unimportant(b"Rss"));
        assert!(!is_unimportant(b"THPeligible"));
    }
}
