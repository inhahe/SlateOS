//! What `write_output` (util-linux 2.39.3, `misc-utils/logger.c`) sends and
//! what it echoes to stderr, as bytes. Delivery is the caller's.

/// The byte sequences `write_output` assembles for one message, in upstream's
/// iovec order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// What goes to the socket: `[octet count] header message [\n]`.
    pub wire: Vec<u8>,
    /// What `--stderr` writes: the same, terminated as upstream terminates
    /// it (see [`frame`]).
    pub stderr: Vec<u8>,
}

/// Build one message's frame.
///
/// * `octet_count`: RFC 6587 octet counting, `"%zu "` of header + message
///   lengths (`--octet-count`).
/// * `tcp_newline`: step 4 -- a stream connection without octet counting
///   gets a `\n` so the receiver can find the end. Only when a connection
///   exists and `--no-act` is off; the caller decides.
///
/// # The stderr terminator reads the wrong byte, faithfully
///
/// Upstream adds `\n` for stderr unless `iovec_memcmp(iov, iovlen, "\n", 1)`
/// says the last piece already is one -- and that macro compares the FIRST
/// byte of the last iovec, not its last byte. The last piece is the message
/// (or step 4's `"\n"`), so a message that STARTS with a newline is echoed
/// with no newline at the end, and one that merely ends with one gets a
/// second. `logger -s $'\nx'` shows it; the tests pin it.
#[must_use]
pub fn frame(header: &[u8], msg: &[u8], octet_count: bool, tcp_newline: bool) -> Frame {
    let msg = msg.split(|&b| b == 0).next().unwrap_or(msg);
    let mut pieces: Vec<&[u8]> = Vec::with_capacity(4);
    let octets;
    if octet_count {
        octets = format!("{} ", header.len().saturating_add(msg.len()));
        pieces.push(octets.as_bytes());
    }
    pieces.push(header);
    pieces.push(msg);
    let mut wire_pieces = pieces.clone();
    if tcp_newline {
        wire_pieces.push(b"\n");
    }
    let wire = wire_pieces.concat();

    // stderr sees the pieces the socket saw, plus maybe a terminator.
    let mut err_pieces = wire_pieces;
    let last_starts_with_newline = err_pieces.last().is_some_and(|p| p.first() == Some(&b'\n'));
    if !last_starts_with_newline {
        err_pieces.push(b"\n");
    }
    Frame {
        wire,
        stderr: err_pieces.concat(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn a_local_frame_and_its_stderr_copy() {
        let f = frame(b"<13>Sep 26 06:37:16 t: ", b"hi", false, false);
        assert_eq!(f.wire, b"<13>Sep 26 06:37:16 t: hi");
        assert_eq!(f.stderr, b"<13>Sep 26 06:37:16 t: hi\n");
    }

    #[test]
    fn octet_counting_prefixes_the_length_of_header_and_message() {
        let f = frame(b"<13>1 - - t - - - ", b"hi", true, false);
        assert_eq!(f.wire, b"20 <13>1 - - t - - - hi");
    }

    #[test]
    fn a_stream_without_octet_counting_ends_each_message_with_a_newline() {
        let f = frame(b"H ", b"m", false, true);
        assert_eq!(f.wire, b"H m\n");
        // The last piece is now "\n", so stderr adds nothing.
        assert_eq!(f.stderr, b"H m\n");
    }

    #[test]
    fn a_message_that_starts_with_a_newline_is_echoed_unterminated() {
        let f = frame(b"H ", b"\nx", false, false);
        assert_eq!(f.stderr, b"H \nx");
    }

    #[test]
    fn a_message_that_ends_with_a_newline_gets_a_second_one() {
        let f = frame(b"H ", b"x\n", false, false);
        assert_eq!(f.stderr, b"H x\n\n");
    }

    #[test]
    fn an_empty_message_is_terminated() {
        let f = frame(b"H ", b"", false, false);
        assert_eq!(f.stderr, b"H \n");
    }
}
