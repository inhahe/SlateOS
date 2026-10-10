//! The decoder's handling of pictures held to FFmpeg's: fed the packets
//! FFmpeg's demuxer splits each stream into, every slice's `-debug pict`
//! line -- its picture's frame number and order counts, its reference
//! counts, QP, loop filter and weights -- every `-debug mmco` line -- the
//! reference lists and each marking operation -- and every picture shown,
//! in order, as ffprobe describes it, must be FFmpeg's; and decoding as far
//! as FFmpeg's probe did, the way it did, must leave the decoder holding
//! back as many pictures as the probe found to.
//!
//! The streams are FFmpeg's FATE samples -- every conformance stream and
//! the other H.264 samples -- streams made with libx264, and those cut
//! short at the front or with fields taken out (how a decoder joins a
//! stream late, recovers, and keeps fields of different frames apart).
//! The samples are too large to commit; `tools/pict_expect.py` makes the
//! rest and writes FFmpeg's lines for all of them. With both on disk:
//!
//! ```text
//! H264_FATE=<fate-suite> H264_PICT=<pict_expect.py's output> \
//!   cargo test -p h264 --target x86_64-pc-windows-gnu --lib -- --ignored pictures
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::Decoder;

/// Every `.pict` file under `dir`.
fn pict_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            pict_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "pict") {
            out.push(path);
        }
    }
}

/// `path` with `suffix` added to its name: `a.jsv` and `.pict` give
/// `a.jsv.pict`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// FFmpeg's picture type letter (`av_get_picture_type_char`).
fn type_char(t: i32) -> char {
    use crate::slice::{TYPE_B, TYPE_I, TYPE_P, TYPE_SI, TYPE_SP};
    match t {
        TYPE_I => 'I',
        TYPE_P => 'P',
        TYPE_B => 'B',
        TYPE_SI => 'i',
        TYPE_SP => 'p',
        _ => '?',
    }
}

/// A stream as FFmpeg's demuxer hands it to the decoder.
struct Stream {
    /// The packets end to end.
    data: Vec<u8>,
    /// Each packet's size, and its position as ffprobe prints it.
    packets: Vec<(usize, String)>,
    /// What the decoder that shows the stream is opened with.
    extradata: Option<Vec<u8>>,
}

impl Stream {
    /// The stream `name`: a container's packets as `pict_expect.py` wrote
    /// them, a stream it made, or a FATE sample.
    fn load(fate: &Path, pict: &Path, name: &str) -> Self {
        let made = pict.join(name);
        let data_file = sibling(&made, ".data");
        let data = if data_file.exists() {
            std::fs::read(data_file).unwrap()
        } else if made.exists() {
            std::fs::read(&made).unwrap()
        } else {
            std::fs::read(fate.join(name)).unwrap()
        };
        let packets = lines(&sibling(&made, ".packets"))
            .iter()
            .map(|l| {
                let (size, pos) = l.split_once(' ').unwrap();
                (size.parse().unwrap(), pos.to_owned())
            })
            .collect();
        let extradata = std::fs::read(sibling(&made, ".extradata")).ok();
        Self {
            data,
            packets,
            extradata,
        }
    }

    /// Each packet in turn with its index.
    fn each(&self) -> impl Iterator<Item = (usize, &[u8])> {
        let mut at = 0;
        self.packets.iter().enumerate().map(move |(i, (size, _))| {
            let packet = &self.data[at..at + size];
            at += size;
            (i, packet)
        })
    }
}

/// What this decoder makes of a stream, started as FFmpeg starts the
/// decoder that shows it -- holding back `delay` pictures, opened with the
/// extradata: each slice's line, each picture handed out as ffprobe
/// describes one -- the position of the packet that began it, its type,
/// and whether it is a key frame, interlaced, top field first, and how many
/// fields it repeats -- and each marking line.
fn run(s: &Stream, delay: u32) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut d = Decoder::new();
    d.set_reorder_delay(delay);
    if let Some(extradata) = &s.extradata {
        d.configure(extradata);
    }
    let mut frames = Vec::new();
    let row = |p: crate::Picture| {
        let o = &p.out;
        let pos = &s.packets[usize::try_from(o.props.time).unwrap()].1;
        format!(
            "{pos} {} {} {} {} {}",
            type_char(o.props.pict_type),
            i32::from(o.props.key),
            i32::from(o.props.interlaced),
            i32::from(o.props.top_field_first),
            o.props.repeat_pict
        )
    };
    for (i, packet) in s.each() {
        // A packet with no picture in it is FFmpeg's "no frame!" error,
        // which the stream goes on past.
        if let Ok(Some(p)) = d.decode(packet, i64::try_from(i).unwrap(), 1) {
            frames.push(row(p));
        }
    }
    while let Some(p) = d.drain() {
        frames.push(row(p));
    }
    (
        std::mem::take(&mut d.h.debug),
        frames,
        std::mem::take(&mut d.h.debug_mmco),
    )
}

/// How many pictures a decoder holds back after probing a raw stream as
/// FFmpeg probes it (`avformat_find_stream_info`): opened with no
/// extradata and holding none back, offered each of the `read` packets
/// probing reads -- each of `parser_b` that the parser takes for a B
/// picture first setting the delay to 1 if it is 0 (`compute_pkt_fields`)
/// -- and decoding each until it knows the stream's size and format and
/// takes its delay as known (`try_decode_frame`).
fn probe_delay(s: &Stream, read: usize, parser_b: &[bool]) -> i32 {
    let mut d = Decoder::new();
    let mut decoded = 0;
    for (i, packet) in s.each().take(read) {
        if parser_b[i] && d.h.avctx.has_b_frames == 0 {
            d.h.avctx.has_b_frames = 1;
        }
        let parameters = d.h.avctx.width != 0 && d.h.avctx.pix_fmt != crate::PixelFormat::None;
        if parameters && delay_guessed(&d, decoded) {
            continue;
        }
        if let Ok(Some(_)) = d.decode(packet, i64::try_from(i).unwrap(), 1) {
            decoded += 1;
        }
    }
    d.h.avctx.has_b_frames
}

/// Whether probing takes the stream's delay as known once `decoded`
/// pictures are out (`has_decode_delay_been_guessed`): when the delay is
/// what the sequence parameter set in use says, or after enough pictures
/// for the delay so far.
fn delay_guessed(d: &Decoder, decoded: usize) -> bool {
    let delay = d.h.avctx.has_b_frames;
    let reorder = d.h.ps.sps.as_ref().map_or(0, |sps| sps.num_reorder_frames);
    if delay != 0 && reorder == delay {
        return true;
    }
    decoded
        >= match delay {
            ..3 => 7,
            3 => 18,
            _ => 20,
        }
}

/// The first line where `got` is not `want`, described.
fn first_difference(what: &str, want: &[String], got: &[String]) -> Option<String> {
    (0..want.len().max(got.len()))
        .find(|&k| want.get(k) != got.get(k))
        .map(|k| {
            format!(
                "{what} {k} of {}: FFmpeg {:?}, here {:?}",
                want.len(),
                want.get(k),
                got.get(k)
            )
        })
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The one number in a file.
fn number<T: std::str::FromStr>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.trim()
            .parse()
            .unwrap_or_else(|_| panic!("{}: not a number", path.display())),
    )
}

#[test]
#[ignore = "needs FFmpeg's FATE samples and tools/pict_expect.py's lines: set H264_FATE and H264_PICT"]
fn pictures_are_set_up_and_shown_as_ffmpeg_does() {
    let fate =
        PathBuf::from(std::env::var("H264_FATE").expect("H264_FATE: the fate-suite directory"));
    let pict =
        PathBuf::from(std::env::var("H264_PICT").expect("H264_PICT: pict_expect.py's output"));
    // To look into a difference: only the streams whose names hold
    // H264_ONLY, and this decoder's lines written under H264_DUMP beside
    // where FFmpeg's are, to diff.
    let only = std::env::var("H264_ONLY").ok();
    let dump = std::env::var_os("H264_DUMP").map(PathBuf::from);
    let mut files = Vec::new();
    pict_files(&pict, &mut files);
    files.retain(|f| only.as_ref().is_none_or(|o| f.to_string_lossy().contains(o.as_str())));
    files.sort();
    assert!(!files.is_empty(), "no .pict files under {}", pict.display());

    let mut failures = Vec::new();
    let (mut slices, mut shown, mut probes) = (0, 0, 0);
    for file in &files {
        let rel = file.strip_prefix(&pict).unwrap();
        let name = rel
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches(".pict")
            .to_owned();
        let base = pict.join(&name);
        let stream = Stream::load(&fate, &pict, &name);
        let delay = number(&sibling(&base, ".delay")).unwrap_or(0);
        let (got_slices, got_frames, got_mmco) = run(&stream, delay);
        if let Some(dump) = &dump {
            let to = dump.join(&name);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            for (suffix, got) in [(".pict", &got_slices), (".frames", &got_frames), (".mmco", &got_mmco)] {
                let text: String = got.iter().map(|l| format!("{l}\n")).collect();
                std::fs::write(sibling(&to, suffix), text).unwrap();
            }
        }
        let want_slices = lines(file);
        let want_frames = lines(&sibling(&base, ".frames"));
        let want_mmco = lines(&sibling(&base, ".mmco"));
        slices += want_slices.len();
        shown += want_frames.len();
        let mut found = vec![
            first_difference("slice", &want_slices, &got_slices),
            first_difference("picture shown", &want_frames, &got_frames),
            first_difference("marking line", &want_mmco, &got_mmco),
        ];
        if let Ok(probe) = std::fs::read_to_string(sibling(&base, ".probe")) {
            probes += 1;
            let mut probe = probe.lines();
            let read: usize = probe.next().unwrap().parse().unwrap();
            let parser_b: Vec<bool> = probe.next().unwrap_or("").bytes().map(|b| b == b'B').collect();
            let got = probe_delay(&stream, read, &parser_b);
            if i64::from(got) != i64::from(delay) {
                found.push(Some(format!(
                    "probed over {read} packets: FFmpeg holds back {delay}, here {got}"
                )));
            }
        }
        failures.extend(found.into_iter().flatten().map(|d| format!("{name}: {d}")));
    }
    let report = failures.join("\n");
    assert!(
        failures.is_empty(),
        "{} differences over {} streams ({slices} slices, {shown} pictures shown, {probes} \
         probes):\n{report}",
        failures.len(),
        files.len()
    );
}
