//! `<linux/n_tty.h>` — N_TTY line discipline constants.
//!
//! N_TTY is the default line discipline that provides POSIX terminal
//! semantics: canonical mode (line editing with backspace, kill,
//! word-erase), echo processing, signal character handling (^C → SIGINT,
//! ^Z → SIGTSTP, ^\ → SIGQUIT), and flow control (^S/^Q). It
//! maintains input and output buffers with configurable sizes and
//! handles both raw and cooked modes.

// ---------------------------------------------------------------------------
// N_TTY buffer sizes
// ---------------------------------------------------------------------------

/// Input buffer size (4 KiB, one page).
pub const N_TTY_BUF_SIZE: u32 = 4096;
/// Echo buffer size (must be power of 2).
pub const N_TTY_ECHO_BUF_SIZE: u32 = 8192;

// ---------------------------------------------------------------------------
// Special character indices in c_cc array
// ---------------------------------------------------------------------------

/// Interrupt character (usually ^C) → SIGINT.
pub const VINTR: u32 = 0;
/// Quit character (usually ^\) → SIGQUIT.
pub const VQUIT: u32 = 1;
/// Erase character (usually Backspace/Delete).
pub const VERASE: u32 = 2;
/// Kill character (usually ^U, erases line).
pub const VKILL: u32 = 3;
/// End-of-file character (usually ^D).
pub const VEOF: u32 = 4;
/// TIME value (inter-character timer for non-canonical).
pub const VTIME: u32 = 5;
/// MIN value (minimum characters for non-canonical read).
pub const VMIN: u32 = 6;
/// Switch character (job control, unused on Linux).
pub const VSWTC: u32 = 7;
/// Start character (usually ^Q, XON).
pub const VSTART: u32 = 8;
/// Stop character (usually ^S, XOFF).
pub const VSTOP: u32 = 9;
/// Suspend character (usually ^Z) → SIGTSTP.
pub const VSUSP: u32 = 10;
/// End-of-line character (alternate, usually disabled).
pub const VEOL: u32 = 11;
/// Reprint character (usually ^R, redisplay line).
pub const VREPRINT: u32 = 12;
/// Discard character (usually ^O, toggle output discard).
pub const VDISCARD: u32 = 13;
/// Word-erase character (usually ^W, erase word).
pub const VWERASE: u32 = 14;
/// Literal next character (usually ^V, quote next char).
pub const VLNEXT: u32 = 15;
/// Second end-of-line character.
pub const VEOL2: u32 = 16;
/// Number of control characters in c_cc array.
pub const NCCS: u32 = 19;

// ---------------------------------------------------------------------------
// N_TTY processing states
// ---------------------------------------------------------------------------

/// Normal processing.
pub const N_TTY_STATE_NORMAL: u32 = 0;
/// Literal next (next character is not interpreted).
pub const N_TTY_STATE_LNEXT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_sizes_power_of_two() {
        assert!(N_TTY_BUF_SIZE.is_power_of_two());
        assert!(N_TTY_ECHO_BUF_SIZE.is_power_of_two());
    }

    #[test]
    fn test_cc_indices_distinct() {
        let indices = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME, VMIN, VSWTC, VSTART, VSTOP, VSUSP, VEOL,
            VREPRINT, VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for i in 0..indices.len() {
            assert!(indices[i] < NCCS);
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(N_TTY_STATE_NORMAL, N_TTY_STATE_LNEXT);
    }
}
