//! A video track's `Projection`, held to `ffprobe`: the display matrix FFmpeg
//! makes of a rectangular projection's pose ([`matroska::Video::display_matrix`]),
//! none for the poses and kinds it passes over, and the files it refuses to
//! open for a spherical projection's private data refused here too
//! (`tests/data/generate_fixtures.py`, `projections()`).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test: a failure should be loud"
)]

use std::fs::File;

use matroska::{Demuxer, TrackKind};

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_projection_is_read_as_ffmpeg_reads_it() {
    let text = std::fs::read_to_string(data("projections.txt")).unwrap();
    let mut seen = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (name, want) = line.split_once(' ').unwrap();
        let opened = Demuxer::open(File::open(data(name)).unwrap());
        let got = match opened {
            Err(_) => "refused".to_owned(),
            Ok(d) => {
                let video = d
                    .tracks()
                    .iter()
                    .find(|t| t.kind == TrackKind::Video)
                    .and_then(|t| t.video);
                match video.and_then(|v| v.display_matrix()) {
                    Some(m) => m.map(|n| n.to_string()).join(","),
                    None => "none".to_owned(),
                }
            }
        };
        assert_eq!(got, want, "{name}");
        seen += 1;
    }
    assert!(seen >= 20, "{seen} projections");
}

/// The pose is kept as the file gives it, whatever FFmpeg makes of it.
#[test]
fn a_projection_keeps_its_pose_and_kind() {
    let d = Demuxer::open(File::open(data("proj_mirror_roll.mkv")).unwrap()).unwrap();
    let p = d.tracks()[0].video.unwrap().projection.unwrap();
    assert_eq!((p.kind, p.version), (0, None));
    assert_eq!((p.yaw, p.pitch, p.roll), (180.0, 0.0, -90.0));
    let d = Demuxer::open(File::open(data("proj_cubemap.mkv")).unwrap()).unwrap();
    let p = d.tracks()[0].video.unwrap().projection.unwrap();
    assert_eq!((p.kind, p.version), (2, Some(0)));
    // A file without one has none.
    let d = Demuxer::open(File::open(data("vp9_opus.webm")).unwrap()).unwrap();
    assert!(d.tracks()[0].video.unwrap().projection.is_none());
}
