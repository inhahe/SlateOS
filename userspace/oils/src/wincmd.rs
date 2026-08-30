//! Encoding a child's arguments for a Windows command line that the child will
//! parse by Cygwin's rules.
//!
//! Windows has no argv. The parent builds one string, the child decodes it, and
//! correctness is entirely a matter of the two agreeing on the encoding.
//! [`std::process::Command::arg`] encodes for the Microsoft C runtime, which is
//! what a program built by MSVC — or by MinGW, which borrows the convention —
//! decodes with. A Cygwin or MSYS2 program does not: its runtime re-parses the
//! command line itself, with rules of its own, on the assumption that no Unix
//! shell was there to split it — so it also *glob- and tilde-expands* what it
//! finds.
//!
//! Both halves corrupt arguments the shell has already finished expanding:
//! `sed -E 's/x/\1/'` arrives with the backslash eaten, and `tr '\n' '~'`
//! arrives with `~` replaced by the home directory. The fix is to encode per
//! callee — hence [`is_cygwin_program`], which asks an executable's import
//! table which runtime it links against, and [`quote_arg`], which spells an
//! argument the way that runtime reads it back.
//!
//! The rules below are measured, not recalled (Cygwin 3.x via MSYS2 bash; see
//! `known-issues.md` TD-OILS-WIN-ARG-QUOTING for the full table). Inside a
//! `"…"` section `\\` is a backslash, `\"` is a quote, `""` is a quote, and
//! every other byte — including a backslash before anything else — is literal;
//! the closing quote ends the *section*, not the argument; and a quoted section
//! is exempt from the glob and tilde pass. Note where this differs from the
//! MSVC convention, which halves a run of backslashes only when a quote
//! follows: the encoding `a\\b` means two backslashes to the MSVC runtime and
//! one to Cygwin's. That disagreement is small, but it is enough that no single
//! encoder can serve both — hence the per-callee choice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// One argument, spelled so that a Cygwin/MSYS child reads back exactly these
/// bytes.
///
/// Always quoted, even when nothing inside needs an escape: the quotes are what
/// exempt the argument from the child's glob and tilde expansion, so a bare `*`
/// or `~` would otherwise arrive as something else entirely. An empty argument
/// needs them for a second reason — without them it would not arrive at all.
#[must_use]
pub fn quote_arg(arg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(arg.len().saturating_add(2));
    out.push(b'"');
    for &b in arg {
        // The only two bytes the quoted section reads as anything but
        // themselves; escaping just these leaves `\n`, `$`, backtick and the
        // rest to pass through untouched, which is what the child expects.
        if b == b'\\' || b == b'"' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b'"');
    out
}

/// Does the command about to be spawned link the Cygwin or MSYS2 runtime?
///
/// Answered from the executable's PE import table, because it is the runtime —
/// not the program — that re-parses the command line. `resolve` turns the
/// command word into a file to read, and is called only on a cache miss: the
/// word reaching here is often a bare name, whose `$PATH` search costs a stat
/// per directory and would otherwise be paid on every single spawn.
///
/// `key` must therefore identify the *file* the word resolves to and not just
/// the word, so the caller composes it from the command word **and the `$PATH`
/// it will be searched in**: a script that puts a different directory in front
/// must not keep the earlier answer. What that leaves uncovered is a file
/// replaced in place at the same path mid-session, which nothing short of
/// re-reading every executable on every spawn could catch, and which costs a
/// mis-encoded argument rather than a crash.
pub fn is_cygwin_program(key: &[u8], resolve: impl FnOnce() -> Option<PathBuf>) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<Vec<u8>, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    // A poisoned lock costs the cache, not the answer: fall through and probe.
    if let Ok(map) = cache.lock()
        && let Some(&known) = map.get(key)
    {
        return known;
    }
    let answer = resolve()
        .and_then(|p| read_image(&p))
        .is_some_and(|image| imports_cygwin_runtime(&image));
    if let Ok(mut map) = cache.lock() {
        map.insert(key.to_vec(), answer);
    }
    answer
}

/// An executable's bytes, up to a cap far past where any real import table
/// lives. Anything beyond it is unreadable to the walk below, which then
/// answers "not Cygwin" — the conservative direction, since that is the
/// encoding used today.
fn read_image(path: &Path) -> Option<Vec<u8>> {
    const CAP: u64 = 16 << 20;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, CAP), &mut buf).ok()?;
    Some(buf)
}

/// One PE section, reduced to what mapping a virtual address needs.
struct Section {
    /// Where the section is mapped, relative to the image base.
    va: u32,
    /// How much address space it occupies once mapped, which may exceed what is
    /// stored (a `.bss`-like tail is zero-filled at load time).
    vsize: u32,
    /// Where its stored bytes begin in the file, and how many there are.
    raw_ptr: u32,
    raw_size: u32,
}

/// Does this PE image's import table name a Unix-emulation runtime?
///
/// Split from the file reading so it can be exercised against a synthetic image
/// as well as the host's real binaries. A malformed image answers `false`
/// rather than failing: every step is a bounds-checked read that gives up on
/// the first thing that does not look like a PE file.
fn imports_cygwin_runtime(image: &[u8]) -> bool {
    walk_imports(image).unwrap_or(false)
}

fn walk_imports(image: &[u8]) -> Option<bool> {
    if image.get(..2)? != b"MZ" {
        return None;
    }
    // The DOS stub's one useful field: where the real header starts.
    let pe = usize::try_from(le32(image, 0x3C)?).ok()?;
    if image.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let coff = pe.checked_add(4)?;
    let nsections = le16(image, coff.checked_add(2)?)?;
    let opt_size = usize::from(le16(image, coff.checked_add(16)?)?);
    let opt = coff.checked_add(20)?;
    // The data directories follow the optional header's fixed fields, whose
    // size is the one thing 32- and 64-bit images disagree about.
    let fixed = match le16(image, opt)? {
        0x10b => 96,  // PE32
        0x20b => 112, // PE32+
        _ => return None,
    };
    // Directory 1 of 16 is the import table; each entry is an RVA then a size.
    let import_rva = le32(image, opt.checked_add(fixed)?.checked_add(8)?)?;
    if import_rva == 0 {
        return Some(false);
    }
    let sections = section_table(image, opt.checked_add(opt_size)?, nsections)?;
    let mut at = rva_to_offset(&sections, import_rva)?;
    // The descriptor array ends at an all-zero entry, of which the name RVA is
    // the field this walk needs anyway. The iteration bound is not a real
    // limit — no binary imports thousands of libraries — only insurance that a
    // corrupt table cannot spin.
    for _ in 0..4096u32 {
        let name_rva = le32(image, at.checked_add(12)?)?;
        if name_rva == 0 {
            return Some(false);
        }
        if let Some(off) = rva_to_offset(&sections, name_rva)
            && names_unix_runtime(image, off)
        {
            return Some(true);
        }
        at = at.checked_add(20)?;
    }
    Some(false)
}

/// The section headers, which follow the optional header back to back.
fn section_table(image: &[u8], at: usize, count: u16) -> Option<Vec<Section>> {
    let mut out = Vec::with_capacity(usize::from(count));
    for i in 0..usize::from(count) {
        let s = at.checked_add(i.checked_mul(40)?)?;
        out.push(Section {
            vsize: le32(image, s.checked_add(8)?)?,
            va: le32(image, s.checked_add(12)?)?,
            raw_size: le32(image, s.checked_add(16)?)?,
            raw_ptr: le32(image, s.checked_add(20)?)?,
        });
    }
    Some(out)
}

/// Where a mapped address lives in the file, or `None` if it is in a part of a
/// section that has no stored bytes (or in no section at all).
fn rva_to_offset(sections: &[Section], rva: u32) -> Option<usize> {
    for s in sections {
        let span = s.vsize.max(s.raw_size);
        if rva < s.va || rva >= s.va.checked_add(span)? {
            continue;
        }
        let delta = rva.checked_sub(s.va)?;
        if delta >= s.raw_size {
            return None;
        }
        return usize::try_from(s.raw_ptr.checked_add(delta)?).ok();
    }
    None
}

/// Is the NUL-terminated library name at `off` one of the two Unix-emulation
/// runtimes?
///
/// Matched by shape rather than against an exact list, because the soname
/// carries the version (`cygwin1.dll`, `msys-2.0.dll`): an exact list would
/// silently stop matching the day either is bumped, and the failure would look
/// like a shell bug rather than a stale table.
fn names_unix_runtime(image: &[u8], off: usize) -> bool {
    let Some(rest) = image.get(off..) else {
        return false;
    };
    let name = rest.split(|&b| b == 0).next().unwrap_or(rest);
    // A DLL name is short; anything longer is a misread offset, not a name.
    if name.len() > 64 {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    lower.ends_with(b".dll") && (lower.starts_with(b"cygwin") || lower.starts_with(b"msys-"))
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    let raw = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(<[u8; 2]>::try_from(raw).ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    let raw = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(<[u8; 4]>::try_from(raw).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(arg: &str) -> String {
        String::from_utf8_lossy(&quote_arg(arg.as_bytes())).into_owned()
    }

    #[test]
    fn quote_arg_escapes_only_backslash_and_quote() {
        // Nothing to escape — but still quoted, because that is what suppresses
        // the child's own glob and tilde pass.
        assert_eq!(q("plain"), "\"plain\"");
        assert_eq!(q("*"), "\"*\"");
        assert_eq!(q("~"), "\"~\"");
        assert_eq!(q(""), "\"\"");
        // The two bytes the quoted section reads as something else.
        assert_eq!(q("a\\b"), "\"a\\\\b\"");
        assert_eq!(q("q\"r"), "\"q\\\"r\"");
        assert_eq!(q("a\\"), "\"a\\\\\"");
        assert_eq!(q("\\1"), "\"\\\\1\"");
        // Everything else passes through, notably the characters a Windows
        // command processor would act on — the child parses the line itself,
        // and `CreateProcess` does not read it.
        assert_eq!(q("a;b|c&d"), "\"a;b|c&d\"");
        assert_eq!(q("$v `x` 'q'"), "\"$v `x` 'q'\"");
        assert_eq!(q("x y"), "\"x y\"");
    }

    #[test]
    fn quote_arg_keeps_arbitrary_bytes() {
        let arg = [0x01, 0xff, b'\n', b'\t', 0x80];
        let out = quote_arg(&arg);
        assert_eq!(out.first(), Some(&b'"'));
        assert_eq!(out.last(), Some(&b'"'));
        assert_eq!(out.get(1..out.len().saturating_sub(1)), Some(&arg[..]));
    }

    /// A minimal PE32+ image whose import table names one library.
    fn image_importing(lib: &[u8]) -> Vec<u8> {
        // Layout: headers in the first 0x200 bytes, one section mapped at RVA
        // 0x1000 whose stored bytes start at file offset 0x200 and hold the
        // descriptor array followed by the name.
        let mut img = vec![0u8; 0x400];
        img[..2].copy_from_slice(b"MZ");
        let pe = 0x80usize;
        img[0x3C..0x40].copy_from_slice(&u32::try_from(pe).unwrap().to_le_bytes());
        img[pe..pe + 4].copy_from_slice(b"PE\0\0");
        let coff = pe + 4;
        img[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes()); // one section
        img[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes()); // optional header size
        let opt = coff + 20;
        img[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes()); // PE32+
        let dirs = opt + 112;
        img[dirs + 8..dirs + 12].copy_from_slice(&0x1000u32.to_le_bytes()); // import RVA
        let sec = opt + 240;
        img[sec + 8..sec + 12].copy_from_slice(&0x200u32.to_le_bytes()); // virtual size
        img[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // virtual address
        img[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes()); // raw size
        img[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes()); // raw pointer
        // One descriptor naming a library at RVA 0x1100, then the terminator.
        img[0x200 + 12..0x200 + 16].copy_from_slice(&0x1100u32.to_le_bytes());
        img[0x300..0x300 + lib.len()].copy_from_slice(lib);
        img
    }

    #[test]
    fn import_walk_recognises_the_unix_runtimes() {
        assert!(imports_cygwin_runtime(&image_importing(b"cygwin1.dll")));
        assert!(imports_cygwin_runtime(&image_importing(b"msys-2.0.dll")));
        // Case is not significant in an import name.
        assert!(imports_cygwin_runtime(&image_importing(b"CYGWIN1.DLL")));
        // A version bump must keep matching; an unrelated library must not.
        assert!(imports_cygwin_runtime(&image_importing(b"cygwin2.dll")));
        assert!(!imports_cygwin_runtime(&image_importing(b"KERNEL32.dll")));
        assert!(!imports_cygwin_runtime(&image_importing(b"msvcrt.dll")));
        // Not a DLL name at all, however suggestive.
        assert!(!imports_cygwin_runtime(&image_importing(b"cygwin")));
    }

    #[test]
    fn import_walk_gives_up_on_anything_that_is_not_a_pe_file() {
        assert!(!imports_cygwin_runtime(b""));
        assert!(!imports_cygwin_runtime(b"#!/bin/sh\necho hi\n"));
        assert!(!imports_cygwin_runtime(b"MZ"));
        // Truncation at every length must be answered, never panicked on.
        let full = image_importing(b"cygwin1.dll");
        for cut in 0..full.len() {
            let _ = imports_cygwin_runtime(full.get(..cut).unwrap_or(&full));
        }
        // A PE header pointing outside the file.
        let mut bad = full.clone();
        bad[0x3C..0x40].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        assert!(!imports_cygwin_runtime(&bad));
    }
}
