//! The encoders against XZ Utils 5.2.5's own output. Every file in
//! `tests/data/made` was written by `xz` 5.2.5 single-threaded, with the
//! arguments `made.txt` records; encoding what it holds with those arguments
//! must give the file back, byte for byte.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::path::PathBuf;

use xz::{Bcj, Check, LzmaOptions, MatchFinder, Mode, PreFilter, Preset, XzOptions};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// What an `xz` command line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Xz,
    Lzma,
    Raw,
    RawLzma1,
}

/// A preset name, `6` or `9e`.
fn preset(level: &str) -> Preset {
    let (digits, extreme) = level
        .strip_suffix('e')
        .map_or((level, false), |d| (d, true));
    let p = Preset::new(digits.parse().unwrap()).unwrap();
    if extreme { p.extreme() } else { p }
}

/// `--lzma2=` and `--lzma1=`'s comma-separated settings, applied in order.
fn lzma_settings(spec: &str) -> LzmaOptions {
    let mut o = LzmaOptions::preset(Preset::DEFAULT);
    for item in spec.split(',') {
        let (key, value) = item.split_once('=').unwrap();
        let number = || value.parse::<u32>().unwrap();
        match key {
            "preset" => o = LzmaOptions::preset(preset(value)),
            "lc" => o.lc = number(),
            "lp" => o.lp = number(),
            "pb" => o.pb = number(),
            "nice" => o.nice_len = number(),
            "depth" => o.depth = number(),
            "dict" => o.dict_size = number(),
            "mode" => {
                o.mode = match value {
                    "fast" => Mode::Fast,
                    "normal" => Mode::Normal,
                    other => panic!("mode {other}"),
                }
            }
            "mf" => {
                o.match_finder = match value {
                    "hc3" => MatchFinder::Hc3,
                    "hc4" => MatchFinder::Hc4,
                    "bt2" => MatchFinder::Bt2,
                    "bt3" => MatchFinder::Bt3,
                    "bt4" => MatchFinder::Bt4,
                    other => panic!("mf {other}"),
                }
            }
            other => panic!("an LZMA setting this test does not know: {other}"),
        }
    }
    o
}

/// Parses the `xz` arguments `made.txt` records into what the crate takes.
fn parse_args(args: &str) -> (Format, XzOptions) {
    let mut format = Format::Xz;
    let mut lzma1 = false;
    let mut options = XzOptions::preset(Preset::DEFAULT);
    for arg in args.split_whitespace() {
        let bcj = |arch: Bcj, rest: &str| {
            let start = rest
                .strip_prefix("=start=")
                .map_or(0, |s| s.parse().unwrap());
            PreFilter::Bcj { arch, start }
        };
        match arg {
            "--format=xz" => format = Format::Xz,
            "--format=lzma" => format = Format::Lzma,
            "--format=raw" => format = Format::Raw,
            "--check=crc32" => options.check = Check::CRC32,
            "--check=crc64" => options.check = Check::CRC64,
            "--check=sha256" => options.check = Check::SHA256,
            "--check=none" => options.check = Check::NONE,
            a if a.starts_with("--block-size=") => {
                options.block_size = Some(a["--block-size=".len()..].parse().unwrap());
            }
            a if a.starts_with("--lzma2=") => options.lzma2 = lzma_settings(&a["--lzma2=".len()..]),
            a if a.starts_with("--lzma1=") => {
                lzma1 = true;
                options.lzma2 = lzma_settings(&a["--lzma1=".len()..]);
            }
            a if a.starts_with("--delta=dist=") => options.filters.push(PreFilter::Delta {
                distance: a["--delta=dist=".len()..].parse().unwrap(),
            }),
            a if a.starts_with("--x86") => options.filters.push(bcj(Bcj::X86, &a[5..])),
            a if a.starts_with("--armthumb") => options.filters.push(bcj(Bcj::ArmThumb, &a[10..])),
            a if a.starts_with("--arm") => options.filters.push(bcj(Bcj::Arm, &a[5..])),
            a if a.starts_with("--powerpc") => options.filters.push(bcj(Bcj::PowerPc, &a[9..])),
            a if a.starts_with("--ia64") => options.filters.push(bcj(Bcj::Ia64, &a[6..])),
            a if a.starts_with("--sparc") => options.filters.push(bcj(Bcj::Sparc, &a[7..])),
            a if a.starts_with('-')
                && a[1..].chars().next().is_some_and(|c| c.is_ascii_digit()) =>
            {
                options.lzma2 = LzmaOptions::preset(preset(&a[1..]));
            }
            other => panic!("an xz argument this test does not know: {other}"),
        }
    }
    if format == Format::Raw && lzma1 {
        format = Format::RawLzma1;
    }
    (format, options)
}

/// Encodes `data` as `xz` would with these arguments.
fn encode(format: Format, options: &XzOptions, data: &[u8]) -> Vec<u8> {
    match format {
        Format::Xz => xz::compress_with(data, options).unwrap(),
        Format::Lzma => xz::compress_lzma(data, &options.lzma2).unwrap(),
        Format::Raw => xz::lzma2_encode(data, &options.lzma2).unwrap(),
        Format::RawLzma1 => xz::lzma1_encode(data, &options.lzma2).unwrap(),
    }
}

/// Decodes a file `xz` made, to get back what it was made from.
fn decode(format: Format, packed: &[u8]) -> Vec<u8> {
    match format {
        Format::Xz => xz::decompress(packed).unwrap(),
        Format::Lzma => xz::decompress_lzma(packed).unwrap(),
        // `generate.py` writes raw LZMA2 with preset 6's 8 MiB dictionary.
        Format::Raw => xz::lzma2(22, packed, xz::MAX_OUTPUT).unwrap(),
        Format::RawLzma1 => panic!("made.txt has no raw LZMA1"),
    }
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
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// `generate.py`'s `gen`: the same bytes from the same kind, size and seed.
fn input(kind: &str, n: usize, seed: u64) -> Vec<u8> {
    const WORDS: [&[u8]; 8] = [
        b"alpha", b"beta", b"gamma", b"delta", b"epsilon", b"zeta", b"eta", b"theta",
    ];
    let mut r = Rng(seed);
    let mut out = Vec::with_capacity(n + 300);
    match kind {
        "text" => {
            while out.len() < n {
                out.extend_from_slice(WORDS[(r.next() % 8) as usize]);
                out.push(if r.next().is_multiple_of(9) {
                    b'\n'
                } else {
                    b' '
                });
            }
        }
        "random" => out.extend((0..n).map(|_| (r.next() & 0xff) as u8)),
        "zeros" => out.resize(n, 0),
        "edge" => {
            let mut period = vec![0u8; 4097];
            for b in &mut period[..32] {
                *b = (1 + r.next() % 255) as u8;
            }
            while out.len() < n {
                out.extend_from_slice(&period);
            }
        }
        "mixed" => {
            let third = n / 3;
            out.extend(input("text", third, seed));
            out.extend(input("random", third, seed + 1));
            out.extend(input("text", n - 2 * third, seed + 2));
        }
        "periodic" => {
            while out.len() < n {
                let period = 1 + r.next() % 300;
                let pattern: Vec<u8> = (0..period).map(|_| (r.next() & 0xff) as u8).collect();
                for _ in 0..=(r.next() % 50) {
                    out.extend_from_slice(&pattern);
                }
            }
        }
        "code" => {
            let snippets: Vec<Vec<u8>> = (0..12u32)
                .map(|i| {
                    (0..6 + i % 5)
                        .map(|j| ((i * 37 + j * 11) & 0xff) as u8)
                        .collect()
                })
                .collect();
            while out.len() < n {
                match r.next() % 8 {
                    0 => {
                        out.push(0xe8);
                        out.extend_from_slice(&((0x400 * (r.next() % 9)) as u32).to_le_bytes());
                    }
                    1 => {
                        let v = (0x100 * (r.next() % 7)) as u32;
                        out.extend_from_slice(&v.to_le_bytes()[..3]);
                        out.push(0xeb);
                    }
                    2 => {
                        let k = 1 + r.next() % 4;
                        out.extend(std::iter::repeat_n(0x90, k as usize));
                    }
                    _ => out.extend_from_slice(&snippets[(r.next() % 12) as usize]),
                }
            }
        }
        other => panic!("input kind {other}"),
    }
    out.truncate(n);
    out
}

/// The first byte at which two outputs part, for the failure message.
fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

/// Every file `xz` 5.2.5 made from the generated inputs -- every preset
/// they use, every check, every filter and a chain, several blocks, `.lzma`,
/// raw LZMA2, empty input -- is what this crate writes for the same input.
#[test]
fn files_xz_made_are_written_byte_for_byte() {
    let list = std::fs::read_to_string(data_dir().join("made.txt")).unwrap();
    let mut seen = 0;
    let mut wrong = Vec::new();
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let (fields, args) = line.split_once(" -- ").unwrap();
        if args == "concatenated" {
            continue;
        }
        let name = fields.split_whitespace().next().unwrap();
        let (format, options) = parse_args(args);
        let want = read(&format!("made/{name}"));
        let data = decode(format, &want);
        let got = encode(format, &options, &data);
        if got != want {
            wrong.push(format!(
                "{name} ({args}): {} bytes, xz wrote {}; first difference at {}",
                got.len(),
                want.len(),
                first_difference(&got, &want)
            ));
        }
        seen += 1;
    }
    assert!(seen >= 20, "only {seen} files");
    assert!(
        wrong.is_empty(),
        "not what xz writes:\n{}",
        wrong.join("\n")
    );
}

/// Whatever is written reads back, at every preset.
#[test]
fn every_preset_round_trips() {
    let mut data = Vec::new();
    for i in 0u32..20_000 {
        data.extend_from_slice(format!("line {} of {}\n", i % 97, i % 13).as_bytes());
    }
    data.extend((0u32..5000).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8));
    for level in 0..=9 {
        for preset in [
            Preset::new(level).unwrap(),
            Preset::new(level).unwrap().extreme(),
        ] {
            let packed = xz::compress(&data, preset);
            assert_eq!(xz::decompress(&packed).unwrap(), data, "{preset:?}");
        }
    }
    for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", b"aaaaaaaaaaaaaaaa"] {
        let packed = xz::compress(data, Preset::DEFAULT);
        assert_eq!(xz::decompress(&packed).unwrap(), data);
    }
}

/// What `xz` 5.2.5 writes for generated inputs, held to its length and hash
/// (`encoded.txt`): every preset and extreme preset over text, random bytes,
/// machine-code-like bytes, periodic runs and a mixture; the LZMA2 chunk
/// limits -- 2 MiB of input, 64 KiB of output, stored chunks; lc, lp, pb,
/// every match finder in both modes, nice lengths, depths and dictionary
/// sizes no preset uses, a 4 KiB dictionary among them; `.lzma` and raw
/// LZMA1; and inputs too short to hash.
#[test]
fn what_xz_writes_for_every_preset_and_setting_is_written_byte_for_byte() {
    let list = std::fs::read_to_string(data_dir().join("encoded.txt")).unwrap();
    let mut seen = 0;
    let mut wrong = Vec::new();
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let (fields, args) = line.split_once(" -- ").unwrap();
        let f: Vec<&str> = fields.split_whitespace().collect();
        let (name, kind) = (f[0], f[1]);
        let size: usize = f[2].parse().unwrap();
        let seed: u64 = f[3].parse().unwrap();
        let len: usize = f[4].parse().unwrap();
        let hash = u64::from_str_radix(f[5], 16).unwrap();
        let (format, options) = parse_args(args);
        let data = input(kind, size, seed);
        let got = encode(format, &options, &data);
        if got.len() != len || fnv(&got) != hash {
            wrong.push(format!(
                "{name} ({args}): {} bytes, xz wrote {len}",
                got.len()
            ));
        }
        seen += 1;
    }
    assert!(seen >= 130, "only {seen} cases");
    assert!(
        wrong.is_empty(),
        "not what xz writes:\n{}",
        wrong.join("\n")
    );
}
