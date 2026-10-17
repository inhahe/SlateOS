//! fdisk -- display or manipulate a disk partition table.
//!
//! A port of util-linux 2.39.3's `disk-utils/fdisk.c` (this file) and
//! `fdisk-menu.c` ([`menu`]), function by function and with upstream's
//! names, on the libfdisk port, printing listings through `fdisk::list`
//! (`fdisk-list.c`); measured against Ubuntu's `fdisk` by
//! `scripts/fdisk-diff.sh`.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **Every answer is a line `fgets` reads from standard input's buffer**,
//!   after a `poll` of the descriptor: a reply that is a blank line takes
//!   the default, and the end of the input (or `^C`, which interrupts the
//!   `poll`) cancels the question -- at the main menu, quits.
//! * **A line of information that follows another** gets a blank line
//!   before it (`info_count`), except in a listing.
//! * **Messages from libfdisk** go through the dialog callback: information
//!   to standard output, warnings to standard error after standard output
//!   is flushed.
//!
//! # What is not upstream's
//!
//! * **No readline.** On a terminal upstream reads with GNU readline --
//!   history, completion, its own line editing -- and here a line is read
//!   as from a pipe, the terminal's line discipline the only editing
//!   (known-issues `TD-B-FDISK-HAS-NO-READLINE`).
//! * **No pager** for the list of partition types on a GPT, which upstream
//!   sends through `less` on a terminal (the same entry).
//! * **Table headers are not coloured**: the `smartcols` port draws no
//!   colour (known-issues `TD-B-SMARTCOLS-DRAWS-NO-COLOUR`). The other
//!   colours are upstream's.

stdfdguard::guard_std_fds!();

mod menu;

use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Read, Seek, SeekFrom};
use std::rc::Rc;

use fdisk::Term;
use fdisk::list::{self, Fields, WIPEMODE_ALWAYS, WIPEMODE_AUTO, WIPEMODE_NEVER};
use getoptlong::{LongVals, Opt, Program, Takes};
use libfdisk::errno::{ECANCELED, EINVAL};
use libfdisk::parttype::{PARSE_ALIAS, PARSE_DATA, PARSE_NAME, PARSE_SEQNUM};
use libfdisk::{Ask, AskType, Context, LabelType, PartType, Table};
use quoting::{escape_unprintable, os_bytes};
use ulclosestream::{stderr_write, warn, warnx};

/// `PACKAGE_STRING`.
const PACKAGE_STRING: &str = "util-linux 2.39.3";

/// glibc's `BUFSIZ`: the reply buffer of most questions.
const BUFSIZ: usize = 8192;

/// `EACCES` and `EAGAIN`, `EINTR`, `ENOSYS`.
const EACCES: i32 = 13;
const EAGAIN: i32 = 11;
const EINTR: i32 = 4;
const ENOSYS: i32 = 38;

/// What the dialog callback shares with the rest of the program: upstream's
/// globals.
pub struct Shared {
    /// Standard output, its colours, the program's name.
    pub term: Term,
    /// `is_interactive`: standard input is a terminal.
    pub is_interactive: Cell<bool>,
    /// `info_count`: lines of information in a row.
    info_count: Cell<u32>,
}

/// The program's state: upstream's other globals and `main`'s context.
pub struct Fdisk {
    pub sh: Rc<Shared>,
    /// The context the menu works on: a nested one while in one.
    pub cxt: Context,
    /// `wipemode`: `--wipe`.
    pub wipemode: i32,
    /// `pwipemode`: `--wipe-partitions`.
    pub pwipemode: i32,
    /// `device_is_used`: the device was in use when it was opened.
    pub device_is_used: bool,
    /// `original_layout`: the partitions as the device had them.
    pub original_layout: Table,
    /// `-o`.
    pub fields: Fields,
}

/// `reply_sighandler (sig)`: nothing -- a `SIGINT` only interrupts the
/// wait for a reply.
extern "C" fn reply_sighandler(_sig: i32) {}

/// `poll` of standard input for `POLLIN`, as `get_user_reply` waits: `Err`
/// with the `errno` when it fails. A host without `poll` does not wait.
fn poll_stdin() -> Result<(), i32> {
    let mut fds = [libcall::fd::PollFd {
        fd: 0,
        events: libcall::fd::POLLIN,
        revents: 0,
    }];
    match libcall::fd::poll(&mut fds, -1) {
        Ok(_) | Err(ENOSYS) => Ok(()),
        Err(e) => Err(e),
    }
}

/// `ltrim_whitespace (buf)`.
fn ltrim_whitespace(buf: &[u8]) -> &[u8] {
    let n = buf
        .iter()
        .take_while(|&&c| ulstrutils::c_isspace(c))
        .count();
    buf.get(n..).unwrap_or_default()
}

/// `get_user_reply (prompt, buf, bufsz)`: `prompt` written, and a line read,
/// its leading blanks and its newline taken off; `-ECANCELED` at the end of
/// the input or on `^C`.
pub fn get_user_reply(sh: &Shared, prompt: &[u8], bufsz: usize) -> Result<Vec<u8>, i32> {
    // ^C, while the reply is awaited, only interrupts the wait.
    let saved = libcall::signal::save_action(libcall::signal::SIGINT).ok();
    // A host without signals (the unit tests' build) has no ^C to catch.
    let _ = libcall::signal::set_handler(libcall::signal::SIGINT, reply_sighandler, false);

    sh.term.write(prompt);
    sh.term.flush();
    let reply = match poll_stdin() {
        // Interrupted by the signal.
        Err(EINTR) => Err(-ECANCELED),
        Err(e) if e != EAGAIN => Err(e.wrapping_neg()),
        _ => {
            // The reply is the C string fgets left: up to a NUL in it.
            let line = fdisk::fgets_stdin(bufsz).line;
            let line = libfdisk::utils::c_str(&line);
            if line.is_empty() {
                // ^D.
                Err(-ECANCELED)
            } else {
                let mut buf = ltrim_whitespace(line).to_vec();
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                Ok(buf)
            }
        }
    };

    if let Some(saved) = saved {
        // Put back as it was: it was read from the system a moment ago.
        let _ = libcall::signal::restore_action(libcall::signal::SIGINT, &saved);
    }
    reply
}

/// `snprintf (prompt, 128, ...)`: at most 127 bytes of `s`.
fn prompt128(s: &[u8]) -> Vec<u8> {
    s.get(..s.len().min(127)).unwrap_or(s).to_vec()
}

/// `ask_menu (cxt, ask, buf, bufsz)`.
fn ask_menu(sh: &Shared, cxt: &mut Context, ask: &mut Ask) -> i32 {
    let dft = ask.menu_get_default();
    let dftc = char::from(u8::try_from(dft & 0xff).unwrap_or(0));
    if let Some(q) = ask.get_query() {
        // The header.
        sh.term.write(q);
        sh.term.write(b"\n");
    }
    loop {
        let mut i = 0;
        while let Some(item) = ask.menu_get_item(i) {
            let key = char::from(u8::try_from(item.key & 0xff).unwrap_or(0));
            sh.term
                .write(format!("   {key}   {} ({})\n", item.name, item.desc).as_bytes());
            i = i.wrapping_add(1);
        }
        let prompt = prompt128(format!("Select (default {dftc}): ").as_bytes());
        let buf = match get_user_reply(sh, &prompt, BUFSIZ) {
            Ok(buf) => buf,
            Err(rc) => return rc,
        };
        let c = match buf.first() {
            None => {
                cxt.info(format!("Using default response {dftc}.").as_bytes());
                dft
            }
            Some(&b) => i32::from(b.to_ascii_lowercase()),
        };
        let mut i = 0;
        while let Some(item) = ask.menu_get_item(i) {
            if c == item.key {
                ask.menu_set_result(c);
                return 0;
            }
            i = i.wrapping_add(1);
        }
        cxt.warnx(b"Value out of range.");
    }
}

/// `tochar (num)`: a partition number as its letter (BSD's).
fn tochar(num: u64) -> char {
    char::from(u8::try_from(num.wrapping_add(u64::from(b'a')).wrapping_sub(1) & 0xff).unwrap_or(0))
}

/// `ask_number (cxt, ask, buf, bufsz)`.
fn ask_number(sh: &Shared, cxt: &mut Context, ask: &mut Ask) -> i32 {
    let q = String::from_utf8_lossy(ask.get_query().unwrap_or_default()).into_owned();
    let range = ask.number_get_range();
    let (dflt, low, high) = (
        ask.number_get_default(),
        ask.number_get_low(),
        ask.number_get_high(),
    );
    let inchar = ask.number_inchars();
    let in_range = dflt >= low && dflt <= high;

    let prompt = match (&range, in_range, inchar) {
        (Some(r), true, true) => format!(
            "{q} ({}, default {}): ",
            String::from_utf8_lossy(r),
            tochar(dflt)
        ),
        (Some(r), true, false) => format!("{q} ({}, default {dflt}): ", String::from_utf8_lossy(r)),
        (_, true, true) => format!(
            "{q} ({}-{}, default {}): ",
            tochar(low),
            tochar(high),
            tochar(dflt)
        ),
        (_, true, false) => format!("{q} ({low}-{high}, default {dflt}): "),
        (_, false, true) => format!("{q} ({}-{}): ", tochar(low), tochar(high)),
        (_, false, false) => format!("{q} ({low}-{high}): "),
    };
    let prompt = prompt128(prompt.as_bytes());

    loop {
        let buf = match get_user_reply(sh, &prompt, BUFSIZ) {
            Ok(buf) => buf,
            Err(rc) => return rc,
        };
        if buf.is_empty() && in_range {
            return ask.number_set_result(dflt);
        }
        let mut rc = 0;
        let mut num: u64 = 0;
        if ulstrutils::isdigit_string(&buf) {
            match ulstrutils::ul_strtou64(&buf, 10) {
                Ok(n) => num = n,
                // Too large: asked again, without a word.
                Err(_) => continue,
            }
        } else if let Some(&b) = buf.first().filter(|b| inchar && b.is_ascii_alphabetic()) {
            num = u64::from(b.to_ascii_lowercase())
                .wrapping_sub(u64::from(b'a'))
                .wrapping_add(1);
        } else {
            rc = -EINVAL;
        }
        if rc == 0 && num >= low && num <= high {
            return ask.number_set_result(num);
        }
        cxt.warnx(b"Value out of range.");
    }
}

/// `ask_offset (cxt, ask, buf, bufsz)`: a sector, or `+`/`-` a number of
/// sectors or a size (`+100M`) from the base.
fn ask_offset(sh: &Shared, cxt: &mut Context, ask: &mut Ask) -> i32 {
    let q = String::from_utf8_lossy(ask.get_query().unwrap_or_default()).into_owned();
    let range = ask.number_get_range();
    let (dflt, low, high, base) = (
        ask.number_get_default(),
        ask.number_get_low(),
        ask.number_get_high(),
        ask.number_get_base(),
    );
    let in_range = dflt >= low && dflt <= high;
    let prompt = match (&range, in_range) {
        (Some(r), true) => format!("{q} ({}, default {dflt}): ", String::from_utf8_lossy(r)),
        (_, true) => format!("{q} ({low}-{high}, default {dflt}): "),
        (_, false) => format!("{q} ({low}-{high}): "),
    };
    let prompt = prompt128(prompt.as_bytes());

    loop {
        let buf = match get_user_reply(sh, &prompt, BUFSIZ) {
            Ok(buf) => buf,
            Err(rc) => return rc,
        };
        if buf.is_empty() && in_range {
            return ask.number_set_result(dflt);
        }
        let sig = buf.first().copied().filter(|&c| c == b'+' || c == b'-');
        let p = if sig.is_some() {
            buf.get(1..).unwrap_or_default()
        } else {
            &buf
        };
        let (rc, parsed, pwr) = ulstrutils::parse_size_power(p);
        let Ok(mut num) = rc else {
            // Asked again, without a word.
            let _ = parsed;
            continue;
        };
        if sig.is_some() && pwr != 0 {
            // `+{size}{K,M,...}`: bytes, rounded to the unit.
            let unit = ask.number_get_unit();
            num = num.wrapping_add(unit / 2);
            num = num.checked_div(unit).unwrap_or(0);
        }
        match sig {
            Some(b'+') => num = num.wrapping_add(base),
            Some(_) if ask.number_is_wrap_negative() => num = high.wrapping_sub(num),
            Some(_) => num = base.wrapping_sub(num),
            None => {}
        }
        if num >= low && num <= high {
            if sig.is_some() && pwr != 0 {
                ask.number_set_relative(true);
            }
            return ask.number_set_result(num);
        }
        cxt.warnx(b"Value out of range.");
    }
}

/// `fputs_info (ask, out)`: an information line -- the second of a run
/// after a blank line.
fn fputs_info(sh: &Shared, ask: &Ask) {
    let Some(msg) = ask.print_get_mesg() else {
        return;
    };
    if sh.info_count.get() == 1 {
        sh.term.write(b"\n");
    }
    sh.term.write(msg);
    sh.term.write(b"\n");
}

/// `rpmatch (s)`: 1 for yes, 0 for no, -1 for neither, as the C locale's
/// `^[yY]` and `^[nN]` say.
fn rpmatch(s: &[u8]) -> i32 {
    match s.first() {
        Some(b'y' | b'Y') => 1,
        Some(b'n' | b'N') => 0,
        _ => -1,
    }
}

/// `ask_callback (cxt, ask, data)`: libfdisk's questions, answered from
/// standard input, and its messages printed.
fn ask_callback(sh: &Shared, cxt: &mut Context, ask: &mut Ask) -> i32 {
    if ask.get_type() != AskType::Info {
        sh.info_count.set(0);
    }
    match ask.get_type() {
        AskType::Menu => ask_menu(sh, cxt, ask),
        AskType::Number => ask_number(sh, cxt, ask),
        AskType::Offset => ask_offset(sh, cxt, ask),
        AskType::Info => {
            if !cxt.is_listonly() {
                sh.info_count.set(sh.info_count.get().wrapping_add(1));
            }
            fputs_info(sh, ask);
            0
        }
        AskType::Warnx => {
            let msg = ask.print_get_mesg().unwrap_or_default().to_vec();
            sh.term.warning(&msg);
            stderr_write(b"\n");
            0
        }
        AskType::Warn => {
            let mut msg = ask.print_get_mesg().unwrap_or_default().to_vec();
            let e = std::io::Error::from_raw_os_error(ask.print_get_errno());
            msg.extend_from_slice(format!(": {}\n", errmsg::strerror(&e)).as_bytes());
            sh.term.warning(&msg);
            0
        }
        AskType::YesNo => {
            sh.term.write(b"\n");
            loop {
                sh.term.write(ask.get_query().unwrap_or_default());
                match get_user_reply(sh, b" [Y]es/[N]o: ", BUFSIZ) {
                    Ok(buf) => {
                        let x = rpmatch(&buf);
                        if x == 1 || x == 0 {
                            ask.yesno_set_result(x);
                            return 0;
                        }
                    }
                    Err(rc) => return rc,
                }
            }
        }
        AskType::String => {
            let mut prmt = ask.get_query().unwrap_or_default().to_vec();
            prmt.extend_from_slice(b": ");
            prmt.truncate(BUFSIZ - 1);
            sh.term.write(b"\n");
            match get_user_reply(sh, &prmt, BUFSIZ) {
                Ok(buf) => {
                    ask.string_set_result(buf);
                    0
                }
                Err(rc) => rc,
            }
        }
        AskType::None => {
            warnx(&sh.term.short, "internal error: unsupported dialog type 0");
            -EINVAL
        }
    }
}

impl Fdisk {
    /// `ask_partition_type (cxt, &canceled)`: a type read for the current
    /// label, or none; whether the question was cancelled.
    fn ask_partition_type(&mut self) -> (Option<PartType>, bool) {
        let Some(lb) = self.cxt.label_ref() else {
            return (None, false);
        };
        let q: &[u8] = match (lb.has_parttypes_shortcuts(), lb.has_code_parttypes()) {
            (true, true) => b"Hex code or alias (type L to list all): ",
            (true, false) => b"Partition type or alias (type L to list all): ",
            (false, true) => b"Hex code (type L to list all codes): ",
            (false, false) => b"Partition type (type L to list all types): ",
        };
        loop {
            let buf = match get_user_reply(&self.sh, q, 256) {
                Ok(buf) => buf,
                Err(rc) => return (None, rc == -ECANCELED),
            };
            if buf.len() == 1 && buf.first().is_some_and(|c| c.eq_ignore_ascii_case(&b'L')) {
                self.list_partition_types();
            } else if !buf.is_empty() {
                let t = self.cxt.label_ref().and_then(|lb| {
                    lb.advparse_parttype(&buf, PARSE_DATA | PARSE_ALIAS | PARSE_NAME | PARSE_SEQNUM)
                });
                if t.is_none() {
                    let mut m = b"Failed to parse '".to_vec();
                    m.extend_from_slice(&buf);
                    m.extend_from_slice(b"' partition type.");
                    self.cxt.info(&m);
                }
                return (t, false);
            }
        }
    }

    /// `list_partition_types (cxt)`: the label's types -- codes in four
    /// columns, or strings one to a line -- and its aliases.
    fn list_partition_types(&mut self) {
        let Some(lb) = self.cxt.label_ref() else {
            return;
        };
        let ntypes = lb.get_nparttypes();
        if ntypes == 0 {
            return;
        }
        let mut out = Vec::new();
        if lb.has_code_parttypes() {
            // Four columns of `<hex> <name>`, down then across.
            let mut last = [0usize; 4];
            let mut done = 0usize;
            for (k, i) in (0..4usize).rev().enumerate() {
                done = done.wrapping_add(
                    ntypes
                        .wrapping_add(i)
                        .wrapping_sub(done)
                        .checked_div(i.wrapping_add(1))
                        .unwrap_or(0),
                );
                if let Some(l) = last.get_mut(k) {
                    *l = done;
                }
            }
            let (mut i, mut next) = (0usize, 0usize);
            done = 0;
            let utf8 = smartcols::tty::codeset_is_utf8();
            loop {
                if let Some(t) = lb.get_parttype(next)
                    && let Some(name) = t.get_name()
                {
                    out.extend_from_slice(
                        format!("{}{:02x} ", if i == 0 { "\n" } else { "  " }, t.code).as_bytes(),
                    );
                    // NAME_WIDTH.
                    let mut width = 15usize;
                    out.extend_from_slice(&smartcols::mbs::mbsalign(
                        name,
                        &mut width,
                        smartcols::mbs::Align::Left,
                        b' ',
                        utf8,
                    ));
                }
                next = last.get(i).copied().unwrap_or(0).wrapping_add(done);
                i = i.wrapping_add(1);
                if i > 3 || next >= last.get(i).copied().unwrap_or(0) {
                    i = 0;
                    done = done.wrapping_add(1);
                    next = done;
                }
                if done >= last[0] {
                    break;
                }
            }
            out.push(b'\n');
        } else {
            // One a line: `<idx> <name> <typestr>`.
            for i in 0..ntypes {
                let Some(t) = lb.get_parttype(i) else {
                    continue;
                };
                let name = String::from_utf8_lossy(t.get_name().unwrap_or(b"(null)")).into_owned();
                out.extend_from_slice(format!("{:3} {name:<30} ", i.wrapping_add(1)).as_bytes());
                out.extend_from_slice(t.get_string().unwrap_or(b"(null)"));
                out.push(b'\n');
            }
        }

        // The aliases.
        if lb.has_parttypes_shortcuts() {
            out.extend_from_slice(b"\nAliases:\n");
            let mut next = 0;
            // 0: one; 2: a deprecated one, not shown; past the end, Err.
            while let Ok((typestr, _, alias, rc)) = lb.get_parttype_shortcut(next) {
                if rc == 0 {
                    out.extend_from_slice(
                        format!("   {:<14} - {typestr}\n", alias.unwrap_or("(null)")).as_bytes(),
                    );
                }
                next = next.wrapping_add(1);
            }
        }
        self.sh.term.write(&out);
    }

    /// `toggle_dos_compatibility_flag (cxt)`.
    pub fn toggle_dos_compatibility_flag(&mut self) {
        let Some(i) = self.cxt.get_label(Some(b"dos")) else {
            return;
        };
        let flag = !self
            .cxt
            .label_at(i)
            .is_some_and(libfdisk::dos::is_compatible);
        self.cxt.info(if flag {
            b"DOS Compatibility flag is set (DEPRECATED!)".as_slice()
        } else {
            b"DOS Compatibility flag is not set"
        });
        if let Some(lb) = self.cxt.label_at_mut(i) {
            libfdisk::dos::enable_compatible(lb, flag);
        }
        if self.cxt.is_labeltype(LabelType::Dos) {
            // The current label: its alignment again.
            self.cxt.reset_alignment();
        }
    }

    /// `change_partition_type (cxt)`.
    pub fn change_partition_type(&mut self) {
        let Ok(i) = self.cxt.ask_partnum(false) else {
            return;
        };
        let Ok(pa) = self.cxt.get_partition(i) else {
            let m = format!("Partition {} does not exist yet!", i.wrapping_add(1));
            self.cxt.warnx(m.as_bytes());
            return;
        };
        let old = pa.ty.as_ref().map_or_else(
            || b"Unknown".to_vec(),
            |t| t.get_name().unwrap_or(b"(null)").to_vec(),
        );
        let (t, canceled) = loop {
            let (t, canceled) = self.ask_partition_type();
            if canceled || t.is_some() {
                break (t, canceled);
            }
        };
        if let Some(t) = t.filter(|_| !canceled)
            && self.cxt.set_partition_type(i, &t) == 0
        {
            let mut m = b"Changed type of partition '".to_vec();
            m.extend_from_slice(&old);
            m.extend_from_slice(b"' to '");
            m.extend_from_slice(t.get_name().unwrap_or(b"Unknown"));
            m.extend_from_slice(b"'.");
            self.cxt.info(&m);
        } else {
            let mut m =
                format!("Type of partition {} is unchanged: ", i.wrapping_add(1)).into_bytes();
            m.extend_from_slice(&old);
            m.push(b'.');
            self.cxt.info(&m);
        }
    }

    /// `print_partition_info (cxt)`: each field of a partition the label
    /// has, a line each.
    pub fn print_partition_info(&mut self) -> i32 {
        let i = match self.cxt.ask_partnum(false) {
            Ok(i) => i,
            Err(rc) => return rc,
        };
        let mut pa = match self.cxt.get_partition(i) {
            Ok(pa) => pa,
            Err(rc) => {
                let m = format!("Partition {} does not exist yet!", i.wrapping_add(1));
                self.cxt.warnx(m.as_bytes());
                return rc;
            }
        };
        let fields = match self.cxt.label_get_fields_ids_all() {
            Ok(f) => f,
            Err(rc) => return rc,
        };
        for id in fields {
            let Some(fd) = self.cxt.label_ref().and_then(|lb| lb.get_field(id)) else {
                continue;
            };
            let data = match self.cxt.partition_to_string(&mut pa, id) {
                Ok(d) => d,
                Err(rc) => return rc,
            };
            let Some(data) = data.filter(|d| !d.is_empty()) else {
                continue;
            };
            let mut m = format!("{:>15}: ", fd.get_name()).into_bytes();
            m.extend_from_slice(&data);
            self.cxt.info(&m);
        }
        0
    }

    /// `dump_blkdev (cxt, name, offset, size)`: `size` bytes of the device
    /// from `offset`, in hex.
    fn dump_blkdev(&mut self, name: &str, offset: u64, size: usize) {
        self.cxt
            .info(format!("\n{name}: offset = {offset}, size = {size} bytes.").as_bytes());
        let Some(dev) = self.cxt.get_dev().cloned() else {
            return;
        };
        let mut f = &*dev;
        if let Err(e) = f.seek(SeekFrom::Start(offset)) {
            self.cxt.warn(e.raw_os_error().unwrap_or(0), b"cannot seek");
            return;
        }
        let mut buf = vec![0u8; size];
        let mut got = 0usize;
        // `read_all`: until it is all read, or the end.
        let err = loop {
            let Some(rest) = buf.get_mut(got..).filter(|r| !r.is_empty()) else {
                break 0;
            };
            match f.read(rest) {
                Ok(0) => break 0,
                Ok(n) => got = got.saturating_add(n),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => break e.raw_os_error().unwrap_or(0),
            }
        };
        if got == size {
            self.sh.term.write(&dump_buffer(offset, &buf));
        } else {
            self.cxt.warn(err, b"cannot read");
        }
    }

    /// `dump_firstsector (cxt)`.
    pub fn dump_firstsector(&mut self) {
        let size = usize::try_from(self.cxt.get_sector_size()).unwrap_or(0);
        self.dump_blkdev("First sector", 0, size);
    }

    /// `dump_disklabel (cxt)`: each of the label's on-disk parts.
    pub fn dump_disklabel(&mut self) {
        let mut i = 0;
        while let Ok((name, offset, size)) = self.cxt.locate_disklabel(i) {
            if size == 0 {
                break;
            }
            self.dump_blkdev(name, offset, size);
            i = i.wrapping_add(1);
        }
    }

    /// `follow_wipe_mode (cxt)`: whether a write is to wipe the signature
    /// the device carries, and a warning that says which.
    pub fn follow_wipe_mode(&mut self) {
        let mut dowipe = self.wipemode == WIPEMODE_ALWAYS;
        if std::io::stdin().is_terminal() && self.wipemode == WIPEMODE_AUTO {
            // In interactive mode.
            dowipe = true;
        }
        if self.cxt.is_ptcollision() && self.wipemode != WIPEMODE_NEVER {
            // An old partition table always goes.
            dowipe = true;
        }
        self.cxt.enable_wipe(dowipe);
        let collision = self.cxt.get_collision().unwrap_or(b"(null)").to_vec();
        let mut m = b"The device contains '".to_vec();
        m.extend_from_slice(&collision);
        m.extend_from_slice(if dowipe {
            b"' signature and it will be removed by a write command. See fdisk(8) man page and --wipe option for \
              more details."
                .as_slice()
        } else {
            b"' signature and it may remain on the device. It is recommended to wipe the device with wipefs(8) \
              or fdisk --wipe, in order to avoid possible collisions."
        });
        self.cxt.warnx(&m);
    }
}

/// `skip_empty (buf, i, sz)`: where the run of 16-byte lines equal to the
/// one at `i` ends, or `i` when the next differs.
fn skip_empty(buf: &[u8], i: usize, sz: usize) -> usize {
    let line = |at: usize| buf.get(at..at.saturating_add(16));
    let mut next = i.saturating_add(16);
    while next < sz {
        if line(i) != line(next) {
            break;
        }
        next = next.saturating_add(16);
    }
    if next == i.saturating_add(16) {
        i
    } else {
        next
    }
}

/// `dump_buffer (base, buf, sz)`: 16 bytes a line, a run of equal lines
/// shown as `*`.
fn dump_buffer(base: u64, buf: &[u8]) -> Vec<u8> {
    let sz = buf.len();
    let mut out = Vec::new();
    // `l` is a `size_t` set to -1 after a line, so it is 0 again on the
    // increment.
    let (mut i, mut l, mut next) = (0usize, 0usize, 0usize);
    while i < sz {
        if l == 0 {
            if next == 0 {
                next = skip_empty(buf, i, sz);
            }
            out.extend_from_slice(format!("{:08x} ", base.wrapping_add(i as u64)).as_bytes());
        }
        out.extend_from_slice(format!(" {:02x}", buf.get(i).copied().unwrap_or(0)).as_bytes());
        if l == 7 {
            // The words' separator.
            out.push(b' ');
        } else if l == 15 {
            out.push(b'\n');
            l = usize::MAX;
            if next > i {
                out.extend_from_slice(b"*\n");
                i = next.wrapping_sub(1);
            }
            next = 0;
        }
        i = i.wrapping_add(1);
        l = l.wrapping_add(1);
    }
    if l > 0 {
        out.push(b'\n');
    }
    out
}

/// The program's name, as `program_invocation_short_name`: `argv[0]`'s last
/// component.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// `usage ()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let p = String::from_utf8_lossy(short).into_owned();
    let mut out = format!(
        "\nUsage:\n {p} [options] <disk>         change partition table\n {p} [options] -l [<disk>...] list \
         partition table(s)\n\nDisplay or manipulate a disk partition table.\n\nOptions:\n"
    );
    out.push_str(
        " -b, --sector-size <size>      physical and logical sector size\n -B, --protect-boot            don't \
         erase bootbits when creating a new label\n -c, --compatibility[=<mode>]  mode is 'dos' or 'nondos' \
         (default)\n -L, --color[=<when>]          colorize output (auto, always or never)\n",
    );
    out.push_str(&format!(
        "                                 {}\n",
        ulcolors::usage_colors_default()
    ));
    out.push_str(
        " -l, --list                    display partitions and exit\n -x, --list-details            like --list \
         but with more details\n -n, --noauto-pt               don't create default partition table on empty \
         devices\n -o, --output <list>           output columns\n -t, --type <type>             recognize \
         specified partition table type only\n -u, --units[=<unit>]          display units: 'cylinders' or \
         'sectors' (default)\n -s, --getsz                   display device size in 512-byte sectors \
         [DEPRECATED]\n     --bytes                   print SIZE in bytes rather than in human readable format\n \
         \x20   --lock[=<mode>]           use exclusive device lock (yes, no or nonblock)\n -w, --wipe <mode>       \
         \x20     wipe signatures (auto, always or never)\n -W, --wipe-partitions <mode>  wipe signatures from new \
         partitions (auto, always or never)\n\n -C, --cylinders <number>      specify the number of cylinders\n \
         -H, --heads <number>          specify the number of heads\n -S, --sectors <number>        specify the \
         number of sectors per track\n\n",
    );
    out.push_str(&format!(
        "{:<31}{}\n{:<31}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    let mut out = out.into_bytes();
    list::list_available_columns(&mut out);
    out.extend_from_slice(b"\nFor more details see fdisk(8).\n");
    out
}

/// The program, as invoked by `fdisk`.
const FDISK: Program = Program::new("fdisk", 1);

/// Upstream's option string.
const SHORTS: &str = "b:Bc::C:hH:lL::no:sS:t:u::vVw:W:x";

/// `OPT_BYTES` and `OPT_LOCK`: `CHAR_MAX + 1` on.
const OPT_BYTES: i32 = 128;
const OPT_LOCK: i32 = 129;

/// Upstream's `longopts[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("bytes", Takes::Nothing),
    ("color", Takes::Optional),
    ("compatibility", Takes::Optional),
    ("cylinders", Takes::Required),
    ("heads", Takes::Required),
    ("sectors", Takes::Required),
    ("getsz", Takes::Nothing),
    ("help", Takes::Nothing),
    ("list", Takes::Nothing),
    ("list-details", Takes::Nothing),
    ("lock", Takes::Optional),
    ("noauto-pt", Takes::Nothing),
    ("sector-size", Takes::Required),
    ("type", Takes::Required),
    ("units", Takes::Optional),
    ("version", Takes::Nothing),
    ("output", Takes::Required),
    ("protect-boot", Takes::Nothing),
    ("wipe", Takes::Required),
    ("wipe-partitions", Takes::Required),
];
/// Each one's `val`.
const LONG_VALS: [i32; 20] = [
    OPT_BYTES,
    b'L' as i32,
    b'c' as i32,
    b'C' as i32,
    b'H' as i32,
    b'S' as i32,
    b's' as i32,
    b'h' as i32,
    b'l' as i32,
    b'x' as i32,
    OPT_LOCK,
    b'n' as i32,
    b'b' as i32,
    b't' as i32,
    b'u' as i32,
    b'V' as i32,
    b'o' as i32,
    b'B' as i32,
    b'w' as i32,
    b'W' as i32,
];
/// [`LONGS`] and [`LONG_VALS`] as one table.
const OPTIONS: LongVals<'static, i32> = LongVals::new(LONGS, &LONG_VALS);

/// What `main` does: `ACT_*`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Fdisk,
    List,
    ListDetails,
    ShowSize,
}

/// An `exit` status, to end the program with.
struct Exit(u8);

impl Fdisk {
    /// `errx (EXIT_FAILURE, msg)`.
    fn errx(&self, msg: &str) -> Exit {
        warnx(&self.sh.term.short, msg);
        Exit(1)
    }

    /// `err (EXIT_FAILURE, msg)`: with the `errno` `errnum`.
    fn err(&self, msg: &str, errnum: i32) -> Exit {
        warn(
            &self.sh.term.short,
            msg,
            &std::io::Error::from_raw_os_error(errnum),
        );
        Exit(1)
    }

    /// `errtryhelp (EXIT_FAILURE)`.
    fn errtryhelp(&self) -> Exit {
        stderr_write(
            format!(
                "Try '{} --help' for more information.\n",
                escape_unprintable(&self.sh.term.short)
            )
            .as_bytes(),
        );
        Exit(1)
    }

    /// `strtou32_or_err (arg, msg)`.
    fn strtou32_or_err(&self, arg: &OsStr, msg: &str) -> Result<u32, Exit> {
        ulstrutils::ul_strtou32(&os_bytes(arg), 10)
            .map_err(|e| self.errx(&ulstrutils::num_error_message(msg, arg, e)))
    }

    /// `get_dev_blocks (dev)`: the device's size in 1 KiB blocks.
    fn get_dev_blocks(&self, dev: &[u8]) -> Result<u64, Exit> {
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            /// `O_NONBLOCK`.
            const O_NONBLOCK: i32 = 0o4000;
            opts.custom_flags(O_NONBLOCK);
        }
        let shown = escape_unprintable(dev);
        let f = opts.open(ulsysfs::path_of(dev)).map_err(|e| {
            self.err(
                &format!("cannot open {shown}"),
                e.raw_os_error().unwrap_or(0),
            )
        })?;
        let size = ulblkid::blkdev::get_sectors(&f)
            .map_err(|e| self.err(&format!("BLKGETSIZE ioctl failed on {shown}"), e))?;
        Ok(size / 2)
    }

    /// `main ()`, from its options on: the exit status.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's main, kept in one piece so it can be read against it"
    )]
    fn run(&mut self, argv: &[OsString]) -> Result<u8, Exit> {
        let arg0 = argv
            .first()
            .map_or(OsStr::new("fdisk"), OsString::as_os_str);
        let mut act = Act::Fdisk;
        let mut noauto_pt = false;
        let mut colormode = ulcolors::ColorMode::Undef;
        let mut outarg: Option<Vec<u8>> = None;
        let mut lockmode: Option<Vec<u8>> = None;
        let mut operands: Vec<Vec<u8>> = Vec::new();

        let own = argv.get(1..).unwrap_or_default();
        for item in FDISK.parse(own, SHORTS, LONGS) {
            let opt = match item {
                Ok(opt) => opt,
                Err(e) => {
                    // glibc names the program by argv[0] as given.
                    stderr_write(
                        format!("{}: {}\n", escape_unprintable(&os_bytes(arg0)), e.sentence)
                            .as_bytes(),
                    );
                    return Err(self.errtryhelp());
                }
            };
            let Some((c, value)) = OPTIONS.code::<i32>(&opt) else {
                if let Opt::Operand(w) = opt {
                    operands.push(os_bytes(w).into_owned());
                }
                continue;
            };
            let arg = value.clone().unwrap_or_default();
            let argb = os_bytes(&arg).into_owned();
            match c {
                c if c == i32::from(b'b') => {
                    let sz = self.strtou32_or_err(&arg, "invalid sector size argument")?;
                    if ![512, 1024, 2048, 4096].contains(&sz) {
                        return Err(self.errx("invalid sector size argument"));
                    }
                    self.cxt.save_user_sector_size(sz, sz);
                }
                c if c == i32::from(b'B') => self.cxt.enable_bootbits_protection(true),
                c if c == i32::from(b'C') => {
                    let n = self.strtou32_or_err(&arg, "invalid cylinders argument")?;
                    self.cxt.save_user_geometry(n, 0, 0);
                }
                c if c == i32::from(b'c') => {
                    // This setting is the DOS driver's, whatever the label.
                    if value.is_some() {
                        let p = argb.strip_prefix(b"=").unwrap_or(&argb);
                        let Some(i) = self.cxt.get_label(Some(b"dos")) else {
                            return Err(self.err("not found DOS label driver", 0));
                        };
                        let enable = match p {
                            b"dos" => true,
                            b"nondos" => false,
                            _ => {
                                return Err(self.errx(&format!(
                                    "unknown compatibility mode '{}'",
                                    escape_unprintable(p)
                                )));
                            }
                        };
                        if let Some(lb) = self.cxt.label_at_mut(i) {
                            libfdisk::dos::enable_compatible(lb, enable);
                        }
                    }
                }
                c if c == i32::from(b'H') => {
                    let n = self.strtou32_or_err(&arg, "invalid heads argument")?;
                    self.cxt.save_user_geometry(0, n, 0);
                }
                c if c == i32::from(b'S') => {
                    let n = self.strtou32_or_err(&arg, "invalid sectors argument")?;
                    self.cxt.save_user_geometry(0, 0, n);
                }
                c if c == i32::from(b'l') => act = Act::List,
                c if c == i32::from(b'x') => act = Act::ListDetails,
                c if c == i32::from(b'L') => {
                    colormode = ulcolors::ColorMode::Auto;
                    if value.is_some() {
                        colormode = ulcolors::colormode_or_err(&argb).map_err(|bad| {
                            self.errx(&format!(
                                "unsupported color mode: {}",
                                quoting::escaped_in_quotes(&bad)
                            ))
                        })?;
                    }
                }
                c if c == i32::from(b'n') => noauto_pt = true,
                c if c == i32::from(b'o') => outarg = Some(argb),
                c if c == i32::from(b's') => act = Act::ShowSize,
                c if c == i32::from(b't') => {
                    for i in 0..self.cxt.get_nlabels() {
                        if let Some(lb) = self.cxt.label_at_mut(i) {
                            lb.set_disabled(true);
                        }
                    }
                    let Some(i) = self.cxt.get_label(Some(&argb)) else {
                        return Err(self.errx(&format!(
                            "unsupported disklabel: {}",
                            escape_unprintable(&argb)
                        )));
                    };
                    if let Some(lb) = self.cxt.label_at_mut(i) {
                        lb.set_disabled(false);
                    }
                }
                c if c == i32::from(b'u') => {
                    let unit = value
                        .as_ref()
                        .map(|_| argb.strip_prefix(b"=").unwrap_or(&argb));
                    self.cxt.set_unit(unit);
                }
                c if c == i32::from(b'V') || c == i32::from(b'v') => {
                    self.sh.term.write(
                        format!(
                            "{} from {PACKAGE_STRING}\n",
                            String::from_utf8_lossy(&self.sh.term.short)
                        )
                        .as_bytes(),
                    );
                    return Ok(0);
                }
                c if c == i32::from(b'w') => {
                    self.wipemode = list::wipemode_from_string(&argb)
                        .ok_or_else(|| self.errx("unsupported wipe mode"))?;
                }
                c if c == i32::from(b'W') => {
                    self.pwipemode = list::wipemode_from_string(&argb)
                        .ok_or_else(|| self.errx("unsupported wipe mode"))?;
                }
                c if c == i32::from(b'h') => {
                    let text = usage(&self.sh.term.short);
                    self.sh.term.write(&text);
                    return Ok(0);
                }
                OPT_BYTES => self.cxt.set_size_unit(libfdisk::SIZEUNIT_BYTES),
                OPT_LOCK => {
                    lockmode = Some(match value {
                        Some(_) => argb.strip_prefix(b"=").unwrap_or(&argb).to_vec(),
                        None => b"1".to_vec(),
                    });
                }
                _ => return Err(self.errtryhelp()),
            }
        }

        if operands.len() != 1 && self.cxt.has_user_device_properties() {
            warnx(
                &self.sh.term.short,
                "The device properties (sector size and geometry) should be used with one specified device only.",
            );
        }

        *self.sh.term.colors.borrow_mut() = Some(ulcolors::Colors::init(
            colormode,
            b"fdisk",
            smartcols::tty::stdout_is_tty(),
        ));
        self.sh.is_interactive.set(std::io::stdin().is_terminal());
        self.fields.string = outarg;

        match act {
            Act::List | Act::ListDetails => {
                self.cxt.enable_listonly(true);
                if act == Act::ListDetails {
                    self.cxt.enable_details(true);
                }
                if operands.is_empty() {
                    list::print_all_devices_pt(&mut self.cxt, &self.sh.term, &self.fields, false);
                } else {
                    let mut rc: i32 = 0;
                    for (k, dev) in operands.iter().enumerate() {
                        rc = rc.wrapping_add(list::print_device_pt(
                            &mut self.cxt,
                            &self.sh.term,
                            &self.fields,
                            dev,
                            true,
                            false,
                            k != 0,
                        ));
                    }
                    if rc != 0 {
                        return Ok(1);
                    }
                }
                Ok(0)
            }
            Act::ShowSize => {
                // Deprecated.
                if operands.is_empty() {
                    warnx(&self.sh.term.short, "bad usage");
                    return Err(self.errtryhelp());
                }
                for dev in &operands {
                    let blks = self.get_dev_blocks(dev)?;
                    if operands.len() == 1 {
                        self.sh.term.write(format!("{blks}\n").as_bytes());
                    } else {
                        let mut line = dev.clone();
                        line.extend_from_slice(format!(": {blks}\n").as_bytes());
                        self.sh.term.write(&line);
                    }
                }
                Ok(0)
            }
            Act::Fdisk => {
                let [devname] = operands.as_slice() else {
                    warnx(&self.sh.term.short, "bad usage");
                    return Err(self.errtryhelp());
                };
                self.interactive(devname, noauto_pt, lockmode.as_deref())
            }
        }
    }

    /// `main`'s `ACT_FDISK`: the device opened and the menu run, until a
    /// command ends the program.
    fn interactive(
        &mut self,
        devname: &[u8],
        noauto_pt: bool,
        lockmode: Option<&[u8]>,
    ) -> Result<u8, Exit> {
        // From here, messages go through fdisk_info and the rest.
        self.sh
            .term
            .color_scheme_enable(b"welcome", ulcolors::GREEN);
        self.cxt
            .info(format!("Welcome to fdisk ({PACKAGE_STRING}).").as_bytes());
        self.sh.term.color_disable();
        self.cxt.info(
            b"Changes will remain in memory only, until you decide to write them.\nBe careful before using the \
              write command.\n",
        );

        let mut rc = self.cxt.assign_device(devname, false);
        if rc == -EACCES {
            rc = self.cxt.assign_device(devname, true);
            if rc == 0 {
                self.cxt.warnx(b"Device is open in read-only mode.");
            }
        }
        if rc != 0 {
            return Err(self.err(
                &format!("cannot open {}", escape_unprintable(devname)),
                rc.wrapping_neg(),
            ));
        }
        if self.cxt.device_is_used() {
            self.cxt.warnx(
                b"This disk is currently in use - repartitioning is probably a bad idea.\nIt's recommended to \
                  umount all file systems, and swapoff all swap\npartitions on this disk.\n",
            );
        }
        self.sh.term.flush();

        if !self.cxt.is_readonly()
            && let Some(dev) = self.cxt.get_dev().cloned()
            && ulblkid::blkdev::lock(&dev, devname, lockmode, &self.sh.term.short) != 0
        {
            // Read-only or locked out: nothing written to sync.
            let _ = self.cxt.deassign_device(true);
            return Ok(1);
        }
        if self.cxt.get_collision().is_some() {
            self.follow_wipe_mode();
        }
        if !self.cxt.has_label() {
            self.cxt
                .info(b"Device does not contain a recognized partition table.");
            if !noauto_pt {
                // Upstream ignores the result too: the menu says what is there.
                let _ = self.cxt.create_disklabel(None);
            }
        } else if self.cxt.is_labeltype(LabelType::Gpt) && libfdisk::gpt::is_hybrid(&mut self.cxt) {
            self.cxt.warnx(
                b"A hybrid GPT was detected. You have to sync the hybrid MBR manually (expert command 'M').",
            );
        }
        // `-o`, checked against the label.
        let _ = list::init_fields(&self.cxt, &self.sh.term, &self.fields);

        if !self.cxt.is_readonly() {
            // Upstream keeps whatever it got: none, if this failed.
            let _ = self.cxt.get_partitions(&mut self.original_layout);
            self.device_is_used = self.cxt.device_is_used();
        }
        loop {
            // The menu ends the program itself, on `w` and `q`.
            let _ = self.process_fdisk_menu();
        }
    }
}

fn main() {
    // Before anything touches standard I/O: a descriptor the process was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("fdisk"), OsString::as_os_str),
    );
    let sh = Rc::new(Shared {
        term: Term::new(short),
        is_interactive: Cell::new(false),
        info_count: Cell::new(0),
    });
    let mut cxt = Context::new();
    let cb_sh = Rc::clone(&sh);
    cxt.set_ask(Some(Rc::new(move |cxt: &mut Context, ask: &mut Ask| {
        ask_callback(&cb_sh, cxt, ask)
    })));
    let mut f = Fdisk {
        sh: Rc::clone(&sh),
        cxt,
        wipemode: WIPEMODE_AUTO,
        pwipemode: WIPEMODE_AUTO,
        device_is_used: false,
        original_layout: Table::new(),
        fields: Fields::default(),
    };
    let status = match f.run(&argv) {
        Ok(status) | Err(Exit(status)) => status,
    };
    sh.term.exit(status)
}

#[cfg(test)]
mod tests;
