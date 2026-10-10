//! Tests for the file chooser's messages: what goes out comes back, every
//! bound holds both ways, and nothing malformed is read in part.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::ffi::OsString;
use std::path::PathBuf;

use super::{
    Decoded, Filter, MAX_FILTERS, MAX_FRAME, MAX_NAME, MAX_PATTERNS, MAX_TEXT, Mode, Reply,
    Request, TooLarge, decode_reply, decode_request, encode_reply, encode_request,
};

/// A path that is absolute here, whatever the platform.
fn absolute(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

fn saving() -> Request {
    Request {
        mode: Mode::Save,
        owner: 0x1234_5678_9abc,
        title: "Export as PDF".into(),
        start: absolute("docs"),
        name: OsString::from("report.pdf"),
        filters: vec![
            Filter {
                label: "PDF".into(),
                patterns: vec!["*.pdf".into()],
            },
            Filter {
                label: "All files".into(),
                patterns: vec!["*".into()],
            },
        ],
        filter: 1,
    }
}

/// **What is sent is what is read**: a request in each mode, a reply of
/// each kind.
#[test]
fn what_is_sent_is_what_is_read() {
    for mode in [Mode::Open, Mode::Save, Mode::Folder] {
        let request = Request { mode, ..saving() };
        let frame = encode_request(&request).unwrap();
        assert_eq!(
            decode_request(&frame),
            Decoded::Complete(request, frame.len())
        );
    }
    // The least a request can say: no title, no start, no name, no filters.
    let bare = Request {
        mode: Mode::Open,
        owner: 0,
        title: String::new(),
        start: PathBuf::new(),
        name: OsString::new(),
        filters: Vec::new(),
        filter: 0,
    };
    let frame = encode_request(&bare).unwrap();
    assert_eq!(decode_request(&frame), Decoded::Complete(bare, frame.len()));
    for reply in [
        Reply::Cancelled,
        Reply::Chosen {
            path: absolute("report.pdf"),
            filter: 0,
        },
    ] {
        let frame = encode_reply(&reply).unwrap();
        assert_eq!(decode_reply(&frame), Decoded::Complete(reply, frame.len()));
    }
}

/// **Part of a frame is not yet a message**, at every cut; and the frame is
/// read off the front of whatever follows it.
#[test]
fn part_of_a_frame_is_not_yet_a_message() {
    let frame = encode_request(&saving()).unwrap();
    for cut in 0..frame.len() {
        assert_eq!(decode_request(&frame[..cut]), Decoded::Partial, "{cut}");
    }
    let mut two = frame.clone();
    two.extend_from_slice(&encode_reply(&Reply::Cancelled).unwrap());
    assert_eq!(
        decode_request(&two),
        Decoded::Complete(saving(), frame.len())
    );
}

/// **A frame too long is refused before it arrives**: the length alone is
/// enough to give up.
#[test]
fn a_frame_too_long_is_refused_before_it_arrives() {
    let declared = u32::try_from(MAX_FRAME + 1).unwrap().to_le_bytes();
    assert_eq!(decode_request(&declared), Decoded::Malformed);
    assert_eq!(decode_reply(&declared), Decoded::Malformed);
}

/// `frame` with its message's byte at `at` (counting from the message's
/// start, after the length) set to `value`.
fn with_byte(mut frame: Vec<u8>, at: usize, value: u8) -> Vec<u8> {
    frame[4 + at] = value;
    frame
}

/// **What is not this protocol's message is malformed, never half-read**: a
/// wrong mark, version or kind; a mode, outcome or filter that is not one;
/// bytes left over.
#[test]
fn what_is_not_a_message_is_malformed() {
    let request = encode_request(&saving()).unwrap();
    // The header: `SFC`, the version, the kind.
    assert_eq!(
        decode_request(&with_byte(request.clone(), 0, b'X')),
        Decoded::Malformed
    );
    assert_eq!(
        decode_request(&with_byte(request.clone(), 3, 2)),
        Decoded::Malformed
    );
    assert_eq!(decode_reply(&request), Decoded::Malformed);
    // The mode, after the header.
    assert_eq!(
        decode_request(&with_byte(request.clone(), 5, 3)),
        Decoded::Malformed
    );
    // The filter offered first, the last byte: past the two filters.
    let last = request.len() - 5;
    assert_eq!(
        decode_request(&with_byte(request.clone(), last, 2)),
        Decoded::Malformed
    );
    // A byte more in the message than its fields.
    let mut longer = request.clone();
    longer.push(0);
    let length = u32::try_from(longer.len() - 4).unwrap().to_le_bytes();
    longer[..4].copy_from_slice(&length);
    assert_eq!(decode_request(&longer), Decoded::Malformed);
    // An outcome that is neither.
    let reply = encode_reply(&Reply::Cancelled).unwrap();
    assert_eq!(decode_reply(&with_byte(reply, 5, 7)), Decoded::Malformed);
}

/// A request frame, written by hand, holding `name` and `start` as given --
/// what a peer that does not encode through this module could send.
fn hand_written(start: &[u8], name: &[u8], filters: u8) -> Vec<u8> {
    let mut message = b"SFC".to_vec();
    message.extend_from_slice(&[1, 1, 0]);
    message.extend_from_slice(&0u64.to_le_bytes());
    message.extend_from_slice(&0u16.to_le_bytes());
    for field in [start, name] {
        message.extend_from_slice(&u16::try_from(field.len()).unwrap().to_le_bytes());
        message.extend_from_slice(field);
    }
    message.push(filters);
    for _ in 0..filters {
        message.extend_from_slice(&1u16.to_le_bytes());
        message.push(b'A');
        message.push(0);
    }
    message.push(0);
    let mut frame = u32::try_from(message.len()).unwrap().to_le_bytes().to_vec();
    frame.extend_from_slice(&message);
    frame
}

/// **A name or path that is not one is malformed**: a name holding a `/` or
/// a NUL, a start holding a NUL; and more filters than the bound.
#[test]
fn a_name_or_path_that_is_not_one_is_malformed() {
    assert!(matches!(
        decode_request(&hand_written(b"", b"ok.txt", 1)),
        Decoded::Complete(..)
    ));
    assert_eq!(
        decode_request(&hand_written(b"", b"a/b", 1)),
        Decoded::Malformed
    );
    assert_eq!(
        decode_request(&hand_written(b"", b"a\0b", 1)),
        Decoded::Malformed
    );
    assert_eq!(
        decode_request(&hand_written(b"x\0", b"", 1)),
        Decoded::Malformed
    );
    let too_many = u8::try_from(MAX_FILTERS + 1).unwrap();
    assert_eq!(
        decode_request(&hand_written(b"", b"", too_many)),
        Decoded::Malformed
    );
}

/// **A chosen path is absolute, and something**: a reply naming a relative
/// or empty path is malformed, and cannot be sent.
#[test]
fn a_chosen_path_is_absolute() {
    for path in [PathBuf::from("relative.txt"), PathBuf::new()] {
        let reply = Reply::Chosen { path, filter: 0 };
        assert_eq!(encode_reply(&reply), Err(TooLarge));
    }
    // By hand: outcome 1, a two-byte length, "a", filter 0.
    let mut message = b"SFC".to_vec();
    message.extend_from_slice(&[1, 2, 1, 1, 0, b'a', 0]);
    let mut frame = u32::try_from(message.len()).unwrap().to_le_bytes().to_vec();
    frame.extend_from_slice(&message);
    assert_eq!(decode_reply(&frame), Decoded::Malformed);
}

/// **What would break a bound is not sent**: a title, a name, too many
/// filters or patterns, a filter index past the filters.
#[test]
fn what_would_break_a_bound_is_not_sent() {
    let refused = |request: Request| encode_request(&request) == Err(TooLarge);
    assert!(refused(Request {
        title: "t".repeat(MAX_TEXT + 1),
        ..saving()
    }));
    assert!(refused(Request {
        name: OsString::from("n".repeat(MAX_NAME + 1)),
        ..saving()
    }));
    assert!(refused(Request {
        name: OsString::from("a/b"),
        ..saving()
    }));
    assert!(refused(Request {
        filters: vec![
            Filter {
                label: "x".into(),
                patterns: vec!["*".into()],
            };
            MAX_FILTERS + 1
        ],
        filter: 0,
        ..saving()
    }));
    assert!(refused(Request {
        filters: vec![Filter {
            label: "x".into(),
            patterns: vec!["*".into(); MAX_PATTERNS + 1],
        }],
        filter: 0,
        ..saving()
    }));
    assert!(refused(Request {
        filter: 2,
        ..saving()
    }));
    // At the bounds, it is.
    assert!(
        encode_request(&Request {
            title: "t".repeat(MAX_TEXT),
            name: OsString::from("n".repeat(MAX_NAME)),
            ..saving()
        })
        .is_ok()
    );
}

/// **A name is its bytes**: one that is not UTF-8 goes and comes back as it
/// was, never decoded.
#[cfg(unix)]
#[test]
fn a_name_is_its_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let request = Request {
        name: OsString::from_vec(vec![b'r', 0xff, b'.', b't']),
        ..saving()
    };
    let frame = encode_request(&request).unwrap();
    assert_eq!(
        decode_request(&frame),
        Decoded::Complete(request, frame.len())
    );
}
