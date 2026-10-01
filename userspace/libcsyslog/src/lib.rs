//! The C library's `syslog(3)` for Rust programs: [`openlog`], [`syslog`] and
//! [`closelog`], reached through the C ABI as the one-libc rule requires
//! (design-decisions §768).
//!
//! # Why the libc, and not a client of our own
//!
//! A program that logs "to syslog" hands its message to the C library, and
//! the library alone decides where it goes: glibc sends it to `/dev/log`, and
//! the SlateOS libc writes it to stderr until it can do the same (known-issues
//! TD-B-NOTHING-RECEIVES-SYSLOG-MESSAGES). Going through the libc keeps ONE
//! syslog client on the system, so when that client changes, every caller --
//! C and Rust alike -- changes with it, and none of them has its own idea of
//! the frame, the socket or the fallback. `logger` is the one exception, and
//! only because util-linux's `logger` is: it speaks the protocol itself, and
//! its port does too.
//!
//! # The identity's lifetime
//!
//! `openlog` keeps the POINTER it is given, not a copy -- glibc stores it as
//! `LogTag` and reads it on every later `syslog` -- so the text has to outlive
//! them all. [`openlog`] keeps its copy in a process-wide slot, and replaces a
//! previous one only after the libc has been handed the new pointer. The slot's
//! lock is also held across every [`syslog`] and [`closelog`], so no message is
//! ever formatted from an identity being freed, whatever locking the libc
//! itself does.
//!
//! # A message is data, never a format
//!
//! `syslog`'s second argument is a `printf` format. [`syslog`] always passes
//! `"%s"` and the message as its one argument, so a `%` in a message is
//! printed, not interpreted. A message stops at its first NUL byte, as any C
//! string does.
//!
//! # On the Windows host
//!
//! The unit tests run on Windows, which has no `syslog`. There a message goes
//! to stderr as `IDENT[PID]: MESSAGE` -- the shape `LOG_PERROR` gives it -- so
//! a program's logging stays visible where its tests run.

use std::ffi::CString;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// `LOG_PID`: include the process ID in each message.
pub const LOG_PID: i32 = 0x01;
/// `LOG_CONS`: write to the console when the log cannot be reached.
pub const LOG_CONS: i32 = 0x02;
/// `LOG_ODELAY`: connect on the first message (the default).
pub const LOG_ODELAY: i32 = 0x04;
/// `LOG_NDELAY`: connect at `openlog`.
pub const LOG_NDELAY: i32 = 0x08;
/// `LOG_NOWAIT`: historical; ignored by glibc.
pub const LOG_NOWAIT: i32 = 0x10;
/// `LOG_PERROR`: also write each message to stderr.
pub const LOG_PERROR: i32 = 0x20;

/// `LOG_EMERG`: the system is unusable.
pub const LOG_EMERG: i32 = 0;
/// `LOG_ALERT`: action must be taken immediately.
pub const LOG_ALERT: i32 = 1;
/// `LOG_CRIT`: critical conditions.
pub const LOG_CRIT: i32 = 2;
/// `LOG_ERR`: error conditions.
pub const LOG_ERR: i32 = 3;
/// `LOG_WARNING`: warning conditions.
pub const LOG_WARNING: i32 = 4;
/// `LOG_NOTICE`: normal but significant.
pub const LOG_NOTICE: i32 = 5;
/// `LOG_INFO`: informational.
pub const LOG_INFO: i32 = 6;
/// `LOG_DEBUG`: debug-level messages.
pub const LOG_DEBUG: i32 = 7;

/// `LOG_KERN`.
pub const LOG_KERN: i32 = 0;
/// `LOG_USER`, what `syslog` uses when `openlog` named no facility.
pub const LOG_USER: i32 = 1 << 3;
/// `LOG_MAIL`.
pub const LOG_MAIL: i32 = 2 << 3;
/// `LOG_DAEMON`: system daemons without a facility of their own.
pub const LOG_DAEMON: i32 = 3 << 3;
/// `LOG_AUTH`.
pub const LOG_AUTH: i32 = 4 << 3;
/// `LOG_SYSLOG`.
pub const LOG_SYSLOG: i32 = 5 << 3;
/// `LOG_LPR`.
pub const LOG_LPR: i32 = 6 << 3;
/// `LOG_NEWS`.
pub const LOG_NEWS: i32 = 7 << 3;
/// `LOG_UUCP`.
pub const LOG_UUCP: i32 = 8 << 3;
/// `LOG_CRON`.
pub const LOG_CRON: i32 = 9 << 3;
/// `LOG_AUTHPRIV`.
pub const LOG_AUTHPRIV: i32 = 10 << 3;
/// `LOG_FTP`.
pub const LOG_FTP: i32 = 11 << 3;
/// `LOG_LOCAL0`; `LOG_LOCAL1` .. `LOG_LOCAL7` follow at steps of `1 << 3`.
pub const LOG_LOCAL0: i32 = 16 << 3;

/// The identity the libc was last handed, kept alive for as long as it may
/// read it (module docs).
static IDENT: Mutex<Option<CString>> = Mutex::new(None);

/// The slot, whatever became of a thread that panicked holding it: the value
/// inside is always either `None` or a whole `CString`, so it stays usable.
fn ident_slot() -> MutexGuard<'static, Option<CString>> {
    IDENT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `bytes` as C sees a string: up to its first NUL.
fn c_string(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = bytes.get(..end).unwrap_or_default();
    // Cannot fail: `text` holds no NUL. Were it to, an empty string is the
    // C reading of a string that begins with one.
    CString::new(text).unwrap_or_default()
}

/// `openlog(ident, option, facility)`: name this process's messages, choose
/// their default facility, and set the `LOG_*` options. A later call replaces
/// all three, as in C.
pub fn openlog(ident: &[u8], option: i32, facility: i32) {
    let ident = c_string(ident);
    let mut slot = ident_slot();
    imp::openlog(&ident, option, facility);
    // Only now is the previous identity unused: the libc holds the new one.
    *slot = Some(ident);
}

/// `syslog(priority, "%s", message)`: one message, at `priority` -- a
/// severity, or a severity with a facility OR-ed in to override `openlog`'s.
pub fn syslog(priority: i32, message: &[u8]) {
    let message = c_string(message);
    let slot = ident_slot();
    imp::syslog(slot.as_ref(), priority, &message);
    drop(slot);
}

/// `closelog()`: close the connection to the log and forget the identity.
pub fn closelog() {
    let mut slot = ident_slot();
    imp::closelog();
    *slot = None;
}

#[cfg(unix)]
mod imp {
    use std::ffi::{CStr, CString};

    mod ffi {
        use std::ffi::{c_char, c_int};

        unsafe extern "C" {
            pub fn openlog(ident: *const c_char, option: c_int, facility: c_int);
            pub fn syslog(priority: c_int, format: *const c_char, ...);
            pub fn closelog();
        }
    }

    pub fn openlog(ident: &CStr, option: i32, facility: i32) {
        // SAFETY: `ident` is NUL-terminated, and the caller keeps it alive in
        // the identity slot until a later `openlog` or `closelog` has
        // replaced it in the libc; the options and facility are plain ints.
        unsafe { ffi::openlog(ident.as_ptr(), option, facility) }
    }

    pub fn syslog(_ident: Option<&CString>, priority: i32, message: &CStr) {
        // SAFETY: the format `"%s"` consumes exactly one `char *`, which is
        // what `message` is; both are NUL-terminated and outlive the call.
        // The identity the libc may read is kept alive by the caller's lock.
        unsafe {
            ffi::syslog(priority, c"%s".as_ptr(), message.as_ptr());
        }
    }

    pub fn closelog() {
        // SAFETY: `closelog` has no preconditions; it closes the libc's own
        // descriptor, if it has one.
        unsafe { ffi::closelog() }
    }
}

#[cfg(not(unix))]
mod imp {
    //! The Windows host has no `syslog`; messages go to stderr in
    //! `LOG_PERROR`'s shape (module docs).
    use std::borrow::Cow;
    use std::ffi::{CStr, CString};
    use std::io::Write;

    /// A byte string for the host's stderr: UTF-8 as itself, anything else
    /// with its bytes escaped rather than replaced.
    pub fn shown(bytes: &[u8]) -> Cow<'_, str> {
        match std::str::from_utf8(bytes) {
            Ok(text) => Cow::Borrowed(text),
            Err(_) => Cow::Owned(bytes.escape_ascii().to_string()),
        }
    }

    pub fn openlog(_ident: &CStr, _option: i32, _facility: i32) {}

    pub fn syslog(ident: Option<&CString>, _priority: i32, message: &CStr) {
        let ident = ident.map_or(&b""[..], |i| i.as_bytes());
        let line = format!(
            "{}[{}]: {}\n",
            shown(ident),
            std::process::id(),
            shown(message.to_bytes())
        );
        // A diagnostic that cannot be written has nowhere else to go.
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
    }

    pub fn closelog() {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn a_message_is_a_c_string_ending_at_its_first_nul() {
        assert_eq!(c_string(b"abc").as_bytes(), b"abc");
        assert_eq!(c_string(b"ab\0cd").as_bytes(), b"ab");
        assert_eq!(c_string(b"\0").as_bytes(), b"");
        assert_eq!(c_string(b"").as_bytes(), b"");
        assert_eq!(c_string(b"100% \xff").as_bytes(), b"100% \xff");
    }

    #[test]
    fn the_identity_is_kept_until_it_is_replaced_or_closed() {
        openlog(b"first", LOG_PID, LOG_DAEMON);
        assert_eq!(
            ident_slot().as_deref().map(CStr::to_bytes),
            Some(&b"first"[..])
        );
        openlog(b"second\0ignored", LOG_PID, LOG_DAEMON);
        assert_eq!(
            ident_slot().as_deref().map(CStr::to_bytes),
            Some(&b"second"[..])
        );
        // No `syslog` here: on a Unix host it would reach the real system log.
        // What the libc makes of a message is `syslog-client-check.sh`'s.
        closelog();
        assert!(ident_slot().is_none());
    }

    #[cfg(not(unix))]
    #[test]
    fn on_the_host_a_name_that_is_not_utf8_is_escaped_not_replaced() {
        assert_eq!(imp::shown(b"ntpdate"), "ntpdate");
        assert_eq!(imp::shown(b"a\xffb"), "a\\xffb");
    }
}
