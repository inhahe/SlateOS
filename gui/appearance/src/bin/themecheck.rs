//! `themecheck` -- check a theme's folder before it is installed or shared.
//!
//! ```text
//! themecheck [--strict] [--quiet] FOLDER...
//! ```
//!
//! For each folder, prints what [`appearance::themecheck::check`] found --
//! the errors, then the warnings, then the notes, a line each -- and a line
//! saying what the theme covers. Exits 0 when every folder passes, 1 when one
//! does not, and 2 for a command line it does not understand. `--strict`
//! counts a warning against a folder as well, as a theme repository's CI
//! would (`roadmap-detailed.md` §4.6).
//!
//! The folders are taken as given, every byte of them (`args_os`): a theme's
//! folder may be named with any bytes a name may hold.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use appearance::themecheck::{self, Report, Severity};

const USAGE: &str = "\
usage: themecheck [--strict] [--quiet] FOLDER...

Check each theme folder as the desktop reads it: what in it would be
refused, ignored or adjusted, and what it covers.

  --strict     fail on a warning as well as an error, as a theme repository does
  --quiet      leave out the notes
  -h, --help   show this and stop

Exit status: 0 when every folder passes, 1 when one does not, 2 for a
command line this does not understand.
";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    strict: bool,
    quiet: bool,
    folders: Vec<PathBuf>,
}

/// The command line after the program's name: what it asks for, `None` for
/// a request for the usage, or why it cannot be understood.
fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut options = Options {
        strict: false,
        quiet: false,
        folders: Vec::new(),
    };
    let mut only_folders = false;
    for arg in args {
        if only_folders {
            options.folders.push(PathBuf::from(arg));
        } else if arg == "--" {
            only_folders = true;
        } else if arg == "--strict" {
            options.strict = true;
        } else if arg == "--quiet" {
            options.quiet = true;
        } else if arg == "--help" || arg == "-h" {
            return Ok(None);
        } else if arg.len() > 1 && arg.as_encoded_bytes().first() == Some(&b'-') {
            return Err(format!(
                "`{}` is not an option this understands",
                pathcodec::display_os(&arg)
            ));
        } else {
            options.folders.push(PathBuf::from(arg));
        }
    }
    if options.folders.is_empty() {
        return Err("no theme folder was named".to_owned());
    }
    Ok(Some(options))
}

fn main() -> ExitCode {
    let options = match parse(std::env::args_os().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprint!("themecheck: {why}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut out = io::stdout().lock();
    let mut failed = false;
    for folder in &options.folders {
        let report = themecheck::check(folder);
        if let Err(err) = show(&mut out, folder, &report, options.quiet) {
            // Nowhere left to say anything -- a closed pipe -- so stop, and
            // say on the other stream why the rest was not said.
            eprintln!("themecheck: {err}");
            return ExitCode::from(1);
        }
        failed |= fails(&report, options.strict);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Whether `report` counts against its folder: an error always does, a
/// warning when `strict`.
fn fails(report: &Report, strict: bool) -> bool {
    !report.passes() || (strict && report.count(Severity::Warning) > 0)
}

/// `report` on `folder`, as the program prints it.
fn show(out: &mut impl Write, folder: &Path, report: &Report, quiet: bool) -> io::Result<()> {
    writeln!(out, "{}", pathcodec::display_path(folder))?;
    for finding in &report.findings {
        if quiet && finding.severity == Severity::Note {
            continue;
        }
        writeln!(out, "  {finding}")?;
    }
    let covers = if report.covers.is_empty() {
        "nothing".to_owned()
    } else {
        report.covers.join(", ")
    };
    writeln!(
        out,
        "  = {}, {}, {} -- covers {covers} -- {}, {} bytes",
        counted(report.count(Severity::Error), "error"),
        counted(report.count(Severity::Warning), "warning"),
        counted(report.count(Severity::Note), "note"),
        counted(report.files, "file"),
        report.bytes
    )
}

/// `1 error`, `2 errors`.
fn counted(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_options_and_the_folders_are_read() {
        let options = parse(args(&["--strict", "a", "--quiet", "b"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            options,
            Options {
                strict: true,
                quiet: true,
                folders: vec![PathBuf::from("a"), PathBuf::from("b")],
            }
        );
        let plain = parse(args(&["a"])).unwrap().unwrap();
        assert!(!plain.strict && !plain.quiet);
    }

    /// After `--`, a folder may be called anything, `--strict` included; and
    /// a lone `-` is a folder's name, not an option.
    #[test]
    fn after_two_dashes_everything_is_a_folder() {
        let options = parse(args(&["--", "--strict", "-"])).unwrap().unwrap();
        assert!(!options.strict);
        assert_eq!(
            options.folders,
            [PathBuf::from("--strict"), PathBuf::from("-")]
        );
        let dash = parse(args(&["-"])).unwrap().unwrap();
        assert_eq!(dash.folders, [PathBuf::from("-")]);
    }

    #[test]
    fn help_is_asked_for_by_either_spelling() {
        assert_eq!(parse(args(&["--help"])), Ok(None));
        assert_eq!(parse(args(&["a", "-h"])), Ok(None));
    }

    #[test]
    fn what_is_not_understood_is_said() {
        let unknown = parse(args(&["--loud", "a"])).unwrap_err();
        assert!(unknown.contains("`--loud`"), "{unknown}");
        let none = parse(args(&["--strict"])).unwrap_err();
        assert!(none.contains("no theme folder"), "{none}");
    }

    fn finding(severity: Severity) -> themecheck::Finding {
        themecheck::Finding {
            severity,
            place: String::new(),
            message: String::new(),
        }
    }

    /// An error fails a folder always; a warning only under `--strict`; a
    /// note never.
    #[test]
    fn an_error_fails_and_a_warning_fails_when_strict() {
        let with = |severities: &[Severity]| Report {
            findings: severities.iter().copied().map(finding).collect(),
            ..Report::default()
        };
        for strict in [false, true] {
            assert!(fails(&with(&[Severity::Error]), strict));
            assert!(!fails(&with(&[Severity::Note]), strict));
            assert!(!fails(&with(&[]), strict));
        }
        assert!(!fails(&with(&[Severity::Warning]), false));
        assert!(fails(&with(&[Severity::Warning]), true));
    }

    #[test]
    fn a_report_is_shown_a_line_a_finding_and_a_summary() {
        let report = Report {
            findings: vec![
                themecheck::Finding {
                    severity: Severity::Error,
                    place: "tool.sh".to_owned(),
                    message: "is a script".to_owned(),
                },
                themecheck::Finding {
                    severity: Severity::Note,
                    place: String::new(),
                    message: "a note".to_owned(),
                },
            ],
            covers: vec!["colors"],
            files: 1,
            bytes: 10,
        };
        let mut out = Vec::new();
        show(&mut out, Path::new("t"), &report, false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "t\n  error: tool.sh: is a script\n  note: a note\n  = 1 error, 0 warnings, 1 note -- covers colors -- 1 file, 10 bytes\n"
        );
        let mut quiet = Vec::new();
        show(&mut quiet, Path::new("t"), &report, true).unwrap();
        assert!(!String::from_utf8(quiet).unwrap().contains("a note"));
    }
}
