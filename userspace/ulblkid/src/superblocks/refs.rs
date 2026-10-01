//! `refs.c`: Microsoft ReFS, by its magic alone.

use crate::USAGE_FILESYSTEM;
use crate::probe::{IdInfo, IdMag};

/// `refs_idinfo`.
pub static REFS: IdInfo = IdInfo {
    name: "ReFS",
    usage: USAGE_FILESYSTEM,
    flags: 0,
    minsz: 0,
    probefunc: None,
    magics: &[IdMag::new(b"\0\0\0ReFS\0", 0, 0)],
};
