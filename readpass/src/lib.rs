//! Read a secret from the terminal without showing it — or refuse to read it.
//!
//! # Why this crate exists
//!
//! Three programs prompted for a password and echoed it to the screen, and all
//! three said in a comment that they should not:
//!
//! | Program | The comment it shipped |
//! |---|---|
//! | `userspace/ftp` | "In a real terminal we would disable echo here. For now, just read a line." |
//! | `userspace/passwd` | "Attempt to disable echo. On Slate OS this would use termios ioctls." |
//! | `userspace/su` | "the terminal may not yet support disabling echo, so we read a line" |
//!
//! The comments were accurate and the user never saw them. Anyone looking at
//! the screen, and anything capturing it, saw the password — and `su` and
//! `passwd` are exactly the two prompts a shoulder-surfer wants.
//!
//! Three copies of a promise that none of them kept is the shape this tree
//! keeps finding: an enumeration that needs one entry per instance. The fix is
//! one implementation with one contract.
//!
//! # The contract, and the one case that matters
//!
//! Echo is a property of a *terminal*. When stdin is a pipe or a file there is
//! no echo and nothing on screen, so reading plainly is correct — that is how
//! a password arrives from a script, and refusing there would break it.
//!
//! The case that matters is a terminal whose echo could **not** be turned off.
//! Reading then would print the secret, so this does not read at all and
//! returns [`PassError::EchoStuckOn`]. "I could not hide it" must not be
//! silently treated as "it is hidden" — the same rule as everywhere else in
//! this tree: for a value that guards a decision, *I do not know* and *there is
//! nothing there* must not be the same.
//!
//! # Restoring echo is not optional
//!
//! A prompt that fails after turning echo off leaves the user typing into an
//! invisible shell. The restore is therefore in a `Drop` guard rather than at
//! the end of the happy path, so it also runs on the error path and on unwind.
//!
//! # Bytes, not `String`
//!
//! A password is whatever the user typed. `String` would force UTF-8 on it and
//! a lossy conversion would silently change the secret into one that does not
//! authenticate, which is a bug with no visible symptom beyond "wrong
//! password".

use std::io::{self, BufRead, Write};

/// Terminal settings as used by the `TCGETS` / `TCSETSF` ioctls.
///
/// `#[repr(C)]` and laid out to match `posix::ioctl::Termios` exactly
/// (`NCCS` = 32), because the pointer is handed to that libc.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

/// Read the terminal settings.
const TCGETS: u64 = 0x5401;
/// Write them, after flushing pending input.
///
/// `TCSETSF` rather than `TCSETS`: anything typed before echo went off is
/// still queued and would be read as part of the password — and echoed,
/// because it was typed while echo was on. Flushing drops it.
const TCSETSF: u64 = 0x5404;
/// `c_lflag` bit: echo input characters.
const ECHO: u32 = 0o10;

// Terminal control goes through the SlateOS posix libc `ioctl()` SYMBOL, not
// through `posix` as a Rust dependency. Listing `posix` in Cargo.toml compiles
// a second copy of it with `target_os = "linux"`, whose syscalls all return
// `-38` and whose `errno` is a different cell — see
// `scripts/check-one-libc-per-process.py`, which refuses that dependency. This
// is the same binding `userspace/stty` uses, for the same reason.
#[cfg(unix)]
unsafe extern "C" {
    /// posix libc `ioctl` symbol — dispatches terminal control requests.
    fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32;
}

// Host build (test-only, non-Unix). It reports FAILURE rather than pretending
// to succeed, which lands on the not-a-terminal branch below and reads
// plainly. A stub that returned 0 would claim echo was off while the console
// went on displaying every character — the precise defect this crate removes.
#[cfg(not(unix))]
unsafe fn ioctl(_fd: i32, _request: u64, _arg: *mut u8) -> i32 {
    -1
}

/// Why a secret could not be read.
#[derive(Debug)]
pub enum PassError {
    /// stdin is a terminal and echo could not be turned off. **Nothing was
    /// read**, because reading would have displayed the secret.
    EchoStuckOn(String),
    /// The read itself failed.
    Io(String),
}

impl std::fmt::Display for PassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EchoStuckOn(e) => write!(
                f,
                "cannot turn off terminal echo ({e}); refusing to read a password that would be displayed"
            ),
            Self::Io(e) => write!(f, "cannot read: {e}"),
        }
    }
}

/// Puts `c_lflag` back the way it was, on every path out.
struct EchoGuard {
    fd: i32,
    saved: Termios,
    armed: bool,
}

impl Drop for EchoGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut t = self.saved;
        let p: *mut Termios = &raw mut t;
        // SAFETY: `p` points to a live, correctly-typed `Termios` for the
        // duration of the call, and `TCSETSF` only reads through it. The
        // result is deliberately discarded: this runs while unwinding from an
        // error that is already being reported, and there is no second action
        // available if the terminal will not take its old settings back.
        let _ = unsafe { ioctl(self.fd, TCSETSF, p.cast::<u8>()) };
    }
}

/// Whether `fd` is a terminal, and its current settings if so.
///
/// `None` means "not a terminal, or it cannot be asked" — both of which mean
/// there is no echo to suppress, because there is no screen involved.
fn terminal_settings(fd: i32) -> Option<Termios> {
    let mut t = Termios::default();
    let p: *mut Termios = &raw mut t;
    // SAFETY: `p` points to a live, correctly-typed `Termios` that outlives
    // the call, and `TCGETS` writes at most `size_of::<Termios>()` bytes
    // through it. The layout matches `posix::ioctl::Termios`.
    let rc = unsafe { ioctl(fd, TCGETS, p.cast::<u8>()) };
    if rc == 0 { Some(t) } else { None }
}

/// Read one line from stdin with terminal echo off, returning it without the
/// line ending.
///
/// `prompt` is written to **stderr**, so it is still seen when stdout is
/// redirected — which is the normal way these programs are used.
///
/// # Errors
///
/// [`PassError::EchoStuckOn`] if stdin is a terminal whose echo could not be
/// turned off; nothing is read in that case. [`PassError::Io`] if the read
/// fails.
pub fn read_password(prompt: &[u8]) -> Result<Vec<u8>, PassError> {
    let mut err = io::stderr();
    // A prompt that is not on screen yet is a program that looks hung.
    let _ = err.write_all(prompt);
    let _ = err.flush();

    let fd = 0; // stdin
    let mut guard = EchoGuard {
        fd,
        saved: Termios::default(),
        armed: false,
    };

    if let Some(saved) = terminal_settings(fd) {
        let mut quiet = saved;
        quiet.c_lflag &= !ECHO;
        let p: *mut Termios = &raw mut quiet;
        // SAFETY: `p` points to a live, correctly-typed `Termios` for the
        // duration of the call; `TCSETSF` only reads through it.
        let rc = unsafe { ioctl(fd, TCSETSF, p.cast::<u8>()) };
        if rc != 0 {
            return Err(PassError::EchoStuckOn(format!(
                "ioctl(TCSETSF) returned {rc}"
            )));
        }
        guard.saved = saved;
        guard.armed = true;
    }
    // else: not a terminal. Nothing is being displayed, so there is nothing to
    // suppress, and refusing here would break every script that pipes one in.

    let mut line: Vec<u8> = Vec::new();
    let read = io::stdin()
        .lock()
        .read_until(b'\n', &mut line)
        .map_err(|e| PassError::Io(e.to_string()));

    // The user's Return was not echoed, so the cursor is still on the prompt
    // line. Emitted before `?` so it happens whether or not the read failed.
    if guard.armed {
        let _ = err.write_all(b"\n");
        let _ = err.flush();
    }
    read?;

    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_bit_is_cleared_not_assigned() {
        // `c_lflag &= !ECHO` must leave every other flag alone. Assigning
        // instead would silently turn off canonical mode and signal handling,
        // so Ctrl-C would stop working during the prompt.
        const ISIG: u32 = 0o1;
        const ICANON: u32 = 0o2;
        const ECHOE: u32 = 0o20;
        let mut t = Termios {
            c_lflag: ISIG | ICANON | ECHO | ECHOE,
            ..Termios::default()
        };
        t.c_lflag &= !ECHO;
        assert_eq!(t.c_lflag & ECHO, 0, "echo must be off");
        // Everything else survives. ISIG in particular: clearing it would stop
        // Ctrl-C working while the prompt is up.
        assert_eq!(t.c_lflag, ISIG | ICANON | ECHOE);
    }

    #[test]
    fn termios_layout_matches_the_libc() {
        // The pointer is handed to posix's `ioctl`, which casts it back to
        // `posix::ioctl::Termios`. NCCS is 32 there.
        assert_eq!(core::mem::size_of::<Termios>(), 4 * 4 + 1 + 32 + 3 + 4 + 4);
        assert_eq!(core::mem::align_of::<Termios>(), 4);
    }

    #[test]
    fn guard_does_nothing_until_armed() {
        // An unarmed guard must not issue an ioctl: on the not-a-terminal path
        // there is no saved state, and writing `Termios::default()` to a
        // terminal would zero every setting it has.
        let g = EchoGuard {
            fd: 0,
            saved: Termios::default(),
            armed: false,
        };
        drop(g); // must not panic, must not touch fd 0
    }

    #[test]
    fn error_text_says_nothing_was_read() {
        let e = PassError::EchoStuckOn("ioctl(TCSETSF) returned -1".to_string());
        let s = e.to_string();
        assert!(s.contains("refusing"), "{s}");
        assert!(s.contains("displayed"), "{s}");
    }

    #[test]
    fn not_a_terminal_on_this_host() {
        // The host stub returns -1, so this must report "no terminal" rather
        // than claim it configured one.
        assert!(terminal_settings(0).is_none() || cfg!(unix));
    }
}
