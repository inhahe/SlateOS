//! What `chattr` and `lsattr` take from e2fsprogs' libraries: `lib/e2p`'s
//! `pf.c` (`print_flags`), `fgetflags.c`, `fsetflags.c`, `fgetversion.c`,
//! `fsetversion.c`, `fgetproject.c`, `fsetproject.c` and `iod.c`
//! (`iterate_on_dir`), and `lib/et`'s `com_err`.

use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;

use cstdio::Output;
// `O_CLOEXEC` is what `opendir` opens with.
use libcall::file::{
    AT_FDCWD, Directory, O_CLOEXEC, O_DIRECTORY, O_NOFOLLOW, O_NONBLOCK, O_RDONLY,
};
use libcall::{ENXIO, EOPNOTSUPP};

/// `ELOOP` and `ENOTTY`, which the wrappers turn into [`EOPNOTSUPP`].
const ELOOP: i32 = libcall::pty::ELOOP;
const ENOTTY: i32 = libcall::pty::ENOTTY;

// The flags, as `lib/ext2fs/ext2_fs.h` numbers them.
pub const EXT2_SECRM_FL: u64 = 0x0000_0001;
pub const EXT2_UNRM_FL: u64 = 0x0000_0002;
pub const EXT2_COMPR_FL: u64 = 0x0000_0004;
pub const EXT2_SYNC_FL: u64 = 0x0000_0008;
pub const EXT2_IMMUTABLE_FL: u64 = 0x0000_0010;
pub const EXT2_APPEND_FL: u64 = 0x0000_0020;
pub const EXT2_NODUMP_FL: u64 = 0x0000_0040;
pub const EXT2_NOATIME_FL: u64 = 0x0000_0080;
pub const EXT2_NOCOMPR_FL: u64 = 0x0000_0400;
pub const EXT4_ENCRYPT_FL: u64 = 0x0000_0800;
pub const EXT2_INDEX_FL: u64 = 0x0000_1000;
pub const EXT3_JOURNAL_DATA_FL: u64 = 0x0000_4000;
pub const EXT2_NOTAIL_FL: u64 = 0x0000_8000;
pub const EXT2_DIRSYNC_FL: u64 = 0x0001_0000;
pub const EXT2_TOPDIR_FL: u64 = 0x0002_0000;
pub const EXT4_EXTENTS_FL: u64 = 0x0008_0000;
pub const EXT4_VERITY_FL: u64 = 0x0010_0000;
pub const FS_NOCOW_FL: u64 = 0x0080_0000;
pub const FS_DAX_FL: u64 = 0x0200_0000;
pub const EXT4_INLINE_DATA_FL: u64 = 0x1000_0000;
pub const EXT4_PROJINHERIT_FL: u64 = 0x2000_0000;
pub const EXT4_CASEFOLD_FL: u64 = 0x4000_0000;

/// `pf.c`'s `flags_array`: each flag's letter and long name, in the order
/// `lsattr` prints them.
const FLAGS_ARRAY: [(u64, &str, &str); 22] = [
    (EXT2_SECRM_FL, "s", "Secure_Deletion"),
    (EXT2_UNRM_FL, "u", "Undelete"),
    (EXT2_SYNC_FL, "S", "Synchronous_Updates"),
    (EXT2_DIRSYNC_FL, "D", "Synchronous_Directory_Updates"),
    (EXT2_IMMUTABLE_FL, "i", "Immutable"),
    (EXT2_APPEND_FL, "a", "Append_Only"),
    (EXT2_NODUMP_FL, "d", "No_Dump"),
    (EXT2_NOATIME_FL, "A", "No_Atime"),
    (EXT2_COMPR_FL, "c", "Compression_Requested"),
    (EXT4_ENCRYPT_FL, "E", "Encrypted"),
    (EXT3_JOURNAL_DATA_FL, "j", "Journaled_Data"),
    (EXT2_INDEX_FL, "I", "Indexed_directory"),
    (EXT2_NOTAIL_FL, "t", "No_Tailmerging"),
    (EXT2_TOPDIR_FL, "T", "Top_of_Directory_Hierarchies"),
    (EXT4_EXTENTS_FL, "e", "Extents"),
    (FS_NOCOW_FL, "C", "No_COW"),
    (FS_DAX_FL, "x", "DAX"),
    (EXT4_CASEFOLD_FL, "F", "Casefold"),
    (EXT4_INLINE_DATA_FL, "N", "Inline_Data"),
    (EXT4_PROJINHERIT_FL, "P", "Project_Hierarchy"),
    (EXT4_VERITY_FL, "V", "Verity"),
    (EXT2_NOCOMPR_FL, "m", "Dont_Compress"),
];

/// `print_flags (f, flags, options)`: a letter or a `-` for each flag, or
/// (`long`) the long names of those set, `, `-separated, `---` for none.
pub fn print_flags(out: &mut Output, flags: u64, long: bool) {
    let mut first = true;
    for (flag, short, long_name) in FLAGS_ARRAY {
        if flags & flag != 0 {
            if long {
                if first {
                    first = false;
                } else {
                    out.write(b", ");
                }
                out.write(long_name.as_bytes());
            } else {
                out.write(short.as_bytes());
            }
        } else if !long {
            out.write(b"-");
        }
    }
    if long && first {
        out.write(b"---");
    }
}

/// `name` as a path.
fn path(name: &[u8]) -> &std::path::Path {
    std::path::Path::new(OsStr::from_bytes(name))
}

/// Whether `md` is neither a regular file nor a directory.
fn neither_file_nor_dir(md: &std::fs::Metadata) -> bool {
    !md.is_file() && !md.is_dir()
}

/// A descriptor's status, borrowed.
fn fstat(fd: i32) -> std::io::Result<std::fs::Metadata> {
    use std::os::fd::FromRawFd;
    // SAFETY: `fd` is open and stays open while it is borrowed here; the
    // `File` is never dropped, so it does not close it.
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    file.metadata()
}

/// `open (name, flags)`.
fn open(name: &[u8], flags: i32) -> Result<i32, i32> {
    let c = CString::new(name).map_err(|_| libcall::ENOENT)?;
    libcall::file::openat(AT_FDCWD, &c, flags, 0)
}

/// Close a descriptor read and done with: a failure has nothing to say.
fn close(fd: i32) {
    // Read-only, nothing written: a close cannot lose anything.
    let _ = libcall::fd::close(fd);
}

/// The descriptor `fgetflags` and `fsetflags` work on: refused
/// (`EOPNOTSUPP`) for anything but a regular file or a directory, and for
/// a symbolic link, which `O_NOFOLLOW` will not open.
fn open_for_flags(name: &[u8]) -> Result<i32, i32> {
    if let Ok(md) = std::fs::metadata(path(name)) {
        if neither_file_nor_dir(&md) {
            return Err(EOPNOTSUPP);
        }
    }
    let fd = open(name, O_RDONLY | O_NONBLOCK | O_NOFOLLOW).map_err(|e| {
        if e == ELOOP || e == ENXIO {
            EOPNOTSUPP
        } else {
            e
        }
    })?;
    if let Ok(md) = fstat(fd) {
        if neither_file_nor_dir(&md) {
            close(fd);
            return Err(EOPNOTSUPP);
        }
    }
    Ok(fd)
}

/// `fgetflags (name, &flags)`: the flags, as an `unsigned long` takes an
/// `int` (sign-extended).
pub fn fgetflags(name: &[u8]) -> Result<u64, i32> {
    let fd = open_for_flags(name)?;
    let got = libcall::inode::get_flags(fd).map_err(|e| if e == ENOTTY { EOPNOTSUPP } else { e });
    close(fd);
    got.map(|f| i64::from(f).cast_unsigned())
}

/// `fsetflags (name, flags)`: set as an `int`.
pub fn fsetflags(name: &[u8], flags: u64) -> Result<(), i32> {
    let fd = open_for_flags(name)?;
    let set = libcall::inode::set_flags(fd, cstrtol::low_i32(flags.cast_signed()))
        .map_err(|e| if e == ENOTTY { EOPNOTSUPP } else { e });
    close(fd);
    set
}

/// `fgetversion (name, &version)`.
pub fn fgetversion(name: &[u8]) -> Result<u64, i32> {
    let fd = open(name, O_RDONLY | O_NONBLOCK)?;
    let got = libcall::inode::get_version(fd);
    close(fd);
    got.map(u64::from)
}

/// `fsetversion (name, version)`: set as an `int`.
pub fn fsetversion(name: &[u8], version: u64) -> Result<(), i32> {
    let fd = open(name, O_RDONLY | O_NONBLOCK)?;
    let set = libcall::inode::set_version(fd, cstrtol::low_i32(version.cast_signed()));
    close(fd);
    set
}

/// `fgetproject (name, &project)`.
pub fn fgetproject(name: &[u8]) -> Result<u64, i32> {
    let fd = open(name, O_RDONLY | O_NONBLOCK)?;
    let got = libcall::inode::get_project(fd);
    close(fd);
    got.map(u64::from)
}

/// `fsetproject (name, project)`: `fsx_projid`, a `__u32`, takes the low
/// bits.
pub fn fsetproject(name: &[u8], project: u64) -> Result<(), i32> {
    let fd = open(name, O_RDONLY | O_NONBLOCK)?;
    let set =
        libcall::inode::set_project(fd, cstrtol::low_i32(project.cast_signed()).cast_unsigned());
    close(fd);
    set
}

/// `iterate_on_dir (dir_name, func, private)`: `func` on each entry of the
/// directory, `.` and `..` included, in the order the file system keeps
/// them. How many calls answered non-zero, or -1 (and nothing called) when
/// the directory cannot be opened.
pub fn iterate_on_dir(dir_name: &[u8], func: &mut dyn FnMut(&[u8], &[u8]) -> i32) -> i32 {
    let Ok(fd) = open(dir_name, O_RDONLY | O_NONBLOCK | O_DIRECTORY | O_CLOEXEC) else {
        return -1;
    };
    let mut dir = match Directory::from_fd(fd) {
        Ok(d) => d,
        Err(_) => {
            close(fd);
            return -1;
        }
    };
    let mut ret = 0i32;
    let mut buf = [0u8; 256];
    // A read error ends the walk, as `readdir`'s NULL does.
    while let Some(Ok(name)) = dir.next_name(&mut buf) {
        if func(dir_name, name) != 0 {
            ret = ret.saturating_add(1);
        }
    }
    // The stream is the descriptor's; closing it has nothing to report.
    let _ = dir.close();
    ret
}

/// `com_err (whoami, code, fmt...)`: `whoami: `, the error's words and a
/// space when there is one, `message`, and a newline -- after a carriage
/// return when standard error is a terminal that does not turn one newline
/// into both itself.
pub fn com_err(err: &mut Output, whoami: &[u8], code: i32, message: &[u8]) {
    err.write(whoami);
    err.write(b": ");
    if code != 0 {
        let words = errmsg::strerror(&std::io::Error::from_raw_os_error(code));
        err.write(words.as_bytes());
        err.write(b" ");
    }
    err.write(message);
    if needs_carriage_return() {
        err.write(b"\r");
    }
    err.write(b"\n");
    err.flush();
}

/// Whether standard error is a terminal whose output processing does not
/// put a carriage return before each newline (`OPOST` and `ONLCR`).
fn needs_carriage_return() -> bool {
    if !libcall::fd::is_terminal(2) {
        return false;
    }
    match libcall::termios::get_attr(2) {
        Ok(t) => {
            !(t.c_oflag & libcall::termios::OPOST != 0 && t.c_oflag & libcall::termios::ONLCR != 0)
        }
        Err(_) => true,
    }
}

/// `lstat (name)`.
pub fn lstat(name: &[u8]) -> Result<std::fs::Metadata, i32> {
    std::fs::symlink_metadata(path(name)).map_err(|e| e.raw_os_error().unwrap_or(libcall::EINVAL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cstdio::Sink;

    fn flags(f: u64, long: bool) -> Vec<u8> {
        let mut out = Output::new(Sink::Memory(Vec::new()), false, false);
        print_flags(&mut out, f, long);
        out.take_written()
    }

    #[test]
    fn flags_print_as_lsattr_prints_them() {
        assert_eq!(flags(0, false), b"----------------------");
        assert_eq!(flags(EXT4_EXTENTS_FL, false), b"--------------e-------");
        assert_eq!(
            flags(EXT2_IMMUTABLE_FL | EXT2_NOCOMPR_FL, false),
            b"----i----------------m"
        );
        assert_eq!(flags(0, true), b"---");
        assert_eq!(
            flags(EXT2_APPEND_FL | EXT4_EXTENTS_FL, true),
            b"Append_Only, Extents"
        );
    }
}
