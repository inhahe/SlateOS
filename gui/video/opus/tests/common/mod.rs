//! What the tests share: the `.bit` format, the loss pattern, the digest,
//! and the decode loop -- each the same as `tools/references.py` and
//! `tools/reference.c` have it, so that a digest here and one there are of
//! the same thing.

#![allow(
    dead_code,
    reason = "each test binary uses its own part of this module"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "test helpers: a failure should be loud, and their sizes are small"
)]

use opus::{AnyDecoder, Bandwidth, Decoder, Head, packet_has_lbrr};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The directory of the test streams.
pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// One packet of a `.bit` file (opus_demo's format: a 4-byte big-endian
/// length, the encoder's 4-byte final range, the packet).
pub struct Packet<'a> {
    pub range: u32,
    pub data: &'a [u8],
}

/// The packets of a `.bit` file, a short last one cut to what is there.
pub fn packets(bit: &[u8]) -> Vec<Packet<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= bit.len() {
        let len = u32::from_be_bytes(bit[at..at + 4].try_into().expect("4 bytes")) as usize;
        let range = u32::from_be_bytes(bit[at + 4..at + 8].try_into().expect("4 bytes"));
        at += 8;
        out.push(Packet {
            range,
            data: &bit[at..(at + len).min(bit.len())],
        });
        at += len;
    }
    out
}

/// FNV-1a, 64 bits: the digest the reference file holds.
pub fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// FNV-1a, 32 bits: a stream's loss seed.
pub fn fnv1a32(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The loss pattern's seed for a stream: an RFC vector's number times 7919,
/// any other stream's name's FNV-1a.
pub fn loss_seed(name: &str) -> u64 {
    name.strip_prefix("testvector").map_or_else(
        || u64::from(fnv1a32(name.as_bytes())),
        |n| n.parse::<u64>().expect("a vector's number") * 7919,
    )
}

/// `bit` with packets marked lost (a length of 0, the range kept), in
/// bursts of 1 to 8, none of the first three: `tools/references.py`'s
/// `lossy`.
pub fn lossy(bit: &[u8], mut seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(bit.len());
    let mut burst = 0u64;
    for (i, p) in packets(bit).iter().enumerate() {
        let total = i + 1;
        seed = (seed * 1_103_515_245 + 12_345) & 0x7FFF_FFFF;
        if burst == 0 && seed % 100 < 6 {
            burst = 1 + (seed >> 8) % 8;
        }
        if burst > 0 && total > 3 {
            burst -= 1;
            out.extend(0u32.to_be_bytes());
            out.extend(p.range.to_be_bytes());
        } else {
            if total <= 3 {
                burst = burst.saturating_sub(1);
            }
            out.extend((p.data.len() as u32).to_be_bytes());
            out.extend(p.range.to_be_bytes());
            out.extend(p.data);
        }
    }
    out
}

/// The damage pattern's generator: 15 bits a draw.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> usize {
        self.0 = (self.0 * 1_103_515_245 + 12_345) & 0x7FFF_FFFF;
        (self.0 >> 16) as usize
    }
}

/// The damage pattern's seed for a stream.
pub fn damage_seed(name: &str) -> u64 {
    u64::from(fnv1a32(format!("{name}/damage").as_bytes()))
}

/// The odd calls' seed for a stream.
pub fn odd_seed(name: &str) -> u64 {
    u64::from(fnv1a32(format!("{name}/odd").as_bytes()))
}

/// `bit` with packets damaged -- bits flipped, cut short (to nothing:
/// lost; or to a few bytes), the payload made random, the TOC changed,
/// random bytes added -- and every final range 0: `tools/references.py`'s
/// `damaged`.
pub fn damaged(bit: &[u8], seed: u64) -> Vec<u8> {
    let mut rng = Lcg(seed);
    let mut out = Vec::with_capacity(bit.len());
    for packet in packets(bit) {
        let mut p = packet.data.to_vec();
        match rng.next() % 12 {
            0..=2 if !p.is_empty() => {
                for _ in 0..=rng.next() % 4 {
                    let i = rng.next() % p.len();
                    p[i] ^= 1 << (rng.next() % 8);
                }
            }
            3 => {
                let keep = rng.next() % (p.len() + 1);
                p.truncate(keep);
            }
            4 if !p.is_empty() => {
                for b in p.iter_mut().skip(1) {
                    *b = rng.next() as u8;
                }
            }
            5 if !p.is_empty() => p[0] = rng.next() as u8,
            6 => {
                let n = 1 + rng.next() % 16;
                for _ in 0..n {
                    p.push(rng.next() as u8);
                }
                p.truncate(1500);
            }
            // Cut to 1 to 8 bytes: shorter than a multistream packet's
            // self-delimited lengths can be.
            7 => {
                let keep = 1 + rng.next() % 8;
                p.truncate(keep);
            }
            _ => {}
        }
        out.extend((p.len() as u32).to_be_bytes());
        out.extend(0u32.to_be_bytes());
        out.extend(&p);
    }
    out
}

/// The `@random` stream: every TOC, with random payloads of 0 to 23 bytes
/// and a few longer -- `tools/references.py`'s `random_packets`.
pub fn random_packets() -> Vec<u8> {
    let mut rng = Lcg(1);
    let mut out = Vec::new();
    for toc in 0..=255u8 {
        for len in (0..24).chain([40, 64, 100, 300, 1000, 1275]) {
            out.extend((1 + len as u32).to_be_bytes());
            out.extend(0u32.to_be_bytes());
            out.push(toc);
            for _ in 0..len {
                out.push(rng.next() as u8);
            }
        }
    }
    out
}

/// A reference decode: a line of a reference file.
#[derive(Clone, Debug)]
pub struct Reference {
    pub name: String,
    pub rate: u32,
    pub channels: usize,
    pub variant: String,
    pub bytes: usize,
    pub digest: u64,
    /// The decode calls that failed or made nothing, and a digest of
    /// `PACKET:CODE;` for each.
    pub errors: usize,
    pub errors_digest: u32,
    /// The digest of the decoder's state after each packet,
    /// `BANDWIDTH:PITCH:DURATION:RANGE;`.
    pub state_digest: u32,
}

impl Reference {
    /// The name `tools/references.py --keep` gives its output.
    pub fn file_name(&self) -> String {
        format!(
            "{}_{}_{}_{}.pcm",
            self.name, self.rate, self.channels, self.variant
        )
    }
}

/// A reference file's lines.
pub fn references(file: &Path) -> Vec<Reference> {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(f.len(), 9, "a reference line: {l}");
            Reference {
                name: f[0].to_owned(),
                rate: f[1].parse().expect("a rate"),
                channels: f[2].parse().expect("a channel count"),
                variant: f[3].to_owned(),
                bytes: f[4].parse().expect("a length"),
                digest: u64::from_str_radix(f[5], 16).expect("a digest"),
                errors: f[6].parse().expect("a count"),
                errors_digest: u32::from_str_radix(f[7], 16).expect("a digest"),
                state_digest: u32::from_str_radix(f[8], 16).expect("a digest"),
            }
        })
        .collect()
}

/// What a variant does to the stream before it is decoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Damage {
    #[default]
    None,
    Lost,
    Damaged,
}

/// How a variant decodes: `plain`, `lossy` or `damaged`, then any of
/// `gain=N`, `noinv`, `float`, `reset=N`, `odd`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    pub damage: Damage,
    pub gain: Option<i32>,
    pub noinv: bool,
    pub float: bool,
    /// Reset the decoder before every Nth packet.
    pub reset: Option<usize>,
    /// Odd calls between packets.
    pub odd: bool,
}

pub fn settings(variant: &str) -> Settings {
    let mut tokens = variant.split('+');
    let mut s = Settings {
        damage: match tokens.next() {
            Some("plain") => Damage::None,
            Some("lossy") => Damage::Lost,
            Some("damaged") => Damage::Damaged,
            other => panic!("a variant starts plain, lossy or damaged, not {other:?}"),
        },
        ..Settings::default()
    };
    for t in tokens {
        match t {
            "noinv" => s.noinv = true,
            "float" => s.float = true,
            "odd" => s.odd = true,
            _ => {
                if let Some(n) = t.strip_prefix("reset=") {
                    s.reset = Some(n.parse().unwrap_or_else(|_| panic!("variant {t}")));
                } else {
                    s.gain = Some(
                        t.strip_prefix("gain=")
                            .and_then(|g| g.parse().ok())
                            .unwrap_or_else(|| panic!("variant {t}")),
                    );
                }
            }
        }
    }
    s
}

/// What a decode made: its samples' bytes (16-bit or 32-bit float,
/// little-endian); the decode calls that failed or made nothing, as
/// `PACKET:CODE;` (libopus's codes, 0 for no samples); and the decoder's
/// state after each packet, as `BANDWIDTH:PITCH:DURATION:RANGE;`.
pub struct Decoded {
    pub bytes: Vec<u8>,
    pub errors: Vec<String>,
    pub state: String,
}

/// `OPUS_GET_BANDWIDTH`'s number for a bandwidth: 0 for none yet.
fn bandwidth_code(b: Option<Bandwidth>) -> i32 {
    match b {
        None => 0,
        Some(Bandwidth::Narrowband) => 1101,
        Some(Bandwidth::Mediumband) => 1102,
        Some(Bandwidth::Wideband) => 1103,
        Some(Bandwidth::Superwideband) => 1104,
        Some(Bandwidth::Fullband) => 1105,
    }
}

/// One decode call, its samples appended to `out` or its failure noted.
struct Calls<'a> {
    dec: &'a mut AnyDecoder,
    float: bool,
    pcm: Vec<i16>,
    pcm_float: Vec<f32>,
    out: Vec<u8>,
    errors: Vec<String>,
}

impl Calls<'_> {
    fn call(&mut self, count: usize, packet: Option<&[u8]>, n: usize, fec: bool) {
        let channels = self.dec.channels();
        let result = if self.float {
            self.dec
                .decode_float(packet, &mut self.pcm_float[..n * channels], fec)
        } else {
            self.dec.decode(packet, &mut self.pcm[..n * channels], fec)
        };
        match result {
            Ok(0) => self.errors.push(format!("{count}:0;")),
            Ok(n) if self.float => {
                for s in &self.pcm_float[..n * channels] {
                    self.out.extend_from_slice(&s.to_le_bytes());
                }
            }
            Ok(n) => {
                for s in &self.pcm[..n * channels] {
                    self.out.extend_from_slice(&s.to_le_bytes());
                }
            }
            Err(e) => self.errors.push(format!("{count}:{};", e.code())),
        }
    }
}

/// A length of concealment or FEC for `odd`'s calls: up to 49 steps of 2.5
/// ms, a quarter of the time a little off the grid (`tools/reference.c`'s
/// `odd_length`).
fn odd_length(rng: &mut Lcg, rate: usize) -> usize {
    let step = rate / 400;
    let mut n = step * (rng.next() % 50);
    if rng.next().is_multiple_of(4) {
        n += rng.next() % step;
    }
    n
}

/// `bit` decoded as `tools/reference.c` decodes it -- for a single stream,
/// `opus_demo -d`'s loop: each packet lost (of length 0) concealed when the
/// next arrives, the last of a run of them from that one's in-band FEC if it
/// has some (a multistream decoder always asks for it), then that one decoded
/// into room for two seconds; and the decoder's final range held to the
/// encoder's after each packet decoded whole.
pub fn decode(
    dec: &mut AnyDecoder,
    bit: &[u8],
    multistream: bool,
    settings: &Settings,
    name: &str,
    rate: u32,
) -> Result<Decoded, String> {
    const MAX_PACKET: usize = 1500;
    const MAX_FRAME: usize = 48000 * 2;
    let channels = dec.channels();
    let float = settings.float;
    let mut calls = Calls {
        dec,
        float,
        pcm: vec![0i16; MAX_FRAME * channels],
        pcm_float: vec![0f32; if float { MAX_FRAME * channels } else { 0 }],
        out: Vec::new(),
        errors: Vec::new(),
    };
    let mut state = String::new();
    let mut odd = Lcg(odd_seed(name));
    let mut lost_count = 0usize;
    let mut lost_prev = true;
    let mut at = 0usize;
    let mut count = 0usize;
    while at + 8 <= bit.len() {
        let len = u32::from_be_bytes(bit[at..at + 4].try_into().expect("4 bytes")) as usize;
        let enc_final_range = u32::from_be_bytes(bit[at + 4..at + 8].try_into().expect("4 bytes"));
        at += 8;
        if len > MAX_PACKET || at + len > bit.len() {
            break;
        }
        let data = &bit[at..at + len];
        at += len;
        if let Some(every) = settings.reset {
            if count > 0 && count.is_multiple_of(every) {
                calls.dec.reset();
            }
        }
        let lost = len == 0;
        if settings.odd && !lost {
            // One time in six concealment of an odd length, one in six FEC of
            // an odd length from this packet, one in six this packet into a
            // buffer of any size (`tools/reference.c`'s `odd_call`).
            match odd.next() % 6 {
                0 => {
                    let n = odd_length(&mut odd, rate as usize);
                    calls.call(count, None, n, false);
                }
                1 => {
                    let n = odd_length(&mut odd, rate as usize);
                    calls.call(count, Some(data), n, true);
                }
                2 => {
                    let n = odd.next() % (MAX_FRAME + 1);
                    calls.call(count, Some(data), n, false);
                }
                _ => {}
            }
        }
        let run_decoder = if lost { 0 } else { 1 + lost_count };
        if lost {
            lost_count += 1;
        }
        for fr in 0..run_decoder {
            // opus_demo's `opus_packet_has_lbrr` test: an error is true in C.
            let fec_here = lost_count > 0
                && fr == lost_count - 1
                && (multistream || packet_has_lbrr(data).unwrap_or(true));
            if fec_here {
                let n = calls.dec.last_packet_duration();
                calls.call(count, Some(data), n, true);
            } else if fr < lost_count {
                let n = calls.dec.last_packet_duration();
                calls.call(count, None, n, false);
            } else {
                calls.call(count, Some(data), MAX_FRAME, false);
            }
        }
        let dec_final_range = calls.dec.final_range();
        if enc_final_range != 0 && !lost && !lost_prev && dec_final_range != enc_final_range {
            return Err(format!(
                "packet {count}: final range {dec_final_range:#010x}, the encoder's {enc_final_range:#010x}"
            ));
        }
        let pitch = match &*calls.dec {
            AnyDecoder::Single(d) => d.pitch(),
            _ => 0,
        };
        state.push_str(&format!(
            "{}:{pitch}:{}:{dec_final_range};",
            bandwidth_code(calls.dec.bandwidth()),
            calls.dec.last_packet_duration()
        ));
        lost_prev = lost;
        if !lost {
            lost_count = 0;
        }
        count += 1;
    }
    Ok(Decoded {
        bytes: calls.out,
        errors: calls.errors,
        state,
    })
}

/// A stream's files: its packets, and its `OpusHead` if it is a multistream
/// one.
pub struct Stream {
    pub bit: Vec<u8>,
    pub head: Option<Vec<u8>>,
}

/// Every stream a reference file names, read from `dir`.
pub fn streams(dir: &Path, refs: &[Reference]) -> HashMap<String, Stream> {
    let mut out = HashMap::new();
    for r in refs {
        if out.contains_key(&r.name) {
            continue;
        }
        if r.name == "@random" {
            out.insert(
                r.name.clone(),
                Stream {
                    bit: random_packets(),
                    head: None,
                },
            );
            continue;
        }
        let bit = std::fs::read(dir.join(format!("{}.bit", r.name)))
            .unwrap_or_else(|e| panic!("{}.bit: {e}", r.name));
        let head = std::fs::read(dir.join(format!("{}.head", r.name))).ok();
        out.insert(r.name.clone(), Stream { bit, head });
    }
    out
}

/// One reference decode made again: `None` if it matches, else what went
/// wrong.
pub fn check(r: &Reference, stream: &Stream, keep: Option<&Path>) -> Option<String> {
    let s = settings(&r.variant);
    let mut dec = match &stream.head {
        Some(h) => {
            let head = Head::parse(h).expect("the stream's OpusHead");
            assert_eq!(head.channels, r.channels, "{}: the head's channels", r.name);
            head.decoder(r.rate).expect("a decoder")
        }
        None => AnyDecoder::Single(Box::new(
            Decoder::new(r.rate, r.channels).expect("a decoder"),
        )),
    };
    if let Some(g) = s.gain {
        dec.set_gain(g).expect("a gain in range");
    }
    if s.noinv {
        dec.set_phase_inversion_disabled(true);
    }
    let bit = match s.damage {
        Damage::None => stream.bit.clone(),
        Damage::Lost => lossy(&stream.bit, loss_seed(&r.name)),
        Damage::Damaged => damaged(&stream.bit, damage_seed(&r.name)),
    };
    let decoded = match decode(&mut dec, &bit, stream.head.is_some(), &s, &r.name, r.rate) {
        Ok(d) => d,
        Err(e) => return Some(e),
    };
    let errors = decoded.errors.concat();
    let errors_match =
        decoded.errors.len() == r.errors && fnv1a32(errors.as_bytes()) == r.errors_digest;
    let state_match = fnv1a32(decoded.state.as_bytes()) == r.state_digest;
    if decoded.bytes.len() == r.bytes
        && fnv1a64(&decoded.bytes) == r.digest
        && errors_match
        && state_match
    {
        return None;
    }
    let mut why = format!("{} bytes, libopus {}", decoded.bytes.len(), r.bytes);
    if !errors_match {
        why = format!(
            "{why}; {} failed decode calls, libopus {} (or others)",
            decoded.errors.len(),
            r.errors
        );
    }
    if !state_match {
        why = format!("{why}; the decoder's state differs");
        // `tools/references.py --keep` writes libopus's, a packet a line.
        if let Some(theirs) = keep.and_then(|dir| {
            std::fs::read_to_string(dir.join(r.file_name().replace(".pcm", ".state"))).ok()
        }) {
            if let Some((i, (a, b))) = decoded
                .state
                .split(';')
                .zip(theirs.lines())
                .enumerate()
                .find(|(_, (a, b))| a != b)
            {
                why = format!("{why} first after packet {i}: {a}, libopus {b}");
            }
        }
    }
    if let Some(dir) = keep {
        if let Ok(theirs) = std::fs::read(dir.join(r.file_name())) {
            let size = if s.float { 4 } else { 2 };
            if let Some(i) = decoded
                .bytes
                .chunks(size)
                .zip(theirs.chunks(size))
                .position(|(a, b)| a != b)
            {
                let sample = |b: &[u8]| {
                    let b = &b[i * size..(i + 1) * size];
                    if s.float {
                        format!("{}", f32::from_le_bytes(b.try_into().expect("4 bytes")))
                    } else {
                        format!("{}", i16::from_le_bytes(b.try_into().expect("2 bytes")))
                    }
                };
                why = format!(
                    "{why}; first difference at sample {i} (frame {}, channel {}): {}, libopus {}",
                    i / r.channels,
                    i % r.channels,
                    sample(&decoded.bytes),
                    sample(&theirs)
                );
            }
        }
    }
    if !decoded.errors.is_empty() {
        let shown: String = decoded.errors.iter().take(12).map(String::as_str).collect();
        why = format!(
            "{why} [{shown}{}]",
            if decoded.errors.len() > 12 { "..." } else { "" }
        );
    }
    Some(why)
}

/// Every reference in `refs` made again, on every core: the failures.
pub fn check_all(refs: &[Reference], dir: &Path) -> Vec<String> {
    let streams = streams(dir, refs);
    let keep = std::env::var_os("OPUS_REFERENCE").map(PathBuf::from);
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut failures = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(r) = refs.get(i) else { break };
                        if let Some(why) = check(r, &streams[&r.name], keep.as_deref()) {
                            failures.push(format!(
                                "{} {} {} {}: {why}",
                                r.name, r.rate, r.channels, r.variant
                            ));
                        }
                    }
                    failures
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a worker"))
            .collect()
    })
}
