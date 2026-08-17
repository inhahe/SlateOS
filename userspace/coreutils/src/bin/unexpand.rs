//! `unexpand` — convert blanks in each file to tabs.
//!
//! ```text
//! unexpand [-a] [--first-only] [-t LIST] [-LIST] [--] [FILE...]
//! ```
//!
//! | option | effect |
//! |---|---|
//! | `-a`, `--all` | convert every run of blanks, not only the leading one |
//! | `--first-only` | convert only leading blanks — cancels `-a`, even an earlier one |
//! | `-t LIST`, `--tabs=LIST` | tab stops, comma- or blank-separated; default 8. Implies `-a` |
//! | `-LIST` | the obsolete form of the same list — but see below, it is *not* `expand`'s |
//!
//! The grammar of `LIST` lives in [`coreutils::tabstops`], shared with
//! [`expand`](../expand/index.html); `unexpand` additionally uses that module's
//! `max_column_width`, which exists only for this program.
//!
//! ## The obsolete digit form is a different mechanism from `expand`'s
//!
//! The two manuals describe `-LIST` the same way and the two implementations do
//! not share a line of it:
//!
//! | | `expand` | `unexpand` |
//! |---|---|---|
//! | getopt string | `"0::"` … `"9::"` — optional argument | `",0123456789at:"` — **no** argument |
//! | how a list is read | the whole cluster at once, via `parse_tab_stops (optarg - 1)` | one digit at a time into an accumulator, `,` flushing it |
//! | `-1 -2` | stops at 1 and 2 | **one** stop at 12 |
//! | `-1,3` | one call, two stops | `1` accumulated, `,` flushes it, `3` accumulated |
//!
//! The accumulator spans the whole command line, not one argument, because
//! `getopt` hands the digits over one at a time with no notion of which
//! argument they came from. That is why `unexpand -1 -2` is twelve. It is not a
//! typo-tolerance feature; it is what an eleven-option no-argument string does.
//!
//! ## What this used to be
//!
//! `unexpand [-t N] [FILE...]`, with a hand-rolled converter that shared the
//! same four defects as the old `expand`:
//!
//! * **Input was decoded as UTF-8 a line at a time via `BufRead::lines()`**, so
//!   a non-UTF-8 file stopped at its first bad byte, a CRLF file silently lost
//!   its CRs, and a final line with no newline gained one from `writeln!`.
//! * **`-t`'s argument went through `parse().unwrap_or(8)`**, so `-t oops` was
//!   `-t 8` and reported success.
//! * **Every exit was `0`**, including a missing file, and every write was
//!   `let _ = writeln!(…)`, so a full disk truncated the output silently.
//! * **Only `-t N` and `-a` were recognised.** `-t8`, `--tabs=8`, `--all`,
//!   `--first-only`, `-8` and `--` were all treated as filenames.
//!
//! Beyond the options, the conversion itself was a different algorithm rather
//! than an incomplete one. It counted columns with `col % tab_width`, which is
//! only right for a single uniform tab size; it never emitted a tab for a run
//! that *reached* a stop from a non-blank when the run began mid-column; it
//! treated `\b` as an ordinary character rather than moving the column back;
//! and under `-a` it required `space_count > 1` at the moment of crossing a
//! stop, which is a different rule from GNU's (GNU keeps a single blank that
//! sits just before a stop and only refuses to *turn it into* a tab).
//!
//! ## Blanks are buffered, because it is not yet known if they become a tab
//!
//! The converter cannot decide a run of blanks until it sees what follows, so
//! blanks accumulate in a pending buffer that is emitted when the next
//! non-blank arrives — either verbatim, or with its first byte overwritten by a
//! tab. Two consequences worth stating:
//!
//! * **A tab is never used to replace exactly one blank**, even when that blank
//!   lands on a tab stop, because it would not be shorter. The flag for this is
//!   upstream's `one_blank_before_tab_stop`, which is why the pending run is
//!   truncated to *one* rather than to zero when a stop is reached.
//! * **A tab in the input is itself pending-able.** `unexpand -a` on `"x \t y"`
//!   folds the space and the tab together, because the tab's arrival rewrites
//!   `pending_blank[0]`.
//!
//! ## Columns are bytes, and a backspace moves back one
//!
//! As in `expand`: one column per byte — measured identical under `LC_ALL=C`
//! and `LC_ALL=C.UTF-8` — and nothing here decodes, so a file that is not valid
//! UTF-8 passes through byte for byte. `\b` decrements the column and the
//! position in the explicit stop list, both clamped at zero.
//!
//! ## Checked against GNU
//!
//! `scripts/unexpand-diff.sh` runs this and glibc's `unexpand` over the same
//! command lines and compares stdout, stderr and the exit status byte for byte.
//! The corners it pinned down:
//!
//! * **`-t` implies `-a`**, and `--first-only` undoes it — but only at the end
//!   of parsing, so the order of `-a`, `-t` and `--first-only` on the command
//!   line does not matter.
//! * **Past the last explicit stop, conversion stops for the rest of the
//!   line.** `expand` treats an exhausted list as "one space per tab";
//!   `unexpand` treats it as "leave everything alone from here", which is
//!   upstream's `last_tab` out-parameter and a genuinely different reading of
//!   the same `get_next_tab_column`.
//! * **The operands are one byte stream**, so a file that does not end in a
//!   newline continues the same line — and the same column count — into the
//!   next file.
//! * **An unterminated final line stays unterminated**, with its pending blanks
//!   still flushed.
//! * **A file that cannot be opened does not stop the run**; the rest are
//!   converted and the status is 1 at the end.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::quotef_os;
use coreutils::tabstops::TabStops;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

const UNEXPAND: Program = Program::new("unexpand", 1);

const USAGE: &str = "usage: unexpand [-a] [--first-only] [-t LIST] [-LIST] [--] [FILE...]";

/// `\b`, which Rust has no escape for.
const BACKSPACE: u8 = 0x08;

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it.
/// Measured with `unexpand --=x`, whose empty prefix matches every entry.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("tabs", Long::Tabs),
    ("all", Long::All),
    ("first-only", Long::FirstOnly),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Tabs,
    All,
    FirstOnly,
    Help,
    Version,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Convert these operands (`-` meaning standard input).
    Run(Settings, Vec<OsString>),
    Help,
    Version,
}

/// The two things the options actually decide.
///
/// Note the default is the opposite of `expand`'s: `expand` converts the whole
/// line unless `-i`, `unexpand` converts only the leading run unless `-a`.
#[derive(Debug, Default, PartialEq, Eq)]
struct Settings {
    tabs: TabStops,
    /// True under `-a` or `-t`, and forced false by `--first-only`.
    entire_line: bool,
}

/// A command line that cannot be run.
///
/// As in `expand`: a getopt diagnostic ends in `Try 'unexpand --help' for more
/// information.` and carries the program's usage status, while a tab-stop
/// diagnostic is `error (0, 0, …)` repeated up to twice and then a bare
/// `exit (EXIT_FAILURE)`, so it has no referral and is always 1.
#[derive(Debug)]
enum Refusal {
    Getopt(getopt::Error),
    Tabs(Vec<String>),
}

impl Refusal {
    fn report(&self) -> ExitCode {
        let status = match self {
            Self::Getopt(e) => {
                eprintln!("unexpand: {}", e.message());
                e.status
            }
            Self::Tabs(messages) => {
                for message in messages {
                    eprintln!("unexpand: {message}");
                }
                1
            }
        };
        ExitCode::from(u8::try_from(status).unwrap_or(1))
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(refusal) => return refusal.report(),
    };

    let (settings, mut files) = match request {
        Request::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("unexpand (SlateOS coreutils)");
            return ExitCode::SUCCESS;
        }
        Request::Run(settings, files) => (settings, files),
    };

    if files.is_empty() {
        files.push(OsString::from("-"));
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut input = Input::new(files);

    // Upstream opens the first operand *before* allocating the pending-blank
    // buffer (`if (!fp) return;` precedes the `xmalloc`), and that order is
    // observable: `unexpand -t 18446744073709551615 nosuch` reports the missing
    // file, while the same option with a readable file — even an empty one —
    // reports `memory exhausted`. So the buffer is sized here rather than at
    // the top of `main`.
    if input.advance() {
        let mut unexpander = match Unexpander::new(&settings.tabs, settings.entire_line) {
            Ok(unexpander) => unexpander,
            Err(trouble) => return trouble.report(),
        };
        while let Some(byte) = input.next_byte() {
            if let Err(trouble) = unexpander.push(byte, &mut out) {
                return trouble.report();
            }
        }
        if let Err(trouble) = unexpander.finish(&mut out) {
            return trouble.report();
        }
    }

    // Buffered output has to reach the OS before success can be claimed; a
    // flush that fails here is a truncated conversion reported as a complete
    // one. Upstream gets this from `atexit (close_stdout)`.
    if let Err(e) = out.flush() {
        return Trouble::Write(e).report();
    }

    if input.failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// A failure that ends the run rather than the file.
#[derive(Debug)]
enum Trouble {
    Write(io::Error),
    /// The column counter wrapped, or a tab stop landed behind the column.
    /// Unreachable with 64-bit columns on any input that fits in memory, and
    /// ported anyway because the alternative is a silent wrap that mangles
    /// alignment rather than reporting it.
    TooLong,
    /// The pending-blank buffer could not be allocated, which is gnulib's
    /// `xalloc_die`. Reachable from an ordinary command line rather than only
    /// under real memory pressure: `-t 18446744073709551615` is a valid tab
    /// size, and it asks for a buffer of 2**64-1 bytes.
    MemoryExhausted,
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Write(e) => eprintln!("unexpand: write error: {}", strerror(e)),
            Self::TooLong => eprintln!("unexpand: input line is too long"),
            Self::MemoryExhausted => eprintln!("unexpand: memory exhausted"),
        }
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------- conversion

/// The running column state, which spans the whole command line rather than
/// one file, and resets at each newline.
struct Unexpander<'a> {
    tabs: &'a TabStops,
    entire_line: bool,
    /// Whether blanks on this line are still being converted. Cleared by the
    /// first non-blank without `-a`, and by running past the last tab stop.
    convert: bool,
    column: u64,
    /// Column of the next tab stop. Recomputed on every blank, and rewound by a
    /// backspace so the recomputation starts from where the cursor now is.
    next_tab_column: u64,
    /// Position in the explicit stop list, rewound by a backspace in step with
    /// `column`.
    tab_index: usize,
    /// Set when the first pending blank sits exactly on a tab stop, which is
    /// the one blank that must survive rather than be folded into a tab.
    one_blank_before_tab_stop: bool,
    /// Whether the previous input character was a blank. Initially true,
    /// because a line's leading blanks are treated as if a blank preceded them.
    prev_blank: bool,
    /// Blanks whose fate is not yet decided.
    ///
    /// Upstream sizes this once, at `max_column_width` bytes, from the comment
    /// "a non-blank character, then one blank, then a tab stop, then
    /// MAX_COLUMN_WIDTH - 1 blanks, then a non-blank". A `Vec` does not need
    /// the bound to be right, which is the reason to prefer it: an off-by-one
    /// in that reasoning would be a buffer overrun there and a growth here.
    ///
    /// The reservation is still made, and still allowed to fail, because
    /// failing is observable — see [`Unexpander::new`].
    pending: Vec<u8>,
}

impl<'a> Unexpander<'a> {
    /// # Errors
    ///
    /// [`Trouble::MemoryExhausted`], when the tab stops are so far apart that
    /// the pending-blank buffer cannot be allocated. This is not defensive
    /// programming against a full machine: `unexpand -t 18446744073709551615`
    /// is a valid command line that GNU answers with `memory exhausted` and
    /// status 1, because `xmalloc (max_column_width)` is asked for 2**64-1
    /// bytes. Growing the `Vec` on demand instead would silently *accept* that
    /// command line, which is a difference a user could see.
    fn new(tabs: &'a TabStops, entire_line: bool) -> Result<Self, Trouble> {
        let mut pending = Vec::new();
        let width = usize::try_from(tabs.max_column_width()).map_err(|_| {
            // A 32-bit host cannot represent the width at all, which is the
            // same answer for the same reason.
            Trouble::MemoryExhausted
        })?;
        pending
            .try_reserve_exact(width)
            .map_err(|_| Trouble::MemoryExhausted)?;
        Ok(Self {
            tabs,
            entire_line,
            convert: true,
            column: 0,
            next_tab_column: 0,
            tab_index: 0,
            one_blank_before_tab_stop: false,
            prev_blank: true,
            pending,
        })
    }

    /// Feed one input byte and write whatever that settles.
    ///
    /// This is the whole of upstream's `unexpand()` inner loop, one character
    /// per call. The state it keeps between calls is what makes two files
    /// behave as one stream.
    fn push<W: Write>(&mut self, byte: u8, out: &mut W) -> Result<(), Trouble> {
        let mut c = byte;
        if self.convert {
            // `isblank` in the C locale is these two and nothing else.
            let blank = c == b' ' || c == b'\t';
            if blank {
                match self.tabs.next_stop(self.column, &mut self.tab_index) {
                    // Upstream's `last_tab` out-parameter: past the final stop
                    // with no `/` or `+` to continue it, `unexpand` gives up on
                    // the rest of the line entirely. (`expand` instead treats
                    // each further tab as one space — the same predicate read
                    // two different ways.)
                    None => self.convert = false,
                    Some(stop) => self.next_tab_column = stop,
                }
            }

            if self.convert {
                if blank {
                    if self.next_tab_column < self.column {
                        return Err(Trouble::TooLong);
                    }
                    if c == b'\t' {
                        self.column = self.next_tab_column;
                        // A tab arriving over pending blanks turns them into
                        // one tab, whatever they were.
                        if let Some(first) = self.pending.first_mut() {
                            *first = b'\t';
                        }
                    } else {
                        self.column = self.column.wrapping_add(1);
                        if !(self.prev_blank && self.column == self.next_tab_column) {
                            // Undecided: this blank might yet become part of a
                            // tab, so buffer it and wait for the next byte.
                            if self.column == self.next_tab_column {
                                self.one_blank_before_tab_stop = true;
                            }
                            self.pending.push(c);
                            self.prev_blank = true;
                            return Ok(());
                        }
                        // A run of blanks that reached a stop: replace it with a
                        // tab. Upstream writes `pending_blank[0]` unguarded
                        // here, into a buffer that may have length 0 — harmless
                        // there because the truncation below immediately
                        // discards it, and spelled out here so it stays so.
                        c = b'\t';
                        if self.pending.is_empty() {
                            self.pending.push(b'\t');
                        } else if let Some(first) = self.pending.first_mut() {
                            *first = b'\t';
                        }
                    }
                    // Discard the pending blanks — unless it was the single
                    // blank sitting just before the previous stop, which stays
                    // because one blank is not worth a tab.
                    self.pending
                        .truncate(usize::from(self.one_blank_before_tab_stop));
                }
            }

            if !blank {
                if c == BACKSPACE {
                    // Back one column, and force the next tab stop to be looked
                    // up again from the rewound position.
                    self.column = self.column.saturating_sub(1);
                    self.next_tab_column = self.column;
                    self.tab_index = self.tab_index.saturating_sub(1);
                } else {
                    self.column = self.column.wrapping_add(1);
                    if self.column == 0 {
                        return Err(Trouble::TooLong);
                    }
                }
            }

            self.flush_pending(out)?;
            self.prev_blank = blank;
            self.convert &= self.entire_line || blank;
        }

        out.write_all(&[c]).map_err(Trouble::Write)?;
        if c == b'\n' {
            self.reset();
        }
        Ok(())
    }

    /// End of input: upstream reaches its `if (c < 0) return` through the
    /// non-blank branch, so a pending run is still flushed — an unterminated
    /// final line keeps its converted blanks.
    fn finish<W: Write>(&mut self, out: &mut W) -> Result<(), Trouble> {
        if self.convert {
            self.column = self.column.wrapping_add(1);
            if self.column == 0 {
                return Err(Trouble::TooLong);
            }
            self.flush_pending(out)?;
        }
        Ok(())
    }

    fn flush_pending<W: Write>(&mut self, out: &mut W) -> Result<(), Trouble> {
        if self.pending.is_empty() {
            return Ok(());
        }
        // More than one blank *and* the first one was on a stop: the run really
        // does span a stop, so the leading blank becomes a tab after all.
        if self.pending.len() > 1 && self.one_blank_before_tab_stop {
            if let Some(first) = self.pending.first_mut() {
                *first = b'\t';
            }
        }
        out.write_all(&self.pending).map_err(Trouble::Write)?;
        self.pending.clear();
        self.one_blank_before_tab_stop = false;
        Ok(())
    }

    fn reset(&mut self) {
        self.convert = true;
        self.column = 0;
        self.next_tab_column = 0;
        self.tab_index = 0;
        self.one_blank_before_tab_stop = false;
        self.prev_blank = true;
        self.pending.clear();
    }
}

// --------------------------------------------------------------------- input

/// The operands as one byte stream.
///
/// Upstream's `while ((c = getc (fp)) < 0 && (fp = next_file (fp)))` makes the
/// file boundary invisible to the converter: a line that begins in one file and
/// ends in the next is one line, and its columns keep counting across the join.
struct Input {
    names: std::vec::IntoIter<OsString>,
    open: Option<Open>,
    /// Set by an operand that could not be opened or read; the run continues
    /// and the status is 1 at the end.
    failed: bool,
}

struct Open {
    name: OsString,
    reader: Box<dyn BufRead>,
}

/// What one attempt at a byte produced. Named rather than inlined so the
/// borrow of `open` ends before the stream is closed or replaced.
enum Step {
    Byte(u8),
    Eof,
    Retry,
    Failed(io::Error),
}

impl Input {
    fn new(names: Vec<OsString>) -> Self {
        Self {
            names: names.into_iter(),
            open: None,
            failed: false,
        }
    }

    /// Open operands until one succeeds; report and skip the ones that do not.
    fn advance(&mut self) -> bool {
        for name in self.names.by_ref() {
            let opened: io::Result<Box<dyn BufRead>> = if name == "-" {
                Ok(Box::new(BufReader::new(io::stdin())))
            } else {
                File::open(&name).map(|f| Box::new(BufReader::new(f)) as Box<dyn BufRead>)
            };
            match opened {
                Ok(reader) => {
                    self.open = Some(Open { name, reader });
                    return true;
                }
                Err(e) => {
                    eprintln!("unexpand: {}: {}", quotef_os(&name), strerror(&e));
                    self.failed = true;
                }
            }
        }
        false
    }

    fn next_byte(&mut self) -> Option<u8> {
        loop {
            if self.open.is_none() && !self.advance() {
                return None;
            }
            let step = {
                let open = self.open.as_mut()?;
                match open.reader.fill_buf() {
                    Ok(chunk) => match chunk.first().copied() {
                        Some(byte) => {
                            open.reader.consume(1);
                            Step::Byte(byte)
                        }
                        None => Step::Eof,
                    },
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => Step::Retry,
                    Err(e) => Step::Failed(e),
                }
            };
            match step {
                Step::Byte(byte) => return Some(byte),
                Step::Retry => {}
                Step::Eof => self.open = None,
                Step::Failed(e) => {
                    // Upstream notices this through `ferror` in `next_file`, so
                    // the file is named and the run moves on to the next one.
                    if let Some(open) = self.open.take() {
                        eprintln!("unexpand: {}: {}", quotef_os(&open.name), strerror(&e));
                    }
                    self.failed = true;
                }
            }
        }
    }
}

// ------------------------------------------------------------------- parsing

/// The digits of the obsolete form, accumulated across the whole command line.
///
/// Upstream keeps this as `have_tabval` plus `tabval`, two locals of `main`
/// that no option other than `,` and a digit ever touches — which is precisely
/// why `-1 -t4 -2` puts 4 in the list before 12.
#[derive(Default)]
struct Obsolete {
    value: Option<u64>,
}

impl Obsolete {
    /// One more digit. Fails on overflow, which upstream reports and exits on
    /// immediately, inside the option loop.
    fn digit(&mut self, c: u8) -> Result<(), Refusal> {
        let digit = u64::from(c.saturating_sub(b'0'));
        let next = self
            .value
            .unwrap_or(0)
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(digit));
        match next {
            Some(value) => {
                self.value = Some(value);
                Ok(())
            }
            None => Err(Refusal::Tabs(vec![
                "tab stop value is too large".to_string(),
            ])),
        }
    }

    /// A `,`, or the end of the command line: commit whatever has accumulated.
    fn flush(&mut self, tabs: &mut TabStops) {
        if let Some(value) = self.value.take() {
            tabs.add(value);
        }
    }
}

fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut settings = Settings {
        tabs: TabStops::new(),
        entire_line: false,
    };
    // `--first-only` is applied after the loop rather than where it is found,
    // so it beats an `-a` on either side of it.
    let mut first_only = false;
    let mut obsolete = Obsolete::default();
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut at = 0usize;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        if only_operands {
            files.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand.
            files.push(arg.clone());
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) =
                long_option(body, &bytes, args, &mut at, &mut settings, &mut first_only)?
            {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut at, &mut settings, &mut obsolete)?;
        }
    }

    if first_only {
        settings.entire_line = false;
    }
    obsolete.flush(&mut settings.tabs);
    settings
        .tabs
        .finalize()
        .map_err(|message| Refusal::Tabs(vec![message]))?;

    Ok(Request::Run(settings, files))
}

/// One `--name`, `--name=value` or `--name value` argument.
///
/// `next` is the caller's position in `args`, advanced when `--tabs` has to
/// reach forward for a separated argument.
fn long_option(
    body: &[u8],
    whole: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
    first_only: &mut bool,
) -> Result<Option<Request>, Refusal> {
    // Split before resolving, so the *name* is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 matches none of
    // them and takes the unrecognised path, reported as the bytes typed.
    let typed = std::str::from_utf8(typed)
        .map_err(|_| Refusal::Getopt(UNEXPAND.unrecognized_option(whole)))?;
    let (name, which) = UNEXPAND
        .resolve_long(typed, whole, LONG_OPTIONS)
        .map_err(Refusal::Getopt)?;

    match which {
        Long::Tabs => {
            // A long option's *required* argument may be written either way:
            // `--tabs=8` or `--tabs 8`. (An *optional* one may not — it only
            // ever comes from the `=` form — but `unexpand` has none.) Only the
            // last argument on the line can leave it genuinely missing.
            let value = match inline {
                Some(value) => value.to_vec(),
                None => {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(UNEXPAND.long_missing_argument(name)));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                }
            };
            settings.entire_line = true;
            settings.tabs.parse(&value).map_err(Refusal::Tabs)?;
        }
        Long::All | Long::FirstOnly | Long::Help | Long::Version => {
            if inline.is_some() {
                return Err(Refusal::Getopt(UNEXPAND.long_unwanted_argument(name)));
            }
            match which {
                Long::All => settings.entire_line = true,
                Long::FirstOnly => *first_only = true,
                Long::Help => return Ok(Some(Request::Help)),
                Long::Version => return Ok(Some(Request::Version)),
                Long::Tabs => {}
            }
        }
    }
    Ok(None)
}

/// One `-abc` cluster.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
///
/// `next` is the caller's position in `args`, advanced when `-t` has to reach
/// forward for a separated argument.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
    obsolete: &mut Obsolete,
) -> Result<(), Refusal> {
    let cluster = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = cluster.get(at) {
        match c {
            b'a' => settings.entire_line = true,
            b't' => {
                // A *required* argument: the rest of the cluster if there is
                // one, otherwise the whole of the next argument.
                let rest = cluster.get(at.saturating_add(1)..).unwrap_or_default();
                let value = if rest.is_empty() {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(UNEXPAND.short_missing_argument(b't')));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                } else {
                    rest.to_vec()
                };
                settings.entire_line = true;
                settings.tabs.parse(&value).map_err(Refusal::Tabs)?;
                return Ok(());
            }
            // The obsolete form: eleven no-argument options, not one option
            // with a list attached. See the module documentation.
            b'0'..=b'9' => obsolete.digit(c)?,
            b',' => obsolete.flush(&mut settings.tabs),
            _ => return Err(Refusal::Getopt(UNEXPAND.invalid_option(c))),
        }
        at = at.saturating_add(1);
    }
    Ok(())
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    // On a host build the arguments arrive as UTF-16 and are converted lossily;
    // the target build takes the branch above and never decodes.
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Run the converter over `input` with the given command line options.
    fn run(options: &[&str], input: &str) -> String {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        let Ok(Request::Run(settings, _)) = parse_args(&args) else {
            panic!("expected {options:?} to parse");
        };
        let mut out: Vec<u8> = Vec::new();
        let mut u = Unexpander::new(&settings.tabs, settings.entire_line).expect("allocate");
        for &byte in input.as_bytes() {
            u.push(byte, &mut out).expect("convert");
        }
        u.finish(&mut out).expect("finish");
        String::from_utf8(out).expect("utf8")
    }

    fn refusal(options: &[&str]) -> String {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        match parse_args(&args) {
            Err(Refusal::Getopt(e)) => e.message(),
            Err(Refusal::Tabs(messages)) => messages.join("\n"),
            Ok(_) => panic!("expected {options:?} to be refused"),
        }
    }

    // ------------------------------------------------------------ conversion

    #[test]
    fn leading_blanks_only_by_default() {
        assert_eq!(run(&[], "        text\n"), "\ttext\n");
        // A run in the middle is left alone without `-a`.
        assert_eq!(run(&[], "a        b\n"), "a        b\n");
    }

    /// Measured, because the intuitive answer is wrong: `a` + 8 blanks + `b`
    /// becomes a tab and *one* blank, not two tabs. The seventh blank reaches
    /// column 8 and is replaced; the eighth starts a new run towards column 16
    /// that never gets there, so it is flushed verbatim.
    #[test]
    fn dash_a_converts_every_run() {
        assert_eq!(run(&["-a"], "a        b\n"), "a\t b\n");
        // Exactly reaching the stop and stopping there is the clean case.
        assert_eq!(run(&["-a"], "a       b\n"), "a\tb\n");
    }

    /// One blank is never worth a tab, even landing exactly on a stop — this is
    /// `one_blank_before_tab_stop`, and it is the rule the old implementation
    /// approximated with `space_count > 1`.
    #[test]
    fn a_single_blank_on_a_stop_stays_a_blank() {
        assert_eq!(run(&["-a"], "abcdefg h\n"), "abcdefg h\n");
        // Two blanks reaching the same stop do become a tab.
        assert_eq!(run(&["-a"], "abcdef  h\n"), "abcdef\th\n");
    }

    #[test]
    fn partial_runs_are_left_as_blanks() {
        assert_eq!(run(&[], "     text\n"), "     text\n");
        assert_eq!(run(&[], "          text\n"), "\t  text\n");
    }

    #[test]
    fn a_tab_folds_the_blanks_before_it() {
        assert_eq!(run(&["-a"], "x \ty\n"), "x\ty\n");
    }

    #[test]
    fn trailing_blanks_are_flushed_verbatim() {
        // Nothing follows them, so nothing decides them.
        assert_eq!(run(&["-a"], "text   "), "text   ");
        assert_eq!(run(&["-a"], "text   \n"), "text   \n");
    }

    #[test]
    fn an_unterminated_final_line_gains_nothing() {
        assert_eq!(run(&[], "        text"), "\ttext");
    }

    /// The one place `unexpand` reads the shared predicate differently from
    /// `expand`: past the last stop it stops converting for the whole line.
    #[test]
    fn past_the_last_explicit_stop_the_line_is_left_alone() {
        // Stops at 2 and 4 only, so the run reaching 4 becomes two tabs — one
        // flushed from the pending buffer, one for the blank that arrived —
        // and everything from column 4 on is left exactly as it was.
        assert_eq!(run(&["-a", "-t", "2,4"], "a       b\n"), "a\t\t    b\n");
    }

    /// A backspace is not a blank, so without `-a` it ends the leading region
    /// before any of the blanks after it are looked at — the line comes out
    /// untouched. With `-a` the rewind is visible: the column is already 0, so
    /// it stays 0 and the eight blanks reach the stop at 8.
    #[test]
    fn a_backspace_is_not_blank_and_moves_the_column_back() {
        assert_eq!(run(&[], "\u{8}        x\n"), "\u{8}        x\n");
        assert_eq!(run(&["-a"], "\u{8}        x\n"), "\u{8}\tx\n");
    }

    #[test]
    fn columns_are_bytes_not_characters() {
        // 'é' is two bytes, so six more blanks reach column 8, not seven.
        assert_eq!(run(&["-a"], "é      x\n"), "é\tx\n");
    }

    #[test]
    fn state_resets_at_every_newline() {
        assert_eq!(run(&[], "        a\n        b\n"), "\ta\n\tb\n");
    }

    // --------------------------------------------------------------- parsing

    #[test]
    fn dash_t_implies_dash_a() {
        // `-t 8` alone converts an interior run, which plain `unexpand` does not.
        assert_eq!(run(&["-t", "8"], "a       b\n"), "a\tb\n");
        assert_eq!(run(&[], "a       b\n"), "a       b\n");
    }

    #[test]
    fn first_only_beats_dash_a_from_either_side() {
        assert_eq!(run(&["-a", "--first-only"], "a       b\n"), "a       b\n");
        assert_eq!(run(&["--first-only", "-a"], "a       b\n"), "a       b\n");
        // …and beats the `-a` that `-t` implies.
        assert_eq!(
            run(&["-t", "8", "--first-only"], "a       b\n"),
            "a       b\n"
        );
    }

    /// The heart of the obsolete-form difference from `expand`: digits
    /// accumulate across arguments, so two `-N` options are *one* number.
    #[test]
    fn obsolete_digits_accumulate_across_the_command_line() {
        // `-1 -2` is one stop every twelve columns, so the two blanks that
        // reach column 12 become a tab…
        assert_eq!(
            run(&["-a", "-1", "-2"], "abcdefghij  l\n"),
            "abcdefghij\tl\n"
        );
        assert_eq!(
            run(&["-a", "-12"], "abcdefghij  l\n"),
            run(&["-a", "-1", "-2"], "abcdefghij  l\n")
        );
        // …while `-1,2` is *two* stops, at 1 and 2, which the blanks at columns
        // 11 and 12 are long past — so the line is left alone. The two spell
        // the same three characters and mean entirely different things.
        assert_eq!(run(&["-a", "-1,2"], "abcdefghij  l\n"), "abcdefghij  l\n");
    }

    #[test]
    fn a_single_obsolete_number_is_a_uniform_size() {
        assert_eq!(run(&["-a", "-4"], "a   b\n"), "a\tb\n");
        assert_eq!(run(&["-a", "-t4"], "a   b\n"), "a\tb\n");
    }

    #[test]
    fn the_separated_and_attached_forms_of_dash_t_agree() {
        assert_eq!(run(&["-t", "4"], "    x\n"), run(&["-t4"], "    x\n"));
        assert_eq!(run(&["-t4"], "    x\n"), run(&["--tabs=4"], "    x\n"));
        // The separated long form. A long option with a *required* argument
        // takes the next word when there is no `=`, exactly as `-t` does;
        // believing otherwise left `--tabs 4` reporting a missing argument.
        assert_eq!(run(&["-t4"], "    x\n"), run(&["--tabs", "4"], "    x\n"));
    }

    #[test]
    fn a_separated_long_argument_is_consumed_rather_than_left_an_operand() {
        let args: Vec<OsString> = ["--tabs", "4", "file"].iter().map(OsString::from).collect();
        let Ok(Request::Run(_, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        assert_eq!(files, vec![OsString::from("file")]);
    }

    #[test]
    fn every_occurrence_of_dash_t_appends() {
        assert_eq!(
            run(&["-t1", "-t3"], "   x\n"),
            run(&["-t", "1,3"], "   x\n")
        );
    }

    #[test]
    fn long_options_abbreviate_and_report_ambiguity_in_declaration_order() {
        // `--a` is unambiguous: only `--all` starts with it.
        assert_eq!(run(&["--a"], "a       b\n"), "a\tb\n");
        assert_eq!(
            refusal(&["--="]),
            "option '--=' is ambiguous; possibilities: '--tabs' '--all' '--first-only' \
             '--help' '--version'\nTry 'unexpand --help' for more information."
        );
    }

    #[test]
    fn the_getopt_diagnostics() {
        assert_eq!(
            refusal(&["-Z"]),
            "invalid option -- 'Z'\nTry 'unexpand --help' for more information."
        );
        assert_eq!(
            refusal(&["--nope"]),
            "unrecognized option '--nope'\nTry 'unexpand --help' for more information."
        );
        assert_eq!(
            refusal(&["-t"]),
            "option requires an argument -- 't'\nTry 'unexpand --help' for more information."
        );
        assert_eq!(
            refusal(&["--tabs"]),
            "option '--tabs' requires an argument\nTry 'unexpand --help' for more information."
        );
        assert_eq!(
            refusal(&["--all=x"]),
            "option '--all' doesn't allow an argument\nTry 'unexpand --help' for more information."
        );
    }

    /// A tab-stop diagnostic has no referral and is always status 1 — a
    /// different shape from every message above.
    #[test]
    fn the_tab_stop_diagnostics_carry_no_referral() {
        assert_eq!(
            refusal(&["-t", "x"]),
            "tab size contains invalid character(s): 'x'"
        );
        assert_eq!(refusal(&["-t", "0"]), "tab size cannot be 0");
        assert_eq!(refusal(&["-t", "4,2"]), "tab sizes must be ascending");
        // The obsolete form's own overflow message, which is `unexpand`'s and
        // not the shared module's.
        assert_eq!(
            refusal(&["-99999999999999999999999"]),
            "tab stop value is too large"
        );
    }

    /// The largest valid tab size asks for a 2**64-1 byte buffer, and GNU
    /// answers `memory exhausted` rather than converting anything. The
    /// reservation fails without touching the allocator, so this test is
    /// instant rather than a machine-killer.
    #[test]
    fn an_unallocatable_tab_size_is_memory_exhausted() {
        let args: Vec<OsString> = ["-t", "18446744073709551615"]
            .iter()
            .map(OsString::from)
            .collect();
        let Ok(Request::Run(settings, _)) = parse_args(&args) else {
            panic!("expected the command line to parse");
        };
        assert!(matches!(
            Unexpander::new(&settings.tabs, settings.entire_line),
            Err(Trouble::MemoryExhausted)
        ));
        // A size that fits is not affected.
        let args: Vec<OsString> = ["-t", "8"].iter().map(OsString::from).collect();
        let Ok(Request::Run(settings, _)) = parse_args(&args) else {
            panic!("expected the command line to parse");
        };
        assert!(Unexpander::new(&settings.tabs, settings.entire_line).is_ok());
    }

    #[test]
    fn operands_and_the_double_dash() {
        let args: Vec<OsString> = ["--", "-a", "x"].iter().map(OsString::from).collect();
        let Ok(Request::Run(settings, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        // `-a` after `--` is a filename, so it did not set the flag.
        assert!(!settings.entire_line);
        assert_eq!(files, vec![OsString::from("-a"), OsString::from("x")]);
    }

    #[test]
    fn a_lone_dash_is_an_operand() {
        let args: Vec<OsString> = ["-"].iter().map(OsString::from).collect();
        let Ok(Request::Run(_, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        assert_eq!(files, vec![OsString::from("-")]);
    }

    #[test]
    fn help_and_version_win_where_they_are_found() {
        let args: Vec<OsString> = ["--help"].iter().map(OsString::from).collect();
        assert_eq!(parse_args(&args).unwrap(), Request::Help);
        let args: Vec<OsString> = ["--version"].iter().map(OsString::from).collect();
        assert_eq!(parse_args(&args).unwrap(), Request::Version);
    }
}
