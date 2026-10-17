//! What `fdisk` and `sfdisk` share: util-linux 2.39.3's
//! `disk-utils/fdisk-list.c` ([`list`]), and the standard output both
//! programs, and the libfdisk dialog callback they install, write to
//! ([`Term`]).

pub mod list;

use std::cell::RefCell;

use ulclosestream::Stdout;
use ulcolors::Colors;

/// A program's standard output, its colours and its name: what the program
/// and its libfdisk dialog callback (which libfdisk calls with only the
/// context) both reach, as C reaches `stdout` and `ul_colors`.
pub struct Term {
    /// `stdout`.
    pub out: RefCell<Stdout>,
    /// `ul_colors`: none until `colors_init`, as in C, where nothing is
    /// coloured before it.
    pub colors: RefCell<Option<Colors>>,
    /// `program_invocation_short_name`.
    pub short: Vec<u8>,
}

impl Term {
    /// A program called `short`, writing to file descriptor 1.
    #[must_use]
    pub fn new(short: Vec<u8>) -> Self {
        Self {
            out: RefCell::new(Stdout::new(1)),
            colors: RefCell::new(None),
            short,
        }
    }

    /// `fputs (b, stdout)`.
    pub fn write(&self, b: &[u8]) {
        self.out.borrow_mut().write(b);
    }

    /// `fflush (stdout)`.
    pub fn flush(&self) {
        self.out.borrow_mut().flush();
    }

    /// `colors_wanted ()`.
    #[must_use]
    pub fn colors_wanted(&self) -> bool {
        self.colors.borrow().as_ref().is_some_and(Colors::wanted)
    }

    /// `color_scheme_get_sequence (name, dflt)`.
    #[must_use]
    pub fn color_scheme_get_sequence(&self, name: &[u8], dflt: &[u8]) -> Option<Vec<u8>> {
        self.colors
            .borrow_mut()
            .as_mut()
            .and_then(|c| c.scheme_get_sequence(name, Some(dflt)))
    }

    /// `color_scheme_enable (name, dflt)`: the scheme's sequence for `name`
    /// (or `dflt`) written, when colours are on.
    pub fn color_scheme_enable(&self, name: &[u8], dflt: &[u8]) {
        if let Some(seq) = self.color_scheme_get_sequence(name, dflt) {
            let on = self
                .colors
                .borrow()
                .as_ref()
                .map(|c| c.enable(&seq).to_vec())
                .unwrap_or_default();
            self.write(&on);
        }
    }

    /// `color_disable ()`.
    pub fn color_disable(&self) {
        let off = self
            .colors
            .borrow()
            .as_ref()
            .map(Colors::disable)
            .unwrap_or_default();
        self.write(off);
    }

    /// `exit (status)`: standard output flushed and closed as `atexit`'s
    /// `close_stdout` does it -- a write error reported and made the
    /// status -- and the process ended.
    pub fn exit(&self, status: u8) -> ! {
        let out = std::mem::replace(&mut *self.out.borrow_mut(), Stdout::new(1));
        let status = out.close(status, &self.short);
        std::process::exit(i32::from(status))
    }
}
