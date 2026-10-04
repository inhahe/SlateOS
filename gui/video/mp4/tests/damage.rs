//! Every byte of several fixtures changed in turn -- its low bit, its high
//! bit, and all of it -- then each file cut short at every length: each
//! opens and reads to its end, or is refused, and a seek in each works or is
//! refused -- never a panic, never more packets or bytes than the file could
//! hold.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

use std::io::Cursor;

use mp4::Demuxer;

fn read_all(name: &str, data: &[u8], what: &dyn Fn() -> String) {
    let Ok(mut d) = Demuxer::open(Cursor::new(data.to_vec())) else {
        return;
    };
    let (mut packets, mut total) = (0usize, 0usize);
    let limit_packets = data.len() * 64 + 1024;
    let mut bounded = |d: &mut Demuxer<Cursor<Vec<u8>>>| {
        while let Ok(Some(p)) = d.next_packet() {
            packets += 1;
            total += p.data.len();
            assert!(packets <= limit_packets, "{name}: {}: packets", what());
            assert!(total <= data.len() * 64 + 1024, "{name}: {}: bytes", what());
        }
    };
    bounded(&mut d);
    let tracks = d.tracks().len();
    for t in 0..tracks {
        if d.seek(t, 3000).is_ok() {
            bounded(&mut d);
        }
    }
}

#[test]
fn a_damaged_file_is_read_or_refused_but_never_panics() {
    for name in [
        "av1.mp4",
        "vp9_opus.mp4",
        "h264_negative_cts.mp4",
        "aac.mp4",
        "fragmented.mp4",
        "delayed_audio.mp4",
        // What a track says of its picture: colr over vpcC, a clean
        // aperture, and two display matrices.
        "colr_nclc_after_vpcc.mp4",
        "clap_offset.mp4",
        "both_rotations.mp4",
    ] {
        let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&path).unwrap();
        let mut damaged = bytes.clone();
        for at in 0..bytes.len() {
            for mask in [0x01, 0x80, 0xff] {
                damaged[at] ^= mask;
                read_all(name, &damaged, &|| format!("byte {at} ^ {mask:#x}"));
                damaged[at] ^= mask;
            }
        }
        for len in 0..bytes.len() {
            read_all(name, &bytes[..len], &|| format!("cut at {len}"));
        }
    }
}
