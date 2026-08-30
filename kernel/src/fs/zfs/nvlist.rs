//! A read-only parser for XDR-encoded name/value lists.
//!
//! The pool configuration in a vdev label is an nvlist: a self-describing,
//! recursively nestable map from names to typed values. It is the one part of
//! the on-disk format that is *not* written in the pool's native byte order —
//! it is always XDR, hence always big-endian — because it is what tells an
//! importing host what the pool's byte order is. A chicken-and-egg problem
//! solved by fixing the byte order of the answer.
//!
//! # Why this parses lazily instead of building a map
//!
//! Every nvpair carries its own encoded length, so an unknown or unsupported
//! type can be stepped over without understanding it. That turns "parse the
//! whole config" into "scan for the four names we need", which means a pool
//! using a feature whose config key we have never heard of still mounts. The
//! alternative — decoding every pair into an owned `BTreeMap<String, Value>` —
//! would allocate a few kilobytes per mount to answer four questions, and
//! would fail the whole parse on the first type it did not model.
//!
//! # Scope
//!
//! `u64`, `string`, and nested `nvlist` are decoded. Everything else is
//! skipped by length. That is exactly the set the vdev-label config needs.

use crate::error::{KernelError, KernelResult};

use super::raw::{read_u32_be, read_u64_be};

/// `NV_ENCODE_XDR`. The only encoding a vdev label uses.
pub const NV_ENCODE_XDR: u8 = 1;
/// `NV_ENCODE_NATIVE`, which never appears on disk.
pub const NV_ENCODE_NATIVE: u8 = 0;

/// `NV_BIG_ENDIAN`, as recorded in the nvlist header's second byte.
pub const NV_BIG_ENDIAN: u8 = 0;
/// `NV_LITTLE_ENDIAN`.
pub const NV_LITTLE_ENDIAN: u8 = 1;

/// Bytes of fixed header before the first nvpair of a *top-level* nvlist:
/// encoding(1) + endian(1) + reserved(2) + version(4) + nvflag(4).
const TOP_HEADER_LEN: usize = 12;

/// Bytes of fixed header before the first nvpair of a *nested* nvlist:
/// version(4) + nvflag(4). A nested list has no encoding byte because it
/// inherits the outer list's.
const NESTED_HEADER_LEN: usize = 8;

/// Refuse to follow more than this many levels of nesting. The vdev-label
/// config is three deep (`config` -> `vdev_tree` -> `children[n]`); a stream
/// claiming more than this is malformed or hostile, and recursing on it would
/// exhaust the kernel stack.
const MAX_DEPTH: u32 = 16;

// `data_type_t` — only the members this parser decodes are named; the rest are
// skipped by encoded length and never need a constant.
pub(super) const DATA_TYPE_UINT64: u32 = 8;
pub(super) const DATA_TYPE_STRING: u32 = 9;
pub(super) const DATA_TYPE_NVLIST: u32 = 19;
pub(super) const DATA_TYPE_NVLIST_ARRAY: u32 = 20;

/// A decoded nvpair value, borrowing the buffer it was found in.
#[derive(Debug, Clone, Copy)]
pub enum Value<'a> {
    /// A `DATA_TYPE_UINT64`.
    U64(u64),
    /// A `DATA_TYPE_STRING`, as raw bytes.
    ///
    /// Not a `&str`: an nvlist string is a NUL-free byte sequence with no
    /// declared encoding, and forcing UTF-8 on it here would be the same
    /// silent corruption the path rules forbid.
    Str(&'a [u8]),
    /// A `DATA_TYPE_NVLIST`, ready to be walked with [`NvList::nested`].
    Nested(NvList<'a>),
    /// A `DATA_TYPE_NVLIST_ARRAY` — the first element, plus the element count.
    ///
    /// The first element is all the vdev tree needs: the only array it holds is
    /// `children`, and the only thing read out of a child is `ashift`, which
    /// every leg of a mirror shares by construction. The count is what
    /// distinguishes a leaf vdev from a parent.
    NestedArray(NvList<'a>, u32),
}

/// A view of an encoded nvlist: the buffer, and where its pairs begin.
#[derive(Debug, Clone, Copy)]
pub struct NvList<'a> {
    buf: &'a [u8],
    first_pair: usize,
    depth: u32,
}

impl<'a> NvList<'a> {
    /// Wrap a top-level encoded nvlist, validating the four-byte header.
    ///
    /// # Errors
    ///
    /// - [`KernelError::InvalidArgument`] if the buffer is too short or the
    ///   encoding byte is not `NV_ENCODE_XDR`.
    /// - [`KernelError::NotSupported`] for `NV_ENCODE_NATIVE`, which is
    ///   host-format and cannot be read portably — it does not occur on disk,
    ///   so seeing it means the buffer is not a label nvlist at all.
    pub fn new(buf: &'a [u8]) -> KernelResult<Self> {
        let encoding = *buf.first().ok_or(KernelError::InvalidArgument)?;
        let endian = *buf.get(1).ok_or(KernelError::InvalidArgument)?;
        if encoding == NV_ENCODE_NATIVE {
            return Err(KernelError::NotSupported);
        }
        if encoding != NV_ENCODE_XDR {
            return Err(KernelError::InvalidArgument);
        }
        if endian != NV_BIG_ENDIAN && endian != NV_LITTLE_ENDIAN {
            return Err(KernelError::InvalidArgument);
        }
        if buf.len() < TOP_HEADER_LEN {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self {
            buf,
            first_pair: TOP_HEADER_LEN,
            depth: 0,
        })
    }

    /// Look up `name` and decode its value.
    ///
    /// Returns `None` when the name is absent or its type is one this parser
    /// does not decode — the two are deliberately not distinguished, because
    /// every caller here treats them the same way (fall back to a default, or
    /// fail the mount with a message about the missing key).
    #[must_use]
    pub fn get(&self, name: &[u8]) -> Option<Value<'a>> {
        let mut off = self.first_pair;
        loop {
            let encoded = read_u32_be(self.buf, off).ok()?;
            if encoded == 0 {
                return None; // End-of-list marker.
            }
            let encoded = usize::try_from(encoded).ok()?;
            if encoded < 8 {
                return None; // Would not advance; malformed.
            }

            // encoded_size(4) decoded_size(4) name(xdr string) type(4) nelem(4)
            let name_off = off.checked_add(8)?;
            let (pair_name, after_name) = read_xdr_bytes(self.buf, name_off)?;
            if pair_name == name {
                let dtype = read_u32_be(self.buf, after_name).ok()?;
                let nelem = read_u32_be(self.buf, after_name.checked_add(4)?).ok()?;
                let value_off = after_name.checked_add(8)?;
                return self.decode(dtype, nelem, value_off);
            }

            off = off.checked_add(encoded)?;
        }
    }

    /// [`Self::get`], narrowed to a `u64`.
    #[must_use]
    pub fn get_u64(&self, name: &[u8]) -> Option<u64> {
        match self.get(name)? {
            Value::U64(v) => Some(v),
            _ => None,
        }
    }

    /// [`Self::get`], narrowed to a string's bytes.
    #[must_use]
    pub fn get_str(&self, name: &[u8]) -> Option<&'a [u8]> {
        match self.get(name)? {
            Value::Str(v) => Some(v),
            _ => None,
        }
    }

    /// [`Self::get`], narrowed to a nested list.
    ///
    /// An `nvlist_array` of one element answers here too, because the vdev
    /// tree stores a single child either way depending on pool layout and a
    /// caller that had to distinguish them would just write this match.
    #[must_use]
    pub fn nested(&self, name: &[u8]) -> Option<NvList<'a>> {
        match self.get(name)? {
            Value::Nested(v) | Value::NestedArray(v, _) => Some(v),
            _ => None,
        }
    }

    /// Number of elements in the `nvlist_array` named `name`, or `None` if it
    /// is not one.
    ///
    /// This is how a parent vdev is told from a leaf: a leaf has no `children`
    /// key at all. `parse_config` uses it to reject a leaf that claims children
    /// and a mirror that claims none — both self-contradictory configs.
    #[must_use]
    pub fn nested_array_len(&self, name: &[u8]) -> Option<u32> {
        match self.get(name)? {
            Value::NestedArray(_, n) => Some(n),
            _ => None,
        }
    }

    fn decode(&self, dtype: u32, nelem: u32, value_off: usize) -> Option<Value<'a>> {
        match dtype {
            DATA_TYPE_UINT64 => read_u64_be(self.buf, value_off).ok().map(Value::U64),
            DATA_TYPE_STRING => read_xdr_bytes(self.buf, value_off).map(|(s, _)| Value::Str(s)),
            DATA_TYPE_NVLIST | DATA_TYPE_NVLIST_ARRAY => {
                if self.depth >= MAX_DEPTH {
                    return None;
                }
                let first = value_off.checked_add(NESTED_HEADER_LEN)?;
                if first > self.buf.len() {
                    return None;
                }
                let child = NvList {
                    buf: self.buf,
                    first_pair: first,
                    depth: self.depth.wrapping_add(1),
                };
                if dtype == DATA_TYPE_NVLIST {
                    Some(Value::Nested(child))
                } else {
                    Some(Value::NestedArray(child, nelem))
                }
            }
            _ => None,
        }
    }
}

/// Read an XDR opaque/string: a big-endian length, the bytes, then padding to
/// a four-byte boundary. Returns the bytes and the offset just past the
/// padding.
fn read_xdr_bytes(buf: &[u8], off: usize) -> Option<(&[u8], usize)> {
    let len = usize::try_from(read_u32_be(buf, off).ok()?).ok()?;
    let start = off.checked_add(4)?;
    let end = start.checked_add(len)?;
    let bytes = buf.get(start..end)?;
    // XDR pads every variable-length item up to a multiple of four.
    let padded = len.checked_add(3)? & !3usize;
    Some((bytes, start.checked_add(padded)?))
}
