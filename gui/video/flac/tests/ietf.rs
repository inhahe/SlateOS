//! The IETF CELLAR working group's FLAC conformance files -- the subset
//! every decoder must read, the uncommon streams a decoder should, and the
//! faulty ones -- held to libFLAC 1.5.0's reading of each, line for line.
//! Ignored: the files are fetched, with their answers made, by
//! `tools/ietf.py DIR`; then `FLAC_IETF=DIR cargo test -p flac --test ietf
//! -- --ignored`.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test: a failure should be loud"
)]

mod common;

#[test]
#[ignore = "needs the IETF files, fetched by tools/ietf.py"]
fn every_ietf_file_reads_as_libflac_reads_it() {
    let root = std::env::var("FLAC_IETF").unwrap_or_else(|_| panic!("FLAC_IETF names the corpus"));
    let mut count = 0;
    for sub in ["subset", "uncommon", "faulty"] {
        let mut names: Vec<String> = std::fs::read_dir(format!("{root}/{sub}"))
            .unwrap()
            .map(|e| e.unwrap().path().to_string_lossy().into_owned())
            .filter(|p| p.ends_with(".flac"))
            .collect();
        names.sort();
        for path in names {
            common::decodes_as_libflac_does(&path);
            count += 1;
        }
    }
    assert!(count > 80, "{count} files");
}
