//! What `fdisk` and `sfdisk` share: util-linux 2.39.3's
//! `disk-utils/fdisk-list.c` ([`list`]), the standard output both
//! programs, and the libfdisk dialog callback they install, write to
//! ([`Term`]), and standard input read as `fgets` reads it
//! ([`fgets_stdin`], [`StdinInput`]).

pub mod list;

use std::cell::RefCell;
use std::io::BufRead;

use libfdisk::script::ScriptInput;
use ulclosestream::Stdout;
use ulcolors::Colors;

/// What [`fgets_stdin`] read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fgets {
    /// The line, through its `\n`, or as much of it as fitted; empty when
    /// nothing was read.
    pub line: Vec<u8>,
    /// Whether a read met the end of the input (`feof`).
    pub eof: bool,
}

/// `fgets (buf, bufsz, stdin)`: the next line of standard input, through its
/// `\n`, or the first `bufsz - 1` bytes of it. It reads through std's
/// buffer of standard input, which every reader in the program shares, as
/// a C program's readers share `stdin`'s: a script line and a yes-or-no
/// answer come from the one stream in order. An error -- a signal's
/// interruption included -- ends the line where it stands, as `fgets`
/// returns what it has.
#[must_use]
pub fn fgets_stdin(bufsz: usize) -> Fgets {
    let max = bufsz.saturating_sub(1);
    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    let mut got = Fgets::default();
    while got.line.len() < max {
        let Ok(data) = lock.fill_buf() else { break };
        if data.is_empty() {
            got.eof = true;
            break;
        }
        let room = max.saturating_sub(got.line.len());
        let take = data
            .iter()
            .position(|&c| c == b'\n')
            .map_or(data.len(), |i| i.saturating_add(1))
            .min(room);
        got.line
            .extend_from_slice(data.get(..take).unwrap_or_default());
        lock.consume(take);
        if got.line.last() == Some(&b'\n') {
            break;
        }
    }
    got
}

/// A libfdisk script reading standard input, through [`fgets_stdin`].
#[derive(Debug, Default)]
pub struct StdinInput {
    eof: bool,
}

impl ScriptInput for StdinInput {
    fn fgets(&mut self, buf: &mut [u8]) -> bool {
        let got = fgets_stdin(buf.len());
        self.eof |= got.eof;
        if got.line.is_empty() {
            return false;
        }
        if let Some(d) = buf.get_mut(..got.line.len()) {
            d.copy_from_slice(&got.line);
        }
        if let Some(nul) = buf.get_mut(got.line.len()) {
            *nul = 0;
        }
        true
    }

    fn feof(&self) -> bool {
        self.eof
    }
}

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

    /// A warning to standard error, as the dialog callbacks print one:
    /// standard output flushed first, and the text coloured as the `warn`
    /// scheme says (red by default) when colours are on.
    pub fn warning(&self, text: &[u8]) {
        self.flush();
        let seq = self.color_scheme_get_sequence(b"warn", ulcolors::RED);
        let (on, off) = match (seq.as_deref(), self.colors.borrow().as_ref()) {
            (Some(s), Some(c)) => (c.enable(s).to_vec(), c.disable().to_vec()),
            _ => (Vec::new(), Vec::new()),
        };
        ulclosestream::stderr_write(&[&on[..], text, &off[..]].concat());
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
