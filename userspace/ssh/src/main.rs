//! The `ssh(1)` binary: a shim over [`ssh::run_cli`].
//!
//! All of the client is in the library next to this file. The split is not
//! organisational tidiness — it is what lets a third crate link the real client
//! and drive it against the real server, which is the only kind of test that
//! can catch the two halves of this protocol disagreeing. See the library's
//! module docs for why that matters here specifically.
//!
//! Ending the process is this file's whole job, and the reason it is a separate
//! job: a library that calls `process::exit` cannot be called by a test, so
//! `run_cli` returns the status and this hands it to the operating system.

fn main() {
    std::process::exit(ssh::run_cli());
}
