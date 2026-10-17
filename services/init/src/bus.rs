//! The service bus as init speaks it, for `org.slateos.ServiceManager`.
//!
//! `libservicebus` is the bus's library, and it needs `std`; init has none,
//! and no heap. So this is that library's wire format again -- the message
//! (version 2) and the `fields` argument encoding -- read from and written
//! into buffers init owns, and what each request asks of init. The tests
//! hold both encodings to `libservicebus`'s: the fields byte for byte against
//! its own encoder, the message against the layout its documentation gives.
//!
//! The interface is the one `userspace/powerctl` and `userspace/service`
//! already call (lane B's
//! `requests/b-d-init-could-answer-org-slateos-servicemanager.md`): each
//! argument a field, an empty return for success -- or the service command's
//! text as its one field -- and an error named `org.slateos.Error.*`
//! otherwise.

/// The name init answers on the bus.
pub const SERVICE_MANAGER: &[u8] = b"org.slateos.ServiceManager";

/// The fixed header's size.
pub const HEADER_SIZE: usize = 24;

/// The largest message, as the bus's channels carry them.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// What a message is: its header's first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// A method call, which expects a reply.
    MethodCall = 1,
    /// A successful reply.
    MethodReturn = 2,
    /// A failed reply; its member names the error.
    Error = 3,
    /// A one-way notification.
    Signal = 4,
}

/// A received message, borrowed from the buffer it arrived in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    /// What it is.
    pub msg_type: MessageType,
    /// The method's name; on an error, the error's (UTF-8).
    pub member: &'a [u8],
    /// The arguments: [`decode_fields`]' input.
    pub payload: &'a [u8],
    /// The sender's number for it, never 0.
    pub serial: u64,
    /// On a reply, the call it answers; 0 otherwise.
    pub reply_serial: u64,
}

/// Read one received message, as strictly as `libservicebus` does: a length
/// that disagrees with the bytes, a member that is not UTF-8, an unknown
/// type, a serial of 0 or a reply serial on the wrong kind of message is
/// `None`.
///
/// ```text
/// [0]       u8   type     [1] u8 flags (carried, not read)
/// [2..4]    u16  member length      [4..8] u32 payload length
/// [8..16]   u64  serial             [16..24] u64 reply serial
/// [24..]         member, then payload, and nothing after
/// ```
#[must_use]
pub fn decode(data: &[u8]) -> Option<Message<'_>> {
    let (header, rest) = data.split_first_chunk::<HEADER_SIZE>()?;
    let [ty, _flags, m0, m1, p0, p1, p2, p3, s @ ..] = *header;
    let serial = u64::from_le_bytes(s.get(..8)?.try_into().ok()?);
    let reply_serial = u64::from_le_bytes(s.get(8..16)?.try_into().ok()?);
    let member_len = usize::from(u16::from_le_bytes([m0, m1]));
    let payload_len = usize::try_from(u32::from_le_bytes([p0, p1, p2, p3])).ok()?;
    if rest.len() != member_len.checked_add(payload_len)? {
        return None;
    }
    let (member, payload) = rest.split_at_checked(member_len)?;
    core::str::from_utf8(member).ok()?;
    let msg_type = match ty {
        1 => MessageType::MethodCall,
        2 => MessageType::MethodReturn,
        3 => MessageType::Error,
        4 => MessageType::Signal,
        _ => return None,
    };
    let is_reply = matches!(msg_type, MessageType::MethodReturn | MessageType::Error);
    if serial == 0 || is_reply == (reply_serial == 0) {
        return None;
    }
    Some(Message {
        msg_type,
        member,
        payload,
        serial,
        reply_serial,
    })
}

/// Write a reply to the call numbered `call_serial` into `out`: a return
/// when `error` is `None`, otherwise an error of that name; `serial` its
/// own number; its payload `fields`, encoded. Its length, or `None` when it
/// does not fit `out` or [`MAX_MESSAGE_SIZE`].
#[must_use]
pub fn encode_reply(
    out: &mut [u8],
    serial: u64,
    call_serial: u64,
    error: Option<&[u8]>,
    fields: &[&[u8]],
) -> Option<usize> {
    let (msg_type, member) = match error {
        None => (MessageType::MethodReturn, &[][..]),
        Some(name) => (MessageType::Error, name),
    };
    if serial == 0 || call_serial == 0 {
        return None;
    }
    let member_len = u16::try_from(member.len()).ok()?;
    let payload_len = fields_len(fields)?;
    let total = HEADER_SIZE
        .checked_add(member.len())?
        .checked_add(payload_len)?;
    if total > MAX_MESSAGE_SIZE || total > out.len() {
        return None;
    }
    let mut w = Writer { out, at: 0 };
    w.put(&[msg_type as u8, 0]);
    w.put(&member_len.to_le_bytes());
    w.put(&u32::try_from(payload_len).ok()?.to_le_bytes());
    w.put(&serial.to_le_bytes());
    w.put(&call_serial.to_le_bytes());
    w.put(member);
    encode_fields_into(&mut w, fields)?;
    Some(w.at)
}

/// The size of `fields` encoded, or `None` past what a field list can say.
fn fields_len(fields: &[&[u8]]) -> Option<usize> {
    u32::try_from(fields.len()).ok()?;
    fields.iter().try_fold(4usize, |n, f| {
        u32::try_from(f.len()).ok()?;
        n.checked_add(4)?.checked_add(f.len())
    })
}

/// A cursor over a buffer [`encode_reply`] has checked is long enough.
struct Writer<'a> {
    out: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) {
        let end = self.at.saturating_add(bytes.len());
        if let Some(dst) = self.out.get_mut(self.at..end) {
            dst.copy_from_slice(bytes);
        }
        self.at = end;
    }
}

/// `libservicebus::fields::encode`, into `w`.
fn encode_fields_into(w: &mut Writer<'_>, fields: &[&[u8]]) -> Option<()> {
    w.put(&u32::try_from(fields.len()).ok()?.to_le_bytes());
    for f in fields {
        w.put(&u32::try_from(f.len()).ok()?.to_le_bytes());
        w.put(f);
    }
    Some(())
}

/// The most arguments any request here takes, and so the most this reads.
pub const MAX_ARGS: usize = 2;

/// `libservicebus::fields::decode_exact(payload, n)`, into `out`: the `n`
/// fields, or `None` for a payload that is truncated, has bytes left over,
/// or holds another number of fields.
#[must_use]
pub fn decode_fields<'a>(
    payload: &'a [u8],
    n: usize,
    out: &mut [&'a [u8]; MAX_ARGS],
) -> Option<()> {
    let (count, mut rest) = payload.split_first_chunk::<4>()?;
    if usize::try_from(u32::from_le_bytes(*count)).ok()? != n || n > MAX_ARGS {
        return None;
    }
    for slot in out.iter_mut().take(n) {
        let (len, after) = rest.split_first_chunk::<4>()?;
        let len = usize::try_from(u32::from_le_bytes(*len)).ok()?;
        let (field, after) = after.split_at_checked(len)?;
        *slot = field;
        rest = after;
    }
    rest.is_empty().then_some(())
}

/// The errors init answers with, as the clients name them.
pub mod error {
    /// A method this interface does not have.
    pub const UNKNOWN_METHOD: &[u8] = b"org.slateos.Error.UnknownMethod";
    /// Arguments of the wrong number or form.
    pub const INVALID_ARGUMENTS: &[u8] = b"org.slateos.Error.InvalidArguments";
    /// A caller who is not root.
    pub const DENIED: &[u8] = b"org.slateos.Error.Denied";
    /// A service name `/etc/startup.conf` does not give.
    pub const NO_SUCH_SERVICE: &[u8] = b"org.slateos.Error.NoSuchService";
    /// Something the machine cannot do yet: suspend, hibernate.
    pub const NOT_SUPPORTED: &[u8] = b"org.slateos.Error.NotSupported";
    /// It was tried, and failed; the explanation says how.
    pub const FAILED: &[u8] = b"org.slateos.Error.Failed";
}

/// What a request does to one service, named as in `/etc/startup.conf`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceOp {
    /// Start it.
    Start,
    /// Stop it: terminate, then force after its stop timeout.
    Stop,
    /// Stop it, then start it.
    Restart,
    /// Ask it to read its configuration again (`SIGHUP`).
    Reload,
    /// Start it at boot.
    Enable,
    /// Do not start it at boot.
    Disable,
}

/// What switching the machine off or on again a request asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    /// Stop every service, then switch off.
    Off,
    /// Stop every service, then restart.
    Reboot,
}

/// A request to `org.slateos.ServiceManager`, read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    /// `PowerOff` or `Reboot`.
    Power(Power),
    /// `Suspend` or `Hibernate`: nothing here can, yet.
    Sleep,
    /// `SchedulePower(minutes, "shutdown" | "reboot")`.
    Schedule {
        /// How long from now.
        minutes: u32,
        /// What then.
        power: Power,
    },
    /// `CancelScheduledPower`.
    CancelSchedule,
    /// `StartService(name)` and the rest.
    Service(ServiceOp, &'a [u8]),
}

/// The longest a `SchedulePower` may be: a year of minutes.
pub const MAX_SCHEDULE_MINUTES: u32 = 366 * 24 * 60;

/// What the call `member` with `payload` asks, or the error to answer it
/// with: [`error::UNKNOWN_METHOD`] for a method this interface does not have,
/// [`error::INVALID_ARGUMENTS`] for arguments of the wrong number or form.
pub fn read_request<'a>(member: &[u8], payload: &'a [u8]) -> Result<Request<'a>, &'static [u8]> {
    let mut args: [&[u8]; MAX_ARGS] = [&[]; MAX_ARGS];
    let mut want = |n: usize| decode_fields(payload, n, &mut args).ok_or(error::INVALID_ARGUMENTS);
    let service = |op: ServiceOp, args: [&'a [u8]; MAX_ARGS]| {
        let name = args[0];
        if name.is_empty() || name.contains(&b'/') {
            Err(error::INVALID_ARGUMENTS)
        } else {
            Ok(Request::Service(op, name))
        }
    };
    match member {
        b"PowerOff" => want(0).map(|()| Request::Power(Power::Off)),
        b"Reboot" => want(0).map(|()| Request::Power(Power::Reboot)),
        b"Suspend" | b"Hibernate" => want(0).map(|()| Request::Sleep),
        b"CancelScheduledPower" => want(0).map(|()| Request::CancelSchedule),
        b"SchedulePower" => {
            want(2)?;
            let minutes = decimal(args[0])
                .filter(|m| (1..=MAX_SCHEDULE_MINUTES).contains(m))
                .ok_or(error::INVALID_ARGUMENTS)?;
            let power = match args[1] {
                b"shutdown" => Power::Off,
                b"reboot" => Power::Reboot,
                _ => return Err(error::INVALID_ARGUMENTS),
            };
            Ok(Request::Schedule { minutes, power })
        }
        _ => {
            let op = match member {
                b"StartService" => ServiceOp::Start,
                b"StopService" => ServiceOp::Stop,
                b"RestartService" => ServiceOp::Restart,
                b"ReloadService" => ServiceOp::Reload,
                b"EnableService" => ServiceOp::Enable,
                b"DisableService" => ServiceOp::Disable,
                _ => return Err(error::UNKNOWN_METHOD),
            };
            want(1)?;
            service(op, args)
        }
    }
}

/// A decimal number of at most nine digits, or `None`.
fn decimal(text: &[u8]) -> Option<u32> {
    if text.is_empty() || text.len() > 9 || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    text.iter().try_fold(0u32, |n, &d| {
        n.checked_mul(10)?.checked_add(u32::from(d & 0x0F))
    })
}

#[cfg(test)]
mod tests;
