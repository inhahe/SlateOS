//! `gzip` -- compress or expand files. GNU gzip 1.12's, ported (`lib.rs`);
//! also `gunzip`, `uncompress` and `zcat`, as Ubuntu's scripts of those
//! names are.

stdfdguard::guard_std_fds!();

#[cfg(unix)]
fn main() {
    // Before anything else: a standard descriptor gzip was started without
    // is closed again, and SIGPIPE given back the disposition it came with,
    // which decides whether gzip catches it.
    stdfdguard::restore();
    let status = gzip::driver::run(std::env::args_os().collect());
    std::process::exit(status);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("gzip: files are only compressed on a Unix host");
    std::process::exit(1);
}
