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

/// The input tree's files, by archive path (`/` between components).
fn inputs() -> Vec<(String, Option<Vec<u8>>)> {
    let root = data_dir().join("input");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let e = e.unwrap();
            let path = e.path();
            let rel = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                out.push((rel, None));
                stack.push(path);
            } else {
                out.push((rel, Some(std::fs::read(&path).unwrap())));
            }
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
                Ok(bytes) => got.push((path, Some(bytes))),
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
