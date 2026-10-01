//! SlateOS service bus client library (`libservicebus`).
//!
//! The transport between programs and named system services. A service
//! registers a name with the kernel, a client connects by that name, and the
//! two exchange [`Message`]s -- method calls, their replies, and one-way
//! signals -- over the channel the kernel brokers between them. This is the
//! "simpler service discovery + RPC mechanism" `design.txt` asks for in place
//! of COM and D-Bus: named services, a channel handle, a small binary protocol.
//!
//! # What the kernel provides, and what this adds
//!
//! | Kernel (native syscalls)                            | Here                               |
//! |-----------------------------------------------------|------------------------------------|
//! | service registry, 280-285: register, connect, accept | [`ServiceHost`], [`Connection::connect`] |
//! | channels, 201-209: one whole message per receive    | [`Connection`]                     |
//! | `SYS_CHANNEL_PEER_CRED`, 286: who connected          | [`Connection::peer_credentials`]   |
//! | completion ports, 250-256; timers, 12-13             | [`EventLoop`], [`Timer`]           |
//!
//! On top of the transport it fixes the two things every service would
//! otherwise invent for itself: the message layout ([`Message`]) and the
//! argument encoding ([`fields`]). [`Connection::call_fields`] puts the two
//! together and is the usual way to call a method: a field list in, and a
//! field list or a refusal out.
//!
//! # What the kernel does not do yet
//!
//! - **A completion port is not woken by channel traffic.** A port polls its
//!   sources when a wait begins, but `channel::send` wakes only a task blocked
//!   in a receive, so [`EventLoop::wait`] reports a message that was already
//!   queued and sleeps through one that arrives later. Lane F's
//!   `requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`,
//!   point 4.
//! - **A listener is not a wait source.** Nothing can wait for "a client is
//!   connecting" together with anything else
//!   (`requests/b-a-a-server-cannot-wait-for-a-new-client-and-its-clients-at-once.md`).
//!
//! Until both land, a server that talks to several clients gives each one a
//! thread blocked in [`Connection::recv`] and accepts on another, as
//! `userspace/logind` does: blocking calls are the part of the channel
//! interface that works.
//!
//! - **Only a native-ABI process reaches any of this.** The kernel picks a
//!   process's syscall table per binary, from its ELF markers: a binary that
//!   carries the SlateOS ABI note runs the native table, one detected as a
//!   Linux binary runs the Linux table -- where the numbers below are other
//!   calls entirely (12 is `brk`, 202 `futex`, 281 `epoll_pwait`). Every
//!   program that uses this library must be built as a native binary. Lane F's
//!   request above, point 3.
//!
//! # Example
//!
//! ```no_run
//! use libservicebus::{Connection, Outcome, secs_to_ns};
//!
//! # fn main() -> Result<(), libservicebus::BusError> {
//! let mut conn = Connection::connect("system.logind")?;
//! match conn.call_fields("ListSessions", &[], secs_to_ns(25))? {
//!     Outcome::Done(lines) => {
//!         for line in lines {
//!             println!("{}", line.escape_ascii());
//!         }
//!     }
//!     Outcome::Refused { error, .. } => eprintln!("refused: {error}"),
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

// ============================================================================
// Syscall numbers -- must match kernel/src/syscall/number.rs
// ============================================================================
//
// The tests read that file and compare, so a renumbering on the kernel side
// fails here rather than sending a message to the wrong syscall.

mod syscall_nr {
    // Channels (200-209).
    pub const SYS_CHANNEL_SEND: u64 = 201;
    pub const SYS_CHANNEL_RECV: u64 = 202;
    pub const SYS_CHANNEL_TRY_RECV: u64 = 203;
    pub const SYS_CHANNEL_CLOSE: u64 = 204;
    pub const SYS_CHANNEL_RECV_TIMEOUT: u64 = 205;
    pub const SYS_CHANNEL_SEND_BLOCKING: u64 = 209;

    // Completion ports (250-256).
    pub const SYS_CP_CREATE: u64 = 250;
    pub const SYS_CP_REGISTER: u64 = 251;
    pub const SYS_CP_UNREGISTER: u64 = 252;
    pub const SYS_CP_WAIT: u64 = 253;
    pub const SYS_CP_TRY_WAIT: u64 = 254;
    pub const SYS_CP_CLOSE: u64 = 255;
    pub const SYS_CP_NOTIFY: u64 = 256;

    // Service registry (280-286).
    pub const SYS_SERVICE_REGISTER: u64 = 280;
    pub const SYS_SERVICE_CONNECT: u64 = 281;
    pub const SYS_SERVICE_ACCEPT: u64 = 282;
    pub const SYS_SERVICE_TRY_ACCEPT: u64 = 283;
    pub const SYS_SERVICE_ACCEPT_TIMEOUT: u64 = 284;
    pub const SYS_SERVICE_UNREGISTER: u64 = 285;
    pub const SYS_CHANNEL_PEER_CRED: u64 = 286;

    // Timers (12-13).
    pub const SYS_TIMER_CREATE: u64 = 12;
    pub const SYS_TIMER_CANCEL: u64 = 13;
}

/// The largest message a channel carries: `MAX_MESSAGE_SIZE` in
/// `kernel/src/ipc/channel.rs` (the tests compare the two).
///
/// A receive buffer this size cannot be outgrown. That matters because the
/// kernel's receive dequeues the message, copies as much as fits and reports
/// the length it *had*: a smaller buffer loses the rest of any message that
/// does not fit, and there is no asking for it again.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024;

// ============================================================================
// Low-level syscall wrappers
// ============================================================================
//
// Each wrapper has two arms, selected by `target_vendor = "slateos"` -- true
// only when compiling for the real OS (see toolchain/x86_64-slateos.json).
//
// The gate is load-bearing. A `syscall` instruction on a development host does
// not fail cleanly: it enters whatever kernel is actually running, carrying a
// SlateOS call number in RAX that means something else there. On Linux, 12 is
// `brk` -- `Timer::one_shot` would move the program break -- and 13 is
// `rt_sigaction`, 202 `futex`, 253 `inotify_init`. The host arm returns
// `ENOSYS` instead.
//
// See known-issues.md
// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.

/// `-ENOSYS`, "function not implemented": what every host arm returns.
///
/// Chosen because it is what a kernel says when a syscall number is not one it
/// knows, which is exactly the honest description of a development host. The
/// native SlateOS table has no code -38, so the value cannot be mistaken for a
/// kernel's answer.
const HOST_ENOSYS: i64 = -38;

/// Raw syscall, x86-64 convention: `rax` = number, `rdi`, `rsi`, `rdx`, `r10`
/// = arguments, result in `rax`; the CPU clobbers `rcx` and `r11`.
///
/// # Safety
///
/// `nr` and the arguments must be valid for that syscall: any pointer among
/// them must be valid for the access the syscall makes through it, for the
/// length it is told.
#[inline(always)]
unsafe fn syscall0(nr: u64) -> i64 {
    #[cfg(target_vendor = "slateos")]
    {
        let ret: i64;
        // SAFETY: the caller guarantees `nr` is a valid syscall that takes no
        // arguments; `rcx` and `r11`, which `syscall` clobbers, are declared.
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
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = nr;
        HOST_ENOSYS
    }
}

/// [`syscall0`] with one argument.
///
/// # Safety
///
/// As [`syscall0`].
#[inline(always)]
unsafe fn syscall1(nr: u64, a0: u64) -> i64 {
    #[cfg(target_vendor = "slateos")]
    {
        let ret: i64;
        // SAFETY: the caller guarantees `nr` and `a0` are valid for the
        // syscall; `rcx` and `r11` are declared clobbered.
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
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = (nr, a0);
        HOST_ENOSYS
    }
}

/// [`syscall0`] with two arguments.
///
/// # Safety
///
/// As [`syscall0`].
#[inline(always)]
unsafe fn syscall2(nr: u64, a0: u64, a1: u64) -> i64 {
    #[cfg(target_vendor = "slateos")]
    {
        let ret: i64;
        // SAFETY: the caller guarantees all arguments are valid for the
        // syscall; `rcx` and `r11` are declared clobbered.
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
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = (nr, a0, a1);
        HOST_ENOSYS
    }
}

/// [`syscall0`] with three arguments.
///
/// # Safety
///
/// As [`syscall0`].
#[inline(always)]
unsafe fn syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> i64 {
    #[cfg(target_vendor = "slateos")]
    {
        let ret: i64;
        // SAFETY: the caller guarantees all arguments are valid for the
        // syscall; `rcx` and `r11` are declared clobbered.
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
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = (nr, a0, a1, a2);
        HOST_ENOSYS
    }
}

/// [`syscall0`] with four arguments.
///
/// # Safety
///
/// As [`syscall0`].
#[inline(always)]
unsafe fn syscall4(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    #[cfg(target_vendor = "slateos")]
    {
        let ret: i64;
        // SAFETY: the caller guarantees all arguments are valid for the
        // syscall; `rcx` and `r11` are declared clobbered.
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
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = (nr, a0, a1, a2, a3);
        HOST_ENOSYS
    }
}

/// A syscall's result: the non-negative value it returned, or the error its
/// negative code names.
fn check(ret: i64) -> Result<u64> {
    u64::try_from(ret).map_err(|_| BusError::from_code(ret))
}

/// A user-space address, as a syscall argument.
fn addr<T>(p: *const T) -> u64 {
    p as usize as u64
}

/// A length, as a syscall argument. `usize` is 64 bits on the only target
/// this library makes syscalls on; the conversion cannot lose anything there.
fn len_arg(n: usize) -> u64 {
    n as u64
}

/// Overwrite `buf` with zeros in a way the optimiser may not drop.
///
/// For buffers that held a message: a message can carry a password (logind's
/// `AuthenticateSession` does), and a plain `fill(0)` just before a buffer is
/// freed is a dead store the compiler is entitled to delete.
fn wipe(buf: &mut [u8]) {
    for byte in buf.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusively borrowed `u8`; a
        // volatile write of a `u8` to it is in bounds.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

// ============================================================================
// Error type
// ============================================================================

/// What went wrong in a service bus operation.
///
/// Most variants are a kernel error code, named for what it means here; the
/// code itself is `KernelError`'s discriminant in `kernel/src/error.rs`, read
/// through the `kerror` table (whose tests hold it to the kernel's source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusError {
    /// No service has that name (`NotFound`).
    NotFound,
    /// Another service already holds that name (`AlreadyExists`).
    AlreadyExists,
    /// The other end closed the channel (`ChannelClosed`).
    Disconnected,
    /// The deadline passed first (`TimedOut`).
    TimedOut,
    /// Nothing was ready, on a call that does not wait (`WouldBlock`).
    WouldBlock,
    /// The channel's queue is full (`ChannelFull`) -- or, from
    /// [`Connection::call`], too many unrelated messages arrived while it
    /// waited for its reply (see [`MAX_PENDING`]).
    QueueFull,
    /// Bigger than a channel carries (`MessageTooLarge`, or this library's
    /// own check before sending: see [`MAX_MESSAGE_SIZE`]).
    MessageTooLarge,
    /// The handle names nothing (`InvalidHandle`).
    InvalidHandle,
    /// The kernel rejected an argument (`InvalidArgument`, `InvalidAddress`),
    /// or a [`Message`] is inconsistent (a reply that answers no call).
    InvalidArgument,
    /// The caller lacks the capability (`PermissionDenied`, `InvalidCapability`).
    PermissionDenied,
    /// A kernel limit or out of memory (`ResourceExhausted`, `OutOfMemory`).
    ResourceExhausted,
    /// The kernel has no such call (`NoSuchSyscall`, `NotSupported`) -- or this
    /// is not SlateOS at all, and the syscall layer is the host stub.
    Unsupported,
    /// A message arrived that is not in this library's wire format.
    Malformed,
    /// Any other kernel code.
    Unknown(i64),
}

impl BusError {
    /// The error a native syscall's negative return names.
    fn from_code(code: i64) -> Self {
        match code {
            kerror::NOT_FOUND => Self::NotFound,
            kerror::ALREADY_EXISTS => Self::AlreadyExists,
            kerror::CHANNEL_CLOSED => Self::Disconnected,
            kerror::TIMED_OUT => Self::TimedOut,
            kerror::WOULD_BLOCK => Self::WouldBlock,
            kerror::CHANNEL_FULL => Self::QueueFull,
            kerror::MESSAGE_TOO_LARGE => Self::MessageTooLarge,
            kerror::INVALID_HANDLE => Self::InvalidHandle,
            kerror::INVALID_ARGUMENT | kerror::INVALID_ADDRESS => Self::InvalidArgument,
            kerror::PERMISSION_DENIED | kerror::INVALID_CAPABILITY => Self::PermissionDenied,
            kerror::RESOURCE_EXHAUSTED | kerror::OUT_OF_MEMORY => Self::ResourceExhausted,
            kerror::NOT_SUPPORTED | kerror::NO_SUCH_SYSCALL | HOST_ENOSYS => Self::Unsupported,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no such service"),
            Self::AlreadyExists => f.write_str("service name already registered"),
            Self::Disconnected => f.write_str("connection closed"),
            Self::TimedOut => f.write_str("timed out"),
            Self::WouldBlock => f.write_str("nothing ready"),
            Self::QueueFull => f.write_str("message queue full"),
            Self::MessageTooLarge => f.write_str("message too large for a channel"),
            Self::InvalidHandle => f.write_str("invalid handle"),
            Self::InvalidArgument => f.write_str("invalid argument"),
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::ResourceExhausted => f.write_str("resource limit reached"),
            Self::Unsupported => f.write_str("not supported here"),
            Self::Malformed => f.write_str("malformed message"),
            Self::Unknown(code) => match kerror::message(*code) {
                Some(words) => write!(f, "{words} (kernel error {code})"),
                None => write!(f, "kernel error {code}"),
            },
        }
    }
}

impl std::error::Error for BusError {}

/// `std::result::Result` with a [`BusError`].
pub type Result<T> = std::result::Result<T, BusError>;

// ============================================================================
// Message format
// ============================================================================

/// What a message is, which is the first byte of its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// A method call: a request, which expects a reply.
    MethodCall = 1,
    /// A successful reply to a method call.
    MethodReturn = 2,
    /// A failed reply to a method call; its `member` names the error.
    Error = 3,
    /// A one-way notification; nothing replies to it.
    Signal = 4,
}

impl MessageType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::MethodCall),
            2 => Some(Self::MethodReturn),
            3 => Some(Self::Error),
            4 => Some(Self::Signal),
            _ => None,
        }
    }
}

/// Size of the fixed message header.
const HEADER_SIZE: usize = 24;

/// A bus message: a header, a member name, and a payload.
///
/// # Wire format (version 2)
///
/// One message is one channel message. All integers little-endian:
///
/// ```text
/// [0]       u8   message type: 1 call, 2 return, 3 error, 4 signal
/// [1]       u8   flags: reserved; carried, not interpreted
/// [2..4]    u16  member length in bytes
/// [4..8]    u32  payload length in bytes
/// [8..16]   u64  serial: the sender's number for this message, never 0
/// [16..24]  u64  reply serial: on a return or error, the serial of the call
///                it answers; 0 on a call or a signal
/// [24..]         member (UTF-8), then payload, and nothing after
/// ```
///
/// Version 1 had a 16-byte header with no reply serial. A reply's
/// `reply_serial` was set in memory by [`Message::reply`] and never written,
/// so every reply arrived answering serial 0, and [`Connection::call`] --
/// which waits for the reply to *its* serial, never 0 -- waited for ever.
///
/// The decoder is strict: a length that disagrees with the bytes received, a
/// member that is not UTF-8, a serial of 0 or a reply serial on the wrong kind
/// of message is [`BusError::Malformed`], not a guess. The member is a method
/// or error name, which this library's own senders only ever make from
/// `&str`; a member that is not UTF-8 was not sent by a peer speaking this
/// protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// What kind of message this is.
    pub msg_type: MessageType,
    /// Reserved, currently 0.
    pub flags: u8,
    /// The method or signal name -- or, on an error reply, the error's name.
    pub member: String,
    /// The arguments, opaque to the bus (services share [`fields`]).
    pub payload: Vec<u8>,
    /// The sender's number for this message. Assigned by
    /// [`Connection::send`]; whatever is here when sending is ignored.
    pub serial: u64,
    /// On a return or error, the serial of the call it answers; 0 otherwise.
    pub reply_serial: u64,
}

impl Message {
    /// A method call.
    #[must_use]
    pub fn method_call(member: &str) -> Self {
        Self::new(MessageType::MethodCall, member, 0)
    }

    /// A successful reply to `call`.
    #[must_use]
    pub fn reply(call: &Message) -> Self {
        Self::new(MessageType::MethodReturn, "", call.serial)
    }

    /// A failed reply to `call`, naming the error.
    #[must_use]
    pub fn error(call: &Message, error_name: &str) -> Self {
        Self::new(MessageType::Error, error_name, call.serial)
    }

    /// A signal.
    #[must_use]
    pub fn signal(member: &str) -> Self {
        Self::new(MessageType::Signal, member, 0)
    }

    fn new(msg_type: MessageType, member: &str, reply_serial: u64) -> Self {
        Self {
            msg_type,
            flags: 0,
            member: member.to_string(),
            payload: Vec::new(),
            serial: 0,
            reply_serial,
        }
    }

    /// The same message with `data` as its payload.
    #[must_use]
    pub fn with_payload(mut self, data: &[u8]) -> Self {
        self.payload = data.to_vec();
        self
    }

    /// Whether this is a reply (a return or an error).
    #[must_use]
    pub fn is_reply(&self) -> bool {
        matches!(
            self.msg_type,
            MessageType::MethodReturn | MessageType::Error
        )
    }

    /// Whether this is an error reply.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.msg_type == MessageType::Error
    }

    /// Serialize, under `serial`.
    ///
    /// Refuses rather than truncates: a member over 65535 bytes or a message
    /// over [`MAX_MESSAGE_SIZE`] is [`BusError::MessageTooLarge`] -- the
    /// version-1 encoder cut both silently, which delivers a different
    /// message from the one that was sent. A reply without a call to answer
    /// (reply serial 0), or a call or signal claiming to answer one, is
    /// [`BusError::InvalidArgument`]: the peer would reject it as malformed,
    /// and the mistake is the sender's to see.
    fn encode(&self, serial: u64) -> Result<Vec<u8>> {
        if self.is_reply() == (self.reply_serial == 0) {
            return Err(BusError::InvalidArgument);
        }
        let member = self.member.as_bytes();
        let member_len = u16::try_from(member.len()).map_err(|_| BusError::MessageTooLarge)?;
        let payload_len =
            u32::try_from(self.payload.len()).map_err(|_| BusError::MessageTooLarge)?;
        let total = HEADER_SIZE
            .checked_add(member.len())
            .and_then(|n| n.checked_add(self.payload.len()))
            .filter(|&n| n <= MAX_MESSAGE_SIZE)
            .ok_or(BusError::MessageTooLarge)?;

        let mut buf = Vec::with_capacity(total);
        buf.push(self.msg_type as u8);
        buf.push(self.flags);
        buf.extend_from_slice(&member_len.to_le_bytes());
        buf.extend_from_slice(&payload_len.to_le_bytes());
        buf.extend_from_slice(&serial.to_le_bytes());
        buf.extend_from_slice(&self.reply_serial.to_le_bytes());
        buf.extend_from_slice(member);
        buf.extend_from_slice(&self.payload);
        Ok(buf)
    }

    /// Deserialize one received message. See the type's documentation for
    /// what is refused.
    fn decode(data: &[u8]) -> Result<Self> {
        Self::decode_inner(data).ok_or(BusError::Malformed)
    }

    fn decode_inner(data: &[u8]) -> Option<Self> {
        let mut rest = data;
        let [ty] = take::<1>(&mut rest)?;
        let [flags] = take::<1>(&mut rest)?;
        let member_len = usize::from(u16::from_le_bytes(take(&mut rest)?));
        let payload_len = usize::try_from(u32::from_le_bytes(take(&mut rest)?)).ok()?;
        let serial = u64::from_le_bytes(take(&mut rest)?);
        let reply_serial = u64::from_le_bytes(take(&mut rest)?);

        if rest.len() != member_len.checked_add(payload_len)? {
            return None;
        }
        let (member, payload) = rest.split_at_checked(member_len)?;
        let member = std::str::from_utf8(member).ok()?;

        let msg_type = MessageType::from_u8(ty)?;
        let is_reply = matches!(msg_type, MessageType::MethodReturn | MessageType::Error);
        if serial == 0 || is_reply == (reply_serial == 0) {
            return None;
        }

        Some(Self {
            msg_type,
            flags,
            member: member.to_string(),
            payload: payload.to_vec(),
            serial,
            reply_serial,
        })
    }
}

/// Split the first `N` bytes off `buf`.
fn take<const N: usize>(buf: &mut &[u8]) -> Option<[u8; N]> {
    let (head, rest) = buf.split_first_chunk::<N>()?;
    *buf = rest;
    Some(*head)
}

// ============================================================================
// Peer credentials
// ============================================================================

/// Who is on the other end of a connection, as reported by the kernel.
///
/// A service that does anything privileged needs this: the *only* trustworthy
/// answer to "who is asking?" comes from the kernel, because it is the one
/// party to the conversation that the caller cannot lie to. An identity the
/// client sends in its own message body is not an identity -- it is a claim.
///
/// Obtained from [`Connection::peer_credentials`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credentials {
    /// Process ID of the peer when the connection was established.
    ///
    /// Recorded at connect time on purpose: a pid read *later* can name a
    /// different process, because the original may have exited and the number
    /// been reused. A credential that changes meaning underneath its holder is
    /// worse than no credential.
    pub pid: u32,
    /// Effective user ID of the peer at connect time.
    pub uid: u32,
    /// Effective group ID of the peer at connect time.
    pub gid: u32,
}

impl Credentials {
    /// Whether the peer is the superuser.
    #[must_use]
    pub const fn is_root(self) -> bool {
        self.uid == 0
    }

    /// Read `SYS_CHANNEL_PEER_CRED`'s 16-byte record: little-endian `pid`,
    /// `uid`, `gid`, then a reserved word -- zero today, kept so a pid
    /// generation can be added without changing the size. It is not read, so
    /// a kernel that starts filling it in does not make this refuse.
    fn from_record(record: [u8; 16]) -> Self {
        let [p0, p1, p2, p3, u0, u1, u2, u3, g0, g1, g2, g3, _, _, _, _] = record;
        Self {
            pid: u32::from_le_bytes([p0, p1, p2, p3]),
            uid: u32::from_le_bytes([u0, u1, u2, u3]),
            gid: u32::from_le_bytes([g0, g1, g2, g3]),
        }
    }
}

// ============================================================================
// The channel underneath a connection
// ============================================================================

/// How long a receive may wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wait {
    /// Not at all: [`BusError::WouldBlock`] if nothing is queued.
    Never,
    /// Until a message arrives or the channel closes.
    Forever,
    /// At most this many nanoseconds (never 0: that is [`Wait::Never`]).
    Nanos(u64),
}

/// The four kernel calls a [`Connection`] makes on its channel.
///
/// A trait so that everything above them -- serials, matching a reply to its
/// call, queueing what arrives meanwhile, the size limits -- can be tested on a
/// development host, where the calls themselves are the `ENOSYS` stub. The
/// version-1 library had no such seam and no test of a round trip, which is
/// how a `call` that could never return went unnoticed.
trait Endpoint: Send {
    /// The kernel's handle, for registering with an [`EventLoop`].
    fn raw_handle(&self) -> u64;
    /// Send one message; with `blocking`, wait for room in a full queue.
    fn send(&mut self, data: &[u8], blocking: bool) -> Result<()>;
    /// Receive one message into `buf` and return the length the kernel
    /// reported for it -- which exceeds `buf.len()` if it did not fit, because
    /// the kernel copies what fits and discards the rest. Nothing queued under
    /// [`Wait::Never`] is `Err(WouldBlock)`.
    fn recv(&mut self, buf: &mut [u8], wait: Wait) -> Result<usize>;
    /// The kernel's record of the peer, if it has one.
    fn peer_credentials(&self) -> Option<Credentials>;
}

/// A channel endpoint the kernel brokered. Closing it is dropping it.
struct KernelChannel {
    handle: u64,
}

impl Endpoint for KernelChannel {
    fn raw_handle(&self) -> u64 {
        self.handle
    }

    fn send(&mut self, data: &[u8], blocking: bool) -> Result<()> {
        let nr = if blocking {
            syscall_nr::SYS_CHANNEL_SEND_BLOCKING
        } else {
            syscall_nr::SYS_CHANNEL_SEND
        };
        // SAFETY: both send syscalls read `data.len()` bytes at the pointer,
        // which is a live borrowed slice for the duration of the call.
        let ret = unsafe { syscall3(nr, self.handle, addr(data.as_ptr()), len_arg(data.len())) };
        check(ret).map(drop)
    }

    fn recv(&mut self, buf: &mut [u8], wait: Wait) -> Result<usize> {
        let (ptr, cap) = (addr(buf.as_mut_ptr()), len_arg(buf.len()));
        // SAFETY: every receive writes at most `cap` bytes at `ptr`, which is
        // a live, exclusively borrowed slice of exactly that length.
        let ret = unsafe {
            match wait {
                Wait::Never => syscall3(syscall_nr::SYS_CHANNEL_TRY_RECV, self.handle, ptr, cap),
                Wait::Forever => syscall3(syscall_nr::SYS_CHANNEL_RECV, self.handle, ptr, cap),
                Wait::Nanos(ns) => syscall4(
                    syscall_nr::SYS_CHANNEL_RECV_TIMEOUT,
                    self.handle,
                    ptr,
                    cap,
                    ns,
                ),
            }
        };
        let len = usize::try_from(check(ret)?).map_err(|_| BusError::MessageTooLarge)?;
        // The non-blocking receive answers an empty queue with length 0. No
        // message in this format is empty -- the header alone is 24 bytes --
        // so 0 cannot be a message, whatever the wait.
        if len == 0 {
            return Err(if wait == Wait::Never {
                BusError::WouldBlock
            } else {
                BusError::Malformed
            });
        }
        Ok(len)
    }

    fn peer_credentials(&self) -> Option<Credentials> {
        let mut record = [0u8; 16];
        // SAFETY: SYS_CHANNEL_PEER_CRED writes exactly 16 bytes at its second
        // argument, which is this live, exclusively borrowed 16-byte array.
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_CHANNEL_PEER_CRED,
                self.handle,
                addr(record.as_mut_ptr()),
            )
        };
        // Every failure collapses into "unknown", deliberately. The kernel
        // already folds "no such channel" and "no record of the peer" into one
        // `NotFound`, because the only thing a caller that must fail closed can
        // do with either is refuse; a bad buffer or the host stub is the same
        // answer for the same reason.
        (ret == 0).then(|| Credentials::from_record(record))
    }
}

impl Drop for KernelChannel {
    fn drop(&mut self) {
        // SAFETY: SYS_CHANNEL_CLOSE takes a handle and no pointers.
        let ret = unsafe { syscall1(syscall_nr::SYS_CHANNEL_CLOSE, self.handle) };
        // A close cannot be retried and has no one to report to: the kernel's
        // close returns 0 unconditionally, and a handle that was already gone
        // has nothing left to release.
        let _ = ret;
    }
}

// ============================================================================
// Connection -- one end of a brokered channel
// ============================================================================

/// The most messages a [`Connection`] holds back while [`Connection::call`]
/// waits for its own reply.
///
/// A call keeps everything else that arrives meanwhile -- signals, replies to
/// abandoned calls -- for [`Connection::recv`] to return in order. Without a
/// bound, a peer that keeps sending while never answering could grow that
/// queue without limit. When it reaches this many the call fails with
/// [`BusError::QueueFull`], keeping every held message (the one that filled
/// the queue included), and later calls are refused until
/// [`Connection::recv`] has drained some.
pub const MAX_PENDING: usize = 64;

/// What a service answered to [`Connection::call_fields`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It did what was asked, and returned these fields.
    Done(Vec<Vec<u8>>),
    /// It refused.
    Refused {
        /// The error's name, such as `system.logind.Error.NoSuchSession`.
        error: String,
        /// Whatever it sent with the refusal -- often nothing.
        fields: Vec<Vec<u8>>,
    },
}

/// One end of a channel to a service, or (from [`ServiceHost::accept`]) to a
/// client.
///
/// Owns the channel and closes it on drop.
pub struct Connection {
    endpoint: Box<dyn Endpoint>,
    /// The serial the next sent message gets. Never 0.
    next_serial: u64,
    /// Receive buffer, [`MAX_MESSAGE_SIZE`] bytes; wiped after every message.
    recv_buf: Vec<u8>,
    /// Messages that arrived while [`Connection::call`] waited for a reply
    /// that was not them, oldest first.
    pending: VecDeque<Message>,
}

impl Connection {
    fn from_endpoint(endpoint: Box<dyn Endpoint>) -> Self {
        Self {
            endpoint,
            next_serial: 1,
            recv_buf: vec![0u8; MAX_MESSAGE_SIZE],
            pending: VecDeque::new(),
        }
    }

    fn from_handle(handle: u64) -> Self {
        Self::from_endpoint(Box::new(KernelChannel { handle }))
    }

    /// Connect to the service registered as `service_name`.
    ///
    /// The kernel creates a channel, queues one end for the service to
    /// accept, and returns the other.
    ///
    /// # Errors
    ///
    /// [`BusError::NotFound`] if no service has that name; any other kernel
    /// refusal as its [`BusError`].
    pub fn connect(service_name: &str) -> Result<Self> {
        let name = service_name.as_bytes();
        // SAFETY: SYS_SERVICE_CONNECT reads `name.len()` bytes at the pointer,
        // a live borrowed slice for the duration of the call.
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_SERVICE_CONNECT,
                addr(name.as_ptr()),
                len_arg(name.len()),
            )
        };
        check(ret).map(Self::from_handle)
    }

    /// The channel's kernel handle, for [`EventLoop::register_connection`].
    #[must_use]
    pub fn handle(&self) -> u64 {
        self.endpoint.raw_handle()
    }

    /// The kernel's record of who is on the other end, if it has one.
    ///
    /// Recorded when the channel was bound to each side -- by the connect for
    /// a client end, by the accept for a server end -- not looked up now: a
    /// process that drops its privileges after connecting keeps the authority
    /// it connected with, and one that gains them later does not gain it here.
    ///
    /// `None` means **the identity of the caller is unknown**, and a caller
    /// whose identity is unknown must be treated as untrusted -- not as
    /// "probably the user". Services must fail closed on `None`. It is the
    /// answer for a channel the kernel did not broker, for one whose peer was
    /// a kernel task, and on a development host.
    ///
    /// One limit is the kernel's and not this function's: today a channel
    /// handle is guessable and its syscalls do not check who holds it, so this
    /// proves who *connected*, not who is sending (lane F's
    /// `requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`,
    /// point 1).
    #[must_use]
    pub fn peer_credentials(&self) -> Option<Credentials> {
        self.endpoint.peer_credentials()
    }

    /// Send `msg`, failing with [`BusError::QueueFull`] rather than waiting if
    /// the channel's queue is full. Returns the serial it was sent under,
    /// which is what a reply to it will carry as its `reply_serial`.
    ///
    /// # Errors
    ///
    /// [`BusError::MessageTooLarge`] or [`BusError::InvalidArgument`] for a
    /// message that cannot be encoded (see [`Message`]); otherwise the
    /// kernel's refusal.
    pub fn send(&mut self, msg: &Message) -> Result<u64> {
        self.send_with(msg, false)
    }

    /// [`send`](Self::send), but waiting for room if the queue is full.
    ///
    /// # Errors
    ///
    /// As [`send`](Self::send), except that a full queue is waited out.
    pub fn send_blocking(&mut self, msg: &Message) -> Result<u64> {
        self.send_with(msg, true)
    }

    fn send_with(&mut self, msg: &Message, blocking: bool) -> Result<u64> {
        let serial = self.next_serial;
        let data = msg.encode(serial)?;
        self.endpoint.send(&data, blocking)?;
        // 2^64 sends will not happen; if they did, the wrap skips 0, which
        // means "answers nothing".
        self.next_serial = serial.checked_add(1).unwrap_or(1);
        Ok(serial)
    }

    /// Receive the next message, waiting for one.
    ///
    /// Messages held back by an earlier [`call`](Self::call) come first.
    ///
    /// # Errors
    ///
    /// [`BusError::Disconnected`] once the peer has closed and nothing is
    /// left; [`BusError::Malformed`] for a message not in this format (it is
    /// consumed); otherwise the kernel's refusal.
    pub fn recv(&mut self) -> Result<Message> {
        match self.pending.pop_front() {
            Some(msg) => Ok(msg),
            None => self.recv_raw(Wait::Forever),
        }
    }

    /// Receive the next message if one is ready.
    ///
    /// # Errors
    ///
    /// As [`recv`](Self::recv); nothing ready is `Ok(None)`, not an error.
    pub fn try_recv(&mut self) -> Result<Option<Message>> {
        if let Some(msg) = self.pending.pop_front() {
            return Ok(Some(msg));
        }
        match self.recv_raw(Wait::Never) {
            Ok(msg) => Ok(Some(msg)),
            Err(BusError::WouldBlock) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Receive the next message, waiting at most `timeout_ns` nanoseconds.
    ///
    /// # Errors
    ///
    /// [`BusError::TimedOut`] if none arrived in time -- including at once,
    /// for a timeout of 0 -- and otherwise as [`recv`](Self::recv).
    pub fn recv_timeout(&mut self, timeout_ns: u64) -> Result<Message> {
        if let Some(msg) = self.pending.pop_front() {
            return Ok(msg);
        }
        let wait = if timeout_ns == 0 {
            Wait::Never
        } else {
            Wait::Nanos(timeout_ns)
        };
        self.recv_raw(wait).map_err(|e| match e {
            BusError::WouldBlock => BusError::TimedOut,
            other => other,
        })
    }

    /// Call `method` with `payload` and wait for its reply.
    ///
    /// The reply is the message whose `reply_serial` is this call's serial --
    /// a return or an error, so check [`Message::is_error`]. Anything else that
    /// arrives first is kept, in order, for [`recv`](Self::recv).
    ///
    /// # Errors
    ///
    /// As [`send`](Self::send) and [`recv`](Self::recv), and
    /// [`BusError::QueueFull`] past [`MAX_PENDING`] held messages.
    pub fn call(&mut self, method: &str, payload: &[u8]) -> Result<Message> {
        let serial = self.send_call(method, payload)?;
        loop {
            let msg = self.recv_raw(Wait::Forever)?;
            if let Some(reply) = self.claim(msg, serial)? {
                return Ok(reply);
            }
        }
    }

    /// [`call`](Self::call), giving up after `timeout_ns` nanoseconds.
    ///
    /// The time limit covers the whole wait, however many unrelated messages
    /// arrive during it. (Version 1 gave up -- with `TimedOut` -- at the
    /// first message that was not the reply, however early.)
    ///
    /// # Errors
    ///
    /// [`BusError::TimedOut`] if the reply did not arrive in time; otherwise
    /// as [`call`](Self::call). A reply that arrives after the deadline is
    /// returned by a later [`recv`](Self::recv), like any other message.
    pub fn call_timeout(
        &mut self,
        method: &str,
        payload: &[u8],
        timeout_ns: u64,
    ) -> Result<Message> {
        // A deadline past the end of the clock is no deadline.
        let deadline = Instant::now().checked_add(Duration::from_nanos(timeout_ns));
        let serial = self.send_call(method, payload)?;
        loop {
            let wait = match deadline {
                None => Wait::Forever,
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    match u64::try_from(left.as_nanos()) {
                        Ok(0) => return Err(BusError::TimedOut),
                        Ok(ns) => Wait::Nanos(ns),
                        Err(_) => Wait::Forever,
                    }
                }
            };
            let msg = self.recv_raw(wait)?;
            if let Some(reply) = self.claim(msg, serial)? {
                return Ok(reply);
            }
        }
    }

    /// Call `method` with `args` as a [`fields`] list, wait at most
    /// `timeout_ns` nanoseconds, and decode the reply's fields.
    ///
    /// The shape nearly every service call has: arguments in and results out,
    /// both as field lists. A refusal is `Ok(Outcome::Refused)`, not an
    /// `Err` -- the service answered, and the answer was no -- so that `Err`
    /// keeps one meaning: no answer was had. A caller that checked only the
    /// `Result` of [`call`](Self::call) would decode an error reply's payload
    /// as results, which is the mistake this exists to make impossible.
    ///
    /// An empty payload is read as no fields, on either kind of reply: an
    /// error built with [`Message::error`] carries nothing.
    ///
    /// # Errors
    ///
    /// As [`call_timeout`](Self::call_timeout), and [`BusError::Malformed`]
    /// for a reply whose payload is not a field list.
    pub fn call_fields(
        &mut self,
        method: &str,
        args: &[&[u8]],
        timeout_ns: u64,
    ) -> Result<Outcome> {
        let reply = self.call_timeout(method, &fields::encode(args), timeout_ns)?;
        let decoded = if reply.payload.is_empty() {
            Vec::new()
        } else {
            fields::decode(&reply.payload)
                .ok_or(BusError::Malformed)?
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect()
        };
        Ok(if reply.is_error() {
            Outcome::Refused {
                error: reply.member,
                fields: decoded,
            }
        } else {
            Outcome::Done(decoded)
        })
    }

    /// Send a method call, unless the held-back queue is already full: a call
    /// made then could only fail at its first unrelated message, after the
    /// service had acted on it.
    fn send_call(&mut self, method: &str, payload: &[u8]) -> Result<u64> {
        if self.pending.len() >= MAX_PENDING {
            return Err(BusError::QueueFull);
        }
        self.send(&Message::method_call(method).with_payload(payload))
    }

    /// `msg` if it is the reply to `serial`; otherwise hold it for `recv`.
    fn claim(&mut self, msg: Message, serial: u64) -> Result<Option<Message>> {
        if msg.is_reply() && msg.reply_serial == serial {
            return Ok(Some(msg));
        }
        // Kept even when it fills the queue: dropping it would lose a message
        // nobody has seen. `send_call` refused to start this call with the
        // queue full, so this push takes it to MAX_PENDING at most.
        self.pending.push_back(msg);
        if self.pending.len() >= MAX_PENDING {
            return Err(BusError::QueueFull);
        }
        Ok(None)
    }

    /// Receive and decode one message from the channel itself.
    fn recv_raw(&mut self, wait: Wait) -> Result<Message> {
        let len = self.endpoint.recv(&mut self.recv_buf, wait)?;
        let Some(bytes) = self.recv_buf.get_mut(..len) else {
            // Longer than any channel carries: the kernel's limit and
            // `MAX_MESSAGE_SIZE` disagree. The kernel has already dequeued
            // it and kept what did not fit, so the message is lost either way;
            // what is in the buffer is a fragment, and goes.
            wipe(&mut self.recv_buf);
            return Err(BusError::MessageTooLarge);
        };
        let decoded = Message::decode(bytes);
        wipe(bytes);
        decoded
    }
}

// ============================================================================
// ServiceHost -- the service side
// ============================================================================

/// A registered service name, and the listener that accepts its clients.
///
/// Unregisters the name on drop.
///
/// A listener cannot join an [`EventLoop`]: the kernel has no wait source for
/// a pending connection (see the crate documentation). A server that must
/// also wait on its clients accepts on a thread of its own.
pub struct ServiceHost {
    /// Kernel listener handle.
    listener: u64,
    /// The registered name.
    name: String,
}

impl ServiceHost {
    /// Register `name`. Names are unique: one service per name.
    ///
    /// # Errors
    ///
    /// [`BusError::AlreadyExists`] if another service holds the name;
    /// otherwise the kernel's refusal.
    pub fn register(name: &str) -> Result<Self> {
        let bytes = name.as_bytes();
        // SAFETY: SYS_SERVICE_REGISTER reads `bytes.len()` bytes at the
        // pointer, a live borrowed slice for the duration of the call.
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_SERVICE_REGISTER,
                addr(bytes.as_ptr()),
                len_arg(bytes.len()),
            )
        };
        Ok(Self {
            listener: check(ret)?,
            name: name.to_string(),
        })
    }

    /// The listener's kernel handle.
    #[must_use]
    pub fn listener_handle(&self) -> u64 {
        self.listener
    }

    /// The registered name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Accept the next client, waiting for one.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn accept(&self) -> Result<Connection> {
        // SAFETY: SYS_SERVICE_ACCEPT takes a handle and no pointers.
        let ret = unsafe { syscall1(syscall_nr::SYS_SERVICE_ACCEPT, self.listener) };
        check(ret).map(Connection::from_handle)
    }

    /// Accept a client if one is waiting.
    ///
    /// # Errors
    ///
    /// The kernel's refusal; no client waiting is `Ok(None)`.
    pub fn try_accept(&self) -> Result<Option<Connection>> {
        // SAFETY: SYS_SERVICE_TRY_ACCEPT takes a handle and no pointers.
        let ret = unsafe { syscall1(syscall_nr::SYS_SERVICE_TRY_ACCEPT, self.listener) };
        match check(ret) {
            Ok(handle) => Ok(Some(Connection::from_handle(handle))),
            Err(BusError::WouldBlock) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Accept the next client, waiting at most `timeout_ns` nanoseconds.
    ///
    /// # Errors
    ///
    /// [`BusError::TimedOut`] if none came; otherwise the kernel's refusal.
    pub fn accept_timeout(&self, timeout_ns: u64) -> Result<Connection> {
        // SAFETY: SYS_SERVICE_ACCEPT_TIMEOUT takes a handle and a duration,
        // no pointers.
        let ret = unsafe {
            syscall2(
                syscall_nr::SYS_SERVICE_ACCEPT_TIMEOUT,
                self.listener,
                timeout_ns,
            )
        };
        check(ret).map(Connection::from_handle)
    }
}

impl Drop for ServiceHost {
    fn drop(&mut self) {
        // SAFETY: SYS_SERVICE_UNREGISTER takes a handle and no pointers.
        let ret = unsafe { syscall1(syscall_nr::SYS_SERVICE_UNREGISTER, self.listener) };
        // Nothing to do with a failure here: the name is being given up, and
        // a listener the kernel no longer has is already given up.
        let _ = ret;
    }
}

// ============================================================================
// Event loop -- completion port based event multiplexing
// ============================================================================

/// The kinds of source a completion port can watch: the `arg1` of
/// `SYS_CP_REGISTER` (the tests compare these with the kernel's encoding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SourceType {
    /// A channel with a message to receive.
    Channel = 0,
    /// A pipe with data to read.
    PipeRead = 1,
    /// A pipe with room to write.
    PipeWrite = 2,
    /// An eventfd whose counter is non-zero.
    EventFd = 3,
    /// A process that has exited.
    ProcessExit = 4,
    /// A timer that has expired.
    Timer = 5,
    /// A semaphore with a unit available.
    Semaphore = 6,
    /// An I/O ring with completions.
    IoCompletion = 7,
}

/// One event from a completion port: `CpEventRaw` in the kernel's
/// `syscall/handlers.rs`, 24 bytes.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Event {
    /// The [`SourceType`] that fired, as its number.
    pub source_type: u64,
    /// The handle of the source that fired.
    pub source_handle: u64,
    /// The value given when the source was registered.
    pub user_data: u64,
}

/// A wait over many sources: a kernel completion port.
///
/// This is the "give me the waitable handle" integration `design.txt` asks
/// libraries to offer. Read the crate documentation before relying on it for
/// channels: the kernel does not yet wake a port when a message arrives.
pub struct EventLoop {
    /// Kernel completion port handle.
    cp_handle: u64,
    /// Where the kernel writes events.
    event_buf: Vec<Event>,
}

impl EventLoop {
    /// Create one (a new kernel completion port).
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn new() -> Result<Self> {
        // SAFETY: SYS_CP_CREATE takes no arguments.
        let ret = unsafe { syscall0(syscall_nr::SYS_CP_CREATE) };
        Ok(Self {
            cp_handle: check(ret)?,
            event_buf: vec![
                Event {
                    source_type: 0,
                    source_handle: 0,
                    user_data: 0,
                };
                64
            ],
        })
    }

    /// The completion port's kernel handle.
    #[must_use]
    pub fn handle(&self) -> u64 {
        self.cp_handle
    }

    /// Watch `conn` for a message to receive, reporting `user_data`.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn register_connection(&self, conn: &Connection, user_data: u64) -> Result<()> {
        self.register_source(SourceType::Channel, conn.handle(), user_data)
    }

    /// Watch any source, reporting `user_data` when it is ready.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn register_source(
        &self,
        source_type: SourceType,
        handle: u64,
        user_data: u64,
    ) -> Result<()> {
        // SAFETY: SYS_CP_REGISTER takes handles and plain values, no pointers.
        let ret = unsafe {
            syscall4(
                syscall_nr::SYS_CP_REGISTER,
                self.cp_handle,
                source_type as u64,
                handle,
                user_data,
            )
        };
        check(ret).map(drop)
    }

    /// Stop watching a source.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn unregister_source(&self, source_type: SourceType, handle: u64) -> Result<()> {
        // SAFETY: SYS_CP_UNREGISTER takes handles and a plain value, no
        // pointers.
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CP_UNREGISTER,
                self.cp_handle,
                source_type as u64,
                handle,
            )
        };
        check(ret).map(drop)
    }

    /// Wait until at least one source is ready, and return what is.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn wait(&mut self) -> Result<&[Event]> {
        self.collect(syscall_nr::SYS_CP_WAIT)
    }

    /// Return whatever is ready now, possibly nothing.
    ///
    /// # Errors
    ///
    /// The kernel's refusal; nothing ready is an empty slice.
    pub fn poll(&mut self) -> Result<&[Event]> {
        match self.collect(syscall_nr::SYS_CP_TRY_WAIT) {
            Err(BusError::WouldBlock) => Ok(&[]),
            other => other,
        }
    }

    fn collect(&mut self, nr: u64) -> Result<&[Event]> {
        let (ptr, cap) = (
            addr(self.event_buf.as_mut_ptr()),
            len_arg(self.event_buf.len()),
        );
        // SAFETY: the wait syscalls write at most `cap` 24-byte `CpEventRaw`
        // records at `ptr`; `event_buf` is a live, exclusively borrowed array
        // of `cap` `Event`s, which are `repr(C)` and the same 24 bytes.
        let ret = unsafe { syscall3(nr, self.cp_handle, ptr, cap) };
        let count = usize::try_from(check(ret)?).map_err(|_| BusError::InvalidArgument)?;
        // The kernel writes at most `cap`; a larger count is not a number of
        // events this buffer holds, and is refused rather than sliced past.
        self.event_buf.get(..count).ok_or(BusError::InvalidArgument)
    }

    /// Post an event for a registered source by hand, waking a waiter.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn notify(&self, source_type: SourceType, handle: u64) -> Result<()> {
        // SAFETY: SYS_CP_NOTIFY takes handles and a plain value, no pointers.
        let ret = unsafe {
            syscall3(
                syscall_nr::SYS_CP_NOTIFY,
                self.cp_handle,
                source_type as u64,
                handle,
            )
        };
        check(ret).map(drop)
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        // SAFETY: SYS_CP_CLOSE takes a handle and no pointers.
        let ret = unsafe { syscall1(syscall_nr::SYS_CP_CLOSE, self.cp_handle) };
        // The port is going away whatever this says; there is nothing to
        // retry and no one left to tell.
        let _ = ret;
    }
}

// ============================================================================
// Timer
// ============================================================================

/// A kernel timer, which an [`EventLoop`] can wait on.
pub struct Timer {
    handle: u64,
}

impl Timer {
    /// A timer that fires once, `duration_ns` nanoseconds from now.
    ///
    /// # Errors
    ///
    /// [`BusError::ResourceExhausted`] if the kernel's timer table is full;
    /// otherwise the kernel's refusal.
    pub fn one_shot(duration_ns: u64) -> Result<Self> {
        Self::create(duration_ns, 0)
    }

    /// A timer that fires every `interval_ns` nanoseconds.
    ///
    /// # Errors
    ///
    /// As [`one_shot`](Self::one_shot).
    pub fn periodic(interval_ns: u64) -> Result<Self> {
        Self::create(interval_ns, 1)
    }

    fn create(ns: u64, flags: u64) -> Result<Self> {
        // SAFETY: SYS_TIMER_CREATE takes a duration and flags, no pointers.
        let ret = unsafe { syscall2(syscall_nr::SYS_TIMER_CREATE, ns, flags) };
        // The kernel answers a full table with handle 0, not an error code.
        match check(ret)? {
            0 => Err(BusError::ResourceExhausted),
            handle => Ok(Self { handle }),
        }
    }

    /// The timer's kernel handle.
    #[must_use]
    pub fn handle(&self) -> u64 {
        self.handle
    }

    /// Have `evloop` report `user_data` when this timer fires.
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    pub fn register(&self, evloop: &EventLoop, user_data: u64) -> Result<()> {
        evloop.register_source(SourceType::Timer, self.handle, user_data)
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: SYS_TIMER_CANCEL takes a handle and no pointers.
        let ret = unsafe { syscall1(syscall_nr::SYS_TIMER_CANCEL, self.handle) };
        // A timer that already fired and was reaped is already cancelled.
        let _ = ret;
    }
}

// ============================================================================
// Payload fields -- the argument encoding services share
// ============================================================================

/// A length-prefixed list of byte strings, for message payloads.
///
/// [`Message`] carries its payload as opaque bytes, which is the right choice
/// for the transport -- a compositor's pixel buffer and a session manager's
/// method arguments have nothing in common, and forcing one type system on
/// both would serve neither. But it leaves *argument lists* unspecified, and
/// an unspecified thing that every service needs is a thing every service
/// invents separately. That is the shape that produced three disagreeing
/// password hashers (`design-decisions.md` §329) and five disagreeing YAML
/// parsers (§330). So the one encoding lives here, once.
///
/// Wire format, all integers little-endian:
///
/// ```text
/// [0..4]   u32   field count
/// then, per field:
///   [0..4] u32   byte length
///   [4..]  bytes
/// ```
///
/// Fields are **bytes, not text**: a username or a path may legally be
/// non-UTF-8, and a codec that decodes to `String` would corrupt it (see the
/// project rule on OS-boundary data). Callers that want text validate it
/// themselves, at the point where they know whether invalid UTF-8 is an error
/// or just an unusual name.
pub mod fields {
    /// The largest field count this decoder will believe.
    ///
    /// The count is read from untrusted bytes and used to size a `Vec`, so
    /// without a cap a four-byte header could ask for four billion entries.
    /// No interface here has anything like this many arguments; the limit
    /// exists to bound the allocation, not to express a design.
    pub const MAX_FIELDS: usize = 64;

    /// Encode a list of byte strings into a payload.
    #[must_use]
    pub fn encode(items: &[&[u8]]) -> Vec<u8> {
        let total = items
            .iter()
            .fold(4usize, |n, f| n.saturating_add(4).saturating_add(f.len()));
        let mut out = Vec::with_capacity(total);
        // `as u32` is safe against a caller who really does pass more than
        // 4 G fields, or one field over 4 GiB, only because `decode` refuses
        // anything over MAX_FIELDS and any length past the end of the payload:
        // the truncated count or length simply fails to decode. Neither fits
        // in a channel message anyway (`crate::MAX_MESSAGE_SIZE`).
        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
        for item in items {
            out.extend_from_slice(&(item.len() as u32).to_le_bytes());
            out.extend_from_slice(item);
        }
        out
    }

    /// Decode a payload into its byte strings.
    ///
    /// Returns `None` if the payload is truncated, declares more fields than
    /// [`MAX_FIELDS`], declares a field longer than the bytes that remain, or
    /// has bytes left over after the last field. Every one of those is a
    /// malformed message rather than a distinguishable error, and a service's
    /// only sane response to any of them is the same `InvalidArguments` reply
    /// -- so they collapse into one `None`.
    #[must_use]
    pub fn decode(payload: &[u8]) -> Option<Vec<&[u8]>> {
        let (count, mut rest) = payload.split_first_chunk::<4>()?;
        let count = usize::try_from(u32::from_le_bytes(*count)).ok()?;
        if count > MAX_FIELDS {
            return None;
        }

        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let (len, after) = rest.split_first_chunk::<4>()?;
            let len = usize::try_from(u32::from_le_bytes(*len)).ok()?;
            let (field, after) = after.split_at_checked(len)?;
            out.push(field);
            rest = after;
        }
        rest.is_empty().then_some(out)
    }

    /// Decode a payload and require exactly `n` fields.
    ///
    /// The count check belongs with the decode rather than in each method
    /// handler: an interface with a fixed arity that accepts a message of the
    /// wrong arity is an interface that will one day read argument 2 as
    /// argument 1.
    #[must_use]
    pub fn decode_exact(payload: &[u8], n: usize) -> Option<Vec<&[u8]>> {
        let items = decode(payload)?;
        if items.len() == n { Some(items) } else { None }
    }
}

// ============================================================================
// Duration helpers
// ============================================================================

/// Milliseconds as nanoseconds, saturating at `u64::MAX` (some 584 years).
#[must_use]
pub const fn ms_to_ns(ms: u64) -> u64 {
    ms.saturating_mul(1_000_000)
}

/// Seconds as nanoseconds, saturating at `u64::MAX`.
#[must_use]
pub const fn secs_to_ns(s: u64) -> u64 {
    s.saturating_mul(1_000_000_000)
}

#[cfg(test)]
mod tests;
