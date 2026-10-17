//! init's bus encodings, held to `libservicebus`'s.

use super::*;
use libservicebus::fields;

/// A method, its arguments, and what they read as.
type Case<'a, T> = (&'a [u8], &'a [&'a [u8]], T);

/// A message as `libservicebus` documents its wire format (version 2).
fn wire(ty: u8, member: &[u8], payload: &[u8], serial: u64, reply_serial: u64) -> Vec<u8> {
    let mut v = vec![ty, 0];
    v.extend_from_slice(&u16::try_from(member.len()).unwrap().to_le_bytes());
    v.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    v.extend_from_slice(&serial.to_le_bytes());
    v.extend_from_slice(&reply_serial.to_le_bytes());
    v.extend_from_slice(member);
    v.extend_from_slice(payload);
    v
}

/// A call to `member` with `args`, as a client sends it.
fn call(member: &[u8], args: &[&[u8]]) -> Vec<u8> {
    wire(1, member, &fields::encode(args), 7, 0)
}

/// The request a call to `member` with `args` reads as.
fn request<'a>(bytes: &'a [u8]) -> Result<Request<'a>, &'static [u8]> {
    let m = decode(bytes).expect("a well-formed call");
    read_request(m.member, m.payload)
}

#[test]
fn a_call_is_read_and_what_is_malformed_is_not() {
    let payload = fields::encode(&[b"sshd"]);
    let good = wire(1, b"StopService", &payload, 9, 0);
    assert_eq!(
        decode(&good),
        Some(Message {
            msg_type: MessageType::MethodCall,
            member: b"StopService",
            payload: &payload,
            serial: 9,
            reply_serial: 0,
        })
    );
    let mut longer = good.clone();
    longer.push(0);
    let bad: [Vec<u8>; 7] = [
        good.get(..good.len() - 1).unwrap().to_vec(), // a byte short
        longer,                                       // a byte over
        wire(5, b"X", b"", 9, 0),                     // no such type
        wire(1, b"X", b"", 0, 0),                     // serial 0
        wire(1, b"X", b"", 9, 3),                     // a call answering one
        wire(2, b"", b"", 9, 0),                      // a reply answering none
        wire(1, &[0xff, 0xfe], b"", 9, 0),            // a member not UTF-8
    ];
    for b in &bad {
        assert_eq!(decode(b), None, "{b:?}");
    }
    assert_eq!(decode(&good[..HEADER_SIZE - 1]), None);
}

#[test]
fn a_reply_is_written_as_libservicebus_reads_it() {
    let mut out = [0u8; 256];
    let n = encode_reply(&mut out, 4, 9, None, &[b"stopped"]).unwrap();
    assert_eq!(
        &out[..n],
        wire(2, b"", &fields::encode(&[b"stopped"]), 4, 9)
    );
    let n = encode_reply(&mut out, 5, 9, Some(error::DENIED), &[]).unwrap();
    assert_eq!(
        &out[..n],
        wire(3, error::DENIED, &fields::encode(&[]), 5, 9)
    );
    let m = decode(&out[..n]).unwrap();
    assert_eq!(
        (m.msg_type, m.reply_serial, m.member),
        (MessageType::Error, 9, error::DENIED)
    );

    // What does not fit, and what answers nothing, is not written.
    assert_eq!(
        encode_reply(&mut out[..HEADER_SIZE + 3], 4, 9, None, &[]),
        None
    );
    assert_eq!(encode_reply(&mut out, 4, 0, None, &[]), None);
    assert_eq!(encode_reply(&mut out, 0, 9, None, &[]), None);
}

#[test]
fn fields_are_libservicebuss_both_ways() {
    let lists: [&[&[u8]]; 4] = [&[], &[b""], &[b"sshd"], &[b"5", b"reboot"]];
    for list in lists {
        let theirs = fields::encode(list);
        // Ours, written as the payload of a reply: the bytes after its header.
        let mut out = [0u8; 128];
        let n = encode_reply(&mut out, 1, 1, None, list).unwrap();
        assert_eq!(&out[HEADER_SIZE..n], &theirs[..], "{list:?}");
        // And read back as theirs reads it.
        let mut args: [&[u8]; MAX_ARGS] = [&[]; MAX_ARGS];
        assert_eq!(decode_fields(&theirs, list.len(), &mut args), Some(()));
        assert_eq!(&args[..list.len()], list);
        assert_eq!(
            fields::decode_exact(&theirs, list.len()).as_deref(),
            Some(list)
        );
    }
    // What theirs refuses, ours refuses: truncated, bytes left over, another
    // count.
    let two = fields::encode(&[b"5", b"reboot"]);
    let mut args: [&[u8]; MAX_ARGS] = [&[]; MAX_ARGS];
    for (payload, n) in [
        (&two[..two.len() - 1], 2),
        (&[two.as_slice(), &[0]].concat()[..], 2),
        (&two[..], 1),
        (&two[..2], 0),
    ] {
        assert_eq!(fields::decode_exact(payload, n), None);
        assert_eq!(
            decode_fields(payload, n, &mut args),
            None,
            "{payload:?} {n}"
        );
    }
}

#[test]
fn each_request_is_read_with_its_arguments() {
    let cases: [Case<'_, Request<'_>>; 12] = [
        (b"PowerOff", &[], Request::Power(Power::Off)),
        (b"Reboot", &[], Request::Power(Power::Reboot)),
        (b"Suspend", &[], Request::Sleep),
        (b"Hibernate", &[], Request::Sleep),
        (b"CancelScheduledPower", &[], Request::CancelSchedule),
        (
            b"SchedulePower",
            &[b"5", b"reboot"],
            Request::Schedule {
                minutes: 5,
                power: Power::Reboot,
            },
        ),
        (
            b"StartService",
            &[b"sshd"],
            Request::Service(ServiceOp::Start, b"sshd"),
        ),
        (
            b"StopService",
            &[b"sshd"],
            Request::Service(ServiceOp::Stop, b"sshd"),
        ),
        (
            b"RestartService",
            &[b"sshd"],
            Request::Service(ServiceOp::Restart, b"sshd"),
        ),
        (
            b"ReloadService",
            &[b"sshd"],
            Request::Service(ServiceOp::Reload, b"sshd"),
        ),
        (
            b"EnableService",
            &[b"sshd"],
            Request::Service(ServiceOp::Enable, b"sshd"),
        ),
        (
            b"DisableService",
            &[b"sshd"],
            Request::Service(ServiceOp::Disable, b"sshd"),
        ),
    ];
    for (member, args, want) in cases {
        let bytes = call(member, args);
        assert_eq!(
            request(&bytes),
            Ok(want),
            "{}",
            String::from_utf8_lossy(member)
        );
    }
}

#[test]
fn a_request_it_cannot_read_is_refused_by_name() {
    let invalid = error::INVALID_ARGUMENTS;
    let cases: [Case<'_, &[u8]>; 10] = [
        (b"Shutdown", &[], error::UNKNOWN_METHOD),
        (b"PowerOff", &[b"now"], invalid),
        (b"StopService", &[], invalid),
        (b"StopService", &[b""], invalid),
        (b"StopService", &[b"/bin/sshd"], invalid),
        (b"SchedulePower", &[b"0", b"shutdown"], invalid),
        (b"SchedulePower", &[b"527041", b"shutdown"], invalid),
        (b"SchedulePower", &[b"5m", b"shutdown"], invalid),
        (b"SchedulePower", &[b"5", b"halt"], invalid),
        (b"SchedulePower", &[b"5"], invalid),
    ];
    for (member, args, want) in cases {
        let bytes = call(member, args);
        assert_eq!(
            request(&bytes),
            Err(want),
            "{} {args:?}",
            String::from_utf8_lossy(member)
        );
    }
    assert_eq!(
        request(&call(b"SchedulePower", &[b"527040", b"shutdown"])),
        Ok(Request::Schedule {
            minutes: MAX_SCHEDULE_MINUTES,
            power: Power::Off
        })
    );
}
