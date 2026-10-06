//! What the shell's volume controls are the volume of: the sound card's
//! master volume and mute -- read when the controls are about to show it,
//! written when the user changes it -- or, where no card was asked for, the
//! shell's own number.
//!
//! # Why the controls lied before
//!
//! The quick settings' slider, the volume keys and the mute key changed a
//! number in the notification pane, the overlay showed it, and nothing made
//! any sound louder or quieter: the number was the setting confirming
//! itself. The card's master volume is reached as Linux's is, through its
//! ALSA control device (`sound::mixer`), and the controls now move it.
//!
//! # A card out of reach says so
//!
//! On SlateOS today no native program can reach the card: its `ioctl` on a
//! sound device answers `ENOTTY`
//! (`requests/e-ad-no-application-can-reach-the-sound-device.md`). Then the
//! pane shows why in the slider's place, and the volume keys say it in the
//! overlay, rather than move a level that changes nothing
//! (design-decisions §1485). Once a native program can reach the card,
//! nothing here changes.
//!
//! # Where no card was asked for
//!
//! A test, a harness, a picture of a theme: they build a shell and ask for
//! no card ([`Output::Own`]), and the level is the shell's own number, as
//! it was before -- a test must not turn the volume of the machine running
//! it. The `desktop` binary asks for the card ([`Output::open`]) before its
//! session starts, as it allows the shell's sounds to be heard
//! (`crate::event_sounds::allow_playback`).

use sound::mixer::{ControlSys, Master, MixerError};

/// A card's master volume and mute, as the shell uses them.
pub trait Card {
    /// The level, 0 to 100.
    ///
    /// # Errors
    ///
    /// The card's.
    fn level(&mut self) -> Result<u8, MixerError>;
    /// Whether it is muted.
    ///
    /// # Errors
    ///
    /// The card's.
    fn muted(&mut self) -> Result<bool, MixerError>;
    /// Set the level, 0 to 100.
    ///
    /// # Errors
    ///
    /// The card's.
    fn set_level(&mut self, level: u8) -> Result<(), MixerError>;
    /// Mute it, or let it sound.
    ///
    /// # Errors
    ///
    /// The card's.
    fn set_muted(&mut self, muted: bool) -> Result<(), MixerError>;
}

impl<S: ControlSys> Card for Master<S> {
    fn level(&mut self) -> Result<u8, MixerError> {
        Master::level(self)
    }
    fn muted(&mut self) -> Result<bool, MixerError> {
        Master::muted(self)
    }
    fn set_level(&mut self, level: u8) -> Result<(), MixerError> {
        Master::set_level(self, level)
    }
    fn set_muted(&mut self, muted: bool) -> Result<(), MixerError> {
        Master::set_muted(self, muted)
    }
}

/// What the shell's volume is the volume of.
pub enum Output {
    /// No card was asked for: the level is the shell's own number.
    Own,
    /// The sound card's master volume.
    Card {
        /// The card.
        card: Box<dyn Card>,
        /// The level and mute last read from it or written to it, so a
        /// write is made only when the user changed one of them.
        known: Option<(u8, bool)>,
    },
    /// A card was asked for and cannot be used: why, in a few words for
    /// the place the slider would be.
    OutOfReach(&'static str),
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Own => f.write_str("Own"),
            Self::Card { known, .. } => f.debug_struct("Card").field("known", known).finish(),
            Self::OutOfReach(why) => f.debug_tuple("OutOfReach").field(why).finish(),
        }
    }
}

impl Output {
    /// Card 0's master volume, or why it cannot be had: what the `desktop`
    /// binary attaches.
    #[must_use]
    pub fn open() -> Self {
        match sound::mixer::open_master() {
            Ok(master) => Self::card(Box::new(master)),
            Err(e) => Self::OutOfReach(why(e)),
        }
    }

    /// `card`'s master volume.
    #[must_use]
    pub fn card(card: Box<dyn Card>) -> Self {
        Self::Card { card, known: None }
    }

    /// Why the card cannot be used, while it cannot.
    #[must_use]
    pub const fn out_of_reach(&self) -> Option<&'static str> {
        match self {
            Self::OutOfReach(why) => Some(why),
            _ => None,
        }
    }

    /// The card's level and mute, read now: `Ok(None)` where no card was
    /// asked for. A card that fails is out of reach from then on, and the
    /// error says why.
    ///
    /// # Errors
    ///
    /// Why the card cannot be used.
    pub fn read(&mut self) -> Result<Option<(u8, bool)>, &'static str> {
        match self {
            Self::Own => Ok(None),
            Self::OutOfReach(why) => Err(why),
            Self::Card { card, known } => {
                match card.level().and_then(|level| Ok((level, card.muted()?))) {
                    Ok(now) => {
                        *known = Some(now);
                        Ok(Some(now))
                    }
                    Err(e) => Err(self.lose(e)),
                }
            }
        }
    }

    /// Make the card's level and mute `level` and `muted`, writing only what
    /// differs from what it was last known to hold. Nothing where no card
    /// was asked for. A card that fails is out of reach from then on.
    ///
    /// # Errors
    ///
    /// Why the card cannot be used.
    pub fn write(&mut self, level: u8, muted: bool) -> Result<(), &'static str> {
        match self {
            Self::Own => Ok(()),
            Self::OutOfReach(why) => Err(why),
            Self::Card { card, known } => {
                let result = (|| -> Result<(), MixerError> {
                    if known.is_none_or(|(was, _)| was != level) {
                        card.set_level(level)?;
                    }
                    if known.is_none_or(|(_, was)| was != muted) {
                        card.set_muted(muted)?;
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        *known = Some((level, muted));
                        Ok(())
                    }
                    Err(e) => Err(self.lose(e)),
                }
            }
        }
    }

    /// The card failed with `e`: it is out of reach from now on.
    fn lose(&mut self, e: MixerError) -> &'static str {
        let reason = why(e);
        *self = Self::OutOfReach(reason);
        reason
    }
}

/// What the pane shows in the slider's place, and the overlay says, for a
/// card that cannot be used -- a few words, about a slider's width.
///
/// A device that will not open and a device that answers no request are
/// one sentence: from inside a program they cannot be told apart from no
/// card at all -- on SlateOS today a native program meets both with a card
/// in the machine.
#[must_use]
pub const fn why(e: MixerError) -> &'static str {
    match e {
        MixerError::Open(_) | MixerError::Refused(sound::Errno::ENOTTY | sound::Errno::ENOSYS) => {
            "No sound card reachable"
        }
        MixerError::NoMaster => "No volume control",
        MixerError::Refused(_) | MixerError::Garbled => "Sound card not answering",
    }
}

#[cfg(test)]
#[path = "volume_tests.rs"]
pub(crate) mod tests;
