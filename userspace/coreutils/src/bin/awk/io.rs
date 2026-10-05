//! Reading records and writing to redirections.
//!
//! ## Records are not lines
//!
//! `RS` decides where a record ends, and it has three modes rather than one: a
//! single character (the usual `\n`), the empty string (paragraph mode, where
//! records are separated by blank lines *and* a newline becomes a field
//! separator whatever `FS` says), and — every awk in use supports this — a
//! regular expression. So the reader cannot be `BufRead::lines`, and it cannot
//! decode: a record that is not UTF-8 has to reach the program unchanged.
//!
//! ## Redirections are keyed by their *name*
//!
//! `print > "out"` truncates `out` the first time and appends on every later
//! write in the same run — because the file stays open, not because the second
//! write is an append. That is why these are kept in a table keyed by the
//! evaluated target string: `print > ("out" i)` genuinely opens one file per
//! value of `i`, and each keeps its own position.

use crate::value::Str;
use ere::Regex;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

/// How records are separated.
#[derive(Clone)]
pub enum Rs {
    /// The usual case: one byte, normally `\n`.
    Char(u8),
    /// `RS = ""` — records are separated by runs of blank lines.
    Paragraph,
    /// `RS` longer than one character, read as a regular expression.
    Regex(Rc<Regex>),
}

/// How much input is buffered before a record separator has to be found. A
/// record longer than this is still read — the buffer grows — but a file with
/// no separator in it does not turn into an unbounded read *per refill*.
const CHUNK: usize = 64 * 1024;

/// A stream of records.
pub struct Records {
    src: Box<dyn Read>,
    buf: Vec<u8>,
    /// How much of `buf` has been handed out already.
    pos: usize,
    eof: bool,
}

impl Records {
    pub fn new(src: Box<dyn Read>) -> Records {
        Records {
            src,
            buf: Vec::new(),
            pos: 0,
            eof: false,
        }
    }

    /// Read more input. Returns false at end of file.
    fn fill(&mut self) -> io::Result<bool> {
        if self.eof {
            return Ok(false);
        }
        // Drop the consumed prefix rather than letting the buffer grow for the
        // whole file; a 2 GiB input read one line at a time must not become a
        // 2 GiB allocation.
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > CHUNK {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let start = self.buf.len();
        self.buf.resize(start.saturating_add(CHUNK), 0);
        let n = match self.src.read(self.buf.get_mut(start..).unwrap_or_default()) {
            Ok(n) => n,
            Err(e) => {
                self.buf.truncate(start);
                return Err(e);
            }
        };
        self.buf.truncate(start.saturating_add(n));
        if n == 0 {
            self.eof = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn rest(&self) -> &[u8] {
        self.buf.get(self.pos..).unwrap_or_default()
    }

    /// The next record, or `None` at end of input.
    ///
    /// # Errors
    /// Propagates a read failure; awk turns it into `getline`'s -1 or a fatal
    /// diagnostic depending on where the read came from.
    pub fn next(&mut self, rs: &Rs) -> io::Result<Option<Str>> {
        match rs {
            Rs::Char(c) => self.next_delimited(*c),
            Rs::Paragraph => self.next_paragraph(),
            Rs::Regex(re) => self.next_by_regex(re),
        }
    }

    fn next_delimited(&mut self, sep: u8) -> io::Result<Option<Str>> {
        let mut searched = 0usize;
        loop {
            let rest = self.rest();
            if let Some(k) = rest
                .get(searched..)
                .and_then(|r| r.iter().position(|b| *b == sep))
            {
                let end = searched.saturating_add(k);
                let rec = rest.get(..end).unwrap_or_default().to_vec();
                self.pos = self.pos.saturating_add(end).saturating_add(1);
                return Ok(Some(rec));
            }
            searched = rest.len();
            if !self.fill()? {
                let rest = self.rest();
                if rest.is_empty() {
                    return Ok(None);
                }
                // The last record of a file with no trailing separator is still
                // a record.
                let rec = rest.to_vec();
                self.pos = self.buf.len();
                return Ok(Some(rec));
            }
        }
    }

    /// Paragraph mode: a record is text between runs of blank lines, with
    /// leading blank lines skipped and the trailing newline dropped.
    fn next_paragraph(&mut self) -> io::Result<Option<Str>> {
        // Skip leading newlines.
        loop {
            while self.rest().first() == Some(&b'\n') {
                self.pos = self.pos.saturating_add(1);
            }
            if !self.rest().is_empty() || !self.fill()? {
                break;
            }
        }
        if self.rest().is_empty() && self.eof {
            return Ok(None);
        }
        let mut searched = 0usize;
        loop {
            let rest = self.rest();
            if let Some(k) = find(rest.get(searched..).unwrap_or_default(), b"\n\n") {
                let end = searched.saturating_add(k);
                let rec = rest.get(..end).unwrap_or_default().to_vec();
                let mut after = end.saturating_add(1);
                // Consume the whole run of blank lines, not just the first.
                while rest.get(after) == Some(&b'\n') {
                    after = after.saturating_add(1);
                }
                self.pos = self.pos.saturating_add(after);
                return Ok(Some(rec));
            }
            searched = rest.len().saturating_sub(1);
            if !self.fill()? {
                let rest = self.rest();
                if rest.is_empty() {
                    return Ok(None);
                }
                let rec = rest.strip_suffix(b"\n").unwrap_or(rest).to_vec();
                self.pos = self.buf.len();
                return Ok(Some(rec));
            }
        }
    }

    /// A record separator search that gave up is reported as an I/O error.
    ///
    /// `RS` is a regex the program chose, so one with a backreference can
    /// exhaust the matcher's budget. This path has no way to say "I do not
    /// know where the record ends" other than failing the read: carrying on
    /// would silently glue two records into one.
    fn limit_err(e: ere::MatchLimit) -> io::Error {
        io::Error::other(format!("RS: {e}"))
    }

    fn next_by_regex(&mut self, re: &Regex) -> io::Result<Option<Str>> {
        loop {
            let rest = self.rest();
            // A match that runs to the end of what has been read might get
            // longer with more input — `RS = "ab*"` against `a` followed by
            // more `b`s — so only a match that ends before the end is
            // trusted, unless there is no more input.
            if let Some((s, e)) = re.find(rest).map_err(Self::limit_err)?
                && (e < rest.len() || self.eof)
                && e > s
            {
                let rec = rest.get(..s).unwrap_or_default().to_vec();
                self.pos = self.pos.saturating_add(e);
                return Ok(Some(rec));
            }
            if !self.fill()? {
                let rest = self.rest();
                if rest.is_empty() {
                    return Ok(None);
                }
                if let Some((s, e)) = re.find(rest).map_err(Self::limit_err)?
                    && e > s
                {
                    let rec = rest.get(..s).unwrap_or_default().to_vec();
                    self.pos = self.pos.saturating_add(e);
                    return Ok(Some(rec));
                }
                let rec = rest.to_vec();
                self.pos = self.buf.len();
                return Ok(Some(rec));
            }
        }
    }
}

/// The first position of `needle` in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// One place output goes.
enum Sink {
    /// A file, and whether it is a terminal: gawk's `RED_NOBUF`, which flushes
    /// it after every print so a redirection to `/dev/tty` is seen at once.
    File(BufWriter<File>, bool),
    /// A command reading our output. The child is kept so `close` can wait for
    /// it and report its status, as awk's `close` is specified to.
    Pipe(BufWriter<std::process::ChildStdin>, Child),
    Stdout,
    Stderr,
}

/// The redirections a program has open, and the standard streams.
pub struct Outputs {
    stdout: BufWriter<io::Stdout>,
    /// Standard output is a terminal: gawk's `output_is_tty`, which flushes it
    /// after every print, so a line typed at an interactive `awk` comes back
    /// when it is processed rather than when 8 KiB have built up.
    stdout_tty: bool,
    sinks: HashMap<Str, Sink>,
    /// The order the redirections were opened in. At exit they are closed
    /// newest first, as gawk walks its list (each new one goes on the front),
    /// which decides which failure is the one reported when two fail.
    order: Vec<Str>,
}

/// Why output to a redirection failed.
pub enum OutError {
    /// It could not be opened: gawk's `cannot redirect to ...`.
    Open(io::Error),
    /// Writing to it failed -- for a buffered one, the flush a write caused:
    /// gawk's `print to "..." failed`.
    Write(io::Error),
}

/// What `fflush()` with no name, and `system()` before it runs, met.
pub enum FlushError {
    /// Standard output would not flush.
    Stdout(io::Error),
    /// A redirection would not flush; `pipe` says whether it is a command.
    Sink {
        name: Str,
        pipe: bool,
        err: io::Error,
    },
}

impl Outputs {
    #[must_use]
    pub fn new() -> Outputs {
        Outputs {
            stdout: BufWriter::new(io::stdout()),
            stdout_tty: coreutils::stdfd::is_tty(1),
            sinks: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Write to standard output.
    ///
    /// # Errors
    /// Propagates the write failure. awk cannot carry on after one — a report
    /// with a hole in it is worse than no report — so callers make it fatal.
    pub fn write_stdout(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stdout.write_all(bytes)?;
        if self.stdout_tty {
            self.stdout.flush()?;
        }
        Ok(())
    }

    /// Write to a redirection, opening it if this is its first use.
    ///
    /// # Errors
    /// [`OutError::Open`] for a redirection that would not open,
    /// [`OutError::Write`] for a write that failed.
    pub fn write_to(
        &mut self,
        name: &[u8],
        mode: crate::ast::RedirMode,
        bytes: &[u8],
    ) -> Result<(), OutError> {
        if !self.sinks.contains_key(name) {
            // A pipe's child inherits our standard output, so anything already
            // buffered has to be on its way out before the child can write.
            // A failure here is standard output's, and the next write to it,
            // or the exit, reports it; it is not this redirection's.
            let _ = self.stdout.flush();
            let sink = open_sink(name, mode).map_err(OutError::Open)?;
            self.sinks.insert(name.to_vec(), sink);
            self.order.push(name.to_vec());
        }
        let written = match self.sinks.get_mut(name) {
            Some(Sink::File(f, tty)) => f
                .write_all(bytes)
                .and_then(|()| if *tty { f.flush() } else { Ok(()) }),
            Some(Sink::Pipe(w, _)) => w.write_all(bytes),
            Some(Sink::Stdout) => self.write_stdout(bytes),
            Some(Sink::Stderr) => {
                // stderr is unbuffered, as C's is, so a diagnostic appears when
                // it is written rather than when awk exits.
                //
                // The raw `write(2)` and not `io::stderr()`, which answers `Ok`
                // to a write that never happened: the runtime maps `EBADF` on a
                // standard descriptor to success, so `print > "/dev/stderr"`
                // with descriptor 2 closed would look like it had worked. The
                // error is returned rather than recorded because this is awk's
                // *output* and the caller already reports it — measured,
                // `gawk '{print > "/dev/stderr"}' f 2>/dev/full` is status 2.
                coreutils::stdfd::write_all(2, bytes)
            }
            None => Ok(()),
        };
        written.map_err(OutError::Write)
    }

    /// Close one redirection, flushing it first as gawk's `close_redir` does.
    /// `None` if nothing by that name is open for output.
    ///
    /// A pipe's command is waited for, but its status is not the result:
    /// under `--posix` gawk's `close` is 0 once the close succeeds, whatever
    /// the command exited with.
    pub fn close(&mut self, name: &[u8]) -> Option<io::Result<()>> {
        let sink = self.sinks.remove(name)?;
        self.order.retain(|n| n != name);
        Some(finish(sink))
    }

    /// Flush standard output alone: what gawk's `err()` does before it writes
    /// a diagnostic (`fflush(output_fp)`), so that a warning lands after the
    /// lines printed before it when both go to one place.
    ///
    /// # Errors
    /// Propagates the flush failure.
    pub fn flush_stdout(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }

    /// Flush standard output and then every redirection: gawk's `flush_io`,
    /// which `fflush()` is and which `system()` does first.
    ///
    /// # Errors
    /// The first failure, standard output's before any redirection's: each is
    /// gawk's fatal error, so the caller stops at it.
    pub fn flush_all(&mut self) -> Result<(), FlushError> {
        self.stdout.flush().map_err(FlushError::Stdout)?;
        for name in &self.order {
            if let Some(sink) = self.sinks.get_mut(name)
                && let Err(err) = flush_sink(sink)
            {
                return Err(FlushError::Sink {
                    name: name.clone(),
                    pipe: matches!(sink, Sink::Pipe(..)),
                    err,
                });
            }
        }
        Ok(())
    }

    /// `fflush(name)` for an output redirection; `None` if `name` is not one.
    pub fn flush_one(&mut self, name: &[u8]) -> Option<io::Result<()>> {
        let sink = self.sinks.get_mut(name)?;
        Some(match sink {
            Sink::Stdout => self.stdout.flush(),
            other => flush_sink(other),
        })
    }

    /// Close every redirection at exit, newest first, as gawk's `close_io`
    /// walks its list.
    ///
    /// # Errors
    /// The first one that would not flush, with its name: gawk's fatal `flush
    /// to "..." failed`, which ends the closing there.
    pub fn finish_redirects(&mut self) -> Result<(), (Str, io::Error)> {
        while let Some(name) = self.order.pop() {
            if let Some(sink) = self.sinks.remove(&name) {
                finish(sink).map_err(|e| (name, e))?;
            }
        }
        Ok(())
    }

    /// Flush standard output at exit, after the redirections.
    ///
    /// # Errors
    /// The flush failure: gawk warns about it and exits 1 unless the program
    /// said `exit`.
    pub fn finish_stdout(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }

    /// What a fatal error leaves behind: C's `exit` flushes every stream it
    /// has, saying nothing about any that fail, and so does this.
    pub fn finish_quietly(&mut self) {
        while let Some(name) = self.order.pop() {
            if let Some(sink) = self.sinks.remove(&name) {
                // The run is already ending with a diagnostic; a second one
                // about the same full disk would be noise, as it is in gawk.
                let _ = finish(sink);
            }
        }
        let _ = self.stdout.flush();
    }
}

impl Default for Outputs {
    fn default() -> Outputs {
        Outputs::new()
    }
}

fn flush_sink(sink: &mut Sink) -> io::Result<()> {
    match sink {
        Sink::File(f, _) => f.flush(),
        Sink::Pipe(w, _) => w.flush(),
        Sink::Stdout | Sink::Stderr => Ok(()),
    }
}

/// Flush and close a sink.
///
/// # Errors
/// The flush failure. A pipe's command is still waited for -- the child must
/// not outlive the close -- whatever the flush did.
fn finish(sink: Sink) -> io::Result<()> {
    match sink {
        Sink::File(mut f, _) => f.flush(),
        Sink::Pipe(mut w, mut child) => {
            let flushed = w.flush();
            // The child sees end-of-file only once our write end is gone, so
            // the writer has to be dropped before the wait or this deadlocks.
            drop(w);
            // Its status is not `close`'s answer under `--posix` (see
            // `Outputs::close`), and a wait that fails leaves nothing to do.
            let _ = child.wait();
            flushed
        }
        Sink::Stdout | Sink::Stderr => Ok(()),
    }
}

fn open_sink(name: &[u8], mode: crate::ast::RedirMode) -> io::Result<Sink> {
    if mode == crate::ast::RedirMode::Pipe {
        let mut child = shell(name).stdin(Stdio::piped()).spawn()?;
        let Some(stdin) = child.stdin.take() else {
            return Err(io::Error::other("the child has no standard input"));
        };
        return Ok(Sink::Pipe(BufWriter::new(stdin), child));
    }
    // The two names every awk understands without them being real files, which
    // matters most on a system where they are not.
    if name == b"/dev/stdout" || name == b"-" {
        return Ok(Sink::Stdout);
    }
    if name == b"/dev/stderr" {
        return Ok(Sink::Stderr);
    }
    let path = os_path(name);
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .append(mode == crate::ast::RedirMode::Append)
        .truncate(mode == crate::ast::RedirMode::Truncate)
        .open(path)?;
    let tty = is_terminal(&f);
    Ok(Sink::File(BufWriter::new(f), tty))
}

/// Whether an open file is a terminal, which decides whether output to it is
/// flushed after every print.
fn is_terminal(f: &File) -> bool {
    use std::io::IsTerminal;
    f.is_terminal()
}

/// The input redirections a program has open.
pub struct Inputs {
    files: HashMap<Str, Records>,
    cmds: HashMap<Str, (Records, Child)>,
}

impl Inputs {
    #[must_use]
    pub fn new() -> Inputs {
        Inputs {
            files: HashMap::new(),
            cmds: HashMap::new(),
        }
    }

    /// The reader for `getline < name`, opening it on first use.
    ///
    /// # Errors
    /// Propagates the failure to open, which `getline` reports as -1 rather
    /// than as a fatal error — that is what lets `while ((getline < f) > 0)`
    /// be written against a file that may not exist.
    pub fn file(&mut self, name: &[u8]) -> io::Result<&mut Records> {
        if !self.files.contains_key(name) {
            let src: Box<dyn Read> = if name == b"-" || name == b"/dev/stdin" {
                Box::new(io::stdin())
            } else {
                Box::new(File::open(os_path(name))?)
            };
            self.files.insert(name.to_vec(), Records::new(src));
        }
        self.files
            .get_mut(name)
            .ok_or_else(|| io::Error::other("the input redirection vanished"))
    }

    /// The reader for `"cmd" | getline`, spawning it on first use.
    ///
    /// # Errors
    /// Propagates the failure to spawn.
    pub fn command(&mut self, name: &[u8]) -> io::Result<&mut Records> {
        if !self.cmds.contains_key(name) {
            let mut child = shell(name).stdout(Stdio::piped()).spawn()?;
            let Some(stdout) = child.stdout.take() else {
                return Err(io::Error::other("the child has no standard output"));
            };
            self.cmds
                .insert(name.to_vec(), (Records::new(Box::new(stdout)), child));
        }
        self.cmds
            .get_mut(name)
            .map(|(r, _)| r)
            .ok_or_else(|| io::Error::other("the input redirection vanished"))
    }

    /// Close one input redirection. Returns false if nothing by that name was
    /// open, so the caller can try the outputs. A command is waited for; its
    /// status is not `close`'s answer under `--posix` (see `Outputs::close`).
    pub fn close(&mut self, name: &[u8]) -> bool {
        if self.files.remove(name).is_some() {
            return true;
        }
        let Some((reader, mut child)) = self.cmds.remove(name) else {
            return false;
        };
        // Dropping the reader closes the pipe, so a child still writing gets a
        // broken pipe and stops rather than blocking us in `wait`.
        drop(reader);
        // A wait that fails leaves nothing to do, and no status to report.
        let _ = child.wait();
        true
    }

    /// Whether `name` is open for input, as a file (`Some(false)`) or as a
    /// command (`Some(true)`): what `fflush` says it cannot flush.
    #[must_use]
    pub fn open_as(&self, name: &[u8]) -> Option<bool> {
        if self.files.contains_key(name) {
            return Some(false);
        }
        self.cmds.contains_key(name).then_some(true)
    }
}

impl Default for Inputs {
    fn default() -> Inputs {
        Inputs::new()
    }
}

/// A command line, run by the shell — awk's `system`, `print |` and
/// `| getline` all pass their argument to `sh -c` rather than splitting it
/// themselves, because the argument is a *shell* command: it may contain
/// pipes, redirections and quoting that only the shell knows how to read.
///
/// The decision of *which* shell, and what to do on a host that has no
/// `/bin/sh`, is [`coreutils::shell`]'s rather than awk's: `split --filter`
/// has to answer it identically, and two answers that drift apart would mean
/// one utility's filter scripts silently running under different quoting
/// rules from the other's.
pub fn shell(cmd: &[u8]) -> Command {
    coreutils::shell::shell_bytes(cmd)
}

/// A redirection target as a path.
///
/// Paths are bytes on SlateOS and may hold anything but `/` and NUL, so on a
/// platform that agrees they are passed through unchanged. On the development
/// host, where a path is UTF-16, a name that is not UTF-8 cannot be expressed
/// and the lossy conversion is the only thing left — but it only affects the
/// host, and only for a filename that could not have been opened there anyway.
#[cfg(unix)]
pub fn os_path(name: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::path::PathBuf::from(std::ffi::OsStr::from_bytes(name))
}

#[cfg(not(unix))]
pub fn os_path(name: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(String::from_utf8_lossy(name).into_owned())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn recs(data: &[u8], rs: &Rs) -> Vec<Str> {
        let mut r = Records::new(Box::new(std::io::Cursor::new(data.to_vec())));
        let mut out = Vec::new();
        while let Some(rec) = r.next(rs).unwrap() {
            out.push(rec);
        }
        out
    }

    #[test]
    fn a_record_is_the_text_before_the_separator() {
        assert_eq!(
            recs(b"a\nb\nc\n", &Rs::Char(b'\n')),
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
        // A file with no final separator still ends in a record.
        assert_eq!(
            recs(b"a\nb", &Rs::Char(b'\n')),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        assert_eq!(recs(b"", &Rs::Char(b'\n')), Vec::<Str>::new());
        // An empty record between two separators is a record.
        assert_eq!(
            recs(b"a\n\nb\n", &Rs::Char(b'\n')),
            vec![b"a".to_vec(), Str::new(), b"b".to_vec()]
        );
    }

    #[test]
    fn paragraph_mode_splits_on_blank_lines_and_eats_the_run() {
        assert_eq!(
            recs(b"\n\na\nb\n\n\n\nc\n", &Rs::Paragraph),
            vec![b"a\nb".to_vec(), b"c".to_vec()]
        );
    }

    #[test]
    fn a_regex_separator_splits_where_it_matches() {
        let re = Rc::new(Regex::new(b"[;:]+").unwrap());
        assert_eq!(
            recs(b"a;b::c", &Rs::Regex(re)),
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
    }

    #[test]
    fn a_record_longer_than_the_read_chunk_is_still_one_record() {
        let big = vec![b'x'; CHUNK * 3 + 7];
        let mut data = big.clone();
        data.push(b'\n');
        data.extend_from_slice(b"tail\n");
        assert_eq!(recs(&data, &Rs::Char(b'\n')), vec![big, b"tail".to_vec()]);
    }

    #[test]
    fn a_record_that_is_not_text_survives() {
        assert_eq!(
            recs(&[0xff, b'\n', 0xfe], &Rs::Char(b'\n')),
            vec![vec![0xff], vec![0xfe]]
        );
    }
}
