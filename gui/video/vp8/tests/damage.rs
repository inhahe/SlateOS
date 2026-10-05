//! Damaged streams: the decoder must do with them exactly what libvpx does.
//!
//! `tests/data/damage.txt` records, for 446 damaged copies of the committed
//! vectors -- bytes flipped in every part of a frame, frames cut short at
//! every boundary, frames left out -- what libvpx v1.17.0's decoder made of
//! each frame: the picture it showed (its MD5, cut to 8 hex digits) and
//! whether it marked it corrupt, nothing shown, or an error
//! (`tools/generate_damage.py`, from `tools/damage_reference.c`). This
//! decodes the same damage and compares, frame by frame.
//!
//! Errors compare by kind: libvpx's `VPX_CODEC_UNSUP_BITSTREAM` is
//! [`Error::Unsupported`], its `VPX_CODEC_CORRUPT_FRAME` and
//! `VPX_CODEC_ERROR` [`Error::Corrupt`]. A reference copied from a buffer
//! that does not exist is libvpx's success that shows nothing (`badcopy`)
//! and this decoder's error.
//!
//! Each runs on one thread and on several (the streams with token
//! partitions decode their rows on threads of their own), with the same
//! answers.
//!
//! A second test damages the whole committed set at random, far more than
//! libvpx's answers could be kept for, and asks only that nothing panics.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::panic,
    reason = "a test: a mismatch should fail it loudly"
)]

mod common;

use vp8::{Decoder, Error};

/// The frames the harness decodes, at most: `tools/generate_damage.py`'s
/// `LIMIT`.
const LIMIT: usize = 12;

/// One damage, as `tools/damage_reference.c` reads it.
#[derive(Debug)]
enum Mutation {
    Flip { frame: usize, at: usize, xor: u8 },
    Cut { frame: usize, len: usize },
    Drop { frame: usize },
}

fn parse_mutation(s: &str) -> Mutation {
    let parts: Vec<&str> = s.split(':').collect();
    let n = |i: usize| parts[i].parse::<usize>().unwrap();
    match parts[0] {
        "flip" => Mutation::Flip {
            frame: n(1),
            at: n(2),
            xor: n(3) as u8,
        },
        "cut" => Mutation::Cut {
            frame: n(1),
            len: n(2),
        },
        "drop" => Mutation::Drop { frame: n(1) },
        other => panic!("unknown mutation {other}"),
    }
}

/// What the decoder, on up to `threads` threads, makes of each frame of
/// `frames` damaged by `mutations`, in the harness's words.
fn decode_damaged(frames: &[Vec<u8>], mutations: &[Mutation], threads: usize) -> Vec<String> {
    let mut d = Decoder::new();
    d.set_threads(threads);
    let mut out = Vec::new();
    for (i, original) in frames.iter().enumerate().take(LIMIT) {
        let mut frame = original.clone();
        let mut skip = false;
        for m in mutations {
            match *m {
                Mutation::Flip { frame: f, at, xor } if f == i => {
                    if let Some(b) = frame.get_mut(at) {
                        *b ^= xor;
                    }
                }
                Mutation::Cut { frame: f, len } if f == i => frame.truncate(len),
                Mutation::Drop { frame: f } if f == i => skip = true,
                _ => {}
            }
        }
        if skip {
            continue;
        }
        out.push(match d.decode(&frame) {
            Ok(Some(p)) => {
                let mut md5 = md5::Md5::new();
                for plane in 0..3 {
                    let v = p.plane(plane).unwrap();
                    for row in 0..v.height {
                        md5.update(&v.data[row * v.stride..row * v.stride + v.width]);
                    }
                }
                let hex = md5::hex(&md5.finalize()).to_string();
                format!("ok {} {}", &hex[..8], u8::from(p.corrupted()))
            }
            Ok(None) => "none".to_owned(),
            Err(Error::Corrupt("a reference copied from a buffer that does not exist")) => {
                "badcopy".to_owned()
            }
            Err(Error::Unsupported(_)) => "err unsupported".to_owned(),
            Err(Error::Corrupt(_)) => "err corrupt".to_owned(),
        });
    }
    out
}

/// libvpx's error codes, by the kind this decoder reports them as.
fn normalise(line: &str) -> String {
    match line {
        "err 5" => "err unsupported".to_owned(),
        "err 1" | "err 7" => "err corrupt".to_owned(),
        other => other.to_owned(),
    }
}

/// Decode every case of `damage.txt` on up to `threads` threads, and
/// compare with libvpx's answers.
fn check_damage(threads: usize) {
    let text = std::fs::read_to_string(common::committed_dir().join("damage.txt")).unwrap();
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let mut cases = 0;
    let mut failures = Vec::new();
    while let Some(head) = lines.next() {
        let mut words = head.split_whitespace();
        assert_eq!(words.next(), Some("case"), "{head}");
        let vector = words.next().unwrap();
        let mutations: Vec<Mutation> = words.map(parse_mutation).collect();
        let want: Vec<String> = lines
            .by_ref()
            .take_while(|l| *l != "end")
            .map(normalise)
            .collect();
        let v = common::read_vector(&common::committed_dir().join(vector)).unwrap();
        let got = decode_damaged(&v.frames, &mutations, threads);
        if got != want {
            let at = got
                .iter()
                .zip(&want)
                .position(|(g, w)| g != w)
                .unwrap_or(got.len().min(want.len()));
            failures.push(format!(
                "{head}: frame result {at}: got {:?}, libvpx {:?}",
                got.get(at),
                want.get(at)
            ));
        }
        cases += 1;
    }
    assert_eq!(cases, 446, "the cases tools/generate_damage.py writes");
    assert!(
        failures.is_empty(),
        "{} of {cases} damaged streams decode unlike libvpx on {threads} threads:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn damaged_streams_decode_as_libvpx_decodes_them() {
    check_damage(1);
}

#[test]
fn damaged_streams_decode_as_libvpx_decodes_them_on_three_threads() {
    check_damage(3);
}

#[test]
fn damaged_streams_decode_as_libvpx_decodes_them_on_eight_threads() {
    check_damage(8);
}

/// A fixed sequence of pseudo-random numbers: Numerical Recipes' generator.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n.max(1)
    }
}

#[test]
fn heavily_damaged_streams_never_panic() {
    let mut rng = Lcg(0x0bad_5eed);
    for path in common::vectors_in(&common::committed_dir()) {
        let v = common::read_vector(&path).unwrap();
        let frames: Vec<Vec<u8>> = v.frames.iter().take(LIMIT).cloned().collect();
        for round in 0..40 {
            let mut d = Decoder::new();
            d.set_threads([1, 3, 8][round % 3]);
            for frame in &frames {
                let mut f = frame.clone();
                // Up to eight flips, sometimes a cut, now and then a frame
                // of noise.
                for _ in 0..rng.below(9) {
                    let at = rng.below(f.len());
                    if let Some(b) = f.get_mut(at) {
                        *b ^= (rng.next() & 0xff) as u8 | 1;
                    }
                }
                if rng.below(4) == 0 {
                    let len = rng.below(f.len() + 1);
                    f.truncate(len);
                }
                if rng.below(16) == 0 {
                    f = (0..rng.below(4096))
                        .map(|_| (rng.next() & 0xff) as u8)
                        .collect();
                }
                if let Ok(Some(p)) = d.decode(&f) {
                    // Every plane of whatever shows is readable.
                    for plane in 0..3 {
                        let view = p.plane(plane).unwrap();
                        let last = (view.height - 1) * view.stride + view.width;
                        assert!(view.data.len() >= last);
                    }
                }
            }
        }
    }
}
