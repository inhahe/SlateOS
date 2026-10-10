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
    /// The size it is opened with: what probing found (`.size`).
    size: Option<(u32, u32)>,
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
        let size = std::fs::read_to_string(sibling(&made, ".size"))
            .ok()
            .map(|s| {
                let (w, h) = s.trim().split_once(' ').unwrap();
                (w.parse().unwrap(), h.parse().unwrap())
            });
        Self {
            data,
            packets,
            extradata,
            size,
        }
    }

    /// The decoder that shows the stream, opened as FFmpeg's command line
    /// opens it: holding back `delay` pictures, with the size probing found
    /// and the extradata.
    fn open(&self, delay: u32) -> Decoder {
        let mut d = Decoder::new();
        d.set_reorder_delay(delay);
        if let Some((w, h)) = self.size {
            d.set_container_size(w, h);
        }
        if let Some(extradata) = &self.extradata {
            d.configure(extradata);
        }
        d
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
    let mut d = s.open(delay);
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
    files.retain(|f| {
        let f = f.to_string_lossy().replace('\\', "/");
        only.as_ref().is_none_or(|o| f.contains(o.as_str()))
    });
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
            for (suffix, got) in [
                (".pict", &got_slices),
                (".frames", &got_frames),
                (".mmco", &got_mmco),
            ] {
                let mut text = String::new();
                for l in got {
                    text.push_str(l);
                    text.push('\n');
                }
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
            let parser_b: Vec<bool> = probe
                .next()
                .unwrap_or("")
                .bytes()
                .map(|b| b == b'B')
                .collect();
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

/// Adler-32 as FFmpeg's `framecrc` muxer takes it: `av_adler32_update`
/// started from 0, not the standard 1.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (0u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A picture as FFmpeg's `framecrc` muxer sums it: its planes end to end,
/// cropped, rows unpadded, 16-bit samples little-endian; the size and the
/// Adler-32.
fn picture_sum(p: &crate::Picture) -> String {
    let bytes = picture_bytes(p);
    format!("{}, 0x{:08x}", bytes.len(), adler32(&bytes))
}

/// A picture's planes end to end, as FFmpeg's raw video holds them.
fn picture_bytes(p: &crate::Picture) -> Vec<u8> {
    let mut bytes = Vec::new();
    for i in 0..3 {
        if let Some(v) = p.plane8(i) {
            for row in 0..v.height {
                bytes.extend_from_slice(&v.data[row * v.stride..row * v.stride + v.width]);
            }
        } else if let Some(v) = p.plane16(i) {
            for row in 0..v.height {
                for &s in &v.data[row * v.stride..row * v.stride + v.width] {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
            }
        }
    }
    bytes
}

/// Each picture this decoder shows of a stream, summed, and whether the
/// stream is coded with CABAC -- not yet translated, so not yet compared.
/// With `raw`, the pictures themselves end to end too.
fn sums(s: &Stream, delay: u32, mut raw: Option<&mut Vec<u8>>) -> (Vec<String>, bool, Vec<String>) {
    let mut d = s.open(delay);
    let mut out = Vec::new();
    let mut cabac = false;
    let mut take = |p: crate::Picture, out: &mut Vec<String>| {
        out.push(picture_sum(&p));
        if let Some(raw) = raw.as_deref_mut() {
            raw.extend_from_slice(&picture_bytes(&p));
        }
    };
    for (i, packet) in s.each() {
        if let Ok(Some(p)) = d.decode(packet, i64::try_from(i).unwrap(), 1) {
            take(p, &mut out);
        }
        cabac |= d.h.ps.pps.as_ref().is_some_and(|pps| pps.cabac != 0);
    }
    while let Some(p) = d.drain() {
        take(p, &mut out);
    }
    (out, cabac, std::mem::take(&mut d.h.debug_errors))
}

/// Every picture's samples FFmpeg's, as its `framecrc` sums them
/// (`.crc`, `tools/pict_expect.py --add-crc`). Streams coded with CABAC
/// are counted apart until it is translated.
#[test]
#[ignore = "needs FFmpeg's FATE samples and tools/pict_expect.py's lines: set H264_FATE and H264_PICT"]
fn pictures_are_ffmpegs_sample_for_sample() {
    let fate =
        PathBuf::from(std::env::var("H264_FATE").expect("H264_FATE: the fate-suite directory"));
    let pict =
        PathBuf::from(std::env::var("H264_PICT").expect("H264_PICT: pict_expect.py's output"));
    // H264_ONLY picks the streams whose names contain it, or with a
    // leading `=` the one so named; under H264_DUMP each differing
    // stream's pictures are written raw (`<name>.yuv`), for
    // `tools/yuv_diff.py`.
    let only = std::env::var("H264_ONLY").ok();
    let dump = std::env::var_os("H264_DUMP").map(PathBuf::from);
    let mut files = Vec::new();
    pict_files(&pict, &mut files);
    files.retain(|f| {
        let f = f.to_string_lossy().replace('\\', "/");
        only.as_ref().is_none_or(|o| match o.strip_prefix('=') {
            Some(exact) => f.ends_with(&format!("/{exact}.pict")),
            None => f.contains(o.as_str()),
        })
    });
    files.sort();
    let (mut compared, mut cabac_streams, mut pictures, mut matching) = (0, 0, 0, 0);
    let mut undefined = 0;
    let mut failures = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&pict).unwrap();
        let name = rel
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches(".pict")
            .to_owned();
        let base = pict.join(&name);
        let Ok(want) = std::fs::read_to_string(sibling(&base, ".crc")) else {
            continue;
        };
        let want: Vec<String> = want.lines().map(str::to_owned).collect();
        let stream = Stream::load(&fate, &pict, &name);
        let delay = number(&sibling(&base, ".delay")).unwrap_or(0);
        let mut raw = Vec::new();
        let (mut got, cabac, errors) = sums(&stream, delay, dump.as_ref().map(|_| &mut raw));
        if cabac {
            cabac_streams += 1;
            continue;
        }
        // `<size>, ?`: a picture with samples FFmpeg never wrote, which it
        // leaves undefined -- any of that size will do.
        for (w, g) in want.iter().zip(&mut got) {
            if let Some(size) = w.strip_suffix(", ?") {
                undefined += 1;
                if g.split(", ").next() == Some(size) {
                    g.clone_from(w);
                }
            }
        }
        compared += 1;
        pictures += want.len();
        matching += want.iter().zip(&got).filter(|(w, g)| w == g).count();
        if let Some(d) = first_difference("picture", &want, &got) {
            let why = errors.first().map_or(String::new(), |e| {
                format!(" ({} slices failed; first: {e})", errors.len())
            });
            failures.push(format!("{name}: {d}{why}"));
            if let Some(dump) = &dump {
                let to = sibling(&dump.join(&name), ".yuv");
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::write(to, &raw).unwrap();
            }
        }
    }
    let report = failures.join("\n");
    println!(
        "{compared} streams compared, {} differ; {matching} of {pictures} pictures match \
         ({undefined} of them only in size, FFmpeg leaving their samples undefined); \
         {cabac_streams} CABAC streams not compared",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {compared} streams differ ({matching} of {pictures} pictures match; {cabac_streams} \
         CABAC streams not compared):\n{report}",
        failures.len()
    );
}
