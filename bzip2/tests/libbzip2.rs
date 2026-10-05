//! This crate against libbzip2 1.0.8 itself: every fixture in `tests/data`
//! was written by it (`generate.py` there), and the tests hold this port to
//! its answers -- the same compressed bytes for the same input, the same
//! output from the same stream, and the same verdict on every one-byte
//! corruption of two streams.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use bzip2::{Error, Level, compress, decompress, decompress_limited};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// FNV-1a, 64-bit: `generate.py`'s `fnv`.
fn fnv(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `generate.py`'s `Rng`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

const VOCAB: [&[u8]; 48] = [
    b"the", b"of", b"and", b"a", b"to", b"in", b"is", b"you", b"that", b"it", b"he", b"was",
    b"for", b"on", b"are", b"as", b"with", b"his", b"they", b"I", b"at", b"be", b"this", b"have",
    b"from", b"or", b"one", b"had", b"by", b"word", b"but", b"not", b"what", b"all", b"were",
    b"we", b"when", b"your", b"can", b"said", b"there", b"use", b"an", b"each", b"which", b"she",
    b"do", b"how",
];

/// `generate.py`'s `gen`: the input a fixture was made from.
fn generate(kind: &str, args: &[usize]) -> Vec<u8> {
    match (kind, args) {
        ("fill", &[n, byte]) => vec![byte as u8; n],
        ("text", &[n, seed]) => {
            let mut r = Rng(seed as u64);
            let mut out = Vec::new();
            while out.len() < n {
                out.extend_from_slice(VOCAB[r.next() as usize % VOCAB.len()]);
                out.push(if r.next().is_multiple_of(12) {
                    b'\n'
                } else {
                    b' '
                });
            }
            out.truncate(n);
            out
        }
        ("random", &[n, seed]) => {
            let mut r = Rng(seed as u64);
            (0..n).map(|_| r.next() as u8).collect()
        }
        ("runs", &[n, seed]) => {
            let mut r = Rng(seed as u64);
            let mut out = Vec::new();
            while out.len() < n {
                let b = r.next() as u8;
                let len = 1 + r.next() as usize % 300;
                out.extend(std::iter::repeat_n(b, len));
            }
            out.truncate(n);
            out
        }
        ("periodic", &[n, period, seed]) => {
            let pat = generate("random", &[period, seed]);
            pat.iter().copied().cycle().take(n).collect()
        }
        ("aaaab", &[n]) => b"AAAAB".iter().copied().cycle().take(n).collect(),
        ("count", &[n]) => (0..n).map(|i| i as u8).collect(),
        ("ab", &[n, seed]) => {
            let mut r = Rng(seed as u64);
            (0..n).map(|_| b"ab"[(r.next() & 1) as usize]).collect()
        }
        _ => panic!("unknown generator {kind} {args:?}"),
    }
}

struct Case {
    name: String,
    level: u8,
    kind: String,
    args: Vec<usize>,
    len: usize,
    hash: u64,
}

impl Case {
    fn file(&self) -> String {
        if self.kind == "sample" {
            format!("{}.bz2", self.name)
        } else {
            format!("{}.l{}.bz2", self.name, self.level)
        }
    }
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string(data_dir().join("cases.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let n = f.len();
            Case {
                name: f[0].to_owned(),
                level: f[1].parse().unwrap(),
                kind: f[2].to_owned(),
                args: f[3..n - 2]
                    .iter()
                    .filter(|a| **a != "-")
                    .map(|a| a.parse().unwrap())
                    .collect(),
                len: f[n - 2].parse().unwrap(),
                hash: u64::from_str_radix(f[n - 1], 16).unwrap(),
            }
        })
        .collect()
}

/// The generators reproduce the inputs `generate.py` compressed; otherwise
/// every comparison below would be against the wrong bytes.
#[test]
fn the_generators_match_the_fixtures_inputs() {
    for c in cases().iter().filter(|c| c.kind != "sample") {
        let input = generate(&c.kind, &c.args);
        assert_eq!(input.len(), c.len, "{}", c.file());
        assert_eq!(fnv(&input), c.hash, "{}", c.file());
    }
}

/// The compressor writes libbzip2's bytes, at every level the fixtures use:
/// the main sort, its budget running out, the fallback, runs crossing block
/// boundaries, a run-length step that grows the block, no input at all.
#[test]
fn compress_writes_libbzip2s_bytes() {
    for c in cases()
        .iter()
        .filter(|c| c.kind != "sample" && c.name != "randomised")
    {
        let input = generate(&c.kind, &c.args);
        let want = read(&c.file());
        let got = compress(&input, Level::new(c.level).unwrap());
        if got != want {
            let at = got.iter().zip(&want).position(|(a, b)| a != b);
            panic!(
                "{}: {} bytes against libbzip2's {}, first difference at {at:?}",
                c.file(),
                got.len(),
                want.len()
            );
        }
    }
}

/// Every fixture decompresses to its input -- the bzip2 distribution's own
/// sample files among them, which `make test` checks a build of bzip2 with.
#[test]
fn decompress_reads_every_fixture() {
    for c in cases() {
        let packed = read(&c.file());
        let out = decompress(&packed).unwrap_or_else(|e| panic!("{}: {e}", c.file()));
        assert_eq!(out.len(), c.len, "{}", c.file());
        assert_eq!(fnv(&out), c.hash, "{}", c.file());
        if c.kind != "sample" {
            assert_eq!(out, generate(&c.kind, &c.args), "{}", c.file());
        }
    }
}

/// A block bzip2 0.9.0 would have randomised, long enough to use every
/// entry of the randomisation table and wrap round to the first.
#[test]
fn a_randomised_block_is_read() {
    let packed = read("randomised.l4.bz2");
    // The randomised bit: 32 bits of header, 48 of block magic, 32 of CRC.
    assert_eq!(packed[14] >> 7, 1, "the fixture's block must be randomised");
    let out = decompress(&packed).unwrap();
    assert_eq!(out, generate("ab", &[330_000, 12]));
}

/// Every one-byte corruption of two streams, against what `bzip2 -d` made
/// of it: an error, or exactly the output libbzip2 produced -- since a
/// change to padding bits, or to a selector between two equal tables,
/// changes nothing.
#[test]
fn a_corrupted_stream_is_refused_exactly_when_libbzip2_refuses_it() {
    let text = std::fs::read_to_string(data_dir().join("mutations.txt")).unwrap();
    let mut bases: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut checked = 0usize;
    let mut accepted = 0usize;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let f: Vec<&str> = line.split_whitespace().collect();
        let base = bases.entry(f[0].to_owned()).or_insert_with(|| read(f[0]));
        let pos: usize = f[1].parse().unwrap();
        let delta = u8::from_str_radix(f[2], 16).unwrap();
        let mut bad = base.clone();
        bad[pos] ^= delta;
        let got = decompress(&bad);
        match f[3] {
            "ERR" => assert!(
                got.is_err(),
                "{line}: libbzip2 refused it, this accepted it"
            ),
            "OK" => {
                let out =
                    got.unwrap_or_else(|e| panic!("{line}: libbzip2 accepted it, this said {e}"));
                assert_eq!(out.len(), f[4].parse::<usize>().unwrap(), "{line}");
                assert_eq!(fnv(&out), u64::from_str_radix(f[5], 16).unwrap(), "{line}");
                accepted += 1;
            }
            other => panic!("bad verdict {other}"),
        }
        checked += 1;
    }
    assert!(checked > 2_000, "only {checked} mutations");
    assert!(
        accepted > 0,
        "the corpus should hold some harmless corruptions"
    );
}

/// Every proper prefix of a stream is a stream cut short.
#[test]
fn every_truncation_is_an_unexpected_end() {
    for name in [
        "mut-runs.l1.bz2",
        "short.l9.bz2",
        "empty.l9.bz2",
        "sample3.bz2",
    ] {
        let packed = read(name);
        for len in 0..packed.len() {
            assert_eq!(
                decompress(&packed[..len]),
                Err(Error::UnexpectedEnd),
                "{name} cut to {len}"
            );
        }
    }
}

/// The property a parser of untrusted input needs most: whatever the bytes,
/// it answers. Every byte of the fixtures that stand for little data, and the
/// headers, mapping tables and first selectors of the rest (a decode of each
/// of those costs a whole block), changed three ways.
#[test]
fn single_byte_corruption_never_panics() {
    for c in cases() {
        let packed = read(&c.file());
        let small = packed.len() <= 4_096 && c.len <= 20_000;
        let span = if small {
            packed.len()
        } else {
            packed.len().min(64)
        };
        for pos in 0..span {
            for delta in [0x01u8, 0x80, 0xff] {
                let mut bad = packed.clone();
                bad[pos] ^= delta;
                // The answer does not matter here; answering does.
                let _ = decompress_limited(&bad, 1 << 16);
            }
        }
    }
}

/// The output cap holds across a stream of many blocks, and an answer that
/// fits exactly is not refused.
#[test]
fn the_limit_holds_across_blocks() {
    let packed = read("aaaab.l1.bz2");
    assert_eq!(decompress_limited(&packed, 400_000).unwrap().len(), 400_000);
    assert_eq!(
        decompress_limited(&packed, 399_999),
        Err(Error::OutputTooLarge)
    );
    assert_eq!(
        decompress_limited(&packed, 150_000),
        Err(Error::OutputTooLarge)
    );
}

/// `pbzip2` writes a stream per block; `cat a.bz2 b.bz2` makes one file of
/// two. Both read back whole, as `bzip2 -d` reads them.
#[test]
fn concatenated_streams_read_back_whole() {
    let a = read("short.l9.bz2");
    let b = read("random.l2.bz2");
    let mut both = a.clone();
    both.extend_from_slice(&b);
    let mut want = generate("text", &[3_000, 1]);
    want.extend(generate("random", &[3_000, 4]));
    assert_eq!(decompress(&both).unwrap(), want);
}
