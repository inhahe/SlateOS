//! End-to-end tests that a child gets only the descriptors it was *given*.
//!
//! Windows has no per-descriptor close-on-exec: `CreateProcess` takes a single
//! `bInheritHandles` for the whole call, so the child receives every handle that
//! is marked inheritable at that moment — whatever its own stdio was set to. A
//! shell's fd 0/1/2 arrive marked that way, because that is how its parent
//! handed them over, so a shell that does not clear the mark leaks its own
//! standard descriptors into every process it starts. See
//! [`osh::interp::seal_std_handles`].
//!
//! The symptom is that the shell's *output pipe* outlives the shell: whoever is
//! reading it waits for the shell's longest-lived child instead of for the
//! shell. Only a separate process on a real pipe can show that, which is why
//! these drive the built binary rather than a `Shell` in this one. Reading is
//! done on its own thread against a deadline, so a leak fails by name instead of
//! hanging the run.
//!
//! Windows-only: elsewhere inheritance is per-descriptor and decided by whoever
//! opens the descriptor, so there is nothing to leak and nothing to test.
#![cfg(windows)]

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Duration;

/// One definition, shared with the in-process tests rather than copied — see
/// the module's own docs.
#[path = "../src/hostpath.rs"]
mod hostpath;

/// Serialises the spawns in this file.
///
/// `Stdio` hands a child its descriptors by duplicating them *inheritably* for
/// the duration of the `CreateProcess` call — and "inheritable" is a property of
/// the handle, not of the call, so it is visible to every other spawn happening
/// at that moment. Two of these tests starting at once would each hand the
/// other's pipe to its own shell, and the leak that followed would be this test
/// binary's rather than the shell's. One spawn at a time removes the ambiguity
/// and leaves each test measuring only what it set up.
static SPAWN: Mutex<()> = Mutex::new(());

/// How long the background job in these scripts lives. It must be long enough
/// that no machine is slow enough for it to finish on its own — a job that ended
/// by itself would close the leaked descriptor and hide the very thing under
/// test.
const JOB_SECS: u32 = 20;

/// How long the pipe may take to reach EOF once the shell itself has finished.
/// Generous against a loaded machine, and far short of [`JOB_SECS`], which is
/// what a leak costs.
const EOF_DEADLINE: Duration = Duration::from_secs(8);

/// Which of the shell's standard descriptors the test's pipe is attached to.
/// Each is a separate handle, separately marked, so each has to be sealed and
/// each is checked.
#[derive(Clone, Copy)]
enum Piped {
    Stdout,
    Stderr,
}

/// Run the built `osh` on a `-c` script with one of its output descriptors on a
/// real pipe, wait for the shell to exit, then read the pipe to EOF.
///
/// `Ok` is what was read; `Err` is EOF failing to arrive within [`EOF_DEADLINE`]
/// — i.e. something other than the shell still holds the write end. The read is
/// on its own thread precisely so that case can be *reported*: the thread is
/// left parked on a pipe nothing will write to, and the process's own exit
/// collects it.
fn read_after_exit(which: Piped, script: &str) -> Result<String, ()> {
    let (mut reader, writer) = std::io::pipe().expect("pipe");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_osh"));
    // The host's `sleep`, not whichever one cargo staged on the search path:
    // these tests are timed against [`JOB_SECS`], so a `sleep` that ignored its
    // argument would make a leak look like correct behaviour (`hostpath`).
    hostpath::scrub(&mut cmd)
        .args(["-c", script])
        .stdin(Stdio::null());
    match which {
        Piped::Stdout => {
            cmd.stdout(Stdio::from(writer)).stderr(Stdio::null());
        }
        Piped::Stderr => {
            cmd.stdout(Stdio::null()).stderr(Stdio::from(writer));
        }
    }
    let mut child = {
        let _serial = SPAWN.lock().unwrap_or_else(PoisonError::into_inner);
        cmd.spawn().expect("spawn osh")
    };
    // The `Command` owns the write end it was handed and keeps it for as long as
    // it lives, so this process is a writer too until it goes. Letting it live to
    // the end of the function would mean the pipe never reached EOF and every
    // test here failed for a reason of its own making.
    drop(cmd);
    child.wait().expect("wait for osh");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = reader.read_to_string(&mut buf);
        // Past the deadline the receiver is gone and nothing is owed to it; the
        // send failing is how this thread finds that out.
        let _ = tx.send(buf);
    });
    rx.recv_timeout(EOF_DEADLINE).map_err(|_| ())
}

/// A script that starts a long external background job whose own output goes
/// nowhere near the shell's, then reports on `where` (`1` or `2`).
///
/// The pause is what makes the test mean anything: a `&` job runs on a thread
/// that spawns the process, and a shell that exits first takes the thread with
/// it — leaving no child to inherit anything. Getting the pause wrong can only
/// make the test pass when it should have failed, never the reverse.
fn job_script(where_: u8) -> String {
    format!("sleep {JOB_SECS} >/dev/null 2>&1 &\nsleep 0.5\necho done >&{where_}")
}

#[test]
fn a_background_jobs_child_does_not_inherit_the_shells_stdout() {
    // The job was given `/dev/null` for both its own output descriptors, so it
    // holds nothing of the shell's by any route the script can see. If the pipe
    // stays open anyway, the job is holding one it was never handed.
    let out = read_after_exit(Piped::Stdout, &job_script(1))
        .expect("the shell's stdout pipe outlived the shell: a child inherited fd 1");
    assert_eq!(out, "done\n");
}

#[test]
fn a_background_jobs_child_does_not_inherit_the_shells_stderr() {
    // The same again for fd 2 — a different handle, marked and sealed
    // separately. Here it is fd 2 that carries the pipe and fd 1 that goes
    // nowhere, so nothing but an inherited fd 2 can hold the pipe open.
    let out = read_after_exit(Piped::Stderr, &job_script(2))
        .expect("the shell's stderr pipe outlived the shell: a child inherited fd 2");
    assert_eq!(out, "done\n");
}

#[test]
fn a_child_that_should_inherit_stdout_still_does() {
    // The other half of the rule: sealing fd 1 off must not stop a child that is
    // *meant* to write to it. `Stdio::inherit` duplicates the descriptor for the
    // one spawn that asked for it, which is exactly the distinction — this
    // child's stdout, rather than every child's from now on.
    let out = read_after_exit(
        Piped::Stdout,
        r#"echo shell; "$BASH" -c 'echo child' </dev/null"#,
    )
    .expect("the shell's stdout pipe outlived the shell");
    assert_eq!(out, "shell\nchild\n");
}
