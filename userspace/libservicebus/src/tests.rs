//! Tests for the service bus client.
//!
//! The kernel calls themselves are the `ENOSYS` stub on a development host,
//! so the protocol above them is driven through a scripted [`Endpoint`]: the
//! test says what the channel will deliver and reads back what was sent. The
//! version-1 library had no such seam and no round-trip test, and shipped a
//! `call` that could never return.
//!
//! The constants this crate shares with the kernel -- syscall numbers, the
//! message size limit, the event layout, the source-type encoding -- are read
//! out of the kernel's own source and compared, as `kerror`'s tests do for
//! error codes: a renumbering on that side fails here instead of sending a
//! message to the wrong syscall.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use std::sync::{Arc, Mutex};

// ----------------------------------------------------------------------------
// A scripted channel
// ----------------------------------------------------------------------------

/// What one receive on the scripted channel yields.
enum Incoming {
    /// A message, as the kernel would hand over its bytes.
    Bytes(Vec<u8>),
    /// A receive that fails.
    Fails(BusError),
}

/// The channel's script, and what it was sent.
#[derive(Default)]
struct Script {
    incoming: VecDeque<Incoming>,
    sent: Vec<Vec<u8>>,
    peer: Option<Credentials>,
    /// Refuse non-blocking sends, as a full queue does.
    full: bool,
}

struct Scripted(Arc<Mutex<Script>>);

impl Endpoint for Scripted {
    fn raw_handle(&self) -> u64 {
        77
    }

    fn send(&mut self, data: &[u8], blocking: bool) -> Result<()> {
        let mut s = self.0.lock().unwrap();
        if s.full && !blocking {
            return Err(BusError::QueueFull);
        }
        s.sent.push(data.to_vec());
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8], wait: Wait) -> Result<usize> {
        let mut s = self.0.lock().unwrap();
        match s.incoming.pop_front() {
            // As the kernel does: copy what fits, report the whole length.
            Some(Incoming::Bytes(b)) => {
                let n = b.len().min(buf.len());
                buf[..n].copy_from_slice(&b[..n]);
                Ok(b.len())
            }
            Some(Incoming::Fails(e)) => Err(e),
            // An exhausted script under `Forever` would hang a real channel;
            // here it ends the test's conversation as a closed peer would.
            None => Err(match wait {
                Wait::Never => BusError::WouldBlock,
                Wait::Nanos(_) => BusError::TimedOut,
                Wait::Forever => BusError::Disconnected,
            }),
        }
    }

    fn peer_credentials(&self) -> Option<Credentials> {
        self.0.lock().unwrap().peer
    }
}

fn scripted() -> (Connection, Arc<Mutex<Script>>) {
    let script = Arc::new(Mutex::new(Script::default()));
    let conn = Connection::from_endpoint(Box::new(Scripted(Arc::clone(&script))));
    (conn, script)
}

fn deliver(script: &Arc<Mutex<Script>>, msg: &Message, serial: u64) {
    let bytes = msg.encode(serial).unwrap();
    script
        .lock()
        .unwrap()
        .incoming
        .push_back(Incoming::Bytes(bytes));
}

/// A reply to the call that went out under `serial`, with `payload`.
fn reply_to(serial: u64, payload: &[u8]) -> Message {
    let call = Message {
        serial,
        ..Message::method_call("Anything")
    };
    Message::reply(&call).with_payload(payload)
}

fn sent(script: &Arc<Mutex<Script>>) -> Vec<Message> {
    script
        .lock()
        .unwrap()
        .sent
        .iter()
        .map(|b| Message::decode(b).unwrap())
        .collect()
}

// ----------------------------------------------------------------------------
// The wire format
// ----------------------------------------------------------------------------

#[test]
fn a_reply_carries_the_serial_of_the_call_it_answers() {
    // The version-1 bug: `reply_serial` was set in memory and never written.
    let call = Message {
        serial: 7,
        ..Message::method_call("GetSeat")
    };
    let reply = Message::reply(&call).with_payload(b"seat0");
    let decoded = Message::decode(&reply.encode(3).unwrap()).unwrap();
    assert_eq!(decoded.reply_serial, 7);
    assert_eq!(decoded.serial, 3);
    assert_eq!(decoded.msg_type, MessageType::MethodReturn);
    assert_eq!(decoded.payload, b"seat0");

    let error = Message::error(&call, "system.logind.Error.NoSuchSeat");
    let decoded = Message::decode(&error.encode(4).unwrap()).unwrap();
    assert_eq!(decoded.reply_serial, 7);
    assert!(decoded.is_error());
    assert_eq!(decoded.member, "system.logind.Error.NoSuchSeat");
}

#[test]
fn every_kind_of_message_round_trips() {
    let call = Message::method_call("Upload").with_payload(&(0..=255).collect::<Vec<u8>>());
    let signal = Message::signal("Changed");
    let mut flagged = Message::signal("Flagged");
    flagged.flags = 0x80;
    for (msg, serial) in [(call, 1), (signal, 2), (flagged, u64::MAX)] {
        let decoded = Message::decode(&msg.encode(serial).unwrap()).unwrap();
        assert_eq!(
            decoded,
            Message {
                serial,
                ..msg.clone()
            }
        );
    }
}

#[test]
fn the_header_is_laid_out_as_documented() {
    let call = Message {
        serial: 0x0102,
        ..Message::method_call("x")
    };
    let error = Message::error(&call, "E").with_payload(b"yz");
    let bytes = error.encode(0x1122_3344_5566_7788).unwrap();
    assert_eq!(bytes.len(), HEADER_SIZE + 3);
    assert_eq!(bytes[0], 3, "type");
    assert_eq!(bytes[1], 0, "flags");
    assert_eq!(&bytes[2..4], &1u16.to_le_bytes(), "member length");
    assert_eq!(&bytes[4..8], &2u32.to_le_bytes(), "payload length");
    assert_eq!(
        &bytes[8..16],
        &0x1122_3344_5566_7788u64.to_le_bytes(),
        "serial"
    );
    assert_eq!(&bytes[16..24], &0x0102u64.to_le_bytes(), "reply serial");
    assert_eq!(&bytes[24..], b"Eyz");
}

#[test]
fn encode_refuses_rather_than_truncates() {
    // A member whose length does not fit the header's u16.
    let long_member = "m".repeat(usize::from(u16::MAX) + 1);
    assert_eq!(
        Message::method_call(&long_member).encode(1),
        Err(BusError::MessageTooLarge)
    );
    // In practice the channel's limit binds first: the longest member that
    // goes is the whole message less its header, not u16::MAX.
    let longest = "m".repeat(MAX_MESSAGE_SIZE - HEADER_SIZE);
    assert_eq!(
        Message::method_call(&longest).encode(1).unwrap().len(),
        MAX_MESSAGE_SIZE
    );
    assert_eq!(
        Message::method_call(&format!("{longest}m")).encode(1),
        Err(BusError::MessageTooLarge)
    );

    // Exactly the channel's limit is a message; one byte more is not.
    let fits = MAX_MESSAGE_SIZE - HEADER_SIZE - 1;
    let at_limit = Message::method_call("m").with_payload(&vec![0; fits]);
    assert_eq!(at_limit.encode(1).unwrap().len(), MAX_MESSAGE_SIZE);
    let over = Message::method_call("m").with_payload(&vec![0; fits + 1]);
    assert_eq!(over.encode(1), Err(BusError::MessageTooLarge));
}

#[test]
fn encode_refuses_a_reply_that_answers_nothing_and_a_call_that_answers_something() {
    // A call never sent has serial 0, so a reply built from it answers
    // nothing; the peer would reject it as malformed.
    let unsent = Message::method_call("Foo");
    assert_eq!(
        Message::reply(&unsent).encode(1),
        Err(BusError::InvalidArgument)
    );
    let mut signal = Message::signal("Changed");
    signal.reply_serial = 5;
    assert_eq!(signal.encode(1), Err(BusError::InvalidArgument));
}

#[test]
fn decode_refuses_what_a_peer_speaking_this_protocol_cannot_have_sent() {
    let good = Message::method_call("Ping").with_payload(b"abc");
    let bytes = good.encode(9).unwrap();
    assert!(Message::decode(&bytes).is_ok());

    // Every truncation, header or body.
    for cut in 0..bytes.len() {
        assert_eq!(
            Message::decode(&bytes[..cut]),
            Err(BusError::Malformed),
            "cut at {cut}"
        );
    }
    // Bytes after the payload: the lengths no longer describe the message.
    let mut long = bytes.clone();
    long.push(0);
    assert_eq!(Message::decode(&long), Err(BusError::Malformed));

    let patched = |at: usize, value: &[u8]| {
        let mut b = bytes.clone();
        b[at..at + value.len()].copy_from_slice(value);
        Message::decode(&b)
    };
    assert_eq!(patched(0, &[0]), Err(BusError::Malformed), "type 0");
    assert_eq!(patched(0, &[5]), Err(BusError::Malformed), "type 5");
    assert_eq!(
        patched(8, &0u64.to_le_bytes()),
        Err(BusError::Malformed),
        "serial 0"
    );
    assert_eq!(
        patched(16, &1u64.to_le_bytes()),
        Err(BusError::Malformed),
        "a call that claims to answer serial 1"
    );
    assert_eq!(
        patched(0, &[2]),
        Err(BusError::Malformed),
        "a return that answers serial 0"
    );
    assert_eq!(
        patched(24, &[0xff]),
        Err(BusError::Malformed),
        "a member that is not UTF-8"
    );
}

#[test]
fn a_version_1_message_is_refused_not_misread() {
    // v1: type, flags, member_len, payload_len, serial -- 16 bytes, no reply
    // serial. Read as v2, its member would start eight bytes early.
    let mut v1 = vec![1u8, 0];
    v1.extend_from_slice(&4u16.to_le_bytes());
    v1.extend_from_slice(&0u32.to_le_bytes());
    v1.extend_from_slice(&1u64.to_le_bytes());
    v1.extend_from_slice(b"Ping");
    assert_eq!(Message::decode(&v1), Err(BusError::Malformed));
}

// ----------------------------------------------------------------------------
// Connection
// ----------------------------------------------------------------------------

#[test]
fn call_returns_the_reply_to_its_own_serial() {
    let (mut conn, script) = scripted();
    deliver(&script, &reply_to(1, b"7"), 40);
    let reply = conn.call("CreateSession", b"args").unwrap();
    assert_eq!(reply.reply_serial, 1);
    assert_eq!(reply.payload, b"7");

    let out = sent(&script);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].msg_type, MessageType::MethodCall);
    assert_eq!(out[0].member, "CreateSession");
    assert_eq!(out[0].payload, b"args");
    assert_eq!(out[0].serial, 1);
}

#[test]
fn call_holds_everything_else_for_recv_in_arrival_order() {
    let (mut conn, script) = scripted();
    // Serials 1 and 2 go out as calls; the replies to 1 and 2 cross with a
    // signal and a reply to a call this connection never made.
    deliver(&script, &Message::signal("SessionNew"), 40);
    deliver(&script, &reply_to(99, b"stale"), 41);
    deliver(&script, &reply_to(1, b"first"), 42);
    deliver(&script, &reply_to(2, b"second"), 43);

    assert_eq!(conn.call("A", b"").unwrap().payload, b"first");
    assert_eq!(conn.call("B", b"").unwrap().payload, b"second");

    assert_eq!(conn.recv().unwrap().member, "SessionNew");
    assert_eq!(conn.recv().unwrap().payload, b"stale");
    assert_eq!(conn.try_recv().unwrap(), None);
}

#[test]
fn an_error_reply_is_the_reply() {
    let (mut conn, script) = scripted();
    let call = Message {
        serial: 1,
        ..Message::method_call("X")
    };
    deliver(&script, &Message::error(&call, "system.Error.Denied"), 5);
    let reply = conn.call("X", b"").unwrap();
    assert!(reply.is_error());
    assert_eq!(reply.member, "system.Error.Denied");
}

#[test]
fn call_fields_reads_a_return_and_a_refusal_as_answers() {
    let (mut conn, script) = scripted();
    deliver(&script, &reply_to(1, &fields::encode(&[b"7", b""])), 10);
    assert_eq!(
        conn.call_fields("CreateSession", &[b"alice"], secs_to_ns(5)),
        Ok(Outcome::Done(vec![b"7".to_vec(), Vec::new()]))
    );
    // The arguments went out as a field list.
    assert_eq!(
        fields::decode(&sent(&script)[0].payload),
        Some(vec![b"alice".as_slice()])
    );

    // A refusal built with `Message::error` carries no payload at all.
    let call = Message {
        serial: 2,
        ..Message::method_call("Nope")
    };
    deliver(&script, &Message::error(&call, "system.Error.Denied"), 11);
    assert_eq!(
        conn.call_fields("Nope", &[], secs_to_ns(5)),
        Ok(Outcome::Refused {
            error: "system.Error.Denied".to_string(),
            fields: Vec::new(),
        })
    );

    // An empty return is no fields too; a payload that is not a field list
    // is malformed, not an empty result.
    deliver(&script, &reply_to(3, b""), 12);
    assert_eq!(
        conn.call_fields("Quiet", &[], secs_to_ns(5)),
        Ok(Outcome::Done(Vec::new()))
    );
    deliver(&script, &reply_to(4, b"\x01"), 13);
    assert_eq!(
        conn.call_fields("Garbled", &[], secs_to_ns(5)),
        Err(BusError::Malformed)
    );
}

#[test]
fn call_timeout_waits_through_unrelated_messages() {
    // Version 1 returned TimedOut at the first message that was not the
    // reply, however much time was left.
    let (mut conn, script) = scripted();
    deliver(&script, &Message::signal("One"), 10);
    deliver(&script, &Message::signal("Two"), 11);
    deliver(&script, &reply_to(1, b"ok"), 12);
    let reply = conn.call_timeout("Slow", b"", secs_to_ns(60)).unwrap();
    assert_eq!(reply.payload, b"ok");
    assert_eq!(conn.recv().unwrap().member, "One");
    assert_eq!(conn.recv().unwrap().member, "Two");
}

#[test]
fn call_timeout_without_a_reply_times_out_and_the_call_was_still_sent() {
    let (mut conn, script) = scripted();
    assert_eq!(
        conn.call_timeout("Slow", b"", ms_to_ns(5)),
        Err(BusError::TimedOut)
    );
    // A zero budget is spent at once; the call has gone out regardless, and
    // its late reply would reach `recv` like any other message.
    assert_eq!(conn.call_timeout("Now", b"", 0), Err(BusError::TimedOut));
    let out = sent(&script);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].member, "Now");
    assert_eq!(out[1].serial, 2);
}

#[test]
fn a_peer_that_never_answers_cannot_grow_the_held_queue_without_bound() {
    let (mut conn, script) = scripted();
    for i in 0..MAX_PENDING + 5 {
        deliver(&script, &Message::signal(&format!("S{i}")), 100 + i as u64);
    }
    assert_eq!(conn.call("Never", b""), Err(BusError::QueueFull));
    assert_eq!(conn.pending.len(), MAX_PENDING);

    // A further call is refused before anything is sent: it could only fail
    // at its first unrelated message, after the service had acted on it.
    assert_eq!(conn.call("Refused", b""), Err(BusError::QueueFull));
    assert_eq!(sent(&script).len(), 1);

    // Nothing was lost: every held signal comes back, in order, then the
    // ones still in the channel.
    for i in 0..MAX_PENDING + 5 {
        assert_eq!(conn.recv().unwrap().member, format!("S{i}"));
    }
}

#[test]
fn serials_start_at_one_and_wrap_past_zero() {
    let (mut conn, script) = scripted();
    assert_eq!(conn.send(&Message::signal("a")).unwrap(), 1);
    conn.next_serial = u64::MAX;
    assert_eq!(conn.send(&Message::signal("b")).unwrap(), u64::MAX);
    assert_eq!(conn.send(&Message::signal("c")).unwrap(), 1);
    assert_eq!(sent(&script).len(), 3);
}

#[test]
fn a_send_that_fails_does_not_spend_a_serial() {
    let (mut conn, script) = scripted();
    script.lock().unwrap().full = true;
    assert_eq!(conn.send(&Message::signal("x")), Err(BusError::QueueFull));
    // `send_blocking` waits out a full queue rather than failing.
    assert_eq!(conn.send_blocking(&Message::signal("x")).unwrap(), 1);
}

#[test]
fn a_message_longer_than_any_channel_carries_is_refused_and_its_fragment_wiped() {
    let (mut conn, script) = scripted();
    script
        .lock()
        .unwrap()
        .incoming
        .push_back(Incoming::Bytes(vec![0xAB; MAX_MESSAGE_SIZE + 10]));
    assert_eq!(conn.recv(), Err(BusError::MessageTooLarge));
    assert!(conn.recv_buf.iter().all(|&b| b == 0));
}

#[test]
fn the_receive_buffer_keeps_no_copy_of_a_message_once_it_is_read() {
    // logind's AuthenticateSession carries a password.
    let (mut conn, script) = scripted();
    deliver(
        &script,
        &Message::method_call("AuthenticateSession").with_payload(b"hunter2"),
        3,
    );
    let call = conn.recv().unwrap();
    assert_eq!(call.payload, b"hunter2");
    assert!(conn.recv_buf.iter().all(|&b| b == 0));

    // Likewise a message that is refused as malformed.
    script
        .lock()
        .unwrap()
        .incoming
        .push_back(Incoming::Bytes(b"not a bus message, but a secret".to_vec()));
    assert_eq!(conn.recv(), Err(BusError::Malformed));
    assert!(conn.recv_buf.iter().all(|&b| b == 0));
}

#[test]
fn try_recv_and_recv_timeout_on_an_empty_channel() {
    let (mut conn, _script) = scripted();
    assert_eq!(conn.try_recv(), Ok(None));
    assert_eq!(conn.recv_timeout(0), Err(BusError::TimedOut));
    assert_eq!(conn.recv_timeout(ms_to_ns(1)), Err(BusError::TimedOut));
}

#[test]
fn a_receive_error_is_passed_through() {
    let (mut conn, script) = scripted();
    script
        .lock()
        .unwrap()
        .incoming
        .push_back(Incoming::Fails(BusError::Disconnected));
    assert_eq!(conn.recv(), Err(BusError::Disconnected));
}

#[test]
fn peer_credentials_are_the_kernel_s_answer_or_none() {
    let (conn, script) = scripted();
    assert_eq!(conn.peer_credentials(), None);
    let alice = Credentials {
        pid: 412,
        uid: 1000,
        gid: 100,
    };
    script.lock().unwrap().peer = Some(alice);
    assert_eq!(conn.peer_credentials(), Some(alice));
}

#[test]
fn the_kernel_s_credential_record_is_read_little_endian() {
    let mut record = [0u8; 16];
    record[0..4].copy_from_slice(&0x0001_0203u32.to_le_bytes());
    record[4..8].copy_from_slice(&1000u32.to_le_bytes());
    record[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    // The reserved word is not read: a kernel that starts filling it in (a
    // pid generation) must not turn a credential into "unknown".
    record[12..16].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        Credentials::from_record(record),
        Credentials {
            pid: 0x0001_0203,
            uid: 1000,
            gid: u32::MAX,
        }
    );
}

#[test]
fn on_a_development_host_no_peer_is_known() {
    // The real channel's answer comes from the syscall layer, which is the
    // ENOSYS stub here: unknown, never a fabricated uid 0 or uid 1000.
    #[cfg(not(target_vendor = "slateos"))]
    {
        let channel = KernelChannel { handle: 0 };
        assert_eq!(channel.peer_credentials(), None);
        // Its drop "closes" handle 0 through the stub, which is harmless.
    }
}

#[test]
fn root_is_recognised_by_uid_not_by_name() {
    let creds = |uid| Credentials {
        pid: 1,
        uid,
        gid: 0,
    };
    assert!(creds(0).is_root());
    assert!(!creds(1000).is_root());
}

#[test]
fn a_connection_can_be_handed_to_another_thread() {
    // logind serves each client on its own thread.
    fn assert_send<T: Send>() {}
    assert_send::<Connection>();
}

// ----------------------------------------------------------------------------
// Errors
// ----------------------------------------------------------------------------

fn kernel_code(name: &str) -> i64 {
    kerror::ALL
        .iter()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("{name} is not in kerror's table"))
        .code
}

#[test]
fn every_kernel_code_this_library_names_maps_to_its_variant() {
    let table = [
        ("NotFound", BusError::NotFound),
        ("AlreadyExists", BusError::AlreadyExists),
        ("ChannelClosed", BusError::Disconnected),
        ("TimedOut", BusError::TimedOut),
        ("WouldBlock", BusError::WouldBlock),
        ("ChannelFull", BusError::QueueFull),
        ("MessageTooLarge", BusError::MessageTooLarge),
        ("InvalidHandle", BusError::InvalidHandle),
        ("InvalidArgument", BusError::InvalidArgument),
        ("InvalidAddress", BusError::InvalidArgument),
        ("PermissionDenied", BusError::PermissionDenied),
        ("InvalidCapability", BusError::PermissionDenied),
        ("ResourceExhausted", BusError::ResourceExhausted),
        ("OutOfMemory", BusError::ResourceExhausted),
        ("NotSupported", BusError::Unsupported),
        ("NoSuchSyscall", BusError::Unsupported),
    ];
    for (name, expected) in table {
        assert_eq!(BusError::from_code(kernel_code(name)), expected, "{name}");
    }
    // What version 1 got wrong: it read -1 as "not found" (the kernel's -1
    // is InternalError) and -5 as "disconnected" (-5 is Cancelled).
    assert_eq!(BusError::from_code(-1), BusError::Unknown(-1));
    assert_eq!(BusError::from_code(-5), BusError::Unknown(-5));
    // The host stub.
    assert_eq!(BusError::from_code(HOST_ENOSYS), BusError::Unsupported);
}

#[test]
fn an_unnamed_code_is_described_in_the_kernel_s_words() {
    assert_eq!(
        BusError::Unknown(-1).to_string(),
        "internal kernel error (kernel error -1)"
    );
    assert_eq!(BusError::Unknown(-9999).to_string(), "kernel error -9999");
    assert_eq!(BusError::NotFound.to_string(), "no such service");
    assert_eq!(BusError::Malformed.to_string(), "malformed message");
}

#[test]
fn check_splits_values_from_codes() {
    assert_eq!(check(0), Ok(0));
    assert_eq!(check(42), Ok(42));
    assert_eq!(check(kernel_code("NotFound")), Err(BusError::NotFound));
}

// ----------------------------------------------------------------------------
// Agreement with the kernel's source
// ----------------------------------------------------------------------------

const NUMBER_RS: &str = include_str!("../../../kernel/src/syscall/number.rs");
const CHANNEL_RS: &str = include_str!("../../../kernel/src/ipc/channel.rs");
const HANDLERS_RS: &str = include_str!("../../../kernel/src/syscall/handlers.rs");

/// `pub const NAME: u64 = N;` in `kernel/src/syscall/number.rs`.
fn kernel_syscall(name: &str) -> u64 {
    let prefix = format!("pub const {name}: u64 = ");
    let line = NUMBER_RS
        .lines()
        .find_map(|l| l.trim().strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("{name} is not in kernel/src/syscall/number.rs"));
    line.trim_end_matches(';').trim().parse().unwrap()
}

#[test]
fn syscall_numbers_are_the_kernel_s() {
    use syscall_nr::*;
    let ours = [
        ("SYS_CHANNEL_SEND", SYS_CHANNEL_SEND),
        ("SYS_CHANNEL_RECV", SYS_CHANNEL_RECV),
        ("SYS_CHANNEL_TRY_RECV", SYS_CHANNEL_TRY_RECV),
        ("SYS_CHANNEL_CLOSE", SYS_CHANNEL_CLOSE),
        ("SYS_CHANNEL_RECV_TIMEOUT", SYS_CHANNEL_RECV_TIMEOUT),
        ("SYS_CHANNEL_SEND_BLOCKING", SYS_CHANNEL_SEND_BLOCKING),
        ("SYS_CP_CREATE", SYS_CP_CREATE),
        ("SYS_CP_REGISTER", SYS_CP_REGISTER),
        ("SYS_CP_UNREGISTER", SYS_CP_UNREGISTER),
        ("SYS_CP_WAIT", SYS_CP_WAIT),
        ("SYS_CP_TRY_WAIT", SYS_CP_TRY_WAIT),
        ("SYS_CP_CLOSE", SYS_CP_CLOSE),
        ("SYS_CP_NOTIFY", SYS_CP_NOTIFY),
        ("SYS_SERVICE_REGISTER", SYS_SERVICE_REGISTER),
        ("SYS_SERVICE_CONNECT", SYS_SERVICE_CONNECT),
        ("SYS_SERVICE_ACCEPT", SYS_SERVICE_ACCEPT),
        ("SYS_SERVICE_TRY_ACCEPT", SYS_SERVICE_TRY_ACCEPT),
        ("SYS_SERVICE_ACCEPT_TIMEOUT", SYS_SERVICE_ACCEPT_TIMEOUT),
        ("SYS_SERVICE_UNREGISTER", SYS_SERVICE_UNREGISTER),
        ("SYS_CHANNEL_PEER_CRED", SYS_CHANNEL_PEER_CRED),
        ("SYS_TIMER_CREATE", SYS_TIMER_CREATE),
        ("SYS_TIMER_CANCEL", SYS_TIMER_CANCEL),
    ];
    for (name, value) in ours {
        assert_eq!(kernel_syscall(name), value, "{name}");
    }
}

#[test]
fn the_message_size_limit_is_the_kernel_s() {
    let expr = CHANNEL_RS
        .lines()
        .find_map(|l| l.trim().strip_prefix("const MAX_MESSAGE_SIZE: usize = "))
        .expect("MAX_MESSAGE_SIZE in kernel/src/ipc/channel.rs");
    let expr = expr.split(';').next().unwrap();
    let kernel: usize = expr
        .split('*')
        .map(|f| f.trim().replace('_', "").parse::<usize>().unwrap())
        .product();
    assert_eq!(kernel, MAX_MESSAGE_SIZE);
}

#[test]
fn source_types_are_the_kernel_s_encoding() {
    let body = HANDLERS_RS
        .split("fn encode_event(")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .expect("encode_event in kernel/src/syscall/handlers.rs");
    let kernel: Vec<(String, u64)> = body
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("WaitSource::")?;
            let (name, rest) = rest.split_once('(')?;
            let (_, rest) = rest.split_once("=> (")?;
            let (n, _) = rest.split_once(',')?;
            Some((name.to_string(), n.trim_end_matches("u64").parse().ok()?))
        })
        .collect();
    let ours = [
        ("Channel", SourceType::Channel),
        ("PipeRead", SourceType::PipeRead),
        ("PipeWrite", SourceType::PipeWrite),
        ("EventFd", SourceType::EventFd),
        ("ProcessExit", SourceType::ProcessExit),
        ("Timer", SourceType::Timer),
        ("Semaphore", SourceType::Semaphore),
        ("IoCompletion", SourceType::IoCompletion),
    ];
    // The parse found the function: a table this test could not read would
    // pass the loop below vacuously.
    assert!(kernel.len() >= ours.len(), "kernel kinds: {kernel:?}");
    // Every kind here is the kernel's, with its number. A kind the kernel
    // *adds* does not fail -- `kernel/` is lane A's, and its new wait source
    // must not break lane B's tests; it is simply not offered here until
    // this enum has it (the convention `kerror`'s tests follow).
    for (name, ty) in ours {
        let theirs = kernel
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} is not a kernel wait source"));
        assert_eq!(theirs.1, ty as u64, "{name}");
    }
}

#[test]
fn an_event_is_the_kernel_s_record_field_for_field() {
    let body = HANDLERS_RS
        .split("struct CpEventRaw {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("CpEventRaw in kernel/src/syscall/handlers.rs");
    let kernel: Vec<&str> = body
        .lines()
        .filter_map(|l| l.trim().strip_suffix(": u64,"))
        .collect();
    assert_eq!(kernel, ["source_type", "source_handle", "user_data"]);
    assert_eq!(std::mem::size_of::<Event>(), 24);
    assert_eq!(std::mem::align_of::<Event>(), 8);
}

// ----------------------------------------------------------------------------
// Payload fields
// ----------------------------------------------------------------------------

#[test]
fn fields_roundtrip_including_the_awkward_cases() {
    // An empty list, an empty field, a non-UTF-8 field and a normal one:
    // the empty *field* is the one that breaks naive codecs, because it is
    // indistinguishable from "no field" unless the length is explicit.
    assert_eq!(fields::decode(&fields::encode(&[])), Some(vec![]));

    let items: Vec<&[u8]> = vec![b"session-1", b"", &[0xff, 0xfe, 0x00, b'a']];
    let encoded = fields::encode(&items);
    assert_eq!(fields::decode(&encoded), Some(items));
}

#[test]
fn fields_rejects_a_truncated_payload_rather_than_guessing() {
    let encoded = fields::encode(&[b"alice".as_slice(), b"hunter2".as_slice()]);
    // Every proper prefix is malformed; none may decode to something
    // plausible, because a partly-read argument list is how a password
    // ends up being compared against a truncated copy of itself.
    for cut in 0..encoded.len() {
        assert_eq!(fields::decode(&encoded[..cut]), None, "prefix of len {cut}");
    }
    assert!(fields::decode(&encoded).is_some());
}

#[test]
fn fields_rejects_bytes_after_the_last_field() {
    // A count that stops short of the payload is a count that lies: either a
    // field was dropped or the sender and receiver disagree on the arity.
    let mut encoded = fields::encode(&[b"alice".as_slice()]);
    encoded.push(b'!');
    assert_eq!(fields::decode(&encoded), None);
}

#[test]
fn fields_refuses_a_count_it_cannot_have_been_sent() {
    // A four-byte header claiming four billion fields must not become a
    // four-billion-entry Vec::with_capacity.
    let mut hostile = u32::MAX.to_le_bytes().to_vec();
    hostile.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(fields::decode(&hostile), None);

    // A count just over the cap is refused for the same reason, and a
    // count just under it is not.
    let over: Vec<&[u8]> = vec![b"x"; fields::MAX_FIELDS + 1];
    assert_eq!(fields::decode(&fields::encode(&over)), None);
    let at: Vec<&[u8]> = vec![b"x"; fields::MAX_FIELDS];
    assert!(fields::decode(&fields::encode(&at)).is_some());
}

#[test]
fn fields_refuses_a_length_that_overruns_the_buffer() {
    // One field claiming 16 bytes in an 8-byte payload.
    let mut lying = 1u32.to_le_bytes().to_vec();
    lying.extend_from_slice(&16u32.to_le_bytes());
    lying.extend_from_slice(b"short");
    assert_eq!(fields::decode(&lying), None);
}

#[test]
fn decode_exact_enforces_arity() {
    let two = fields::encode(&[b"a".as_slice(), b"b".as_slice()]);
    assert!(fields::decode_exact(&two, 2).is_some());
    assert!(fields::decode_exact(&two, 1).is_none());
    assert!(fields::decode_exact(&two, 3).is_none());
}

// ----------------------------------------------------------------------------
// Duration helpers
// ----------------------------------------------------------------------------

#[test]
fn duration_helpers_convert_and_saturate() {
    assert_eq!(ms_to_ns(1), 1_000_000);
    assert_eq!(ms_to_ns(1000), 1_000_000_000);
    assert_eq!(secs_to_ns(1), 1_000_000_000);
    assert_eq!(secs_to_ns(60), 60_000_000_000);
    assert_eq!(secs_to_ns(u64::MAX), u64::MAX);
    assert_eq!(ms_to_ns(u64::MAX / 2), u64::MAX);
}
