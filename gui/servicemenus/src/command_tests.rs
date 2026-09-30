#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

const HOME: &str = "/home/u";

fn context() -> Context<'static> {
    Context {
        name: "Image tools",
        icon: Some("image"),
        location: Some(Path::new("/usr/share/kio/servicemenus/image.desktop")),
        working_dir: None,
        home: Some(Path::new(HOME)),
    }
}

/// The argument vectors `exec` runs for `files`, as text.
fn runs_with(exec: &str, files: &[&str], context: &Context<'_>) -> Vec<Vec<String>> {
    let paths: Vec<&Path> = files.iter().map(Path::new).collect();
    Command::parse(exec)
        .unwrap_or_else(|e| panic!("{exec:?}: {e}"))
        .runs(&paths, context)
        .unwrap()
        .into_iter()
        .map(|run| {
            run.argv
                .into_iter()
                .map(|a| a.into_string().expect("test arguments are UTF-8"))
                .collect()
        })
        .collect()
}

fn runs(exec: &str, files: &[&str]) -> Vec<Vec<String>> {
    runs_with(exec, files, &context())
}

fn shell(line: &str) -> Vec<String> {
    vec![SHELL.to_owned(), String::from("-c"), line.to_owned()]
}

/// **`%f` is one run per file, `%F` one run for all**, a name with a blank in
/// it one argument either way.
#[test]
fn one_run_per_file_or_one_for_all() {
    assert_eq!(
        runs("mogrify -rotate 90 %f", &["/p/a b.png", "/p/c.png"]),
        [
            ["mogrify", "-rotate", "90", "/p/a b.png"],
            ["mogrify", "-rotate", "90", "/p/c.png"]
        ]
    );
    assert_eq!(
        runs("ark --batch %F", &["/p/a b", "/p/c"]),
        [["ark", "--batch", "/p/a b", "/p/c"]]
    );
    assert_eq!(
        runs("tool %U", &["/p/a", "/p/b"]),
        [["tool", "/p/a", "/p/b"]]
    );
}

/// **A code may appear more than once**, the same file each time -- which
/// the specification forbids and KDE's menus rely on.
#[test]
fn a_code_may_appear_twice() {
    assert_eq!(
        runs("convert %f -rotate 90 %f", &["/p/a.png"]),
        [["convert", "/p/a.png", "-rotate", "90", "/p/a.png"]]
    );
}

/// **A code inside double quotes is expanded**, and the name stays one
/// argument whatever it holds.
#[test]
fn a_code_inside_double_quotes_is_the_name() {
    let name = r#"/p/it's "x" $y `z` \w.wav"#;
    assert_eq!(
        runs(r#"ffmpeg -i "%f" "%f.mp3""#, &[name]),
        [vec![
            String::from("ffmpeg"),
            String::from("-i"),
            name.to_owned(),
            format!("{name}.mp3")
        ]]
    );
}

/// **A code inside single quotes is expanded as KDE expands it**: the value
/// goes in as it is, a `'` in it closing, escaping and reopening the quote --
/// so the argument holds the name, for the shell it is handed to.
#[test]
fn a_code_inside_single_quotes_is_the_name() {
    assert_eq!(
        runs("sh -c 'cd %d && make'", &["/p/src/f.c"]),
        [["sh", "-c", "cd /p/src/ && make"]]
    );
    assert_eq!(
        runs("sh -c 'cd %d && make'", &["/p/it's/f.c"]),
        [["sh", "-c", "cd /p/it's/ && make"]]
    );
}

/// **A code inside `$'...'` is expanded**, its `'` and `\` escaped.
#[test]
fn a_code_inside_dollar_quotes_is_the_name() {
    assert_eq!(
        runs(r"printf $'%f'", &[r"/p/it's \n"]),
        [["printf", r"/p/it's \n"]]
    );
}

/// **A line that needs a shell runs in one**, the names in it quoted so the
/// shell reads each as a name and nothing else.
#[test]
fn a_line_that_needs_a_shell_runs_in_one() {
    assert_eq!(
        runs("echo %f | xclip -selection clipboard", &["/p/a b"]),
        [shell("echo '/p/a b' | xclip -selection clipboard")]
    );
    assert_eq!(
        runs("echo %f | xclip", &["/p/$(touch x);'.txt"]),
        [shell(r"echo '/p/$(touch x);'\''.txt' | xclip")]
    );
    // A variable, an assignment before the program, a backquoted command.
    assert_eq!(
        runs("notify $HOME %f", &["/p/a"]),
        [shell("notify $HOME /p/a")]
    );
    assert_eq!(
        runs("LANG=C sort %f", &["/p/a"]),
        [shell("LANG=C sort /p/a")]
    );
    assert_eq!(
        runs("echo `date` %f", &["/p/a"]),
        [shell("echo $( date) /p/a")],
        "a backquoted command is rewritten as KDE rewrites it"
    );
    // Inside a command substitution in double quotes, the name is a word of
    // its own again.
    assert_eq!(
        runs(r#"sh -c "$(echo %f)""#, &["/p/a b"]),
        [shell(r#"sh -c "$(echo '/p/a b')""#)]
    );
    // Inside backquotes, `\$` is a `$` once they are rewritten.
    assert_eq!(
        runs(r"echo `echo \$HOME` %f", &["/p/a"]),
        [shell("echo $( echo $HOME) /p/a")]
    );
    // A `'` inside double quotes is a character.
    assert_eq!(
        runs(r#"echo "it's %f""#, &["/p/a"]),
        [["echo", "it's /p/a"]]
    );
    // Outside double quotes, `\"` in backquotes is kept as written.
    assert_eq!(
        runs(r#"echo `echo \"x\"` %f"#, &["/p/a"]),
        [shell(r#"echo $( echo \"x\") /p/a"#)]
    );
    // Double quotes that expand something need the shell too.
    assert_eq!(
        runs(r#"sh -c "echo $1" x %f"#, &["/p/a"]),
        [shell(r#"sh -c "echo $1" x /p/a"#)]
    );
}

/// **A line naming no file is given the file anyway**, as its last
/// argument, one run each -- and with none, nothing.
#[test]
fn a_line_naming_no_file_is_given_it() {
    assert_eq!(
        runs("gimp --new-instance", &["/p/a", "/p/b"]),
        [
            ["gimp", "--new-instance", "/p/a"],
            ["gimp", "--new-instance", "/p/b"]
        ]
    );
    assert_eq!(
        runs("gimp --new-instance", &[]),
        [["gimp", "--new-instance"]]
    );
    let command = Command::parse("gimp").unwrap();
    assert_eq!(command.as_written(), "gimp");
}

/// **The deprecated codes still mean something**: `%d` the folder with its
/// `/`, `%n` the name, `%D` and `%N` those of every file in one run.
#[test]
fn the_deprecated_codes_are_the_folder_and_the_name() {
    assert_eq!(
        runs("tool %d %n", &["/p/q/a.txt"]),
        [["tool", "/p/q/", "a.txt"]]
    );
    assert_eq!(
        runs("tool %D -- %N", &["/p/a", "/q/b"]),
        [["tool", "/p/", "/q/", "--", "a", "b"]]
    );
    assert_eq!(runs("tool %v", &["/p/a"]), [["tool"]], "%v is nothing");
    // Each of the list codes alone takes every file in one run.
    assert_eq!(runs("tool %N", &["/p/a", "/q/b"]), [["tool", "a", "b"]]);
    assert_eq!(runs("tool %D", &["/p/a", "/q/b"]), [["tool", "/p/", "/q/"]]);
    // One file's code beside a list's, with several files: nothing.
    assert_eq!(
        runs("tool %F -- %f", &["/p/a", "/p/b"]),
        [["tool", "/p/a", "/p/b", "--"]]
    );
    // A list's code with one file is that file, once.
    assert_eq!(
        runs("ark --batch %F", &["/p/a"]),
        [["ark", "--batch", "/p/a"]]
    );
}

/// **`%c`, `%i` and `%k` are the menu's name, icon and file**; `%i` with no
/// icon is nothing, an empty name an empty argument.
#[test]
fn the_menus_own_codes() {
    assert_eq!(
        runs("tool --title %c %i --menu %k %f", &["/p/a"]),
        [[
            "tool",
            "--title",
            "Image tools",
            "--icon",
            "image",
            "--menu",
            "/usr/share/kio/servicemenus/image.desktop",
            "/p/a"
        ]]
    );
    let bare = Context {
        name: "",
        icon: Some(""),
        location: None,
        ..context()
    };
    assert_eq!(
        runs_with("tool %c %i %k %f", &["/p/a"], &bare),
        [["tool", "", "", "/p/a"]]
    );
}

/// **`%%` is a `%`, and a `%` with a letter that is no code is kept.**
#[test]
fn percent_signs() {
    assert_eq!(
        runs("printf 100%% %z %f", &["/p/a"]),
        [["printf", "100%", "%z", "/p/a"]]
    );
    // `%m`, the mini-icon, is dropped.
    assert_eq!(runs("printf %m %f", &["/p/a"]), [["printf", "/p/a"]]);
}

/// **`~` starting a word is home**; `~user` and a quoted `~` are left as
/// they are.
#[test]
fn a_tilde_is_home() {
    assert_eq!(
        runs("~/bin/tool %f", &["/p/a"]),
        [["/home/u/bin/tool", "/p/a"]]
    );
    assert_eq!(runs("tool ~ %f", &["/p/a"]), [["tool", HOME, "/p/a"]]);
    assert_eq!(runs("tool %f ~", &["/p/a"]), [["tool", "/p/a", HOME]]);
    assert_eq!(
        runs("tool ~other/x %f", &["/p/a"]),
        [["tool", "~other/x", "/p/a"]]
    );
    assert_eq!(runs(r#"tool "~" %f"#, &["/p/a"]), [["tool", "~", "/p/a"]]);
    assert_eq!(runs("tool a~b %f", &["/p/a"]), [["tool", "a~b", "/p/a"]]);
    assert_eq!(
        runs(r#"tool ~"x"/y %f"#, &["/p/a"]),
        [["tool", "~x/y", "/p/a"]],
        "a quote in the name makes the ~ a character"
    );
    // Home as the first word, and an assignment-like word after it.
    assert_eq!(
        split(b"~ X=1", Some(Path::new(HOME))),
        Split::Words(vec![HOME.as_bytes().to_vec(), b"X=1".to_vec()])
    );
    // A file named like home is a file.
    assert_eq!(runs("tool %f", &["~"]), [["tool", "~"]]);
    assert_eq!(runs("tool %f", &["~/x"]), [["tool", "~/x"]]);
    let empty_home = Context {
        home: Some(Path::new("")),
        ..context()
    };
    assert_eq!(
        runs_with("tool ~/x %f", &["/p/a"], &empty_home),
        [["tool", "~/x", "/p/a"]]
    );
    let homeless = Context {
        home: None,
        ..context()
    };
    assert_eq!(
        runs_with("tool ~/x %f", &["/p/a"], &homeless),
        [["tool", "~/x", "/p/a"]]
    );
}

/// **`$'...'` is decoded as KDE decodes it**, escapes and all.
#[test]
fn dollar_quotes_are_decoded() {
    assert_eq!(
        runs(r"printf $'a\tb\x41\101\'\\\q' %f", &["/p/a"]),
        [["printf", "a\tbAA'\\\\q", "/p/a"]]
    );
    // Every named escape, a control character, and KDE's two-digit hex
    // that takes a second digit only when it is not `0`.
    assert_eq!(
        split(br"prog $'\a\b\e\f\n\r\t\cA\x10'", None),
        Split::Words(vec![
            b"prog".to_vec(),
            vec![7, 8, 27, 12, 10, 13, 9, 1, 1, b'0']
        ])
    );
}

/// **A line KDE would refuse is refused when read**, each for its reason.
#[test]
fn lines_kde_refuses_are_refused() {
    let err = |line: &str| Command::parse(line).expect_err(line);
    assert_eq!(err(""), CommandError::Empty);
    assert_eq!(err(" \t "), CommandError::Empty);
    assert_eq!(err("echo %"), CommandError::LoneCode);
    assert_eq!(err("echo 'open %f"), CommandError::Unbalanced);
    assert_eq!(err("echo \"open %f"), CommandError::Unbalanced);
    assert_eq!(err("echo ) %f"), CommandError::Unbalanced);
    assert_eq!(err("echo } %f"), CommandError::Unbalanced);
    assert_eq!(err("echo $(date %f"), CommandError::Unbalanced);
    assert_eq!(err("echo `date %f"), CommandError::Unbalanced);
    assert_eq!(err("echo \\"), CommandError::Unbalanced);
    assert_eq!(err("echo $"), CommandError::Unbalanced);
    // `%'` is kept as text by the expander, so its quote is seen only when
    // the line is split -- and never closed.
    assert_eq!(err("echo 100%'s"), CommandError::BadQuoting);
    // Well formed, and fine: arithmetic, a `$((` that is `$( (`, a
    // parameter with a `)` in it, a group.
    for line in [
        "echo $((1 + 2)) %f",
        "echo $( (true) ) %f",
        "echo $((true) ) %f",
        "echo ${x#)} %f",
        // Inside a parameter inside double quotes, a `'` opens nothing.
        r#"echo "${x:-'}" %f"#,
        "echo ${HOME} %f",
        "{ echo a; } %f",
    ] {
        Command::parse(line).unwrap_or_else(|e| panic!("{line:?}: {e}"));
    }
}

/// **A run starts in the menu's `Path=`, or else in the folder its first
/// file is in** -- links resolved -- or, for a file that is not there,
/// wherever its starter is.
#[test]
fn where_a_run_starts() {
    let scratch = scratchdir::ScratchDir::new("slateos-servicemenus-command-dir");
    let file = scratch.dir().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let command = Command::parse("tool %f").unwrap();
    let run = &command.runs(&[file.as_path()], &context()).unwrap()[0];
    assert_eq!(run.dir, Some(std::fs::canonicalize(scratch.dir()).unwrap()));
    let missing = scratch.dir().join("gone.txt");
    assert_eq!(
        command.runs(&[missing.as_path()], &context()).unwrap()[0].dir,
        None
    );
    let fixed = Context {
        working_dir: Some(Path::new("/work")),
        ..context()
    };
    let both = command
        .runs(&[file.as_path(), missing.as_path()], &fixed)
        .unwrap();
    assert_eq!(both.len(), 2);
    assert!(
        both.iter()
            .all(|run| run.dir.as_deref() == Some(Path::new("/work")))
    );
    // With no file, the file's code is nothing -- and a line that was only
    // that has nothing left to run.
    assert_eq!(command.runs(&[], &context()).unwrap()[0].argv, ["tool"]);
    assert_eq!(
        Command::parse("%f").unwrap().runs(&[], &context()),
        Err(CommandError::Empty)
    );
}

/// **A file name that is not UTF-8 arrives byte for byte**, in an argument
/// of its own or inside quotes.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_arrives_unchanged() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let name = OsStr::from_bytes(b"/p/caf\xe9");
    let command = Command::parse(r#"tool %f "%f.bak""#).unwrap();
    let run = &command.runs(&[Path::new(name)], &context()).unwrap()[0];
    assert_eq!(run.argv[1], name);
    let mut backup = b"/p/caf\xe9".to_vec();
    backup.extend_from_slice(b".bak");
    assert_eq!(run.argv[2], OsString::from_vec(backup));
}

/// A small, fixed sequence of awkward values: every byte that means
/// something to a shell, blanks, controls, and bytes past ASCII.
fn awkward_values() -> Vec<Vec<u8>> {
    const POOL: &[u8] = b" '\"\\$`!#&()*;<>?[]{}|~\t\n=%/aZ0-\x01\x7f\x80\xc3\xa9\xff";
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..3000)
        .map(|_| {
            let len = usize::try_from(next() % 9).unwrap();
            (0..len)
                .map(|_| {
                    let pool = u64::try_from(POOL.len()).unwrap();
                    POOL[usize::try_from(next() % pool).unwrap()]
                })
                .collect()
        })
        .collect()
}

/// **A value quoted for where it stands splits back to itself**, in every
/// place a code can stand: bare, in double quotes, in single quotes, in
/// `$'...'`. This is what keeps a file's name from ever being read as shell.
#[test]
fn a_quoted_value_splits_back_to_itself() {
    for value in awkward_values() {
        let bare = [b"prog ".to_vec(), quote_arg(&value)].concat();
        let double = [
            b"prog \"".to_vec(),
            backslash_before(&value, b"$`\"\\"),
            b"\"".to_vec(),
        ]
        .concat();
        let single = [b"prog '".to_vec(), close_and_reopen(&value), b"'".to_vec()].concat();
        let dollar = [
            b"prog $'".to_vec(),
            backslash_before(&value, b"'\\"),
            b"'".to_vec(),
        ]
        .concat();
        for line in [bare, double, single, dollar] {
            assert_eq!(
                split(&line, None),
                Split::Words(vec![b"prog".to_vec(), value.clone()]),
                "{:?} from {:?}",
                String::from_utf8_lossy(&line),
                String::from_utf8_lossy(&value)
            );
        }
    }
}

/// **Only spaces separate arguments**, as in KDE's splitter: a tab is part
/// of one.
#[test]
fn only_spaces_separate_arguments() {
    assert_eq!(
        split(b"a\tb  c", None),
        Split::Words(vec![b"a\tb".to_vec(), b"c".to_vec()])
    );
    assert_eq!(split(b"prog", None), Split::Words(vec![b"prog".to_vec()]));
    // Nor at the start of one.
    assert_eq!(
        split(b"prog \tx", None),
        Split::Words(vec![b"prog".to_vec(), b"\tx".to_vec()])
    );
    assert_eq!(split(b"  ", None), Split::Words(vec![]));
}

/// **Which characters make a line need a shell** -- KDE's table -- and which
/// are only characters.
#[test]
fn which_characters_need_a_shell() {
    for &c in b"#$&()*;<>?[]`{|}" {
        let line = [b"prog a".as_slice(), &[c], b"b"].concat();
        assert_eq!(split(&line, None), Split::Meta, "{:?}", char::from(c));
    }
    for &c in b"!~=%^,:@+-._/" {
        let line = [b"prog a".as_slice(), &[c], b"b"].concat();
        assert_eq!(
            split(&line, None),
            Split::Words(vec![b"prog".to_vec(), vec![b'a', c, b'b']]),
            "{:?}",
            char::from(c)
        );
    }
}
