//! util-linux 2.39.3's `agetty`, ported: open a terminal line, set its
//! modes, show the issue file, ask for a login name, and run `login(1)`
//! with it -- what a console or serial login runs before anyone has signed
//! in. As Ubuntu 24.04 builds it, `term-utils/agetty.c` function by
//! function, with its diagnostics going to syslog as there.
//!
//! The port is in stages (`roadmap.md`, lane B's agetty item), each held to
//! Ubuntu's `/usr/sbin/agetty` by `scripts/agetty-diff.sh` before the next:
//!
//! | stage | what | here |
//! |---|---|---|
//! | 1 | the command line, `--help`, `--version`, `--list-speeds`, `--reload`'s trigger, `--show-issue` and the issue file's escapes | [`options`], [`speeds`], [`issue`] |
//! | 2 | the line opened and set up, the name read, `login` run | [`run`], [`termio`], [`logname`], [`prompt`], [`login`], [`logindefs`], [`ttys`] |
//! | 3 | the `LOGIN_PROCESS` record in `utmp` and `wtmp` | [`utmp`] |
//! | 4 | the issue and prompt shown again on `--reload`, or when an address the issue shows changes | [`run`]'s `Reload`, [`issue::issue_is_changed`] |
//!
//! Until it ships, the `agetty` binary is built only with the `port`
//! feature, and `userspace/getty`, which it replaces, serves.
//!
//! # Deliberate differences
//!
//! 1. **`--version` lists no `plymouth`**: SlateOS has no boot splash for
//!    agetty to talk to.
//! 2. **`-` as the line, with no terminal on the standard streams, keeps
//!    `-`** where upstream sets the name to NULL and then reads it.
//! 3. **`\S{}` in the issue file shows nothing.** Upstream's scan of
//!    `os-release` for an empty name never advances, and the getty hangs
//!    before its prompt.
//! 4. **The end of the line's input ends the getty even with `--erase-chars`
//!    or `--kill-chars`.** Upstream reads its 0 as a NUL, and a NUL is in
//!    every string `strchr` searches, so it is taken for an erase or a kill
//!    -- on a line that hung up, again and again at full speed.
//! 5. **The netlink socket that hears address changes is closed on `exec`,
//!    and its address is the kernel's choice.** Upstream's is inherited by
//!    `login` and the shell after it, which never read it; and upstream
//!    binds its process id, which the C library's own route socket -- made
//!    by `getaddrinfo` for `\4` -- may already hold, and then hears nothing.

pub mod issue;
pub mod login;
pub mod logindefs;
pub mod logname;
pub mod options;
pub mod prompt;
#[cfg(unix)]
pub mod run;
pub mod speeds;
pub mod termio;
pub mod ttys;
pub mod utmp;

/// `_PATH_LOGIN`: the login program.
pub const PATH_LOGIN: &[u8] = b"/bin/login";

/// `AGETTY_RELOAD_FILENAME`: what `--reload` touches, and a getty waiting
/// for a name watches.
pub const RELOAD_FILE: &core::ffi::CStr = c"/run/agetty.reload";

/// `%m`'s text for `errno`.
#[must_use]
pub fn strerror(errno: i32) -> String {
    errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

/// C's `isspace` in the "C" locale.
pub(crate) fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// How a run ends early: the status to exit with, after whatever was said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit(pub u8);

/// Where agetty's complaints go: standard error for the ones upstream
/// prints, syslog for its `log_warn` and `log_err` -- a fake in the tests.
pub trait Diag {
    /// glibc `getopt`'s own complaint, under `argv[0]`.
    fn getopt(&mut self, sentence: &[u8]);
    /// `errtryhelp (EXIT_FAILURE)`.
    fn errtryhelp(&mut self) -> Exit;
    /// `errx (EXIT_FAILURE, "%s", msg)`.
    fn errx(&mut self, msg: &[u8]) -> Exit;
    /// `log_warn (msg)`: syslog, `LOG_WARNING`.
    fn log_warn(&mut self, msg: &[u8]);
    /// `log_err (msg)`: syslog, `LOG_ERR`, and then -- "be kind to init" --
    /// ten seconds' sleep before the exit.
    fn log_err(&mut self, msg: &[u8]) -> Exit;
    /// `exit_slowly (code)`: the ten seconds' sleep, and then the exit with
    /// `code` -- a getty that ends at once is started again at once, and
    /// init would spin on it.
    fn exit_slowly(&mut self, code: u8) -> Exit;
}

/// The real thing.
#[derive(Clone)]
pub struct System {
    /// `argv[0]`, as getopt names the program.
    pub argv0: Vec<u8>,
    /// `program_invocation_short_name`: `argv[0]` after its last `/`.
    pub short: Vec<u8>,
}

impl System {
    /// The program started as `argv0`.
    #[must_use]
    pub fn new(argv0: &[u8]) -> Self {
        let short = argv0
            .rsplit(|&b| b == b'/')
            .next()
            .unwrap_or(argv0)
            .to_vec();
        Self {
            argv0: argv0.to_vec(),
            short,
        }
    }

    /// `dolog (priority, fmt, ap)`: one message to syslog, under the short
    /// name, with the PID, as `LOG_AUTHPRIV`.
    fn dolog(&self, priority: i32, msg: &[u8]) {
        libcsyslog::openlog(&self.short, libcsyslog::LOG_PID, libcsyslog::LOG_AUTHPRIV);
        libcsyslog::syslog(priority, msg);
        libcsyslog::closelog();
    }
}

impl Diag for System {
    fn getopt(&mut self, sentence: &[u8]) {
        let mut m = self.argv0.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(sentence);
        m.push(b'\n');
        ulclosestream::stderr_write(&m);
    }

    fn errtryhelp(&mut self) -> Exit {
        let mut m = b"Try '".to_vec();
        m.extend_from_slice(&self.short);
        m.extend_from_slice(b" --help' for more information.\n");
        ulclosestream::stderr_write(&m);
        Exit(1)
    }

    fn errx(&mut self, msg: &[u8]) -> Exit {
        let mut m = self.short.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(msg);
        m.push(b'\n');
        ulclosestream::stderr_write(&m);
        Exit(1)
    }

    fn log_warn(&mut self, msg: &[u8]) {
        self.dolog(libcsyslog::LOG_WARNING, msg);
    }

    fn log_err(&mut self, msg: &[u8]) -> Exit {
        self.dolog(libcsyslog::LOG_ERR, msg);
        self.exit_slowly(1)
    }

    fn exit_slowly(&mut self, code: u8) -> Exit {
        // `sleep (10)`: a signal that cuts it short ends the wait early.
        let _ = libcall::clock::nanosleep(10, 0);
        Exit(code)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{Diag, Exit};

    /// What a run said, and where.
    #[derive(Default, Debug)]
    pub(crate) struct Said {
        pub(crate) stderr: Vec<u8>,
        pub(crate) syslog: Vec<Vec<u8>>,
        /// Whether a `log_err` would have slept before the exit.
        pub(crate) slow_exit: bool,
    }

    impl Diag for Said {
        fn getopt(&mut self, sentence: &[u8]) {
            self.stderr.extend_from_slice(b"agetty: ");
            self.stderr.extend_from_slice(sentence);
            self.stderr.push(b'\n');
        }
        fn errtryhelp(&mut self) -> Exit {
            self.stderr
                .extend_from_slice(b"Try 'agetty --help' for more information.\n");
            Exit(1)
        }
        fn errx(&mut self, msg: &[u8]) -> Exit {
            self.stderr.extend_from_slice(b"agetty: ");
            self.stderr.extend_from_slice(msg);
            self.stderr.push(b'\n');
            Exit(1)
        }
        fn log_warn(&mut self, msg: &[u8]) {
            self.syslog.push(msg.to_vec());
        }
        fn log_err(&mut self, msg: &[u8]) -> Exit {
            self.syslog.push(msg.to_vec());
            self.exit_slowly(1)
        }
        fn exit_slowly(&mut self, code: u8) -> Exit {
            self.slow_exit = true;
            Exit(code)
        }
    }

    #[test]
    fn the_short_name_is_argv0_after_its_last_slash() {
        assert_eq!(super::System::new(b"/usr/sbin/agetty").short, b"agetty");
        assert_eq!(super::System::new(b"getty").short, b"getty");
    }
}
