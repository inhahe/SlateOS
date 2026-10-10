//! What the tests share: reading an Ogg file's packets, the digests, and
//! the damage generator -- each as `tools/reference.c` does it, so that a
//! summary line here and one from Tremor can be compared as text.

#![allow(dead_code, reason = "each test file uses its own part")]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc,
    reason = "test support: a panic is a failed test"
)]

use std::path::PathBuf;

use vorbis::Decoder;

/// A fixture's path.
pub fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// Every packet of the file's first logical stream, in order (pages of
/// other streams are skipped; CRCs are not checked -- the fixtures are
/// whole).
pub fn ogg_packets(file: &[u8]) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut partial = Vec::new();
    let mut serial = None;
    let mut pos = 0;
    while pos + 27 <= file.len() {
        if &file[pos..pos + 4] != b"OggS" {
            pos += 1;
            continue;
        }
        let continued = file[pos + 5] & 1 != 0;
        let s = u32::from_le_bytes(file[pos + 14..pos + 18].try_into().unwrap());
        let segments = usize::from(file[pos + 26]);
        let lacing = &file[pos + 27..pos + 27 + segments];
        let mut body = pos + 27 + segments;
        let size: usize = lacing.iter().map(|&l| usize::from(l)).sum();
        if *serial.get_or_insert(s) == s {
            if !continued {
                partial.clear();
            }
            for &l in lacing {
                let l = usize::from(l);
                partial.extend_from_slice(&file[body..body + l]);
                body += l;
                if l < 255 {
                    packets.push(std::mem::take(&mut partial));
                }
            }
        }
        pos += 27 + segments + size;
    }
    packets
}

pub const FNV64: u64 = 0xcbf2_9ce4_8422_2325;

pub fn fnv64(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn fnv32(mut h: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// `reference.c`'s damage generator.
pub struct Lcg(pub u32);

impl Lcg {
    pub fn draw(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.0 >> 16
    }
}

/// What `damage` did to a packet.
pub enum Damage {
    Dropped,
    Restart,
    Kept,
}

/// `reference.c`'s `damage`, draw for draw.
pub fn damage(lcg: &mut Lcg, p: &mut Vec<u8>, cap: usize) -> Damage {
    let len = |p: &Vec<u8>| p.len() as u32;
    match lcg.draw() % 16 {
        0 => return Damage::Dropped,
        1 => {
            let n = lcg.draw() % (len(p) + 1);
            p.truncate(n as usize);
        }
        2 => {
            if !p.is_empty() {
                let i = lcg.draw() % len(p);
                p[i as usize] ^= 1 << (lcg.draw() % 8);
            }
        }
        3 => {
            let n = lcg.draw() % 8 + 1;
            for _ in 0..n {
                if p.is_empty() {
                    break;
                }
                let i = lcg.draw() % len(p);
                p[i as usize] ^= 1 << (lcg.draw() % 8);
            }
        }
        4 => {
            for b in p.iter_mut() {
                *b = (lcg.draw() & 0xff) as u8;
            }
        }
        5 => {
            let i = if p.is_empty() { 0 } else { lcg.draw() % len(p) };
            for b in &mut p[i as usize..] {
                *b = 0;
            }
        }
        6 => {
            let n = lcg.draw() % 16 + 1;
            for _ in 0..n {
                if p.len() >= cap {
                    break;
                }
                p.push((lcg.draw() & 0xff) as u8);
            }
        }
        7 => p.clear(),
        8 => return Damage::Restart,
        _ => {}
    }
    Damage::Kept
}

/// `reference.c`'s `damage_setup`, draw for draw.
pub fn damage_setup(lcg: &mut Lcg, p: &mut Vec<u8>) {
    let action = lcg.draw() % 10;
    let n = if action < 5 {
        1
    } else if action < 8 {
        lcg.draw() % 3 + 2
    } else {
        let len = if p.is_empty() {
            0
        } else {
            lcg.draw() % p.len() as u32
        };
        p.truncate(len as usize);
        return;
    };
    for _ in 0..n {
        if p.is_empty() {
            break;
        }
        let i = lcg.draw() % p.len() as u32;
        p[i as usize] ^= 1 << (lcg.draw() % 8);
    }
}

/// `reference.c`'s `run`: the audio packets decoded in order (damaged
/// first, from `seed`, if there is one; only the first `limit`, if there
/// is a limit), as its summary line; and, with `verbose`, its line a
/// packet.
pub fn run(packets: &[Vec<u8>], seed: Option<u32>, verbose: bool) -> (String, Vec<String>) {
    run_limited(packets, seed, verbose, None)
}

pub fn run_limited(
    packets: &[Vec<u8>],
    seed: Option<u32>,
    verbose: bool,
    limit: Option<usize>,
) -> (String, Vec<String>) {
    let mut dec = Decoder::new(&packets[0], &packets[2]).unwrap();
    let last = limit.map_or(packets.len(), |l| (l + 3).min(packets.len()));
    let channels = dec.info().channels;
    let mut out = vec![0i32; dec.max_samples() * channels];
    let (mut h32, mut h16) = (FNV64, FNV64);
    let (mut errors, mut samples, mut decoded) = (0u64, 0u64, 0u64);
    let mut lines = Vec::new();
    let mut lcg = Lcg(seed.unwrap_or(0));
    let cap = 1 << 20;
    for (k, packet) in packets.iter().enumerate().take(last).skip(3) {
        let mut p = packet.clone();
        if seed.is_some() {
            p.truncate(cap - 64);
            match damage(&mut lcg, &mut p, cap) {
                Damage::Dropped => continue,
                Damage::Restart => dec.reset(),
                Damage::Kept => {}
            }
        }
        let (r, n) = match dec.decode_i32(&p, &mut out) {
            Ok(n) => {
                decoded += 1;
                (0, n)
            }
            Err(e) => {
                errors += 1;
                (e.code(), 0)
            }
        };
        let mut ph = 0x811c_9dc5_u32;
        for &v in &out[..n * channels] {
            let b = v.to_le_bytes();
            h32 = fnv64(h32, &b);
            ph = fnv32(ph, &b);
            let w = (v >> 9).clamp(-32768, 32767) as i16;
            h16 = fnv64(h16, &w.to_le_bytes());
        }
        let mut rb = [0u8; 8];
        rb[..4].copy_from_slice(&r.to_le_bytes());
        rb[4..].copy_from_slice(&(n as i32).to_le_bytes());
        h32 = fnv64(h32, &rb);
        h16 = fnv64(h16, &rb);
        samples += n as u64;
        if verbose {
            lines.push(format!("{k} {r} {n} {ph:08x}"));
        }
    }
    let summary = format!(
        "packets {} decoded {decoded} errors {errors} samples {samples} i32 {h32:016x} i16 {h16:016x}",
        last - 3
    );
    (summary, lines)
}
