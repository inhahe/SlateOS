//! This crate against liblzma 5.2.5 itself. Every verdict in `tests/data`
//! is liblzma's -- `oracle.c`, linked against the 5.2.5 the port is from,
//! judging each file as `xz -d` does (`generate.py`) -- and the tests hold the
//! port to it: the same output from every file liblzma reads, a refusal of
//! every file it refuses, and the same *kind* of refusal (not this format,
//! unsupported, damaged, cut short).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use xz::{Error, MAX_OUTPUT};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// FNV-1a, 64-bit: `generate.py`'s and `oracle.c`'s `fnv`.
fn fnv(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// liblzma's verdict on a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Ok {
        len: usize,
        hash: u64,
    },
    /// The `lzma_ret` that stopped it.
    Err(u32),
}

fn parse_verdict(words: &[&str]) -> Verdict {
    match words {
        ["OK", len, hash, ..] => Verdict::Ok {
            len: len.parse().unwrap(),
            hash: u64::from_str_radix(hash, 16).unwrap(),
        },
        ["ERR", code, ..] => Verdict::Err(code.parse().unwrap()),
        other => panic!("bad verdict {other:?}"),
    }
}

/// Whether this crate's error is the kind of refusal liblzma's code is:
/// `LZMA_FORMAT_ERROR` (7) is "not this format", `LZMA_OPTIONS_ERROR` (8)
/// "unsupported", `LZMA_DATA_ERROR` (9) damage, and `LZMA_BUF_ERROR` (10) --
/// what `xz -d` sees when the input ends early -- a stream cut short.
fn same_kind(code: u32, e: Error) -> bool {
    match code {
        7 => matches!(e, Error::NotXz | Error::NotLzma),
        8 => e == Error::Unsupported,
        9 => matches!(
            e,
            Error::HeaderCrcMismatch
                | Error::CheckMismatch
                | Error::IndexMismatch
                | Error::InvalidPadding
                | Error::InvalidData
                | Error::TrailingData
        ),
        10 => e == Error::UnexpectedEnd,
        _ => false,
    }
}

/// Decodes `data` as the file named `name` says. A `.raw` file is raw LZMA2
/// with preset 6's 8 MiB dictionary, which is properties byte 22.
fn decode(name: &str, data: &[u8]) -> Result<Vec<u8>, Error> {
    if name.ends_with(".lzma") {
        xz::decompress_lzma_limited(data, MAX_OUTPUT)
    } else if name.ends_with(".raw") {
        xz::lzma2(22, data, MAX_OUTPUT)
    } else {
        xz::decompress_limited(data, MAX_OUTPUT)
    }
}

fn check(what: &str, got: Result<Vec<u8>, Error>, want: &Verdict) {
    match (got, want) {
        (Ok(out), Verdict::Ok { len, hash }) => {
            assert_eq!(out.len(), *len, "{what}: length");
            assert_eq!(fnv(&out), *hash, "{what}: contents");
        }
        (Err(e), Verdict::Err(code)) => {
            assert!(
                same_kind(*code, e),
                "{what}: liblzma said {code}, this said {e:?}"
            );
        }
        (got, want) => panic!("{what}: liblzma said {want:?}, this said {got:?}"),
    }
}

/// XZ Utils' own test files, each named for what it tests: `good-` files
/// decode, `bad-` files are refused, `unsupported-` files are refused as
/// unsupported -- except `unsupported-check.xz`, which decodes with its
/// check unverified, as `xz -d` decodes it (with a warning).
#[test]
fn xz_utils_own_test_files() {
    let list = std::fs::read_to_string(data_dir().join("files.txt")).unwrap();
    let mut seen = 0;
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let words: Vec<&str> = line.split_whitespace().collect();
        let name = words[0];
        let want = parse_verdict(&words[1..]);
        let data = read(&format!("files/{name}"));
        check(name, decode(name, &data), &want);
        if name.starts_with("good-") {
            assert!(matches!(want, Verdict::Ok { .. }), "{name}");
        }
        if name.starts_with("bad-") {
            assert!(matches!(want, Verdict::Err(_)), "{name}");
        }
        seen += 1;
    }
    assert!(seen >= 60, "only {seen} files");

    let (_, info) =
        xz::decompress_with_info(&read("files/unsupported-check.xz"), MAX_OUTPUT).unwrap();
    assert!(
        info.unverified,
        "an unsupported check was not reported unverified"
    );
    let (_, info) =
        xz::decompress_with_info(&read("files/good-1-check-sha256.xz"), MAX_OUTPUT).unwrap();
    assert!(!info.unverified);
}

/// Files `xz` 5.2.5 made: every check, every filter (the six branch
/// converters, one with a start offset, delta, and a chain of two), several
/// blocks, `.lzma`, empty input -- and concatenations: two streams, stream
/// padding, padding that is not a multiple of four, trailing garbage.
#[test]
fn files_xz_made() {
    let list = std::fs::read_to_string(data_dir().join("made.txt")).unwrap();
    let mut seen = 0;
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let words: Vec<&str> = line.split_whitespace().collect();
        let name = words[0];
        let want = parse_verdict(&words[4..]);
        let data = read(&format!("made/{name}"));
        check(name, decode(name, &data), &want);
        seen += 1;
    }
    assert!(seen >= 20, "only {seen} files");
}

/// Every byte of four small files XORed with 01, 80 and FF -- a stream with
/// one block, one with three and a SHA-256 check, an `.lzma`, and raw LZMA2,
/// which no check guards, so that the LZMA decoder's own refusals are what is
/// judged -- against
/// liblzma's verdict on each: the same output where it decodes one, and the
/// same kind of refusal where it does not.
#[test]
fn a_corrupted_file_is_refused_exactly_when_liblzma_refuses_it() {
    let list = std::fs::read_to_string(data_dir().join("mutations.txt")).unwrap();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut current = String::new();
    let (mut checked, mut accepted) = (0usize, 0usize);
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        if let Some(name) = line.strip_prefix("== ") {
            current = name.to_owned();
            files.insert(current.clone(), read(&format!("made/{name}")));
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let pos: usize = words[0].parse().unwrap();
        let xor = u8::from_str_radix(words[1], 16).unwrap();
        let want = parse_verdict(&words[2..]);
        let mut bad = files[&current].clone();
        bad[pos] ^= xor;
        if matches!(want, Verdict::Ok { .. }) {
            accepted += 1;
        }
        check(
            &format!("{current} byte {pos} ^ {xor:02x}"),
            decode(&current, &bad),
            &want,
        );
        checked += 1;
    }
    assert!(checked > 3_000, "only {checked} mutations");
    assert!(
        accepted > 0,
        "the corpus should hold harmless corruptions too"
    );
}

/// Every proper prefix of a stream is cut short.
#[test]
fn every_truncation_is_an_unexpected_end() {
    for name in [
        "made/text-small.xz",
        "made/two-blocks-small.xz",
        "files/good-1-x86-lzma2.xz",
    ] {
        let data = read(name);
        for len in 0..data.len() {
            assert_eq!(
                xz::decompress(&data[..len]),
                Err(Error::UnexpectedEnd),
                "{name} cut to {len}"
            );
        }
    }
    let data = read("made/lzma-small.lzma");
    for len in 0..data.len() {
        assert!(
            xz::decompress_lzma(&data[..len]).is_err(),
            "lzma cut to {len}"
        );
    }
}

/// The cap holds: a file that decodes to exactly the cap is read, one byte
/// less is refused, in `.xz` and `.lzma` alike.
#[test]
fn the_limit_is_checked_before_the_output_grows_past_it() {
    let data = read("made/text-blocks.xz");
    assert_eq!(
        xz::decompress_limited(&data, 200_000).unwrap().len(),
        200_000
    );
    assert_eq!(
        xz::decompress_limited(&data, 199_999),
        Err(Error::OutputTooLarge)
    );
    assert_eq!(xz::decompress_limited(&data, 0), Err(Error::OutputTooLarge));
    let data = read("made/lzma-text.lzma");
    assert_eq!(
        xz::decompress_lzma_limited(&data, 40_000).unwrap().len(),
        40_000
    );
    assert_eq!(
        xz::decompress_lzma_limited(&data, 39_999),
        Err(Error::OutputTooLarge)
    );
}

/// The parser of untrusted input answers whatever the bytes: every byte of
/// the small fixtures and the headers of the rest, changed three ways.
#[test]
fn single_byte_corruption_never_panics() {
    let mut names: Vec<String> = Vec::new();
    for dir in ["files", "made"] {
        for entry in std::fs::read_dir(data_dir().join(dir)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if name.ends_with(".xz") || name.ends_with(".lzma") || name.ends_with(".raw") {
                names.push(format!("{dir}/{name}"));
            }
        }
    }
    for name in names {
        let data = read(&name);
        let span = if data.len() <= 4_096 { data.len() } else { 96 };
        for pos in 0..span {
            for xor in [0x01u8, 0x80, 0xff] {
                let mut bad = data.clone();
                bad[pos] ^= xor;
                // The answer does not matter here; answering does.
                let _ = decode(&name, &bad);
            }
        }
    }
}

/// `xz -d` refuses any byte after an `.lzma` or raw stream ("Check that
/// there is no trailing garbage", `coder.c`), so this does too.
#[test]
fn bytes_after_an_lzma_or_raw_stream_are_refused() {
    let mut lzma = read("made/lzma-text.lzma");
    assert!(xz::decompress_lzma(&lzma).is_ok());
    lzma.push(0);
    assert_eq!(xz::decompress_lzma(&lzma), Err(Error::TrailingData));
    let mut raw = read("made/raw-lzma2.raw");
    assert!(xz::lzma2(22, &raw, MAX_OUTPUT).is_ok());
    raw.push(0);
    assert_eq!(xz::lzma2(22, &raw, MAX_OUTPUT), Err(Error::TrailingData));
}

#[test]
fn the_format_tests_tell_xz_and_lzma_apart() {
    assert!(xz::looks_like_xz(&read("made/text-crc64.xz")));
    assert!(!xz::looks_like_xz(&read("made/lzma-text.lzma")));
    assert!(xz::looks_like_lzma(&read("made/lzma-text.lzma")));
    assert!(!xz::looks_like_lzma(&read("made/text-crc64.xz")));
    assert!(!xz::looks_like_lzma(b"BZh91AY&SY"));
}
