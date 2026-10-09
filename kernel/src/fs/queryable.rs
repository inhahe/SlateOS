//! Queryable file attributes: typed facts about a file -- `Audio:Artist` is
//! "The Beatles", `Audio:Bitrate` is 320 -- that files can be found by, as
//! BeOS's BFS let a person find any file by its attributes (`design.txt` lines
//! 35-37). Unlike a tag (a label, `fs::tags`) an attribute has a name and a
//! typed value; unlike a comment (`fs::fcomment`) it is for comparing.
//!
//! **Each attribute is an extended attribute on the file, named
//! `user.slate.<name>`** -- `user.slate.Audio:Artist` -- its value a type
//! marker and the value, readable as text so `getfattr` shows it and
//! `setfattr` can write it:
//!
//! | Type | Kept as | Example |
//! |---|---|---|
//! | text | `t:` then the text, UTF-8 | `t:The Beatles` |
//! | integer | `i:` then the number in decimal | `i:320` |
//! | boolean | `b:true` or `b:false` | `b:true` |
//! | bytes | `x:` then the bytes as they are | `x:...` |
//!
//! A value some other tool set without a marker -- or with one whose rest does
//! not fit it (`i:abc`) -- reads as text if it is UTF-8 and as bytes if not.
//!
//! The file's filesystem keeps them, which settles what the table this used to
//! be could not: they survive a reboot (on ext4), go with the file through
//! every name and rename, end with it, and travel with `cp -a`, `tar --xattrs`
//! and `rsync -X`. Programs reach them with the extended-attribute calls they
//! already have (`getxattr(2)` and its family, through either ABI), under the
//! `user.` namespace's rules (`fs::xattr_policy`): regular files and
//! directories only, read as the file's read permission allows, written as
//! its write permission does. That is the door design-decisions §978 asked
//! for. What this module adds is for the kernel shell: typed reads and writes,
//! and [`query`], which walks a subtree comparing -- bounded, and saying when
//! it could not see all of it, since nothing indexes attributes. An index is a
//! search service's to keep, in userspace.
//!
//! Until 2026-10-09 the attributes were a table here, in memory: keyed by the
//! file's identity since 2026-09-21 (§957), with hooks to follow renames and
//! drop the dead, but gone at every reboot, reachable by no program, and
//! written by whoever asked with no check of the file's permissions
//! (design-decisions §1561). Nothing but the kernel shell and this module's
//! self-test ever set one, so there is nothing to carry over.
//!
//! ## Limits
//!
//! - A name is printable ASCII (BeOS's `Category:Name`), at most
//!   [`MAX_ATTR_NAME_LEN`] bytes: Linux's 255-byte attribute name less the
//!   prefix.
//! - A text or bytes value is at most [`MAX_VALUE_LEN`] bytes. A filesystem
//!   may keep less, and says so: ext4 keeps all of a file's attributes in one
//!   block.
//! - At most [`MAX_ATTRS_PER_FILE`] on a file.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::fswalk::{self, WalkAction, WalkOptions};
use super::path::{Path, PathBuf};
use super::{EntryType, Vfs};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// What every attribute's name begins with.
pub const PREFIX: &[u8] = b"user.slate.";

/// Linux's longest extended attribute name (`XATTR_NAME_MAX`).
const XATTR_NAME_MAX: usize = 255;

/// The longest attribute name: what [`PREFIX`] leaves of Linux's 255 bytes.
pub const MAX_ATTR_NAME_LEN: usize = XATTR_NAME_MAX - PREFIX.len();

/// The longest text or bytes value.
pub const MAX_VALUE_LEN: usize = 4096;

/// The most attributes one file carries.
pub const MAX_ATTRS_PER_FILE: usize = 64;

/// The most entries one [`query`] visits: each visit is a path lookup and a
/// read of the file's attribute names, under the filesystem's lock, in the
/// caller's time.
const MAX_VISITED: u64 = 65536;

/// The most results one [`query`] returns.
const MAX_QUERY_RESULTS: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Typed attribute value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttrValue {
    /// Text.
    Text(String),
    /// A 64-bit signed integer.
    Int(i64),
    /// A boolean.
    Bool(bool),
    /// Raw bytes.
    Bytes(Vec<u8>),
}

impl AttrValue {
    /// The type's name, for display.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Int(_) => "int",
            Self::Bool(_) => "bool",
            Self::Bytes(_) => "bytes",
        }
    }
}

/// Comparison operator for queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Equal: text with ASCII letters' case ignored.
    Equal,
    /// Not equal.
    NotEqual,
    /// Less than (integers).
    LessThan,
    /// Less than or equal (integers).
    LessEqual,
    /// Greater than (integers).
    GreaterThan,
    /// Greater than or equal (integers).
    GreaterEqual,
    /// Text contains (case ignored).
    Contains,
    /// Text starts with (case ignored).
    StartsWith,
    /// Text ends with (case ignored).
    EndsWith,
}

/// A single query predicate: attribute `attr_name` compared by `op` with
/// `value`. A file without the attribute, or with a value of another type,
/// does not match it.
#[derive(Debug, Clone)]
pub struct Predicate {
    /// The attribute's name, without [`PREFIX`].
    pub attr_name: String,
    /// The comparison.
    pub op: CompareOp,
    /// What the file's value is compared with.
    pub value: AttrValue,
}

impl Predicate {
    /// Text equality.
    #[must_use]
    pub fn eq_text(name: &str, value: &str) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::Equal,
            value: AttrValue::Text(String::from(value)),
        }
    }

    /// Integer equality.
    #[must_use]
    pub fn eq_int(name: &str, value: i64) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::Equal,
            value: AttrValue::Int(value),
        }
    }

    /// Integer greater-than.
    #[must_use]
    pub fn gt_int(name: &str, value: i64) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::GreaterThan,
            value: AttrValue::Int(value),
        }
    }

    /// Integer less-than.
    #[must_use]
    pub fn lt_int(name: &str, value: i64) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::LessThan,
            value: AttrValue::Int(value),
        }
    }

    /// Text contains.
    #[must_use]
    pub fn contains(name: &str, substring: &str) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::Contains,
            value: AttrValue::Text(String::from(substring)),
        }
    }

    /// Text starts-with.
    #[must_use]
    pub fn starts_with(name: &str, prefix: &str) -> Self {
        Self {
            attr_name: String::from(name),
            op: CompareOp::StartsWith,
            value: AttrValue::Text(String::from(prefix)),
        }
    }

    /// Whether a file's value `stored` satisfies this predicate.
    fn matches(&self, stored: &AttrValue) -> bool {
        use AttrValue::{Bool, Bytes, Int, Text};
        use CompareOp::{
            Contains, EndsWith, Equal, GreaterEqual, GreaterThan, LessEqual, LessThan, NotEqual,
            StartsWith,
        };
        match (self.op, &self.value, stored) {
            (Equal, Text(a), Text(b)) => a.eq_ignore_ascii_case(b),
            (NotEqual, Text(a), Text(b)) => !a.eq_ignore_ascii_case(b),
            (Contains, Text(needle), Text(hay)) => contains_ignoring_ascii_case(hay, needle),
            (StartsWith, Text(p), Text(hay)) => {
                hay.len() >= p.len()
                    && hay
                        .as_bytes()
                        .get(..p.len())
                        .is_some_and(|h| h.eq_ignore_ascii_case(p.as_bytes()))
            }
            (EndsWith, Text(s), Text(hay)) => {
                hay.len() >= s.len()
                    && hay
                        .as_bytes()
                        .get(hay.len().saturating_sub(s.len())..)
                        .is_some_and(|h| h.eq_ignore_ascii_case(s.as_bytes()))
            }
            (Equal, Int(a), Int(b)) => a == b,
            (NotEqual, Int(a), Int(b)) => a != b,
            (LessThan, Int(t), Int(v)) => v < t,
            (LessEqual, Int(t), Int(v)) => v <= t,
            (GreaterThan, Int(t), Int(v)) => v > t,
            (GreaterEqual, Int(t), Int(v)) => v >= t,
            (Equal, Bool(a), Bool(b)) => a == b,
            (NotEqual, Bool(a), Bool(b)) => a != b,
            (Equal, Bytes(a), Bytes(b)) => a == b,
            (NotEqual, Bytes(a), Bytes(b)) => a != b,
            // A type that does not compare this way, or another type.
            _ => false,
        }
    }
}

/// How a query's predicates combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// Every predicate matches (AND).
    All,
    /// At least one matches (OR).
    Any,
}

/// One file a query found, with the predicates' attributes that matched.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// The path the walk reached it by: bytes, so a name that is not UTF-8 is
    /// found and openable from the result.
    pub path: PathBuf,
    /// Each matching predicate's attribute and the file's value.
    pub matched_attrs: Vec<(String, AttrValue)>,
}

/// What a [`query`] found under its root.
#[derive(Debug)]
pub struct Found {
    /// The matching files, in the order the walk met them.
    pub results: Vec<QueryResult>,
    /// Whether it stopped short of seeing everything under the root: too
    /// many entries or results, a directory or a file's attributes that
    /// could not be read, or a directory the walk left unread. Without it,
    /// "nothing found" and "not all looked at" would read the same.
    pub incomplete: bool,
}

/// An attribute name programs are expected to share, with its type: BeOS's
/// convention of `Category:Name`. Advice, not a rule: any name may be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WellKnown {
    /// The attribute's name.
    pub name: &'static str,
    /// Its value's type: `text`, `int` or `bool`.
    pub value_type: &'static str,
    /// What it holds.
    pub description: &'static str,
}

/// The well-known attributes.
pub const WELL_KNOWN: &[WellKnown] = &[
    WellKnown {
        name: "Audio:Artist",
        value_type: "text",
        description: "Music artist or band name",
    },
    WellKnown {
        name: "Audio:Album",
        value_type: "text",
        description: "Album name",
    },
    WellKnown {
        name: "Audio:Title",
        value_type: "text",
        description: "Track title",
    },
    WellKnown {
        name: "Audio:Genre",
        value_type: "text",
        description: "Music genre",
    },
    WellKnown {
        name: "Audio:Year",
        value_type: "int",
        description: "Release year",
    },
    WellKnown {
        name: "Audio:Track",
        value_type: "int",
        description: "Track number",
    },
    WellKnown {
        name: "Audio:Bitrate",
        value_type: "int",
        description: "Audio bitrate in kbps",
    },
    WellKnown {
        name: "Audio:Duration",
        value_type: "int",
        description: "Duration in seconds",
    },
    WellKnown {
        name: "Image:Width",
        value_type: "int",
        description: "Image width in pixels",
    },
    WellKnown {
        name: "Image:Height",
        value_type: "int",
        description: "Image height in pixels",
    },
    WellKnown {
        name: "Image:ColorSpace",
        value_type: "text",
        description: "Color space (sRGB, AdobeRGB, ...)",
    },
    WellKnown {
        name: "Image:Camera",
        value_type: "text",
        description: "Camera make and model",
    },
    WellKnown {
        name: "Image:DateTaken",
        value_type: "int",
        description: "When the photo was taken (Unix time)",
    },
    WellKnown {
        name: "Document:Author",
        value_type: "text",
        description: "Document author",
    },
    WellKnown {
        name: "Document:Title",
        value_type: "text",
        description: "Document title",
    },
    WellKnown {
        name: "Document:Subject",
        value_type: "text",
        description: "Document subject",
    },
    WellKnown {
        name: "Document:PageCount",
        value_type: "int",
        description: "Number of pages",
    },
    WellKnown {
        name: "Document:WordCount",
        value_type: "int",
        description: "Number of words",
    },
    WellKnown {
        name: "Email:From",
        value_type: "text",
        description: "Sender's address",
    },
    WellKnown {
        name: "Email:To",
        value_type: "text",
        description: "Recipient's address",
    },
    WellKnown {
        name: "Email:Subject",
        value_type: "text",
        description: "Subject line",
    },
    WellKnown {
        name: "Email:Date",
        value_type: "int",
        description: "When it was sent (Unix time)",
    },
    WellKnown {
        name: "Email:Read",
        value_type: "bool",
        description: "Whether it has been read",
    },
    WellKnown {
        name: "App:Rating",
        value_type: "int",
        description: "The user's rating (1-5)",
    },
];

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static GET_COUNT: AtomicU64 = AtomicU64::new(0);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);

/// How many attributes were set or removed, read, and queried for, since boot
/// or [`reset_stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryStats {
    /// Attributes set, removed or cleared.
    pub sets: u64,
    /// Attributes read, one at a time or a file's all.
    pub gets: u64,
    /// Queries.
    pub queries: u64,
}

/// The counters.
#[must_use]
pub fn stats() -> QueryStats {
    QueryStats {
        sets: SET_COUNT.load(Ordering::Relaxed),
        gets: GET_COUNT.load(Ordering::Relaxed),
        queries: QUERY_COUNT.load(Ordering::Relaxed),
    }
}

/// Zero the counters.
pub fn reset_stats() {
    SET_COUNT.store(0, Ordering::Relaxed);
    GET_COUNT.store(0, Ordering::Relaxed);
    QUERY_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Names and values
// ---------------------------------------------------------------------------

/// Whether `name` may name an attribute: see the module's "Limits".
fn validate_name(name: &str) -> KernelResult<()> {
    if name.is_empty()
        || name.len() > MAX_ATTR_NAME_LEN
        || !name.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// The extended attribute `name` is kept in.
fn xattr_name(name: &str) -> Vec<u8> {
    let mut full = Vec::with_capacity(PREFIX.len().saturating_add(name.len()));
    full.extend_from_slice(PREFIX);
    full.extend_from_slice(name.as_bytes());
    full
}

/// The attribute name an extended attribute's name gives, if it is one of
/// ours: [`PREFIX`] then a name [`validate_name`] takes.
fn attr_name_of(xattr: &[u8]) -> Option<&str> {
    let rest = xattr.strip_prefix(PREFIX)?;
    let name = core::str::from_utf8(rest).ok()?;
    validate_name(name).ok()?;
    Some(name)
}

/// `value` as it is kept: a type marker and the value (module documentation).
fn encode(value: &AttrValue) -> KernelResult<Vec<u8>> {
    let number;
    let (marker, body): (&[u8], &[u8]) = match value {
        AttrValue::Text(s) if s.len() > MAX_VALUE_LEN => return Err(KernelError::InvalidArgument),
        AttrValue::Bytes(b) if b.len() > MAX_VALUE_LEN => return Err(KernelError::InvalidArgument),
        AttrValue::Text(s) => (&b"t:"[..], s.as_bytes()),
        AttrValue::Int(n) => {
            number = alloc::format!("{n}");
            (&b"i:"[..], number.as_bytes())
        }
        AttrValue::Bool(v) => (&b"b:"[..], if *v { &b"true"[..] } else { &b"false"[..] }),
        AttrValue::Bytes(b) => (&b"x:"[..], b.as_slice()),
    };
    let mut out = Vec::new();
    out.try_reserve(marker.len().saturating_add(body.len()))
        .map_err(|_| KernelError::OutOfMemory)?;
    out.extend_from_slice(marker);
    out.extend_from_slice(body);
    Ok(out)
}

/// The value a kept attribute holds. A value without a marker, or one whose
/// rest does not fit its marker, reads untyped: text if it is UTF-8, bytes if
/// not.
fn decode(raw: &[u8]) -> AttrValue {
    let typed = match raw.split_at_checked(2) {
        Some((b"t:", rest)) => core::str::from_utf8(rest)
            .ok()
            .map(|s| AttrValue::Text(String::from(s))),
        Some((b"i:", rest)) => core::str::from_utf8(rest)
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(AttrValue::Int),
        Some((b"b:", b"true")) => Some(AttrValue::Bool(true)),
        Some((b"b:", b"false")) => Some(AttrValue::Bool(false)),
        Some((b"x:", rest)) => Some(AttrValue::Bytes(rest.to_vec())),
        _ => None,
    };
    typed.unwrap_or_else(|| match core::str::from_utf8(raw) {
        Ok(s) => AttrValue::Text(String::from(s)),
        Err(_) => AttrValue::Bytes(raw.to_vec()),
    })
}

/// Whether `needle` occurs in `haystack`, ASCII letters' case ignored.
fn contains_ignoring_ascii_case(haystack: &str, needle: &str) -> bool {
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

// ---------------------------------------------------------------------------
// One file's attributes
// ---------------------------------------------------------------------------

/// Give the file at `path` attribute `name` with `value`, replacing any it
/// has. Asked as the caller: `setxattr`'s rules decide who may.
///
/// # Errors
///
/// `InvalidArgument` for a name or a value outside the limits;
/// `ResourceExhausted` for a new name on a file with [`MAX_ATTRS_PER_FILE`]
/// already; the path's, the `user.` namespace's and the filesystem's -- one
/// that keeps no attributes (FAT) answers `NotSupported`.
pub fn set_attr(path: impl AsRef<Path>, name: &str, value: AttrValue) -> KernelResult<()> {
    let path = path.as_ref();
    validate_name(name)?;
    let encoded = encode(&value)?;
    let full = xattr_name(name);
    let held: Vec<Vec<u8>> = Vfs::list_xattrs(path)?;
    let ours = held.iter().filter(|n| attr_name_of(n).is_some()).count();
    if ours >= MAX_ATTRS_PER_FILE && !held.contains(&full) {
        return Err(KernelError::ResourceExhausted);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    Vfs::set_xattr(path, &full, &encoded)
}

/// The value of the file's attribute `name`. Asked as the caller, through a
/// trailing symlink, as `getxattr` asks.
///
/// # Errors
///
/// `NoAttribute` when the file has none by that name; `InvalidArgument` for a
/// name no attribute can have; the path's, the namespace's and the
/// filesystem's.
pub fn get_attr(path: impl AsRef<Path>, name: &str) -> KernelResult<AttrValue> {
    validate_name(name)?;
    GET_COUNT.fetch_add(1, Ordering::Relaxed);
    Vfs::get_xattr(path.as_ref(), &xattr_name(name)).map(|raw| decode(&raw))
}

/// Remove the file's attribute `name`.
///
/// # Errors
///
/// `NoAttribute` when it has none by that name; otherwise as [`set_attr`].
pub fn remove_attr(path: impl AsRef<Path>, name: &str) -> KernelResult<()> {
    validate_name(name)?;
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    Vfs::remove_xattr(path.as_ref(), &xattr_name(name))
}

/// Every attribute on the file, by name.
///
/// # Errors
///
/// As [`get_attr`].
pub fn list_attrs(path: impl AsRef<Path>) -> KernelResult<Vec<(String, AttrValue)>> {
    let path = path.as_ref();
    GET_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut out = Vec::new();
    for xattr in Vfs::list_xattrs(path)? {
        let Some(name) = attr_name_of(&xattr) else {
            continue;
        };
        match Vfs::get_xattr(path, &xattr) {
            Ok(raw) => out.push((String::from(name), decode(&raw))),
            // Removed between the listing and the read: gone, not an error.
            Err(KernelError::NoAttribute) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Remove every attribute from the file; answers how many there were.
///
/// # Errors
///
/// As [`set_attr`].
pub fn clear_attrs(path: impl AsRef<Path>) -> KernelResult<usize> {
    let path = path.as_ref();
    let mut removed: usize = 0;
    for xattr in Vfs::list_xattrs(path)? {
        if attr_name_of(&xattr).is_none() {
            continue;
        }
        match Vfs::remove_xattr(path, &xattr) {
            Ok(()) => removed = removed.saturating_add(1),
            Err(KernelError::NoAttribute) => {}
            Err(e) => return Err(e),
        }
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// The files and directories under `root`, `root` included, whose attributes
/// satisfy `predicates` -- every one ([`QueryMode::All`]) or any one
/// ([`QueryMode::Any`]); no predicates match nothing. `/proc`, `/sys` and
/// `/dev` are not walked (their files keep no attributes), and symlinks are
/// not followed.
///
/// # Errors
///
/// `root`'s: `NotFound`, `NotADirectory`.
pub fn query<R: AsRef<Path> + ?Sized>(
    predicates: &[Predicate],
    mode: QueryMode,
    root: &R,
) -> KernelResult<Found> {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut found = Found {
        results: Vec::new(),
        incomplete: false,
    };
    if predicates.is_empty() {
        return Ok(found);
    }
    let wanted: Vec<Vec<u8>> = predicates
        .iter()
        .map(|p| xattr_name(&p.attr_name))
        .collect();
    let opts = WalkOptions {
        show_hidden: true,
        include_root: true,
        ..WalkOptions::default()
    };
    let mut visited: u64 = 0;
    let stats = fswalk::walk_visit(root.as_ref(), &opts, |entry| {
        visited = visited.saturating_add(1);
        if visited > MAX_VISITED {
            found.incomplete = true;
            return WalkAction::Stop;
        }
        // Only these two carry `user.` attributes.
        if !matches!(entry.entry_type, EntryType::File | EntryType::Directory) {
            return WalkAction::Continue;
        }
        let held = match Vfs::list_xattrs_no_follow(&entry.path) {
            Ok(held) => held,
            // A filesystem that keeps no attributes: nothing to miss.
            Err(KernelError::NotSupported) => return WalkAction::Continue,
            // Gone mid-walk, or unreadable: a match that may not have been seen.
            Err(_) => {
                found.incomplete = true;
                return WalkAction::Continue;
            }
        };
        let mut matched = Vec::new();
        let mut all = true;
        for (pred, name) in predicates.iter().zip(&wanted) {
            if !held.contains(name) {
                all = false;
                continue;
            }
            match Vfs::get_xattr_no_follow(&entry.path, name) {
                Ok(raw) => {
                    let value = decode(&raw);
                    if pred.matches(&value) {
                        matched.push((pred.attr_name.clone(), value));
                    } else {
                        all = false;
                    }
                }
                Err(KernelError::NoAttribute) => all = false,
                Err(_) => {
                    all = false;
                    found.incomplete = true;
                }
            }
        }
        let include = match mode {
            QueryMode::All => all,
            QueryMode::Any => !matched.is_empty(),
        };
        if include {
            if found.results.len() >= MAX_QUERY_RESULTS {
                found.incomplete = true;
                return WalkAction::Stop;
            }
            found.results.push(QueryResult {
                path: entry.path.clone(),
                matched_attrs: matched,
            });
        }
        WalkAction::Continue
    })?;
    if stats.errors > 0 || stats.unwalked > 0 {
        found.incomplete = true;
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Attributes as the file's extended attributes, on a scratch tree in `/tmp`
/// (memfs):
///
/// 1. each type kept and read back, and what a program's `getxattr` sees is
///    the marked value under `user.slate.<name>`;
/// 2. a value another tool set reads untyped -- text, or bytes when not
///    UTF-8 -- and a marker whose rest does not fit reads as text;
/// 3. names and values outside the limits refused, removing one not there
///    `NoAttribute`, clearing counts what it removed, and the per-file limit;
/// 4. the file's, not a name's: a hard link sees them, a rename carries them,
///    and a file made at the name of a deleted one has none;
/// 5. queries: text with case ignored, integer ranges, substrings, all-of and
///    any-of, within their root (which must exist), a directory's own
///    attributes and a name that is not UTF-8 found, everything seen.
///
/// The counters are put back as they were: this is reachable from the shell,
/// and a test run is not the user's activity.
///
/// # Errors
///
/// `InternalError` naming the first step that answered wrongly; the setup's.
pub fn self_test() -> KernelResult<()> {
    const DIR: &str = "/tmp/_qattr";
    let saved = stats();
    // Best effort, both ends: this test's own scratch tree.
    let _ = Vfs::remove_recursive(DIR);
    let result = self_test_on(DIR);
    let _ = Vfs::remove_recursive(DIR);
    SET_COUNT.store(saved.sets, Ordering::Relaxed);
    GET_COUNT.store(saved.gets, Ordering::Relaxed);
    QUERY_COUNT.store(saved.queries, Ordering::Relaxed);
    result?;
    crate::serial_println!(
        "[qattr] Self-test passed (5 tests): typed values as user.slate.* attributes, untyped \
         ones read, limits, links and renames, queries"
    );
    Ok(())
}

#[allow(clippy::too_many_lines)] // one linear script
fn self_test_on(dir: &str) -> KernelResult<()> {
    use crate::serial_println;

    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("[qattr]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    let at = |name: &[u8]| {
        let mut p = PathBuf::from(dir);
        p.push(Path::new(name));
        p
    };
    let song = at(b"song.mp3");
    let alias = at(b"alias.mp3");
    let moved = at(b"moved.mp3");
    let odd = at(b"\xFFodd.mp3");
    let sub = at(b"sub");
    let deep = at(b"sub/deep.mp3");
    let photo = at(b"photo.jpg");

    Vfs::mkdir(dir)?;
    Vfs::mkdir(&sub)?;
    for p in [&song, &odd, &deep, &photo] {
        Vfs::write_file(p, b"data")?;
    }

    // 1: each type, and the attribute a program reads.
    let typed = [
        ("Audio:Artist", AttrValue::Text(String::from("The Beatles"))),
        ("Audio:Bitrate", AttrValue::Int(-320)),
        ("Email:Read", AttrValue::Bool(true)),
        ("App:Blob", AttrValue::Bytes(alloc::vec![0, 0xFF, b':'])),
    ];
    for (name, value) in &typed {
        set_attr(&song, name, value.clone())?;
    }
    let mut back = Vec::new();
    for (name, _) in &typed {
        back.push(get_attr(&song, name)?);
    }
    let program = Vfs::get_xattr(&song, b"user.slate.Audio:Bitrate")?;
    let listed = list_attrs(&song)?;
    if back.iter().zip(&typed).any(|(b, (_, v))| b != v)
        || program != b"i:-320"
        || listed.len() != typed.len()
    {
        serial_println!(
            "[qattr]     read back {:?}, a program's {:?}, listed {}",
            back,
            program,
            listed.len()
        );
        return fail("a typed value did not read back, or is not kept as user.slate.<name>");
    }
    serial_println!("[qattr]   1: each type kept as a marked user.slate.* attribute: ok");

    // 2: values set without a marker.
    Vfs::set_xattr(&photo, b"user.slate.Image:Camera", b"Pentax K-1")?;
    Vfs::set_xattr(&photo, b"user.slate.Image:Raw", b"\xFF\x00")?;
    Vfs::set_xattr(&photo, b"user.slate.Image:Width", b"i:wide")?;
    let camera = get_attr(&photo, "Image:Camera")?;
    let raw = get_attr(&photo, "Image:Raw")?;
    let width = get_attr(&photo, "Image:Width")?;
    if camera != AttrValue::Text(String::from("Pentax K-1"))
        || raw != AttrValue::Bytes(alloc::vec![0xFF, 0])
        || width != AttrValue::Text(String::from("i:wide"))
    {
        serial_println!(
            "[qattr]     camera {:?}, raw {:?}, width {:?}",
            camera,
            raw,
            width
        );
        return fail("a value without a fitting marker read as something other than untyped");
    }
    serial_println!("[qattr]   2: unmarked values read as text or bytes: ok");

    // 3: limits, removal, clearing.
    let long_name = "N".repeat(MAX_ATTR_NAME_LEN.saturating_add(1));
    let big = AttrValue::Text("v".repeat(MAX_VALUE_LEN.saturating_add(1)));
    let refusals = [
        set_attr(&song, "", AttrValue::Int(1)),
        set_attr(&song, "has space", AttrValue::Int(1)),
        set_attr(&song, &long_name, AttrValue::Int(1)),
        set_attr(&song, "Big:Text", big),
    ];
    let absent = remove_attr(&song, "Never:There");
    let cleared = clear_attrs(&song)?;
    let after_clear = list_attrs(&song)?;
    for i in 0..MAX_ATTRS_PER_FILE {
        set_attr(&song, &alloc::format!("Many:{i}"), AttrValue::Int(1))?;
    }
    let one_more = set_attr(&song, "Many:extra", AttrValue::Int(1));
    let overwrite = set_attr(&song, "Many:0", AttrValue::Int(2));
    let cleared_many = clear_attrs(&song)?;
    if refusals
        .iter()
        .any(|r| *r != Err(KernelError::InvalidArgument))
        || absent != Err(KernelError::NoAttribute)
        || cleared != typed.len()
        || !after_clear.is_empty()
        || one_more != Err(KernelError::ResourceExhausted)
        || overwrite.is_err()
        || cleared_many != MAX_ATTRS_PER_FILE
    {
        serial_println!(
            "[qattr]     refusals {:?}, absent {:?}, cleared {} then {:?}, one more {:?}, overwrite {:?}, cleared {}",
            refusals,
            absent,
            cleared,
            after_clear,
            one_more,
            overwrite,
            cleared_many
        );
        return fail("a limit, a removal or a clearing answered wrongly");
    }
    serial_println!("[qattr]   3: limits, removal, clearing: ok");

    // 4: the file's, not a name's.
    set_attr(&song, "Audio:Title", AttrValue::Text(String::from("Help!")))?;
    Vfs::link(&song, &alias)?;
    let through_link = get_attr(&alias, "Audio:Title");
    Vfs::rename(&song, &moved)?;
    let after_rename = get_attr(&moved, "Audio:Title");
    Vfs::remove(&moved)?;
    Vfs::remove(&alias)?;
    Vfs::write_file(&song, b"new")?;
    let newcomer = list_attrs(&song)?;
    let title = Ok(AttrValue::Text(String::from("Help!")));
    if through_link != title || after_rename != title || !newcomer.is_empty() {
        serial_println!(
            "[qattr]     link {:?}, renamed {:?}, a new file at the old name {:?}",
            through_link,
            after_rename,
            newcomer
        );
        return fail("attributes did not follow their file");
    }
    serial_println!("[qattr]   4: through a link, across a rename, gone with the file: ok");

    // 5: queries.
    set_attr(
        &song,
        "Audio:Artist",
        AttrValue::Text(String::from("The Beatles")),
    )?;
    set_attr(&song, "Audio:Year", AttrValue::Int(1965))?;
    set_attr(
        &odd,
        "Audio:Artist",
        AttrValue::Text(String::from("the beatles")),
    )?;
    set_attr(&odd, "Audio:Year", AttrValue::Int(1969))?;
    set_attr(
        &deep,
        "Audio:Artist",
        AttrValue::Text(String::from("Pink Floyd")),
    )?;
    set_attr(&deep, "Audio:Year", AttrValue::Int(1973))?;
    set_attr(&sub, "Folder:Kind", AttrValue::Text(String::from("albums")))?;
    let root = Path::new(dir);
    let beatles = query(
        &[Predicate::eq_text("Audio:Artist", "THE BEATLES")],
        QueryMode::All,
        root,
    )?;
    let sixties = query(
        &[
            Predicate::gt_int("Audio:Year", 1964),
            Predicate::lt_int("Audio:Year", 1970),
        ],
        QueryMode::All,
        root,
    )?;
    let either = query(
        &[
            Predicate::contains("Audio:Artist", "floyd"),
            Predicate::eq_int("Audio:Year", 1965),
        ],
        QueryMode::Any,
        root,
    )?;
    let starts = query(
        &[Predicate::starts_with("Audio:Artist", "the ")],
        QueryMode::All,
        root,
    )?;
    let in_sub = query(&[Predicate::gt_int("Audio:Year", 0)], QueryMode::All, &sub)?;
    let folder = query(
        &[Predicate::eq_text("Folder:Kind", "Albums")],
        QueryMode::All,
        &sub,
    )?;
    let missing_root = query(
        &[Predicate::gt_int("Audio:Year", 0)],
        QueryMode::All,
        "/tmp/_qattr/none",
    );
    let found_odd = beatles.results.iter().any(|r| r.path == odd);
    if beatles.results.len() != 2
        || !found_odd
        || beatles.incomplete
        || sixties.results.len() != 2
        || either.results.len() != 2
        || starts.results.len() != 2
        || in_sub.results.len() != 1
        || in_sub.results.first().map(|r| &r.path) != Some(&deep)
        || folder.results.len() != 1
        || missing_root.is_ok()
    {
        serial_println!(
            "[qattr]     beatles {} (odd name found {}), sixties {}, either {}, starts {}, under sub {}, sub's own {}, a missing root {:?}",
            beatles.results.len(),
            found_odd,
            sixties.results.len(),
            either.results.len(),
            starts.results.len(),
            in_sub.results.len(),
            folder.results.len(),
            missing_root.map(|f| f.results.len())
        );
        return fail("a query found the wrong files");
    }
    serial_println!(
        "[qattr]   5: queries -- case, ranges, substrings, all and any, within a root: ok"
    );
    Ok(())
}
