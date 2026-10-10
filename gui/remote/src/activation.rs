//! Activation tokens: how a program the user started may take the keyboard
//! (design-decisions 1386).
//!
//! The compositor lets a program's window take the keyboard on that program's
//! own say-so only when the user's latest action was in that program, or the
//! program presents the user's word that they asked for it. A program the user
//! *started* -- from the start menu, a file manager, a link in another program
//! -- did not receive that action: its launcher did. So the launcher asks the
//! compositor for a token ([`RequestBody::GetActivationToken`]), the
//! compositor records which of the user's actions the token stands for, and
//! the launcher hands the token to the program it starts in the environment
//! variable [`ACTIVATION_TOKEN_ENV`]. The program presents it with its first
//! window ([`RequestBody::UseActivationToken`]), or with a window it already
//! has that it was asked to bring forward ([`RequestBody::Activate`]).
//!
//! It is Wayland's xdg-activation-v1 with the compositor's own bookkeeping:
//! a token is good once, and only while the user has done nothing in the
//! window holding the keyboard since the action it stands for -- so a program
//! that is slow to start, while the user has gone on typing elsewhere, opens
//! without taking the keys.
//!
//! **Sixteen random bytes, not a counter.** Any program may present a token,
//! because the program that uses one is by design not the one that asked for
//! it. A guessable token could be presented by a program racing the one it was
//! drawn for, and would take the keyboard in its place.
//!
//! [`RequestBody::GetActivationToken`]: crate::control::RequestBody::GetActivationToken
//! [`RequestBody::UseActivationToken`]: crate::control::RequestBody::UseActivationToken
//! [`RequestBody::Activate`]: crate::control::RequestBody::Activate

use std::fmt;
use std::fmt::Write as _;

/// The environment variable a launcher hands a program its activation token
/// in, as [`ActivationToken::to_text`] writes it.
///
/// A launcher sets it on the command it starts, never in its own environment;
/// a program reads it once, when it opens its first window.
pub const ACTIVATION_TOKEN_ENV: &str = "SLATE_ACTIVATION_TOKEN";

/// Length of a token's text form: two hexadecimal digits a byte.
const TEXT_LEN: usize = 32;

/// The compositor's word that the user started a program, or asked for one of
/// its windows: sixteen bytes it drew from the kernel's random source.
///
/// Opaque. Its `Debug` form does not show the bytes, so a token in a logged
/// request is not a token anyone reading the log can present.
#[derive(Clone, Copy, Eq)]
pub struct ActivationToken([u8; ActivationToken::LEN]);

impl ActivationToken {
    /// Bytes in a token.
    pub const LEN: usize = 16;

    /// A token from its bytes -- as the compositor draws one, or the wire
    /// carries one.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    /// The token's bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; Self::LEN] {
        self.0
    }

    /// The token as text, for [`ACTIVATION_TOKEN_ENV`]: 32 lowercase
    /// hexadecimal digits.
    #[must_use]
    pub fn to_text(self) -> String {
        let mut text = String::with_capacity(TEXT_LEN);
        for byte in self.0 {
            // Writing into a `String` cannot fail; `fmt::Write` only says it
            // might because other writers can.
            let _ = write!(text, "{byte:02x}");
        }
        text
    }

    /// Read a token's text form: exactly 32 hexadecimal digits, either case.
    ///
    /// Bytes, because that is what an environment variable is. Anything else
    /// is `None` -- a sign, a space, a stray newline -- rather than a token
    /// read leniently: a launcher that wrote something else did not write a
    /// token.
    #[must_use]
    pub fn from_text(text: &[u8]) -> Option<Self> {
        if text.len() != TEXT_LEN || !text.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let mut bytes = [0u8; Self::LEN];
        for (byte, pair) in bytes.iter_mut().zip(text.chunks_exact(2)) {
            // Both checked above: two ASCII hex digits are UTF-8, and parse.
            // `from_str_radix` alone would also take a leading `+`.
            let pair = std::str::from_utf8(pair).ok()?;
            *byte = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl PartialEq for ActivationToken {
    /// Every byte compared whatever the first differing one, so that how long
    /// a comparison takes says nothing about how much of a guess was right.
    /// The compositor compares a presented token against each it has
    /// outstanding.
    fn eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |differ, (a, b)| differ | (a ^ b))
            == 0
    }
}

impl fmt::Debug for ActivationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationToken(..)")
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly at the line that did
    // it. The defensive lints guard code that runs on a user's data, not this.
    #![allow(clippy::indexing_slicing)]

    use super::*;

    const SAMPLE: [u8; 16] = [
        0x00, 0x01, 0x7f, 0x80, 0xff, 0x10, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
        0xde,
    ];

    #[test]
    fn a_token_survives_its_text_form() {
        let token = ActivationToken::from_bytes(SAMPLE);
        let text = token.to_text();
        assert_eq!(text, "00017f80ff10abcdef123456789abcde");
        assert_eq!(ActivationToken::from_text(text.as_bytes()), Some(token));
        assert_eq!(
            ActivationToken::from_text(b"00017F80FF10ABCDEF123456789ABCDE"),
            Some(token),
            "upper case is the same token"
        );
    }

    #[test]
    fn anything_but_32_hex_digits_is_no_token() {
        for text in [
            &b""[..],
            b"00017f80ff10abcdef123456789abcd",
            b"00017f80ff10abcdef123456789abcdef",
            b"00017f80ff10abcdef123456789abcd\n",
            b" 0017f80ff10abcdef123456789abcde",
            // `from_str_radix` alone would read "+0" as zero.
            b"+0017f80ff10abcdef123456789abcde",
            b"g0017f80ff10abcdef123456789abcde",
            b"00017f80ff10abcdef123456789abc\xc3\xa9",
        ] {
            assert_eq!(ActivationToken::from_text(text), None, "{text:?}");
        }
    }

    #[test]
    fn tokens_are_equal_only_when_every_byte_is() {
        let token = ActivationToken::from_bytes(SAMPLE);
        assert_eq!(token, ActivationToken::from_bytes(SAMPLE));
        for at in 0..ActivationToken::LEN {
            let mut other = SAMPLE;
            other[at] ^= 1;
            assert_ne!(token, ActivationToken::from_bytes(other), "byte {at}");
        }
    }

    #[test]
    fn a_logged_token_shows_none_of_its_bytes() {
        let shown = format!("{:?}", ActivationToken::from_bytes(SAMPLE));
        assert_eq!(shown, "ActivationToken(..)");
    }
}
