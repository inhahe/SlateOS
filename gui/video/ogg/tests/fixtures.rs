//! Every fixture's packets, held to the answers `tests/data/generate_fixtures.py`
//! made: ffprobe's -- FFmpeg's demuxer -- but where the crate means to differ
//! from it, and there checked against Tremor (a Vorbis packet FFmpeg
//! mistimes) or against each link alone (a chained file). A test per
//! fixture, so that a failure names the file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud, and its numbers are small"
)]

use std::fs::File;

use ogg::{Codec, Demuxer};

/// One packet as the answer has it.
#[derive(Debug, PartialEq, Eq)]
struct Line {
    stream: usize,
    pts: Option<i64>,
    duration: u64,
    size: usize,
    pos: u64,
    corrupt: bool,
    md5: String,
    skip: u32,
    discard: u32,
    headers: Option<usize>,
}

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A fixture's answers: each stream's codec and time base, and the packets.
fn answers(name: &str) -> (Vec<(String, String)>, Vec<Line>) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (mut streams, mut packets) = (Vec::new(), Vec::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "stream" => streams.push((w[2].to_owned(), w[3].to_owned())),
            "packet" => packets.push(Line {
                stream: w[1].parse().unwrap(),
                pts: (w[2] != "N/A").then(|| w[2].parse().unwrap()),
                duration: w[3].parse().unwrap(),
                size: w[4].parse().unwrap(),
                pos: w[5].parse().unwrap(),
                corrupt: w[6] == "C",
                md5: w[7].to_owned(),
                skip: w[8].parse().unwrap(),
                discard: w[9].parse().unwrap(),
                headers: (w[10] != "-").then(|| w[10].parse().unwrap()),
            }),
            other => panic!("{path}: a line this test does not know: {other}"),
        }
    }
    (streams, packets)
}

/// The name ffprobe gives a codec.
fn ffprobe_name(codec: Codec) -> &'static str {
    match codec {
        Codec::Opus => "opus",
        Codec::Vorbis => "vorbis",
        Codec::Flac => "flac",
        Codec::Speex => "speex",
        Codec::Theora => "theora",
        Codec::Skeleton | Codec::Other => "none",
    }
}

/// Every packet of the file, as the crate gives them.
fn read(name: &str) -> Vec<Line> {
    let mut d =
        Demuxer::open(File::open(data(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut got = Vec::new();
    while let Some(p) = d.next_packet().unwrap_or_else(|e| panic!("{name}: {e}")) {
        got.push(Line {
            stream: p.stream,
            pts: p.pts,
            duration: p.duration,
            size: p.data.len(),
            pos: p.page_position,
            corrupt: p.corrupt,
            md5: md5::md5_hex(&p.data).to_string(),
            skip: p.skip_samples,
            discard: p.discard_padding,
            headers: p.new_headers.as_ref().map(Vec::len),
        });
    }
    got
}

/// The fixture's streams and every packet -- its stream, time, length,
/// size, page, damage flag, bytes, trimming and headers -- as the answer
/// has them.
fn demuxes_as_answered(name: &str) {
    let d =
        Demuxer::open(File::open(data(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let (streams, expected) = answers(name);
    let got_streams: Vec<(String, String)> = d
        .streams()
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tb = d
                .time_base(i)
                .map_or_else(|| "-".to_owned(), |(n, d)| format!("{n}/{d}"));
            (ffprobe_name(s.codec).to_owned(), tb)
        })
        .collect();
    assert_eq!(got_streams.len(), streams.len(), "{name}: the streams");
    for (i, ((codec, tb), (want_codec, want_tb))) in got_streams.iter().zip(&streams).enumerate() {
        assert_eq!(codec, want_codec, "{name}: stream {i}'s codec");
        // A stream this does not time has no time base here.
        if tb != "-" {
            assert_eq!(tb, want_tb, "{name}: stream {i}'s time base");
        }
    }
    let got = read(name);
    for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
        assert_eq!(g, e, "{name}: packet {i}");
    }
    assert_eq!(got.len(), expected.len(), "{name}: the packets");
}

macro_rules! fixtures {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                demuxes_as_answered($name);
            }
        )*

        /// Every fixture in the directory has a test.
        #[test]
        fn every_fixture_is_tested() {
            let tested = [$($name),*];
            let dir = std::fs::read_dir(data("")).unwrap();
            for entry in dir {
                let name = entry.unwrap().file_name().into_string().unwrap();
                if [".ogg", ".opus", ".oga", ".ogv", ".spx"].iter().any(|e| name.ends_with(e)) {
                    assert!(tested.contains(&name.as_str()), "{name} has no test");
                }
            }
        }
    };
}

fixtures! {
    // Opus, by ffmpeg's libopus.
    packets_of_opus_stereo => "opus_stereo.opus",
    packets_of_opus_mono_silk => "opus_mono_silk.opus",
    packets_of_opus_short_frames => "opus_short_frames.ogg",
    packets_of_opus_51 => "opus_51.opus",
    packets_of_opus_one_page => "opus_one_page.opus",
    packets_of_opus_spanning => "opus_spanning.opus",
    // Vorbis, by libvorbis and by FFmpeg's own encoder.
    packets_of_vorbis_stereo => "vorbis_stereo.ogg",
    packets_of_vorbis_mono_22k => "vorbis_mono_22k.ogg",
    packets_of_vorbis_51 => "vorbis_51.ogg",
    packets_of_vorbis_one_page => "vorbis_one_page.ogg",
    packets_of_vorbis_spanning => "vorbis_spanning.ogg",
    packets_of_vorbis_native => "vorbis_native.ogg",
    // Codecs given untimed, and a film's two streams side by side.
    packets_of_flac => "flac.oga",
    packets_of_speex => "speex.spx",
    packets_of_theora_vorbis => "theora_vorbis.ogv",
    // Chained files, and the links they are made of.
    packets_of_opus_chained => "opus_chained.opus",
    packets_of_opus_chained_link0 => "opus_chained_link0.opus",
    packets_of_opus_chained_link1 => "opus_chained_link1.opus",
    packets_of_vorbis_chained => "vorbis_chained.ogg",
    packets_of_vorbis_chained_link0 => "vorbis_chained_link0.ogg",
    packets_of_vorbis_chained_link1 => "vorbis_chained_link1.ogg",
    packets_of_opus_chained_unlike => "opus_chained_unlike.opus",
    packets_of_opus_unlike_link0 => "opus_unlike_link0.opus",
    packets_of_opus_unlike_link1 => "opus_unlike_link1.opus",
    // Headers and data on one page, as FFmpeg's muxer never writes them.
    packets_of_vorbis_headers_with_data => "vorbis_headers_with_data.ogg",
    packets_of_opus_tags_with_data => "opus_tags_with_data.opus",
}
