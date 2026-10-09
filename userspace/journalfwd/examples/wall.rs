//! `scripts/journalfwd-diff.sh`'s way in to this crate: not a program
//! anyone runs, so an example rather than a `src/bin` that the image would
//! install.
//!
//! ```text
//! wall MESSAGE USERNAME [ORIGIN_TTY]          wall(), printing what it returned
//! forward PRIORITY IDENT|- PID|- MESSAGE      forward_wall(), as journald calls it
//! ```

use std::ffi::OsString;
use std::process::ExitCode;

#[cfg(unix)]
fn bytes(s: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn bytes(s: &OsString) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

fn main() -> ExitCode {
    let args: Vec<Vec<u8>> = std::env::args_os().skip(1).map(|a| bytes(&a)).collect();
    let arg = |i: usize| args.get(i).map(Vec::as_slice);
    match arg(0) {
        Some(b"wall") => {
            let (Some(message), Some(user)) = (arg(1), arg(2)) else {
                return ExitCode::from(2);
            };
            println!("{}", journalfwd::wall(message, user, arg(3)));
        }
        Some(b"forward") => {
            let (Some(pri), Some(ident), Some(pid), Some(message)) =
                (arg(1), arg(2), arg(3), arg(4))
            else {
                return ExitCode::from(2);
            };
            let Some(priority) = std::str::from_utf8(pri)
                .ok()
                .and_then(|p| p.parse::<u32>().ok())
            else {
                return ExitCode::from(2);
            };
            let ident = (ident != b"-").then_some(ident);
            let pid = std::str::from_utf8(pid)
                .ok()
                .and_then(|p| p.parse::<u32>().ok());
            journalfwd::forward_wall(priority, ident, message, pid);
        }
        _ => return ExitCode::from(2),
    }
    ExitCode::SUCCESS
}
