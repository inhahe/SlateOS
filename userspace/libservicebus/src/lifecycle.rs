//! The lifecycle endpoint: how a program is asked to close or to stop, and
//! how it hears the asking (design-decisions §1072, the operator's answer to
//! B-Q22; the kernel half is lane A's, `kernel/src/proc/lifecycle.rs`).
//!
//! A program that can answer registers once ([`Lifecycle::register`]) and is
//! brought every request made of it, one message each, on a channel of its
//! own. Anyone who may signal the program may ask ([`ask`]) -- the rule
//! `kill` has -- and may first find out whether it answers at all
//! ([`answers`]). There are two requests:
//!
//! | [`Kind`] | what the program does |
//! |---|---|
//! | [`Kind::Close`] | what its own close command does: it may ask about unsaved work, and may decline |
//! | [`Kind::Terminate`] | stops now, cleanly -- flush, release, exit -- asking nothing |
//!
//! Forcing is not a request: it is the kill a signal already is.
//!
//! Nothing is sent back; the asker learns the outcome from the exit. A
//! program that never registered -- every program ported from Linux -- is
//! asked with signals instead, and [`ask`] says so with
//! [`BusError::Unsupported`]. So does every call here off SlateOS, where
//! the syscalls are the host stub.

use super::Endpoint as _;
use super::{BusError, KernelChannel, Result, Wait, check, syscall0, syscall1, syscall2};

/// The kernel's numbers for the three calls (`kernel/src/syscall/number.rs`,
/// compared by the tests).
pub(crate) mod nr {
    /// Register: the handle of the channel end requests arrive on.
    pub const SYS_LIFECYCLE_REGISTER: u64 = 1172;
    /// Ask `pid` to close or to terminate: the request's id.
    pub const SYS_LIFECYCLE_REQUEST: u64 = 1173;
    /// Whether `pid` answers requests: 1 or 0.
    pub const SYS_LIFECYCLE_QUERY: u64 = 1174;
}

/// The size of one request on the channel: kind, flags, id, the asker's
/// pid and real uid, little-endian.
pub const REQUEST_SIZE: usize = 24;

/// What is asked of a program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Close, as the user would: the program may ask first, and may decline.
    Close,
    /// Stop now, cleanly, asking nothing.
    Terminate,
}

impl Kind {
    /// The kernel's number for it: `LIFECYCLE_CLOSE` 1, `LIFECYCLE_TERMINATE`
    /// 2.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::Close => 1,
            Self::Terminate => 2,
        }
    }

    /// The kind a request's number names, if it names one.
    #[must_use]
    pub const fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Close),
            2 => Some(Self::Terminate),
            _ => None,
        }
    }
}

/// One request, as the kernel delivered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// What is asked.
    pub kind: Kind,
    /// The kernel's id for it: never 0, never reused.
    pub id: u64,
    /// Who asked.
    pub asker_pid: u32,
    /// The asker's real user.
    pub asker_uid: u32,
}

impl Request {
    /// A request read from its 24 bytes -- `None` for anything that is not
    /// one: another length, a kind this library does not know, or flags it
    /// was not told the meaning of (the kernel sends 0).
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != REQUEST_SIZE {
            return None;
        }
        let u32_at = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(
                bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
            ))
        };
        if u32_at(4)? != 0 {
            return None;
        }
        Some(Self {
            kind: Kind::from_code(u32_at(0)?)?,
            id: u64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?),
            asker_pid: u32_at(16)?,
            asker_uid: u32_at(20)?,
        })
    }

    /// The 24 bytes the kernel sends for this request.
    #[must_use]
    pub fn encode(&self) -> [u8; REQUEST_SIZE] {
        let mut out = [0u8; REQUEST_SIZE];
        let parts: [(&mut [u8], &[u8]); 4] = {
            let (kind, rest) = out.split_at_mut(4);
            let (_flags, rest) = rest.split_at_mut(4);
            let (id, rest) = rest.split_at_mut(8);
            let (pid, uid) = rest.split_at_mut(4);
            [
                (kind, &self.kind.code().to_le_bytes()[..]),
                (id, &self.id.to_le_bytes()[..]),
                (pid, &self.asker_pid.to_le_bytes()[..]),
                (uid, &self.asker_uid.to_le_bytes()[..]),
            ]
        };
        for (dst, src) in parts {
            dst.copy_from_slice(src);
        }
        out
    }
}

/// This program's lifecycle endpoint: the requests made of it arrive here.
/// Dropping it closes the channel, after which the program is asked with
/// signals again.
pub struct Lifecycle {
    channel: KernelChannel,
}

impl Lifecycle {
    /// `SYS_LIFECYCLE_REGISTER`: from now on, requests made of this program
    /// arrive on the endpoint returned.
    ///
    /// # Errors
    ///
    /// [`BusError::AlreadyExists`] while this program has one open;
    /// [`BusError::Unsupported`] off SlateOS.
    pub fn register() -> Result<Self> {
        // SAFETY: SYS_LIFECYCLE_REGISTER takes no arguments and touches no
        // memory of ours.
        let ret = unsafe { syscall0(nr::SYS_LIFECYCLE_REGISTER) };
        Ok(Self {
            channel: KernelChannel {
                handle: check(ret)?,
            },
        })
    }

    /// The channel's handle, to wait on beside the program's other sources
    /// (`SYS_WAIT_MULTIPLE` for `POLLIN`, or an [`super::EventLoop`]).
    #[must_use]
    pub fn handle(&self) -> u64 {
        self.channel.handle
    }

    /// The next request, waiting for one.
    ///
    /// # Errors
    ///
    /// [`BusError::Malformed`] for a message that is not a request (none
    /// should arrive: the kernel alone writes to this channel), else the
    /// channel's own errors.
    pub fn recv(&mut self) -> Result<Request> {
        self.receive(Wait::Forever)
    }

    /// The next request if one is waiting, without waiting.
    ///
    /// # Errors
    ///
    /// As [`Lifecycle::recv`].
    pub fn try_recv(&mut self) -> Result<Option<Request>> {
        match self.receive(Wait::Never) {
            Ok(r) => Ok(Some(r)),
            Err(BusError::WouldBlock) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn receive(&mut self, wait: Wait) -> Result<Request> {
        // Room for more than a request, so that a longer message is seen to
        // be one rather than read as its first 24 bytes.
        let mut buf = [0u8; REQUEST_SIZE * 2];
        let len = self.channel.recv(&mut buf, wait)?;
        buf.get(..len)
            .and_then(Request::decode)
            .ok_or(BusError::Malformed)
    }
}

/// `SYS_LIFECYCLE_REQUEST`: ask program `pid` to close or to terminate. The
/// request's id.
///
/// # Errors
///
/// [`BusError::Unsupported`] when the program does not answer requests --
/// it never registered, has `exec`'d since, or closed its endpoint -- and
/// off SlateOS: ask it with a signal instead. [`BusError::NoSuchProcess`]
/// when there is no such process, or it has ended;
/// [`BusError::PermissionDenied`] when this program may not signal it;
/// [`BusError::QueueFull`] when it has as many requests unread as a channel
/// holds.
pub fn ask(pid: u32, kind: Kind) -> Result<u64> {
    // SAFETY: SYS_LIFECYCLE_REQUEST takes a pid and a kind, no pointers.
    let ret = unsafe {
        syscall2(
            nr::SYS_LIFECYCLE_REQUEST,
            u64::from(pid),
            u64::from(kind.code()),
        )
    };
    check(ret)
}

/// `SYS_LIFECYCLE_QUERY`: whether program `pid` answers requests.
///
/// # Errors
///
/// As [`ask`], except that a program that does not answer is `Ok(false)`;
/// [`BusError::Unsupported`] off SlateOS, where nothing does.
pub fn answers(pid: u32) -> Result<bool> {
    // SAFETY: SYS_LIFECYCLE_QUERY takes a pid, no pointers.
    let ret = unsafe { syscall1(nr::SYS_LIFECYCLE_QUERY, u64::from(pid)) };
    Ok(check(ret)? != 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_read_as_the_kernel_lays_it_out() {
        let mut b = [0u8; REQUEST_SIZE];
        b[0] = 2; // terminate
        b[8..16].copy_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        b[16..20].copy_from_slice(&4242u32.to_le_bytes());
        b[20..24].copy_from_slice(&1000u32.to_le_bytes());
        let r = Request::decode(&b).unwrap();
        assert_eq!(r.kind, Kind::Terminate);
        assert_eq!(r.id, 0x0102_0304_0506_0708);
        assert_eq!(r.asker_pid, 4242);
        assert_eq!(r.asker_uid, 1000);
        assert_eq!(r.encode(), b);
    }

    #[test]
    fn anything_else_is_not_a_request() {
        let r = Request {
            kind: Kind::Close,
            id: 7,
            asker_pid: 1,
            asker_uid: 0,
        };
        let good = r.encode();
        assert_eq!(Request::decode(&good), Some(r));
        // Another length.
        assert_eq!(Request::decode(&good[..23]), None);
        let mut long = good.to_vec();
        long.push(0);
        assert_eq!(Request::decode(&long), None);
        // A kind nobody sends, and flags nobody defined.
        let mut odd = good;
        odd[0] = 3;
        assert_eq!(Request::decode(&odd), None);
        let mut flagged = good;
        flagged[4] = 1;
        assert_eq!(Request::decode(&flagged), None);
    }

    #[test]
    fn the_kinds_are_the_kernel_s_numbers() {
        assert_eq!(Kind::Close.code(), 1);
        assert_eq!(Kind::Terminate.code(), 2);
        assert_eq!(Kind::from_code(1), Some(Kind::Close));
        assert_eq!(Kind::from_code(2), Some(Kind::Terminate));
        assert_eq!(Kind::from_code(0), None);
    }

    /// Off SlateOS nothing answers, and asking says so in the words a caller
    /// falls back to signals on.
    #[cfg(not(target_vendor = "slateos"))]
    #[test]
    fn off_slateos_nothing_answers() {
        assert_eq!(answers(1).err(), Some(BusError::Unsupported));
        assert_eq!(ask(1, Kind::Terminate).err(), Some(BusError::Unsupported));
        assert!(matches!(Lifecycle::register(), Err(BusError::Unsupported)));
    }
}
