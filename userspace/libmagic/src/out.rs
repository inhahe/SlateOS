//! Standard output, buffered as C's stdio buffers it: by line on a terminal,
//! in blocks otherwise -- so that `file`'s answers and its warnings, which go
//! to stderr unbuffered, come out in upstream's order when both are captured.
//! A warning flushes this first, as upstream's `file_magwarn` does.

use std::cell::RefCell;
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::mem::ManuallyDrop;

/// `BUFSIZ`: the block a fully buffered stream writes at a time.
const BUFSIZ: usize = 8192;

struct Out {
    file: ManuallyDrop<File>,
    buf: Vec<u8>,
    line: bool,
    failed: bool,
}

thread_local! {
    static OUT: RefCell<Option<Out>> = const { RefCell::new(None) };
}

/// Descriptor 1 as a `File`, not to be closed.
fn stdout_file() -> ManuallyDrop<File> {
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        // SAFETY: descriptor 1 is open for the life of the process (the
        // standard-descriptor guard sees to that), and `ManuallyDrop` keeps
        // this `File` from closing it.
        ManuallyDrop::new(unsafe { File::from_raw_fd(1) })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let h = std::io::stdout().as_raw_handle();
        // SAFETY: the process's standard-output handle stays open, and
        // `ManuallyDrop` keeps this `File` from closing it.
        ManuallyDrop::new(unsafe { File::from_raw_handle(h) })
    }
}

/// Set the buffering from what standard output is.
pub fn init() {
    let line = std::io::stdout().is_terminal();
    OUT.with(|o| {
        *o.borrow_mut() = Some(Out {
            file: stdout_file(),
            buf: Vec::with_capacity(BUFSIZ),
            line,
            failed: false,
        });
    });
}

fn drain(o: &mut Out) {
    if o.buf.is_empty() {
        return;
    }
    if o.file.write_all(&o.buf).is_err() {
        o.failed = true;
    }
    o.buf.clear();
}

/// Write bytes to standard output.
pub fn write(bytes: &[u8]) {
    OUT.with(|o| {
        let mut g = o.borrow_mut();
        let Some(o) = g.as_mut() else {
            let _written = std::io::stdout().write_all(bytes);
            return;
        };
        if o.line {
            o.buf.extend_from_slice(bytes);
            if bytes.contains(&b'\n') {
                drain(o);
            }
            return;
        }
        for &c in bytes {
            o.buf.push(c);
            if o.buf.len() >= BUFSIZ {
                drain(o);
            }
        }
    });
}

/// `fflush(stdout)`: whether everything written so far got out.
pub fn flush() -> bool {
    OUT.with(|o| {
        let mut g = o.borrow_mut();
        match g.as_mut() {
            Some(o) => {
                drain(o);
                !o.failed
            }
            None => std::io::stdout().flush().is_ok(),
        }
    })
}

/// Whether a write has failed.
pub fn failed() -> bool {
    OUT.with(|o| o.borrow().as_ref().is_some_and(|o| o.failed))
}
