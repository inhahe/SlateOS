//! OurOS Service Bus Daemon
//!
//! A D-Bus-like named service registry and message routing daemon. Programs use
//! this to discover system services by well-known name, introspect their typed
//! interfaces, and exchange RPC messages.
//!
//! # Architecture
//!
//! The service bus maintains:
//! - A registry of named services with typed interface descriptions
//! - A connection table mapping unique names (`:1.N`) to connected clients
//! - A message router that dispatches method calls, returns, errors, and signals
//! - An activation system that can start services on demand
//! - Signal subscription matching with flexible filter rules
//!
//! # Wire Format
//!
//! Messages use a simple binary format with natural alignment for each type,
//! length-prefixed strings and arrays, and a fixed-size header followed by a
//! serialized body.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::too_many_lines)]
// Pedantic lints we relax for this daemon — they flag style/doc rather than bugs:
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::unnecessary_wraps)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Type System
// ---------------------------------------------------------------------------

/// Describes a type in the service bus wire protocol.
///
/// Used for interface introspection and message validation. Each variant
/// corresponds to a wire format encoding with defined alignment and size rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDesc {
    /// No value (used for methods with no return).
    Void,
    /// Boolean value (1 byte on wire).
    Bool,
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
    /// Signed 8-bit integer.
    I8,
    /// Signed 16-bit integer.
    I16,
    /// Signed 32-bit integer.
    I32,
    /// Signed 64-bit integer.
    I64,
    /// 32-bit floating point.
    F32,
    /// 64-bit floating point.
    F64,
    /// UTF-8 string (length-prefixed on wire).
    String,
    /// Raw byte array (length-prefixed on wire).
    Bytes,
    /// Homogeneous array of elements.
    Array(Box<TypeDesc>),
    /// Key-value map.
    Map(Box<TypeDesc>, Box<TypeDesc>),
    /// Named struct with ordered fields.
    Struct(Vec<(String, TypeDesc)>),
    /// Optional value (nullable).
    Optional(Box<TypeDesc>),
    /// Capability handle for cross-process transfer.
    Handle,
}

impl fmt::Display for TypeDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Void => write!(f, "void"),
            Self::Bool => write!(f, "bool"),
            Self::U8 => write!(f, "u8"),
            Self::U16 => write!(f, "u16"),
            Self::U32 => write!(f, "u32"),
            Self::U64 => write!(f, "u64"),
            Self::I8 => write!(f, "i8"),
            Self::I16 => write!(f, "i16"),
            Self::I32 => write!(f, "i32"),
            Self::I64 => write!(f, "i64"),
            Self::F32 => write!(f, "f32"),
            Self::F64 => write!(f, "f64"),
            Self::String => write!(f, "string"),
            Self::Bytes => write!(f, "bytes"),
            Self::Array(inner) => write!(f, "array<{inner}>"),
            Self::Map(k, v) => write!(f, "map<{k}, {v}>"),
            Self::Struct(fields) => {
                write!(f, "struct{{")?;
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{name}: {ty}")?;
                }
                write!(f, "}}")
            }
            Self::Optional(inner) => write!(f, "optional<{inner}>"),
            Self::Handle => write!(f, "handle"),
        }
    }
}

// ---------------------------------------------------------------------------
// Values (runtime typed data matching TypeDesc)
// ---------------------------------------------------------------------------

/// A runtime value that can be serialized over the bus.
///
/// Each variant corresponds to a `TypeDesc` variant and carries the actual data.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Void,
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Struct(Vec<(String, Value)>),
    Optional(Option<Box<Value>>),
    Handle(u64),
}

// ---------------------------------------------------------------------------
// Interface Description
// ---------------------------------------------------------------------------

/// Describes a single method exposed by a service interface.
#[derive(Debug, Clone)]
pub struct MethodDesc {
    /// Method name (e.g., "CreateWindow", "PlayStream").
    pub name: String,
    /// Ordered list of parameter types.
    pub params: Vec<TypeDesc>,
    /// Return type (Void if the method returns nothing).
    pub return_type: TypeDesc,
}

/// Describes a versioned interface provided by a service.
///
/// Interfaces group related methods under a dotted name (e.g.,
/// "org.ouros.compositor.Surface"). Clients use introspection to discover
/// available interfaces before making calls.
#[derive(Debug, Clone)]
pub struct InterfaceDesc {
    /// Interface name (dot-separated, e.g., "org.ouros.audio.Playback").
    pub name: String,
    /// Interface version (monotonically increasing).
    pub version: u32,
    /// Methods provided by this interface.
    pub methods: Vec<MethodDesc>,
}

// ---------------------------------------------------------------------------
// Service Registry
// ---------------------------------------------------------------------------

/// A registered service entry in the bus.
#[derive(Debug, Clone)]
pub struct ServiceEntry {
    /// The well-known name (e.g., "org.ouros.compositor").
    pub name: String,
    /// PID of the process currently owning this name.
    pub owner_pid: u64,
    /// Interfaces exposed by this service.
    pub interfaces: Vec<InterfaceDesc>,
    /// When this service was registered (monotonic).
    pub registered_at: Instant,
    /// The well-known name (same as `name`, kept for clarity in lookups).
    pub well_known_name: String,
}

/// Tracks queued ownership for a well-known name.
///
/// When the current owner disconnects, the next pid in the queue takes over.
#[derive(Debug)]
struct NameOwnership {
    /// Current owner PID.
    current: u64,
    /// Queue of PIDs waiting to own this name (front = next owner).
    queue: VecDeque<u64>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// The type of a bus message, determining routing semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// A method invocation directed at a specific service.
    MethodCall,
    /// A successful return value from a method call.
    MethodReturn,
    /// An error response to a method call.
    Error,
    /// A broadcast notification (routed to subscribers via match rules).
    Signal,
}

/// Header for a bus message.
#[derive(Debug, Clone)]
pub struct MessageHeader {
    /// Unique name of the sender (e.g., ":1.5").
    pub sender: String,
    /// Destination (well-known name or unique name, empty for signals).
    pub destination: String,
    /// Per-connection serial number for request/response matching.
    pub serial: u64,
    /// Reply serial (for MethodReturn/Error, matches the call's serial).
    pub reply_serial: Option<u64>,
    /// Message type.
    pub msg_type: MessageType,
    /// Target interface (optional, for method calls and signals).
    pub interface: Option<String>,
    /// Target method or signal member name.
    pub member: Option<String>,
}

/// A complete bus message: header + serialized body.
#[derive(Debug, Clone)]
pub struct Message {
    pub header: MessageHeader,
    pub body: Vec<Value>,
}

// ---------------------------------------------------------------------------
// Serialization (Binary Wire Format)
// ---------------------------------------------------------------------------

/// Errors that can occur during serialization or deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerializeError {
    /// Buffer too small to contain the value.
    BufferTooSmall,
    /// Data is malformed or truncated.
    InvalidData,
    /// String contains invalid UTF-8.
    InvalidUtf8,
    /// Type mismatch during deserialization.
    TypeMismatch,
    /// Nested structure exceeds maximum depth.
    NestingTooDeep,
}

impl fmt::Display for SerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall => write!(f, "buffer too small"),
            Self::InvalidData => write!(f, "invalid data"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8"),
            Self::TypeMismatch => write!(f, "type mismatch"),
            Self::NestingTooDeep => write!(f, "nesting too deep"),
        }
    }
}

/// Maximum nesting depth for serialized structures to prevent stack overflow.
const MAX_NESTING_DEPTH: u32 = 64;

/// Aligns an offset to the specified alignment boundary.
fn align_to(offset: usize, alignment: usize) -> usize {
    let mask = alignment - 1;
    (offset + mask) & !mask
}

/// Returns the natural alignment for a given type descriptor.
#[allow(dead_code)]
fn alignment_of(ty: &TypeDesc) -> usize {
    match ty {
        TypeDesc::Void => 1,
        TypeDesc::Bool | TypeDesc::U8 | TypeDesc::I8 => 1,
        TypeDesc::U16 | TypeDesc::I16 => 2,
        TypeDesc::U32 | TypeDesc::I32 | TypeDesc::F32 => 4,
        TypeDesc::U64 | TypeDesc::I64 | TypeDesc::F64 | TypeDesc::Handle => 8,
        // Strings, bytes, arrays, maps: aligned to 4 (for their length prefix)
        TypeDesc::String | TypeDesc::Bytes | TypeDesc::Array(_) | TypeDesc::Map(_, _) => 4,
        // Structs aligned to 8
        TypeDesc::Struct(_) => 8,
        // Optional aligned to its inner type (plus 1 byte discriminant, padded)
        TypeDesc::Optional(inner) => alignment_of(inner).max(1),
    }
}

/// Serializes a `Value` into a byte buffer at the given offset.
///
/// Returns the new offset after writing, or a `SerializeError` if the buffer
/// is too small or the value structure exceeds nesting limits.
pub fn serialize_value(
    buf: &mut Vec<u8>,
    offset: usize,
    value: &Value,
    depth: u32,
) -> Result<usize, SerializeError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(SerializeError::NestingTooDeep);
    }

    match value {
        Value::Void => Ok(offset),
        Value::Bool(v) => {
            let aligned = align_to(offset, 1);
            ensure_capacity(buf, aligned + 1);
            buf[aligned] = u8::from(*v);
            Ok(aligned + 1)
        }
        Value::U8(v) => {
            ensure_capacity(buf, offset + 1);
            buf[offset] = *v;
            Ok(offset + 1)
        }
        Value::U16(v) => {
            let aligned = align_to(offset, 2);
            ensure_capacity(buf, aligned + 2);
            buf[aligned..aligned + 2].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 2)
        }
        Value::U32(v) => {
            let aligned = align_to(offset, 4);
            ensure_capacity(buf, aligned + 4);
            buf[aligned..aligned + 4].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 4)
        }
        Value::U64(v) => {
            let aligned = align_to(offset, 8);
            ensure_capacity(buf, aligned + 8);
            buf[aligned..aligned + 8].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 8)
        }
        Value::I8(v) => {
            ensure_capacity(buf, offset + 1);
            buf[offset] = (*v).cast_unsigned();
            Ok(offset + 1)
        }
        Value::I16(v) => {
            let aligned = align_to(offset, 2);
            ensure_capacity(buf, aligned + 2);
            buf[aligned..aligned + 2].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 2)
        }
        Value::I32(v) => {
            let aligned = align_to(offset, 4);
            ensure_capacity(buf, aligned + 4);
            buf[aligned..aligned + 4].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 4)
        }
        Value::I64(v) => {
            let aligned = align_to(offset, 8);
            ensure_capacity(buf, aligned + 8);
            buf[aligned..aligned + 8].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 8)
        }
        Value::F32(v) => {
            let aligned = align_to(offset, 4);
            ensure_capacity(buf, aligned + 4);
            buf[aligned..aligned + 4].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 4)
        }
        Value::F64(v) => {
            let aligned = align_to(offset, 8);
            ensure_capacity(buf, aligned + 8);
            buf[aligned..aligned + 8].copy_from_slice(&v.to_le_bytes());
            Ok(aligned + 8)
        }
        Value::String(s) => {
            let bytes = s.as_bytes();
            let aligned = align_to(offset, 4);
            let len: u32 = bytes
                .len()
                .try_into()
                .map_err(|_| SerializeError::BufferTooSmall)?;
            ensure_capacity(buf, aligned + 4 + bytes.len());
            buf[aligned..aligned + 4].copy_from_slice(&len.to_le_bytes());
            buf[aligned + 4..aligned + 4 + bytes.len()].copy_from_slice(bytes);
            Ok(aligned + 4 + bytes.len())
        }
        Value::Bytes(data) => {
            let aligned = align_to(offset, 4);
            let len: u32 = data
                .len()
                .try_into()
                .map_err(|_| SerializeError::BufferTooSmall)?;
            ensure_capacity(buf, aligned + 4 + data.len());
            buf[aligned..aligned + 4].copy_from_slice(&len.to_le_bytes());
            buf[aligned + 4..aligned + 4 + data.len()].copy_from_slice(data);
            Ok(aligned + 4 + data.len())
        }
        Value::Array(elements) => {
            let aligned = align_to(offset, 4);
            let count: u32 = elements
                .len()
                .try_into()
                .map_err(|_| SerializeError::BufferTooSmall)?;
            ensure_capacity(buf, aligned + 4);
            buf[aligned..aligned + 4].copy_from_slice(&count.to_le_bytes());
            let mut pos = aligned + 4;
            for elem in elements {
                pos = serialize_value(buf, pos, elem, depth + 1)?;
            }
            Ok(pos)
        }
        Value::Map(entries) => {
            let aligned = align_to(offset, 4);
            let count: u32 = entries
                .len()
                .try_into()
                .map_err(|_| SerializeError::BufferTooSmall)?;
            ensure_capacity(buf, aligned + 4);
            buf[aligned..aligned + 4].copy_from_slice(&count.to_le_bytes());
            let mut pos = aligned + 4;
            for (key, val) in entries {
                pos = serialize_value(buf, pos, key, depth + 1)?;
                pos = serialize_value(buf, pos, val, depth + 1)?;
            }
            Ok(pos)
        }
        Value::Struct(fields) => {
            let aligned = align_to(offset, 8);
            let count: u32 = fields
                .len()
                .try_into()
                .map_err(|_| SerializeError::BufferTooSmall)?;
            ensure_capacity(buf, aligned + 4);
            buf[aligned..aligned + 4].copy_from_slice(&count.to_le_bytes());
            let mut pos = aligned + 4;
            for (name, val) in fields {
                // Serialize field name as a string
                pos = serialize_value(buf, pos, &Value::String(name.clone()), depth + 1)?;
                // Serialize field value
                pos = serialize_value(buf, pos, val, depth + 1)?;
            }
            Ok(pos)
        }
        Value::Optional(opt) => {
            ensure_capacity(buf, offset + 1);
            match opt {
                None => {
                    buf[offset] = 0;
                    Ok(offset + 1)
                }
                Some(inner) => {
                    buf[offset] = 1;
                    serialize_value(buf, offset + 1, inner, depth + 1)
                }
            }
        }
        Value::Handle(h) => {
            let aligned = align_to(offset, 8);
            ensure_capacity(buf, aligned + 8);
            buf[aligned..aligned + 8].copy_from_slice(&h.to_le_bytes());
            Ok(aligned + 8)
        }
    }
}

/// Ensures the buffer has at least `needed` bytes, extending with zeroes if necessary.
fn ensure_capacity(buf: &mut Vec<u8>, needed: usize) {
    if buf.len() < needed {
        buf.resize(needed, 0);
    }
}

/// Deserializes a `Value` from a byte slice at the given offset.
///
/// The `ty` parameter specifies the expected type, enabling correct
/// interpretation of the raw bytes. Returns the deserialized value and the
/// new offset, or a `SerializeError` on failure.
pub fn deserialize_value(
    buf: &[u8],
    offset: usize,
    ty: &TypeDesc,
    depth: u32,
) -> Result<(Value, usize), SerializeError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(SerializeError::NestingTooDeep);
    }

    match ty {
        TypeDesc::Void => Ok((Value::Void, offset)),
        TypeDesc::Bool => {
            let aligned = align_to(offset, 1);
            if aligned >= buf.len() {
                return Err(SerializeError::InvalidData);
            }
            Ok((Value::Bool(buf[aligned] != 0), aligned + 1))
        }
        TypeDesc::U8 => {
            if offset >= buf.len() {
                return Err(SerializeError::InvalidData);
            }
            Ok((Value::U8(buf[offset]), offset + 1))
        }
        TypeDesc::U16 => {
            let aligned = align_to(offset, 2);
            if aligned + 2 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = u16::from_le_bytes(
                buf[aligned..aligned + 2]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::U16(v), aligned + 2))
        }
        TypeDesc::U32 => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = u32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::U32(v), aligned + 4))
        }
        TypeDesc::U64 => {
            let aligned = align_to(offset, 8);
            if aligned + 8 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = u64::from_le_bytes(
                buf[aligned..aligned + 8]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::U64(v), aligned + 8))
        }
        TypeDesc::I8 => {
            if offset >= buf.len() {
                return Err(SerializeError::InvalidData);
            }
            Ok((Value::I8(buf[offset].cast_signed()), offset + 1))
        }
        TypeDesc::I16 => {
            let aligned = align_to(offset, 2);
            if aligned + 2 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = i16::from_le_bytes(
                buf[aligned..aligned + 2]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::I16(v), aligned + 2))
        }
        TypeDesc::I32 => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = i32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::I32(v), aligned + 4))
        }
        TypeDesc::I64 => {
            let aligned = align_to(offset, 8);
            if aligned + 8 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = i64::from_le_bytes(
                buf[aligned..aligned + 8]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::I64(v), aligned + 8))
        }
        TypeDesc::F32 => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = f32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::F32(v), aligned + 4))
        }
        TypeDesc::F64 => {
            let aligned = align_to(offset, 8);
            if aligned + 8 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = f64::from_le_bytes(
                buf[aligned..aligned + 8]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::F64(v), aligned + 8))
        }
        TypeDesc::String => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let len = u32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            ) as usize;
            let start = aligned + 4;
            if start + len > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let s = std::str::from_utf8(&buf[start..start + len])
                .map_err(|_| SerializeError::InvalidUtf8)?;
            Ok((Value::String(s.to_owned()), start + len))
        }
        TypeDesc::Bytes => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let len = u32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            ) as usize;
            let start = aligned + 4;
            if start + len > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            Ok((Value::Bytes(buf[start..start + len].to_vec()), start + len))
        }
        TypeDesc::Array(elem_ty) => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let count = u32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            ) as usize;
            let mut pos = aligned + 4;
            let mut elements = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                let (val, new_pos) = deserialize_value(buf, pos, elem_ty, depth + 1)?;
                elements.push(val);
                pos = new_pos;
            }
            Ok((Value::Array(elements), pos))
        }
        TypeDesc::Map(key_ty, val_ty) => {
            let aligned = align_to(offset, 4);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let count = u32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            ) as usize;
            let mut pos = aligned + 4;
            let mut entries = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                let (k, p1) = deserialize_value(buf, pos, key_ty, depth + 1)?;
                let (v, p2) = deserialize_value(buf, p1, val_ty, depth + 1)?;
                entries.push((k, v));
                pos = p2;
            }
            Ok((Value::Map(entries), pos))
        }
        TypeDesc::Struct(fields) => {
            let aligned = align_to(offset, 8);
            if aligned + 4 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let count = u32::from_le_bytes(
                buf[aligned..aligned + 4]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            ) as usize;
            if count != fields.len() {
                return Err(SerializeError::TypeMismatch);
            }
            let mut pos = aligned + 4;
            let mut result_fields = Vec::with_capacity(count);
            for (expected_name, field_ty) in fields {
                // Deserialize field name
                let (name_val, p1) = deserialize_value(buf, pos, &TypeDesc::String, depth + 1)?;
                let field_name = match name_val {
                    Value::String(s) => s,
                    _ => return Err(SerializeError::TypeMismatch),
                };
                if &field_name != expected_name {
                    return Err(SerializeError::TypeMismatch);
                }
                // Deserialize field value
                let (val, p2) = deserialize_value(buf, p1, field_ty, depth + 1)?;
                result_fields.push((field_name, val));
                pos = p2;
            }
            Ok((Value::Struct(result_fields), pos))
        }
        TypeDesc::Optional(inner_ty) => {
            if offset >= buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let discriminant = buf[offset];
            if discriminant == 0 {
                Ok((Value::Optional(None), offset + 1))
            } else {
                let (val, pos) = deserialize_value(buf, offset + 1, inner_ty, depth + 1)?;
                Ok((Value::Optional(Some(Box::new(val))), pos))
            }
        }
        TypeDesc::Handle => {
            let aligned = align_to(offset, 8);
            if aligned + 8 > buf.len() {
                return Err(SerializeError::InvalidData);
            }
            let v = u64::from_le_bytes(
                buf[aligned..aligned + 8]
                    .try_into()
                    .map_err(|_| SerializeError::InvalidData)?,
            );
            Ok((Value::Handle(v), aligned + 8))
        }
    }
}

// ---------------------------------------------------------------------------
// Bus Errors
// ---------------------------------------------------------------------------

/// Errors returned by service bus operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusError {
    /// The requested service name is not registered.
    NameNotFound(String),
    /// The service name is already owned by another connection.
    NameAlreadyOwned(String),
    /// The specified connection does not exist.
    ConnectionNotFound(String),
    /// Invalid service name format (must be dot-separated identifiers).
    InvalidName(String),
    /// The interface was not found on the target service.
    InterfaceNotFound(String),
    /// The method was not found on the target interface.
    MethodNotFound(String),
    /// Message delivery failed (destination disconnected).
    DeliveryFailed(String),
    /// Service activation timed out.
    ActivationTimeout(String),
    /// Service activation failed (process exited or failed to register).
    ActivationFailed(String),
    /// Rate limit exceeded for this connection.
    RateLimited,
    /// The connection has not sent a Hello message yet.
    NotAuthenticated,
    /// Policy denied this operation.
    PolicyDenied(String),
    /// Serialization or deserialization error.
    Serialization(SerializeError),
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameNotFound(n) => write!(f, "name not found: {n}"),
            Self::NameAlreadyOwned(n) => write!(f, "name already owned: {n}"),
            Self::ConnectionNotFound(n) => write!(f, "connection not found: {n}"),
            Self::InvalidName(n) => write!(f, "invalid name: {n}"),
            Self::InterfaceNotFound(n) => write!(f, "interface not found: {n}"),
            Self::MethodNotFound(n) => write!(f, "method not found: {n}"),
            Self::DeliveryFailed(n) => write!(f, "delivery failed: {n}"),
            Self::ActivationTimeout(n) => write!(f, "activation timed out: {n}"),
            Self::ActivationFailed(n) => write!(f, "activation failed: {n}"),
            Self::RateLimited => write!(f, "rate limited"),
            Self::NotAuthenticated => write!(f, "not authenticated (send Hello first)"),
            Self::PolicyDenied(reason) => write!(f, "policy denied: {reason}"),
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
        }
    }
}

impl From<SerializeError> for BusError {
    fn from(e: SerializeError) -> Self {
        Self::Serialization(e)
    }
}

// ---------------------------------------------------------------------------
// Connection Management
// ---------------------------------------------------------------------------

/// Represents a connected client on the bus.
#[derive(Debug)]
pub struct Connection {
    /// Internal connection identifier.
    pub id: u64,
    /// Process ID of the connected client.
    pub pid: u64,
    /// Whether this connection has completed the Hello handshake.
    pub authenticated: bool,
    /// Unique name assigned to this connection (e.g., ":1.42").
    pub unique_name: String,
    /// Well-known names currently owned by this connection.
    pub owned_names: Vec<String>,
    /// Outbound message queue for this connection.
    pub outbox: VecDeque<Message>,
    /// Number of messages sent in the current rate-limit window.
    pub messages_this_window: u64,
    /// When the current rate-limit window started.
    pub window_start: Instant,
}

// ---------------------------------------------------------------------------
// Signal Subscriptions (Match Rules)
// ---------------------------------------------------------------------------

/// A match rule for filtering signal delivery.
///
/// Only signals matching all specified fields (those that are `Some`) are
/// delivered to the subscriber. Fields left as `None` match any value.
#[derive(Debug, Clone)]
pub struct MatchRule {
    /// Filter by sender unique name or well-known name.
    pub sender: Option<String>,
    /// Filter by interface name.
    pub interface: Option<String>,
    /// Filter by member (signal) name.
    pub member: Option<String>,
    /// Filter by destination.
    pub destination: Option<String>,
}

impl MatchRule {
    /// Tests whether a message matches this rule.
    pub fn matches(&self, msg: &Message) -> bool {
        if let Some(ref sender) = self.sender {
            if &msg.header.sender != sender {
                return false;
            }
        }
        if let Some(ref iface) = self.interface {
            match &msg.header.interface {
                Some(msg_iface) if msg_iface == iface => {}
                _ => return false,
            }
        }
        if let Some(ref member) = self.member {
            match &msg.header.member {
                Some(msg_member) if msg_member == member => {}
                _ => return false,
            }
        }
        if let Some(ref dest) = self.destination {
            if &msg.header.destination != dest {
                return false;
            }
        }
        true
    }
}

/// A signal subscription: a connection plus its filter rule.
#[derive(Debug)]
struct Subscription {
    /// The unique name of the subscribing connection.
    connection_name: String,
    /// The filter rule.
    rule: MatchRule,
}

// ---------------------------------------------------------------------------
// Name Watcher
// ---------------------------------------------------------------------------

/// A watch registration for name ownership changes.
#[derive(Debug)]
struct NameWatch {
    /// The well-known name being watched.
    watched_name: String,
    /// The unique name of the watcher connection.
    watcher: String,
}

// ---------------------------------------------------------------------------
// Service Activation
// ---------------------------------------------------------------------------

/// An activation entry: associates a well-known name with an executable.
#[derive(Debug, Clone)]
pub struct ActivationEntry {
    /// The well-known name this activation provides.
    pub name: String,
    /// Path to the executable to launch.
    pub exec_path: String,
    /// Maximum time (in seconds) to wait for the service to register.
    pub timeout_secs: u64,
}

/// Tracks a pending activation attempt.
#[derive(Debug)]
struct PendingActivation {
    /// The well-known name being activated.
    name: String,
    /// When the activation was initiated.
    started_at: Instant,
    /// Timeout duration.
    timeout: Duration,
    /// Messages queued waiting for the service to become available.
    queued_messages: Vec<Message>,
}

// ---------------------------------------------------------------------------
// Bus Policy
// ---------------------------------------------------------------------------

/// A single policy rule for access control.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    /// Whether this rule allows or denies the operation.
    pub allow: bool,
    /// Sender filter (None = match all).
    pub sender: Option<String>,
    /// Destination filter (None = match all).
    pub destination: Option<String>,
    /// Interface filter (None = match all).
    pub interface: Option<String>,
    /// Member filter (None = match all).
    pub member: Option<String>,
}

impl PolicyRule {
    /// Checks if this rule matches the given message context.
    fn matches_message(&self, sender: &str, dest: &str, iface: Option<&str>, member: Option<&str>) -> bool {
        if let Some(ref rule_sender) = self.sender {
            if rule_sender != sender {
                return false;
            }
        }
        if let Some(ref rule_dest) = self.destination {
            if rule_dest != dest {
                return false;
            }
        }
        if let Some(ref rule_iface) = self.interface {
            match iface {
                Some(msg_iface) if msg_iface == rule_iface => {}
                None => return false,
                _ => return false,
            }
        }
        if let Some(ref rule_member) = self.member {
            match member {
                Some(msg_member) if msg_member == rule_member => {}
                None => return false,
                _ => return false,
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Bus Statistics
// ---------------------------------------------------------------------------

/// Runtime statistics for the service bus.
#[derive(Debug, Clone, Default)]
pub struct BusStats {
    /// Total messages routed since startup.
    pub messages_routed: u64,
    /// Total method calls processed.
    pub method_calls: u64,
    /// Total signals dispatched.
    pub signals_dispatched: u64,
    /// Total errors sent.
    pub errors_sent: u64,
    /// Total connections that have been made.
    pub total_connections: u64,
    /// Total activations attempted.
    pub activations_attempted: u64,
    /// Total activations that succeeded.
    pub activations_succeeded: u64,
    /// Total activations that timed out.
    pub activations_timed_out: u64,
}

// ---------------------------------------------------------------------------
// The Service Bus Daemon
// ---------------------------------------------------------------------------

/// Default maximum messages per second per connection.
const DEFAULT_RATE_LIMIT: u64 = 1000;

/// Rate limit window duration.
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);

/// Default activation timeout.
#[allow(dead_code)]
const DEFAULT_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);

/// The service bus daemon: maintains the registry, routes messages, and
/// enforces policy.
pub struct ServiceBus {
    /// All active connections, keyed by unique name.
    connections: HashMap<String, Connection>,
    /// Well-known name → service entry.
    services: BTreeMap<String, ServiceEntry>,
    /// Name ownership tracking (for queued ownership transfer).
    name_ownership: HashMap<String, NameOwnership>,
    /// Signal subscriptions.
    subscriptions: Vec<Subscription>,
    /// Name change watchers.
    name_watches: Vec<NameWatch>,
    /// Activation entries (well-known name → activation config).
    activation_entries: HashMap<String, ActivationEntry>,
    /// Pending activations in progress.
    pending_activations: Vec<PendingActivation>,
    /// Policy rules (evaluated in order; first match wins).
    policy_rules: Vec<PolicyRule>,
    /// Rate limit: max messages per second per connection.
    rate_limit: u64,
    /// Next connection ID to assign.
    next_conn_id: u64,
    /// Next unique-name counter (for `:1.N` names).
    next_unique_counter: u64,
    /// Whether debug logging is enabled.
    debug_mode: bool,
    /// Bus statistics.
    stats: BusStats,
}

impl ServiceBus {
    /// Creates a new service bus daemon with default settings.
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            services: BTreeMap::new(),
            name_ownership: HashMap::new(),
            subscriptions: Vec::new(),
            name_watches: Vec::new(),
            activation_entries: HashMap::new(),
            pending_activations: Vec::new(),
            policy_rules: Vec::new(),
            rate_limit: DEFAULT_RATE_LIMIT,
            next_conn_id: 1,
            next_unique_counter: 1,
            debug_mode: false,
            stats: BusStats::default(),
        }
    }

    /// Enables or disables debug logging mode.
    pub fn set_debug_mode(&mut self, enabled: bool) {
        self.debug_mode = enabled;
    }

    /// Returns a reference to the current bus statistics.
    pub fn stats(&self) -> &BusStats {
        &self.stats
    }

    /// Returns the number of active connections.
    pub fn active_connections(&self) -> usize {
        self.connections.len()
    }

    /// Returns the number of registered services.
    pub fn registered_services(&self) -> usize {
        self.services.len()
    }

    // -----------------------------------------------------------------------
    // Connection Management
    // -----------------------------------------------------------------------

    /// Accepts a new connection from a process with the given PID.
    ///
    /// Returns the unique name assigned to the connection (e.g., `:1.5`).
    /// The connection is not yet authenticated until `handle_hello` is called.
    pub fn accept_connection(&mut self, pid: u64) -> String {
        let id = self.next_conn_id;
        self.next_conn_id = self.next_conn_id.saturating_add(1);

        let unique_name = format!(":1.{}", self.next_unique_counter);
        self.next_unique_counter = self.next_unique_counter.saturating_add(1);

        let conn = Connection {
            id,
            pid,
            authenticated: false,
            unique_name: unique_name.clone(),
            owned_names: Vec::new(),
            outbox: VecDeque::new(),
            messages_this_window: 0,
            window_start: Instant::now(),
        };

        self.connections.insert(unique_name.clone(), conn);
        self.stats.total_connections = self.stats.total_connections.saturating_add(1);

        if self.debug_mode {
            eprintln!("[servicebus] new connection: {unique_name} (pid={pid})");
        }

        unique_name
    }

    /// Handles the Hello message from a newly connected client.
    ///
    /// This authenticates the connection and enables it to send/receive messages.
    /// Returns an error if the connection is not found.
    pub fn handle_hello(&mut self, unique_name: &str) -> Result<(), BusError> {
        let conn = self
            .connections
            .get_mut(unique_name)
            .ok_or_else(|| BusError::ConnectionNotFound(unique_name.to_owned()))?;
        conn.authenticated = true;

        if self.debug_mode {
            eprintln!("[servicebus] hello from {unique_name}");
        }

        Ok(())
    }

    /// Disconnects a connection, releasing all owned names and cleaning up
    /// subscriptions, watches, and pending state.
    pub fn disconnect(&mut self, unique_name: &str) {
        let Some(conn) = self.connections.remove(unique_name) else {
            return;
        };

        if self.debug_mode {
            eprintln!("[servicebus] disconnect: {unique_name} (pid={})", conn.pid);
        }

        // Release all owned names and notify watchers
        for name in &conn.owned_names {
            self.release_name_internal(name, unique_name);
        }

        // Remove subscriptions for this connection
        self.subscriptions
            .retain(|s| s.connection_name != unique_name);

        // Remove name watches for this connection
        self.name_watches.retain(|w| w.watcher != unique_name);
    }

    /// Checks if a connection is within its rate limit.
    ///
    /// Returns `Ok(())` if allowed, or `Err(BusError::RateLimited)` if the
    /// connection has exceeded its message quota for the current window.
    fn check_rate_limit(&mut self, unique_name: &str) -> Result<(), BusError> {
        let conn = self
            .connections
            .get_mut(unique_name)
            .ok_or_else(|| BusError::ConnectionNotFound(unique_name.to_owned()))?;

        let now = Instant::now();
        if now.duration_since(conn.window_start) >= RATE_LIMIT_WINDOW {
            // Start a new window
            conn.window_start = now;
            conn.messages_this_window = 0;
        }

        if conn.messages_this_window >= self.rate_limit {
            return Err(BusError::RateLimited);
        }

        conn.messages_this_window = conn.messages_this_window.saturating_add(1);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Name Validation
    // -----------------------------------------------------------------------

    /// Validates a well-known service name.
    ///
    /// Names must be dot-separated identifiers with at least two segments.
    /// Each segment must start with a letter and contain only alphanumeric
    /// characters, underscores, or hyphens.
    fn validate_name(name: &str) -> Result<(), BusError> {
        if name.is_empty() {
            return Err(BusError::InvalidName("empty name".to_owned()));
        }

        let segments: Vec<&str> = name.split('.').collect();
        if segments.len() < 2 {
            return Err(BusError::InvalidName(
                "name must have at least two dot-separated segments".to_owned(),
            ));
        }

        for segment in &segments {
            if segment.is_empty() {
                return Err(BusError::InvalidName(
                    "empty segment in name".to_owned(),
                ));
            }
            let first = segment.as_bytes().first().copied();
            match first {
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => {}
                _ => {
                    return Err(BusError::InvalidName(format!(
                        "segment must start with a letter or underscore: {segment}"
                    )));
                }
            }
            for ch in segment.chars() {
                if !ch.is_alphanumeric() && ch != '_' && ch != '-' {
                    return Err(BusError::InvalidName(format!(
                        "invalid character '{ch}' in name segment"
                    )));
                }
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Service Registration
    // -----------------------------------------------------------------------

    /// Registers a service under a well-known name.
    ///
    /// If the name is already owned, the caller is added to the ownership queue.
    /// When the current owner disconnects, the next in queue takes over.
    pub fn register_service(
        &mut self,
        owner_unique_name: &str,
        name: &str,
        interfaces: Vec<InterfaceDesc>,
    ) -> Result<(), BusError> {
        Self::validate_name(name)?;

        let conn = self
            .connections
            .get_mut(owner_unique_name)
            .ok_or_else(|| BusError::ConnectionNotFound(owner_unique_name.to_owned()))?;

        if !conn.authenticated {
            return Err(BusError::NotAuthenticated);
        }

        let pid = conn.pid;

        // Check if name is already owned
        if let Some(ownership) = self.name_ownership.get_mut(name) {
            // Name exists — queue this connection as a waiter
            let owner_conn_name = self
                .connections
                .values()
                .find(|c| c.pid == ownership.current)
                .map(|c| c.unique_name.clone());

            if owner_conn_name.as_deref() == Some(owner_unique_name) {
                // Already owns it — update interfaces
                if let Some(entry) = self.services.get_mut(name) {
                    entry.interfaces = interfaces;
                }
                return Ok(());
            }

            // Add to queue
            ownership.queue.push_back(pid);

            if self.debug_mode {
                eprintln!(
                    "[servicebus] {owner_unique_name} queued for name '{name}' (pos={})",
                    ownership.queue.len()
                );
            }

            return Ok(());
        }

        // Name is free — take ownership
        let entry = ServiceEntry {
            name: name.to_owned(),
            owner_pid: pid,
            interfaces,
            registered_at: Instant::now(),
            well_known_name: name.to_owned(),
        };

        self.services.insert(name.to_owned(), entry);
        self.name_ownership.insert(
            name.to_owned(),
            NameOwnership {
                current: pid,
                queue: VecDeque::new(),
            },
        );

        // Update connection's owned names
        let conn = self
            .connections
            .get_mut(owner_unique_name)
            .ok_or_else(|| BusError::ConnectionNotFound(owner_unique_name.to_owned()))?;
        conn.owned_names.push(name.to_owned());

        // Notify name watchers
        self.notify_name_acquired(name, owner_unique_name);

        // Check if any pending activations are waiting for this name
        self.complete_activation(name);

        if self.debug_mode {
            eprintln!("[servicebus] registered: '{name}' owned by {owner_unique_name} (pid={pid})");
        }

        Ok(())
    }

    /// Unregisters a service, releasing the well-known name.
    ///
    /// If there are queued waiters, the next one takes ownership automatically.
    pub fn unregister_service(
        &mut self,
        owner_unique_name: &str,
        name: &str,
    ) -> Result<(), BusError> {
        let conn = self
            .connections
            .get_mut(owner_unique_name)
            .ok_or_else(|| BusError::ConnectionNotFound(owner_unique_name.to_owned()))?;

        if !conn.owned_names.contains(&name.to_owned()) {
            return Err(BusError::NameNotFound(name.to_owned()));
        }

        conn.owned_names.retain(|n| n != name);
        self.release_name_internal(name, owner_unique_name);

        if self.debug_mode {
            eprintln!("[servicebus] unregistered: '{name}' by {owner_unique_name}");
        }

        Ok(())
    }

    /// Internal: releases a name and transfers ownership to the next in queue.
    fn release_name_internal(&mut self, name: &str, old_owner: &str) {
        self.notify_name_lost(name, old_owner);

        if let Some(ownership) = self.name_ownership.get_mut(name) {
            if let Some(next_pid) = ownership.queue.pop_front() {
                // Transfer to next in queue
                ownership.current = next_pid;

                // Find the connection with this PID and update its owned_names
                let next_conn_name = self
                    .connections
                    .values_mut()
                    .find(|c| c.pid == next_pid)
                    .map(|c| {
                        c.owned_names.push(name.to_owned());
                        c.unique_name.clone()
                    });

                if let Some(ref conn_name) = next_conn_name {
                    // Update the service entry with the new owner
                    if let Some(entry) = self.services.get_mut(name) {
                        entry.owner_pid = next_pid;
                        entry.registered_at = Instant::now();
                    }
                    self.notify_name_acquired(name, conn_name);

                    if self.debug_mode {
                        eprintln!(
                            "[servicebus] name '{name}' transferred to {conn_name} (pid={next_pid})"
                        );
                    }
                }
            } else {
                // No waiters — remove the name entirely
                self.name_ownership.remove(name);
                self.services.remove(name);

                if self.debug_mode {
                    eprintln!("[servicebus] name '{name}' released (no waiters)");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Name Resolution and Introspection
    // -----------------------------------------------------------------------

    /// Looks up a service by its well-known name.
    ///
    /// Returns a reference to the service entry, or `NameNotFound` if no service
    /// is registered under that name.
    pub fn lookup(&self, name: &str) -> Result<&ServiceEntry, BusError> {
        self.services
            .get(name)
            .ok_or_else(|| BusError::NameNotFound(name.to_owned()))
    }

    /// Lists all registered well-known service names.
    pub fn list_names(&self) -> Vec<&str> {
        self.services.keys().map(String::as_str).collect()
    }

    /// Lists all active unique connection names.
    pub fn list_connections(&self) -> Vec<&str> {
        self.connections.keys().map(String::as_str).collect()
    }

    /// Introspects a service, returning its interface descriptions.
    pub fn introspect(&self, name: &str) -> Result<&[InterfaceDesc], BusError> {
        let entry = self.lookup(name)?;
        Ok(&entry.interfaces)
    }

    /// Looks up a specific method on a service's interface.
    pub fn lookup_method(
        &self,
        service_name: &str,
        interface_name: &str,
        method_name: &str,
    ) -> Result<&MethodDesc, BusError> {
        let entry = self.lookup(service_name)?;
        let iface = entry
            .interfaces
            .iter()
            .find(|i| i.name == interface_name)
            .ok_or_else(|| BusError::InterfaceNotFound(interface_name.to_owned()))?;
        iface
            .methods
            .iter()
            .find(|m| m.name == method_name)
            .ok_or_else(|| BusError::MethodNotFound(method_name.to_owned()))
    }

    // -----------------------------------------------------------------------
    // Name Watching
    // -----------------------------------------------------------------------

    /// Subscribes to ownership changes for a well-known name.
    ///
    /// The watcher will receive signals when the name is acquired or lost.
    pub fn watch_name(&mut self, watcher: &str, name: &str) -> Result<(), BusError> {
        if !self.connections.contains_key(watcher) {
            return Err(BusError::ConnectionNotFound(watcher.to_owned()));
        }

        self.name_watches.push(NameWatch {
            watched_name: name.to_owned(),
            watcher: watcher.to_owned(),
        });

        Ok(())
    }

    /// Removes a name watch subscription.
    pub fn unwatch_name(&mut self, watcher: &str, name: &str) {
        self.name_watches
            .retain(|w| !(w.watcher == watcher && w.watched_name == name));
    }

    /// Notifies watchers that a name was acquired.
    fn notify_name_acquired(&mut self, name: &str, new_owner: &str) {
        let signal = Message {
            header: MessageHeader {
                sender: "org.ouros.ServiceBus".to_owned(),
                destination: String::new(),
                serial: 0,
                reply_serial: None,
                msg_type: MessageType::Signal,
                interface: Some("org.ouros.ServiceBus".to_owned()),
                member: Some("NameAcquired".to_owned()),
            },
            body: vec![
                Value::String(name.to_owned()),
                Value::String(new_owner.to_owned()),
            ],
        };

        let watchers: Vec<String> = self
            .name_watches
            .iter()
            .filter(|w| w.watched_name == name)
            .map(|w| w.watcher.clone())
            .collect();

        for watcher in watchers {
            if let Some(conn) = self.connections.get_mut(&watcher) {
                conn.outbox.push_back(signal.clone());
            }
        }
    }

    /// Notifies watchers that a name was lost.
    fn notify_name_lost(&mut self, name: &str, old_owner: &str) {
        let signal = Message {
            header: MessageHeader {
                sender: "org.ouros.ServiceBus".to_owned(),
                destination: String::new(),
                serial: 0,
                reply_serial: None,
                msg_type: MessageType::Signal,
                interface: Some("org.ouros.ServiceBus".to_owned()),
                member: Some("NameLost".to_owned()),
            },
            body: vec![
                Value::String(name.to_owned()),
                Value::String(old_owner.to_owned()),
            ],
        };

        let watchers: Vec<String> = self
            .name_watches
            .iter()
            .filter(|w| w.watched_name == name)
            .map(|w| w.watcher.clone())
            .collect();

        for watcher in watchers {
            if let Some(conn) = self.connections.get_mut(&watcher) {
                conn.outbox.push_back(signal.clone());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Signal Subscriptions
    // -----------------------------------------------------------------------

    /// Adds a signal subscription with a match rule.
    ///
    /// Signals matching the rule will be delivered to the subscribing connection.
    pub fn add_match_rule(
        &mut self,
        subscriber: &str,
        rule: MatchRule,
    ) -> Result<(), BusError> {
        if !self.connections.contains_key(subscriber) {
            return Err(BusError::ConnectionNotFound(subscriber.to_owned()));
        }

        self.subscriptions.push(Subscription {
            connection_name: subscriber.to_owned(),
            rule,
        });

        Ok(())
    }

    /// Removes all match rules for a subscriber that match the given rule.
    pub fn remove_match_rule(&mut self, subscriber: &str, rule: &MatchRule) {
        self.subscriptions.retain(|s| {
            !(s.connection_name == subscriber
                && s.rule.sender == rule.sender
                && s.rule.interface == rule.interface
                && s.rule.member == rule.member
                && s.rule.destination == rule.destination)
        });
    }

    // -----------------------------------------------------------------------
    // Message Routing
    // -----------------------------------------------------------------------

    /// Routes a message through the bus.
    ///
    /// - Method calls are delivered to the destination service owner.
    /// - Method returns and errors are delivered to the original caller.
    /// - Signals are delivered to all matching subscribers.
    ///
    /// Returns an error if delivery fails (destination not found, policy denied,
    /// rate limited, etc.).
    pub fn route_message(&mut self, msg: Message) -> Result<(), BusError> {
        let sender = msg.header.sender.clone();

        // Rate-limit check
        self.check_rate_limit(&sender)?;

        // Policy check
        self.check_policy(
            &sender,
            &msg.header.destination,
            msg.header.interface.as_deref(),
            msg.header.member.as_deref(),
        )?;

        // Ensure sender is authenticated
        if let Some(conn) = self.connections.get(&sender) {
            if !conn.authenticated {
                return Err(BusError::NotAuthenticated);
            }
        } else {
            return Err(BusError::ConnectionNotFound(sender));
        }

        if self.debug_mode {
            eprintln!(
                "[servicebus] route: {:?} from {} -> {} ({:?}::{:?})",
                msg.header.msg_type,
                msg.header.sender,
                msg.header.destination,
                msg.header.interface,
                msg.header.member,
            );
        }

        self.stats.messages_routed = self.stats.messages_routed.saturating_add(1);

        match msg.header.msg_type {
            MessageType::MethodCall => self.route_method_call(msg),
            MessageType::MethodReturn | MessageType::Error => self.route_reply(msg),
            MessageType::Signal => self.route_signal(msg),
        }
    }

    /// Routes a method call to the destination service.
    fn route_method_call(&mut self, msg: Message) -> Result<(), BusError> {
        self.stats.method_calls = self.stats.method_calls.saturating_add(1);

        let destination = msg.header.destination.clone();

        // If destination is a unique name, deliver directly
        if destination.starts_with(':') {
            return self.deliver_to_connection(&destination, msg);
        }

        // Look up well-known name
        if let Some(entry) = self.services.get(&destination) {
            let owner_pid = entry.owner_pid;
            // Find the connection owning this service
            let owner_conn = self
                .connections
                .values()
                .find(|c| c.pid == owner_pid)
                .map(|c| c.unique_name.clone());

            if let Some(conn_name) = owner_conn {
                return self.deliver_to_connection(&conn_name, msg);
            }
        }

        // Service not found — try activation
        if self.activation_entries.contains_key(&destination) {
            return self.try_activate(&destination, msg);
        }

        Err(BusError::NameNotFound(destination))
    }

    /// Routes a reply (MethodReturn or Error) to the original caller.
    fn route_reply(&mut self, msg: Message) -> Result<(), BusError> {
        if msg.header.msg_type == MessageType::Error {
            self.stats.errors_sent = self.stats.errors_sent.saturating_add(1);
        }

        let destination = msg.header.destination.clone();
        if destination.is_empty() {
            return Err(BusError::DeliveryFailed(
                "reply has no destination".to_owned(),
            ));
        }

        self.deliver_to_connection(&destination, msg)
    }

    /// Routes a signal to all matching subscribers.
    fn route_signal(&mut self, msg: Message) -> Result<(), BusError> {
        self.stats.signals_dispatched = self.stats.signals_dispatched.saturating_add(1);

        let matching: Vec<String> = self
            .subscriptions
            .iter()
            .filter(|s| s.rule.matches(&msg))
            .map(|s| s.connection_name.clone())
            .collect();

        for conn_name in matching {
            // Signals are best-effort: don't fail if one subscriber is gone
            let _ = self.deliver_to_connection(&conn_name, msg.clone());
        }

        Ok(())
    }

    /// Delivers a message to a specific connection's outbox.
    fn deliver_to_connection(&mut self, unique_name: &str, msg: Message) -> Result<(), BusError> {
        let conn = self
            .connections
            .get_mut(unique_name)
            .ok_or_else(|| BusError::DeliveryFailed(format!("connection gone: {unique_name}")))?;

        conn.outbox.push_back(msg);
        Ok(())
    }

    /// Drains and returns all pending messages for a connection.
    pub fn drain_outbox(&mut self, unique_name: &str) -> Vec<Message> {
        self.connections
            .get_mut(unique_name)
            .map(|conn| conn.outbox.drain(..).collect())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Policy Enforcement
    // -----------------------------------------------------------------------

    /// Checks whether a message is allowed by the bus policy.
    ///
    /// Rules are evaluated in order. The first matching rule determines the
    /// outcome. If no rule matches, the default is to allow (capability-based
    /// security handles fine-grained access control at a lower level).
    fn check_policy(
        &self,
        sender: &str,
        destination: &str,
        interface: Option<&str>,
        member: Option<&str>,
    ) -> Result<(), BusError> {
        for rule in &self.policy_rules {
            if rule.matches_message(sender, destination, interface, member) {
                if rule.allow {
                    return Ok(());
                }
                return Err(BusError::PolicyDenied(format!(
                    "denied: {sender} -> {destination} ({interface:?}::{member:?})"
                )));
            }
        }
        // Default: allow
        Ok(())
    }

    /// Adds a policy rule. Rules are evaluated in insertion order.
    pub fn add_policy_rule(&mut self, rule: PolicyRule) {
        self.policy_rules.push(rule);
    }

    /// Clears all policy rules (resets to default-allow).
    pub fn clear_policy_rules(&mut self) {
        self.policy_rules.clear();
    }

    // -----------------------------------------------------------------------
    // Service Activation
    // -----------------------------------------------------------------------

    /// Registers an activation entry (name → executable mapping).
    pub fn register_activation(&mut self, entry: ActivationEntry) {
        self.activation_entries.insert(entry.name.clone(), entry);
    }

    /// Removes an activation entry.
    pub fn remove_activation(&mut self, name: &str) {
        self.activation_entries.remove(name);
    }

    /// Attempts to activate a service by launching its executable.
    ///
    /// The message that triggered activation is queued and will be delivered
    /// once the service registers its well-known name.
    fn try_activate(&mut self, name: &str, msg: Message) -> Result<(), BusError> {
        self.stats.activations_attempted = self.stats.activations_attempted.saturating_add(1);

        // Check if activation is already in progress
        if let Some(pending) = self.pending_activations.iter_mut().find(|p| p.name == name) {
            pending.queued_messages.push(msg);
            return Ok(());
        }

        let entry = self
            .activation_entries
            .get(name)
            .ok_or_else(|| BusError::ActivationFailed(format!("no activation entry for '{name}'")))?
            .clone();

        let timeout = Duration::from_secs(entry.timeout_secs);

        if self.debug_mode {
            eprintln!(
                "[servicebus] activating '{name}' via '{}'",
                entry.exec_path
            );
        }

        self.pending_activations.push(PendingActivation {
            name: name.to_owned(),
            started_at: Instant::now(),
            timeout,
            queued_messages: vec![msg],
        });

        // In a real implementation, this would spawn the process.
        // The process is expected to connect to the bus and register the name.
        // For now, activation is modeled — the service must register within the timeout.

        Ok(())
    }

    /// Called when a service registers a name — delivers any queued activation messages.
    fn complete_activation(&mut self, name: &str) {
        let idx = self.pending_activations.iter().position(|p| p.name == name);
        if let Some(idx) = idx {
            let pending = self.pending_activations.remove(idx);
            self.stats.activations_succeeded = self.stats.activations_succeeded.saturating_add(1);

            if self.debug_mode {
                eprintln!(
                    "[servicebus] activation complete for '{}', delivering {} queued messages",
                    name,
                    pending.queued_messages.len()
                );
            }

            // Deliver queued messages
            for msg in pending.queued_messages {
                // Best-effort delivery of queued messages
                let _ = self.route_method_call(msg);
            }
        }
    }

    /// Checks for timed-out activations and cleans them up.
    ///
    /// This should be called periodically (e.g., every second). Returns a list
    /// of activation names that timed out.
    pub fn check_activation_timeouts(&mut self) -> Vec<String> {
        let now = Instant::now();
        let mut timed_out = Vec::new();

        self.pending_activations.retain(|pending| {
            if now.duration_since(pending.started_at) >= pending.timeout {
                timed_out.push(pending.name.clone());
                false
            } else {
                true
            }
        });

        for name in &timed_out {
            self.stats.activations_timed_out = self.stats.activations_timed_out.saturating_add(1);
            if self.debug_mode {
                eprintln!("[servicebus] activation timed out for '{name}'");
            }
        }

        timed_out
    }

    // -----------------------------------------------------------------------
    // Message Serialization Helpers
    // -----------------------------------------------------------------------

    /// Serializes a complete message (header + body) into a byte buffer.
    ///
    /// Wire format:
    /// - 4 bytes: total message length (excluding these 4 bytes)
    /// - Header fields serialized as Values
    /// - Body values serialized in sequence
    pub fn serialize_message(&self, msg: &Message) -> Result<Vec<u8>, BusError> {
        let mut buf = Vec::with_capacity(256);

        // Reserve space for total length prefix
        buf.resize(4, 0);
        let mut offset = 4;

        // Serialize header fields
        offset = serialize_value(&mut buf, offset, &Value::String(msg.header.sender.clone()), 0)?;
        offset =
            serialize_value(&mut buf, offset, &Value::String(msg.header.destination.clone()), 0)?;
        offset = serialize_value(&mut buf, offset, &Value::U64(msg.header.serial), 0)?;
        offset = serialize_value(
            &mut buf,
            offset,
            &Value::Optional(
                msg.header.reply_serial.map(|s| Box::new(Value::U64(s))),
            ),
            0,
        )?;
        offset = serialize_value(
            &mut buf,
            offset,
            &Value::U8(match msg.header.msg_type {
                MessageType::MethodCall => 1,
                MessageType::MethodReturn => 2,
                MessageType::Error => 3,
                MessageType::Signal => 4,
            }),
            0,
        )?;
        offset = serialize_value(
            &mut buf,
            offset,
            &Value::Optional(
                msg.header.interface.as_ref().map(|s| Box::new(Value::String(s.clone()))),
            ),
            0,
        )?;
        offset = serialize_value(
            &mut buf,
            offset,
            &Value::Optional(
                msg.header.member.as_ref().map(|s| Box::new(Value::String(s.clone()))),
            ),
            0,
        )?;

        // Serialize body values
        offset = serialize_value(
            &mut buf,
            offset,
            &Value::U32(
                msg.body
                    .len()
                    .try_into()
                    .map_err(|_| SerializeError::BufferTooSmall)?,
            ),
            0,
        )?;
        for val in &msg.body {
            offset = serialize_value(&mut buf, offset, val, 0)?;
        }

        // Write total length (excluding the 4-byte length prefix itself)
        let total_len: u32 = (offset - 4)
            .try_into()
            .map_err(|_| SerializeError::BufferTooSmall)?;
        buf[0..4].copy_from_slice(&total_len.to_le_bytes());

        // Ensure buf is exactly the right length
        buf.truncate(offset);

        Ok(buf)
    }
}

impl Default for ServiceBus {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Daemon Entry Point
// ---------------------------------------------------------------------------

/// Runs the service bus daemon.
///
/// In a full OurOS system, this listens on a well-known IPC endpoint for client
/// connections and processes messages in a loop. For now, this sets up the bus
/// with default configuration and reports readiness.
fn main() {
    eprintln!("[servicebus] OurOS Service Bus Daemon starting...");

    let mut bus = ServiceBus::new();

    // Register built-in activation entries (system services)
    bus.register_activation(ActivationEntry {
        name: "org.ouros.compositor".to_owned(),
        exec_path: "/usr/lib/ouros/compositor".to_owned(),
        timeout_secs: 10,
    });
    bus.register_activation(ActivationEntry {
        name: "org.ouros.audio.mixer".to_owned(),
        exec_path: "/usr/lib/ouros/audio-mixer".to_owned(),
        timeout_secs: 10,
    });
    bus.register_activation(ActivationEntry {
        name: "org.ouros.network.manager".to_owned(),
        exec_path: "/usr/lib/ouros/network-manager".to_owned(),
        timeout_secs: 10,
    });
    bus.register_activation(ActivationEntry {
        name: "org.ouros.power.manager".to_owned(),
        exec_path: "/usr/lib/ouros/power-manager".to_owned(),
        timeout_secs: 10,
    });

    // Enable debug mode via environment variable
    if std::env::var("SERVICEBUS_DEBUG").is_ok() {
        bus.set_debug_mode(true);
        eprintln!("[servicebus] debug mode enabled");
    }

    eprintln!(
        "[servicebus] ready. {} activation entries configured.",
        bus.activation_entries.len()
    );

    // In a real system, this would enter an event loop processing IPC messages.
    // For now, demonstrate the bus is functional by running a self-test.
    if std::env::var("SERVICEBUS_SELFTEST").is_ok() {
        run_self_test(&mut bus);
    }

    eprintln!("[servicebus] daemon running (waiting for connections).");

    // The actual event loop would be here, processing incoming IPC connections.
    // Each connection maps to a `accept_connection` + `handle_hello` call,
    // then messages are routed via `route_message` and responses delivered
    // via `drain_outbox`.
}

/// Runs a built-in self-test demonstrating bus functionality.
///
/// This exercises registration, lookup, introspection, message routing,
/// signals, and name watching.
fn run_self_test(bus: &mut ServiceBus) {
    eprintln!("[servicebus] === SELF-TEST START ===");

    // Connect two clients
    let client_a = bus.accept_connection(100);
    let client_b = bus.accept_connection(200);

    if let Err(e) = bus.handle_hello(&client_a) {
        eprintln!("[selftest] hello A failed: {e}");
        return;
    }
    if let Err(e) = bus.handle_hello(&client_b) {
        eprintln!("[selftest] hello B failed: {e}");
        return;
    }

    eprintln!("[selftest] connected: A={client_a}, B={client_b}");

    // Client A registers a service
    let interfaces = vec![InterfaceDesc {
        name: "org.ouros.test.Calculator".to_owned(),
        version: 1,
        methods: vec![
            MethodDesc {
                name: "Add".to_owned(),
                params: vec![TypeDesc::I32, TypeDesc::I32],
                return_type: TypeDesc::I32,
            },
            MethodDesc {
                name: "Multiply".to_owned(),
                params: vec![TypeDesc::I32, TypeDesc::I32],
                return_type: TypeDesc::I64,
            },
        ],
    }];

    if let Err(e) = bus.register_service(&client_a, "org.ouros.test.calc", interfaces) {
        eprintln!("[selftest] register failed: {e}");
        return;
    }
    eprintln!("[selftest] registered org.ouros.test.calc");

    // Client B looks up the service
    match bus.lookup("org.ouros.test.calc") {
        Ok(entry) => {
            eprintln!(
                "[selftest] lookup OK: owner_pid={}, interfaces={}",
                entry.owner_pid,
                entry.interfaces.len()
            );
        }
        Err(e) => {
            eprintln!("[selftest] lookup failed: {e}");
            return;
        }
    }

    // Introspection
    match bus.introspect("org.ouros.test.calc") {
        Ok(ifaces) => {
            for iface in ifaces {
                eprintln!(
                    "[selftest] interface: {} v{} ({} methods)",
                    iface.name,
                    iface.version,
                    iface.methods.len()
                );
                for method in &iface.methods {
                    eprintln!(
                        "[selftest]   method: {} ({} params) -> {}",
                        method.name,
                        method.params.len(),
                        method.return_type
                    );
                }
            }
        }
        Err(e) => eprintln!("[selftest] introspect failed: {e}"),
    }

    // Client B watches for name changes
    if let Err(e) = bus.watch_name(&client_b, "org.ouros.test.calc") {
        eprintln!("[selftest] watch failed: {e}");
    }

    // Client B subscribes to signals
    if let Err(e) = bus.add_match_rule(
        &client_b,
        MatchRule {
            sender: None,
            interface: Some("org.ouros.test.Calculator".to_owned()),
            member: None,
            destination: None,
        },
    ) {
        eprintln!("[selftest] add_match_rule failed: {e}");
    }

    // Client B sends a method call to client A's service
    let call = Message {
        header: MessageHeader {
            sender: client_b.clone(),
            destination: "org.ouros.test.calc".to_owned(),
            serial: 1,
            reply_serial: None,
            msg_type: MessageType::MethodCall,
            interface: Some("org.ouros.test.Calculator".to_owned()),
            member: Some("Add".to_owned()),
        },
        body: vec![Value::I32(7), Value::I32(35)],
    };

    if let Err(e) = bus.route_message(call) {
        eprintln!("[selftest] route call failed: {e}");
    } else {
        eprintln!("[selftest] method call routed to service");
    }

    // Check client A received the message
    let msgs = bus.drain_outbox(&client_a);
    eprintln!("[selftest] client A outbox: {} messages", msgs.len());

    // Client A sends a reply
    let reply = Message {
        header: MessageHeader {
            sender: client_a.clone(),
            destination: client_b.clone(),
            serial: 1,
            reply_serial: Some(1),
            msg_type: MessageType::MethodReturn,
            interface: None,
            member: None,
        },
        body: vec![Value::I32(42)],
    };

    if let Err(e) = bus.route_message(reply) {
        eprintln!("[selftest] route reply failed: {e}");
    } else {
        eprintln!("[selftest] reply routed back to caller");
    }

    // Check client B received the reply
    let msgs = bus.drain_outbox(&client_b);
    eprintln!("[selftest] client B outbox: {} messages", msgs.len());

    // Test serialization round-trip
    let test_msg = Message {
        header: MessageHeader {
            sender: client_a.clone(),
            destination: client_b.clone(),
            serial: 42,
            reply_serial: None,
            msg_type: MessageType::Signal,
            interface: Some("org.ouros.test.Events".to_owned()),
            member: Some("ValueChanged".to_owned()),
        },
        body: vec![
            Value::String("temperature".to_owned()),
            Value::F64(23.5),
            Value::Optional(Some(Box::new(Value::String("celsius".to_owned())))),
        ],
    };

    match bus.serialize_message(&test_msg) {
        Ok(bytes) => {
            eprintln!("[selftest] serialized message: {} bytes", bytes.len());
        }
        Err(e) => eprintln!("[selftest] serialize failed: {e}"),
    }

    // Test name ownership transfer
    let client_c = bus.accept_connection(300);
    if let Err(e) = bus.handle_hello(&client_c) {
        eprintln!("[selftest] hello C failed: {e}");
        return;
    }

    // Client C requests the same name (should be queued)
    if let Err(e) = bus.register_service(&client_c, "org.ouros.test.calc", vec![]) {
        eprintln!("[selftest] C register failed: {e}");
    } else {
        eprintln!("[selftest] client C queued for org.ouros.test.calc");
    }

    // Disconnect client A — ownership should transfer to C
    bus.disconnect(&client_a);
    eprintln!("[selftest] client A disconnected");

    match bus.lookup("org.ouros.test.calc") {
        Ok(entry) => eprintln!("[selftest] new owner: pid={}", entry.owner_pid),
        Err(e) => eprintln!("[selftest] lookup after transfer: {e}"),
    }

    // Check B received name-change notifications
    let watch_msgs = bus.drain_outbox(&client_b);
    eprintln!(
        "[selftest] client B received {} watch notifications",
        watch_msgs.len()
    );

    // List all names
    let names = bus.list_names();
    eprintln!("[selftest] registered names: {names:?}");

    // Stats
    let stats = bus.stats();
    eprintln!("[selftest] stats: {stats:?}");

    // Cleanup
    bus.disconnect(&client_b);
    bus.disconnect(&client_c);

    eprintln!("[servicebus] === SELF-TEST PASSED ===");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_validation() {
        assert!(ServiceBus::validate_name("org.ouros.test").is_ok());
        assert!(ServiceBus::validate_name("com.example.service").is_ok());
        assert!(ServiceBus::validate_name("a.b").is_ok());
        assert!(ServiceBus::validate_name("org.ouros.audio-mixer").is_ok());
        assert!(ServiceBus::validate_name("org.ouros._private").is_ok());

        // Invalid names
        assert!(ServiceBus::validate_name("").is_err());
        assert!(ServiceBus::validate_name("single").is_err());
        assert!(ServiceBus::validate_name(".leading.dot").is_err());
        assert!(ServiceBus::validate_name("trailing.dot.").is_err());
        assert!(ServiceBus::validate_name("has..double").is_err());
        assert!(ServiceBus::validate_name("1starts.with.digit").is_err());
    }

    #[test]
    fn test_connection_lifecycle() {
        let mut bus = ServiceBus::new();

        let name = bus.accept_connection(42);
        assert!(name.starts_with(":1."));
        assert_eq!(bus.active_connections(), 1);

        assert!(bus.handle_hello(&name).is_ok());

        bus.disconnect(&name);
        assert_eq!(bus.active_connections(), 0);
    }

    #[test]
    fn test_service_registration() {
        let mut bus = ServiceBus::new();

        let conn = bus.accept_connection(1);
        bus.handle_hello(&conn).unwrap();

        let ifaces = vec![InterfaceDesc {
            name: "org.ouros.test.Api".to_owned(),
            version: 1,
            methods: vec![MethodDesc {
                name: "DoThing".to_owned(),
                params: vec![TypeDesc::String],
                return_type: TypeDesc::Bool,
            }],
        }];

        bus.register_service(&conn, "org.ouros.test.svc", ifaces)
            .unwrap();

        assert_eq!(bus.registered_services(), 1);
        let entry = bus.lookup("org.ouros.test.svc").unwrap();
        assert_eq!(entry.owner_pid, 1);
        assert_eq!(entry.interfaces.len(), 1);
    }

    #[test]
    fn test_name_ownership_queue() {
        let mut bus = ServiceBus::new();

        let a = bus.accept_connection(10);
        let b = bus.accept_connection(20);
        bus.handle_hello(&a).unwrap();
        bus.handle_hello(&b).unwrap();

        // A registers first
        bus.register_service(&a, "org.ouros.shared.name", vec![])
            .unwrap();
        assert_eq!(bus.lookup("org.ouros.shared.name").unwrap().owner_pid, 10);

        // B requests same name — gets queued
        bus.register_service(&b, "org.ouros.shared.name", vec![])
            .unwrap();
        // A still owns it
        assert_eq!(bus.lookup("org.ouros.shared.name").unwrap().owner_pid, 10);

        // A disconnects — B takes over
        bus.disconnect(&a);
        assert_eq!(bus.lookup("org.ouros.shared.name").unwrap().owner_pid, 20);
    }

    #[test]
    fn test_message_routing() {
        let mut bus = ServiceBus::new();

        let server = bus.accept_connection(1);
        let client = bus.accept_connection(2);
        bus.handle_hello(&server).unwrap();
        bus.handle_hello(&client).unwrap();

        bus.register_service(&server, "org.ouros.echo", vec![])
            .unwrap();

        // Client calls server
        let call = Message {
            header: MessageHeader {
                sender: client.clone(),
                destination: "org.ouros.echo".to_owned(),
                serial: 1,
                reply_serial: None,
                msg_type: MessageType::MethodCall,
                interface: Some("org.ouros.echo.Api".to_owned()),
                member: Some("Echo".to_owned()),
            },
            body: vec![Value::String("hello".to_owned())],
        };

        bus.route_message(call).unwrap();

        let server_msgs = bus.drain_outbox(&server);
        assert_eq!(server_msgs.len(), 1);
        assert_eq!(server_msgs[0].header.msg_type, MessageType::MethodCall);
    }

    #[test]
    fn test_signal_subscription() {
        let mut bus = ServiceBus::new();

        let sender = bus.accept_connection(1);
        let sub1 = bus.accept_connection(2);
        let sub2 = bus.accept_connection(3);
        bus.handle_hello(&sender).unwrap();
        bus.handle_hello(&sub1).unwrap();
        bus.handle_hello(&sub2).unwrap();

        // sub1 subscribes to all signals from sender
        bus.add_match_rule(
            &sub1,
            MatchRule {
                sender: Some(sender.clone()),
                interface: None,
                member: None,
                destination: None,
            },
        )
        .unwrap();

        // sub2 subscribes to specific interface
        bus.add_match_rule(
            &sub2,
            MatchRule {
                sender: None,
                interface: Some("org.ouros.events".to_owned()),
                member: None,
                destination: None,
            },
        )
        .unwrap();

        // Send a signal matching both
        let sig = Message {
            header: MessageHeader {
                sender: sender.clone(),
                destination: String::new(),
                serial: 1,
                reply_serial: None,
                msg_type: MessageType::Signal,
                interface: Some("org.ouros.events".to_owned()),
                member: Some("SomethingHappened".to_owned()),
            },
            body: vec![Value::U32(99)],
        };

        bus.route_message(sig).unwrap();

        // Both subscribers should get it
        assert_eq!(bus.drain_outbox(&sub1).len(), 1);
        assert_eq!(bus.drain_outbox(&sub2).len(), 1);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let values = vec![
            (Value::Void, TypeDesc::Void),
            (Value::Bool(true), TypeDesc::Bool),
            (Value::U8(255), TypeDesc::U8),
            (Value::U16(1000), TypeDesc::U16),
            (Value::U32(123_456), TypeDesc::U32),
            (Value::U64(9_876_543_210), TypeDesc::U64),
            (Value::I8(-42), TypeDesc::I8),
            (Value::I16(-1000), TypeDesc::I16),
            (Value::I32(-123_456), TypeDesc::I32),
            (Value::I64(-9_876_543_210), TypeDesc::I64),
            (Value::F32(3.25), TypeDesc::F32),
            (Value::F64(1.234_567_890_123_456), TypeDesc::F64),
            (Value::String("hello world".to_owned()), TypeDesc::String),
            (Value::Bytes(vec![1, 2, 3, 4, 5]), TypeDesc::Bytes),
            (Value::Handle(0xDEAD_BEEF), TypeDesc::Handle),
        ];

        for (val, ty) in &values {
            let mut buf = Vec::new();
            let end = serialize_value(&mut buf, 0, val, 0).unwrap();
            let (deserialized, read_end) = deserialize_value(&buf, 0, ty, 0).unwrap();
            assert_eq!(end, read_end, "offset mismatch for {ty}");
            assert_eq!(val, &deserialized, "value mismatch for {ty}");
        }
    }

    #[test]
    fn test_serialization_array() {
        let val = Value::Array(vec![Value::U32(1), Value::U32(2), Value::U32(3)]);
        let ty = TypeDesc::Array(Box::new(TypeDesc::U32));

        let mut buf = Vec::new();
        let end = serialize_value(&mut buf, 0, &val, 0).unwrap();
        let (deserialized, _) = deserialize_value(&buf, 0, &ty, 0).unwrap();
        assert_eq!(val, deserialized);
        assert!(end > 0);
    }

    #[test]
    fn test_serialization_nested_struct() {
        let val = Value::Struct(vec![
            ("name".to_owned(), Value::String("test".to_owned())),
            ("count".to_owned(), Value::U32(42)),
        ]);
        let ty = TypeDesc::Struct(vec![
            ("name".to_owned(), TypeDesc::String),
            ("count".to_owned(), TypeDesc::U32),
        ]);

        let mut buf = Vec::new();
        let end = serialize_value(&mut buf, 0, &val, 0).unwrap();
        let (deserialized, _) = deserialize_value(&buf, 0, &ty, 0).unwrap();
        assert_eq!(val, deserialized);
        assert!(end > 0);
    }

    #[test]
    fn test_serialization_optional() {
        let val_some = Value::Optional(Some(Box::new(Value::String("present".to_owned()))));
        let val_none = Value::Optional(None);
        let ty = TypeDesc::Optional(Box::new(TypeDesc::String));

        let mut buf = Vec::new();
        let end = serialize_value(&mut buf, 0, &val_some, 0).unwrap();
        let (deserialized, _) = deserialize_value(&buf, 0, &ty, 0).unwrap();
        assert_eq!(val_some, deserialized);
        assert!(end > 0);

        let mut buf2 = Vec::new();
        let end2 = serialize_value(&mut buf2, 0, &val_none, 0).unwrap();
        let (deserialized2, _) = deserialize_value(&buf2, 0, &ty, 0).unwrap();
        assert_eq!(val_none, deserialized2);
        assert_eq!(end2, 1);
    }

    #[test]
    fn test_nesting_depth_limit() {
        // Build a deeply nested optional value
        let mut val = Value::U8(1);
        let mut ty = TypeDesc::U8;
        for _ in 0..70 {
            val = Value::Optional(Some(Box::new(val)));
            ty = TypeDesc::Optional(Box::new(ty));
        }

        let mut buf = Vec::new();
        let result = serialize_value(&mut buf, 0, &val, 0);
        assert_eq!(result, Err(SerializeError::NestingTooDeep));
    }

    #[test]
    fn test_rate_limiting() {
        let mut bus = ServiceBus::new();
        bus.rate_limit = 3; // Very low for testing

        let conn = bus.accept_connection(1);
        bus.handle_hello(&conn).unwrap();
        bus.register_service(&conn, "org.ouros.target", vec![])
            .unwrap();

        let other = bus.accept_connection(2);
        bus.handle_hello(&other).unwrap();

        // Send messages up to the limit
        for i in 0..3 {
            let msg = Message {
                header: MessageHeader {
                    sender: other.clone(),
                    destination: "org.ouros.target".to_owned(),
                    serial: i + 1,
                    reply_serial: None,
                    msg_type: MessageType::MethodCall,
                    interface: None,
                    member: Some("Ping".to_owned()),
                },
                body: vec![],
            };
            assert!(bus.route_message(msg).is_ok());
        }

        // Next message should be rate-limited
        let msg = Message {
            header: MessageHeader {
                sender: other.clone(),
                destination: "org.ouros.target".to_owned(),
                serial: 4,
                reply_serial: None,
                msg_type: MessageType::MethodCall,
                interface: None,
                member: Some("Ping".to_owned()),
            },
            body: vec![],
        };
        assert_eq!(bus.route_message(msg), Err(BusError::RateLimited));
    }

    #[test]
    fn test_policy_deny() {
        let mut bus = ServiceBus::new();

        let conn = bus.accept_connection(1);
        bus.handle_hello(&conn).unwrap();
        bus.register_service(&conn, "org.ouros.secure", vec![])
            .unwrap();

        let attacker = bus.accept_connection(2);
        bus.handle_hello(&attacker).unwrap();

        // Add policy: deny attacker from calling secure service
        bus.add_policy_rule(PolicyRule {
            allow: false,
            sender: Some(attacker.clone()),
            destination: Some("org.ouros.secure".to_owned()),
            interface: None,
            member: None,
        });

        let msg = Message {
            header: MessageHeader {
                sender: attacker.clone(),
                destination: "org.ouros.secure".to_owned(),
                serial: 1,
                reply_serial: None,
                msg_type: MessageType::MethodCall,
                interface: None,
                member: Some("Secret".to_owned()),
            },
            body: vec![],
        };

        let result = bus.route_message(msg);
        assert!(matches!(result, Err(BusError::PolicyDenied(_))));
    }

    #[test]
    fn test_unauthenticated_rejected() {
        let mut bus = ServiceBus::new();

        let conn = bus.accept_connection(1);
        // Do NOT call handle_hello

        let result = bus.register_service(&conn, "org.ouros.sneaky", vec![]);
        assert_eq!(result, Err(BusError::NotAuthenticated));
    }

    #[test]
    fn test_name_watching() {
        let mut bus = ServiceBus::new();

        let owner = bus.accept_connection(1);
        let watcher = bus.accept_connection(2);
        bus.handle_hello(&owner).unwrap();
        bus.handle_hello(&watcher).unwrap();

        // Watch before the name exists
        bus.watch_name(&watcher, "org.ouros.watched").unwrap();

        // Register triggers NameAcquired
        bus.register_service(&owner, "org.ouros.watched", vec![])
            .unwrap();

        let msgs = bus.drain_outbox(&watcher);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].header.member,
            Some("NameAcquired".to_owned())
        );

        // Unregister triggers NameLost
        bus.unregister_service(&owner, "org.ouros.watched").unwrap();

        let msgs = bus.drain_outbox(&watcher);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].header.member, Some("NameLost".to_owned()));
    }

    #[test]
    fn test_method_lookup() {
        let mut bus = ServiceBus::new();

        let conn = bus.accept_connection(1);
        bus.handle_hello(&conn).unwrap();

        let ifaces = vec![InterfaceDesc {
            name: "org.ouros.math.Basic".to_owned(),
            version: 2,
            methods: vec![
                MethodDesc {
                    name: "Add".to_owned(),
                    params: vec![TypeDesc::I64, TypeDesc::I64],
                    return_type: TypeDesc::I64,
                },
                MethodDesc {
                    name: "Sqrt".to_owned(),
                    params: vec![TypeDesc::F64],
                    return_type: TypeDesc::F64,
                },
            ],
        }];

        bus.register_service(&conn, "org.ouros.math", ifaces)
            .unwrap();

        // Successful lookup
        let method = bus
            .lookup_method("org.ouros.math", "org.ouros.math.Basic", "Add")
            .unwrap();
        assert_eq!(method.name, "Add");
        assert_eq!(method.params.len(), 2);

        // Interface not found
        let result = bus.lookup_method("org.ouros.math", "org.ouros.math.Advanced", "Foo");
        assert!(matches!(result, Err(BusError::InterfaceNotFound(_))));

        // Method not found
        let result = bus.lookup_method("org.ouros.math", "org.ouros.math.Basic", "Divide");
        assert!(matches!(result, Err(BusError::MethodNotFound(_))));
    }

    #[test]
    fn test_activation_timeout() {
        let mut bus = ServiceBus::new();

        // Register activation with 0-second timeout for testing
        bus.register_activation(ActivationEntry {
            name: "org.ouros.lazy".to_owned(),
            exec_path: "/bin/lazy".to_owned(),
            timeout_secs: 0,
        });

        let client = bus.accept_connection(1);
        bus.handle_hello(&client).unwrap();

        // Send message to trigger activation
        let msg = Message {
            header: MessageHeader {
                sender: client.clone(),
                destination: "org.ouros.lazy".to_owned(),
                serial: 1,
                reply_serial: None,
                msg_type: MessageType::MethodCall,
                interface: None,
                member: Some("Wake".to_owned()),
            },
            body: vec![],
        };

        // Should succeed (activation starts)
        bus.route_message(msg).unwrap();
        assert_eq!(bus.pending_activations.len(), 1);

        // Check timeouts — should immediately time out with 0s timeout
        let timed_out = bus.check_activation_timeouts();
        assert_eq!(timed_out, vec!["org.ouros.lazy"]);
        assert_eq!(bus.pending_activations.len(), 0);
        assert_eq!(bus.stats().activations_timed_out, 1);
    }

    #[test]
    fn test_list_services() {
        let mut bus = ServiceBus::new();

        let c1 = bus.accept_connection(1);
        let c2 = bus.accept_connection(2);
        bus.handle_hello(&c1).unwrap();
        bus.handle_hello(&c2).unwrap();

        bus.register_service(&c1, "org.ouros.alpha", vec![])
            .unwrap();
        bus.register_service(&c2, "org.ouros.beta", vec![]).unwrap();

        let names = bus.list_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"org.ouros.alpha"));
        assert!(names.contains(&"org.ouros.beta"));
    }

    #[test]
    fn test_bus_stats() {
        let mut bus = ServiceBus::new();

        let a = bus.accept_connection(1);
        let b = bus.accept_connection(2);
        bus.handle_hello(&a).unwrap();
        bus.handle_hello(&b).unwrap();

        bus.register_service(&a, "org.ouros.target", vec![])
            .unwrap();

        let msg = Message {
            header: MessageHeader {
                sender: b.clone(),
                destination: "org.ouros.target".to_owned(),
                serial: 1,
                reply_serial: None,
                msg_type: MessageType::MethodCall,
                interface: None,
                member: Some("Ping".to_owned()),
            },
            body: vec![],
        };
        bus.route_message(msg).unwrap();

        let stats = bus.stats();
        assert_eq!(stats.messages_routed, 1);
        assert_eq!(stats.method_calls, 1);
        assert_eq!(stats.total_connections, 2);
    }

    #[test]
    fn test_match_rule_filtering() {
        let msg = Message {
            header: MessageHeader {
                sender: ":1.5".to_owned(),
                destination: String::new(),
                serial: 1,
                reply_serial: None,
                msg_type: MessageType::Signal,
                interface: Some("org.ouros.events".to_owned()),
                member: Some("Click".to_owned()),
            },
            body: vec![],
        };

        // Match all
        let rule_all = MatchRule {
            sender: None,
            interface: None,
            member: None,
            destination: None,
        };
        assert!(rule_all.matches(&msg));

        // Match sender
        let rule_sender = MatchRule {
            sender: Some(":1.5".to_owned()),
            interface: None,
            member: None,
            destination: None,
        };
        assert!(rule_sender.matches(&msg));

        // Wrong sender
        let rule_wrong_sender = MatchRule {
            sender: Some(":1.99".to_owned()),
            interface: None,
            member: None,
            destination: None,
        };
        assert!(!rule_wrong_sender.matches(&msg));

        // Match interface + member
        let rule_iface_member = MatchRule {
            sender: None,
            interface: Some("org.ouros.events".to_owned()),
            member: Some("Click".to_owned()),
            destination: None,
        };
        assert!(rule_iface_member.matches(&msg));

        // Wrong member
        let rule_wrong_member = MatchRule {
            sender: None,
            interface: Some("org.ouros.events".to_owned()),
            member: Some("KeyPress".to_owned()),
            destination: None,
        };
        assert!(!rule_wrong_member.matches(&msg));
    }

    #[test]
    fn test_type_desc_display() {
        assert_eq!(TypeDesc::Void.to_string(), "void");
        assert_eq!(TypeDesc::U32.to_string(), "u32");
        assert_eq!(TypeDesc::String.to_string(), "string");
        assert_eq!(
            TypeDesc::Array(Box::new(TypeDesc::U8)).to_string(),
            "array<u8>"
        );
        assert_eq!(
            TypeDesc::Map(Box::new(TypeDesc::String), Box::new(TypeDesc::I32)).to_string(),
            "map<string, i32>"
        );
        assert_eq!(
            TypeDesc::Optional(Box::new(TypeDesc::Bool)).to_string(),
            "optional<bool>"
        );
    }

    #[test]
    fn test_disconnect_cleanup() {
        let mut bus = ServiceBus::new();

        let conn = bus.accept_connection(1);
        bus.handle_hello(&conn).unwrap();

        bus.register_service(&conn, "org.ouros.ephemeral", vec![])
            .unwrap();
        bus.watch_name(&conn, "org.ouros.something").unwrap();
        bus.add_match_rule(
            &conn,
            MatchRule {
                sender: None,
                interface: None,
                member: None,
                destination: None,
            },
        )
        .unwrap();

        assert_eq!(bus.registered_services(), 1);
        assert_eq!(bus.subscriptions.len(), 1);
        assert_eq!(bus.name_watches.len(), 1);

        bus.disconnect(&conn);

        assert_eq!(bus.registered_services(), 0);
        assert_eq!(bus.active_connections(), 0);
        assert_eq!(bus.subscriptions.len(), 0);
        assert_eq!(bus.name_watches.len(), 0);
    }
}
