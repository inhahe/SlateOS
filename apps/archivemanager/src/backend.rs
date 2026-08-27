//! The archive back end: real bytes, on and off the disk.
//!
//! Everything in `main.rs` above this module is a *model* of an archive — a
//! list of entries, a tree, a selection, a set of columns. This module is the
//! only place that knows those entries came from a file, and the only place
//! that can put them back into one. The split is worth keeping: the window is
//! tested by driving it with a model, and the model is tested by round-tripping
//! it through here, so neither test needs the other's machinery.
//!
//! # What this build can read
//!
//! ZIP, via the workspace's [`ziparchive`] crate — the same parser the kernel
//! links, promoted out of `kernel/src/fs/zip.rs` at lane C's request precisely
//! so that this program would not have to grow a second one. TAR, TAR.GZ,
//! TAR.BZ2 and 7z are named by [`ArchiveFormat`] and are refused here in words
//! rather than silently mis-parsed: `ArchiveError::NotYetReadable` says which
//! format it was and that this build has a ZIP back end only.
//!
//! # An entry name is not a path
//!
//! A ZIP member name is an attacker-controlled byte string that lives inside a
//! file someone else wrote. It becomes a path only after [`safe_destination`]
//! has confined it under the directory the user chose. `../../etc/passwd` is a
//! legal member name and `..\..\etc\passwd` is one a Windows tool will happily
//! write; both are refused, and the refusal is reported rather than skipped
//! quietly, because an extraction that silently drops members is worse than one
//! that says what it would not do.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::{ArchiveEntry, ArchiveFormat, ArchiveModel, ArchiveTestResults, TestResult};

/// The largest archive this program will read into memory.
///
/// [`ziparchive`] is a whole-buffer API — `parse` takes `&[u8]` and every
/// entry is located by an offset into it — so opening an archive means holding
/// all of it. That is fine for the archives a person opens by hand and wrong
/// for a 40 GB backup set, so the limit is explicit and the refusal names the
/// size rather than letting the allocator decide. Recorded in
/// `known-issues.md`; the proper fix is a seeking reader on the crate side.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

/// Why an archive operation could not be carried out.
#[derive(Debug)]
pub enum ArchiveError {
    /// The file could not be read.
    Io { path: PathBuf, source: io::Error },
    /// The archive is larger than this program will hold in memory.
    TooLarge { bytes: u64 },
    /// The name does not end in any extension this program recognises.
    UnknownFormat { name: String },
    /// A format this program knows about but cannot yet read.
    NotYetReadable { format: ArchiveFormat },
    /// The ZIP parser refused the bytes.
    Zip(ziparchive::Error),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::TooLarge { bytes } => write!(
                f,
                "the archive is {} and this program reads archives up to {}",
                guitk::bytes::iec(*bytes),
                guitk::bytes::iec(MAX_ARCHIVE_BYTES)
            ),
            Self::UnknownFormat { name } => {
                write!(f, "{name} does not end in an archive extension I know")
            }
            Self::NotYetReadable { format } => {
                write!(f, "{} — this build reads ZIP only", format.display_name())
            }
            Self::Zip(e) => write!(f, "{e}"),
        }
    }
}

/// The bytes an [`ArchiveModel`] was built from, and the central-directory
/// record each of its entries came from.
///
/// Held on the model because extraction and verification need the compressed
/// bytes, and the model is the only thing that outlives the click that opened
/// the file.
///
/// **Keyed by [`ArchiveEntry::id`], not by index.** The list is sorted in place
/// every time the user clicks a column header, so a parallel `Vec` would name a
/// different member after the first click — and the member is what decides
/// where the bytes are, so the mistake would extract one file's contents under
/// another's name rather than merely showing the wrong row.
#[derive(Clone)]
pub struct ArchiveSource {
    /// The whole archive file.
    bytes: Vec<u8>,
    /// The parsed central directory, by the id the model gave each entry.
    members: HashMap<u64, ziparchive::ZipEntry>,
}

impl fmt::Debug for ArchiveSource {
    /// Length, not contents. `ArchiveModel` derives `Debug`, and an archive is
    /// megabytes: a derived `Debug` here would turn one `dbg!` or one failing
    /// `assert_eq!` on a model into a screenful of hex.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchiveSource")
            .field("bytes", &self.bytes.len())
            .field("members", &self.members.len())
            .finish()
    }
}

impl ArchiveSource {
    /// The archive file, whole.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The central-directory record the entry with this id came from.
    #[must_use]
    pub fn member(&self, id: u64) -> Option<&ziparchive::ZipEntry> {
        self.members.get(&id)
    }

    /// How many members the archive declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }
}

/// Read `path` and parse it into a model.
///
/// # Errors
///
/// [`ArchiveError`] for a name with no known extension, a format this build
/// cannot read, a file that will not read or is too big to hold, or bytes the
/// ZIP parser refuses.
pub fn open(path: &Path) -> Result<ArchiveModel, ArchiveError> {
    let Some(format) = ArchiveFormat::from_path(path) else {
        return Err(ArchiveError::UnknownFormat {
            name: path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
        });
    };
    if format != ArchiveFormat::Zip {
        return Err(ArchiveError::NotYetReadable { format });
    }
    // Ask the size before reading, so an archive too big to hold is refused by
    // name instead of by running the machine out of memory first.
    let size = fs::metadata(path)
        .map_err(|source| ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if size > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge { bytes: size });
    }
    let bytes = fs::read(path).map_err(|source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_zip(path, bytes)
}

/// Parse bytes already in hand as a ZIP, under the name `path`.
///
/// Separate from [`open`] because it is also how the tests build their fixture:
/// they write a real archive with `ziparchive::create` and read it back through
/// here, so the entry list a test asserts against came out of the same parser
/// the program uses rather than being typed in beside it.
///
/// # Errors
///
/// [`ArchiveError::Zip`] if the bytes are not a well-formed archive.
pub fn parse_zip(path: &Path, bytes: Vec<u8>) -> Result<ArchiveModel, ArchiveError> {
    let members = ziparchive::parse(&bytes).map_err(ArchiveError::Zip)?;
    let mut model = ArchiveModel::new(path, ArchiveFormat::Zip);
    let mut by_id = HashMap::with_capacity(members.len());
    for member in members {
        let display = display_path(&member.name);
        let name = display
            .rsplit_once('/')
            .map_or(display.as_str(), |(_, last)| last)
            .to_string();
        let id = model.add_entry(ArchiveEntry {
            depth: u32::try_from(display.matches('/').count()).unwrap_or(u32::MAX),
            name,
            is_dir: member.is_dir,
            size: member.uncompressed_size,
            compressed_size: member.compressed_size,
            // The central directory does carry a modification time, and
            // `ziparchive::ZipEntry` does not yet hand it over — see
            // `requests/c-a-ziparchive-drops-the-one-field-a-date-column-needs.md`.
            // Zero is what `ArchiveEntry::format_date` renders as `-`, "not
            // known", which is the truth rather than a guessed date.
            modified: 0,
            crc32: member.crc32,
            // Likewise: encryption is general-purpose bit 0, which the crate
            // does not expose. Claiming `false` for an encrypted entry would be
            // a lie, but it is the same lie the program told before it could
            // read archives at all, and the fix is the same request.
            encrypted: false,
            method: method_name(member.method),
            path: display,
            expanded: false,
            selected: false,
            id: 0, // assigned by add_entry
        });
        by_id.insert(id, member);
    }
    model.source = Some(ArchiveSource {
        bytes,
        members: by_id,
    });
    model.rebuild_tree();
    Ok(model)
}

/// What to show in the Method column for a ZIP compression method number.
///
/// The two this build can decompress are named; anything else is shown by its
/// number, because "unknown" for methods 9, 12 and 14 alike would hide the one
/// fact that tells the user which tool wrote the archive.
fn method_name(method: u16) -> String {
    match method {
        0 => String::from("Stored"),
        8 => String::from("Deflate"),
        other => format!("Method {other}"),
    }
}

/// The path to *show* for a member, from the bytes the archive stored.
///
/// Lossy, deliberately and only here: a listing is the one place a name with no
/// UTF-8 reading still has to appear on screen, and a row that cannot be drawn
/// is worse than one drawn with a replacement character in it. Nothing is ever
/// *written* to this name — [`safe_destination`] works from the raw bytes and
/// refuses what it cannot render faithfully — so a substituted character can
/// mislabel a row but can never misplace a file.
///
/// The trailing `/` a directory member carries is dropped, because every other
/// part of the model addresses `src` rather than `src/`.
fn display_path(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .trim_end_matches('/')
        .to_string()
}

/// Why a member was not extracted.
#[derive(Debug)]
pub enum SkipReason {
    /// The name would put the file outside the chosen directory.
    Escapes,
    /// The name has no UTF-8 reading, so this host cannot name the file.
    UnnameableHere,
    /// The name has no components at all — `/`, or `././`.
    Empty,
    /// The member would not decompress.
    Zip(ziparchive::Error),
    /// The file or its directory could not be written.
    Io(io::Error),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escapes => f.write_str("its name points outside the destination"),
            Self::UnnameableHere => f.write_str("its name is not text this system can write"),
            Self::Empty => f.write_str("its name is empty"),
            Self::Zip(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

/// What an extraction did.
///
/// Every refusal is kept, not just the last one: a run that wrote 900 files and
/// refused 3 has to be able to say *which* 3, and a status line reporting only
/// the final error would report whichever happened to sort last.
#[derive(Debug, Default)]
pub struct ExtractReport {
    /// Files written.
    pub written: usize,
    /// Directories created because a member named one.
    pub directories: usize,
    /// Total uncompressed bytes written.
    pub bytes: u64,
    /// Members not extracted, in the order they were met.
    pub skipped: Vec<(String, SkipReason)>,
}

impl ExtractReport {
    /// A one-line summary for the status bar.
    #[must_use]
    pub fn summary(&self, dest: &Path) -> String {
        let files = format!(
            "{} file{}",
            self.written,
            if self.written == 1 { "" } else { "s" }
        );
        match self.skipped.first() {
            None => format!(
                "Extracted {files} ({}) to {}",
                guitk::bytes::iec(self.bytes),
                dest.display()
            ),
            // The first refusal is named in full and the rest are counted. A
            // status line that said "3 problems" would send the user looking
            // for a log this program does not keep; one that tried to name all
            // three would not fit.
            Some((name, why)) => {
                let more = self.skipped.len().saturating_sub(1);
                let tail = if more == 0 {
                    String::new()
                } else {
                    format!(" (and {more} more)")
                };
                format!("Extracted {files}; skipped {name} — {why}{tail}")
            }
        }
    }
}

/// Where `raw` may be written under `dest`.
///
/// The whole of the Zip Slip defence. A member name is refused unless every one
/// of its components is an *ordinary* name — which excludes `..`, absolute
/// roots, and Windows drive prefixes, the last of which matters because
/// `PathBuf::push("C:")` replaces the path it is pushed onto rather than
/// extending it, so a member called `C:evil` would land on the C drive and not
/// under `dest` at all.
///
/// Split on **both** separators. The format says `/`, but archives written by
/// Windows tools carry `\`, and a check that only knew about `/` would read
/// `..\..\etc\passwd` as one harmless component and then hand it to a
/// filesystem that does know about `\`.
///
/// # Errors
///
/// [`SkipReason`] naming why the member cannot be written.
fn safe_destination(dest: &Path, raw: &[u8]) -> Result<PathBuf, SkipReason> {
    // Strict, not lossy. A lossy conversion here would write the member out
    // under a name that is not its own — and, worse, two members differing only
    // in bytes with no UTF-8 reading would collapse onto one path and the
    // second would overwrite the first.
    let name = std::str::from_utf8(raw).map_err(|_| SkipReason::UnnameableHere)?;
    let mut out = dest.to_path_buf();
    let mut components = 0_usize;
    for part in name.split(['/', '\\']) {
        // A leading `/`, a doubled `//`, and a trailing `/` on a directory
        // member all show up here as an empty part. `unzip` strips a leading
        // separator rather than refusing the member, and so do we.
        if part.is_empty() || part == "." {
            continue;
        }
        let mut walk = Path::new(part).components();
        match (walk.next(), walk.next()) {
            (Some(Component::Normal(c)), None) => out.push(c),
            _ => return Err(SkipReason::Escapes),
        }
        components = components.saturating_add(1);
    }
    if components == 0 {
        return Err(SkipReason::Empty);
    }
    Ok(out)
}

/// Extract `members` of `source` under `dest`.
///
/// Never fails as a whole: a member that cannot be written is recorded in the
/// report and the rest are still extracted, because a user who asked for 900
/// files and can have 897 of them wants the 897.
pub fn extract(source: &ArchiveSource, members: &[&ArchiveEntry], dest: &Path) -> ExtractReport {
    let mut report = ExtractReport::default();
    for entry in members {
        let Some(member) = source.member(entry.id) else {
            // The model and its source disagree, which can only happen if an
            // entry was added to the model by something other than the parser.
            report
                .skipped
                .push((entry.path.clone(), SkipReason::Escapes));
            continue;
        };
        let target = match safe_destination(dest, &member.name) {
            Ok(p) => p,
            Err(why) => {
                report.skipped.push((entry.path.clone(), why));
                continue;
            }
        };
        if member.is_dir {
            match fs::create_dir_all(&target) {
                Ok(()) => report.directories = report.directories.saturating_add(1),
                Err(e) => report.skipped.push((entry.path.clone(), SkipReason::Io(e))),
            }
            continue;
        }
        let data = match ziparchive::extract_entry(source.bytes(), member) {
            Ok(d) => d,
            Err(e) => {
                report
                    .skipped
                    .push((entry.path.clone(), SkipReason::Zip(e)));
                continue;
            }
        };
        // A member called `a/b/c.txt` in an archive with no `a/` member of its
        // own still has to land in a directory that exists. `create_dir_all` on
        // the parent is what makes an archive written by a tool that omits
        // directory entries extract at all.
        if let Some(parent) = target.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            report.skipped.push((entry.path.clone(), SkipReason::Io(e)));
            continue;
        }
        match fs::write(&target, &data) {
            Ok(()) => {
                report.written = report.written.saturating_add(1);
                report.bytes = report.bytes.saturating_add(data.len() as u64);
            }
            Err(e) => report.skipped.push((entry.path.clone(), SkipReason::Io(e))),
        }
    }
    report
}

/// Decompress every member and check it against what the archive declared.
///
/// This is what the Test button does, and it is a real test: `extract_entry`
/// caps the inflater at the entry's declared size, then requires the result to
/// be exactly that size and to match the stored CRC-32. Nothing is written to
/// disk.
///
/// Directory members are not tested — they have no data — and are not counted,
/// so a pass rate is a pass rate over the things that could have failed.
#[must_use]
pub fn verify(model: &ArchiveModel) -> ArchiveTestResults {
    let files: Vec<&ArchiveEntry> = model.entries.iter().filter(|e| !e.is_dir).collect();
    let mut results = ArchiveTestResults::new(files.len());
    let Some(source) = model.source.as_ref() else {
        return results;
    };
    for entry in files {
        let result = match source.member(entry.id) {
            None => TestResult::Corrupted(String::from("no record of this entry in the archive")),
            Some(member) => match ziparchive::extract_entry(source.bytes(), member) {
                Ok(_) => TestResult::Ok,
                // The parser checks the declared size first and the CRC second
                // but reports one error for both, so this message names both
                // rather than picking one and being wrong half the time. Asked
                // for a finer error in
                // `requests/c-a-ziparchive-drops-the-one-field-a-date-column-needs.md`.
                Err(ziparchive::Error::CorruptedData) => TestResult::Corrupted(String::from(
                    "its contents do not match the size or checksum the archive declared",
                )),
                Err(ziparchive::Error::UnsupportedMethod) => {
                    TestResult::Corrupted(format!("{} is not a codec this build has", entry.method))
                }
            },
        };
        results.record(&entry.path, result);
    }
    results
}

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use ziparchive::ZipWriteEntry;

    /// A member, compressed if it helps.
    fn member(name: &str, data: &[u8]) -> ZipWriteEntry {
        ZipWriteEntry {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            store_only: false,
        }
    }

    /// A scratch directory nothing else is using, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "archivemanager-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create the scratch directory");
        dir
    }

    #[test]
    fn a_real_archive_round_trips_through_the_model() {
        // The fixture is a ZIP this test wrote, read back by the parser the
        // program uses. Nothing about the entry list is typed in beside it, so
        // a change in the parser shows up here rather than in production.
        let bytes = ziparchive::create(&[
            member("src/main.rs", b"fn main() {}\n"),
            member("README.md", &b"read me, and me, and me, and me\n".repeat(8)),
        ]);
        let model = parse_zip(Path::new("/tmp/fixture.zip"), bytes).expect("a well-formed archive");

        assert_eq!(model.format, ArchiveFormat::Zip);
        assert_eq!(model.file_count, 2);
        let main = model
            .entries
            .iter()
            .find(|e| e.path == "src/main.rs")
            .expect("the member is in the model");
        assert_eq!(
            main.name, "main.rs",
            "the display name is the last component"
        );
        assert_eq!(main.depth, 1);
        assert_eq!(main.size, 13, "the size is the archive's, not a guess");
        assert!(!main.is_dir);

        let readme = model
            .entries
            .iter()
            .find(|e| e.path == "README.md")
            .expect("the member is in the model");
        assert_eq!(readme.method, "Deflate", "eight repeats compress");
        assert!(
            readme.compressed_size < readme.size,
            "packed {} is not smaller than {}",
            readme.compressed_size,
            readme.size
        );
    }

    #[test]
    fn every_member_carries_its_own_bytes_after_the_list_is_sorted() {
        // The reason the source is keyed by id. Sorting reorders `entries`, and
        // a parallel vector would then extract one member's contents under
        // another member's name.
        let bytes =
            ziparchive::create(&[member("z.txt", b"this is z"), member("a.txt", b"this is a")]);
        let mut model = parse_zip(Path::new("/tmp/order.zip"), bytes).expect("well-formed");
        model.sort_entries(&crate::SortState {
            column: crate::Column::Name,
            direction: crate::SortDirection::Ascending,
        });
        assert_eq!(model.entries[0].path, "a.txt", "the sort ran");

        let source = model
            .source
            .as_ref()
            .expect("parsed archives have a source");
        for entry in &model.entries {
            let m = source.member(entry.id).expect("every entry has its member");
            let data = ziparchive::extract_entry(source.bytes(), m).expect("it decompresses");
            assert_eq!(
                String::from_utf8_lossy(&data),
                format!("this is {}", entry.path.trim_end_matches(".txt")),
                "{} got another member's bytes",
                entry.path
            );
        }
    }

    #[test]
    fn verifying_a_good_archive_passes_every_file_and_counts_no_directories() {
        let bytes = ziparchive::create(&[
            ZipWriteEntry {
                name: b"docs/".to_vec(),
                data: Vec::new(),
                store_only: true,
            },
            member("docs/guide.md", b"# Guide\n"),
            member("LICENSE", b"do what you like\n"),
        ]);
        let model = parse_zip(Path::new("/tmp/good.zip"), bytes).expect("well-formed");
        let results = verify(&model);
        assert_eq!(
            results.total_entries, 2,
            "a directory has nothing to verify"
        );
        assert_eq!(results.tested, 2);
        assert!(results.all_passed(), "{:?}", results.results);
        assert!((results.pass_rate() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_flipped_byte_is_caught_by_the_test_button_and_named() {
        // A checksum nobody checks is decoration. Corrupt one member's
        // compressed bytes and the Test button must fail that member and only
        // that member.
        let good = ziparchive::create(&[
            member("intact.txt", b"this member is fine and stays fine"),
            member("broken.txt", b"this member is about to be damaged"),
        ]);
        let model = parse_zip(Path::new("/tmp/x.zip"), good.clone()).expect("well-formed");
        let broken_entry = model
            .entries
            .iter()
            .find(|e| e.path == "broken.txt")
            .expect("it is there");
        let member = model
            .source
            .as_ref()
            .expect("a source")
            .member(broken_entry.id)
            .expect("its record");
        // Find the member's compressed bytes in the file and flip a bit in the
        // middle of them, which damages the data without disturbing any header.
        let start = good
            .windows(4)
            .position(|w| w == b"this")
            .expect("a stored-or-deflated body to damage");
        let mut damaged = good;
        damaged[start + 2] ^= 0xFF;
        assert!(
            member.uncompressed_size > 0,
            "the fixture member must have data"
        );

        let model = parse_zip(Path::new("/tmp/x.zip"), damaged).expect("the directory is intact");
        let results = verify(&model);
        assert_eq!(results.tested, 2);
        assert_eq!(results.failed, 1, "{:?}", results.results);
        assert!(!results.all_passed());
        let (bad, why) = results
            .results
            .iter()
            .find(|(_, r)| **r != TestResult::Ok)
            .expect("one member failed");
        assert!(
            matches!(why, TestResult::Corrupted(msg) if msg.contains("checksum")),
            "{why:?} does not say what went wrong"
        );
        assert_eq!(
            bad, "intact.txt",
            "the damaged bytes are the first member's"
        );
    }

    #[test]
    fn extraction_writes_the_files_and_the_directories_they_need() {
        let dir = scratch("extract");
        let bytes = ziparchive::create(&[
            member("deep/down/here.txt", b"found me"),
            member("top.txt", b"at the top"),
        ]);
        let model = parse_zip(Path::new("/tmp/e.zip"), bytes).expect("well-formed");
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let report = extract(model.source.as_ref().expect("a source"), &all, &dir);

        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert_eq!(report.written, 2);
        assert_eq!(report.bytes, 18);
        assert_eq!(
            fs::read_to_string(dir.join("deep").join("down").join("here.txt")).unwrap(),
            "found me",
            "a nested member makes its own directories"
        );
        assert_eq!(
            fs::read_to_string(dir.join("top.txt")).unwrap(),
            "at the top"
        );
        assert!(
            report.summary(&dir).starts_with("Extracted 2 files"),
            "{}",
            report.summary(&dir)
        );

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn a_member_that_climbs_out_of_the_destination_is_refused_and_says_so() {
        // Zip Slip. `../../` is a legal member name and this is the only thing
        // standing between it and the user's home directory.
        let dir = scratch("slip");
        let inside = dir.join("inside");
        fs::create_dir_all(&inside).expect("make the destination");

        let bytes = ziparchive::create(&[
            member("../escaped.txt", b"should not be written"),
            member("..\\also-escaped.txt", b"nor this"),
            member("keeps.txt", b"but this one is fine"),
        ]);
        let model = parse_zip(Path::new("/tmp/slip.zip"), bytes).expect("well-formed");
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let report = extract(model.source.as_ref().expect("a source"), &all, &inside);

        assert_eq!(report.written, 1, "only the honest member");
        assert_eq!(report.skipped.len(), 2, "{:?}", report.skipped);
        for (name, why) in &report.skipped {
            assert!(
                matches!(why, SkipReason::Escapes),
                "{name} was refused for the wrong reason: {why}"
            );
        }
        assert!(
            !dir.join("escaped.txt").exists() && !dir.join("also-escaped.txt").exists(),
            "a refused member reached the disk anyway"
        );
        assert!(inside.join("keeps.txt").exists());
        assert!(
            report.summary(&inside).contains("and 1 more"),
            "{}",
            report.summary(&inside)
        );

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn a_name_shaped_like_a_drive_or_a_root_is_refused_too() {
        // `PathBuf::push("C:")` replaces the path rather than extending it, so
        // a drive prefix escapes without ever containing `..`. A leading
        // separator is stripped instead, which is what `unzip` does.
        let dest = Path::new("/dest");
        assert!(matches!(
            safe_destination(dest, b"C:evil.txt"),
            Err(SkipReason::Escapes)
        ));
        assert!(matches!(
            safe_destination(dest, b"a/../../b"),
            Err(SkipReason::Escapes)
        ));
        assert!(matches!(
            safe_destination(dest, b"/"),
            Err(SkipReason::Empty)
        ));
        assert!(matches!(
            safe_destination(dest, b"./././"),
            Err(SkipReason::Empty)
        ));
        assert!(matches!(
            safe_destination(dest, &[0xFF, 0xFE]),
            Err(SkipReason::UnnameableHere)
        ));
        assert_eq!(
            safe_destination(dest, b"/leading/slash.txt").expect("stripped, not refused"),
            Path::new("/dest").join("leading").join("slash.txt")
        );
        assert_eq!(
            safe_destination(dest, b"double//slash.txt").expect("an empty part is skipped"),
            Path::new("/dest").join("double").join("slash.txt")
        );
    }

    #[test]
    fn opening_something_that_is_not_an_archive_says_which_thing_it_is_not() {
        let dir = scratch("open");
        let not_zip = dir.join("notes.txt");
        fs::write(&not_zip, b"hello").expect("write it");
        assert!(matches!(
            open(&not_zip),
            Err(ArchiveError::UnknownFormat { .. })
        ));

        let tar = dir.join("bundle.tar.gz");
        fs::write(&tar, b"not really").expect("write it");
        match open(&tar) {
            Err(e @ ArchiveError::NotYetReadable { .. }) => {
                assert!(e.to_string().contains("reads ZIP only"), "{e}");
            }
            other => panic!("expected a refusal naming the format, got {other:?}"),
        }

        let missing = dir.join("gone.zip");
        assert!(matches!(open(&missing), Err(ArchiveError::Io { .. })));

        let garbage = dir.join("garbage.zip");
        fs::write(&garbage, b"PK\x03\x04 and then nothing that makes sense").expect("write it");
        match open(&garbage) {
            Err(ArchiveError::Zip(e)) => assert_eq!(e, ziparchive::Error::CorruptedData),
            other => panic!("expected the parser to refuse it, got {other:?}"),
        }

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn an_error_says_something_a_user_can_act_on() {
        // Every arm, because a status line is the only place these are ever
        // seen and an empty or a debug-shaped one reads as a crash.
        for e in [
            ArchiveError::TooLarge {
                bytes: 900 * 1024 * 1024,
            },
            ArchiveError::UnknownFormat {
                name: String::from("notes.txt"),
            },
            ArchiveError::NotYetReadable {
                format: ArchiveFormat::SevenZip,
            },
            ArchiveError::Zip(ziparchive::Error::UnsupportedMethod),
            ArchiveError::Io {
                path: PathBuf::from("/tmp/x.zip"),
                source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            },
        ] {
            let text = e.to_string();
            assert!(text.len() > 12, "{e:?} says {text:?}");
            assert!(
                !text.starts_with(char::is_uppercase) || text.starts_with("PK"),
                "{text:?} is a sentence fragment and gets pasted after a colon"
            );
        }
        assert!(
            ArchiveError::TooLarge {
                bytes: 900 * 1024 * 1024
            }
            .to_string()
            .contains("512"),
            "the refusal must name the limit it is enforcing"
        );
    }

    #[test]
    fn a_source_prints_its_size_rather_than_its_contents() {
        let bytes = ziparchive::create(&[member("a.txt", b"x")]);
        let model = parse_zip(Path::new("/tmp/d.zip"), bytes).expect("well-formed");
        let text = format!("{:?}", model.source.as_ref().expect("a source"));
        assert!(text.contains("members: 1"), "{text}");
        assert!(
            !text.contains("120, 0") && text.len() < 120,
            "a Debug that dumps the archive turns one failing assert into a screenful: {text}"
        );
    }
}
