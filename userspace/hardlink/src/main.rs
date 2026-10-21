//! hardlink -- consolidate duplicate files using hard links.
//!
//! A port of util-linux 2.39.3's `misc-utils/hardlink.c`, function by
//! function, with the parts of glibc and of util-linux's `lib/` whose
//! behaviour shows: `tsearch`/`twalk` ([`tree`]), which decide the order
//! files are linked in and printed; `nftw` ([`walk`]), which decides the
//! order they are found in; and `lib/fileeq.c` ([`fileeq`]), which compares
//! their contents. Measured against Ubuntu 24.04's `hardlink` by
//! `scripts/hardlink-diff.sh`.
//!
//! # What is upstream's, and easy to get wrong
//!
//! * **The order.** Files of one size on one device are a list headed by
//!   the one to keep: the newest, then the lowest inode number (`-O` the
//!   oldest; `-m`/`-M` the most or fewest links first). The lists are
//!   visited in the post-order of a red-black tree of (device, size) as
//!   glibc shapes it, which is neither size order nor the order found.
//! * **Contents are compared block by block,** the first 32 bytes first,
//!   then `--io-size` at a time; with `sha256` (the default), `sha1` or
//!   `crc32` each block's digest is made by the kernel (`AF_ALG`) and kept,
//!   so a file compared with many is read once. Where the kernel cannot --
//!   SlateOS's cannot -- the method falls back to `memcmp`, and `Method:`
//!   says so.
//! * **A link that fails with `EMLINK`** (the file has all the links it
//!   may) makes the file that could not be replaced the one kept from
//!   there on, and the files in between are never masters.
//! * **The summary is printed at exit,** once the walk has begun -- also
//!   after an interrupt and after an attribute that cannot be read.
//!   `SIGUSR1` prints it in the middle; `SIGINT` writes a newline and stops
//!   at the next file.
//! * **`Saved:` counts a file only when its last link went,** and `Linked:`
//!   every name replaced.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370); stdout prints names as they are.
//! * **The signal handler blocks no other signal** while it runs, where
//!   upstream blocks all; it only stores the signal and writes a newline.
//! * **Two of upstream's assertions** -- that a file's attribute list or
//!   value is as long the second time it is read as the first -- become
//!   "not the same", as only a change made in between can break them.
//! * **A file with no extents at all** is not taken for a clone by
//!   `--skip-reflinks`, where upstream reads past its extent map.

stdfdguard::guard_std_fds!();

#[cfg(unix)]
mod fileeq;
#[cfg(unix)]
mod hardlink;
// On every host, for its tests: the program that uses it is Unix's alone.
#[cfg_attr(
    not(unix),
    allow(
        dead_code,
        reason = "used by the Unix-only program; built here for its tests"
    )
)]
mod tree;
#[cfg(unix)]
mod walk;

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    // Before anything else: a standard descriptor the program was started
    // without is closed again, and SIGPIPE given back the disposition it
    // came with.
    stdfdguard::restore();
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    std::process::ExitCode::from(hardlink::main(&argv))
}

#[cfg(not(unix))]
fn main() {
    eprintln!("hardlink: runs only on a Unix host");
    std::process::exit(1);
}
