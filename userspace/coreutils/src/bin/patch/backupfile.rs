//! gnulib's `backupfile.c` and `backup-find.c`, as patch 2.7.6 bundles them
//! (2018): a backup's name -- simple (`FILE.orig`, or `$SIMPLE_BACKUP_SUFFIX`)
//! or numbered (`FILE.~N~`, one more than the highest there is) -- and the
//! `-V` words. `patch` sets `simple_backup_suffix` itself, so the rule that a
//! suffix may hold no slash, which `set_simple_backup_suffix` applies, does
//! not apply here.

use crate::util::{base_len, last_component};

/// `enum backup_type`, under gnulib's names.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupType {
    NoBackups,
    SimpleBackups,
    NumberedExistingBackups,
    NumberedBackups,
}

/// `backup_args` and `backup_types`, in order.
const ARGS: [(&str, BackupType); 8] = [
    ("none", BackupType::NoBackups),
    ("off", BackupType::NoBackups),
    ("simple", BackupType::SimpleBackups),
    ("never", BackupType::SimpleBackups),
    ("existing", BackupType::NumberedExistingBackups),
    ("nil", BackupType::NumberedExistingBackups),
    ("numbered", BackupType::NumberedBackups),
    ("t", BackupType::NumberedBackups),
];

/// `get_version`: the type `version` names (an unambiguous prefix will do),
/// `numbered_existing_backups` for none; `Err (ambiguous)` when it names
/// none.
pub fn get_version(version: Option<&[u8]>) -> Result<BackupType, bool> {
    let Some(v) = version.filter(|v| !v.is_empty()) else {
        return Ok(BackupType::NumberedExistingBackups);
    };
    let mut found: Option<BackupType> = None;
    let mut ambiguous = false;
    for (name, t) in ARGS {
        if name.as_bytes() == v {
            return Ok(t);
        }
        if name.as_bytes().starts_with(v) {
            match found {
                None => found = Some(t),
                Some(f) if f != t => ambiguous = true,
                Some(_) => {}
            }
        }
    }
    match found {
        Some(t) if !ambiguous => Ok(t),
        _ => Err(ambiguous),
    }
}

/// gnulib's `quote` in the C locale (`locale_quoting_style`, under no
/// `setlocale`): single quotes, C escapes inside.
pub fn quote_c_locale(s: &[u8]) -> Vec<u8> {
    quoting::Style::Locale.quote_in(s, quoting::Charset::Ascii)
}

/// `XARGMATCH`'s failure for `-V`: `argmatch_invalid`, then
/// `argmatch_valid`, then `exit (exit_failure)` -- which the caller does.
pub fn complain(program_name: &[u8], context: &str, value: &[u8], ambiguous: bool) {
    let mut m = program_name.to_vec();
    m.extend_from_slice(if ambiguous {
        b": ambiguous argument "
    } else {
        b": invalid argument "
    });
    m.extend_from_slice(&quote_c_locale(value));
    m.extend_from_slice(b" for ");
    m.extend_from_slice(&quote_c_locale(context.as_bytes()));
    m.extend_from_slice(b"\nValid arguments are:");
    let mut last: Option<BackupType> = None;
    for (name, t) in ARGS {
        if last == Some(t) {
            m.extend_from_slice(b", ");
        } else {
            m.extend_from_slice(b"\n  - ");
        }
        m.extend_from_slice(&quote_c_locale(name.as_bytes()));
        last = Some(t);
    }
    m.push(b'\n');
    crate::util::print_stderr(&m);
}

/// `numbered_backup`: `file.~N~` with N one more than the highest numbered
/// backup of `file` there is -- `None` when there is none (`BACKUP_IS_NEW`)
/// -- and whether the number grew a digit (`BACKUP_IS_LONGER`).
fn numbered_backup(file: &[u8], base_offset: usize) -> Option<(Vec<u8>, bool)> {
    let base = file.get(base_offset..).unwrap_or_default();
    let baselen = base_len(base);
    let mut dir = file.get(..base_offset).unwrap_or_default().to_vec();
    dir.push(b'.');
    let entries = std::fs::read_dir(quoting::os_from_bytes(&dir)).ok()?;

    // The highest version seen so far, plus one, as digits.
    let mut best: Option<Vec<u8>> = None;
    let mut longer = false;
    let mut prefix = base.get(..baselen).unwrap_or_default().to_vec();
    prefix.extend_from_slice(b".~");
    for entry in entries.flatten() {
        let name = quoting::os_bytes(&entry.file_name()).into_owned();
        if name.len() < baselen.saturating_add(4) || !name.starts_with(&prefix) {
            continue;
        }
        let p = name.get(baselen.saturating_add(2)..).unwrap_or_default();
        if !p.first().is_some_and(|c| (b'1'..=b'9').contains(c)) {
            continue;
        }
        let versionlen = p.iter().take_while(|c| c.is_ascii_digit()).count();
        if p.get(versionlen) != Some(&b'~') || p.len() != versionlen.saturating_add(1) {
            continue;
        }
        let digits = p.get(..versionlen).unwrap_or_default();
        let bigger = match &best {
            None => true,
            Some(b) => {
                // The candidate is compared with the incremented best, as
                // gnulib compares with the buffer it already bumped; an
                // equal one is taken again, and bumps to the same.
                let cur_len = b.len();
                versionlen > cur_len || (versionlen == cur_len && b.as_slice() <= digits)
            }
        };
        if !bigger {
            continue;
        }
        let all_9s = digits.iter().all(|&c| c == b'9');
        let mut next: Vec<u8> = Vec::with_capacity(versionlen.saturating_add(1));
        if all_9s {
            next.push(b'0');
        }
        next.extend_from_slice(digits);
        // Add 1 to the version number.
        let mut i = next.len();
        while i > 0 {
            i = i.saturating_sub(1);
            if let Some(d) = next.get_mut(i) {
                if *d == b'9' {
                    *d = b'0';
                } else {
                    *d = d.saturating_add(1);
                    break;
                }
            }
        }
        longer = all_9s;
        best = Some(next);
    }
    let digits = best?;
    let mut out = file.to_vec();
    out.extend_from_slice(b".~");
    out.extend_from_slice(&digits);
    out.push(b'~');
    Some((out, longer))
}

/// `check_extension`: a backup name whose base is longer than the
/// directory allows has its extension replaced with `e`, and is cut short
/// if it is still too long.
fn check_extension(name: &mut Vec<u8>, filelen: usize, e: u8) {
    let base = last_component(name);
    let baselen = base_len(name.get(base..).unwrap_or_default());
    if baselen <= 255 {
        return;
    }
    let mut dir = name.get(..base).unwrap_or_default().to_vec();
    dir.push(b'.');
    let name_max = path_name_max(&dir).unwrap_or(255);
    if name_max < baselen {
        let mut keep = filelen.saturating_sub(base);
        if name_max <= keep {
            keep = name_max.saturating_sub(1);
        }
        name.truncate(base.saturating_add(keep));
        name.push(e);
    }
}

/// `pathconf (dir, _PC_NAME_MAX)`.
fn path_name_max(dir: &[u8]) -> Option<usize> {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn pathconf(path: *const std::ffi::c_char, name: i32) -> i64;
        }
        /// `_PC_NAME_MAX`.
        const PC_NAME_MAX: i32 = 3;
        let c = std::ffi::CString::new(dir).ok()?;
        // SAFETY: a live NUL-terminated string, read by the call.
        let r = unsafe { pathconf(c.as_ptr(), PC_NAME_MAX) };
        usize::try_from(r).ok()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

/// `find_backup_file_name`.
pub fn find_backup_file_name(
    file: &[u8],
    backup_type: BackupType,
    simple_suffix: &[u8],
) -> Vec<u8> {
    let base_offset = last_component(file);
    let filelen = file.len();
    let simple = || {
        let mut s = file.to_vec();
        s.extend_from_slice(simple_suffix);
        s
    };
    if backup_type == BackupType::SimpleBackups {
        return simple();
    }
    match numbered_backup(file, base_offset) {
        Some((mut s, longer)) => {
            if longer {
                check_extension(&mut s, filelen, b'~');
            }
            s
        }
        None => {
            let mut s = if backup_type == BackupType::NumberedExistingBackups {
                simple()
            } else {
                let mut s = file.to_vec();
                s.extend_from_slice(b".~1~");
                s
            };
            check_extension(&mut s, filelen, b'~');
            s
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn version_words_resolve_as_argmatch_does() {
        assert_eq!(
            get_version(None).unwrap(),
            BackupType::NumberedExistingBackups
        );
        assert_eq!(
            get_version(Some(b"")).unwrap(),
            BackupType::NumberedExistingBackups
        );
        assert_eq!(
            get_version(Some(b"t")).unwrap(),
            BackupType::NumberedBackups
        );
        assert_eq!(
            get_version(Some(b"nev")).unwrap(),
            BackupType::SimpleBackups
        );
        assert_eq!(get_version(Some(b"n")), Err(true));
        assert_eq!(get_version(Some(b"x")), Err(false));
    }

    #[test]
    fn values_are_quoted_as_the_c_locale_quotes_them() {
        assert_eq!(quote_c_locale(b"abc"), b"'abc'");
        assert_eq!(quote_c_locale(b"a\nb"), b"'a\\nb'");
        assert_eq!(quote_c_locale(b"\xe9"), b"'\\351'");
    }
}
