//! A service menu's command, run as KDE's file manager runs it.
//!
//! The desktop entry specification reads `Exec` as a list of arguments: field
//! codes alone or inside an argument, never inside quotes, and at most one of
//! `%f %F %u %U` in a line. That is how `desktopentry::Exec` reads it, and the
//! launchers use it. KDE reads a service menu's `Exec` otherwise, and the
//! menus written for KDE are written for its reading:
//!
//! - **It is a line of shell.** Codes are expanded anywhere -- `"%f.mp3"`,
//!   `sh -c 'cd %d && make'` -- each value quoted for where it stands, so a
//!   file's name is never read as shell. A line that then needs a shell -- a
//!   `|`, `;` or `&&`, a `$VAR`, a `VAR=value` before the program -- runs as
//!   `/bin/sh -c`; any other is split into arguments and started directly,
//!   `~` at the start of a word meaning home.
//! - **A code may appear more than once**: `convert %f -rotate 90 %f` is the
//!   one file twice.
//! - **The deprecated codes still mean something**: `%d` is the file's folder
//!   (with its `/`), `%n` its name; `%D` and `%N` are those of every file.
//! - **A line with no file code is given the file anyway**, as its last
//!   argument: KDE appends `" %f"`.
//! - **One run per file**, unless the line has `%F`, `%U`, `%N` or `%D`.
//! - **The working directory** is the menu's `Path=`, or else the folder the
//!   run's (first) file is in.
//!
//! What follows is a port, over bytes, of KDE's own steps -- KIO's
//! `DesktopExecParser::resultingArguments`, and KCoreAddons'
//! `KMacroExpanderBase::expandMacrosShellQuote` and `KShell::splitArgs` -- so
//! that a line reads here as it reads there, down to what it does with
//! unusual input. Bytes, not text: a file's name is bytes, and one with no
//! UTF-8 spelling is passed as it is. Every character that matters to the
//! shell is ASCII, and no byte of a multi-byte UTF-8 character is, so reading
//! UTF-8 a byte at a time sees what reading it a character at a time does.
//!
//! Where this differs from KDE, on purpose:
//!
//! - `%i` with no icon is nothing; KDE passes `--icon ''`.
//! - `%v` (a device entry's `Dev`) is nothing.
//! - `~user` is left as written; KDE looks the user up. `~` alone is home.
//! - A line of nothing but blanks is refused; KDE would run the file itself.
//! - The program is found on `PATH` when started; KDE also looks in its own
//!   `libexec` directory.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// The shell a line that needs one is run in, as KDE runs it.
pub const SHELL: &str = "/bin/sh";

/// Why a command cannot be run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    /// Nothing to run.
    Empty,
    /// A `%` at the end of the line.
    LoneCode,
    /// A quote, bracket, brace or backquote that is never closed, a `)` or
    /// `}` that closes nothing, or a `\` or `$` with nothing after it -- the
    /// shell syntax KDE refuses before it runs anything.
    Unbalanced,
    /// A quote never closed where the arguments are split, or a `\` at the
    /// end of the line.
    BadQuoting,
    /// A file whose name this machine cannot pass to a program (only where
    /// names are not bytes -- never on SlateOS).
    Unspellable,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "nothing to run",
            Self::LoneCode => "a % at the end of the line",
            Self::Unbalanced => "a quote, bracket or brace that is never closed or never opened",
            Self::BadQuoting => "a quote that is never closed, or a \\ at the end",
            Self::Unspellable => "a file name that cannot be passed to a program here",
        })
    }
}

impl std::error::Error for CommandError {}

/// A service menu item's command, read. See the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The line as the menu wrote it.
    written: String,
    /// The line run: as written, with `" %f"` after it when it names no
    /// file.
    line: String,
    /// Whether one run takes every file: the line as written holds `%F`,
    /// `%U`, `%N` or `%D` (KService's `allowMultipleFiles`, which looks for
    /// the two characters anywhere). Otherwise each file has a run of its
    /// own.
    all_at_once: bool,
}

/// What the codes that are not a file's stand for, and where a run starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    /// `%c`: the menu's name.
    pub name: &'a str,
    /// `%i`: the menu's icon.
    pub icon: Option<&'a str>,
    /// `%k`: the menu's file.
    pub location: Option<&'a Path>,
    /// The menu's `Path=`: where every run starts, when it gives one.
    pub working_dir: Option<&'a Path>,
    /// The user's home, for a `~` starting a word.
    pub home: Option<&'a Path>,
}

/// One program to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The program and its arguments, program first.
    pub argv: Vec<OsString>,
    /// Where it starts; `None` for wherever its starter is.
    pub dir: Option<PathBuf>,
}

impl Command {
    /// Read an `Exec` value whose string escapes are already decoded (as
    /// `desktopentry::DesktopEntry::string` returns it).
    ///
    /// # Errors
    ///
    /// [`CommandError`]: a line KDE would refuse to run, or one with nothing
    /// to run.
    pub fn parse(written: &str) -> Result<Self, CommandError> {
        if written.bytes().all(|b| b == b' ' || b == b'\t') {
            return Err(CommandError::Empty);
        }
        let context = Context::default();
        // KIO's first pass: whether the line is well formed, and whether it
        // names the files at all.
        let first = expand(written.as_bytes(), &[], &context)?;
        let line = if first.names_files {
            written.to_owned()
        } else {
            format!("{written} %f")
        };
        // Split once with a file in it, so a line that cannot be split is
        // refused here rather than at the click.
        let probe = expand(line.as_bytes(), &[b"/f"], &context)?;
        if let Split::BadQuoting = split(&probe.text, None) {
            return Err(CommandError::BadQuoting);
        }
        Ok(Self {
            all_at_once: ["%F", "%U", "%N", "%D"]
                .iter()
                .any(|code| written.contains(code)),
            written: written.to_owned(),
            line,
        })
    }

    /// The line as the menu wrote it.
    #[must_use]
    pub fn as_written(&self) -> &str {
        &self.written
    }

    /// The programs to start for `files`: one for all of them, or one each
    /// (see the module docs).
    ///
    /// # Errors
    ///
    /// [`CommandError::Unspellable`] where a file's name cannot be passed;
    /// the others only if this were handed a command [`parse`](Self::parse)
    /// did not check.
    pub fn runs(&self, files: &[&Path], context: &Context<'_>) -> Result<Vec<Run>, CommandError> {
        let groups: Vec<&[&Path]> = if self.all_at_once || files.len() <= 1 {
            vec![files]
        } else {
            files.chunks(1).collect()
        };
        groups
            .into_iter()
            .map(|group| {
                let names: Vec<&[u8]> = group
                    .iter()
                    .map(|file| file.as_os_str().as_encoded_bytes())
                    .collect();
                let expanded = expand(self.line.as_bytes(), &names, context)?;
                let argv = match split(&expanded.text, context.home) {
                    Split::Words(words) => words,
                    Split::Meta => vec![SHELL.as_bytes().to_vec(), b"-c".to_vec(), expanded.text],
                    Split::BadQuoting => return Err(CommandError::BadQuoting),
                };
                let argv = argv
                    .into_iter()
                    .map(os_from)
                    .collect::<Option<Vec<OsString>>>()
                    .ok_or(CommandError::Unspellable)?;
                if argv.is_empty() {
                    return Err(CommandError::Empty);
                }
                let dir = context
                    .working_dir
                    .map(Path::to_path_buf)
                    .or_else(|| group.first().and_then(|file| folder_of(file)));
                Ok(Run { argv, dir })
            })
            .collect()
    }
}

/// The folder `file` is in, links resolved, as KIO takes a run's working
/// directory from its first file. A file that cannot be resolved -- gone,
/// or unreadable on the way -- gives none, and the program starts where its
/// starter is, as KDE's does when the file does not exist.
fn folder_of(file: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(file).ok()?;
    real.parent().map(Path::to_path_buf)
}

/// `bytes` as an argument: itself where names are bytes; where they are not,
/// only if it is UTF-8.
#[cfg(unix)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the other platforms' version can fail"
)]
fn os_from(bytes: Vec<u8>) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt;
    Some(OsString::from_vec(bytes))
}

/// `bytes` as an argument: itself where names are bytes; where they are not,
/// only if it is UTF-8.
#[cfg(not(unix))]
fn os_from(bytes: Vec<u8>) -> Option<OsString> {
    String::from_utf8(bytes).ok().map(OsString::from)
}

// ============================================================================
// Expanding the codes (KMacroExpanderBase::expandMacrosShellQuote)
// ============================================================================

/// The quoting the expander is inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quoting {
    None,
    Single,
    Double,
    /// `$'...'`.
    Dollar,
    /// `$( ... )`, `( ... )` or a backquoted command.
    Paren,
    /// `${ ... }`.
    Subst,
    /// `{ ... }`.
    Group,
    /// `$(( ... ))`.
    Math,
}

#[derive(Clone, Copy, Debug)]
struct State {
    current: Quoting,
    /// Whether a double quote is open anywhere up the stack that has not
    /// been left by a command substitution.
    dquote: bool,
}

/// A line with its codes expanded.
struct Expanded {
    text: Vec<u8>,
    /// Whether it held a file's code: `%f %F %u %U %n %N %d %D %v`.
    names_files: bool,
}

/// The byte at `i`, or NUL past the end -- which is what KDE reads there,
/// from the terminator of its string, and which matches nothing it looks
/// for.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// Expand every code in `line` for `files`, each value quoted for where it
/// stands. A port of KDE's expander; the comments name its quirks.
///
/// # Errors
///
/// [`CommandError::LoneCode`], or [`CommandError::Unbalanced`] where KDE's
/// expander reports the line's shell syntax broken.
#[allow(
    clippy::too_many_lines,
    reason = "one state machine, transcribed branch for branch from KDE's; split up it could no longer be checked against it"
)]
fn expand(line: &[u8], files: &[&[u8]], context: &Context<'_>) -> Result<Expanded, CommandError> {
    let mut s = line.to_vec();
    let mut pos = 0usize;
    let mut state = State {
        current: Quoting::None,
        dquote: false,
    };
    let mut stack: Vec<State> = Vec::new();
    // Where each `$((` began, to go back to if it turns out to be `$( (`.
    let mut maths: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut names_files = false;
    while pos < s.len() {
        let cc = at(&s, pos);
        if cc == b'%' {
            let Some(&code) = s.get(pos.saturating_add(1)) else {
                return Err(CommandError::LoneCode);
            };
            match code_values(code, files, context, &mut names_files) {
                // Not a code: kept, both characters, and neither is looked at
                // again -- so `%'` does not open a quote. KDE's reading.
                None => pos = pos.saturating_add(2),
                Some(values) => {
                    let quoted = if state.dquote {
                        backslash_before(&values.join(&b' '), b"$`\"\\")
                    } else if state.current == Quoting::Dollar {
                        backslash_before(&values.join(&b' '), b"'\\")
                    } else if state.current == Quoting::Single {
                        close_and_reopen(&values.join(&b' '))
                    } else {
                        // No values: nothing, which removes the code.
                        join_args(&values)
                    };
                    let end = pos.saturating_add(2);
                    s.splice(pos..end, quoted.iter().copied());
                    pos = pos.saturating_add(quoted.len());
                }
            }
            continue;
        }
        if state.current == Quoting::Single {
            if cc == b'\'' {
                state = pop(&mut stack)?;
            }
        } else if cc == b'\\' {
            // The escaped character is never looked at, so no expansion can
            // start or end inside an escape.
            pos = pos.saturating_add(2);
            continue;
        } else if state.current == Quoting::Dollar {
            if cc == b'\'' {
                state = pop(&mut stack)?;
            }
        } else if cc == b'$' {
            pos = pos.saturating_add(1);
            let next = at(&s, pos);
            if next == b'(' {
                stack.push(state);
                if at(&s, pos.saturating_add(1)) == b'(' {
                    maths.push((s.clone(), pos.saturating_add(2)));
                    state.current = Quoting::Math;
                    pos = pos.saturating_add(2);
                    continue;
                }
                state.current = Quoting::Paren;
                state.dquote = false;
            } else if next == b'{' {
                stack.push(state);
                state.current = Quoting::Subst;
            } else if !state.dquote {
                if next == b'\'' {
                    stack.push(state);
                    state.current = Quoting::Dollar;
                } else if next == b'"' {
                    stack.push(state);
                    state.current = Quoting::Double;
                    state.dquote = true;
                }
            }
            // The character after `$` is passed over below, whatever it was.
        } else if cc == b'`' {
            // A backquoted command is rewritten as `$( ... )`, in the line
            // the shell will be given too -- KDE's rewriting.
            s.splice(pos..pos.saturating_add(1), b"$( ".iter().copied());
            pos = pos.saturating_add(3);
            let mut end = pos;
            loop {
                let Some(&c) = s.get(end) else {
                    return Err(CommandError::Unbalanced);
                };
                if c == b'`' {
                    break;
                }
                if c == b'\\' {
                    end = end.saturating_add(1);
                    let escaped = at(&s, end);
                    if escaped == b'$'
                        || escaped == b'`'
                        || escaped == b'\\'
                        || (escaped == b'"' && state.dquote)
                    {
                        s.remove(end.saturating_sub(1));
                        continue;
                    }
                }
                end = end.saturating_add(1);
            }
            if let Some(close) = s.get_mut(end) {
                *close = b')';
            }
            stack.push(state);
            state.current = Quoting::Paren;
            state.dquote = false;
            continue;
        } else if state.current == Quoting::Double {
            if cc == b'"' {
                state = pop(&mut stack)?;
            }
        } else if cc == b'\'' {
            if !state.dquote {
                stack.push(state);
                state.current = Quoting::Single;
            }
        } else if cc == b'"' {
            if !state.dquote {
                stack.push(state);
                state.current = Quoting::Double;
                state.dquote = true;
            }
        } else if state.current == Quoting::Subst {
            if cc == b'}' {
                state = pop(&mut stack)?;
            }
        } else if cc == b')' {
            if state.current == Quoting::Math {
                if at(&s, pos.saturating_add(1)) == b')' {
                    state = pop(&mut stack)?;
                    pos = pos.saturating_add(2);
                } else {
                    // Not `$((` after all but `$( (`: back to just after it,
                    // as it was then, read as two commands.
                    let (saved, from) = maths.pop().ok_or(CommandError::Unbalanced)?;
                    s = saved;
                    pos = from;
                    state.current = Quoting::Paren;
                    state.dquote = false;
                    stack.push(state);
                }
                continue;
            } else if state.current == Quoting::Paren {
                state = pop(&mut stack)?;
            } else {
                // A `)` that closes nothing ends the reading here, short of
                // the end of the line -- which KDE reports as broken.
                break;
            }
        } else if cc == b'}' {
            if state.current == Quoting::Group {
                state = pop(&mut stack)?;
            } else {
                break;
            }
        } else if cc == b'(' {
            stack.push(state);
            state.current = Quoting::Paren;
        } else if cc == b'{' {
            stack.push(state);
            state.current = Quoting::Group;
        }
        pos = pos.saturating_add(1);
    }
    if stack.is_empty() && pos == s.len() {
        Ok(Expanded {
            text: s,
            names_files,
        })
    } else {
        Err(CommandError::Unbalanced)
    }
}

/// The state a quote or bracket was opened from.
fn pop(stack: &mut Vec<State>) -> Result<State, CommandError> {
    stack.pop().ok_or(CommandError::Unbalanced)
}

/// The values a code stands for; `None` for a `%` and letter that are not a
/// code, which KDE keeps as written.
fn code_values(
    code: u8,
    files: &[&[u8]],
    context: &Context<'_>,
    names_files: &mut bool,
) -> Option<Vec<Vec<u8>>> {
    Some(match code {
        b'c' => vec![context.name.as_bytes().to_vec()],
        b'k' => vec![
            context
                .location
                .map(|path| path.as_os_str().as_encoded_bytes().to_vec())
                .unwrap_or_default(),
        ],
        b'i' => match context.icon.filter(|icon| !icon.is_empty()) {
            Some(icon) => vec![b"--icon".to_vec(), icon.as_bytes().to_vec()],
            None => Vec::new(),
        },
        // `%m`, the mini-icon, which KDE warns about and drops.
        b'm' => Vec::new(),
        b'f' | b'u' | b'n' | b'd' | b'v' => {
            *names_files = true;
            // One file's code with other than one file stands for nothing
            // -- KDE's "N URLs supplied to single-URL service".
            match files {
                [file] => subst(code, file).into_iter().collect(),
                _ => Vec::new(),
            }
        }
        b'F' | b'U' | b'N' | b'D' => {
            *names_files = true;
            let each = code.to_ascii_lowercase();
            files.iter().filter_map(|file| subst(each, file)).collect()
        }
        b'%' => vec![b"%".to_vec()],
        _ => return None,
    })
}

/// What one file's code stands for.
fn subst(code: u8, file: &[u8]) -> Option<Vec<u8>> {
    let slash = file.iter().rposition(|&b| b == b'/');
    match code {
        b'f' | b'u' => Some(file.to_vec()),
        // The folder, ending in its `/`, as a URL's path with its file name
        // removed is.
        b'd' => Some(match slash {
            Some(i) => file.get(..=i).unwrap_or_default().to_vec(),
            None => Vec::new(),
        }),
        b'n' => Some(match slash {
            Some(i) => file.get(i.saturating_add(1)..).unwrap_or_default().to_vec(),
            None => file.to_vec(),
        }),
        _ => None,
    }
}

/// `text` with a backslash before each byte in `special`: a value inside
/// double quotes, or inside `$'...'`.
fn backslash_before(text: &[u8], special: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &b in text {
        if special.contains(&b) {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// `text` inside single quotes: each `'` closes the quote, is escaped, and
/// opens it again.
fn close_and_reopen(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &b in text {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out
}

/// Values where no quote is open: each one word, quoted if it needs it.
fn join_args(values: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        if !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(&quote_arg(value));
    }
    out
}

/// One word that the shell will read as `value` and nothing else: as it is
/// if nothing in it is special, else in single quotes.
fn quote_arg(value: &[u8]) -> Vec<u8> {
    if value.is_empty() {
        return b"''".to_vec();
    }
    if !value.iter().any(|&b| is_special(b)) {
        return value.to_vec();
    }
    let mut out = vec![b'\''];
    out.extend_from_slice(&close_and_reopen(value));
    out.push(b'\'');
    out
}

/// A byte that makes a value need quoting: a control character or blank,
/// or one of `` !"#$&'()*;<>?[\]`{|}~ ``.
fn is_special(b: u8) -> bool {
    b <= b' ' || b"!\"#$&'()*;<>?[\\]`{|}~".contains(&b)
}

// ============================================================================
// Splitting into arguments (KShell::splitArgs, AbortOnMeta | TildeExpand)
// ============================================================================

/// How a line splits.
#[derive(Debug, PartialEq, Eq)]
enum Split {
    /// Into these arguments.
    Words(Vec<Vec<u8>>),
    /// It needs a shell.
    Meta,
    /// A quote never closed, or a `\` at the end.
    BadQuoting,
}

/// A byte that means the line needs a shell, where no quote holds it.
fn is_meta(b: u8) -> bool {
    b"\"#$&'()*;<>?[\\]`{|}".contains(&b)
}

/// The home `~name` means: the user's own for `~`, and none -- the `~` kept
/// as written -- for anyone else's.
fn home_dir(name: &[u8], home: Option<&[u8]>) -> Option<Vec<u8>> {
    if name.is_empty() {
        home.filter(|h| !h.is_empty()).map(<[u8]>::to_vec)
    } else {
        None
    }
}

/// `char` `code` in UTF-8, as KDE's text would be when handed to a program.
fn push_char(out: &mut Vec<u8>, code: u32) {
    if let Some(c) = char::from_u32(code) {
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

/// An octal digit's value; the callers have matched one.
fn octal(b: u8) -> u32 {
    char::from(b).to_digit(8).unwrap_or(0)
}

/// A hexadecimal digit's value.
fn hex(b: u8) -> Option<u32> {
    char::from(b).to_digit(16)
}

/// Split `args` into arguments as a shell would, or say it needs a shell.
/// A port of KDE's splitter; words are separated by spaces only, as there.
#[allow(
    clippy::too_many_lines,
    reason = "one scanner, transcribed branch for branch from KDE's"
)]
fn split(args: &[u8], home: Option<&Path>) -> Split {
    let home = home.map(|h| h.as_os_str().as_encoded_bytes());
    let len = args.len();
    let mut words: Vec<Vec<u8>> = Vec::new();
    let mut first_word = true;
    let mut pos = 0usize;
    loop {
        let mut c;
        loop {
            let Some(&b) = args.get(pos) else {
                return Split::Words(words);
            };
            pos = pos.saturating_add(1);
            c = b;
            if c != b' ' {
                break;
            }
        }
        let mut word: Vec<u8> = Vec::new();
        if c == b'~' {
            let start = pos;
            // KDE's scan also stops at a quote, `\` or `$` -- the `~` is then
            // a character -- and at a shell character, handing the line to a
            // shell. Here either makes the name someone else's home, which is
            // never looked up, so the `~` is a character all the same and the
            // shell character is met below.
            while let Some(&b) = args.get(pos) {
                c = b;
                if c == b'/' || c == b' ' {
                    break;
                }
                pos = pos.saturating_add(1);
            }
            match home_dir(args.get(start..pos).unwrap_or_default(), home) {
                None => {
                    pos = start;
                    c = b'~';
                }
                Some(dir) => {
                    if pos >= len {
                        words.push(dir);
                        return Split::Words(words);
                    }
                    pos = pos.saturating_add(1);
                    if c == b' ' {
                        words.push(dir);
                        first_word = false;
                        continue;
                    }
                    word = dir;
                }
            }
        }
        // `NAME=value` before the program is an assignment, for a shell. (A
        // `~` read as a character is never a name's start, so it needs no
        // check of its own, as KDE's `goto` gives it.)
        if first_word && (c == b'_' || c.is_ascii_alphabetic()) {
            let after_name = args
                .get(pos..)
                .unwrap_or_default()
                .iter()
                .find(|&&b| !(b == b'_' || b.is_ascii_alphanumeric()));
            if after_name == Some(&b'=') {
                return Split::Meta;
            }
        }
        loop {
            if c == b'\'' {
                let start = pos;
                loop {
                    let Some(&b) = args.get(pos) else {
                        return Split::BadQuoting;
                    };
                    pos = pos.saturating_add(1);
                    if b == b'\'' {
                        break;
                    }
                }
                word.extend_from_slice(args.get(start..pos.saturating_sub(1)).unwrap_or_default());
            } else if c == b'"' {
                loop {
                    let Some(&b) = args.get(pos) else {
                        return Split::BadQuoting;
                    };
                    pos = pos.saturating_add(1);
                    let mut ch = b;
                    if ch == b'"' {
                        break;
                    }
                    if ch == b'\\' {
                        let Some(&e) = args.get(pos) else {
                            return Split::BadQuoting;
                        };
                        pos = pos.saturating_add(1);
                        ch = e;
                        if ch != b'"' && ch != b'\\' && ch != b'$' && ch != b'`' {
                            word.push(b'\\');
                        }
                    } else if ch == b'$' || ch == b'`' {
                        return Split::Meta;
                    }
                    word.push(ch);
                }
            } else if c == b'$' && args.get(pos) == Some(&b'\'') {
                pos = pos.saturating_add(1);
                if let Err(bad) = dollar_quote(args, &mut pos, &mut word) {
                    return bad;
                }
            } else {
                let mut ch = c;
                if ch == b'\\' {
                    let Some(&e) = args.get(pos) else {
                        return Split::BadQuoting;
                    };
                    pos = pos.saturating_add(1);
                    ch = e;
                } else if is_meta(ch) {
                    return Split::Meta;
                }
                word.push(ch);
            }
            let Some(&b) = args.get(pos) else {
                break;
            };
            pos = pos.saturating_add(1);
            c = b;
            if c == b' ' {
                break;
            }
        }
        words.push(word);
        first_word = false;
    }
}

/// The inside of a `$'...'`, from just past its opening quote to just past
/// its closing one, its escapes decoded as KDE decodes them.
fn dollar_quote(args: &[u8], pos: &mut usize, word: &mut Vec<u8>) -> Result<(), Split> {
    loop {
        let &b = args.get(*pos).ok_or(Split::BadQuoting)?;
        *pos = pos.saturating_add(1);
        if b == b'\'' {
            return Ok(());
        }
        if b != b'\\' {
            word.push(b);
            continue;
        }
        let &e = args.get(*pos).ok_or(Split::BadQuoting)?;
        *pos = pos.saturating_add(1);
        match e {
            b'a' => word.push(0x07),
            b'b' => word.push(0x08),
            b'e' => word.push(0x1b),
            b'f' => word.push(0x0c),
            b'n' => word.push(b'\n'),
            b'r' => word.push(b'\r'),
            b't' => word.push(b'\t'),
            b'\\' => word.push(b'\\'),
            b'\'' => word.push(b'\''),
            b'c' => {
                let &x = args.get(*pos).ok_or(Split::BadQuoting)?;
                *pos = pos.saturating_add(1);
                push_char(word, u32::from(x & 31));
            }
            b'x' => {
                let &d = args.get(*pos).ok_or(Split::BadQuoting)?;
                let mut value = hex(d).ok_or(Split::BadQuoting)?;
                *pos = pos.saturating_add(1);
                // A second digit only if it is not `0`: KDE's reading,
                // kept. (KDE also adds nothing at the very end of the line,
                // where a `$'` left open is refused anyway.)
                if let Some(low) = args.get(*pos).and_then(|&d| hex(d)).filter(|&v| v > 0) {
                    value = value.saturating_mul(16).saturating_add(low);
                    *pos = pos.saturating_add(1);
                }
                push_char(word, value);
            }
            b'0'..=b'7' => {
                let mut value = octal(e);
                for _ in 0..2 {
                    match args.get(*pos) {
                        Some(&d @ b'0'..=b'7') => {
                            value = value.saturating_mul(8).saturating_add(octal(d));
                            *pos = pos.saturating_add(1);
                        }
                        _ => break,
                    }
                }
                push_char(word, value);
            }
            other => {
                word.push(b'\\');
                word.push(other);
            }
        }
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
