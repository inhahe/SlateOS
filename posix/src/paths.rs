//! `<paths.h>` — standard path name constants.
//!
//! Defines the default paths used by various system utilities.
//! These are compile-time string constants matching the BSD/glibc
//! `<paths.h>` header.

// ---------------------------------------------------------------------------
// Standard paths
// ---------------------------------------------------------------------------

/// Default shell.
pub const _PATH_BSHELL: &[u8] = b"/bin/sh\0";

/// Console device.
pub const _PATH_CONSOLE: &[u8] = b"/dev/console\0";

/// Default `PATH` environment variable.
pub const _PATH_DEFPATH: &[u8] = b"/usr/bin:/bin\0";

/// Default superuser `PATH`.
pub const _PATH_STDPATH: &[u8] = b"/usr/sbin:/usr/bin:/sbin:/bin\0";

/// Device directory.
pub const _PATH_DEV: &[u8] = b"/dev/\0";

/// Null device.
pub const _PATH_DEVNULL: &[u8] = b"/dev/null\0";

/// TTY device.
pub const _PATH_TTY: &[u8] = b"/dev/tty\0";

/// Mounted file systems table.
pub const _PATH_MOUNTED: &[u8] = b"/etc/mtab\0";

/// File system table.
pub const _PATH_MNTTAB: &[u8] = b"/etc/fstab\0";

/// Temporary directory.
pub const _PATH_TMP: &[u8] = b"/tmp/\0";

/// Var/tmp directory (for larger or longer-lived temp files).
pub const _PATH_VARTMP: &[u8] = b"/var/tmp/\0";

/// Var/run directory (for PID files, sockets, etc.).
pub const _PATH_VARRUN: &[u8] = b"/var/run/\0";

/// Password file.
pub const _PATH_PASSWD: &[u8] = b"/etc/passwd\0";

/// Group file.
pub const _PATH_GROUP: &[u8] = b"/etc/group\0";

/// Shadow password file.
pub const _PATH_SHADOW: &[u8] = b"/etc/shadow\0";

/// Shells file.
pub const _PATH_SHELLS: &[u8] = b"/etc/shells\0";

/// Services file.
pub const _PATH_SERVICES: &[u8] = b"/etc/services\0";

/// Hosts file.
pub const _PATH_HOSTS: &[u8] = b"/etc/hosts\0";

/// Resolver configuration.
pub const _PATH_RESCONF: &[u8] = b"/etc/resolv.conf\0";

/// Locale directory.
pub const _PATH_LOCALEDIR: &[u8] = b"/usr/share/locale\0";

/// Log file.
pub const _PATH_LOG: &[u8] = b"/dev/log\0";

/// Last-login database.
pub const _PATH_LASTLOG: &[u8] = b"/var/log/lastlog\0";

/// utmp file.
pub const _PATH_UTMP: &[u8] = b"/var/run/utmp\0";

/// wtmp file.
pub const _PATH_WTMP: &[u8] = b"/var/log/wtmp\0";

/// Mail spool directory.
pub const _PATH_MAILDIR: &[u8] = b"/var/mail\0";

/// Man pages directory.
pub const _PATH_MAN: &[u8] = b"/usr/share/man\0";

/// Random device.
pub const _PATH_URANDOM: &[u8] = b"/dev/urandom\0";

/// Blocking random device.
pub const _PATH_RANDOM: &[u8] = b"/dev/random\0";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Path strings are null-terminated
    // -----------------------------------------------------------------------

    #[test]
    fn test_paths_null_terminated() {
        let paths: &[&[u8]] = &[
            _PATH_BSHELL,
            _PATH_CONSOLE,
            _PATH_DEFPATH,
            _PATH_STDPATH,
            _PATH_DEV,
            _PATH_DEVNULL,
            _PATH_TTY,
            _PATH_MOUNTED,
            _PATH_MNTTAB,
            _PATH_TMP,
            _PATH_VARTMP,
            _PATH_VARRUN,
            _PATH_PASSWD,
            _PATH_GROUP,
            _PATH_SHADOW,
            _PATH_SHELLS,
            _PATH_SERVICES,
            _PATH_HOSTS,
            _PATH_RESCONF,
            _PATH_LOCALEDIR,
            _PATH_LOG,
            _PATH_LASTLOG,
            _PATH_UTMP,
            _PATH_WTMP,
            _PATH_MAILDIR,
            _PATH_MAN,
            _PATH_URANDOM,
            _PATH_RANDOM,
        ];
        for path in paths {
            assert!(
                path.last() == Some(&0),
                "path should be null-terminated: {:?}",
                core::str::from_utf8(path)
            );
        }
    }

    // -----------------------------------------------------------------------
    // Path strings start with /
    // -----------------------------------------------------------------------

    #[test]
    fn test_paths_absolute() {
        let paths: &[&[u8]] = &[
            _PATH_BSHELL,
            _PATH_CONSOLE,
            _PATH_DEV,
            _PATH_DEVNULL,
            _PATH_TTY,
            _PATH_TMP,
            _PATH_VARTMP,
            _PATH_VARRUN,
            _PATH_PASSWD,
            _PATH_GROUP,
            _PATH_SHADOW,
            _PATH_SHELLS,
            _PATH_SERVICES,
            _PATH_HOSTS,
            _PATH_RESCONF,
            _PATH_LOCALEDIR,
            _PATH_LOG,
            _PATH_LASTLOG,
            _PATH_UTMP,
            _PATH_WTMP,
            _PATH_MAILDIR,
            _PATH_MAN,
            _PATH_URANDOM,
            _PATH_RANDOM,
        ];
        for path in paths {
            assert!(
                path.first() == Some(&b'/'),
                "path should be absolute: {:?}",
                core::str::from_utf8(path)
            );
        }
    }

    // -----------------------------------------------------------------------
    // Specific path contents
    // -----------------------------------------------------------------------

    #[test]
    fn test_path_bshell() {
        assert_eq!(_PATH_BSHELL, b"/bin/sh\0");
    }

    #[test]
    fn test_path_devnull() {
        assert_eq!(_PATH_DEVNULL, b"/dev/null\0");
    }

    #[test]
    fn test_path_tty() {
        assert_eq!(_PATH_TTY, b"/dev/tty\0");
    }

    #[test]
    fn test_path_tmp() {
        assert_eq!(_PATH_TMP, b"/tmp/\0");
    }

    #[test]
    fn test_path_passwd() {
        assert_eq!(_PATH_PASSWD, b"/etc/passwd\0");
    }

    #[test]
    fn test_path_group() {
        assert_eq!(_PATH_GROUP, b"/etc/group\0");
    }

    #[test]
    fn test_path_hosts() {
        assert_eq!(_PATH_HOSTS, b"/etc/hosts\0");
    }

    #[test]
    fn test_path_resconf() {
        assert_eq!(_PATH_RESCONF, b"/etc/resolv.conf\0");
    }

    #[test]
    fn test_path_devnull_no_trailing_slash() {
        // Device paths should not have trailing slash.
        let without_null = &_PATH_DEVNULL[.._PATH_DEVNULL.len() - 1];
        assert!(without_null.last() != Some(&b'/'));
    }

    #[test]
    fn test_path_tmp_trailing_slash() {
        // Directory paths should have trailing slash.
        let without_null = &_PATH_TMP[.._PATH_TMP.len() - 1];
        assert_eq!(without_null.last(), Some(&b'/'));
    }

    #[test]
    fn test_path_dev_trailing_slash() {
        let without_null = &_PATH_DEV[.._PATH_DEV.len() - 1];
        assert_eq!(without_null.last(), Some(&b'/'));
    }

    #[test]
    fn test_path_defpath_contains_bin() {
        // Default PATH must include /bin.
        let s = core::str::from_utf8(&_PATH_DEFPATH[.._PATH_DEFPATH.len() - 1]).unwrap();
        assert!(s.contains("/bin"), "DEFPATH should include /bin");
    }

    #[test]
    fn test_path_stdpath_contains_sbin() {
        let s = core::str::from_utf8(&_PATH_STDPATH[.._PATH_STDPATH.len() - 1]).unwrap();
        assert!(s.contains("/sbin"), "STDPATH should include /sbin");
    }

    #[test]
    fn test_path_urandom() {
        assert_eq!(_PATH_URANDOM, b"/dev/urandom\0");
    }

    #[test]
    fn test_path_random() {
        assert_eq!(_PATH_RANDOM, b"/dev/random\0");
    }
}
