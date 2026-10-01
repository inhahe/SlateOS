//! libmount's `optstr.c`: comma-separated mount option strings
//! (`rw,noatime,data=ordered`), read and rewritten as upstream does it.
//!
//! The scanner is `ul_optstr_next` (the `ulstrutils` port), so quotes keep
//! a comma inside a value and a backslash-comma is not a separator.

use crate::optmap::{self, MNT_INVERT, Map};
use ulstrutils::{OptstrItem, ul_optstr_next};

/// `-EINVAL` from the scanner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invalid;

/// Where an option was found: `struct libmnt_optloc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptLoc {
    /// The option's first byte.
    pub begin: usize,
    /// One past its last byte (before its comma).
    pub end: usize,
    /// Its value, if it has `=`: where it starts and its length.
    pub value: Option<(usize, usize)>,
    pub namesz: usize,
}

/// The items of `s`, in order.
fn items(s: &[u8]) -> impl Iterator<Item = Result<(OptstrItem, usize), Invalid>> + '_ {
    let mut pos = 0usize;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        match ul_optstr_next(s, &mut pos) {
            Ok(Some(item)) => Some(Ok((item, pos))),
            Ok(None) => {
                done = true;
                None
            }
            Err(_) => {
                done = true;
                Some(Err(Invalid))
            }
        }
    })
}

/// `mnt_optstr_locate_option(optstr, name, &ol)`: the first option called
/// exactly `name`.
///
/// # Errors
///
/// The scanner's refusal.
pub fn locate_option(optstr: &[u8], name: &[u8]) -> Result<Option<OptLoc>, Invalid> {
    if name.is_empty() {
        return Ok(None);
    }
    for item in items(optstr) {
        let (it, pos) = item?;
        if it.namesz == name.len()
            && optstr.get(it.name..it.name.saturating_add(it.namesz)) == Some(name)
        {
            let end = if pos > 0 && optstr.get(pos.saturating_sub(1)) == Some(&b',') {
                pos.saturating_sub(1)
            } else {
                pos
            };
            return Ok(Some(OptLoc {
                begin: it.name,
                end,
                value: it.value.map(|v| (v, it.valsz)),
                namesz: it.namesz,
            }));
        }
    }
    Ok(None)
}

/// `mnt_optstr_get_option(optstr, name, &value, &valsz)`: whether the
/// option is there, and its value's bytes if it has one.
///
/// # Errors
///
/// The scanner's refusal.
pub fn get_option<'a>(optstr: &'a [u8], name: &[u8]) -> Result<Option<Option<&'a [u8]>>, Invalid> {
    Ok(locate_option(optstr, name)?.map(|ol| {
        ol.value
            .map(|(v, sz)| optstr.get(v..v.saturating_add(sz)).unwrap_or_default())
    }))
}

/// `mnt_optstr_remove_option_at(optstr, begin, end)`: the bytes from
/// `begin` to `end` out, with the comma after them when they started the
/// string or followed one, and a comma left trailing dropped.
pub fn remove_option_at(optstr: &mut Vec<u8>, begin: usize, end: usize) {
    let mut end = end;
    if (begin == 0 || optstr.get(begin.saturating_sub(1)) == Some(&b','))
        && optstr.get(end) == Some(&b',')
    {
        end = end.saturating_add(1);
    }
    let tail = optstr.get(end..).unwrap_or_default().to_vec();
    optstr.truncate(begin);
    optstr.extend_from_slice(&tail);
    if optstr.len() == begin && begin > 0 && optstr.get(begin.saturating_sub(1)) == Some(&b',') {
        optstr.truncate(begin.saturating_sub(1));
    }
}

/// `mnt_optstr_remove_option(&optstr, name)`: whether it was there.
///
/// # Errors
///
/// The scanner's refusal.
pub fn remove_option(optstr: &mut Vec<u8>, name: &[u8]) -> Result<bool, Invalid> {
    match locate_option(optstr, name)? {
        Some(ol) => {
            remove_option_at(optstr, ol.begin, ol.end);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// `mnt_buffer_append_option(buf, name, namesz, val, valsz, 0)`: a comma
/// if the buffer holds anything, the name, and `=value` if there is a value
/// -- `=` alone for an empty one.
pub fn buffer_append_option(buf: &mut Vec<u8>, name: &[u8], value: Option<&[u8]>) {
    if !buf.is_empty() {
        buf.push(b',');
    }
    buf.extend_from_slice(name);
    if let Some(v) = value {
        buf.push(b'=');
        buf.extend_from_slice(v);
    }
}

/// `mnt_optstr_append_option(&optstr, name, value)`: nothing for an empty
/// name.
pub fn append_option(optstr: &mut Option<Vec<u8>>, name: &[u8], value: Option<&[u8]>) {
    if name.is_empty() {
        return;
    }
    let buf = optstr.get_or_insert_with(Vec::new);
    buffer_append_option(buf, name, value);
}

/// `mnt_optstr_prepend_option(&optstr, name, value)`: the option, then a
/// comma and the old string -- when there was one at all, empty or not.
pub fn prepend_option(optstr: &mut Option<Vec<u8>>, name: &[u8], value: Option<&[u8]>) {
    if name.is_empty() {
        return;
    }
    let mut buf = Vec::new();
    buffer_append_option(&mut buf, name, value);
    if let Some(old) = optstr.take() {
        buf.push(b',');
        buf.extend_from_slice(&old);
    }
    *optstr = Some(buf);
}

/// What [`split_optstr`] returns: `(user, vfs, fs)`, each `None` when
/// empty, as `ul_buffer_get_data` of an empty buffer is `NULL`.
pub type Split = (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);

/// `mnt_split_optstr(optstr, &user, &vfs, &fs, ignore_user, ignore_vfs)`:
/// the options the kernel's flags name (VFS), the ones only userspace
/// knows (`noauto`, `user`, `x-...`), and the rest (the filesystem's own).
/// `defaults` goes nowhere, and a name the maps know as flag-only but that
/// carries a value is the filesystem's.
///
/// # Errors
///
/// The scanner's refusal.
pub fn split_optstr(optstr: &[u8], ignore_user: u32, ignore_vfs: u32) -> Result<Split, Invalid> {
    let (mut user, mut vfs, mut fs) = (Vec::new(), Vec::new(), Vec::new());
    for item in items(optstr) {
        let (it, _) = item?;
        let rest = optstr.get(it.name..).unwrap_or_default();
        let found = optmap::get_entry(&[Map::Linux, Map::Userspace], rest, it.namesz);
        if let Some((_, ent)) = found
            && ent.id == 0
        {
            continue;
        }
        let mut map = found.map(|(m, _)| m);
        if it.value.is_some_and(|_| it.valsz > 0)
            && found.is_some_and(|(_, ent)| optmap::entry_novalue(ent))
        {
            map = None;
        }
        let buf = match (found, map) {
            (Some((_, ent)), Some(Map::Linux)) => {
                if ignore_vfs != 0 && ent.mask & ignore_vfs != 0 {
                    continue;
                }
                &mut vfs
            }
            (Some((_, ent)), Some(Map::Userspace)) => {
                if ignore_user != 0 && ent.mask & ignore_user != 0 {
                    continue;
                }
                &mut user
            }
            (_, None) => &mut fs,
            _ => continue,
        };
        let name = optstr
            .get(it.name..it.name.saturating_add(it.namesz))
            .unwrap_or_default();
        let value = it.value.map(|v| {
            optstr
                .get(v..v.saturating_add(it.valsz))
                .unwrap_or_default()
        });
        buffer_append_option(buf, name, value);
    }
    let some = |v: Vec<u8>| (!v.is_empty()).then_some(v);
    Ok((some(user), some(vfs), some(fs)))
}

/// `mnt_optstr_get_flags(optstr, &flags, map)`: the flags the options set,
/// starting from `flags`. For the kernel's map, `user`/`users` (without a
/// value) add `MS_SECURE` and `owner`/`group` `MS_OWNERSECURE`, as
/// `mount(8)` treats them.
///
/// # Errors
///
/// None the scanner gives here: upstream ignores its refusal and stops.
pub fn get_flags(optstr: &[u8], map: Map, flags: u64) -> u64 {
    let maps: &[Map] = if map == Map::Linux {
        &[Map::Linux, Map::Userspace]
    } else {
        &[Map::Userspace]
    };
    let mut flags = flags;
    for item in items(optstr) {
        let Ok((it, _)) = item else {
            break;
        };
        let rest = optstr.get(it.name..).unwrap_or_default();
        let Some((m, ent)) = optmap::get_entry(maps, rest, it.namesz) else {
            continue;
        };
        if ent.id == 0 {
            continue;
        }
        let valsz = if it.value.is_some() { it.valsz } else { 0 };
        if valsz > 0 && optmap::entry_novalue(ent) {
            continue;
        }
        if m == map {
            if ent.mask & MNT_INVERT != 0 {
                flags &= !ent.id;
            } else {
                flags |= ent.id;
            }
        } else if maps.len() == 2 && m == Map::Userspace && valsz == 0 {
            if ent.mask & MNT_INVERT != 0 {
                continue;
            }
            if ent.id & (optmap::MNT_MS_OWNER | optmap::MNT_MS_GROUP) != 0 {
                flags |= optmap::MS_OWNERSECURE;
            } else if ent.id & (optmap::MNT_MS_USER | optmap::MNT_MS_USERS) != 0 {
                flags |= optmap::MS_SECURE;
            }
        }
    }
    flags
}

/// `mnt_match_options(optstr, pattern)`: every option of `pattern` is in
/// `optstr` -- or, written `noNAME`, is not; `+NAME` is `NAME`, and
/// `NAME=value` wants that value too.
#[must_use]
pub fn match_options(optstr: Option<&[u8]>, pattern: Option<&[u8]>) -> bool {
    match (optstr, pattern) {
        (None, None) => return true,
        (Some(o), Some(p)) if o.is_empty() && p.is_empty() => return true,
        (_, None) => return false,
        _ => {}
    }
    let Some(pattern) = pattern else {
        return false;
    };
    let mut matched = true;
    for item in items(pattern) {
        if !matched {
            break;
        }
        let Ok((it, _)) = item else {
            break;
        };
        let mut name_at = it.name;
        let mut namesz = it.namesz;
        let mut no = false;
        if pattern.get(name_at) == Some(&b'+') {
            name_at = name_at.saturating_add(1);
            namesz = namesz.saturating_sub(1);
        } else if pattern.get(name_at..).is_some_and(|r| r.starts_with(b"no")) {
            no = true;
            name_at = name_at.saturating_add(2);
            namesz = namesz.saturating_sub(2);
            if matches!(pattern.get(name_at), None | Some(&b',')) {
                // "no" alone is an error.
                matched = false;
                break;
            }
        }
        let name_first = pattern.get(name_at).copied().unwrap_or(0);
        let rc: i32 = match optstr {
            Some(o) if !o.is_empty() && name_first != 0 => {
                let name = pattern
                    .get(name_at..name_at.saturating_add(namesz))
                    .unwrap_or_default();
                match get_option(o, crate::c_str(name)) {
                    Ok(Some(val)) => {
                        let patval = it.value.map(|v| {
                            pattern
                                .get(v..v.saturating_add(it.valsz))
                                .unwrap_or_default()
                        });
                        match patval {
                            Some(pv) if !pv.is_empty() && val.unwrap_or_default() != pv => 1,
                            _ => 0,
                        }
                    }
                    Ok(None) => 1,
                    Err(_) => -1,
                }
            }
            _ if name_first == 0 => 0,
            _ => 1,
        };
        matched = match rc {
            0 => !no,
            1 => no,
            _ => false,
        };
    }
    matched
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;

    #[test]
    fn options_are_found_and_removed() {
        assert_eq!(
            get_option(b"rw,data=ordered,x", b"data"),
            Ok(Some(Some(&b"ordered"[..])))
        );
        assert_eq!(get_option(b"rw,x", b"x"), Ok(Some(None)));
        assert_eq!(get_option(b"rw,x", b"y"), Ok(None));
        let mut s = b"rw,noatime,x".to_vec();
        assert_eq!(remove_option(&mut s, b"rw"), Ok(true));
        assert_eq!(s, b"noatime,x".to_vec());
        let mut s = b"rw,noatime,x".to_vec();
        assert_eq!(remove_option(&mut s, b"x"), Ok(true));
        assert_eq!(s, b"rw,noatime".to_vec());
        let mut s = b"rw,noatime,x".to_vec();
        assert_eq!(remove_option(&mut s, b"noatime"), Ok(true));
        assert_eq!(s, b"rw,x".to_vec());
        let mut s = b"rw".to_vec();
        assert_eq!(remove_option(&mut s, b"rw"), Ok(true));
        assert_eq!(s, Vec::<u8>::new());
    }

    #[test]
    fn options_are_appended_and_prepended() {
        let mut o = None;
        append_option(&mut o, b"rw", None);
        append_option(&mut o, b"data", Some(b"x"));
        append_option(&mut o, b"e", Some(b""));
        assert_eq!(o.as_deref(), Some(&b"rw,data=x,e="[..]));
        let mut o = Some(Vec::new());
        prepend_option(&mut o, b"ro", None);
        assert_eq!(o.as_deref(), Some(&b"ro,"[..]));
    }

    #[test]
    fn fstab_options_split_as_upstream() {
        let (u, v, f) = split_optstr(
            b"defaults,noatime,user,x-systemd.automount,data=ordered,ro=1",
            0,
            0,
        )
        .unwrap_or((None, None, None));
        assert_eq!(u.as_deref(), Some(&b"user,x-systemd.automount"[..]));
        assert_eq!(v.as_deref(), Some(&b"noatime"[..]));
        assert_eq!(f.as_deref(), Some(&b"data=ordered,ro=1"[..]));
    }

    #[test]
    fn flags_as_upstream() {
        assert_eq!(
            get_flags(b"ro,noexec,user", Map::Linux, 0),
            optmap::MS_RDONLY | optmap::MS_NOEXEC | optmap::MS_SECURE
        );
        assert_eq!(get_flags(b"rw", Map::Linux, optmap::MS_RDONLY), 0);
        assert_eq!(get_flags(b"user=joe", Map::Linux, 0), 0);
    }

    #[test]
    fn patterns_match_as_upstream() {
        let m = |o: &str, p: &str| match_options(Some(o.as_bytes()), Some(p.as_bytes()));
        assert!(m("rw,noatime", "noatime"));
        assert!(m("rw,noatime", "+rw"));
        assert!(!m("rw,noatime", "ro"));
        assert!(m("rw,noatime", "noro"));
        assert!(!m("rw,noatime", "norw"));
        assert!(m("rw,data=ordered", "data=ordered"));
        assert!(!m("rw,data=ordered", "data=journal"));
        assert!(m("rw,data=ordered", "data"));
        assert!(!m("rw", "no"));
        assert!(m("", ""));
        assert!(match_options(None, None));
        assert!(!match_options(Some(b"rw"), None));
        assert!(!match_options(None, Some(b"rw")));
        assert!(match_options(None, Some(b"noro")));
    }
}
