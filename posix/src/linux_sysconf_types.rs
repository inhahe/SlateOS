//! `<unistd.h>` — sysconf() name constants.
//!
//! `sysconf()` queries system configuration values at runtime.
//! These constants are the `name` parameter identifying which
//! value to query.

// ---------------------------------------------------------------------------
// POSIX sysconf names (_SC_*)
// ---------------------------------------------------------------------------

/// Maximum argument length for exec (ARG_MAX).
pub const SC_ARG_MAX: u32 = 0;
/// Maximum number of simultaneous processes per user.
pub const SC_CHILD_MAX: u32 = 1;
/// Clock ticks per second (CLK_TCK).
pub const SC_CLK_TCK: u32 = 2;
/// Maximum number of open files per process.
pub const SC_OPEN_MAX: u32 = 4;
/// Size of a memory page in bytes.
pub const SC_PAGESIZE: u32 = 30;
/// Number of CPUs configured.
pub const SC_NPROCESSORS_CONF: u32 = 83;
/// Number of CPUs currently online.
pub const SC_NPROCESSORS_ONLN: u32 = 84;
/// Total physical memory pages.
pub const SC_PHYS_PAGES: u32 = 85;
/// Available physical memory pages.
pub const SC_AVPHYS_PAGES: u32 = 86;
/// Maximum hostname length.
pub const SC_HOST_NAME_MAX: u32 = 180;
/// Maximum login name length.
pub const SC_LOGIN_NAME_MAX: u32 = 71;
/// Maximum number of supplementary groups.
pub const SC_NGROUPS_MAX: u32 = 3;
/// Maximum length of a terminal device name.
pub const SC_TTY_NAME_MAX: u32 = 72;
/// Maximum length of a timezone name.
pub const SC_TZNAME_MAX: u32 = 6;
/// POSIX version supported.
pub const SC_VERSION: u32 = 29;
/// Line length limit.
pub const SC_LINE_MAX: u32 = 43;
/// Maximum number of semaphores per process.
pub const SC_SEM_NSEMS_MAX: u32 = 31;
/// Maximum value of a semaphore.
pub const SC_SEM_VALUE_MAX: u32 = 32;
/// Thread stack minimum size.
pub const SC_THREAD_STACK_MIN: u32 = 75;
/// Maximum number of threads per process.
pub const SC_THREAD_THREADS_MAX: u32 = 76;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_names_distinct() {
        let names = [
            SC_ARG_MAX, SC_CHILD_MAX, SC_CLK_TCK, SC_OPEN_MAX,
            SC_PAGESIZE, SC_NPROCESSORS_CONF, SC_NPROCESSORS_ONLN,
            SC_PHYS_PAGES, SC_AVPHYS_PAGES, SC_HOST_NAME_MAX,
            SC_LOGIN_NAME_MAX, SC_NGROUPS_MAX, SC_TTY_NAME_MAX,
            SC_TZNAME_MAX, SC_VERSION, SC_LINE_MAX,
            SC_SEM_NSEMS_MAX, SC_SEM_VALUE_MAX,
            SC_THREAD_STACK_MIN, SC_THREAD_THREADS_MAX,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_arg_max_is_zero() {
        assert_eq!(SC_ARG_MAX, 0);
    }

    #[test]
    fn test_pagesize() {
        assert_eq!(SC_PAGESIZE, 30);
    }

    #[test]
    fn test_nprocessors() {
        assert_eq!(SC_NPROCESSORS_CONF, 83);
        assert_eq!(SC_NPROCESSORS_ONLN, 84);
    }
}
