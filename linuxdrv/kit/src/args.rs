//! A native process's arguments, as `SYS_PROCESS_GET_ARGS` (519) delivers
//! them: a header of four little-endian `u32`s -- the argument count, the
//! environment count, and the byte lengths of the two packed lists -- then
//! the arguments, each NUL-terminated, then the environment.
//!
//! A native program is not given them on its stack, as a Linux one is: it
//! asks for them, once ([`crate::sys`]).

/// The header's size in bytes.
pub const HEADER: usize = 16;

/// The arguments in `packed`, each without its terminator, in order. What
/// the header claims beyond what is there -- more arguments than strings,
/// lengths past the end -- is not read: the walk stops at the end of the
/// argument bytes the header gives and the buffer holds, whichever is
/// first, and a string missing its terminator there is not yielded.
pub fn arguments(packed: &[u8]) -> impl Iterator<Item = &[u8]> {
    let word = |i: usize| -> usize {
        packed
            .get(i..i.saturating_add(4))
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, |b| usize::try_from(u32::from_le_bytes(b)).unwrap_or(0))
    };
    let count = word(0);
    let len = word(8);
    let body = packed
        .get(HEADER..)
        .unwrap_or(&[])
        .get(..len)
        .or_else(|| packed.get(HEADER..))
        .unwrap_or(&[]);
    body.split_inclusive(|&b| b == 0)
        .filter(|s| s.last() == Some(&0))
        .map(|s| s.get(..s.len().saturating_sub(1)).unwrap_or(&[]))
        .take(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(argv: &[&[u8]], claimed: Option<(u32, u32)>) -> Vec<u8> {
        let body: Vec<u8> = argv
            .iter()
            .flat_map(|a| a.iter().copied().chain([0]))
            .collect();
        let (count, len) = claimed.unwrap_or((
            u32::try_from(argv.len()).unwrap_or(0),
            u32::try_from(body.len()).unwrap_or(0),
        ));
        let mut out = Vec::new();
        for w in [count, 0, len, 0] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend(body);
        out
    }

    #[test]
    fn the_arguments_come_out_in_order() {
        let buf = packed(&[b"hello", b"stop", b""], None);
        let got: Vec<&[u8]> = arguments(&buf).collect();
        assert_eq!(got, [&b"hello"[..], b"stop", b""]);
    }

    #[test]
    fn what_the_header_claims_beyond_the_buffer_is_not_read() {
        // More arguments than strings, and a length past the end.
        let buf = packed(&[b"one", b"two"], Some((9, 1000)));
        let got: Vec<&[u8]> = arguments(&buf).collect();
        assert_eq!(got, [&b"one"[..], b"two"]);
        // A last string cut off before its terminator is not yielded.
        let mut cut = packed(&[b"one", b"two"], None);
        cut.pop();
        let got: Vec<&[u8]> = arguments(&cut).collect();
        assert_eq!(got, [&b"one"[..]]);
    }

    #[test]
    fn the_environment_is_not_an_argument_and_nothing_is_nothing() {
        let mut buf = packed(&[b"a"], None);
        buf.extend_from_slice(b"PATH=/bin\0");
        assert_eq!(arguments(&buf).count(), 1);
        assert_eq!(arguments(&[]).count(), 0);
        assert_eq!(arguments(&[1, 0, 0]).count(), 0);
    }
}
