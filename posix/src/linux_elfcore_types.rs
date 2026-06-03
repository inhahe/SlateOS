//! `<linux/elfcore.h>` — ELF core-dump note constants.
//!
//! Note types (`n_type`) found in core-dump ELF notes — used by
//! crash, gdb, and our own `coredumpctl` equivalent to parse
//! `/proc/<pid>/core` images.

// ---------------------------------------------------------------------------
// Standard core-dump note types
// ---------------------------------------------------------------------------

/// NT_PRSTATUS — struct elf_prstatus (general-purpose registers).
pub const NT_PRSTATUS: u32 = 1;
/// NT_PRFPREG — floating-point registers.
pub const NT_PRFPREG: u32 = 2;
/// NT_PRPSINFO — process info (struct elf_prpsinfo).
pub const NT_PRPSINFO: u32 = 3;
/// NT_TASKSTRUCT — task struct image (Linux-specific debugging).
pub const NT_TASKSTRUCT: u32 = 4;
/// NT_AUXV — process auxiliary vector.
pub const NT_AUXV: u32 = 6;
/// NT_SIGINFO — siginfo_t at signal-handling time.
pub const NT_SIGINFO: u32 = 0x53494749; // "SIGI" little-endian
/// NT_FILE — file mappings dumped as (start, end, file_ofs, name) records.
pub const NT_FILE: u32 = 0x4649_4c45; // "FILE"

// ---------------------------------------------------------------------------
// x86_64-specific extended state notes
// ---------------------------------------------------------------------------

/// NT_PRXFPREG — Linux-style extended FP.
pub const NT_PRXFPREG: u32 = 0x46e62b7f;
/// NT_X86_XSTATE — XSAVE state area.
pub const NT_X86_XSTATE: u32 = 0x202;

// ---------------------------------------------------------------------------
// Arch-independent extension notes
// ---------------------------------------------------------------------------

/// Per-thread TLS area on architectures that support it.
pub const NT_386_TLS: u32 = 0x200;
/// 386 IOPERM bitmap.
pub const NT_386_IOPERM: u32 = 0x201;

// ---------------------------------------------------------------------------
// Linux process-status note flags (struct elf_prstatus.pr_flag)
// ---------------------------------------------------------------------------

/// Process is currently executing.
pub const ELF_PR_RUNNING: u32 = 0;
/// Process is sleeping.
pub const ELF_PR_SLEEPING: u32 = 1;
/// Process is stopped.
pub const ELF_PR_STOPPED: u32 = 2;
/// Process is a zombie.
pub const ELF_PR_ZOMBIE: u32 = 3;
/// Process is dead (rare in core dumps).
pub const ELF_PR_DEAD: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_notes_distinct() {
        let notes = [
            NT_PRSTATUS,
            NT_PRFPREG,
            NT_PRPSINFO,
            NT_TASKSTRUCT,
            NT_AUXV,
            NT_SIGINFO,
            NT_FILE,
        ];
        for i in 0..notes.len() {
            for j in (i + 1)..notes.len() {
                assert_ne!(notes[i], notes[j]);
            }
        }
    }

    #[test]
    fn test_x86_notes_distinct_from_standard() {
        // The XSTATE / TLS / IOPERM note types must not collide with
        // the canonical NT_PR* notes; debuggers select handlers by
        // exact match.
        let standard = [
            NT_PRSTATUS,
            NT_PRFPREG,
            NT_PRPSINFO,
            NT_TASKSTRUCT,
            NT_AUXV,
        ];
        let arch = [NT_PRXFPREG, NT_X86_XSTATE, NT_386_TLS, NT_386_IOPERM];
        for &a in &arch {
            for &s in &standard {
                assert_ne!(a, s);
            }
        }
    }

    #[test]
    fn test_signature_notes_are_ascii() {
        // NT_FILE / NT_SIGINFO encode their 4-byte ASCII tag as a
        // big-endian magic value — the kernel writes the byte sequence
        // {'F','I','L','E'} / {'S','I','G','I'} into the note header
        // n_type field. A regression here means we silently misread
        // file-mapping or siginfo notes in core dumps.
        assert_eq!(NT_FILE.to_be_bytes(), *b"FILE");
        assert_eq!(NT_SIGINFO.to_be_bytes(), *b"SIGI");
    }

    #[test]
    fn test_pr_flags_distinct() {
        let flags = [
            ELF_PR_RUNNING,
            ELF_PR_SLEEPING,
            ELF_PR_STOPPED,
            ELF_PR_ZOMBIE,
            ELF_PR_DEAD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
