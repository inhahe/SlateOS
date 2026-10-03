//! The reader against 7-Zip 26.00's own archives and verdicts
//! (`tests/data/generate.py`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::path::PathBuf;

use sevenz::{Archive, Error, Threads};

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
        // Archives of other trees, for the corruption tests.
        if ["small-", "chunks-", "blocks-"]
            .iter()
            .any(|p| name.starts_with(p))
        {
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

/// LZMA2 streams of several chunks under one dictionary, and of several
/// blocks each with its own, decode whole either way 7-Zip decodes them.
#[test]
fn lzma2_of_many_chunks_and_blocks_is_read() {
    for name in ["chunks-lzma2.7z", "blocks-lzma2.7z"] {
        let data = read(&format!("made/{name}"));
        for threads in [Threads::Many, Threads::One] {
            let mut archive = Archive::open(&data).unwrap();
            archive.set_threads(threads);
            let mut got: Vec<(String, usize)> = archive
                .entries()
                .map(|e| (e.name().unwrap(), e.read(1 << 24).unwrap().len()))
                .collect();
            got.sort();
            let want = [("a.txt", 240_000), ("b.bin", 30_000), ("c.txt", 240_000)];
            let want: Vec<(String, usize)> =
                want.iter().map(|(n, s)| ((*n).to_owned(), *s)).collect();
            assert_eq!(got, want, "{name} {threads:?}");
        }
    }
}

/// What this reader, as 7-Zip with `threads`, makes of an archive, in the
/// words of `generate.py`'s `verdict`.
fn our_verdict(data: &[u8], threads: Threads) -> String {
    let mut archive = match Archive::open(data) {
        Ok(a) => a,
        Err(e) => {
            let kind = match e {
                Error::NotSevenZip => "Is_not_archive",
                Error::Header => "Headers_Error",
                Error::UnexpectedEnd => "Unexpected_end_of_archive",
                other => return format!("OPENFAIL ?{other:?}"),
            };
            return format!("OPENFAIL {kind}");
        }
    };
    archive.set_threads(threads);
    let mut errors = Vec::new();
    if archive.warnings().header {
        errors.push("Headers_Error");
    }
    let mut items = Vec::new();
    for folder in 0..archive.num_folders() {
        match archive.read_folder(folder, 1 << 24) {
            Ok(result) => {
                for (index, r) in result.files {
                    if let Err(e) = r {
                        let name = archive.entry(index).unwrap().name().unwrap();
                        items.push(format!("{name}:{}", kind(e)));
                    }
                }
                if let Some(e) = result.error_after_files {
                    items.push(format!("#0:{}", kind(e)));
                }
            }
            Err(e) => items.push(format!("#folder:{}", kind(e))),
        }
    }
    if errors.is_empty() && items.is_empty() {
        return "OK".to_owned();
    }
    format!(
        "OPEN errors={} warnings=- items={}",
        if errors.is_empty() {
            "-".to_owned()
        } else {
            errors.join(",")
        },
        if items.is_empty() {
            "-".to_owned()
        } else {
            items.join("|")
        }
    )
}

fn kind(e: Error) -> &'static str {
    match e {
        Error::Data => "Data_Error",
        Error::Crc => "CRC_Failed",
        Error::Unsupported => "Unsupported_Method",
        Error::UnexpectedEnd => "Unexpected_end_of_data",
        _ => "?",
    }
}

/// Every byte of three small archives XORed with 01, 80 and FF -- LZMA2
/// with a packed header, LZMA with a plain one, and Copy -- and the chunk
/// headers and chunk edges of two LZMA2 archives of several chunks, with
/// 7-Zip's verdict on each: whether it opens, what is wrong with the header,
/// and which files fail and how. This reader must give the same.
#[test]
fn a_corrupted_archive_fails_where_7zip_says() {
    corrupted("mutations.txt", Threads::Many);
}

/// The same, against 7-Zip with one thread, whose LZMA2 decoding gives back
/// more of some damaged streams.
#[test]
fn a_corrupted_archive_fails_where_7zip_with_one_thread_says() {
    corrupted("mutations-mmt-off.txt", Threads::One);
}

fn corrupted(file: &str, threads: Threads) {
    let list = std::fs::read_to_string(data_dir().join(file)).unwrap();
    let mut current: Vec<u8> = Vec::new();
    let mut name = String::new();
    let (mut checked, mut wrong, mut crashed) = (0usize, Vec::new(), 0usize);
    for line in list.lines().filter(|l| !l.starts_with('#')) {
        if let Some(n) = line.strip_prefix("== ") {
            name = n.to_owned();
            current = read(&format!("made/{n}"));
            continue;
        }
        let mut f = line.splitn(3, ' ');
        let pos: usize = f.next().unwrap().parse().unwrap();
        let xor = u8::from_str_radix(f.next().unwrap(), 16).unwrap();
        let want = f.next().unwrap();
        let mut bad = current.clone();
        bad[pos] ^= xor;
        let got = our_verdict(&bad, threads);
        if want == "CRASH" {
            // 7-Zip died on every attempt: there is nothing to agree with,
            // and this reader must merely come back.
            crashed += 1;
            continue;
        }
        // An open failure 7-Zip gives no reason for may be any of ours.
        let agree = got == want || (want == "OPENFAIL -" && got.starts_with("OPENFAIL"));
        if !agree {
            wrong.push(format!(
                "{name} byte {pos} ^ {xor:02x}: 7-Zip {want} / here {got}"
            ));
        }
        checked += 1;
    }
    assert!(checked > 3000, "only {checked}");
    assert!(
        crashed * 100 < checked,
        "7-Zip crashed on {crashed} of {checked}"
    );
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(400)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
