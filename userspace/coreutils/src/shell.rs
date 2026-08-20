//! Handing a command line to the shell, the one way every utility that does it
//! must do it.
//!
//! Several utilities take a *shell command* as data rather than as an argv:
//! `awk`'s `system()`, `print | "cmd"` and `"cmd" | getline`, and `split
//! --filter=COMMAND`. None of them may split the string themselves — the
//! string may contain pipes, redirections, `&&`, and quoting that only the
//! shell knows how to read, and a utility that tokenised it on spaces would
//! turn `sort > out file` into three arguments.
//!
//! That much is obvious. What is not obvious, and is why this lives here
//! rather than being written out at each call site, is the **host fallback**.
//! On SlateOS the shell is at `/bin/sh` and naming it by absolute path is
//! correct: it cannot be shadowed by a `PATH` the caller controls. On the
//! Windows development host there is no `/bin/sh`, so the name has to be
//! looked up — and the tempting alternative, `cmd /c`, is wrong in a way that
//! is silent: it would change the *quoting rules* under a script that was
//! written for `sh`, so `--filter='cat > "$FILE".out'` would still run and
//! still exit 0, having created a file with quotes in its name. Every copy of
//! this idiom has to make the same choice, so there is one copy.

use std::ffi::OsStr;
use std::process::Command;

/// A [`Command`] that runs `text` through `sh -c`, with no redirections set up
/// yet — the caller pipes whichever of stdin/stdout it needs.
#[must_use]
pub fn shell<S: AsRef<OsStr>>(text: S) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(text);
        command
    }
    #[cfg(not(unix))]
    {
        // On the development host there is no `/bin/sh` at a fixed path, but a
        // POSIX shell is on PATH; falling back to `cmd /c` would change the
        // quoting rules under the script's feet, so this does not.
        let mut command = Command::new("sh");
        command.arg("-c").arg(text);
        command
    }
}

/// [`shell`] for a caller that holds the command as bytes rather than as an
/// `OsStr` — `awk`, whose strings are byte strings.
///
/// Lossy on a host whose `OsStr` is not bytes, for the same reason path
/// handling is: there is no byte view that round-trips there. On the target
/// the conversion is exact.
#[must_use]
pub fn shell_bytes(text: &[u8]) -> Command {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        shell(OsStr::from_bytes(text))
    }
    #[cfg(not(unix))]
    {
        shell(String::from_utf8_lossy(text).into_owned())
    }
}
