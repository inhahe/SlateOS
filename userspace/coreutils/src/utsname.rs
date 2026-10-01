//! The `uname(2)` answers that more than one program prints.
//!
//! GNU builds `arch` from `src/uname.c` -- the same file as `uname`, with
//! `uname_mode = UNAME_ARCH` -- so `arch` and `uname -m` cannot print different
//! machine names: there is one call to `uname(2)` behind both. Here the value
//! is a constant rather than a call (see `uname.rs` for why it is not read
//! from `/proc`), and this module is where it lives so that there is still
//! only one of it. Before `arch` existed it was a private constant of `uname`;
//! a second private constant in `arch` would have been a second answer to the
//! same question, which is how `uname`'s other fields once came to hold four.

/// The machine hardware name: `uname -m`, `uname -a`'s machine field, and
/// everything `arch` prints.
///
/// The kernel's `sys_uname` reports the same string, and it is the only
/// architecture this system is built for.
pub const MACHINE: &[u8] = b"x86_64";
