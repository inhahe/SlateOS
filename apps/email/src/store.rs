//! Mail kept in files: the folders of a mail directory, the files opened for
//! the session, the drafts this client writes, and the flags a reader sets.
//!
//! The client has no network, so a server is never where mail comes from.
//! What it can read is what people already keep on disk: **mbox files** (a
//! Thunderbird local folder, a Gmail export, `mutt`'s folders) and
//! **directories of `.eml` files** (one message a file, as most clients
//! save a message). A mail directory -- `~/Mail` -- is listed as folders:
//! each mbox file one, each directory holding `.eml` files one, and `Drafts`,
//! which is this client's own and the only one it writes.
//!
//! **It never writes anything it did not make.** An mbox or a message
//! someone else keeps is read, never rewritten: marking a message read or
//! flagged is kept in a file of this client's own (see [`Flags`]), and
//! deleting one is refused. A draft it saved is its own to replace and to
//! delete.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::EmailMessage;
use crate::decode;

/// The most of one mbox file that is read: a mailbox bigger than this is
/// listed as far as this, and says so.
pub const MAX_MBOX_BYTES: usize = 256 << 20;

/// The most of one `.eml` file that is read.
pub const MAX_MESSAGE_BYTES: usize = 64 << 20;

/// The name of the folder this client writes its drafts into.
pub const DRAFTS: &str = "Drafts";

/// Where a folder's messages come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderSource {
    /// One mbox file.
    Mbox(PathBuf),
    /// A directory of `.eml` files.
    Dir(PathBuf),
    /// The files opened this session, each a message or an mbox.
    Opened(Vec<PathBuf>),
}

/// A folder of messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// What the list calls it.
    pub name: String,
    pub source: FolderSource,
    /// Whether it is this client's own, to write into and delete from: the
    /// drafts, and nothing else.
    pub own: bool,
}

/// A message read from a file, and where it came from.
#[derive(Debug, Clone)]
pub struct Stored {
    pub message: EmailMessage,
    /// The file it is in.
    pub origin: PathBuf,
    /// Which message of that file, from 0: an mbox holds many.
    pub index: usize,
    /// Its size in bytes, as stored.
    pub size: u64,
}

impl Stored {
    /// What its flags are kept under: its `Message-ID`, which follows it
    /// between files, or where it is when it has none.
    #[must_use]
    pub fn key(&self) -> String {
        self.message.message_id.clone().map_or_else(
            || format!("file:{}#{}", self.origin.display(), self.index),
            |id| format!("id:{id}"),
        )
    }
}

/// A folder's messages, and anything worth saying about reading them.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub messages: Vec<Stored>,
    /// Files that were too long to read whole, or that would not read.
    pub notes: Vec<String>,
}

/// Whether `path` looks like an mbox file: an `.mbox` or `.mbx` name, or a
/// first line that is an envelope (`From sender date`), which is how a
/// Thunderbird folder -- a file with no extension -- is told apart.
fn is_mbox(path: &Path) -> bool {
    let named = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mbox") || e.eq_ignore_ascii_case("mbx"));
    if named {
        return true;
    }
    if path.extension().is_some() {
        return false;
    }
    safeio::read_capped(path, 1024).is_ok_and(|head| {
        let first = head.bytes.split(|b| *b == b'\n').next().unwrap_or_default();
        decode::is_envelope(first)
    })
}

/// Whether `path` is a `.eml` file.
fn is_eml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"))
}

/// A file's name, as the list shows it.
fn shown(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| Path::new(n).display().to_string(),
    )
}

/// The folders in the mail directory `root`, by name, with `Drafts` always
/// among them -- made when the first draft is saved.
///
/// # Errors
///
/// When `root` exists and cannot be listed.
pub fn folders(root: &Path) -> Result<Vec<Folder>, String> {
    let mut found = Vec::new();
    if root.exists() {
        let listing = std::fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;
        for entry in listing {
            // An entry the listing cannot describe is not a folder this could
            // read; the rest are still worth listing.
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.is_dir() {
                let is_drafts = path.file_name().is_some_and(|n| n == DRAFTS);
                let has_mail = std::fs::read_dir(&path)
                    .map(|l| l.flatten().any(|e| is_eml(&e.path())))
                    .unwrap_or(false);
                if has_mail && !is_drafts {
                    found.push(Folder {
                        name: shown(&path),
                        source: FolderSource::Dir(path),
                        own: false,
                    });
                }
            } else if path.is_file() && is_mbox(&path) {
                let name = path
                    .file_stem()
                    .map_or_else(|| shown(&path), |s| Path::new(s).display().to_string());
                found.push(Folder {
                    name,
                    source: FolderSource::Mbox(path),
                    own: false,
                });
            }
        }
    }
    found.sort_by_key(|f| f.name.to_lowercase());
    found.push(Folder {
        name: String::from(DRAFTS),
        source: FolderSource::Dir(root.join(DRAFTS)),
        own: true,
    });
    Ok(found)
}

/// Read one file's messages: an mbox's many, or a message's one.
fn read_file(path: &Path, into: &mut Loaded) {
    let mbox = is_mbox(path);
    let cap = if mbox {
        MAX_MBOX_BYTES
    } else {
        MAX_MESSAGE_BYTES
    };
    let read = match safeio::read_capped(path, cap) {
        Ok(read) => read,
        Err(e) => {
            into.notes.push(format!("{}: {e}", shown(path)));
            return;
        }
    };
    if read.truncated {
        into.notes.push(format!(
            "Only the first {} MiB of {} were read.",
            cap >> 20,
            shown(path)
        ));
    }
    let messages = if mbox {
        decode::split_mbox(&read.bytes)
    } else {
        vec![read.bytes]
    };
    for (index, raw) in messages.into_iter().enumerate() {
        match EmailMessage::parse_bytes(&raw) {
            Ok(message) => into.messages.push(Stored {
                message,
                origin: path.to_path_buf(),
                index,
                size: u64::try_from(raw.len()).unwrap_or(u64::MAX),
            }),
            Err(e) => into.notes.push(format!(
                "{} message {}: {e}",
                shown(path),
                index.saturating_add(1)
            )),
        }
    }
}

/// Read a folder's messages.
#[must_use]
pub fn load(folder: &Folder) -> Loaded {
    let mut loaded = Loaded::default();
    match &folder.source {
        FolderSource::Mbox(path) => read_file(path, &mut loaded),
        FolderSource::Dir(dir) => {
            // A drafts folder not made yet has nothing in it, which is not a
            // reason to say anything.
            let Ok(listing) = std::fs::read_dir(dir) else {
                return loaded;
            };
            let mut files: Vec<PathBuf> = listing
                .flatten()
                .map(|e| e.path())
                .filter(|p| is_eml(p) && p.is_file())
                .collect();
            files.sort();
            for file in files {
                read_file(&file, &mut loaded);
            }
        }
        FolderSource::Opened(files) => {
            for file in files {
                read_file(file, &mut loaded);
            }
        }
    }
    loaded
}

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// The first line of the flags file.
const FLAGS_HEADER: &str = "# SlateOS mail flags, format 1";

/// What a reader has marked a message: read, flagged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub seen: bool,
    pub flagged: bool,
}

/// The marks a reader sets, kept by message key in a file of this client's
/// own rather than in the mail it reads, which it never rewrites.
///
/// One line a message: `sf<TAB>key`, where `s` and `f` are `0` or `1`. A key
/// has its tabs, line breaks and backslashes escaped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    marks: BTreeMap<String, Marks>,
}

fn escape(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for c in key.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut chars = key.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Flags {
    /// The marks on `key`, or none.
    #[must_use]
    pub fn get(&self, key: &str) -> Marks {
        self.marks.get(key).copied().unwrap_or_default()
    }

    /// Set the marks on `key`; a message with no marks is not kept.
    pub fn set(&mut self, key: &str, marks: Marks) {
        if marks == Marks::default() {
            self.marks.remove(key);
        } else {
            self.marks.insert(key.to_owned(), marks);
        }
    }

    /// The file's text.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::from(FLAGS_HEADER);
        out.push('\n');
        for (key, marks) in &self.marks {
            out.push(if marks.seen { '1' } else { '0' });
            out.push(if marks.flagged { '1' } else { '0' });
            out.push('\t');
            out.push_str(&escape(key));
            out.push('\n');
        }
        out
    }

    /// Read a flags file's text.
    ///
    /// # Errors
    ///
    /// When it is not a flags file this reads: the whole file is refused
    /// rather than read in part, so that saving what was understood does
    /// not throw away the rest.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(FLAGS_HEADER) {
            return Err(String::from("not a mail flags file this reads"));
        }
        let mut flags = Self::default();
        for (n, line) in lines.enumerate() {
            if line.is_empty() {
                continue;
            }
            let bad = || {
                format!(
                    "line {} of the flags file is not a flag",
                    n.saturating_add(2)
                )
            };
            let (bits, key) = line.split_once('\t').ok_or_else(bad)?;
            let marks = match bits {
                "00" => Marks::default(),
                "10" => Marks {
                    seen: true,
                    flagged: false,
                },
                "01" => Marks {
                    seen: false,
                    flagged: true,
                },
                "11" => Marks {
                    seen: true,
                    flagged: true,
                },
                _ => return Err(bad()),
            };
            flags.set(&unescape(key), marks);
        }
        Ok(flags)
    }
}

/// A file name for a draft: its subject made safe to be a name, or
/// "Draft" -- the first `(2)`, `(3)` free is the caller's to find.
#[must_use]
pub fn draft_stem(subject: &str) -> String {
    let cleaned: String = subject
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = cleaned.chars().take(60).collect();
    if cleaned.is_empty() {
        String::from("Draft")
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("email-store-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leftover temporary directory is harmless.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const MBOX: &str = "From a@example.com Thu Sep 25 10:00:00 2026\n\
        From: a@example.com\nSubject: first\nMessage-ID: <one@example.com>\n\nbody one\n\n\
        From b@example.com Thu Sep 25 11:00:00 2026\n\
        From: b@example.com\nSubject: second\n\nbody two\n";

    /// The mail directory's mbox files and message directories are its
    /// folders; Drafts is always one, and the only one of its own.
    #[test]
    fn the_mail_directory_lists_its_folders() {
        let dir = Scratch::new("folders");
        std::fs::write(dir.0.join("Archive.mbox"), MBOX).unwrap();
        std::fs::write(dir.0.join("Inbox"), MBOX).unwrap();
        std::fs::write(dir.0.join("Inbox.msf"), "not mail").unwrap();
        std::fs::write(dir.0.join("notes.txt"), "not mail").unwrap();
        std::fs::create_dir(dir.0.join("Saved")).unwrap();
        std::fs::write(dir.0.join("Saved").join("a.eml"), "Subject: x\n\ny").unwrap();
        std::fs::create_dir(dir.0.join("Empty")).unwrap();
        let found = folders(&dir.0).unwrap();
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Archive", "Inbox", "Saved", "Drafts"]);
        assert!(found.iter().filter(|f| f.own).all(|f| f.name == DRAFTS));
        // A mail directory that does not exist yet still has somewhere to
        // save a draft.
        let none = folders(&dir.0.join("missing")).unwrap();
        assert_eq!(none.len(), 1);
        assert!(none[0].own);
    }

    /// An mbox is read message by message, and a key is its Message-ID when
    /// it has one, and where it is when it has none.
    #[test]
    fn a_folder_is_read_message_by_message() {
        let dir = Scratch::new("load");
        let path = dir.0.join("Archive.mbox");
        std::fs::write(&path, MBOX).unwrap();
        let loaded = load(&Folder {
            name: String::from("Archive"),
            source: FolderSource::Mbox(path.clone()),
            own: false,
        });
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        let subjects: Vec<&str> = loaded
            .messages
            .iter()
            .map(|m| m.message.subject.as_str())
            .collect();
        assert_eq!(subjects, ["first", "second"]);
        assert_eq!(loaded.messages[0].key(), "id:one@example.com");
        assert!(loaded.messages[1].key().starts_with("file:"));
        assert!(loaded.messages[1].key().ends_with("#1"));
        assert_eq!(loaded.messages[1].index, 1);
    }

    /// Flags round-trip through their file, keys with tabs and line breaks
    /// included, and a file this does not read is refused whole.
    #[test]
    fn flags_are_kept_in_a_file_of_their_own() {
        let mut flags = Flags::default();
        flags.set(
            "id:a@x",
            Marks {
                seen: true,
                flagged: false,
            },
        );
        flags.set(
            "file:odd\tname\n#0",
            Marks {
                seen: false,
                flagged: true,
            },
        );
        flags.set(
            "id:gone",
            Marks {
                seen: true,
                flagged: true,
            },
        );
        flags.set("id:gone", Marks::default());
        let text = flags.to_text();
        assert_eq!(text.lines().count(), 3, "{text}");
        let back = Flags::parse(&text).unwrap();
        assert_eq!(back, flags);
        assert!(back.get("file:odd\tname\n#0").flagged);
        assert_eq!(back.get("id:never"), Marks::default());
        assert!(Flags::parse("# something else\n10\tid:a").is_err());
        assert!(Flags::parse(&format!("{FLAGS_HEADER}\n2x\tid:a")).is_err());
    }

    /// A draft's file name is its subject made safe to be one.
    #[test]
    fn a_draft_is_named_for_its_subject() {
        assert_eq!(draft_stem("Re: plans / dates?"), "Re plans dates");
        assert_eq!(draft_stem("   "), "Draft");
        assert_eq!(draft_stem("line\nbreak"), "line break");
        assert_eq!(draft_stem(&"x".repeat(100)).len(), 60);
    }
}
