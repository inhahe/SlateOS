//! Slate OS Service Bus Client Library (`libservicebus`)
//!
//! A high-level client library for connecting to system services via the
//! kernel's named service registry and communicating over IPC channels.
//!
//! # Architecture
//!
//! The kernel provides:
//! - **Service registry**: named service discovery (register/connect/accept)
//! - **Channels**: bidirectional message-passing IPC primitives
//! - **Completion ports**: event multiplexing (wait on multiple channels/timers)
//!
//! This library wraps those primitives into an ergonomic API:
//! - [`Connection`] — a connected channel to a named service
//! - [`ServiceHost`] — host (server) side of a named service
//! - [`EventLoop`] — multiplexed event-driven dispatcher (completion port wrapper)
//! - [`Message`] — structured message with header + payload
//!
//! # Examples
//!
//! ```no_run
//! use libservicebus::{Connection, Message};
//!
//! # fn main() -> Result<(), libservicebus::BusError> {
//! // Connect to the display compositor
//! let mut conn = Connection::connect("display.compositor")?;
//!
//! // Send a method call
//! let msg = Message::method_call("CreateWindow")
//!     .with_payload(&[800u32.to_le_bytes(), 600u32.to_le_bytes()].concat());
//! conn.send(&msg)?;
//!
//! // Wait for reply
//! let reply = conn.recv()?;
//! assert!(reply.is_reply());
//! # Ok(())
//! # }
//! ```
//!
//! # Event Loop
//!
//! ```no_run
//! use libservicebus::{EventLoop, Connection};
//!
//! # fn main() -> Result<(), libservicebus::BusError> {
//! let mut evloop = EventLoop::new()?;
//! let conn = Connection::connect("net.dns")?;
//! evloop.register_connection(&conn, 1)?;  // user_data = 1
//!
//! loop {
//!     let events = evloop.wait()?;
//!     for event in events {
//!         match event.user_data {
//!             1 => { /* handle DNS reply */ }
//!             _ => {}
//!         }
//!     }
//! }
//! # }
//! ```

use std::fmt;

// ============================================================================
// Syscall numbers — must match kernel/src/syscall/number.rs
// ============================================================================

mod syscall_nr {
    // Channel IPC (200–209)
    pub const SYS_CHANNEL_SEND: u64 = 201;
    pub const SYS_CHANNEL_RECV: u64 = 202;
    pub const SYS_CHANNEL_TRY_RECV: u64 = 203;
    pub const SYS_CHANNEL_CLOSE: u64 = 204;
    pub const SYS_CHANNEL_RECV_TIMEOUT: u64 = 205;
    pub const SYS_CHANNEL_SEND_BLOCKING: u64 = 209;

    // Completion ports (250–256)
    pub const SYS_CP_CREATE: u64 = 250;
    pub const SYS_CP_REGISTER: u64 = 251;
    pub const SYS_CP_UNREGISTER: u64 = 252;
    pub const SYS_CP_WAIT: u64 = 253;
    pub const SYS_CP_TRY_WAIT: u64 = 254;
    pub const SYS_CP_CLOSE: u64 = 255;
    pub const SYS_CP_NOTIFY: u64 = 256;

    // Service registry (280–285)
    pub const SYS_SERVICE_REGISTER: u64 = 280;
    pub const SYS_SERVICE_CONNECT: u64 = 281;
    pub const SYS_SERVICE_ACCEPT: u64 = 282;
    pub const SYS_SERVICE_TRY_ACCEPT: u64 = 283;
    pub const SYS_SERVICE_ACCEPT_TIMEOUT: u64 = 284;
    pub const SYS_SERVICE_UNREGISTER: u64 = 285;

    // Timers (12–13)
    pub const SYS_TIMER_CREATE: u64 = 12;
    pub const SYS_TIMER_CANCEL: u64 = 13;

}

// ============================================================================
// Low-level syscall wrappers
// ============================================================================

/// Raw syscall with variable argument count.
/// On x86_64, syscall ABI: rax=nr, rdi=a0, rsi=a1, rdx=a2, r10=a3, r8=a4, r9=a5
/// Returns: rax (result).
#[inline(always)]
unsafe fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: caller guarantees nr is a valid syscall number.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall1(nr: u64, a0: u64) -> i64 {
    let ret: i64;
    // SAFETY: caller guarantees nr and a0 are valid for the syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") a0,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall2(nr: u64, a0: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: caller guarantees all arguments are valid for the syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") a0,
            in("rsi") a1,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    // SAFETY: caller guarantees all arguments are valid for the syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall4(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: caller guarantees all arguments are valid for the syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            in("r10") a3,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// ============================================================================
// Error type
// ============================================================================

/// Errors returned by service bus operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusError {
    /// Service name not found in registry.
    NotFound,
    /// Channel or connection was closed by the peer.
    Disconnected,
    /// Operation timed out.
    TimedOut,
    /// No data available (non-blocking operation).
    WouldBlock,
    /// Channel send queue is full.
    QueueFull,
    /// Invalid handle or argument.
    InvalidArgument,
    /// Permission denied (capability missing).
    PermissionDenied,
    /// Resource limit exceeded.
    ResourceExhausted,
    /// Kernel returned an unexpected error code.
    Unknown(i64),
}

impl BusError {
    fn from_errno(code: i64) -> Self {
        match code {
            -1 => BusError::NotFound,
            -2 => BusError::InvalidArgument,
            -3 => BusError::PermissionDenied,
            -4 => BusError::WouldBlock,
            -5 => BusError::Disconnected,
            -6 => BusError::TimedOut,
            -7 => BusError::QueueFull,
            -8 => BusError::ResourceExhausted,
            other => BusError::Unknown(other),
        }
    }
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusError::NotFound => write!(f, "service not found"),
            BusError::Disconnected => write!(f, "connection closed"),
            BusError::TimedOut => write!(f, "operation timed out"),
            BusError::WouldBlock => write!(f, "would block"),
            BusError::QueueFull => write!(f, "channel queue full"),
            BusError::InvalidArgument => write!(f, "invalid argument"),
            BusError::PermissionDenied => write!(f, "permission denied"),
            BusError::ResourceExhausted => write!(f, "resource exhausted"),
            BusError::Unknown(code) => write!(f, "unknown error ({code})"),
        }
    }
}

impl std::error::Error for BusError {}

pub type Result<T> = std::result::Result<T, BusError>;

// ============================================================================
// Message format
// ============================================================================

/// Message type identifiers (first byte of wire header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// Method call (request).
    MethodCall = 1,
    /// Method return (success reply).
    MethodReturn = 2,
    /// Error reply.
    Error = 3,
    /// One-way signal (no reply expected).
    Signal = 4,
}

impl MessageType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(MessageType::MethodCall),
            2 => Some(MessageType::MethodReturn),
            3 => Some(MessageType::Error),
            4 => Some(MessageType::Signal),
            _ => None,
        }
    }
}

/// Wire header for bus messages (16 bytes, naturally aligned).
///
/// Layout:
/// ```text
/// [0]     message_type: u8
/// [1]     flags: u8
/// [2..4]  member_len: u16 (length of member/method name)
/// [4..8]  payload_len: u32
/// [8..16] serial: u64 (message sequence number, for matching replies)
/// ```
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // Documents the wire layout, not directly constructed.
struct WireHeader {
    message_type: u8,
    flags: u8,
    member_len: u16,
    payload_len: u32,
    serial: u64,
}

const WIRE_HEADER_SIZE: usize = 16;

/// A structured bus message with header + member name + payload.
#[derive(Clone, Debug)]
pub struct Message {
    /// Type of this message.
    pub msg_type: MessageType,
    /// Flags (reserved, currently 0).
    pub flags: u8,
    /// Method or signal member name.
    pub member: String,
    /// Arbitrary payload bytes.
    pub payload: Vec<u8>,
    /// Serial number (set by send, used to correlate replies).
    pub serial: u64,
    /// Reply serial (for MethodReturn/Error, matches the call's serial).
    pub reply_serial: u64,
}

impl Message {
    /// Create a new method call message.
    pub fn method_call(member: &str) -> Self {
        Message {
            msg_type: MessageType::MethodCall,
            flags: 0,
            member: member.to_string(),
            payload: Vec::new(),
            serial: 0,
            reply_serial: 0,
        }
    }

    /// Create a success reply to a given call.
    pub fn reply(call: &Message) -> Self {
        Message {
            msg_type: MessageType::MethodReturn,
            flags: 0,
            member: String::new(),
            payload: Vec::new(),
            serial: 0,
            reply_serial: call.serial,
        }
    }

    /// Create an error reply to a given call.
    pub fn error(call: &Message, error_name: &str) -> Self {
        Message {
            msg_type: MessageType::Error,
            flags: 0,
            member: error_name.to_string(),
            payload: Vec::new(),
            serial: 0,
            reply_serial: call.serial,
        }
    }

    /// Create a signal message.
    pub fn signal(member: &str) -> Self {
        Message {
            msg_type: MessageType::Signal,
            flags: 0,
            member: member.to_string(),
            payload: Vec::new(),
            serial: 0,
            reply_serial: 0,
        }
    }

    /// Attach payload data.
    pub fn with_payload(mut self, data: &[u8]) -> Self {
        self.payload = data.to_vec();
        self
    }

    /// Check if this is a reply (success or error).
    pub fn is_reply(&self) -> bool {
        matches!(self.msg_type, MessageType::MethodReturn | MessageType::Error)
    }

    /// Check if this is an error reply.
    pub fn is_error(&self) -> bool {
        self.msg_type == MessageType::Error
    }

    /// Serialize to wire format.
    fn to_bytes(&self) -> Vec<u8> {
        let member_bytes = self.member.as_bytes();
        let member_len = member_bytes.len().min(u16::MAX as usize) as u16;
        let payload_len = self.payload.len().min(u32::MAX as usize) as u32;

        let total = WIRE_HEADER_SIZE + member_len as usize + payload_len as usize;
        let mut buf = Vec::with_capacity(total);

        // Write header fields manually (packed struct).
        buf.push(self.msg_type as u8);
        buf.push(self.flags);
        buf.extend_from_slice(&member_len.to_le_bytes());
        buf.extend_from_slice(&payload_len.to_le_bytes());
        buf.extend_from_slice(&self.serial.to_le_bytes());

        // Member name.
        buf.extend_from_slice(&member_bytes[..member_len as usize]);

        // Payload.
        buf.extend_from_slice(&self.payload[..payload_len as usize]);

        buf
    }

    /// Deserialize from wire bytes.
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < WIRE_HEADER_SIZE {
            return None;
        }

        let msg_type = MessageType::from_u8(data[0])?;
        let flags = data[1];
        let member_len = u16::from_le_bytes([data[2], data[3]]) as usize;
        let payload_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let serial = u64::from_le_bytes([
            data[8], data[9], data[10], data[11],
            data[12], data[13], data[14], data[15],
        ]);

        let expected_len = WIRE_HEADER_SIZE + member_len + payload_len;
        if data.len() < expected_len {
            return None;
        }

        let member_start = WIRE_HEADER_SIZE;
        let member_end = member_start + member_len;
        let member = String::from_utf8_lossy(&data[member_start..member_end]).to_string();

        let payload_start = member_end;
        let payload_end = payload_start + payload_len;
        let payload = data[payload_start..payload_end].to_vec();

        Some(Message {
            msg_type,
            flags,
            member,
            payload,
            serial,
            reply_serial: 0,
        })
    }
}

// ============================================================================
// Connection — client side of a service connection
// ============================================================================

/// A connection to a named service via an IPC channel.
///
/// Created by calling [`Connection::connect`] with a service name.
/// The connection owns the channel handle and closes it on drop.
pub struct Connection {
    /// Kernel channel handle.
    handle: u64,
    /// Next message serial number.
    next_serial: u64,
    /// Receive buffer.
    recv_buf: Vec<u8>,
}

impl Connection {
    /// Connect to a named service.
    ///
    /// The kernel looks up the service in its registry, creates a channel pair,
    /// queues one end for the service, and returns the client end.
    pub fn connect(service_name: &str) -> Result<Self> {
        let name_bytes = service_name.as_bytes();
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_SERVICE_CONNECT,
                name_bytes.as_ptr() as u64,
                name_bytes.len() as u64,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(Connection {
            handle: ret as u64,
            next_serial: 1,
            recv_buf: vec![0u8; 4096],
        })
    }

    /// Get the raw channel handle (for use with EventLoop).
    pub fn handle(&self) -> u64 {
        self.handle
    }

    /// Send a message on this connection.
    ///
    /// Assigns a serial number and sends the serialized message.
    /// Returns the assigned serial (useful for matching replies).
    pub fn send(&mut self, msg: &Message) -> Result<u64> {
        let serial = self.next_serial;
        self.next_serial += 1;

        let mut msg_copy = msg.clone();
        msg_copy.serial = serial;
        let data = msg_copy.to_bytes();

        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CHANNEL_SEND,
                self.handle,
                data.as_ptr() as u64,
                data.len() as u64,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(serial)
    }

    /// Send a message, blocking if the channel queue is full.
    pub fn send_blocking(&mut self, msg: &Message) -> Result<u64> {
        let serial = self.next_serial;
        self.next_serial += 1;

        let mut msg_copy = msg.clone();
        msg_copy.serial = serial;
        let data = msg_copy.to_bytes();

        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CHANNEL_SEND_BLOCKING,
                self.handle,
                data.as_ptr() as u64,
                data.len() as u64,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(serial)
    }

    /// Receive a message (blocking).
    pub fn recv(&mut self) -> Result<Message> {
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CHANNEL_RECV,
                self.handle,
                self.recv_buf.as_mut_ptr() as u64,
                self.recv_buf.len() as u64,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        let len = ret as usize;
        Message::from_bytes(&self.recv_buf[..len])
            .ok_or(BusError::InvalidArgument)
    }

    /// Try to receive a message (non-blocking).
    pub fn try_recv(&mut self) -> Result<Option<Message>> {
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CHANNEL_TRY_RECV,
                self.handle,
                self.recv_buf.as_mut_ptr() as u64,
                self.recv_buf.len() as u64,
            )
        };

        if ret < 0 {
            let err = BusError::from_errno(ret);
            if err == BusError::WouldBlock {
                return Ok(None);
            }
            return Err(err);
        }

        if ret == 0 {
            return Ok(None);
        }

        let len = ret as usize;
        let msg = Message::from_bytes(&self.recv_buf[..len])
            .ok_or(BusError::InvalidArgument)?;
        Ok(Some(msg))
    }

    /// Receive a message with a timeout.
    pub fn recv_timeout(&mut self, timeout_ns: u64) -> Result<Message> {
        let ret = unsafe {
            syscall4(
                syscall_nr::SYS_CHANNEL_RECV_TIMEOUT,
                self.handle,
                self.recv_buf.as_mut_ptr() as u64,
                self.recv_buf.len() as u64,
                timeout_ns,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        let len = ret as usize;
        Message::from_bytes(&self.recv_buf[..len])
            .ok_or(BusError::InvalidArgument)
    }

    /// Send a method call and wait for the reply.
    ///
    /// Convenience method that sends a MethodCall and blocks until a
    /// MethodReturn or Error with the matching serial is received.
    pub fn call(&mut self, method: &str, payload: &[u8]) -> Result<Message> {
        let msg = Message::method_call(method).with_payload(payload);
        let serial = self.send(&msg)?;

        // Loop receiving until we get a reply matching our serial.
        loop {
            let reply = self.recv()?;
            if reply.is_reply() && reply.reply_serial == serial {
                return Ok(reply);
            }
            // Discard non-matching messages (signals, etc.)
            // In a real implementation, these would be queued for later dispatch.
        }
    }

    /// Send a method call and wait for the reply with a timeout.
    pub fn call_timeout(&mut self, method: &str, payload: &[u8], timeout_ns: u64) -> Result<Message> {
        let msg = Message::method_call(method).with_payload(payload);
        let serial = self.send(&msg)?;

        let reply = self.recv_timeout(timeout_ns)?;
        if reply.is_reply() && reply.reply_serial == serial {
            return Ok(reply);
        }

        Err(BusError::TimedOut)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        unsafe {
            syscall1(syscall_nr::SYS_CHANNEL_CLOSE, self.handle);
        }
    }
}

// ============================================================================
// ServiceHost — server side
// ============================================================================

/// A named service host that accepts incoming connections.
///
/// Created by calling [`ServiceHost::register`] with a service name.
pub struct ServiceHost {
    /// Kernel listener handle.
    listener: u64,
    /// Service name.
    name: String,
}

impl ServiceHost {
    /// Register a named service.
    ///
    /// The service name must be unique in the registry.
    pub fn register(name: &str) -> Result<Self> {
        let name_bytes = name.as_bytes();
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_SERVICE_REGISTER,
                name_bytes.as_ptr() as u64,
                name_bytes.len() as u64,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(ServiceHost {
            listener: ret as u64,
            name: name.to_string(),
        })
    }

    /// Get the listener handle (for use with EventLoop).
    pub fn listener_handle(&self) -> u64 {
        self.listener
    }

    /// Get the service name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Accept a pending connection (blocking).
    ///
    /// Returns a `Connection` representing the server's end of the channel.
    pub fn accept(&self) -> Result<Connection> {
        let ret = unsafe {
            syscall1(syscall_nr::SYS_SERVICE_ACCEPT, self.listener)
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(Connection {
            handle: ret as u64,
            next_serial: 1,
            recv_buf: vec![0u8; 4096],
        })
    }

    /// Try to accept a connection (non-blocking).
    pub fn try_accept(&self) -> Result<Option<Connection>> {
        let ret = unsafe {
            syscall1(syscall_nr::SYS_SERVICE_TRY_ACCEPT, self.listener)
        };

        if ret < 0 {
            let err = BusError::from_errno(ret);
            if err == BusError::WouldBlock {
                return Ok(None);
            }
            return Err(err);
        }

        Ok(Some(Connection {
            handle: ret as u64,
            next_serial: 1,
            recv_buf: vec![0u8; 4096],
        }))
    }

    /// Accept a connection with a timeout.
    pub fn accept_timeout(&self, timeout_ns: u64) -> Result<Connection> {
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_SERVICE_ACCEPT_TIMEOUT,
                self.listener,
                timeout_ns,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(Connection {
            handle: ret as u64,
            next_serial: 1,
            recv_buf: vec![0u8; 4096],
        })
    }
}

impl Drop for ServiceHost {
    fn drop(&mut self) {
        unsafe {
            syscall1(syscall_nr::SYS_SERVICE_UNREGISTER, self.listener);
        }
    }
}

// ============================================================================
// Event Loop — completion port based event multiplexing
// ============================================================================

/// Source types for completion port registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SourceType {
    /// Channel ready to receive.
    Channel = 0,
    /// Pipe has data to read.
    PipeRead = 1,
    /// Pipe has space to write.
    PipeWrite = 2,
    /// EventFD signaled.
    EventFd = 3,
    /// Process exited.
    ProcessExit = 4,
    /// Timer expired.
    Timer = 5,
    /// Semaphore available.
    Semaphore = 6,
    /// IO completion (io_uring style).
    IoCompletion = 7,
}

/// An event received from the completion port.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Event {
    /// Source type that fired.
    pub source_type: u64,
    /// Source handle that produced the event.
    pub source_handle: u64,
    /// User-provided data associated with this registration.
    pub user_data: u64,
}

/// Multiplexed event loop based on kernel completion ports.
///
/// Allows waiting on multiple channels, timers, and other waitable
/// handles simultaneously. This is the "give me the waitable handle"
/// integration API described in the design.
pub struct EventLoop {
    /// Kernel completion port handle.
    cp_handle: u64,
    /// Buffer for receiving events.
    event_buf: Vec<Event>,
}

impl EventLoop {
    /// Create a new event loop (allocates a kernel completion port).
    pub fn new() -> Result<Self> {
        let ret = unsafe { syscall0(syscall_nr::SYS_CP_CREATE) };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(EventLoop {
            cp_handle: ret as u64,
            event_buf: vec![Event { source_type: 0, source_handle: 0, user_data: 0 }; 64],
        })
    }

    /// Get the raw completion port handle.
    pub fn handle(&self) -> u64 {
        self.cp_handle
    }

    /// Register a connection's channel with the event loop.
    ///
    /// When the channel has a message ready to receive, an event will
    /// fire with the given `user_data`.
    pub fn register_connection(&self, conn: &Connection, user_data: u64) -> Result<()> {
        self.register_source(SourceType::Channel, conn.handle(), user_data)
    }

    /// Register a service host's listener with the event loop.
    ///
    /// When a new connection is pending, an event fires.
    pub fn register_listener(&self, host: &ServiceHost, user_data: u64) -> Result<()> {
        // Listeners are channels internally.
        self.register_source(SourceType::Channel, host.listener_handle(), user_data)
    }

    /// Register any waitable source with the event loop.
    pub fn register_source(&self, source_type: SourceType, handle: u64, user_data: u64) -> Result<()> {
        let ret = unsafe {
            syscall4(
                syscall_nr::SYS_CP_REGISTER,
                self.cp_handle,
                source_type as u64,
                handle,
                user_data,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(())
    }

    /// Unregister a source from the event loop.
    pub fn unregister_source(&self, source_type: SourceType, handle: u64) -> Result<()> {
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CP_UNREGISTER,
                self.cp_handle,
                source_type as u64,
                handle,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(())
    }

    /// Wait for events (blocking).
    ///
    /// Returns a slice of events that fired. Blocks until at least one
    /// event is available.
    pub fn wait(&mut self) -> Result<&[Event]> {
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CP_WAIT,
                self.cp_handle,
                self.event_buf.as_mut_ptr() as u64,
                self.event_buf.len() as u64,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        let count = ret as usize;
        Ok(&self.event_buf[..count])
    }

    /// Poll for events (non-blocking).
    ///
    /// Returns events that are immediately ready, or an empty slice if none.
    pub fn poll(&mut self) -> Result<&[Event]> {
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CP_TRY_WAIT,
                self.cp_handle,
                self.event_buf.as_mut_ptr() as u64,
                self.event_buf.len() as u64,
            )
        };

        if ret < 0 {
            let err = BusError::from_errno(ret);
            if err == BusError::WouldBlock {
                return Ok(&[]);
            }
            return Err(err);
        }

        let count = ret as usize;
        Ok(&self.event_buf[..count])
    }

    /// Post a manual notification to the event loop.
    ///
    /// Useful for waking up a waiter from another thread.
    pub fn notify(&self, source_type: SourceType, handle: u64) -> Result<()> {
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CP_NOTIFY,
                self.cp_handle,
                source_type as u64,
                handle,
            )
        };

        if ret < 0 {
            return Err(BusError::from_errno(ret));
        }

        Ok(())
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        unsafe {
            syscall1(syscall_nr::SYS_CP_CLOSE, self.cp_handle);
        }
    }
}

// ============================================================================
// Timer — kernel timer for periodic/one-shot events
// ============================================================================

/// A kernel timer that can be registered with an event loop.
pub struct Timer {
    handle: u64,
}

impl Timer {
    /// Create a one-shot timer that fires after `duration_ns` nanoseconds.
    pub fn one_shot(duration_ns: u64) -> Result<Self> {
        let ret = unsafe {
            syscall2(syscall_nr::SYS_TIMER_CREATE, duration_ns, 0)
        };

        if ret <= 0 {
            return Err(BusError::ResourceExhausted);
        }

        Ok(Timer { handle: ret as u64 })
    }

    /// Create a periodic timer that fires every `interval_ns` nanoseconds.
    pub fn periodic(interval_ns: u64) -> Result<Self> {
        let ret = unsafe {
            syscall2(syscall_nr::SYS_TIMER_CREATE, interval_ns, 1) // flag bit 0 = periodic
        };

        if ret <= 0 {
            return Err(BusError::ResourceExhausted);
        }

        Ok(Timer { handle: ret as u64 })
    }

    /// Get the raw timer handle (for registering with EventLoop).
    pub fn handle(&self) -> u64 {
        self.handle
    }

    /// Register this timer with an event loop.
    pub fn register(&self, evloop: &EventLoop, user_data: u64) -> Result<()> {
        evloop.register_source(SourceType::Timer, self.handle, user_data)
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        unsafe {
            syscall1(syscall_nr::SYS_TIMER_CANCEL, self.handle);
        }
    }
}

// ============================================================================
// Helper: duration conversions
// ============================================================================

/// Convert milliseconds to nanoseconds.
pub const fn ms_to_ns(ms: u64) -> u64 {
    ms * 1_000_000
}

/// Convert seconds to nanoseconds.
pub const fn secs_to_ns(s: u64) -> u64 {
    s * 1_000_000_000
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_roundtrip() {
        let msg = Message::method_call("GetWindow")
            .with_payload(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut serialized_msg = msg.clone();
        serialized_msg.serial = 42;

        let bytes = serialized_msg.to_bytes();
        let decoded = Message::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.msg_type, MessageType::MethodCall);
        assert_eq!(decoded.member, "GetWindow");
        assert_eq!(decoded.payload, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(decoded.serial, 42);
    }

    #[test]
    fn test_message_types() {
        let call = Message::method_call("Foo");
        let reply = Message::reply(&Message { serial: 7, ..Message::method_call("") });
        let error = Message::error(&Message { serial: 9, ..Message::method_call("") }, "NotFound");
        let signal = Message::signal("Changed");

        assert_eq!(call.msg_type, MessageType::MethodCall);
        assert_eq!(reply.msg_type, MessageType::MethodReturn);
        assert_eq!(reply.reply_serial, 7);
        assert_eq!(error.msg_type, MessageType::Error);
        assert_eq!(error.reply_serial, 9);
        assert_eq!(error.member, "NotFound");
        assert_eq!(signal.msg_type, MessageType::Signal);
    }

    #[test]
    fn test_message_empty_payload() {
        let msg = Message::signal("Ping");
        let bytes = msg.to_bytes();
        let decoded = Message::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.msg_type, MessageType::Signal);
        assert_eq!(decoded.member, "Ping");
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_message_large_payload() {
        let payload: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let msg = Message::method_call("Upload").with_payload(&payload);
        let mut serialized_msg = msg.clone();
        serialized_msg.serial = 1;

        let bytes = serialized_msg.to_bytes();
        let decoded = Message::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.payload.len(), 1024);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn test_wire_header_size() {
        assert_eq!(std::mem::size_of::<WireHeader>(), WIRE_HEADER_SIZE);
    }

    #[test]
    fn test_error_display() {
        assert_eq!(format!("{}", BusError::NotFound), "service not found");
        assert_eq!(format!("{}", BusError::TimedOut), "operation timed out");
        assert_eq!(format!("{}", BusError::Unknown(-99)), "unknown error (-99)");
    }

    #[test]
    fn test_error_from_errno() {
        assert_eq!(BusError::from_errno(-1), BusError::NotFound);
        assert_eq!(BusError::from_errno(-4), BusError::WouldBlock);
        assert_eq!(BusError::from_errno(-5), BusError::Disconnected);
        assert_eq!(BusError::from_errno(-6), BusError::TimedOut);
        assert_eq!(BusError::from_errno(-100), BusError::Unknown(-100));
    }

    #[test]
    fn test_duration_helpers() {
        assert_eq!(ms_to_ns(1), 1_000_000);
        assert_eq!(ms_to_ns(1000), 1_000_000_000);
        assert_eq!(secs_to_ns(1), 1_000_000_000);
        assert_eq!(secs_to_ns(60), 60_000_000_000);
    }
}
