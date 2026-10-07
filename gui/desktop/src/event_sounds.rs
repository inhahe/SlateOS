//! The shell's sounds: which of its events make one, and playing it.
//!
//! # Which events
//!
//! By the freedesktop sound naming specification's names, so a sound theme
//! made for another desktop sounds right here:
//!
//! | Event | Sound |
//! |---|---|
//! | a notification arrives | `message-new-instant`; `dialog-warning` for an urgent one, nothing for a low one |
//! | the volume is changed | `audio-volume-change` -- the level heard, not only seen |
//! | a device is plugged in or taken out | `device-added`, `device-removed` |
//! | a screenshot is taken | `screen-capture` |
//! | the battery runs low | `battery-low` |
//! | the network comes or goes | `network-connectivity-established`, `-lost` |
//! | someone signs in, or the desktop starts signed in | `desktop-login` |
//! | someone signs out | `desktop-logout` |
//!
//! The list a settings page offers is `appearance::sounds::SHELL_EVENTS`,
//! which this crate's tests hold to this table.
//!
//! What plays for each is the user's choice ([`AppearanceSettings::sound_for`]:
//! sounds on or off, their own sound for an event, the sound theme, then the
//! built-in sound `gui/sound` synthesizes), at their volume. A notification
//! sounds as it arrives -- not one focus assist silenced -- and only for a
//! program whose notifications are allowed a sound (`notifsettings`'
//! `AppRule::sound`), whether or not it pops up: sound and banner are that
//! rule's separate switches.
//!
//! # Playing
//!
//! [`sound::play`] decodes and plays on a thread of its own and returns at
//! once: the shell's frame never waits on a chime. A sound that cannot play
//! -- no sound device, too many playing -- is not the event's failure, and is
//! dropped. Every event asked for is kept with what was chosen for it, the
//! latest [`RECENT`], for the tests and for whatever wants to show what the
//! shell has said aloud.
//!
//! Nothing is heard until the process says it may be ([`allow_playback`]),
//! which the `desktop` binary does first. A switch for the process, not a
//! test's `cfg`: everything else that builds a shell -- this crate's tests,
//! its integration tests, another crate's, a harness -- records what it
//! would sound and makes no noise on the machine running it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use appearance::AppearanceSettings;
use appearance::sounds::SoundChoice;

use crate::notif_pane::NotifPriority;
use crate::osd::OsdKind;

/// How many of the events asked for are kept.
pub const RECENT: usize = 32;

/// Whether this process's shell sounds are heard: not until
/// [`allow_playback`].
static PLAYBACK: AtomicBool = AtomicBool::new(false);

/// Let this process's shell sounds be heard -- what the `desktop` binary
/// does before it starts a session. See the module's "Playing".
pub fn allow_playback() {
    // Relaxed: a flag set once at start, guarding nothing but itself.
    PLAYBACK.store(true, Ordering::Relaxed);
}

/// Whether [`allow_playback`] has been called in this process.
#[must_use]
pub fn playback_allowed() -> bool {
    PLAYBACK.load(Ordering::Relaxed)
}

/// An event the shell asked to sound, and what was chosen for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The event's sound naming specification name.
    pub event: String,
    /// What was chosen to play -- [`SoundChoice::Silent`] when nothing was.
    pub choice: SoundChoice,
}

/// The sound for a notification of `priority`: none for a low one, which
/// asks to be seen when looked for rather than heard; a warning for an
/// urgent one; a message's chime for the rest.
#[must_use]
pub fn for_notification(priority: NotifPriority) -> Option<&'static str> {
    match priority {
        NotifPriority::Low => None,
        NotifPriority::Normal | NotifPriority::High => Some("message-new-instant"),
        NotifPriority::Urgent => Some("dialog-warning"),
    }
}

/// The sound for the on-screen display `kind` -- `None` for one that says
/// nothing aloud: brightness, a media track, a lock key, the microphone, a
/// display of the shell's own words.
#[must_use]
pub fn for_osd(kind: &OsdKind) -> Option<&'static str> {
    match kind {
        OsdKind::Volume { muted: false, .. } => Some("audio-volume-change"),
        OsdKind::DeviceEvent { ejected: false, .. } => Some("device-added"),
        OsdKind::DeviceEvent { ejected: true, .. } => Some("device-removed"),
        OsdKind::ScreenshotTaken { .. } => Some("screen-capture"),
        OsdKind::BatteryLow { .. } => Some("battery-low"),
        OsdKind::NetworkStatus {
            connected: true, ..
        } => Some("network-connectivity-established"),
        OsdKind::NetworkStatus {
            connected: false, ..
        } => Some("network-connectivity-lost"),
        // Muted: the change is to silence, which a sound would contradict.
        OsdKind::Volume { muted: true, .. }
        | OsdKind::Brightness { .. }
        | OsdKind::MediaTrack { .. }
        | OsdKind::MediaPlayPause { .. }
        | OsdKind::KeyboardLock { .. }
        | OsdKind::Microphone { .. }
        | OsdKind::Custom { .. } => None,
    }
}

/// The shell's sounds: what was asked for, played if the process allows it.
#[derive(Debug, Default)]
pub struct EventSounds {
    /// The events asked for, oldest first, at most [`RECENT`].
    recent: VecDeque<Asked>,
}

impl EventSounds {
    /// None asked for yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The events asked for, oldest first.
    pub fn recent(&self) -> impl Iterator<Item = &Asked> {
        self.recent.iter()
    }

    /// Sound the event `name` as `appearance` says -- heard if the process
    /// allows it ([`allow_playback`]) -- and say what was chosen.
    pub fn sound(&mut self, appearance: &AppearanceSettings, name: &str) -> SoundChoice {
        let choice = appearance.sound_for(name);
        while self.recent.len() >= RECENT && self.recent.pop_front().is_some() {}
        self.recent.push_back(Asked {
            event: name.to_owned(),
            choice: choice.clone(),
        });
        if playback_allowed() {
            let volume = appearance.sounds.volume;
            let sound = match &choice {
                SoundChoice::File(path) => Some(sound::Sound::File(path.clone())),
                SoundChoice::BuiltIn(event) => {
                    sound::BuiltIn::for_event(event).map(sound::Sound::BuiltIn)
                }
                SoundChoice::Silent => None,
            };
            if let Some(sound) = sound {
                // Busy, no device, no thread: each means this sound is not
                // heard, which is no failure of the event that asked for it
                // -- see the module's "Playing".
                let _unheard = sound::play(sound, volume);
            }
        }
        choice
    }
}

#[cfg(test)]
#[path = "event_sounds_tests.rs"]
mod tests;
