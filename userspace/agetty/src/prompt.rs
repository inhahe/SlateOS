//! What `do_prompt` writes after the issue: on a virtual console, a hint
//! when a keyboard light is not as the console starts it; the host's name;
//! and `login: `.

use crate::issue::Host;
use crate::options::{F_LONGHNAME, F_NOHOSTNAME, Options};

/// `LOGIN_PROMPT`.
pub const LOGIN_PROMPT: &[u8] = b"login: ";

/// `_PATH_NUMLOCK_ON`: present when the console was told to start with Num
/// Lock on.
pub const NUMLOCK_ON: &str = "/run/numlock-on";

/// The `Hint: ...` paragraph for keyboard flags `kb` (`KDGKBLED`: the
/// lights in the low three bits -- Scroll 1, Num 2, Caps 4 -- and their
/// start-up states in the three above), Num Lock being meant to be on when
/// `numlock_on`. `None` when every light is as it should be.
#[must_use]
pub fn keyboard_hint(kb: u8, numlock_on: bool) -> Option<Vec<u8>> {
    let mut hint: Vec<&str> = Vec::new();
    if numlock_on && kb & 0x02 == 0 {
        hint.push("Num Lock off");
    } else if !numlock_on && kb & 0x02 != 0 && kb & 0x20 == 0 {
        hint.push("Num Lock on");
    }
    if kb & 0x04 != 0 && kb & 0x40 == 0 {
        hint.push("Caps Lock on");
    }
    if kb & 0x01 != 0 && kb & 0x10 == 0 {
        hint.push("Scroll Lock on");
    }
    (!hint.is_empty()).then(|| format!("Hint: {}\n\n", hint.join(", ")).into_bytes())
}

/// The host's name and a space, as the prompt begins: up to its first `.`,
/// or under `--long-hostname` the whole of it -- its canonical name when it
/// has no `.` of its own. `None` under `--nohostname`, or when the name
/// cannot be had.
#[must_use]
pub fn host_name(op: &Options, host: &dyn Host) -> Option<Vec<u8>> {
    if op.flags & F_NOHOSTNAME != 0 {
        return None;
    }
    let name = host.hostname()?;
    let dot = name.iter().position(|&b| b == b'.');
    let mut shown = if op.flags & F_LONGHNAME == 0 {
        name.get(..dot.unwrap_or(name.len()))
            .unwrap_or_default()
            .to_vec()
    } else if dot.is_none() {
        host.canonical_name(&name).unwrap_or(name)
    } else {
        name
    };
    shown.push(b' ');
    Some(shown)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use libcall::clock::Local;

    struct Named(&'static [u8], Option<&'static [u8]>);

    impl Host for Named {
        fn uname(&self) -> crate::issue::Uts {
            crate::issue::Uts::default()
        }
        fn hostname(&self) -> Option<Vec<u8>> {
            (!self.0.is_empty()).then(|| self.0.to_vec())
        }
        fn domainname(&self) -> Option<Vec<u8>> {
            None
        }
        fn canonical_name(&self, _host: &[u8]) -> Option<Vec<u8>> {
            self.1.map(<[u8]>::to_vec)
        }
        fn local_now(&self) -> Option<Local> {
            None
        }
        fn langinfo(&self, _item: i32) -> Vec<u8> {
            Vec::new()
        }
        fn users(&self) -> usize {
            0
        }
        fn interfaces(&self) -> Option<Vec<crate::issue::Iface>> {
            None
        }
        fn ntop(&self, _family: i32, _addr: &[u8]) -> Vec<u8> {
            Vec::new()
        }
        fn host_address(&self, _host: &[u8], _family: i32, _flags: i32) -> Option<Vec<u8>> {
            None
        }
    }

    #[test]
    fn the_host_s_name_short_or_long() {
        let op = Options::default();
        let long = Options {
            flags: F_LONGHNAME,
            ..Options::default()
        };
        let quiet = Options {
            flags: F_NOHOSTNAME | F_LONGHNAME,
            ..Options::default()
        };
        let dotted = Named(b"box.example.org", Some(b"other.example"));
        assert_eq!(host_name(&op, &dotted).unwrap(), b"box ");
        assert_eq!(host_name(&long, &dotted).unwrap(), b"box.example.org ");
        let bare = Named(b"box", Some(b"box.lan"));
        assert_eq!(host_name(&op, &bare).unwrap(), b"box ");
        assert_eq!(host_name(&long, &bare).unwrap(), b"box.lan ");
        assert_eq!(host_name(&long, &Named(b"box", None)).unwrap(), b"box ");
        assert_eq!(host_name(&quiet, &bare), None);
        assert_eq!(host_name(&op, &Named(b"", None)), None);
    }

    #[test]
    fn a_hint_says_which_lights_are_not_as_they_start() {
        assert_eq!(keyboard_hint(0, false), None);
        assert_eq!(keyboard_hint(0, true).unwrap(), b"Hint: Num Lock off\n\n");
        assert_eq!(
            keyboard_hint(0x02, false).unwrap(),
            b"Hint: Num Lock on\n\n"
        );
        // On, and meant to start on: nothing to say.
        assert_eq!(keyboard_hint(0x22, false), None);
        assert_eq!(keyboard_hint(0x02, true), None);
        assert_eq!(
            keyboard_hint(0x07, false).unwrap(),
            b"Hint: Num Lock on, Caps Lock on, Scroll Lock on\n\n"
        );
        assert_eq!(
            keyboard_hint(0x45, false).unwrap(),
            b"Hint: Scroll Lock on\n\n"
        );
        assert_eq!(
            keyboard_hint(0x14, true).unwrap(),
            b"Hint: Num Lock off, Caps Lock on\n\n"
        );
    }
}
