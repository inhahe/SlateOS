//! `<grp.h>` — group database access.
//!
//! Re-exports group-database functions from the `pwd` module, which
//! implements both password and group databases.

pub use crate::pwd::Group;
pub use crate::pwd::getgrnam;
pub use crate::pwd::getgrgid;
pub use crate::pwd::getgrnam_r;
pub use crate::pwd::getgrgid_r;
pub use crate::pwd::setgrent;
pub use crate::pwd::getgrent;
pub use crate::pwd::endgrent;
pub use crate::pwd::getgrouplist;
pub use crate::pwd::initgroups;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_struct_size() {
        assert!(core::mem::size_of::<Group>() > 0);
    }

    #[test]
    fn test_getgrnam_root() {
        let g = unsafe { getgrnam(b"root\0".as_ptr()) };
        assert!(!g.is_null());
        let gr = unsafe { &*g };
        assert_eq!(gr.gr_gid, 0);
    }

    #[test]
    fn test_getgrnam_unknown() {
        let g = unsafe { getgrnam(b"no_such_group_xyz_999\0".as_ptr()) };
        assert!(g.is_null());
    }

    #[test]
    fn test_getgrgid_root() {
        let g = getgrgid(0);
        assert!(!g.is_null());
    }

    #[test]
    fn test_getgrgid_unknown() {
        let g = getgrgid(99999);
        assert!(g.is_null());
    }

    #[test]
    fn test_setgrent_endgrent() {
        setgrent();
        endgrent();
        // Should not panic.
    }

    #[test]
    fn test_getgrent_returns_entry() {
        setgrent();
        let g = getgrent();
        // Should return at least the built-in "root" group.
        assert!(!g.is_null());
        endgrent();
    }

    #[test]
    fn test_cross_module() {
        // Verify re-exports match the source module.
        let a = unsafe { crate::pwd::getgrnam(b"root\0".as_ptr()) };
        let b = unsafe { getgrnam(b"root\0".as_ptr()) };
        assert_eq!(a, b);
    }
}
