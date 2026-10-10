//! Bytes that must not outlive their use: a password, a key.
//!
//! A `Vec<u8>` that held a password leaves it behind three ways: dropped,
//! its allocation is freed as it was; grown, it moves to a bigger one and
//! frees the old one as it was; and with bytes taken out of its middle, the
//! ones shifted past its new end stay where they were. And a fourth way that
//! has nothing to do with memory: printed by `{:?}`, into a log or a failed
//! test's message.
//!
//! [`Secret`] closes all four. It is overwritten when dropped or cleared. It
//! never grows past the room it was given ([`Secret::with_room`]): an edit
//! that would need more is refused, so its bytes are never moved. What an
//! edit frees is overwritten. And `Debug` prints `Secret(..)`. Two secrets
//! compare equal in a time that depends on their lengths alone.
//!
//! What it cannot do: reach copies made before the bytes were put in it -- a
//! frame read off a connection, a key event's text -- or copies a caller
//! makes of [`as_bytes`](Secret::as_bytes), or a page the system swapped out.
//! It shortens how long a secret lingers; it does not promise that no copy
//! exists.
//!
//! Held by the toolkit's password field (`guitk::secretinput`) and the
//! credential service's protocol (`gui/credentials`), so that a password
//! typed into one and sent by the other is one kind of thing throughout.

use std::fmt;
use std::hint::black_box;
use std::ops::Range;

/// Bytes kept where they were put, overwritten when let go, never printed.
#[derive(Default)]
pub struct Secret {
    /// Never grown past its capacity; every byte past its length that it has
    /// held is zero.
    bytes: Vec<u8>,
}

impl Secret {
    /// `bytes`, kept where they are: their allocation becomes the secret's,
    /// and its room is their capacity.
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Nothing yet, with room reserved for `room` bytes.
    #[must_use]
    pub fn with_room(room: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(room),
        }
    }

    /// The bytes. A copy made of them is the caller's to look after.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// How many bytes it can hold without moving.
    #[must_use]
    pub fn room(&self) -> usize {
        self.bytes.capacity()
    }

    /// Put `bytes` in at `at`, moving what follows up: `false`, with nothing
    /// changed, when `at` is past the end or there is not the room.
    pub fn insert(&mut self, at: usize, bytes: &[u8]) -> bool {
        let fits = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_some_and(|len| len <= self.bytes.capacity());
        if at > self.bytes.len() || !fits {
            return false;
        }
        // Within the capacity, so neither moves the allocation: the new bytes
        // go on the end, and are turned round to `at`.
        self.bytes.extend_from_slice(bytes);
        if let Some(after) = self.bytes.get_mut(at..) {
            after.rotate_right(bytes.len());
        }
        true
    }

    /// Take the bytes in `range` out, moving what follows down, and overwrite
    /// the ones that frees at the end: `false`, with nothing changed, for a
    /// range not inside it.
    pub fn remove(&mut self, range: Range<usize>) -> bool {
        let Range { start, end } = range;
        let Some(count) = end.checked_sub(start) else {
            return false;
        };
        let Some(kept) = self.bytes.len().checked_sub(count) else {
            return false;
        };
        if end > self.bytes.len() {
            return false;
        }
        if let Some(from) = self.bytes.get_mut(start..) {
            from.rotate_left(count);
        }
        if let Some(freed) = self.bytes.get_mut(kept..) {
            freed.fill(0);
        }
        black_box(&self.bytes);
        self.bytes.truncate(kept);
        true
    }

    /// Empty it, overwriting what it held. Its room stays.
    pub fn clear(&mut self) {
        self.bytes.fill(0);
        // So the overwrite is not taken for a store nothing reads and left
        // out: the zeros are observed.
        black_box(&self.bytes);
        self.bytes.clear();
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

/// A copy with the same room, so it too can be edited without moving.
impl Clone for Secret {
    fn clone(&self) -> Self {
        let mut bytes = Vec::with_capacity(self.bytes.capacity());
        bytes.extend_from_slice(&self.bytes);
        Self { bytes }
    }
}

/// Equal in a time that depends on the lengths alone: every byte of two
/// secrets of one length is compared, wherever the first difference is.
impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        if self.bytes.len() != other.bytes.len() {
            return false;
        }
        let differ = self
            .bytes
            .iter()
            .zip(&other.bytes)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b));
        black_box(differ) == 0
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::Secret;

    /// **Bytes go in anywhere there is room, and never move it**: at the
    /// front, the middle and the end; refused past the end or the room,
    /// with nothing changed.
    #[test]
    fn bytes_go_in_where_there_is_room_and_never_move_it() {
        let mut secret = Secret::with_room(8);
        let home = secret.as_bytes().as_ptr();
        assert!(secret.insert(0, b"ace"));
        assert!(secret.insert(1, b"b"));
        assert!(secret.insert(4, b"!"));
        assert!(secret.insert(0, b">"));
        assert_eq!(secret.as_bytes(), b">abce!");
        assert!(!secret.insert(7, b"x"));
        assert!(!secret.insert(0, b"xyz"));
        assert_eq!(secret.as_bytes(), b">abce!");
        assert!(secret.insert(6, b"yz"));
        assert_eq!(secret.as_bytes(), b">abce!yz");
        assert_eq!((secret.len(), secret.room()), (8, 8));
        assert_eq!(secret.as_bytes().as_ptr(), home);
        // A secret made from bytes has their capacity for room.
        let mut made = Secret::new(Vec::with_capacity(4));
        assert!(made.insert(0, b"1234") && !made.insert(4, b"5"));
    }

    /// **Bytes come out of any range inside it, and nowhere else.**
    #[test]
    fn bytes_come_out_of_any_range_inside_it() {
        let mut secret = Secret::with_room(8);
        assert!(secret.insert(0, b"abcdef"));
        let home = secret.as_bytes().as_ptr();
        assert!(secret.remove(1..3));
        assert_eq!(secret.as_bytes(), b"adef");
        assert!(secret.remove(3..4));
        assert_eq!(secret.as_bytes(), b"ade");
        assert!(secret.remove(0..0));
        assert_eq!(secret.as_bytes(), b"ade");
        #[allow(clippy::reversed_empty_ranges)]
        let backwards = 2..1;
        assert!(!secret.remove(backwards));
        assert!(!secret.remove(2..4));
        assert!(!secret.remove(4..4));
        assert_eq!(secret.as_bytes(), b"ade");
        assert!(secret.remove(0..3));
        assert!(secret.is_empty());
        assert_eq!(secret.as_bytes().as_ptr(), home);
        assert_eq!(secret.room(), 8);
    }

    /// **Cleared, it is empty and keeps its room.**
    #[test]
    fn cleared_it_is_empty_and_keeps_its_room() {
        let mut secret = Secret::with_room(4);
        assert!(secret.insert(0, b"abc"));
        secret.clear();
        assert!(secret.is_empty());
        assert_eq!(secret.room(), 4);
        assert!(secret.insert(0, b"wxyz"));
    }

    /// **A copy is equal and has the same room; secrets compare by their
    /// bytes; none is ever printed.**
    #[test]
    fn a_copy_is_equal_and_none_is_printed() {
        let mut secret = Secret::with_room(16);
        assert!(secret.insert(0, b"hunter2"));
        let copy = secret.clone();
        assert_eq!(copy, secret);
        assert_eq!(copy.room(), 16);
        assert_ne!(Secret::new(b"hunter3".to_vec()), secret);
        assert_ne!(Secret::new(b"hunter".to_vec()), secret);
        assert_eq!(Secret::new(b"hunter2".to_vec()), secret);
        assert_eq!(format!("{secret:?}"), "Secret(..)");
        assert_eq!(Secret::default().room(), 0);
    }
}
