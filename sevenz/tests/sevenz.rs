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

/// The password `generate.py` gives 7-Zip.
const PASSWORD: &str = "secret";

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
        // `generate.py` gives 7-Zip this password for every archive.
        let archive = match Archive::open_with_password(&data, PASSWORD) {
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
    // Every method and filter 7-Zip used here is read.
    assert!(skipped.is_empty(), "skipped {skipped:?}, read {read_ok:?}");
}

/// Archives of one file each, made for what the input tree has too little of
/// (`single.txt`) -- RISC-V code whose AUIPC pairs 7-Zip's encoder escapes,
/// above all -- give their file back whole.
#[test]
fn single_file_archives_give_back_their_file() {
    let list = std::fs::read_to_string(data_dir().join("single.txt")).unwrap();
    let mut n = 0;
    for line in list.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.splitn(4, ' ');
        let name = f.next().unwrap();
        let size: usize = f.next().unwrap().parse().unwrap();
        let hash = u64::from_str_radix(f.next().unwrap(), 16).unwrap();
        let file = f.next().unwrap();
        let data = read(&format!("made/{name}"));
        let archive = Archive::open(&data).unwrap();
        let entries: Vec<_> = archive.entries().collect();
        assert_eq!(entries.len(), 1, "{name}");
        assert_eq!(entries[0].name().unwrap(), file, "{name}");
        let got = entries[0].read(1 << 24).unwrap();
        assert!(
            got.len() == size && fnv(&got) == hash,
            "{name}: {file} differs"
        );
        assert_eq!(archive.warnings(), sevenz::Warnings::default(), "{name}");
        n += 1;
    }
    assert!(n >= 2, "single.txt lists {n} archives");
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

/// Each folder's methods are named as `7z l -slt` names a file's (its
/// archive-wide `Method` line, first, is another list).
#[test]
fn methods_are_named_as_7zip_names_them() {
    for (name, want) in [
        ("bcj-lzma2.7z", "BCJ LZMA2:17"),
        ("bcj2.7z", "BCJ2 LZMA:17 LZMA:17:lc0:lp2 LZMA:17:lc0:lp2"),
        ("delta.7z", "Delta:4 LZMA2:17"),
        ("encrypted.7z", "LZMA2:17 7zAES:19"),
        ("ppmd.7z", "PPMD:o6:mem21"),
        ("ppmd-small-mem.7z", "PPMD:o32:mem16"),
        ("lzma.7z", "LZMA:17"),
        ("arm64.7z", "ARM64 LZMA2:17"),
        ("riscv.7z", "RISCV LZMA2:17"),
        ("deflate.7z", "Deflate"),
        ("deflate64.7z", "Deflate64"),
        ("bzip2.7z", "BZip2"),
        ("copy.7z", "Copy"),
        ("lzma2-files.7z", "LZMA2:16"),
        // A property other than lc changed: lc is named only when it differs.
        ("lzma-pb0.7z", "LZMA:12:pb0"),
        ("riscv-pairs.7z", "RISCV Copy"),
    ] {
        let data = read(&format!("made/{name}"));
        let archive = Archive::open_with_password(&data, PASSWORD).unwrap();
        assert!(archive.num_folders() > 0, "{name}");
        for folder in 0..archive.num_folders() {
            assert_eq!(
                archive.folder_method(folder),
                want,
                "{name} folder {folder}"
            );
            assert!(archive.folder_packed_size(folder) > 0, "{name}");
        }
    }
}

/// What this reader, as 7-Zip with `threads`, makes of an archive, in the
/// words of `generate.py`'s `verdict`.
fn our_verdict(data: &[u8], threads: Threads) -> String {
    let mut archive = match Archive::open_with_password(data, PASSWORD) {
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
    let name_of = |index: usize| archive.entry(index).unwrap().name().unwrap();
    for folder in 0..archive.num_folders() {
        match archive.read_folder(folder, 1 << 24) {
            Ok(result) => {
                for (index, r) in result.files {
                    if let Err(e) = r {
                        items.push(format!("{}:{}", name_of(index), kind(e)));
                    }
                }
                // What 7-Zip reports of a folder rather than a file, it
                // labels with the folder's index.
                if let Some(e) = result.error_after_files {
                    items.push(format!("#{folder}:{}", kind(e)));
                }
                if result.data_after_end {
                    items.push(format!(
                        "#{folder}:There_are_some_data_after_the_end_of_the_payload_data"
                    ));
                }
            }
            // A folder that cannot be decoded at all: 7-Zip fails each of
            // its files.
            Err(e) => {
                for index in 0..archive.len() {
                    if archive.folder_of(index) == Some(folder) {
                        items.push(format!("{}:{}", name_of(index), kind(e)));
                    }
                }
            }
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

/// What no one-byte mutant reaches (`crafted.txt`): bytes after a coder's
/// stream -- data after the end for BZip2, Deflate and Copy, the coder's
/// own error for LZMA and PPMd -- two BZip2 streams in one coder, of which
/// 7-Zip reads one, Deflate streams zlib refuses and 7-Zip does not, and
/// Deflate and BZip2 coders given a property they do not take.
#[test]
fn crafted_archives_fail_where_7zip_says() {
    let list = std::fs::read_to_string(data_dir().join("crafted.txt")).unwrap();
    let mut checked = 0;
    for line in list.lines().filter(|l| !l.starts_with('#')) {
        let (name, rest) = line.split_once(' ').unwrap();
        let want = rest.split(" -- ").next().unwrap();
        let data = read(&format!("made/{name}"));
        for threads in [Threads::Many, Threads::One] {
            assert_eq!(our_verdict(&data, threads), want, "{name}");
        }
        checked += 1;
    }
    assert!(checked >= 12, "only {checked}");
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
