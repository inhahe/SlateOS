//! `parttype.c`: partition types, and how a user's text becomes one.
//!
//! A type is a code (MBR's `0x83`) or a string (GPT's GUID), with a name.
//! Each label has a static table of the types it knows; a type that is not
//! in it is "unknown", made up on the spot from the code or string given.
//! Upstream reference-counts the made-up ones and not the static ones; here
//! every type is a value, cloned where C takes a reference.

use std::borrow::Cow;

use crate::label::Label;
use crate::utils::{stralnumcmp, strcasecmp, strtol, trunc_i32, trunc_u32};

/// `FDISK_PARTTYPE_UNKNOWN`: not in the label's table.
pub const PARTTYPE_UNKNOWN: u32 = 1 << 1;
/// `FDISK_PARTTYPE_INVISIBLE`: not to be offered in a list.
pub const PARTTYPE_INVISIBLE: u32 = 1 << 2;
/// `FDISK_PARTTYPE_ALLOCATED`: made at run time rather than static.
pub const PARTTYPE_ALLOCATED: u32 = 1 << 3;

/// `FDISK_PARTTYPE_PARSE_DATA`: hex code or GUID.
pub const PARSE_DATA: i32 = 1 << 1;
/// `FDISK_PARTTYPE_PARSE_DATALAST`: try the data last.
pub const PARSE_DATALAST: i32 = 1 << 2;
/// `FDISK_PARTTYPE_PARSE_SHORTCUT`: `L` for Linux.
pub const PARSE_SHORTCUT: i32 = 1 << 3;
/// `FDISK_PARTTYPE_PARSE_ALIAS`: `linux` for Linux.
pub const PARSE_ALIAS: i32 = 1 << 4;
/// `FDISK_PARTTYPE_PARSE_DEPRECATED`: accept deprecated shortcuts and aliases.
pub const PARSE_DEPRECATED: i32 = 1 << 5;
/// `FDISK_PARTTYPE_PARSE_NOUNKNOWN`: no made-up type for what is not known.
pub const PARSE_NOUNKNOWN: i32 = 1 << 6;
/// `FDISK_PARTTYPE_PARSE_SEQNUM`: the type's position in the list.
pub const PARSE_SEQNUM: i32 = 1 << 7;
/// `FDISK_PARTTYPE_PARSE_NAME`: the type's human-readable name.
pub const PARSE_NAME: i32 = 1 << 8;
/// `FDISK_PARTTYPE_PARSE_DEFAULT`.
pub const PARSE_DEFAULT: i32 =
    PARSE_DATA | PARSE_SHORTCUT | PARSE_ALIAS | PARSE_NAME | PARSE_SEQNUM;

/// `struct fdisk_parttype`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartType {
    /// The type as a number (MBR), or 0.
    pub code: u32,
    /// The human-readable name.
    pub name: Option<Cow<'static, [u8]>>,
    /// The type as a string (GPT's GUID), or `None`.
    pub typestr: Option<Cow<'static, [u8]>>,
    /// `FDISK_PARTTYPE_*`.
    pub flags: u32,
}

impl PartType {
    /// A static code type: `DEF_PARTTYPE(code, name)`.
    #[must_use]
    pub const fn code(code: u32, name: &'static [u8]) -> Self {
        Self {
            code,
            name: Some(Cow::Borrowed(name)),
            typestr: None,
            flags: 0,
        }
    }

    /// A static string type: `DEF_GUID(guid, name)`.
    #[must_use]
    pub const fn guid(guid: &'static [u8], name: &'static [u8]) -> Self {
        Self {
            code: 0,
            name: Some(Cow::Borrowed(name)),
            typestr: Some(Cow::Borrowed(guid)),
            flags: 0,
        }
    }

    /// `fdisk_new_parttype ()`: an empty run-time type.
    #[must_use]
    pub fn new() -> Self {
        Self {
            code: 0,
            name: None,
            typestr: None,
            flags: PARTTYPE_ALLOCATED,
        }
    }

    /// `fdisk_new_unknown_parttype (code, typestr)`: a type named
    /// "unknown".
    #[must_use]
    pub fn new_unknown(code: u32, typestr: Option<&[u8]>) -> Self {
        let mut t = Self::new();
        t.name = Some(Cow::Borrowed(b"unknown"));
        t.code = code;
        t.typestr = typestr.map(|s| Cow::Owned(s.to_vec()));
        t.flags |= PARTTYPE_UNKNOWN;
        t
    }

    /// `fdisk_copy_parttype (type)`: a run-time copy.
    #[must_use]
    pub fn copy(&self) -> Self {
        let mut t = Self::new();
        t.name.clone_from(&self.name);
        t.code = self.code;
        t.typestr.clone_from(&self.typestr);
        t
    }

    /// `fdisk_parttype_set_name`: -EINVAL for a static type.
    pub fn set_name(&mut self, s: Option<&[u8]>) -> i32 {
        if self.flags & PARTTYPE_ALLOCATED == 0 {
            return -crate::errno::EINVAL;
        }
        self.name = s.map(|s| Cow::Owned(s.to_vec()));
        0
    }

    /// `fdisk_parttype_set_typestr`: -EINVAL for a static type.
    pub fn set_typestr(&mut self, s: Option<&[u8]>) -> i32 {
        if self.flags & PARTTYPE_ALLOCATED == 0 {
            return -crate::errno::EINVAL;
        }
        self.typestr = s.map(|s| Cow::Owned(s.to_vec()));
        0
    }

    /// `fdisk_parttype_set_code`: -EINVAL for a static type.
    pub fn set_code(&mut self, code: u32) -> i32 {
        if self.flags & PARTTYPE_ALLOCATED == 0 {
            return -crate::errno::EINVAL;
        }
        self.code = code;
        0
    }

    /// `fdisk_parttype_get_string`: the type string, unless it is empty.
    #[must_use]
    pub fn get_string(&self) -> Option<&[u8]> {
        self.typestr.as_deref().filter(|s| !s.is_empty())
    }

    /// `fdisk_parttype_get_code`.
    #[must_use]
    pub fn get_code(&self) -> u32 {
        self.code
    }

    /// `fdisk_parttype_get_name`.
    #[must_use]
    pub fn get_name(&self) -> Option<&[u8]> {
        self.name.as_deref()
    }

    /// `fdisk_parttype_is_unknown`.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.flags & PARTTYPE_UNKNOWN != 0
    }

    /// `fdisk_parttype_is_invisible`.
    #[must_use]
    pub fn is_invisible(&self) -> bool {
        self.flags & PARTTYPE_INVISIBLE != 0
    }
}

impl Default for PartType {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct fdisk_shortcut`: a short name for a type, as the user may type
/// it.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    /// Usually one letter: `L`.
    pub shortcut: Option<&'static str>,
    /// A word: `linux`.
    pub alias: Option<&'static str>,
    /// The type it stands for, as type data: `83`, or a GUID.
    pub data: &'static str,
    /// Accepted only with [`PARSE_DEPRECATED`].
    pub deprecated: bool,
}

/// A shortcut as [`Label::get_parttype_shortcut`] gives it: the type data,
/// the shortcut, the alias, and 2 when deprecated (else 0).
pub type ShortcutInfo = (
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    i32,
);

impl Label {
    /// `fdisk_label_get_nparttypes`.
    #[must_use]
    pub fn get_nparttypes(&self) -> usize {
        self.parttypes.len()
    }

    /// `fdisk_label_get_parttype (lb, n)`.
    #[must_use]
    pub fn get_parttype(&self, n: usize) -> Option<&PartType> {
        self.parttypes.get(n)
    }

    /// `fdisk_label_get_parttype_shortcut (lb, n, ...)`: the `n`th
    /// shortcut as (type string, shortcut, alias), and 0, or 2 for a
    /// deprecated one; `Err(1)` past the end.
    pub fn get_parttype_shortcut(&self, n: usize) -> Result<ShortcutInfo, i32> {
        let sc = self.parttype_cuts.get(n).ok_or(1)?;
        Ok((
            sc.data,
            sc.shortcut,
            sc.alias,
            if sc.deprecated { 2 } else { 0 },
        ))
    }

    /// `fdisk_label_has_code_parttypes`: whether types are codes (MBR)
    /// rather than strings.
    #[must_use]
    pub fn has_code_parttypes(&self) -> bool {
        self.parttypes.first().is_none_or(|t| t.typestr.is_none())
    }

    /// `fdisk_label_has_parttypes_shortcuts`.
    #[must_use]
    pub fn has_parttypes_shortcuts(&self) -> bool {
        !self.parttype_cuts.is_empty()
    }

    /// `fdisk_label_get_parttype_from_code`: the label's type with `code`.
    #[must_use]
    pub fn get_parttype_from_code(&self, code: u32) -> Option<&PartType> {
        self.parttypes.iter().find(|t| t.code == code)
    }

    /// `fdisk_label_get_parttype_from_string`: the label's type whose
    /// string is `s`, in any case.
    #[must_use]
    pub fn get_parttype_from_string(&self, s: &[u8]) -> Option<&PartType> {
        self.parttypes.iter().find(|t| {
            t.typestr
                .as_deref()
                .is_some_and(|ts| strcasecmp(ts, s) == 0)
        })
    }

    /// `parttype_from_data (lb, str, &xcode, use_seqnum)`: the type and
    /// the code `str` parsed to (0 unless the label has code types).
    fn parttype_from_data(&self, s: &[u8], use_seqnum: bool) -> (Option<&PartType>, u32) {
        let Some(first) = self.parttypes.first() else {
            return (None, 0);
        };
        if first.typestr.is_none() {
            let (v, end, err) = strtol(s, 16);
            if err != 0 || end != s.len() {
                return (None, 0);
            }
            // `unsigned int code = strtol (...)`.
            let code = trunc_u32(v);
            return (self.get_parttype_from_code(code), code);
        }
        if let Some(t) = self.get_parttype_from_string(s) {
            return (Some(t), 0);
        }
        // Maybe the type's number in the list.
        let (v, end, err) = strtol(s, 0);
        let i = trunc_i32(v);
        if use_seqnum && err == 0 && end == s.len() && i > 0 {
            if let Some(i) = i.checked_sub(1).and_then(|i| usize::try_from(i).ok()) {
                return (self.parttypes.get(i), 0);
            }
        }
        (None, 0)
    }

    /// `parttype_from_shortcut (lb, str, deprecated)`.
    fn parttype_from_shortcut(&self, s: &[u8], deprecated: bool) -> Option<&PartType> {
        self.parttype_cuts
            .iter()
            .filter(|sc| deprecated || !sc.deprecated)
            .find(|sc| sc.shortcut.is_some_and(|c| c.as_bytes() == s))
            .and_then(|sc| self.parttype_from_data(sc.data.as_bytes(), false).0)
    }

    /// `parttype_from_alias (lb, str, deprecated)`.
    fn parttype_from_alias(&self, s: &[u8], deprecated: bool) -> Option<&PartType> {
        self.parttype_cuts
            .iter()
            .filter(|sc| deprecated || !sc.deprecated)
            .find(|sc| sc.alias.is_some_and(|a| a.as_bytes() == s))
            .and_then(|sc| self.parttype_from_data(sc.data.as_bytes(), false).0)
    }

    /// `parttype_from_name (lb, str)`: by name, ignoring case and
    /// everything but letters and digits.
    fn parttype_from_name(&self, s: &[u8]) -> Option<&PartType> {
        self.parttypes.iter().find(|t| {
            t.name
                .as_deref()
                .is_some_and(|n| !n.is_empty() && stralnumcmp(n, s) == 0)
        })
    }

    /// `fdisk_label_advparse_parttype (lb, str, flags)`: `str` as type
    /// data, alias, shortcut or name, as `flags` allow; an unknown type
    /// made from it when none matches, unless [`PARSE_NOUNKNOWN`].
    #[must_use]
    pub fn advparse_parttype(&self, s: &[u8], flags: i32) -> Option<PartType> {
        let first_is_string = self.parttypes.first()?.typestr.is_some();
        let mut code = 0;
        let mut res: Option<&PartType> = None;

        if flags & PARSE_DATA != 0 && flags & PARSE_DATALAST == 0 {
            let (t, c) = self.parttype_from_data(s, flags & PARSE_SEQNUM != 0);
            res = t;
            code = c;
        }
        if res.is_none() && flags & PARSE_ALIAS != 0 {
            res = self.parttype_from_alias(s, flags & PARSE_DEPRECATED != 0);
        }
        if res.is_none() && flags & PARSE_SHORTCUT != 0 {
            res = self.parttype_from_shortcut(s, flags & PARSE_DEPRECATED != 0);
        }
        if res.is_none() && flags & PARSE_NAME != 0 {
            res = self.parttype_from_name(s);
        }
        if res.is_none() && flags & PARSE_DATA != 0 && flags & PARSE_DATALAST != 0 {
            let (t, c) = self.parttype_from_data(s, flags & PARSE_SEQNUM != 0);
            res = t;
            code = c;
        }
        if let Some(t) = res {
            return Some(t.clone());
        }
        if flags & PARSE_NOUNKNOWN != 0 {
            return None;
        }
        Some(if first_is_string {
            PartType::new_unknown(0, Some(s))
        } else {
            PartType::new_unknown(code, None)
        })
    }

    /// `fdisk_label_parse_parttype (lb, str)`: type data only.
    #[must_use]
    pub fn parse_parttype(&self, s: &[u8]) -> Option<PartType> {
        self.advparse_parttype(s, PARSE_DATA)
    }
}
