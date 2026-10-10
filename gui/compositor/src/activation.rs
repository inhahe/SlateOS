//! Who may take the keyboard on a program's own say-so (design-decisions
//! 1386).
//!
//! The user moves the keyboard by clicking, by a shortcut, through the shell.
//! A *program* moves it by opening a window or by restoring one, and this
//! module decides when that is allowed:
//!
//! | The keyboard is held by ... | A program's window may take it |
//! |---|---|
//! | no window | yes |
//! | a window of the same program (connection) | yes |
//! | another program's, and the program presents an activation still good | yes |
//! | another program's, and the activation it presents is not | no |
//! | another program's, no activation, a new window | yes -- unless the policy is [`NewWindows::Strict`] |
//! | another program's, no activation, a restore or an activate | no |
//!
//! Refused, a new window opens behind the one that has the keyboard, and a
//! minimised one a program tried to restore stays minimised; either asks for
//! the user's attention, as GNOME's "is ready" does.
//!
//! **An activation** is the compositor's record that the user did something
//! that asked for the program: a token a launcher drew and handed it
//! (`guiremote::activation`), or a click on its tray icon. It names one of the
//! user's actions by number -- the compositor counts every key press, typed
//! text and button press -- and is good while the window holding the keyboard
//! has seen no later one. That is Mutter's comparison of a new window's user
//! time with the focused window's: a program the user started and then left,
//! typing elsewhere while it loaded, opens without taking their keys.
//!
//! **New windows with no activation take the keyboard by default** -- GNOME's
//! "smart" default, and KDE's -- because programs started by something that
//! cannot hand a token on (a terminal: its shell's environment was fixed when
//! the terminal started) would otherwise all open behind it. The strict policy
//! is the one both desktops are moving to, and is here for when the launchers
//! hand tokens on. A *restore* has no such excuse: a program restoring its own
//! window was not just started, and with no activation it is asking to take the
//! keys at a moment of its choosing. That was allowed until this module, and
//! is what the clipboard -- readable only by the window with the keyboard --
//! made worse than a nuisance.

use std::collections::HashMap;

use guiremote::ActivationToken;

/// Tokens one connection may have outstanding at once. A launcher needs one
/// per program it starts and they are used within seconds; the bound is what
/// stops one program from drawing tokens until another's are pushed out.
const MAX_PER_ISSUER: usize = 8;

/// Tokens outstanding at once on the whole desktop, whoever drew them: the
/// bound on memory, reached only by many connections each the user has
/// touched.
const MAX_OUTSTANDING: usize = 64;

/// Whether a new window presented with no activation may take the keyboard
/// from another program's (design-decisions 1386).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewWindows {
    /// It may: GNOME's and KDE's default. A program started by something
    /// that cannot hand on a token -- a terminal -- still opens in front.
    #[default]
    Smart,
    /// It may not: only a program the user is in, or one presenting a good
    /// activation, takes the keyboard. What GNOME's "strict" and KDE's
    /// "extreme" do; right once every launcher hands tokens on.
    Strict,
}

/// What a program presented for its next window or restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presented {
    /// An activation standing for the user's action with this number.
    StandsFor(u64),
    /// A token the compositor does not hold: never drawn, used already, or
    /// pushed out. Refused rather than read as "no token": a program that
    /// claims the user started it, and cannot show it, does not get the
    /// benefit of the doubt a program that claims nothing does.
    Unknown,
}

/// Why [`Activations::issue`] draws no token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoToken {
    /// The user has done nothing in the asking program's windows, so it has
    /// no action of theirs to vouch for. Refused rather than drawn and made
    /// worthless: a program started with a worthless token is refused the
    /// keyboard, where one started with none -- what a launcher hands on when
    /// refused -- is treated as any program is.
    NoAction,
    /// The compositor has no random source, so it cannot draw a token
    /// anybody could not guess.
    NoRandomness,
}

impl std::fmt::Display for NoToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoAction => {
                "the user has done nothing in this program's windows, so it has nothing to vouch for"
            }
            Self::NoRandomness => {
                "the compositor has no random source to draw an activation token from"
            }
        })
    }
}

impl std::error::Error for NoToken {}

/// Where a token's bytes come from.
#[derive(Clone, Debug)]
pub(crate) enum TokenSource {
    /// The kernel's random source, through `randrange::fill_secret` -- the
    /// one route to it every program shares.
    System,
    /// Numbered tokens, for tests: the host the tests run on has no kernel
    /// source, and `randrange` refuses there by design.
    #[cfg(test)]
    Counted(u64),
}

/// One token drawn and not yet used.
#[derive(Clone, Copy, Debug)]
struct Issued {
    token: ActivationToken,
    /// The connection that drew it, for [`MAX_PER_ISSUER`].
    issuer: u64,
    /// The user's action it stands for.
    stands_for: u64,
}

/// The compositor's activation bookkeeping: the action count, the tokens
/// outstanding, what each program has presented.
#[derive(Clone, Debug)]
pub(crate) struct Activations {
    /// The number of the user's latest action: key presses, typed text and
    /// button presses, counted from 1. Zero until the first.
    serial: u64,
    /// Tokens drawn and not yet used, oldest first.
    issued: Vec<Issued>,
    /// What each connection presented for its next window or restore.
    /// A connection's entry goes when it is used or the connection ends.
    pending: HashMap<u64, Presented>,
    /// See [`NewWindows`].
    pub(crate) new_windows: NewWindows,
    source: TokenSource,
}

impl Activations {
    pub(crate) fn new() -> Self {
        Self {
            serial: 0,
            issued: Vec::new(),
            pending: HashMap::new(),
            new_windows: NewWindows::default(),
            source: TokenSource::System,
        }
    }

    /// Tokens numbered rather than drawn, for tests.
    #[cfg(test)]
    pub(crate) fn count_tokens(&mut self) {
        self.source = TokenSource::Counted(0);
    }

    /// The number of the user's latest action.
    pub(crate) const fn serial(&self) -> u64 {
        self.serial
    }

    /// Count one of the user's actions, and answer its number.
    pub(crate) fn note_action(&mut self) -> u64 {
        // Saturating rather than wrapping: a wrapped count would make every
        // activation drawn before the wrap look newer than every action after
        // it. Two to the sixty-four key presses is not a session.
        self.serial = self.serial.saturating_add(1);
        self.serial
    }

    /// Draw a token for `issuer`, standing for the user's action
    /// `stands_for` -- the latest in the issuer's windows.
    ///
    /// # Errors
    ///
    /// [`NoToken::NoAction`] when `stands_for` is 0, the user never having
    /// touched the issuer's windows: such a token could never be good, and
    /// drawing one would let a program nobody has used fill the table.
    /// [`NoToken::NoRandomness`] when no unguessable token can be drawn.
    pub(crate) fn issue(
        &mut self,
        issuer: u64,
        stands_for: u64,
    ) -> Result<ActivationToken, NoToken> {
        if stands_for == 0 {
            return Err(NoToken::NoAction);
        }
        let token = self.draw()?;
        if self.issued.iter().filter(|i| i.issuer == issuer).count() >= MAX_PER_ISSUER
            && let Some(oldest) = self.issued.iter().position(|i| i.issuer == issuer)
        {
            self.issued.remove(oldest);
        }
        if self.issued.len() >= MAX_OUTSTANDING {
            self.issued.remove(0);
        }
        self.issued.push(Issued {
            token,
            issuer,
            stands_for,
        });
        Ok(token)
    }

    fn draw(&mut self) -> Result<ActivationToken, NoToken> {
        self.draw_bytes().map(ActivationToken::from_bytes)
    }

    /// Sixteen unguessable bytes from the same source tokens come from: what
    /// an exported window's handle is made of too (design-decisions 1387).
    ///
    /// # Errors
    ///
    /// [`NoToken::NoRandomness`] when no unguessable bytes can be drawn.
    pub(crate) fn draw_bytes(&mut self) -> Result<[u8; 16], NoToken> {
        let mut bytes = [0u8; ActivationToken::LEN];
        match &mut self.source {
            TokenSource::System => {
                randrange::fill_secret(&mut bytes).map_err(|_| NoToken::NoRandomness)?;
            }
            #[cfg(test)]
            TokenSource::Counted(next) => {
                *next = next.saturating_add(1);
                let (head, _) = bytes.split_at_mut(8);
                head.copy_from_slice(&next.to_le_bytes());
            }
        }
        Ok(bytes)
    }

    /// `client` presents `token` for its next window or restore. The token is
    /// used up whether or not that turns out to be good.
    pub(crate) fn present(&mut self, client: u64, token: ActivationToken) {
        let presented = match self.issued.iter().position(|i| i.token == token) {
            Some(at) => Presented::StandsFor(self.issued.remove(at).stands_for),
            None => Presented::Unknown,
        };
        self.pending.insert(client, presented);
    }

    /// Give `client` an activation standing for the user's action
    /// `stands_for`, as a token would: the shell clicked its tray icon.
    pub(crate) fn grant(&mut self, client: u64, stands_for: u64) {
        if stands_for != 0 {
            self.pending
                .insert(client, Presented::StandsFor(stands_for));
        }
    }

    /// What `client` presented, taken: one window or restore uses it.
    pub(crate) fn take(&mut self, client: u64) -> Option<Presented> {
        self.pending.remove(&client)
    }

    /// Forget a connection that has ended. Its pending activation goes; the
    /// tokens it drew stay, because the programs it started may not have
    /// presented them yet -- a launcher often exits as soon as it has started
    /// something.
    pub(crate) fn forget(&mut self, client: u64) {
        self.pending.remove(&client);
    }

    /// Whether `client`'s window may take the keyboard from `holder` --
    /// the window that has it, as its connection and its user time -- given
    /// what the client presented. `opening` is a new window; otherwise a
    /// restore or an activate. See the module table.
    pub(crate) fn allows(
        &self,
        holder: Option<(u64, u64)>,
        client: u64,
        presented: Option<Presented>,
        opening: bool,
    ) -> bool {
        let Some((holder_client, holder_time)) = holder else {
            return true;
        };
        if holder_client == client {
            return true;
        }
        match presented {
            Some(Presented::StandsFor(action)) => action != 0 && action >= holder_time,
            Some(Presented::Unknown) => false,
            None => opening && self.new_windows == NewWindows::Smart,
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that unwraps should fail loudly at the line that did it. The
    // defensive lints guard code that runs on a user's data, not this.
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn counted() -> Activations {
        let mut a = Activations::new();
        a.count_tokens();
        a
    }

    /// The table in the module docs, row by row.
    #[test]
    fn who_may_take_the_keyboard() {
        let a = counted();
        let good = Some(Presented::StandsFor(9));
        let stale = Some(Presented::StandsFor(3));
        let holder = Some((1, 5));
        assert!(a.allows(None, 2, None, false), "nobody has it");
        assert!(
            a.allows(holder, 1, None, false),
            "the program the user is in"
        );
        assert!(a.allows(holder, 2, good, false), "a good activation");
        assert!(
            a.allows(Some((1, 9)), 2, good, false),
            "the action the token stands for is the holder's latest"
        );
        assert!(!a.allows(holder, 2, stale, true), "a stale activation");
        assert!(
            !a.allows(holder, 2, Some(Presented::Unknown), true),
            "a token nobody drew"
        );
        assert!(
            !a.allows(holder, 2, Some(Presented::StandsFor(0)), true),
            "an activation for no action"
        );
        assert!(a.allows(holder, 2, None, true), "a new window, smart");
        assert!(!a.allows(holder, 2, None, false), "a restore with nothing");

        let mut strict = counted();
        strict.new_windows = NewWindows::Strict;
        assert!(
            !strict.allows(holder, 2, None, true),
            "a new window, strict"
        );
        assert!(strict.allows(holder, 2, good, true), "strict, with a token");
        assert!(
            strict.allows(holder, 1, None, true),
            "strict, the same program"
        );
    }

    #[test]
    fn a_token_is_good_once() {
        let mut a = counted();
        let token = a.issue(7, 4).unwrap();
        a.present(2, token);
        assert_eq!(a.take(2), Some(Presented::StandsFor(4)));
        a.present(3, token);
        assert_eq!(a.take(3), Some(Presented::Unknown), "used up");
        assert_eq!(a.take(3), None, "taken");
    }

    /// A program the user has not touched draws no token -- refused, so that
    /// a launcher hands on none rather than one that could never be good.
    #[test]
    fn a_program_the_user_has_not_touched_draws_no_token() {
        let mut a = counted();
        assert_eq!(a.issue(7, 0), Err(NoToken::NoAction));
        assert!(a.issued.is_empty());
    }

    /// A token the compositor never drew counts against the program that
    /// presents it, as a used one does.
    #[test]
    fn a_token_never_drawn_is_unknown() {
        let mut a = counted();
        a.present(2, ActivationToken::from_bytes([0x55; 16]));
        assert_eq!(a.take(2), Some(Presented::Unknown));
    }

    /// One program drawing tokens in a loop pushes out only its own.
    #[test]
    fn one_issuer_cannot_push_out_anothers_tokens() {
        let mut a = counted();
        let theirs = a.issue(1, 5).unwrap();
        for _ in 0..100 {
            a.issue(2, 6).unwrap();
        }
        assert_eq!(
            a.issued.iter().filter(|i| i.issuer == 2).count(),
            MAX_PER_ISSUER
        );
        a.present(3, theirs);
        assert_eq!(a.take(3), Some(Presented::StandsFor(5)));
    }

    #[test]
    fn the_whole_table_is_bounded() {
        let mut a = counted();
        for issuer in 0..200 {
            a.issue(issuer, 1).unwrap();
        }
        assert_eq!(a.issued.len(), MAX_OUTSTANDING);
    }

    #[test]
    fn tokens_are_distinct_and_outlive_their_issuer() {
        let mut a = counted();
        let one = a.issue(1, 2).unwrap();
        let two = a.issue(1, 2).unwrap();
        assert_ne!(one, two);
        a.forget(1);
        a.present(5, two);
        assert_eq!(a.take(5), Some(Presented::StandsFor(2)));
    }

    #[test]
    fn a_grant_for_no_action_grants_nothing() {
        let mut a = counted();
        a.grant(4, 0);
        assert_eq!(a.take(4), None);
        a.grant(4, 3);
        assert_eq!(a.take(4), Some(Presented::StandsFor(3)));
    }

    #[test]
    fn an_ended_connection_presents_nothing() {
        let mut a = counted();
        let token = a.issue(1, 2).unwrap();
        a.present(9, token);
        a.forget(9);
        assert_eq!(a.take(9), None);
    }

    /// On the host the tests run on, the system source refuses -- which is
    /// what makes "no randomness, no token" testable at all.
    #[cfg(not(unix))]
    #[test]
    fn without_a_random_source_there_is_no_token() {
        let mut a = Activations::new();
        assert_eq!(a.issue(1, 2), Err(NoToken::NoRandomness));
        assert!(a.issued.is_empty());
    }
}
