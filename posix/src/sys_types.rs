//! `<sys/types.h>` — primitive system data types.
//!
//! Re-exports all POSIX type definitions from the `types` module.
//! Programs that include `<sys/types.h>` can find everything here.

pub use crate::types::BlkcntT;
pub use crate::types::BlksizeT;
pub use crate::types::ClockidT;
pub use crate::types::DevT;
pub use crate::types::Fd;
pub use crate::types::GidT;
pub use crate::types::IdT;
pub use crate::types::InoT;
pub use crate::types::IntptrT;
pub use crate::types::KeyT;
pub use crate::types::ModeT;
pub use crate::types::NlinkT;
pub use crate::types::OffT;
pub use crate::types::PidT;
pub use crate::types::PtrdiffT;
pub use crate::types::SizeT;
pub use crate::types::SsizeT;
pub use crate::types::SusecondsT;
pub use crate::types::TimeT;
pub use crate::types::UidT;
pub use crate::types::UintptrT;
pub use crate::types::UsecT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_t_size() {
        assert_eq!(core::mem::size_of::<PidT>(), 4);
    }

    #[test]
    fn test_uid_gid_size() {
        assert_eq!(core::mem::size_of::<UidT>(), 4);
        assert_eq!(core::mem::size_of::<GidT>(), 4);
    }

    #[test]
    fn test_off_t_is_64_bit() {
        assert_eq!(core::mem::size_of::<OffT>(), 8);
    }

    #[test]
    fn test_size_ssize_match_pointer() {
        let ptr_size = core::mem::size_of::<usize>();
        assert_eq!(core::mem::size_of::<SizeT>(), ptr_size);
        assert_eq!(core::mem::size_of::<SsizeT>(), ptr_size);
    }

    #[test]
    fn test_time_t_is_64_bit() {
        assert_eq!(core::mem::size_of::<TimeT>(), 8);
    }

    #[test]
    fn test_mode_t_size() {
        assert_eq!(core::mem::size_of::<ModeT>(), 4);
    }

    #[test]
    fn test_dev_t_size() {
        assert_eq!(core::mem::size_of::<DevT>(), 8);
    }

    #[test]
    fn test_ino_t_size() {
        assert_eq!(core::mem::size_of::<InoT>(), 8);
    }

    #[test]
    fn test_intptr_uintptr_match_pointer() {
        let ptr_size = core::mem::size_of::<usize>();
        assert_eq!(core::mem::size_of::<IntptrT>(), ptr_size);
        assert_eq!(core::mem::size_of::<UintptrT>(), ptr_size);
    }

    #[test]
    fn test_cross_module() {
        // Verify all types are identical to the source module.
        assert_eq!(
            core::mem::size_of::<PidT>(),
            core::mem::size_of::<crate::types::PidT>()
        );
        assert_eq!(
            core::mem::size_of::<OffT>(),
            core::mem::size_of::<crate::types::OffT>()
        );
    }
}
