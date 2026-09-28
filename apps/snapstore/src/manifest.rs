//! What a snapshot holds, and the record beside it: the two files written for
//! every snapshot, in the format `apps/backup` has always written them.
//!
//! **Versions.** A manifest with no `version` is version 1, whose paths were
//! written verbatim; version 2 percent-encodes every path (design-decisions
//! 426), so a name that is not text is recorded whole. Version 3 (2026-09-27)
//! makes every manifest *complete* -- it lists every file of the source, not
//! only what changed since a parent -- and adds what a restore that makes a
//! folder match the snapshot needs: each file's mode, the directories, the
//! paths that could not be read, and the exclusion patterns the capture used.
//! Every older manifest still reads.

use std::path::PathBuf;

use pathcodec::{decode_path, encode_path};

use crate::json::{JsonValue, json_parse, json_pretty};

/// Manifest format version written into every new manifest. See the module
/// docs for what each version added.
pub const MANIFEST_VERSION: u64 = 3;

/// Record (`meta.json`) format version written into every new record.
///
/// Version 1 stored `source` as a plain JSON string, which it had reached
/// through `to_string_lossy` -- so the record of *which directory was backed
/// up* named a different directory whenever that path was not text. Version 2
/// stores it percent-encoded, as manifests do. A record with no version is
/// version 1 and is read verbatim, so every existing snapshot still lists.
pub const META_VERSION: u64 = 2;

/// How a snapshot was taken relative to the ones before it.
///
/// Since manifest version 3 every snapshot lists every file, so the kinds
/// differ only in which earlier snapshot's hashes were reused for files that
/// had not changed -- none, the latest of any kind, or the latest full one.
/// For an older manifest the kind still decides whether the listing is
/// complete: an incremental or differential one before version 3 held only
/// what changed, and its files are found by walking back to a full one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupType {
    /// Every file hashed from the source.
    Full,
    /// Unchanged files' hashes taken from the latest snapshot.
    Incremental,
    /// Unchanged files' hashes taken from the latest full snapshot.
    Differential,
}

impl BackupType {
    /// The word written to disk and printed.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            BackupType::Full => "full",
            BackupType::Incremental => "incremental",
            BackupType::Differential => "differential",
        }
    }

    /// The kind [`BackupType::as_str`] names, if it names one.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(BackupType::Full),
            "incremental" => Some(BackupType::Incremental),
            "differential" => Some(BackupType::Differential),
            _ => None,
        }
    }
}

impl std::fmt::Display for BackupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One file, or one symbolic link, in a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative path from the snapshot's source, `/`-separated.
    ///
    /// A `PathBuf`, not a `String`: paths on this OS may contain any byte
    /// except `/` and NUL, and a snapshot that cannot record the name of a
    /// file it copied cannot restore it either.
    pub path: PathBuf,
    /// Size in bytes of what was stored.
    pub size: u64,
    /// Modification time, seconds since the UNIX epoch.
    pub mtime: u64,
    /// SHA-256 of what was stored, in hex: the blob's name. For a link, the
    /// hash of its target, which is carried here rather than as a blob.
    pub hash: String,
    /// Whether this entry is a symbolic link.
    pub is_symlink: bool,
    /// The link's target, if it is one. Also an arbitrary byte string.
    pub link_target: Option<PathBuf>,
    /// The permission bits, where the platform has them (version 3 on).
    ///
    /// Recorded because a restore that recreates a file somebody had deleted
    /// would otherwise give it the default mode -- and a private file (a
    /// credential vault written `0600`) would come back readable by everyone.
    pub mode: Option<u32>,
}

impl FileEntry {
    /// This entry as the object written to the manifest on disk.
    ///
    /// A field missing from here is a field the snapshot does not record, and
    /// so one the restore cannot put back: the value is set, the capture
    /// reports success, and the loss surfaces only when someone needs the
    /// file. Exhaustive (no `..`), so a new field stops this compiling until
    /// someone decides whether it belongs on disk -- see known-issues.md
    /// lesson 44.
    pub(crate) fn to_json(&self) -> JsonValue {
        let Self {
            path,
            size,
            mtime,
            hash,
            is_symlink,
            link_target,
            mode,
        } = self;
        #[allow(
            clippy::cast_precision_loss,
            reason = "the format stores numbers as JSON numbers; sizes and times this \
                      large (past 2^53) do not occur, and the reader rounds the same way"
        )]
        let mut entries = vec![
            ("path".to_string(), JsonValue::Str(encode_path(path))),
            ("size".to_string(), JsonValue::Number(*size as f64)),
            ("mtime".to_string(), JsonValue::Number(*mtime as f64)),
            ("hash".to_string(), JsonValue::Str(hash.clone())),
            ("is_symlink".to_string(), JsonValue::Bool(*is_symlink)),
        ];
        // Absent rather than null when there is no target: `from_json` reads
        // this key with `.and_then`, so absent and null mean the same to the
        // reader, and a non-link has no target to record.
        if let Some(target) = link_target {
            entries.push((
                "link_target".to_string(),
                JsonValue::Str(encode_path(target)),
            ));
        }
        if let Some(mode) = mode {
            entries.push(("mode".to_string(), JsonValue::Number(f64::from(*mode))));
        }
        JsonValue::Object(entries)
    }

    /// Read one entry. `encoded` selects the path representation: version 2
    /// on percent-encodes, version 1 stored the path verbatim.
    pub(crate) fn from_json(val: &JsonValue, encoded: bool) -> Option<Self> {
        let read_path = |s: &str| {
            if encoded {
                decode_path(s)
            } else {
                PathBuf::from(s)
            }
        };
        let path = read_path(val.get("path")?.as_str()?);
        let size = val.get("size")?.as_u64()?;
        let mtime = val.get("mtime")?.as_u64()?;
        let hash = val.get("hash")?.as_str()?.to_string();
        let is_symlink = val
            .get("is_symlink")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let link_target = val
            .get("link_target")
            .and_then(JsonValue::as_str)
            .map(&read_path);
        // A mode that is present and not a small whole number is a corrupt
        // manifest, not an absent mode: refusing it keeps a restore from
        // applying a permission nobody recorded.
        let mode = match val.get("mode") {
            None | Some(JsonValue::Null) => None,
            Some(v) => Some(u32::try_from(v.as_u64()?).ok()?),
        };
        Some(FileEntry {
            path,
            size,
            mtime,
            hash,
            is_symlink,
            link_target,
            mode,
        })
    }
}

/// Everything one snapshot holds.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Manifest {
    /// Every file and link, when [`Manifest::complete`]; before version 3, an
    /// incremental or differential snapshot's changes only.
    pub files: Vec<FileEntry>,
    /// Every directory under the source, relative, so a restore can put back
    /// an empty one and knows which it may remove (version 3 on).
    pub dirs: Vec<PathBuf>,
    /// Paths under the source that were there and could not be read, with
    /// why. Not in the snapshot -- and so never removed by a restore that
    /// makes the folder match it, since nobody knows what they held.
    pub unread: Vec<(PathBuf, String)>,
    /// The exclusion patterns the capture used. What they matched is not in
    /// the snapshot on purpose, and a restore leaves it alone.
    pub excluded: Vec<String>,
    /// Whether `files` lists every file of the source. True for everything
    /// written from version 3 on; for an older manifest, true only when the
    /// record says the snapshot was full, which the store decides (the
    /// manifest alone does not know).
    pub complete: bool,
    /// Whether `dirs` is a record of the folders, rather than empty because
    /// the manifest predates recording them. A restore removes a folder the
    /// snapshot did not have only when it knows which folders it had.
    pub dirs_recorded: bool,
}

impl Manifest {
    /// The manifest as written to disk.
    ///
    /// Destructured for the reason [`FileEntry::to_json`] gives. `version` is
    /// in the object but not in the struct: it is a property of the format
    /// being written, which is always the current one.
    pub(crate) fn to_json(&self) -> JsonValue {
        let Self {
            files,
            dirs,
            unread,
            excluded,
            complete,
            dirs_recorded,
        } = self;
        #[allow(
            clippy::cast_precision_loss,
            reason = "a small constant, exactly representable"
        )]
        let version = JsonValue::Number(MANIFEST_VERSION as f64);
        let mut entries = vec![
            ("version".to_string(), version),
            ("complete".to_string(), JsonValue::Bool(*complete)),
            (
                "files".to_string(),
                JsonValue::Array(files.iter().map(FileEntry::to_json).collect()),
            ),
            (
                "unread".to_string(),
                JsonValue::Array(
                    unread
                        .iter()
                        .map(|(path, why)| {
                            JsonValue::Object(vec![
                                ("path".to_string(), JsonValue::Str(encode_path(path))),
                                ("why".to_string(), JsonValue::Str(why.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "excluded".to_string(),
                JsonValue::Array(excluded.iter().cloned().map(JsonValue::Str).collect()),
            ),
        ];
        // Written only when the folders were recorded: an empty list would say
        // "there were no folders", which a manifest that never looked cannot
        // say -- and a restore would remove every folder on the strength of it.
        if *dirs_recorded {
            entries.push((
                "dirs".to_string(),
                JsonValue::Array(
                    dirs.iter()
                        .map(|d| JsonValue::Str(encode_path(d)))
                        .collect(),
                ),
            ));
        }
        JsonValue::Object(entries)
    }

    /// Read a manifest of any version.
    pub(crate) fn from_json(val: &JsonValue) -> Option<Self> {
        // A manifest with no version field predates path escaping and stored
        // paths verbatim. Reading it is still worth doing: refusing would
        // strand every snapshot taken before that change.
        let version = val.get("version").and_then(JsonValue::as_u64).unwrap_or(1);
        let encoded = version >= 2;
        let mut files = Vec::new();
        for fv in val.get("files")?.as_array()? {
            files.push(FileEntry::from_json(fv, encoded)?);
        }
        // The version-3 lists are absent from older manifests, which is what
        // they held: no directories recorded, nothing recorded as unread.
        let mut dirs = Vec::new();
        let dirs_recorded = val.get("dirs").is_some();
        if let Some(list) = val.get("dirs") {
            for d in list.as_array()? {
                dirs.push(decode_path(d.as_str()?));
            }
        }
        let mut unread = Vec::new();
        if let Some(list) = val.get("unread") {
            for u in list.as_array()? {
                let path = decode_path(u.get("path")?.as_str()?);
                let why = u.get("why")?.as_str()?.to_string();
                unread.push((path, why));
            }
        }
        let mut excluded = Vec::new();
        if let Some(list) = val.get("excluded") {
            for e in list.as_array()? {
                excluded.push(e.as_str()?.to_string());
            }
        }
        let complete = val
            .get("complete")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        Some(Manifest {
            files,
            dirs,
            unread,
            excluded,
            complete,
            dirs_recorded,
        })
    }

    /// The manifest as the text written to `manifest.json`.
    #[must_use]
    pub fn serialize(&self) -> String {
        json_pretty(&self.to_json(), 2)
    }

    /// Read `manifest.json`'s text.
    ///
    /// # Errors
    ///
    /// Says what is wrong with text that is not a manifest.
    pub fn deserialize(input: &str) -> Result<Self, String> {
        let val = json_parse(input)?;
        Self::from_json(&val).ok_or_else(|| "invalid manifest structure".to_string())
    }
}

/// The record written beside a snapshot, last: its presence marks the
/// snapshot as complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupMeta {
    /// The snapshot's id, unique in its store: its directory's name.
    pub id: String,
    /// How it was taken.
    pub backup_type: BackupType,
    /// When, in seconds since the UNIX epoch.
    pub timestamp: u64,
    /// The directory the snapshot is of.
    ///
    /// A `PathBuf`, for the reason [`FileEntry::path`] is one: this is the
    /// record of a directory, and a directory's name need not be text.
    pub source: PathBuf,
    /// The snapshot whose hashes were reused, if any.
    pub parent_id: Option<String>,
    /// How many files and links it holds.
    pub file_count: u64,
    /// The total size of its files.
    pub total_size: u64,
    /// How many blobs it added to the store.
    pub new_blobs: u64,
    /// How many of its files were already in the store.
    pub dedup_blobs: u64,
    /// How many paths could not be read, and so are not in it.
    pub unread_count: u64,
}

impl BackupMeta {
    /// This record as the object written beside the snapshot on disk.
    ///
    /// Held to the struct by the destructure, for the reason
    /// [`FileEntry::to_json`] gives: a field omitted here is not recorded, and
    /// on the next read comes back as whatever `from_json` defaults it to -- a
    /// wrong number reported confidently, which is worse than a missing one.
    pub(crate) fn to_json(&self) -> JsonValue {
        let Self {
            id,
            backup_type,
            timestamp,
            source,
            parent_id,
            file_count,
            total_size,
            new_blobs,
            dedup_blobs,
            unread_count,
        } = self;
        #[allow(
            clippy::cast_precision_loss,
            reason = "counts, sizes and times past 2^53 do not occur; the format is JSON numbers"
        )]
        let mut entries = vec![
            (
                "version".to_string(),
                JsonValue::Number(META_VERSION as f64),
            ),
            ("id".to_string(), JsonValue::Str(id.clone())),
            (
                "backup_type".to_string(),
                JsonValue::Str(backup_type.as_str().to_string()),
            ),
            (
                "timestamp".to_string(),
                JsonValue::Number(*timestamp as f64),
            ),
            ("source".to_string(), JsonValue::Str(encode_path(source))),
            (
                "file_count".to_string(),
                JsonValue::Number(*file_count as f64),
            ),
            (
                "total_size".to_string(),
                JsonValue::Number(*total_size as f64),
            ),
            (
                "new_blobs".to_string(),
                JsonValue::Number(*new_blobs as f64),
            ),
            (
                "dedup_blobs".to_string(),
                JsonValue::Number(*dedup_blobs as f64),
            ),
            (
                "unread_count".to_string(),
                JsonValue::Number(*unread_count as f64),
            ),
        ];
        // Explicitly null rather than absent, unlike `FileEntry::link_target`:
        // a snapshot having no parent is a fact about it, worth being able to
        // read back off the disk rather than infer from a missing key.
        match parent_id {
            Some(pid) => entries.push(("parent_id".to_string(), JsonValue::Str(pid.clone()))),
            None => entries.push(("parent_id".to_string(), JsonValue::Null)),
        }
        JsonValue::Object(entries)
    }

    pub(crate) fn from_json(val: &JsonValue) -> Option<Self> {
        let id = val.get("id")?.as_str()?.to_string();
        let backup_type = BackupType::parse(val.get("backup_type")?.as_str()?)?;
        let timestamp = val.get("timestamp")?.as_u64()?;
        // See `META_VERSION`: absent is version 1, whose source was written
        // verbatim; a percent-decode of it would misread a real `%41` in a
        // directory's name as `A`.
        let encoded = val.get("version").and_then(JsonValue::as_u64).unwrap_or(1) >= 2;
        let raw_source = val.get("source")?.as_str()?;
        let source = if encoded {
            decode_path(raw_source)
        } else {
            PathBuf::from(raw_source)
        };
        let parent_id = val
            .get("parent_id")
            .and_then(JsonValue::as_str)
            .map(String::from);
        // The counts are a summary, not the snapshot: an older record without
        // one reads as zero, and nothing is decided from them.
        let count = |key: &str| val.get(key).and_then(JsonValue::as_u64).unwrap_or(0);
        Some(BackupMeta {
            id,
            backup_type,
            timestamp,
            source,
            parent_id,
            file_count: count("file_count"),
            total_size: count("total_size"),
            new_blobs: count("new_blobs"),
            dedup_blobs: count("dedup_blobs"),
            unread_count: count("unread_count"),
        })
    }

    /// The record as the text written to `meta.json`.
    #[must_use]
    pub fn serialize(&self) -> String {
        json_pretty(&self.to_json(), 2)
    }

    /// Read `meta.json`'s text.
    ///
    /// # Errors
    ///
    /// Says what is wrong with text that is not a record.
    pub fn deserialize(input: &str) -> Result<Self, String> {
        let val = json_parse(input)?;
        Self::from_json(&val).ok_or_else(|| "invalid backup meta structure".to_string())
    }
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did it
    // -- that is the diagnosis. The defensive lints keep panics out of code
    // that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use std::path::Path;

    // --- Manifest Serialization Tests ---

    #[test]
    fn test_manifest_roundtrip() {
        let manifest = Manifest {
            files: vec![
                FileEntry {
                    path: PathBuf::from("src/main.rs"),
                    size: 1024,
                    mtime: 1700000000,
                    hash: "abcdef0123456789".to_string(),
                    is_symlink: false,
                    link_target: None,
                    mode: None,
                },
                FileEntry {
                    path: PathBuf::from("README.md"),
                    size: 512,
                    mtime: 1699999000,
                    hash: "9876543210fedcba".to_string(),
                    is_symlink: false,
                    link_target: None,
                    mode: None,
                },
            ],
            ..Manifest::default()
        };

        let serialized = manifest.serialize();
        let deserialized = Manifest::deserialize(&serialized).unwrap();

        // Compared whole rather than field by field. The list this used to be
        // checked four of the six `FileEntry` fields and one of the two
        // entries; `mtime` and `is_symlink` went through the disk format
        // unexamined, so a writer that dropped either would have round-tripped
        // "successfully". A whole-value comparison has no list to fall behind
        // the struct — a seventh field is checked the day it is added.
        assert_eq!(
            deserialized, manifest,
            "a manifest did not survive the trip through its own disk format"
        );
    }

    #[test]
    fn test_manifest_with_symlink() {
        let manifest = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("link"),
                size: 0,
                mtime: 0,
                hash: "linkhash".to_string(),
                is_symlink: true,
                link_target: Some(PathBuf::from("/usr/bin/target")),
                mode: None,
            }],
            ..Manifest::default()
        };

        let serialized = manifest.serialize();
        let deserialized = Manifest::deserialize(&serialized).unwrap();

        // The symlink case is separate from `test_manifest_roundtrip` because
        // `link_target` is the one field whose *key* is conditional — written
        // only when there is a target — so it is the one that can be dropped
        // without the object changing shape. Compared whole for the reason
        // given there.
        assert_eq!(
            deserialized, manifest,
            "a symlink entry did not survive the trip through the disk format"
        );
    }

    #[test]
    fn test_meta_roundtrip() {
        let meta = BackupMeta {
            id: "1700000000-full".to_string(),
            backup_type: BackupType::Full,
            timestamp: 1700000000,
            source: PathBuf::from("/home/user"),
            parent_id: None,
            file_count: 42,
            total_size: 1048576,
            new_blobs: 40,
            dedup_blobs: 2,
            unread_count: 0,
        };

        let serialized = meta.serialize();
        let deserialized = BackupMeta::deserialize(&serialized).unwrap();

        // This was a list of six assertions over a nine-field struct, and the
        // three it left out — `total_size`, `new_blobs`, `dedup_blobs` — are
        // exactly the ones a reader cannot sanity-check by eye, so a writer
        // that dropped one would have shown a plausible wrong number and no
        // failing test. Compared whole, there is no list left to fall behind.
        assert_eq!(
            deserialized, meta,
            "backup metadata did not survive the trip through its own format"
        );
    }

    #[test]
    fn test_meta_with_parent() {
        let meta = BackupMeta {
            id: "1700100000-incremental".to_string(),
            backup_type: BackupType::Incremental,
            timestamp: 1700100000,
            source: PathBuf::from("/home/user"),
            parent_id: Some("1700000000-full".to_string()),
            file_count: 5,
            total_size: 4096,
            new_blobs: 3,
            dedup_blobs: 2,
            unread_count: 0,
        };

        let serialized = meta.serialize();
        let deserialized = BackupMeta::deserialize(&serialized).unwrap();

        // Separate from `test_meta_roundtrip` for the same reason the symlink
        // manifest is separate: `parent_id` is the only `Option` here, so
        // `Some` and `None` are two different objects on disk and both need
        // walking. Compared whole either way.
        assert_eq!(
            deserialized, meta,
            "an incremental backup's parent did not survive the disk format"
        );
    }

    /// `prefix` followed by one unit that makes a name not text: a lone
    /// `0xE9` byte where names are bytes, an unpaired surrogate on the Windows
    /// host this suite also runs on -- so these tests run in the ordinary host
    /// suite rather than only on a Unix machine.
    #[cfg(unix)]
    fn not_text(prefix: &str) -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.push(0xE9);
        std::ffi::OsString::from_vec(bytes)
    }

    #[cfg(windows)]
    fn not_text(prefix: &str) -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.push(0xD800);
        std::ffi::OsString::from_wide(&units)
    }

    /// The record of *which directory was backed up* keeps that directory's
    /// name when the name is not text. It used to go through
    /// `to_string_lossy`, so `meta.json` named a directory that does not exist.
    ///
    /// The record is checked on every build: it holds the escaped bytes and
    /// no replacement character. The round trip back to a path is checked
    /// where names are bytes. On the Windows host `pathcodec` can rebuild only
    /// valid Unicode (its `os_string_from_bytes` host arm), which is a limit
    /// of developing on Windows rather than of the record.
    #[test]
    fn a_source_whose_name_is_not_text_is_recorded_whole() {
        // Joined as one `OsString` with `/`, not with `PathBuf::push`: on the
        // Windows host `push` inserts `\`, which JSON then escapes to `\\`, and
        // the substring test below would be about the host's separator
        // rather than about the record.
        let mut name = std::ffi::OsString::from("/srv/");
        name.push(not_text("caf"));
        let source = PathBuf::from(name);
        assert!(
            source.to_str().is_none(),
            "the fixture's name is text after all, so this test proves nothing"
        );
        let meta = BackupMeta {
            id: "1700000000-full".to_string(),
            backup_type: BackupType::Full,
            timestamp: 1_700_000_000,
            source: source.clone(),
            parent_id: None,
            file_count: 1,
            total_size: 1,
            new_blobs: 1,
            dedup_blobs: 0,
            unread_count: 0,
        };
        let record = meta.serialize();
        assert!(
            record.contains(&encode_path(&source)),
            "the record does not hold the source's escaped bytes: {record}"
        );
        assert!(
            !record.contains('\u{FFFD}'),
            "a replacement character reached the record: {record}"
        );
        #[cfg(unix)]
        {
            let back = BackupMeta::deserialize(&record).unwrap();
            assert_eq!(
                back.source, source,
                "the source directory's name changed on disk"
            );
        }
    }

    /// A record written before `META_VERSION` 2 stored its source verbatim.
    /// Reading it as percent-encoded would turn a directory really named
    /// `100%41` into `100A` -- so an old backup would list under a directory it
    /// never came from.
    #[test]
    fn a_version_1_metadata_record_reads_its_source_verbatim() {
        let legacy = r#"{"id": "1-full", "backup_type": "full", "timestamp": 1, "source": "/home/100%41", "parent_id": null}"#;
        let meta = BackupMeta::deserialize(legacy).unwrap();
        assert_eq!(meta.source, PathBuf::from("/home/100%41"));
    }

    /// A whole manifest, not just one string: this is the path the bug actually
    /// took, since `Manifest::serialize` is what writes the file on disk.
    #[test]
    fn a_manifest_of_non_ascii_paths_round_trips() {
        let mut manifest = Manifest::default();
        for path in ["写真/2024.jpg", "Ωμέγα.txt", "plain.txt"] {
            manifest.files.push(FileEntry {
                path: PathBuf::from(path),
                size: 7,
                mtime: 11,
                hash: "abc".to_string(),
                is_symlink: false,
                link_target: None,
                mode: None,
            });
        }
        let serialized = manifest.serialize();
        let back = Manifest::deserialize(&serialized).expect("manifest should parse");
        let got: Vec<&Path> = back.files.iter().map(|f| f.path.as_path()).collect();
        assert_eq!(
            got,
            [
                Path::new("写真/2024.jpg"),
                Path::new("Ωμέγα.txt"),
                Path::new("plain.txt")
            ]
        );
    }

    /// Version 1 manifests stored paths verbatim. Refusing to read them would
    /// strand every backup taken before path escaping existed.
    #[test]
    fn a_version_1_manifest_is_still_readable() {
        let legacy = r#"{
          "files": [
            {"path": "src/main.rs", "size": 10, "mtime": 1, "hash": "aa", "is_symlink": false},
            {"path": "50%20off.txt", "size": 20, "mtime": 2, "hash": "bb", "is_symlink": false}
          ]
        }"#;
        let back = Manifest::deserialize(legacy).expect("a v1 manifest should still parse");
        let got: Vec<&Path> = back.files.iter().map(|f| f.path.as_path()).collect();
        assert_eq!(
            got,
            [Path::new("src/main.rs"), Path::new("50%20off.txt")],
            "v1 paths are literal: `%20` is part of the name, not an escape"
        );
    }

    /// The version marker is what tells the two formats apart, so a manifest we
    /// write must carry it.
    #[test]
    fn a_manifest_we_write_declares_its_version() {
        let mut manifest = Manifest::default();
        manifest.files.push(FileEntry {
            path: PathBuf::from("100% done.txt"),
            size: 1,
            mtime: 2,
            hash: "aa".to_string(),
            is_symlink: false,
            link_target: None,
            mode: None,
        });
        let text = manifest.serialize();
        let val = json_parse(&text).expect("our own output should parse");
        assert_eq!(
            val.get("version").and_then(JsonValue::as_u64),
            Some(MANIFEST_VERSION)
        );
        // Written escaped, and read back as the original name.
        assert!(text.contains("100%25 done.txt"), "escaped on disk: {text}");
        let back = Manifest::deserialize(&text).expect("round trip");
        assert_eq!(back.files[0].path, PathBuf::from("100% done.txt"));
    }

    #[test]
    fn a_symlink_target_is_escaped_like_any_other_path() {
        let mut manifest = Manifest::default();
        manifest.files.push(FileEntry {
            path: PathBuf::from("link"),
            size: 0,
            mtime: 0,
            hash: "aa".to_string(),
            is_symlink: true,
            link_target: Some(PathBuf::from("/opt/50% off/bin")),
            mode: None,
        });
        let back = Manifest::deserialize(&manifest.serialize()).expect("round trip");
        assert_eq!(
            back.files[0].link_target.as_deref(),
            Some(Path::new("/opt/50% off/bin"))
        );
    }

    /// A file's permission bits survive the manifest: a private file restored
    /// with the default mode would be readable by everyone.
    #[test]
    fn a_mode_survives_the_round_trip() {
        let manifest = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("vault"),
                size: 6,
                mtime: 1,
                hash: "ab".to_string(),
                is_symlink: false,
                link_target: None,
                mode: Some(0o600),
            }],
            complete: true,
            dirs_recorded: true,
            ..Manifest::default()
        };
        let back = Manifest::deserialize(&manifest.serialize()).unwrap();
        assert_eq!(back, manifest);
        assert_eq!(back.files[0].mode, Some(0o600));
    }

    /// A mode that is not a whole number is a corrupt manifest, refused
    /// rather than rounded to a permission nobody recorded.
    #[test]
    fn a_mode_that_is_not_whole_is_refused() {
        let text = r#"{"version": 3, "files": [{"path": "a", "size": 1, "mtime": 1,
            "hash": "ab", "is_symlink": false, "mode": 420.5}]}"#;
        assert!(Manifest::deserialize(text).is_err());
    }
}
