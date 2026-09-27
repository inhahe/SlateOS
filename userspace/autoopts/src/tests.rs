//! Unit tests: the parser against hand-made descriptor tables. What the
//! programs print (usage, version, messages) is compared against the real
//! sharutils by `scripts/uu-diff.sh`; these pin the mechanics.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::Path;

use scratchdir::ScratchDir;

use crate::charmap::{self, strneqvcmp, strtoul};
use crate::cook::string_cook;
use crate::pathfind::canonicalize_pathname;
#[cfg(unix)]
use crate::pathfind::pathfind;
use crate::{Action, Callbacks, Desc, Exit, NOLIMIT, Options, Program, Role, pr, st};

const OUTPUT: usize = 0;
const CHMOD: usize = 1;
const LEVEL: usize = 2;
const SAVE: usize = 6;
const LOAD: usize = 7;

static DESCS: [Desc; 8] = [
    Desc {
        value: b'o',
        name: "output-file",
        disable_name: None,
        text: "direct output to file",
        flags: st::DISABLED | st::ARG_STRING,
        min: 0,
        max: 1,
        role: Role::User,
        action: Action::User,
    },
    Desc {
        value: b'c',
        name: "ignore-chmod",
        disable_name: None,
        text: "ignore fchmod(3P) errors",
        flags: st::DISABLED,
        min: 0,
        max: 1,
        role: Role::User,
        action: Action::None,
    },
    Desc {
        value: b'l',
        name: "level",
        disable_name: None,
        text: "an optional argument",
        flags: st::DISABLED | st::ARG_STRING | st::ARG_OPTIONAL,
        min: 0,
        max: 3,
        role: Role::User,
        action: Action::None,
    },
    Desc {
        value: b'v',
        name: "version",
        disable_name: None,
        text: "output version information and exit",
        flags: st::ARG_STRING | st::ARG_OPTIONAL | st::IMM | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::Version,
        action: Action::PrintVersion,
    },
    Desc {
        value: b'h',
        name: "help",
        disable_name: None,
        text: "display extended usage information and exit",
        flags: st::IMM | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::Help,
        action: Action::Usage,
    },
    Desc {
        value: b'!',
        name: "more-help",
        disable_name: None,
        text: "extended usage information passed thru pager",
        flags: st::IMM | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::MoreHelp,
        action: Action::PagedUsage,
    },
    Desc {
        value: b'R',
        name: "save-opts",
        disable_name: None,
        text: "save the option state to a config file",
        flags: st::ARG_STRING | st::ARG_OPTIONAL | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::SaveOpts,
        action: Action::None,
    },
    Desc {
        value: b'r',
        name: "load-opts",
        disable_name: Some("no-load-opts"),
        text: "load options from a config file",
        flags: st::ARG_STRING | st::DISABLE_IMM,
        min: 0,
        max: NOLIMIT,
        role: Role::LoadOpts,
        action: Action::LoadOpt,
    },
];

/// A test program whose rc file lives under `home`, if given.
fn program(home: Option<&Path>) -> &'static Program {
    let home_list: &'static [&'static str] = match home {
        Some(p) => {
            let s: &'static str = Box::leak(p.to_string_lossy().into_owned().into_boxed_str());
            Box::leak(vec![s].into_boxed_slice())
        }
        None => &[],
    };
    Box::leak(Box::new(Program {
        name: "testprog",
        upper: "TESTPROG",
        rc_name: ".testrc",
        copyright: "testprog 1.0\n",
        copy_notice: "notice\n",
        full_version: "testprog 1.0",
        home_list,
        usage_title: "testprog - a test\nUsage:  %s [ -<flag> ]...\n",
        explain: None,
        detail: None,
        bug_addr: "bugs@example.org",
        descs: &DESCS,
        preset_ct: 3,
        save_opts: SAVE,
        full_usage: "full usage\n",
        short_usage: "short usage\n",
        proc_flags: pr::ERRSTOP
            | pr::SHORTOPT
            | pr::LONGOPT
            | pr::NO_REQ_OPT
            | pr::GNUUSAGE
            | pr::MISUSE,
        usage_error: 64,
    }))
}

/// Every call of the program's own procedure, with the argument it saw.
#[derive(Default)]
struct Record(Vec<(usize, Option<Vec<u8>>)>);

impl Callbacks for Record {
    fn option(&mut self, opts: &Options, index: usize) -> Result<(), Exit> {
        self.0.push((index, opts.arg(index).map(<[u8]>::to_vec)));
        Ok(())
    }
}

fn argv(words: &[&str]) -> Vec<Vec<u8>> {
    words.iter().map(|w| w.as_bytes().to_vec()).collect()
}

/// Run the parser; the options and what it returned.
fn run(home: Option<&Path>, words: &[&str]) -> (Options, Record, Result<usize, Exit>) {
    let mut opts = Options::new(program(home), argv(words));
    let mut rec = Record::default();
    let r = opts.process(&mut rec);
    (opts, rec, r)
}

/// A home directory holding `rc` as its rc file.
fn home_with(rc: &[u8]) -> ScratchDir {
    let dir = ScratchDir::new("autoopts-home");
    fs::write(dir.path(".testrc"), rc).unwrap();
    dir
}

// ------------------------------------------------------------ command line

#[test]
fn a_flag_sets_its_option_and_processing_stops_at_the_operand() {
    let (o, _, r) = run(None, &["t", "-c", "file", "-o", "x"]);
    assert_eq!(r, Ok(2));
    assert!(o.have(CHMOD));
    assert!(!o.have(OUTPUT));
}

#[test]
fn long_names_abbreviate_ignore_case_and_equate_separators() {
    let (o, rec, r) = run(None, &["t", "--IGN", "--output_FILE=o"]);
    assert_eq!(r, Ok(3));
    assert!(o.have(CHMOD));
    assert_eq!(o.arg(OUTPUT), Some(&b"o"[..]));
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"o".to_vec()))]);
}

#[test]
fn flags_bundle_and_a_required_argument_takes_the_next_word() {
    let (o, _, r) = run(None, &["t", "-co", "-x"]);
    assert_eq!(r, Ok(3));
    assert!(o.have(CHMOD));
    assert_eq!(o.arg(OUTPUT), Some(&b"-x"[..]));
}

#[test]
fn a_required_argument_may_be_glued_to_its_flag() {
    let (o, _, r) = run(None, &["t", "-oout"]);
    assert_eq!(r, Ok(2));
    assert_eq!(o.arg(OUTPUT), Some(&b"out"[..]));
}

#[test]
fn an_optional_argument_takes_the_next_word_unless_it_starts_with_a_dash() {
    let (o, _, r) = run(None, &["t", "-l", "word", "rest"]);
    assert_eq!(r, Ok(3));
    assert_eq!(o.arg(LEVEL), Some(&b"word"[..]));

    let (o, _, r) = run(None, &["t", "--level", "-c"]);
    assert_eq!(r, Ok(3));
    assert_eq!(o.arg(LEVEL), None);
    assert!(o.have(CHMOD));

    let (o, _, r) = run(None, &["t", "-lx", "--level="]);
    assert_eq!(r, Ok(3));
    // The second occurrence's empty argument replaced the first's.
    assert_eq!(o.arg(LEVEL), Some(&b""[..]));
    assert_eq!(o.count(LEVEL), 2);
}

#[test]
fn double_dash_ends_the_options_and_is_consumed() {
    let (o, _, r) = run(None, &["t", "--", "-c"]);
    assert_eq!(r, Ok(2));
    assert!(!o.have(CHMOD));
}

#[test]
fn a_lone_dash_is_an_operand() {
    let (o, _, r) = run(None, &["t", "-", "-c"]);
    assert_eq!(r, Ok(1));
    assert!(!o.have(CHMOD));
}

#[test]
fn no_operands_returns_the_argument_count() {
    let (_, _, r) = run(None, &["t", "-c"]);
    assert_eq!(r, Ok(2));
}

#[test]
fn usage_errors_exit_1() {
    for words in [
        &["t", "--bogus"][..],
        &["t", "-z"],
        &["t", "-o"],
        &["t", "--output-file"],
        &["t", "-c", "-c"],
        &["t", "--ignore-chmod=x"],
        &["t", "--i"],
        &["t", "--=x"],
        &["t", "--no-load-opts=x"],
    ] {
        let (_, _, r) = run(None, words);
        assert_eq!(r, Err(Exit(1)), "{words:?}");
    }
}

#[test]
fn a_syntax_error_anywhere_stops_before_any_procedure_runs() {
    // The immediate pass finds the error before the regular pass would run
    // `-o`'s procedure.
    let (_, rec, r) = run(None, &["t", "-o", "out", "--bogus"]);
    assert_eq!(r, Err(Exit(1)));
    assert!(rec.0.is_empty());
}

#[test]
fn a_name_of_128_bytes_is_not_a_name() {
    let long = format!("--{}", "i".repeat(128));
    let (_, _, r) = run(None, &["t", &long]);
    assert_eq!(r, Err(Exit(1)));
}

// ------------------------------------------------------------------ rc files

#[test]
fn the_rc_file_presets_options() {
    let home = home_with(b"ignore-chmod\noutput-file out\n");
    let (o, rec, r) = run(Some(home.dir()), &["t"]);
    assert_eq!(r, Ok(1));
    assert!(o.have(CHMOD));
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"out".to_vec()))]);
    // Preset, not defined: a command-line `-c` is still its first.
    let (o, _, r) = run(Some(home.dir()), &["t", "-c"]);
    assert_eq!(r, Ok(2));
    assert_eq!(o.count(CHMOD), 1);
}

#[test]
fn a_home_entry_naming_a_file_is_read_as_the_rc_file() {
    let home = home_with(b"ignore-chmod\n");
    let (o, _, _) = run(Some(&home.path(".testrc")), &["t"]);
    assert!(o.have(CHMOD));
}

#[test]
fn no_load_opts_skips_the_rc_file() {
    let home = home_with(b"ignore-chmod\n");
    let (o, _, r) = run(Some(home.dir()), &["t", "--no-load-opts"]);
    assert_eq!(r, Ok(2));
    assert!(!o.have(CHMOD));
    assert!(!o.enabled(LOAD));
}

#[test]
fn the_rc_file_s_last_line_needs_its_newline() {
    let home = home_with(b"ignore-chmod");
    let (o, _, _) = run(Some(home.dir()), &["t"]);
    assert!(!o.have(CHMOD));
}

#[test]
fn a_trailing_backslash_is_dropped_not_a_continuation() {
    let home = home_with(b"output-file = a\\\nignore-chmod\n");
    let (o, rec, _) = run(Some(home.dir()), &["t"]);
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"a".to_vec()))]);
    // The next line was an entry of its own.
    assert!(o.have(CHMOD));
}

#[test]
fn separators_between_name_and_value() {
    for rc in [
        &b"output-file:v\n"[..],
        b"output-file=v\n",
        b"output-file v\n",
        b"output-file : v\n",
        b"  output-file\tv\n",
    ] {
        let home = home_with(rc);
        let (_, rec, _) = run(Some(home.dir()), &["t"]);
        assert_eq!(rec.0, vec![(OUTPUT, Some(b"v".to_vec()))], "{rc:?}");
    }
}

#[test]
fn a_quoted_value_is_cooked() {
    let home = home_with(b"output-file \"a\\tb\" 'c'\n");
    let (_, rec, _) = run(Some(home.dir()), &["t"]);
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"a\tbc".to_vec()))]);
}

#[test]
fn an_xml_value_loses_its_first_byte() {
    let home = home_with(b"<output-file>Xfoo</output-file>\n");
    let (_, rec, _) = run(Some(home.dir()), &["t"]);
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"foo".to_vec()))]);

    let home = home_with(b"<output-file>\nbar\n</output-file>\n");
    let (_, rec, _) = run(Some(home.dir()), &["t"]);
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"bar\n".to_vec()))]);
}

#[test]
fn xml_forms_self_closing_and_cooked() {
    let home = home_with(b"<ignore-chmod/>\n<output-file cooked>\na&amp;b%41</output-file>\n");
    let (o, rec, _) = run(Some(home.dir()), &["t"]);
    assert!(o.have(CHMOD));
    assert_eq!(rec.0, vec![(OUTPUT, Some(b"a&bA".to_vec()))]);
}

#[test]
fn options_that_may_not_be_preset_are_ignored_in_the_rc_file() {
    let home = home_with(b"help\nversion\nsave-opts x\nignore-chmod\n");
    let (o, _, r) = run(Some(home.dir()), &["t"]);
    assert_eq!(r, Ok(1));
    assert!(o.have(CHMOD));
    assert!(!o.have(SAVE));
}

#[test]
fn another_program_s_section_is_skipped() {
    let home = home_with(b"[OTHER]\nlevel\n[TESTPROG]\nignore-chmod\n");
    let (o, _, _) = run(Some(home.dir()), &["t"]);
    assert!(o.have(CHMOD));
    assert!(!o.have(LEVEL));
}

#[test]
fn only_the_first_program_directive_is_compared() {
    let home = home_with(b"<?program other>\nlevel\n<?program t>\nignore-chmod\n");
    let (o, _, _) = run(Some(home.dir()), &["t"]);
    assert!(!o.have(CHMOD));
    assert!(!o.have(LEVEL));

    let home = home_with(b"<?program t>\nignore-chmod\n");
    let (o, _, _) = run(Some(home.dir()), &["t"]);
    assert!(o.have(CHMOD));
}

#[test]
fn comments_and_unknown_entries_are_skipped() {
    let home = home_with(b"# a comment\n<!-- xml comment -->\nbogus-name\nignore-chmod\n");
    let (o, _, _) = run(Some(home.dir()), &["t"]);
    assert!(o.have(CHMOD));
}

#[test]
fn a_file_loaded_from_the_rc_file_counts_as_the_command_line() {
    let home = ScratchDir::new("autoopts-nested");
    let inner = home.path("inner");
    fs::write(&inner, b"ignore-chmod\n").unwrap();
    let mut rc = b"load-opts ".to_vec();
    rc.extend_from_slice(inner.to_string_lossy().as_bytes());
    rc.push(b'\n');
    fs::write(home.path(".testrc"), &rc).unwrap();
    let (o, _, r) = run(Some(home.dir()), &["t"]);
    assert_eq!(r, Ok(1));
    assert_eq!(o.count(CHMOD), 1);
    // So a `-c` on the command line is one too many.
    let (_, _, r) = run(Some(home.dir()), &["t", "-c"]);
    assert_eq!(r, Err(Exit(1)));
}

#[test]
fn load_opts_reads_a_file_and_refuses_a_missing_one() {
    let dir = ScratchDir::new("autoopts-load");
    let f = dir.path("opts");
    fs::write(&f, b"ignore-chmod\n").unwrap();
    let name = f.to_string_lossy().into_owned();
    let (o, _, r) = run(None, &["t", "--load-opts", &name, "op"]);
    assert_eq!(r, Ok(3));
    assert!(o.have(CHMOD));

    let (_, _, r) = run(None, &["t", "-r", "/nonexistent/autoopts-test"]);
    assert_eq!(r, Err(Exit(1)));
    let dir_name = dir.dir().to_string_lossy().into_owned();
    let (_, _, r) = run(None, &["t", "-r", &dir_name]);
    assert_eq!(r, Err(Exit(1)));
}

// ------------------------------------------------------------------ save

#[test]
fn save_opts_writes_the_program_s_options_and_exits_0() {
    let dir = ScratchDir::new("autoopts-save");
    let out = dir.path("saved");
    let name = out.to_string_lossy().into_owned();
    let (_, _, r) = run(None, &["t", "-c", "-o", "a\nb", "-R", &name]);
    assert_eq!(r, Err(Exit(0)));
    let text = fs::read(&out).unwrap();
    let lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    assert_eq!(lines[0], b"#  testprog - a test");
    assert_eq!(lines[1], b"#  preset/initialization file");
    assert!(lines[2].starts_with(b"#  "));
    assert_eq!(lines[3], b"#");
    assert_eq!(lines[4], b"output-file =       a\\");
    assert_eq!(lines[5], b"b");
    assert_eq!(lines[6], b"ignore-chmod");
    assert_eq!(lines[7], b"");
    assert_eq!(lines.len(), 8);
}

#[test]
fn save_opts_into_a_directory_uses_the_rc_name() {
    let dir = ScratchDir::new("autoopts-save-dir");
    let name = dir.dir().to_string_lossy().into_owned();
    let (_, _, r) = run(None, &["t", "-l", "-R", &name]);
    assert_eq!(r, Err(Exit(0)));
    let text = fs::read(dir.path(".testrc")).unwrap();
    assert!(text.ends_with(b"#\nlevel\n"), "{text:?}");
}

// ------------------------------------------------------------------ pieces

#[test]
fn names_compare_without_case_and_with_separators_equal() {
    assert_eq!(strneqvcmp(b"Load_Opts", b"load-opts", 9), 0);
    assert_eq!(strneqvcmp(b"load^opts", b"load-opts", 9), 0);
    assert_ne!(strneqvcmp(b"loa", b"load-opts", 4), 0);
    assert_eq!(strneqvcmp(b"lo", b"load-opts", 2), 0);
}

#[test]
fn character_classes() {
    assert!(charmap::is(0x08, charmap::WHITESPACE));
    assert!(charmap::is(b':', charmap::VALUE_NAME));
    assert!(!charmap::is(b':', charmap::OPTION_NAME));
    assert!(charmap::is(0, charmap::END_LIST_ENTRY));
    assert!(!charmap::is(0xe9, charmap::GRAPHIC));
    assert_eq!(charmap::spn(b"  \x08x", 0, charmap::WHITESPACE), 3);
    assert_eq!(charmap::spn(b"ab\0cd", 0, charmap::VALUE_NAME), 2);
}

#[test]
fn strtoul_as_glibc() {
    assert_eq!(strtoul(b"0x1F;", 0, 16), (0x1f, 4));
    assert_eq!(strtoul(b"0x;", 0, 16), (0, 1));
    assert_eq!(strtoul(b" 12;", 0, 10), (12, 3));
    assert_eq!(strtoul(b";", 0, 10), (0, 0));
    assert_eq!(strtoul(b"-1", 0, 10), (u64::MAX, 2));
}

fn cooked(s: &[u8]) -> (Vec<u8>, Option<usize>) {
    let mut buf = s.to_vec();
    buf.extend_from_slice(&[0, 0]);
    let stop = string_cook(&mut buf, 0);
    (charmap::cstr(&buf, 0), stop)
}

#[test]
fn quoted_strings_cook() {
    assert_eq!(cooked(b"\"a\\tb\\x41\\101\\q\"").0, b"a\tbAAq");
    assert_eq!(cooked(b"'a\\tb\\'c'").0, b"a\\tb'c");
    assert_eq!(cooked(b"\"a\" \"b\" 'c' x").0, b"abc");
    assert_eq!(cooked(b"\"a\" /* c */ \"b\"").0, b"ab");
    assert_eq!(cooked(b"\"unterminated").0, b"unterminated");
    assert_eq!(cooked(b"\"unterminated").1, None);
    assert_eq!(cooked(b"\"a\\\nb\"").0, b"ab");
    assert_eq!(cooked(b"\"\\777\"").0, [0xff]);
}

#[test]
fn canonicalize_pathname_as_pathfind_c() {
    assert_eq!(canonicalize_pathname(b"/usr/bin/x"), b"/usr/bin/x");
    assert_eq!(canonicalize_pathname(b"/usr//bin/./x"), b"/usr/bin/x");
    assert_eq!(canonicalize_pathname(b"/usr/lib/../bin/x"), b"/usr/bin/x");
    assert_eq!(canonicalize_pathname(b"./x"), b"./x");
    // Upstream's leading `..` leaves the slash that followed it.
    assert_eq!(canonicalize_pathname(b"bin/../bin/x"), b"/bin/x");
    assert_eq!(canonicalize_pathname(b"/usr/bin/"), b"/usr/bin");
}

// A Windows path's drive letter has a colon, which a PATH element cannot.
#[cfg(unix)]
#[test]
fn pathfind_walks_path_and_skips_empty_elements() {
    let dir = ScratchDir::new("autoopts-path");
    fs::create_dir(dir.path("a")).unwrap();
    fs::create_dir(dir.path("b")).unwrap();
    fs::write(dir.path("b/prog"), b"").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path("b/prog"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let a = dir.path("a").to_string_lossy().replace('\\', "/");
    let b = dir.path("b").to_string_lossy().replace('\\', "/");
    let path = format!("::{a}::{b}:");
    let found = pathfind(Some(path.as_bytes()), b"prog").unwrap();
    assert_eq!(found, format!("{b}/prog").into_bytes());
    assert_eq!(pathfind(Some(path.as_bytes()), b"b/prog"), None);
    assert_eq!(pathfind(None, b"prog"), None);
}

/// A name in a message is upstream's bytes when they are printable, and
/// escaped where they could end the line or drive the terminal
/// (design-decisions.md §1033).
#[test]
fn a_name_in_a_message_cannot_forge_a_line() {
    use crate::{shown, shown_in_quotes};
    assert_eq!(shown(b"plain name.uu"), b"plain name.uu");
    assert_eq!(shown("caf\u{e9}".as_bytes()), "caf\u{e9}".as_bytes());
    assert_eq!(
        shown(b"x\nuudecode: forged"),
        br"x\012uudecode: forged".to_vec()
    );
    assert_eq!(shown(b"\x1b[31mred"), br"\033[31mred".to_vec());
    assert_eq!(shown(b"\xff"), br"\377".to_vec());
    assert_eq!(shown_in_quotes(b"it's"), b"'it's'");
    assert_eq!(shown_in_quotes(b"a\rb"), br"'a\015b'".to_vec());
}
