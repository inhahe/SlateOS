//! `bfs.c`: the SCO/UnixWare boot filesystem, by its magic alone.

use crate::USAGE_FILESYSTEM;
use crate::probe::{IdInfo, IdMag};

/// `bfs_idinfo`.
pub static BFS: IdInfo = IdInfo {
    name: "bfs",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: None,
    magics: &[IdMag::new(b"\xce\xfa\xad\x1b", 0, 0)],
};
