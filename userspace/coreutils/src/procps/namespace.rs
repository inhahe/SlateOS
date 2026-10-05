//! procps-ng 4.0.4's `library/namespace.c`: which namespaces a process is in.
//!
//! A namespace is identified by the inode number of its `/proc/<pid>/ns/<name>`
//! link: two processes are in the same network namespace exactly when their
//! `ns/net` links `stat` to the same inode. `ps -o netns` prints those numbers
//! and `pgrep --ns` compares them, both through these two functions.

use std::path::Path;

/// `ns_names`, indexed by upstream's `PROCPS_NS_*` numbers.
///
/// The order is the library's, not the help text's: `pgrep`'s default set of
/// namespaces to compare is the bit mask `0x3f`, which is the first six of
/// these -- `cgroup` to `time`, and not `user` or `uts` -- whatever its
/// `--help` lists as available.
pub const NS_NAMES: [&str; 8] = ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"];

/// `procps_ns_get_id`: a namespace's number from its name, compared exactly
/// (`strcmp`), or `None` for a name that is not one.
#[must_use]
pub fn ns_get_id(name: &[u8]) -> Option<usize> {
    NS_NAMES.iter().position(|n| n.as_bytes() == name)
}

/// `stat`'s inode number for `path`, following links, as namespace links
/// must be followed to reach the namespace.
#[cfg(unix)]
fn inode(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

/// The host build has no inodes; it never reads a real `/proc`.
#[cfg(not(unix))]
fn inode(_path: &Path) -> Option<u64> {
    None
}

/// `procps_ns_read_pid`: each namespace's inode for process `pid` under
/// `root` (which is `/proc`, but for tests), with 0 for a link `stat` cannot
/// reach.
///
/// `None` is upstream's `-EINVAL`, which it returns for a PID below 1 and for
/// nothing else: a process that does not exist is all zeros, not an error.
#[must_use]
pub fn ns_read_pid(root: &Path, pid: i32) -> Option<[u64; 8]> {
    if pid < 1 {
        return None;
    }
    let mut ns = [0u64; 8];
    for (slot, name) in ns.iter_mut().zip(NS_NAMES) {
        // A failed `stat` is the 0 upstream stores; it is not an error.
        *slot = inode(&root.join(format!("{pid}/ns/{name}"))).unwrap_or(0);
    }
    Some(ns)
}

#[cfg(test)]
mod tests {
    use super::{NS_NAMES, ns_get_id, ns_read_pid};
    use std::path::Path;

    #[test]
    fn names_are_looked_up_exactly() {
        assert_eq!(ns_get_id(b"cgroup"), Some(0));
        assert_eq!(ns_get_id(b"net"), Some(3));
        assert_eq!(ns_get_id(b"uts"), Some(7));
        assert_eq!(ns_get_id(b"NET"), None);
        assert_eq!(ns_get_id(b"ne"), None);
        assert_eq!(ns_get_id(b""), None);
        assert_eq!(NS_NAMES.len(), 8);
    }

    #[test]
    fn a_pid_below_one_is_an_error_and_a_missing_process_is_zeros() {
        let root = Path::new("this-directory-does-not-exist");
        assert_eq!(ns_read_pid(root, 0), None);
        assert_eq!(ns_read_pid(root, -5), None);
        assert_eq!(ns_read_pid(root, 1), Some([0; 8]));
    }
}
