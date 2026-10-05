//! `signames.c`: `--signames`, a signal mask written as names.
//!
//! Upstream reads the mask with `sscanf (sig, "%" PRIu64, …)` -- *decimal* --
//! although `/proc` writes it in hexadecimal. So `0000000000010000` is read
//! as ten thousand, whose set bits name `TRAP,KILL,USR1,SEGV,ALRM`, and a mask
//! with a letter in it is read up to the letter. That is kept: it is what
//! `ps --signames` prints. A mask with no leading digits is not a number,
//! and the column falls back to hexadecimal.

use coreutils::procps::scanf::Scan;

/// glibc's `sigabbrev_np` for 1 to 31, and its real-time naming above.
fn abbrev(sig: u32) -> String {
    const NAMES: [&str; 31] = [
        "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1", "SEGV", "USR2",
        "PIPE", "ALRM", "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU", "URG",
        "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "POLL", "PWR", "SYS",
    ];
    // glibc reserves 32 and 33 for itself: `SIGRTMIN` is 34, `SIGRTMAX` 64.
    const SIGRTMIN: u32 = 34;
    const SIGRTMAX: u32 = 64;
    if sig == 0 || sig >= 65 {
        return format!("BOGUS_{:02}", i64::from(sig).wrapping_sub(65));
    }
    if sig < 32
        && let Some(name) = usize::try_from(sig.saturating_sub(1))
            .ok()
            .and_then(|i| NAMES.get(i))
    {
        return (*name).to_string();
    }
    if sig >= SIGRTMIN {
        if sig == SIGRTMIN {
            return "RTMIN".to_string();
        }
        if sig == SIGRTMAX {
            return "RTMAX".to_string();
        }
        return format!("RTMIN+{:02}", sig.wrapping_sub(SIGRTMIN));
    }
    format!("LIBC+{:02}", sig.wrapping_sub(32))
}

/// `print_signame`: the names of the signals in `sig` (read as decimal),
/// separated by commas, in at most `len` bytes -- a `+` where the next name
/// would not fit, and `-` for none. Empty when `sig` is not a number at all.
#[must_use]
pub fn print_signame(sig: &[u8], len: i64) -> Vec<u8> {
    let Some(mask) = Scan::new(sig).ulong() else {
        return Vec::new();
    };
    let mut out: Vec<u8> = Vec::new();
    let mut len = len;
    for i in 1..65u32 {
        if mask & 1u64.wrapping_shl(i.wrapping_sub(1)) == 0 {
            continue;
        }
        let name = abbrev(i);
        let n = i64::try_from(name.len()).unwrap_or(i64::MAX);
        if n.saturating_add(1) >= len {
            out.push(b'+');
            break;
        }
        if !out.is_empty() {
            out.push(b',');
            len = len.saturating_sub(1);
        }
        out.extend_from_slice(name.as_bytes());
        len = len.saturating_sub(n);
    }
    if out.is_empty() {
        out.push(b'-');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_are_read_as_decimal() {
        // Ten thousand: bits 4, 8, 9, 10 and 13.
        assert_eq!(
            print_signame(b"0000000000010000", 100),
            b"TRAP,KILL,USR1,SEGV,ALRM"
        );
        assert_eq!(print_signame(b"0000000000000000", 100), b"-");
        assert_eq!(print_signame(b"zz", 100), b"");
        // Stops at the letter: 4 is QUIT.
        assert_eq!(print_signame(b"000000004b813efb", 100), b"QUIT");
    }

    #[test]
    fn a_name_that_does_not_fit_is_a_plus() {
        assert_eq!(print_signame(b"3", 100), b"HUP,INT");
        assert_eq!(print_signame(b"3", 6), b"HUP+");
        assert_eq!(print_signame(b"1", 3), b"+");
    }

    #[test]
    fn real_time_names() {
        assert_eq!(abbrev(32), "LIBC+00");
        assert_eq!(abbrev(34), "RTMIN");
        assert_eq!(abbrev(35), "RTMIN+01");
        assert_eq!(abbrev(64), "RTMAX");
    }
}
