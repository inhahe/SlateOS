//! The `ssh-keygen` binary. Everything it does is in the library next to it,
//! for the reason spelled out in `Cargo.toml`: a binary crate produces no rlib,
//! so while this tool was a single `main.rs` no test in any other crate could
//! link it -- and the bug that made this split necessary was precisely one that
//! only a test spanning two crates can see. `ssh-keygen` wrote a private key
//! format `sshd` could not read, and both crates' suites passed.

fn main() {
    if let Err(e) = ssh_keygen::run() {
        ssh_keygen::report_and_exit(&e);
    }
}
