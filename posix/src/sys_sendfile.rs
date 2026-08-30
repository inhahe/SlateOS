//! `<sys/sendfile.h>` — zero-copy file-to-socket data transfer.
//!
//! Re-exports `sendfile` and `sendfile64` from the `file` module.
//! Programs that include `<sys/sendfile.h>` can find the functions
//! here.

pub use crate::file::sendfile;
pub use crate::file::sendfile64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sendfile_accessible() {
        // Verify we can call sendfile through this module.
        let result = sendfile(1, 0, core::ptr::null_mut(), 0);
        // Stub returns 0 for zero count.
        assert_eq!(result, 0);
    }

    #[test]
    fn test_sendfile64_accessible() {
        let result = sendfile64(1, 0, core::ptr::null_mut(), 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_sendfile_with_offset() {
        let mut off: i64 = 0;
        let result = sendfile(1, 0, &raw mut off, 0);
        assert_eq!(result, 0);
    }
}
