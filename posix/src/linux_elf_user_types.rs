//! `<linux/elf.h>` — userspace-facing ELF constants (auxv, notes, e_ident).
//!
//! Every dynamic loader, core-file analyzer (gdb, eu-readelf), and
//! profiler (perf, BCC) walks ELF auxiliary vectors and note records.
//! The subset here is the part the kernel guarantees to userspace
//! when it builds the initial stack and core dumps.

// ---------------------------------------------------------------------------
// e_ident magic
// ---------------------------------------------------------------------------

/// `\x7f`.
pub const ELFMAG0: u8 = 0x7f;
/// `'E'`.
pub const ELFMAG1: u8 = b'E';
/// `'L'`.
pub const ELFMAG2: u8 = b'L';
/// `'F'`.
pub const ELFMAG3: u8 = b'F';
/// 4-byte magic at the start of every ELF file.
pub const ELFMAG: [u8; 4] = [ELFMAG0, ELFMAG1, ELFMAG2, ELFMAG3];
/// Number of bytes compared by ELFMAG check.
pub const SELFMAG: usize = 4;

// ---------------------------------------------------------------------------
// ELF classes
// ---------------------------------------------------------------------------

/// `ELFCLASSNONE`.
pub const ELFCLASSNONE: u8 = 0;
/// `ELFCLASS32`.
pub const ELFCLASS32: u8 = 1;
/// `ELFCLASS64`.
pub const ELFCLASS64: u8 = 2;

// ---------------------------------------------------------------------------
// Data encodings
// ---------------------------------------------------------------------------

/// `ELFDATANONE`.
pub const ELFDATANONE: u8 = 0;
/// `ELFDATA2LSB` — little-endian.
pub const ELFDATA2LSB: u8 = 1;
/// `ELFDATA2MSB` — big-endian.
pub const ELFDATA2MSB: u8 = 2;

// ---------------------------------------------------------------------------
// Auxiliary vector entry types (AT_*)
// ---------------------------------------------------------------------------

/// End of auxv.
pub const AT_NULL: u32 = 0;
/// File-descriptor of program.
pub const AT_IGNORE: u32 = 1;
/// File descriptor of executable.
pub const AT_EXECFD: u32 = 2;
/// Program header table address.
pub const AT_PHDR: u32 = 3;
/// Size of a program header entry.
pub const AT_PHENT: u32 = 4;
/// Number of program headers.
pub const AT_PHNUM: u32 = 5;
/// System page size.
pub const AT_PAGESZ: u32 = 6;
/// Interpreter base address.
pub const AT_BASE: u32 = 7;
/// Flags.
pub const AT_FLAGS: u32 = 8;
/// Program entry point.
pub const AT_ENTRY: u32 = 9;
/// Real UID.
pub const AT_UID: u32 = 11;
/// Effective UID.
pub const AT_EUID: u32 = 12;
/// Real GID.
pub const AT_GID: u32 = 13;
/// Effective GID.
pub const AT_EGID: u32 = 14;
/// Platform name string.
pub const AT_PLATFORM: u32 = 15;
/// HWCAP bitmask.
pub const AT_HWCAP: u32 = 16;
/// Clock tick frequency.
pub const AT_CLKTCK: u32 = 17;
/// Filename of executable.
pub const AT_EXECFN: u32 = 31;
/// vDSO address.
pub const AT_SYSINFO: u32 = 32;
/// vDSO ELF header address.
pub const AT_SYSINFO_EHDR: u32 = 33;
/// Random 16 bytes for stack-protector canary seeding.
pub const AT_RANDOM: u32 = 25;
/// Secure-mode indicator (suid/sgid).
pub const AT_SECURE: u32 = 23;
/// Extra HWCAP.
pub const AT_HWCAP2: u32 = 26;

// ---------------------------------------------------------------------------
// Common core-note types (n_type)
// ---------------------------------------------------------------------------

/// General-purpose registers.
pub const NT_PRSTATUS: u32 = 1;
/// FP registers.
pub const NT_PRFPREG: u32 = 2;
/// `prpsinfo` (pid, command, state).
pub const NT_PRPSINFO: u32 = 3;
/// `taskstats` snapshot.
pub const NT_TASKSTRUCT: u32 = 4;
/// Auxv replay.
pub const NT_AUXV: u32 = 6;
/// File-map info for core dump.
pub const NT_FILE: u32 = 0x4646_4946;
/// Siginfo of crashing signal.
pub const NT_SIGINFO: u32 = 0x5349_4749;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        // ELF magic 0x7f, 'E', 'L', 'F' must remain byte-stable.
        assert_eq!(ELFMAG, [0x7f, b'E', b'L', b'F']);
        assert_eq!(SELFMAG, 4);
    }

    #[test]
    fn test_class_and_data_dense() {
        assert_eq!(ELFCLASSNONE, 0);
        assert_eq!(ELFCLASS32, 1);
        assert_eq!(ELFCLASS64, 2);
        assert_eq!(ELFDATANONE, 0);
        assert_eq!(ELFDATA2LSB, 1);
        assert_eq!(ELFDATA2MSB, 2);
    }

    #[test]
    fn test_auxv_well_known_distinct() {
        let a = [
            AT_NULL,
            AT_IGNORE,
            AT_EXECFD,
            AT_PHDR,
            AT_PHENT,
            AT_PHNUM,
            AT_PAGESZ,
            AT_BASE,
            AT_FLAGS,
            AT_ENTRY,
            AT_UID,
            AT_EUID,
            AT_GID,
            AT_EGID,
            AT_PLATFORM,
            AT_HWCAP,
            AT_CLKTCK,
            AT_SECURE,
            AT_RANDOM,
            AT_HWCAP2,
            AT_EXECFN,
            AT_SYSINFO,
            AT_SYSINFO_EHDR,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        // AT_NULL terminates the vector — must be zero.
        assert_eq!(AT_NULL, 0);
        // AT_PHDR..AT_ENTRY are dense 3..9; loaders rely on that order.
        assert_eq!(AT_PHDR, 3);
        assert_eq!(AT_PHENT, 4);
        assert_eq!(AT_PHNUM, 5);
        assert_eq!(AT_PAGESZ, 6);
        assert_eq!(AT_BASE, 7);
        assert_eq!(AT_FLAGS, 8);
        assert_eq!(AT_ENTRY, 9);
    }

    #[test]
    fn test_core_notes_distinct() {
        let n = [
            NT_PRSTATUS,
            NT_PRFPREG,
            NT_PRPSINFO,
            NT_TASKSTRUCT,
            NT_AUXV,
            NT_FILE,
            NT_SIGINFO,
        ];
        for i in 0..n.len() {
            for j in (i + 1)..n.len() {
                assert_ne!(n[i], n[j]);
            }
        }
        // NT_FILE is "FIFF" ASCII (little-endian read), NT_SIGINFO is
        // "IGIS" — distinctive cookies in core files.
        assert_eq!(NT_FILE, u32::from_le_bytes(*b"FIFF"));
        assert_eq!(NT_SIGINFO, u32::from_le_bytes(*b"IGIS"));
    }
}
