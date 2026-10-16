//! `iconv`'s unit tests: the command line and the texts argp prints. The
//! conversions themselves are the C library's, and are measured with the
//! program around them by `scripts/iconv-diff.sh`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

fn words(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn parsed(parts: &[&str]) -> Result<Config, Stop> {
    let mut names = Names::from_argv0(b"iconv");
    parse(&words(parts), &mut names)
}

/// What Ubuntu 24.04's `iconv --help` prints, less its last paragraph.
const UBUNTU_HELP: &str = "Usage: iconv [OPTION...] [FILE...]
Convert encoding of given files from one encoding to another.

 Input/Output format specification:
  -f, --from-code=NAME       encoding of original text
  -t, --to-code=NAME         encoding for output

 Information:
  -l, --list                 list all known coded character sets

 Output control:
  -c                         omit invalid characters from output
  -o, --output=FILE          output file
  -s, --silent               suppress warnings
      --verbose              print progress information

  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.
";

/// What Ubuntu 24.04's `iconv --usage` prints.
const UBUNTU_USAGE: &str = "Usage: iconv [-lcs?V] [-f NAME] [-t NAME] [-o FILE] [--from-code=NAME]
            [--to-code=NAME] [--list] [--output=FILE] [--silent] [--verbose]
            [--help] [--usage] [--version] [FILE...]
";

#[test]
fn help_and_usage_are_ubuntus() {
    assert_eq!(help_text(b"iconv"), UBUNTU_HELP.as_bytes());
    assert_eq!(usage_text(b"iconv"), UBUNTU_USAGE.as_bytes());
    assert_eq!(
        parsed(&["--help"]),
        Err(Stop::Print(UBUNTU_HELP.as_bytes().to_vec()))
    );
    assert_eq!(
        parsed(&["-?"]),
        Err(Stop::Print(UBUNTU_HELP.as_bytes().to_vec()))
    );
    assert_eq!(
        parsed(&["--usage"]),
        Err(Stop::Print(UBUNTU_USAGE.as_bytes().to_vec()))
    );
    assert!(matches!(parsed(&["-V"]), Err(Stop::Print(v)) if v.starts_with(b"iconv (SlateOS")));
}

/// The usage error `text` would end the run with.
fn usage(text: &str) -> Result<Config, Stop> {
    Err(Stop::Usage(text.as_bytes().to_vec()))
}

#[test]
fn a_getopt_error_takes_argps_referral_and_status_64() {
    let referral = "Try `iconv --help' or `iconv --usage' for more information.";
    assert_eq!(
        parsed(&["-x"]),
        usage(&format!("iconv: invalid option -- 'x'\n{referral}"))
    );
    assert_eq!(
        parsed(&["--bogus"]),
        usage(&format!("iconv: unrecognized option '--bogus'\n{referral}"))
    );
    assert_eq!(
        parsed(&["-f"]),
        usage(&format!(
            "iconv: option requires an argument -- 'f'\n{referral}"
        ))
    );
    // argp's table order decides the possibilities' order.
    assert_eq!(
        parsed(&["--ver"]),
        usage(&format!(
            "iconv: option '--ver' is ambiguous; possibilities: '--verbose' '--version'\n{referral}"
        ))
    );
    assert_eq!(EX_USAGE, 64);
}

#[test]
fn options_are_taken_in_order_and_operands_kept() {
    let cfg = parsed(&[
        "-f",
        "UTF-8",
        "a",
        "--to-code=ASCII",
        "-c",
        "-o",
        "out",
        "b",
    ])
    .unwrap();
    assert_eq!(cfg.from_code, b"UTF-8");
    assert_eq!(cfg.to_code, b"ASCII");
    assert!(cfg.omit_invalid);
    assert_eq!(cfg.output_file.as_deref(), Some(&b"out"[..]));
    assert_eq!(cfg.files, words(&["a", "b"]));
    // `-s` is taken and does nothing.
    assert_eq!(parsed(&["-s"]).unwrap(), Config::default());
    // Help after a bad option is never reached; before one, it is.
    assert!(matches!(parsed(&["-x", "--help"]), Err(Stop::Usage(_))));
    assert!(matches!(parsed(&["--help", "-x"]), Err(Stop::Print(_))));
}

#[test]
fn program_name_renames_what_the_messages_say() {
    let mut names = Names::from_argv0(b"/usr/bin/iconv");
    assert_eq!(names.short, b"iconv");
    let got = parse(&words(&["--program-name=/x/conv", "-x"]), &mut names);
    // getopt's own sentence keeps `argv[0]`; argp's referral and `error()`
    // take the new name.
    assert_eq!(
        got,
        usage(
            "/usr/bin/iconv: invalid option -- 'x'\n\
             Try `conv --help' or `conv --usage' for more information."
        )
    );
    assert_eq!(names.full, b"/x/conv");
    // A name that is not text is printed as the bytes it is.
    let mut names = Names::from_argv0(b"/bin/\xffconv");
    let got = parse(&words(&["-x"]), &mut names);
    let Err(Stop::Usage(text)) = got else {
        panic!("expected a usage error, got {got:?}");
    };
    assert!(
        text.starts_with(b"/bin/\xffconv: invalid option"),
        "{text:?}"
    );
    assert!(text.ends_with(b"Try `\xffconv --help' or `\xffconv --usage' for more information."));
    assert!(help_text(b"\xffconv").starts_with(b"Usage: \xffconv [OPTION...]"));
}

#[test]
fn atoi_reads_as_c_reads() {
    assert_eq!(atoi(b"3"), 3);
    assert_eq!(atoi(b"  -2x"), -2);
    assert_eq!(atoi(b"x"), 0);
    assert_eq!(atoi(b""), 0);
    assert_eq!(atoi(b"+7"), 7);
}

#[test]
fn the_name_list_is_one_a_line_or_a_paragraph_on_a_terminal() {
    let names = ["437//", "500//", "ISO-10646/UCS2/", "UTF-8//"];
    assert_eq!(
        names_text(&names, false),
        "437//\n500//\nISO-10646/UCS2/\nUTF-8//\n"
    );
    let human = names_text(&names, true);
    assert!(human.starts_with("The following list contains"));
    assert!(
        human.ends_with("  437, 500, ISO-10646/UCS2, UTF-8\n"),
        "{human}"
    );
    // A line is filled to 77 columns and continued two spaces in.
    let many: Vec<String> = (0..40).map(|i| format!("NAME-{i:03}//")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let text = names_text(&many, true);
    for line in text.lines().skip(5) {
        assert!(line.len() <= 78, "{line:?}");
        assert!(line.starts_with("  "), "{line:?}");
    }
}

#[test]
fn the_embedded_list_is_glibcs() {
    let lines: Vec<&str> = GLIBC_NAMES.lines().collect();
    assert_eq!(lines.len(), 1180);
    assert_eq!(lines.first(), Some(&"437//"));
    assert!(lines.contains(&"UTF-8//"));
    assert!(lines.contains(&"ISO-10646/UCS2/"));
}
