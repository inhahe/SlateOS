//! Every stream in tests/data, decoded whole, against Tremor: the headers
//! as Tremor reads them, and every sample at Tremor's own precision and as
//! its 16-bit output (`references.txt`, from `tools/references.py`).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

mod common;

use vorbis::{Comments, Decoder, Info};

/// The references of one kind: (stream, the rest of the line's key, what
/// Tremor said).
fn references(kind: &str) -> Vec<(String, String, String)> {
    let text = std::fs::read_to_string(common::data("references.txt")).unwrap();
    text.lines()
        .filter_map(|line| {
            let (key, want) = line.split_once(" => ")?;
            let mut words = key.split(' ');
            let name = words.next()?.to_owned();
            (words.next()? == kind)
                .then(|| (name, words.collect::<Vec<_>>().join(" "), want.to_owned()))
        })
        .collect()
}

/// `reference.c`'s `headers` output for the stream's first three packets.
fn headers(packets: &[Vec<u8>]) -> String {
    let info = match Info::parse(&packets[0]) {
        Ok(info) => info,
        Err(e) => return format!("headers refused {}", 100 - e.code()),
    };
    let comments = match Comments::parse(&packets[1]) {
        Ok(c) => c,
        Err(e) => return format!("headers refused {}", 200 - e.code()),
    };
    let vendor_len = comments
        .vendor
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(comments.vendor.len());
    let vendor = &comments.vendor[..vendor_len];
    let mut h = common::fnv64(common::FNV64, vendor);
    for c in &comments.comments {
        h = common::fnv64(h, c);
    }
    let init = if Decoder::new(&packets[0], &packets[2]).is_ok() {
        "ok"
    } else {
        "refused"
    };
    format!(
        "channels {} rate {} bitrates {} {} {} blocksizes {} {} | vendor {} comments {} digest {h:016x} | init {init}",
        info.channels,
        info.rate,
        info.bitrate_upper as u32,
        info.bitrate_nominal as u32,
        info.bitrate_lower as u32,
        info.blocksizes[0],
        info.blocksizes[1],
        String::from_utf8_lossy(vendor),
        comments.comments.len(),
    )
}

#[test]
fn headers_read_as_tremor_reads_them() {
    let refs = references("headers");
    assert!(refs.len() >= 20, "{} streams", refs.len());
    for (name, _, want) in refs {
        let packets = common::ogg_packets(&std::fs::read(common::data(&name)).unwrap());
        assert_eq!(headers(&packets), want, "{name}");
    }
}

#[test]
fn every_stream_decodes_as_tremor_decodes_it() {
    let refs = references("decode");
    assert!(refs.len() >= 20, "{} streams", refs.len());
    let mut failed = Vec::new();
    for (name, _, want) in refs {
        let packets = common::ogg_packets(&std::fs::read(common::data(&name)).unwrap());
        let (got, _) = common::run(&packets, None, false);
        if got != want {
            failed.push(format!("{name}\n   got {got}\n  want {want}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of the streams differ:\n{}",
        failed.len(),
        failed.join("\n")
    );
}
