//! WebM files played end to end: each fixture's video packets, as this
//! demuxer gives them, decoded by SlateOS's VP9 decoder (`gui/video/vp9`) or
//! AV1 decoder (rav1d), every frame held to the MD5 ffmpeg prints for it
//! (`-f framemd5`, `tests/data/generate_fixtures.py`): a frame's planes, row
//! by row. VP9 with alpha decodes twice -- the picture from the block, the
//! alpha channel from its BlockAdditional, a second VP9 stream -- and hashes
//! as ffmpeg's YUVA does, the alpha stream's luma as the fourth plane.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

use std::fs::File;

use matroska::{Codec, Demuxer};

/// Each frame's size and MD5, as ffmpeg decodes the fixture.
fn frames(name: &str) -> Vec<(usize, String)> {
    let base = name.rsplit_once('.').unwrap().0;
    let path = format!(
        "{}/tests/data/{base}.frames.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let w: Vec<&str> = l.split(' ').collect();
            (w[1].parse().unwrap(), w[2].to_owned())
        })
        .collect()
}

/// A packet's bytes and its additions.
type Frame = (Vec<u8>, Vec<(u64, Vec<u8>)>);

/// The video track's packets: its codec, and each packet's bytes and
/// additions.
fn video_packets(name: &str) -> (Codec, Vec<Frame>) {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    let mut d = Demuxer::open(File::open(&path).unwrap()).unwrap();
    let video = d
        .tracks()
        .iter()
        .find(|t| t.kind == matroska::TrackKind::Video)
        .expect("a video track")
        .clone();
    let mut out = Vec::new();
    while let Some(p) = d.next_packet().unwrap() {
        if p.track == video.number {
            out.push((p.data, p.additions));
        }
    }
    (video.codec, out)
}

/// Planes, row by row, hashed as one frame.
fn hash(planes: &[&[u8]]) -> (usize, String) {
    let all: Vec<u8> = planes.concat();
    (all.len(), md5::md5_hex(&all).to_string())
}

/// A VP9 picture's three planes, each row without its padding.
fn vp9_planes(picture: &vp9::Picture) -> [Vec<u8>; 3] {
    [0, 1, 2].map(|p| {
        let plane = picture.plane8(p).unwrap();
        (0..plane.height)
            .flat_map(|y| {
                plane.data[y * plane.stride..y * plane.stride + plane.width]
                    .iter()
                    .copied()
            })
            .collect()
    })
}

#[test]
fn vp9_in_webm_decodes_as_ffmpeg_decodes_it() {
    let (codec, packets) = video_packets("vp9_opus.webm");
    assert_eq!(codec, Codec::Vp9);
    let mut decoder = vp9::Decoder::new();
    let got: Vec<(usize, String)> = packets
        .iter()
        .filter_map(|(data, _)| decoder.decode(data).unwrap())
        .map(|picture| {
            let [y, u, v] = vp9_planes(&picture);
            hash(&[&y, &u, &v])
        })
        .collect();
    assert_eq!(got, frames("vp9_opus.webm"));
}

#[test]
fn vp9_with_alpha_decodes_as_libvpx_decodes_it() {
    let (codec, packets) = video_packets("vp9_alpha.webm");
    assert_eq!(codec, Codec::Vp9);
    let (mut picture_decoder, mut alpha_decoder) = (vp9::Decoder::new(), vp9::Decoder::new());
    let mut got = Vec::new();
    for (data, additions) in &packets {
        let alpha = additions
            .iter()
            .find(|(id, _)| *id == 1)
            .map(|(_, bytes)| bytes)
            .expect("each frame's alpha, BlockAddID 1");
        let picture = picture_decoder.decode(data).unwrap().expect("a picture");
        let alpha = alpha_decoder
            .decode(alpha)
            .unwrap()
            .expect("an alpha picture");
        let [y, u, v] = vp9_planes(&picture);
        let [a, _, _] = vp9_planes(&alpha);
        got.push(hash(&[&y, &u, &v, &a]));
    }
    assert_eq!(got, frames("vp9_alpha.webm"));
}

#[test]
fn av1_in_webm_decodes_as_dav1d_decodes_it() {
    use rav1d::safe::{Data, Decoder, Error, Settings};

    let (codec, packets) = video_packets("av1.webm");
    assert_eq!(codec, Codec::Av1);
    let mut decoder = Decoder::new(&Settings {
        threads: 1,
        max_frame_delay: 1,
        ..Settings::default()
    })
    .unwrap();
    let mut got = Vec::new();
    for (data, _) in &packets {
        let mut sample = Data::new(data).unwrap();
        loop {
            if !sample.is_consumed() {
                match decoder.send(&mut sample) {
                    Ok(()) | Err(Error::Again) => {}
                    Err(e) => panic!("{e}"),
                }
            }
            match decoder.picture() {
                Ok(picture) => {
                    let [y, u, v] = [0, 1, 2].map(|p| picture.plane_u8(p).unwrap().samples);
                    got.push(hash(&[&y, &u, &v]));
                }
                Err(Error::Again) if sample.is_consumed() => break,
                Err(Error::Again) => {}
                Err(e) => panic!("{e}"),
            }
        }
    }
    assert_eq!(got, frames("av1.webm"));
}
