//! System logging: `openlog`, `syslog`, `vsyslog`, `setlogmask`, `closelog`
//! and the `_FORTIFY_SOURCE` forms -- glibc 2.39's `misc/syslog.c`, with one
//! SlateOS adaptation, the journal.
//!
//! ## A record
//!
//! Each call the mask lets through becomes one record:
//!
//! ```text
//! <pri>Mmm dd hh:mm:ss tag[pid]: message
//! ```
//!
//! `pri` is the priority with its facility -- the call's, or `openlog`'s when
//! the call names none. The timestamp is local time, spelled in the C locale
//! whatever the program's is. The tag is `openlog`'s identity or the
//! program's name, `[pid]` is there under `LOG_PID`, and the message is the
//! format expanded as `printf` expands it -- `%m` with the `errno` the call
//! began with. Bits outside a priority and a facility are complained about in
//! a record of their own, `syslog: unknown facility/priority: %x`, and then
//! dropped. The complaint goes at BSD's `INTERNALLOG`, which is a set of
//! *options*: read as a priority, `LOG_PERROR` is `LOG_AUTH` and the rest fold
//! into `LOG_ERR`, so it is filed at `<35>`, as glibc files it.
//!
//! ## Where it goes
//!
//! To the daemon at `/dev/log`: one datagram per record, or -- when the socket
//! there turns out to be a stream (`EPROTOTYPE`) -- one write per record, with
//! a NUL after it. The connection is made by the first record, or by `openlog`
//! under `LOG_NDELAY`, and kept. A send that fails is retried once on a new
//! connection, since the daemon may have restarted; when that fails too, the
//! record goes to `/dev/console` under `LOG_CONS`, or nowhere. `LOG_PERROR`
//! copies each record to standard error first, from its tag on.
//!
//! All of that is glibc's, quirks included:
//!
//! - whether a stream record carries its NUL is decided before the first
//!   attempt, so a record resent on a connection that has just become a stream
//!   goes without one;
//! - `openlog`'s facility replaces the default even when it is 0 (`LOG_KERN`),
//!   so after `openlog (tag, 0, 0)` a record that names no facility is the
//!   kernel's;
//! - the copies on standard error and the console stop at a NUL in the message,
//!   though the record sent does not;
//! - the console's copy ends in `\r\n` whatever the message ends in.
//!
//! `posix/tools/oracle/syslog_harness.py` asked glibc 2.39 all of it, with
//! daemons of its own at `/dev/log` -- of either kind, restarting, absent --
//! and a FIFO of its own at `/dev/console`; `tests::every_scenario_is_glibcs`
//! replays the answers.
//!
//! ## The journal (SlateOS)
//!
//! SlateOS has no Unix-domain sockets that can have a name: `socket(AF_UNIX,
//! ...)` fails with `EAFNOSUPPORT`, so there is no `/dev/log` to reach, and
//! glibc's code would lose every record not under `LOG_CONS`. While that is
//! so, the journal file `journalctl` reads stands in for the daemon, as lane B
//! asked in `requests/b-d-libc-syslog-could-reach-journalctl-today.md`
//! (design-decisions §1166). A record becomes one JSON line in
//! `/var/log/syslog.jsonl`, in `journalrec`'s spelling, appended under the
//! journal's lock as `logger` and `syslogd` append theirs (`journalio`'s
//! protocol, design-decisions §1037).
//!
//! The journal is chosen where glibc would have connected, and only when the
//! socket call fails with exactly that error, so it retires itself once the
//! sockets exist. Everything around it is glibc's: a failed append is a failed
//! send, retried once and then given to `LOG_CONS`.
//!
//! A record whose tag or message is not UTF-8 cannot become a JSON string
//! without being altered, so it goes to standard error instead -- once, so not
//! again under `LOG_PERROR`, which has already put it there.
//!
//! ## Threads
//!
//! The logger is the process's. One lock, glibc's `syslog_lock`, is held for
//! each call, and under it the record is built, copied and sent, so records
//! from two threads neither interleave nor share a connection mid-send. The
//! timestamp's `localtime_r` takes the time-zone lock inside it; nothing takes
//! the two the other way round.

// `printf::_snprintf_impl` is the engine's `snprintf`; the underscore is the
// convention for libc implementation symbols, not a privacy marker.
#![allow(clippy::used_underscore_items)]

use core::sync::atomic::AtomicI32;

use crate::errno;
use crate::fcntl::{O_APPEND, O_CLOEXEC, O_CREAT, O_NOCTTY, O_WRONLY};
use crate::file::LOCK_EX;
use crate::printf::{self, VaList};
use crate::socket::{AF_UNIX, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_STREAM};
use crate::time::Tm;

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

// ---------------------------------------------------------------------------
// Assembly trampolines -- `va_start`, then call the matching `v*` variant.
//
// The named-argument counts decide the initial `gp_offset` and which register
// carries the `va_list*`:
//   syslog(priority, fmt, ...)              : 2 named args (like `fprintf`)
//   __syslog_chk(priority, flag, fmt, ...)  : 3 named args
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
va_trampoline!("syslog", "vsyslog", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("__syslog_chk", "__vsyslog_chk", "24", "rcx");

// ---------------------------------------------------------------------------
// Priorities, facilities and options -- <syslog.h>'s values, glibc's and
// musl's alike.
// ---------------------------------------------------------------------------

/// System is unusable.
pub const LOG_EMERG: i32 = 0;
/// Action must be taken immediately.
pub const LOG_ALERT: i32 = 1;
/// Critical conditions.
pub const LOG_CRIT: i32 = 2;
/// Error conditions.
pub const LOG_ERR: i32 = 3;
/// Warning conditions.
pub const LOG_WARNING: i32 = 4;
/// Normal but significant.
pub const LOG_NOTICE: i32 = 5;
/// Informational.
pub const LOG_INFO: i32 = 6;
/// Debug-level messages.
pub const LOG_DEBUG: i32 = 7;

/// The priority's bits in a priority value.
pub const LOG_PRIMASK: i32 = 0x07;

/// Kernel messages.
pub const LOG_KERN: i32 = 0;
/// User-level messages.
pub const LOG_USER: i32 = 1 << 3;
/// Mail system.
pub const LOG_MAIL: i32 = 2 << 3;
/// System daemons.
pub const LOG_DAEMON: i32 = 3 << 3;
/// Security/authorization.
pub const LOG_AUTH: i32 = 4 << 3;
/// syslogd internal.
pub const LOG_SYSLOG: i32 = 5 << 3;
/// Line printer.
pub const LOG_LPR: i32 = 6 << 3;
/// Network news.
pub const LOG_NEWS: i32 = 7 << 3;
/// UUCP.
pub const LOG_UUCP: i32 = 8 << 3;
/// Clock daemon.
pub const LOG_CRON: i32 = 9 << 3;
/// Security/authorization, private.
pub const LOG_AUTHPRIV: i32 = 10 << 3;
/// FTP daemon.
pub const LOG_FTP: i32 = 11 << 3;
/// Local use 0-7.
pub const LOG_LOCAL0: i32 = 16 << 3;
pub const LOG_LOCAL1: i32 = 17 << 3;
pub const LOG_LOCAL2: i32 = 18 << 3;
pub const LOG_LOCAL3: i32 = 19 << 3;
pub const LOG_LOCAL4: i32 = 20 << 3;
pub const LOG_LOCAL5: i32 = 21 << 3;
pub const LOG_LOCAL6: i32 = 22 << 3;
pub const LOG_LOCAL7: i32 = 23 << 3;

/// How many facilities there are.
pub const LOG_NFACILITIES: i32 = 24;
/// The facility's bits in a priority value.
pub const LOG_FACMASK: i32 = 0x03f8;

/// Log the PID with each record.
pub const LOG_PID: i32 = 0x01;
/// Write to the console when the log cannot be reached.
pub const LOG_CONS: i32 = 0x02;
/// Connect on the first record (the default).
pub const LOG_ODELAY: i32 = 0x04;
/// Connect at `openlog`.
pub const LOG_NDELAY: i32 = 0x08;
/// Do not wait for children (ignored, as glibc ignores it).
pub const LOG_NOWAIT: i32 = 0x10;
/// Copy each record to standard error.
pub const LOG_PERROR: i32 = 0x20;

/// The priority in a priority value (`LOG_PRI`).
#[inline]
const fn log_pri(p: i32) -> i32 {
    p & LOG_PRIMASK
}

/// The mask that lets priority `p` through (`LOG_MASK`). `p` is 0..=7.
#[inline]
#[must_use]
pub const fn log_mask(p: i32) -> i32 {
    1 << p
}

/// The mask that lets every priority up to and including `p` through
/// (`LOG_UPTO`).
#[inline]
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
// Shift and subtract are safe: p is always 0-7 (LOG_EMERG..LOG_DEBUG),
// so (p+1) is at most 8, and 1<<8 = 256, well within i32 range.
pub const fn log_upto(p: i32) -> i32 {
    (1 << (p + 1)) - 1
}

/// What glibc files its own complaints at: BSD's `INTERNALLOG`, option bits
/// and all (module docs).
const INTERNALLOG: i32 = LOG_ERR | LOG_CONS | LOG_PERROR | LOG_PID;

/// glibc's `bufs`: a record that fits is built here, on the stack; a longer
/// one in a buffer of its own length.
const BUFS: usize = 1024;

/// `_PATH_LOG`.
const PATH_LOG: &[u8] = b"/dev/log\0";
/// `_PATH_CONSOLE`.
const PATH_CONSOLE: &[u8] = b"/dev/console\0";
/// The journal file `journalctl` reads first, and `syslogd` and `logger`
/// append to (`journalrec::MAIN_LOG_PATH`).
const JOURNAL_PATH: &[u8] = b"/var/log/syslog.jsonl\0";

/// `STDERR_FILENO`.
const STDERR: i32 = 2;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// glibc's `syslog.c` statics, under one name and one lock.
struct State {
    /// `LogType`: the kind of socket the next connection is made with.
    log_type: i32,
    /// `LogFile`: the socket, or -1.
    log_file: i32,
    /// `connected`.
    connected: bool,
    /// Connected to the journal rather than to a socket (module docs).
    journal: bool,
    /// `LogStat`: `openlog`'s options.
    log_stat: i32,
    /// `LogTag`: `openlog`'s identity, or null for the program's name. Kept,
    /// not copied, as glibc keeps it.
    log_tag: *const u8,
    /// `LogFacility`: the facility of a record that names none.
    log_facility: i32,
    /// `LogMask`: the priorities let through.
    log_mask: i32,
}

impl State {
    /// A process's logger before any call: glibc's initialisers.
    const FRESH: Self = Self {
        log_type: SOCK_DGRAM,
        log_file: -1,
        connected: false,
        journal: false,
        log_stat: 0,
        log_tag: core::ptr::null(),
        log_facility: LOG_USER,
        log_mask: 0xff,
    };
}

/// The process's logger. Reached only through [`Locked`].
static mut STATE: State = State::FRESH;

/// glibc's `syslog_lock`, which guards [`STATE`].
static SYSLOG_LOCK: AtomicI32 = AtomicI32::new(0);

/// Holds [`SYSLOG_LOCK`]; the state is reached through it.
struct Locked;

impl Locked {
    fn take() -> Self {
        crate::lowlevellock::lll_lock(&SYSLOG_LOCK);
        Self
    }

    #[allow(clippy::unused_self)]
    fn state(&mut self) -> &mut State {
        // SAFETY: the lock is held, so this is the only reference to STATE.
        unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        crate::lowlevellock::lll_unlock(&SYSLOG_LOCK);
    }
}

// ---------------------------------------------------------------------------
// The C entry points
// ---------------------------------------------------------------------------

/// `openlog(ident, option, facility)`: set the identity (unless `ident` is
/// null), the options, and -- when `facility` is a facility and nothing more
/// -- the facility of records that name none; under `LOG_NDELAY`, connect
/// now.
///
/// `ident` is kept, not copied, as glibc keeps it: it must stay valid for as
/// long as the logger may use it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openlog(ident: *const u8, option: i32, facility: i32) {
    let mut held = Locked::take();
    openlog_internal(held.state(), ident, option, facility);
}

/// `closelog()`: close the connection, and forget the identity and the
/// kind of socket. The options, facility and mask stay, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn closelog() {
    let mut held = Locked::take();
    let st = held.state();
    closelog_internal(st);
    st.log_tag = core::ptr::null();
    st.log_type = SOCK_DGRAM;
}

/// `setlogmask(mask)`: the mask before the call; `mask` replaces it unless it
/// is 0, which only asks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setlogmask(mask: i32) -> i32 {
    let mut held = Locked::take();
    let st = held.state();
    let old = st.log_mask;
    if mask != 0 {
        st.log_mask = mask;
    }
    old
}

/// `vsyslog(priority, fmt, ap)` -- the `va_list` form, and what the `syslog`
/// trampoline calls. `ap` is read from a copy, as glibc's `va_copy` reads
/// it, so the caller's is as it was.
///
/// On the x86_64 System V ABI a `va_list` is `__va_list_tag[1]`, so the
/// parameter decays to a pointer and this function is plain Rust.
///
/// # Safety
///
/// `fmt` must be a NUL-terminated string, or null (nothing is logged), and
/// `ap` a valid `va_list` whose arguments match `fmt`'s conversions, or null
/// (every conversion reads zero).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vsyslog(priority: i32, fmt: *const u8, ap: *mut VaList) {
    // SAFETY: a non-null `ap` is a valid `va_list`, by the caller's contract.
    let va = (!ap.is_null()).then(|| unsafe { *ap });
    // SAFETY: the caller's contract on `fmt` and the `va_list`, passed on.
    unsafe { vsyslog_internal(priority, fmt, va) };
}

/// `__vsyslog_chk(priority, flag, fmt, ap)` -- what `_FORTIFY_SOURCE` makes of
/// `vsyslog`. The flag asks the formatter for glibc's fortified checks, which
/// this library's `printf` family does not make (`fortify_printf.rs`), so it
/// is accepted and ignored there as here.
///
/// # Safety
///
/// As [`vsyslog`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vsyslog_chk(priority: i32, _flag: i32, fmt: *const u8, ap: *mut VaList) {
    // SAFETY: as vsyslog's, whose contract this one shares.
    unsafe { vsyslog(priority, fmt, ap) };
}

// ---------------------------------------------------------------------------
// glibc's internals
// ---------------------------------------------------------------------------

/// glibc's `openlog_internal`.
fn openlog_internal(st: &mut State, ident: *const u8, logstat: i32, logfac: i32) {
    if !ident.is_null() {
        st.log_tag = ident;
    }
    st.log_stat = logstat;
    if logfac & !LOG_FACMASK == 0 {
        st.log_facility = logfac;
    }

    // At most twice: a second time with the other kind of socket when the
    // first is the wrong kind for what listens at /dev/log.
    for _ in 0..2 {
        // `!connected` is implied in glibc, where a connection always has a
        // socket; the journal is a connection without one.
        if st.log_file == -1 && !st.connected && st.log_stat & LOG_NDELAY != 0 {
            let old_errno = errno::get_errno();
            st.log_file = sys::socket(AF_UNIX, st.log_type | SOCK_CLOEXEC, 0);
            if st.log_file == -1 {
                if errno::get_errno() == errno::EAFNOSUPPORT {
                    // No Unix-domain sockets at all: the journal is the log
                    // (module docs). A connection made leaves `errno` alone.
                    st.connected = true;
                    st.journal = true;
                    errno::set_errno(old_errno);
                }
                return;
            }
        }
        if st.log_file != -1 && !st.connected {
            let old_errno = errno::get_errno();
            if sys::connect(st.log_file, PATH_LOG) == -1 {
                let failed = errno::get_errno();
                let fd = st.log_file;
                st.log_file = -1;
                sys::close(fd);
                errno::set_errno(old_errno);
                if failed == errno::EPROTOTYPE {
                    st.log_type = if st.log_type == SOCK_DGRAM {
                        SOCK_STREAM
                    } else {
                        SOCK_DGRAM
                    };
                    continue;
                }
            } else {
                st.connected = true;
            }
        }
        break;
    }
}

/// glibc's `closelog_internal`.
fn closelog_internal(st: &mut State) {
    if !st.connected {
        return;
    }
    // The journal is a connection without a descriptor.
    if st.log_file != -1 {
        sys::close(st.log_file);
    }
    st.log_file = -1;
    st.connected = false;
    st.journal = false;
}

/// glibc's `syslog (INTERNALLOG, "syslog: unknown facility/priority: %x",
/// pri)`. The text is built here and passed as a format with no conversions
/// in it -- hex digits hold no `%` -- rather than through a `va_list` made by
/// hand.
fn complain(pri: i32) {
    const HEAD: &[u8] = b"syslog: unknown facility/priority: ";
    let mut text = [0u8; HEAD.len() + 8 + 1];
    let mut out = Cursor::new(&mut text);
    out.put(HEAD);
    out.put_hex(pri.cast_unsigned());
    // The array's last byte stays 0 -- eight hex digits at most -- so the
    // text is terminated.
    // SAFETY: `text` is NUL-terminated and converts nothing, so no argument
    // is read.
    unsafe { vsyslog_internal(INTERNALLOG, text.as_ptr(), None) };
}

/// glibc's `__vsyslog_internal`: everything `syslog` does.
///
/// # Safety
///
/// `fmt` is a NUL-terminated string or null, and `va`, when present, a
/// `va_list` whose arguments match its conversions ([`printf::Args::new`]).
unsafe fn vsyslog_internal(pri: i32, fmt: *const u8, va: Option<VaList>) {
    let saved_errno = errno::get_errno();
    let mut pri = pri;

    // Check for invalid bits.
    if pri & !(LOG_PRIMASK | LOG_FACMASK) != 0 {
        complain(pri);
        pri &= LOG_PRIMASK | LOG_FACMASK;
    }

    let mut held = Locked::take();
    let st = held.state();

    // Check the priority against the mask.
    if log_mask(log_pri(pri)) & st.log_mask == 0 {
        return;
    }
    // The default facility, if the call names none.
    if pri & LOG_FACMASK == 0 {
        pri |= st.log_facility;
    }
    let pid = if st.log_stat & LOG_PID != 0 {
        sys::getpid()
    } else {
        0
    };
    let now = sys::now();
    let header = Header {
        pri,
        stamp: sys::local_time(now).as_ref().and_then(timestamp),
        tag: tag_of(st),
        pid,
    };

    let mut bufs = [0u8; BUFS];
    let mut heap = None;
    // SAFETY: the caller's contract on `fmt` and `va`, passed on.
    let Some(built) = (unsafe { build(&header, &mut bufs, &mut heap, saved_errno, fmt, va) })
    else {
        return;
    };
    let whole: &[u8] = match &heap {
        Some(h) => h.bytes(),
        None => &bufs,
    };
    // The record, and the NUL after it that a stream sends.
    let Some(record) = whole.get(..=built.size) else {
        return;
    };
    let Some(body) = record.get(..built.size) else {
        return;
    };

    // What standard error and the console are given: `"%s", buf + msgoff`
    // stops at a NUL in the message. Standard error's newline is left off
    // when the record -- not the copy -- ends in one.
    let copy = until_nul(body.get(built.msgoff..).unwrap_or_default());
    let end: &'static [u8] = if body.last() == Some(&b'\n') {
        b""
    } else {
        b"\n"
    };
    let perror = st.log_stat & LOG_PERROR != 0;
    if perror {
        write_line(STDERR, copy, end);
    }

    // Connected, and the record to the log.
    if !st.connected {
        let (stat, facility) = (st.log_stat, st.log_facility);
        openlog_internal(st, core::ptr::null(), stat | LOG_NDELAY, facility);
    }
    // A stream record ends in its NUL. Decided once, before either attempt.
    let wire = if st.log_type == SOCK_STREAM {
        record
    } else {
        body
    };
    let entry = Entry {
        pri,
        tag: header.tag,
        msg: body.get(built.msg_start..).unwrap_or_default(),
        now,
        copy,
        end,
        perror,
    };
    if !st.connected || !transmit(st, wire, &entry) {
        if st.connected {
            // The connection may have gone down: try a new one.
            closelog_internal(st);
            let (stat, facility) = (st.log_stat, st.log_facility);
            openlog_internal(st, core::ptr::null(), stat | LOG_NDELAY, facility);
        }
        if !st.connected || !transmit(st, wire, &entry) {
            // Opened again on the next record.
            closelog_internal(st);
            if st.log_stat & LOG_CONS != 0 {
                let fd = sys::open(PATH_CONSOLE, O_WRONLY | O_NOCTTY | O_CLOEXEC, 0);
                if fd >= 0 {
                    write_line(fd, copy, b"\r\n");
                    sys::close(fd);
                }
            }
        }
    }
}

/// Where a built record is: its length, and where in it the copies start
/// (`msgoff`, the tag) and the message does.
struct Built {
    size: usize,
    msgoff: usize,
    msg_start: usize,
}

/// glibc's building of the record: in `bufs` when it fits, in a buffer of its
/// own (`heap`) when it does not, and `out of memory[pid]` in `bufs` when
/// there is none. `None` where glibc gives up (`goto out`) -- the format
/// failing, or a record of `INT_MAX` bytes or more.
///
/// # Safety
///
/// As [`vsyslog_internal`]'s.
unsafe fn build(
    header: &Header<'_>,
    bufs: &mut [u8; BUFS],
    heap: &mut Option<HeapBuf>,
    saved_errno: i32,
    fmt: *const u8,
    va: Option<VaList>,
) -> Option<Built> {
    let l = header.len()?;
    // The header in `bufs` when it fits, and the message after it; when it
    // does not, the message is formatted at the start of `bufs` only to be
    // measured.
    let fits = l < BUFS;
    if fits {
        header.write(bufs.get_mut(..l)?);
    }
    let at = if fits { l } else { 0 };
    // `%m` is the `errno` the call began with.
    errno::set_errno(saved_errno);
    // SAFETY: the caller's contract on `fmt` and `va`.
    let vl = unsafe { format_into(bufs.get_mut(at..)?, fmt, va) };
    let vl = usize::try_from(vl).ok()?;
    let int_max = usize::try_from(i32::MAX).ok()?;
    if vl >= int_max.checked_sub(l)? {
        return None;
    }
    let size = l.checked_add(vl)?;
    if fits && vl < BUFS.checked_sub(l)? {
        return Some(Built {
            size,
            msgoff: header.msgoff(),
            msg_start: l,
        });
    }

    let Some(mut own) = HeapBuf::alloc(size.checked_add(1)?) else {
        // Nothing much to do but say so.
        let mut out = Cursor::new(bufs);
        out.put(b"out of memory[");
        out.put_dec(sys::getpid());
        out.put(b"]");
        let size = out.at;
        *bufs.get_mut(size)? = 0;
        return Some(Built {
            size,
            msgoff: 0,
            msg_start: 0,
        });
    };
    let buf = own.bytes_mut();
    header.write(buf.get_mut(..l)?);
    // Unlike glibc, `errno` is set again before the second pass too, so the
    // `%m` it expands cannot differ from the first's if anything between
    // them moved it -- glibc would drop the record when it did (`cl != vl`).
    errno::set_errno(saved_errno);
    // SAFETY: the caller's contract on `fmt` and `va`.
    let cl = unsafe { format_into(buf.get_mut(l..)?, fmt, va) };
    if usize::try_from(cl).ok()? != vl {
        return None;
    }
    *heap = Some(own);
    Some(Built {
        size,
        msgoff: header.msgoff(),
        msg_start: l,
    })
}

/// `vsnprintf (dst, sizeof dst, fmt, ap)` from a copy of `va`, as glibc's
/// `va_copy` gives each pass its own, so that each starts from the first
/// argument.
///
/// # Safety
///
/// As [`vsyslog_internal`]'s.
unsafe fn format_into(dst: &mut [u8], fmt: *const u8, va: Option<VaList>) -> i32 {
    let mut copy = va;
    // SAFETY: the caller's contract on `va`, for this copy of it.
    let mut args = unsafe { printf::Args::new(copy.as_mut()) };
    printf::_snprintf_impl(dst.as_mut_ptr(), dst.len(), fmt, &mut args)
}

/// Send `wire` on the connection -- or, connected to the journal, append
/// `entry` to it. `false` when it failed, with `errno` saying why.
fn transmit(st: &State, wire: &[u8], entry: &Entry<'_>) -> bool {
    if st.journal {
        journal_deliver(entry)
    } else {
        sys::send(st.log_file, wire) >= 0
    }
}

/// The record's tag: `openlog`'s identity, or the program's name; `(null)`,
/// as `%s` prints it, when that is null too.
fn tag_of<'a>(st: &State) -> &'a [u8] {
    let tag = if st.log_tag.is_null() {
        sys::progname()
    } else {
        st.log_tag
    };
    if tag.is_null() {
        return b"(null)";
    }
    // SAFETY: a non-null identity is a NUL-terminated string kept valid by
    // `openlog`'s caller (its contract), and the program's name is one the
    // startup code set for the life of the process.
    unsafe { core::slice::from_raw_parts(tag, crate::string::strlen(tag)) }
}

/// `s` up to its first NUL, as `%s` reads it.
fn until_nul(s: &[u8]) -> &[u8] {
    s.split(|&b| b == 0).next().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The header
// ---------------------------------------------------------------------------

/// glibc's `SYSLOG_HEADER`, `"<%d>%s%n%s%s%.0d%s: "`: the priority, the
/// timestamp, the tag, and `[pid]` when `pid` is not 0. Without a timestamp,
/// `SYSLOG_HEADER_WITHOUT_TS`'s `"<%d>: %n"`: no tag, no PID.
struct Header<'a> {
    pri: i32,
    /// `None` when `localtime_r` failed.
    stamp: Option<[u8; 16]>,
    tag: &'a [u8],
    pid: i32,
}

impl Header<'_> {
    /// Its length; `None` past `INT_MAX`, where `snprintf` fails.
    fn len(&self) -> Option<usize> {
        let mut count = Cursor::new(&mut []);
        self.put(&mut count);
        let len = count.at;
        (len <= usize::try_from(i32::MAX).ok()?).then_some(len)
    }

    /// `%n`'s position: where the copies on standard error and the console
    /// start -- the tag, or after the header without a timestamp.
    fn msgoff(&self) -> usize {
        let mut count = Cursor::new(&mut []);
        count.put(b"<");
        count.put_dec(self.pri);
        match &self.stamp {
            Some(stamp) => {
                count.put(b">");
                count.put(stamp);
            }
            None => count.put(b">: "),
        }
        count.at
    }

    /// Into `dst`, exactly [`Header::len`] bytes long.
    fn write(&self, dst: &mut [u8]) {
        self.put(&mut Cursor::new(dst));
    }

    fn put(&self, out: &mut Cursor<'_>) {
        out.put(b"<");
        out.put_dec(self.pri);
        let Some(stamp) = &self.stamp else {
            out.put(b">: ");
            return;
        };
        out.put(b">");
        out.put(stamp);
        out.put(self.tag);
        if self.pid != 0 {
            out.put(b"[");
            out.put_dec(self.pid);
            out.put(b"]");
        }
        out.put(b": ");
    }
}

/// `strftime (ts, sizeof ts, "%h %e %T ", tm)` in the C locale:
/// `Oct  1 08:26:20 `. `None` for fields outside their ranges, which
/// `localtime_r` does not produce; glibc's `strftime` would not fit them in
/// its sixteen bytes either.
fn timestamp(tm: &Tm) -> Option<[u8; 16]> {
    const MONTHS: [&[u8; 3]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    let field = |v: i32, max: u8| u8::try_from(v).ok().filter(|&v| v <= max);
    let [m0, m1, m2] = **MONTHS.get(usize::try_from(tm.tm_mon).ok()?)?;
    let day = field(tm.tm_mday, 31).filter(|&d| d >= 1)?;
    let hour = field(tm.tm_hour, 23)?;
    let min = field(tm.tm_min, 59)?;
    let sec = field(tm.tm_sec, 60)?;
    let digit = |v: u8| b'0'.wrapping_add(v % 10);
    let tens = |v: u8| digit(v / 10);
    let day_tens = if day < 10 { b' ' } else { tens(day) };
    Some([
        m0,
        m1,
        m2,
        b' ',
        day_tens,
        digit(day),
        b' ',
        tens(hour),
        digit(hour),
        b':',
        tens(min),
        digit(min),
        b':',
        tens(sec),
        digit(sec),
        b' ',
    ])
}

/// Bytes put into a buffer one piece after another. What does not fit is
/// counted and dropped, as `snprintf` counts it, so a cursor over an empty
/// buffer measures.
struct Cursor<'a> {
    buf: &'a mut [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, at: 0 }
    }

    fn put(&mut self, bytes: &[u8]) {
        let end = self.at.saturating_add(bytes.len());
        if let Some(dst) = self.buf.get_mut(self.at..end) {
            dst.copy_from_slice(bytes);
        }
        self.at = end;
    }

    /// `%d`.
    fn put_dec(&mut self, v: i32) {
        if v < 0 {
            self.put(b"-");
        }
        let mut digits = [0u8; 10];
        let mut n = v.unsigned_abs();
        let mut i = digits.len();
        loop {
            i = i.saturating_sub(1);
            if let Some(d) = digits.get_mut(i) {
                *d = b'0'.wrapping_add((n % 10) as u8);
            }
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.put(digits.get(i..).unwrap_or_default());
    }

    /// `%x`.
    fn put_hex(&mut self, v: u32) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut digits = [0u8; 8];
        let mut n = v;
        let mut i = digits.len();
        loop {
            i = i.saturating_sub(1);
            if let (Some(d), Some(&h)) = (digits.get_mut(i), HEX.get((n & 0xf) as usize)) {
                *d = h;
            }
            n >>= 4;
            if n == 0 {
                break;
            }
        }
        self.put(digits.get(i..).unwrap_or_default());
    }
}

/// A buffer from `malloc`, zeroed, and freed when dropped -- glibc frees its
/// on every way out of the call (`buf != bufs`).
struct HeapBuf {
    ptr: *mut u8,
    len: usize,
}

impl HeapBuf {
    fn alloc(len: usize) -> Option<Self> {
        let ptr = sys::alloc(len);
        if ptr.is_null() {
            return None;
        }
        // SAFETY: `ptr` is a fresh allocation of `len` bytes; zeroing it
        // makes every byte initialised before a slice is made over it.
        unsafe { core::ptr::write_bytes(ptr, 0, len) };
        Some(Self { ptr, len })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: `len` initialised bytes, owned by this buffer until drop.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as `bytes`, and `&mut self` makes this the only view.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for HeapBuf {
    fn drop(&mut self) {
        // SAFETY: `ptr` came from `sys::alloc` and is freed once, here.
        unsafe { sys::free(self.ptr) };
    }
}

/// glibc's `dprintf (fd, "%s%s", a, b)`: `a` then `b`, in one write when the
/// file takes it whole, the rest after when it does not, and given up at the
/// first error, as `dprintf` gives up. Its result is nobody's: glibc ignores
/// it too, there being nowhere left to report a logger's failure.
fn write_line(fd: i32, a: &[u8], b: &[u8]) {
    let Ok(mut skip) = usize::try_from(sys::writev2(fd, a, b)) else {
        return;
    };
    for part in [a, b] {
        let Some(mut rest) = part.get(skip..) else {
            skip = skip.saturating_sub(part.len());
            continue;
        };
        skip = 0;
        while !rest.is_empty() {
            match usize::try_from(sys::write(fd, rest)) {
                Ok(n) if n > 0 => rest = rest.get(n..).unwrap_or_default(),
                _ => return,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The journal (module docs)
// ---------------------------------------------------------------------------

/// What the journal files for one record.
struct Entry<'a> {
    /// The priority with its facility.
    pri: i32,
    tag: &'a [u8],
    /// The message: what follows the header.
    msg: &'a [u8],
    /// The record's time, in seconds since the epoch.
    now: i64,
    /// What `LOG_PERROR` writes to standard error, before its newline.
    copy: &'a [u8],
    /// Its newline, or nothing when the record ends in one.
    end: &'static [u8],
    /// Whether `LOG_PERROR` has already written it there.
    perror: bool,
}

/// The eight priorities in the spellings `journalctl` reads
/// (`journalrec::PRIORITY_NAMES`), by number.
const PRIORITY_NAMES: [&[u8]; 8] = [
    b"emerg", b"alert", b"crit", b"err", b"warning", b"notice", b"info", b"debug",
];

/// glibc's `facilitynames` (`<syslog.h>` under `SYSLOG_NAMES`), in its order:
/// a record names its facility by the first entry with its value, so `auth`
/// rather than its old alias `security` -- as `logger` names it.
const FACILITY_NAMES: [(&[u8], i32); 22] = [
    (b"auth", LOG_AUTH),
    (b"authpriv", LOG_AUTHPRIV),
    (b"cron", LOG_CRON),
    (b"daemon", LOG_DAEMON),
    (b"ftp", LOG_FTP),
    (b"kern", LOG_KERN),
    (b"lpr", LOG_LPR),
    (b"mail", LOG_MAIL),
    (b"mark", LOG_NFACILITIES << 3),
    (b"news", LOG_NEWS),
    (b"security", LOG_AUTH),
    (b"syslog", LOG_SYSLOG),
    (b"user", LOG_USER),
    (b"uucp", LOG_UUCP),
    (b"local0", LOG_LOCAL0),
    (b"local1", LOG_LOCAL1),
    (b"local2", LOG_LOCAL2),
    (b"local3", LOG_LOCAL3),
    (b"local4", LOG_LOCAL4),
    (b"local5", LOG_LOCAL5),
    (b"local6", LOG_LOCAL6),
    (b"local7", LOG_LOCAL7),
];

/// How many times an append opens the journal again after finding it
/// replaced while it waited for the lock (`journalio::RETRIES`).
const RETRIES: usize = 64;

/// Bytes of the stack buffer a record is built in; a longer one is built in
/// a buffer of its own.
const RECORD_BUF: usize = 1024;

/// Append `entry` to the journal, as the record a daemon would have filed.
/// `false` when it could not be, with `errno` saying why; a record that
/// cannot be one goes to standard error instead (module docs), and that is
/// not a failure.
fn journal_deliver(entry: &Entry<'_>) -> bool {
    // A syslog daemon takes one newline at the end of a message as the end of
    // the line rather than part of it, as glibc's own `LOG_PERROR` copy does.
    let msg = entry.msg.strip_suffix(b"\n").unwrap_or(entry.msg);
    if core::str::from_utf8(entry.tag).is_err() || core::str::from_utf8(msg).is_err() {
        if !entry.perror {
            write_line(STDERR, entry.copy, entry.end);
        }
        return true;
    }
    let fields = Fields {
        ts: u64::try_from(entry.now).unwrap_or(0),
        level: PRIORITY_NAMES
            .get(usize::try_from(log_pri(entry.pri)).unwrap_or(0))
            .copied()
            .unwrap_or(b"notice"),
        service: entry.tag,
        msg,
        pid: sys::getpid(),
        facility: FACILITY_NAMES
            .iter()
            .find(|&&(_, value)| value == entry.pri & LOG_FACMASK)
            .map(|&(name, _)| name),
    };
    let before = errno::get_errno();
    let mut count = Cursor::new(&mut []);
    fields.put(&mut count);
    let need = count.at;
    let mut small = [0u8; RECORD_BUF];
    let mut big = None;
    let line = if let Some(line) = small.get_mut(..need) {
        line
    } else if let Some(own) = HeapBuf::alloc(need) {
        big.insert(own).bytes_mut()
    } else {
        errno::set_errno(errno::ENOMEM);
        return false;
    };
    fields.put(&mut Cursor::new(&mut *line));
    match append(line) {
        Ok(()) => {
            // A record sent leaves `errno` as it was.
            errno::set_errno(before);
            true
        }
        Err(e) => {
            errno::set_errno(e);
            false
        }
    }
}

/// One journal record's fields, in `journalrec::Record::to_json_line_with`'s
/// order and spelling, with the `facility` after them as `logger` writes it.
struct Fields<'a> {
    ts: u64,
    level: &'a [u8],
    service: &'a [u8],
    msg: &'a [u8],
    /// Left out when it is not a process's.
    pid: i32,
    /// Left out when the facility has no name.
    facility: Option<&'a [u8]>,
}

impl Fields<'_> {
    /// The record and its newline.
    fn put(&self, out: &mut Cursor<'_>) {
        out.put(b"{\"ts\":");
        put_u64(out, self.ts);
        out.put(b",\"level\":\"");
        put_escaped(out, self.level);
        out.put(b"\",\"service\":\"");
        put_escaped(out, self.service);
        out.put(b"\",\"msg\":\"");
        put_escaped(out, self.msg);
        out.put(b"\"");
        if self.pid > 0 {
            out.put(b",\"pid\":");
            out.put_dec(self.pid);
        }
        if let Some(facility) = self.facility {
            out.put(b",\"facility\":\"");
            put_escaped(out, facility);
            out.put(b"\"");
        }
        out.put(b"}\n");
    }
}

/// A `u64` in decimal.
fn put_u64(out: &mut Cursor<'_>, v: u64) {
    let mut digits = [0u8; 20];
    let mut n = v;
    let mut i = digits.len();
    loop {
        i = i.saturating_sub(1);
        if let Some(d) = digits.get_mut(i) {
            *d = b'0'.wrapping_add((n % 10) as u8);
        }
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.put(digits.get(i..).unwrap_or_default());
}

/// `s`, UTF-8, as a JSON string's body: `journalrec::escape`, byte for byte.
/// A quote or a backslash is escaped, newline, return and tab by name, any
/// other control character as `\u00XX`; everything else is itself.
fn put_escaped(out: &mut Cursor<'_>, s: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in s {
        match b {
            b'"' => out.put(b"\\\""),
            b'\\' => out.put(b"\\\\"),
            b'\n' => out.put(b"\\n"),
            b'\r' => out.put(b"\\r"),
            b'\t' => out.put(b"\\t"),
            0..0x20 => {
                let hi = HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0');
                let lo = HEX.get(usize::from(b & 0xf)).copied().unwrap_or(b'0');
                out.put(&[b'\\', b'u', b'0', b'0', hi, lo]);
            }
            _ => out.put(&[b]),
        }
    }
}

/// `journalio::append`: open the journal for appending, take its exclusive
/// lock, check that the path still names the file -- a rotation or a rewrite
/// may have replaced it while this waited -- and if not, open it again;
/// then write the whole record. Where there are no locks, append without
/// one.
fn append(line: &[u8]) -> Result<(), i32> {
    let mut attempt = 0usize;
    loop {
        let fd = sys::open(
            JOURNAL_PATH,
            O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC,
            0o666,
        );
        if fd < 0 {
            return Err(errno::get_errno());
        }
        let file = Closing(fd);
        match lock(file.0) {
            Ok(()) => {}
            Err(errno::ENOLCK | errno::ENOSYS | errno::EOPNOTSUPP) => {
                return write_all(file.0, line);
            }
            Err(e) => return Err(e),
        }
        attempt = attempt.saturating_add(1);
        if attempt >= RETRIES || still_names(file.0)? {
            return write_all(file.0, line);
        }
        // Replaced while this waited: the path names another file now.
    }
}

/// A descriptor closed when dropped.
struct Closing(i32);

impl Drop for Closing {
    fn drop(&mut self) {
        sys::close(self.0);
    }
}

/// `flock (fd, LOCK_EX)`, again when a signal interrupts the wait.
fn lock(fd: i32) -> Result<(), i32> {
    loop {
        if sys::flock(fd, LOCK_EX) == 0 {
            return Ok(());
        }
        let e = errno::get_errno();
        if e != errno::EINTR {
            return Err(e);
        }
    }
}

/// Whether the journal's path still names the file `fd` has open; `false`
/// when it names nothing.
fn still_names(fd: i32) -> Result<bool, i32> {
    match sys::same_file(fd, JOURNAL_PATH) {
        Err(errno::ENOENT) => Ok(false),
        other => other,
    }
}

/// All of `data` on `fd`: again after a short write, and after a signal.
fn write_all(fd: i32, data: &[u8]) -> Result<(), i32> {
    let mut rest = data;
    while !rest.is_empty() {
        let n = sys::write(fd, rest);
        match usize::try_from(n) {
            Ok(0) => return Err(errno::EIO),
            Ok(n) => rest = rest.get(n..).unwrap_or_default(),
            Err(_) => {
                let e = errno::get_errno();
                if e != errno::EINTR {
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The system's calls -- the tests' own world in the test build
// ---------------------------------------------------------------------------

#[cfg(not(test))]
mod sys {
    use crate::socket::{AF_UNIX, MSG_NOSIGNAL, Sockaddr, SockaddrUn, SocklenT};
    use crate::time::Tm;

    pub(super) fn socket(domain: i32, ty: i32, protocol: i32) -> i32 {
        crate::socket::socket(domain, ty, protocol)
    }

    /// `connect (fd, &addr, sizeof addr)` to the `AF_UNIX` socket at `path`,
    /// NUL-terminated: glibc's `SyslogAddr` for `_PATH_LOG`, which always
    /// fits.
    pub(super) fn connect(fd: i32, path: &[u8]) -> i32 {
        let mut addr = SockaddrUn {
            sun_family: u16::try_from(AF_UNIX).unwrap_or_default(),
            sun_path: [0; 108],
        };
        if let Some(dst) = addr.sun_path.get_mut(..path.len()) {
            dst.copy_from_slice(path);
        }
        let len = SocklenT::try_from(core::mem::size_of::<SockaddrUn>()).unwrap_or(0);
        // SAFETY: `addr` is a whole `sockaddr_un`, alive for the call, and
        // `len` its size.
        unsafe { crate::socket::connect(fd, (&raw const addr).cast::<Sockaddr>(), len) }
    }

    /// `send (fd, data, len, MSG_NOSIGNAL)`.
    pub(super) fn send(fd: i32, data: &[u8]) -> isize {
        // SAFETY: `data` is a live slice of `data.len()` bytes.
        unsafe { crate::socket::send(fd, data.as_ptr(), data.len(), MSG_NOSIGNAL) }
    }

    /// `open (path, flags, mode)`; `path` is NUL-terminated.
    pub(super) fn open(path: &[u8], flags: i32, mode: u32) -> i32 {
        crate::file::open(path.as_ptr(), flags, mode)
    }

    pub(super) fn write(fd: i32, data: &[u8]) -> isize {
        crate::file::write(fd, data.as_ptr(), data.len())
    }

    /// `writev (fd, {a, b}, 2)`.
    pub(super) fn writev2(fd: i32, a: &[u8], b: &[u8]) -> isize {
        let iov = [
            crate::file::Iovec {
                iov_base: a.as_ptr().cast_mut(),
                iov_len: a.len(),
            },
            crate::file::Iovec {
                iov_base: b.as_ptr().cast_mut(),
                iov_len: b.len(),
            },
        ];
        // writev only reads through the bases, so the casts to `*mut` do not
        // license a write.
        crate::file::writev(fd, iov.as_ptr(), 2)
    }

    pub(super) fn close(fd: i32) {
        // A close that fails leaves nothing to do: the descriptor is gone
        // either way, as POSIX leaves it after `EINTR` on Linux.
        let _ = crate::file::close(fd);
    }

    pub(super) fn flock(fd: i32, op: i32) -> i32 {
        crate::file::flock(fd, op)
    }

    /// Whether `path` names the file `fd` has open: `fstat` against `stat`.
    /// `Err` carries `stat`'s or `fstat`'s `errno`.
    pub(super) fn same_file(fd: i32, path: &[u8]) -> Result<bool, i32> {
        let mut held = crate::stat::Stat::zeroed();
        if crate::file::fstat(fd, &raw mut held) != 0 {
            return Err(crate::errno::get_errno());
        }
        let mut now = crate::stat::Stat::zeroed();
        if crate::file::stat(path.as_ptr(), &raw mut now) != 0 {
            return Err(crate::errno::get_errno());
        }
        Ok(now.st_dev == held.st_dev && now.st_ino == held.st_ino)
    }

    pub(super) fn getpid() -> i32 {
        crate::process::getpid()
    }

    /// glibc's `time64_now`, which cannot fail; a clock that does is not the
    /// caller's `errno` to see.
    pub(super) fn now() -> i64 {
        let e = crate::errno::get_errno();
        let t = crate::time::time(core::ptr::null_mut());
        crate::errno::set_errno(e);
        t
    }

    /// `localtime_r (&t, &tm)`, which does not read `TZ` again.
    pub(super) fn local_time(t: i64) -> Option<Tm> {
        let mut tm = Tm::ZERO;
        crate::tz::localtime(t, &mut tm, false).then_some(tm)
    }

    /// `__progname`.
    pub(super) fn progname() -> *const u8 {
        // SAFETY: a plain read of the pointer the startup code set.
        unsafe { core::ptr::addr_of!(crate::crt::__progname).read() }
    }

    pub(super) fn alloc(len: usize) -> *mut u8 {
        crate::malloc::malloc(len)
    }

    /// # Safety
    ///
    /// `ptr` came from [`alloc`] and is freed once.
    pub(super) unsafe fn free(ptr: *mut u8) {
        // SAFETY: the caller's contract.
        unsafe { crate::malloc::free(ptr) };
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
use tests::world as sys;
