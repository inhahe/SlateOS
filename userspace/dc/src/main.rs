//! `dc` -- an arbitrary-precision reverse-Polish calculator: GNU dc 1.4.1
//! (bc 1.07.1), ported. The interpreter is the library (`lib.rs`); this is
//! upstream's `dc.c`: the options, the files, `~/.dcrc`, and the checks of
//! standard output at the end.

stdfdguard::guard_std_fds!();

#[cfg(unix)]
fn main() {
    // Before anything else: a standard descriptor dc was started without is
    // closed again, and SIGPIPE given back the disposition it came with.
    stdfdguard::restore();
    std::process::exit(run::main(&std::env::args_os().collect::<Vec<_>>()));
}

#[cfg(not(unix))]
fn main() {
    eprintln!("dc: runs only on a Unix host");
    std::process::exit(1);
}

#[cfg(unix)]
mod run {
    use std::ffi::{CString, OsString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    use dc::io::{Input, Output, Source};
    use dc::{Dc, System, Verdict, base_name};
    use getoptlong::{Opt, Program, Takes};

    /// `show_version`'s text.
    const VERSION: &str = "dc (GNU bc 1.07.1) 1.4.1\n\
\n\
Copyright 1994, 1997, 1998, 2000, 2001, 2003-2006, 2008, 2010, 2012-2017 Free Software Foundation, Inc.\n\
This is free software; see the source for copying conditions.  There is NO\n\
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE,\n\
to the extent permitted by law.\n";

    /// `usage`'s text after its first line.
    const USAGE: &str = "  -e, --expression=EXPR    evaluate expression\n\
\x20 -f, --file=FILE          evaluate contents of file\n\
\x20 -h, --help               display this help and exit\n\
\x20 -V, --version            output version information and exit\n\
\n";

    const SHORTOPTS: &str = "hVe:f:";
    const LONGOPTS: &[(&str, Takes)] = &[
        ("expression", Takes::Required),
        ("file", Takes::Required),
        ("help", Takes::Nothing),
        ("version", Takes::Nothing),
    ];

    /// `bug_report_info ()`: to standard output, whichever stream the usage
    /// went to.
    fn bug_report_info(dc: &mut Dc) {
        dc.out.write(b"Email bug reports to:  bug-dc@gnu.org .\n");
        dc.check_stdout();
    }

    /// `usage (f)`: to standard error when `to_stderr`, else standard output.
    fn usage(dc: &mut Dc, to_stderr: bool) {
        let mut text = b"Usage: ".to_vec();
        text.extend_from_slice(dc.progname());
        text.extend_from_slice(b" [OPTION] [file ...]\n");
        text.extend_from_slice(USAGE.as_bytes());
        if to_stderr {
            dc.err.write(&text);
        } else {
            dc.out.write(&text);
        }
        bug_report_info(dc);
        if to_stderr {
            dc.check_stderr();
        } else {
            dc.check_stdout();
        }
    }

    /// A file name for a diagnostic: its unprintable bytes escaped.
    fn shown(name: &[u8]) -> Vec<u8> {
        quoting::escape_unprintable(name).into_bytes()
    }

    /// The kinds of file `try_file` refuses, by their words, or `None` for
    /// one it reads (a regular file, a character device, a FIFO, a socket).
    fn refused_kind(fd: i32) -> Result<Option<&'static [u8]>, i32> {
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::FileTypeExt;
        // SAFETY: `fd` is open and stays open while this borrows it; the
        // `File` is never dropped, so it does not close it.
        let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
        let kind = file
            .metadata()
            .map_err(|e| e.raw_os_error().unwrap_or(libcall::EINVAL))?
            .file_type();
        Ok(if kind.is_dir() {
            Some(b"directory")
        } else if kind.is_block_device() {
            Some(b"block-special file")
        } else if kind.is_file() || kind.is_char_device() || kind.is_fifo() || kind.is_socket() {
            None
        } else {
            Some(b"file of unusual type:")
        })
    }

    /// `try_file (filename)`: evaluate a file, `-` standard input; refuse a
    /// directory, a block device and anything unusual; end the program if
    /// the evaluation ends it.
    fn try_file(dc: &mut Dc, name: &[u8]) {
        let fd = if name == b"-" {
            0
        } else {
            let opened = CString::new(name)
                .map_err(|_| libcall::ENOENT)
                .and_then(|c| {
                    libcall::file::openat(libcall::file::AT_FDCWD, &c, libcall::file::O_RDONLY, 0)
                });
            match opened {
                Ok(fd) => fd,
                Err(_) => {
                    let mut text = b"Could not open file ".to_vec();
                    text.extend_from_slice(&shown(name));
                    text.push(b'\n');
                    dc.say(&text);
                    return;
                }
            }
        };
        let close = |fd: i32| {
            if fd != 0 {
                // A file read to its end and done with: nothing to report.
                let _ = libcall::fd::close(fd);
            }
        };
        match refused_kind(fd) {
            Err(e) => {
                dc.say(b"Could not fstat file ");
                dc.perror(&shown(name), e);
                close(fd);
                return;
            }
            Ok(Some(kind)) => {
                let mut text = b"Will not attempt to process ".to_vec();
                text.extend_from_slice(kind);
                text.push(b' ');
                text.extend_from_slice(&shown(name));
                text.push(b'\n');
                dc.say(&text);
                close(fd);
                return;
            }
            Ok(None) => {}
        }
        let input = (fd != 0).then(|| Input::new(Source::Fd(fd)));
        if dc.eval_file(input) != Verdict::Success {
            dc.exit(1);
        }
        close(fd);
    }

    /// `try_rcfile ()`: `$HOME/.dcrc` (the home directory the password
    /// database gives when `HOME` is unset), evaluated if it can be opened.
    /// Ubuntu's patch. An account the database does not know has no rc file
    /// here, where upstream reads through a null pointer.
    fn try_rcfile(dc: &mut Dc) {
        let home = match std::env::var_os("HOME") {
            Some(h) => h.into_vec(),
            None => {
                let mut buf = vec![0u8; 4096];
                match libcall::pwd::home_directory(libcall::process::getuid(), &mut buf) {
                    Some(n) => {
                        buf.truncate(n);
                        buf
                    }
                    None => return,
                }
            }
        };
        let mut path = home;
        path.extend_from_slice(b"/.dcrc");
        let Ok(c) = CString::new(path) else {
            return;
        };
        let Ok(fd) = libcall::file::openat(libcall::file::AT_FDCWD, &c, libcall::file::O_RDONLY, 0)
        else {
            return;
        };
        if dc.eval_file(Some(Input::new(Source::Fd(fd)))) != Verdict::Success {
            dc.exit(1);
        }
        // Read to its end: nothing to report.
        let _ = libcall::fd::close(fd);
    }

    /// `flush_okay ()`: standard output's errors reported, standard output
    /// and error closed; the status to exit with.
    fn flush_okay(dc: &mut Dc) -> i32 {
        let mut status = 0;
        let failed: Option<(&[u8], i32)> = if dc.out.error() {
            Some((b"error writing to stdout", dc.out.errno()))
        } else if !dc.out.flush() {
            Some((b"error flushing stdout", dc.out.errno()))
        } else {
            match libcall::fd::close(1) {
                Ok(()) => None,
                Err(e) => Some((b"error closing stdout", e)),
            }
        };
        if let Some((what, e)) = failed {
            dc.say(b"");
            dc.perror(what, e);
            status = 1;
        }
        if dc.err.error() || libcall::fd::close(2).is_err() {
            status = 1;
        }
        status
    }

    /// `main`: the program, its exit status returned.
    pub fn main(args: &[OsString]) -> i32 {
        let argv0 = args
            .first()
            .map(|a| a.as_bytes().to_vec())
            .unwrap_or_default();
        let line_length = std::env::var_os("DC_LINE_LENGTH").map(OsString::into_vec);
        let mut dc = Dc::new(
            base_name(&argv0),
            Output::stdout(),
            Output::stderr(),
            Input::stdin(),
            Box::new(System),
            line_length,
        );
        try_rcfile(&mut dc);

        const DC: Program = Program::new("dc", 1);
        let words = args.get(1..).unwrap_or_default();
        let mut operands: Vec<Vec<u8>> = Vec::new();
        let mut did_eval = false;
        for item in DC.parse(words, SHORTOPTS, LONGOPTS) {
            let (code, value) = match item {
                Err(e) => {
                    // getopt names the program as it was called.
                    let mut text = argv0.clone();
                    text.extend_from_slice(b": ");
                    text.extend_from_slice(e.sentence.as_bytes());
                    text.push(b'\n');
                    dc.err.write(&text);
                    usage(&mut dc, true);
                    // `return EXIT_FAILURE`: `exit` flushes standard output,
                    // which holds the bug-report line.
                    dc.out.flush();
                    return 1;
                }
                Ok(Opt::Operand(w)) => {
                    operands.push(w.as_bytes().to_vec());
                    continue;
                }
                Ok(Opt::Short(c, v)) => (c, v),
                Ok(Opt::Long(name, v)) => (name.as_bytes().first().copied().unwrap_or(0), v),
            };
            let value = value.map(OsString::into_vec).unwrap_or_default();
            match code {
                b'e' => {
                    if dc.eval_string(&value) != Verdict::Success {
                        return flush_okay(&mut dc);
                    }
                    did_eval = true;
                }
                b'f' => {
                    try_file(&mut dc, &value);
                    did_eval = true;
                }
                b'h' => {
                    usage(&mut dc, false);
                    return flush_okay(&mut dc);
                }
                _ => {
                    dc.out.write(VERSION.as_bytes());
                    dc.check_stdout();
                    return flush_okay(&mut dc);
                }
            }
        }
        for name in &operands {
            try_file(&mut dc, name);
            did_eval = true;
        }
        if !did_eval && dc.eval_file(None) != Verdict::Success {
            dc.exit(1);
        }
        flush_okay(&mut dc)
    }
}
