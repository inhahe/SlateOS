//! The reader against 7-Zip 26.00's own archives and verdicts
//! (`tests/data/generate.py`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::path::PathBuf;

use sevenz::{Archive, Error};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
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

/// The input tree (`input.txt`), by archive path: a folder, or a file's
/// size and hash.
fn inputs() -> Vec<(String, Option<(usize, u64)>)> {
    let list = std::fs::read_to_string(data_dir().join("input.txt")).unwrap();
    let mut out: Vec<_> = list
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| match l.split_once(' ').unwrap() {
            ("D", path) => (path.to_owned(), None),
            ("F", rest) => {
                let mut f = rest.splitn(3, ' ');
                let size = f.next().unwrap().parse().unwrap();
                let hash = u64::from_str_radix(f.next().unwrap(), 16).unwrap();
                (f.next().unwrap().to_owned(), Some((size, hash)))
            }
            other => panic!("input.txt: {other:?}"),
        })
        .collect();
    // The archive lists each folder a file is in, too.
    let mut dirs = Vec::new();
    for (path, _) in &out {
        let mut p = path.as_str();
        while let Some((parent, _)) = p.rsplit_once('/') {
            dirs.push(parent.to_owned());
            p = parent;
        }
    }
    for d in dirs {
        if !out.iter().any(|(p, _)| *p == d) {
            out.push((d, None));
        }
    }
    out.sort();
    out
}

/// Every archive 7-Zip made of the input tree opens and gives back every
/// file and folder -- for the methods this reader has.
#[test]
fn archives_7zip_made_are_read() {
    let want = inputs();
    let list = std::fs::read_to_string(data_dir().join("made.txt")).unwrap();
    let mut read_ok = Vec::new();
    let mut skipped = Vec::new();
    for line in list.lines().filter(|l| !l.starts_with('#')) {
        let name = line.split_whitespace().next().unwrap();
        if name.starts_with("small-") {
            continue;
        }
        let data = read(&format!("made/{name}"));
        let archive = match Archive::open(&data) {
            Ok(a) => a,
            Err(Error::Unsupported | Error::PasswordRequired) => {
                skipped.push(name);
                continue;
            }
            Err(e) => panic!("{name}: {e:?}"),
        };
        let mut got = Vec::new();
        let mut unsupported = false;
        for entry in archive.entries() {
            let path = entry.name().unwrap();
            if entry.is_dir() {
                got.push((path, None));
                continue;
            }
            match entry.read(1 << 24) {
                Ok(bytes) => got.push((path, Some((bytes.len(), fnv(&bytes))))),
                Err(Error::Unsupported | Error::PasswordRequired) => {
                    unsupported = true;
                    break;
                }
                Err(e) => panic!("{name}: {path}: {e:?}"),
            }
        }
        if unsupported {
            skipped.push(name);
            continue;
        }
        got.sort();
        assert_eq!(got.len(), want.len(), "{name}: entries");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(g.0, w.0, "{name}: names");
            assert!(g.1 == w.1, "{name}: {} differs", g.0);
        }
        assert_eq!(archive.warnings(), sevenz::Warnings::default(), "{name}");
        read_ok.push(name);
    }
    // What this reader does not yet decode: PPMd, BCJ2, ARM64 and AES.
    assert_eq!(
        skipped,
        [
            "ppmd.7z",
            "bcj2.7z",
            "arm64.7z",
            "encrypted.7z",
            "encrypted-headers.7z"
        ],
        "read: {read_ok:?}"
    );
}
