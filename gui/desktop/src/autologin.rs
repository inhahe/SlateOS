//! Whether this start of the machine signs somebody in without asking.
//!
//! `design-decisions.md` §1427, the operator's answer to C-Q22. An account set
//! to sign in by itself does so as the desktop starts, and the login screen is
//! never drawn -- "I definitely don't want an unnecessary pause during bootup".
//! Holding a key while the machine starts shows the login screen instead, so a
//! different account can be chosen, and a machine started for repair skips the
//! automatic sign-in altogether.
//!
//! # What decides it
//!
//! Three facts, each read once, as the desktop starts:
//!
//! | Fact | Where it comes from | What it does |
//! |---|---|---|
//! | Which account is marked | [`LoginUser::autologin`], set from [`loginusers::automatic_account`]: exactly one account marked `auto_login`, and not locked | that account signs in |
//! | The chooser key is held | the compositor, which alone reads the keyboard ([`StartConditions::new`]) | the login screen is shown |
//! | The machine started for repair | a word on the kernel command line ([`REPAIR_WORDS`]) | the login screen is shown |
//!
//! The session carries the answer out before its first frame
//! (`ShellSession::start`), so an automatic sign-in shows no login screen at
//! all -- not even for one frame, which would read as exactly the pause the
//! operator ruled out.
//!
//! # Only at start
//!
//! Logging out returns to the login screen and stays there. A log-out that
//! signed the same account straight back in would be a desktop nobody could
//! leave, and choosing another account is exactly what logging out is for --
//! which is also the operator's reason the key is a convenience rather than a
//! necessity: "The user can always just use the menu to logout and
//! login/switch accounts after it autologs in".
//!
//! # What does not work yet
//!
//! **The chooser key cannot be read.** A key held since the machine was
//! switched on went down before the compositor opened the keyboard, and the
//! compositor starts out believing nothing is held; nor can a client ask it
//! what is. Both are lane F's
//! (`requests/c-f-which-keys-are-held-when-the-desktop-starts.md`). Until then
//! [`StartConditions::of_this_start`] reports no key held, so an account marked
//! to sign in by itself always does, and the way to another account is the one
//! the operator named: log out.
//!
//! **Nothing marks a start for repair yet.** `/proc/cmdline` is a line the
//! kernel makes up today, not the one the machine was started with, and no
//! boot entry starts for repair. Both are lane A's
//! (`requests/c-a-the-kernels-app-registry-and-the-first-screen-hint.md`).
//! The reading here is written against the real line, so it takes effect the
//! day the kernel publishes it.

use guitk::event::Modifiers;

use crate::login_screen::LoginUser;

/// Where the kernel publishes the command line the machine was started with.
pub const KERNEL_COMMAND_LINE: &str = "/proc/cmdline";

/// The words on the kernel command line that mean "started for repair".
///
/// `recovery` is the word the recovery entry of the kernel's own boot
/// configuration writes (`kernel/src/fs/bootcfg.rs`), and the one lane A is
/// asked to put on SlateOS's; `single` is the older word for the same kind of
/// start -- single-user mode -- which that entry writes beside it. Either one
/// alone is enough. Matched as whole words, exactly: `norecovery` or
/// `recovery=0` is some other parameter, and reading it as this one would turn
/// a start that asked for nothing into a start that refuses to sign in.
pub const REPAIR_WORDS: [&[u8]; 2] = [b"recovery", b"single"];

/// What this start of the machine says about signing in by itself.
///
/// [`Default`] is an ordinary start: no key held, not for repair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartConditions {
    /// The key that asks for the account chooser was held as the desktop
    /// started: Shift, either one ([`StartConditions::new`]).
    pub chooser_held: bool,
    /// The machine was started for repair ([`REPAIR_WORDS`]).
    pub repair: bool,
}

impl StartConditions {
    /// From the modifiers held as the desktop started and the kernel command
    /// line it was started with.
    ///
    /// **The chooser key is Shift**, either one -- the key GRUB reads during
    /// a start that hides its menu, to show the menu after all (on BIOS
    /// machines; on UEFI ones it is Esc), so a person who knows one knows the
    /// other. A modifier rather than a letter because a modifier held on its
    /// own types nothing: a letter held through the start would arrive in
    /// whatever window was focused first.
    ///
    /// Only Shift is read. Another modifier held beside it does not cancel it:
    /// someone holding keys to stop a sign-in is not to be let in because they
    /// held one too many.
    #[must_use]
    pub fn new(held: Modifiers, cmdline: &[u8]) -> Self {
        Self {
            chooser_held: held.shift,
            repair: started_for_repair(cmdline),
        }
    }

    /// This machine's, as the desktop starts.
    #[must_use]
    pub fn of_this_start() -> Self {
        // Nothing can say a key is held yet: see the module docs. Written as a
        // value handed to `new` rather than as `chooser_held: false`, so the
        // day the compositor can be asked, the answer goes in here and nothing
        // else changes.
        let held = Modifiers::NONE;
        // A line that cannot be read -- not SlateOS, or no /proc -- reads as an
        // ordinary start. Safe to discard the error because skipping the
        // automatic sign-in for repair is a convenience and not a lock: a
        // repair start that signs in by itself is no worse off than the
        // machine before this decision, and nearly every start is ordinary.
        let cmdline = std::fs::read(KERNEL_COMMAND_LINE).unwrap_or_default();
        Self::new(held, &cmdline)
    }
}

/// Whether the kernel command line `cmdline` asks for a start for repair.
///
/// Bytes, not text: a command line is whatever the boot loader was told, and a
/// parameter this function does not know about may not be UTF-8. Split on the
/// whitespace the kernel splits on; quoting (`name="a b"`) cannot produce a
/// bare `recovery` word from inside a quoted value except by the value being
/// that word, which is then a parameter of that name.
#[must_use]
pub fn started_for_repair(cmdline: &[u8]) -> bool {
    cmdline
        .split(u8::is_ascii_whitespace)
        .any(|word| REPAIR_WORDS.contains(&word))
}

/// The account this start signs in as, or `None` to show the login screen.
///
/// The account [`LoginUser::autologin`] marks, provided the chooser key was
/// not held and the machine was not started for repair. A list that marks
/// more than one -- which the database path never produces, see
/// [`loginusers::automatic_account`] -- signs nobody in, by the same rule and
/// for the same reason: which was meant is written down nowhere.
#[must_use]
pub fn account_to_sign_in(users: &[LoginUser], start: StartConditions) -> Option<&LoginUser> {
    if start.chooser_held || start.repair {
        return None;
    }
    let mut marked = users.iter().filter(|user| user.autologin);
    let user = marked.next()?;
    marked.next().is_none().then_some(user)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn alice_signs_in_by_itself() -> Vec<LoginUser> {
        vec![
            LoginUser::new(Some(1000), "bob", "Bob"),
            LoginUser::new(Some(1001), "alice", "Alice").with_autologin(),
        ]
    }

    #[test]
    fn an_ordinary_start_signs_the_marked_account_in() {
        let users = alice_signs_in_by_itself();
        let user = account_to_sign_in(&users, StartConditions::default()).unwrap();
        assert_eq!(user.username, "alice");
    }

    #[test]
    fn with_no_account_marked_nobody_is_signed_in() {
        let users = vec![LoginUser::new(Some(1000), "bob", "Bob")];
        assert!(account_to_sign_in(&users, StartConditions::default()).is_none());
    }

    /// Two marked is the administrator's contradiction, not a list to take
    /// the first of.
    #[test]
    fn two_marked_accounts_sign_nobody_in() {
        let users = vec![
            LoginUser::new(Some(1000), "bob", "Bob").with_autologin(),
            LoginUser::new(Some(1001), "alice", "Alice").with_autologin(),
        ];
        assert!(account_to_sign_in(&users, StartConditions::default()).is_none());
    }

    #[test]
    fn the_chooser_key_shows_the_login_screen_instead() {
        let users = alice_signs_in_by_itself();
        let start = StartConditions::new(Modifiers::shift(), b"");
        assert!(start.chooser_held);
        assert!(account_to_sign_in(&users, start).is_none());
    }

    /// Holding one key too many does not let the sign-in through.
    #[test]
    fn shift_held_with_another_modifier_still_asks() {
        let held = Modifiers {
            ctrl: true,
            ..Modifiers::shift()
        };
        assert!(StartConditions::new(held, b"").chooser_held);
    }

    /// Only Shift is the chooser key: a Ctrl held through start is somebody
    /// resting a hand on the keyboard.
    #[test]
    fn a_modifier_other_than_shift_is_not_the_chooser_key() {
        let held = Modifiers {
            ctrl: true,
            alt: true,
            super_key: true,
            shift: false,
        };
        assert!(!StartConditions::new(held, b"").chooser_held);
    }

    #[test]
    fn a_start_for_repair_shows_the_login_screen_instead() {
        let users = alice_signs_in_by_itself();
        let start = StartConditions::new(Modifiers::NONE, b"root=/dev/sda2 ro recovery quiet");
        assert!(start.repair);
        assert!(account_to_sign_in(&users, start).is_none());
    }

    /// The command line of the kernel's own recovery entry, word for word:
    /// either word alone would do.
    #[test]
    fn both_repair_words_are_read_and_either_is_enough() {
        assert!(started_for_repair(b"root=/dev/sda2 single recovery"));
        assert!(started_for_repair(b"single"));
        assert!(started_for_repair(b"recovery"));
        assert!(started_for_repair(b"quiet\trecovery\n"));
    }

    /// Whole words only: a parameter that merely contains one is another
    /// parameter.
    #[test]
    fn a_parameter_that_contains_a_repair_word_is_not_one() {
        for line in [
            &b"norecovery"[..],
            b"recovery=0",
            b"single-user=no",
            b"init=/sbin/singleton",
            b"RECOVERY",
            b"",
            b"root=/dev/sda2 ro quiet splash",
        ] {
            assert!(
                !started_for_repair(line),
                "{} read as a start for repair",
                line.escape_ascii()
            );
        }
    }

    /// A command line is bytes; a parameter that is not UTF-8 does not hide
    /// the word beside it.
    #[test]
    fn a_line_that_is_not_text_is_still_read() {
        assert!(started_for_repair(b"name=\xff\xfe recovery"));
    }
}
