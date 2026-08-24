//! tsort — order a set of items so that every recorded "before" holds.
//!
//! # What this used to be
//!
//! The shipped `tsort` had no option parser at all. `--help` and `--version`
//! were read as file names, so `tsort --help` tried to open a file called
//! `--help`; `--` was likewise a file name; and a second operand was silently
//! ignored rather than refused. Past the command line it disagreed with GNU in
//! four ways that changed *output*, not just diagnostics:
//!
//! - Input was decoded as UTF-8 through `BufRead::lines`, so a name with a
//!   stray `\xff` in it was replaced or rejected, and a `\r\n` file lost the
//!   `\r` — which matters here because `\r` is **not** one of `tsort`'s
//!   delimiters, so `a\r\nb` is one token to GNU and two to a line reader.
//! - Tokens were split with `str::split_whitespace`, whose idea of a space
//!   includes `\r`, `\v`, `\f` and every Unicode separator. GNU splits on
//!   exactly three bytes: space, tab and newline.
//! - The order among items that are simultaneously ready was Kahn's algorithm
//!   over a first-appearance numbering, which is not GNU's order. GNU scans a
//!   *sorted* tree, and takes an item's successors in the reverse of the order
//!   the relations were read. `a c / a b / a d` prints `a d b c` there and
//!   `a b c d` here.
//! - A cycle produced one line — `tsort: input contains a cycle` — and then
//!   printed the cycle's members **on standard output**. GNU names the file,
//!   prints the members on standard *error*, breaks one relation and carries
//!   on, so its standard output still lists every item exactly once.
//!
//! # The tree's shape is not observable, only its order
//!
//! Upstream keeps the items in a balanced binary tree (Knuth's Algorithm A,
//! rotations and all) keyed by `strcmp`, and every pass over the items is
//! `walk_tree`, which is an in-order traversal. An in-order traversal of a
//! search tree visits its keys in sorted order whatever the balancing did, so
//! none of the rotation code is reachable through the output: a vector of item
//! indices sorted by name gives the identical sequence. That is what this file
//! keeps, and it is why there is no tree in it.
//!
//! The sort is bytewise, because `strcmp` compares `unsigned char`s. It does
//! not consult the locale, so unlike `comm` and `join` there is nothing here
//! that changes under `LC_COLLATE`.
//!
//! # Successors are recorded backwards, and that is visible
//!
//! `record_relation` *prepends* to the predecessor's successor list, so the
//! list runs newest-first. The order matters because the ready queue is filled
//! in the order the successors are walked, and the queue is what standard
//! output prints:
//!
//! ```text
//! $ printf 'a c\na b\na d\n' | tsort
//! a
//! d
//! b
//! c
//! ```
//!
//! `d` — the last relation read — reaches the queue first. The list is stored
//! here back to front and walked in reverse, which is the same sequence with
//! an O(1) prepend instead of a shift.
//!
//! A relation is recorded once per pair read, not once per distinct pair, so
//! `a b` twice gives `b` an in-degree of two and `a` two successor entries.
//! Only `a a` is dropped, and it is dropped by comparing the *strings*, which
//! for interned tokens is comparing the items.
//!
//! # A token is three delimiters wide and stops at a NUL
//!
//! `DELIM` is `" \t\n"`. Carriage return, vertical tab and form feed are
//! ordinary characters inside a token. Tokens are read by gnulib's
//! `readtoken`, which is careful to preserve NUL bytes — and then handed to
//! `xstrdup` and `strcmp`, which are not. The visible result is that a token
//! is truncated at its first NUL:
//!
//! ```text
//! $ printf 'a\0b c\n' | tsort
//! a
//! c
//! ```
//!
//! and that a token consisting only of a NUL is an item with an empty name.
//! Reproduced rather than repaired: it is the behaviour a script that already
//! works against GNU would have been written around.
//!
//! # A loop is reported, broken, and then sorted anyway
//!
//! When the ready queue empties with items left over, the remainder contains a
//! cycle. GNU announces it, walks the graph backwards to name one cycle,
//! deletes a single relation to break it, and resumes — so the run reports
//! every cycle it finds, prints all the items, and exits 1:
//!
//! ```text
//! $ printf 'a b\nb c\nc a\nx y\n' | tsort
//! x            (stdout)
//! y
//! a
//! b
//! c
//! tsort: -: input contains a loop:      (stderr)
//! tsort: a
//! tsort: b
//! tsort: c
//! ```
//!
//! The backward walk is transcribed from upstream's `detect_loop`, including
//! the detail that it reuses the queue link (`qlink`) for its chain and clears
//! the chain only when a cycle has actually been closed. The chain is built
//! over several passes when necessary, which is why the caller repeats the walk
//! until the chain is empty again.
//!
//! # Options
//!
//! `tsort` reaches getopt through gnulib's `parse_gnu_standard_options_only`,
//! which builds a table of just `--help` and `--version` and calls
//! `getopt_long` **once**. Three consequences, all measured:
//!
//! - There are no short options at all, so `-h` is `invalid option -- 'h'`.
//! - Options still permute, so `tsort FILE --version` prints the version.
//! - Because the single call either exits or reports nothing, every argument
//!   that survives to the operand check is an operand.
//!
//! # Checked against GNU
//!
//! `scripts/tsort-diff.sh` runs both binaries over the same fixtures and
//! compares stdout byte for byte through `od -An -c`, stderr in full, and the
//! exit status.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::{quote, quotef_os};
use coreutils::stdfd::{self, Stream};
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's
// `tsort >&-` as the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const TSORT: Program = Program::new("tsort", 1);

const USAGE: &str = "usage: tsort [OPTION] [FILE]";

/// The long options, in GNU's declaration order — which is observable, because
/// `getopt_long` lists an ambiguous prefix's candidates in it. Measured with
/// `tsort --=x`, whose empty prefix matches both.
///
/// gnulib's `parse_gnu_standard_options_only` builds this table itself, so it
/// is the same two entries in the same order for every utility that uses it.
const LONG_OPTIONS: &[(&str, Long)] = &[("help", Long::Help), ("version", Long::Version)];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Help,
    Version,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// The operand, or `-` when there was none. Both name standard input, and
    /// both are spelled `-` in the diagnostics, so one field covers them.
    Run(OsString),
    Help,
    Version,
}

/// The three delimiters. Not `u8::is_ascii_whitespace`, which also counts `\r`
/// and `\x0c`.
const DELIM: &[u8] = b" \t\n";

/// A failure that ends the run.
#[derive(Debug)]
enum Trouble {
    /// The operand would not open: upstream's `freopen` failing, reported with
    /// the bare `errno` text and no mention of reading.
    Open(OsString, io::Error),
    /// The operand opened and then would not read. A directory takes this path
    /// on a system whose `open` accepts one, which is why GNU says
    /// `sub: read error: Is a directory` rather than blaming the open.
    Read(OsString, io::Error),
    /// An odd token count: the last token had nothing to precede.
    OddTokens(OsString),
    Write(io::Error),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Open(name, e) => diag!("tsort: {}: {}", quotef_os(name), strerror(e)),
            Self::Read(name, e) => {
                diag!("tsort: {}: read error: {}", quotef_os(name), strerror(e));
            }
            Self::OddTokens(name) => diag!(
                "tsort: {}: input contains an odd number of tokens",
                quotef_os(name)
            ),
            Self::Write(e) => stdfd::write_error("tsort", e),
        }
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    stdfd::close_stderr(run_main(), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            diag!("tsort: {}", e.message());
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `--help` and `--version` are writes like any other, so they fail like
    // any other: measured, `tsort --help >&-` is
    // `tsort: write error: Bad file descriptor` and exits 1.
    let mut out = Stream::stdout();
    let file = match request {
        Request::Help => return say(out, format!("{USAGE}\n").as_bytes()),
        Request::Version => return say(out, b"tsort (SlateOS coreutils)\n"),
        Request::Run(file) => file,
    };

    let data = match read_input(&file) {
        Ok(data) => data,
        Err(trouble) => return trouble.report(),
    };
    // A `Stream` and not `io::stderr()`: the cycle report is threaded through
    // an `impl Write` so the tests can read it out of a `Vec`, and
    // `io::stderr()` would answer `Ok` to a write that never happened -- the
    // runtime swallows its `EBADF`, and the `let _ =` at the call site swallows
    // its `ENOSPC`. A `Stream` on descriptor 2 records the failure in the same
    // crate-wide flag `diag!` sets, so `close_stderr` in `main` can turn it
    // into the status upstream's `fclose (stderr)` would have produced.
    let mut err = Stream::stderr();

    let outcome = tsort(&data, &file, &mut out, &mut err);

    // Buffered output has to reach the OS on every exit path, the cycle one
    // included: upstream gets that from `atexit (close_stdout)`, and a cyclic
    // input still prints every item.
    // The reader having gone away is the one write failure not reported: GNU
    // dies of `SIGPIPE` there and says nothing, and this system has no signal
    // to die of -- see `coreutils::stdfd::reader_gone`. It therefore counts as
    // a flush that succeeded, and the run keeps the status it had earned.
    let flushed = match out.finish() {
        Err(e) if stdfd::reader_gone(&e) => Ok(()),
        verdict => verdict,
    };

    let ok = match outcome {
        Ok(ok) => ok,
        // `close_stdout` runs *after* the diagnostic and overrides its status,
        // so a run that failed for its own reason still reports a standard
        // output that could not take what it had written.
        Err(trouble) => {
            let code = trouble.report();
            return match flushed {
                Ok(()) => code,
                Err(e) => Trouble::Write(e).report(),
            };
        }
    };
    if let Err(e) = flushed {
        return Trouble::Write(e).report();
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Say one thing and stop -- `--help` and `--version`.
///
/// The stream is closed here rather than at the end of `main`, because these
/// two return without reaching it -- and closing it is what discovers that
/// there was nowhere to say it.
fn say(mut out: Stream, bytes: &[u8]) -> ExitCode {
    let _ = out.write_all(bytes);
    stdfd::close_stdout("tsort", out, ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------- input

fn read_input(file: &OsStr) -> Result<Vec<u8>, Trouble> {
    let mut data = Vec::new();
    if file == "-" {
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| Trouble::Read(file.to_os_string(), e))?;
    } else {
        let mut handle = File::open(file).map_err(|e| Trouble::Open(file.to_os_string(), e))?;
        handle
            .read_to_end(&mut data)
            .map_err(|e| Trouble::Read(file.to_os_string(), e))?;
    }
    Ok(data)
}

/// Cut the input into tokens the way `readtoken (stdin, DELIM, …)` does, then
/// apply the truncation `xstrdup` imposes on each.
///
/// Leading and repeated delimiters are skipped rather than producing empty
/// tokens — upstream asserts a token is never empty — but a token whose *first*
/// byte is a NUL still becomes the empty name, because the truncation happens
/// after the token has been cut.
fn tokenise(data: &[u8]) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    let mut at = 0usize;
    while at < data.len() {
        let Some(rest) = data.get(at..) else { break };
        let Some(start) = rest.iter().position(|c| !DELIM.contains(c)) else {
            break;
        };
        let begin = at.saturating_add(start);
        let Some(tail) = data.get(begin..) else { break };
        let len = tail
            .iter()
            .position(|c| DELIM.contains(c))
            .unwrap_or(tail.len());
        let token = tail.get(..len).unwrap_or_default();
        // `strcmp` and `xstrdup` both stop at the first NUL, so everything past
        // one is invisible to the item that gets stored.
        let kept = match token.iter().position(|&c| c == 0) {
            Some(nul) => token.get(..nul).unwrap_or_default(),
            None => token,
        };
        tokens.push(kept);
        at = begin.saturating_add(len);
    }
    tokens
}

// ---------------------------------------------------------------------- graph

/// One distinct token, and everything Algorithm T needs to know about it.
#[derive(Debug)]
struct Item {
    name: Vec<u8>,
    /// In-degree: how many recorded relations name this item as the successor.
    count: usize,
    printed: bool,
    /// Knuth's queue link. [`Graph::detect_loop`] borrows the same field for
    /// its backward chain, which is safe only because the queue is empty
    /// whenever a loop is being traced.
    qlink: Option<usize>,
    /// The successors, **stored back to front**. Upstream prepends to a linked
    /// list, so its walk runs newest-first; pushing here and iterating in
    /// reverse is the same sequence with an O(1) prepend.
    succ: Vec<usize>,
}

#[derive(Debug, Default)]
struct Graph {
    items: Vec<Item>,
    /// Name to index. Upstream's tree serves both as the index and as the
    /// traversal order; here the two are separate, because the traversal order
    /// is a plain sort (see the module docs).
    index: HashMap<Vec<u8>, usize>,
}

impl Graph {
    /// Upstream's `search_item`: find the token, or insert it.
    fn search_item(&mut self, name: &[u8]) -> usize {
        if let Some(&id) = self.index.get(name) {
            return id;
        }
        let id = self.items.len();
        self.items.push(Item {
            name: name.to_vec(),
            count: 0,
            printed: false,
            qlink: None,
            succ: Vec::new(),
        });
        self.index.insert(name.to_vec(), id);
        id
    }

    /// Record that `j` precedes `k`.
    ///
    /// Upstream guards with `!STREQ (j->str, k->str)`, i.e. it drops a relation
    /// from an item to itself. Tokens are interned, so that is `j != k`.
    fn record_relation(&mut self, j: usize, k: usize) {
        if j == k {
            return;
        }
        if let Some(item) = self.items.get_mut(k) {
            item.count = item.count.saturating_add(1);
        }
        if let Some(item) = self.items.get_mut(j) {
            item.succ.push(k);
        }
    }

    /// The order every pass over the items uses: sorted by name, which is what
    /// an in-order walk of upstream's `strcmp`-keyed tree produces.
    fn traversal_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        order.sort_by(|&a, &b| match (self.items.get(a), self.items.get(b)) {
            (Some(x), Some(y)) => x.name.cmp(&y.name),
            _ => std::cmp::Ordering::Equal,
        });
        order
    }

    fn count_of(&self, id: usize) -> usize {
        self.items.get(id).map_or(0, |item| item.count)
    }

    fn qlink_of(&self, id: usize) -> Option<usize> {
        self.items.get(id).and_then(|item| item.qlink)
    }

    fn set_qlink(&mut self, id: usize, to: Option<usize>) {
        if let Some(item) = self.items.get_mut(id) {
            item.qlink = to;
        }
    }

    /// Upstream's `detect_loop`, called for each item in traversal order until
    /// one returns `true`.
    ///
    /// `chain` is upstream's global `LOOP`: the head of a backwards chain of
    /// items, linked by `qlink`, each of which precedes the one before it. One
    /// pass may extend the chain several times or not at all, because the item
    /// that precedes the current head can sort anywhere — so the caller repeats
    /// the whole walk until the chain has been consumed, which happens exactly
    /// when a cycle has been closed, printed and broken.
    ///
    /// It terminates because every item with a non-zero in-degree has an
    /// unprinted predecessor, which also has a non-zero in-degree: the chain can
    /// always be extended, the item set is finite, and extending it eventually
    /// reaches an item already on it.
    fn detect_loop(&mut self, k: usize, chain: &mut Option<usize>, err: &mut dyn Write) -> bool {
        // An item with no predecessors left cannot be inside the cycle, though
        // an item with some need not be either — it may merely lead into one.
        if self.count_of(k) == 0 {
            return false;
        }
        let Some(target) = *chain else {
            *chain = Some(k);
            return false;
        };

        // Find `target` among `k`'s successors, i.e. find that `k` precedes the
        // item currently at the head of the chain. The position is kept because
        // that is the relation deleted to break the cycle.
        let succ = self.items.get(k).map_or(0, |item| item.succ.len());
        for step in 0..succ {
            // Stored back to front; walk it the way upstream's list runs.
            let Some(at) = succ.checked_sub(1).and_then(|last| last.checked_sub(step)) else {
                break;
            };
            let Some(&suc) = self.items.get(k).and_then(|item| item.succ.get(at)) else {
                break;
            };
            if suc != target {
                continue;
            }
            if self.qlink_of(k).is_none() {
                // `k` is new to the chain: link it on and stop searching *this*
                // item. The walk carries on from the next item, which may well
                // extend the chain again before the pass ends.
                self.set_qlink(k, Some(target));
                *chain = Some(k);
                return false;
            }
            // `k` is already on the chain, so following it forwards from here
            // returns to `k`: that is the cycle. Retrace, naming each item,
            // until `k` comes round again.
            let mut cursor = *chain;
            while let Some(id) = cursor {
                let next = self.qlink_of(id);
                if let Some(item) = self.items.get(id) {
                    // Upstream's `error (0, 0, "%s", …)`, which likewise cannot
                    // report a failed write to the stream it would report on.
                    let _ = err.write_all(b"tsort: ");
                    let _ = err.write_all(&item.name);
                    let _ = err.write_all(b"\n");
                }
                if id == k {
                    // Break the cycle by deleting this one relation. Its
                    // successor's in-degree drops, which may or may not reach
                    // zero; either way the caller rescans.
                    if let Some(item) = self.items.get_mut(suc) {
                        item.count = item.count.saturating_sub(1);
                    }
                    if let Some(item) = self.items.get_mut(k)
                        && at < item.succ.len()
                    {
                        item.succ.remove(at);
                    }
                    break;
                }
                self.set_qlink(id, None);
                cursor = next;
            }
            // Whatever is left of the chain — including `k`, whose link the
            // retrace deliberately left alone — is cleared, so the next cycle
            // starts from a clean field.
            while let Some(id) = cursor {
                let next = self.qlink_of(id);
                self.set_qlink(id, None);
                cursor = next;
            }
            *chain = None;
            return true;
        }
        false
    }
}

// ------------------------------------------------------------------ the sort

/// Knuth's Algorithm T over the whole input. Returns whether the run is to be
/// called a success — that is, whether no cycle was found.
fn tsort(
    data: &[u8],
    file: &OsStr,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<bool, Trouble> {
    let mut graph = Graph::default();

    // T2/T3. Read the relations. Tokens are taken in pairs, and a lone
    // trailing token is fatal.
    let tokens = tokenise(data);
    let mut pending: Option<usize> = None;
    for token in tokens {
        let k = graph.search_item(token);
        match pending.take() {
            Some(j) => graph.record_relation(j, k),
            None => pending = Some(k),
        }
    }
    if pending.is_some() {
        return Err(Trouble::OddTokens(file.to_os_string()));
    }

    // T1. N <- n, the number of distinct items.
    let order = graph.traversal_order();
    let mut remaining = graph.items.len();

    let mut ok = true;
    let mut head: Option<usize> = None;
    let mut tail: Option<usize> = None;

    while remaining > 0 {
        // T4. Scan for zeros. Every item whose predecessors have all been
        // printed joins the queue, in name order.
        for &k in &order {
            let ready = graph
                .items
                .get(k)
                .is_some_and(|item| item.count == 0 && !item.printed);
            if !ready {
                continue;
            }
            match head {
                None => head = Some(k),
                Some(_) => {
                    if let Some(t) = tail {
                        graph.set_qlink(t, Some(k));
                    }
                }
            }
            tail = Some(k);
        }

        while let Some(h) = head {
            // T5. Output the front of the queue.
            let succ = {
                let Some(item) = graph.items.get_mut(h) else {
                    break;
                };
                item.printed = true;
                std::mem::take(&mut item.succ)
            };
            if let Some(item) = graph.items.get(h) {
                out.write_all(&item.name).map_err(Trouble::Write)?;
                out.write_all(b"\n").map_err(Trouble::Write)?;
            }
            remaining = remaining.saturating_sub(1);

            // T6. Erase the relations that led out of it, queueing whatever
            // that leaves ready. New arrivals go on the same queue, so this
            // inner loop drains a whole component before T4 runs again.
            for &s in succ.iter().rev() {
                let now_zero = match graph.items.get_mut(s) {
                    Some(item) => {
                        item.count = item.count.saturating_sub(1);
                        item.count == 0
                    }
                    None => false,
                };
                if now_zero {
                    if let Some(t) = tail {
                        graph.set_qlink(t, Some(s));
                    }
                    tail = Some(s);
                }
            }
            if let Some(item) = graph.items.get_mut(h) {
                item.succ = succ;
            }

            // T7. Remove it from the queue.
            head = graph.qlink_of(h);
        }

        // T8. End of process — unless items are left, which means a cycle.
        if remaining > 0 {
            // Upstream's `error (0, 0, …)`: a diagnostic that cannot itself
            // report a failed write, since the stream it would report on is the
            // one that just failed. Standard output's writes *are* checked, a
            // few lines above, because losing an item is losing an answer.
            let _ = err.write_all(b"tsort: ");
            let _ = err.write_all(quotef_os(file).as_bytes());
            let _ = err.write_all(b": input contains a loop:\n");
            ok = false;
            let mut chain: Option<usize> = None;
            loop {
                for &k in &order {
                    if graph.detect_loop(k, &mut chain, err) {
                        break;
                    }
                }
                if chain.is_none() {
                    break;
                }
            }
        }
    }

    Ok(ok)
}

// -------------------------------------------------------------------- parsing

/// gnulib's `parse_gnu_standard_options_only`, then upstream's operand count.
///
/// The one `getopt_long` call either returns `-1` — no options anywhere on the
/// line — or ends the program, so the operand list is only ever reached when
/// every argument is an operand.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut operands: Vec<&OsString> = Vec::new();
    let mut only_operands = false;
    let mut at = 0usize;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        if only_operands {
            operands.push(arg);
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand.
            operands.push(arg);
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            return long_option(body, &bytes);
        } else {
            // There are no short options, so the first byte of the cluster is
            // already an error and the rest is never looked at.
            let Some(&c) = bytes.get(1) else {
                break;
            };
            return Err(TSORT.invalid_option(c));
        }
    }

    // `error (0, 0, "extra operand %s", quote (argv[optind + 1]))` — the
    // *second* operand is the one named, not the last.
    let mut rest = operands.iter();
    let first = rest.next();
    if let Some(extra) = rest.next() {
        return Err(TSORT.usage_referring(format!("extra operand {}", quote(&arg_bytes(extra)))));
    }
    Ok(Request::Run(match first {
        Some(name) => (*name).clone(),
        None => OsString::from("-"),
    }))
}

/// One `--name` or `--name=value` argument. Neither option takes one.
fn long_option(body: &[u8], whole: &[u8]) -> Result<Request, getopt::Error> {
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Both option names are ASCII, so a name that is not UTF-8 matches neither
    // and is reported as the bytes typed.
    let typed = std::str::from_utf8(typed).map_err(|_| TSORT.unrecognized_option(whole))?;
    let (name, which) = TSORT.resolve_long(typed, whole, LONG_OPTIONS)?;
    if inline.is_some() {
        return Err(TSORT.long_unwanted_argument(name));
    }
    Ok(match which {
        Long::Help => Request::Help,
        Long::Version => Request::Version,
    })
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// Run the sort over `input`, returning `(stdout, stderr, ok)`.
    fn run(input: &[u8]) -> (Vec<u8>, Vec<u8>, bool) {
        run_named(input, "-")
    }

    fn run_named(input: &[u8], file: &str) -> (Vec<u8>, Vec<u8>, bool) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let ok = tsort(input, OsStr::new(file), &mut out, &mut err).unwrap();
        (out, err, ok)
    }

    // ------------------------------------------------------------ tokenising

    #[test]
    fn tokenise_splits_on_the_three_delimiters() {
        assert_eq!(tokenise(b" a\tb\nc "), vec![&b"a"[..], b"b", b"c"]);
    }

    #[test]
    fn tokenise_keeps_carriage_return_inside_a_token() {
        // `\r` is not in DELIM, so this is one token, not two.
        assert_eq!(tokenise(b"a\rb x"), vec![&b"a\rb"[..], b"x"]);
    }

    #[test]
    fn tokenise_keeps_vertical_tab_and_form_feed() {
        assert_eq!(tokenise(b"a\x0bb\x0cc"), vec![&b"a\x0bb\x0cc"[..]]);
    }

    #[test]
    fn tokenise_truncates_at_a_nul() {
        assert_eq!(tokenise(b"a\0b c"), vec![&b"a"[..], b"c"]);
    }

    #[test]
    fn tokenise_a_lone_nul_is_the_empty_name() {
        assert_eq!(tokenise(b"\0 x"), vec![&b""[..], b"x"]);
    }

    #[test]
    fn tokenise_empty_input_has_no_tokens() {
        assert!(tokenise(b"").is_empty());
        assert!(tokenise(b" \t\n \n").is_empty());
    }

    #[test]
    fn tokenise_keeps_high_bytes() {
        assert_eq!(tokenise(b"\xff\xfe a"), vec![&b"\xff\xfe"[..], b"a"]);
    }

    // ---------------------------------------------------------------- sorting

    #[test]
    fn empty_input_sorts_to_nothing() {
        let (out, err, ok) = run(b"");
        assert!(out.is_empty());
        assert!(err.is_empty());
        assert!(ok);
    }

    #[test]
    fn one_relation() {
        let (out, _, ok) = run(b"a b\n");
        assert_eq!(out, b"a\nb\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn a_self_relation_is_an_item_and_no_edge() {
        let (out, _, ok) = run(b"x x\n");
        assert_eq!(out, b"x\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn successors_are_taken_newest_first() {
        // Measured: GNU prints `a d b c`, not `a b c d`.
        let (out, _, ok) = run(b"a c\na b\na d\n");
        assert_eq!(out, b"a\nd\nb\nc\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn a_repeated_relation_counts_twice() {
        // `b`'s in-degree is 2 and `a` has two successor entries, so the
        // bookkeeping has to balance for `b` to be printed at all.
        let (out, _, ok) = run(b"a b\na b\n");
        assert_eq!(out, b"a\nb\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn ready_items_come_out_in_name_order() {
        // Nothing relates `b` to `a`, so the tree walk's order decides.
        let (out, _, ok) = run(b"b z\na z\n");
        assert_eq!(out, b"a\nb\nz\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn name_order_is_bytewise_not_locale_aware() {
        let (out, _, ok) = run(b"B q\na q\n");
        assert_eq!(out, b"B\na\nq\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn a_diamond_takes_the_second_branch_first() {
        // Both rules at once: `b` and `c` become ready together, and `a`'s
        // successors are walked newest-first, so `c` — the later relation —
        // reaches the queue before `b` even though `b` sorts first.
        let (out, _, ok) = run(b"a b\na c\nb d\nc d\n");
        assert_eq!(out, b"a\nc\nb\nd\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn disconnected_components_both_appear() {
        let (out, _, ok) = run(b"a b\nc d\n");
        assert_eq!(out, b"a\nc\nb\nd\n".to_vec());
        assert!(ok);
    }

    #[test]
    fn a_long_chain_stays_in_order() {
        let (out, _, ok) = run(b"a b\nb c\nc d\nd e\n");
        assert_eq!(out, b"a\nb\nc\nd\ne\n".to_vec());
        assert!(ok);
    }

    // ----------------------------------------------------------------- cycles

    #[test]
    fn a_two_item_cycle_is_named_and_still_printed() {
        let (out, err, ok) = run_named(b"a b\nb a\n", "cyc");
        assert_eq!(out, b"a\nb\n".to_vec());
        assert_eq!(
            err,
            b"tsort: cyc: input contains a loop:\ntsort: a\ntsort: b\n".to_vec()
        );
        assert!(!ok);
    }

    #[test]
    fn a_three_item_cycle_beside_an_acyclic_part() {
        let (out, err, ok) = run_named(b"a b\nb c\nc a\nx y\n", "cyc2");
        assert_eq!(out, b"x\ny\na\nb\nc\n".to_vec());
        assert_eq!(
            err,
            b"tsort: cyc2: input contains a loop:\ntsort: a\ntsort: b\ntsort: c\n".to_vec()
        );
        assert!(!ok);
    }

    #[test]
    fn a_backwards_cycle_names_its_members_in_the_traced_order() {
        // Measured: `a c b`, which is the backward walk, not the sorted order.
        let (out, err, ok) = run_named(b"b a\nc b\na c\nd e\n", "cyc3");
        assert_eq!(out, b"d\ne\na\nc\nb\n".to_vec());
        assert_eq!(
            err,
            b"tsort: cyc3: input contains a loop:\ntsort: a\ntsort: c\ntsort: b\n".to_vec()
        );
        assert!(!ok);
    }

    #[test]
    fn two_independent_cycles_are_both_reported() {
        let (out, err, ok) = run_named(b"a b\nb a\nc d\nd c\n", "two");
        assert_eq!(out, b"a\nb\nc\nd\n".to_vec());
        assert_eq!(
            err,
            b"tsort: two: input contains a loop:\ntsort: a\ntsort: b\n\
              tsort: two: input contains a loop:\ntsort: c\ntsort: d\n"
                .to_vec()
        );
        assert!(!ok);
    }

    #[test]
    fn odd_token_count_is_fatal() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let e = tsort(b"a b c\n", OsStr::new("-"), &mut out, &mut err).unwrap_err();
        assert!(matches!(e, Trouble::OddTokens(_)));
    }

    // ---------------------------------------------------------------- parsing

    #[test]
    fn no_operand_is_standard_input() {
        assert_eq!(
            parse_args(&args(&[])).unwrap(),
            Request::Run(OsString::from("-"))
        );
    }

    #[test]
    fn a_lone_dash_is_standard_input() {
        assert_eq!(
            parse_args(&args(&["-"])).unwrap(),
            Request::Run(OsString::from("-"))
        );
    }

    #[test]
    fn one_operand() {
        assert_eq!(
            parse_args(&args(&["f"])).unwrap(),
            Request::Run(OsString::from("f"))
        );
    }

    #[test]
    fn two_operands_name_the_second() {
        let e = parse_args(&args(&["a", "b", "c"])).unwrap_err();
        assert_eq!(e.sentence, "extra operand ‘b’");
        assert_eq!(
            e.message(),
            "extra operand ‘b’\nTry 'tsort --help' for more information."
        );
        assert_eq!(e.status, 1);
    }

    #[test]
    fn double_dash_ends_the_options() {
        assert_eq!(
            parse_args(&args(&["--", "--help"])).unwrap(),
            Request::Run(OsString::from("--help"))
        );
    }

    #[test]
    fn double_dash_alone_leaves_standard_input() {
        assert_eq!(
            parse_args(&args(&["--"])).unwrap(),
            Request::Run(OsString::from("-"))
        );
    }

    #[test]
    fn options_permute_past_an_operand() {
        assert_eq!(
            parse_args(&args(&["f", "--version"])).unwrap(),
            Request::Version
        );
    }

    #[test]
    fn help_and_version_abbreviate() {
        assert_eq!(parse_args(&args(&["--hel"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--v"])).unwrap(), Request::Version);
    }

    #[test]
    fn an_empty_prefix_is_ambiguous_between_both() {
        let e = parse_args(&args(&["--=x"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "option '--=x' is ambiguous; possibilities: '--help' '--version'"
        );
    }

    #[test]
    fn help_takes_no_argument() {
        let e = parse_args(&args(&["--help=x"])).unwrap_err();
        assert_eq!(e.sentence, "option '--help' doesn't allow an argument");
    }

    #[test]
    fn an_abbreviation_is_named_by_its_resolution() {
        let e = parse_args(&args(&["--hel=x"])).unwrap_err();
        assert_eq!(e.sentence, "option '--help' doesn't allow an argument");
    }

    #[test]
    fn there_are_no_short_options() {
        // `-h` would be the natural spelling of `--help`, and is not accepted.
        assert_eq!(
            parse_args(&args(&["-h"])).unwrap_err().sentence,
            "invalid option -- 'h'"
        );
        assert_eq!(
            parse_args(&args(&["-x"])).unwrap_err().sentence,
            "invalid option -- 'x'"
        );
    }

    #[test]
    fn a_cluster_is_blamed_on_its_first_byte() {
        assert_eq!(
            parse_args(&args(&["-qz"])).unwrap_err().sentence,
            "invalid option -- 'q'"
        );
    }

    #[test]
    fn an_unknown_long_option_is_echoed_whole() {
        assert_eq!(
            parse_args(&args(&["--nope"])).unwrap_err().sentence,
            "unrecognized option '--nope'"
        );
    }
}
