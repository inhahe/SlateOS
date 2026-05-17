//! `<linux/termios.h>` — Terminal I/O (termios) constants.
//!
//! termios defines the interface for controlling terminal behavior:
//! input/output processing, character echoing, signal generation,
//! baud rates, and special characters (^C, ^Z, ^D, etc.). The
//! tcgetattr()/tcsetattr() functions get/set these parameters.
//! Terminal settings are inherited across fork/exec but each process
//! group has its own foreground terminal association.

// ---------------------------------------------------------------------------
// termios c_iflag (input mode flags)
// ---------------------------------------------------------------------------

/// Ignore BREAK condition on input.
pub const IGNBRK: u32 = 0o000001;
/// Signal on BREAK (generate SIGINT).
pub const BRKINT: u32 = 0o000002;
/// Ignore characters with parity errors.
pub const IGNPAR: u32 = 0o000004;
/// Mark parity and framing errors.
pub const PARMRK: u32 = 0o000010;
/// Enable input parity checking.
pub const INPCK: u32 = 0o000020;
/// Strip 8th bit off input characters.
pub const ISTRIP: u32 = 0o000040;
/// Map NL to CR on input.
pub const INLCR: u32 = 0o000100;
/// Ignore CR on input.
pub const IGNCR: u32 = 0o000200;
/// Map CR to NL on input.
pub const ICRNL: u32 = 0o000400;
/// Enable XON/XOFF flow control on output.
pub const IXON: u32 = 0o002000;
/// Enable XON/XOFF flow control on input.
pub const IXOFF: u32 = 0o010000;
/// Enable any character to restart output.
pub const IXANY: u32 = 0o004000;
/// Ring bell when input queue is full.
pub const IMAXBEL: u32 = 0o020000;
/// Input is UTF-8 encoded.
pub const IUTF8: u32 = 0o040000;

// ---------------------------------------------------------------------------
// termios c_oflag (output mode flags)
// ---------------------------------------------------------------------------

/// Perform output processing.
pub const OPOST: u32 = 0o000001;
/// Map NL to CR-NL on output.
pub const ONLCR: u32 = 0o000004;
/// Map CR to NL on output.
pub const OCRNL: u32 = 0o000010;
/// Don't output CR at column 0.
pub const ONOCR: u32 = 0o000020;
/// Don't output CR.
pub const ONLRET: u32 = 0o000040;

// ---------------------------------------------------------------------------
// termios c_lflag (local mode flags)
// ---------------------------------------------------------------------------

/// Enable signals (SIGINT, SIGQUIT, SIGTSTP).
pub const ISIG: u32 = 0o000001;
/// Enable canonical (line-by-line) mode.
pub const ICANON: u32 = 0o000002;
/// Echo input characters.
pub const ECHO: u32 = 0o000010;
/// Echo erase as backspace-space-backspace.
pub const ECHOE: u32 = 0o000020;
/// Echo kill by erasing the line.
pub const ECHOK: u32 = 0o000040;
/// Echo NL even if ECHO is off.
pub const ECHONL: u32 = 0o000100;
/// Disable flush after interrupt/quit.
pub const NOFLSH: u32 = 0o000200;
/// Stop background processes that write to terminal.
pub const TOSTOP: u32 = 0o000400;
/// Enable extended input processing.
pub const IEXTEN: u32 = 0o100000;

// ---------------------------------------------------------------------------
// termios tcsetattr() action values
// ---------------------------------------------------------------------------

/// Apply changes immediately.
pub const TCSANOW: u32 = 0;
/// Apply after all output is transmitted.
pub const TCSADRAIN: u32 = 1;
/// Apply after output, discard pending input.
pub const TCSAFLUSH: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iflag_distinct() {
        let flags = [
            IGNBRK, BRKINT, IGNPAR, PARMRK, INPCK, ISTRIP,
            INLCR, IGNCR, ICRNL, IXON, IXOFF, IXANY,
            IMAXBEL, IUTF8,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_lflag_distinct() {
        let flags = [ISIG, ICANON, ECHO, ECHOE, ECHOK, ECHONL, NOFLSH, TOSTOP, IEXTEN];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_tcsetattr_actions_distinct() {
        let actions = [TCSANOW, TCSADRAIN, TCSAFLUSH];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }
}
