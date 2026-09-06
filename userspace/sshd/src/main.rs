//! The `sshd(8)` binary: a shim over [`sshd::run_cli`].
//!
//! All of the daemon is in the library next to this file. The split is not
//! organisational tidiness — it is what lets a third crate link the real server
//! and drive the real client against it, which is the only kind of test that
//! can catch the two halves of this protocol disagreeing. Six bugs have come
//! out of exactly that, every one of them with both crates' own suites green.
//! See the library's module docs for why that matters here specifically.
//!
//! Ending the process is this file's whole job, and the reason it is a separate
//! job: a library that calls `process::exit` cannot be called by a test, so
//! `run_cli` returns the status and this hands it to the operating system.

fn main() {
    std::process::exit(sshd::run_cli());
}
