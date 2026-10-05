//! What a program asks the file chooser, and what it is answered.
//!
//! # On the wire
//!
//! One [`Request`] and one [`Reply`] per connection. Each is a frame: a
//! four-byte little-endian length, then that many bytes of message, so a
//! reader over a transport that carries bytes knows when it has all of one.
//! A message is `SFC`, the version ([`VERSION`]), its kind, and its fields:
//! integers little-endian, text as a two-byte length and UTF-8, a path or a
//! name as a two-byte length and its bytes -- never decoded, since a file's
//! name is whatever bytes the filesystem holds.
//!
//! # Bounds
//!
//! A peer may be anyone: on a program's side, whatever registered the
//! service's name; on the chooser's, every program. Every field is bounded
//! ([`MAX_PATH`], [`MAX_NAME`], [`MAX_TEXT`], [`MAX_FILTERS`],
//! [`MAX_PATTERNS`], [`MAX_PATTERN`]), the frame as a whole by
//! [`MAX_FRAME`], and a frame declaring more is refused before it is
//! buffered. A message that breaks any rule -- a name holding a `/`, a path
//! holding a NUL, a chosen path that is not absolute, a filter index past the
//! filters, bytes left over -- is [`Decoded::Malformed`], never read in part.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub use msgframe::{Decoded, TooLarge};
use msgframe::{Protocol, Reader, Writer};

/// The name the file chooser registers, and programs connect to.
pub const SERVICE: &str = "org.slateos.FileChooser";

/// The protocol this module speaks. A message of another version is
/// malformed to it.
pub const VERSION: u8 = 1;

/// The longest path either side may send, in bytes: Linux's `PATH_MAX`.
pub const MAX_PATH: usize = 4096;
/// The longest file name offered for saving, in bytes: one path component,
/// as ext4 bounds it.
pub const MAX_NAME: usize = 255;
/// The longest title or filter label, in bytes.
pub const MAX_TEXT: usize = 1024;
/// The most filters one request may offer.
pub const MAX_FILTERS: usize = 64;
/// The most patterns one filter may hold.
pub const MAX_PATTERNS: usize = 32;
/// The longest pattern, in bytes.
pub const MAX_PATTERN: usize = 128;
/// The longest frame either side reads: comfortably above the largest
/// message the other bounds allow (about 350 KiB), so no valid message is
/// refused for its size.
pub const MAX_FRAME: usize = 1 << 20;

/// The protocol: its mark, its version and its longest frame.
const PROTOCOL: Protocol = Protocol {
    mark: b"SFC",
    version: VERSION,
    max_frame: MAX_FRAME,
};
const KIND_REQUEST: u8 = 1;
const KIND_REPLY: u8 = 2;
const OUTCOME_CANCELLED: u8 = 0;
const OUTCOME_CHOSEN: u8 = 1;

/// What the chooser is asked to choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A file to open.
    Open,
    /// A place and a name to save to.
    Save,
    /// A folder.
    Folder,
}

impl Mode {
    const fn code(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::Save => 1,
            Self::Folder => 2,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Open),
            1 => Some(Self::Save),
            2 => Some(Self::Folder),
            _ => None,
        }
    }
}

/// One file type the chooser offers to show: "Images", `*.png` and `*.jpg`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    /// What it is called.
    pub label: String,
    /// Its patterns: `*.png`, `*.tar.gz`, `*` for everything.
    pub patterns: Vec<String>,
}

/// What a program asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// A file to open, a place to save to, or a folder.
    pub mode: Mode,
    /// The window asking, as the compositor numbers it, so the chooser's
    /// window can be kept above it; `0` for none.
    pub owner: u64,
    /// A title of the program's own -- "Export as PDF" -- or empty, for the
    /// chooser's own by mode.
    pub title: String,
    /// The folder to start in; empty for the chooser's own choice.
    pub start: PathBuf,
    /// The name to offer, saving; empty otherwise.
    pub name: OsString,
    /// The file types offered, in order.
    pub filters: Vec<Filter>,
    /// The filter offered first: an index into `filters`, and `0` when there
    /// are none.
    pub filter: usize,
}

/// What a program is answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Nothing was chosen.
    Cancelled,
    /// The user chose `path`, with the filter `filter` in force.
    ///
    /// The path is the one to open or write, as the user confirmed it: any
    /// extension the filter adds is already on it, and saving over a file
    /// that exists has already been agreed to.
    Chosen {
        /// What was chosen; always absolute.
        path: PathBuf,
        /// The filter in force when it was, as the request numbered them.
        filter: usize,
    },
}

/// `request` as a frame.
///
/// # Errors
///
/// [`TooLarge`] if a field is past its bound, a path or name is not one
/// (a NUL; a `/` in the name), or the filter index is past the filters.
pub fn encode_request(request: &Request) -> Result<Vec<u8>, TooLarge> {
    let mut out = Writer::new(PROTOCOL, KIND_REQUEST);
    out.u8(request.mode.code());
    out.u64(request.owner);
    out.text(&request.title, MAX_TEXT)?;
    write_path(&mut out, &request.start, true)?;
    let name = path_bytes(Path::new(&request.name));
    if !is_name(name) {
        return Err(TooLarge);
    }
    out.bytes(name, MAX_NAME)?;
    if request.filters.len() > MAX_FILTERS
        || !filter_in_range(request.filter, request.filters.len())
    {
        return Err(TooLarge);
    }
    out.u8(u8::try_from(request.filters.len()).map_err(|_| TooLarge)?);
    for filter in &request.filters {
        out.text(&filter.label, MAX_TEXT)?;
        if filter.patterns.len() > MAX_PATTERNS {
            return Err(TooLarge);
        }
        out.u8(u8::try_from(filter.patterns.len()).map_err(|_| TooLarge)?);
        for pattern in &filter.patterns {
            out.text(pattern, MAX_PATTERN)?;
        }
    }
    out.u8(u8::try_from(request.filter).map_err(|_| TooLarge)?);
    out.finish()
}

/// `reply` as a frame.
///
/// # Errors
///
/// [`TooLarge`] if the path is past [`MAX_PATH`], empty, relative or holds a
/// NUL, or the filter index does not fit the protocol.
pub fn encode_reply(reply: &Reply) -> Result<Vec<u8>, TooLarge> {
    let mut out = Writer::new(PROTOCOL, KIND_REPLY);
    match reply {
        Reply::Cancelled => out.u8(OUTCOME_CANCELLED),
        Reply::Chosen { path, filter } => {
            if !path.is_absolute() {
                return Err(TooLarge);
            }
            out.u8(OUTCOME_CHOSEN);
            write_path(&mut out, path, false)?;
            out.u8(u8::try_from(*filter).map_err(|_| TooLarge)?);
        }
    }
    out.finish()
}

/// The request at the front of `bytes`, if a whole frame of one is there.
#[must_use]
pub fn decode_request(bytes: &[u8]) -> Decoded<Request> {
    msgframe::decode(bytes, PROTOCOL, KIND_REQUEST, read_request)
}

/// The reply at the front of `bytes`, if a whole frame of one is there.
#[must_use]
pub fn decode_reply(bytes: &[u8]) -> Decoded<Reply> {
    msgframe::decode(bytes, PROTOCOL, KIND_REPLY, read_reply)
}

fn read_request(r: &mut Reader<'_>) -> Option<Request> {
    let mode = Mode::from_code(r.u8()?)?;
    let owner = r.u64()?;
    let title = r.text(MAX_TEXT)?;
    let start = read_path(r, true)?;
    let name = r.bytes(MAX_NAME)?;
    if !is_name(name) {
        return None;
    }
    let name = path_from_bytes(name)?.into_os_string();
    let count = usize::from(r.u8()?);
    if count > MAX_FILTERS {
        return None;
    }
    let mut filters = Vec::with_capacity(count);
    for _ in 0..count {
        let label = r.text(MAX_TEXT)?;
        let patterns = usize::from(r.u8()?);
        if patterns > MAX_PATTERNS {
            return None;
        }
        let patterns = (0..patterns)
            .map(|_| r.text(MAX_PATTERN))
            .collect::<Option<Vec<_>>>()?;
        filters.push(Filter { label, patterns });
    }
    let filter = usize::from(r.u8()?);
    filter_in_range(filter, filters.len()).then_some(Request {
        mode,
        owner,
        title,
        start,
        name,
        filters,
        filter,
    })
}

fn read_reply(r: &mut Reader<'_>) -> Option<Reply> {
    match r.u8()? {
        OUTCOME_CANCELLED => Some(Reply::Cancelled),
        OUTCOME_CHOSEN => {
            let path = read_path(r, false)?;
            let filter = usize::from(r.u8()?);
            path.is_absolute().then_some(Reply::Chosen { path, filter })
        }
        _ => None,
    }
}

/// Whether `filter` names one of `count` filters -- or is `0` where there
/// are none.
const fn filter_in_range(filter: usize, count: usize) -> bool {
    filter < count || (count == 0 && filter == 0)
}

/// Whether `name` can be a file's name: no `/` and no NUL. An empty one is
/// allowed: "offer no name".
fn is_name(name: &[u8]) -> bool {
    !name.iter().any(|&b| b == b'/' || b == 0)
}

/// A path's bytes, as the platform holds them.
fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// The path `bytes` are, on a platform whose paths are bytes.
#[cfg(unix)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the other platforms' form refuses what is not UTF-8; this one has nothing to refuse"
)]
fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

/// The path `bytes` are, on a platform whose paths are not bytes: only what
/// is UTF-8 can be one there, and anything else is refused rather than
/// guessed at.
#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

/// `path`'s bytes into `out`: no NUL in it, and empty only where
/// `may_be_empty`.
fn write_path(out: &mut Writer, path: &Path, may_be_empty: bool) -> Result<(), TooLarge> {
    let bytes = path_bytes(path);
    if bytes.contains(&0) || (bytes.is_empty() && !may_be_empty) {
        return Err(TooLarge);
    }
    out.bytes(bytes, MAX_PATH)
}

/// A path from `r`: no NUL in it, and empty only where `may_be_empty`.
fn read_path(r: &mut Reader<'_>, may_be_empty: bool) -> Option<PathBuf> {
    let bytes = r.bytes(MAX_PATH)?;
    if bytes.contains(&0) || (bytes.is_empty() && !may_be_empty) {
        return None;
    }
    path_from_bytes(bytes)
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
