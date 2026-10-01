//! The `Exec` key: a command line with its own quoting rules and `%` codes
//! for the files, icon and name it is started with.
//!
//! Parsed once, when the entry is read, so that a line that cannot be started
//! is refused then -- a menu does not offer a row that fails when clicked --
//! and expanded per launch into an argument vector. Never into a string for a
//! shell: a file called `a b.txt` stays one argument, and one whose name has
//! no UTF-8 spelling still arrives, byte for byte.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// A parsed `Exec` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exec {
    /// The program and its arguments; the first is plain text.
    args: Vec<Arg>,
}

/// One argument: text and codes, joined when expanded.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Arg {
    pieces: Vec<Piece>,
    /// Whether any of it was quoted. `""` is an argument -- an empty one --
    /// where an argument whose only content was a code that expanded to
    /// nothing is not.
    quoted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    Code(Code),
}

/// The field codes the specification defines and does not deprecate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Code {
    /// `%f`: one local file.
    File,
    /// `%F`: every local file, one argument each.
    Files,
    /// `%u`: one URL or local file.
    Url,
    /// `%U`: every URL and local file, one argument each.
    Urls,
    /// `%i`: `--icon <Icon>`, or nothing when there is no icon.
    Icon,
    /// `%c`: the entry's name, in the reader's language.
    Name,
    /// `%k`: where the desktop entry file is.
    Location,
}

impl Code {
    const fn takes_targets(self) -> bool {
        matches!(self, Self::File | Self::Files | Self::Url | Self::Urls)
    }

    const fn is_list(self) -> bool {
        matches!(self, Self::Files | Self::Urls)
    }
}

/// Why an `Exec` line cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// Nothing to run.
    Empty,
    /// A `"` with no closing `"`.
    UnterminatedQuote,
    /// A `%` followed by a letter the specification does not define -- which
    /// makes the entry invalid, by its own words.
    UnknownCode(char),
    /// A `%` as the last character.
    LoneCode,
    /// `%F` or `%U` sharing an argument with other text: they expand to
    /// several arguments and "may only be used as an argument on their own".
    ListCodeNotAlone(char),
    /// More than one of `%f`, `%F`, `%u`, `%U`, which the specification
    /// allows once.
    SeveralFileCodes,
    /// The program itself is, or contains, a field code.
    CodeInProgram,
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "nothing to run"),
            Self::UnterminatedQuote => write!(f, "a quote that is never closed"),
            Self::UnknownCode(c) => write!(f, "%{c} is not a field code"),
            Self::LoneCode => write!(f, "a % at the end of the line"),
            Self::ListCodeNotAlone(c) => write!(f, "%{c} must be an argument on its own"),
            Self::SeveralFileCodes => write!(f, "more than one of %f, %F, %u and %U"),
            Self::CodeInProgram => write!(f, "the program is a field code"),
        }
    }
}

impl std::error::Error for ExecError {}

/// What a program is started with: a file on this machine, or a URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A local file, as the bytes of its path.
    File(PathBuf),
    /// A URL that is not a local file.
    Url(String),
}

/// What the `%i`, `%c` and `%k` codes stand for, in one launch.
#[derive(Clone, Copy, Debug, Default)]
pub struct Invocation<'a> {
    /// The entry's `Icon`, for `%i`.
    pub icon: Option<&'a str>,
    /// The entry's `Name` in the reader's language, for `%c`.
    pub name: &'a str,
    /// The desktop entry file, for `%k`.
    pub location: Option<&'a Path>,
}

impl Exec {
    /// Parse an `Exec` value whose string escapes are already decoded (as
    /// [`crate::DesktopEntry::string`] returns it).
    ///
    /// Arguments are separated by spaces or tabs. A quoted argument is in
    /// double quotes, inside which `\"`, `` \` ``, `\$` and `\\` stand for the
    /// character after the backslash; a code inside quotes is text, since the
    /// specification leaves that undefined and text is what it was written
    /// as. `%%` is a `%`; the deprecated codes (`%d %D %n %N %v %m`) are
    /// removed, as the specification says to.
    ///
    /// Two things the specification leaves undefined are read as GLib reads
    /// them, because entries written for the desktops that use it are the
    /// entries a user installs: single quotes hold everything up to the next
    /// single quote literally (`sh -c 'echo hi'`), and an unquoted backslash
    /// makes the character after it ordinary (`/opt/My\ App/run`).
    ///
    /// # Errors
    ///
    /// [`ExecError`].
    pub fn parse(line: &str) -> Result<Self, ExecError> {
        let mut args: Vec<Arg> = Vec::new();
        let mut current: Option<Arg> = None;
        let mut text = String::new();
        let mut chars = line.chars();
        // Moves pending text into the argument being built.
        fn flush(current: &mut Option<Arg>, text: &mut String) {
            if !text.is_empty() {
                let arg = current.get_or_insert_with(|| Arg {
                    pieces: Vec::new(),
                    quoted: false,
                });
                arg.pieces.push(Piece::Text(std::mem::take(text)));
            }
        }
        while let Some(c) = chars.next() {
            match c {
                ' ' | '\t' => {
                    flush(&mut current, &mut text);
                    if let Some(arg) = current.take() {
                        args.push(arg);
                    }
                }
                '"' => {
                    current
                        .get_or_insert_with(|| Arg {
                            pieces: Vec::new(),
                            quoted: false,
                        })
                        .quoted = true;
                    let mut closed = false;
                    while let Some(q) = chars.next() {
                        match q {
                            '"' => {
                                closed = true;
                                break;
                            }
                            '\\' => match chars.next() {
                                Some(e @ ('"' | '`' | '$' | '\\')) => text.push(e),
                                Some(other) => {
                                    text.push('\\');
                                    text.push(other);
                                }
                                None => return Err(ExecError::UnterminatedQuote),
                            },
                            other => text.push(other),
                        }
                    }
                    if !closed {
                        return Err(ExecError::UnterminatedQuote);
                    }
                }
                '%' => {
                    let code = match chars.next() {
                        None => return Err(ExecError::LoneCode),
                        Some('%') => {
                            text.push('%');
                            continue;
                        }
                        Some('f') => Some(Code::File),
                        Some('F') => Some(Code::Files),
                        Some('u') => Some(Code::Url),
                        Some('U') => Some(Code::Urls),
                        Some('i') => Some(Code::Icon),
                        Some('c') => Some(Code::Name),
                        Some('k') => Some(Code::Location),
                        Some('d' | 'D' | 'n' | 'N' | 'v' | 'm') => None,
                        Some(other) => return Err(ExecError::UnknownCode(other)),
                    };
                    flush(&mut current, &mut text);
                    let arg = current.get_or_insert_with(|| Arg {
                        pieces: Vec::new(),
                        quoted: false,
                    });
                    if let Some(code) = code {
                        arg.pieces.push(Piece::Code(code));
                    }
                }
                '\'' => {
                    current
                        .get_or_insert_with(|| Arg {
                            pieces: Vec::new(),
                            quoted: false,
                        })
                        .quoted = true;
                    let mut closed = false;
                    for q in chars.by_ref() {
                        if q == '\'' {
                            closed = true;
                            break;
                        }
                        text.push(q);
                    }
                    if !closed {
                        return Err(ExecError::UnterminatedQuote);
                    }
                }
                '\\' => text.push(chars.next().unwrap_or('\\')),
                other => text.push(other),
            }
        }
        flush(&mut current, &mut text);
        if let Some(arg) = current.take() {
            args.push(arg);
        }
        // An argument that was nothing but deprecated codes is no argument.
        args.retain(|arg| arg.quoted || !arg.pieces.is_empty());

        let program = args.first().ok_or(ExecError::Empty)?;
        if program.pieces.iter().any(|p| matches!(p, Piece::Code(_))) {
            return Err(ExecError::CodeInProgram);
        }
        if program.pieces.is_empty() {
            return Err(ExecError::Empty);
        }
        let mut target_codes = 0usize;
        for arg in &args {
            for piece in &arg.pieces {
                if let Piece::Code(code) = piece {
                    if code.takes_targets() {
                        target_codes = target_codes.saturating_add(1);
                    }
                    if code.is_list() && arg.pieces.len() > 1 {
                        let letter = if *code == Code::Files { 'F' } else { 'U' };
                        return Err(ExecError::ListCodeNotAlone(letter));
                    }
                }
            }
        }
        if target_codes > 1 {
            return Err(ExecError::SeveralFileCodes);
        }
        Ok(Self { args })
    }

    /// The program to run: the line's first argument, as written.
    #[must_use]
    pub fn program(&self) -> String {
        self.args
            .first()
            .map(|arg| {
                arg.pieces
                    .iter()
                    .map(|p| match p {
                        Piece::Text(t) => t.as_str(),
                        Piece::Code(_) => "",
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the program can be started with files or URLs at all.
    #[must_use]
    pub fn takes_targets(&self) -> bool {
        self.codes().any(Code::takes_targets)
    }

    fn codes(&self) -> impl Iterator<Item = Code> + '_ {
        self.args.iter().flat_map(|arg| {
            arg.pieces.iter().filter_map(|p| match p {
                Piece::Code(code) => Some(*code),
                Piece::Text(_) => None,
            })
        })
    }

    /// The command lines that start the program on `targets`, program first.
    ///
    /// One line for all of them, unless the line asks for one at a time
    /// (`%f`, `%u`), when it is one line per target that code accepts -- `%f`
    /// takes local files only, and a URL given to it is left out, since there
    /// is no file to pass. With no targets, one line with the file codes
    /// removed: "if the application should not open any file the %f, %u, %F
    /// and %U field codes must be removed from the command line and ignored".
    #[must_use]
    pub fn command_lines(
        &self,
        targets: &[Target],
        invocation: &Invocation<'_>,
    ) -> Vec<Vec<OsString>> {
        let one_at_a_time = self
            .codes()
            .find(|code| matches!(code, Code::File | Code::Url));
        match one_at_a_time {
            Some(code) => {
                let accepted: Vec<&Target> = targets
                    .iter()
                    .filter(|t| code == Code::Url || matches!(t, Target::File(_)))
                    .collect();
                if accepted.is_empty() {
                    return vec![self.expand(&[], invocation)];
                }
                accepted
                    .into_iter()
                    .map(|target| self.expand(std::slice::from_ref(target), invocation))
                    .collect()
            }
            None => vec![self.expand(targets, invocation)],
        }
    }

    /// One command line with `targets` substituted.
    fn expand(&self, targets: &[Target], invocation: &Invocation<'_>) -> Vec<OsString> {
        let files = || {
            targets.iter().filter_map(|t| match t {
                Target::File(path) => Some(path.as_os_str().to_owned()),
                Target::Url(_) => None,
            })
        };
        let all = || {
            targets.iter().map(|t| match t {
                Target::File(path) => path.as_os_str().to_owned(),
                Target::Url(url) => OsString::from(url),
            })
        };
        let mut out = Vec::new();
        for arg in &self.args {
            // Codes that stand for several arguments -- or none -- when they
            // are the whole argument.
            if let [Piece::Code(code)] = arg.pieces.as_slice() {
                match code {
                    Code::Files => {
                        out.extend(files());
                        continue;
                    }
                    Code::Urls => {
                        out.extend(all());
                        continue;
                    }
                    Code::File => {
                        out.extend(files().take(1));
                        continue;
                    }
                    Code::Url => {
                        out.extend(all().take(1));
                        continue;
                    }
                    Code::Icon => {
                        if let Some(icon) = invocation.icon.filter(|i| !i.is_empty()) {
                            out.push(OsString::from("--icon"));
                            out.push(OsString::from(icon));
                        }
                        continue;
                    }
                    Code::Name | Code::Location => {}
                }
            }
            let mut built = OsString::new();
            for piece in &arg.pieces {
                match piece {
                    Piece::Text(text) => built.push(text),
                    Piece::Code(Code::File) => {
                        if let Some(file) = files().next() {
                            built.push(file);
                        }
                    }
                    Piece::Code(Code::Url) => {
                        if let Some(target) = all().next() {
                            built.push(target);
                        }
                    }
                    // Refused by `parse` when not alone; unreachable here,
                    // and nothing is the right expansion if it were not.
                    Piece::Code(Code::Files | Code::Urls) => {}
                    Piece::Code(Code::Icon) => built.push(invocation.icon.unwrap_or_default()),
                    Piece::Code(Code::Name) => built.push(invocation.name),
                    Piece::Code(Code::Location) => {
                        built.push(invocation.location.map_or(OsStr::new(""), Path::as_os_str));
                    }
                }
            }
            if arg.quoted || !built.is_empty() {
                out.push(built);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    fn lines(exec: &str, targets: &[Target]) -> Vec<Vec<String>> {
        let invocation = Invocation {
            icon: Some("calc"),
            name: "Calculator",
            location: Some(Path::new("/usr/share/applications/calc.desktop")),
        };
        Exec::parse(exec)
            .expect("parses")
            .command_lines(targets, &invocation)
            .into_iter()
            .map(|line| {
                line.into_iter()
                    .map(|a| a.into_string().expect("test arguments are UTF-8"))
                    .collect()
            })
            .collect()
    }

    fn file(p: &str) -> Target {
        Target::File(PathBuf::from(p))
    }

    /// **A line splits into arguments on blanks, and quoting keeps an
    /// argument together**, with the four escapes quoting allows.
    #[test]
    fn arguments_split_on_blanks_and_quoting_holds_one_together() {
        assert_eq!(lines("prog  a\tb", &[]), [["prog", "a", "b"]]);
        assert_eq!(
            lines(r#"prog "two words" "a \"b\" \`c\` \$d \\e" "" x"#, &[]),
            [["prog", "two words", r#"a "b" `c` $d \e"#, "", "x"]]
        );
        // Any other backslash inside quotes is itself.
        assert_eq!(lines(r#"prog "a\zb""#, &[]), [["prog", r"a\zb"]]);
        // Quoted and unquoted parts of one word are one argument.
        assert_eq!(
            lines(r#"prog --title="My App""#, &[]),
            [["prog", "--title=My App"]]
        );
    }

    /// With no files, the file codes go, and any argument that was only a
    /// code with them.
    #[test]
    fn with_no_files_the_file_codes_are_removed() {
        assert_eq!(lines("prog %U", &[]), [["prog"]]);
        assert_eq!(lines("prog %f", &[]), [["prog"]]);
        assert_eq!(lines("prog --open=%f", &[]), [["prog", "--open="]]);
    }

    /// **`%F` and `%U` pass every target in one line; `%f` and `%u` start
    /// the program once per target.**
    #[test]
    fn list_codes_pass_all_and_single_codes_start_one_each() {
        let t = [
            file("/a.txt"),
            file("/b c.txt"),
            Target::Url("https://x.org/".into()),
        ];
        assert_eq!(lines("prog %F", &t), [["prog", "/a.txt", "/b c.txt"]]);
        assert_eq!(
            lines("prog %U", &t),
            [["prog", "/a.txt", "/b c.txt", "https://x.org/"]]
        );
        assert_eq!(
            lines("prog %f", &t),
            [["prog", "/a.txt"], ["prog", "/b c.txt"]],
            "%f takes local files only"
        );
        assert_eq!(
            lines("prog %u", &t),
            [
                ["prog", "/a.txt"],
                ["prog", "/b c.txt"],
                ["prog", "https://x.org/"]
            ]
        );
        assert_eq!(
            lines("prog --file=%f", &[file("/a")]),
            [["prog", "--file=/a"]]
        );
    }

    /// `%i` is two arguments or none; `%c` and `%k` are text; `%%` is a `%`;
    /// the deprecated codes vanish.
    #[test]
    fn the_other_codes_expand_as_the_specification_says() {
        assert_eq!(lines("prog %i", &[]), [["prog", "--icon", "calc"]]);
        assert_eq!(
            lines("prog --name=%c %k 100%%", &[]),
            [[
                "prog",
                "--name=Calculator",
                "/usr/share/applications/calc.desktop",
                "100%"
            ]]
        );
        assert_eq!(lines("prog %d %D %n %N %v %m x", &[]), [["prog", "x"]]);
        let no_icon = Invocation {
            icon: None,
            name: "n",
            location: None,
        };
        assert_eq!(
            Exec::parse("prog %i")
                .expect("parses")
                .command_lines(&[], &no_icon),
            [[OsString::from("prog")]]
        );
    }

    /// Inside quotes a `%` is text, as it was written.
    #[test]
    fn a_code_inside_quotes_is_text() {
        assert_eq!(lines(r#"prog "%f""#, &[file("/a")]), [["prog", "%f"]]);
        assert_eq!(lines("prog '%f'", &[file("/a")]), [["prog", "%f"]]);
    }

    /// **Lines written for GLib-based desktops read as GLib reads them**:
    /// single quotes hold their contents literally, and an unquoted
    /// backslash makes the next character ordinary.
    #[test]
    fn single_quotes_and_backslashes_read_as_glib_reads_them() {
        assert_eq!(
            lines("sh -c 'echo \"hi\" $HOME'", &[]),
            [["sh", "-c", "echo \"hi\" $HOME"]]
        );
        assert_eq!(
            lines("/opt/My\\ App/run --x", &[]),
            [["/opt/My App/run", "--x"]]
        );
        assert_eq!(lines("prog \\\"quoted\\\"", &[]), [["prog", "\"quoted\""]]);
        assert_eq!(lines("prog trailing\\", &[]), [["prog", "trailing\\"]]);
        assert_eq!(Exec::parse("prog 'open"), Err(ExecError::UnterminatedQuote));
        assert_eq!(lines("prog ''", &[]), [["prog", ""]]);
    }

    /// **A file name that is not UTF-8 arrives byte for byte.**
    #[cfg(unix)]
    #[test]
    fn a_file_name_that_is_not_utf8_arrives_unchanged() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"/caf\xe9.txt");
        let line = Exec::parse("prog %f")
            .expect("parses")
            .command_lines(&[Target::File(PathBuf::from(name))], &Invocation::default());
        assert_eq!(line, [[OsString::from("prog"), name.to_owned()]]);
    }

    /// **Lines that cannot be started are refused when read**, each for its
    /// own reason.
    #[test]
    fn lines_that_cannot_be_started_are_refused() {
        let err = |line: &str| Exec::parse(line).expect_err("refused");
        assert_eq!(err(""), ExecError::Empty);
        assert_eq!(err("   "), ExecError::Empty);
        assert_eq!(err("%d"), ExecError::Empty);
        assert_eq!(err(r#"prog "open"#), ExecError::UnterminatedQuote);
        assert_eq!(err(r#"prog "open\"#), ExecError::UnterminatedQuote);
        assert_eq!(err("prog %z"), ExecError::UnknownCode('z'));
        assert_eq!(err("prog %"), ExecError::LoneCode);
        assert_eq!(err("prog --files=%F"), ExecError::ListCodeNotAlone('F'));
        assert_eq!(err("prog x%U"), ExecError::ListCodeNotAlone('U'));
        assert_eq!(err("prog %f %U"), ExecError::SeveralFileCodes);
        assert_eq!(err("%k"), ExecError::CodeInProgram);
    }

    /// The program is the first argument, as written.
    #[test]
    fn the_program_is_the_first_argument() {
        assert_eq!(
            Exec::parse("calculator %U").expect("parses").program(),
            "calculator"
        );
        assert_eq!(
            Exec::parse(r#""/opt/My Apps/run" --x"#)
                .expect("parses")
                .program(),
            "/opt/My Apps/run"
        );
        assert!(Exec::parse("prog %U").expect("parses").takes_targets());
        assert!(!Exec::parse("prog --x").expect("parses").takes_targets());
    }
}
