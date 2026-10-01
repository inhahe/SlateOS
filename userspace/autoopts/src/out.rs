//! Where usage and version text go (`option_usage_fp`): standard output,
//! standard error, or `--more-help`'s temporary file -- each with stdio's
//! error flag, which libopts checks after its `fflush`.

use std::fs::File;
use std::io::{self, Write};

/// One of the three streams.
pub(crate) enum Out {
    /// stdout, buffered as glibc buffers it.
    Stdout(ulclosestream::Stdout),
    /// stderr, unbuffered.
    Stderr,
    /// `--more-help`'s file, written at the flush.
    Temp {
        /// The open file.
        file: File,
        /// Written, not yet flushed.
        held: Vec<u8>,
        /// The first failed write, which sets the stream's error flag.
        failed: Option<io::Error>,
    },
}

impl Out {
    /// A new stdout stream: nothing has been written to descriptor 1 before
    /// usage or version text in any sharutils program.
    pub(crate) fn stdout() -> Self {
        Out::Stdout(ulclosestream::Stdout::new(1))
    }

    /// `fputs`.
    pub(crate) fn put(&mut self, data: &[u8]) {
        match self {
            Out::Stdout(s) => s.write(data),
            Out::Stderr => ulclosestream::stderr_write(data),
            Out::Temp { held, .. } => held.extend_from_slice(data),
        }
    }

    /// `fflush`.
    pub(crate) fn flush(&mut self) {
        match self {
            Out::Stdout(s) => s.flush(),
            Out::Stderr => {}
            Out::Temp { file, held, failed } => {
                if !held.is_empty() {
                    let r = file.write_all(held);
                    held.clear();
                    if let Err(e) = r
                        && failed.is_none()
                    {
                        *failed = Some(e);
                    }
                }
            }
        }
    }

    /// `ferror`, with the failure that set it; `None` for a stream without
    /// its error flag set.
    pub(crate) fn error(&self) -> Option<io::Error> {
        match self {
            Out::Stdout(s) => s.error().map(copy_error),
            // What a failed diagnostic's errno was cannot matter: the report
            // of it goes to the same broken stderr.
            Out::Stderr => ulclosestream::stderr_failed().then(io::Error::last_os_error),
            Out::Temp { failed, .. } => failed.as_ref().map(copy_error),
        }
    }

    /// Whether this is stdout, for the stream's name in a write error.
    pub(crate) fn is_stdout(&self) -> bool {
        matches!(self, Out::Stdout(_))
    }

    /// Whether this is stderr.
    pub(crate) fn is_stderr(&self) -> bool {
        matches!(self, Out::Stderr)
    }
}

/// An `io::Error` again, keeping the errno the message prints.
fn copy_error(e: &io::Error) -> io::Error {
    e.raw_os_error().map_or_else(
        || io::Error::new(e.kind(), e.to_string()),
        io::Error::from_raw_os_error,
    )
}
