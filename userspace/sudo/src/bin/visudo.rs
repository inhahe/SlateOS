//! `visudo` -- edit the sudoers file with syntax checking.
//!
//! ```text
//! visudo                 Edit /etc/sudoers
//! visudo -c              Check syntax only
//! visudo -f file         Edit alternate sudoers file
//! visudo -s              Strict mode (error on warnings)
//! ```
//!
//! A program of its own since 2026-10-01, where it had been a name `sudo`
//! answered to: it needs none of the right to change identity that `sudo`
//! holds, and one binary answering to both would hold the union
//! (design-decisions 1045). The sudoers file itself -- model, parser, check --
//! is the package's library, which `sudo` reads too.

#![deny(clippy::all)]

use quoting::{os_bytes, quoteaf_os};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use sudo::{SUDOERS_PATH, SudoError, editor_command, editor_words, validate_sudoers};
// The editing half is unix's; the host build only checks syntax.
#[cfg(unix)]
use std::io::{self, Read as _, Write as _};
#[cfg(unix)]
use std::time::SystemTime;
#[cfg(unix)]
use sudo::{O_NOFOLLOW, SyntaxError};

// ============================================================================
// Visudo options
// ============================================================================

/// Parsed command-line options for the visudo personality.
///
/// `file` is [`OsString`]: `-f` names a file to open, and an alternate sudoers
/// path is exactly as free in its bytes as any other path on this OS.
#[derive(Debug)]
struct VisudoOpts {
    check_only: bool,
    file: OsString,
    strict: bool,
}

impl Default for VisudoOpts {
    fn default() -> Self {
        Self {
            check_only: false,
            file: OsString::from(SUDOERS_PATH),
            strict: false,
        }
    }
}

/// Parse visudo command-line arguments.
fn parse_visudo_args(args: &[OsString]) -> Result<VisudoOpts, SudoError> {
    let mut opts = VisudoOpts::default();
    // A slice cursor, as in `parse_sudo_args`: `-f` takes its value from a tail
    // already proved non-empty, so there is no `i + 1` to bounds-check
    // separately from the `args[i + 1]` that follows it.
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        // Matching on bytes rather than on `&str`: the three options are ASCII
        // so the recognised set does not change, and an argument that is not
        // text now reaches the diagnostic below instead of the panic that used
        // to happen before this function was ever entered.
        match &*os_bytes(arg) {
            b"-c" => opts.check_only = true,
            b"-s" => opts.strict = true,
            b"-f" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-f requires an argument".to_string()));
                };
                opts.file = value.clone();
                rest = after_value;
            }
            other if other.starts_with(b"-") => {
                return Err(SudoError::UsageError(format!(
                    "unknown option: {}",
                    quoteaf_os(arg)
                )));
            }
            _ => {
                return Err(SudoError::UsageError(format!(
                    "unexpected argument: {}",
                    quoteaf_os(arg)
                )));
            }
        }
    }

    Ok(opts)
}

// ============================================================================
// Usage
// ============================================================================

fn print_visudo_usage() {
    eprintln!("usage: visudo [-c] [-f file] [-s]");
    eprintln!("       -c          Check syntax only");
    eprintln!("       -f file     Edit alternate sudoers file");
    eprintln!("       -s          Strict mode (error on warnings)");
}

// ============================================================================
// Editing
// ============================================================================

/// Main entry point for the `visudo` personality.
fn run_visudo(args: &[OsString]) -> i32 {
    let opts = match parse_visudo_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("visudo: {e}");
            print_visudo_usage();
            return 1;
        }
    };

    let file_path = Path::new(&opts.file);

    // Check-only mode.
    if opts.check_only {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("visudo: cannot read {}: {e}", quoteaf_os(&opts.file));
                return 1;
            }
        };

        let errors = validate_sudoers(&content, opts.strict);
        if errors.is_empty() {
            println!("{}: parsed OK", quoteaf_os(&opts.file));
            return 0;
        }

        for err in &errors {
            eprintln!("visudo: {}: {err}", quoteaf_os(&opts.file));
        }

        let fatal_count = errors.iter().filter(|e| !e.is_warning).count();
        if fatal_count > 0 {
            return 1;
        }
        if opts.strict {
            return 1;
        }
        println!("{}: parsed with warnings", quoteaf_os(&opts.file));
        return 0;
    }

    // Editing mode: upstream's `visudo`, with the file opened and LOCKED
    // first, the copy beside it, and the result installed by rename.
    edit_sudoers(file_path, opts.strict, &editor_words(&editor_command()))
}

/// The mode a sudoers file is installed with: read-only, owner and group.
#[cfg(unix)]
const SUDOERS_MODE: u32 = 0o440;

/// What becomes of an edit when the copy does not parse: upstream's
/// `whatnow`.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhatNow {
    /// `e`: edit the copy again.
    Edit,
    /// `x`, or the end of input: leave the file as it was.
    Exit,
    /// `Q`: install it anyway.
    Quit,
}

/// One answer to "What now?" -- `None` for anything else, which upstream
/// answers with the list of options and asks again.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn what_now(answer: &str) -> Option<WhatNow> {
    match answer.trim() {
        "e" => Some(WhatNow::Edit),
        "x" => Some(WhatNow::Exit),
        "Q" => Some(WhatNow::Quit),
        _ => None,
    }
}

/// Ask "What now?" until there is an answer. The end of input is `x`, as it
/// is upstream: until 2026-10-01 it was "edit again", forever.
#[cfg(unix)]
fn ask_what_now() -> WhatNow {
    loop {
        eprint!("What now? ");
        // Ignored: a prompt that cannot be shown leaves nothing better to do
        // than read the answer anyway.
        let _ = io::stderr().flush();
        let mut answer = String::new();
        match io::stdin().read_line(&mut answer) {
            Ok(0) | Err(_) => return WhatNow::Exit,
            Ok(_) => {}
        }
        if let Some(choice) = what_now(&answer) {
            return choice;
        }
        eprintln!(
            "Options are:\n  (e)dit sudoers file again\n  e(x)it without saving changes to sudoers file\n  (Q)uit and save changes to sudoers file (DANGER!)\n"
        );
    }
}

/// The name of the copy `visudo` edits: upstream's `<file>.tmp`, beside the
/// file, so the install is a rename within one directory -- atomic, and never
/// a moment with half a sudoers file -- and in a directory only root writes,
/// unlike `/tmp`, where the copy used to be made at a name anyone could
/// predict and plant a symlink on.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn sudoers_temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// The contents as the copy should start: the file's own, with a final
/// newline if it lacked one, as upstream adds it.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn sudoers_starting_text(original: &[u8]) -> Vec<u8> {
    let mut text = original.to_vec();
    if text.last().is_some_and(|&b| b != b'\n') {
        text.push(b'\n');
    }
    text
}

/// Edit the sudoers file at `path`: upstream's `visudo` editing loop.
///
/// Until 2026-10-01 the lock was a `.lck` file that was checked for and then
/// written, so two `visudo`s could both take it, and one more than five
/// minutes old was "stale" and taken while its owner was still editing; the
/// copy was made at `/tmp/visudo-<pid>`, a name anyone could predict and point
/// at another file with a symlink; and the result was written over the file
/// in place, with no mode or owner set. Now:
///
/// 1. the file itself is opened (created 0440 when absent) and locked with
///    `flock`, held until `visudo` is done; a held lock is "busy, try again
///    later";
/// 2. the copy is `<file>.tmp`, opened without following a symlink, holding
///    the file's text with a final newline and the file's time;
/// 3. the editor runs on it (`EDITOR -- file.tmp`); a copy left empty where the
///    file was not is refused; one whose size and time did not move, with time
///    spent in the editor, is "unchanged" and nothing is installed;
/// 4. a copy that does not parse is reported and the choice offered --
///    (e)dit again, e(x)it, or (Q)uit and save;
/// 5. a copy that does is given to root:root, mode 0440, and renamed over the
///    file.
#[cfg(unix)]
fn edit_sudoers(path: &Path, strict: bool, editor: &[OsString]) -> i32 {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let shown = quoteaf_os(path.as_os_str());
    let mut sudoers = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(SUDOERS_MODE)
        .open(path)
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!(
                "visudo: {}: {}",
                quoteaf_os(path.as_os_str()),
                errmsg::strerror(&e)
            );
            return 1;
        }
    };
    match sudoers.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            eprintln!("visudo: {shown} busy, try again later");
            return 1;
        }
        Err(fs::TryLockError::Error(e)) => {
            eprintln!("visudo: unable to lock {shown}: {}", errmsg::strerror(&e));
            eprint!("Edit anyway? [y/N]");
            // Ignored, as the prompt above it: the answer is read either way.
            let _ = io::stderr().flush();
            let mut answer = String::new();
            let yes = io::stdin().read_line(&mut answer).is_ok()
                && answer.trim_start().starts_with(['y', 'Y']);
            if !yes {
                return 1;
            }
        }
    }

    let mut original = Vec::new();
    let original_meta = match sudoers
        .read_to_end(&mut original)
        .and_then(|_| sudoers.metadata())
    {
        Ok(meta) => meta,
        Err(e) => {
            eprintln!(
                "visudo: {}: {}",
                quoteaf_os(path.as_os_str()),
                errmsg::strerror(&e)
            );
            return 1;
        }
    };

    let temp = sudoers_temp_path(path);
    let temp_shown = quoteaf_os(temp.as_os_str());
    let discard = |code: i32| -> i32 {
        // Ignored: the copy is being abandoned, and the reason has been said.
        let _ = fs::remove_file(&temp);
        code
    };
    {
        let made = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o700)
            .custom_flags(O_NOFOLLOW)
            .open(&temp)
            .and_then(|mut copy| {
                copy.write_all(&sudoers_starting_text(&original))?;
                // The file's time, so an untouched copy can be told; ignored on
                // failure, as upstream ignores it.
                if let Ok(time) = original_meta.modified() {
                    let _ = copy.set_modified(time);
                }
                Ok(())
            });
        if let Err(e) = made {
            eprintln!(
                "visudo: {}: {}",
                quoteaf_os(temp.as_os_str()),
                errmsg::strerror(&e)
            );
            return discard(1);
        }
    }

    let Some((editor, editor_args)) = editor.split_first() else {
        return discard(1);
    };
    loop {
        let started = SystemTime::now();
        let ran = process::Command::new(editor)
            .args(editor_args)
            .arg("--")
            .arg(&temp)
            .status();
        let finished = SystemTime::now();
        // vi's exit status counts its errors (XPG4), so only a failure to run
        // the editor at all stops here -- as upstream.
        if ran.is_err() {
            eprintln!(
                "visudo: editor ({}) failed, {shown} unchanged",
                quoteaf_os(editor)
            );
            return discard(1);
        }
        let edited = match fs::metadata(&temp) {
            Ok(meta) => meta,
            Err(_) => {
                eprintln!(
                    "visudo: unable to stat temporary file ({temp_shown}), {shown} unchanged"
                );
                return discard(1);
            }
        };
        if edited.len() == 0 && !original.is_empty() {
            eprintln!("visudo: zero length temporary file ({temp_shown}), {shown} unchanged");
            return discard(1);
        }
        let untouched = edited.len() == original_meta.len()
            && edited.modified().ok() == original_meta.modified().ok()
            && finished != started;
        if untouched {
            eprintln!("visudo: {temp_shown} unchanged");
            return discard(0);
        }

        let text = match fs::read_to_string(&temp) {
            Ok(text) => text,
            Err(e) => {
                eprintln!(
                    "visudo: {}: {}",
                    quoteaf_os(temp.as_os_str()),
                    errmsg::strerror(&e)
                );
                return discard(1);
            }
        };
        let errors = validate_sudoers(&text, strict);
        let fatal: Vec<&SyntaxError> = errors.iter().filter(|e| !e.is_warning).collect();
        if !fatal.is_empty() {
            for err in &fatal {
                eprintln!("visudo: {}: {err}", quoteaf_os(temp.as_os_str()));
            }
            match ask_what_now() {
                WhatNow::Edit => continue,
                WhatNow::Exit => return discard(0),
                WhatNow::Quit => {}
            }
        }

        // Install: root's, 0440, renamed over the file.
        // The order matters: owner and mode are set on the copy BEFORE the
        // rename, so the file is never readable under the wrong ones.
        if let Err(e) = std::os::unix::fs::chown(&temp, Some(0), Some(0)) {
            eprintln!(
                "visudo: unable to set (uid, gid) of {temp_shown} to (0, 0): {}",
                errmsg::strerror(&e)
            );
        }
        if let Err(e) = fs::set_permissions(&temp, fs::Permissions::from_mode(SUDOERS_MODE)) {
            eprintln!(
                "visudo: unable to change mode of {temp_shown} to 0{SUDOERS_MODE:o}: {}",
                errmsg::strerror(&e)
            );
        }
        if let Err(e) = fs::rename(&temp, path) {
            eprintln!(
                "visudo: error renaming {temp_shown}, {shown} unchanged: {}",
                errmsg::strerror(&e)
            );
            return discard(1);
        }
        return 0;
    }
}

/// `visudo` installs root's file and must be able to change owners; a host
/// build cannot.
#[cfg(not(unix))]
fn edit_sudoers(_path: &Path, _strict: bool, _editor: &[OsString]) -> i32 {
    eprintln!("visudo: this build cannot install a sudoers file");
    1
}

fn main() {
    // `args_os`, not `args`: the file `-f` names is a path, and a path may hold
    // any byte but `/` and NUL.
    let rest: Vec<OsString> = std::env::args_os().skip(1).collect();
    process::exit(run_visudo(&rest));
}

#[cfg(test)]
// Panicking on bad data is what a test is *for*; see sudo's test module.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use scratchdir::ScratchDir;

    /// A command line as `OsString`s, the shape `run_visudo` takes.
    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    /// An `OsString` spelled `prefix`, then one unit that is not valid
    /// Unicode, then `suffix`: byte 0xff on unix, an unpaired surrogate on
    /// Windows -- what each platform's `OsString` holds that `to_str()`
    /// refuses. sudo's tests have the same helper and say more about it.
    fn not_text(prefix: &str, suffix: &str) -> OsString {
        #[cfg(unix)]
        let out = {
            let mut v = prefix.as_bytes().to_vec();
            v.push(0xff);
            v.extend_from_slice(suffix.as_bytes());
            quoting::os_from_bytes(&v)
        };
        #[cfg(not(unix))]
        let out = {
            use std::os::windows::ffi::OsStringExt;
            let mut w: Vec<u16> = prefix.encode_utf16().collect();
            w.push(0xD800);
            w.extend(suffix.encode_utf16());
            OsString::from_wide(&w)
        };
        assert!(out.to_str().is_none(), "the helper must produce non-text");
        out
    }

    /// A file to edit is a path, not a name, so `visudo -f` takes it as it came.
    #[test]
    fn visudo_takes_the_file_to_edit_as_bytes() {
        let wanted = not_text("/etc/sudoers.d/", "");
        let opts = parse_visudo_args(&[OsString::from("-f"), wanted.clone()]).unwrap();
        assert_eq!(opts.file, wanted);
    }

    // -- Visudo option parsing tests --

    #[test]
    fn parse_visudo_defaults() {
        let args: Vec<OsString> = vec![];
        let opts = parse_visudo_args(&args).unwrap();
        assert!(!opts.check_only);
        assert_eq!(opts.file, SUDOERS_PATH);
        assert!(!opts.strict);
    }

    #[test]
    fn parse_visudo_check_only() {
        let args = argv(&["-c"]);
        let opts = parse_visudo_args(&args).unwrap();
        assert!(opts.check_only);
    }

    #[test]
    fn parse_visudo_alternate_file() {
        let args = argv(&["-f", "/tmp/sudoers"]);
        let opts = parse_visudo_args(&args).unwrap();
        assert_eq!(opts.file, "/tmp/sudoers");
    }

    #[test]
    fn parse_visudo_strict() {
        let args = argv(&["-s"]);
        let opts = parse_visudo_args(&args).unwrap();
        assert!(opts.strict);
    }

    #[test]
    fn parse_visudo_unknown_flag() {
        let args = argv(&["-z"]);
        assert!(parse_visudo_args(&args).is_err());
    }

    #[test]
    fn parse_visudo_f_missing_value() {
        let args = argv(&["-f"]);
        assert!(parse_visudo_args(&args).is_err());
    }

    // -- visudo: lock, copy beside the file, install by rename --

    #[test]
    fn what_now_takes_upstreams_three_answers() {
        assert_eq!(what_now("e\n"), Some(WhatNow::Edit));
        assert_eq!(what_now("x"), Some(WhatNow::Exit));
        assert_eq!(what_now("Q\n"), Some(WhatNow::Quit));
        // Anything else is asked again; `q` is not `Q`, as upstream has it.
        assert_eq!(what_now("q"), None);
        assert_eq!(what_now(""), None);
    }

    #[test]
    fn the_copy_sits_beside_the_file() {
        assert_eq!(
            sudoers_temp_path(Path::new("/etc/sudoers")),
            PathBuf::from("/etc/sudoers.tmp")
        );
    }

    #[test]
    fn the_copy_ends_in_a_newline() {
        assert_eq!(sudoers_starting_text(b"a\nb"), b"a\nb\n");
        assert_eq!(sudoers_starting_text(b"a\n"), b"a\n");
        assert_eq!(sudoers_starting_text(b""), b"");
    }

    /// An "editor" that replaces the copy's contents: `sh -c SCRIPT sh -- FILE`,
    /// so the copy is `$2`.
    #[cfg(unix)]
    fn writes(text: &str) -> Vec<OsString> {
        vec![
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(format!("printf '%s' '{text}' > \"$2\"")),
            OsString::from("sh"),
        ]
    }

    #[cfg(unix)]
    #[test]
    fn a_held_lock_is_busy() {
        let dir = ScratchDir::new("visudo_lock");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let held = fs::File::open(&path).expect("open");
        held.lock().expect("lock");
        assert_eq!(edit_sudoers(&path, false, &writes("x")), 1);
        // Nothing was copied or changed while it was busy.
        assert!(!sudoers_temp_path(&path).exists());
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_good_edit_is_installed_read_only_by_rename() {
        use std::os::unix::fs::MetadataExt as _;
        let dir = ScratchDir::new("visudo_install");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let wanted = "root ALL = (ALL) ALL\nalice ALL = (root) /usr/bin/id\n";
        assert_eq!(edit_sudoers(&path, false, &writes(wanted)), 0);
        assert_eq!(fs::read_to_string(&path).expect("read"), wanted);
        assert_eq!(fs::metadata(&path).expect("meta").mode() & 0o777, 0o440);
        assert!(!sudoers_temp_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_untouched_copy_installs_nothing() {
        let dir = ScratchDir::new("visudo_untouched");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let editor = vec![OsString::from("true")];
        assert_eq!(edit_sudoers(&path, false, &editor), 0);
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
        assert!(!sudoers_temp_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_emptied_copy_is_refused() {
        let dir = ScratchDir::new("visudo_empty");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        assert_eq!(edit_sudoers(&path, false, &writes("")), 1);
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
        assert!(!sudoers_temp_path(&path).exists());
    }

    /// The copy is opened without following a symlink, so one planted at its
    /// name cannot aim the edit at another file.
    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_copys_name_is_not_followed() {
        let dir = ScratchDir::new("visudo_link");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let victim = dir.dir().join("victim");
        fs::write(&victim, b"untouched\n").expect("victim");
        std::os::unix::fs::symlink(&victim, sudoers_temp_path(&path)).expect("plant");
        assert_eq!(edit_sudoers(&path, false, &writes("x")), 1);
        assert_eq!(fs::read(&victim).expect("read"), b"untouched\n");
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
    }
}
