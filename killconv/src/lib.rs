//! The exit code a process reports when it is killed by a signal.
//!
//! # Why this crate exists
//!
//! `SYS_PROCESS_KILL` (506) takes a PID and an **exit code**, not a signal
//! number. The convention for "killed by signal N" is the shell's `128 + N`,
//! so a process terminated as if by `SIGKILL` exits `137`, not `9`.
//!
//! Three crates encoded that as bare literals at their own call sites —
//! `userspace/kill`, `userspace/pgrep` and `userspace/htop` — and one of them
//! was wrong. `htop` passed `9`, with the comment *"We pass exit code 9
//! (SIGKILL equivalent)"*. Nine is SIGKILL's **signal** number; as an exit
//! code it is indistinguishable from a program that chose to exit with status
//! 9. Anything waiting on a process killed from htop's list was told it had
//! exited normally.
//!
//! **No test could see it.** The value is only observable from the parent of
//! the killed process, and none of the three crates has one. That is what
//! makes a bare literal expensive here rather than merely untidy: the
//! difference between `9` and `137` is invisible at every point a test can
//! reach.
//!
//! # What this deliberately does not contain
//!
//! The `SYS_PROCESS_KILL` syscall stub stays in each caller. Moving inline
//! assembly between crates buys nothing here — the three copies of it are
//! identical and mechanical, and the defect was never in the stub. What was
//! worth centralising is the arithmetic nobody can check by looking.

#![no_std]
#![forbid(unsafe_code)]

/// Hangup. `kill -HUP`.
pub const SIGHUP: u32 = 1;
/// Interrupt. What Ctrl-C sends.
pub const SIGINT: u32 = 2;
/// Quit, with a core dump on a system that writes them.
pub const SIGQUIT: u32 = 3;
/// Kill. Cannot be caught or ignored.
pub const SIGKILL: u32 = 9;
/// Terminate. The polite default, and what `kill` sends with no argument.
pub const SIGTERM: u32 = 15;

/// The exit code a process killed by `sig` reports to its parent.
///
/// The shell's convention: `128 + signal`. `SIGKILL` (9) becomes 137.
///
/// # Why `u32` in and `u64` out
///
/// Signal numbers are small and unsigned; the syscall takes a `u64` exit code.
/// Converting here rather than at each call site means no caller writes the
/// cast, and no caller writes the addition — which is the whole point, since
/// the addition is what one of them got wrong.
///
/// # Out of range
///
/// A signal above 127 would collide with the codes a program may return for
/// itself, so it saturates at 255 rather than wrapping into that space. No
/// real signal is anywhere near: the highest on Linux is 64.
#[must_use]
// The allow is a claim, not a silence: the branch above bounds `sig` at 127,
// so the sum is at most 255 and cannot overflow a u64. If that guard is ever
// removed this allow becomes false, which is why it names the guard.
#[allow(clippy::arithmetic_side_effects)]
pub const fn exit_code_for_signal(sig: u32) -> u64 {
    if sig > 127 { 255 } else { 128 + sig as u64 }
}

/// The signal a process was killed by, if `code` looks like a signal death.
///
/// The inverse of [`exit_code_for_signal`], and deliberately an `Option`: a
/// process that exits 137 of its own accord is indistinguishable from one
/// killed by `SIGKILL`, so this answers "consistent with" rather than "was".
/// Callers that print it should say so.
#[must_use]
// Same shape: `code > 128` is checked before the subtraction, so it cannot
// underflow, and `code < 256` bounds the result inside u32.
#[allow(clippy::arithmetic_side_effects)]
pub const fn signal_from_exit_code(code: u64) -> Option<u32> {
    if code > 128 && code < 256 {
        Some((code - 128) as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_signals_the_tree_actually_sends() {
        // The literals that were spread across three crates.
        assert_eq!(exit_code_for_signal(SIGTERM), 143);
        assert_eq!(exit_code_for_signal(SIGKILL), 137);
        assert_eq!(exit_code_for_signal(SIGHUP), 129);
        assert_eq!(exit_code_for_signal(SIGINT), 130);
    }

    #[test]
    fn nine_is_a_signal_number_and_not_an_exit_code() {
        // htop passed 9 for "SIGKILL equivalent". This is the distinction it
        // lost: 9 as an exit code means a program chose to exit with 9.
        assert_ne!(exit_code_for_signal(SIGKILL), u64::from(SIGKILL));
        assert_eq!(signal_from_exit_code(9), None);
        assert_eq!(signal_from_exit_code(137), Some(SIGKILL));
    }

    #[test]
    fn the_round_trip_holds_for_every_real_signal() {
        for sig in 1..=64u32 {
            assert_eq!(signal_from_exit_code(exit_code_for_signal(sig)), Some(sig));
        }
    }

    #[test]
    fn an_ordinary_exit_status_is_not_read_as_a_signal() {
        // 0..=128 are codes a program can return for itself. Reporting one of
        // them as a signal death would invent a cause.
        for code in 0..=128u64 {
            assert_eq!(signal_from_exit_code(code), None, "code {code}");
        }
    }

    #[test]
    fn an_out_of_range_signal_saturates_rather_than_wrapping() {
        // Wrapping would land inside the range a program uses for its own
        // status, which is the same confusion in the other direction.
        assert_eq!(exit_code_for_signal(128), 255);
        assert_eq!(exit_code_for_signal(u32::MAX), 255);
        assert_eq!(signal_from_exit_code(256), None);
    }
}
