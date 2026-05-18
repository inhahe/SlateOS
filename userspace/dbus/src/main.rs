//! OurOS D-Bus Message Bus Daemon
//!
//! A multi-personality binary providing:
//! - `dbus-daemon`: The message bus daemon that routes D-Bus messages between clients
//! - `dbus-send`: Send a D-Bus message from the command line
//! - `dbus-monitor`: Watch D-Bus traffic in real time
//!
//! Implements the D-Bus wire protocol including message serialization, the D-Bus
//! type system, bus name ownership, signal matching rules, introspection, and
//! the properties interface.
//!
//! # Usage
//!
//! ```text
//! dbus-daemon --system           Run the system message bus
//! dbus-daemon --session          Run a session message bus
//! dbus-send --dest=NAME --print-reply /path iface.Method [type:value...]
//! dbus-monitor [--system|--session] [match-rules...]
//! ```

#![cfg_attr(not(test), no_main)]
#![allow(clippy::needless_range_loop)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write as IoWrite};
use std::path::Path;

// ============================================================================
// D-Bus Constants
// ============================================================================

/// D-Bus protocol version.
const PROTOCOL_VERSION: u8 = 1;

/// Message type codes.
// Protocol-defined constant; kept for spec compliance.
#[allow(dead_code)]
const MSG_INVALID: u8 = 0;
const MSG_METHOD_CALL: u8 = 1;
const MSG_METHOD_RETURN: u8 = 2;
const MSG_ERROR: u8 = 3;
const MSG_SIGNAL: u8 = 4;

/// Header field codes.
// Protocol-defined constant; kept for spec compliance.
#[allow(dead_code)]
const FIELD_INVALID: u8 = 0;
const FIELD_PATH: u8 = 1;
const FIELD_INTERFACE: u8 = 2;
const FIELD_MEMBER: u8 = 3;
const FIELD_ERROR_NAME: u8 = 4;
const FIELD_REPLY_SERIAL: u8 = 5;
const FIELD_DESTINATION: u8 = 6;
const FIELD_SENDER: u8 = 7;
const FIELD_SIGNATURE: u8 = 8;

/// Endianness markers.
const LITTLE_ENDIAN_MARKER: u8 = b'l';
const BIG_ENDIAN_MARKER: u8 = b'B';

/// Standard bus name for the daemon itself.
const DBUS_BUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";

/// Introspection interface name.
const INTROSPECTABLE_IFACE: &str = "org.freedesktop.DBus.Introspectable";

/// Properties interface name.
const PROPERTIES_IFACE: &str = "org.freedesktop.DBus.Properties";

/// Name request flags.
const NAME_FLAG_ALLOW_REPLACEMENT: u32 = 0x1;
const NAME_FLAG_REPLACE_EXISTING: u32 = 0x2;
const NAME_FLAG_DO_NOT_QUEUE: u32 = 0x4;

/// Name request reply codes.
const NAME_REPLY_PRIMARY_OWNER: u32 = 1;
const NAME_REPLY_IN_QUEUE: u32 = 2;
const NAME_REPLY_EXISTS: u32 = 3;
const NAME_REPLY_ALREADY_OWNER: u32 = 4;

/// Name release reply codes.
const NAME_RELEASE_REPLY_RELEASED: u32 = 1;
const NAME_RELEASE_REPLY_NON_EXISTENT: u32 = 2;
const NAME_RELEASE_REPLY_NOT_OWNER: u32 = 3;

/// Socket path defaults.
const SYSTEM_SOCKET: &str = "/var/run/dbus/system_bus_socket";
const SESSION_SOCKET_DIR: &str = "/tmp/dbus-session";

/// Config file paths.
const SYSTEM_CONF: &str = "/etc/dbus-1/system.conf";
const SESSION_CONF: &str = "/etc/dbus-1/session.conf";

/// Maximum message size (128 MiB, per spec).
const MAX_MESSAGE_SIZE: u32 = 128 * 1024 * 1024;

/// Maximum array length (64 MiB, per spec).
// Protocol-defined limit; kept for spec compliance.
#[allow(dead_code)]
const MAX_ARRAY_LENGTH: u32 = 64 * 1024 * 1024;

// ============================================================================
// D-Bus Type System
// ============================================================================

/// D-Bus type signatures.
#[derive(Debug, Clone, PartialEq)]
pub enum DbusType {
    /// BYTE: unsigned 8-bit integer (y)
    Byte(u8),
    /// BOOLEAN: 0 or 1 (b)
    Boolean(bool),
    /// INT16: signed 16-bit integer (n)
    Int16(i16),
    /// UINT16: unsigned 16-bit integer (q)
    Uint16(u16),
    /// INT32: signed 32-bit integer (i)
    Int32(i32),
    /// UINT32: unsigned 32-bit integer (u)
    Uint32(u32),
    /// INT64: signed 64-bit integer (x)
    Int64(i64),
    /// UINT64: unsigned 64-bit integer (t)
    Uint64(u64),
    /// DOUBLE: IEEE 754 double-precision (d)
    Double(f64),
    /// STRING: UTF-8 NUL-terminated (s)
    String(String),
    /// OBJECT_PATH: like a string with path constraints (o)
    ObjectPath(String),
    /// SIGNATURE: type signature string (g)
    Signature(String),
    /// ARRAY: ordered collection of a single complete type (a...)
    Array(Vec<DbusType>),
    /// STRUCT: fixed-ordered collection of complete types ((...))
    Struct(Vec<DbusType>),
    /// VARIANT: self-describing value (v)
    Variant(Box<DbusType>),
    /// DICT_ENTRY: key-value pair, used within arrays ({...})
    DictEntry(Box<DbusType>, Box<DbusType>),
    /// UNIX_FD: file descriptor index (h)
    UnixFd(u32),
}

impl fmt::Display for DbusType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Byte(v) => write!(f, "byte {v}"),
            Self::Boolean(v) => write!(f, "boolean {v}"),
            Self::Int16(v) => write!(f, "int16 {v}"),
            Self::Uint16(v) => write!(f, "uint16 {v}"),
            Self::Int32(v) => write!(f, "int32 {v}"),
            Self::Uint32(v) => write!(f, "uint32 {v}"),
            Self::Int64(v) => write!(f, "int64 {v}"),
            Self::Uint64(v) => write!(f, "uint64 {v}"),
            Self::Double(v) => write!(f, "double {v}"),
            Self::String(v) => write!(f, "string \"{v}\""),
            Self::ObjectPath(v) => write!(f, "object_path \"{v}\""),
            Self::Signature(v) => write!(f, "signature \"{v}\""),
            Self::Array(items) => {
                write!(f, "array [")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Self::Struct(fields) => {
                write!(f, "struct (")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{field}")?;
                }
                write!(f, ")")
            }
            Self::Variant(inner) => write!(f, "variant {inner}"),
            Self::DictEntry(k, v) => write!(f, "dict_entry({k}, {v})"),
            Self::UnixFd(v) => write!(f, "unix_fd {v}"),
        }
    }
}

impl DbusType {
    /// Return the D-Bus type signature character(s) for this value.
    pub fn signature_str(&self) -> String {
        match self {
            Self::Byte(_) => "y".into(),
            Self::Boolean(_) => "b".into(),
            Self::Int16(_) => "n".into(),
            Self::Uint16(_) => "q".into(),
            Self::Int32(_) => "i".into(),
            Self::Uint32(_) => "u".into(),
            Self::Int64(_) => "x".into(),
            Self::Uint64(_) => "t".into(),
            Self::Double(_) => "d".into(),
            Self::String(_) => "s".into(),
            Self::ObjectPath(_) => "o".into(),
            Self::Signature(_) => "g".into(),
            Self::Array(items) => {
                let inner_sig = if let Some(first) = items.first() {
                    first.signature_str()
                } else {
                    "v".into() // fallback for empty array
                };
                format!("a{inner_sig}")
            }
            Self::Struct(fields) => {
                let mut s = "(".to_string();
                for field in fields {
                    s.push_str(&field.signature_str());
                }
                s.push(')');
                s
            }
            Self::Variant(_) => "v".into(),
            Self::DictEntry(k, v) => {
                format!("{{{}{}}}", k.signature_str(), v.signature_str())
            }
            Self::UnixFd(_) => "h".into(),
        }
    }

    /// Return the alignment requirement for this type.
    pub fn alignment(&self) -> usize {
        match self {
            Self::Byte(_) | Self::Signature(_) => 1,
            Self::Boolean(_) | Self::Int16(_) | Self::Uint16(_) => 2,
            // Note: boolean is actually 4-byte aligned in D-Bus wire format
            Self::Int32(_) | Self::Uint32(_) | Self::String(_)
            | Self::ObjectPath(_) | Self::Array(_) | Self::UnixFd(_) => 4,
            Self::Int64(_) | Self::Uint64(_) | Self::Double(_)
            | Self::Struct(_) | Self::DictEntry(_, _) => 8,
            Self::Variant(_) => 1,
        }
    }
}

// ============================================================================
// Wire Format - Marshaling
// ============================================================================

/// Tracks endianness for marshaling/unmarshaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Endianness {
    Little,
    Big,
}

impl Endianness {
    pub fn marker(self) -> u8 {
        match self {
            Self::Little => LITTLE_ENDIAN_MARKER,
            Self::Big => BIG_ENDIAN_MARKER,
        }
    }

    pub fn from_marker(m: u8) -> Option<Self> {
        match m {
            LITTLE_ENDIAN_MARKER => Some(Self::Little),
            BIG_ENDIAN_MARKER => Some(Self::Big),
            _ => None,
        }
    }
}

/// A buffer for marshaling D-Bus data.
pub struct MarshalBuffer {
    data: Vec<u8>,
    endian: Endianness,
}

impl MarshalBuffer {
    pub fn new(endian: Endianness) -> Self {
        Self {
            data: Vec::new(),
            endian,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// Align the buffer to the given boundary by inserting zero padding.
    pub fn align_to(&mut self, alignment: usize) {
        let offset = self.data.len() % alignment;
        if offset != 0 {
            let padding = alignment - offset;
            self.data.extend(std::iter::repeat_n(0u8, padding));
        }
    }

    pub fn write_byte(&mut self, v: u8) {
        self.data.push(v);
    }

    pub fn write_boolean(&mut self, v: bool) {
        self.align_to(4);
        let val: u32 = if v { 1 } else { 0 };
        self.write_u32_raw(val);
    }

    pub fn write_i16(&mut self, v: i16) {
        self.align_to(2);
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    pub fn write_u16(&mut self, v: u16) {
        self.align_to(2);
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    pub fn write_i32(&mut self, v: i32) {
        self.align_to(4);
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    pub fn write_u32(&mut self, v: u32) {
        self.align_to(4);
        self.write_u32_raw(v);
    }

    fn write_u32_raw(&mut self, v: u32) {
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    pub fn write_i64(&mut self, v: i64) {
        self.align_to(8);
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    pub fn write_u64(&mut self, v: u64) {
        self.align_to(8);
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    pub fn write_f64(&mut self, v: f64) {
        self.align_to(8);
        let bytes = match self.endian {
            Endianness::Little => v.to_le_bytes(),
            Endianness::Big => v.to_be_bytes(),
        };
        self.data.extend_from_slice(&bytes);
    }

    /// Write a string: u32 length + UTF-8 bytes + NUL.
    pub fn write_string(&mut self, s: &str) {
        let len = s.len() as u32;
        self.write_u32(len);
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0); // NUL terminator
    }

    /// Write an object path (same format as string).
    pub fn write_object_path(&mut self, path: &str) {
        self.write_string(path);
    }

    /// Write a signature: u8 length + bytes + NUL.
    pub fn write_signature(&mut self, sig: &str) {
        let len = sig.len() as u8;
        self.write_byte(len);
        self.data.extend_from_slice(sig.as_bytes());
        self.data.push(0); // NUL terminator
    }

    /// Marshal a complete D-Bus value.
    pub fn write_value(&mut self, val: &DbusType) {
        match val {
            DbusType::Byte(v) => self.write_byte(*v),
            DbusType::Boolean(v) => self.write_boolean(*v),
            DbusType::Int16(v) => self.write_i16(*v),
            DbusType::Uint16(v) => self.write_u16(*v),
            DbusType::Int32(v) => self.write_i32(*v),
            DbusType::Uint32(v) => self.write_u32(*v),
            DbusType::Int64(v) => self.write_i64(*v),
            DbusType::Uint64(v) => self.write_u64(*v),
            DbusType::Double(v) => self.write_f64(*v),
            DbusType::String(v) => self.write_string(v),
            DbusType::ObjectPath(v) => self.write_object_path(v),
            DbusType::Signature(v) => self.write_signature(v),
            DbusType::Array(items) => {
                // Array: u32 length of data, then aligned elements
                let len_pos = self.data.len();
                self.write_u32(0); // placeholder for length

                // Align to element type alignment
                let elem_alignment = if let Some(first) = items.first() {
                    wire_alignment_for_sig_char(
                        first.signature_str().as_bytes().first().copied().unwrap_or(b'v'),
                    )
                } else {
                    1
                };
                self.align_to(elem_alignment);

                let data_start = self.data.len();
                for item in items {
                    self.write_value(item);
                }
                let data_len = (self.data.len() - data_start) as u32;

                // Patch the length field
                let len_bytes = match self.endian {
                    Endianness::Little => data_len.to_le_bytes(),
                    Endianness::Big => data_len.to_be_bytes(),
                };
                if len_pos + 4 <= self.data.len() {
                    self.data[len_pos..len_pos + 4].copy_from_slice(&len_bytes);
                }
            }
            DbusType::Struct(fields) => {
                self.align_to(8);
                for field in fields {
                    self.write_value(field);
                }
            }
            DbusType::Variant(inner) => {
                let sig = inner.signature_str();
                self.write_signature(&sig);
                self.write_value(inner);
            }
            DbusType::DictEntry(k, v) => {
                self.align_to(8);
                self.write_value(k);
                self.write_value(v);
            }
            DbusType::UnixFd(v) => self.write_u32(*v),
        }
    }
}

// ============================================================================
// Wire Format - Unmarshaling
// ============================================================================

/// A cursor for unmarshaling D-Bus data from a byte slice.
pub struct UnmarshalCursor<'a> {
    data: &'a [u8],
    pos: usize,
    endian: Endianness,
}

impl<'a> UnmarshalCursor<'a> {
    pub fn new(data: &'a [u8], endian: Endianness) -> Self {
        Self {
            data,
            pos: 0,
            endian,
        }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn check_remaining(&self, need: usize) -> Result<(), DbusError> {
        if self.remaining() < need {
            Err(DbusError::UnmarshalError(format!(
                "need {need} bytes at position {}, only {} remaining",
                self.pos,
                self.remaining()
            )))
        } else {
            Ok(())
        }
    }

    pub fn align_to(&mut self, alignment: usize) {
        let offset = self.pos % alignment;
        if offset != 0 {
            self.pos += alignment - offset;
        }
    }

    pub fn read_byte(&mut self) -> Result<u8, DbusError> {
        self.check_remaining(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_boolean(&mut self) -> Result<bool, DbusError> {
        self.align_to(4);
        let v = self.read_u32()?;
        match v {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DbusError::UnmarshalError(format!(
                "invalid boolean value: {v}"
            ))),
        }
    }

    pub fn read_i16(&mut self) -> Result<i16, DbusError> {
        self.align_to(2);
        self.check_remaining(2)?;
        let bytes: [u8; 2] = [self.data[self.pos], self.data[self.pos + 1]];
        self.pos += 2;
        Ok(match self.endian {
            Endianness::Little => i16::from_le_bytes(bytes),
            Endianness::Big => i16::from_be_bytes(bytes),
        })
    }

    pub fn read_u16(&mut self) -> Result<u16, DbusError> {
        self.align_to(2);
        self.check_remaining(2)?;
        let bytes: [u8; 2] = [self.data[self.pos], self.data[self.pos + 1]];
        self.pos += 2;
        Ok(match self.endian {
            Endianness::Little => u16::from_le_bytes(bytes),
            Endianness::Big => u16::from_be_bytes(bytes),
        })
    }

    pub fn read_i32(&mut self) -> Result<i32, DbusError> {
        self.align_to(4);
        self.check_remaining(4)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(match self.endian {
            Endianness::Little => i32::from_le_bytes(bytes),
            Endianness::Big => i32::from_be_bytes(bytes),
        })
    }

    pub fn read_u32(&mut self) -> Result<u32, DbusError> {
        self.align_to(4);
        self.check_remaining(4)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(match self.endian {
            Endianness::Little => u32::from_le_bytes(bytes),
            Endianness::Big => u32::from_be_bytes(bytes),
        })
    }

    pub fn read_i64(&mut self) -> Result<i64, DbusError> {
        self.align_to(8);
        self.check_remaining(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(match self.endian {
            Endianness::Little => i64::from_le_bytes(bytes),
            Endianness::Big => i64::from_be_bytes(bytes),
        })
    }

    pub fn read_u64(&mut self) -> Result<u64, DbusError> {
        self.align_to(8);
        self.check_remaining(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(match self.endian {
            Endianness::Little => u64::from_le_bytes(bytes),
            Endianness::Big => u64::from_be_bytes(bytes),
        })
    }

    pub fn read_f64(&mut self) -> Result<f64, DbusError> {
        self.align_to(8);
        self.check_remaining(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(match self.endian {
            Endianness::Little => f64::from_le_bytes(bytes),
            Endianness::Big => f64::from_be_bytes(bytes),
        })
    }

    /// Read a D-Bus string: u32 length, then bytes, then NUL.
    pub fn read_string(&mut self) -> Result<String, DbusError> {
        let len = self.read_u32()? as usize;
        self.check_remaining(len + 1)?; // +1 for NUL
        let bytes = &self.data[self.pos..self.pos + len];
        let s = String::from_utf8(bytes.to_vec()).map_err(|e| {
            DbusError::UnmarshalError(format!("invalid UTF-8 in string: {e}"))
        })?;
        self.pos += len + 1; // skip NUL
        Ok(s)
    }

    /// Read an object path (same wire format as string).
    pub fn read_object_path(&mut self) -> Result<String, DbusError> {
        self.read_string()
    }

    /// Read a signature: u8 length, then bytes, then NUL.
    pub fn read_signature(&mut self) -> Result<String, DbusError> {
        let len = self.read_byte()? as usize;
        self.check_remaining(len + 1)?; // +1 for NUL
        let bytes = &self.data[self.pos..self.pos + len];
        let s = String::from_utf8(bytes.to_vec()).map_err(|e| {
            DbusError::UnmarshalError(format!("invalid UTF-8 in signature: {e}"))
        })?;
        self.pos += len + 1; // skip NUL
        Ok(s)
    }

    /// Read a single complete type from the given signature character stream.
    pub fn read_value(&mut self, sig: &mut &[u8]) -> Result<DbusType, DbusError> {
        if sig.is_empty() {
            return Err(DbusError::UnmarshalError(
                "unexpected end of signature".into(),
            ));
        }
        let type_code = sig[0];
        *sig = &sig[1..];

        match type_code {
            b'y' => Ok(DbusType::Byte(self.read_byte()?)),
            b'b' => Ok(DbusType::Boolean(self.read_boolean()?)),
            b'n' => Ok(DbusType::Int16(self.read_i16()?)),
            b'q' => Ok(DbusType::Uint16(self.read_u16()?)),
            b'i' => Ok(DbusType::Int32(self.read_i32()?)),
            b'u' => Ok(DbusType::Uint32(self.read_u32()?)),
            b'x' => Ok(DbusType::Int64(self.read_i64()?)),
            b't' => Ok(DbusType::Uint64(self.read_u64()?)),
            b'd' => Ok(DbusType::Double(self.read_f64()?)),
            b's' => Ok(DbusType::String(self.read_string()?)),
            b'o' => Ok(DbusType::ObjectPath(self.read_object_path()?)),
            b'g' => Ok(DbusType::Signature(self.read_signature()?)),
            b'v' => {
                let variant_sig = self.read_signature()?;
                let mut vsig = variant_sig.as_bytes();
                let val = self.read_value(&mut vsig)?;
                Ok(DbusType::Variant(Box::new(val)))
            }
            b'a' => {
                // Peek at element type signature (could be dict if next is '{')
                let array_len = self.read_u32()? as usize;

                // Capture the element signature for alignment
                let elem_first = sig.first().copied().unwrap_or(b'v');
                let elem_align = wire_alignment_for_sig_char(elem_first);
                self.align_to(elem_align);

                let array_end = self.pos + array_len;
                let mut items = Vec::new();

                while self.pos < array_end {
                    let val = self.read_value(sig)?;
                    items.push(val);
                    // For arrays, we reuse the same element sig for each element.
                    // However, read_value consumes sig chars, so we need to reset.
                    // This is handled by having the caller manage sig properly.
                }
                Ok(DbusType::Array(items))
            }
            b'(' => {
                self.align_to(8);
                let mut fields = Vec::new();
                while !sig.is_empty() && sig[0] != b')' {
                    fields.push(self.read_value(sig)?);
                }
                if !sig.is_empty() {
                    *sig = &sig[1..]; // consume ')'
                }
                Ok(DbusType::Struct(fields))
            }
            b'{' => {
                self.align_to(8);
                let key = self.read_value(sig)?;
                let val = self.read_value(sig)?;
                if !sig.is_empty() && sig[0] == b'}' {
                    *sig = &sig[1..]; // consume '}'
                }
                Ok(DbusType::DictEntry(Box::new(key), Box::new(val)))
            }
            b'h' => Ok(DbusType::UnixFd(self.read_u32()?)),
            _ => Err(DbusError::UnmarshalError(format!(
                "unknown type code: '{}'",
                type_code as char
            ))),
        }
    }
}

/// Return the alignment for a type given its first signature character.
fn wire_alignment_for_sig_char(c: u8) -> usize {
    match c {
        b'y' | b'g' | b'v' => 1,
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b's' | b'o' | b'a' | b'h' => 4,
        b'x' | b't' | b'd' | b'(' | b'{' => 8,
        _ => 1,
    }
}

// ============================================================================
// Error Type
// ============================================================================

#[derive(Debug, Clone)]
pub enum DbusError {
    /// Error marshaling data to wire format.
    MarshalError(String),
    /// Error unmarshaling data from wire format.
    UnmarshalError(String),
    /// Invalid bus name.
    InvalidBusName(String),
    /// Invalid object path.
    InvalidObjectPath(String),
    /// Invalid interface name.
    InvalidInterface(String),
    /// Invalid member name.
    InvalidMember(String),
    /// Name-related error.
    NameError(String),
    /// Configuration error.
    ConfigError(String),
    /// I/O error description.
    IoError(String),
    /// Protocol error.
    ProtocolError(String),
    /// Destination not found.
    ServiceUnknown(String),
}

impl fmt::Display for DbusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MarshalError(s) => write!(f, "marshal error: {s}"),
            Self::UnmarshalError(s) => write!(f, "unmarshal error: {s}"),
            Self::InvalidBusName(s) => write!(f, "invalid bus name: {s}"),
            Self::InvalidObjectPath(s) => write!(f, "invalid object path: {s}"),
            Self::InvalidInterface(s) => write!(f, "invalid interface: {s}"),
            Self::InvalidMember(s) => write!(f, "invalid member: {s}"),
            Self::NameError(s) => write!(f, "name error: {s}"),
            Self::ConfigError(s) => write!(f, "config error: {s}"),
            Self::IoError(s) => write!(f, "I/O error: {s}"),
            Self::ProtocolError(s) => write!(f, "protocol error: {s}"),
            Self::ServiceUnknown(s) => write!(f, "service unknown: {s}"),
        }
    }
}

// ============================================================================
// D-Bus Message
// ============================================================================

/// A D-Bus message (method call, return, error, or signal).
#[derive(Debug, Clone)]
pub struct DbusMessage {
    pub endianness: Endianness,
    pub message_type: u8,
    pub flags: u8,
    pub serial: u32,
    /// Header fields.
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error_name: Option<String>,
    pub reply_serial: Option<u32>,
    pub destination: Option<String>,
    pub sender: Option<String>,
    pub signature: Option<String>,
    /// Body arguments.
    pub body: Vec<DbusType>,
}

impl DbusMessage {
    /// Create a new message with default values.
    pub fn new(msg_type: u8, serial: u32) -> Self {
        Self {
            endianness: Endianness::Little,
            message_type: msg_type,
            flags: 0,
            serial,
            path: None,
            interface: None,
            member: None,
            error_name: None,
            reply_serial: None,
            destination: None,
            sender: None,
            signature: None,
            body: Vec::new(),
        }
    }

    /// Create a method call message.
    pub fn method_call(serial: u32, path: &str, interface: &str, member: &str) -> Self {
        let mut msg = Self::new(MSG_METHOD_CALL, serial);
        msg.path = Some(path.to_string());
        msg.interface = Some(interface.to_string());
        msg.member = Some(member.to_string());
        msg
    }

    /// Create a method return message.
    pub fn method_return(serial: u32, reply_to: u32) -> Self {
        let mut msg = Self::new(MSG_METHOD_RETURN, serial);
        msg.reply_serial = Some(reply_to);
        msg
    }

    /// Create an error message.
    pub fn error(serial: u32, reply_to: u32, error_name: &str, error_msg: &str) -> Self {
        let mut msg = Self::new(MSG_ERROR, serial);
        msg.reply_serial = Some(reply_to);
        msg.error_name = Some(error_name.to_string());
        if !error_msg.is_empty() {
            msg.body.push(DbusType::String(error_msg.to_string()));
            msg.signature = Some("s".to_string());
        }
        msg
    }

    /// Create a signal message.
    pub fn signal(serial: u32, path: &str, interface: &str, member: &str) -> Self {
        let mut msg = Self::new(MSG_SIGNAL, serial);
        msg.path = Some(path.to_string());
        msg.interface = Some(interface.to_string());
        msg.member = Some(member.to_string());
        msg
    }

    /// Compute the body signature from the body values.
    pub fn compute_signature(&self) -> String {
        let mut sig = String::new();
        for val in &self.body {
            sig.push_str(&val.signature_str());
        }
        sig
    }

    /// Return the message type as a human-readable string.
    pub fn type_name(&self) -> &'static str {
        match self.message_type {
            MSG_METHOD_CALL => "method_call",
            MSG_METHOD_RETURN => "method_return",
            MSG_ERROR => "error",
            MSG_SIGNAL => "signal",
            _ => "invalid",
        }
    }

    /// Serialize this message to D-Bus wire format.
    pub fn marshal(&self) -> Result<Vec<u8>, DbusError> {
        let endian = self.endianness;

        // Marshal the body first to know its length.
        let mut body_buf = MarshalBuffer::new(endian);
        for val in &self.body {
            body_buf.write_value(val);
        }
        let body_bytes = body_buf.into_bytes();
        let body_len = body_bytes.len() as u32;

        // Build header fields array.
        let mut header_fields: Vec<DbusType> = Vec::new();

        if let Some(ref path) = self.path {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_PATH),
                DbusType::Variant(Box::new(DbusType::ObjectPath(path.clone()))),
            ]));
        }
        if let Some(ref iface) = self.interface {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_INTERFACE),
                DbusType::Variant(Box::new(DbusType::String(iface.clone()))),
            ]));
        }
        if let Some(ref member) = self.member {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_MEMBER),
                DbusType::Variant(Box::new(DbusType::String(member.clone()))),
            ]));
        }
        if let Some(ref ename) = self.error_name {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_ERROR_NAME),
                DbusType::Variant(Box::new(DbusType::String(ename.clone()))),
            ]));
        }
        if let Some(rs) = self.reply_serial {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_REPLY_SERIAL),
                DbusType::Variant(Box::new(DbusType::Uint32(rs))),
            ]));
        }
        if let Some(ref dest) = self.destination {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_DESTINATION),
                DbusType::Variant(Box::new(DbusType::String(dest.clone()))),
            ]));
        }
        if let Some(ref sender) = self.sender {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_SENDER),
                DbusType::Variant(Box::new(DbusType::String(sender.clone()))),
            ]));
        }

        // Signature field: compute from body if not explicitly set
        let sig = if let Some(ref s) = self.signature {
            s.clone()
        } else {
            self.compute_signature()
        };
        if !sig.is_empty() {
            header_fields.push(DbusType::Struct(vec![
                DbusType::Byte(FIELD_SIGNATURE),
                DbusType::Variant(Box::new(DbusType::Signature(sig))),
            ]));
        }

        // Marshal header fields as an array of structs
        let mut hdr_buf = MarshalBuffer::new(endian);
        // Array length placeholder
        let array_len_pos = 0;
        hdr_buf.write_u32(0); // placeholder
        // Align to 8 for struct elements
        hdr_buf.align_to(8);
        let data_start = hdr_buf.len();
        for field in &header_fields {
            hdr_buf.write_value(field);
        }
        let array_data_len = (hdr_buf.len() - data_start) as u32;

        // Patch the array length
        let mut hdr_bytes = hdr_buf.into_bytes();
        let len_bytes = match endian {
            Endianness::Little => array_data_len.to_le_bytes(),
            Endianness::Big => array_data_len.to_be_bytes(),
        };
        if array_len_pos + 4 <= hdr_bytes.len() {
            hdr_bytes[array_len_pos..array_len_pos + 4].copy_from_slice(&len_bytes);
        }

        // Build the complete message
        let mut result = Vec::new();

        // Fixed header: endianness(1) + type(1) + flags(1) + version(1) + body_len(4) + serial(4)
        result.push(endian.marker());
        result.push(self.message_type);
        result.push(self.flags);
        result.push(PROTOCOL_VERSION);

        let bl = match endian {
            Endianness::Little => body_len.to_le_bytes(),
            Endianness::Big => body_len.to_be_bytes(),
        };
        result.extend_from_slice(&bl);

        let ser = match endian {
            Endianness::Little => self.serial.to_le_bytes(),
            Endianness::Big => self.serial.to_be_bytes(),
        };
        result.extend_from_slice(&ser);

        // Header fields array
        result.extend_from_slice(&hdr_bytes);

        // Pad to 8-byte boundary before body
        let pad_needed = (8 - (result.len() % 8)) % 8;
        result.extend(std::iter::repeat_n(0u8, pad_needed));

        // Body
        result.extend_from_slice(&body_bytes);

        Ok(result)
    }

    /// Deserialize a message from wire format bytes.
    pub fn unmarshal(data: &[u8]) -> Result<Self, DbusError> {
        if data.len() < 16 {
            return Err(DbusError::UnmarshalError(
                "message too short (need at least 16 bytes)".into(),
            ));
        }

        let endian = Endianness::from_marker(data[0]).ok_or_else(|| {
            DbusError::UnmarshalError(format!("invalid endianness marker: 0x{:02x}", data[0]))
        })?;

        let msg_type = data[1];
        let flags = data[2];
        let _version = data[3];

        let body_len = match endian {
            Endianness::Little => u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            Endianness::Big => u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
        };

        let serial = match endian {
            Endianness::Little => u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            Endianness::Big => u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
        };

        if body_len > MAX_MESSAGE_SIZE {
            return Err(DbusError::UnmarshalError(format!(
                "body length {body_len} exceeds maximum"
            )));
        }

        let mut msg = DbusMessage::new(msg_type, serial);
        msg.endianness = endian;
        msg.flags = flags;

        // Parse header fields array starting at offset 12
        let mut cursor = UnmarshalCursor::new(&data[12..], endian);
        let header_array_len = cursor.read_u32()? as usize;

        if header_array_len > 0 {
            // Align to 8 for the struct elements
            cursor.align_to(8);
            let fields_end = cursor.position() + header_array_len;

            while cursor.position() < fields_end {
                cursor.align_to(8);
                if cursor.remaining() < 2 {
                    break;
                }
                let field_code = cursor.read_byte()?;
                // Read the variant
                let variant_sig = cursor.read_signature()?;
                let mut vsig = variant_sig.as_bytes();
                let value = cursor.read_value(&mut vsig)?;

                match field_code {
                    FIELD_PATH => {
                        if let DbusType::ObjectPath(p) | DbusType::String(p) = &value {
                            msg.path = Some(p.clone());
                        }
                    }
                    FIELD_INTERFACE => {
                        if let DbusType::String(s) = &value {
                            msg.interface = Some(s.clone());
                        }
                    }
                    FIELD_MEMBER => {
                        if let DbusType::String(s) = &value {
                            msg.member = Some(s.clone());
                        }
                    }
                    FIELD_ERROR_NAME => {
                        if let DbusType::String(s) = &value {
                            msg.error_name = Some(s.clone());
                        }
                    }
                    FIELD_REPLY_SERIAL => {
                        if let DbusType::Uint32(n) = &value {
                            msg.reply_serial = Some(*n);
                        }
                    }
                    FIELD_DESTINATION => {
                        if let DbusType::String(s) = &value {
                            msg.destination = Some(s.clone());
                        }
                    }
                    FIELD_SENDER => {
                        if let DbusType::String(s) = &value {
                            msg.sender = Some(s.clone());
                        }
                    }
                    FIELD_SIGNATURE => {
                        if let DbusType::Signature(s) = &value {
                            msg.signature = Some(s.clone());
                        }
                    }
                    _ => {} // Unknown fields are ignored per spec
                }
            }
        }

        // Align to 8-byte boundary for body start
        let header_total = 12 + cursor.position();
        let body_start = header_total + ((8 - (header_total % 8)) % 8);

        // Parse body if there's a signature
        if let Some(ref sig) = msg.signature {
            if !sig.is_empty() && body_start < data.len() {
                let body_data = &data[body_start..];
                let mut body_cursor = UnmarshalCursor::new(body_data, endian);
                let mut sig_bytes = sig.as_bytes();

                while !sig_bytes.is_empty() && body_cursor.remaining() > 0 {
                    let val = body_cursor.read_value(&mut sig_bytes)?;
                    msg.body.push(val);
                }
            }
        }

        Ok(msg)
    }
}

impl fmt::Display for DbusMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.type_name())?;
        if let Some(ref sender) = self.sender {
            write!(f, " sender={sender}")?;
        }
        if let Some(ref dest) = self.destination {
            write!(f, " dest={dest}")?;
        }
        write!(f, " serial={}", self.serial)?;
        if let Some(ref path) = self.path {
            write!(f, " path={path}")?;
        }
        if let Some(ref iface) = self.interface {
            write!(f, " iface={iface}")?;
        }
        if let Some(ref member) = self.member {
            write!(f, " member={member}")?;
        }
        if let Some(ref ename) = self.error_name {
            write!(f, " error_name={ename}")?;
        }
        if let Some(rs) = self.reply_serial {
            write!(f, " reply_serial={rs}")?;
        }
        for val in &self.body {
            write!(f, "\n   {val}")?;
        }
        Ok(())
    }
}

// ============================================================================
// Validation Helpers
// ============================================================================

/// Validate a D-Bus bus name (well-known or unique).
pub fn validate_bus_name(name: &str) -> Result<(), DbusError> {
    if name.is_empty() {
        return Err(DbusError::InvalidBusName("empty name".into()));
    }
    if name.len() > 255 {
        return Err(DbusError::InvalidBusName("name too long".into()));
    }

    // Unique names start with ':'
    if name.starts_with(':') {
        // Unique connection name: :N.M
        if name.len() < 4 {
            return Err(DbusError::InvalidBusName(
                "unique name too short".into(),
            ));
        }
        return Ok(());
    }

    // Well-known name: must have at least two elements separated by '.'
    let elements: Vec<&str> = name.split('.').collect();
    if elements.len() < 2 {
        return Err(DbusError::InvalidBusName(
            "well-known name must have at least two elements".into(),
        ));
    }

    for elem in &elements {
        if elem.is_empty() {
            return Err(DbusError::InvalidBusName(
                "empty element in name".into(),
            ));
        }
        for (i, c) in elem.chars().enumerate() {
            if i == 0 && c.is_ascii_digit() {
                return Err(DbusError::InvalidBusName(
                    "element cannot start with digit".into(),
                ));
            }
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                return Err(DbusError::InvalidBusName(format!(
                    "invalid character '{c}' in name"
                )));
            }
        }
    }
    Ok(())
}

/// Validate a D-Bus object path.
pub fn validate_object_path(path: &str) -> Result<(), DbusError> {
    if path.is_empty() || !path.starts_with('/') {
        return Err(DbusError::InvalidObjectPath(
            "path must start with '/'".into(),
        ));
    }
    if path.len() > 1 && path.ends_with('/') {
        return Err(DbusError::InvalidObjectPath(
            "path must not end with '/' (except root)".into(),
        ));
    }
    if path.contains("//") {
        return Err(DbusError::InvalidObjectPath(
            "path must not contain '//'".into(),
        ));
    }
    if path == "/" {
        return Ok(());
    }

    for element in path[1..].split('/') {
        if element.is_empty() {
            return Err(DbusError::InvalidObjectPath("empty element".into()));
        }
        for c in element.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' {
                return Err(DbusError::InvalidObjectPath(format!(
                    "invalid character '{c}'"
                )));
            }
        }
    }
    Ok(())
}

/// Validate a D-Bus interface name.
pub fn validate_interface_name(name: &str) -> Result<(), DbusError> {
    if name.is_empty() {
        return Err(DbusError::InvalidInterface("empty name".into()));
    }
    if name.len() > 255 {
        return Err(DbusError::InvalidInterface("name too long".into()));
    }

    let elements: Vec<&str> = name.split('.').collect();
    if elements.len() < 2 {
        return Err(DbusError::InvalidInterface(
            "must have at least two elements".into(),
        ));
    }
    for elem in &elements {
        if elem.is_empty() {
            return Err(DbusError::InvalidInterface("empty element".into()));
        }
        for (i, c) in elem.chars().enumerate() {
            if i == 0 && c.is_ascii_digit() {
                return Err(DbusError::InvalidInterface(
                    "element cannot start with digit".into(),
                ));
            }
            if !c.is_ascii_alphanumeric() && c != '_' {
                return Err(DbusError::InvalidInterface(format!(
                    "invalid character '{c}'"
                )));
            }
        }
    }
    Ok(())
}

/// Validate a D-Bus member (method/signal) name.
pub fn validate_member_name(name: &str) -> Result<(), DbusError> {
    if name.is_empty() {
        return Err(DbusError::InvalidMember("empty name".into()));
    }
    if name.len() > 255 {
        return Err(DbusError::InvalidMember("name too long".into()));
    }
    for (i, c) in name.chars().enumerate() {
        if i == 0 && c.is_ascii_digit() {
            return Err(DbusError::InvalidMember(
                "cannot start with digit".into(),
            ));
        }
        if !c.is_ascii_alphanumeric() && c != '_' {
            return Err(DbusError::InvalidMember(format!(
                "invalid character '{c}'"
            )));
        }
    }
    Ok(())
}

/// Validate a D-Bus type signature string.
pub fn validate_signature(sig: &str) -> Result<(), DbusError> {
    if sig.len() > 255 {
        return Err(DbusError::MarshalError("signature too long".into()));
    }
    let bytes = sig.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        pos = validate_single_complete_type(bytes, pos)?;
    }
    Ok(())
}

fn validate_single_complete_type(sig: &[u8], pos: usize) -> Result<usize, DbusError> {
    if pos >= sig.len() {
        return Err(DbusError::MarshalError(
            "unexpected end of signature".into(),
        ));
    }
    match sig[pos] {
        b'y' | b'b' | b'n' | b'q' | b'i' | b'u' | b'x' | b't' | b'd' | b's' | b'o'
        | b'g' | b'v' | b'h' => Ok(pos + 1),
        b'a' => validate_single_complete_type(sig, pos + 1),
        b'(' => {
            let mut p = pos + 1;
            if p >= sig.len() || sig[p] == b')' {
                return Err(DbusError::MarshalError(
                    "empty struct in signature".into(),
                ));
            }
            while p < sig.len() && sig[p] != b')' {
                p = validate_single_complete_type(sig, p)?;
            }
            if p >= sig.len() {
                return Err(DbusError::MarshalError(
                    "unmatched '(' in signature".into(),
                ));
            }
            Ok(p + 1) // skip ')'
        }
        b'{' => {
            let mut p = pos + 1;
            // Key must be a basic type
            if p >= sig.len() {
                return Err(DbusError::MarshalError(
                    "unmatched '{' in signature".into(),
                ));
            }
            p = validate_single_complete_type(sig, p)?;
            // Value
            if p >= sig.len() {
                return Err(DbusError::MarshalError(
                    "dict entry needs key and value".into(),
                ));
            }
            p = validate_single_complete_type(sig, p)?;
            if p >= sig.len() || sig[p] != b'}' {
                return Err(DbusError::MarshalError(
                    "unmatched '{' in signature".into(),
                ));
            }
            Ok(p + 1) // skip '}'
        }
        c => Err(DbusError::MarshalError(format!(
            "unknown type code '{}' in signature",
            c as char
        ))),
    }
}

// ============================================================================
// Signal Match Rules
// ============================================================================

/// A match rule for filtering D-Bus signals.
#[derive(Debug, Clone, Default)]
pub struct MatchRule {
    pub msg_type: Option<u8>,
    pub sender: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub path: Option<String>,
    pub path_namespace: Option<String>,
    pub destination: Option<String>,
    pub arg0: Option<String>,
}

impl MatchRule {
    /// Parse a match rule string like "type='signal',interface='org.foo',member='Bar'".
    pub fn parse(rule_str: &str) -> Result<Self, DbusError> {
        let mut rule = MatchRule::default();

        for part in rule_str.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let eq_pos = part.find('=').ok_or_else(|| {
                DbusError::ProtocolError(format!("invalid match rule part: {part}"))
            })?;
            let key = part[..eq_pos].trim();
            let mut val = part[eq_pos + 1..].trim();

            // Strip quotes
            if val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2 {
                val = &val[1..val.len() - 1];
            }

            match key {
                "type" => {
                    rule.msg_type = Some(match val {
                        "method_call" => MSG_METHOD_CALL,
                        "method_return" => MSG_METHOD_RETURN,
                        "error" => MSG_ERROR,
                        "signal" => MSG_SIGNAL,
                        _ => {
                            return Err(DbusError::ProtocolError(format!(
                                "unknown message type: {val}"
                            )));
                        }
                    });
                }
                "sender" => rule.sender = Some(val.to_string()),
                "interface" => rule.interface = Some(val.to_string()),
                "member" => rule.member = Some(val.to_string()),
                "path" => rule.path = Some(val.to_string()),
                "path_namespace" => rule.path_namespace = Some(val.to_string()),
                "destination" => rule.destination = Some(val.to_string()),
                "arg0" => rule.arg0 = Some(val.to_string()),
                _ => {
                    // Ignore unknown keys for forward compatibility
                }
            }
        }
        Ok(rule)
    }

    /// Check if a message matches this rule.
    pub fn matches(&self, msg: &DbusMessage) -> bool {
        if let Some(t) = self.msg_type {
            if msg.message_type != t {
                return false;
            }
        }
        if let Some(ref s) = self.sender {
            if msg.sender.as_deref() != Some(s.as_str()) {
                return false;
            }
        }
        if let Some(ref i) = self.interface {
            if msg.interface.as_deref() != Some(i.as_str()) {
                return false;
            }
        }
        if let Some(ref m) = self.member {
            if msg.member.as_deref() != Some(m.as_str()) {
                return false;
            }
        }
        if let Some(ref p) = self.path {
            if msg.path.as_deref() != Some(p.as_str()) {
                return false;
            }
        }
        if let Some(ref ns) = self.path_namespace {
            match &msg.path {
                Some(p) => {
                    if p != ns && !p.starts_with(&format!("{ns}/")) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        if let Some(ref d) = self.destination {
            if msg.destination.as_deref() != Some(d.as_str()) {
                return false;
            }
        }
        if let Some(ref a0) = self.arg0 {
            // Match first string argument in body
            let first_str = msg.body.first().and_then(|v| {
                if let DbusType::String(s) = v {
                    Some(s.as_str())
                } else {
                    None
                }
            });
            if first_str != Some(a0.as_str()) {
                return false;
            }
        }
        true
    }

    /// Format the rule back to string form.
    pub fn to_rule_string(&self) -> String {
        let mut parts = Vec::new();
        if let Some(t) = self.msg_type {
            let name = match t {
                MSG_METHOD_CALL => "method_call",
                MSG_METHOD_RETURN => "method_return",
                MSG_ERROR => "error",
                MSG_SIGNAL => "signal",
                _ => "unknown",
            };
            parts.push(format!("type='{name}'"));
        }
        if let Some(ref s) = self.sender {
            parts.push(format!("sender='{s}'"));
        }
        if let Some(ref i) = self.interface {
            parts.push(format!("interface='{i}'"));
        }
        if let Some(ref m) = self.member {
            parts.push(format!("member='{m}'"));
        }
        if let Some(ref p) = self.path {
            parts.push(format!("path='{p}'"));
        }
        if let Some(ref ns) = self.path_namespace {
            parts.push(format!("path_namespace='{ns}'"));
        }
        if let Some(ref d) = self.destination {
            parts.push(format!("destination='{d}'"));
        }
        if let Some(ref a) = self.arg0 {
            parts.push(format!("arg0='{a}'"));
        }
        parts.join(",")
    }
}

// ============================================================================
// Name Ownership Registry
// ============================================================================

/// Tracks ownership of well-known names on the bus.
#[derive(Debug, Clone)]
pub struct NameEntry {
    /// Current owner (unique connection name).
    pub owner: String,
    /// Whether replacement is allowed.
    pub allow_replacement: bool,
    /// Queue of connections waiting for this name.
    pub queue: Vec<String>,
}

/// The name registry managing well-known bus names.
pub struct NameRegistry {
    /// Well-known name -> entry mapping.
    names: HashMap<String, NameEntry>,
    /// Unique connection name counter.
    next_unique_id: u64,
}

impl NameRegistry {
    pub fn new() -> Self {
        Self {
            names: HashMap::new(),
            next_unique_id: 1,
        }
    }

    /// Allocate a new unique connection name.
    pub fn allocate_unique_name(&mut self) -> String {
        let id = self.next_unique_id;
        self.next_unique_id = self.next_unique_id.saturating_add(1);
        format!(":1.{id}")
    }

    /// Request ownership of a well-known name.
    pub fn request_name(
        &mut self,
        name: &str,
        owner: &str,
        flags: u32,
    ) -> Result<(u32, Option<String>), DbusError> {
        validate_bus_name(name)?;

        // Check if already owned by this connection
        if let Some(entry) = self.names.get(name) {
            if entry.owner == owner {
                return Ok((NAME_REPLY_ALREADY_OWNER, None));
            }

            // Try to replace?
            if (flags & NAME_FLAG_REPLACE_EXISTING) != 0 && entry.allow_replacement {
                let old_owner = entry.owner.clone();
                let entry = self.names.get_mut(name).expect("checked above");
                entry.owner = owner.to_string();
                entry.allow_replacement = (flags & NAME_FLAG_ALLOW_REPLACEMENT) != 0;
                return Ok((NAME_REPLY_PRIMARY_OWNER, Some(old_owner)));
            }

            // Queue or reject
            if (flags & NAME_FLAG_DO_NOT_QUEUE) != 0 {
                return Ok((NAME_REPLY_EXISTS, None));
            }

            let entry = self.names.get_mut(name).expect("checked above");
            if !entry.queue.contains(&owner.to_string()) {
                entry.queue.push(owner.to_string());
            }
            return Ok((NAME_REPLY_IN_QUEUE, None));
        }

        // Name is free — take it
        self.names.insert(
            name.to_string(),
            NameEntry {
                owner: owner.to_string(),
                allow_replacement: (flags & NAME_FLAG_ALLOW_REPLACEMENT) != 0,
                queue: Vec::new(),
            },
        );

        Ok((NAME_REPLY_PRIMARY_OWNER, None))
    }

    /// Release ownership of a well-known name.
    pub fn release_name(
        &mut self,
        name: &str,
        owner: &str,
    ) -> Result<(u32, Option<String>), DbusError> {
        let Some(entry) = self.names.get_mut(name) else {
            return Ok((NAME_RELEASE_REPLY_NON_EXISTENT, None));
        };

        if entry.owner == owner {
            // Transfer to next in queue, or remove entirely
            if let Some(next_owner) = entry.queue.first().cloned() {
                entry.queue.remove(0);
                entry.owner = next_owner.clone();
                Ok((NAME_RELEASE_REPLY_RELEASED, Some(next_owner)))
            } else {
                self.names.remove(name);
                Ok((NAME_RELEASE_REPLY_RELEASED, None))
            }
        } else {
            // Check if in queue
            let pos = entry.queue.iter().position(|q| q == owner);
            if let Some(idx) = pos {
                entry.queue.remove(idx);
                Ok((NAME_RELEASE_REPLY_RELEASED, None))
            } else {
                Ok((NAME_RELEASE_REPLY_NOT_OWNER, None))
            }
        }
    }

    /// Get the owner of a well-known name.
    pub fn get_name_owner<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        // If it's a unique name, it "owns itself"
        if name.starts_with(':') {
            return Some(name);
        }
        self.names.get(name).map(|e| e.owner.as_str())
    }

    /// Check if a name exists.
    pub fn name_has_owner(&self, name: &str) -> bool {
        if name.starts_with(':') {
            return true; // Unique names always "exist"
        }
        self.names.contains_key(name)
    }

    /// List all well-known names.
    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.names.keys().cloned().collect();
        names.push(DBUS_BUS_NAME.to_string());
        names.sort();
        names
    }

    /// Remove all names owned by a connection (on disconnect).
    pub fn remove_connection(&mut self, unique_name: &str) -> Vec<(String, Option<String>)> {
        let mut changes = Vec::new();
        let owned: Vec<String> = self
            .names
            .iter()
            .filter(|(_, v)| v.owner == unique_name)
            .map(|(k, _)| k.clone())
            .collect();

        for name in owned {
            let (_, new_owner) = self
                .release_name(&name, unique_name)
                .unwrap_or((NAME_RELEASE_REPLY_NOT_OWNER, None));
            changes.push((name, new_owner));
        }

        // Also remove from any queues
        for entry in self.names.values_mut() {
            entry.queue.retain(|q| q != unique_name);
        }

        changes
    }
}

// ============================================================================
// Bus Connection
// ============================================================================

/// Represents a connected client on the bus.
#[derive(Debug, Clone)]
pub struct BusConnection {
    pub unique_name: String,
    pub match_rules: Vec<MatchRule>,
    pub authenticated: bool,
}

impl BusConnection {
    pub fn new(unique_name: String) -> Self {
        Self {
            unique_name,
            match_rules: Vec::new(),
            authenticated: false,
        }
    }
}

// ============================================================================
// Bus Daemon Core
// ============================================================================

/// The D-Bus message bus daemon.
pub struct BusDaemon {
    /// Bus type (system or session).
    pub bus_type: BusType,
    /// Name registry.
    pub names: NameRegistry,
    /// Connected clients.
    pub connections: HashMap<String, BusConnection>,
    /// Serial number counter for daemon-originated messages.
    pub next_serial: u32,
    /// Properties exposed by the daemon.
    pub properties: HashMap<String, DbusType>,
    /// Configuration.
    pub config: BusConfig,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BusType {
    System,
    Session,
}

impl fmt::Display for BusType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::Session => write!(f, "session"),
        }
    }
}

impl BusDaemon {
    pub fn new(bus_type: BusType) -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "Features".to_string(),
            DbusType::Array(vec![
                DbusType::String("SystemdActivation".to_string()),
            ]),
        );
        properties.insert(
            "Interfaces".to_string(),
            DbusType::Array(vec![
                DbusType::String(DBUS_INTERFACE.to_string()),
                DbusType::String(INTROSPECTABLE_IFACE.to_string()),
                DbusType::String(PROPERTIES_IFACE.to_string()),
            ]),
        );

        Self {
            bus_type,
            names: NameRegistry::new(),
            connections: HashMap::new(),
            next_serial: 1,
            properties,
            config: BusConfig::default(),
        }
    }

    /// Allocate the next serial number for daemon messages.
    pub fn alloc_serial(&mut self) -> u32 {
        let s = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1);
        if self.next_serial == 0 {
            self.next_serial = 1;
        }
        s
    }

    /// Register a new connection and assign a unique name.
    pub fn register_connection(&mut self) -> String {
        let unique = self.names.allocate_unique_name();
        let conn = BusConnection::new(unique.clone());
        self.connections.insert(unique.clone(), conn);
        unique
    }

    /// Unregister a connection (client disconnected).
    pub fn unregister_connection(&mut self, unique_name: &str) -> Vec<DbusMessage> {
        let mut signals = Vec::new();
        self.connections.remove(unique_name);

        // Release all owned names and generate NameOwnerChanged signals
        let changes = self.names.remove_connection(unique_name);
        for (name, new_owner) in &changes {
            let serial = self.alloc_serial();
            let mut sig =
                DbusMessage::signal(serial, DBUS_PATH, DBUS_INTERFACE, "NameOwnerChanged");
            sig.sender = Some(DBUS_BUS_NAME.to_string());
            sig.body.push(DbusType::String(name.clone()));
            sig.body.push(DbusType::String(unique_name.to_string()));
            sig.body.push(DbusType::String(
                new_owner.as_deref().unwrap_or("").to_string(),
            ));
            sig.signature = Some("sss".to_string());
            signals.push(sig);
        }

        // Also signal that the unique name itself went away
        let serial = self.alloc_serial();
        let mut sig =
            DbusMessage::signal(serial, DBUS_PATH, DBUS_INTERFACE, "NameOwnerChanged");
        sig.sender = Some(DBUS_BUS_NAME.to_string());
        sig.body
            .push(DbusType::String(unique_name.to_string()));
        sig.body
            .push(DbusType::String(unique_name.to_string()));
        sig.body.push(DbusType::String(String::new()));
        sig.signature = Some("sss".to_string());
        signals.push(sig);

        signals
    }

    /// Handle a message sent to the bus daemon itself (org.freedesktop.DBus).
    pub fn handle_bus_message(
        &mut self,
        msg: &DbusMessage,
        sender: &str,
    ) -> Result<Vec<DbusMessage>, DbusError> {
        let member = msg
            .member
            .as_deref()
            .ok_or_else(|| DbusError::ProtocolError("no member in method call".into()))?;

        let interface = msg.interface.as_deref().unwrap_or(DBUS_INTERFACE);

        match interface {
            DBUS_INTERFACE => self.handle_dbus_interface(msg, sender, member),
            INTROSPECTABLE_IFACE => self.handle_introspect(msg, sender, member),
            PROPERTIES_IFACE => self.handle_properties(msg, sender, member),
            _ => {
                let serial = self.alloc_serial();
                Ok(vec![DbusMessage::error(
                    serial,
                    msg.serial,
                    "org.freedesktop.DBus.Error.UnknownInterface",
                    &format!("unknown interface: {interface}"),
                )])
            }
        }
    }

    fn handle_dbus_interface(
        &mut self,
        msg: &DbusMessage,
        sender: &str,
        member: &str,
    ) -> Result<Vec<DbusMessage>, DbusError> {
        let mut responses = Vec::new();

        match member {
            "Hello" => {
                // The sender's unique name was already assigned at connection time.
                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::String(sender.to_string()));
                reply.signature = Some("s".to_string());
                responses.push(reply);

                // NameAcquired signal
                let serial2 = self.alloc_serial();
                let mut sig =
                    DbusMessage::signal(serial2, DBUS_PATH, DBUS_INTERFACE, "NameAcquired");
                sig.sender = Some(DBUS_BUS_NAME.to_string());
                sig.destination = Some(sender.to_string());
                sig.body.push(DbusType::String(sender.to_string()));
                sig.signature = Some("s".to_string());
                responses.push(sig);
            }

            "RequestName" => {
                let name = match msg.body.first() {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "RequestName requires (su) arguments".into(),
                        ));
                    }
                };
                let flags = match msg.body.get(1) {
                    Some(DbusType::Uint32(f)) => *f,
                    _ => 0,
                };

                let (result_code, old_owner) =
                    self.names.request_name(&name, sender, flags)?;

                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::Uint32(result_code));
                reply.signature = Some("u".to_string());
                responses.push(reply);

                // NameOwnerChanged if ownership changed
                if result_code == NAME_REPLY_PRIMARY_OWNER {
                    let old = old_owner.as_deref().unwrap_or("");
                    let sig_serial = self.alloc_serial();
                    let mut sig = DbusMessage::signal(
                        sig_serial,
                        DBUS_PATH,
                        DBUS_INTERFACE,
                        "NameOwnerChanged",
                    );
                    sig.sender = Some(DBUS_BUS_NAME.to_string());
                    sig.body.push(DbusType::String(name.clone()));
                    sig.body.push(DbusType::String(old.to_string()));
                    sig.body.push(DbusType::String(sender.to_string()));
                    sig.signature = Some("sss".to_string());
                    responses.push(sig);

                    // NameAcquired to new owner
                    let acq_serial = self.alloc_serial();
                    let mut acq = DbusMessage::signal(
                        acq_serial,
                        DBUS_PATH,
                        DBUS_INTERFACE,
                        "NameAcquired",
                    );
                    acq.sender = Some(DBUS_BUS_NAME.to_string());
                    acq.destination = Some(sender.to_string());
                    acq.body.push(DbusType::String(name));
                    acq.signature = Some("s".to_string());
                    responses.push(acq);
                }
            }

            "ReleaseName" => {
                let name = match msg.body.first() {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "ReleaseName requires (s) argument".into(),
                        ));
                    }
                };

                let (result_code, new_owner) =
                    self.names.release_name(&name, sender)?;

                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::Uint32(result_code));
                reply.signature = Some("u".to_string());
                responses.push(reply);

                if result_code == NAME_RELEASE_REPLY_RELEASED {
                    let new = new_owner.as_deref().unwrap_or("");
                    let sig_serial = self.alloc_serial();
                    let mut sig = DbusMessage::signal(
                        sig_serial,
                        DBUS_PATH,
                        DBUS_INTERFACE,
                        "NameOwnerChanged",
                    );
                    sig.sender = Some(DBUS_BUS_NAME.to_string());
                    sig.body.push(DbusType::String(name.clone()));
                    sig.body.push(DbusType::String(sender.to_string()));
                    sig.body.push(DbusType::String(new.to_string()));
                    sig.signature = Some("sss".to_string());
                    responses.push(sig);

                    // NameLost to old owner
                    let lost_serial = self.alloc_serial();
                    let mut lost = DbusMessage::signal(
                        lost_serial,
                        DBUS_PATH,
                        DBUS_INTERFACE,
                        "NameLost",
                    );
                    lost.sender = Some(DBUS_BUS_NAME.to_string());
                    lost.destination = Some(sender.to_string());
                    lost.body.push(DbusType::String(name));
                    lost.signature = Some("s".to_string());
                    responses.push(lost);
                }
            }

            "GetNameOwner" => {
                let name = match msg.body.first() {
                    Some(DbusType::String(s)) => s.as_str(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "GetNameOwner requires (s) argument".into(),
                        ));
                    }
                };

                let serial = self.alloc_serial();
                if let Some(owner) = self.names.get_name_owner(name) {
                    let mut reply = DbusMessage::method_return(serial, msg.serial);
                    reply.sender = Some(DBUS_BUS_NAME.to_string());
                    reply.destination = Some(sender.to_string());
                    reply.body.push(DbusType::String(owner.to_string()));
                    reply.signature = Some("s".to_string());
                    responses.push(reply);
                } else {
                    responses.push(DbusMessage::error(
                        serial,
                        msg.serial,
                        "org.freedesktop.DBus.Error.NameHasNoOwner",
                        &format!("name '{name}' has no owner"),
                    ));
                }
            }

            "NameHasOwner" => {
                let name = match msg.body.first() {
                    Some(DbusType::String(s)) => s.as_str(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "NameHasOwner requires (s) argument".into(),
                        ));
                    }
                };

                let has_owner = self.names.name_has_owner(name);
                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::Boolean(has_owner));
                reply.signature = Some("b".to_string());
                responses.push(reply);
            }

            "ListNames" => {
                let names = self.names.list_names();
                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::Array(
                    names.into_iter().map(DbusType::String).collect(),
                ));
                reply.signature = Some("as".to_string());
                responses.push(reply);
            }

            "ListActivatableNames" => {
                // We don't support activation yet; return empty list.
                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::Array(Vec::new()));
                reply.signature = Some("as".to_string());
                responses.push(reply);
            }

            "AddMatch" => {
                let rule_str = match msg.body.first() {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "AddMatch requires (s) argument".into(),
                        ));
                    }
                };

                let rule = MatchRule::parse(&rule_str)?;

                if let Some(conn) = self.connections.get_mut(sender) {
                    conn.match_rules.push(rule);
                }

                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                responses.push(reply);
            }

            "RemoveMatch" => {
                let rule_str = match msg.body.first() {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "RemoveMatch requires (s) argument".into(),
                        ));
                    }
                };

                let rule = MatchRule::parse(&rule_str)?;

                if let Some(conn) = self.connections.get_mut(sender) {
                    // Remove first matching rule
                    if let Some(pos) = conn
                        .match_rules
                        .iter()
                        .position(|r| r.to_rule_string() == rule.to_rule_string())
                    {
                        conn.match_rules.remove(pos);
                    }
                }

                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                responses.push(reply);
            }

            "GetId" => {
                // Return a unique bus ID (normally a UUID).
                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply
                    .body
                    .push(DbusType::String("ouros-dbus-00000001".to_string()));
                reply.signature = Some("s".to_string());
                responses.push(reply);
            }

            "ListQueuedOwners" => {
                let name = match msg.body.first() {
                    Some(DbusType::String(s)) => s.as_str(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "ListQueuedOwners requires (s) argument".into(),
                        ));
                    }
                };

                let serial = self.alloc_serial();
                if let Some(entry) = self.names.names.get(name) {
                    let mut owners = vec![DbusType::String(entry.owner.clone())];
                    for q in &entry.queue {
                        owners.push(DbusType::String(q.clone()));
                    }
                    let mut reply = DbusMessage::method_return(serial, msg.serial);
                    reply.sender = Some(DBUS_BUS_NAME.to_string());
                    reply.destination = Some(sender.to_string());
                    reply.body.push(DbusType::Array(owners));
                    reply.signature = Some("as".to_string());
                    responses.push(reply);
                } else {
                    responses.push(DbusMessage::error(
                        serial,
                        msg.serial,
                        "org.freedesktop.DBus.Error.NameHasNoOwner",
                        &format!("name '{name}' not found"),
                    ));
                }
            }

            "StartServiceByName" => {
                // We don't support service activation yet.
                let serial = self.alloc_serial();
                responses.push(DbusMessage::error(
                    serial,
                    msg.serial,
                    "org.freedesktop.DBus.Error.ServiceUnknown",
                    "service activation not supported",
                ));
            }

            "GetConnectionUnixUser" | "GetConnectionUnixProcessID"
            | "GetConnectionCredentials" => {
                // Return dummy values (no real Unix underneath).
                let serial = self.alloc_serial();
                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());

                match member {
                    "GetConnectionUnixUser" => {
                        reply.body.push(DbusType::Uint32(0)); // root
                        reply.signature = Some("u".to_string());
                    }
                    "GetConnectionUnixProcessID" => {
                        reply.body.push(DbusType::Uint32(1)); // init
                        reply.signature = Some("u".to_string());
                    }
                    _ => {
                        // GetConnectionCredentials returns a{sv}
                        let entries = vec![
                            DbusType::DictEntry(
                                Box::new(DbusType::String("UnixUserID".to_string())),
                                Box::new(DbusType::Variant(Box::new(DbusType::Uint32(0)))),
                            ),
                            DbusType::DictEntry(
                                Box::new(DbusType::String("ProcessID".to_string())),
                                Box::new(DbusType::Variant(Box::new(DbusType::Uint32(1)))),
                            ),
                        ];
                        reply.body.push(DbusType::Array(entries));
                        reply.signature = Some("a{sv}".to_string());
                    }
                }
                responses.push(reply);
            }

            _ => {
                let serial = self.alloc_serial();
                responses.push(DbusMessage::error(
                    serial,
                    msg.serial,
                    "org.freedesktop.DBus.Error.UnknownMethod",
                    &format!("unknown method: {member}"),
                ));
            }
        }

        Ok(responses)
    }

    /// Handle introspection requests.
    fn handle_introspect(
        &mut self,
        msg: &DbusMessage,
        sender: &str,
        member: &str,
    ) -> Result<Vec<DbusMessage>, DbusError> {
        if member != "Introspect" {
            let serial = self.alloc_serial();
            return Ok(vec![DbusMessage::error(
                serial,
                msg.serial,
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("unknown method on Introspectable: {member}"),
            )]);
        }

        let xml = generate_introspection_xml();
        let serial = self.alloc_serial();
        let mut reply = DbusMessage::method_return(serial, msg.serial);
        reply.sender = Some(DBUS_BUS_NAME.to_string());
        reply.destination = Some(sender.to_string());
        reply.body.push(DbusType::String(xml));
        reply.signature = Some("s".to_string());
        Ok(vec![reply])
    }

    /// Handle properties interface requests.
    fn handle_properties(
        &mut self,
        msg: &DbusMessage,
        sender: &str,
        member: &str,
    ) -> Result<Vec<DbusMessage>, DbusError> {
        match member {
            "Get" => {
                let iface = match msg.body.first() {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "Get requires (ss) arguments".into(),
                        ));
                    }
                };
                let prop_name = match msg.body.get(1) {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "Get requires (ss) arguments".into(),
                        ));
                    }
                };

                let serial = self.alloc_serial();
                if iface != DBUS_INTERFACE {
                    return Ok(vec![DbusMessage::error(
                        serial,
                        msg.serial,
                        "org.freedesktop.DBus.Error.UnknownInterface",
                        &format!("unknown interface: {iface}"),
                    )]);
                }

                if let Some(val) = self.properties.get(&prop_name) {
                    let mut reply = DbusMessage::method_return(serial, msg.serial);
                    reply.sender = Some(DBUS_BUS_NAME.to_string());
                    reply.destination = Some(sender.to_string());
                    reply
                        .body
                        .push(DbusType::Variant(Box::new(val.clone())));
                    reply.signature = Some("v".to_string());
                    Ok(vec![reply])
                } else {
                    Ok(vec![DbusMessage::error(
                        serial,
                        msg.serial,
                        "org.freedesktop.DBus.Error.UnknownProperty",
                        &format!("unknown property: {prop_name}"),
                    )])
                }
            }

            "Set" => {
                let serial = self.alloc_serial();
                Ok(vec![DbusMessage::error(
                    serial,
                    msg.serial,
                    "org.freedesktop.DBus.Error.PropertyReadOnly",
                    "all bus daemon properties are read-only",
                )])
            }

            "GetAll" => {
                let iface = match msg.body.first() {
                    Some(DbusType::String(s)) => s.clone(),
                    _ => {
                        return Err(DbusError::ProtocolError(
                            "GetAll requires (s) argument".into(),
                        ));
                    }
                };

                let serial = self.alloc_serial();
                if iface != DBUS_INTERFACE && !iface.is_empty() {
                    return Ok(vec![DbusMessage::error(
                        serial,
                        msg.serial,
                        "org.freedesktop.DBus.Error.UnknownInterface",
                        &format!("unknown interface: {iface}"),
                    )]);
                }

                let mut entries = Vec::new();
                for (name, val) in &self.properties {
                    entries.push(DbusType::DictEntry(
                        Box::new(DbusType::String(name.clone())),
                        Box::new(DbusType::Variant(Box::new(val.clone()))),
                    ));
                }

                let mut reply = DbusMessage::method_return(serial, msg.serial);
                reply.sender = Some(DBUS_BUS_NAME.to_string());
                reply.destination = Some(sender.to_string());
                reply.body.push(DbusType::Array(entries));
                reply.signature = Some("a{sv}".to_string());
                Ok(vec![reply])
            }

            _ => {
                let serial = self.alloc_serial();
                Ok(vec![DbusMessage::error(
                    serial,
                    msg.serial,
                    "org.freedesktop.DBus.Error.UnknownMethod",
                    &format!("unknown method on Properties: {member}"),
                )])
            }
        }
    }

    /// Route a message to the appropriate destination.
    pub fn route_message(
        &self,
        msg: &DbusMessage,
    ) -> Vec<String> {
        let mut destinations = Vec::new();

        // Direct destination
        if let Some(ref dest) = msg.destination {
            if dest == DBUS_BUS_NAME {
                // Message to the daemon itself
                return vec![DBUS_BUS_NAME.to_string()];
            }

            // Resolve well-known name to unique name
            if let Some(owner) = self.names.get_name_owner(dest) {
                if !destinations.contains(&owner.to_string()) {
                    destinations.push(owner.to_string());
                }
            }
        }

        // For signals, also check match rules
        if msg.message_type == MSG_SIGNAL {
            for (unique, conn) in &self.connections {
                if destinations.contains(unique) {
                    continue;
                }
                for rule in &conn.match_rules {
                    if rule.matches(msg) {
                        destinations.push(unique.clone());
                        break;
                    }
                }
            }
        }

        destinations
    }
}

// ============================================================================
// Introspection XML Generation
// ============================================================================

fn generate_introspection_xml() -> String {
    let mut xml = String::new();
    xml.push_str("<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\"\n");
    xml.push_str("  \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n");
    xml.push_str("<node name=\"/org/freedesktop/DBus\">\n");

    // Main D-Bus interface
    xml.push_str("  <interface name=\"org.freedesktop.DBus\">\n");

    // Methods
    xml.push_str("    <method name=\"Hello\">\n");
    xml.push_str("      <arg direction=\"out\" type=\"s\" name=\"unique_name\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"RequestName\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"in\" type=\"u\" name=\"flags\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"u\" name=\"result\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"ReleaseName\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"u\" name=\"result\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"GetNameOwner\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"s\" name=\"owner\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"NameHasOwner\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"b\" name=\"has_owner\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"ListNames\">\n");
    xml.push_str("      <arg direction=\"out\" type=\"as\" name=\"names\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"ListActivatableNames\">\n");
    xml.push_str("      <arg direction=\"out\" type=\"as\" name=\"names\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"AddMatch\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"rule\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"RemoveMatch\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"rule\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"GetId\">\n");
    xml.push_str("      <arg direction=\"out\" type=\"s\" name=\"id\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"ListQueuedOwners\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"as\" name=\"owners\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"GetConnectionUnixUser\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"u\" name=\"uid\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"GetConnectionUnixProcessID\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"u\" name=\"pid\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"GetConnectionCredentials\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"a{sv}\" name=\"credentials\"/>\n");
    xml.push_str("    </method>\n");

    xml.push_str("    <method name=\"StartServiceByName\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg direction=\"in\" type=\"u\" name=\"flags\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"u\" name=\"result\"/>\n");
    xml.push_str("    </method>\n");

    // Signals
    xml.push_str("    <signal name=\"NameOwnerChanged\">\n");
    xml.push_str("      <arg type=\"s\" name=\"name\"/>\n");
    xml.push_str("      <arg type=\"s\" name=\"old_owner\"/>\n");
    xml.push_str("      <arg type=\"s\" name=\"new_owner\"/>\n");
    xml.push_str("    </signal>\n");

    xml.push_str("    <signal name=\"NameAcquired\">\n");
    xml.push_str("      <arg type=\"s\" name=\"name\"/>\n");
    xml.push_str("    </signal>\n");

    xml.push_str("    <signal name=\"NameLost\">\n");
    xml.push_str("      <arg type=\"s\" name=\"name\"/>\n");
    xml.push_str("    </signal>\n");

    // Properties
    xml.push_str("    <property name=\"Features\" type=\"as\" access=\"read\"/>\n");
    xml.push_str("    <property name=\"Interfaces\" type=\"as\" access=\"read\"/>\n");

    xml.push_str("  </interface>\n");

    // Introspectable interface
    xml.push_str("  <interface name=\"org.freedesktop.DBus.Introspectable\">\n");
    xml.push_str("    <method name=\"Introspect\">\n");
    xml.push_str("      <arg direction=\"out\" type=\"s\" name=\"xml_data\"/>\n");
    xml.push_str("    </method>\n");
    xml.push_str("  </interface>\n");

    // Properties interface
    xml.push_str("  <interface name=\"org.freedesktop.DBus.Properties\">\n");
    xml.push_str("    <method name=\"Get\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"interface_name\"/>\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"property_name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"v\" name=\"value\"/>\n");
    xml.push_str("    </method>\n");
    xml.push_str("    <method name=\"Set\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"interface_name\"/>\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"property_name\"/>\n");
    xml.push_str("      <arg direction=\"in\" type=\"v\" name=\"value\"/>\n");
    xml.push_str("    </method>\n");
    xml.push_str("    <method name=\"GetAll\">\n");
    xml.push_str("      <arg direction=\"in\" type=\"s\" name=\"interface_name\"/>\n");
    xml.push_str("      <arg direction=\"out\" type=\"a{sv}\" name=\"properties\"/>\n");
    xml.push_str("    </method>\n");
    xml.push_str("    <signal name=\"PropertiesChanged\">\n");
    xml.push_str("      <arg type=\"s\" name=\"interface_name\"/>\n");
    xml.push_str("      <arg type=\"a{sv}\" name=\"changed_properties\"/>\n");
    xml.push_str("      <arg type=\"as\" name=\"invalidated_properties\"/>\n");
    xml.push_str("    </signal>\n");
    xml.push_str("  </interface>\n");

    xml.push_str("</node>\n");
    xml
}

// ============================================================================
// Bus Configuration
// ============================================================================

/// Parsed bus configuration.
#[derive(Debug, Clone)]
pub struct BusConfig {
    pub bus_type: BusType,
    pub listen_address: String,
    pub auth_required: bool,
    pub max_connections: usize,
    pub max_message_size: u32,
    pub policies: Vec<PolicyRule>,
}

impl Default for BusConfig {
    fn default() -> Self {
        Self {
            bus_type: BusType::Session,
            listen_address: SESSION_SOCKET_DIR.to_string(),
            auth_required: true,
            max_connections: 256,
            max_message_size: MAX_MESSAGE_SIZE,
            policies: Vec::new(),
        }
    }
}

/// A policy rule from configuration.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub allow: bool,
    pub context: PolicyContext,
    pub own: Option<String>,
    pub send_destination: Option<String>,
    pub send_interface: Option<String>,
    pub send_member: Option<String>,
    pub receive_sender: Option<String>,
    pub receive_interface: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PolicyContext {
    Default,
    User(String),
    Group(String),
    AtConsole,
}

impl BusConfig {
    /// Parse a D-Bus configuration file (simplified XML-like format).
    pub fn parse_file(path: &Path) -> Result<Self, DbusError> {
        let content = fs::read_to_string(path).map_err(|e| {
            DbusError::ConfigError(format!("cannot read {}: {e}", path.display()))
        })?;
        Self::parse_str(&content)
    }

    /// Parse configuration from a string.
    pub fn parse_str(content: &str) -> Result<Self, DbusError> {
        let mut config = BusConfig::default();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("<!--") {
                continue;
            }

            if line.contains("<type>system</type>") {
                config.bus_type = BusType::System;
                config.listen_address = SYSTEM_SOCKET.to_string();
            } else if line.contains("<type>session</type>") {
                config.bus_type = BusType::Session;
                config.listen_address = SESSION_SOCKET_DIR.to_string();
            } else if line.contains("<listen>") {
                if let Some(addr) = extract_xml_text(line, "listen") {
                    config.listen_address = addr;
                }
            } else if line.contains("<limit name=\"max_incoming_bytes\">") {
                // Parse limits
                if let Some(val) = extract_xml_text(line, "limit") {
                    if let Ok(n) = val.parse::<u32>() {
                        config.max_message_size = n;
                    }
                }
            } else if line.contains("<limit name=\"max_connections\">") {
                if let Some(val) = extract_xml_text(line, "limit") {
                    if let Ok(n) = val.parse::<usize>() {
                        config.max_connections = n;
                    }
                }
            } else if line.contains("<allow") {
                if let Some(rule) = parse_policy_rule(line, true) {
                    config.policies.push(rule);
                }
            } else if line.contains("<deny") {
                if let Some(rule) = parse_policy_rule(line, false) {
                    config.policies.push(rule);
                }
            }
        }

        Ok(config)
    }

    /// Check if a name ownership is allowed by policy.
    pub fn check_own_policy(&self, name: &str) -> bool {
        let mut allowed = true; // default: allow
        for rule in &self.policies {
            if let Some(ref own_name) = rule.own {
                if own_name == "*" || own_name == name {
                    allowed = rule.allow;
                }
            }
        }
        allowed
    }

    /// Check if sending to a destination/interface/member is allowed.
    pub fn check_send_policy(
        &self,
        destination: Option<&str>,
        interface: Option<&str>,
        member: Option<&str>,
    ) -> bool {
        let mut allowed = true;
        for rule in &self.policies {
            let mut matches = true;
            if let Some(ref dest) = rule.send_destination {
                if destination != Some(dest.as_str()) && dest != "*" {
                    matches = false;
                }
            }
            if let Some(ref iface) = rule.send_interface {
                if interface != Some(iface.as_str()) && iface != "*" {
                    matches = false;
                }
            }
            if let Some(ref mem) = rule.send_member {
                if member != Some(mem.as_str()) && mem != "*" {
                    matches = false;
                }
            }
            // Only apply if there was at least one send_* constraint
            if rule.send_destination.is_some()
                || rule.send_interface.is_some()
                || rule.send_member.is_some()
            {
                if matches {
                    allowed = rule.allow;
                }
            }
        }
        allowed
    }
}

/// Extract text between XML tags like `<tag>text</tag>`.
fn extract_xml_text(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = line.find(&open).map(|i| i + open.len())?;
    let end = line.find(&close)?;
    if start <= end {
        Some(line[start..end].trim().to_string())
    } else {
        None
    }
}

/// Parse a simple policy rule from an XML-like line.
fn parse_policy_rule(line: &str, allow: bool) -> Option<PolicyRule> {
    let mut rule = PolicyRule {
        allow,
        context: PolicyContext::Default,
        own: None,
        send_destination: None,
        send_interface: None,
        send_member: None,
        receive_sender: None,
        receive_interface: None,
    };

    if let Some(val) = extract_xml_attr(line, "own") {
        rule.own = Some(val);
    }
    if let Some(val) = extract_xml_attr(line, "send_destination") {
        rule.send_destination = Some(val);
    }
    if let Some(val) = extract_xml_attr(line, "send_interface") {
        rule.send_interface = Some(val);
    }
    if let Some(val) = extract_xml_attr(line, "send_member") {
        rule.send_member = Some(val);
    }
    if let Some(val) = extract_xml_attr(line, "receive_sender") {
        rule.receive_sender = Some(val);
    }
    if let Some(val) = extract_xml_attr(line, "receive_interface") {
        rule.receive_interface = Some(val);
    }

    // Only return if there's at least one constraint
    if rule.own.is_some()
        || rule.send_destination.is_some()
        || rule.send_interface.is_some()
        || rule.send_member.is_some()
        || rule.receive_sender.is_some()
        || rule.receive_interface.is_some()
    {
        Some(rule)
    } else {
        None
    }
}

/// Extract an attribute value from an XML-like tag.
fn extract_xml_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = line.find(&pattern)?;
    let val_start = start + pattern.len();
    let rest = &line[val_start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// ============================================================================
// dbus-send CLI
// ============================================================================

/// Parse command-line arguments for dbus-send.
struct SendArgs {
    system: bool,
    session: bool,
    dest: String,
    print_reply: bool,
    object_path: String,
    method: String,
    args: Vec<DbusType>,
    msg_type: u8,
}

fn parse_send_args(args: &[String]) -> Result<SendArgs, String> {
    let mut sa = SendArgs {
        system: false,
        session: true,
        dest: String::new(),
        print_reply: false,
        object_path: String::new(),
        method: String::new(),
        args: Vec::new(),
        msg_type: MSG_METHOD_CALL,
    };

    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--system" {
            sa.system = true;
            sa.session = false;
        } else if arg == "--session" {
            sa.session = true;
            sa.system = false;
        } else if arg == "--print-reply" {
            sa.print_reply = true;
        } else if arg.starts_with("--dest=") {
            sa.dest = arg[7..].to_string();
        } else if arg == "--type=method_call" {
            sa.msg_type = MSG_METHOD_CALL;
        } else if arg == "--type=signal" {
            sa.msg_type = MSG_SIGNAL;
        } else if arg.starts_with("--") {
            // ignore unknown flags
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }

    // positional: <object_path> <interface.member> [type:value ...]
    if positional.is_empty() {
        return Err("missing object path".into());
    }
    sa.object_path = positional[0].clone();

    if positional.len() >= 2 {
        sa.method = positional[1].clone();
    }

    for p in positional.iter().skip(2) {
        if let Some(val) = parse_typed_value(p) {
            sa.args.push(val);
        } else {
            return Err(format!("cannot parse argument: {p}"));
        }
    }

    Ok(sa)
}

/// Parse a typed value from "type:value" notation used by dbus-send.
fn parse_typed_value(s: &str) -> Option<DbusType> {
    let colon_pos = s.find(':')?;
    let type_name = &s[..colon_pos];
    let val_str = &s[colon_pos + 1..];

    match type_name {
        "string" => Some(DbusType::String(val_str.to_string())),
        "byte" => val_str.parse::<u8>().ok().map(DbusType::Byte),
        "boolean" | "bool" => match val_str {
            "true" | "1" => Some(DbusType::Boolean(true)),
            "false" | "0" => Some(DbusType::Boolean(false)),
            _ => None,
        },
        "int16" => val_str.parse::<i16>().ok().map(DbusType::Int16),
        "uint16" => val_str.parse::<u16>().ok().map(DbusType::Uint16),
        "int32" => val_str.parse::<i32>().ok().map(DbusType::Int32),
        "uint32" => val_str.parse::<u32>().ok().map(DbusType::Uint32),
        "int64" => val_str.parse::<i64>().ok().map(DbusType::Int64),
        "uint64" => val_str.parse::<u64>().ok().map(DbusType::Uint64),
        "double" => val_str.parse::<f64>().ok().map(DbusType::Double),
        "objpath" | "object_path" => Some(DbusType::ObjectPath(val_str.to_string())),
        "variant" => {
            // For simple variant encoding: "variant:type:value"
            parse_typed_value(val_str).map(|v| DbusType::Variant(Box::new(v)))
        }
        _ => None,
    }
}

/// Run the dbus-send functionality.
fn run_dbus_send(args: &[String]) -> i32 {
    let sa = match parse_send_args(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("dbus-send: {e}");
            eprintln!("Usage: dbus-send [--system|--session] --dest=NAME [--print-reply] PATH INTERFACE.MEMBER [type:value ...]");
            return 1;
        }
    };

    // Split method into interface + member
    let (interface, member) = match sa.method.rfind('.') {
        Some(pos) => (&sa.method[..pos], &sa.method[pos + 1..]),
        None => ("", sa.method.as_str()),
    };

    // Build the message
    let mut msg = DbusMessage::new(sa.msg_type, 1);
    msg.path = Some(sa.object_path.clone());
    if !interface.is_empty() {
        msg.interface = Some(interface.to_string());
    }
    msg.member = Some(member.to_string());
    if !sa.dest.is_empty() {
        msg.destination = Some(sa.dest.clone());
    }
    msg.body = sa.args;
    msg.signature = Some(msg.compute_signature());

    // In a real implementation, we'd connect to the bus socket and send.
    // For now, marshal and print the message.
    match msg.marshal() {
        Ok(bytes) => {
            if sa.print_reply {
                println!("method call sender=:1.0 -> dest={} serial=1 path={} interface={} member={}",
                    sa.dest, sa.object_path, interface, member);
                for val in &msg.body {
                    println!("   {val}");
                }
                println!("(message: {} bytes on wire)", bytes.len());
            } else {
                // Write raw bytes to stdout
                let _ = io::stdout().write_all(&bytes);
            }
            0
        }
        Err(e) => {
            eprintln!("dbus-send: failed to marshal message: {e}");
            1
        }
    }
}

// ============================================================================
// dbus-monitor CLI
// ============================================================================

/// Run the dbus-monitor functionality.
fn run_dbus_monitor(args: &[String]) -> i32 {
    let mut system = false;
    let mut rules = Vec::new();

    for arg in args {
        if arg == "--system" {
            system = true;
        } else if arg == "--session" {
            system = false;
        } else if arg.starts_with("--") {
            // ignore unknown flags
        } else {
            // Treat as match rule
            match MatchRule::parse(arg) {
                Ok(rule) => rules.push(rule),
                Err(e) => {
                    eprintln!("dbus-monitor: invalid match rule: {e}");
                    return 1;
                }
            }
        }
    }

    let bus_type = if system { "system" } else { "session" };
    println!("Monitoring {bus_type} bus traffic...");
    if !rules.is_empty() {
        for rule in &rules {
            println!("  match: {}", rule.to_rule_string());
        }
    }
    println!("(In full implementation, would connect to bus and display messages)");

    // In a real implementation, we'd connect to the bus, add match rules,
    // and print incoming messages. For now, just show we parsed everything.
    0
}

// ============================================================================
// dbus-daemon CLI
// ============================================================================

/// Run the dbus-daemon functionality.
fn run_dbus_daemon(args: &[String]) -> i32 {
    let mut bus_type = BusType::Session;
    let mut config_path: Option<String> = None;
    let mut print_address = false;
    let mut print_pid = false;
    let mut fork_mode = false;

    for arg in args {
        match arg.as_str() {
            "--system" => bus_type = BusType::System,
            "--session" => bus_type = BusType::Session,
            "--print-address" => print_address = true,
            "--print-pid" => print_pid = true,
            "--fork" => fork_mode = true,
            s if s.starts_with("--config-file=") => {
                config_path = Some(s[14..].to_string());
            }
            _ => {}
        }
    }

    // Load configuration
    let config = if let Some(ref path) = config_path {
        match BusConfig::parse_file(Path::new(path)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("dbus-daemon: {e}");
                return 1;
            }
        }
    } else {
        let default_path = match bus_type {
            BusType::System => SYSTEM_CONF,
            BusType::Session => SESSION_CONF,
        };
        BusConfig::parse_file(Path::new(default_path)).unwrap_or_else(|_| {
            let mut c = BusConfig::default();
            c.bus_type = bus_type;
            if bus_type == BusType::System {
                c.listen_address = SYSTEM_SOCKET.to_string();
            }
            c
        })
    };

    // Create the daemon
    let _daemon = BusDaemon::new(bus_type);

    // Print address/pid if requested
    if print_address {
        println!("unix:path={}", config.listen_address);
    }
    if print_pid {
        // In a real OS, we'd use getpid(). Use 1 as placeholder.
        println!("1");
    }

    // Ensure socket directory exists
    let socket_path = Path::new(&config.listen_address);
    if let Some(parent) = socket_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    println!(
        "dbus-daemon[1]: {bus_type} bus listening at {}",
        config.listen_address
    );
    println!("dbus-daemon[1]: max_connections={}", config.max_connections);

    if fork_mode {
        println!("dbus-daemon[1]: forking to background (simulated)");
    }

    // In a real implementation, we'd create a Unix domain socket, accept
    // connections, perform authentication, and route messages.
    // The daemon object is ready to handle messages from connected clients.

    // Write PID file for system bus
    if bus_type == BusType::System {
        let pid_dir = Path::new("/var/run/dbus");
        let _ = fs::create_dir_all(pid_dir);
        let _ = fs::write(pid_dir.join("pid"), "1\n");
    }

    // Event loop placeholder: in a real implementation this would use
    // epoll/io_uring to accept connections and route messages.
    println!("dbus-daemon[1]: ready");

    0
}

// ============================================================================
// Personality Detection & Entry Point
// ============================================================================

/// Determine which personality to run based on argv[0] or subcommand.
fn detect_personality(args: &[String]) -> &'static str {
    if let Some(arg0) = args.first() {
        let basename = arg0.rsplit('/').next().unwrap_or(arg0);
        let basename = basename.rsplit('\\').next().unwrap_or(basename);
        let basename = basename.strip_suffix(".exe").unwrap_or(basename);

        if basename.contains("dbus-send") {
            return "dbus-send";
        }
        if basename.contains("dbus-monitor") {
            return "dbus-monitor";
        }
        if basename.contains("dbus-daemon") {
            return "dbus-daemon";
        }
    }

    // Check for subcommand
    if let Some(sub) = args.get(1) {
        match sub.as_str() {
            "send" => return "dbus-send",
            "monitor" => return "dbus-monitor",
            "daemon" => return "dbus-daemon",
            _ => {}
        }
    }

    // Default to daemon
    "dbus-daemon"
}

fn run_main() -> i32 {
    let args: Vec<String> = env::args().collect();

    let personality = detect_personality(&args);

    // Strip the subcommand if present to pass remaining args
    let sub_args: Vec<String> = if args.len() > 1
        && matches!(args[1].as_str(), "send" | "monitor" | "daemon")
    {
        args[2..].to_vec()
    } else if args.len() > 1 {
        args[1..].to_vec()
    } else {
        Vec::new()
    };

    match personality {
        "dbus-send" => run_dbus_send(&sub_args),
        "dbus-monitor" => run_dbus_monitor(&sub_args),
        "dbus-daemon" => run_dbus_daemon(&sub_args),
        _ => {
            eprintln!("dbus: unknown personality");
            1
        }
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    run_main()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Type system tests ---

    #[test]
    fn test_byte_signature() {
        assert_eq!(DbusType::Byte(42).signature_str(), "y");
    }

    #[test]
    fn test_boolean_signature() {
        assert_eq!(DbusType::Boolean(true).signature_str(), "b");
    }

    #[test]
    fn test_int16_signature() {
        assert_eq!(DbusType::Int16(-100).signature_str(), "n");
    }

    #[test]
    fn test_uint16_signature() {
        assert_eq!(DbusType::Uint16(1000).signature_str(), "q");
    }

    #[test]
    fn test_int32_signature() {
        assert_eq!(DbusType::Int32(-50000).signature_str(), "i");
    }

    #[test]
    fn test_uint32_signature() {
        assert_eq!(DbusType::Uint32(100000).signature_str(), "u");
    }

    #[test]
    fn test_int64_signature() {
        assert_eq!(DbusType::Int64(-1_000_000).signature_str(), "x");
    }

    #[test]
    fn test_uint64_signature() {
        assert_eq!(DbusType::Uint64(1_000_000).signature_str(), "t");
    }

    #[test]
    fn test_double_signature() {
        assert_eq!(DbusType::Double(3.14).signature_str(), "d");
    }

    #[test]
    fn test_string_signature() {
        assert_eq!(DbusType::String("hello".into()).signature_str(), "s");
    }

    #[test]
    fn test_object_path_signature() {
        assert_eq!(DbusType::ObjectPath("/foo".into()).signature_str(), "o");
    }

    #[test]
    fn test_signature_signature() {
        assert_eq!(DbusType::Signature("si".into()).signature_str(), "g");
    }

    #[test]
    fn test_array_signature() {
        let arr = DbusType::Array(vec![DbusType::String("a".into())]);
        assert_eq!(arr.signature_str(), "as");
    }

    #[test]
    fn test_struct_signature() {
        let s = DbusType::Struct(vec![
            DbusType::String("a".into()),
            DbusType::Int32(1),
        ]);
        assert_eq!(s.signature_str(), "(si)");
    }

    #[test]
    fn test_variant_signature() {
        let v = DbusType::Variant(Box::new(DbusType::String("x".into())));
        assert_eq!(v.signature_str(), "v");
    }

    #[test]
    fn test_dict_entry_signature() {
        let de = DbusType::DictEntry(
            Box::new(DbusType::String("key".into())),
            Box::new(DbusType::Int32(42)),
        );
        assert_eq!(de.signature_str(), "{si}");
    }

    #[test]
    fn test_unix_fd_signature() {
        assert_eq!(DbusType::UnixFd(0).signature_str(), "h");
    }

    // --- Marshaling tests ---

    #[test]
    fn test_marshal_byte() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_byte(0xAB);
        assert_eq!(buf.into_bytes(), vec![0xAB]);
    }

    #[test]
    fn test_marshal_boolean_true() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_boolean(true);
        assert_eq!(buf.into_bytes(), vec![1, 0, 0, 0]);
    }

    #[test]
    fn test_marshal_boolean_false() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_boolean(false);
        assert_eq!(buf.into_bytes(), vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_marshal_u32_little_endian() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_u32(0x12345678);
        assert_eq!(buf.into_bytes(), vec![0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn test_marshal_u32_big_endian() {
        let mut buf = MarshalBuffer::new(Endianness::Big);
        buf.write_u32(0x12345678);
        assert_eq!(buf.into_bytes(), vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_marshal_i16() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_i16(-1);
        assert_eq!(buf.into_bytes(), vec![0xFF, 0xFF]);
    }

    #[test]
    fn test_marshal_u64() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_u64(1);
        assert_eq!(buf.into_bytes(), vec![1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_marshal_f64() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_f64(1.0);
        let bytes = buf.into_bytes();
        assert_eq!(bytes.len(), 8);
        assert_eq!(f64::from_le_bytes(bytes.try_into().unwrap()), 1.0);
    }

    #[test]
    fn test_marshal_string() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_string("Hi");
        let bytes = buf.into_bytes();
        // u32 len (2) + "Hi" + NUL = 4 + 2 + 1 = 7
        assert_eq!(bytes.len(), 7);
        assert_eq!(&bytes[0..4], &[2, 0, 0, 0]); // length
        assert_eq!(&bytes[4..6], b"Hi");
        assert_eq!(bytes[6], 0); // NUL
    }

    #[test]
    fn test_marshal_signature() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_signature("su");
        let bytes = buf.into_bytes();
        // u8 len (2) + "su" + NUL = 1 + 2 + 1 = 4
        assert_eq!(bytes.len(), 4);
        assert_eq!(bytes[0], 2); // length
        assert_eq!(&bytes[1..3], b"su");
        assert_eq!(bytes[3], 0);
    }

    #[test]
    fn test_marshal_alignment() {
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_byte(1);
        buf.align_to(4);
        assert_eq!(buf.len(), 4); // 1 byte + 3 padding
        buf.write_u32(42);
        assert_eq!(buf.len(), 8);
    }

    // --- Unmarshal tests ---

    #[test]
    fn test_unmarshal_byte() {
        let data = [0xCD];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert_eq!(cursor.read_byte().unwrap(), 0xCD);
    }

    #[test]
    fn test_unmarshal_boolean() {
        let data = [1, 0, 0, 0];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert!(cursor.read_boolean().unwrap());
    }

    #[test]
    fn test_unmarshal_invalid_boolean() {
        let data = [2, 0, 0, 0];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert!(cursor.read_boolean().is_err());
    }

    #[test]
    fn test_unmarshal_u32_little() {
        let data = [0x78, 0x56, 0x34, 0x12];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert_eq!(cursor.read_u32().unwrap(), 0x12345678);
    }

    #[test]
    fn test_unmarshal_u32_big() {
        let data = [0x12, 0x34, 0x56, 0x78];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Big);
        assert_eq!(cursor.read_u32().unwrap(), 0x12345678);
    }

    #[test]
    fn test_unmarshal_string() {
        let data = [2, 0, 0, 0, b'O', b'K', 0];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert_eq!(cursor.read_string().unwrap(), "OK");
    }

    #[test]
    fn test_unmarshal_signature() {
        let data = [2, b's', b'u', 0];
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert_eq!(cursor.read_signature().unwrap(), "su");
    }

    #[test]
    fn test_unmarshal_truncated() {
        let data = [1, 0]; // too short for u32
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert!(cursor.read_u32().is_err());
    }

    #[test]
    fn test_unmarshal_i64() {
        let val: i64 = -12345;
        let data = val.to_le_bytes();
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        assert_eq!(cursor.read_i64().unwrap(), val);
    }

    #[test]
    fn test_unmarshal_f64() {
        let val: f64 = 2.718281828;
        let data = val.to_le_bytes();
        let mut cursor = UnmarshalCursor::new(&data, Endianness::Little);
        let read = cursor.read_f64().unwrap();
        assert!((read - val).abs() < 1e-9);
    }

    // --- Roundtrip marshal/unmarshal ---

    #[test]
    fn test_roundtrip_string_value() {
        let val = DbusType::String("hello world".into());
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_value(&val);
        let bytes = buf.into_bytes();

        let mut cursor = UnmarshalCursor::new(&bytes, Endianness::Little);
        let mut sig: &[u8] = b"s";
        let result = cursor.read_value(&mut sig).unwrap();
        assert_eq!(result, val);
    }

    #[test]
    fn test_roundtrip_u32_value() {
        let val = DbusType::Uint32(99999);
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_value(&val);
        let bytes = buf.into_bytes();

        let mut cursor = UnmarshalCursor::new(&bytes, Endianness::Little);
        let mut sig: &[u8] = b"u";
        let result = cursor.read_value(&mut sig).unwrap();
        assert_eq!(result, val);
    }

    #[test]
    fn test_roundtrip_variant() {
        let val = DbusType::Variant(Box::new(DbusType::Int32(-42)));
        let mut buf = MarshalBuffer::new(Endianness::Little);
        buf.write_value(&val);
        let bytes = buf.into_bytes();

        let mut cursor = UnmarshalCursor::new(&bytes, Endianness::Little);
        let mut sig: &[u8] = b"v";
        let result = cursor.read_value(&mut sig).unwrap();
        assert_eq!(result, val);
    }

    // --- Message construction ---

    #[test]
    fn test_method_call_creation() {
        let msg = DbusMessage::method_call(1, "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello");
        assert_eq!(msg.message_type, MSG_METHOD_CALL);
        assert_eq!(msg.serial, 1);
        assert_eq!(msg.path.as_deref(), Some("/org/freedesktop/DBus"));
        assert_eq!(msg.interface.as_deref(), Some("org.freedesktop.DBus"));
        assert_eq!(msg.member.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_method_return_creation() {
        let msg = DbusMessage::method_return(2, 1);
        assert_eq!(msg.message_type, MSG_METHOD_RETURN);
        assert_eq!(msg.reply_serial, Some(1));
    }

    #[test]
    fn test_error_creation() {
        let msg = DbusMessage::error(3, 1, "org.freedesktop.DBus.Error.Failed", "oops");
        assert_eq!(msg.message_type, MSG_ERROR);
        assert_eq!(msg.error_name.as_deref(), Some("org.freedesktop.DBus.Error.Failed"));
        assert_eq!(msg.reply_serial, Some(1));
        assert_eq!(msg.body.len(), 1);
    }

    #[test]
    fn test_signal_creation() {
        let msg = DbusMessage::signal(4, "/org/test", "org.test.Iface", "Changed");
        assert_eq!(msg.message_type, MSG_SIGNAL);
        assert_eq!(msg.path.as_deref(), Some("/org/test"));
    }

    #[test]
    fn test_message_type_name() {
        assert_eq!(DbusMessage::new(MSG_METHOD_CALL, 1).type_name(), "method_call");
        assert_eq!(DbusMessage::new(MSG_METHOD_RETURN, 1).type_name(), "method_return");
        assert_eq!(DbusMessage::new(MSG_ERROR, 1).type_name(), "error");
        assert_eq!(DbusMessage::new(MSG_SIGNAL, 1).type_name(), "signal");
        assert_eq!(DbusMessage::new(MSG_INVALID, 1).type_name(), "invalid");
    }

    #[test]
    fn test_compute_signature() {
        let mut msg = DbusMessage::new(MSG_METHOD_CALL, 1);
        msg.body.push(DbusType::String("test".into()));
        msg.body.push(DbusType::Uint32(42));
        assert_eq!(msg.compute_signature(), "su");
    }

    // --- Message marshal/unmarshal roundtrip ---

    #[test]
    fn test_message_marshal_unmarshal() {
        let mut msg = DbusMessage::method_call(42, "/test/path", "org.test.Iface", "DoSomething");
        msg.destination = Some("org.test.Service".to_string());
        msg.sender = Some(":1.99".to_string());
        msg.body.push(DbusType::String("arg1".into()));
        msg.body.push(DbusType::Int32(123));
        msg.signature = Some("si".to_string());

        let bytes = msg.marshal().unwrap();
        assert!(bytes.len() >= 16);

        let decoded = DbusMessage::unmarshal(&bytes).unwrap();
        assert_eq!(decoded.message_type, MSG_METHOD_CALL);
        assert_eq!(decoded.serial, 42);
        assert_eq!(decoded.path.as_deref(), Some("/test/path"));
        assert_eq!(decoded.interface.as_deref(), Some("org.test.Iface"));
        assert_eq!(decoded.member.as_deref(), Some("DoSomething"));
        assert_eq!(decoded.destination.as_deref(), Some("org.test.Service"));
        assert_eq!(decoded.sender.as_deref(), Some(":1.99"));
    }

    #[test]
    fn test_message_unmarshal_too_short() {
        let data = [0u8; 8];
        assert!(DbusMessage::unmarshal(&data).is_err());
    }

    #[test]
    fn test_message_unmarshal_invalid_endian() {
        let mut data = [0u8; 16];
        data[0] = b'X'; // invalid endian marker
        assert!(DbusMessage::unmarshal(&data).is_err());
    }

    // --- Validation tests ---

    #[test]
    fn test_valid_bus_names() {
        assert!(validate_bus_name("org.freedesktop.DBus").is_ok());
        assert!(validate_bus_name("com.example.Service").is_ok());
        assert!(validate_bus_name(":1.42").is_ok());
        assert!(validate_bus_name("org.test-with-dashes").is_ok());
    }

    #[test]
    fn test_invalid_bus_names() {
        assert!(validate_bus_name("").is_err());
        assert!(validate_bus_name("singleelement").is_err());
        assert!(validate_bus_name("org..double").is_err());
        assert!(validate_bus_name("org.1starts_digit").is_err());
    }

    #[test]
    fn test_valid_object_paths() {
        assert!(validate_object_path("/").is_ok());
        assert!(validate_object_path("/org/freedesktop/DBus").is_ok());
        assert!(validate_object_path("/a/b/c").is_ok());
    }

    #[test]
    fn test_invalid_object_paths() {
        assert!(validate_object_path("").is_err());
        assert!(validate_object_path("no_slash").is_err());
        assert!(validate_object_path("/trailing/").is_err());
        assert!(validate_object_path("/double//slash").is_err());
    }

    #[test]
    fn test_valid_interface_names() {
        assert!(validate_interface_name("org.freedesktop.DBus").is_ok());
        assert!(validate_interface_name("com.example.Foo").is_ok());
    }

    #[test]
    fn test_invalid_interface_names() {
        assert!(validate_interface_name("").is_err());
        assert!(validate_interface_name("singleelement").is_err());
        assert!(validate_interface_name("org..empty").is_err());
    }

    #[test]
    fn test_valid_member_names() {
        assert!(validate_member_name("Hello").is_ok());
        assert!(validate_member_name("get_property").is_ok());
        assert!(validate_member_name("DoSomething123").is_ok());
    }

    #[test]
    fn test_invalid_member_names() {
        assert!(validate_member_name("").is_err());
        assert!(validate_member_name("1starts_digit").is_err());
        assert!(validate_member_name("has.dot").is_err());
    }

    #[test]
    fn test_validate_signature_basic() {
        assert!(validate_signature("s").is_ok());
        assert!(validate_signature("su").is_ok());
        assert!(validate_signature("a{sv}").is_ok());
        assert!(validate_signature("(siu)").is_ok());
    }

    #[test]
    fn test_validate_signature_invalid() {
        assert!(validate_signature("(").is_err()); // unmatched
        assert!(validate_signature("{s}").is_err()); // dict needs 2 types
        assert!(validate_signature("Z").is_err()); // unknown type
    }

    // --- Match rule tests ---

    #[test]
    fn test_parse_match_rule() {
        let rule = MatchRule::parse("type='signal',interface='org.test'").unwrap();
        assert_eq!(rule.msg_type, Some(MSG_SIGNAL));
        assert_eq!(rule.interface.as_deref(), Some("org.test"));
    }

    #[test]
    fn test_match_rule_matches_signal() {
        let rule = MatchRule::parse("type='signal',interface='org.test'").unwrap();
        let msg = DbusMessage::signal(1, "/test", "org.test", "Changed");
        assert!(rule.matches(&msg));
    }

    #[test]
    fn test_match_rule_no_match_wrong_type() {
        let rule = MatchRule::parse("type='signal'").unwrap();
        let msg = DbusMessage::new(MSG_METHOD_CALL, 1);
        assert!(!rule.matches(&msg));
    }

    #[test]
    fn test_match_rule_sender_filter() {
        let rule = MatchRule::parse("sender=':1.5'").unwrap();
        let mut msg = DbusMessage::signal(1, "/test", "org.test", "Foo");
        msg.sender = Some(":1.5".to_string());
        assert!(rule.matches(&msg));

        msg.sender = Some(":1.6".to_string());
        assert!(!rule.matches(&msg));
    }

    #[test]
    fn test_match_rule_path_namespace() {
        let rule = MatchRule::parse("path_namespace='/org/test'").unwrap();

        let mut msg = DbusMessage::signal(1, "/org/test", "org.test", "X");
        assert!(rule.matches(&msg));

        msg.path = Some("/org/test/sub".to_string());
        assert!(rule.matches(&msg));

        msg.path = Some("/org/other".to_string());
        assert!(!rule.matches(&msg));
    }

    #[test]
    fn test_match_rule_roundtrip() {
        let rule = MatchRule::parse("type='signal',interface='org.test',member='Changed'").unwrap();
        let s = rule.to_rule_string();
        assert!(s.contains("type='signal'"));
        assert!(s.contains("interface='org.test'"));
        assert!(s.contains("member='Changed'"));
    }

    #[test]
    fn test_match_rule_arg0() {
        let rule = MatchRule::parse("arg0='hello'").unwrap();
        let mut msg = DbusMessage::signal(1, "/", "org.a.B", "C");
        msg.body.push(DbusType::String("hello".into()));
        assert!(rule.matches(&msg));

        msg.body.clear();
        msg.body.push(DbusType::String("world".into()));
        assert!(!rule.matches(&msg));
    }

    // --- Name registry tests ---

    #[test]
    fn test_allocate_unique_names() {
        let mut reg = NameRegistry::new();
        assert_eq!(reg.allocate_unique_name(), ":1.1");
        assert_eq!(reg.allocate_unique_name(), ":1.2");
        assert_eq!(reg.allocate_unique_name(), ":1.3");
    }

    #[test]
    fn test_request_name_primary() {
        let mut reg = NameRegistry::new();
        let (code, _) = reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        assert_eq!(code, NAME_REPLY_PRIMARY_OWNER);
    }

    #[test]
    fn test_request_name_already_owner() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        let (code, _) = reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        assert_eq!(code, NAME_REPLY_ALREADY_OWNER);
    }

    #[test]
    fn test_request_name_queued() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        let (code, _) = reg.request_name("org.test.Foo", ":1.2", 0).unwrap();
        assert_eq!(code, NAME_REPLY_IN_QUEUE);
    }

    #[test]
    fn test_request_name_do_not_queue() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        let (code, _) = reg.request_name("org.test.Foo", ":1.2", NAME_FLAG_DO_NOT_QUEUE).unwrap();
        assert_eq!(code, NAME_REPLY_EXISTS);
    }

    #[test]
    fn test_request_name_replace() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", NAME_FLAG_ALLOW_REPLACEMENT).unwrap();
        let (code, old) = reg.request_name("org.test.Foo", ":1.2", NAME_FLAG_REPLACE_EXISTING).unwrap();
        assert_eq!(code, NAME_REPLY_PRIMARY_OWNER);
        assert_eq!(old.as_deref(), Some(":1.1"));
    }

    #[test]
    fn test_release_name() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        let (code, _) = reg.release_name("org.test.Foo", ":1.1").unwrap();
        assert_eq!(code, NAME_RELEASE_REPLY_RELEASED);
        assert!(!reg.name_has_owner("org.test.Foo"));
    }

    #[test]
    fn test_release_name_not_owner() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        let (code, _) = reg.release_name("org.test.Foo", ":1.2").unwrap();
        assert_eq!(code, NAME_RELEASE_REPLY_NOT_OWNER);
    }

    #[test]
    fn test_release_name_nonexistent() {
        let mut reg = NameRegistry::new();
        let (code, _) = reg.release_name("org.test.Gone", ":1.1").unwrap();
        assert_eq!(code, NAME_RELEASE_REPLY_NON_EXISTENT);
    }

    #[test]
    fn test_release_name_transfers_to_queue() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        reg.request_name("org.test.Foo", ":1.2", 0).unwrap();
        let (code, new_owner) = reg.release_name("org.test.Foo", ":1.1").unwrap();
        assert_eq!(code, NAME_RELEASE_REPLY_RELEASED);
        assert_eq!(new_owner.as_deref(), Some(":1.2"));
        assert_eq!(reg.get_name_owner("org.test.Foo"), Some(":1.2"));
    }

    #[test]
    fn test_get_name_owner() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.5", 0).unwrap();
        assert_eq!(reg.get_name_owner("org.test.Foo"), Some(":1.5"));
        assert_eq!(reg.get_name_owner("org.test.Missing"), None);
    }

    #[test]
    fn test_unique_name_owns_itself() {
        let reg = NameRegistry::new();
        assert_eq!(reg.get_name_owner(":1.42"), Some(":1.42"));
        assert!(reg.name_has_owner(":1.42"));
    }

    #[test]
    fn test_list_names() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.B", ":1.1", 0).unwrap();
        reg.request_name("org.test.A", ":1.2", 0).unwrap();
        let names = reg.list_names();
        assert!(names.contains(&DBUS_BUS_NAME.to_string()));
        assert!(names.contains(&"org.test.A".to_string()));
        assert!(names.contains(&"org.test.B".to_string()));
    }

    #[test]
    fn test_remove_connection() {
        let mut reg = NameRegistry::new();
        reg.request_name("org.test.Foo", ":1.1", 0).unwrap();
        reg.request_name("org.test.Bar", ":1.1", 0).unwrap();
        let changes = reg.remove_connection(":1.1");
        assert_eq!(changes.len(), 2);
        assert!(!reg.name_has_owner("org.test.Foo"));
        assert!(!reg.name_has_owner("org.test.Bar"));
    }

    // --- Bus daemon tests ---

    #[test]
    fn test_daemon_register_connection() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let name = daemon.register_connection();
        assert!(name.starts_with(":1."));
        assert!(daemon.connections.contains_key(&name));
    }

    #[test]
    fn test_daemon_hello() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();
        let msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "Hello");
        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert!(responses.len() >= 2); // reply + NameAcquired signal
        assert_eq!(responses[0].message_type, MSG_METHOD_RETURN);
    }

    #[test]
    fn test_daemon_request_name() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let mut msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "RequestName");
        msg.body.push(DbusType::String("org.test.MyService".into()));
        msg.body.push(DbusType::Uint32(0));
        msg.signature = Some("su".to_string());

        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        // Should have reply + NameOwnerChanged + NameAcquired
        assert!(!responses.is_empty());

        let reply = &responses[0];
        assert_eq!(reply.message_type, MSG_METHOD_RETURN);
        assert_eq!(reply.body[0], DbusType::Uint32(NAME_REPLY_PRIMARY_OWNER));
    }

    #[test]
    fn test_daemon_list_names() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "ListNames");
        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses.len(), 1);
        if let DbusType::Array(items) = &responses[0].body[0] {
            let names: Vec<&str> = items.iter().filter_map(|v| {
                if let DbusType::String(s) = v { Some(s.as_str()) } else { None }
            }).collect();
            assert!(names.contains(&DBUS_BUS_NAME));
        } else {
            panic!("expected array");
        }
    }

    #[test]
    fn test_daemon_get_id() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "GetId");
        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].body[0], DbusType::String("ouros-dbus-00000001".into()));
    }

    #[test]
    fn test_daemon_name_has_owner() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let mut msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "NameHasOwner");
        msg.body.push(DbusType::String("org.nonexistent".into()));
        msg.signature = Some("s".into());

        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].body[0], DbusType::Boolean(false));
    }

    #[test]
    fn test_daemon_unknown_method() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "BogusMethod");
        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].message_type, MSG_ERROR);
    }

    #[test]
    fn test_daemon_introspect() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let msg = DbusMessage::method_call(1, DBUS_PATH, INTROSPECTABLE_IFACE, "Introspect");
        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses.len(), 1);
        if let DbusType::String(xml) = &responses[0].body[0] {
            assert!(xml.contains("<interface name=\"org.freedesktop.DBus\">"));
            assert!(xml.contains("<method name=\"Hello\">"));
        } else {
            panic!("expected string");
        }
    }

    #[test]
    fn test_daemon_properties_get() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let mut msg = DbusMessage::method_call(1, DBUS_PATH, PROPERTIES_IFACE, "Get");
        msg.body.push(DbusType::String(DBUS_INTERFACE.into()));
        msg.body.push(DbusType::String("Features".into()));
        msg.signature = Some("ss".into());

        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].message_type, MSG_METHOD_RETURN);
    }

    #[test]
    fn test_daemon_properties_set_readonly() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let mut msg = DbusMessage::method_call(1, DBUS_PATH, PROPERTIES_IFACE, "Set");
        msg.body.push(DbusType::String(DBUS_INTERFACE.into()));
        msg.body.push(DbusType::String("Features".into()));
        msg.body.push(DbusType::Variant(Box::new(DbusType::String("x".into()))));
        msg.signature = Some("ssv".into());

        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].message_type, MSG_ERROR);
    }

    #[test]
    fn test_daemon_properties_getall() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let mut msg = DbusMessage::method_call(1, DBUS_PATH, PROPERTIES_IFACE, "GetAll");
        msg.body.push(DbusType::String(DBUS_INTERFACE.into()));
        msg.signature = Some("s".into());

        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].message_type, MSG_METHOD_RETURN);
    }

    #[test]
    fn test_daemon_add_match() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        let mut msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "AddMatch");
        msg.body.push(DbusType::String("type='signal',interface='org.test'".into()));
        msg.signature = Some("s".into());

        let responses = daemon.handle_bus_message(&msg, &conn).unwrap();
        assert_eq!(responses[0].message_type, MSG_METHOD_RETURN);
        assert_eq!(daemon.connections[&conn].match_rules.len(), 1);
    }

    #[test]
    fn test_daemon_remove_match() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        // Add
        let mut msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "AddMatch");
        msg.body.push(DbusType::String("type='signal'".into()));
        msg.signature = Some("s".into());
        daemon.handle_bus_message(&msg, &conn).unwrap();

        // Remove
        let mut msg2 = DbusMessage::method_call(2, DBUS_PATH, DBUS_INTERFACE, "RemoveMatch");
        msg2.body.push(DbusType::String("type='signal'".into()));
        msg2.signature = Some("s".into());
        daemon.handle_bus_message(&msg2, &conn).unwrap();

        assert_eq!(daemon.connections[&conn].match_rules.len(), 0);
    }

    #[test]
    fn test_daemon_unregister_signals() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn = daemon.register_connection();

        // Acquire a name
        let mut msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "RequestName");
        msg.body.push(DbusType::String("org.test.Svc".into()));
        msg.body.push(DbusType::Uint32(0));
        msg.signature = Some("su".into());
        daemon.handle_bus_message(&msg, &conn).unwrap();

        // Unregister
        let signals = daemon.unregister_connection(&conn);
        // Should have NameOwnerChanged for the well-known name + unique name
        assert!(signals.len() >= 2);
        assert!(!daemon.connections.contains_key(&conn));
    }

    #[test]
    fn test_daemon_route_to_destination() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn1 = daemon.register_connection();
        let _conn2 = daemon.register_connection();

        // Route to bus daemon
        let msg = DbusMessage::method_call(1, DBUS_PATH, DBUS_INTERFACE, "Hello");
        let mut msg_with_dest = msg;
        msg_with_dest.destination = Some(DBUS_BUS_NAME.to_string());
        let dests = daemon.route_message(&msg_with_dest);
        assert_eq!(dests, vec![DBUS_BUS_NAME.to_string()]);

        // Route to specific connection
        let mut msg2 = DbusMessage::new(MSG_METHOD_CALL, 2);
        msg2.destination = Some(conn1.clone());
        let dests2 = daemon.route_message(&msg2);
        assert!(dests2.contains(&conn1));
    }

    #[test]
    fn test_daemon_signal_routing_via_match() {
        let mut daemon = BusDaemon::new(BusType::Session);
        let conn1 = daemon.register_connection();

        // Add a match rule
        if let Some(c) = daemon.connections.get_mut(&conn1) {
            c.match_rules.push(
                MatchRule::parse("type='signal',interface='org.test.Foo'").unwrap()
            );
        }

        let mut sig = DbusMessage::signal(10, "/test", "org.test.Foo", "Bar");
        sig.sender = Some(":1.99".to_string());
        let dests = daemon.route_message(&sig);
        assert!(dests.contains(&conn1));
    }

    // --- Configuration tests ---

    #[test]
    fn test_config_default() {
        let cfg = BusConfig::default();
        assert_eq!(cfg.bus_type, BusType::Session);
        assert_eq!(cfg.max_connections, 256);
    }

    #[test]
    fn test_config_parse_system() {
        let content = "<busconfig>\n  <type>system</type>\n</busconfig>\n";
        let cfg = BusConfig::parse_str(content).unwrap();
        assert_eq!(cfg.bus_type, BusType::System);
    }

    #[test]
    fn test_config_parse_session() {
        let content = "<busconfig>\n  <type>session</type>\n</busconfig>\n";
        let cfg = BusConfig::parse_str(content).unwrap();
        assert_eq!(cfg.bus_type, BusType::Session);
    }

    #[test]
    fn test_config_parse_listen() {
        let content = "<listen>unix:path=/tmp/test-bus</listen>\n";
        let cfg = BusConfig::parse_str(content).unwrap();
        assert_eq!(cfg.listen_address, "unix:path=/tmp/test-bus");
    }

    #[test]
    fn test_config_parse_policy_allow() {
        let content = "<allow own=\"org.test.Foo\"/>\n";
        let cfg = BusConfig::parse_str(content).unwrap();
        assert_eq!(cfg.policies.len(), 1);
        assert!(cfg.policies[0].allow);
        assert_eq!(cfg.policies[0].own.as_deref(), Some("org.test.Foo"));
    }

    #[test]
    fn test_config_parse_policy_deny() {
        let content = "<deny send_destination=\"org.test.Bar\"/>\n";
        let cfg = BusConfig::parse_str(content).unwrap();
        assert_eq!(cfg.policies.len(), 1);
        assert!(!cfg.policies[0].allow);
    }

    #[test]
    fn test_config_check_own_policy() {
        let mut cfg = BusConfig::default();
        cfg.policies.push(PolicyRule {
            allow: false,
            context: PolicyContext::Default,
            own: Some("org.test.Forbidden".into()),
            send_destination: None,
            send_interface: None,
            send_member: None,
            receive_sender: None,
            receive_interface: None,
        });
        assert!(!cfg.check_own_policy("org.test.Forbidden"));
        assert!(cfg.check_own_policy("org.test.Other"));
    }

    #[test]
    fn test_config_check_send_policy() {
        let mut cfg = BusConfig::default();
        cfg.policies.push(PolicyRule {
            allow: false,
            context: PolicyContext::Default,
            own: None,
            send_destination: Some("org.test.Blocked".into()),
            send_interface: None,
            send_member: None,
            receive_sender: None,
            receive_interface: None,
        });
        assert!(!cfg.check_send_policy(Some("org.test.Blocked"), None, None));
        assert!(cfg.check_send_policy(Some("org.test.OK"), None, None));
    }

    // --- Typed value parsing (dbus-send) ---

    #[test]
    fn test_parse_typed_string() {
        let v = parse_typed_value("string:hello").unwrap();
        assert_eq!(v, DbusType::String("hello".into()));
    }

    #[test]
    fn test_parse_typed_int32() {
        let v = parse_typed_value("int32:-42").unwrap();
        assert_eq!(v, DbusType::Int32(-42));
    }

    #[test]
    fn test_parse_typed_uint32() {
        let v = parse_typed_value("uint32:100").unwrap();
        assert_eq!(v, DbusType::Uint32(100));
    }

    #[test]
    fn test_parse_typed_boolean() {
        assert_eq!(parse_typed_value("boolean:true").unwrap(), DbusType::Boolean(true));
        assert_eq!(parse_typed_value("boolean:false").unwrap(), DbusType::Boolean(false));
        assert_eq!(parse_typed_value("bool:1").unwrap(), DbusType::Boolean(true));
    }

    #[test]
    fn test_parse_typed_byte() {
        assert_eq!(parse_typed_value("byte:255").unwrap(), DbusType::Byte(255));
    }

    #[test]
    fn test_parse_typed_double() {
        if let DbusType::Double(v) = parse_typed_value("double:3.14").unwrap() {
            assert!((v - 3.14).abs() < 0.001);
        } else {
            panic!("expected double");
        }
    }

    #[test]
    fn test_parse_typed_objpath() {
        let v = parse_typed_value("objpath:/org/test").unwrap();
        assert_eq!(v, DbusType::ObjectPath("/org/test".into()));
    }

    #[test]
    fn test_parse_typed_invalid() {
        assert!(parse_typed_value("nosuchtype:foo").is_none());
        assert!(parse_typed_value("nocolon").is_none());
    }

    // --- Personality detection ---

    #[test]
    fn test_detect_dbus_daemon() {
        let args = vec!["dbus-daemon".to_string(), "--session".to_string()];
        assert_eq!(detect_personality(&args), "dbus-daemon");
    }

    #[test]
    fn test_detect_dbus_send() {
        let args = vec!["dbus-send".to_string(), "--dest=org.foo".to_string()];
        assert_eq!(detect_personality(&args), "dbus-send");
    }

    #[test]
    fn test_detect_dbus_monitor() {
        let args = vec!["dbus-monitor".to_string()];
        assert_eq!(detect_personality(&args), "dbus-monitor");
    }

    #[test]
    fn test_detect_subcommand_send() {
        let args = vec!["dbus".to_string(), "send".to_string()];
        assert_eq!(detect_personality(&args), "dbus-send");
    }

    #[test]
    fn test_detect_subcommand_monitor() {
        let args = vec!["dbus".to_string(), "monitor".to_string()];
        assert_eq!(detect_personality(&args), "dbus-monitor");
    }

    #[test]
    fn test_detect_subcommand_daemon() {
        let args = vec!["dbus".to_string(), "daemon".to_string()];
        assert_eq!(detect_personality(&args), "dbus-daemon");
    }

    #[test]
    fn test_detect_default() {
        let args = vec!["dbus".to_string()];
        assert_eq!(detect_personality(&args), "dbus-daemon");
    }

    // --- Display / formatting ---

    #[test]
    fn test_dbustype_display() {
        assert_eq!(format!("{}", DbusType::Byte(42)), "byte 42");
        assert_eq!(format!("{}", DbusType::Boolean(true)), "boolean true");
        assert_eq!(format!("{}", DbusType::String("hi".into())), "string \"hi\"");
        assert_eq!(format!("{}", DbusType::ObjectPath("/x".into())), "object_path \"/x\"");
    }

    #[test]
    fn test_message_display() {
        let msg = DbusMessage::method_call(1, "/test", "org.test.I", "M");
        let s = format!("{msg}");
        assert!(s.contains("method_call"));
        assert!(s.contains("/test"));
        assert!(s.contains("org.test.I"));
    }

    #[test]
    fn test_error_display() {
        let e = DbusError::InvalidBusName("bad".into());
        assert!(format!("{e}").contains("bad"));
    }

    // --- Endianness ---

    #[test]
    fn test_endianness_marker_roundtrip() {
        assert_eq!(Endianness::from_marker(Endianness::Little.marker()), Some(Endianness::Little));
        assert_eq!(Endianness::from_marker(Endianness::Big.marker()), Some(Endianness::Big));
        assert_eq!(Endianness::from_marker(b'X'), None);
    }

    // --- Introspection ---

    #[test]
    fn test_introspection_xml_valid() {
        let xml = generate_introspection_xml();
        assert!(xml.contains("<!DOCTYPE node"));
        assert!(xml.contains("<node name="));
        assert!(xml.contains("</node>"));
        assert!(xml.contains("org.freedesktop.DBus"));
        assert!(xml.contains("org.freedesktop.DBus.Introspectable"));
        assert!(xml.contains("org.freedesktop.DBus.Properties"));
    }

    // --- Marshal buffer ---

    #[test]
    fn test_marshal_buffer_empty() {
        let buf = MarshalBuffer::new(Endianness::Little);
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_extract_xml_text() {
        assert_eq!(extract_xml_text("<type>system</type>", "type"), Some("system".into()));
        assert_eq!(extract_xml_text("<type>session</type>", "type"), Some("session".into()));
        assert_eq!(extract_xml_text("no tags here", "type"), None);
    }

    #[test]
    fn test_extract_xml_attr() {
        assert_eq!(
            extract_xml_attr("<allow own=\"org.test\"/>", "own"),
            Some("org.test".into())
        );
        assert_eq!(extract_xml_attr("<deny/>", "own"), None);
    }

    #[test]
    fn test_wire_alignment() {
        assert_eq!(wire_alignment_for_sig_char(b'y'), 1);
        assert_eq!(wire_alignment_for_sig_char(b'n'), 2);
        assert_eq!(wire_alignment_for_sig_char(b'i'), 4);
        assert_eq!(wire_alignment_for_sig_char(b's'), 4);
        assert_eq!(wire_alignment_for_sig_char(b'x'), 8);
        assert_eq!(wire_alignment_for_sig_char(b'd'), 8);
        assert_eq!(wire_alignment_for_sig_char(b'('), 8);
    }
}
