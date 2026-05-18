//! `<asm-generic/termbits.h>` — Terminal control character index constants.
//!
//! These constants define indices into the `c_cc[]` array of
//! `struct termios`, identifying special control characters
//! like interrupt, quit, erase, etc.

// ---------------------------------------------------------------------------
// Control character indices
// ---------------------------------------------------------------------------

/// Interrupt character (SIGINT).
pub const VINTR: u32 = 0;
/// Quit character (SIGQUIT).
pub const VQUIT: u32 = 1;
/// Erase character (backspace).
pub const VERASE: u32 = 2;
/// Kill character (erase line).
pub const VKILL: u32 = 3;
/// End-of-file character.
pub const VEOF: u32 = 4;
/// Inter-character timer (tenths of a second).
pub const VTIME: u32 = 5;
/// Minimum characters for non-canonical read.
pub const VMIN: u32 = 6;
/// Switch character (job control).
pub const VSWTC: u32 = 7;
/// Start output character (XON).
pub const VSTART: u32 = 8;
/// Stop output character (XOFF).
pub const VSTOP: u32 = 9;
/// Suspend character (SIGTSTP).
pub const VSUSP: u32 = 10;
/// End-of-line character.
pub const VEOL: u32 = 11;
/// Reprint character.
pub const VREPRINT: u32 = 12;
/// Discard character (toggle output discard).
pub const VDISCARD: u32 = 13;
/// Word erase character.
pub const VWERASE: u32 = 14;
/// Literal next character.
pub const VLNEXT: u32 = 15;
/// Second end-of-line character.
pub const VEOL2: u32 = 16;

/// Number of control characters in c_cc array.
pub const NCCS: u32 = 19;

// ---------------------------------------------------------------------------
// Disable character value
// ---------------------------------------------------------------------------

/// Value that disables a control character.
pub const CDISABLE: u8 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cc_indices_distinct() {
        let indices = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME,
            VMIN, VSWTC, VSTART, VSTOP, VSUSP, VEOL,
            VREPRINT, VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j]);
            }
        }
    }

    #[test]
    fn test_all_indices_less_than_nccs() {
        let indices = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME,
            VMIN, VSWTC, VSTART, VSTOP, VSUSP, VEOL,
            VREPRINT, VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for idx in &indices {
            assert!(*idx < NCCS);
        }
    }

    #[test]
    fn test_vintr_is_zero() {
        assert_eq!(VINTR, 0);
    }

    #[test]
    fn test_nccs() {
        assert_eq!(NCCS, 19);
    }

    #[test]
    fn test_cdisable() {
        assert_eq!(CDISABLE, 0);
    }
}
