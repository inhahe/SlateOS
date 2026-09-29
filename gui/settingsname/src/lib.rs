//! The name of a settings file, and the one rule for what may be one.
//!
//! Each program keeps its settings in a file of its own, `<name>.yaml` in the
//! settings folder, and a service tells every open window when one changes
//! (`design-decisions.md` §1418). Three places handle that name: the file
//! (`gui/settingsfile`, which writes it), the display protocol (whose
//! `SettingsChanged` announcement carries it, as `guitk::event::SettingsName`)
//! and the watcher (`gui/settingswatch`, which reads it off the folder). A
//! name one of them accepts and another refuses is a file that is saved and
//! never announced, or announced and never read -- so the rule is here, once,
//! and all three use it.
//!
//! It was written in `guitk::event` beside the protocol's `SettingsGroup`,
//! and moved here when `settingsfile` needed it too: `settingsfile` sits
//! below the toolkit and must not depend on a widget library to name its own
//! file. `guitk::event` re-exports it, so its path there is unchanged.

#![no_std]

#[cfg(test)]
extern crate std;

/// The name of a program's settings file, without its `.yaml`: 1 to
/// [`MAX_LEN`](Self::MAX_LEN) bytes of `a`-`z`, `0`-`9`, `_` and `-`.
///
/// Narrow on purpose. A name crosses the display protocol, where anyone who
/// can open the socket may send one, so it must not be able to name anything
/// but a file in the settings folder: no `/`, no `.`, nothing that is not
/// plain on every filesystem and in every log. Every name in use when this
/// was written fits (the longest, `markdowneditor`, is 14 bytes).
///
/// Held inline, so that it is `Copy` like the rest of the protocol's
/// `SettingsGroup`; the bytes past the name are always zero, which is what
/// makes the derived equality and hash compare names.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingsName {
    len: u8,
    bytes: [u8; Self::MAX_LEN],
}

impl SettingsName {
    /// The longest name, in bytes.
    pub const MAX_LEN: usize = 32;

    /// `name` if it is a settings name, else `None`: the one validation,
    /// which the protocol's decoders, the settings file and the watcher all
    /// use.
    ///
    /// `const`, so a program can hold its own name as a constant.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "a const fn cannot call slice::get; the loop keeps i below name.len(), \
                  which is at most MAX_LEN, the array's length, so neither index can \
                  miss and i + 1 cannot overflow"
    )]
    pub const fn new(name: &[u8]) -> Option<Self> {
        if name.is_empty() || name.len() > Self::MAX_LEN {
            return None;
        }
        let mut bytes = [0u8; Self::MAX_LEN];
        let mut i = 0;
        while i < name.len() {
            let b = name[i];
            if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-') {
                return None;
            }
            bytes[i] = b;
            i += 1;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the length was checked against MAX_LEN, 32, above"
        )]
        let len = name.len() as u8;
        Some(Self { len, bytes })
    }

    /// The name's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or_default()
    }

    /// The name as text -- it is always ASCII.
    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.as_bytes()).unwrap_or_default()
    }
}

impl core::fmt::Debug for SettingsName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SettingsName({:?})", self.as_str())
    }
}

impl core::fmt::Display for SettingsName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsName;
    use std::borrow::ToOwned;
    use std::format;
    use std::string::ToString;
    use std::vec::Vec;

    /// A settings name is a file in the settings folder and nothing else: the
    /// names in use are accepted, and anything that could reach another
    /// directory, hide a suffix, or not be plain everywhere is refused.
    #[test]
    fn a_settings_name_is_plain_lowercase_and_short() {
        for good in [
            &b"calendar"[..],
            b"markdowneditor",
            b"a",
            b"x_1-y",
            &[b'z'; 32],
        ] {
            assert_eq!(
                SettingsName::new(good).map(|n| n.as_bytes().to_vec()),
                Some(good.to_vec()),
                "{good:?} refused"
            );
        }
        for bad in [
            &b""[..],
            &[b'z'; 33],
            b"Calendar",
            b"../passwd",
            b"a/b",
            b"notes.yaml",
            b"two words",
            b"caf\xc3\xa9",
            b"nul\0",
        ] {
            assert_eq!(SettingsName::new(bad), None, "{bad:?} accepted");
        }
    }

    /// Every byte, alone: the rule is exactly the four classes, nothing
    /// adjacent to them in ASCII (`` ` `` before `a`, `{` after `z`, `/` and
    /// `:` either side of the digits, `^` before `_`, `.` beside `-`).
    #[test]
    fn exactly_lowercase_digits_underscore_and_hyphen_are_allowed() {
        let allowed: Vec<u8> = (0..=u8::MAX)
            .filter(|b| SettingsName::new(&[*b]).is_some())
            .collect();
        let want: Vec<u8> = (b'a'..=b'z')
            .chain(b'0'..=b'9')
            .chain([b'_', b'-'])
            .collect::<std::collections::BTreeSet<u8>>()
            .into_iter()
            .collect();
        assert_eq!(allowed, want);
    }

    /// Equality and hashing are by name, whatever the length: the unused tail
    /// is zero in every name, so `ab` is not `abc` cut short.
    #[test]
    fn two_settings_names_are_equal_when_their_names_are() {
        const OWN: Option<SettingsName> = SettingsName::new(b"notes");
        let ab = SettingsName::new(b"ab");
        assert_eq!(ab, SettingsName::new(b"ab"));
        assert_ne!(ab, SettingsName::new(b"abc"));
        assert_ne!(ab, SettingsName::new(b"a"));
        let name = SettingsName::new(b"calendar");
        assert_eq!(
            name.map(|n| n.as_str().to_owned()).as_deref(),
            Some("calendar")
        );
        assert_eq!(name.map(|n| n.to_string()).as_deref(), Some("calendar"));
        assert_eq!(
            name.map(|n| format!("{n:?}")).as_deref(),
            Some("SettingsName(\"calendar\")")
        );
        assert_eq!(OWN.map(|n| n.to_string()).as_deref(), Some("notes"));
    }
}
