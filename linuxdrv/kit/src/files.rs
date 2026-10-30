//! Files, read by path with the process's own authority: what Linux's
//! firmware loader reads its firmware through (`linuxdrv/emul/lx_fs.c`'s
//! `kernel_read_file_from_path_initns`, design-decisions 1590).
//!
//! The system's file calls answer the kernel's own error codes
//! (`kernel/src/error.rs`); the C half is Linux's code, which reads errnos,
//! so each answer is translated here, as the kernel's own Linux entry points
//! translate theirs (`linux_errno_for` in `kernel/src/syscall/linux.rs`),
//! for the codes a file's open, stat or read is known to answer (any other
//! is `-EIO`). The tests read both kernel files, so a translation here that
//! disagrees with the kernel's fails; and the stat record's layout, which
//! the kernel writes whole, against the kernel's own.

/// Linux's errnos the translation answers, by their numbers in Linux's ABI
/// (`include/uapi/asm-generic/errno-base.h`, `errno.h`).
pub mod errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const EBADF: i32 = 9;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EBUSY: i32 = 16;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const EFBIG: i32 = 27;
    pub const ENOSYS: i32 = 38;
    pub const ELOOP: i32 = 40;
    pub const ESTALE: i32 = 116;
}

/// The kernel's codes a file's open, stat or read is known to answer --
/// a FIFO's or socket's open `NoSuchDeviceOrAddress`, a file system that
/// cannot read `NotSupported` -- each with the errno the kernel's own Linux
/// entry points give for it: (code, the `KernelError` variant's name,
/// errno).
pub const TRANSLATED: [(i64, &str, i32); 22] = [
    (-2, "NotSupported", errno::ENOSYS),
    (-3, "InvalidArgument", errno::EINVAL),
    (-8, "Interrupted", errno::EINTR),
    (-100, "OutOfMemory", errno::ENOMEM),
    (-101, "InvalidAddress", errno::EFAULT),
    (-304, "ResourceExhausted", errno::ENFILE),
    (-400, "PermissionDenied", errno::EACCES),
    (-401, "InvalidCapability", errno::EPERM),
    (-402, "NotPermitted", errno::EPERM),
    (-500, "NotFound", errno::ENOENT),
    (-502, "NotADirectory", errno::ENOTDIR),
    (-503, "IsADirectory", errno::EISDIR),
    (-505, "InvalidHandle", errno::EBADF),
    (-506, "TooManyLinks", errno::ELOOP),
    (-508, "CorruptedData", errno::EIO),
    (-510, "TooManyOpenFiles", errno::EMFILE),
    (-511, "FileTooLarge", errno::EFBIG),
    (-513, "StaleHandle", errno::ESTALE),
    (-600, "IoError", errno::EIO),
    (-601, "NoSuchDevice", errno::ENODEV),
    (-602, "DeviceBusy", errno::EBUSY),
    (-603, "NoSuchDeviceOrAddress", errno::ENXIO),
];

/// The negative errno for the kernel's (negative) code `code`: its
/// translation, or `-EIO` for a code no file call is expected to answer --
/// a failure all the same, which Linux's code reports as one.
#[must_use]
pub fn errno_for(code: i64) -> i64 {
    let e = TRANSLATED
        .iter()
        .find(|&&(c, _, _)| c == code)
        .map_or(errno::EIO, |&(_, _, e)| e);
    negative(e)
}

/// A file call's answer as the C half takes it: a count or a handle as it
/// is, a failure as Linux's errno.
#[must_use]
pub fn answer(ret: i64) -> i64 {
    if ret < 0 { errno_for(ret) } else { ret }
}

/// The errno `e` as a failure is answered: negated. An errno is small and
/// positive, so the negation never wraps.
#[must_use]
pub fn negative(e: i32) -> i64 {
    i64::from(e).wrapping_neg()
}

/// The file calls' numbers (`kernel/src/syscall/number.rs`).
pub const SYS_FS_OPEN: u64 = 610;
pub const SYS_FS_CLOSE: u64 = 611;
pub const SYS_FS_FSTAT: u64 = 617;
pub const SYS_FS_PREAD: u64 = 1080;
/// `SYS_FS_OPEN`'s flags (`kernel/src/fs/handle.rs`'s `OpenFlags`): to
/// read, and a directory too -- as Linux's own open for reading takes one
/// -- which the C half then refuses as Linux's `kernel_read_file` does, for
/// not being a regular file.
pub const O_READ: u64 = 1 << 0;
pub const O_DIRECTORY_ALLOWED: u64 = 1 << 9;

/// The size of the native stat record (`SYS_FS_FSTAT`, the kernel's
/// `FS_STAT_RESULT_LEN`).
pub const STAT_LEN: usize = 80;

/// What a stat record says of a file, as the C half asks: its size in
/// bytes, whether it is a regular file (type byte 0, the kernel's
/// `EntryType::File`), and its change time -- moved by every write -- in
/// nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub size: u64,
    pub regular: bool,
    pub changed_ns: u64,
}

/// The little-endian `u64` at `at` in `record`.
fn word(record: &[u8; STAT_LEN], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(record.get(at..at.saturating_add(8)).unwrap_or(&[0; 8]));
    u64::from_le_bytes(w)
}

/// The record's size (at 0), type (the byte at 8) and change time (at 56),
/// as the kernel's `encode_fs_stat_result` lays them out.
#[must_use]
pub fn decode_stat(record: &[u8; STAT_LEN]) -> Stat {
    Stat {
        size: word(record, 0),
        regular: record.get(8) == Some(&0),
        changed_ns: word(record, 56),
    }
}

/// The C half's calls: `slx_file_*` in `linuxdrv/emul/slx.h`.
#[cfg(target_os = "none")]
mod calls {
    use super::{
        O_DIRECTORY_ALLOWED, O_READ, STAT_LEN, SYS_FS_CLOSE, SYS_FS_FSTAT, SYS_FS_OPEN,
        SYS_FS_PREAD, answer, decode_stat, errno, errno_for, negative,
    };
    use crate::sys::{syscall3, syscall4};

    /// `slx_file_open`: the file at `path` opened to read: its handle, or a
    /// negative errno.
    ///
    /// # Safety
    ///
    /// `path` is a NUL-terminated string.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn slx_file_open(path: *const u8) -> i64 {
        // SAFETY: the caller's.
        let path = unsafe { core::ffi::CStr::from_ptr(path.cast()) }.to_bytes();
        if path.is_empty() {
            return negative(errno::EINVAL);
        }
        answer(syscall3(
            SYS_FS_OPEN,
            path.as_ptr() as u64,
            path.len() as u64,
            O_READ | O_DIRECTORY_ALLOWED,
        ))
    }

    /// `slx_file_stat`: the open file `handle`'s size, at `*regular`
    /// whether it is a regular file, and at `*changed_ns` its change time;
    /// or a negative errno (`-EFBIG` for a size the answer cannot carry).
    ///
    /// # Safety
    ///
    /// `regular` points at a writable `bool`, `changed_ns` at a writable
    /// `u64`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn slx_file_stat(
        handle: i64,
        regular: *mut bool,
        changed_ns: *mut u64,
    ) -> i64 {
        let mut record = [0u8; STAT_LEN];
        let ret = syscall3(
            SYS_FS_FSTAT,
            handle.cast_unsigned(),
            record.as_mut_ptr() as u64,
            0,
        );
        if ret < 0 {
            return errno_for(ret);
        }
        let stat = decode_stat(&record);
        // SAFETY: the caller's.
        unsafe {
            regular.write(stat.regular);
            changed_ns.write(stat.changed_ns);
        }
        i64::try_from(stat.size).unwrap_or(negative(errno::EFBIG))
    }

    /// `slx_file_read`: up to `len` bytes of the open file `handle` from
    /// byte `offset` into `buf`: how many (0 at or past its end), or a
    /// negative errno.
    ///
    /// # Safety
    ///
    /// `buf` points at `len` writable bytes.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn slx_file_read(
        handle: i64,
        buf: *mut u8,
        len: usize,
        offset: u64,
    ) -> i64 {
        answer(syscall4(
            SYS_FS_PREAD,
            handle.cast_unsigned(),
            buf as u64,
            len as u64,
            offset,
        ))
    }

    /// `slx_file_close`: close the open file `handle`. A close the system
    /// refuses leaves nothing for the caller to do: the handle was this
    /// process's, and its exit closes what it holds.
    #[unsafe(no_mangle)]
    pub extern "C" fn slx_file_close(handle: i64) {
        syscall3(SYS_FS_CLOSE, handle.cast_unsigned(), 0, 0);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used)]

    use super::*;
    use std::collections::HashMap;

    fn kernel_file(rel: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../kernel/src")
            .join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// `Name = -N,` lines of `KernelError`'s declaration.
    fn kernel_codes() -> HashMap<String, i64> {
        kernel_file("error.rs")
            .lines()
            .filter_map(|l| {
                let (name, code) = l.trim().strip_suffix(',')?.split_once(" = ")?;
                Some((name.to_string(), code.parse().ok()?))
            })
            .filter(|(name, _)| name.chars().all(char::is_alphanumeric))
            .collect()
    }

    /// `linux_errno_for`'s arms, variant to errno number, through the
    /// kernel's own `errno` module.
    fn kernel_translation() -> HashMap<String, i32> {
        let linux = kernel_file("syscall/linux.rs");
        let numbers: HashMap<&str, i32> = linux
            .lines()
            .filter_map(|l| {
                let rest = l.trim().strip_prefix("pub const E")?;
                let (name, value) = rest.split_once(": i32 = ")?;
                Some((name, value.strip_suffix(';')?.parse().ok()?))
            })
            .collect();
        let body = linux
            .split("pub const fn linux_errno_for(")
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("linux_errno_for in kernel/src/syscall/linux.rs");
        body.lines()
            .filter_map(|l| {
                let rest = l.trim().strip_prefix("KernelError::")?;
                let (variant, e) = rest.split_once(" => errno::E")?;
                let number = *numbers.get(e.strip_suffix(',')?)?;
                Some((variant.to_string(), number))
            })
            .collect()
    }

    #[test]
    fn each_translation_is_the_kernels_own() {
        let codes = kernel_codes();
        let translation = kernel_translation();
        assert!(
            codes.len() > 40 && translation.len() > 40,
            "the kernel's files were not read"
        );
        for (code, name, e) in TRANSLATED {
            assert_eq!(codes.get(name), Some(&code), "KernelError::{name}'s code");
            assert_eq!(
                translation.get(name),
                Some(&e),
                "linux_errno_for(KernelError::{name})"
            );
            assert_eq!(errno_for(code), -i64::from(e), "{name}");
        }
    }

    #[test]
    fn an_answer_is_a_count_or_a_handle_or_an_errno() {
        for ok in [0, 1, 70_001, i64::MAX] {
            assert_eq!(answer(ok), ok);
        }
        assert_eq!(answer(-500), negative(errno::ENOENT));
        assert_eq!(answer(-1), negative(errno::EIO));
    }

    /// The calls' numbers and the open's flags are the kernel's.
    #[test]
    fn the_calls_and_flags_are_the_kernels() {
        let numbers = kernel_file("syscall/number.rs");
        for (name, value) in [
            ("SYS_FS_OPEN", SYS_FS_OPEN),
            ("SYS_FS_CLOSE", SYS_FS_CLOSE),
            ("SYS_FS_FSTAT", SYS_FS_FSTAT),
            ("SYS_FS_PREAD", SYS_FS_PREAD),
        ] {
            assert!(
                numbers.contains(&format!("pub const {name}: u64 = {value};")),
                "{name} is not {value}"
            );
        }
        let handle = kernel_file("fs/handle.rs");
        for (name, bit) in [("READ", O_READ), ("DIRECTORY_ALLOWED", O_DIRECTORY_ALLOWED)] {
            let shift = bit.trailing_zeros();
            assert_eq!(bit, 1 << shift);
            assert!(
                handle.contains(&format!("pub const {name}: Self = Self(1 << {shift});")),
                "OpenFlags::{name} is not 1 << {shift}"
            );
        }
    }

    #[test]
    fn a_code_no_file_call_answers_is_an_io_error() {
        // `ChannelClosed`, and a code the kernel has none for.
        assert_eq!(errno_for(-300), -i64::from(errno::EIO));
        assert_eq!(errno_for(-9999), -i64::from(errno::EIO));
    }

    #[test]
    fn the_stat_record_gives_the_size_whether_regular_and_the_change_time() {
        let mut record = [0u8; STAT_LEN];
        record[..8].copy_from_slice(&70_001u64.to_le_bytes());
        // The modification time beside it (at 40), which is not read.
        record[40..48].copy_from_slice(&5u64.to_le_bytes());
        record[56..64].copy_from_slice(&1_761_000_000_123_456_789u64.to_le_bytes());
        assert_eq!(
            decode_stat(&record),
            Stat {
                size: 70_001,
                regular: true,
                changed_ns: 1_761_000_000_123_456_789,
            }
        );
        // A directory (1), a symlink (3), a FIFO (7): not regular.
        for kind in [1u8, 3, 7] {
            record[8] = kind;
            assert!(!decode_stat(&record).regular, "type {kind}");
        }
        record[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode_stat(&record).size, u64::MAX);
    }

    /// The record is the kernel's to lay out, and `SYS_FS_FSTAT` writes all
    /// of it whatever the caller's buffer: its length and the three fields
    /// read here must be what `encode_fs_stat_result` writes.
    #[test]
    fn the_stat_record_is_laid_out_as_the_kernel_writes_it() {
        let handlers = kernel_file("syscall/handlers.rs");
        assert!(
            handlers.contains(&format!(
                "pub const FS_STAT_RESULT_LEN: usize = {STAT_LEN};"
            )),
            "FS_STAT_RESULT_LEN is not {STAT_LEN}"
        );
        for field in [
            "put(0, &meta.size.to_le_bytes());",
            "put(8, &[type_byte]);",
            "put(56, &meta.changed_ns.to_le_bytes());",
        ] {
            assert!(
                handlers.contains(field),
                "encode_fs_stat_result no longer has `{field}`"
            );
        }
        let vfs = kernel_file("fs/vfs.rs");
        let type_byte = vfs
            .split("pub const fn type_byte(self) -> u8 {")
            .nth(1)
            .expect("EntryType::type_byte in kernel/src/fs/vfs.rs");
        assert!(
            type_byte
                .trim_start()
                .starts_with("match self {\n            Self::File => 0,"),
            "a regular file's type byte is no longer 0"
        );
    }
}
