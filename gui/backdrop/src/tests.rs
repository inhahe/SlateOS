#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

fn pixels(n: usize) -> Vec<u32> {
    (0..n)
        .map(|i| 0xFF00_0000 | u32::try_from(i).unwrap())
        .collect()
}

/// **A picture written is the picture read**, one after another, and the
/// end of the stream between two is the end of the pictures.
#[test]
fn pictures_written_are_read_back() {
    let mut out = Vec::new();
    write_frame(&mut out, 3, 2, &pixels(6)).unwrap();
    write_frame(&mut out, 1, 1, &[0x8012_3456]).unwrap();
    assert_eq!(out.len(), (HEAD_LEN + 24) + (HEAD_LEN + 4));
    assert_eq!(&out[..4], b"SBG1");
    assert_eq!(
        &out[16..20],
        &[0x00, 0x00, 0x00, 0xFF],
        "little-endian ARGB"
    );
    let mut input = &out[..];
    assert_eq!(
        read_frame(&mut input).unwrap(),
        Some(Frame {
            width: 3,
            height: 2,
            pixels: pixels(6),
        })
    );
    assert_eq!(
        read_frame(&mut input).unwrap().map(|f| f.pixels),
        Some(vec![0x8012_3456])
    );
    assert_eq!(read_frame(&mut input).unwrap(), None, "the end");
}

/// **What is not a picture is refused**: another stream, a picture cut
/// short, of nothing, or larger than a screen -- before its pixels are
/// allocated.
#[test]
fn what_is_not_a_picture_is_refused() {
    assert_eq!(
        read_frame(&mut &b"not a picture at all"[..]),
        Err(FrameError::NotAPicture)
    );
    let mut whole = Vec::new();
    write_frame(&mut whole, 2, 2, &pixels(4)).unwrap();
    for cut in [5, HEAD_LEN, HEAD_LEN + 1, whole.len() - 1] {
        assert_eq!(
            read_frame(&mut &whole[..cut]),
            Err(FrameError::CutShort),
            "cut at {cut}"
        );
    }
    let head = |w: u32, h: u32| {
        let mut b = MAGIC.to_vec();
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b
    };
    assert_eq!(read_frame(&mut &head(0, 5)[..]), Err(FrameError::Empty));
    assert_eq!(
        read_frame(&mut &head(100_000, 100_000)[..]),
        Err(FrameError::TooLarge {
            width: 100_000,
            height: 100_000
        })
    );
    assert_eq!(
        read_frame(&mut &head(7680, 4321)[..]),
        Err(FrameError::TooLarge {
            width: 7680,
            height: 4321
        }),
        "one row past the largest"
    );
}

/// **A picture that cannot be written is not**: no size, or pixels that do
/// not fill it.
#[test]
fn a_bad_picture_is_not_written() {
    let mut out = Vec::new();
    assert!(write_frame(&mut out, 0, 1, &[]).is_err());
    assert!(write_frame(&mut out, 2, 2, &pixels(3)).is_err());
    assert!(out.is_empty(), "nothing half-written");
}

/// **Every event is a line that reads back as itself.**
#[test]
fn every_event_reads_back() {
    let rect = Rect {
        x: -10,
        y: 20,
        width: 300,
        height: 400,
    };
    for event in [
        Event::Size {
            width: 1920,
            height: 1080,
        },
        Event::Pause,
        Event::Resume,
        Event::Pointer { x: -3, y: 7 },
        Event::Desktop(2),
        Event::Windows(vec![rect, rect]),
        Event::Windows(Vec::new()),
        Event::Theme {
            dark: true,
            accent: 0x0078D4,
        },
        Event::Theme {
            dark: false,
            accent: 0,
        },
    ] {
        let line = event.line();
        assert!(!line.contains('\n'), "{line}");
        assert_eq!(Event::parse(&line), Some(event), "{line}");
    }
    assert_eq!(
        Event::Size {
            width: 3,
            height: 4
        }
        .line(),
        "size 3 4"
    );
    assert_eq!(Event::Windows(vec![rect]).line(), "windows -10,20,300,400");
    assert_eq!(
        Event::Theme {
            dark: false,
            accent: 0x00AB_CDEF
        }
        .line(),
        "theme light abcdef"
    );
}

/// **A line that is not an event is none**: unknown, short, too long, or
/// with words that are not numbers -- which a program passes over.
#[test]
fn a_line_that_is_not_an_event_is_none() {
    for line in [
        "",
        "dance",
        "size 3",
        "size 3 4 5",
        "size x 4",
        "pointer 1",
        "desktop -1",
        "windows 1,2,3",
        "windows 1,2,3,4,5",
        "theme dim 000000",
        "theme dark 1000000",
        "pause now",
    ] {
        assert_eq!(Event::parse(line), None, "{line:?}");
    }
}
