//! Small messages, framed with their length and read field by field, every
//! field bounded: the wire format of SlateOS's small services -- the file
//! chooser (`gui/filechooser`), the credential service (`gui/credentials`).
//!
//! # A frame
//!
//! A four-byte little-endian length, then that many bytes of message, so a
//! reader over a transport that carries bytes knows when it has all of one.
//! A message is the protocol's mark, its version, the message's kind, and
//! its fields: integers little-endian, and text or bytes as a two-byte
//! length and then themselves.
//!
//! # Bounds
//!
//! A peer may be anyone. Every field a [`Reader`] reads names its own bound,
//! and a frame declaring more than the protocol's bound is refused by
//! [`decode`] before its bytes are buffered. A message that breaks any rule
//! -- a wrong mark, version or kind, a field past its bound, text that is
//! not UTF-8, bytes left over -- is [`Decoded::Malformed`], never read in
//! part. A [`Writer`] refuses ([`TooLarge`]) what the reader would.

use std::fmt;

/// What a protocol is: the mark its messages open with, and its version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Protocol {
    /// The bytes every message opens with.
    pub mark: &'static [u8],
    /// The version this side speaks; a message of another is malformed.
    pub version: u8,
    /// The longest frame either side reads.
    pub max_frame: usize,
}

/// A message that would break a bound, and so cannot be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge;

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a message larger than its protocol allows")
    }
}

impl std::error::Error for TooLarge {}

/// What reading a frame from the front of some bytes found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decoded<T> {
    /// Not all of a frame yet: read more.
    Partial,
    /// A message, and how many bytes its frame took.
    Complete(T, usize),
    /// Not a message the protocol sends: give up on the connection.
    Malformed,
}

/// A message being written: its fields after the header, and the frame's
/// length put in front when it is finished.
#[derive(Debug)]
pub struct Writer {
    protocol: Protocol,
    out: Vec<u8>,
}

impl Writer {
    /// A message of `kind` in `protocol`.
    #[must_use]
    pub fn new(protocol: Protocol, kind: u8) -> Self {
        let mut out = vec![0; 4];
        out.extend_from_slice(protocol.mark);
        out.push(protocol.version);
        out.push(kind);
        Self { protocol, out }
    }

    /// One byte.
    pub fn u8(&mut self, value: u8) {
        self.out.push(value);
    }

    /// A number up to [`u16::MAX`].
    pub fn u16(&mut self, value: u16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// A number up to [`u64::MAX`].
    pub fn u64(&mut self, value: u64) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// `bytes`, at most `max` of them.
    ///
    /// # Errors
    ///
    /// [`TooLarge`] past `max`, or past what two bytes can count.
    pub fn bytes(&mut self, bytes: &[u8], max: usize) -> Result<(), TooLarge> {
        if bytes.len() > max {
            return Err(TooLarge);
        }
        let length = u16::try_from(bytes.len()).map_err(|_| TooLarge)?;
        self.out.extend_from_slice(&length.to_le_bytes());
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    /// `text`, at most `max` bytes of it.
    ///
    /// # Errors
    ///
    /// As [`bytes`](Self::bytes).
    pub fn text(&mut self, text: &str, max: usize) -> Result<(), TooLarge> {
        self.bytes(text.as_bytes(), max)
    }

    /// The frame.
    ///
    /// # Errors
    ///
    /// [`TooLarge`] past the protocol's longest frame.
    pub fn finish(mut self) -> Result<Vec<u8>, TooLarge> {
        let length = self.out.len().saturating_sub(4);
        if length > self.protocol.max_frame {
            return Err(TooLarge);
        }
        let length = u32::try_from(length).map_err(|_| TooLarge)?;
        if let Some(prefix) = self.out.get_mut(..4) {
            prefix.copy_from_slice(&length.to_le_bytes());
        }
        Ok(self.out)
    }
}

/// A message being read, field by field; every read `None` past its end or
/// its bound.
#[derive(Debug)]
pub struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl<'b> Reader<'b> {
    /// The next `count` bytes.
    pub fn take(&mut self, count: usize) -> Option<&'b [u8]> {
        let end = self.at.checked_add(count)?;
        let taken = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(taken)
    }

    /// One byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    /// A number written by [`Writer::u16`].
    pub fn u16(&mut self) -> Option<u16> {
        let mut value = [0u8; 2];
        value.copy_from_slice(self.take(2)?);
        Some(u16::from_le_bytes(value))
    }

    /// A number written by [`Writer::u64`].
    pub fn u64(&mut self) -> Option<u64> {
        let mut value = [0u8; 8];
        value.copy_from_slice(self.take(8)?);
        Some(u64::from_le_bytes(value))
    }

    /// Bytes written by [`Writer::bytes`], at most `max` of them.
    pub fn bytes(&mut self, max: usize) -> Option<&'b [u8]> {
        let length = usize::from(self.u16()?);
        if length > max {
            return None;
        }
        self.take(length)
    }

    /// Text written by [`Writer::text`], at most `max` bytes, and UTF-8.
    pub fn text(&mut self, max: usize) -> Option<String> {
        std::str::from_utf8(self.bytes(max)?)
            .ok()
            .map(str::to_owned)
    }

    /// Whether every byte of the message has been read.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// The message of `kind` in `protocol` at the front of `bytes`, its fields
/// read by `read` -- which must read all of them.
pub fn decode<T>(
    bytes: &[u8],
    protocol: Protocol,
    kind: u8,
    read: impl FnOnce(&mut Reader<'_>) -> Option<T>,
) -> Decoded<T> {
    let Some(prefix) = bytes.get(..4) else {
        return Decoded::Partial;
    };
    let mut length = [0u8; 4];
    length.copy_from_slice(prefix);
    let Ok(length) = usize::try_from(u32::from_le_bytes(length)) else {
        return Decoded::Malformed;
    };
    if length > protocol.max_frame {
        return Decoded::Malformed;
    }
    let Some(end) = length.checked_add(4) else {
        return Decoded::Malformed;
    };
    let Some(message) = bytes.get(4..end) else {
        return Decoded::Partial;
    };
    let mut reader = Reader {
        bytes: message,
        at: 0,
    };
    let header = reader.take(protocol.mark.len()) == Some(protocol.mark)
        && reader.u8() == Some(protocol.version)
        && reader.u8() == Some(kind);
    if !header {
        return Decoded::Malformed;
    }
    match read(&mut reader) {
        Some(value) if reader.is_done() => Decoded::Complete(value, end),
        _ => Decoded::Malformed,
    }
}

#[cfg(test)]
mod tests;
